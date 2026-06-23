//! Native pane delivery: type text directly into a running agent's iTerm2 pane.
//!
//! This is the load-bearing primitive that replaces the former TS delivery model
//! (agent-runner's `global.writeToAgentPty` + the `injectToAgent`/`deliver-pane.py`
//! Temporal activities). The runtime now owns delivery end-to-end: look up the
//! target pane's iTerm2 session id from SQLite and inject via `async_send_text`.

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Inline Python that sends text (read from stdin) into an iTerm2 session by id.
/// Text is passed on stdin rather than argv to avoid shell/escaping limits on
/// large multi-line prompts.
///
/// SUBMIT SEMANTICS (load-bearing): TUI agent CLIs (claude/codex/kimi/agy) treat
/// the Enter key as a SEPARATE input event from the body text. So we send the
/// body, sleep 400ms to let the CLI's input box settle, then send a lone
/// carriage return `"\r"` as a second `async_send_text` call. Appending `"\n"`
/// to the body in one call does NOT reliably submit. Mirrors the former TS
/// inject-pty.ts / deliver-pane.py two-write behavior.
const INJECT_PY: &str = r#"
import asyncio
import sys
import iterm2

TARGET = sys.argv[1]
TEXT = sys.stdin.read()

async def main(connection):
    app = await iterm2.async_get_app(connection)
    session = app.get_session_by_id(TARGET)
    if session is None:
        for window in app.windows:
            for tab in window.tabs:
                for s in tab.sessions:
                    if s.session_id == TARGET:
                        session = s
                        break
                if session is not None:
                    break
            if session is not None:
                break
    if session is None:
        raise SystemExit(2)
    await session.async_send_text(TEXT)
    await asyncio.sleep(0.4)
    await session.async_send_text("\r")

iterm2.run_until_complete(main)
"#;

/// Candidate python interpreters, in priority order (mirrors `terminal::iterm`).
fn python_candidates() -> Vec<String> {
    [
        std::env::var("LANTERN_PYTHON").ok(),
        Some("/opt/homebrew/bin/python3.14".to_string()),
        Some("python3".to_string()),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Type `text` into the iTerm2 pane identified by `iterm_session_id`.
///
/// The body is sent as-is; submission (the Enter keypress) is handled by the
/// python helper as a SEPARATE `"\r"` event after a 400ms settle. Callers must
/// NOT append their own trailing newline expecting it to submit — it will not.
pub async fn inject_text(iterm_session_id: &str, text: &str) -> Result<()> {
    let payload = text.to_string();

    let mut last_err = String::new();
    for py in python_candidates() {
        let mut child = match Command::new(&py)
            .args(["-c", INJECT_PY, iterm_session_id])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                last_err = format!("{py}: {e}");
                continue;
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            // Best-effort: if the write fails the wait below surfaces the error.
            let _ = stdin.write_all(payload.as_bytes()).await;
            let _ = stdin.shutdown().await;
        }

        match child.wait_with_output().await {
            Ok(output) if output.status.success() => return Ok(()),
            Ok(output) => {
                last_err = String::from_utf8_lossy(&output.stderr).trim().to_string();
                // Exit code 2 = session not found; other interpreters won't help.
                if output.status.code() == Some(2) {
                    anyhow::bail!(
                        "iTerm2 session {iterm_session_id} not found (pane closed?): {last_err}"
                    );
                }
            }
            Err(e) => last_err = format!("{py}: {e}"),
        }
    }

    anyhow::bail!("failed to inject into iTerm2 session {iterm_session_id}: {last_err}")
}

/// Resolve the live iTerm2 session id for a given squad session + role by joining
/// `agents` → `terminal_targets`. Returns `None` if the pane is unknown.
pub async fn resolve_iterm_session(
    pool: &SqlitePool,
    session_id: &str,
    role: &str,
) -> Result<Option<String>> {
    let sid: Option<String> = sqlx::query_scalar(
        "SELECT t.iterm_session_id \
         FROM terminal_targets t \
         JOIN agents a ON a.id = t.agent_id \
         WHERE a.session_id = ? AND a.role = ? \
         LIMIT 1",
    )
    .bind(session_id)
    .bind(role)
    .fetch_optional(pool)
    .await
    .context("query terminal_target for (session, role)")?;
    Ok(sid)
}

/// Delivery transport backend. Selected per-process via `LANTERN_TRANSPORT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Type text into a live iTerm2 pane (default; preserves legacy behavior).
    Iterm,
    /// Spawn a headless Goose/ACP worker (see `delivery::acp`).
    Acp,
}

/// Resolve the active transport from the `LANTERN_TRANSPORT` env var.
/// `acp` selects the ACP backend; anything else (or unset) is `Iterm`.
pub fn current_transport() -> Transport {
    // Read once at first use: the transport is a process-global launch choice and
    // must not change mid-session (also keeps concurrent tests deterministic).
    static CACHE: std::sync::OnceLock<Transport> = std::sync::OnceLock::new();
    *CACHE.get_or_init(
        || match std::env::var("LANTERN_TRANSPORT").ok().as_deref() {
            Some("acp") => Transport::Acp,
            _ => Transport::Iterm,
        },
    )
}

/// Resolve the pane and inject in one step. Returns a clear error if the role has
/// no known pane so callers can fall back to SQLite-only persistence.
async fn deliver_to_role_iterm(
    pool: &SqlitePool,
    session_id: &str,
    role: &str,
    text: &str,
) -> Result<()> {
    let iterm_session = resolve_iterm_session(pool, session_id, role)
        .await?
        .with_context(|| format!("no live pane for role '{role}' in session '{session_id}'"))?;
    inject_text(&iterm_session, text).await
}

/// Deliver `text` to the agent for `role`, dispatching to the active transport.
///
/// Public signature is intentionally identical across transports so callers
/// (autoheal, mcp::tools::try_inject) need no changes.
pub async fn deliver_to_role(
    pool: &SqlitePool,
    session_id: &str,
    role: &str,
    text: &str,
) -> Result<()> {
    match current_transport() {
        Transport::Iterm => deliver_to_role_iterm(pool, session_id, role, text).await,
        Transport::Acp => {
            crate::delivery::acp::deliver_to_role_acp(pool, session_id, role, text).await
        }
    }
}
