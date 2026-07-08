# CLAUDE.md — Lantern

## What This Repo Is

Lantern is a self-contained Rust binary providing local orchestration for AI coding squads. It runs on each developer machine as an MCP server, local runner (iTerm2/worktrees), and Temporal client. SQLite is the local state store.

No cloud dependency. No secrets. No credentials required.

## Build & Test

```bash
# Build
cargo build --release

# Test
cargo test

# Lint
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Install the binary and register the launchd service:

```bash
./scripts/install.sh
source ~/.zshrc
```

Reinstall after code changes:

```bash
cargo build --release
cp target/release/lantern ~/.lantern/bin/lantern
lantern restart
```

## Secrets

None. This repo has no secrets, no cloud credentials, and no secret-management wiring.

Do not add `.op-environment`, `.envrc`, or SOPS config — this is a local CLI tool with no remote cloud or secret dependencies.

## Architecture

- Single Rust binary (`lantern`) installed to `~/.lantern/bin/`
- MCP server: serves `devorch_report_status`, `devorch_peer_message`, `devorch_query_team_state`, `devorch_get_setup_instructions`
- Local runner: manages iTerm2 terminal panes and git worktrees
- Temporal client: connects to local Temporal dev server at `127.0.0.1:8243`
- SQLite: local projection/audit state at `~/.lantern/data/relay/lantern.db` (not runtime authority)

Docker Temporal is strictly unsupported.

## Launch Patterns

`startwork` dispatches on one of four squad configurations, each with its own pane layout, role specs, and model menu:

### Pattern 1: Team Orchestrator
The legacy 9-pane grid: 3 columns (orch+input, then two 4-pane columns). All panes use the same agent CLI family (`--agent claude|codex|gemini`). Behavior is byte-identical to the main-branch team launch.

**Flags:**
- `--pattern team`
- `--agent claude|codex|gemini` (default: claude)

**Layout:** 3 columns (33/33/34 weight)
- Column 1: `orch`, `inp`
- Column 2: `ai`, `dat`, `plt`, `doc`
- Column 3: `sec`, `ops`, `ui`, `qa`

### Pattern 2: Executor
Focused single-pane workflow: 70% executor worktree + 30% non-worktree advisor (Fable 5 XHIGH, implicit, not selectable) + input router stacked below executor.

**Flags:**
- `--pattern executor`
- `--model <label|id>` (default: Sonnet 5 High)

**Layout:** 2 columns
- Column 1 (70%): `executor`, `inp`
- Column 2 (30%): `advisor`

**Advisor Model:** Always `claude-fable-5` @ xhigh. Rides alongside without claiming a worktree slot.

### Pattern 3: Simple Orchestrator
Coordinated multi-worker: 1 orchestrator + 1–10 workers (default 4) + input router. Orchestrator picks from orchestrator menu (Fable 5 XHIGH | Opus 4.8 XHIGH); workers pick from executor menu.

**Flags:**
- `--pattern simple`
- `--orch-model <label|id>` (default: Fable 5 XHIGH)
- `--model <label|id>` (default: Sonnet 5 High)
- `--workers 1-10` (default: 4)

**Layout:**
- Workers ≤ 5: 2 columns (33/67 weight)
  - Column 1: `orch`, `inp`
  - Column 2: worker panes
- Workers > 5: 3 columns (33/34/33 weight)
  - Column 1: `orch`, `inp`
  - Column 2–3: workers split across columns

**Worker Names:** `worker-1`, `worker-2`, … with distinct RGB colors from the team palette.

### Pattern 4: Fix a Bug
Targeted single-pane fix: 100% fixer worktree + input router stacked below.

**Flags:**
- `--pattern fixbug`
- `--issue <ref>` (required)
- `--model <label|id>` (default: Sonnet 5 High)

**Layout:** 1 column (100% weight)
- Column 1: `fixer`, `inp`

### Model Menus

**Executor/Worker/Fixer Menu** (first entry is default):
1. Sonnet 5 High — `claude-sonnet-5` @ high
2. Haiku High — `claude-haiku-4-5` @ high
3. GPT 5.5 High — `gpt-5.5` @ high
4. GPT 5.5 Medium — `gpt-5.5` @ medium
5. GPT 5.3 Codex Spark — `gpt-5.3-codex-spark` @ medium
6. Gemini 3.5 Flash (High) — `gemini-3.5-flash` @ high
7. Gemini 3.1 Pro (High) — `gemini-3.1-pro` @ high

**Orchestrator Menu** (first entry is default):
1. Fable 5 XHIGH — `claude-fable-5` @ xhigh
2. Opus 4.8 XHIGH — `claude-opus-4-8` @ xhigh

### Interaction Modes

**Interactive (tty-attached, no `--pattern`):**
- Menu 1: Select pattern (Team / Executor / Simple / Fix a Bug)
- Menu 2+: Pattern-specific prompts (agent, model, worker count, issue, etc.)

**Non-Interactive (`--pattern` set):**
- Fully declarative: all required flags must be supplied or defaults apply
- Errors if a required flag is missing (e.g., `--pattern fixbug` without `--issue`)
- No prompts, suitable for CI/scripts

**Scripted (no `--pattern`, stdin not tty):**
- Defaults to `team + claude` (legacy `startwork` behavior)

### Environment Export

Every pane's environment receives `DEVORCH_PATTERN=<slug>` (`team`, `executor`, `simple`, `fixbug`), enabling agents to self-configure based on squad shape.

### Models Freshness

The `models.json` manifest at the repo root is canon; `src/models_registry.rs` keeps installs current:
- Cached locally at `~/.lantern/data/models_cache.json` with 24-hour freshness
- Non-blocking freshness check on `lantern up` and `lantern doctor` (never fails offline)
- Manual refresh: `lantern models sync` (or `--sync`); `lantern models` shows compiled-in vs cached
- Weekly GitHub Action (`models-manifest.yml`) scrapes public model docs and PRs manifest updates
- Menus prefer the cached manifest and fall back to the compiled-in table

## Key Commands

### Service Management
```bash
lantern up                                        # Start background services (Temporal + relay)
lantern down                                      # Stop background services
lantern restart                                   # Restart background services
lantern doctor                                    # Health check all local dependencies
lantern logs <relay|temporal>                     # Tail service logs
```

### Squad Control
```bash
lantern startwork <project> <slot> [--pattern <pattern>] [flags]
                                                  # Launch a squad (interactive or --pattern for non-interactive)
lantern stopwork [session]                        # Tear down a squad
lantern stopwork --all                            # Stop all active squads
lantern stopwork --list                           # List active squads
lantern status                                    # Show all squads and agents from SQLite
```

### Agent Control
```bash
lantern pause <agent>                             # Pause an agent pane
lantern resume <agent>                            # Resume a paused agent
lantern takeover <agent>                          # Human takes manual control
lantern release <agent>                           # Release manual control back to agent
lantern recover <agent>                           # Force recovery of an agent
lantern note <agent> <message>                    # Inject a note into an agent pane
```

### MCP & Introspection
```bash
lantern mcp                                       # Start MCP server (stdio, for agent CLIs)
lantern models                                    # Display available model menu
lantern models sync                               # Refresh model freshness (24h cache)
```

## CI

GitHub Actions runs Rust formatting, clippy, release build, tests, security scans, action linting, CodeQL, and release packaging. Branch protection requires the `lint`, `build-test`, `secrets-scan`, `sast`, `deps-scan`, `iac-scan`, and `actions-lint` contexts.

## Repository

- Remote: `git@github-palmetto:Palmetto-Interactive-LLC/Lantern.git`
- SSH alias: `github-palmetto`
- Org: Palmetto-Interactive-LLC


<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:6cd5cc61 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->

<!-- BEGIN GITHUB SSH TRANSPORT POLICY v:1 -->
## GitHub SSH Transport Policy

GitHub Git transport is SSH-only through the configured per-account host aliases.
Before any GitHub operation, run `git remote -v` and use that remote exactly for
`git fetch`, `git pull`, `git push`, and Beads/Dolt sync.

Allowed canonical GitHub SSH aliases:

- `git@github-meridian7:...`
- `git@github-palmetto:...`
- `git@github-personal:...`
- `git@github-shelterfitness:...`

Never rewrite a GitHub remote to HTTPS. Never use `https://github.com/...` for
Git transport. Never use direct `git@github.com:...`. Treat legacy duplicate
aliases such as `github.com-client`, `github.com-work`, and `github.com-primary`
as drift and normalize them to the canonical aliases above.

`gh` and GitHub API auth are separate from Git transport. A broken or wrong
`gh` account does not block branch fetch/push when SSH works. Use `gh` only for
PR/API operations, and prove Git access with `git ls-remote <remote-url>` or
`ssh -T git@<alias>`, not with `gh auth status`.

Reference: `/Users/matt/Development/AGENT-GITHUB-MODEL.md`.
<!-- END GITHUB SSH TRANSPORT POLICY -->
