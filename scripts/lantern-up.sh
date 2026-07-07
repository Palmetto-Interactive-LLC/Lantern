#!/bin/bash
set -euo pipefail

LANTERN_HOME="${HOME}/.lantern"
LANTERN_DATA="${LANTERN_HOME}/data"
LANTERN_LOGS="${LANTERN_HOME}/logs"
LANTERN_RUN="${LANTERN_HOME}/run"
TEMPORAL_LABEL="com.lantern.temporal"

mkdir -p "$LANTERN_DATA/temporal" "$LANTERN_LOGS" "$LANTERN_RUN"

log() {
    echo "[$(date '+%Y-%m-%dT%H:%M:%S%z')] $*"
}

OS=$(uname -s)

temporal_launchd_pid() {
    if [[ "$OS" != "Darwin" ]]; then
        return 0
    fi
    launchctl list "$TEMPORAL_LABEL" 2>/dev/null \
        | awk -F'= ' '/"PID"/ {gsub(/[;"]/, "", $2); print $2; exit}'
}

# ------------------------------------------------------------------
# Split-Brain & Port 8243 Conflict Prevention
# ------------------------------------------------------------------
check_temporal_conflicts() {
    # 1. Check if Docker container is exposing/occupying port 8243
    if command -v docker >/dev/null 2>&1; then
        DOCKER_PORT_CONFLICT=$(docker ps --format '{{.Names}} {{.Ports}}' | grep "8243" || true)
        if [[ -n "$DOCKER_PORT_CONFLICT" ]]; then
            log "ERROR: Architecture Violation! Docker container found publishing/occupying port 8243:"
            log "       $DOCKER_PORT_CONFLICT"
            log "       Docker Temporal is NOT supported. Stop the container first."
            exit 1
        fi
    fi

    # 2. Check if another process is listening on 8243 (any interface, including IPv6)
    if command -v lsof >/dev/null 2>&1; then
        LSOF_CONFLICT=$(lsof -nP -iTCP:8243 -sTCP:LISTEN 2>/dev/null || true)
        if [[ -n "$LSOF_CONFLICT" ]]; then
            NATIVE_PID=""
            if [[ -f "$LANTERN_RUN/temporal.pid" ]]; then
                NATIVE_PID=$(cat "$LANTERN_RUN/temporal.pid")
            fi
            if [[ -z "$NATIVE_PID" ]]; then
                NATIVE_PID=$(temporal_launchd_pid)
            fi
            
            LISTENING_PIDS=$(echo "$LSOF_CONFLICT" | awk 'NR>1 {print $2}' | sort -u)
            for lpid in $LISTENING_PIDS; do
                if [[ "$lpid" != "$NATIVE_PID" ]]; then
                    if kill -0 "$lpid" >/dev/null 2>&1; then
                        log "ERROR: Architecture Violation! Duplicate/Conflicting process (PID $lpid) is occupying port 8243:"
                        log "$LSOF_CONFLICT"
                        log "       Only the native installer-managed Temporal instance is supported on 8243."
                        log "       Please stop process $lpid and free up port 8243."
                        exit 1
                    fi
                fi
            done
        fi
    fi
}

log "INFO: Checking for port 8243 conflicts..."
check_temporal_conflicts

# ------------------------------------------------------------------
# Temporal Daemon Lifecycle
# ------------------------------------------------------------------
TEMPORAL_HEALTH_CHECK_TIMEOUT=60  # seconds to wait for Temporal to become SERVING
TEMPORAL_HEALTH_CHECK_INTERVAL=2   # seconds between checks

temporal_is_serving() {
    # Query local native Temporal dev server health specifically on 127.0.0.1:8243
    temporal operator cluster health --address 127.0.0.1:8243 2>/dev/null | grep "SERVING" >/dev/null
}

temporal_needs_restart=false

# Check if stale pid exists (process died but file wasn't cleaned)
if [[ -f "$LANTERN_RUN/temporal.pid" ]]; then
    TEMPORAL_PID=$(cat "$LANTERN_RUN/temporal.pid")
    if ! kill -0 "$TEMPORAL_PID" >/dev/null 2>&1; then
        log "INFO: Temporal stale PID file found (PID $TEMPORAL_PID is dead), cleaning up..."
        rm -f "$LANTERN_RUN/temporal.pid"
        temporal_needs_restart=true
    fi
fi

# Check if already running and healthy
if [[ "$temporal_needs_restart" == "false" ]] && temporal_is_serving 2>/dev/null; then
    log "INFO: Temporal already running and healthy on 127.0.0.1:8243"
else
    log "INFO: Starting native Temporal dev server..."
    if [[ "$OS" == "Darwin" ]]; then
        TEMPORAL_BIN=$(command -v temporal || true)
        if [[ -z "$TEMPORAL_BIN" ]]; then
            log "ERROR: Temporal CLI not found. Install it with: brew install temporal"
            exit 1
        fi
        launchctl remove "$TEMPORAL_LABEL" 2>/dev/null || true
        launchctl submit \
            -l "$TEMPORAL_LABEL" \
            -o "$LANTERN_LOGS/temporal.log" \
            -e "$LANTERN_LOGS/temporal.error.log" \
            -- "$TEMPORAL_BIN" server start-dev \
            --db-filename "$LANTERN_DATA/temporal/temporal.db" \
            --ui-port 8244 \
            --ip 127.0.0.1 \
            --port 8243
        TEMPORAL_PID=$(temporal_launchd_pid)
        if [[ -n "$TEMPORAL_PID" ]]; then
            echo "$TEMPORAL_PID" > "$LANTERN_RUN/temporal.pid"
            log "INFO: Temporal submitted to launchd (PID $TEMPORAL_PID)"
        else
            rm -f "$LANTERN_RUN/temporal.pid"
            log "INFO: Temporal submitted to launchd"
        fi
    else
        # Explicitly bind native server to 127.0.0.1:8243 and UI to 8244 to prevent loopback/Docker conflicts.
        nohup temporal server start-dev \
            --db-filename "$LANTERN_DATA/temporal/temporal.db" \
            --ui-port 8244 \
            --ip 127.0.0.1 \
            --port 8243 \
            > "$LANTERN_LOGS/temporal.log" 2>&1 < /dev/null &
        TEMPORAL_PID=$!
        echo "$TEMPORAL_PID" > "$LANTERN_RUN/temporal.pid"
        log "INFO: Temporal started (PID $TEMPORAL_PID)"
    fi

    # Active health check: wait for server to become SERVING
    log "INFO: Waiting for Temporal to become SERVING (timeout: ${TEMPORAL_HEALTH_CHECK_TIMEOUT}s)..."
    ELAPSED=0
    while [[ $ELAPSED -lt $TEMPORAL_HEALTH_CHECK_TIMEOUT ]]; do
        if temporal_is_serving 2>/dev/null; then
            log "INFO: Temporal is SERVING on 127.0.0.1:8243"
            break
        fi
        sleep "$TEMPORAL_HEALTH_CHECK_INTERVAL"
        ELAPSED=$((ELAPSED + TEMPORAL_HEALTH_CHECK_INTERVAL))
    done

    if ! temporal_is_serving 2>/dev/null; then
        log "ERROR: Temporal failed to become SERVING within ${TEMPORAL_HEALTH_CHECK_TIMEOUT}s"
        log "ERROR: Check $LANTERN_LOGS/temporal.log for details"
        exit 1
    fi
fi

# ------------------------------------------------------------------
# Custom search attributes
# ------------------------------------------------------------------
log "INFO: Ensuring custom search attributes are registered..."
for sa in repo_id repo_root session run_id role transport_status message_status delivery_status; do
    temporal operator search-attribute create \
        --address 127.0.0.1:8243 --namespace default \
        --name "$sa" --type Keyword >/dev/null 2>&1 || true
done

if ! temporal_is_serving 2>/dev/null; then
    log "ERROR: Temporal stopped after startup checks"
    log "ERROR: Check $LANTERN_LOGS/temporal.log for details"
    exit 1
fi

# ------------------------------------------------------------------
# Lantern Relay
# ------------------------------------------------------------------
if [[ "$OS" == "Darwin" ]]; then
    PLIST="$HOME/Library/LaunchAgents/com.lantern.relay.plist"
    if launchctl list com.lantern.relay >/dev/null 2>&1; then
        log "INFO: Lantern Relay is managed by launchd"
    else
        if [[ -f "$PLIST" ]]; then
            log "INFO: Loading Lantern Relay via launchd..."
            launchctl load -w "$PLIST" 2>/dev/null || true
        fi
        launchctl start com.lantern.relay 2>/dev/null || true
    fi
else
    if [[ -f "$LANTERN_RUN/relay.pid" ]] && kill -0 "$(cat "$LANTERN_RUN/relay.pid")" >/dev/null 2>&1; then
        log "INFO: Lantern Relay already running (PID $(cat "$LANTERN_RUN/relay.pid"))"
    else
        log "INFO: Starting Lantern Relay..."
        lantern relay --machine "$(hostname -s)" > "$LANTERN_LOGS/relay.log" 2>&1 &
        echo $! > "$LANTERN_RUN/relay.pid"
        log "INFO: Lantern Relay started (PID $!)"
    fi
fi

# ------------------------------------------------------------------
# Status
# ------------------------------------------------------------------
log "INFO: --- Service Status ---"

if [[ -f "$LANTERN_RUN/temporal.pid" ]]; then
    log "INFO: Temporal: PID $(cat "$LANTERN_RUN/temporal.pid")"
fi

if [[ "$OS" == "Darwin" ]]; then
    if launchctl list com.lantern.relay >/dev/null 2>&1; then
        RELAY_PID=$(launchctl list com.lantern.relay 2>/dev/null \
            | awk -F'= ' '/"PID"/ {gsub(/[;"]/, "", $2); print $2; exit}')
        if [[ -n "$RELAY_PID" ]]; then
            log "INFO: Relay:    launchd PID $RELAY_PID"
        else
            log "INFO: Relay:    launchd loaded"
        fi
    else
        log "INFO: Relay:    not loaded"
    fi
else
    if [[ -f "$LANTERN_RUN/relay.pid" ]]; then
        log "INFO: Relay:    PID $(cat "$LANTERN_RUN/relay.pid")"
    fi
fi
