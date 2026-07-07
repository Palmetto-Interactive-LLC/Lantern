# Roadmap

This roadmap is a public planning guide, not a release commitment. Lantern is useful today as a local macOS agent-squad runtime, and the near-term work is focused on making that surface easier to install, verify, and operate.

## Stable Today

- Rust CLI binary: `lantern`
- Local install path under `~/.lantern/`
- iTerm2-based squad display and worktree launcher
- SQLite local inventory and audit projection
- Temporal dev-server integration for local workflow diagnostics
- MCP-compatible agent status, dispatch, inbox, and control helpers
- Release workflow for macOS Apple Silicon and Intel binaries

## Experimental

- ACP/Goose headless runtime
- Beads-backed task substrate replacing SQLite work item tables
- Pull-mode PR/CI loop closure
- Additional agent CLI recipes

Experimental work may change command names, config shape, or storage behavior before it is promoted to stable.

## Near-Term Priorities

1. Prove release artifacts with a clean-machine install smoke after every tag.
2. Finish the Beads/ACP runtime substrate work tracked under `lan-54v`.
3. Add a live disposable-squad smoke test runbook and evidence capture.
4. Keep CI strict: format, clippy warnings, tests, release build, action linting, and security scans.
5. Improve runtime observability around failed agent startup and MCP registration.

## Not In Scope Yet

- Hosted SaaS control plane
- Cloud credential management
- Linux or Windows squad launching
- Production Temporal cluster management
- Guaranteed behavior for every third-party agent CLI release

## Graduation Criteria

A feature moves from experimental to stable when it has:

- documented install and rollback behavior
- tests covering the normal path and at least one failure path
- CI or manual release-smoke evidence
- clear configuration defaults
- no dependency on untracked local machine state
