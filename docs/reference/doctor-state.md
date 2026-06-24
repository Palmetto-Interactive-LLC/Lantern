# Doctor-State Planning

`lantern doctor` currently reports process and dependency health. Issue #31 tracks a follow-up diagnostic model, referred to here as doctor-state, that can summarize runtime readiness without making local SQLite or terminals authoritative.

## Goal

Doctor-state should answer: "Can this machine safely launch and observe a squad, and what runtime state does Temporal currently own?"

The answer should combine:

- dependency health from local checks
- Temporal workflow state from Queries
- iTerm display-target inventory
- runner and MCP readiness
- quarantine and migration warnings
- audit projection freshness

## Authority Rules

| Data | Source of truth |
|------|-----------------|
| Session lifecycle | `SessionLifecycleWorkflow` |
| Setup readiness | `SessionSetupWorkflow` |
| Message routing and delivery | `SessionMessageBusWorkflow` and `MessageDeliveryWorkflow` |
| Per-role execution readiness | `ExecutionWindowWorkflow` |
| Runner heartbeat/lease | `RunnerLeaseWorkflow` |
| MCP readiness/recovery | `McpSetupWorkflow` and `McpRecoveryWorkflow` |
| Human control | `HumanControlWorkflow` |
| Local display inventory | iTerm target projection in SQLite |
| Quarantine warnings | `terminal_target_quarantine` and migration audit rows |

SQLite can cache projections for display, but doctor-state must distinguish projected local data from queried workflow state.

## Planned Sections

### Dependencies

- Temporal CLI installed and connected
- Temporal dev server reachable
- Relay process or launchd service present
- git available
- iTerm2 installed and Python API reachable on macOS
- `agent-runner` available
- Orchestration client configured for each agent CLI

The shell doctor checks iTerm, runner, and MCP readiness. Legacy tmux binary checks are intentionally excluded from current runtime diagnostics.

### Workflow State

For each active session, doctor-state should query workflow state and show:

- lifecycle status
- setup barriers: worktree, iTerm, runner, MCP
- active roles and availability
- message bus backlog and rejected count
- delivery states that are degraded, retrying, or dead-lettered
- human-control commands in effect

### Local Projection State

Local projection output should show:

- registered iTerm session IDs by role
- terminal target transport status
- projection age
- recent audit events
- quarantine row count and reasons

Projection rows are diagnostic only. They must not be used to route runtime messages.

### Quarantine

Legacy terminal target rows that mention tmux, direct injection methods, or non-iTerm windows belong in quarantine output only. They can explain old logs and cleanup needs, but must not be repaired into active delivery targets.

## Suggested Status Levels

| Level | Meaning |
|-------|---------|
| `OK` | Dependency or workflow state is ready |
| `WARN` | Launch or observation may work, but a stale projection, missing optional tool, or degraded workflow exists |
| `FAIL` | Required dependency or workflow state is missing |
| `LEGACY` | Historical tmux/local-queue/Rust-MCP data found; diagnostic only |

## Related

- [Temporal runtime control plane ADR](../architecture/adr-0001-temporal-runtime-control-plane.md)
- [MCP runtime authority](mcp-tools.md)
- [Database schema](database-schema.md)
- [Paths and environment](paths-and-environment.md)
