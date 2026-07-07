use crate::types::*;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

pub async fn insert_machine(pool: &SqlitePool, machine_id: &str) -> anyhow::Result<()> {
    sqlx::query("INSERT OR IGNORE INTO machines (id, created_at) VALUES (?, ?)")
        .bind(machine_id)
        .bind(Utc::now().to_rfc3339())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn insert_session(pool: &SqlitePool, session: &Session) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO sessions (id, machine_id, project_slug, slot_number, status, created_at) VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(&session.id)
    .bind(&session.machine_id)
    .bind(&session.project_slug)
    .bind(session.slot_number)
    .bind(&session.status)
    .bind(session.created_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_agent(pool: &SqlitePool, agent: &Agent) -> anyhow::Result<()> {
    // OR IGNORE: agents has a UNIQUE(session_id, role) lease key (migration 006).
    // startwork seeds the row; the per-pane runner later claims it (sets
    // runner_pid) via UPDATE. A stale row from a prior run must not crash
    // startwork — the runner reconciles it.
    sqlx::query(
        "INSERT OR IGNORE INTO agents (id, session_id, role, pane_id, worktree_path, branch, agent_kind, status, last_seen_at, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&agent.id)
    .bind(&agent.session_id)
    .bind(&agent.role)
    .bind(&agent.pane_id)
    .bind(&agent.worktree_path)
    .bind(&agent.branch)
    .bind(&agent.agent_kind)
    .bind(&agent.status)
    .bind(agent.last_seen_at.map(|t| t.to_rfc3339()))
    .bind(agent.created_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_terminal_target(
    pool: &SqlitePool,
    target: &TerminalTarget,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT OR REPLACE INTO terminal_targets (agent_id, iterm_session_id, pane_id, transport_status, last_seen_at) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(&target.agent_id)
    .bind(&target.iterm_session_id)
    .bind(&target.pane_id)
    .bind(&target.transport_status)
    .bind(target.last_seen_at.map(|t| t.to_rfc3339()))
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_acknowledgement(
    pool: &SqlitePool,
    ack: &Acknowledgement,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO acknowledgements (id, work_item_id, agent_id, ack_type, generation, received_at) VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(&ack.id)
    .bind(&ack.work_item_id)
    .bind(&ack.agent_id)
    .bind(&ack.ack_type)
    .bind(ack.generation)
    .bind(ack.received_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn log_event(
    pool: &SqlitePool,
    session_id: &str,
    agent_id: Option<&str>,
    event_type: &str,
    payload: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO events (session_id, agent_id, event_type, payload, created_at) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(session_id)
    .bind(agent_id)
    .bind(event_type)
    .bind(payload)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_agents_for_session(
    pool: &SqlitePool,
    session_id: &str,
) -> anyhow::Result<Vec<Agent>> {
    let rows = sqlx::query_as::<_, Agent>(
        "SELECT id, session_id, role, pane_id, worktree_path, branch, agent_kind, status, last_seen_at, created_at FROM agents WHERE session_id = ?"
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_agent_by_id(pool: &SqlitePool, agent_id: &str) -> anyhow::Result<Option<Agent>> {
    let row = sqlx::query_as::<_, Agent>(
        "SELECT id, session_id, role, pane_id, worktree_path, branch, agent_kind, status, last_seen_at, created_at FROM agents WHERE id = ?"
    )
    .bind(agent_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn update_agent_status(
    pool: &SqlitePool,
    agent_id: &str,
    status: &str,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE agents SET status = ?, last_seen_at = ? WHERE id = ?")
        .bind(status)
        .bind(Utc::now().to_rfc3339())
        .bind(agent_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_session_status(
    pool: &SqlitePool,
    session_id: &str,
    status: &str,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE sessions SET status = ? WHERE id = ?")
        .bind(status)
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_terminal_target_status(
    pool: &SqlitePool,
    agent_id: &str,
    transport_status: &str,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE terminal_targets SET transport_status = ? WHERE agent_id = ?")
        .bind(transport_status)
        .bind(agent_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn is_terminal_target_quarantined(
    pool: &SqlitePool,
    agent_id: &str,
) -> anyhow::Result<bool> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(1) FROM terminal_target_quarantine WHERE agent_id = ?")
            .bind(agent_id)
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}

pub struct QuarantineParams<'a> {
    pub agent_id: &'a str,
    pub legacy_tmux_session: &'a str,
    pub legacy_tmux_window: &'a str,
    pub legacy_tmux_pane: &'a str,
    pub legacy_inject_method: &'a str,
    pub legacy_last_injected_at: Option<&'a str>,
    pub quarantine_reason: &'a str,
}

pub async fn insert_terminal_target_quarantine(
    pool: &SqlitePool,
    params: QuarantineParams<'_>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO terminal_target_quarantine (agent_id, legacy_tmux_session, legacy_tmux_window, legacy_tmux_pane, legacy_inject_method, legacy_last_injected_at, quarantine_reason, quarantined_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(params.agent_id)
    .bind(params.legacy_tmux_session)
    .bind(params.legacy_tmux_window)
    .bind(params.legacy_tmux_pane)
    .bind(params.legacy_inject_method)
    .bind(params.legacy_last_injected_at)
    .bind(params.quarantine_reason)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update_work_item_status(
    pool: &SqlitePool,
    work_item_id: &str,
    status: &str,
) -> anyhow::Result<()> {
    let now = Utc::now().to_rfc3339();
    let query = match status {
        "acked" => "UPDATE work_items SET status = ?, accepted_at = ? WHERE id = ?",
        "accepted" | "done_claimed" | "stale" | "cancelled" => {
            "UPDATE work_items SET status = ?, completed_at = ? WHERE id = ?"
        }
        _ => "UPDATE work_items SET status = ? WHERE id = ?",
    };

    if status == "acked"
        || status == "accepted"
        || status == "done_claimed"
        || status == "stale"
        || status == "cancelled"
    {
        sqlx::query(query)
            .bind(status)
            .bind(now)
            .bind(work_item_id)
            .execute(pool)
            .await?;
    } else {
        sqlx::query(query)
            .bind(status)
            .bind(work_item_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn get_stale_leases(pool: &SqlitePool) -> anyhow::Result<Vec<Lease>> {
    let now = Utc::now().to_rfc3339();
    let rows = sqlx::query_as::<_, Lease>(
        "SELECT id, work_item_id, agent_id, generation, expires_at, created_at FROM leases WHERE expires_at < ?"
    )
    .bind(now)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_active_sessions(pool: &SqlitePool) -> anyhow::Result<Vec<String>> {
    let rows = sqlx::query_scalar("SELECT id FROM sessions WHERE status = 'active'")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn get_session(pool: &SqlitePool, session_id: &str) -> anyhow::Result<Option<Session>> {
    let row = sqlx::query_as::<_, Session>(
        "SELECT id, machine_id, project_slug, slot_number, status, created_at FROM sessions WHERE id = ?"
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_all_sessions(pool: &SqlitePool) -> anyhow::Result<Vec<Session>> {
    let rows = sqlx::query_as::<_, Session>(
        "SELECT id, machine_id, project_slug, slot_number, status, created_at FROM sessions ORDER BY created_at DESC"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn count_agents_by_session(pool: &SqlitePool, session_id: &str) -> anyhow::Result<i64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agents WHERE session_id = ?")
        .bind(session_id)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

pub async fn count_work_items_by_status(
    pool: &SqlitePool,
    session_id: &str,
    status: &str,
) -> anyhow::Result<i64> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM work_items WHERE session_id = ? AND status = ?")
            .bind(session_id)
            .bind(status)
            .fetch_one(pool)
            .await?;
    Ok(count)
}

pub async fn get_recent_events(pool: &SqlitePool, limit: i64) -> anyhow::Result<Vec<Event>> {
    let rows = sqlx::query_as::<_, Event>(
        "SELECT id, session_id, agent_id, event_type, payload, created_at FROM events ORDER BY created_at DESC LIMIT ?"
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_terminal_target(
    pool: &SqlitePool,
    agent_id: &str,
) -> anyhow::Result<Option<TerminalTarget>> {
    let row = sqlx::query_as::<_, TerminalTarget>(
        "SELECT agent_id, iterm_session_id, pane_id, transport_status, last_seen_at FROM terminal_targets WHERE agent_id = ?"
    )
    .bind(agent_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn delete_leases_by_session_agents(
    pool: &SqlitePool,
    session_id: &str,
) -> anyhow::Result<u64> {
    let agents = get_agents_for_session(pool, session_id).await?;

    let mut total_removed = 0u64;
    for agent in agents {
        let result = sqlx::query("DELETE FROM leases WHERE agent_id = ?")
            .bind(agent.id)
            .execute(pool)
            .await?;
        total_removed += result.rows_affected();
    }

    Ok(total_removed)
}

// ── Blackboard discovery cards (migration 007) ────────────────────────────────

/// A shared insight posted by a worker on complete/blocked/failed and read back
/// into every dispatch prompt for the session.
#[derive(Debug, Clone)]
pub struct DiscoveryCard {
    pub card_id: String,
    pub role: String,
    pub category: String,
    pub summary: String,
    pub solution: String,
}

/// Insert a discovery card, deduped by `card_id` (INSERT OR IGNORE mirrors the
/// TS blackboard's `cards.some(c => c.card_id === card.card_id)` guard).
pub async fn insert_discovery_card(
    pool: &SqlitePool,
    session_id: &str,
    card: &DiscoveryCard,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO discovery_cards \
         (card_id, session_id, role, category, summary, solution, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&card.card_id)
    .bind(session_id)
    .bind(&card.role)
    .bind(&card.category)
    .bind(&card.summary)
    .bind(&card.solution)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// List all discovery cards for a session, oldest first (dispatch-prompt order).
pub async fn list_discovery_cards(
    pool: &SqlitePool,
    session_id: &str,
) -> anyhow::Result<Vec<DiscoveryCard>> {
    let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT card_id, role, category, summary, solution \
         FROM discovery_cards WHERE session_id = ? ORDER BY created_at ASC",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(card_id, role, category, summary, solution)| DiscoveryCard {
                card_id,
                role,
                category,
                summary,
                solution,
            },
        )
        .collect())
}

// ── MCP event subscriptions (migration 007) ───────────────────────────────────

/// Register a role's subscription to an event type for a session (idempotent).
pub async fn add_subscription(
    pool: &SqlitePool,
    session_id: &str,
    event_type: &str,
    role: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO mcp_subscriptions (session_id, event_type, role) VALUES (?, ?, ?)",
    )
    .bind(session_id)
    .bind(event_type)
    .bind(role)
    .execute(pool)
    .await?;
    Ok(())
}

/// List the roles subscribed to `event_type` in `session_id`.
pub async fn list_subscribed_roles(
    pool: &SqlitePool,
    session_id: &str,
    event_type: &str,
) -> anyhow::Result<Vec<String>> {
    let roles: Vec<String> = sqlx::query_scalar(
        "SELECT role FROM mcp_subscriptions WHERE session_id = ? AND event_type = ?",
    )
    .bind(session_id)
    .bind(event_type)
    .fetch_all(pool)
    .await?;
    Ok(roles)
}

// ── Nudge clock + active-task tracking (migration 007) ─────────────────────────

/// Advance an agent's last_signal_at (silence clock reset) and mark it busy on a
/// given active task. Called on dispatch / peer / transition — NOT heartbeat.
/// `active_task: None` leaves the existing active_task untouched.
pub async fn mark_agent_signal(
    pool: &SqlitePool,
    agent_id: &str,
    active_task: Option<&str>,
    busy: bool,
) -> anyhow::Result<()> {
    let now = Utc::now().to_rfc3339();
    match active_task {
        Some(task) => {
            sqlx::query(
                "UPDATE agents SET last_signal_at = ?, busy = ?, active_task = ? WHERE id = ?",
            )
            .bind(&now)
            .bind(busy as i64)
            .bind(task)
            .bind(agent_id)
            .execute(pool)
            .await?;
        }
        None => {
            sqlx::query("UPDATE agents SET last_signal_at = ?, busy = ? WHERE id = ?")
                .bind(&now)
                .bind(busy as i64)
                .bind(agent_id)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

/// Clear an agent's busy/active-task state (e.g. on complete/failed).
pub async fn clear_agent_active_task(pool: &SqlitePool, agent_id: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE agents SET busy = 0, active_task = NULL WHERE id = ?")
        .bind(agent_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// A busy agent the nudge loop may need to poke. Times are raw RFC3339 strings.
pub struct NudgeCandidate {
    pub agent_id: String,
    pub session_id: String,
    pub role: String,
    pub active_task: String,
    pub last_signal_at: Option<DateTime<Utc>>,
    pub last_nudge_at: Option<DateTime<Utc>>,
}

type NudgeCandidateRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);

/// All agents that are busy with an active task — candidates for the nudge scan.
pub async fn get_nudge_candidates(pool: &SqlitePool) -> anyhow::Result<Vec<NudgeCandidate>> {
    let rows: Vec<NudgeCandidateRow> = sqlx::query_as(
        "SELECT id, session_id, role, active_task, last_signal_at, last_nudge_at \
         FROM agents WHERE busy = 1 AND active_task IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(agent_id, session_id, role, active_task, last_signal_at, last_nudge_at)| {
                NudgeCandidate {
                    agent_id,
                    session_id,
                    role,
                    active_task,
                    last_signal_at: last_signal_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|t| t.with_timezone(&Utc)),
                    last_nudge_at: last_nudge_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|t| t.with_timezone(&Utc)),
                }
            },
        )
        .collect())
}

/// Stamp last_nudge_at = now for an agent (nudge throttle).
pub async fn mark_agent_nudged(pool: &SqlitePool, agent_id: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE agents SET last_nudge_at = ? WHERE id = ?")
        .bind(Utc::now().to_rfc3339())
        .bind(agent_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::create_test_pool;
    use chrono::Utc;

    fn make_session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            machine_id: "machine-1".to_string(),
            project_slug: "test-project".to_string(),
            slot_number: 1,
            status: "active".to_string(),
            created_at: Utc::now(),
        }
    }

    fn make_agent(id: &str, session_id: &str, role: &str, status: &str) -> Agent {
        Agent {
            id: id.to_string(),
            session_id: session_id.to_string(),
            role: role.to_string(),
            pane_id: Some(format!("%{}-pane", id)),
            worktree_path: format!("/tmp/{}-wt", id),
            branch: format!("branch-{}", id),
            agent_kind: "claude".to_string(),
            status: status.to_string(),
            last_seen_at: Some(Utc::now()),
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_insert_and_get_machine() {
        let (pool, _dir) = create_test_pool().await;
        insert_machine(&pool, "machine-test").await.unwrap();
        // machines table has no get_by_id query, but insert should succeed
    }

    #[tokio::test]
    async fn test_insert_and_get_session() {
        let (pool, _dir) = create_test_pool().await;
        let session = make_session("sess-1");
        insert_session(&pool, &session).await.unwrap();

        let active = get_active_sessions(&pool).await.unwrap();
        assert!(active.contains(&"sess-1".to_string()));
    }

    #[tokio::test]
    async fn test_get_session_by_id() {
        let (pool, _dir) = create_test_pool().await;
        let session = make_session("sess-2");
        insert_session(&pool, &session).await.unwrap();

        let fetched = get_session(&pool, "sess-2").await.unwrap();
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.id, "sess-2");
        assert_eq!(fetched.status, "active");
    }

    #[tokio::test]
    async fn test_insert_and_get_agent() {
        let (pool, _dir) = create_test_pool().await;
        let session = make_session("sess-1");
        insert_session(&pool, &session).await.unwrap();

        let agent = make_agent("agent-1", "sess-1", "ai", "idle");
        insert_agent(&pool, &agent).await.unwrap();

        let fetched = get_agent_by_id(&pool, "agent-1").await.unwrap();
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.id, "agent-1");
        assert_eq!(fetched.role, "ai");
    }

    #[tokio::test]
    async fn test_update_agent_status() {
        let (pool, _dir) = create_test_pool().await;
        let session = make_session("sess-1");
        insert_session(&pool, &session).await.unwrap();

        let agent = make_agent("agent-1", "sess-1", "ai", "idle");
        insert_agent(&pool, &agent).await.unwrap();

        update_agent_status(&pool, "agent-1", "busy").await.unwrap();

        let fetched = get_agent_by_id(&pool, "agent-1").await.unwrap().unwrap();
        assert_eq!(fetched.status, "busy");
    }

    /// Verifies that agent status is correctly filtered by role when agents have different statuses.
    /// This exercises get_agents_for_session, which the supervisor and MCP tools depend on.
    #[tokio::test]
    async fn test_get_agents_for_session_returns_all_roles() {
        let (pool, _dir) = create_test_pool().await;
        let session = make_session("sess-1");
        insert_session(&pool, &session).await.unwrap();

        let agent1 = make_agent("agent-1", "sess-1", "ai", "idle");
        let agent2 = make_agent("agent-2", "sess-1", "dat", "busy");
        insert_agent(&pool, &agent1).await.unwrap();
        insert_agent(&pool, &agent2).await.unwrap();

        let agents = get_agents_for_session(&pool, "sess-1").await.unwrap();
        assert_eq!(agents.len(), 2);
        let roles: Vec<&str> = agents.iter().map(|a| a.role.as_str()).collect();
        assert!(roles.contains(&"ai"));
        assert!(roles.contains(&"dat"));
    }

    /// get_stale_leases returns only leases whose expires_at is in the past.
    /// This is the production code path used by delivery::stale::check_stale_assignments.
    #[tokio::test]
    async fn test_get_stale_leases_empty_when_none_expired() {
        let (pool, _dir) = create_test_pool().await;
        // Insert a future-expiring lease using RFC3339 so it matches the query format.
        let future = (Utc::now() + chrono::Duration::minutes(5)).to_rfc3339();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO leases (id, work_item_id, agent_id, generation, expires_at, created_at) \
             VALUES ('lease-fresh', 'wi-x', 'agent-x', 1, ?, ?)",
        )
        .bind(&future)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let stale = get_stale_leases(&pool).await.unwrap();
        assert!(
            stale.is_empty(),
            "future lease must not be flagged as stale"
        );
    }

    /// A lease with expires_at in the past is returned by get_stale_leases.
    #[tokio::test]
    async fn test_get_stale_leases_returns_expired() {
        let (pool, _dir) = create_test_pool().await;
        let past_expires = (Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        let past_created = (Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        sqlx::query(
            "INSERT INTO leases (id, work_item_id, agent_id, generation, expires_at, created_at) \
             VALUES ('lease-old', 'wi-y', 'agent-y', 1, ?, ?)",
        )
        .bind(&past_expires)
        .bind(&past_created)
        .execute(&pool)
        .await
        .unwrap();

        let stale = get_stale_leases(&pool).await.unwrap();
        assert_eq!(stale.len(), 1, "expired lease must appear in stale results");
        assert_eq!(stale[0].id, "lease-old");
    }

    #[tokio::test]
    async fn test_log_event() {
        let (pool, _dir) = create_test_pool().await;
        log_event(
            &pool,
            "sess-1",
            Some("agent-1"),
            "test_event",
            Some("payload"),
        )
        .await
        .unwrap();
        // Should not panic
    }

    /// Verifies the QuarantineParams struct wires correctly to the INSERT.
    #[tokio::test]
    async fn test_insert_terminal_target_quarantine_via_params() {
        let (pool, _dir) = create_test_pool().await;
        let session = make_session("sess-q");
        insert_session(&pool, &session).await.unwrap();

        let agent = make_agent("agent-q", "sess-q", "ai", "idle");
        insert_agent(&pool, &agent).await.unwrap();

        // Insert a terminal_target so the foreign key (if enforced) is satisfied.
        sqlx::query(
            "INSERT INTO terminal_targets (agent_id, iterm_session_id, transport_status) VALUES (?, 'iterm-q', 'ready')"
        )
        .bind(&agent.id)
        .execute(&pool)
        .await
        .unwrap();

        insert_terminal_target_quarantine(
            &pool,
            QuarantineParams {
                agent_id: &agent.id,
                legacy_tmux_session: "tmux-q",
                legacy_tmux_window: "win-0",
                legacy_tmux_pane: "%0",
                legacy_inject_method: "write_keys",
                legacy_last_injected_at: None,
                quarantine_reason: "test quarantine",
            },
        )
        .await
        .unwrap();

        let quarantined = is_terminal_target_quarantined(&pool, &agent.id)
            .await
            .unwrap();
        assert!(
            quarantined,
            "agent must be flagged as quarantined after insert"
        );
    }
}
