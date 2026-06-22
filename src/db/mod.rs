use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{migrate::MigrateDatabase, SqlitePool};
use std::str::FromStr;
use std::time::Duration;
use tracing::{error, info};

pub mod queries;

pub async fn init_db(database_url: &str) -> anyhow::Result<SqlitePool> {
    if !sqlx::Sqlite::database_exists(database_url)
        .await
        .unwrap_or(false)
    {
        info!("Creating SQLite database at {}", database_url);
        sqlx::Sqlite::create_database(database_url).await?;
    }

    // WAL lets the relay, `lantern status`, and the MCP server share this file
    // (one writer + many readers). busy_timeout makes a blocked writer wait for
    // the lock instead of failing with SQLITE_BUSY — explicit here so the
    // multi-writer contract doesn't depend on a library default.
    let options = SqliteConnectOptions::from_str(database_url)?
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
        .create_if_missing(true);

    let pool = SqlitePool::connect_with(options).await?;

    info!("Running database migrations");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| {
            error!("Migration failed: {}", e);
            e
        })?;

    info!("Database initialized successfully");
    Ok(pool)
}

#[cfg(test)]
pub mod test_helpers {
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
    use sqlx::SqlitePool;
    use std::str::FromStr;
    use tempfile::TempDir;

    /// Process-global env vars (`DEVORCH_SESSION` / `DEVORCH_RUN_ID`) are read by
    /// the MCP scope guard in production code. Tests run in parallel within one
    /// binary, so ANY test that mutates these vars — or exercises code that reads
    /// them — must hold this single crate-wide lock for its full duration.
    /// Otherwise a test that sets `DEVORCH_SESSION` can leak its value into a
    /// concurrent test that expects it unset (the source of the historic flake in
    /// `mcp::server::tests::tools_call_does_not_double_wrap_content_envelope`).
    /// `tokio::sync::Mutex` is used so the guard is `Send` across `.await` points.
    pub static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    pub async fn create_test_pool() -> (SqlitePool, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let database_url = format!("sqlite://{}", db_path.to_string_lossy());

        let options = SqliteConnectOptions::from_str(&database_url)
            .unwrap()
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .create_if_missing(true);

        let pool = SqlitePool::connect_with(options).await.unwrap();

        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        (pool, dir)
    }
}
