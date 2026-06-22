# Lantern

Lantern is a self-contained Rust agent that provides local orchestration for AI coding squads. It runs on each developer machine, serving as both the MCP server for agents and the local runtime authority backed by Temporal.

## Architecture

Lantern combines three functions into one deployable unit:

- **MCP server**: Serves tools (`devorch_report_status`, `devorch_peer_message`, `devorch_query_team_state`, `devorch_get_setup_instructions`) for agents to interact with the runtime
- **Local runner**: Manages terminal windows (iTerm2), git worktrees, process execution, and recovery
- **Delivery**: Communication runs directly over iTerm2 injection keyed off SQLite state; SQLite is the single source of truth. An optional native Temporal instance (127.0.0.1:8243) may run for the relay/worker but is OFF the delivery path (Docker Temporal is strictly unsupported)

No remote server dependency. Deploy on N machines, each runs independently.

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

- macOS with iTerm2
- git, Rust 1.70+
- Temporal CLI (used by local dev server)
- An agent CLI (`claude`, `agy`, `codex`, or `kimi`)

## Development

```bash
cargo test
cargo build --release
```

See [How to develop and contribute](docs/how-to/develop-and-contribute.md).

## License

Proprietary - Palmetto Interactive LLC
