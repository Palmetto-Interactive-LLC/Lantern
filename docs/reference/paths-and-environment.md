# Paths and Environment Reference

Filesystem layout, environment variables, and external tool locations.

## Directory Layout

```text
~/.lantern/
├── bin/
│   ├── lantern
│   ├── temporal
│   ├── lantern-up
│   ├── lantern-down
│   ├── lantern-doctor
│   ├── lantern-install
│   ├── lantern-setup-iterm
│   ├── iterm_launch.py
│   └── iterm_close.py
├── config/
│   └── lantern.toml
├── data/
│   ├── temporal/temporal.db
│   └── relay/lantern.db
├── logs/
│   ├── relay.log
│   ├── relay.error.log
│   └── temporal.log
└── run/
    ├── relay.pid
    ├── temporal.pid
    └── relay.sock
```

## macOS launchd

Plist: `~/Library/LaunchAgents/com.lantern.relay.plist`

| Key | Value |
|-----|-------|
| Label | `com.lantern.relay` |
| Program | `~/.lantern/bin/lantern relay --machine <hostname>` |
| KeepAlive | true |
| RunAtLoad | true |

## Worktree Layout (per repo)

```text
<repo>/
└── .claude/
    └── worktrees/
        └── <session-id>/
            ├── <name>-ai-<n>/
            ├── <name>-dat-<n>/
            └── ...
```

Orchestrator uses repo root; branch name equals session ID.

## Per-Agent Environment

Set by `startwork` before launching each agent:

| Variable | Example | Description |
|----------|---------|-------------|
| `DEVORCH_SESSION` | `myproject-1` | Session ID |
| `DEVORCH_RUN_ID` | `myproject-1-20260523T143022Z` | Unique run ID |
| `DEVORCH_ROLE` | `ai` | Agent role |
| `DEVORCH_PROJECT_SLUG` | `myproject` | Project name |
| `DEVORCH_SLOT` | `1` | Slot number |

## Host Environment

| Variable | Used by | Description |
|----------|---------|-------------|
| `PATH` | All | Must include `~/.lantern/bin` and `~/.local/bin` |
| `DEVORCH_SESSION` | `stopwork`, agent tooling | Session auto-detection |
| `RUST_LOG` | Relay | Tracing filter, such as `info` or `lantern=debug` |

## Devorch Config

Each agent process sources `~/.config/devorch/env` if present. Use it for API keys and agent environment configuration.

Runtime MCP configuration belongs in the agent CLI settings and should point to the orchestration client.

## External Tools

| Tool | Expected location | Purpose |
|------|-------------------|---------|
| `agent-runner` | `~/.local/bin/agent-runner` | Agent process wrapper and execution-window participant |
| Orchestration client | Agent MCP config | MCP bridge to Temporal workflows |
| iTerm2 | `/Applications/iTerm.app` | Display-only launcher |
| `claude` | PATH | Agent CLI |
| `agy` | PATH | Agent CLI |
| `codex` | PATH | Agent CLI |
| `kimi` | PATH | Agent CLI |
| `git` | PATH | Worktree management |
| `temporal` | `~/.lantern/bin/temporal` | Temporal CLI |

## Temporary Files

| Pattern | Purpose |
|---------|---------|
| `/tmp/devorch-kimi-init-{session}.json` | Temporal-gated init prompts for agent-runner |
| `/tmp/codex-devorch-sockets/` | Codex app-server sockets |

Legacy-only patterns such as `/tmp/devorch-startup-*` and `/tmp/devorch-runner-*` may appear in old logs or old workstations. Treat them as migration artifacts unless current code explicitly recreates them.

## Service Endpoints

| Service | Address |
|---------|---------|
| Temporal gRPC | `127.0.0.1:8243` (Plain `localhost` is strictly banned) |
| Temporal UI | http://127.0.0.1:8244 |

## Claude Model Defaults

| Role | Model |
|------|-------|
| orchestrator | opus |
| ai, dat, ops, plt, ui, sec, qa | sonnet |
| doc | haiku |

## Codex Model Defaults

Verified with `codex debug models --bundled` on 2026-05-23.

| Role | Model | Reasoning effort |
|------|-------|------------------|
| orchestrator, ai, sec | `gpt-5.5` | `xhigh` |
| dat, ops, plt, ui, doc, qa | `gpt-5.4-mini` | `low` |

## Legacy Note

`TMUX` and tmux pane identifiers may still appear in old shells, old rows, or quarantined migration data. They are not current runtime delivery targets.
