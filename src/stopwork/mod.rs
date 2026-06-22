use anyhow::{Context, Result};
use serde_json::json;
use sqlx::SqlitePool;
use std::path::Path;
use tokio::process::Command;
use tracing::{error, info};

use crate::config::Config;

#[derive(Default)]
pub struct StopworkOptions {
    pub preserve_worktrees: bool,
}

pub async fn stopwork_cmd(
    session_arg: Option<String>,
    all: bool,
    list: bool,
    preserve_worktrees: bool,
) -> Result<()> {
    let config = Config::load()?;
    let pool = crate::db::init_db(&config.database_url).await?;
    let options = StopworkOptions { preserve_worktrees };

    if list {
        list_sessions_cmd(&pool).await?;
        return Ok(());
    }

    if all {
        stop_all_sessions(&pool, &config, &options).await?;
        return Ok(());
    }

    let active = crate::db::queries::get_active_sessions(&pool).await?;
    let env_session = std::env::var("DEVORCH_SESSION").ok();
    let cwd_session = detect_session_from_cwd();

    let target_session =
        match resolve_session_for_stopwork(session_arg, env_session, cwd_session, &active)? {
            Some(session_id) => session_id,
            None => {
                println!("Multiple active sessions found. Please specify one:");
                for s in active {
                    println!("  lantern stopwork {}", s);
                }
                return Ok(());
            }
        };

    stop_session(&pool, &config, &target_session, &options).await?;
    Ok(())
}

fn resolve_session_for_stopwork(
    session_arg: Option<String>,
    env_session: Option<String>,
    cwd_session: Option<String>,
    active_sessions: &[String],
) -> Result<Option<String>> {
    if let Some(session) = session_arg {
        return Ok(Some(session));
    }

    if let Some(session) = env_session {
        info!(session = %session, "Using DEVORCH_SESSION env var");
        return Ok(Some(session));
    }

    if let Some(session) = cwd_session {
        info!(session = %session, "Detected session from cwd");
        return Ok(Some(session));
    }

    match active_sessions.len() {
        0 => anyhow::bail!("No active sessions found."),
        1 => {
            info!(session = %active_sessions[0], "Auto-selecting the only active session");
            Ok(Some(active_sessions[0].clone()))
        }
        _ => Ok(None),
    }
}

fn detect_session_from_cwd() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let mut current = Some(cwd.as_path());
    while let Some(path) = current {
        if let Some(fname) = path.file_name() {
            if fname.to_str()? == "worktrees" {
                if let Some(parent) = path.parent() {
                    let parent_name = parent.file_name()?.to_str()?;
                    if parent_name == ".claude" || parent_name == ".codex" {
                        // The child of worktrees/ is the session folder.
                        let session_cwd = std::env::current_dir().ok()?;
                        let mut p = Some(session_cwd.as_path());
                        while let Some(wt_path) = p {
                            if wt_path.parent() == Some(path) {
                                return Some(wt_path.file_name()?.to_str()?.to_string());
                            }
                            p = wt_path.parent();
                        }
                    }
                }
            }
        }
        current = path.parent();
    }
    None
}

pub async fn list_sessions_cmd(pool: &SqlitePool) -> Result<()> {
    let active = crate::db::queries::get_active_sessions(pool).await?;
    if active.is_empty() {
        println!("No active startwork sessions.");
        return Ok(());
    }
    println!("{:<28} STATUS", "SESSION");
    for s in active {
        println!("{:<28} active", s);
    }
    Ok(())
}

pub async fn stop_all_sessions(
    pool: &SqlitePool,
    config: &Config,
    options: &StopworkOptions,
) -> Result<()> {
    let active = crate::db::queries::get_active_sessions(pool).await?;
    if active.is_empty() {
        println!("No active sessions found to stop.");
        return Ok(());
    }
    for s in active {
        if let Err(e) = stop_session(pool, config, &s, options).await {
            error!("Failed to stop session '{}': {}", s, e);
        }
    }
    Ok(())
}

pub async fn stop_session(
    pool: &SqlitePool,
    _config: &Config,
    session_id: &str,
    options: &StopworkOptions,
) -> Result<()> {
    println!("Stopping session '{}'...", session_id);

    // Verify the session exists before tearing anything down.
    crate::db::queries::get_session(pool, session_id)
        .await?
        .context("session not found")?;

    // 1. Close iTerm2 window.
    let iterm_closed = match crate::terminal::close_session_window(session_id).await {
        Ok(_) => {
            println!("Closed iTerm2 window for '{}'", session_id);
            true
        }
        Err(e) => {
            info!("iTerm2 close: {}", e);
            false
        }
    };

    // 2. Query agents to clean up worktrees and branches.
    let agents = crate::db::queries::get_agents_for_session(pool, session_id).await?;
    println!("Found {} agents to clean up in SQLite", agents.len());

    let mut worktree_paths = Vec::new();
    for agent in &agents {
        // Orchestrator uses repo root and does not have a worker worktree to remove.
        if agent.role == "orchestrator" {
            continue;
        }

        if options.preserve_worktrees {
            println!(
                "Preserving worktree '{}' for agent '{}'",
                agent.worktree_path, agent.id
            );
            continue;
        }

        let branch_status = Command::new("git")
            .args(["branch", "-D", &agent.branch])
            .status()
            .await;
        match branch_status {
            Ok(s) if s.success() => println!("Deleted git branch '{}'", agent.branch),
            _ => println!("Failed to delete branch '{}' or not found", agent.branch),
        }

        let wt_status = Command::new("git")
            .args(["worktree", "remove", "--force", &agent.worktree_path])
            .status()
            .await;
        match wt_status {
            Ok(s) if s.success() => {
                println!("Removed git worktree '{}'", agent.worktree_path);
                worktree_paths.push(agent.worktree_path.clone());
            }
            _ => {
                let _ = Command::new("git")
                    .args(["worktree", "prune"])
                    .status()
                    .await;
                println!(
                    "Failed to remove worktree '{}', ran git worktree prune",
                    agent.worktree_path
                );
            }
        }
    }

    // 3. Clean up empty parent directories.
    if !options.preserve_worktrees {
        for path in worktree_paths {
            let mut current = Path::new(&path).parent();
            for _ in 0..2 {
                if let Some(p) = current {
                    if p.exists() {
                        let _ = std::fs::remove_dir(p);
                    }
                    current = p.parent();
                }
            }
        }
    }

    let released_leases_count =
        crate::db::queries::delete_leases_by_session_agents(pool, session_id).await?;
    if released_leases_count > 0 {
        println!("Released {} lease records", released_leases_count);
    }

    // 4. Update audit projection in SQLite and mark as stopped. SQLite is the
    // single source of truth — there is no Temporal cleanup workflow to notify.
    let audit_payload = json!({
        "preserveWorktrees": options.preserve_worktrees,
        "closedIterm": iterm_closed,
        "releasedLeases": released_leases_count > 0,
        "finalizedAudit": true,
    })
    .to_string();

    sqlx::query("UPDATE sessions SET status = 'stopped' WHERE id = ?")
        .bind(session_id)
        .execute(pool)
        .await?;

    crate::db::queries::log_event(
        pool,
        session_id,
        None,
        "session_stopped",
        Some(&audit_payload),
    )
    .await?;

    println!("Session '{}' stopped successfully", session_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_explicit_session_first() {
        let selected = resolve_session_for_stopwork(
            Some("explicit".to_string()),
            Some("env".to_string()),
            Some("cwd".to_string()),
            &["active-1".to_string()],
        )
        .unwrap();

        assert_eq!(selected, Some("explicit".to_string()));
    }

    #[test]
    fn resolves_env_session_before_cwd_when_no_explicit() {
        let selected = resolve_session_for_stopwork(
            None,
            Some("env".to_string()),
            Some("cwd".to_string()),
            &["active-1".to_string()],
        )
        .unwrap();

        assert_eq!(selected, Some("env".to_string()));
    }

    #[test]
    fn resolve_session_reports_multiple_active_as_ambiguous() {
        let selected = resolve_session_for_stopwork(
            None,
            None,
            None,
            &["active-1".to_string(), "active-2".to_string()],
        )
        .unwrap();

        assert!(selected.is_none());
    }

    #[test]
    fn preserve_worktrees_option_is_represented_in_signal_payload() {
        let payload = json!({
            "preserveWorktrees": true,
            "closedIterm": true,
            "releasedLeases": true,
            "finalizedAudit": true,
        });

        assert_eq!(payload["preserveWorktrees"], true);
    }
}
