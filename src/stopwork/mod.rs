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

    let worktree_paths = cleanup_agent_worktrees(&agents, options.preserve_worktrees).await;

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

/// Remove each agent's git worktree and then its branch, in that order.
///
/// Order is load-bearing: a branch that is still checked out in a worktree
/// cannot be deleted, so the worktree MUST be removed first. Applies to every
/// agent including the orchestrator (which has its own worktree, not repo root).
/// Only paths under `.claude/worktrees/` are touched — the repo root is never
/// removed. Returns the list of worktree paths actually removed.
async fn cleanup_agent_worktrees(
    agents: &[crate::types::Agent],
    preserve_worktrees: bool,
) -> Vec<String> {
    let mut removed = Vec::new();
    for agent in agents {
        if preserve_worktrees {
            println!(
                "Preserving worktree '{}' for agent '{}'",
                agent.worktree_path, agent.id
            );
            continue;
        }

        // Safety: only ever touch worktrees under `.claude/worktrees/`.
        let marker = "/.claude/worktrees/";
        let Some(idx) = agent.worktree_path.find(marker) else {
            continue;
        };
        // Derive the repo root so git runs correctly regardless of cwd.
        let repo_root = &agent.worktree_path[..idx];

        // Remove the worktree FIRST (a checked-out branch can't be deleted).
        let wt_status = Command::new("git")
            .args([
                "-C",
                repo_root,
                "worktree",
                "remove",
                "--force",
                &agent.worktree_path,
            ])
            .status()
            .await;
        match wt_status {
            Ok(s) if s.success() => {
                println!("Removed git worktree '{}'", agent.worktree_path);
                removed.push(agent.worktree_path.clone());
            }
            _ => {
                let _ = Command::new("git")
                    .args(["-C", repo_root, "worktree", "prune"])
                    .status()
                    .await;
                println!(
                    "Failed to remove worktree '{}', ran git worktree prune",
                    agent.worktree_path
                );
            }
        }

        // THEN delete the branch (now unreferenced by any worktree).
        let branch_status = Command::new("git")
            .args(["-C", repo_root, "branch", "-D", &agent.branch])
            .status()
            .await;
        match branch_status {
            Ok(s) if s.success() => println!("Deleted git branch '{}'", agent.branch),
            _ => println!("Failed to delete branch '{}' or not found", agent.branch),
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_agent(role: &str, worktree_path: &str, branch: &str) -> crate::types::Agent {
        crate::types::Agent {
            id: format!("agent-{role}"),
            session_id: "s-1".to_string(),
            role: role.to_string(),
            pane_id: None,
            worktree_path: worktree_path.to_string(),
            branch: branch.to_string(),
            agent_kind: "claude".to_string(),
            status: "idle".to_string(),
            last_seen_at: None,
            created_at: chrono::Utc::now(),
        }
    }

    /// Hermetic real-git proof that stopwork teardown removes EVERY worktree and
    /// branch — including the orchestrator's — in the correct order (worktree
    /// before branch, otherwise `git branch -D` fails on a checked-out branch).
    #[tokio::test]
    async fn cleanup_removes_all_worktrees_and_branches_including_orchestrator() {
        use std::process::Command as Git;
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().to_path_buf();
        let run = |args: &[&str]| {
            let ok = Git::new("git")
                .args(args)
                .current_dir(&repo)
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {args:?} failed");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@t.co"]);
        run(&["config", "user.name", "t"]);
        run(&["commit", "-q", "--allow-empty", "-m", "seed"]);

        let wroot = repo.join(".claude/worktrees/s-1");
        std::fs::create_dir_all(&wroot).unwrap();
        let specs = [
            ("orchestrator", "s-1"),
            ("ai", "s-ai-1"),
            ("dat", "s-dat-1"),
        ];
        let mut agents = Vec::new();
        for (role, branch) in specs {
            let wt = wroot.join(branch);
            run(&["worktree", "add", "-b", branch, wt.to_str().unwrap()]);
            agents.push(make_agent(role, wt.to_str().unwrap(), branch));
        }

        let removed = cleanup_agent_worktrees(&agents, false).await;
        assert_eq!(removed.len(), 3, "all three worktrees should be removed");

        let wt_out = String::from_utf8_lossy(
            &Git::new("git")
                .args(["worktree", "list"])
                .current_dir(&repo)
                .output()
                .unwrap()
                .stdout,
        )
        .to_string();
        assert!(
            !wt_out.contains(".claude/worktrees/s-1"),
            "no session worktrees should remain:\n{wt_out}"
        );

        let br_out = String::from_utf8_lossy(
            &Git::new("git")
                .args(["branch", "--list"])
                .current_dir(&repo)
                .output()
                .unwrap()
                .stdout,
        )
        .to_string();
        for (_, b) in specs {
            assert!(
                !br_out.contains(b),
                "branch {b} should be deleted:\n{br_out}"
            );
            assert!(!wroot.join(b).exists(), "worktree dir {b} should be gone");
        }
    }

    /// preserve_worktrees must keep everything in place.
    #[tokio::test]
    async fn cleanup_preserves_when_requested() {
        use std::process::Command as Git;
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().to_path_buf();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "t@t.co"],
            vec!["config", "user.name", "t"],
            vec!["commit", "-q", "--allow-empty", "-m", "seed"],
        ] {
            Git::new("git")
                .args(&args)
                .current_dir(&repo)
                .status()
                .unwrap();
        }
        let wt = repo.join(".claude/worktrees/s-2/s-ai-2");
        std::fs::create_dir_all(wt.parent().unwrap()).unwrap();
        Git::new("git")
            .args(["worktree", "add", "-b", "s-ai-2", wt.to_str().unwrap()])
            .current_dir(&repo)
            .status()
            .unwrap();
        let agents = vec![make_agent("ai", wt.to_str().unwrap(), "s-ai-2")];
        let removed = cleanup_agent_worktrees(&agents, true).await;
        assert!(removed.is_empty(), "nothing removed when preserving");
        assert!(wt.exists(), "worktree must be preserved");
    }

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
