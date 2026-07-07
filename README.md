# Lantern

[![CI](https://github.com/Palmetto-Interactive-LLC/Lantern/actions/workflows/ci.yml/badge.svg)](https://github.com/Palmetto-Interactive-LLC/Lantern/actions/workflows/ci.yml)
[![Security](https://github.com/Palmetto-Interactive-LLC/Lantern/actions/workflows/pi-standard-security.yml/badge.svg)](https://github.com/Palmetto-Interactive-LLC/Lantern/actions/workflows/pi-standard-security.yml)
[![Releases](https://img.shields.io/github/v/release/Palmetto-Interactive-LLC/Lantern)](https://github.com/Palmetto-Interactive-LLC/Lantern/releases)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

Lantern is a macOS-first Rust runtime for local AI coding squads. It launches agent workspaces, manages iTerm2 panes and git worktrees, records local SQLite audit state, and exposes MCP-compatible orchestration tools for agent status, messaging, and recovery.

Lantern is for developers and operators who want repeatable local multi-agent development without adding a hosted control plane to their repository. It is intentionally local-first: Lantern does not require cloud credentials, and its own state stays under `~/.lantern/`.

## Project Status

Lantern is usable for Palmetto-style local agent squads and is still evolving as an OSS project. The stable surface is the CLI, installer, local state model, and documented iTerm2 workflow. The ACP/Goose and Beads runtime re-platforming work is experimental and tracked in Beads.

See [ROADMAP.md](ROADMAP.md) for the stable/experimental boundary and near-term priorities.

## Prerequisites

- macOS with iTerm2 for full squad launching
- git
- Temporal CLI for local workflow diagnostics (`brew install temporal`)
- At least one supported agent CLI on PATH: `claude`, `codex`, `agy`, or `kimi`
- Rust stable toolchain for source builds and local development

## Five-Minute Smoke Test

This path verifies the source checkout without installing a launchd service or starting a real squad:

```bash
git clone https://github.com/Palmetto-Interactive-LLC/Lantern.git
cd Lantern
cargo fmt --check
cargo test --test cli
cargo build --release
target/release/lantern --help
```

Expected result: the commands pass and `lantern --help` lists commands such as `relay`, `status`, `startwork`, `stopwork`, and `mcp`.

## Install Locally

Install the latest release:

```bash
curl -fsSL https://raw.githubusercontent.com/Palmetto-Interactive-LLC/Lantern/main/scripts/install.sh | sh
source ~/.zshrc
lantern --version
lantern doctor
```

The installer downloads the latest release asset for your Mac architecture,
verifies `SHA256SUMS` when available, and installs Lantern under
`~/.lantern/bin`.

From a source checkout, run `./scripts/install.sh`; it builds the current tree
instead of downloading a release unless `LANTERN_FORCE_DOWNLOAD=1` is set.

## Launch A Squad

```bash
lantern up
lantern startwork myproject 99 --agent claude --no-init
lantern status
lantern stopwork myproject-99
```

Use a disposable repository and high slot number for first tests. Full launch requires iTerm2's Python API and the selected agent CLI to be configured locally.

## Documentation

| Need | Start here |
| --- | --- |
| Guided first run | [Tutorial: Your first squad](docs/tutorial/first-squad.md) |
| Installation details | [How to install Lantern](docs/how-to/install-lantern.md) |
| Daily service commands | [How to manage services](docs/how-to/manage-services.md) |
| CLI reference | [Command reference](docs/reference/cli.md) |
| Configuration | [Configuration reference](docs/reference/configuration.md) |
| Architecture | [Architecture overview](docs/explanation/architecture.md) |
| Troubleshooting | [Troubleshooting guide](docs/how-to/troubleshoot-issues.md) |
| Contributing | [CONTRIBUTING.md](CONTRIBUTING.md) |

## Development Gates

```bash
make verify
make security
```

`make verify` runs Rust formatting, strict clippy, tests, release build, and local Markdown link checks. `make security` runs cargo audit, shellcheck, actionlint, and gitleaks. GitHub branch protection requires the `lint`, `build-test`, and security workflow contexts to pass before changes merge to `main`.

For support expectations and issue-reporting details, see [SUPPORT.md](SUPPORT.md).

Maintainer release steps, including signed-tag requirements, are documented in [How to cut a release](docs/how-to/cut-a-release.md).

## Security

Lantern's own runtime is local-only, but agent CLIs may use their own external authentication and network behavior. Do not commit credentials, local databases, or machine-specific secrets. Report vulnerabilities privately through GitHub private vulnerability reporting when available; see [SECURITY.md](SECURITY.md).

## License

Lantern is licensed under the Apache License 2.0. See [LICENSE](LICENSE).
