# Lantern

[![CI](https://github.com/Palmetto-Interactive-LLC/Lantern/actions/workflows/ci.yml/badge.svg)](https://github.com/Palmetto-Interactive-LLC/Lantern/actions/workflows/ci.yml)
[![Security](https://github.com/Palmetto-Interactive-LLC/Lantern/actions/workflows/pi-standard-security.yml/badge.svg)](https://github.com/Palmetto-Interactive-LLC/Lantern/actions/workflows/pi-standard-security.yml)
[![Releases](https://img.shields.io/github/v/release/Palmetto-Interactive-LLC/Lantern)](https://github.com/Palmetto-Interactive-LLC/Lantern/releases)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

Lantern is a macOS-first Rust runtime for local AI coding squads. It launches agent workspaces, manages iTerm2 panes and git worktrees, records local SQLite audit state, and exposes MCP-compatible orchestration tools for agent status, messaging, and recovery.

Lantern is for developers and operators who want repeatable local multi-agent development without adding a hosted control plane to their repository. It is intentionally local-first: Lantern does not require cloud credentials, and its own state stays under `~/.lantern/`.

## Project Status

Lantern is usable for Palmetto-style local agent squads and is still evolving as an OSS project. The stable surface is the CLI, installer, local state model, and documented iTerm2 workflow. ACP/Goose runtime work is experimental; planning and delivery are tracked in Linear.

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

### Quick Start (Legacy Team Pattern)

```bash
lantern up
lantern startwork myproject 99 --agent claude --no-init
lantern status
lantern stopwork myproject-99
```

### Launch Patterns

`startwork` supports four patterns for squad composition. When run interactively (connected to a terminal), you'll see a menu. Non-interactive use (pipes, CI) requires the `--pattern` flag and related model/configuration flags.

#### 1. Team Orchestrator (default)
Traditional 9-pane grid: orchestrator, AI, data, platform, docs, security, ops, UI, QA, plus an input router. All panes run the same agent CLI family.

```bash
# Interactive: menu prompts for agent (Claude/Codex/Gemini)
lantern startwork myproject 99

# Non-interactive: specify agent
lantern startwork myproject 99 --pattern team --agent claude
lantern startwork myproject 99 --pattern team --agent codex
```

#### 2. Executor
Single executor worktree (70%) + a non-worktree Fable 5 XHIGH advisor (30%) + input router. Suited for focused deep-work with advisor feedback.

```bash
# Interactive: menu prompts for executor model
lantern startwork myproject 99 --pattern executor

# Non-interactive: specify executor model
lantern startwork myproject 99 --pattern executor --model "Sonnet 5 High"
lantern startwork myproject 99 --pattern executor --model claude-haiku-4-5
```

#### 3. Simple Orchestrator
Orchestrator pane (33%) + 1–10 worker panes (default 4) + input router. Orchestrator coordinates; workers execute tasks independently.

```bash
# Interactive: menus for orchestrator model, worker count, worker model
lantern startwork myproject 99 --pattern simple

# Non-interactive: specify models and worker count
lantern startwork myproject 99 --pattern simple --orch-model "Fable 5 XHIGH" --workers 6 --model "Sonnet 5 High"

# Defaults: Fable 5 XHIGH orchestrator, 4 workers, Sonnet 5 High workers
lantern startwork myproject 99 --pattern simple
```

#### 4. Fix a Bug
Single fixer worktree (full width) + input router. Targeted at a specific issue.

```bash
# Interactive: menu prompts for fixer model and issue reference
lantern startwork myproject 99 --pattern fixbug

# Non-interactive: specify fixer model and issue reference
lantern startwork myproject 99 --pattern fixbug --issue "PROJ-123" --model "Sonnet 5 High"
```

### Model Menu

All patterns use one of these model menus:

**Executor/Worker/Fixer models** (default: **Sonnet 5 High**):
- Sonnet 5 High — `claude-sonnet-5` @ high effort
- Haiku High — `claude-haiku-4-5` @ high effort
- GPT 5.5 High — `gpt-5.5` @ high effort
- GPT 5.5 Medium — `gpt-5.5` @ medium effort
- GPT 5.3 Codex Spark — `gpt-5.3-codex-spark` @ medium effort
- Gemini 3.5 Flash (High) — `gemini-3.5-flash` @ high effort
- Gemini 3.1 Pro (High) — `gemini-3.1-pro` @ high effort

**Orchestrator models** (default: **Fable 5 XHIGH**):
- Fable 5 XHIGH — `claude-fable-5` @ xhigh effort
- Opus 4.8 XHIGH — `claude-opus-4-8` @ xhigh effort

Specify models by label or model ID: `--model "Sonnet 5 High"` or `--model claude-sonnet-5`. Interactive menus show labels; non-interactive flags accept both.

### Flags Reference

- `--pattern <team|executor|simple|fixbug>`: Choose launch pattern. Skips interactive menus if set.
- `--agent <claude|codex|gemini>`: Agent CLI family (Team pattern only).
- `--model <label|id>`: Model for executor/worker/fixer panes.
- `--orch-model <label|id>`: Orchestrator model (Simple pattern only).
- `--workers <1-10>`: Worker count (Simple pattern only, default 4).
- `--issue <ref>`: Issue reference (Fix a Bug pattern only).
- `--no-init`: Skip initialization prompts.

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
