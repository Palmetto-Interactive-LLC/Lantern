//! iTerm2 native launcher for AI coding squads.
//!
//! Creates worktrees, opens a new iTerm2 window with the 4×2+1 squad layout using the
//! iTerm2 Python API, launches agent CLIs in each pane, registers everything with the
//! local Relay database, and exits.
//!
//! Layout (1 tab, 9 split panes):
//!   [ORCH (33% width, full height)] | [AI  | SEC]
//!                                   | [DAT | OPS]
//!                                   | [PLT | UI ]
//!                                   | [DOC | QA ]

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tokio::task::JoinSet;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use crate::db::queries;
use crate::types::{generate_id, Agent, Session, TerminalTarget};

pub mod menu;
pub mod patterns;

use patterns::{AgentKind, ModelChoice, PatternConfig};

/// Teams in the squad grid (4×2 + 1 qa).
pub(crate) const GRID_ORDER: &[&str] = &[
    "orch", "ai", "dat", "sec", "ops", "plt", "ui", "doc", "qa", "inp",
];

/// Codex models verified with `codex debug models --bundled` on 2026-05-23; docs: https://developers.openai.com/codex/models
pub(crate) const CODEX_ROLE_MODELS: &[(&str, &str)] = &[
    ("orchestrator", "gpt-5.5"),
    ("ai", "gpt-5.5"),
    ("sec", "gpt-5.5"),
];

pub(crate) const CODEX_DEFAULT_MODEL: &str = "gpt-5.4-mini";
const CODEX_LAUNCH_ERROR_WINDOW: Duration = Duration::from_secs(5);
const DEVORCH_DEFAULT_TASK_QUEUE: &str = "lantern-devorch";

/// Team labels for pane titles.
pub(crate) const TEAM_LABELS: &[(&str, &str)] = &[
    ("orch", "ORCH"),
    ("ai", "AI"),
    ("dat", "DAT"),
    ("sec", "SEC"),
    ("ops", "OPS"),
    ("plt", "PLT"),
    ("ui", "UI"),
    ("doc", "DOC"),
    ("qa", "QA"),
    ("inp", "INPUT"),
];

/// Agent kinds accepted as the last positional argument (legacy startwork syntax).
const KNOWN_AGENT_KINDS: &[&str] = &["claude", "codex", "gemini", "agy", "agi", "kimi", "goose"];

/// Parse `[name] [number] [agent]` positionals from the startwork command line.
///
/// `--agent` on the CLI wins over a trailing agent token.
pub fn parse_startwork_args(
    mut positionals: Vec<String>,
    agent_flag: Option<String>,
) -> (Option<String>, Option<u32>, Option<String>) {
    let mut agent = agent_flag;

    if agent.is_none() {
        if let Some(last) = positionals.last() {
            let last_lower = last.to_ascii_lowercase();
            if KNOWN_AGENT_KINDS.contains(&last_lower.as_str()) {
                let mut a = positionals.pop().unwrap();
                if a.eq_ignore_ascii_case("agi") {
                    a = "agy".to_string();
                }
                agent = Some(a);
            }
        }
    }

    let number = if let Some(last) = positionals.last() {
        if let Ok(n) = last.parse::<u32>() {
            if n > 0 {
                positionals.pop();
                Some(n)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let name = positionals.first().cloned();
    (name, number, agent)
}

/// Team colors (RGB) for pane backgrounds.
pub(crate) const TEAM_COLORS: &[(&str, [u8; 3])] = &[
    ("orch", [30, 32, 35]),
    ("ai", [62, 49, 0]),
    ("dat", [45, 27, 83]),
    ("ops", [0, 53, 58]),
    ("plt", [7, 57, 25]),
    ("ui", [78, 24, 24]),
    ("sec", [0, 17, 51]),
    ("doc", [70, 28, 0]),
    ("qa", [80, 0, 80]),
    ("inp", [45, 45, 45]),
];

/// Launch a new squad workspace for the given `PatternConfig`.
///
/// `team_agent_override`, when set, wins over `pattern_config`'s own
/// `TeamOrchestrator { agent }` for the actual agent CLI invoked — this lets
/// legacy agent kinds with no `AgentKind` equivalent (e.g. `agy`/Antigravity,
/// still accepted by `parse_startwork_args`) keep working byte-for-byte via
/// the raw `--agent`/trailing-positional string, while `pattern_config`
/// still carries a representable `AgentKind` for menus/metadata/env.
pub async fn launch(
    name: Option<&str>,
    number: Option<u32>,
    no_init: bool,
    pattern_config: PatternConfig,
    team_agent_override: Option<&str>,
) -> Result<()> {
    let pattern_slug = pattern_config.pattern_slug();
    match &pattern_config.pattern {
        patterns::LaunchPattern::TeamOrchestrator { agent } => {
            let agent_str = team_agent_override.unwrap_or_else(|| agent.as_str());
            launch_team(name, number, Some(agent_str), no_init, pattern_slug).await
        }
        patterns::LaunchPattern::Executor { executor } => {
            launch_executor(name, number, no_init, executor.clone(), pattern_slug).await
        }
        patterns::LaunchPattern::SimpleOrchestrator {
            orch, worker_model, ..
        } => {
            launch_simple(
                name,
                number,
                no_init,
                &pattern_config,
                orch,
                worker_model,
                pattern_slug,
            )
            .await
        }
        patterns::LaunchPattern::FixABug { .. } => {
            anyhow::bail!(
                "--pattern fixbug is not implemented yet in this build (Phase 1 foundation only)"
            )
        }
    }
}

/// Launch the Executor pattern: one worktree `executor` pane running the
/// user-picked model, a non-worktree `advisor` pane (always Fable 5 XHIGH)
/// sharing the executor's directory, and the usual `inp` input router.
///
/// Reuses the same worktree/skill-sync/MCP-registration/agent-registration
/// helpers `launch_team` uses; only the pane layout (3 panes, not 10) and the
/// window-def construction are Executor-specific.
async fn launch_executor(
    name: Option<&str>,
    number: Option<u32>,
    no_init: bool,
    executor_model: patterns::ModelChoice,
    pattern_slug: &str,
) -> Result<()> {
    let repo = find_git_repo()?;
    ensure_squad_services();
    let repo_name = repo
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string();
    let config = crate::config::Config::load()?;
    let db_pool = crate::db::init_db(&config.database_url).await?;

    let name = name.unwrap_or(&repo_name);
    let number = match number {
        Some(n) => n,
        None => allocate_session_number(&db_pool, &repo, name).await,
    };
    let session_id = workspace_session_id(name, number);
    let worktree_root = repo.join(".claude").join("worktrees").join(&session_id);

    if worktree_root.exists() {
        anyhow::bail!(
            "worktree root {} already exists. Pick a different number or clean up manually.",
            worktree_root.display()
        );
    }

    info!(repo = %repo.display(), session = %session_id, "Launching executor-pattern workspace");

    let _ = ensure_antigravity_project_trusted(&repo);
    let _ = ensure_gemini_project_trusted(&repo);

    // The executor is the only worktree in this pattern; advisor and inp share
    // its directory (advisor.needs_worktree = false — see patterns::executor()).
    let executor_worktree =
        create_orchestrator_worktree(&repo, &worktree_root, &session_id).await?;

    let run_id = format!(
        "{}-{}",
        session_id,
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    );
    let runtime_identity = RuntimeIdentityEnv::new(
        &repo,
        name,
        &config.temporal_namespace,
        DEVORCH_DEFAULT_TASK_QUEUE,
    );

    // Minimal input router: plain text routes to `executor`; `/advisor <text>`
    // routes to the advisor pane. Mirrors the team pattern's router script,
    // trimmed to this pattern's two addressable roles.
    let router_script_path = format!("/tmp/devorch-input-router-{}.py", session_id);
    let router_script_content = format!(
        r#"import sys, os, subprocess, select
try:
    import readline
    readline.set_history_length(100)
except ImportError:
    pass
session = '{session_id}'
sys.stdout.write(f'\x1b]0;INPUT - {{session}}\x07\x1b]1;INPUT - {{session}}\x07\x1b]2;INPUT - {{session}}\x07')
sys.stdout.flush()
print('\x1b[1;36m====================================================\x1b[0m')
print('\x1b[1;37m                  INPUT ROUTER                     \x1b[0m')
print('\x1b[1;36m====================================================\x1b[0m')
print('Type your note. Press Enter to submit (Ctrl-D for multiline).')
print('Prefix with "/advisor " to route to the advisor pane; default is executor.\n')

def process_cmd(cmd):
    cmd = cmd.strip()
    if not cmd:
        return
    if cmd.startswith('/advisor '):
        target_role = 'advisor'
        actual_cmd = cmd[len('/advisor '):].strip()
    elif cmd == '/advisor':
        target_role = 'advisor'
        actual_cmd = ''
    else:
        target_role = 'executor'
        actual_cmd = cmd

    print(f'\x1b[1;33mRouting note to {{target_role.upper()}}: "{{actual_cmd}}"\x1b[0m')

    env = os.environ.copy()
    env['DEVORCH_SESSION'] = '{session_id}'
    env['DEVORCH_RUN_ID'] = '{run_id}'
    env['DEVORCH_REPO_ID'] = '{repo_id}'
    env['DEVORCH_REPO_ROOT'] = '{repo_root}'
    env['DEVORCH_TEMPORAL_NAMESPACE'] = '{temporal_namespace}'
    env['DEVORCH_TASK_QUEUE'] = '{task_queue}'
    subprocess.run(['lantern', 'note', target_role, actual_cmd], env=env)

    if actual_cmd:
        try:
            import iterm2

            def find_session_by_role(app, session_id, role):
                parts = session_id.rsplit("-", 1)
                if len(parts) == 2:
                    project_slug, slot = parts
                    target_contains = f"{{project_slug}}-{{role}}-{{slot}}"
                else:
                    target_contains = f"{{session_id}}-{{role}}"
                for w in app.windows:
                    for t in w.tabs:
                        for s in t.sessions:
                            name = s.name or ""
                            if target_contains in name:
                                return s
                return None

            async def inject(connection):
                app = await iterm2.async_get_app(connection)
                s = find_session_by_role(app, session, target_role)
                if s:
                    await s.async_send_text(actual_cmd)
                    import asyncio
                    await asyncio.sleep(0.05)
                    await s.async_send_text("\r")

            iterm2.run_until_complete(inject)
        except Exception as e:
            import sys
            print(f"Error injecting to iTerm2 pane: {{e}}", file=sys.stderr)

def edit_loop():
    lines = []
    while True:
        try:
            prompt = '\x1b[1;32m[INPUT] ❯ \x1b[0m' if not lines else '          '
            line = input(prompt)
            lines.append(line)
            if not select.select([sys.stdin], [], [], 0.05)[0]:
                break
        except EOFError:
            break
        except KeyboardInterrupt:
            raise
    return '\n'.join(lines)

try:
    while True:
        try:
            cmd = edit_loop()
        except (KeyboardInterrupt, EOFError):
            break
        if cmd.strip():
            process_cmd(cmd)
finally:
    sys.stdout.write('\r\n')
    sys.stdout.flush()"#,
        session_id = session_id,
        run_id = run_id,
        repo_id = name,
        repo_root = runtime_identity.repo_root,
        temporal_namespace = runtime_identity.temporal_namespace,
        task_queue = runtime_identity.task_queue,
    );
    let _ = std::fs::write(&router_script_path, &router_script_content);

    let advisor_model = patterns::advisor_model();

    let mut window_defs: Vec<WindowDef> = Vec::with_capacity(3);

    // executor pane (worktree)
    {
        let role = "executor";
        let init = if no_init {
            None
        } else {
            Some(format!(
                "Fetch your initialization instructions by calling the `devorch_get_setup_instructions` MCP tool immediately. Use session={}, role={}, agent={}, repo_id={}, temporal_namespace={}.",
                session_id, role, executor_model.agent.as_str(), name, runtime_identity.temporal_namespace
            ))
        };
        let env = base_window_env(
            &runtime_identity,
            &session_id,
            &run_id,
            role,
            name,
            number,
            pattern_slug,
        );
        window_defs.push(WindowDef {
            name: session_id.clone(),
            label: "EXEC".to_string(),
            color: (30, 32, 35),
            dir: executor_worktree.to_string_lossy().to_string(),
            env,
            cmd: build_model_agent_command(
                &executor_model,
                role,
                init.as_deref(),
                Some(&session_id),
            ),
            agent_kind: executor_model.agent.as_str().to_string(),
        });
    }

    // advisor pane — SAME directory as executor, no worktree of its own.
    {
        let role = "advisor";
        let init = if no_init {
            None
        } else {
            Some(format!(
                "Fetch your initialization instructions by calling the `devorch_get_setup_instructions` MCP tool immediately. Use session={}, role={}, agent={}, repo_id={}, temporal_namespace={}.",
                session_id, role, advisor_model.agent.as_str(), name, runtime_identity.temporal_namespace
            ))
        };
        let env = base_window_env(
            &runtime_identity,
            &session_id,
            &run_id,
            role,
            name,
            number,
            pattern_slug,
        );
        window_defs.push(WindowDef {
            name: format!("{}-advisor", session_id),
            label: "ADVISOR".to_string(),
            color: (45, 27, 83),
            dir: executor_worktree.to_string_lossy().to_string(),
            env,
            cmd: build_model_agent_command(&advisor_model, role, init.as_deref(), None),
            agent_kind: advisor_model.agent.as_str().to_string(),
        });
    }

    // inp pane
    {
        let role = "input";
        let env = base_window_env(
            &runtime_identity,
            &session_id,
            &run_id,
            role,
            name,
            number,
            pattern_slug,
        );
        let input_cmd = format!("python3 /tmp/devorch-input-router-{}.py", session_id);
        window_defs.push(WindowDef {
            name: format!("{}-inp-{}", name, number),
            label: "INPUT".to_string(),
            color: (45, 45, 45),
            dir: executor_worktree.to_string_lossy().to_string(),
            env,
            cmd: input_cmd,
            agent_kind: "none".to_string(),
        });
    }

    sync_skills_parallel(std::slice::from_ref(&executor_worktree)).await;
    trust_workspaces(std::slice::from_ref(&executor_worktree)).await;

    let unique_agents: std::collections::HashSet<String> = window_defs
        .iter()
        .map(|w| w.agent_kind.to_ascii_lowercase())
        .collect();
    for agent in &unique_agents {
        if agent != "kimi" && agent != "none" {
            ensure_mcp_server_registered(agent);
        }
    }

    let titles_by_role = build_titles_by_role(&window_defs);
    let startup_by_role = build_startup_commands(&window_defs, &session_id);
    let init_by_role = build_init_by_role(&window_defs, no_init);
    let iterm_sessions =
        create_executor_iterm_layout(&session_id, &titles_by_role, &startup_by_role).await?;
    if iterm_sessions.len() != 3 {
        anyhow::bail!(
            "expected 3 iTerm2 sessions for the executor pattern, got {}",
            iterm_sessions.len()
        );
    }

    if !init_by_role.is_empty() {
        run_batch_init(&session_id, &init_by_role, &iterm_sessions, &titles_by_role).await;
    }

    for wdef in window_defs.iter() {
        let role = wdef
            .env
            .get("DEVORCH_ROLE")
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        let iterm_session_id = iterm_sessions
            .get(role)
            .map(|s| &s[..s.len().min(8)])
            .unwrap_or("?");
        println!(
            "  + {:<32} {} (iterm: {})",
            wdef.name, wdef.label, iterm_session_id
        );
    }

    queries::insert_machine(&db_pool, &config.machine_id).await?;
    let session = Session {
        id: session_id.clone(),
        machine_id: config.machine_id.clone(),
        project_slug: name.to_string(),
        slot_number: number as i64,
        status: "active".to_string(),
        created_at: chrono::Utc::now(),
        pattern: pattern_slug.to_string(),
    };
    queries::insert_session(&db_pool, &session).await?;

    register_agents_parallel(
        &db_pool,
        &session_id,
        name,
        number,
        &window_defs,
        &iterm_sessions,
    )
    .await?;

    println!(
        "\nWorkspace ready — iTerm2 window opened for session '{}'.",
        session_id
    );
    println!("Executor pane is active. Switch to iTerm2 to begin.");

    Ok(())
}

/// Build the CLI invocation for a pattern-selected `ModelChoice` (executor /
/// advisor / simple / fixbug panes). Unlike `build_agent_command` (which
/// derives the model from a fixed team-role lookup table), this launches the
/// exact model the user picked from the pattern menus.
fn build_model_agent_command(
    model: &patterns::ModelChoice,
    _role: &str,
    init: Option<&str>,
    pane_name: Option<&str>,
) -> String {
    let suffix = init
        .map(|s| format!(" {}", shell_escape(s)))
        .unwrap_or_default();
    match model.agent {
        patterns::AgentKind::Claude => {
            let name_arg = pane_name
                .map(|n| format!(" --name {}", shell_escape(n)))
                .unwrap_or_default();
            format!(
                "claude --model {} --dangerously-skip-permissions{}{}",
                model.model_id, name_arg, suffix
            )
        }
        patterns::AgentKind::Codex => {
            let remote = pane_name
                .map(|n| {
                    format!(
                        "--remote {} ",
                        shell_escape(&format!("unix://codex-devorch-sockets/{}.sock", n))
                    )
                })
                .unwrap_or_default();
            let cd_arg = if pane_name.is_some() {
                "--cd \"$workdir\" "
            } else {
                ""
            };
            let codex_cmd = format!(
                "codex {}{}--model {} -c 'model_reasoning_effort=\"{}\"' -c shell_environment_policy.inherit=all --dangerously-bypass-approvals-and-sandbox{}",
                remote, cd_arg, model.model_id, model.effort, suffix
            );
            if let Some(name) = pane_name {
                codex_app_server_wrapper(name, &codex_cmd)
            } else {
                codex_cmd
            }
        }
        // Gemini launch flags are not yet verified against the live CLI in this
        // pattern; falls back to a best-effort invocation. TODO(patterns): wire
        // up the real `gemini` CLI flags once verified (tracked for whichever
        // agent owns the Gemini menu path).
        patterns::AgentKind::Gemini | patterns::AgentKind::Kimi | patterns::AgentKind::Goose => {
            format!(
                "claude --model {} --dangerously-skip-permissions{}",
                model.model_id, suffix
            )
        }
    }
}

/// Open the 3-pane Executor layout (executor/advisor/inp) via
/// `iterm_executor.py`. Mirrors `create_iterm_layout`'s file-handoff protocol.
async fn create_executor_iterm_layout(
    session_id: &str,
    titles_by_role: &std::collections::HashMap<String, String>,
    startup_by_role: &std::collections::HashMap<String, String>,
) -> Result<std::collections::HashMap<String, String>> {
    let script_path = crate::terminal::locate_script("iterm_executor.py")?;

    let startup_file = format!("/tmp/devorch-iterm-startup-{}.json", session_id);
    let titles_file = format!("/tmp/devorch-iterm-titles-{}.json", session_id);
    std::fs::write(
        &startup_file,
        serde_json::to_string(startup_by_role).context("serialize startup commands")?,
    )?;
    std::fs::write(
        &titles_file,
        serde_json::to_string(titles_by_role).context("serialize pane titles")?,
    )?;

    let cmd_args = [
        script_path.to_str().context("non-UTF-8 script path")?,
        "--session",
        session_id,
        "--titles-file",
        &titles_file,
        "--startup-file",
        &startup_file,
    ];

    let output = Command::new("python3")
        .args(cmd_args)
        .output()
        .await
        .context("failed to launch iterm_executor.py")?;

    let _ = std::fs::remove_file(&startup_file);
    let _ = std::fs::remove_file(&titles_file);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("iterm_executor.py failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let map: std::collections::HashMap<String, String> = serde_json::from_str(stdout.trim())
        .with_context(|| format!("iterm_executor.py returned invalid JSON: {}", stdout.trim()))?;
    Ok(map)
}

/// Launch a new squad workspace using the legacy 4x2+1 team grid.
///
/// This is byte-identical to the pre-pattern `startwork` behavior; `launch()`
/// above dispatches into it for `LaunchPattern::TeamOrchestrator`.
async fn launch_team(
    name: Option<&str>,
    number: Option<u32>,
    agent_kind: Option<&str>,
    no_init: bool,
    pattern_slug: &str,
) -> Result<()> {
    let repo = find_git_repo()?;
    ensure_squad_services();
    let repo_name = repo
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string();
    let config = crate::config::Config::load()?;
    let db_pool = crate::db::init_db(&config.database_url).await?;

    let name = name.unwrap_or(&repo_name);
    let number = match number {
        Some(n) => n,
        None => allocate_session_number(&db_pool, &repo, name).await,
    };
    let session_id = workspace_session_id(name, number);
    let worktree_root = repo.join(".claude").join("worktrees").join(&session_id);

    // Ensure uniqueness
    if worktree_root.exists() {
        anyhow::bail!(
            "worktree root {} already exists. Pick a different number or clean up manually.",
            worktree_root.display()
        );
    }

    // SOLO GOOSE MODEL (opt-in): ONE headed, full-featured native `goose session`
    // for focused fixes — no orchestrator role, no specialist fleet, no devorch.
    // Only `startwork --agent goose` uses this. Bare `startwork` defaults to the
    // legacy all-panes grid (claude per role).
    if agent_kind
        .map(|a| a.eq_ignore_ascii_case("goose"))
        .unwrap_or(false)
    {
        return launch_solo_goose(
            &repo,
            name,
            number,
            &session_id,
            &worktree_root,
            &config,
            &db_pool,
            pattern_slug,
        )
        .await;
    }

    info!(repo = %repo.display(), session = %session_id, "Launching legacy all-panes squad workspace");

    // Register root repo as trusted in Antigravity and Gemini
    let _ = ensure_antigravity_project_trusted(&repo);
    let _ = ensure_gemini_project_trusted(&repo);

    // 1. Create worktrees and branches — one async task per worker (parallel git).
    let orchestrator_worktree =
        create_orchestrator_worktree(&repo, &worktree_root, &session_id).await?;
    let worktree_records =
        create_worker_worktrees_parallel(&repo, &worktree_root, name, number).await?;

    let run_id = format!(
        "{}-{}",
        session_id,
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    );

    let runtime_identity = RuntimeIdentityEnv::new(
        &repo,
        name,
        &config.temporal_namespace,
        DEVORCH_DEFAULT_TASK_QUEUE,
    );

    // Write the Python input router script for this session
    let router_script_path = format!("/tmp/devorch-input-router-{}.py", session_id);
    let router_script_content = format!(
        r#"import sys, os, subprocess, select
try:
    import readline
    readline.set_history_length(100)
except ImportError:
    pass
session = '{session_id}'
sys.stdout.write(f'\x1b]0;INPUT - {{session}}\x07\x1b]1;INPUT - {{session}}\x07\x1b]2;INPUT - {{session}}\x07')
sys.stdout.flush()
print('\x1b[1;36m====================================================\x1b[0m')
print('\x1b[1;37m             ORCHESTRATOR INPUT ROUTER             \x1b[0m')
print('\x1b[1;36m====================================================\x1b[0m')
print('Type your note or command. Press Enter to submit.')
print('  - Arrow keys and backspace work for editing.')
print('  - For multiline, type or paste lines then press Ctrl-D to submit.')
print('  - Ctrl-C aborts current input.')
print('Type "/<role> <command>" to route to a specific worker window.')
print('  (Available roles: ai, dat, sec, ops, plt, ui, doc, qa)\n')

worker_roles = ['ai', 'dat', 'sec', 'ops', 'plt', 'ui', 'doc', 'qa']

def process_cmd(cmd):
    cmd = cmd.strip()
    if not cmd:
        return
    matched_worker = None
    for role in worker_roles:
        if cmd.startswith(f'/{{role}} '):
            matched_worker = role
            actual_cmd = cmd[len(role) + 2:].strip()
            break
        elif cmd == f'/{{role}}':
            matched_worker = role
            actual_cmd = ""
            break
    
    if matched_worker:
        target_role = matched_worker
        role_label = matched_worker.upper()
    else:
        target_role = 'orchestrator'
        actual_cmd = cmd
        role_label = 'ORCHESTRATOR'
    
    print(f'\x1b[1;33mRouting note to {{role_label}}: "{{actual_cmd}}"\x1b[0m')
    
    env = os.environ.copy()
    env['DEVORCH_SESSION'] = '{session_id}'
    env['DEVORCH_RUN_ID'] = '{run_id}'
    env['DEVORCH_REPO_ID'] = '{repo_id}'
    env['DEVORCH_REPO_ROOT'] = '{repo_root}'
    env['DEVORCH_TEMPORAL_NAMESPACE'] = '{temporal_namespace}'
    env['DEVORCH_TASK_QUEUE'] = '{task_queue}'
    
    subprocess.run(['lantern', 'note', target_role, actual_cmd], env=env)
    
    # Inject the note directly into the active iTerm2 terminal pane
    if actual_cmd:
        try:
            import iterm2

            def find_session_by_role(app, session_id, role):
                if role == "orchestrator":
                    target_contains = "ORCH - " + session_id
                else:
                    parts = session_id.rsplit("-", 1)
                    if len(parts) == 2:
                        project_slug, slot = parts
                        target_contains = f"{{project_slug}}-{{role}}-{{slot}}"
                    else:
                        target_contains = f"{{session_id}}-{{role}}"
                for w in app.windows:
                    for t in w.tabs:
                        for s in t.sessions:
                            name = s.name or ""
                            if target_contains in name:
                                return s
                return None

            async def inject(connection):
                app = await iterm2.async_get_app(connection)
                s = find_session_by_role(app, session, target_role)
                if s:
                    await s.async_send_text(actual_cmd)
                    import asyncio
                    await asyncio.sleep(0.05)
                    await s.async_send_text("\r")

            iterm2.run_until_complete(inject)
        except Exception as e:
            import sys
            print(f"Error injecting to iTerm2 pane: {{e}}", file=sys.stderr)

def edit_loop():
    lines = []
    while True:
        try:
            prompt = '\x1b[1;32m[INPUT] ❯ \x1b[0m' if not lines else '          '
            line = input(prompt)
            lines.append(line)
            # If no more input arrives within 50 ms (single line typed), submit now.
            # Multiline paste fills the buffer immediately so select returns True.
            if not select.select([sys.stdin], [], [], 0.05)[0]:
                break
        except EOFError:
            # Ctrl-D: submit whatever has been collected
            break
        except KeyboardInterrupt:
            raise
    return '\n'.join(lines)

try:
    while True:
        try:
            cmd = edit_loop()
        except (KeyboardInterrupt, EOFError):
            break
        if cmd.strip():
            process_cmd(cmd)
finally:
    sys.stdout.write('\r\n')
    sys.stdout.flush()"#,
        session_id = session_id,
        run_id = run_id,
        repo_id = name,
        repo_root = runtime_identity.repo_root,
        temporal_namespace = runtime_identity.temporal_namespace,
        task_queue = runtime_identity.task_queue,
    );
    let _ = std::fs::write(&router_script_path, &router_script_content);

    // 2. Build per-pane structural configs
    let window_defs = build_window_defs(
        &repo,
        &worktree_root,
        &orchestrator_worktree,
        agent_kind,
        no_init,
        name,
        number,
        &session_id,
        &run_id,
        &runtime_identity,
        pattern_slug,
    );

    // Skills + MCP: sync all roots concurrently (blocking I/O off the async runtime).
    let skill_roots: Vec<PathBuf> = std::iter::once(orchestrator_worktree.clone())
        .chain(worktree_records.iter().map(|(_, _, path, _)| path.clone()))
        .collect();
    sync_skills_parallel(&skill_roots).await;
    trust_workspaces(&skill_roots).await;

    // Check unique agent kinds inside the squad to set up MCP properly
    let unique_agents: std::collections::HashSet<String> = window_defs
        .iter()
        .map(|w| w.agent_kind.to_ascii_lowercase())
        .collect();

    if unique_agents.contains("kimi") {
        kill_toad_processes();
        ensure_devorch_mcp_ready().await?;
    }
    for agent in &unique_agents {
        if agent != "kimi" && agent != "none" {
            ensure_mcp_server_registered(agent);
        }
    }

    // 3. Build per-pane startup commands, then create layout and inject in one Python session
    let titles_by_role = build_titles_by_role(&window_defs);
    let startup_by_role = build_startup_commands(&window_defs, &session_id);
    let init_by_role = build_init_by_role(&window_defs, no_init);
    // The team layout is fixed regardless of agent kind — any `AgentKind` here
    // yields the same `LayoutSpec`/pane count (agent only affects per-role
    // model metadata, unused for geometry).
    let team_layout = PatternConfig::team(patterns::AgentKind::Claude);
    let iterm_sessions = create_iterm_layout(
        &session_id,
        &titles_by_role,
        &startup_by_role,
        &team_layout.layout,
        team_layout.pane_count(),
    )
    .await?;
    if iterm_sessions.len() != GRID_ORDER.len() {
        anyhow::bail!(
            "expected {} iTerm2 sessions, got {}",
            GRID_ORDER.len(),
            iterm_sessions.len()
        );
    }

    // Kimi panes launch the interactive CLI directly and cannot take their init
    // prompt as a launch argument, so inject it straight into the pane after the
    // layout settles (fast-retry until the pane is ready). claude/agy/codex panes
    // already receive their init inline on the agent command line.
    if !init_by_role.is_empty() {
        run_batch_init(&session_id, &init_by_role, &iterm_sessions, &titles_by_role).await;
    }

    for wdef in window_defs.iter() {
        let role = wdef
            .env
            .get("DEVORCH_ROLE")
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        let iterm_key = if role == "orchestrator" {
            "orchestrator"
        } else {
            role
        };
        let iterm_session_id = iterm_sessions
            .get(iterm_key)
            .map(|s| &s[..s.len().min(8)])
            .unwrap_or("?");
        println!(
            "  + {:<32} {} (iterm: {})",
            wdef.name, wdef.label, iterm_session_id
        );
    }

    // 5. Register with Relay DB
    queries::insert_machine(&db_pool, &config.machine_id).await?;

    let session = Session {
        id: session_id.clone(),
        machine_id: config.machine_id.clone(),
        project_slug: name.to_string(),
        slot_number: number as i64,
        status: "active".to_string(),
        created_at: chrono::Utc::now(),
        pattern: pattern_slug.to_string(),
    };
    queries::insert_session(&db_pool, &session).await?;

    // NOTE: there is no session-lifecycle workflow. Each pane hosts its agent CLI
    // directly and SQLite is the single source of truth for session/agent state;
    // delivery injects straight into the iTerm2 panes registered below. No Temporal
    // bootstrap is started on the launch path.

    // Register agents + terminal targets concurrently (one task per pane).
    register_agents_parallel(
        &db_pool,
        &session_id,
        name,
        number,
        &window_defs,
        &iterm_sessions,
    )
    .await?;

    surface_codex_launch_errors(&db_pool, &session_id, &window_defs, &iterm_sessions).await?;

    println!(
        "\nWorkspace ready — iTerm2 window opened for session '{}'.",
        session_id
    );
    println!("Orch pane is active. Switch to iTerm2 to begin.");

    Ok(())
}

/// Launch a Simple Orchestrator squad: one orchestrator worktree + N worker
/// worktrees (`<name>-worker-<i>-<number>`), panes laid out per
/// `pattern_config.layout`. All workers share `worker_model`; the
/// orchestrator uses `orch`. Mirrors `launch_team`'s worktree/registration
/// pipeline but is pane-count-generic instead of hardcoded to `GRID_ORDER`.
#[allow(clippy::too_many_arguments)]
async fn launch_simple(
    name: Option<&str>,
    number: Option<u32>,
    no_init: bool,
    pattern_config: &PatternConfig,
    orch: &ModelChoice,
    worker_model: &ModelChoice,
    pattern_slug: &str,
) -> Result<()> {
    let repo = find_git_repo()?;
    ensure_squad_services();
    let repo_name = repo
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string();
    let config = crate::config::Config::load()?;
    let db_pool = crate::db::init_db(&config.database_url).await?;

    let name = name.unwrap_or(&repo_name);
    let number = match number {
        Some(n) => n,
        None => allocate_session_number(&db_pool, &repo, name).await,
    };
    let session_id = workspace_session_id(name, number);
    let worktree_root = repo.join(".claude").join("worktrees").join(&session_id);

    if worktree_root.exists() {
        anyhow::bail!(
            "worktree root {} already exists. Pick a different number or clean up manually.",
            worktree_root.display()
        );
    }

    info!(repo = %repo.display(), session = %session_id, "Launching simple-orchestrator squad workspace");

    let _ = ensure_antigravity_project_trusted(&repo);
    let _ = ensure_gemini_project_trusted(&repo);

    let worker_count = pattern_config
        .roles
        .iter()
        .filter(|r| r.role.starts_with("worker-"))
        .count() as u8;

    // 1. Create worktrees in parallel: orchestrator + N workers.
    let orchestrator_worktree =
        create_orchestrator_worktree(&repo, &worktree_root, &session_id).await?;
    let _worker_records =
        create_simple_worker_worktrees_parallel(&repo, &worktree_root, name, number, worker_count)
            .await?;

    let run_id = format!(
        "{}-{}",
        session_id,
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    );

    let runtime_identity = RuntimeIdentityEnv::new(
        &repo,
        name,
        &config.temporal_namespace,
        DEVORCH_DEFAULT_TASK_QUEUE,
    );

    // Input router: routes to `orch` by default, or `/worker-<i> ...` to a
    // specific worker pane.
    let worker_roles: Vec<String> = (1..=worker_count).map(|i| format!("worker-{i}")).collect();
    let router_script_path = format!("/tmp/devorch-input-router-{}.py", session_id);
    let router_script_content = build_simple_input_router_script(
        &session_id,
        &run_id,
        name,
        &runtime_identity,
        &worker_roles,
    );
    let _ = std::fs::write(&router_script_path, &router_script_content);

    // 2. Build per-pane structural configs from the pattern's RoleSpecs.
    let window_defs = build_simple_window_defs(
        &worktree_root,
        &orchestrator_worktree,
        no_init,
        name,
        number,
        &session_id,
        &run_id,
        &runtime_identity,
        pattern_slug,
        pattern_config,
    );

    // Skills + MCP: sync all roots concurrently (blocking I/O off the async runtime).
    let skill_roots: Vec<PathBuf> = window_defs
        .iter()
        .map(|w| PathBuf::from(&w.dir))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    sync_skills_parallel(&skill_roots).await;
    trust_workspaces(&skill_roots).await;

    let unique_agents: std::collections::HashSet<String> = window_defs
        .iter()
        .map(|w| w.agent_kind.to_ascii_lowercase())
        .collect();
    if unique_agents.contains("kimi") {
        kill_toad_processes();
        ensure_devorch_mcp_ready().await?;
    }
    for agent in &unique_agents {
        if agent != "kimi" && agent != "none" {
            ensure_mcp_server_registered(agent);
        }
    }

    // 3. Build per-pane startup commands, then create layout and inject in one Python session.
    let titles_by_role = build_titles_by_role(&window_defs);
    let startup_by_role = build_startup_commands(&window_defs, &session_id);
    let init_by_role = build_init_by_role(&window_defs, no_init);
    let iterm_sessions = create_iterm_layout_for_pattern(
        &session_id,
        &pattern_config.layout,
        &titles_by_role,
        &startup_by_role,
    )
    .await?;
    if iterm_sessions.len() != pattern_config.pane_count() {
        anyhow::bail!(
            "expected {} iTerm2 sessions, got {}",
            pattern_config.pane_count(),
            iterm_sessions.len()
        );
    }

    if !init_by_role.is_empty() {
        run_batch_init(&session_id, &init_by_role, &iterm_sessions, &titles_by_role).await;
    }

    for wdef in window_defs.iter() {
        let role = wdef
            .env
            .get("DEVORCH_ROLE")
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        let iterm_key = iterm_role_key(role);
        let iterm_session_id = iterm_sessions
            .get(iterm_key)
            .map(|s| &s[..s.len().min(8)])
            .unwrap_or("?");
        println!(
            "  + {:<32} {} (iterm: {})",
            wdef.name, wdef.label, iterm_session_id
        );
    }

    // 5. Register with Relay DB.
    queries::insert_machine(&db_pool, &config.machine_id).await?;

    let session = Session {
        id: session_id.clone(),
        machine_id: config.machine_id.clone(),
        project_slug: name.to_string(),
        slot_number: number as i64,
        status: "active".to_string(),
        created_at: chrono::Utc::now(),
        pattern: pattern_slug.to_string(),
    };
    queries::insert_session(&db_pool, &session).await?;

    register_agents_parallel(
        &db_pool,
        &session_id,
        name,
        number,
        &window_defs,
        &iterm_sessions,
    )
    .await?;

    surface_codex_launch_errors(&db_pool, &session_id, &window_defs, &iterm_sessions).await?;

    println!(
        "\nWorkspace ready — iTerm2 window opened for session '{}'.",
        session_id
    );
    println!(
        "Orch pane is active ({} workers: {} — model {}). Switch to iTerm2 to begin.",
        worker_count, worker_model.label, orch.label
    );

    Ok(())
}

/// Create N worker worktrees concurrently for the Simple Orchestrator
/// pattern, named `<name>-worker-<i>-<number>` (mirrors
/// `create_worker_worktrees_parallel`'s team-role variant but over
/// `worker-1..=N` instead of the fixed `GRID_ORDER` team roles).
async fn create_simple_worker_worktrees_parallel(
    repo: &Path,
    worktree_root: &Path,
    name: &str,
    number: u32,
    workers: u8,
) -> Result<Vec<(String, String, PathBuf, String)>> {
    let repo_str = repo.to_string_lossy().to_string();
    let mut set = JoinSet::new();
    for i in 1..=workers {
        let team = format!("worker-{i}");
        let repo_str = repo_str.clone();
        let worktree_root = worktree_root.to_path_buf();
        let name = name.to_string();
        set.spawn(async move {
            create_one_worker_worktree(&repo_str, &worktree_root, &name, &team, number).await
        });
    }
    let mut records = Vec::with_capacity(workers as usize);
    while let Some(res) = set.join_next().await {
        records.push(res.context("worktree task join")??);
    }
    records.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(records)
}

/// Build one `WindowDef` per `RoleSpec` in `pattern_config.roles` — generic
/// over pane count, unlike `build_window_defs` (which walks the fixed
/// `GRID_ORDER`).
#[allow(clippy::too_many_arguments)]
fn build_simple_window_defs(
    worktree_root: &Path,
    orchestrator_worktree: &Path,
    no_init: bool,
    name: &str,
    number: u32,
    workspace_session: &str,
    run_id: &str,
    runtime_identity: &RuntimeIdentityEnv,
    pattern_slug: &str,
    pattern_config: &PatternConfig,
) -> Vec<WindowDef> {
    let mut defs = Vec::with_capacity(pattern_config.roles.len());
    for role_spec in &pattern_config.roles {
        let role = role_spec.role.as_str();
        let agent_kind_str = role_spec.model.agent.as_str();
        let env = base_window_env(
            runtime_identity,
            workspace_session,
            run_id,
            role,
            name,
            number,
            pattern_slug,
        );

        if role == "inp" {
            let pane_name = format!("{}-inp-{}", name, number);
            defs.push(WindowDef {
                name: pane_name,
                label: role_spec.label.clone(),
                color: role_spec.color,
                dir: orchestrator_worktree.to_string_lossy().to_string(),
                env,
                cmd: format!("python3 /tmp/devorch-input-router-{}.py", workspace_session),
                agent_kind: "none".to_string(),
            });
            continue;
        }

        let (pane_name, dir) = if role == "orch" {
            (
                workspace_session.to_string(),
                orchestrator_worktree.to_string_lossy().to_string(),
            )
        } else {
            let leaf = pane_name_for(name, role, number);
            (
                leaf.clone(),
                worktree_root.join(&leaf).to_string_lossy().to_string(),
            )
        };

        let init = if no_init {
            None
        } else {
            Some(format!(
                "Fetch your initialization instructions by calling the `devorch_get_setup_instructions` MCP tool immediately. Use session={}, role={}, agent={}, repo_id={}, temporal_namespace={}.",
                workspace_session, role, agent_kind_str, name, runtime_identity.temporal_namespace
            ))
        };

        defs.push(WindowDef {
            name: pane_name.clone(),
            label: role_spec.label.clone(),
            color: role_spec.color,
            dir,
            env,
            cmd: build_pattern_agent_command(&role_spec.model, init.as_deref(), Some(&pane_name)),
            agent_kind: agent_kind_str.to_string(),
        });
    }
    defs
}

/// Build the CLI invocation for a `ModelChoice` (Simple Orchestrator's
/// orch/worker panes) — same shape as `build_agent_command`, but keyed on the
/// explicit `AgentKind`/`model_id`/`effort` from the pattern menu instead of
/// team's role-name → model lookup tables.
fn build_pattern_agent_command(
    model: &ModelChoice,
    init: Option<&str>,
    pane_name: Option<&str>,
) -> String {
    let suffix = init
        .map(|s| format!(" {}", shell_escape(s)))
        .unwrap_or_default();
    match model.agent {
        AgentKind::Claude => {
            let name_arg = pane_name
                .map(|n| format!(" --name {}", shell_escape(n)))
                .unwrap_or_default();
            format!(
                "claude --model {} --dangerously-skip-permissions{}{}",
                model.model_id, name_arg, suffix
            )
        }
        AgentKind::Codex => {
            let remote = pane_name
                .map(|n| {
                    format!(
                        "--remote {} ",
                        shell_escape(&format!("unix://codex-devorch-sockets/{}.sock", n))
                    )
                })
                .unwrap_or_default();
            let cd_arg = if pane_name.is_some() {
                "--cd \"$workdir\" "
            } else {
                ""
            };
            let codex_cmd = format!(
                "codex {}{}--model {} -c 'model_reasoning_effort=\"{}\"' -c shell_environment_policy.inherit=all --dangerously-bypass-approvals-and-sandbox{}",
                remote, cd_arg, model.model_id, model.effort, suffix
            );
            if let Some(name) = pane_name {
                codex_app_server_wrapper(name, &codex_cmd)
            } else {
                codex_cmd
            }
        }
        AgentKind::Gemini => {
            // No standalone `gemini` CLI wiring exists in this codebase today —
            // Gemini-family models ride the Antigravity (`agy`) CLI via
            // ANTIGRAVITY_MODEL, matching the only other Gemini launch path
            // (`build_agent_command`'s "agy" arm).
            let prompt_arg = init
                .map(|s| format!(" --prompt-interactive {}", shell_escape(s)))
                .unwrap_or_default();
            format!(
                "env -u TERM_PROGRAM -u ITERM_SESSION_ID -u TERM_PROGRAM_VERSION ANTIGRAVITY_MODEL={} agy --dangerously-skip-permissions{}",
                shell_escape(&model.model_id), prompt_arg
            )
        }
        AgentKind::Kimi => {
            let mcp_cfg = devorch_mcp_config_path();
            format!(
                "command env -u TERM_PROGRAM -u ITERM_SESSION_ID -u TERM_PROGRAM_VERSION PATH={}:{}:$PATH kimi --mcp-config-file {} -m {} -y",
                shell_escape("/opt/homebrew/bin"),
                shell_escape(&dirs::home_dir().unwrap().join(".local/bin").to_string_lossy()),
                shell_escape(&mcp_cfg.to_string_lossy()),
                shell_escape(&model.model_id)
            )
        }
        AgentKind::Goose => {
            let provider = crate::delivery::acp::goose_provider_for_agent("claude");
            let name_arg = pane_name
                .map(|n| format!(" --name {}", shell_escape(n)))
                .unwrap_or_default();
            format!(
                "env -u TERM_PROGRAM -u ITERM_SESSION_ID -u TERM_PROGRAM_VERSION \
                 GOOSE_PROVIDER={} GOOSE_MODEL={} GOOSE_DISABLE_KEYRING=1 \
                 goose session{} --with-extension {}",
                provider,
                model.model_id,
                name_arg,
                shell_escape(&crate::delivery::acp::devorch_extension_value()),
            )
        }
    }
}

/// Python input router for the Simple Orchestrator pattern: routes free-text
/// to `orch` by default, or `/worker-<i> <cmd>` to a specific worker pane.
/// Same shape as `launch_team`'s inline router script, parameterized over the
/// dynamic worker role list instead of the fixed team roles.
fn build_simple_input_router_script(
    session_id: &str,
    run_id: &str,
    repo_id: &str,
    runtime_identity: &RuntimeIdentityEnv,
    worker_roles: &[String],
) -> String {
    let worker_roles_py = worker_roles
        .iter()
        .map(|r| format!("'{}'", r))
        .collect::<Vec<_>>()
        .join(", ");
    let worker_roles_display = worker_roles.join(", ");
    format!(
        r#"import sys, os, subprocess, select
try:
    import readline
    readline.set_history_length(100)
except ImportError:
    pass
session = '{session_id}'
sys.stdout.write(f'\x1b]0;INPUT - {{session}}\x07\x1b]1;INPUT - {{session}}\x07\x1b]2;INPUT - {{session}}\x07')
sys.stdout.flush()
print('\x1b[1;36m====================================================\x1b[0m')
print('\x1b[1;37m           SIMPLE ORCHESTRATOR INPUT ROUTER        \x1b[0m')
print('\x1b[1;36m====================================================\x1b[0m')
print('Type your note or command. Press Enter to submit.')
print('  - Arrow keys and backspace work for editing.')
print('  - For multiline, type or paste lines then press Ctrl-D to submit.')
print('  - Ctrl-C aborts current input.')
print('Type "/<role> <command>" to route to a specific worker window.')
print('  (Available roles: {worker_roles_display})\n')

worker_roles = [{worker_roles_py}]

def process_cmd(cmd):
    cmd = cmd.strip()
    if not cmd:
        return
    matched_worker = None
    for role in worker_roles:
        if cmd.startswith(f'/{{role}} '):
            matched_worker = role
            actual_cmd = cmd[len(role) + 2:].strip()
            break
        elif cmd == f'/{{role}}':
            matched_worker = role
            actual_cmd = ""
            break

    if matched_worker:
        target_role = matched_worker
        role_label = matched_worker.upper()
    else:
        target_role = 'orch'
        actual_cmd = cmd
        role_label = 'ORCH'

    print(f'\x1b[1;33mRouting note to {{role_label}}: "{{actual_cmd}}"\x1b[0m')

    env = os.environ.copy()
    env['DEVORCH_SESSION'] = '{session_id}'
    env['DEVORCH_RUN_ID'] = '{run_id}'
    env['DEVORCH_REPO_ID'] = '{repo_id}'
    env['DEVORCH_REPO_ROOT'] = '{repo_root}'
    env['DEVORCH_TEMPORAL_NAMESPACE'] = '{temporal_namespace}'
    env['DEVORCH_TASK_QUEUE'] = '{task_queue}'

    subprocess.run(['lantern', 'note', target_role, actual_cmd], env=env)

    # Inject the note directly into the active iTerm2 terminal pane
    if actual_cmd:
        try:
            import iterm2

            def find_session_by_role(app, session_id, role):
                if role == "orch":
                    target_contains = "ORCH - " + session_id
                else:
                    parts = session_id.rsplit("-", 1)
                    if len(parts) == 2:
                        project_slug, slot = parts
                        target_contains = f"{{project_slug}}-{{role}}-{{slot}}"
                    else:
                        target_contains = f"{{session_id}}-{{role}}"
                for w in app.windows:
                    for t in w.tabs:
                        for s in t.sessions:
                            name = s.name or ""
                            if target_contains in name:
                                return s
                return None

            async def inject(connection):
                app = await iterm2.async_get_app(connection)
                s = find_session_by_role(app, session, target_role)
                if s:
                    await s.async_send_text(actual_cmd)
                    import asyncio
                    await asyncio.sleep(0.05)
                    await s.async_send_text("\r")

            iterm2.run_until_complete(inject)
        except Exception as e:
            import sys
            print(f"Error injecting to iTerm2 pane: {{e}}", file=sys.stderr)

def edit_loop():
    lines = []
    while True:
        try:
            prompt = '\x1b[1;32m[INPUT] ❯ \x1b[0m' if not lines else '          '
            line = input(prompt)
            lines.append(line)
            # If no more input arrives within 50 ms (single line typed), submit now.
            # Multiline paste fills the buffer immediately so select returns True.
            if not select.select([sys.stdin], [], [], 0.05)[0]:
                break
        except EOFError:
            # Ctrl-D: submit whatever has been collected
            break
        except KeyboardInterrupt:
            raise
    return '\n'.join(lines)

try:
    while True:
        try:
            cmd = edit_loop()
        except (KeyboardInterrupt, EOFError):
            break
        if cmd.strip():
            process_cmd(cmd)
finally:
    sys.stdout.write('\r\n')
    sys.stdout.flush()"#,
        session_id = session_id,
        run_id = run_id,
        repo_id = repo_id,
        repo_root = runtime_identity.repo_root,
        temporal_namespace = runtime_identity.temporal_namespace,
        task_queue = runtime_identity.task_queue,
        worker_roles_py = worker_roles_py,
        worker_roles_display = worker_roles_display,
    )
}

/// Launch the SOLO GOOSE model: ONE headed, full-featured native `goose session`
/// in its own worktree, for focused fixes.
///
/// - You talk to goose directly in a single window; it does the work itself (and
///   can fan out via goose's own native subagents). No devorch, no specialist
///   fleet, no orchestrator role.
/// - Full-featured on purpose: we do NOT strip `TERM_PROGRAM`/`ITERM_SESSION_ID`
///   (so goose sees iTerm2 and keeps its terminal-rich TUI), and we leave the
///   keyring enabled. The provider stays `claude-acp` — the only zero-credential
///   path (rides the Claude Code CLI subscription; no API key, matching this
///   repo's no-secrets model).
/// - beads is available via the shell. `stopwork` closes the window and removes
///   the worktree/branch.
#[allow(clippy::too_many_arguments)]
async fn launch_solo_goose(
    repo: &Path,
    name: &str,
    number: u32,
    session_id: &str,
    worktree_root: &Path,
    config: &crate::config::Config,
    db_pool: &sqlx::SqlitePool,
    pattern_slug: &str,
) -> Result<()> {
    let _ = ensure_antigravity_project_trusted(repo);
    let _ = ensure_gemini_project_trusted(repo);
    info!(repo = %repo.display(), session = %session_id, "Launching solo goose session");

    // One worktree for the solo session.
    let worktree = create_orchestrator_worktree(repo, worktree_root, session_id).await?;
    sync_skills_parallel(std::slice::from_ref(&worktree)).await;

    // Full-featured native goose: claude-acp/opus, terminal env left intact so
    // iTerm2 features work, keyring on, no devorch extension.
    let provider = crate::delivery::acp::goose_provider_for_agent("claude");
    let model = crate::delivery::acp::goose_model_for_role("claude", "orchestrator");
    let command = format!("GOOSE_PROVIDER={provider} GOOSE_MODEL={model} goose session");

    let title = format!("GOOSE - {session_id}");
    let iterm_session_id =
        create_solo_window(&title, &worktree.to_string_lossy(), &command).await?;

    // Register session + the single agent so `stopwork` can tear it down.
    queries::insert_machine(db_pool, &config.machine_id).await?;
    let session = Session {
        id: session_id.to_string(),
        machine_id: config.machine_id.clone(),
        project_slug: name.to_string(),
        slot_number: number as i64,
        status: "active".to_string(),
        created_at: chrono::Utc::now(),
        pattern: pattern_slug.to_string(),
    };
    queries::insert_session(db_pool, &session).await?;
    let agent = Agent {
        id: generate_id(&format!("agent-{}-goose-solo", session_id)),
        session_id: session_id.to_string(),
        role: "orchestrator".to_string(),
        pane_id: Some(iterm_session_id.clone()),
        worktree_path: worktree.to_string_lossy().to_string(),
        branch: session_id.to_string(),
        agent_kind: "goose".to_string(),
        status: "idle".to_string(),
        last_seen_at: Some(chrono::Utc::now()),
        created_at: chrono::Utc::now(),
    };
    queries::insert_agent(db_pool, &agent).await?;
    queries::insert_terminal_target(
        db_pool,
        &TerminalTarget {
            agent_id: agent.id.clone(),
            iterm_session_id: iterm_session_id.clone(),
            pane_id: Some(iterm_session_id),
            transport_status: "ready".to_string(),
            last_seen_at: Some(chrono::Utc::now()),
        },
    )
    .await?;

    println!("\nSolo goose window opened for session '{session_id}'.");
    println!("Talk to it directly — single full-featured goose session for focused fixes.");
    println!("Inspect: `lantern status` · beads `bd ready` · stop: `stopwork {session_id}`");
    Ok(())
}

/// Open a single iTerm2 window running `command` in `cwd`; returns its session id.
async fn create_solo_window(title: &str, cwd: &str, command: &str) -> Result<String> {
    let script_path = crate::terminal::locate_script("iterm_solo.py")?;
    let output = Command::new("python3")
        .args([
            script_path.to_str().context("non-UTF-8 script path")?,
            "--title",
            title,
            "--cwd",
            cwd,
            "--command",
            command,
        ])
        .output()
        .await
        .context("failed to launch iterm_solo.py")?;
    if !output.status.success() {
        anyhow::bail!(
            "iterm_solo.py failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let map: std::collections::HashMap<String, String> = serde_json::from_str(stdout.trim())
        .with_context(|| format!("iterm_solo.py returned invalid JSON: {}", stdout.trim()))?;
    map.get("orchestrator")
        .cloned()
        .context("iterm_solo.py did not return an orchestrator session id")
}

fn get_agent_for_role(_role: &str) -> &'static str {
    "claude"
}

/// Per-pane definition matching the Python window_def.
struct WindowDef {
    name: String,
    label: String,
    color: (u8, u8, u8),
    dir: String,
    env: std::collections::HashMap<String, String>,
    cmd: String,
    agent_kind: String,
}

struct RuntimeIdentityEnv {
    repo_id: String,
    repo_root: String,
    temporal_namespace: String,
    task_queue: String,
}

impl RuntimeIdentityEnv {
    fn new(repo: &Path, repo_id: &str, temporal_namespace: &str, task_queue: &str) -> Self {
        let repo_root = repo
            .canonicalize()
            .unwrap_or_else(|_| repo.to_path_buf())
            .to_string_lossy()
            .to_string();
        Self {
            repo_id: repo_id.to_string(),
            repo_root,
            temporal_namespace: temporal_namespace.to_string(),
            task_queue: task_queue.to_string(),
        }
    }

    fn apply_to(&self, env: &mut std::collections::HashMap<String, String>) {
        env.insert("DEVORCH_REPO_ID".to_string(), self.repo_id.clone());
        env.insert("DEVORCH_REPO_ROOT".to_string(), self.repo_root.clone());
        env.insert(
            "DEVORCH_TEMPORAL_NAMESPACE".to_string(),
            self.temporal_namespace.clone(),
        );
        env.insert("DEVORCH_TASK_QUEUE".to_string(), self.task_queue.clone());
    }
}

fn base_window_env(
    runtime_identity: &RuntimeIdentityEnv,
    workspace_session: &str,
    run_id: &str,
    role: &str,
    name: &str,
    number: u32,
    pattern_slug: &str,
) -> std::collections::HashMap<String, String> {
    let mut env = std::collections::HashMap::new();
    env.insert("DEVORCH_SESSION".to_string(), workspace_session.to_string());
    env.insert("DEVORCH_RUN_ID".to_string(), run_id.to_string());
    env.insert("DEVORCH_ROLE".to_string(), role.to_string());
    env.insert("DEVORCH_PROJECT_SLUG".to_string(), name.to_string());
    env.insert("DEVORCH_SLOT".to_string(), number.to_string());
    env.insert("DEVORCH_PATTERN".to_string(), pattern_slug.to_string());
    // Local self-contained backend — no remote Temporal or remote DB.
    env.insert(
        "DEVORCH_TEMPORAL_ADDRESS".to_string(),
        "127.0.0.1:8243".to_string(),
    );
    let db_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("$HOME"))
        .join(".lantern")
        .join("data")
        .join("relay")
        .join("lantern.db");
    env.insert(
        "DEVORCH_DB_URL".to_string(),
        format!("file:{}", db_path.to_string_lossy()),
    );
    runtime_identity.apply_to(&mut env);
    env
}

fn orchestrator_worktree_path(worktree_root: &Path, workspace_session: &str) -> PathBuf {
    worktree_root.join(workspace_session)
}

#[allow(clippy::too_many_arguments)]
fn build_window_defs(
    _repo: &Path,
    worktree_root: &Path,
    orchestrator_worktree: &Path,
    agent_override: Option<&str>,
    no_init: bool,
    name: &str,
    number: u32,
    workspace_session: &str,
    run_id: &str,
    runtime_identity: &RuntimeIdentityEnv,
    pattern_slug: &str,
) -> Vec<WindowDef> {
    let mut defs = Vec::new();
    for grid_team in GRID_ORDER {
        if *grid_team == "orch" {
            let role = "orchestrator";
            let role_agent = agent_override.unwrap_or_else(|| get_agent_for_role(role));
            let init = if no_init {
                None
            } else {
                Some(format!(
                    "Fetch your initialization instructions by calling the `devorch_get_setup_instructions` MCP tool immediately. Use session={}, role={}, agent={}, repo_id={}, temporal_namespace={}.",
                    workspace_session, role, role_agent, name, runtime_identity.temporal_namespace
                ))
            };
            let env = base_window_env(
                runtime_identity,
                workspace_session,
                run_id,
                role,
                name,
                number,
                pattern_slug,
            );
            defs.push(WindowDef {
                name: workspace_session.to_string(),
                label: "ORCH".to_string(),
                color: (30, 32, 35),
                dir: orchestrator_worktree.to_string_lossy().to_string(),
                env,
                cmd: build_agent_command(
                    role_agent,
                    role,
                    init.as_deref(),
                    Some(workspace_session),
                ),
                agent_kind: role_agent.to_string(),
            });
        } else if *grid_team == "inp" {
            let role = "input";
            let env = base_window_env(
                runtime_identity,
                workspace_session,
                run_id,
                role,
                name,
                number,
                pattern_slug,
            );
            let (r, g, b) = team_color(grid_team);
            let input_cmd = format!("python3 /tmp/devorch-input-router-{}.py", workspace_session);

            defs.push(WindowDef {
                name: format!("{}-inp-{}", name, number),
                label: "INPUT".to_string(),
                color: (r, g, b),
                dir: orchestrator_worktree.to_string_lossy().to_string(),
                env,
                cmd: input_cmd,
                agent_kind: "none".to_string(),
            });
        } else {
            let role = *grid_team;
            let role_agent = agent_override.unwrap_or_else(|| get_agent_for_role(role));
            let leaf = pane_name_for(name, grid_team, number);
            let init = if no_init {
                None
            } else {
                Some(format!(
                    "Fetch your initialization instructions by calling the `devorch_get_setup_instructions` MCP tool immediately. Use session={}, role={}, agent={}, repo_id={}, temporal_namespace={}.",
                    workspace_session, role, role_agent, name, runtime_identity.temporal_namespace
                ))
            };
            let env = base_window_env(
                runtime_identity,
                workspace_session,
                run_id,
                role,
                name,
                number,
                pattern_slug,
            );
            let (r, g, b) = team_color(grid_team);
            defs.push(WindowDef {
                name: leaf.clone(),
                label: team_label(grid_team).to_string(),
                color: (r, g, b),
                dir: worktree_root.join(&leaf).to_string_lossy().to_string(),
                env,
                cmd: build_agent_command(role_agent, role, init.as_deref(), Some(&leaf)),
                agent_kind: role_agent.to_string(),
            });
        }
    }
    defs
}

/// Build the CLI invocation for a given agent kind.
fn build_agent_command(
    agent_kind: &str,
    role: &str,
    init: Option<&str>,
    pane_name: Option<&str>,
) -> String {
    let suffix = init
        .map(|s| format!(" {}", shell_escape(s)))
        .unwrap_or_default();
    let model = get_model_for_role(agent_kind, role);
    let cmd = match agent_kind.to_lowercase().as_str() {
        "claude" => {
            let name_arg = pane_name
                .map(|n| format!(" --name {}", shell_escape(n)))
                .unwrap_or_default();
            format!(
                "claude --model {} --dangerously-skip-permissions{}{}",
                model, name_arg, suffix
            )
        }
        "agy" => {
            let prompt_arg = init
                .map(|s| format!(" --prompt-interactive {}", shell_escape(s)))
                .unwrap_or_default();
            format!(
                "env -u TERM_PROGRAM -u ITERM_SESSION_ID -u TERM_PROGRAM_VERSION ANTIGRAVITY_MODEL={} agy --dangerously-skip-permissions{}",
                shell_escape(&model), prompt_arg
            )
        }
        "codex" => {
            let reasoning = codex_reasoning_effort_for_role(role);
            info!(
                role = %role,
                agent = %agent_kind,
                model = %model,
                reasoning_effort = %reasoning,
                "Resolved Codex launch configuration"
            );
            let remote = pane_name
                .map(|n| {
                    format!(
                        "--remote {} ",
                        shell_escape(&format!("unix://codex-devorch-sockets/{}.sock", n))
                    )
                })
                .unwrap_or_default();
            let cd_arg = if pane_name.is_some() {
                "--cd \"$workdir\" "
            } else {
                ""
            };
            let codex_cmd = format!(
                "codex {}{}--model {} -c 'model_reasoning_effort=\"{}\"' -c shell_environment_policy.inherit=all --dangerously-bypass-approvals-and-sandbox{}",
                remote, cd_arg, model, reasoning, suffix
            );
            if let Some(name) = pane_name {
                codex_app_server_wrapper(name, &codex_cmd)
            } else {
                codex_cmd
            }
        }
        "kimi" => {
            // Kimi Code CLI — native MCP via --mcp-config-file (see ensure_devorch_mcp_ready).
            let _ = init;
            let mcp_cfg = devorch_mcp_config_path();
            let cmd = format!(
                "command env -u TERM_PROGRAM -u ITERM_SESSION_ID -u TERM_PROGRAM_VERSION PATH={}:{}:$PATH kimi --mcp-config-file {} -m {} -y",
                shell_escape("/opt/homebrew/bin"),
                shell_escape(&dirs::home_dir().unwrap().join(".local/bin").to_string_lossy()),
                shell_escape(&mcp_cfg.to_string_lossy()),
                shell_escape(&model)
            );
            debug_assert!(
                !cmd.contains(" term") && !cmd.contains(" -p "),
                "kimi spawn must stay interactive (no Toad, no -p)"
            );
            cmd
        }
        "goose" => {
            // Headed Goose session driving an ACP provider (claude-acp), riding
            // existing CLI auth. The session is watchable in the pane and is a
            // live command target (delivery injects via the iTerm transport just
            // like the other TUIs). devorch passes through via --with-extension;
            // the init prompt is injected post-launch (see build_init_by_role),
            // because `goose session` has no inline-prompt flag.
            let _ = init;
            let provider = crate::delivery::acp::goose_provider_for_agent("claude");
            let gmodel = crate::delivery::acp::goose_model_for_role("claude", role);
            let name_arg = pane_name
                .map(|n| format!(" --name {}", shell_escape(n)))
                .unwrap_or_default();
            format!(
                "env -u TERM_PROGRAM -u ITERM_SESSION_ID -u TERM_PROGRAM_VERSION \
                 GOOSE_PROVIDER={} GOOSE_MODEL={} GOOSE_DISABLE_KEYRING=1 \
                 goose session{} --with-extension {}",
                provider,
                gmodel,
                name_arg,
                shell_escape(&crate::delivery::acp::devorch_extension_value()),
            )
        }
        "gemini" => {
            // gemini CLI (`gemini --help`) has no reasoning-effort/thinking
            // flag as of this writing; `-i/--prompt-interactive` runs the
            // init prompt then continues interactively, mirroring how the
            // other TUIs get their init message.
            let prompt_arg = init
                .map(|s| format!(" --prompt-interactive {}", shell_escape(s)))
                .unwrap_or_default();
            format!("gemini --model {}{}", model, prompt_arg)
        }
        _ => {
            format!(
                "claude --model {} --dangerously-skip-permissions{}",
                model, suffix
            )
        }
    };
    cmd
}

/// Build the CLI invocation for a `ModelChoice` (agent kind + model id +
/// effort) directly, independent of the role-based `get_model_for_role`
/// lookup. Used by the non-team launch patterns (executor / simple / fixbug);
/// the legacy `TeamOrchestrator` path keeps using `build_agent_command` +
/// `get_model_for_role` unchanged.
pub fn build_agent_command_for(
    choice: &ModelChoice,
    init: Option<&str>,
    pane_name: Option<&str>,
) -> String {
    let suffix = init
        .map(|s| format!(" {}", shell_escape(s)))
        .unwrap_or_default();
    let model = choice.model_id.as_str();
    let effort = choice.effort.as_str();
    match choice.agent {
        AgentKind::Claude => {
            let name_arg = pane_name
                .map(|n| format!(" --name {}", shell_escape(n)))
                .unwrap_or_default();
            format!(
                "claude --model {} --effort {} --dangerously-skip-permissions{}{}",
                model, effort, name_arg, suffix
            )
        }
        AgentKind::Codex => {
            info!(
                agent = "codex",
                model = %model,
                reasoning_effort = %effort,
                "Resolved Codex launch configuration (ModelChoice)"
            );
            let remote = pane_name
                .map(|n| {
                    format!(
                        "--remote {} ",
                        shell_escape(&format!("unix://codex-devorch-sockets/{}.sock", n))
                    )
                })
                .unwrap_or_default();
            let cd_arg = if pane_name.is_some() {
                "--cd \"$workdir\" "
            } else {
                ""
            };
            let codex_cmd = format!(
                "codex {}{}--model {} -c 'model_reasoning_effort=\"{}\"' -c shell_environment_policy.inherit=all --dangerously-bypass-approvals-and-sandbox{}",
                remote, cd_arg, model, effort, suffix
            );
            if let Some(name) = pane_name {
                codex_app_server_wrapper(name, &codex_cmd)
            } else {
                codex_cmd
            }
        }
        AgentKind::Gemini => {
            // gemini CLI (`gemini --help`) has no reasoning-effort/thinking
            // flag as of this writing; `choice.effort` is carried for menu
            // shape parity but is not injectable here — omitted.
            let prompt_arg = init
                .map(|s| format!(" --prompt-interactive {}", shell_escape(s)))
                .unwrap_or_default();
            format!("gemini --model {}{}", model, prompt_arg)
        }
        AgentKind::Kimi => {
            let _ = init;
            let mcp_cfg = devorch_mcp_config_path();
            format!(
                "command env -u TERM_PROGRAM -u ITERM_SESSION_ID -u TERM_PROGRAM_VERSION PATH={}:{}:$PATH kimi --mcp-config-file {} -m {} -y",
                shell_escape("/opt/homebrew/bin"),
                shell_escape(&dirs::home_dir().unwrap().join(".local/bin").to_string_lossy()),
                shell_escape(&mcp_cfg.to_string_lossy()),
                shell_escape(model)
            )
        }
        AgentKind::Goose => {
            let _ = init;
            let provider = crate::delivery::acp::goose_provider_for_agent("claude");
            let name_arg = pane_name
                .map(|n| format!(" --name {}", shell_escape(n)))
                .unwrap_or_default();
            format!(
                "env -u TERM_PROGRAM -u ITERM_SESSION_ID -u TERM_PROGRAM_VERSION \
                 GOOSE_PROVIDER={} GOOSE_MODEL={} GOOSE_DISABLE_KEYRING=1 \
                 goose session{} --with-extension {}",
                provider,
                model,
                name_arg,
                shell_escape(&crate::delivery::acp::devorch_extension_value()),
            )
        }
    }
}

fn codex_app_server_wrapper(pane_name: &str, codex_cmd: &str) -> String {
    let socket_dir = "/tmp/codex-devorch-sockets";
    let socket = format!("{}/{}.sock", socket_dir, pane_name);
    let listen = format!("unix://codex-devorch-sockets/{}.sock", pane_name);
    let log = format!("/tmp/codex-app-server-{}.log", pane_name);
    format!(
        "__devorch_run_codex() {{ \
         local sockdir={}; \
         local sock={}; \
         local listen={}; \
         local log={}; \
         local workdir=\"$PWD\"; \
         local app_pid codex_status i; \
         mkdir -p \"$sockdir\"; \
         rm -f \"$sock\"; \
         cd /tmp; \
         codex app-server --listen \"$listen\" > \"$log\" 2>&1 & \
         app_pid=$!; \
         i=0; \
         while [ \"$i\" -lt 100 ]; do \
         [ -S \"$sock\" ] && break; \
         sleep 0.1; \
         i=$((i + 1)); \
         done; \
         if [ ! -S \"$sock\" ]; then \
         echo \"codex app-server did not create $sock; see $log\" >&2; \
         return 1; \
         fi; \
         {}; \
         codex_status=$?; \
         kill \"$app_pid\" >/dev/null 2>&1 || true; \
         wait \"$app_pid\" >/dev/null 2>&1 || true; \
         rm -f \"$sock\"; \
         cd \"$workdir\"; \
         return \"$codex_status\"; \
         }}; __devorch_run_codex",
        shell_escape(socket_dir),
        shell_escape(&socket),
        shell_escape(&listen),
        shell_escape(&log),
        codex_cmd
    )
}

/// Get the configurable model setting for each role and agent kind.
fn get_model_for_role(agent_kind: &str, role: &str) -> String {
    match agent_kind.to_lowercase().as_str() {
        "claude" => match role {
            "orchestrator" | "ai" | "sec" => "opus".to_string(),
            "doc" => "haiku".to_string(),
            _ => "sonnet".to_string(),
        },
        "agy" => match role {
            "orchestrator" | "ai" | "sec" => "Gemini 3.1 Pro (Low)".to_string(),
            "doc" => "GPT-OSS 120B (Medium)".to_string(),
            _ => "Gemini 3.5 Flash (Medium)".to_string(),
        },
        "codex" => codex_model_for_role(role).to_string(),
        "gemini" => match role {
            "orchestrator" | "ai" | "sec" => "gemini-3.1-pro".to_string(),
            _ => "gemini-3.5-flash".to_string(),
        },
        "goose" => crate::delivery::acp::goose_model_for_role("claude", role),
        "kimi" => {
            // Must match a model id in ~/.kimi/config.toml (not the Toad "Default" alias).
            "kimi-code/kimi-for-coding".to_string()
        }
        _ => "default".to_string(),
    }
}

fn codex_model_for_role(role: &str) -> &'static str {
    CODEX_ROLE_MODELS
        .iter()
        .find(|(r, _)| *r == role)
        .map(|(_, model)| *model)
        .unwrap_or(CODEX_DEFAULT_MODEL)
}

fn codex_reasoning_effort_for_role(role: &str) -> &'static str {
    match role {
        "orchestrator" | "ai" | "sec" => "xhigh",
        _ => "low",
    }
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "'\"'\"'"))
}

fn team_label(role: &str) -> &str {
    TEAM_LABELS
        .iter()
        .find(|(r, _)| *r == role)
        .map(|(_, l)| *l)
        .unwrap_or(role)
}

fn team_color(role: &str) -> (u8, u8, u8) {
    TEAM_COLORS
        .iter()
        .find(|(r, _)| *r == role)
        .map(|(_, c)| (c[0], c[1], c[2]))
        .unwrap_or((40, 40, 40))
}

fn workspace_session_id(name: &str, number: u32) -> String {
    format!("{}-{}", name, number)
}

/// Find the next available session number for the given project name.
/// Checks existing worktree directories and the SQLite database.
async fn allocate_session_number(
    db_pool: &sqlx::SqlitePool,
    repo: &std::path::Path,
    name: &str,
) -> u32 {
    let worktree_root = repo.join(".claude").join("worktrees");
    let mut max = 0;

    if let Ok(entries) = std::fs::read_dir(&worktree_root) {
        for entry in entries.flatten() {
            if let Ok(fname) = entry.file_name().into_string() {
                // Match directories like "m7-navi-1", "m7-navi-2"
                if let Some(prefix) = fname.strip_prefix(&format!("{}-", name)) {
                    if let Ok(n) = prefix.parse::<u32>() {
                        if n > max {
                            max = n;
                        }
                    }
                }
            }
        }
    }

    // Check database for any allocated slot_number (active or stopped)
    if let Ok(db_max) = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(slot_number), 0) FROM sessions WHERE project_slug = ?",
    )
    .bind(name)
    .fetch_one(db_pool)
    .await
    {
        if db_max > max as i64 {
            max = db_max as u32;
        }
    }

    max + 1
}

/// Path passed to every Kimi pane via `--mcp-config-file`.
fn devorch_mcp_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".config")
        .join("devorch")
        .join("mcp-devorch.json")
}

/// Resolve the lantern binary path for MCP server registration.
/// Prefers the running executable; falls back to ~/.lantern/bin/lantern.
fn resolve_lantern_binary() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if exe.is_file() {
            return exe;
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".lantern")
        .join("bin")
        .join("lantern")
}

fn write_devorch_mcp_config(command: &Path, args: &[&str]) -> Result<PathBuf> {
    let path = devorch_mcp_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::json!({
        "mcpServers": {
            "devorch": {
                "command": command.to_string_lossy(),
                "args": args,
            }
        }
    });
    std::fs::write(&path, serde_json::to_string_pretty(&json)?)?;
    Ok(path)
}

async fn verify_devorch_mcp_stdio(
    command: &Path,
    args: &[&str],
    kimi_spawn_env: bool,
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::time::{timeout, Duration};

    let init = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","#,
        r#""params":{"protocolVersion":"2024-11-05","capabilities":{},"#,
        r#""clientInfo":{"name":"startwork","version":"1"}}}"#
    );

    let mut cmd = tokio::process::Command::new(command);
    cmd.args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if kimi_spawn_env {
        cmd.env_clear();
        if let Some(home) = dirs::home_dir() {
            cmd.env("HOME", home);
        }
        if let Ok(user) = std::env::var("USER") {
            cmd.env("USER", user);
        }
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn {} {:?}", command.display(), args))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(format!("{init}\n").as_bytes())
            .await
            .context("write MCP initialize")?;
    }

    let stdout = child.stdout.take().context("MCP stdout")?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();

    timeout(Duration::from_secs(5), reader.read_line(&mut line))
        .await
        .context("MCP initialize timed out after 5s")?
        .context("read MCP initialize response")?;

    let _ = child.kill().await;

    // Expect the lantern self-contained MCP server name.
    if !line.contains("lantern-relay-mcp") {
        anyhow::bail!(
            "lantern mcp failed health check{}: {}",
            if kimi_spawn_env {
                " (Kimi empty-PATH spawn)"
            } else {
                ""
            },
            line.trim()
        );
    }
    Ok(())
}

fn ensure_kimi_devorch_mcp_registered(command: &Path, args: &[&str]) -> Result<()> {
    let _ = std::process::Command::new("kimi")
        .args(["mcp", "remove", "devorch"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    info!(
        command = %command.display(),
        args = ?args,
        "Registering devorch MCP via kimi mcp add"
    );
    let mut add_cmd = std::process::Command::new("kimi");
    add_cmd.args(["mcp", "add", "--transport", "stdio", "devorch", "--"]);
    add_cmd.arg(command);
    for arg in args {
        add_cmd.arg(arg);
    }
    let add = add_cmd.output().context("kimi mcp add")?;
    if !add.status.success() {
        anyhow::bail!(
            "kimi mcp add devorch failed: {}",
            String::from_utf8_lossy(&add.stderr).trim()
        );
    }

    info!("Verifying devorch MCP via kimi mcp test");
    let test = std::process::Command::new("kimi")
        .args(["mcp", "test", "devorch"])
        .output()
        .context("kimi mcp test")?;
    if !test.status.success() {
        anyhow::bail!(
            "kimi mcp test devorch failed: {}",
            String::from_utf8_lossy(&test.stderr).trim()
        );
    }
    Ok(())
}

/// Write kimi MCP config, verify stdio, and register with kimi CLI.
/// Uses the lantern binary itself (`lantern mcp`) as the self-contained MCP server.
async fn ensure_devorch_mcp_ready() -> Result<()> {
    let lantern = resolve_lantern_binary();
    let mcp_args: &[&str] = &["mcp"];

    let config_path = write_devorch_mcp_config(&lantern, mcp_args)?;
    verify_devorch_mcp_stdio(&lantern, mcp_args, true).await?;
    ensure_kimi_devorch_mcp_registered(&lantern, mcp_args)?;

    info!(
        config = %config_path.display(),
        binary = %lantern.display(),
        "lantern mcp ready for Kimi squads"
    );
    Ok(())
}

fn pane_name_for(name: &str, team: &str, number: u32) -> String {
    format!("{}-{}-{}", name, team, number)
}

async fn create_orchestrator_worktree(
    repo: &Path,
    worktree_root: &Path,
    session_id: &str,
) -> Result<PathBuf> {
    let path = orchestrator_worktree_path(worktree_root, session_id);
    let repo_str = repo.to_string_lossy().to_string();
    create_one_worktree(&repo_str, &path, session_id, "orchestrator").await?;
    Ok(path)
}

/// Create all 8 worker worktrees concurrently — one tokio task per team.
async fn create_worker_worktrees_parallel(
    repo: &Path,
    worktree_root: &Path,
    name: &str,
    number: u32,
) -> Result<Vec<(String, String, PathBuf, String)>> {
    let repo_str = repo.to_string_lossy().to_string();
    let mut set = JoinSet::new();
    for team in GRID_ORDER.iter().skip(1) {
        if *team == "inp" {
            continue;
        }
        let team = team.to_string();
        let repo_str = repo_str.clone();
        let worktree_root = worktree_root.to_path_buf();
        let name = name.to_string();
        set.spawn(async move {
            create_one_worker_worktree(&repo_str, &worktree_root, &name, &team, number).await
        });
    }
    let mut records = Vec::with_capacity(8);
    while let Some(res) = set.join_next().await {
        records.push(res.context("worktree task join")??);
    }
    records.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(records)
}

async fn create_one_worker_worktree(
    repo: &str,
    worktree_root: &Path,
    name: &str,
    team: &str,
    number: u32,
) -> Result<(String, String, PathBuf, String)> {
    let pane_name = pane_name_for(name, team, number);
    let branch = pane_name.clone();
    let path = worktree_root.join(&pane_name);
    create_one_worktree(repo, &path, &branch, team).await?;
    Ok((team.to_string(), pane_name, path, branch))
}

async fn create_one_worktree(repo: &str, path: &Path, branch: &str, team: &str) -> Result<()> {
    let existing = Command::new("git")
        .args([
            "-C",
            repo,
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{}", branch),
        ])
        .output()
        .await?;
    if existing.status.success() {
        anyhow::bail!("branch '{}' already exists", branch);
    }

    info!(path = %path.display(), branch = %branch, team = %team, "Creating worktree (parallel)");
    std::fs::create_dir_all(path)?;

    let status = Command::new("git")
        .args([
            "-C",
            repo,
            "worktree",
            "add",
            "-b",
            branch,
            &path.to_string_lossy(),
            "HEAD",
        ])
        .status()
        .await?;
    if !status.success() {
        anyhow::bail!("git worktree add failed for {}", branch);
    }

    let path_buf = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let _ = ensure_antigravity_project_trusted(&path_buf);
        let _ = ensure_gemini_project_trusted(&path_buf);
    })
    .await?;

    Ok(())
}

/// Sync skills into repo + worktrees on blocking thread pool (parallel).
async fn sync_skills_parallel(roots: &[PathBuf]) {
    let mut set = JoinSet::new();
    for root in roots {
        let root = root.clone();
        set.spawn_blocking(move || {
            if let Err(e) = copy_skills_to_project(&root) {
                tracing::warn!(path = %root.display(), error = %e, "skill sync skipped");
            }
        });
    }
    while set.join_next().await.is_some() {}
}

async fn register_agents_parallel(
    db_pool: &sqlx::SqlitePool,
    session_id: &str,
    project_name: &str,
    slot_number: u32,
    window_defs: &[WindowDef],
    iterm_sessions: &std::collections::HashMap<String, String>,
) -> Result<()> {
    let mut set = JoinSet::new();
    for wdef in window_defs {
        let role = wdef
            .env
            .get("DEVORCH_ROLE")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        if role == "input" {
            continue;
        }
        let is_orch = role == "orchestrator";
        let iterm_key = if is_orch {
            "orchestrator".to_string()
        } else {
            role.clone()
        };
        let iterm_session_id = iterm_sessions.get(&iterm_key).cloned().unwrap_or_default();
        let pool = db_pool.clone();
        let session_id = session_id.to_string();
        let agent_kind = wdef.agent_kind.clone();
        let wdef_name = wdef.name.clone();
        let wdef_dir = wdef.dir.clone();
        let project_name = project_name.to_string();
        let role_for_id = if is_orch {
            "orch".to_string()
        } else {
            role.clone()
        };

        set.spawn(async move {
            let agent = Agent {
                id: generate_id(&format!(
                    "agent-{}-{}-{}",
                    project_name, role_for_id, slot_number
                )),
                session_id: session_id.clone(),
                role: role.clone(),
                pane_id: Some(iterm_session_id.clone()),
                worktree_path: wdef_dir.clone(),
                branch: wdef_name.clone(),
                agent_kind,
                status: "idle".to_string(),
                last_seen_at: Some(chrono::Utc::now()),
                created_at: chrono::Utc::now(),
            };
            queries::insert_agent(&pool, &agent).await?;
            queries::insert_terminal_target(
                &pool,
                &TerminalTarget {
                    agent_id: agent.id.clone(),
                    iterm_session_id: iterm_session_id.clone(),
                    pane_id: Some(iterm_session_id),
                    transport_status: "ready".to_string(),
                    last_seen_at: Some(chrono::Utc::now()),
                },
            )
            .await?;
            Ok::<(), anyhow::Error>(())
        });
    }
    while let Some(res) = set.join_next().await {
        res.context("agent registration task join")??;
    }
    Ok(())
}

async fn surface_codex_launch_errors(
    db_pool: &sqlx::SqlitePool,
    session_id: &str,
    window_defs: &[WindowDef],
    iterm_sessions: &std::collections::HashMap<String, String>,
) -> Result<()> {
    let codex_roles: Vec<(&str, &str)> = window_defs
        .iter()
        .filter(|w| w.agent_kind.eq_ignore_ascii_case("codex"))
        .filter_map(|w| {
            let role = w.env.get("DEVORCH_ROLE")?.as_str();
            let iterm_key = iterm_role_key(role);
            let pane_id = iterm_sessions.get(iterm_key)?.as_str();
            Some((role, pane_id))
        })
        .collect();

    if codex_roles.is_empty() {
        return Ok(());
    }

    sleep(CODEX_LAUNCH_ERROR_WINDOW).await;
    let agents = queries::get_agents_for_session(db_pool, session_id).await?;

    for (role, pane_id) in codex_roles {
        let text = match crate::terminal::iterm::capture_text(pane_id).await {
            Ok(text) => text,
            Err(e) => {
                warn!(role = %role, pane_id = %pane_id, error = %e, "Failed to inspect Codex launch buffer");
                continue;
            }
        };

        if !is_codex_invalid_request_error(&text) {
            continue;
        }

        let Some(agent) = agents.iter().find(|agent| agent.role == role) else {
            warn!(role = %role, pane_id = %pane_id, "Codex launch error found before agent lookup");
            continue;
        };

        queries::update_agent_status(db_pool, &agent.id, "failed").await?;
        let payload = serde_json::json!({
            "role": role,
            "pane_id": pane_id,
            "agent": "codex",
            "model": codex_model_for_role(role),
            "reasoning_effort": codex_reasoning_effort_for_role(role),
            "error": first_matching_error_line(&text).unwrap_or("400 invalid_request_error"),
        });
        let payload = serde_json::to_string(&payload)?;
        queries::log_event(
            db_pool,
            session_id,
            Some(&agent.id),
            "agent_launch_error",
            Some(&payload),
        )
        .await?;

        warn!(
            role = %role,
            agent_id = %agent.id,
            model = %codex_model_for_role(role),
            "Codex launch failed with invalid_request_error"
        );
    }

    Ok(())
}

fn is_codex_invalid_request_error(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("400") && lower.contains("invalid_request_error")
}

fn first_matching_error_line(text: &str) -> Option<&str> {
    text.lines()
        .find(|line| is_codex_invalid_request_error(line))
        .map(str::trim)
        .filter(|line| !line.is_empty())
}

/// Find the git repository root by walking up from the current directory.
fn find_git_repo() -> Result<PathBuf> {
    let mut cwd = std::env::current_dir()?;
    loop {
        if cwd.join(".git").is_dir() {
            return Ok(cwd);
        }
        if !cwd.pop() {
            break;
        }
    }
    anyhow::bail!("not inside a git repository")
}

/// Pane title shown on iTerm split dividers: `ORCH - m7-navi-52`, `AI - m7-navi-ai-52`, …
fn pane_display_title(team_label: &str, worktree: &str) -> String {
    format!("{team_label} - {worktree}")
}

fn iterm_role_key(devorch_role: &str) -> &str {
    if devorch_role == "orchestrator" {
        "orchestrator"
    } else {
        devorch_role
    }
}

fn build_init_by_role(
    window_defs: &[WindowDef],
    no_init: bool,
) -> std::collections::HashMap<String, String> {
    if no_init {
        return std::collections::HashMap::new();
    }
    let mut map = std::collections::HashMap::new();
    for wdef in window_defs {
        // Kimi and headed Goose sessions need post-launch injection; every other
        // agent CLI takes its init prompt inline on the command line (see
        // build_agent_command).
        if !wdef.agent_kind.eq_ignore_ascii_case("kimi")
            && !wdef.agent_kind.eq_ignore_ascii_case("goose")
        {
            continue;
        }
        let role = wdef
            .env
            .get("DEVORCH_ROLE")
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        let session = wdef
            .env
            .get("DEVORCH_SESSION")
            .map(|s| s.as_str())
            .unwrap_or("");
        let project_slug = wdef
            .env
            .get("DEVORCH_PROJECT_SLUG")
            .map(|s| s.as_str())
            .unwrap_or("");
        let temporal_namespace = wdef
            .env
            .get("DEVORCH_TEMPORAL_NAMESPACE")
            .map(|s| s.as_str())
            .unwrap_or("");
        map.insert(
            iterm_role_key(role).to_string(),
            format!(
                "Call the devorch_get_setup_instructions MCP tool now. session={session} role={role} agent={} repo_id={project_slug} temporal_namespace={temporal_namespace}",
                wdef.agent_kind
            ),
        );
    }
    map
}

fn build_titles_by_role(window_defs: &[WindowDef]) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for wdef in window_defs {
        let role = wdef
            .env
            .get("DEVORCH_ROLE")
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        map.insert(
            iterm_role_key(role).to_string(),
            pane_display_title(&wdef.label, &wdef.name),
        );
    }
    map
}

/// Build shell startup lines keyed by iTerm role (orchestrator, ai, …).
fn build_startup_commands(
    window_defs: &[WindowDef],
    session_id: &str,
) -> std::collections::HashMap<String, String> {
    // Optionally source the user's env file. Wrapped in a group with a trailing
    // `true` so a missing (or non-matching) file never returns non-zero — this
    // segment is part of a single `&&` startup chain, and without the guard a
    // missing config/env short-circuits the chain and the agent CLI never launches.
    let env_src =
        r#"{ [ -f "$HOME/.lantern/config/env" ] && source "$HOME/.lantern/config/env"; true; }"#;
    let corepack_bootstrap = "export COREPACK_ENABLE_DOWNLOAD_PROMPT=0";
    let local_bin = dirs::home_dir().unwrap().join(".local").join("bin");
    let ambient_path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string());
    let path_export = format!(
        "export PATH={}:{}:{}",
        shell_escape(&local_bin.to_string_lossy()),
        shell_escape("/opt/homebrew/bin"),
        shell_escape(&ambient_path)
    );

    let mut map = std::collections::HashMap::new();
    for (i, wdef) in window_defs.iter().enumerate() {
        let role = wdef
            .env
            .get("DEVORCH_ROLE")
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        let iterm_key = iterm_role_key(role).to_string();

        let env_exports: Vec<String> = wdef
            .env
            .iter()
            .map(|(k, v)| format!("export {}={}", k, shell_escape(v)))
            .collect();
        let env_line = format!("{} && {}", path_export, env_exports.join(" && "));

        let (r, g, b) = wdef.color;
        let pane_title = pane_display_title(&wdef.label, &wdef.name);
        let banner = format!(
            "printf '\\033]0;{}\\007\\033]1;{}\\007\\033]2;{}\\007\\033[1;37m\\033[48;2;{};{};{}m {} {} \\033[0m\\n'",
            pane_title.replace('\\', "\\\\").replace('\'', "'\\''"),
            pane_title.replace('\\', "\\\\").replace('\'', "'\\''"),
            pane_title.replace('\\', "\\\\").replace('\'', "'\\''"),
            r,
            g,
            b,
            wdef.label,
            wdef.name
        );

        // Every pane runs its agent CLI (or the input router) DIRECTLY — no
        // agent-runner --spawn wrapper, no tmux session. SQLite is the source of
        // truth and delivery injects straight into the iTerm2 pane, so the pane
        // only needs to host the live agent process.
        let startup = format!(
            "{} && {} && {} && cd {} && {} && {}",
            corepack_bootstrap,
            env_src,
            env_line,
            shell_escape(&wdef.dir),
            banner,
            wdef.cmd
        );

        let script_path = format!("/tmp/devorch-startup-{}-{}.sh", session_id, i);
        if std::fs::write(&script_path, format!("{}\n", startup)).is_ok() {
            let startup_cmd = format!(
                "source {} && rm {}",
                shell_escape(&script_path),
                shell_escape(&script_path)
            );
            map.insert(iterm_key, startup_cmd);
        }
    }
    map
}

/// Inject post-launch init prompts directly into the relevant panes via
/// `iterm_batch_init.py` (fast-retry until each pane's CLI is ready). Used for
/// Kimi, whose interactive CLI cannot accept the init prompt as a launch arg.
/// Best-effort: a failure here is logged, not fatal.
async fn run_batch_init(
    session_id: &str,
    init_by_role: &std::collections::HashMap<String, String>,
    iterm_sessions: &std::collections::HashMap<String, String>,
    titles_by_role: &std::collections::HashMap<String, String>,
) {
    let script_path = match crate::terminal::locate_script("iterm_batch_init.py") {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "iterm_batch_init.py not found; skipping init injection");
            return;
        }
    };

    let init_file = format!("/tmp/devorch-init-{}.json", session_id);
    let sessions_file = format!("/tmp/devorch-init-sessions-{}.json", session_id);
    let titles_file = format!("/tmp/devorch-init-titles-{}.json", session_id);

    let write_all = (|| -> Result<()> {
        std::fs::write(&init_file, serde_json::to_string(init_by_role)?)?;
        std::fs::write(&sessions_file, serde_json::to_string(iterm_sessions)?)?;
        std::fs::write(&titles_file, serde_json::to_string(titles_by_role)?)?;
        Ok(())
    })();
    if let Err(e) = write_all {
        tracing::warn!(error = %e, "failed to stage init injection files");
        return;
    }

    let script_str = match script_path.to_str() {
        Some(s) => s,
        None => {
            tracing::warn!("non-UTF-8 iterm_batch_init.py path; skipping init injection");
            return;
        }
    };
    let result = Command::new("python3")
        .args([
            script_str,
            "--init-file",
            &init_file,
            "--sessions-file",
            &sessions_file,
            "--titles-file",
            &titles_file,
        ])
        .output()
        .await;

    let _ = std::fs::remove_file(&init_file);
    let _ = std::fs::remove_file(&sessions_file);
    let _ = std::fs::remove_file(&titles_file);

    match result {
        Ok(output) if output.status.success() => {
            info!(
                count = init_by_role.len(),
                "Injected post-launch init prompts"
            );
        }
        Ok(output) => {
            tracing::warn!(
                stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                "iterm_batch_init.py reported errors"
            );
        }
        Err(e) => tracing::warn!(error = %e, "failed to run iterm_batch_init.py"),
    }
}

/// Payload written to the `--startup-file` JSON: per-role shell commands plus
/// the `LayoutSpec` describing the pane grid `iterm_launch.py` should build.
#[derive(serde::Serialize)]
struct ItermStartupPayload<'a> {
    commands: &'a std::collections::HashMap<String, String>,
    layout: &'a patterns::LayoutSpec,
}

// Create the squad layout in a new iTerm2 window using the Python API.
// Calls `src/startwork/iterm_launch.py` which opens the window, injects startup
// commands into each pane on the same connection, and returns session IDs.
async fn create_iterm_layout(
    session_id: &str,
    titles_by_role: &std::collections::HashMap<String, String>,
    startup_by_role: &std::collections::HashMap<String, String>,
    layout: &patterns::LayoutSpec,
    expected_panes: usize,
) -> Result<std::collections::HashMap<String, String>> {
    let script_path = crate::terminal::locate_script("iterm_launch.py")?;

    let startup_file = format!("/tmp/devorch-iterm-startup-{}.json", session_id);
    let titles_file = format!("/tmp/devorch-iterm-titles-{}.json", session_id);
    let startup_payload = ItermStartupPayload {
        commands: startup_by_role,
        layout,
    };
    std::fs::write(
        &startup_file,
        serde_json::to_string(&startup_payload).context("serialize startup commands and layout")?,
    )?;
    std::fs::write(
        &titles_file,
        serde_json::to_string(titles_by_role).context("serialize pane titles")?,
    )?;

    let cmd_args = [
        script_path.to_str().context("non-UTF-8 script path")?,
        "--session",
        session_id,
        "--titles-file",
        &titles_file,
        "--startup-file",
        &startup_file,
    ];

    let output = Command::new("python3")
        .args(cmd_args)
        .output()
        .await
        .context("failed to launch iterm_launch.py")?;

    let _ = std::fs::remove_file(&startup_file);
    let _ = std::fs::remove_file(&titles_file);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("iterm_launch.py failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let map: std::collections::HashMap<String, String> = serde_json::from_str(stdout.trim())
        .with_context(|| format!("iterm_launch.py returned invalid JSON: {}", stdout.trim()))?;

    // Verify we got exactly the number of sessions this pattern's layout expects.
    if map.len() != expected_panes {
        anyhow::bail!(
            "iterm_launch.py returned {} sessions (expected {}): {:?}",
            map.len(),
            expected_panes,
            map
        );
    }

    Ok(map)
}

/// Create a pane layout for a non-team `LaunchPattern` (Simple Orchestrator
/// today; Executor/FixABug can reuse this too) from its `LayoutSpec`. Unlike
/// `create_iterm_layout` (fixed 9-pane `GRID_ORDER` grid), this calls
/// `iterm_launch_pattern.py`, which builds columns/rows generically off the
/// `LayoutSpec` JSON instead of a hardcoded split tree.
async fn create_iterm_layout_for_pattern(
    session_id: &str,
    layout: &patterns::LayoutSpec,
    titles_by_role: &std::collections::HashMap<String, String>,
    startup_by_role: &std::collections::HashMap<String, String>,
) -> Result<std::collections::HashMap<String, String>> {
    let script_path = crate::terminal::locate_script("iterm_launch_pattern.py")?;

    let layout_file = format!("/tmp/devorch-iterm-layout-{}.json", session_id);
    let startup_file = format!("/tmp/devorch-iterm-startup-{}.json", session_id);
    let titles_file = format!("/tmp/devorch-iterm-titles-{}.json", session_id);
    std::fs::write(
        &layout_file,
        serde_json::to_string(layout).context("serialize layout spec")?,
    )?;
    std::fs::write(
        &startup_file,
        serde_json::to_string(startup_by_role).context("serialize startup commands")?,
    )?;
    std::fs::write(
        &titles_file,
        serde_json::to_string(titles_by_role).context("serialize pane titles")?,
    )?;

    let cmd_args = [
        script_path.to_str().context("non-UTF-8 script path")?,
        "--session",
        session_id,
        "--layout-file",
        &layout_file,
        "--titles-file",
        &titles_file,
        "--startup-file",
        &startup_file,
    ];

    let output = Command::new("python3")
        .args(cmd_args)
        .output()
        .await
        .context("failed to launch iterm_launch_pattern.py")?;

    let _ = std::fs::remove_file(&layout_file);
    let _ = std::fs::remove_file(&startup_file);
    let _ = std::fs::remove_file(&titles_file);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("iterm_launch_pattern.py failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let map: std::collections::HashMap<String, String> = serde_json::from_str(stdout.trim())
        .with_context(|| {
            format!(
                "iterm_launch_pattern.py returned invalid JSON: {}",
                stdout.trim()
            )
        })?;

    Ok(map)
}

/// Register the path as trusted for Antigravity CLI.
fn ensure_antigravity_project_trusted(repo_path: &std::path::Path) -> Result<()> {
    let home = dirs::home_dir().context("could not find home directory")?;
    let gemini_projects_dir = home.join(".gemini").join("config").join("projects");
    let antigravity_projects_dir = home.join(".antigravitycli").join("config").join("projects");
    let antigravity_legacy_dir = home.join(".antigravitycli");
    let antigravity_cli_projects_dir = home
        .join(".gemini")
        .join("antigravity-cli")
        .join("config")
        .join("projects");
    let antigravity_cli_legacy_dir = home.join(".gemini").join("antigravity-cli");

    std::fs::create_dir_all(&gemini_projects_dir)?;
    std::fs::create_dir_all(&antigravity_projects_dir)?;
    std::fs::create_dir_all(&antigravity_legacy_dir)?;
    std::fs::create_dir_all(&antigravity_cli_projects_dir)?;
    std::fs::create_dir_all(&antigravity_cli_legacy_dir)?;

    let resolved_path = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf());
    let target_uri = format!("file://{}", resolved_path.to_string_lossy());

    // 1. Check if there's already a config matching this folderUri in config dirs
    let mut existing_filename: Option<String> = None;
    let mut found_target_file: Option<std::path::PathBuf> = None;

    for dir in &[
        &gemini_projects_dir,
        &antigravity_projects_dir,
        &antigravity_cli_projects_dir,
    ] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                            let resources = val
                                .get("projectResources")
                                .and_then(|pr| pr.get("resources"))
                                .and_then(|r| r.as_array());
                            if let Some(arr) = resources {
                                for item in arr {
                                    let folder_uri = item
                                        .get("gitFolder")
                                        .and_then(|gf| gf.get("folderUri"))
                                        .and_then(|fu| fu.as_str());
                                    if folder_uri == Some(&target_uri) {
                                        // Found a matching config file
                                        if let Some(filename) =
                                            path.file_name().and_then(|f| f.to_str())
                                        {
                                            existing_filename = Some(filename.to_string());
                                            found_target_file = Some(path.clone());
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if existing_filename.is_some() {
                    break;
                }
            }
        }
        if existing_filename.is_some() {
            break;
        }
    }

    let (filename, actual_file_path) =
        if let (Some(fname), Some(fpath)) = (existing_filename, found_target_file) {
            (fname, fpath)
        } else {
            // 2. If not found, generate a new UUID and trust record in the gemini projects directory
            let proj_uuid = uuid::Uuid::new_v4().to_string();
            let fname = format!("{}.json", proj_uuid);
            let fpath = gemini_projects_dir.join(&fname);

            let trust_data = serde_json::json!({
                "id": proj_uuid,
                "name": resolved_path.to_string_lossy(),
                "projectResources": {
                    "resources": [
                        {
                            "gitFolder": {
                                "folderUri": target_uri,
                                "allowWrite": true
                            }
                        }
                    ]
                }
            });

            std::fs::write(&fpath, serde_json::to_string_pretty(&trust_data)?)?;
            info!(
                "startwork: marked {} trusted in {}",
                resolved_path.display(),
                fpath.display()
            );
            (fname, fpath)
        };

    // 3. Ensure the symlinks exist in all config and legacy dirs
    let symlink_in_projects = antigravity_projects_dir.join(&filename);
    let symlink_in_legacy = antigravity_legacy_dir.join(&filename);
    let symlink_in_cli_projects = antigravity_cli_projects_dir.join(&filename);
    let symlink_in_cli_legacy = antigravity_cli_legacy_dir.join(&filename);

    for symlink_path in &[
        symlink_in_projects,
        symlink_in_legacy,
        symlink_in_cli_projects,
        symlink_in_cli_legacy,
    ] {
        if symlink_path.exists() {
            let _ = std::fs::remove_file(symlink_path);
        }
        #[cfg(unix)]
        {
            let _ = std::os::unix::fs::symlink(&actual_file_path, symlink_path);
        }
    }

    let _ = ensure_antigravity_settings_trusted(repo_path);

    Ok(())
}

fn ensure_antigravity_settings_trusted(repo_path: &std::path::Path) -> Result<()> {
    let home = dirs::home_dir().context("could not find home directory")?;
    let settings_file = home
        .join(".gemini")
        .join("antigravity-cli")
        .join("settings.json");

    let resolved_path = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf());
    let resolved_str = resolved_path.to_string_lossy().to_string();

    let mut val = if settings_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&settings_file) {
            serde_json::from_str::<serde_json::Value>(&content)
                .unwrap_or_else(|_| serde_json::json!({}))
        } else {
            serde_json::json!({})
        }
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = val.as_object_mut() {
        // 1. Ensure folderTrust is disabled globally
        let security = obj
            .entry("security")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if let Some(sec_obj) = security.as_object_mut() {
            let folder_trust = sec_obj
                .entry("folderTrust")
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            if let Some(ft_obj) = folder_trust.as_object_mut() {
                ft_obj.insert("enabled".to_string(), serde_json::Value::Bool(false));
            }
        }

        // 2. Add repo path to trustedWorkspaces
        let trusted = obj
            .entry("trustedWorkspaces")
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));

        if let Some(arr) = trusted.as_array_mut() {
            let mut found = false;
            for item in arr.iter() {
                if item.as_str() == Some(&resolved_str) {
                    found = true;
                    break;
                }
            }
            if !found {
                arr.push(serde_json::Value::String(resolved_str.clone()));
            }
        }

        let _ = std::fs::write(&settings_file, serde_json::to_string_pretty(&val)?);
        info!(
            "startwork: trusted {} in antigravity settings.json",
            resolved_str
        );
    }

    Ok(())
}

/// Register the path in trustedFolders.json and projects.json for legacy systems.
fn ensure_gemini_project_trusted(repo_path: &std::path::Path) -> Result<()> {
    let home = dirs::home_dir().context("could not find home directory")?;
    let resolved_path = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf());
    let resolved_str = resolved_path.to_string_lossy().to_string();

    let dirs = &[
        home.join(".gemini"),
        home.join(".antigravitycli"),
        home.join(".gemini").join("antigravity-cli"),
    ];

    for config_dir in dirs {
        std::fs::create_dir_all(config_dir)?;

        // 1. Update trustedFolders.json
        let trusted_folders_file = config_dir.join("trustedFolders.json");
        let mut trusted_folders = serde_json::Map::new();
        if trusted_folders_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&trusted_folders_file) {
                if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(&content) {
                    trusted_folders = map;
                }
            }
        }

        if trusted_folders.get(&resolved_str).and_then(|v| v.as_str()) != Some("TRUST_FOLDER") {
            trusted_folders.insert(
                resolved_str.clone(),
                serde_json::Value::String("TRUST_FOLDER".to_string()),
            );
            let _ = std::fs::write(
                &trusted_folders_file,
                serde_json::to_string_pretty(&serde_json::Value::Object(trusted_folders))?,
            );
            info!(
                "startwork: marked {} as TRUST_FOLDER in {}",
                resolved_str,
                trusted_folders_file.display()
            );
        }

        // 2. Update projects.json
        let projects_file = config_dir.join("projects.json");
        let mut projects_data = serde_json::json!({ "projects": {} });
        if projects_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&projects_file) {
                if let Ok(val) = serde_json::from_str(&content) {
                    projects_data = val;
                }
            }
        }

        if projects_data.get("projects").is_none() {
            projects_data["projects"] = serde_json::json!({});
        }

        if projects_data["projects"].get(&resolved_str).is_none() {
            let name = resolved_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project");
            projects_data["projects"][&resolved_str] = serde_json::json!(name);
            let _ = std::fs::write(
                &projects_file,
                serde_json::to_string_pretty(&projects_data)?,
            );
            info!(
                "startwork: mapped {} -> {} in {}",
                resolved_str,
                name,
                projects_file.display()
            );
        }
    }

    Ok(())
}

/// Stop any leftover Toad (`kimi term`) processes before launching Kimi Code CLI panes.
fn kill_toad_processes() {
    for pattern in ["-m toad.cli", "kimi term", "kimi-cli.*term"] {
        let _ = std::process::Command::new("pkill")
            .args(["-f", pattern])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// Start Temporal and Relay if the installer left a service helper in place.
fn ensure_squad_services() {
    let script = dirs::home_dir().and_then(|h| {
        let bin = h.join(".lantern").join("bin");
        let current = bin.join("lantern-up");
        if current.exists() {
            return Some(current);
        }
        let legacy = bin.join("lantern-up.sh");
        legacy.exists().then_some(legacy)
    });
    let Some(script) = script else {
        return;
    };
    let _ = std::process::Command::new("bash")
        .arg(&script)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Register devorch MCP for agent CLIs using the self-contained `lantern mcp` server.
fn ensure_mcp_server_registered(agent_kind: &str) {
    let lantern = resolve_lantern_binary();
    let lantern_str = lantern.to_string_lossy().to_string();

    match agent_kind.to_lowercase().as_str() {
        "claude" => {
            let list = std::process::Command::new("claude")
                .args(["mcp", "list"])
                .output();
            let already_registered = list
                .map(|o| String::from_utf8_lossy(&o.stdout).contains("devorch"))
                .unwrap_or(false);
            if already_registered {
                return;
            }
            info!("Registering devorch MCP server for claude (lantern mcp)");
            let _ = std::process::Command::new("claude")
                .args([
                    "mcp",
                    "add",
                    "-s",
                    "user",
                    "devorch",
                    &lantern_str,
                    "--",
                    "mcp",
                ])
                .status();
        }
        "codex" => {
            if let Some(home) = dirs::home_dir() {
                let config_path = home.join(".codex").join("config.toml");
                let _ = ensure_devorch_in_codex_mcp_config(&config_path, &lantern);
            }
        }
        "kimi" => {
            // Handled by ensure_devorch_mcp_ready() before pane launch.
        }
        "goose" => {
            // devorch is passed inline via `goose session --with-extension`;
            // no global MCP registration needed.
        }
        "gemini" | "agy" | "agi" => {
            let home = match dirs::home_dir() {
                Some(h) => h,
                None => return,
            };
            let config_paths = [
                home.join(".gemini").join("config").join("mcp_config.json"),
                home.join(".gemini")
                    .join("antigravity")
                    .join("mcp_config.json"),
            ];
            for path in config_paths {
                let _ = ensure_devorch_in_gemini_mcp_config(&path, &lantern);
            }
        }
        _ => {}
    }
}

fn ensure_devorch_in_gemini_mcp_config(
    path: &std::path::Path,
    lantern: &std::path::Path,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut data: serde_json::Value = if path.exists() && path.metadata()?.len() > 0 {
        serde_json::from_str(&std::fs::read_to_string(path)?).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    let servers = data.as_object_mut().and_then(|o| {
        if !o.contains_key("mcpServers") {
            o.insert("mcpServers".to_string(), serde_json::json!({}));
        }
        o.get_mut("mcpServers").and_then(|v| v.as_object_mut())
    });
    if let Some(servers) = servers {
        if !servers.contains_key("devorch") {
            servers.insert(
                "devorch".to_string(),
                serde_json::json!({
                    "command": lantern.to_string_lossy(),
                    "args": ["mcp"],
                }),
            );
            std::fs::write(path, serde_json::to_string_pretty(&data)?)?;
            info!("Registered devorch MCP (lantern mcp) in {}", path.display());
        }
    }
    Ok(())
}

fn ensure_devorch_in_codex_mcp_config(
    path: &std::path::Path,
    lantern: &std::path::Path,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Read existing TOML if present.
    let existing = if path.exists() {
        std::fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };

    // Skip if already registered.
    if existing.contains("[mcp_servers.devorch]") {
        return Ok(());
    }

    // Append the devorch stanza (TOML format expected by codex).
    let stanza = format!(
        "\n[mcp_servers.devorch]\ncommand = {:?}\nargs = [\"mcp\"]\n",
        lantern.to_string_lossy().as_ref()
    );
    let mut content = existing;
    content.push_str(&stanza);
    std::fs::write(path, &content)?;
    info!("Registered devorch MCP (lantern mcp) in {}", path.display());
    Ok(())
}

/// Copies valid global skill profiles (folders containing SKILL.md/skill.md)
/// from ~/.claude/skills/ into project .claude, .kimi, and .gemini skills dirs.
fn copy_skills_to_project(repo_path: &std::path::Path) -> Result<()> {
    let home = dirs::home_dir().context("could not find home directory")?;
    let global_skills_dir = home.join(".claude").join("skills");

    if !global_skills_dir.exists() {
        return Ok(());
    }

    let mut copied = 0usize;
    for root_name in &[".claude", ".kimi", ".gemini"] {
        let local_skills_dir = repo_path.join(root_name).join("skills");
        if let Ok(entries) = std::fs::read_dir(&global_skills_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let folder_name = path.file_name().context("no folder name")?;
                    // Only copy if the folder represents a valid skill profile
                    let has_skill_md =
                        path.join("SKILL.md").exists() || path.join("skill.md").exists();
                    if has_skill_md {
                        let dest_folder = local_skills_dir.join(folder_name);
                        let _ = std::fs::create_dir_all(&dest_folder);
                        if let Ok(sub_entries) = std::fs::read_dir(&path) {
                            for sub_entry in sub_entries.flatten() {
                                let sub_path = sub_entry.path();
                                if sub_path.is_file() {
                                    let file_name = sub_path.file_name().context("no file name")?;
                                    let dest_file = dest_folder.join(file_name);
                                    if std::fs::copy(&sub_path, &dest_file).is_ok() {
                                        copied += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if copied > 0 {
        info!(
            path = %repo_path.display(),
            files = copied,
            "Synced global skills into project"
        );
    }

    Ok(())
}

async fn trust_workspaces(skill_roots: &[PathBuf]) {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return,
    };
    let trusted_file = home
        .join(".gemini")
        .join("antigravity-cli")
        .join("trustedFolders.json");
    let mut trusted: serde_json::Value = match tokio::fs::read_to_string(&trusted_file).await {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({})),
        Err(_) => serde_json::json!({}),
    };

    if let Some(obj) = trusted.as_object_mut() {
        for root in skill_roots {
            if let Some(path_str) = root.to_str() {
                obj.insert(path_str.to_string(), serde_json::json!("TRUST_FOLDER"));
            }
        }
    }

    if let Ok(s) = serde_json::to_string_pretty(&trusted) {
        let _ = tokio::fs::write(&trusted_file, s).await;
    }
}

#[cfg(test)]
mod parse_tests {
    use super::parse_startwork_args;

    #[test]
    fn agent_only_kimi() {
        let (name, number, agent) = parse_startwork_args(vec!["kimi".into()], None);
        assert_eq!(name, None);
        assert_eq!(number, None);
        assert_eq!(agent.as_deref(), Some("kimi"));
    }

    #[test]
    fn name_number_agent() {
        let (name, number, agent) =
            parse_startwork_args(vec!["m7-navi".into(), "40".into(), "claude".into()], None);
        assert_eq!(name.as_deref(), Some("m7-navi"));
        assert_eq!(number, Some(40));
        assert_eq!(agent.as_deref(), Some("claude"));
    }

    #[test]
    fn agent_flag_overrides_trailing() {
        let (name, number, agent) = parse_startwork_args(vec!["kimi".into()], Some("codex".into()));
        // --agent codex wins; "kimi" is not a known agent token when flag is set → project name
        assert_eq!(name.as_deref(), Some("kimi"));
        assert_eq!(number, None);
        assert_eq!(agent.as_deref(), Some("codex"));
    }

    #[test]
    fn agi_maps_to_agy() {
        let (_, _, agent) = parse_startwork_args(vec!["agi".into()], None);
        assert_eq!(agent.as_deref(), Some("agy"));
    }

    #[test]
    fn kimi_command_uses_current_cli_flags() {
        let cmd = super::build_agent_command("kimi", "ai", Some("init prompt"), None);
        assert!(!cmd.contains("dangerously-skip-permissions"));
        assert!(!cmd.contains("prompt-interactive"));
        assert!(cmd.contains("command env"));
        assert!(cmd.contains(" -m "));
        assert!(cmd.contains(" --mcp-config-file "));
        assert!(cmd.contains("mcp-devorch.json"));
        assert!(cmd.contains(" -y"));
        assert!(!cmd.contains(" -p "));
        assert!(!cmd.contains(" term"));
        assert!(!cmd.contains("toad"));
        assert!(cmd.contains("kimi-code/kimi-for-coding"));
    }

    #[test]
    fn goose_command_is_headed_acp_session() {
        // Heavyweight role -> claude-acp + opus, watchable `goose session` in a
        // pane, devorch wired via --with-extension, init injected post-launch.
        let cmd = super::build_agent_command("goose", "ai", Some("init prompt"), Some("pane-1"));
        assert!(
            cmd.contains("goose session"),
            "must be a headed session: {cmd}"
        );
        assert!(
            !cmd.contains("goose run"),
            "must not be the headless one-shot"
        );
        assert!(cmd.contains("GOOSE_PROVIDER=claude-acp"));
        assert!(cmd.contains("GOOSE_MODEL=opus"));
        assert!(cmd.contains("GOOSE_DISABLE_KEYRING=1"));
        assert!(cmd.contains("--with-extension"));
        assert!(cmd.contains("mcp"), "devorch extension wired: {cmd}");
        assert!(cmd.contains("--name 'pane-1'"));
        // init is injected post-launch (build_init_by_role), not inline.
        assert!(
            !cmd.contains("init prompt"),
            "init must not be inline for goose"
        );
    }

    #[test]
    fn goose_model_is_role_based() {
        assert_eq!(super::get_model_for_role("goose", "ai"), "opus");
        assert_eq!(super::get_model_for_role("goose", "doc"), "haiku");
        assert_eq!(super::get_model_for_role("goose", "qa"), "sonnet");
    }

    #[test]
    fn codex_model_mapping_is_role_based() {
        let cases = [
            ("orchestrator", "gpt-5.5"),
            ("ai", "gpt-5.5"),
            ("sec", "gpt-5.5"),
            ("dat", "gpt-5.4-mini"),
            ("ops", "gpt-5.4-mini"),
            ("plt", "gpt-5.4-mini"),
            ("ui", "gpt-5.4-mini"),
            ("doc", "gpt-5.4-mini"),
            ("qa", "gpt-5.4-mini"),
            ("unknown", "gpt-5.4-mini"),
        ];

        for (role, expected) in cases {
            assert_eq!(super::codex_model_for_role(role), expected, "role={role}");
        }
    }

    #[test]
    fn window_defs_include_hard_runtime_identity_env() {
        let repo = std::path::PathBuf::from("/tmp/devorch-repo");
        let worktree_root = repo.join(".claude").join("worktrees").join("demo-7");
        let orch_worktree = super::orchestrator_worktree_path(&worktree_root, "demo-7");
        let runtime_identity =
            super::RuntimeIdentityEnv::new(&repo, "repo-demo", "lantern", "queue-demo");

        let defs = super::build_window_defs(
            &repo,
            &worktree_root,
            &orch_worktree,
            Some("claude"),
            true,
            "demo",
            7,
            "demo-7",
            "run-1",
            &runtime_identity,
            "team",
        );

        assert_eq!(defs.len(), super::GRID_ORDER.len());
        for wdef in defs {
            assert_eq!(
                wdef.env.get("DEVORCH_REPO_ID").map(String::as_str),
                Some("repo-demo")
            );
            assert_eq!(
                wdef.env.get("DEVORCH_REPO_ROOT").map(String::as_str),
                Some("/tmp/devorch-repo")
            );
            assert_eq!(
                wdef.env
                    .get("DEVORCH_TEMPORAL_NAMESPACE")
                    .map(String::as_str),
                Some("lantern")
            );
            assert_eq!(
                wdef.env.get("DEVORCH_TASK_QUEUE").map(String::as_str),
                Some("queue-demo")
            );
        }
    }

    #[test]
    fn window_defs_put_orchestrator_in_session_worktree() {
        let repo = std::path::PathBuf::from("/tmp/devorch-repo");
        let worktree_root = repo.join(".claude").join("worktrees").join("demo-7");
        let orch_worktree = super::orchestrator_worktree_path(&worktree_root, "demo-7");
        let runtime_identity = super::RuntimeIdentityEnv::new(
            &repo,
            "repo-demo",
            "default",
            super::DEVORCH_DEFAULT_TASK_QUEUE,
        );

        let defs = super::build_window_defs(
            &repo,
            &worktree_root,
            &orch_worktree,
            Some("claude"),
            true,
            "demo",
            7,
            "demo-7",
            "run-1",
            &runtime_identity,
            "team",
        );

        let orch = defs
            .iter()
            .find(|wdef| wdef.env.get("DEVORCH_ROLE").map(String::as_str) == Some("orchestrator"))
            .expect("orchestrator window");
        assert_eq!(orch.name, "demo-7");
        assert_eq!(orch.dir, orch_worktree.to_string_lossy());
        assert_ne!(orch.dir, repo.to_string_lossy());

        let ai = defs
            .iter()
            .find(|wdef| wdef.env.get("DEVORCH_ROLE").map(String::as_str) == Some("ai"))
            .expect("ai window");
        assert_eq!(ai.dir, worktree_root.join("demo-ai-7").to_string_lossy());
    }
}
