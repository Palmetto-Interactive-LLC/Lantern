//! ACP transport backend: headless Goose/ACP worker runtime.
//!
//! Instead of typing text into a live iTerm2 pane (see `inject.rs`), this backend
//! spawns a one-shot headless `goose run` process configured to drive an ACP
//! agent (claude-acp / codex-acp). The agent rides EXISTING CLI auth (claude CLI
//! -> ~/.claude.json, codex CLI -> ~/.codex); goose stores no AI credentials.
//!
//! The devorch stdio MCP server is passed through to the ACP agent via goose's
//! `--with-extension` so the headless worker still participates in the squad.
//!
//! NOTE: we resolve `std::env::current_exe()` for the `lantern mcp` extension
//! command (falling back to bare `lantern` on PATH if that fails) so the headless
//! worker uses the same binary that spawned it rather than relying on PATH alone.

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::path::Path;
use std::process::Output;
use tokio::process::Command;

/// Map an agent CLI family to the goose ACP provider id.
///
/// claude -> claude-acp; codex -> codex-acp. Everything else (gemini/agy/kimi)
/// falls back to claude-acp for now.
pub fn goose_provider_for_agent(agent_kind: &str) -> &'static str {
    match agent_kind.to_lowercase().as_str() {
        "claude" => "claude-acp",
        "codex" => "codex-acp",
        _ => "claude-acp",
    }
}

/// Codex model id for a role, using the SAME canonical source of truth as
/// `startwork` (`CODEX_ROLE_MODELS` / `CODEX_DEFAULT_MODEL`, verified against
/// `codex debug models`) so the two launch paths never diverge.
fn codex_model_for_role(role: &str) -> String {
    crate::startwork::CODEX_ROLE_MODELS
        .iter()
        .find(|(r, _)| *r == role)
        .map(|(_, m)| *m)
        .unwrap_or(crate::startwork::CODEX_DEFAULT_MODEL)
        .to_string()
}

/// Map (agent_kind, role) to an ACP model id, mirroring the role tiers used by
/// `startwork` so the ACP and iTerm launch paths request identical models.
///
/// claude-acp: opus (orchestrator/ai/sec) | haiku (doc) | sonnet (default).
/// codex-acp: from the shared `CODEX_ROLE_MODELS` table (gpt-5.5 / gpt-5.4-mini).
pub fn goose_model_for_role(agent_kind: &str, role: &str) -> String {
    match goose_provider_for_agent(agent_kind) {
        "codex-acp" => codex_model_for_role(role),
        // claude-acp (default for claude and all fallbacks)
        _ => match role {
            "orchestrator" | "ai" | "sec" => "opus".to_string(),
            "doc" => "haiku".to_string(),
            _ => "sonnet".to_string(),
        },
    }
}

/// Resolve the command used for the `lantern mcp` extension. Prefer the current
/// executable so the headless worker uses the same binary; fall back to bare
/// `lantern` (resolved via PATH, e.g. ~/.lantern/bin) if that lookup fails.
fn lantern_mcp_command() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "lantern".to_string())
}

/// Quote a token for goose's space-delimited `--with-extension` value if it
/// contains whitespace, so a binary path with spaces is not split mid-path.
fn quote_if_needed(s: &str) -> String {
    if s.chars().any(|c| c.is_whitespace()) {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}

/// Spawn a headless one-shot Goose/ACP worker for `task` and wait for it to exit.
///
/// Returns the captured process `Output` (caller inspects status/stdout/stderr).
pub async fn spawn_acp_worker(
    agent_kind: &str,
    role: &str,
    cwd: &Path,
    task: &str,
    session: &str,
    run_id: Option<&str>,
) -> Result<Output> {
    let provider = goose_provider_for_agent(agent_kind);
    let model = goose_model_for_role(agent_kind, role);
    let mcp_cmd = lantern_mcp_command();

    // The devorch stdio MCP server, with its env passed through so it knows which
    // session/role it is acting for inside the ACP agent.
    let extension = format!(
        "DEVORCH_SESSION={session} DEVORCH_ROLE={role} {mcp} mcp",
        mcp = quote_if_needed(&mcp_cmd),
    );

    let mut cmd = Command::new("goose");
    cmd.arg("run")
        .arg("-t")
        .arg(task)
        .arg("--with-extension")
        .arg(&extension)
        .current_dir(cwd)
        .env("GOOSE_PROVIDER", provider)
        .env("GOOSE_MODEL", &model)
        .env("GOOSE_DISABLE_KEYRING", "1")
        .env("DEVORCH_SESSION", session)
        .env("DEVORCH_ROLE", role);

    if let Some(rid) = run_id {
        cmd.env("DEVORCH_RUN_ID", rid);
    }

    let output = cmd.output().await.with_context(|| {
        format!(
            "failed to spawn goose ACP worker (provider={provider}, model={model}, role={role}, session={session})"
        )
    })?;

    Ok(output)
}

/// ACP analog of `inject::deliver_to_role_iterm`: find the agent for `role`, then
/// spawn a headless Goose/ACP worker in its worktree to carry out `text`.
///
/// Fire-and-forget by design: a headless `goose run` can take minutes, and the
/// iTerm callers (`autoheal`, `mcp::tools::try_inject`, `human::commands`) treat
/// delivery as best-effort and must not block. The worker reports its result back
/// through the devorch MCP (passed via `--with-extension`), exactly like a pane
/// agent — so we resolve the target synchronously (surfacing a missing role
/// immediately) then spawn the run in the background and return.
pub async fn deliver_to_role_acp(
    pool: &SqlitePool,
    session_id: &str,
    role: &str,
    text: &str,
) -> Result<()> {
    let agents = crate::db::queries::get_agents_for_session(pool, session_id)
        .await
        .context("load agents for ACP delivery")?;

    let agent = agents
        .into_iter()
        .find(|a| a.role == role)
        .with_context(|| format!("no agent for role '{role}' in session '{session_id}'"))?;

    let agent_kind = agent.agent_kind.clone();
    let worktree = agent.worktree_path.clone();
    let role = role.to_string();
    let session = session_id.to_string();
    let task = text.to_string();

    tokio::spawn(async move {
        match spawn_acp_worker(
            &agent_kind,
            &role,
            Path::new(&worktree),
            &task,
            &session,
            None,
        )
        .await
        {
            Ok(output) if output.status.success() => {}
            Ok(output) => tracing::warn!(
                role = %role,
                session = %session,
                status = %output.status,
                stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                "ACP worker exited non-zero"
            ),
            Err(e) => {
                tracing::error!(role = %role, session = %session, error = %e, "failed to spawn ACP worker")
            }
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_mapping() {
        assert_eq!(goose_provider_for_agent("claude"), "claude-acp");
        assert_eq!(goose_provider_for_agent("Claude"), "claude-acp");
        assert_eq!(goose_provider_for_agent("codex"), "codex-acp");
        // gemini/agy/kimi fall back to claude-acp for now.
        assert_eq!(goose_provider_for_agent("agy"), "claude-acp");
        assert_eq!(goose_provider_for_agent("kimi"), "claude-acp");
        assert_eq!(goose_provider_for_agent("gemini"), "claude-acp");
    }

    #[test]
    fn claude_model_tiers() {
        assert_eq!(goose_model_for_role("claude", "orchestrator"), "opus");
        assert_eq!(goose_model_for_role("claude", "ai"), "opus");
        assert_eq!(goose_model_for_role("claude", "sec"), "opus");
        assert_eq!(goose_model_for_role("claude", "doc"), "haiku");
        assert_eq!(goose_model_for_role("claude", "qa"), "sonnet");
    }

    #[test]
    fn codex_model_tiers_match_canonical_startwork() {
        // Single source of truth: these must equal startwork's CODEX_ROLE_MODELS.
        assert_eq!(goose_model_for_role("codex", "orchestrator"), "gpt-5.5");
        assert_eq!(goose_model_for_role("codex", "ai"), "gpt-5.5");
        assert_eq!(goose_model_for_role("codex", "sec"), "gpt-5.5");
        // doc/qa fall back to CODEX_DEFAULT_MODEL.
        assert_eq!(goose_model_for_role("codex", "doc"), "gpt-5.4-mini");
        assert_eq!(goose_model_for_role("codex", "qa"), "gpt-5.4-mini");
    }

    #[test]
    fn extension_quotes_paths_with_spaces() {
        assert_eq!(quote_if_needed("/no/space/lantern"), "/no/space/lantern");
        assert_eq!(
            quote_if_needed("/has space/lantern"),
            "'/has space/lantern'"
        );
    }
}
