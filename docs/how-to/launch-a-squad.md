# How to Launch a Squad

Launch a 9-agent coding squad with git worktrees, iTerm2 display panes, agent processes, and Temporal-backed runtime control.

## Prerequisites

- [Lantern installed](install-lantern.md) and [services running](manage-services.md)
- macOS with iTerm2 and the iTerm2 Python API enabled
- Current directory inside a git repository
- `agent-runner` at `~/.local/bin/agent-runner`
- Orchestration client configured in your agent's MCP settings
- Agent CLI on PATH

Squads open in a new iTerm2 window with nine native split panes. iTerm is display and lifecycle support only; runtime delivery and control go through Temporal workflows.

## Basic Launch

From your git repository:

```bash
cd ~/Development/my-project
lantern startwork myproject 1 --agent claude
```

Creates session `myproject-1`, nine iTerm display panes, eight worker worktrees, local inventory rows, and Temporal-facing runner/MCP setup state.

## Auto-Detect Name and Slot

```bash
lantern startwork
```

- **Name** defaults to the repo directory name.
- **Slot** auto-allocates the next available number.

## Use a Different Agent CLI

```bash
lantern startwork myproject 2 --agent agy
lantern startwork myproject 3 --agent codex
lantern startwork myproject 4 --agent kimi
```

## Skip Initialization Prompts

If you want to launch processes without startup prompts:

```bash
lantern startwork myproject 1 --agent claude --no-init
```

For agents that use Temporal-gated initialization, `agent-runner` waits until the execution window and MCP readiness barriers exist before delivering startup content.

## Daily Workflow

```bash
lantern up
cd ~/Development/my-project
lantern startwork
lantern status
```

Use iTerm2 to view or manually inspect agent panes. Use Temporal UI for runtime state:

```text
http://127.0.0.1:8244
```

## What Gets Created

| Resource | Location / pattern |
|----------|--------------------|
| iTerm window | One display window for `{name}-{number}` |
| iTerm panes | One pane per role |
| Worktrees | `.claude/worktrees/{session-id}/` |
| Branches | `{name}-{role}-{number}` |
| SQLite records | Local inventory, terminal targets, audit/projection rows |
| Temporal workflows | Session, setup, message, delivery, execution-window, runner, MCP, and human-control state |

Orchestrator uses the repo root. Other roles use dedicated worktrees.

## Squad Layout

```text
+------+-----+-----+
| orch | ai  | sec |
|      +-----+-----+
|      | dat | ops |
|      +-----+-----+
|      | plt | ui  |
|      +-----+-----+
|      | doc | qa  |
+------+-----+-----+
```

## If Launch Fails

| Error | Action |
|-------|--------|
| Branch already exists | [Stop the old squad](stop-a-squad.md) or pick a new slot number |
| Worktree root exists | Clean up `.claude/worktrees/{session-id}/` or use a different slot |
| Not inside git repo | `cd` into a directory with `.git` |
| iTerm Python API unavailable | Enable iTerm2 Python API and run `lantern-setup-iterm` |
| `agent-runner` not found | Install Lantern and required tooling |
| MCP readiness missing | Verify orchestration client configured in agent MCP settings |

For startup and readiness issues, see [How to troubleshoot issues](troubleshoot-issues.md).

## Legacy Note

Older runbooks describe tmux sessions, tmux detach/reattach, and tmux pane IDs. Those instructions are legacy-only and should not be used for current launches.

## Related

- [How to stop a squad](stop-a-squad.md)
- [CLI reference: startwork](../reference/cli.md#startwork)
- [Architecture](../explanation/architecture.md)
