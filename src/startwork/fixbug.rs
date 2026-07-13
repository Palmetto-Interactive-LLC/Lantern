//! `--pattern fixbug` launch: one fixer worktree pane + an input router pane.
//!
//! Resolves the `--issue` reference (a Linear issue identifier, plain number,
//! or GitHub issue URL) to the context available locally, then opens a fixer
//! worktree named `<project>-fix-<issue>-<slot>` and
//! injects the kickoff prompt from `crate::prompts::fixbug` directly into
//! the fixer's launch command.
//!
//! Deviation from the team pattern's single split-pane iTerm2 window:
//! `iterm_launch.py` is hardcoded to the 9-role team grid layout, so this
//! opens two separate iTerm2 windows (fixer, input) via the existing
//! `create_solo_window` helper rather than a single 100%-width two-row
//! layout. `PatternConfig::fixbug`'s `LayoutSpec` (weight 100, rows
//! `["fixer", "inp"]`) is therefore descriptive metadata only until a
//! generic layout-driven launcher exists.

use anyhow::Result;
use tokio::process::Command;
use tracing::info;

use crate::db::queries;
use crate::prompts::fixbug::{kickoff_instructions, IssueContext};
use crate::types::{generate_id, Agent, Session, TerminalTarget};

use super::patterns::{AgentKind, ModelChoice};

const DEVORCH_DEFAULT_TASK_QUEUE: &str = "lantern-devorch";

/// Resolve `--issue` into title/body/url, best-effort. Never fails the
/// launch — on any resolution error the raw ref is kept and
/// `resolution_note` records what was tried, so the kickoff prompt can tell
/// the fixer to look it up itself.
pub async fn resolve_issue(issue_ref: &str) -> IssueContext {
    let trimmed = issue_ref.trim();

    if is_linear_issue_identifier(trimmed) {
        return IssueContext {
            raw_ref: trimmed.to_string(),
            resolution_note: Some(
                "Linear issue references are intentionally passed through without local resolution; open the linked Linear issue before proceeding.".to_string(),
            ),
            ..Default::default()
        };
    }

    if let Some(number) = extract_github_issue_number(trimmed) {
        return resolve_github(trimmed, &number).await;
    }

    IssueContext {
        raw_ref: trimmed.to_string(),
        resolution_note: Some(
            "not a Linear issue identifier or GitHub issue number/URL; skipped auto-resolution"
                .to_string(),
        ),
        ..Default::default()
    }
}

/// Return true for a Linear issue identifier such as `PAL-123`.
///
/// Linear remains the task-system authority. Lantern deliberately preserves
/// the key as opaque context instead of introducing a second client or a local
/// issue cache.
fn is_linear_issue_identifier(s: &str) -> bool {
    let Some((team, number)) = s.rsplit_once('-') else {
        return false;
    };

    !team.is_empty()
        && team.chars().all(|c| c.is_ascii_alphanumeric())
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

/// Pull the trailing issue number out of a plain number or a
/// `https://github.com/<org>/<repo>/issues/<n>` URL.
fn extract_github_issue_number(s: &str) -> Option<String> {
    if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() {
        return Some(s.to_string());
    }
    if let Some(idx) = s.find("/issues/") {
        let tail = &s[idx + "/issues/".len()..];
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return Some(digits);
        }
    }
    None
}

async fn resolve_github(raw_ref: &str, number: &str) -> IssueContext {
    let output = Command::new("gh")
        .args(["issue", "view", number, "--json", "title,body,url"])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            match serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                Ok(json) => IssueContext {
                    raw_ref: raw_ref.to_string(),
                    title: json.get("title").and_then(|v| v.as_str()).map(String::from),
                    body: json.get("body").and_then(|v| v.as_str()).map(String::from),
                    url: json.get("url").and_then(|v| v.as_str()).map(String::from),
                    resolution_note: None,
                },
                Err(e) => IssueContext {
                    raw_ref: raw_ref.to_string(),
                    resolution_note: Some(format!("gh issue view returned invalid JSON: {e}")),
                    ..Default::default()
                },
            }
        }
        Ok(out) => IssueContext {
            raw_ref: raw_ref.to_string(),
            resolution_note: Some(format!(
                "gh issue view {number} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )),
            ..Default::default()
        },
        Err(e) => IssueContext {
            raw_ref: raw_ref.to_string(),
            resolution_note: Some(format!("failed to run gh issue view {number}: {e}")),
            ..Default::default()
        },
    }
}

/// Turn a raw `--issue` reference into a short filesystem/branch-safe slug
/// for `<project>-fix-<issue>-<slot>` — prefers the bare GitHub issue number so
/// a GitHub URL doesn't blow up the branch name.
fn branch_slug(raw_ref: &str) -> String {
    let trimmed = raw_ref.trim();
    if let Some(number) = extract_github_issue_number(trimmed) {
        return issue_slug(&number);
    }
    issue_slug(trimmed)
}

fn issue_slug(raw_ref: &str) -> String {
    let slug: String = raw_ref
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "issue".to_string()
    } else {
        slug
    }
}

/// Build the fixer pane's CLI invocation directly from its `ModelChoice`
/// (unlike the legacy team grid, whose `build_agent_command` derives the
/// model from a hardcoded role table — executor/fixer picks carry an
/// explicit `model_id`/`effort` that must be used as-is).
fn build_fixer_command(model: &ModelChoice, init: &str, pane_name: &str) -> String {
    let suffix = format!(" {}", super::shell_escape(init));
    match model.agent {
        AgentKind::Codex => {
            let codex_cmd = format!(
                "codex --model {} -c 'model_reasoning_effort=\"{}\"' -c shell_environment_policy.inherit=all --dangerously-bypass-approvals-and-sandbox{}",
                model.model_id, model.effort, suffix
            );
            super::codex_app_server_wrapper(pane_name, &codex_cmd)
        }
        AgentKind::Gemini => {
            // Antigravity (`agy`) launch, selected by display name — same
            // convention as the other Gemini launch arms.
            format!(
                "env -u TERM_PROGRAM -u ITERM_SESSION_ID -u TERM_PROGRAM_VERSION ANTIGRAVITY_MODEL={} agy --dangerously-skip-permissions --prompt-interactive {}",
                super::shell_escape(model.antigravity_model()),
                super::shell_escape(init)
            )
        }
        // Claude (and any other fallback) — same shape as the legacy grid's
        // claude branch, but with the fixer's own picked model id.
        _ => format!(
            "claude --model {} --dangerously-skip-permissions --name {}{}",
            model.model_id,
            super::shell_escape(pane_name),
            suffix
        ),
    }
}

/// Small single-target input router: forwards notes to the `fixer` pane via
/// `lantern note fixer <msg>` and injects the text directly into the fixer's
/// iTerm2 session. Trimmed-down variant of the team pattern's multi-role
/// router (see `launch_team`'s `router_script_content`) since fixbug has
/// only one worker role to address.
fn write_input_router_script(session_id: &str, run_id: &str, repo_root: &str) -> String {
    let path = format!("/tmp/devorch-input-router-{}.py", session_id);
    let content = format!(
        r#"import sys, os, select
try:
    import readline
    readline.set_history_length(100)
except ImportError:
    pass
session = '{session_id}'
sys.stdout.write(f'\x1b]0;INPUT - {{session}}\x07\x1b]1;INPUT - {{session}}\x07\x1b]2;INPUT - {{session}}\x07')
sys.stdout.flush()
print('\x1b[1;36m====================================================\x1b[0m')
print('\x1b[1;37m         FIXBUG INPUT ROUTER (-> fixer)            \x1b[0m')
print('\x1b[1;36m====================================================\x1b[0m')
print('Type your note or command for the fixer. Press Enter to submit.')
print('  - For multiline, type or paste lines then press Ctrl-D to submit.')
print('  - Ctrl-C aborts current input.\n')

def process_cmd(cmd):
    cmd = cmd.strip()
    if not cmd:
        return
    print(f'\x1b[1;33mRouting note to FIXER: "{{cmd}}"\x1b[0m')
    env = os.environ.copy()
    env['DEVORCH_SESSION'] = '{session_id}'
    env['DEVORCH_RUN_ID'] = '{run_id}'
    env['DEVORCH_REPO_ROOT'] = '{repo_root}'
    import subprocess
    subprocess.run(['lantern', 'note', 'fixer', cmd], env=env)
    try:
        import iterm2

        def find_fixer(app, session_id):
            for w in app.windows:
                for t in w.tabs:
                    for s in t.sessions:
                        name = s.name or ""
                        if f"{{session_id}}-fixer" in name or "FIXER" in name:
                            return s
            return None

        async def inject(connection):
            app = await iterm2.async_get_app(connection)
            s = find_fixer(app, session)
            if s:
                await s.async_send_text(cmd)
                import asyncio
                await asyncio.sleep(0.05)
                await s.async_send_text("\r")

        iterm2.run_until_complete(inject)
    except Exception as e:
        print(f"Error injecting to iTerm2 pane: {{e}}", file=sys.stderr)

def edit_loop():
    lines = []
    while True:
        try:
            prompt = '\x1b[1;32m[INPUT] > \x1b[0m' if not lines else '          '
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
    sys.stdout.flush()
"#,
        session_id = session_id,
        run_id = run_id,
        repo_root = repo_root,
    );
    let _ = std::fs::write(&path, &content);
    path
}

/// Launch a `fixbug` session: one fixer worktree pane + an input pane.
pub async fn launch(
    name: Option<&str>,
    number: Option<u32>,
    no_init: bool,
    issue: String,
    fixer: ModelChoice,
) -> Result<()> {
    // Fail fast before any worktree/branch side effects if the layout helper
    // is missing (e.g. binary updated without reinstalling the scripts).
    crate::terminal::locate_script("iterm_launch_pattern.py")?;
    let repo = super::find_git_repo()?;
    super::ensure_squad_services();
    let repo_name = repo
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string();
    let config = crate::config::Config::load()?;
    let db_pool = crate::db::init_db(&config.database_url).await?;

    let name = name.unwrap_or(&repo_name).to_string();
    let number = match number {
        Some(n) => n,
        None => super::allocate_session_number(&db_pool, &repo, &name).await,
    };

    let resolved = resolve_issue(&issue).await;
    if let Some(note) = &resolved.resolution_note {
        tracing::warn!(issue = %issue, note = %note, "fixbug: issue resolution did not fully succeed, proceeding with raw ref");
    }

    let slug = branch_slug(&issue);
    let session_id = format!("{name}-fix-{slug}-{number}");
    let branch = session_id.clone();
    let worktree_root = repo.join(".claude").join("worktrees").join(&session_id);
    let fixer_worktree = worktree_root.join("fixer");

    if fixer_worktree.exists() {
        anyhow::bail!(
            "worktree {} already exists. Pick a different --issue/slot or clean up manually.",
            fixer_worktree.display()
        );
    }

    info!(repo = %repo.display(), session = %session_id, issue = %issue, "Launching fixbug session");

    let _ = super::ensure_antigravity_project_trusted(&repo);
    let _ = super::ensure_gemini_project_trusted(&repo);

    let repo_str = repo.to_string_lossy().to_string();
    super::create_one_worktree(&repo_str, &fixer_worktree, &branch, "fixer").await?;
    super::sync_skills_parallel(std::slice::from_ref(&fixer_worktree)).await;
    super::trust_workspaces(std::slice::from_ref(&fixer_worktree)).await;

    if !fixer.agent.as_str().eq_ignore_ascii_case("none") {
        super::ensure_mcp_server_registered(fixer.agent.as_str());
    }

    let run_id = format!(
        "{}-{}",
        session_id,
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    );
    let repo_root = repo
        .canonicalize()
        .unwrap_or_else(|_| repo.clone())
        .to_string_lossy()
        .to_string();

    let issue_context = IssueContext {
        raw_ref: issue.clone(),
        ..resolved
    };
    let kickoff = kickoff_instructions(&issue_context, &branch, &name);
    let init = if no_init { None } else { Some(kickoff) };

    let pane_name = format!("{name}-fixer-{number}");
    let fixer_cmd = match &init {
        Some(text) => build_fixer_command(&fixer, text, &pane_name),
        None => build_fixer_command(&fixer, "", &pane_name),
    };

    let fixer_title = format!("FIXER - {session_id}");
    let fixer_session_id =
        super::create_solo_window(&fixer_title, &fixer_worktree.to_string_lossy(), &fixer_cmd)
            .await?;

    let router_path = write_input_router_script(&session_id, &run_id, &repo_root);
    let input_cmd = format!("python3 {router_path}");
    let input_title = format!("INPUT - {session_id}");
    let input_session_id =
        super::create_solo_window(&input_title, &fixer_worktree.to_string_lossy(), &input_cmd)
            .await?;

    queries::insert_machine(&db_pool, &config.machine_id).await?;
    let session = Session {
        id: session_id.clone(),
        machine_id: config.machine_id.clone(),
        project_slug: name.clone(),
        slot_number: number as i64,
        status: "active".to_string(),
        created_at: chrono::Utc::now(),
        pattern: "fixbug".to_string(),
    };
    queries::insert_session(&db_pool, &session).await?;

    let agent = Agent {
        id: generate_id(&format!("agent-{}-fixer-{}", name, number)),
        session_id: session_id.clone(),
        role: "fixer".to_string(),
        pane_id: Some(fixer_session_id.clone()),
        worktree_path: fixer_worktree.to_string_lossy().to_string(),
        branch: branch.clone(),
        agent_kind: fixer.agent.as_str().to_string(),
        status: "idle".to_string(),
        last_seen_at: Some(chrono::Utc::now()),
        created_at: chrono::Utc::now(),
    };
    queries::insert_agent(&db_pool, &agent).await?;
    queries::insert_terminal_target(
        &db_pool,
        &TerminalTarget {
            agent_id: agent.id.clone(),
            iterm_session_id: fixer_session_id.clone(),
            pane_id: Some(fixer_session_id),
            transport_status: "ready".to_string(),
            last_seen_at: Some(chrono::Utc::now()),
        },
    )
    .await?;

    let _ = input_session_id; // input pane has no devorch Agent row (mirrors team's "inp" role)
    let _ = DEVORCH_DEFAULT_TASK_QUEUE; // reserved for parity with team's env wiring if MCP is added here later

    println!("\nFixbug workspace ready for session '{session_id}'.");
    println!("Fixer branch: {branch}");
    println!("Fixer worktree: {}", fixer_worktree.display());
    if let Some(note) = &issue_context.resolution_note {
        println!("Note: issue resolution incomplete — {note}");
    }
    println!("Inspect: `lantern status` · stop: `stopwork {session_id}`");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_github_issue_number_handles_plain_and_url() {
        assert_eq!(extract_github_issue_number("42"), Some("42".to_string()));
        assert_eq!(
            extract_github_issue_number("https://github.com/org/repo/issues/123"),
            Some("123".to_string())
        );
        assert_eq!(extract_github_issue_number("PAL-123"), None);
    }

    #[test]
    fn linear_issue_identifiers_are_accepted_as_opaque_context() {
        assert!(is_linear_issue_identifier("PAL-123"));
        assert!(is_linear_issue_identifier("LANTERN-1"));
        assert!(!is_linear_issue_identifier("123"));
        assert!(!is_linear_issue_identifier("PAL-"));
    }

    #[test]
    fn branch_slug_prefers_bare_number_over_full_url() {
        assert_eq!(branch_slug("https://github.com/org/repo/issues/123"), "123");
        assert_eq!(branch_slug("42"), "42");
        assert_eq!(branch_slug("PAL-123"), "pal-123");
    }
}
