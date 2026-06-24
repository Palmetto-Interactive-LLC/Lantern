# Changelog

All notable changes to Lantern are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project uses [Calendar Versioning](https://calver.org/) (YYYY.MM.PATCH).

## [2026.6.0] — 2026-06-23

Initial public release.

### Added

- **Core MCP Server**: Serves `devorch_report_status`, `devorch_peer_message`, `devorch_query_team_state`, `devorch_get_setup_instructions` tools for agent orchestration
- **Local Runner**: Manages iTerm2 terminal panes and git worktrees for isolated agent workspaces
- **SQLite State Store**: Persistent local storage for squad state, task tracking, and audit logs at `~/.lantern/data/relay/lantern.db`
- **Temporal Integration**: Optional local Temporal dev server (127.0.0.1:8243) for workflow execution logging and diagnostics
- **Squad Lifecycle Management**: Launch (`startwork`) and tear down (`stopwork`) multi-agent squads with automatic cleanup
- **Beads Issue Tracking**: Integrated issue tracking backed by Dolt; tasks sync via git refs
- **Installer Script**: Automated installation via `./scripts/install.sh` with launchd service registration
- **Comprehensive Documentation**: Multi-format docs (tutorial, how-to, reference, explanation) following the Diátaxis framework

### Features

- **Multi-Agent Orchestration**: Run specialized agent roles (code review, data architecture, UI design, etc.) in parallel
- **Local-Only Design**: No cloud connectivity, no credentials, no secrets management required
- **Process Recovery**: Automatic detection and recovery from agent or service failures
- **Workspace Isolation**: Each squad gets dedicated terminal panes and git worktrees with automatic cleanup
- **Status Reporting**: Real-time status updates and peer-to-peer messaging between agents

### Requirements

- macOS with iTerm2
- git, Rust 1.70+
- Temporal CLI (for optional Temporal dev server)
- Agent CLIs (`claude`, `agy`, `codex`, or `kimi`)

### Documentation

- [Installation Guide](docs/how-to/install-lantern.md)
- [Quick Start Tutorial](docs/tutorial/first-squad.md)
- [CLI Reference](docs/reference/cli.md)
- [Architecture Overview](docs/explanation/architecture.md)
- [Development Guide](docs/how-to/develop-and-contribute.md)

### Known Limitations

- macOS only (Linux/Windows support planned)
- Docker Temporal is not supported; use native Temporal CLI only
- `cargo check`, `clippy`, and `cargo test` omitted from CI pending SQLite offline cache setup
