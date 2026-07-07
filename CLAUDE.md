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

## Key Commands

```bash
lantern up          # Start background services (Temporal dev server + relay)
lantern down        # Stop background services
lantern doctor      # Health check all local dependencies
lantern status      # Show local inventory from SQLite
lantern startwork <project> <slot> --agent claude   # Launch a squad
lantern stopwork <project>-<slot>                   # Tear down a squad
lantern logs <relay|temporal>                       # Tail service logs
lantern mcp         # Start MCP server (for agents)
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
