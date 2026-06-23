# ACP + Beads Runtime — Re-platforming Design

**Status:** Draft — 2026-06-22  
**Branch:** feat/acp-beads-runtime  
**Scope:** Lantern (pi-code-orchestrator) substrate swap + headless agent runtime

---

## Problem

Lantern currently drives 8 concurrent agents through **iTerm2 text injection**.  
This creates two structural weaknesses:

1. **Fragile transport.** The entire squad dies if the iTerm2 AppleScript socket is absent
   (headless CI, remote SSH, or simply iTerm not running). There is no graceful fallback.

2. **Duplicate substrate.** Lantern maintains its own `work_items` and `discovery_cards`
   tables in SQLite. Every other Palmetto repo (8 total) already uses `bd` (beads v1.0.5).
   We are the only repo not on beads — two separate data models for the same concepts.

---

## Goals

- Replace iTerm injection as the **primary** agent runtime with headless Goose + ACP.
- Swap `work_items` / `discovery_cards` for **beads** as the durable task+knowledge substrate.
- Keep Lantern's structural moat (blackboard, leases, nudge-clock, auto-heal, recovery,
  Temporal control plane) — none of that changes.
- Preserve iTerm as an **optional** dashboard / local-dev convenience, not a dependency.

---

## Target Architecture

```
┌─────────────────────────────────────────────────────────┐
│                        Lantern                          │
│  blackboard · leases · nudge-clock · auto-heal          │
│  Temporal workflows · recovery                          │
│                                                         │
│   ┌────────────────┐        ┌─────────────────────┐    │
│   │  Transport trait│        │   beads substrate   │    │
│   │                │        │  (git-native JSONL + │    │
│   │ ItermTransport │        │   SQLite, dep DAG)   │    │
│   │ (optional dash)│        │                      │    │
│   │ AcpTransport   │        │  bd ready            │    │
│   │ (primary)      │        │  bd create / dep     │    │
│   └──────┬─────────┘        │  bd close            │    │
│          │                  └──────────────────────┘    │
└──────────┼──────────────────────────────────────────────┘
           │ spawn
           ▼
   goose run -t "<task>"
     --with-extension 'DEVORCH_SESSION=X lantern mcp'
     (GOOSE_PROVIDER=claude-acp | codex-acp)
     (GOOSE_MODEL=opus | sonnet | haiku | gpt-5.2-codex)
           │
           ▼
   ACP agent (claude-agent-acp / codex-acp)
     rides existing ~/.claude.json or ~/.codex auth
```

**Layer responsibilities:**

| Layer | Owns | Does NOT own |
|---|---|---|
| beads | task state, dependency DAG, knowledge cards | scheduling, leases, recovery |
| Lantern | orchestration, control plane, blackboard | auth, AI credentials, raw task storage |
| ACP runtime | agent execution environment | task routing, lease management |

---

## Key Concept Mappings

### work_items → beads issues

`bd create --title <role-task> --json` replaces `INSERT INTO work_items`.  
`bd ready --json` replaces the `SELECT ... WHERE status = 'ready'` lease queries.  
`bd close <id>` replaces `UPDATE work_items SET status = 'done'`.

Lantern still holds the lease (its lease ID stored as a beads tag or metadata field);
beads is the source of truth for task existence and dependency state.

### discovery_cards → beads knowledge issues

When auto-heal produces a discovery card, Lantern calls:
```
bd create --title "discovery: <finding>" --kind knowledge
bd dep <discovery-id> discovered-from <parent-work-item-id>
```
The `discovered-from` edge type is natively supported by beads; no schema migration needed.

### auto-heal gate → beads DAG blocking

When a discovered issue must be resolved before the parent can continue:
```
bd dep <blocker-id> blocks <parent-id>
```
`bd ready` will exclude `<parent-id>` from the ready set until `<blocker-id>` is closed.
This replaces the ad-hoc SQLite gate logic in the current auto-heal path.

### deliver-to-role → goose spawn

Current: inject text into an iTerm pane addressed to a role.  
New: `goose run -t "<task prompt>" --with-extension 'DEVORCH_SESSION=<id> lantern mcp'`

The MCP extension gives the agent access to the Lantern blackboard and any tools Lantern
exposes. The session ID ties the agent run back to the Lantern lease.

Role-to-provider mapping (configurable, not hardcoded):

| Role | GOOSE_PROVIDER | GOOSE_MODEL |
|---|---|---|
| ai, doc | claude-acp | opus |
| ui, plt, dat | claude-acp | sonnet |
| qa, sec, ops | claude-acp | haiku |
| any (fallback) | codex-acp | gpt-5.2-codex |

---

## Safety Model

**Auth chain:** ACP adapters ride existing CLI auth — `claude-agent-acp` reads
`~/.claude.json`; `codex-acp` reads `~/.codex`. Goose itself stores no AI credentials
(`~/.config/goose` contains no `secrets.yaml` after runs).

**Headless guard:** All goose invocations set `GOOSE_DISABLE_KEYRING=1` to prevent
keychain prompts that would stall a headless process.

**Residual risk — ACP metering:** ACP is pre-GA and metering is paused-but-coming.
Mitigation: the `Transport` trait seam (see Phase 2) lets us swap in a `NativeCliTransport`
that invokes `claude` / `codex` directly with zero ACP overhead. The switchover is a
single config change, no Rust refactor needed.

**Blast radius:** A failing ACP spawn is a single-task failure, not a squad-level outage.
Lantern's existing lease-expiry + auto-heal loop already handles hung workers; the
transport change does not widen the failure surface — it narrows it (no shared iTerm socket).

---

## Phased Plan

### Phase 0 — beads in repo (this PR target)

- `bd` already installed (v1.0.5 Homebrew). Verify `bd ready --json` produces valid output.
- Initialize a beads repo inside `pi-code-orchestrator` (`bd init` if not present).
- Document schema conventions (tag namespace `lantern:`, metadata fields `lease_id`, `role`).
- No Rust changes. Design doc only (this file).

### Phase 1 — substrate swap

- Replace `work_items` CRUD calls with `bd` shell invocations via a `BeadsClient` wrapper
  (thin Rust struct, `std::process::Command` or `tokio::process::Command`).
- Replace `discovery_cards` inserts/queries with `bd create + bd dep discovered-from`.
- Keep SQLite migrations in place but stop writing to `work_items`/`discovery_cards`.
- All existing Lantern tests pass; add integration tests asserting beads state.

### Phase 2 — Transport trait

Introduce `trait AgentTransport { fn deliver(&self, role: &str, task: &str, session: &str) }`.

Implementations:
- `ItermTransport` — current injection logic, moved behind the trait.
- `AcpTransport` — `goose run` spawn (new).
- `NativeCliTransport` — direct `claude`/`codex` invocation (fallback, Phase 4).

Lantern configuration selects transport at startup; default switches from `iterm` to `acp`.

### Phase 3 — headless ACP runtime + recipes

- Implement `AcpTransport::deliver` — build `goose run` command, set env vars, capture
  stdout/stderr, report exit code back to Lantern lease tracking.
- Add per-role provider/model recipes (TOML config, not hardcoded).
- Wire `lantern mcp` extension into every spawn via `--with-extension`.
- End-to-end smoke test: single-agent goose run picks up a beads task, writes output,
  Lantern marks it closed.

### Phase 4 — pull-mode + PR/CI loop

- Agents poll `bd ready --json` directly for self-service task pickup (reduces push coupling).
- PR creation and CI status fed back into beads as dependency edges (`bd dep <ci-check> blocks <pr-merge>`).
- `NativeCliTransport` implemented as ACP metering fallback.
- `ItermTransport` retained but marked optional; removed from default config.

**This PR delivers: Phase 0 (design doc, beads conventions). Phases 1-4 are follow-on work.**

---

## Open Questions

1. **Lease granularity:** Does the beads issue ID become the canonical task ID, or does
   Lantern maintain its own UUID and store the beads ID as metadata? Preference: beads ID
   is canonical; Lantern UUID retained only in Temporal workflow history for correlation.

2. **Concurrent writes:** Multiple Lantern workers calling `bd` simultaneously — beads uses
   a SQLite WAL under the hood. Need to verify write contention at 8-worker concurrency
   before Phase 1 lands.

3. **Blackboard vs. beads knowledge:** Lantern's blackboard (shared scratchpad) and beads
   knowledge issues overlap in purpose. Decision deferred to Phase 1: start with knowledge
   issues for discoveries only; keep the blackboard for live shared state.

4. **goose version pin:** v1.38.0 verified installed. Lock this in `scripts/` or a
   `devenv.toml` so squad member machines don't drift.
