# Lantern

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Releases](https://img.shields.io/github/v/release/Palmetto-Interactive-LLC/Lantern)](https://github.com/Palmetto-Interactive-LLC/Lantern/releases)
[![Issues](https://img.shields.io/github/issues/Palmetto-Interactive-LLC/Lantern)](https://github.com/Palmetto-Interactive-LLC/Lantern/issues)

**Lantern** is a self-contained Rust binary that orchestrates AI coding squads on your local machine—managing multiple agent processes, terminal windows, git worktrees, and workflow state with zero cloud dependencies.

## Why Lantern?

Multi-agent development needs coordination across specialized roles (code review, data design, security, UI, ops). Lantern consolidates this into one lightweight service: one command launches your entire squad, each agent gets an isolated terminal pane and git branch, and agents communicate via MCP tools.

## Quick Start

### Install

**Quick install (macOS):**

```bash
curl -fsSL https://raw.githubusercontent.com/Palmetto-Interactive-LLC/Lantern/main/scripts/install.sh | sh
source ~/.zshrc
```

**Or build from source:**

```bash
git clone https://github.com/Palmetto-Interactive-LLC/Lantern.git
cd Lantern
./scripts/install.sh
source ~/.zshrc
```

### Launch a Squad

```bash
# Default: headed orchestrator + 8 headless specialists (claude agent)
lantern up
lantern startwork myproject

# Solo Goose mode (single focused agent, no orchestrator overhead)
lantern startwork myproject --agent goose

# Different agent CLI
lantern startwork myproject --agent codex
```

### Common Commands

```bash
lantern doctor        # Health check (Rust, Temporal, iTerm2, git, agents)
lantern status        # Show all active squads
lantern stopwork myproject-1  # Tear down a squad
```

## How It Works

- **MCP Server**: Agents report status, send peer messages, query team state via `devorch_*` tools
- **Local Runner**: Creates iTerm2 window with colored panes (one per role), git worktrees for isolation
- **State Store**: SQLite at `~/.lantern/data/relay/lantern.db` (local, no cloud)
- **Optional Temporal**: Dev server at `127.0.0.1:8243` for workflow logging (Docker Temporal unsupported)

## Learn More

| Topic | Link |
|-------|------|
| **Agent Modes** | [Wiki: Agent Modes](https://github.com/Palmetto-Interactive-LLC/Lantern/wiki/Agent-Modes) |
| **Full Command Reference** | [Wiki: Commands](https://github.com/Palmetto-Interactive-LLC/Lantern/wiki/Command-Reference) |
| **Architecture** | [Wiki: Architecture](https://github.com/Palmetto-Interactive-LLC/Lantern/wiki/Architecture) |
| **Installation** | [Wiki: Installation & Setup](https://github.com/Palmetto-Interactive-LLC/Lantern/wiki/Installation-&-Setup) |
| **MCP Tools** | [Wiki: MCP Tools](https://github.com/Palmetto-Interactive-LLC/Lantern/wiki/MCP-Tools) |
| **Troubleshooting** | [Wiki: Troubleshooting](https://github.com/Palmetto-Interactive-LLC/Lantern/wiki/Troubleshooting) |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development workflow, build gates, and issue tracking with Beads.

## Security

Local-only design: no cloud connectivity, no credentials, no secrets. See [SECURITY.md](SECURITY.md).

## License

Apache License 2.0 — see [LICENSE](LICENSE).

---

**Questions?** [Open an issue](https://github.com/Palmetto-Interactive-LLC/Lantern/issues) or check the [Wiki](https://github.com/Palmetto-Interactive-LLC/Lantern/wiki).
