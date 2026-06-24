# Lantern Documentation

Lantern is the local support layer for AI coding squads backed by Temporal workflows. It launches iTerm2 display windows, manages worktrees, records local inventory and audit projections, and leaves durable runtime control to Temporal.

This documentation is organized by **what you need**, not by source file. It follows the [Diataxis](https://diataxis.fr/) framework used by projects like Gatsby and Django.

## Current Runtime Contract

- Temporal workflows own runtime control, message delivery, setup readiness, runner leases, human control, and recovery.
- iTerm2 is display and lifecycle support only. It is not a message bus or reconciliation authority.
- The MCP runtime layer provides MCP tools for agent status reporting, peer messaging, and workflow control.
- SQLite stores inventory, audit projection, quarantine records, and planned doctor-state snapshots.
- Legacy tmux/local-queue/Lantern-Rust-MCP runtime references are migration history only.

## I want to...

| Goal | Start here |
|------|------------|
| Learn Lantern by doing | [Tutorial: Your first squad](tutorial/first-squad.md) |
| Install Lantern on my machine | [How to install Lantern](how-to/install-lantern.md) |
| Start or stop background services | [How to manage services](how-to/manage-services.md) |
| Launch a coding squad | [How to launch a squad](how-to/launch-a-squad.md) |
| Tear down a squad workspace | [How to stop a squad](how-to/stop-a-squad.md) |
| Pause, take over, or recover an agent | [How to intervene with agents](how-to/intervene-with-agents.md) |
| Fix something that broke | [How to troubleshoot issues](how-to/troubleshoot-issues.md) |
| Build or contribute to Lantern | [How to develop and contribute](how-to/develop-and-contribute.md) |
| Plan diagnostics and health output | [Doctor-state planning](reference/doctor-state.md) |
| Verify the Temporal runtime migration | [Resilience verification report](reference/resilience-verification-report.md) |
| Look up a command or config field | [Reference](#reference) |
| Understand how Lantern works | [Explanation](#explanation) |

## By Documentation Type

### Tutorial

Learning-oriented. Follow step by step to build confidence.

- [Your first squad](tutorial/first-squad.md) - install services, launch a 9-agent iTerm workspace, verify it works, tear it down

### How-to Guides

Task-oriented. Assumes you know what you want to accomplish.

- [Install Lantern](how-to/install-lantern.md)
- [Manage services](how-to/manage-services.md)
- [Launch a squad](how-to/launch-a-squad.md)
- [Stop a squad](how-to/stop-a-squad.md)
- [Intervene with agents](how-to/intervene-with-agents.md)
- [Troubleshoot issues](how-to/troubleshoot-issues.md)
- [Develop and contribute](how-to/develop-and-contribute.md)

### Reference

Information-oriented. Facts only - no steps, no teaching.

- [CLI commands](reference/cli.md)
- [Configuration](reference/configuration.md)
- [MCP runtime authority](reference/mcp-tools.md)
- [Database schema](reference/database-schema.md)
- [Paths and environment](reference/paths-and-environment.md)
- [Doctor-state planning](reference/doctor-state.md)
- [Resilience verification report](reference/resilience-verification-report.md)

### Explanation

Understanding-oriented. Context, design, and why things work this way.

- [Architecture](explanation/architecture.md)
- [Design principles](explanation/design-principles.md)
- [Temporal runtime control plane ADR](architecture/adr-0001-temporal-runtime-control-plane.md)

## Audiences

| You are... | Read first | Then |
|------------|------------|------|
| **New operator** | [Tutorial](tutorial/first-squad.md) | [How to launch a squad](how-to/launch-a-squad.md) |
| **Daily user** | [How to manage services](how-to/manage-services.md) | [CLI reference](reference/cli.md) |
| **Agent author** | [MCP runtime authority](reference/mcp-tools.md) | [Architecture](explanation/architecture.md) |
| **Contributor** | [How to develop](how-to/develop-and-contribute.md) | [Architecture](explanation/architecture.md) |

## External Dependencies

Lantern orchestrates tools that live outside this repository:

| Tool | Location | Role |
|------|----------|------|
| `agent-runner` | `~/.local/bin/agent-runner` | Wraps agent CLI startup and coordinates runner readiness |
| MCP client | Agent MCP config | Runtime bridge for agent status, messaging, and workflow control via Temporal |
| iTerm2 | `/Applications/iTerm.app` on macOS | Displays local agent processes |
| Agent CLIs | `claude`, `agy`, `codex`, `kimi` | AI coding agents |

## Maintainer Notes

Planning and handoff documents (`HANDOFF.md`) remain in the repo root for maintainers but are not the best entry point for new users.
