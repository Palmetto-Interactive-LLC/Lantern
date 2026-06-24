# How to Manage Services

Start, stop, restart, and monitor Lantern's local infrastructure: the Temporal dev server and the Relay daemon.

## Start All Services

```bash
lantern up
```

Services started:

| Service | Endpoint | Log file |
|---------|----------|----------|
| Temporal | gRPC `8243`, UI `8244` | `~/.lantern/logs/temporal.log` |
| Relay | Temporal worker, compatibility MCP, projection support | `~/.lantern/logs/relay.log` |

The Temporal dev server uses embedded file-based SQLite (`--db-filename`) for its own persistence; Lantern's projection state is a separate SQLite database. There is no PostgreSQL dependency.

On macOS, Relay runs under launchd (`com.lantern.relay`). On Linux, it runs as a background process with PID at `~/.lantern/run/relay.pid`.

## Check Health

```bash
lantern doctor
```

Address any `FAIL` lines before launching squads. A `WARN` on Temporal often means the service is stopped or still starting.

Doctor-state planning is tracked in [Doctor-state planning](../reference/doctor-state.md). The planned model will report Temporal workflow readiness, iTerm display targets, runner/MCP readiness, quarantine rows, and projection freshness.

## Stop All Services

```bash
lantern down
```

Stops Relay and Temporal. Active iTerm windows may remain visible, but runtime MCP/Temporal connectivity is not available while Relay and Temporal are down.

## Restart After Config or Code Changes

```bash
lantern restart
```

Equivalent to `lantern down` then `lantern up`.

## View Service Logs

```bash
lantern logs relay
lantern logs temporal
```

Prints the last 50 lines. For live tailing:

```bash
tail -f ~/.lantern/logs/relay.log
```

## Inspect Local Projection State

```bash
lantern status
```

Shows sessions, agents, terminal targets, work items, and recent events from SQLite. Treat this as local inventory and audit projection. Temporal workflows remain the runtime authority.

## Temporal UI

When Temporal is running, open:

```text
http://127.0.0.1:8243
```

Use workflow Queries, history, and Search Attributes there to verify runtime state.

## Manage Relay on macOS Manually

```bash
launchctl list com.lantern.relay
launchctl start com.lantern.relay
launchctl stop com.lantern.relay
```

## Legacy Note

Older service docs said active tmux squads continue running after services stop. Current squads are displayed in iTerm; the important runtime distinction is that Temporal and the orchestration client connectivity are unavailable while services are down.

## Related

- [How to troubleshoot issues](troubleshoot-issues.md)
- [Paths and environment](../reference/paths-and-environment.md)
