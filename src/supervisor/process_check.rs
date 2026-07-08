use anyhow::Result;
use sqlx::SqlitePool;
use tracing::info;

use crate::db::queries::{get_terminal_target, log_event};
use crate::types::Agent;

/// Verify that each agent's CLI process is still alive.
pub async fn check_agent_processes(
    pool: &SqlitePool,
    session_id: &str,
    agents: &[Agent],
) -> Result<()> {
    for agent in agents {
        let terminal_target = get_terminal_target(pool, &agent.id).await?;

        // Record the process check as audit-only; the supervisor does not own runtime recovery.
        if let Some(pane_id) = &agent.pane_id {
            log_event(
                pool,
                session_id,
                Some(&agent.id),
                "runner_process_check_temporal_required",
                Some(pane_id),
            )
            .await?;
        }

        match terminal_target {
            None => {
                let payload = format!("missing_iterm_session for {}", agent.id);
                log_event(
                    pool,
                    session_id,
                    Some(&agent.id),
                    "temporal_recovery_required",
                    Some(&payload),
                )
                .await?;
            }
            Some(target) => {
                if target.transport_status != "ready" {
                    let payload = format!(
                        "stale_terminal_target for {} status={}",
                        agent.id, target.transport_status
                    );
                    log_event(
                        pool,
                        session_id,
                        Some(&agent.id),
                        "temporal_recovery_required",
                        Some(&payload),
                    )
                    .await?;
                }

                if target.iterm_session_id.trim().is_empty() {
                    let payload = format!("empty_iterm_session_id for {}", agent.id);
                    log_event(
                        pool,
                        session_id,
                        Some(&agent.id),
                        "temporal_recovery_required",
                        Some(&payload),
                    )
                    .await?;
                }

                if let Some(expected) = &agent.pane_id {
                    if target.pane_id.as_deref() != Some(expected.as_str()) {
                        let payload = format!(
                            "iterm_pane_mismatch for {} expected={} actual={}",
                            agent.id,
                            expected,
                            target.pane_id.as_deref().unwrap_or("<none>"),
                        );
                        log_event(
                            pool,
                            session_id,
                            Some(&agent.id),
                            "temporal_recovery_required",
                            Some(&payload),
                        )
                        .await?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Perform a self-health check on the relay.
pub async fn check_relay_health(pool: &SqlitePool) -> Result<()> {
    let _ = sqlx::query("SELECT 1").execute(pool).await?;
    info!("relay health check: database reachable");
    log_event(pool, "system", None, "relay_health_check", Some("ok")).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::queries::{insert_agent, insert_session};
    use crate::db::test_helpers::create_test_pool;
    use crate::types::{Agent, Session};
    use chrono::Utc;

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
            pane_id: Some(format!("{}-pane", id)),
            worktree_path: format!("/tmp/{}", id),
            branch: "main".to_string(),
            agent_kind: "claude".to_string(),
            status: "idle".to_string(),
            last_seen_at: Some(Utc::now()),
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_missing_iterm_session_flags_temporal_recovery() {
        let (pool, _dir) = create_test_pool().await;
        insert_session(&pool, &make_session("sess-process-1"))
            .await
            .unwrap();
        let agent = make_agent("agent-process-1", "sess-process-1");
        insert_agent(&pool, &agent).await.unwrap();

        check_agent_processes(&pool, "sess-process-1", std::slice::from_ref(&agent))
            .await
            .unwrap();

        let recovery_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE event_type = 'temporal_recovery_required' AND payload LIKE '%missing_iterm_session for agent-process-1%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(recovery_count, 1);
    }
}
