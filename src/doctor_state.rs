use std::collections::HashSet;

use chrono::{Duration, Utc};
use clap::ValueEnum;
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::config::Config;
use crate::db::queries;
use crate::types::TerminalTarget;

#[cfg(unix)]
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[derive(Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum DoctorStateFix {
    Quarantine,
}

pub async fn run(
    pool: &SqlitePool,
    config: &Config,
    fix: Option<DoctorStateFix>,
) -> anyhow::Result<()> {
    let active_sessions = queries::get_active_sessions(pool).await?;
    let agents_by_session = classify_agents_by_session(pool, &active_sessions).await?;
    let temporal_discovery = classify_temporal_coverage(&active_sessions).await;

    let stale_threshold = config.stale_threshold_secs;
    let mut session_missing_run: Vec<String> = Vec::new();
    let mut stale_targets: Vec<(String, String)> = Vec::new();
    let mut degraded_runners: Vec<(String, String)> = Vec::new();
    let mut quarantined_targets: Vec<String> = Vec::new();

    for session_id in &active_sessions {
        let session_has_temporal = temporal_discovery.contains(session_id);
        if !session_has_temporal {
            session_missing_run.push(session_id.clone());
        }
    }

    for (session_id, agents) in &agents_by_session {
        for agent in agents {
            let maybe_target = queries::get_terminal_target(pool, &agent.id).await?;
            let Some(target) = maybe_target else {
                continue;
            };

            if target.transport_status == "ready" && is_runner_pid_dead(&target) {
                degraded_runners.push((session_id.clone(), agent.id.clone()));
                continue;
            }

            if is_likely_legacy_tmux_target(&target) {
                quarantined_targets.push(agent.id.clone());
                continue;
            }

            if is_stale_terminal_target(&target, stale_threshold) {
                stale_targets.push((session_id.clone(), agent.id.clone()));
            }
        }
    }

    let unique_sessions_missing_run: Vec<_> = unique_sorted(session_missing_run);
    if unique_sessions_missing_run.is_empty()
        && stale_targets.is_empty()
        && degraded_runners.is_empty()
        && quarantined_targets.is_empty()
    {
        println!("doctor-state: no stale projected state found");
        return Ok(());
    }

    if let Some(DoctorStateFix::Quarantine) = fix {
        let mut fixed_targets = 0usize;
        for session_id in &unique_sessions_missing_run {
            if let Some(agents) = agents_by_session.get(session_id) {
                for agent in agents {
                    queries::update_agent_status(pool, &agent.id, "degraded").await?;
                }
            }
            queries::update_session_status(pool, session_id, "stopped").await?;
            queries::log_event(
                pool,
                session_id,
                None,
                "doctor_state_audit",
                Some("{\"reason\":\"temporal_run_unknown\",\"severity\":\"degraded\",\"mode\":\"quarantine\"}"),
            )
            .await?;
            info!(session_id = %session_id, "marked session as stopped from doctor-state fix");
        }

        for (_, agent_id) in &stale_targets {
            queries::update_terminal_target_status(pool, agent_id, "stale").await?;
            fixed_targets += 1;
        }

        for (_, agent_id) in &degraded_runners {
            queries::update_terminal_target_status(pool, agent_id, "degraded").await?;
            fixed_targets += 1;
        }

        for agent_id in &quarantined_targets {
            let target = queries::get_terminal_target(pool, agent_id).await?;
            if let Some(target) = target {
                let quarantined = queries::is_terminal_target_quarantined(pool, agent_id).await?;
                if !quarantined {
                    queries::insert_terminal_target_quarantine(
                        pool,
                        queries::QuarantineParams {
                            agent_id,
                            legacy_tmux_session: &target.iterm_session_id,
                            legacy_tmux_window: "legacy-window",
                            legacy_tmux_pane: target.pane_id.as_deref().unwrap_or(""),
                            legacy_inject_method: "legacy_tmux",
                            legacy_last_injected_at: target
                                .last_seen_at
                                .as_ref()
                                .map(|t| t.to_rfc3339())
                                .as_deref(),
                            quarantine_reason: "legacy_tmux_target",
                        },
                    )
                    .await?;
                }
                queries::update_terminal_target_status(pool, agent_id, "quarantined").await?;
                fixed_targets += 1;
            }
        }

        queries::log_event(
            pool,
            "system",
            None,
            "doctor_state_audit",
            Some(&format!(
                "{{\"targets_fixed\":{fixed_targets},\"temporal_runs_discovered\":{},\"degraded_runners\":{}}}",
                temporal_discovery.len(),
                degraded_runners.len()
            )),
        )
        .await?;

        println!(
            "doctor-state: fixed {} stale-state entries (sessions: {}, targets: {}, degraded: {}, quarantined: {})",
            fixed_targets,
            unique_sessions_missing_run.len(),
            stale_targets.len(),
            degraded_runners.len(),
            quarantined_targets.len()
        );

        return Ok(());
    }

    if temporal_discovery.is_empty() {
        warn!("Temporal run discovery unavailable: marking active sessions as degraded state in output only");
    }

    println!("doctor-state summary");
    if !unique_sessions_missing_run.is_empty() {
        println!(
            "sessions without Temporal run state: {}",
            unique_sessions_missing_run.join(", ")
        );
    }
    if !stale_targets.is_empty() {
        println!("stale terminal targets: {}", stale_targets.len());
    }
    if !degraded_runners.is_empty() {
        println!("dead runner pids: {}", degraded_runners.len());
    }
    if !quarantined_targets.is_empty() {
        println!("legacy terminal targets: {}", quarantined_targets.len());
    }

    Ok(())
}

async fn classify_agents_by_session(
    pool: &SqlitePool,
    sessions: &[String],
) -> anyhow::Result<std::collections::HashMap<String, Vec<crate::types::Agent>>> {
    let mut map = std::collections::HashMap::new();
    for session_id in sessions {
        let agents = queries::get_agents_for_session(pool, session_id).await?;
        map.insert(session_id.clone(), agents);
    }
    Ok(map)
}

async fn classify_temporal_coverage(sessions: &[String]) -> HashSet<String> {
    let _ = sessions.len();

    match discover_temporal_sessions().await {
        Ok(active_runs) => active_runs,
        Err(error) => {
            warn!("Temporal run discovery failed: {}", error);
            HashSet::new()
        }
    }
}

fn is_stale_terminal_target(target: &TerminalTarget, stale_threshold_secs: u64) -> bool {
    if target.transport_status == "quarantined" || target.transport_status == "stale" {
        return target.transport_status != "quarantined";
    }
    if target.last_seen_at.is_none() {
        return true;
    }

    let now = Utc::now();
    let stale_limit = Duration::seconds(stale_threshold_secs as i64);
    now - target.last_seen_at.expect("timestamp must exist") > stale_limit
}

fn is_runner_pid_dead(target: &TerminalTarget) -> bool {
    let Some(pane_id) = target.pane_id.as_deref() else {
        return false;
    };
    let Ok(pid) = pane_id.parse::<i32>() else {
        return false;
    };
    !process_is_alive(pid)
}

fn process_is_alive(pid: i32) -> bool {
    #[cfg(unix)]
    {
        if pid <= 0 {
            return false;
        }

        if unsafe { kill(pid, 0) } == 0 {
            return true;
        }

        // EPERM means the process exists, but the current user cannot signal it.
        std::io::Error::last_os_error().raw_os_error() == Some(1)
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn is_likely_legacy_tmux_target(target: &TerminalTarget) -> bool {
    if target.transport_status == "quarantined" {
        return false;
    }
    let iterm_session_id = target.iterm_session_id.to_ascii_lowercase();
    let pane_id = target
        .pane_id
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    iterm_session_id.starts_with("legacy")
        || iterm_session_id.contains("tmux")
        || pane_id.starts_with('%')
}

async fn discover_temporal_sessions() -> anyhow::Result<HashSet<String>> {
    Err(anyhow::anyhow!("Temporal run discovery unavailable"))
}

fn unique_sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort_unstable();
    values.dedup();
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::create_test_pool;
    use crate::types::{Agent, Session};
    use chrono::Duration as ChronoDuration;

    fn now_minus(seconds: i64) -> chrono::DateTime<chrono::Utc> {
        Utc::now() - ChronoDuration::seconds(seconds)
    }

    fn make_session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            machine_id: "machine-1".to_string(),
            project_slug: "test-project".to_string(),
            slot_number: 1,
            status: "active".to_string(),
            created_at: Utc::now(),
            pattern: "team".to_string(),
        }
    }

    fn make_agent(id: &str, session_id: &str) -> Agent {
        Agent {
            id: id.to_string(),
            session_id: session_id.to_string(),
            role: "ai".to_string(),
            pane_id: Some("1".to_string()),
            worktree_path: format!("/tmp/{id}"),
            branch: "main".to_string(),
            agent_kind: "claude".to_string(),
            status: "idle".to_string(),
            last_seen_at: Some(Utc::now()),
            created_at: Utc::now(),
        }
    }

    async fn insert_active_session_with_agent(
        pool: &SqlitePool,
        session_id: &str,
        agent_id: &str,
    ) -> String {
        let session = make_session(session_id);
        queries::insert_session(pool, &session)
            .await
            .expect("insert session");

        let agent = make_agent(agent_id, session_id);
        queries::insert_agent(pool, &agent)
            .await
            .expect("insert agent");
        queries::insert_terminal_target(
            pool,
            &crate::types::TerminalTarget {
                agent_id: agent.id.clone(),
                iterm_session_id: format!("iterm-{session_id}"),
                pane_id: Some("pane-{session_id}".to_string()),
                transport_status: "ready".to_string(),
                last_seen_at: Some(Utc::now()),
            },
        )
        .await
        .expect("insert target");
        agent.id.clone()
    }

    fn make_config() -> Config {
        Config {
            stale_threshold_secs: 300,
            ..Config::default()
        }
    }

    #[tokio::test]
    async fn marks_active_sessions_without_temporal_run_as_stopped() {
        let (pool, _dir) = create_test_pool().await;
        let session_id = "sess-no-temporal";
        let agent_id =
            insert_active_session_with_agent(&pool, session_id, "agent-no-temporal").await;

        run(&pool, &make_config(), Some(DoctorStateFix::Quarantine))
            .await
            .expect("run doctor-state");

        let session = queries::get_session(&pool, session_id)
            .await
            .expect("query session")
            .expect("session row");
        assert_eq!(session.status, "stopped");

        let agent = queries::get_agent_by_id(&pool, &agent_id)
            .await
            .expect("query agent")
            .expect("agent row");
        assert_eq!(agent.status, "degraded");
    }

    #[tokio::test]
    async fn stale_terminal_targets_are_marked_stale() {
        let (pool, _dir) = create_test_pool().await;
        let session_id = "sess-stale-target";
        let agent_id = insert_active_session_with_agent(&pool, session_id, "agent-stale").await;

        sqlx::query(
            "UPDATE terminal_targets SET transport_status = 'ready', last_seen_at = ? WHERE agent_id = ?",
        )
        .bind(now_minus(600).to_rfc3339())
        .bind(&agent_id)
        .execute(&pool)
        .await
        .expect("set stale target heartbeat");

        run(&pool, &make_config(), Some(DoctorStateFix::Quarantine))
            .await
            .expect("run doctor-state");

        let target = queries::get_terminal_target(&pool, &agent_id)
            .await
            .expect("query target")
            .expect("target row");
        assert_eq!(target.transport_status, "stale");
    }

    #[tokio::test]
    async fn dead_runner_pids_are_marked_degraded() {
        let (pool, _dir) = create_test_pool().await;
        let session_id = "sess-dead-runner";
        let agent_id = insert_active_session_with_agent(&pool, session_id, "agent-dead").await;

        let dead_pid = "999999999";
        sqlx::query("UPDATE terminal_targets SET pane_id = ? WHERE agent_id = ?")
            .bind(dead_pid)
            .bind(&agent_id)
            .execute(&pool)
            .await
            .expect("set dead pid in pane_id");

        run(&pool, &make_config(), Some(DoctorStateFix::Quarantine))
            .await
            .expect("run doctor-state");

        let target = queries::get_terminal_target(&pool, &agent_id)
            .await
            .expect("query target")
            .expect("target row");
        assert_eq!(target.transport_status, "degraded");
    }

    #[tokio::test]
    async fn legacy_tmux_targets_are_quarantined() {
        let (pool, _dir) = create_test_pool().await;
        let session_id = "sess-legacy";
        let agent_id = insert_active_session_with_agent(&pool, session_id, "agent-legacy").await;

        sqlx::query(
            "UPDATE terminal_targets SET iterm_session_id = 'tmux-legacy-session', pane_id = '%1', transport_status = 'ready' WHERE agent_id = ?",
        )
        .bind(&agent_id)
        .execute(&pool)
        .await
        .expect("set legacy target fields");

        run(&pool, &make_config(), Some(DoctorStateFix::Quarantine))
            .await
            .expect("run doctor-state");

        let target = queries::get_terminal_target(&pool, &agent_id)
            .await
            .expect("query target")
            .expect("target row");
        assert_eq!(target.transport_status, "quarantined");

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM terminal_target_quarantine WHERE agent_id = ?",
        )
        .bind(&agent_id)
        .fetch_one(&pool)
        .await
        .expect("query quarantine count");

        assert_eq!(count, 1);
    }
}
