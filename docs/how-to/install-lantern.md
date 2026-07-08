# How to Install Lantern

Install Lantern and its local dependencies on a new machine.

## Recommended Install

Install the latest Lantern release:

```bash
curl -fsSL https://raw.githubusercontent.com/Palmetto-Interactive-LLC/Lantern/main/scripts/install.sh | sh
```

The installer detects your Mac architecture, downloads the matching GitHub
Release archive, verifies `SHA256SUMS` when available, and installs Lantern to
`~/.lantern/bin`.

If `lantern` is already on PATH, you can reinstall with:

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

1. Creates the `~/.lantern/` directory structure.
2. Downloads the latest GitHub Release archive for your architecture, or builds from source when run from a checkout.
3. Verifies downloaded release archives against `SHA256SUMS` when the checksum file is present.
4. Installs `lantern`, service helpers, wrapper commands, and iTerm2 helper scripts to `~/.lantern/bin`.
5. Writes the launchd plist for `com.lantern.relay`.
6. Writes `~/.lantern/config/lantern.toml`.
7. Adds `~/.lantern/bin` to PATH in `~/.zshrc` or `~/.bashrc` when those files exist.
8. Ad-hoc codesigns the installed binary on macOS when `codesign` is available.

When run from a source checkout, `scripts/install.sh` builds from source instead
of downloading a release. Set `LANTERN_FORCE_DOWNLOAD=1` to force release
download behavior from a checkout.

When you run `lantern up` on macOS, Lantern submits the Temporal dev server to
launchd as `com.lantern.temporal`.

For directory layout and config defaults, see [Paths and environment](../reference/paths-and-environment.md).

## Release Kit

Each GitHub Release publishes:

- `install-lantern.sh` - a copy of the installer script for distribution.
- `lantern-vYYYY.M.PATCH-aarch64-apple-darwin.tar.gz` - Apple Silicon archive.
- `lantern-vYYYY.M.PATCH-x86_64-apple-darwin.tar.gz` - Intel Mac archive.
- `SHA256SUMS` - checksums for release assets.

## Manual Source Install

If you prefer to build from source:

```bash
git clone https://github.com/Palmetto-Interactive-LLC/Lantern.git
cd Lantern
cargo build --release
mkdir -p ~/.lantern/bin
cp target/release/lantern ~/.lantern/bin/
cp scripts/lantern-up.sh ~/.lantern/bin/lantern-up
cp scripts/lantern-down.sh ~/.lantern/bin/lantern-down
cp scripts/lantern-doctor.sh ~/.lantern/bin/lantern-doctor
cp scripts/install.sh ~/.lantern/bin/lantern-install
cp scripts/setup-iterm.sh ~/.lantern/bin/lantern-setup-iterm
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
cd Lantern
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
