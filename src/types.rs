use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Session {
    pub id: String,
    pub machine_id: String,
    pub project_slug: String,
    pub slot_number: i64,
    pub status: String,
    pub created_at: DateTime<Utc>,
    /// Launch pattern this session was started with: "team" | "executor" | "simple" | "fixbug".
    /// See migration 008_pattern_column.sql.
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Agent {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub pane_id: Option<String>,
    pub worktree_path: String,
    pub branch: String,
    pub agent_kind: String,
    pub status: String,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TerminalTarget {
    pub agent_id: String,
    pub iterm_session_id: String,
    pub pane_id: Option<String>,
    pub transport_status: String,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Lease {
    pub id: String,
    pub work_item_id: String,
    pub agent_id: String,
    pub generation: i64,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Acknowledgement {
    pub id: String,
    pub work_item_id: String,
    pub agent_id: String,
    pub ack_type: String,
    pub generation: i64,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Event {
    pub id: i64,
    pub session_id: String,
    pub agent_id: Option<String>,
    pub event_type: String,
    pub payload: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub assignment_id: String,
    pub task_id: String,
    pub session_id: String,
    pub target_role: String,
    pub summary: String,
    pub files: Vec<String>,
    pub next_action: Option<String>,
    pub priority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusReport {
    pub session: String,
    pub role: String,
    pub status: String,
    pub task_id: Option<String>,
    pub summary: Option<String>,
    pub validation: Option<String>,
    pub next_action: Option<String>,
    pub assignment_id: Option<String>,
    pub generation: Option<i64>,
    // Scope fields — used by enforce_scope but ignored by tools logic
    pub team_id: Option<String>,
    pub temporal_namespace: Option<String>,
    pub task_queue: Option<String>,
    pub repo_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMessage {
    pub session: String,
    pub from_role: String,
    pub to_role: String,
    pub task_id: Option<String>,
    pub info: String,
    pub requested_action: Option<String>,
    // Scope fields
    pub team_id: Option<String>,
    pub temporal_namespace: Option<String>,
    pub task_queue: Option<String>,
    pub repo_id: Option<String>,
}

pub fn generate_id(prefix: &str) -> String {
    format!("{}-{}", prefix, Uuid::new_v4().as_simple())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_id_format() {
        let id = generate_id("test");
        assert!(id.starts_with("test-"));
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert!(!parts[1].is_empty());
    }

    #[test]
    fn test_generate_id_uniqueness() {
        let id1 = generate_id("agent");
        let id2 = generate_id("agent");
        assert_ne!(id1, id2);
    }
}
