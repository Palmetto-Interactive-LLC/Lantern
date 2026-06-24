# Lantern

Lantern is a self-contained Rust binary that provides local orchestration for AI coding squads. It runs on each developer machine as an MCP server, local runtime, and terminal orchestrator—no cloud dependency, no credentials required.

## What Problem Does Lantern Solve?

Developing with AI agents on your local machine requires coordination across multiple agent processes, terminal windows, git worktrees, and workflow state. Lantern consolidates this orchestration into one lightweight, local service that:

- Manages multiple agent sessions (specialized AI roles like code review, data architecture, UI design)
- Maintains terminal panes and git worktrees per task without manual intervention
- Handles agent-to-agent communication and status tracking
- Recovers from failures transparently—no setup required, no coordination overhead

## Architecture

Lantern combines three core functions into one deployable unit:

- **MCP server**: Exposes tools for agents to report status, send peer messages, query team state, and request setup instructions
- **Local runner**: Manages iTerm2 terminal panes, git worktrees, subprocess execution, and process recovery
- **Runtime authority**: SQLite stores all orchestration state locally; optional Temporal instance (127.0.0.1:8243) logs activity but is not on the delivery path

No remote server dependency. Each developer machine runs independently; Lantern can run on N machines in parallel without coordination.

## Quick start

```bash
git clone https://github.com/Palmetto-Interactive-LLC/pi-code-orchestrator.git
cd pi-code-orchestrator
./scripts/install.sh
source ~/.zshrc

lantern up
cd ~/Development/my-project
lantern startwork myproject 1 --agent claude
```

## Documentation

**[Full documentation](docs/README.md)**

| If you want to... | Read |
|-------------------|------|
| Learn by doing | [Tutorial: Your first squad](docs/tutorial/first-squad.md) |
| Install or operate | [How-to guides](docs/README.md#how-to-guides) |
| Look up a command | [CLI reference](docs/reference/cli.md) |
| Understand the runtime model | [Architecture](docs/explanation/architecture.md) |
| Plan diagnostics output | [Doctor-state planning](docs/reference/doctor-state.md) |

## Requirements

- **macOS** with iTerm2 (Lantern is macOS-only; Linux/Windows support planned)
- **git** (for worktree management)
- **Rust 1.70+** (for building from source)
- **Temporal CLI** (for optional local Temporal dev server; install via `brew install temporal-cli`)
- **Agent CLI** (one or more of: `claude`, `agy`, `codex`, or `kimi`)

## Installation

### Pre-built Binary

Download the latest release from the [GitHub releases page](https://github.com/Palmetto-Interactive-LLC/pi-code-orchestrator/releases) and run the installer:

```bash
./lantern-installer-macos.sh
source ~/.zshrc  # reload shell to pick up lantern command
```

### Build from Source

```bash
git clone https://github.com/Palmetto-Interactive-LLC/pi-code-orchestrator.git
cd pi-code-orchestrator
./scripts/install.sh
source ~/.zshrc
```

The installer places the binary at `~/.lantern/bin/lantern` and registers a launchd service for automatic startup.

## Configuration

Lantern stores all configuration and state locally:

- **Binary**: `~/.lantern/bin/lantern`
- **Data**: `~/.lantern/data/` (SQLite, session logs, worktree metadata)
- **Services**: Registered as a launchd service; starts automatically on login

No additional configuration files are required. All behavior is controlled via command-line flags and the local machine environment.

## Environment Variables

Lantern does not require environment variables to run. Optional configuration:

| Variable | Purpose | Default |
|----------|---------|---------|
| `RUST_LOG` | Control logging verbosity | `info` |
| `RUST_LOG` | Debug example | Set to `debug` for verbose output |

Example:

```bash
RUST_LOG=debug lantern up
```

## Commands

Core commands for development workflows:

```bash
lantern up                              # Start background services
lantern down                            # Stop background services
lantern doctor                          # Health check all dependencies
lantern status                          # Show local squad inventory
lantern startwork <project> <slot> --agent <agent>   # Launch a squad
lantern stopwork <project>-<slot>                     # Tear down a squad
lantern logs <relay|temporal>                         # Tail service logs
lantern mcp                                           # Start MCP server
```

See [CLI reference](docs/reference/cli.md) for full command documentation.

## Security

Lantern is designed for local development use only:

- **No cloud connectivity**: All operations run on your local machine
- **No credentials required**: No API keys, tokens, or secrets to manage
- **No remote dependencies**: Works completely offline after installation
- **Local state only**: SQLite database stored at `~/.lantern/data/relay/lantern.db`

For more information, see [SECURITY.md](SECURITY.md) and the [security reporting policy](SECURITY.md#reporting-vulnerabilities).

## Development

### Build

```bash
cargo build --release
```

### Test

```bash
cargo test
```

### Lint

```bash
cargo fmt --check
cargo clippy
```

For detailed development instructions, see [How to develop and contribute](docs/how-to/develop-and-contribute.md).

## Contributing

Contributions are welcome! Please read:

- [CONTRIBUTING.md](CONTRIBUTING.md) for development workflow and code conventions
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) for community standards
- [How to develop and contribute](docs/how-to/develop-and-contribute.md) for build, test, and documentation guidelines

## Support

For questions, feature requests, or issues:

- File a [GitHub issue](https://github.com/Palmetto-Interactive-LLC/pi-code-orchestrator/issues)
- Check [existing issues](https://github.com/Palmetto-Interactive-LLC/pi-code-orchestrator/issues) and [discussions](https://github.com/Palmetto-Interactive-LLC/pi-code-orchestrator/discussions)
- Read the [full documentation](docs/README.md) and [troubleshooting guide](docs/troubleshooting.md)

## License

This project is licensed under the Apache License 2.0 — see the [LICENSE](LICENSE) file for details.

Copyright 2026 Palmetto Interactive LLC
