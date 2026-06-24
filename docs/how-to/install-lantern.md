# How to Install Lantern

Install Lantern and its local dependencies on a new machine.

## Automated Install

From the repository root:

```bash
git clone https://github.com/Palmetto-Interactive-LLC/pi-code-orchestrator.git
cd pi-code-orchestrator
./scripts/install.sh
```

Or, if `lantern` is already on PATH:

```bash
lantern install
```

Reload your shell after install:

```bash
source ~/.zshrc
```

Verify:

```bash
lantern --version
lantern doctor
```

## What the Installer Does

1. Creates `~/.lantern/` directory structure.
2. Installs Rust via rustup if missing.
3. Downloads the Temporal CLI to `~/.lantern/bin/temporal`.
4. Builds `lantern` with `cargo build --release`.
5. Copies helper scripts to `~/.lantern/bin/`.
6. Installs iTerm2 helper scripts and the `iterm2` Python package on macOS.
7. Writes `~/.lantern/config/lantern.toml`.
8. Adds `~/.lantern/bin` to PATH in `~/.zshrc`.
9. Registers `com.lantern.relay` launchd service on macOS.
10. Runs health checks.

For directory layout and config defaults, see [Paths and environment](../reference/paths-and-environment.md).

## Manual Install

If you prefer not to run the full installer:

```bash
cargo build --release
mkdir -p ~/.lantern/bin
cp target/release/lantern ~/.lantern/bin/
cp scripts/lantern-{up,down,doctor}.sh ~/.lantern/bin/
cp scripts/install.sh ~/.lantern/bin/lantern-install
chmod +x ~/.lantern/bin/*
```

Add `~/.lantern/bin` to your PATH manually.

## Prerequisites for Squad Launch

The installer configures iTerm2 helpers on macOS. You still need:

| Tool | Location / requirement |
|------|------------------------|
| iTerm2 | `/Applications/iTerm.app`, Python API enabled |
| `agent-runner` | `~/.local/bin/agent-runner` |
| Orchestration client | Agent MCP configuration |
| Agent CLI | `claude`, `agy`, `codex`, or `kimi` on PATH |
| git | System PATH |

After install, open iTerm2 once and enable **Settings -> General -> Magic -> Enable Python API**. Then run:

```bash
lantern-setup-iterm
```

Optional: `~/.config/devorch/env` for API keys and agent environment configuration.

## Reinstall After Code Changes

```bash
cd pi-code-orchestrator
cargo build --release
cp target/release/lantern ~/.lantern/bin/lantern
lantern restart
```

## Legacy Note

Older install docs required tmux for the runtime launcher. The current launcher uses iTerm2. Any remaining tmux health check output is migration residue and should be treated as legacy-only.

## Related

- [Tutorial: Your first squad](../tutorial/first-squad.md)
- [How to manage services](manage-services.md)
- [Configuration reference](../reference/configuration.md)
