mod autoheal;
mod config;
mod db;
mod delivery;
mod doctor_state;
mod human;
mod mcp;
mod models_registry;
mod prompts;
mod recovery;
mod startwork;
mod stopwork;
mod supervisor;
mod temporal;
mod terminal;
mod types;

use clap::{Parser, Subcommand};
use doctor_state::DoctorStateFix;
use std::path::PathBuf;
use std::process::Command;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "lantern")]
#[command(about = "Native local orchestration runtime for terminal AI coding squads")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install Lantern and all dependencies
    Install,
    /// Start local services (Temporal, Relay)
    Up,
    /// Stop local services
    Down,
    /// Restart local services
    Restart,
    /// Check health of all services
    Doctor,
    /// Inspect local projected state and apply quarantine-safe repair passes.
    DoctorState {
        /// Apply remediation-style fixes.
        #[arg(long, value_enum)]
        fix: Option<DoctorStateFix>,
    },
    /// Show status of all squads and agents
    Status,
    /// Show the self-updating model registry (compiled-in vs. cached manifest)
    Models {
        /// Fetch models.json from GitHub and write it to the local cache
        #[arg(long)]
        sync: bool,
        /// Optional action: `sync` (same as --sync; the form the docs use)
        action: Option<String>,
    },
    /// Tail logs for a service
    Logs {
        /// Service name: relay, temporal
        service: String,
    },
    /// Run the Lantern Relay daemon (normally started by lantern up)
    Relay {
        /// Machine identifier
        #[arg(long, default_value = "local")]
        machine: String,
    },
    /// Pause an agent
    Pause { agent: String },
    /// Resume an agent
    Resume { agent: String },
    /// Human takes control of an agent pane
    Takeover { agent: String },
    /// Release human control of an agent pane
    Release { agent: String },
    /// Force recovery of an agent
    Recover { agent: String },
    /// Inject a note into an agent pane
    Note {
        agent: String,
        #[arg(trailing_var_arg = true)]
        message: Vec<String>,
    },
    /// Launch a new squad workspace (dumb launcher)
    ///
    /// Positional args match legacy startwork: `[name] [number] [agent]`
    /// e.g. `startwork kimi` or `startwork m7-navi 40 claude`
    Startwork {
        positionals: Vec<String>,
        /// Agent CLI family (overrides trailing agent arg)
        #[arg(long)]
        agent: Option<String>,
        /// Skip initialization prompts
        #[arg(long)]
        no_init: bool,
        /// Launch pattern: team | executor | simple | fixbug. Skips all
        /// interactive menus when set (errors if a required flag is missing).
        #[arg(long)]
        pattern: Option<String>,
        /// Model pick (label or id) for executor/worker/fixer, depending on pattern.
        #[arg(long)]
        model: Option<String>,
        /// Orchestrator model pick (label or id) for --pattern simple.
        #[arg(long = "orch-model")]
        orch_model: Option<String>,
        /// Worker count (1-10, default 4) for --pattern simple.
        #[arg(long)]
        workers: Option<u8>,
        /// Issue reference for --pattern fixbug.
        #[arg(long)]
        issue: Option<String>,
    },
    /// Run the Lantern MCP stdio server (spawn this as a child process from agent CLIs)
    Mcp,
    /// Run a headless Goose/ACP worker for a one-shot task (direct, testable dispatch)
    AcpRun {
        /// Agent CLI family (claude | codex; others fall back to claude-acp)
        #[arg(long)]
        agent: String,
        /// Squad role (orchestrator | ai | sec | doc | qa | ...)
        #[arg(long)]
        role: String,
        /// Working directory to run the worker in (worktree path)
        #[arg(long)]
        cwd: String,
        /// Task prompt for the headless worker
        #[arg(long)]
        task: String,
        /// Squad session id passed through to the devorch MCP extension
        #[arg(long, default_value = "adhoc")]
        session: String,
    },
    /// Stop a squad workspace and clean up its resources
    Stopwork {
        /// Session ID to stop (e.g. m7-navi-1), or omit to auto-detect
        session: Option<String>,
        /// Stop all active sessions
        #[arg(long)]
        all: bool,
        /// List active sessions
        #[arg(long)]
        list: bool,
        /// Keep worktrees in place when stopping
        #[arg(long)]
        preserve_worktrees: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // MCP subcommand must only write JSON-RPC to stdout; route all logs to stderr.
    // All other subcommands use the default (stderr) subscriber as well.
    match &cli.command {
        Commands::Mcp => {
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .init();
        }
        _ => {
            tracing_subscriber::fmt::init();
        }
    }

    match cli.command {
        Commands::Install => commands::install().await,
        Commands::Up => commands::up().await,
        Commands::Down => commands::down().await,
        Commands::Restart => commands::restart().await,
        Commands::Doctor => commands::doctor().await,
        Commands::DoctorState { fix } => commands::doctor_state(fix).await,
        Commands::Status => commands::status().await,
        Commands::Models { sync, action } => {
            let do_sync = match action.as_deref() {
                None => sync,
                Some("sync") => true,
                Some(other) => anyhow::bail!("unknown models action '{other}' (expected: sync)"),
            };
            commands::models(do_sync).await
        }
        Commands::Logs { service } => commands::logs(&service).await,
        Commands::Relay { machine } => commands::relay(&machine).await,
        Commands::Pause { agent } => commands::pause(&agent).await,
        Commands::Resume { agent } => commands::resume(&agent).await,
        Commands::Takeover { agent } => commands::takeover(&agent).await,
        Commands::Release { agent } => commands::release(&agent).await,
        Commands::Recover { agent } => commands::recover(&agent).await,
        Commands::Note { agent, message } => commands::note(&agent, &message.join(" ")).await,
        Commands::Mcp => commands::mcp().await,
        Commands::AcpRun {
            agent,
            role,
            cwd,
            task,
            session,
        } => {
            let output = delivery::acp::spawn_acp_worker(
                &agent,
                &role,
                std::path::Path::new(&cwd),
                &task,
                &session,
                None,
            )
            .await?;
            print!("{}", String::from_utf8_lossy(&output.stdout));
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            if !output.status.success() {
                anyhow::bail!("ACP worker exited with status: {}", output.status);
            }
            Ok(())
        }
        Commands::Startwork {
            positionals,
            agent,
            no_init,
            pattern,
            model,
            orch_model,
            workers,
            issue,
        } => {
            let (name, number, agent_override) =
                startwork::parse_startwork_args(positionals, agent);
            let pattern_config =
                startwork::menu::resolve_pattern(startwork::menu::PatternCliArgs {
                    pattern,
                    agent: agent_override.clone(),
                    model,
                    orch_model,
                    workers,
                    issue,
                })?;
            startwork::launch(
                name.as_deref(),
                number,
                no_init,
                pattern_config,
                agent_override.as_deref(),
            )
            .await
        }
        Commands::Stopwork {
            session,
            all,
            list,
            preserve_worktrees,
        } => stopwork::stopwork_cmd(session, all, list, preserve_worktrees).await,
    }
}

async fn session_terminal_alive(pool: &sqlx::SqlitePool, session_id: &str) -> &'static str {
    let agents = match db::queries::get_agents_for_session(pool, session_id).await {
        Ok(a) => a,
        Err(_) => return "unknown",
    };
    if agents.is_empty() {
        return "—";
    }
    if let Ok(Some(target)) = db::queries::get_terminal_target(pool, &agents[0].id).await {
        if terminal::is_iterm(&target) {
            return "iterm";
        }
    }
    "stale"
}

mod commands {
    use super::*;

    fn script_path(name: &str) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        let installed = home.join(".lantern").join("bin").join(name);
        if installed.exists() {
            return Some(installed);
        }
        let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
        let fallback = exe_dir.join(name);
        if fallback.exists() {
            Some(fallback)
        } else {
            None
        }
    }

    fn run_script(name: &str) -> anyhow::Result<()> {
        let path = script_path(name).ok_or_else(|| anyhow::anyhow!("{} script not found", name))?;
        let status = Command::new("bash").arg(&path).status()?;
        if !status.success() {
            anyhow::bail!("{} exited with status: {}", name, status);
        }
        Ok(())
    }

    pub async fn install() -> anyhow::Result<()> {
        info!("Lantern installer starting...");
        run_script("lantern-install")?;
        Ok(())
    }

    pub async fn up() -> anyhow::Result<()> {
        info!("Starting Lantern services...");
        run_script("lantern-up")?;
        models_registry::spawn_freshness_check();
        Ok(())
    }

    pub async fn down() -> anyhow::Result<()> {
        info!("Stopping Lantern services...");
        run_script("lantern-down")?;
        Ok(())
    }

    pub async fn restart() -> anyhow::Result<()> {
        down().await?;
        // `lantern-down` returns before launchd finishes tearing the Temporal
        // process down; starting a new server while the old one still holds
        // the Temporal port fails its bind. Wait (up to 10s) for the port to
        // free before bringing services back up.
        let temporal_addr: std::net::SocketAddr = config::Config::load()
            .map(|c| c.temporal_address)
            .unwrap_or_else(|_| "127.0.0.1:8243".to_string())
            .parse()
            .unwrap_or_else(|_| "127.0.0.1:8243".parse().expect("static addr parses"));
        for _ in 0..40 {
            if std::net::TcpStream::connect_timeout(
                &temporal_addr,
                std::time::Duration::from_millis(250),
            )
            .is_err()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        up().await
    }

    pub async fn doctor() -> anyhow::Result<()> {
        info!("Running Lantern doctor...");
        run_script("lantern-doctor")?;
        models_registry::spawn_freshness_check();
        Ok(())
    }

    pub async fn models(sync: bool) -> anyhow::Result<()> {
        if sync {
            models_registry::sync().await
        } else {
            models_registry::print_status().await
        }
    }

    pub async fn doctor_state(fix: Option<DoctorStateFix>) -> anyhow::Result<()> {
        let config = config::Config::load()?;
        let db_pool = db::init_db(&config.database_url).await?;
        doctor_state::run(&db_pool, &config, fix).await
    }

    pub async fn status() -> anyhow::Result<()> {
        let config = config::Config::load()?;
        let db_pool = db::init_db(&config.database_url).await?;

        println!("╔══════════════════════════════════════════════════════════════════╗");
        println!("║                    Lantern Relay Status                         ║");
        println!("╚══════════════════════════════════════════════════════════════════╝");
        println!();
        println!("Machine:    {}", config.machine_id);
        println!("Database:   {}", config.database_url);
        println!("Temporal:   {}", config.temporal_address);
        println!("Logs:       {}", config.logs_dir.display());
        println!();

        // Sessions
        let sessions = db::queries::get_all_sessions(&db_pool).await?;
        if sessions.is_empty() {
            println!("No active sessions.");
        } else {
            println!("┌─────────────────────────────────────────────────────────────────┐");
            println!("│ Sessions                                                        │");
            println!("├──────────────┬─────────────┬──────┬────────┬──────────┬─────────┤");
            println!("│ Session      │ Project     │ Slot │ Status │ Terminal │ Agents  │");
            println!("├──────────────┼─────────────┼──────┼────────┼──────────┼─────────┤");
            for sess in &sessions {
                let agent_count = db::queries::count_agents_by_session(&db_pool, &sess.id)
                    .await
                    .unwrap_or(0);
                let terminal_alive = session_terminal_alive(&db_pool, &sess.id).await;
                println!(
                    "│ {:12} │ {:11} │ {:4} │ {:6} │ {:8} │ {:7} │",
                    sess.id,
                    sess.project_slug,
                    sess.slot_number,
                    sess.status,
                    terminal_alive,
                    agent_count
                );
            }
            println!("└──────────────┴─────────────┴──────┴────────┴──────────┴─────────┘");
            println!();

            // Agents per session
            for sess in &sessions {
                let agents = db::queries::get_agents_for_session(&db_pool, &sess.id).await?;
                if !agents.is_empty() {
                    println!("  Session: {} ({} agents)", sess.id, agents.len());
                    println!(
                        "  ┌─────────────┬──────────┬────────┬────────────────────┬──────────┐"
                    );
                    println!(
                        "  │ Role        │ Status   │ Kind   │ Pane               │ Branch   │"
                    );
                    println!(
                        "  ├─────────────┼──────────┼────────────────────────────┼──────────┤"
                    );
                    for agent in &agents {
                        println!(
                            "  │ {:11} │ {:8} │ {:6} │ {:18} │ {:8} │",
                            agent.role,
                            agent.status,
                            agent.agent_kind,
                            agent.pane_id.as_deref().unwrap_or("—"),
                            agent.branch
                        );
                    }
                    println!(
                        "  └─────────────┴──────────┴────────┴────────────────────┴──────────┘"
                    );
                    println!();
                }
            }
        }

        // Work items summary
        let mut total_pending = 0i64;
        let mut total_in_progress = 0i64;
        for sess in &sessions {
            total_pending += db::queries::count_work_items_by_status(&db_pool, &sess.id, "pending")
                .await
                .unwrap_or(0);
            total_in_progress +=
                db::queries::count_work_items_by_status(&db_pool, &sess.id, "in_progress")
                    .await
                    .unwrap_or(0);
        }

        println!(
            "Work Items:  pending={}  in_progress={}",
            total_pending, total_in_progress
        );
        println!();

        // Recent events
        let events = db::queries::get_recent_events(&db_pool, 5).await?;
        if !events.is_empty() {
            println!("Recent Events:");
            for ev in &events {
                let agent = ev.agent_id.as_deref().unwrap_or("system");
                println!(
                    "  [{}] {} | {} | {}",
                    ev.created_at.format("%H:%M:%S"),
                    agent,
                    ev.event_type,
                    ev.payload.as_deref().unwrap_or("—")
                );
            }
            println!();
        }

        Ok(())
    }

    pub async fn logs(service: &str) -> anyhow::Result<()> {
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("home directory required"))?;
        let logs_dir = home.join(".lantern").join("logs");

        let log_file = match service.to_lowercase().as_str() {
            "relay" => logs_dir.join("relay.log"),
            "temporal" => logs_dir.join("temporal.log"),
            _ => {
                anyhow::bail!("Unknown service: {}. Valid: relay, temporal", service);
            }
        };

        if !log_file.exists() {
            anyhow::bail!("Log file not found: {}", log_file.display());
        }

        println!("=== {} logs ({}) ===\n", service, log_file.display());

        // Print last 50 lines
        let output = std::process::Command::new("tail")
            .arg("-n")
            .arg("50")
            .arg(&log_file)
            .output()?;

        if output.status.success() {
            print!("{}", String::from_utf8_lossy(&output.stdout));
        } else {
            anyhow::bail!("Failed to read log file");
        }

        Ok(())
    }

    pub async fn relay(machine: &str) -> anyhow::Result<()> {
        info!("Starting Lantern Relay daemon for machine: {}", machine);
        let config = config::Config::load()?;
        let db_pool = db::init_db(&config.database_url).await?;

        db::queries::insert_machine(&db_pool, &config.machine_id).await?;

        info!("Lantern Relay initialized. Machine: {}", machine);

        // Initialize human intervention module
        human::set_db_pool(db_pool.clone());

        // Initialize supervisor
        supervisor::init(db_pool.clone());
        supervisor::start_reconciliation_loop(std::time::Duration::from_secs(
            config.reconciliation_interval_secs,
        ));

        // Shared shutdown channel
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Start Temporal worker (local activity executor)
        let temporal_pool = db_pool.clone();
        let temporal_addr = config.temporal_address.clone();
        let temporal_queue = format!("lantern-{}", machine);
        let temporal_shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            if let Err(e) = temporal::run_worker(
                temporal_pool,
                &temporal_addr,
                &temporal_queue,
                temporal_shutdown,
            )
            .await
            {
                error!("Temporal worker error: {}", e);
            }
        });

        // Start delivery runner (stale lease / ack audit loop)
        let delivery_pool = db_pool.clone();
        let delivery_shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            let mut shutdown = delivery_shutdown;
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = delivery::stale::check_stale_assignments(&delivery_pool).await {
                            error!("Delivery stale check error: {}", e);
                        }
                    }
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            info!("Delivery runner shutting down");
                            break;
                        }
                    }
                }
            }
        });

        // Start the auto-heal nudge loop (30s scan; pokes silent busy agents).
        autoheal::start_nudge_loop(db_pool.clone(), shutdown_rx.clone());

        info!("All subsystems started. Waiting for shutdown signal...");
        tokio::signal::ctrl_c().await?;
        info!("Shutting down Lantern Relay...");
        let _ = shutdown_tx.send(true);
        // Give subsystems a moment to observe shutdown
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        Ok(())
    }

    pub async fn pause(agent: &str) -> anyhow::Result<()> {
        let config = config::Config::load()?;
        let db_pool = db::init_db(&config.database_url).await?;
        human::set_db_pool(db_pool);
        human::commands::pause_agent(agent).await?;
        println!("Pause requested for agent {}", agent);
        Ok(())
    }

    pub async fn resume(agent: &str) -> anyhow::Result<()> {
        let config = config::Config::load()?;
        let db_pool = db::init_db(&config.database_url).await?;
        human::set_db_pool(db_pool);
        human::commands::resume_agent(agent).await?;
        println!("Resume requested for agent {}", agent);
        Ok(())
    }

    pub async fn takeover(agent: &str) -> anyhow::Result<()> {
        let config = config::Config::load()?;
        let db_pool = db::init_db(&config.database_url).await?;
        human::set_db_pool(db_pool);
        human::commands::takeover_agent(agent).await?;
        println!("Takeover requested for agent {}", agent);
        Ok(())
    }

    pub async fn release(agent: &str) -> anyhow::Result<()> {
        let config = config::Config::load()?;
        let db_pool = db::init_db(&config.database_url).await?;
        human::set_db_pool(db_pool);
        human::commands::release_agent(agent).await?;
        println!("Release requested for agent {}", agent);
        Ok(())
    }

    pub async fn recover(agent: &str) -> anyhow::Result<()> {
        let config = config::Config::load()?;
        let db_pool = db::init_db(&config.database_url).await?;
        human::set_db_pool(db_pool);
        human::commands::recover_agent(agent).await?;
        println!("Recovery requested for agent {}", agent);
        Ok(())
    }

    pub async fn note(agent: &str, message: &str) -> anyhow::Result<()> {
        let config = config::Config::load()?;
        let db_pool = db::init_db(&config.database_url).await?;
        human::set_db_pool(db_pool);
        human::commands::note_agent(agent, message).await?;
        println!("Note requested for agent {}: {}", agent, message);
        Ok(())
    }

    pub async fn mcp() -> anyhow::Result<()> {
        let config = config::Config::load()?;
        let db_pool = db::init_db(&config.database_url).await?;
        mcp::run_mcp_server(db_pool).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn stopwork_parses_session_and_cleanup_flags() {
        let cli = Cli::try_parse_from(["lantern", "stopwork", "--preserve-worktrees", "sess-1"])
            .expect("parse stopwork command");

        match cli.command {
            Commands::Stopwork {
                session: Some(session),
                all: false,
                list: false,
                preserve_worktrees: true,
            } => assert_eq!(session, "sess-1"),
            _ => panic!("expected stopwork session subcommand"),
        }
    }

    #[test]
    fn stopwork_preserve_worktrees_flag_is_recognized_without_session() {
        let cli = Cli::try_parse_from(["lantern", "stopwork", "--preserve-worktrees", "--all"])
            .expect("parse stopwork command");

        match cli.command {
            Commands::Stopwork {
                session: None,
                all: true,
                list: false,
                preserve_worktrees: true,
            } => {}
            _ => panic!("expected stopwork all command"),
        }
    }

    #[test]
    fn stopwork_does_not_accept_tmux_flags() {
        let err = match Cli::try_parse_from(["lantern", "stopwork", "--tmux-session", "alpha"]) {
            Ok(_) => panic!("unexpectedly accepted tmux stopwork flag"),
            Err(e) => e,
        };
        let msg = err.to_string();
        assert!(msg.contains("unexpected argument") || msg.contains("unrecognized"));
    }

    #[test]
    fn doctor_state_supports_optional_fix_quarantine_flag() {
        let cli = Cli::try_parse_from(["lantern", "doctor-state", "--fix", "quarantine"])
            .expect("parse doctor-state command");

        match cli.command {
            Commands::DoctorState {
                fix: Some(DoctorStateFix::Quarantine),
            } => {}
            _ => panic!("expected doctor-state fix command"),
        }
    }

    #[test]
    fn startwork_accepts_options_after_positionals() {
        let cli = Cli::try_parse_from([
            "lantern",
            "startwork",
            "lantern-smoke",
            "97",
            "--agent",
            "goose",
            "--no-init",
        ])
        .expect("parse startwork command");

        match cli.command {
            Commands::Startwork {
                positionals,
                agent: Some(agent),
                no_init: true,
                ..
            } => {
                assert_eq!(positionals, ["lantern-smoke", "97"]);
                assert_eq!(agent, "goose");
            }
            _ => panic!("expected startwork command with parsed options"),
        }
    }

    #[test]
    fn startwork_accepts_options_before_positionals() {
        let cli = Cli::try_parse_from([
            "lantern",
            "startwork",
            "--agent",
            "goose",
            "--no-init",
            "lantern-smoke",
            "97",
        ])
        .expect("parse startwork command");

        match cli.command {
            Commands::Startwork {
                positionals,
                agent: Some(agent),
                no_init: true,
                ..
            } => {
                assert_eq!(positionals, ["lantern-smoke", "97"]);
                assert_eq!(agent, "goose");
            }
            _ => panic!("expected startwork command with parsed options"),
        }
    }
}
