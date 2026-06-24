# How to Develop and Contribute

Build, test, and extend Lantern Relay.

## Set Up the Repository

```bash
git clone https://github.com/Palmetto-Interactive-LLC/pi-code-orchestrator.git
cd pi-code-orchestrator
cargo build
```

Run without installing:

```bash
cargo run -- status
cargo run -- relay --machine dev
```

## Run Tests

```bash
cargo test
cargo test --test cli
cargo clippy
cargo fmt --check
```

## Install a Local Build

```bash
cargo build --release
cp target/release/lantern ~/.lantern/bin/lantern
lantern restart
```

## Inspect Local Projection Data During Development

```bash
sqlite3 ~/.lantern/data/relay/lantern.db
```

Useful queries:

```sql
SELECT id, status FROM sessions;
SELECT id, role, status, pane_id FROM agents;
SELECT agent_id, iterm_session_id, transport_status FROM terminal_targets;
SELECT agent_id, quarantine_reason FROM terminal_target_quarantine;
SELECT * FROM events ORDER BY created_at DESC LIMIT 10;
```

Reset local projection data:

```bash
rm ~/.lantern/data/relay/lantern.db
```

Do not treat SQLite as runtime authority. It is inventory, audit projection, quarantine, and future doctor-state support.

## Test Startwork Manually

Requires full host prerequisites:

```bash
cd /path/to/test-repo
lantern startwork testproj 99 --no-init
lantern status
lantern stopwork testproj-99
```

Use Temporal UI to inspect runtime workflow state:

```text
http://127.0.0.1:8244
```

## Project Layout

```text
src/
├── main.rs           CLI entry + command dispatch
├── startwork/        iTerm display launcher + worktree setup
├── stopwork/         Squad teardown
├── terminal/         iTerm helper integration
├── db/               SQLite projection, inventory, quarantine queries
├── supervisor/       Local checks during migration
├── temporal/         Worker/client integration
├── mcp/              JSON-RPC compatibility; runtime tools disabled
├── human/            Human command entrypoints
└── recovery/         Recovery entrypoints
packages/
├── devorch-contracts/   Runtime contracts
└── devorch-workflows/   Temporal workflow definitions
migrations/             SQLite schema and quarantine migrations
scripts/                Install and service scripts
tests/                  Integration tests
```

## Add a New CLI Command

1. Add a variant to `Commands` in `src/main.rs`.
2. Add the match arm in `main()`.
3. Implement the handler in `commands` or a submodule.
4. Add an integration test in `tests/cli.rs`.

## Add or Change Runtime MCP Behavior

Runtime MCP authority belongs to the orchestration layer, not Lantern Rust MCP.

For runtime behavior:

1. Update the relevant Temporal workflow contract.
2. Map agent MCP calls in the orchestration client.
3. Keep Lantern Rust MCP compatibility responses aligned with [MCP runtime authority](../reference/mcp-tools.md).
4. Document the workflow Update, Signal, or Query that owns the behavior.

Do not add new local Rust MCP tools for peer messages, status, team state, setup instructions, or human control.

## Add a Database Migration

1. Create `migrations/NNN_*.sql`.
2. Make the migration projection/audit/quarantine focused unless a local support table is explicitly needed.
3. Add queries in `src/db/queries.rs`.
4. Add unit tests in the queries test module.
5. Update [Database schema](../reference/database-schema.md).

## Code Conventions

- Tokio async throughout; `tokio::process::Command` for subprocesses.
- `anyhow::Result` at boundaries.
- `tracing` for logging.
- `generate_id(prefix)` for IDs.
- Parameterized sqlx queries only.

## Legacy Notes

The Python workbench and tmux launcher are legacy references only. Do not diff current startup behavior against tmux command sequences as part of normal development. Use iTerm launcher behavior, Temporal workflow history, and doctor-state planning instead.

## Related

- [Architecture](../explanation/architecture.md)
- [CLI reference](../reference/cli.md)
- [Doctor-state planning](../reference/doctor-state.md)
