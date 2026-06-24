# Temporal-Owned Runtime Resilience Verification

This report is the final evidence artifact for issue #32 and epic #1. It verifies that Lantern's runtime control paths have moved to Temporal workflows, while Lantern remains a launcher, local audit/inventory tool, iTerm helper, and local activity host.

## Scope Verified

- Rust Lantern no longer owns runtime message routing, peer delivery, queue leasing, or terminal fallback injection.
- Runtime MCP/status/message/control operations are represented by Temporal workflow Updates, Signals, or Queries through the orchestration client.
- SQLite remains useful for inventory, audit projection, stale-state diagnosis, and quarantine records, but not runtime routing authority.
- iTerm remains launch/display/close support. Runtime delivery is guarded by Temporal execution-window state and runner readiness.
- Legacy tmux-era data is quarantined or migrated as historical compatibility data.

## Command Evidence

| Command | Result | Captured Output Summary |
|---------|--------|-------------------------|
| `cargo test --no-run` | Pass | Rust test binaries built successfully. Warnings are non-fatal and mostly existing unused/dead-code warnings from the broad migration surface. |
| `cargo test` | Pass | `src/main.rs`: 56 passed; `tests/cli.rs`: 4 passed; `tests/migration_quarantine.rs`: 1 passed. Total: 61 passed, 0 failed. |
| `pnpm typecheck` | Pass | 3/3 workspace projects passed: `packages/devorch-contracts`, `packages/devorch-workflows`, `apps/temporal-worker`. |
| `pnpm test` | Pass | 4 test files, 25 tests passed. Contracts: 1 test. Workflows: 22 tests. Worker: 2 tests. |

The first full Rust CLI run exposed tests that read the operator's real Lantern database. Those tests now isolate `HOME` with temporary directories, and the final full `cargo test` pass above used the isolated path.

## Static Gates

### tmux and Terminal-Control Tokens

Command:

```bash
rg -n "tmux|send-keys|capture-pane|kill-session" src || true
```

Result: no active terminal-control commands remain. The remaining matches are quarantine, migration compatibility, or negative CLI tests:

- `src/doctor_state.rs` detects and quarantines legacy tmux-like terminal target rows.
- `src/db/queries.rs` writes legacy tmux fields only into `terminal_target_quarantine`.
- `src/main.rs` asserts `stopwork` rejects `--tmux-session`.
- No `send-keys`, `capture-pane`, or `kill-session` matches remain in `src`.

### Removed Runtime Delivery Tokens

Command:

```bash
rg -n "send_to_agent|safe_inject|enqueue_peer_delivery|QueueManager|DeliveryOrchestrator" src || true
```

Result: no matches.

### Legacy tmux Schema Tokens

Command:

```bash
rg -n "tmux_send_keys|tmux_session|tmux_pane" migrations src || true
```

Result: matches are migration/quarantine compatibility only:

- `migrations/001_initial.sql` contains original historical schema fields.
- `migrations/003_iterm_terminal_targets.sql` migrates historical tmux fields into iTerm-neutral target rows and quarantine records.
- `src/db/queries.rs` stores legacy values in `terminal_target_quarantine`.

### Direct iTerm Injection Tokens

Command:

```bash
rg -n "iterm_inject|send_text\(|send_agent_prompt|send_to_iterm\(" src scripts docs README.md HANDOFF.md || true
```

Result: runtime injection helpers are removed. The remaining `async_send_text` calls are launch-only:

- `src/startwork/iterm_batch_init.py` sends startup commands during initial iTerm pane bootstrap.
- `src/startwork/iterm_launch.py` sends the process launch command to a newly created iTerm session.
- `src/startwork/iterm_inject.py` was deleted.

## Temporal Workflow Evidence

Workflow coverage lives in `packages/devorch-workflows/src/workflow-behavior.test.ts` and `packages/devorch-workflows/src/index.ts`.

| Required Scenario | Evidence |
|-------------------|----------|
| Two repos with the same session number cannot cross-route | `SessionMessageBusWorkflow behavior` rejects same-session cross-repo messages and uses hard-scoped IDs under `devorch/{repo_id}/{session}/{run_id}`. |
| Missing or wrong repo identity is rejected before message acceptance | Message bus, setup, cleanup, audit shadow, execution window, runner lease, and MCP setup workflows call shared identity validation before accepting state changes. |
| Peer message survives relay/worker restart | `SessionMessageBusWorkflow` and `MessageDeliveryWorkflow` tests replay persisted ledger and delivery snapshots to simulate workflow reactivation. |
| Pane closed creates pending/degraded Temporal delivery, not fallback injection | `ExecutionWindowWorkflow` keeps delivery pending/degraded until runner, MCP, and transport are ready; it does not route to another terminal target. |
| Stale DB target never routes to another iTerm window | Execution-window tests keep stale targets pending/degraded, and doctor-state quarantines stale/legacy local targets instead of repairing them into active delivery routes. |
| Orchestrator cwd is its own worktree | Startwork now creates and launches the orchestrator from its own worktree while root repo state remains launcher/control metadata. |
| MCP down recovers through Temporal workflow state | `McpSetupWorkflow` and `McpRecoveryWorkflow` represent missing tools and recovered registration in workflow state. |
| Runner heartbeat expiry marks role degraded and recovery is Temporal-led | `RunnerLeaseWorkflow` marks stale leases after heartbeat timeout; supervisor recovery emits Temporal recovery requests instead of terminal writes. |
| Message compression does not compact pending/unacked messages | `MessageCompressionWorkflow` compacts only accepted non-control messages and preserves rejected/control entries; pending delivery state remains outside the compaction set. |
| Stopwork is idempotent and leaves no running session workflows | `SessionCleanupWorkflow` handles repeated cleanup requests deterministically, and Rust `stopwork` signals cleanup while closing iTerm and preserving/cleaning worktrees by flag. |

## Epic Acceptance Mapping

| Acceptance Criterion | Evidence |
|----------------------|----------|
| No tmux runtime code remains | `src/tmux/**`, tmux supervisor checks, and tmux recovery modules were deleted. Remaining tmux strings are quarantine/migration compatibility or CLI rejection tests. |
| No local message delivery remains | Peer delivery queues, scheduler/work queues, `DeliveryOrchestrator`, and local MCP-to-worker queues were removed. Static token gate has no matches. |
| All MCP/status/message/control operations are Temporal workflow Updates, Signals, or Queries | TypeScript workflow contracts implement message bus, delivery, state, execution window, leases, MCP setup/recovery, human control, compression, and audit projection. Rust human/startwork/stopwork paths build Temporal signal/update requests. |
| SQLite is only inventory/audit and never runtime routing authority | New schema uses iTerm-neutral terminal target projection plus quarantine rows. Doctor-state and supervisor classify stale DB state as diagnostic/audit data. |
| iTerm is only launch/display/close and never fallback injection | Direct injection helper and terminal send wrappers were removed. Remaining iTerm text sends are startup-only launch commands. Runtime delivery is gated by `ExecutionWindowWorkflow`. |
| Restart and stale-state tests pass | Workflow replay tests cover restart durability; execution-window, doctor-state, cleanup, and migration tests cover stale-state behavior. |
| Required static gates and resilience scenarios are documented | This report records Rust, TypeScript, static grep, and resilience scenario evidence. |

## Issue Closure Evidence

- Wave 0 issues #2 through #5 are closed with contracts, ADR, issue templates, and TypeScript workspace scaffold in the tree.
- Wave 1 issues #6 through #10 are closed with unsafe runtime paths removed.
- Wave 2 issues #11 through #19 are closed with Temporal workflow implementations and workflow tests.
- Wave 3 issues #20 through #26 are closed with schema migration, identity export, startwork, supervisor, human command, stopwork, and doctor-state changes.
- Wave 4 validation/docs issues #27 through #31 are closed with isolation, restart, failure-mode, cleanup/stale-state, and docs/runbook evidence.
- Issue #32 is satisfied by this report.

## Residual Notes

- Build output under `target/` is intentionally ignored for implementation scope.
- Existing local `.gemini/skills/orchestrator/SKILL.md` and `.kimi/skills/orchestrator/SKILL.md` changes are unrelated to this rollout and are not part of the evidence set.
- Rust warnings remain non-fatal and should be handled as a cleanup pass after the runtime migration lands.
