-- Runner-lease support for the self-contained agent-runner port (Option C-sqlite).
-- Lets the local agent-runner hold its process lease and record runs/signals in
-- lantern's SQLite DB instead of a remote Postgres. Local-only; no remote deps.

-- Process-level lease tracking on the existing per-(session,role) agent row.
ALTER TABLE agents ADD COLUMN runner_pid INTEGER;
ALTER TABLE agents ADD COLUMN last_heartbeat_at TEXT;

-- Denormalized pointer to the session's current run.
ALTER TABLE sessions ADD COLUMN current_run_id TEXT;

-- Immutable run record: one per startwork invocation.
CREATE TABLE IF NOT EXISTS runs (
    run_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    temporal_workflow_id TEXT NOT NULL,
    agent_kind TEXT NOT NULL,
    repo_head_sha TEXT NOT NULL DEFAULT 'unknown',
    started_at TEXT NOT NULL,
    ended_at TEXT,
    end_reason TEXT CHECK (end_reason IN ('clean', 'killed', 'crashed', 'orphaned'))
);

-- Signal audit trail (written by the persistence-shadow path).
CREATE TABLE IF NOT EXISTS signals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL,
    role TEXT NOT NULL,
    task_id TEXT,
    status TEXT NOT NULL,
    summary TEXT,
    files_changed TEXT NOT NULL DEFAULT '[]',
    validation_performed TEXT,
    blockers TEXT NOT NULL DEFAULT '[]',
    recommended_next_action TEXT,
    source TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_runs_session ON runs(session_id);
CREATE INDEX IF NOT EXISTS idx_agents_runner_pid ON agents(runner_pid);
CREATE INDEX IF NOT EXISTS idx_signals_run_id ON signals(run_id);
CREATE INDEX IF NOT EXISTS idx_signals_created_at ON signals(created_at);
