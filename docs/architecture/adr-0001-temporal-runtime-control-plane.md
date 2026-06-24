# ADR 0001: Self-Contained Lantern with Local Temporal Runtime

Status: Accepted

## Context

Lantern is being restored to a **self-contained architecture** where each developer machine runs its own complete instance. Lantern must:

1. Serve MCP tools directly to agents
2. Own the Temporal client and workflow integration
3. Drive all runtime decisions through local Temporal (127.0.0.1:8243, Docker strictly unsupported)
4. Use SQLite for audit projections and diagnostics only
5. Be deployable standalone with no remote server dependency

The previous distributed model split runtime authority across a remote control plane. That approach required agents to route through an external MCP client, forcing workflow definitions and Temporal state off-machine. The restoration returns to a self-contained per-machine model.

## Decision

**Lantern is the complete local agent.** It is Rust binary that runs on each machine and provides:

1. **MCP server**: Listens on stdin/stdout and exposes runtime tools to agents
   - `devorch_report_status` - agent readiness reports
   - `devorch_peer_message` - inter-agent messaging
   - `devorch_query_team_state` - session/agent state queries
   - `devorch_get_setup_instructions` - setup guidance
   - Human control (pause, resume, takeover, release, recover, note)

2. **Temporal client**: Integrates with local Temporal instance
   - All tool calls route through Temporal Updates/Signals/Queries
   - Workflow state drives all runtime decisions
   - Temporal history provides durability across restarts

3. **Local runner**: Manages execution surfaces
   - Creates and manages git worktrees
   - Launches iTerm2 panes for agents
   - Integrates with `agent-runner` for process execution
   - Updates SQLite projections for diagnostics

### Workflow Ownership

All runtime authority lives in **local** Temporal workflows:

- `SessionLifecycleWorkflow` owns session start/stop
- `SessionSetupWorkflow` owns setup readiness barriers
- `SessionStateWorkflow` owns queryable role/task/control state
- `SessionMessageBusWorkflow` owns message routing
- `MessageDeliveryWorkflow` owns retries, acks, recovery
- `ExecutionWindowWorkflow` owns per-role delivery readiness
- `RunnerLeaseWorkflow` owns runner heartbeats and leases
- `McpSetupWorkflow` and `McpRecoveryWorkflow` own MCP readiness
- `HumanControlWorkflow` owns pause/resume/takeover control
- `MessageCompressionWorkflow` owns message history compaction
- `AuditShadowWorkflow` projects Temporal state to SQLite (diagnostic only)

### MCP Server Implementation

The MCP server runs as part of the `lantern relay` daemon:

```
lantern relay
  ├── stdin/stdout JSON-RPC MCP server
  ├── Temporal client (connects to 127.0.0.1:8243)
  ├── SQLite connection pool
  ├── iTerm2 integration (launch/manage panes)
  └── Projection updates (audit, inventory, terminal targets)
```

Tool calls from agents:
- Are received by the MCP server
- Route to appropriate Temporal Update/Signal/Query
- Response is returned to the agent
- Side effects (SQLite audit, iTerm display) are asynchronous

### Local Temporal Instance

Each machine runs a local Temporal server (dev mode):

```bash
temporal server start-dev --db-filename ~/.local/temporal/development.db
```

- **No remote dependency**: Temporal lives on the machine
- **Recoverable**: Persisted to SQLite on disk
- **Scoped**: Workflows include hard machine/repo/session identifiers to prevent cross-session leakage

### SQLite Role

SQLite is inventory and audit only:

- Session and agent inventory
- iTerm terminal target projections
- Audit events (sourced from Temporal projection workflow)
- Worktree snapshots
- Doctor-state snapshots

SQLite must **never**:
- Route messages or decide delivery order
- Lease work or track runtime state
- Act as a fallback if Temporal is down

### iTerm2 Role

iTerm2 is display and pane-lifecycle support:

- Lantern launches iTerm panes for agents
- Lantern registers pane IDs in SQLite for recovery
- iTerm shows output from agents running in `agent-runner` execution windows
- Direct pane injection is not used

iTerm state (open/closed panes) is not authoritative for runtime control. Temporal workflow state is.

## Consequences

1. **Durability**: Runtime message acceptance, delivery, acknowledgement, pause/resume, recovery all survive relay and worker restarts through Temporal history
2. **Simplicity**: No remote server, remote Postgres, or distributed coordination needed
3. **Observability**: `lantern status` and `lantern doctor` query local Temporal and SQLite
4. **Deployability**: Copy the binary, run `lantern install`, each machine is ready
5. **Stale-state cleanup**: Old tmux/queue/lease rows are migration data only; must not be reconciled into active delivery

## Deprecated: Distributed Model (2025)

The previous architecture (2025) split runtime authority:

- The external orchestration client was the MCP authority
- Workflows lived in the external workflow package on a remote server
- Lantern Rust MCP served empty tools list
- Remote Postgres held workflow state
- iTerm2 was display-only with no delivery participation

**This is fully deprecated.** All references to external MCP clients, remote Temporal, remote Postgres, "Lantern MCP disabled", and split-runtime ownership are legacy history only.

## Doctor-State Evolution

`lantern doctor` should evolve to combine:

- Dependency checks (Temporal, SQLite, agent-runner)
- Temporal workflow Queries (session state, delivery status, recovery status)
- iTerm target readiness checks
- Runner/MCP readiness (via agent-runner heartbeat)
- Quarantine row inspection
- Projection freshness (time since last audit update)

Doctor-state is diagnostic output only; it does not make any local component authoritative over Temporal.

## References

- [Architecture](../explanation/architecture.md)
- [MCP tools](../reference/mcp-tools.md)
- [Doctor-state planning](../reference/doctor-state.md)
- Temporal TypeScript message passing: https://docs.temporal.io/develop/typescript/workflows/message-passing
- Temporal TypeScript workflows: https://docs.temporal.io/develop/typescript/workflows
