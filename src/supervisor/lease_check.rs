use anyhow::Result;
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::db::queries::{get_agent_by_id, get_stale_leases, log_event};

/// Query leases past expiry and trigger escalation.
pub async fn check_stale_leases(pool: &SqlitePool) -> Result<()> {
    let stale_leases = get_stale_leases(pool).await?;

    if !stale_leases.is_empty() {
        info!(count = stale_leases.len(), "found stale leases");
    }

    for lease in stale_leases {
        warn!(
            lease_id = %lease.id,
            agent_id = %lease.agent_id,
            work_item_id = %lease.work_item_id,
            "stale lease detected"
        );

        let agent = get_agent_by_id(pool, &lease.agent_id).await?;
        let session_id = agent
            .map(|a| a.session_id)
            .unwrap_or_else(|| "system".to_string());

        log_event(
            pool,
            &session_id,
            Some(&lease.agent_id),
            "stale_lease",
            Some(&lease.work_item_id),
        )
        .await?;

        let payload = format!(
            "stale_runner_lease work_item_id={} agent_id={} generation={}",
            lease.work_item_id, lease.agent_id, lease.generation
        );
        log_event(
            pool,
            &session_id,
            Some(&lease.agent_id),
            "temporal_recovery_required",
            Some(&payload),
        )
        .await?;

        // Local process-side recovery is now audit-only. Execution recovery is coordinated
        // by Temporal workflows using this recovery event as signal material.
        info!(
            lease_id = %lease.id,
            session_id = %session_id,
            "emitted temporal_recovery_required for stale lease"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::queries::{insert_agent, insert_session};
    use crate::db::test_helpers::create_test_pool;
    use crate::types::{Agent, Lease, Session};
    use chrono::{Duration, Utc};

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

    fn make_lease(id: &str, agent_id: &str, work_item_id: &str) -> Lease {
        Lease {
            id: id.to_string(),
            work_item_id: work_item_id.to_string(),
            agent_id: agent_id.to_string(),
            generation: 1,
            expires_at: Utc::now() - Duration::minutes(10),
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn stale_lease_generates_temporal_recovery_event() {
        let (pool, _dir) = create_test_pool().await;
        insert_session(&pool, &make_session("sess-stale-1"))
            .await
            .unwrap();
        insert_agent(&pool, &make_agent("agent-stale", "sess-stale-1"))
            .await
            .unwrap();

        let lease = make_lease("lease-stale", "agent-stale", "wi-stale");
        sqlx::query(
            "INSERT INTO leases (id, work_item_id, agent_id, generation, expires_at, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&lease.id)
        .bind(&lease.work_item_id)
        .bind(&lease.agent_id)
        .bind(lease.generation)
        .bind((lease.expires_at).to_rfc3339())
        .bind(lease.created_at.to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();

        check_stale_leases(&pool).await.unwrap();

        let has_stale = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM events WHERE event_type = 'stale_lease'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(has_stale, 1);

        let has_recovery = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM events WHERE event_type = 'temporal_recovery_required'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(has_recovery >= 1);

        let payload: String = sqlx::query_scalar(
            "SELECT payload FROM events WHERE event_type = 'temporal_recovery_required' ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(payload.contains("stale_runner_lease"));
    }
}
