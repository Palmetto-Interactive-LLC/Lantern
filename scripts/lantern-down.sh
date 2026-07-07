#!/bin/bash
set -euo pipefail

LANTERN_HOME="${HOME}/.lantern"
LANTERN_RUN="${LANTERN_HOME}/run"
TEMPORAL_LABEL="com.lantern.temporal"

log() {
    echo "[$(date '+%Y-%m-%dT%H:%M:%S%z')] $*"
}

OS=$(uname -s)

# ------------------------------------------------------------------
# Lantern Relay
# ------------------------------------------------------------------
if [[ "$OS" == "Darwin" ]]; then
    log "INFO: Stopping Lantern Relay via launchd..."
    launchctl stop com.lantern.relay 2>/dev/null || true
    launchctl unload -w "$HOME/Library/LaunchAgents/com.lantern.relay.plist" 2>/dev/null || true
else
    if [[ -f "$LANTERN_RUN/relay.pid" ]]; then
        PID=$(cat "$LANTERN_RUN/relay.pid")
        if kill -0 "$PID" >/dev/null 2>&1; then
            log "INFO: Stopping Lantern Relay (PID $PID)..."
            kill "$PID" || true
            sleep 1
        fi
        rm -f "$LANTERN_RUN/relay.pid" "$LANTERN_RUN/relay.sock"
    fi
fi

# ------------------------------------------------------------------
# Temporal
# ------------------------------------------------------------------
if [[ "$OS" == "Darwin" ]]; then
    log "INFO: Stopping Temporal via launchd..."
    launchctl remove "$TEMPORAL_LABEL" 2>/dev/null || true
    rm -f "$LANTERN_RUN/temporal.pid"
elif [[ -f "$LANTERN_RUN/temporal.pid" ]]; then
    PID=$(cat "$LANTERN_RUN/temporal.pid")
    if kill -0 "$PID" >/dev/null 2>&1; then
        log "INFO: Stopping Temporal (PID $PID)..."
        kill "$PID" || true
        sleep 1
    fi
    rm -f "$LANTERN_RUN/temporal.pid"
fi

log "INFO: All services stopped"
