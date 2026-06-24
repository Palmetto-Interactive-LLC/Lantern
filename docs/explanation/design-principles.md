# Design Principles

Why Lantern is built this way.

## Temporal Owns Runtime Truth

Temporal workflows decide what runtime work exists, whether a message is accepted or rejected, when delivery starts, whether an agent acknowledged it, and which human-control commands are in effect.

Lantern must not recreate a second runtime control plane in SQLite, terminal state, or Rust MCP handlers.

## Local Tools Support Execution

Lantern still owns local workstation support:

- create and remove git worktrees
- open and close iTerm2 display windows
- launch agent processes
- record local inventory and audit projections
- surface diagnostics and quarantine state

Those local surfaces help people observe and operate the system. They do not decide message routing or delivery state.

## iTerm Is Display-Only

iTerm2 is useful because operators need visible agent windows. It is not a message bus, delivery fallback, or reconciliation authority.

Runtime delivery should target agent-runner-owned execution windows gated by Temporal workflow state. If iTerm target data is stale, doctor-state should report it as diagnostic information.

## MCP Runtime Uses the Orchestration Layer

Agents should use the orchestration client for runtime MCP operations. That client maps status updates, peer messages, setup instructions, team-state reads, and human-control operations to Temporal workflow Updates, Signals, and Queries.

Lantern Rust MCP remains compatibility-only and must not expose local runtime tools.

## SQLite Is Projection and Quarantine

SQLite stores:

- local machine and session inventory
- agent and iTerm target projections
- audit events
- quarantine rows for legacy terminal targets
- future doctor-state snapshots

SQLite is not the authoritative queue, lease table, status store, or delivery router.

## Agents Are Not Trusted

Agent reports are claims. Temporal workflows validate them against accepted messages, runner leases, MCP readiness, delivery state, and human-control state.

This keeps a recovered or stale agent from corrupting runtime state with old assumptions.

## Human Control Is Workflow State

Operators can pause, resume, take over, release, note, and recover. Those commands should be represented in `HumanControlWorkflow` so the rest of the runtime can see and honor them.

Direct terminal injection is legacy-only and should not be used as a control-plane shortcut.

## Legacy Data Stays Quarantined

Old tmux terminal targets, local queue rows, and Lantern Rust MCP runtime tool references can explain old logs or migrations. They must be labeled legacy or quarantine-only and must not be reconciled into active runtime delivery.

## See Also

- [Architecture](architecture.md)
- [Temporal runtime control plane ADR](../architecture/adr-0001-temporal-runtime-control-plane.md)
- [Doctor-state planning](../reference/doctor-state.md)
