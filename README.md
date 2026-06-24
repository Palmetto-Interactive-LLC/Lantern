# Lantern

Lantern is a self-contained Rust binary that orchestrates AI coding squads on your local machine. It manages multiple agent processes (specialized roles like code review, data architecture, UI design), terminal windows, git worktrees, and workflow state—all locally, with no cloud dependency or credentials required.

## What Problem Does Lantern Solve?

Developing with AI agents requires coordinating multiple agent processes, keeping their work isolated in separate git branches and terminal panes, and maintaining state across sessions. Lantern consolidates this complexity into one lightweight, local service that:

- **Launches multi-agent squads** with specialized roles (Architect, Data, Security, Operations, Platform, UI, Documentation, QA) working in parallel
- **Manages terminal isolation** — one pane per agent role, color-coded, with role-specific agent CLIs
- **Handles worktree isolation** — each squad member gets a dedicated git branch and worktree
- **Coordinates communication** — agents can report status, send peer-to-peer messages, and query team state via MCP tools
- **Recovers from failures** — transparent detection and recovery of agent crashes or terminal issues
- **Supports multiple agent CLIs** — claude, codex, kimi, gemini, agy, or goose

## Architecture

Lantern combines three core functions in one deployable Rust binary:

### 1. MCP Server

Exposes tools for agents to interact with the runtime:

- `devorch_report_status` — agent reports its current status (ack, complete, blocked, failed, degraded, recovered)
- `devorch_peer_message` — agent sends a message to another agent role
- `devorch_query_team_state` — agent queries the state of all team members
- `devorch_get_setup_instructions` — agent fetches its initialization instructions

### 2. Local Runner (iTerm2 + Git)

- **iTerm2 Native Integration**: Creates a new terminal window with a squad layout (default 4×2+1 grid of panes); each pane runs one agent role
- **Git Worktrees**: Each squad member gets a dedicated worktree branching off the repo root, isolated for parallel work
- **Process Management**: Launches agent CLIs in panes, monitors their health, recovers from crashes

### 3. Runtime Authority

- **SQLite Database**: Local state store at `~/.lantern/data/relay/lantern.db` — the single source of truth for squad state, agent health, session metadata
- **Optional Temporal Integration**: Local Temporal dev server at `127.0.0.1:8243` for workflow execution logging and advanced diagnostics (not on the delivery path; Docker Temporal is unsupported)

## Agent Modes and Workflows

Lantern supports multiple modes for different coding scenarios:

### Mode 1: Default Squad (Headed Orchestrator + Headless Specialists)

```bash
lantern startwork myproject
# or explicitly:
lantern startwork myproject 1 --agent claude
```

**What launches:**

- **Orchestrator pane (headed)**: The conductor role managing the squad, running `claude` agent CLI in an interactive terminal
- **8 specialist panes (headless ACP)**: Dedicated agents for AI, Data Architecture, Security, Operations, Platform, UI, Documentation, Quality Assurance — each runs headless (no terminal interactivity), driven by MCP messages from the orchestrator
- **9 total panes** in a 4×2+1 grid layout, each with a unique color, role label, and dedicated git worktree

**When to use:** Multi-agent projects needing coordination across diverse specialties (e.g., building a full-stack feature with separate code review, data, security, and UI concerns).

**Example workflow:**
```bash
lantern startwork my-app 1 --agent claude
# Terminal opens with 9 panes, orchestrator pane is interactive
# Type instructions in the orchestrator; team members execute tasks in parallel
```

### Mode 2: Solo Goose Mode (Single Focused Agent)

```bash
lantern startwork myproject --agent goose
```

**What launches:**

- **Single Goose pane (headed)**: One interactive goose session (full-featured native Goose with no devorch orchestrator extensions)
- **1 worktree** for the session
- **No specialist fleet** — just you and Goose, focused work

**When to use:** Single-agent focused fixes, prototyping, or when you want Goose's native terminal features (readline, keyring, full feature set) without the overhead of a multi-agent squad.

**Example workflow:**
```bash
lantern startwork my-app --agent goose
# Terminal opens with single Goose pane; full interactive goose session
goose> help
```

### Mode 3: Squad with Different Agent CLI

```bash
lantern startwork myproject 2 --agent codex
lantern startwork myproject 3 --agent kimi
lantern startwork myproject 4 --agent agy
```

**What launches:**

- Same 9-pane grid layout as Mode 1
- **Orchestrator and 8 specialists use the specified agent** (codex, kimi, or agy instead of claude)
- Each agent role gets its native model configuration

**When to use:** Testing different AI providers, or when your project requires a specific agent (e.g., Codex for IDE integration, Kimi for multilingual work).

**Example workflow:**
```bash
lantern startwork my-app 1 --agent kimi
lantern startwork my-app 2 --agent codex
# Run two squads in parallel with different agents
lantern status  # see both sessions
```

## Requirements

- **macOS** with iTerm2 (Lantern is macOS-only; Linux/Windows support planned)
- **git** (for worktree management)
- **Rust 1.70+** (if building from source; pre-built binaries available)
- **Temporal CLI** (for optional local Temporal dev server; install via `brew install temporal-cli`)
- **Agent CLI** (at least one of: `claude`, `codex`, `kimi`, `gemini`, `agy`, or `goose`)

## Installation

### Step 1: Install Dependencies

Ensure you have Rust and the Temporal CLI:

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Install Temporal CLI
brew install temporal-cli
```

### Step 2: Build and Install Lantern

Clone the repository and run the installer:

```bash
git clone https://github.com/Palmetto-Interactive-LLC/pi-code-orchestrator.git
cd pi-code-orchestrator
./scripts/install.sh
source ~/.zshrc
```

**What the installer does:**

1. Creates `~/.lantern/bin/` and `~/.lantern/data/` directories
2. Builds `lantern` from source with `cargo build --release`
3. Installs the binary to `~/.lantern/bin/lantern`
4. Registers a macOS launchd service for automatic startup on login

After installation, `lantern` is available on your `$PATH`.

### Step 3: Verify Installation

```bash
lantern doctor
# Output: all systems green (Rust, Temporal, iTerm2, git, agent CLIs)
```

## Reinstall After Source Changes

If you modify the Lantern source code, rebuild and reinstall:

```bash
cargo build --release
cp target/release/lantern ~/.lantern/bin/lantern
lantern restart
```

This restarts the launchd service with the new binary.

## Configuration

### Directory Structure

Lantern stores all state and configuration locally (no cloud sync, no secrets):

```
~/.lantern/
├── bin/                          # Binaries
│   ├── lantern                   # Main executable
│   └── temporal                  # Temporal CLI (downloaded by installer)
├── data/
│   ├── relay/
│   │   └── lantern.db            # SQLite state store (squads, agents, sessions)
│   └── temporal/                 # Temporal dev server data (optional)
├── logs/                         # Service logs (relay, temporal)
├── config/                       # Configuration files (rarely used)
└── run/                          # Runtime state (pidfiles, sockets)
```

### No Configuration Files Required

Lantern requires zero configuration files to run. All behavior is controlled via:

1. **Command-line flags** (`--agent`, `--no-init`, etc.)
2. **Environment variables** (optional; see below)
3. **Interactive input** (initialization prompts from agents via MCP)

### Environment Variables

Lantern does not require any environment variables. Optional configuration:

| Variable | Purpose | Example |
|----------|---------|---------|
| `RUST_LOG` | Control logging verbosity | `RUST_LOG=debug lantern up` |
| `DEVORCH_SESSION` | Session ID (set automatically by startwork) | (internal use) |
| `DEVORCH_ROLE` | Agent role (set automatically by startwork) | (internal use) |

Minimal example with debug logging:

```bash
RUST_LOG=debug lantern up
RUST_LOG=debug lantern startwork myproject
```

### Example .env (Not Required)

Lantern is designed with zero environment variables. For reference, see `.env.example` in the repo — it documents that no env vars are needed to run Lantern.

## Command Reference

All commands are subcommands of `lantern`. Run `lantern --help` for the full list.

### Service Management

#### `lantern up`

Start all local background services (Temporal dev server and Relay daemon).

```bash
lantern up
# Output: Starting Temporal at 127.0.0.1:8243 and 127.0.0.1:8244...
#         Relay daemon running, listening for agent MCP clients...
```

Run this once per session (or let the launchd service start it automatically at login).

#### `lantern down`

Stop all background services.

```bash
lantern down
# Output: Stopping Temporal...
#         Stopping Relay...
```

#### `lantern restart`

Restart all services (useful after installing a new binary).

```bash
lantern restart
```

#### `lantern doctor`

Health check all dependencies and services.

```bash
lantern doctor
# Output:
# ✓ Rust: 1.71.0 (found at /opt/homebrew/bin/rustc)
# ✓ Cargo: 1.71.0 (found at /opt/homebrew/bin/cargo)
# ✓ Temporal CLI: v0.12.0 (found at ~/.lantern/bin/temporal)
# ✓ Git: 2.42.0 (found at /usr/bin/git)
# ✓ iTerm2: 3.4.x (found)
# ✓ Agent CLIs: claude, codex, kimi (found)
# ✓ Relay daemon: running at 127.0.0.1:7233
# ✓ Temporal server: running at 127.0.0.1:8243
```

### Squad Management

#### `lantern startwork [project] [slot] [--agent AGENT] [--no-init]`

Launch a new squad workspace.

**Arguments:**

- `project` — project name (optional; defaults to current repo name)
- `slot` — numeric slot (optional; auto-allocated if omitted)
- `--agent AGENT` — agent CLI to use for all roles: `claude` (default), `codex`, `kimi`, `gemini`, `agy`, or `goose`
- `--no-init` — skip initialization prompts

**Examples:**

```bash
# Default squad (claude agent, default slot)
lantern startwork myproject

# Named slot
lantern startwork myproject 1

# Different agent
lantern startwork myproject 2 --agent codex

# Goose solo mode
lantern startwork myproject --agent goose

# Skip initialization
lantern startwork myproject --no-init

# Legacy positional syntax
lantern startwork myproject 1 claude
```

**What happens:**

1. Git worktree is created per agent role (9 worktrees in default mode, 1 in goose mode)
2. iTerm2 window opens with the squad layout
3. Agents are launched in panes
4. Session is registered in SQLite
5. If initialized, agents fetch their setup instructions via MCP

#### `lantern status`

Show the state of all active squads and agents.

```bash
lantern status
# Output:
# Session: myproject-1 (created 2 hours ago, status: active)
#   + orchestrator     (pane: w1s0s0, worktree: .claude/worktrees/myproject-1/myproject-1)
#   + ai               (pane: w1s0s1, agent: claude, status: idle)
#   + dat              (pane: w1s0s2, agent: claude, status: busy)
#   + sec              (pane: w1s0s3, agent: claude, status: idle)
#   ... (6 more panes)
```

#### `lantern stopwork [session] [--preserve-worktrees] [--all]`

Tear down a squad workspace.

**Arguments:**

- `session` — session ID to stop (e.g., `myproject-1`)
- `--all` — stop all active sessions
- `--preserve-worktrees` — keep git worktrees; only close terminal panes

**Examples:**

```bash
# Stop a specific session
lantern stopwork myproject-1

# Stop all sessions
lantern stopwork --all

# Close panes but keep worktrees (for manual cleanup)
lantern stopwork myproject-1 --preserve-worktrees
```

**What happens:**

1. iTerm2 window is closed
2. Git worktrees are deleted (unless `--preserve-worktrees`)
3. Session is marked inactive in SQLite
4. Agents are unregistered

### Pane Control (Advanced)

#### `lantern pause <session>-<role>`

Pause an agent (suspend it without killing the terminal pane).

```bash
lantern pause myproject-1-ai
```

#### `lantern resume <session>-<role>`

Resume a paused agent.

```bash
lantern resume myproject-1-ai
```

#### `lantern takeover <session>-<role>`

Take human control of an agent pane (disable agent input; allow manual commands).

```bash
lantern takeover myproject-1-ui
# Now you can type directly in the UI pane
```

#### `lantern release <session>-<role>`

Release human control; let the agent resume.

```bash
lantern release myproject-1-ui
```

#### `lantern recover <session>-<role>`

Force recovery of a stuck or crashed agent.

```bash
lantern recover myproject-1-ai
```

#### `lantern note <session>-<role> <message>`

Inject a note into an agent pane.

```bash
lantern note myproject-1-ai "Review the error in src/main.rs line 42"
```

### Logging

#### `lantern logs <service>`

Tail logs for a service.

**Services:**

- `relay` — Lantern Relay daemon logs
- `temporal` — Temporal dev server logs

**Examples:**

```bash
lantern logs relay
lantern logs temporal
```

### MCP Server

#### `lantern mcp`

Start the Lantern MCP stdio server (for agent CLI integration).

```bash
lantern mcp
```

This is normally spawned by agent CLIs (claude, codex, etc.) as a child process. You rarely run it manually.

## Development Workflow

### Build

```bash
cargo build --release
```

The binary is produced at `target/release/lantern`.

### Test

```bash
cargo test
```

All 77 tests must pass before submitting a PR.

### Code Quality

```bash
# Format check
cargo fmt --check

# Linting
cargo clippy

# Format code
cargo fmt
```

All code must pass `cargo fmt --check` and `cargo clippy` before merge (enforced by CI).

## Security

Lantern is designed for local development use only:

- **No cloud connectivity** — all operations run on your local machine
- **No credentials required** — no API keys, tokens, or secrets to manage
- **No remote dependencies** — works completely offline after installation
- **Local state only** — SQLite database at `~/.lantern/data/relay/lantern.db`
- **Temporal is local** — optional dev server at `127.0.0.1:8243` (loopback only)
- **iTerm2 is local** — terminal integration via macOS process APIs; no remote interaction

For more information, see [SECURITY.md](SECURITY.md) and the [security reporting policy](SECURITY.md#reporting-vulnerabilities).

## Contributing

We welcome contributions. Please read:

- [CONTRIBUTING.md](CONTRIBUTING.md) — development workflow, build gates, PR workflow, and beads issue tracking
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) — community standards
- [How to develop and contribute](docs/how-to/develop-and-contribute.md) — detailed development guide

### Quick Start for Contributors

```bash
# Clone and build
git clone https://github.com/Palmetto-Interactive-LLC/pi-code-orchestrator.git
cd pi-code-orchestrator
cargo build --release

# Run tests
cargo test

# Check formatting
cargo fmt --check
cargo clippy

# Create a feature branch
git checkout -b feature/your-feature

# Commit with conventional messages
git commit -m "feat(startwork): add --agent goose option"

# Push and open a PR (base: main, requires 1 approval from CODEOWNERS)
git push origin feature/your-feature
gh pr create --title "feat: add goose agent support" --body "..."
```

For full details on issue tracking with Beads (`bd`), see [CONTRIBUTING.md](CONTRIBUTING.md#issue-tracking-with-beads).

## Support

For questions, bug reports, or feature requests:

- **File a GitHub issue**: [github.com/Palmetto-Interactive-LLC/pi-code-orchestrator/issues](https://github.com/Palmetto-Interactive-LLC/pi-code-orchestrator/issues)
- **Check existing issues**: [github.com/Palmetto-Interactive-LLC/pi-code-orchestrator/issues](https://github.com/Palmetto-Interactive-LLC/pi-code-orchestrator/issues)
- **Read the full documentation**: [docs/README.md](docs/README.md)
- **Troubleshooting guide**: [docs/troubleshooting.md](docs/troubleshooting.md)

## License

This project is licensed under the Apache License 2.0 — see the [LICENSE](LICENSE) file for details.

Copyright 2026 Palmetto Interactive LLC
