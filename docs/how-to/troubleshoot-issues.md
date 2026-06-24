# How to Troubleshoot Issues

Diagnose current Lantern problems using Temporal workflow state, iTerm display readiness, orchestration client configuration, and local projections.

## Run Diagnostics First

```bash
lantern doctor
lantern status
lantern logs relay
```

Then inspect Temporal workflow state:

```text
http://127.0.0.1:8244
```

Include this output when reporting issues. Local status is a projection; Temporal workflow state is the runtime authority.

## Services Will Not Start

### Temporal Fails

```bash
cat ~/.lantern/run/temporal.pid
kill $(cat ~/.lantern/run/temporal.pid) 2>/dev/null
rm ~/.lantern/run/temporal.pid
lantern up
```

Then open Temporal UI and verify workflow visibility.

### Relay Not Running

macOS:

```bash
launchctl list com.lantern.relay
tail -50 ~/.lantern/logs/relay.log ~/.lantern/logs/relay.error.log
launchctl start com.lantern.relay
```

Linux:

```bash
lantern relay --machine $(hostname -s) &
```

## iTerm Window Does Not Open

Check local dependencies:

```bash
lantern doctor
open -a iTerm
lantern-setup-iterm
```

In iTerm2, enable **Settings -> General -> Magic -> Enable Python API**. Re-run `lantern startwork` after the Python API is reachable.

## Agents Start but MCP Is Not Ready

- Confirm Relay and Temporal are running: `lantern doctor`
- Check Relay logs: `lantern logs relay`
- Verify the orchestration client is configured in each agent CLI MCP setting.
- In Temporal UI, inspect `McpSetupWorkflow`, `McpRecoveryWorkflow`, and `ExecutionWindowWorkflow` for the affected role.

Do not repair this by direct terminal injection. Runtime MCP readiness belongs to the Temporal workflow path.

## Messages Are Not Delivered

Check workflow state in this order:

1. `SessionMessageBusWorkflow` for message acceptance or rejection.
2. `MessageDeliveryWorkflow` for retrying, degraded, or dead-letter status.
3. `ExecutionWindowWorkflow` for runner and transport readiness.
4. `RunnerLeaseWorkflow` for stale or released runner leases.
5. `HumanControlWorkflow` for pause or takeover state.

Use local SQLite only to inspect projections and audit trails:

```bash
sqlite3 ~/.lantern/data/relay/lantern.db "SELECT id, status FROM sessions;"
sqlite3 ~/.lantern/data/relay/lantern.db \
  "SELECT agent_id, transport_status, last_seen_at FROM terminal_targets;"
```

## Branch or Worktree Already Exists

```bash
lantern stopwork <session>
git worktree list
git worktree prune
git branch -D myproject-ai-1
git worktree remove --force .claude/worktrees/<session>/myproject-ai-1
```

## `agent-runner` Not Found

```bash
which agent-runner
```

Expected location: `~/.local/bin/agent-runner`. Install from Lantern and required tooling.

## Quarantined Terminal Targets

Legacy terminal transport rows are preserved for audit:

```bash
sqlite3 ~/.lantern/data/relay/lantern.db \
  "SELECT agent_id, quarantine_reason, quarantined_at FROM terminal_target_quarantine;"
```

Quarantined rows are diagnostic only. They must not be repaired into active delivery targets.

## Reset Local Projection State

Use this only when local projection data is corrupt and Temporal workflow state has been reviewed:

```bash
rm ~/.lantern/data/relay/lantern.db
lantern status
```

This loses local inventory and audit projection data. It does not clean up Temporal workflow history.

## Known Migration Gaps

| Issue | Current handling |
|-------|------------------|
| Doctor output is still dependency-focused | See [Doctor-state planning](../reference/doctor-state.md) |
| Old tmux terminal rows may exist | They belong in quarantine output only |
| Rust MCP runtime tools are disabled | Use the orchestration client |

## Legacy-Only Checks

If you are investigating pre-iTerm history, you may see old tmux sessions or logs. Treat them as legacy artifacts. They are not active runtime state and should not be used for delivery or recovery decisions.

## Related

- [How to manage services](manage-services.md)
- [How to intervene with agents](intervene-with-agents.md)
- [Doctor-state planning](../reference/doctor-state.md)
