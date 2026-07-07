#!/bin/bash
set -euo pipefail

LANTERN_HOME="${HOME}/.lantern"
LANTERN_RUN="${LANTERN_HOME}/run"
TEMPORAL_LABEL="com.lantern.temporal"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

log() {
    echo "[$(date '+%Y-%m-%dT%H:%M:%S%z')] $*"
}

status_ok() {
    printf "${GREEN}OK${NC}    %s\n" "$1"
}

status_warn() {
    printf "${YELLOW}WARN${NC}  %s\n" "$1"
}

status_err() {
    printf "${RED}FAIL${NC}  %s\n" "$1"
}

OS=$(uname -s)

temporal_launchd_pid() {
    if [[ "$OS" != "Darwin" ]]; then
        return 0
    fi
    launchctl list "$TEMPORAL_LABEL" 2>/dev/null \
        | awk -F'= ' '/"PID"/ {gsub(/[;"]/, "", $2); print $2; exit}'
}

log "INFO: Running Lantern health checks..."

# ------------------------------------------------------------------
# Temporal Dedicated Control Plane Verification
# ------------------------------------------------------------------
temporal_conflict=false

# 1. Docker check specifically for our isolated port 8243
if command -v docker >/dev/null 2>&1; then
    DOCKER_CONFLICT=$(docker ps --format '{{.Names}} {{.Ports}}' | grep "8243" || true)
    if [[ -n "$DOCKER_CONFLICT" ]]; then
        status_err "Docker container is exposing dedicated port 8243! Docker Temporal is NOT supported."
        status_err "      Conflict details: $DOCKER_CONFLICT"
        temporal_conflict=true
    fi
fi

# 2. Port listening checks specifically for our isolated port 8243
if command -v lsof >/dev/null 2>&1; then
    LSOF_OUT=$(lsof -nP -iTCP:8243 -sTCP:LISTEN 2>/dev/null || true)
    if [[ -n "$LSOF_OUT" ]]; then
        NATIVE_PID=""
        if [[ -f "$LANTERN_RUN/temporal.pid" ]]; then
            NATIVE_PID=$(cat "$LANTERN_RUN/temporal.pid")
        fi
        if [[ -z "$NATIVE_PID" ]]; then
            NATIVE_PID=$(temporal_launchd_pid)
        fi

        LISTENING_PIDS=$(echo "$LSOF_OUT" | awk 'NR>1 {print $2}' | sort -u)
        for lpid in $LISTENING_PIDS; do
            if [[ "$lpid" != "$NATIVE_PID" ]]; then
                if kill -0 "$lpid" >/dev/null 2>&1; then
                    status_err "Duplicate listener on port 8243 detected (PID: $lpid)!"
                    status_err "      Active process output:"
                    status_err "      $(ps -p "$lpid" -o pid,comm,args | tail -n1)"
                    temporal_conflict=true
                fi
            fi
        done
    fi
fi

# 3. Connection & Health Checks
if [[ "$temporal_conflict" == "true" ]]; then
    status_err "Temporal configuration has CRITICAL violations! Free up port 8243."
elif command -v temporal >/dev/null 2>&1; then
    # Verify strict connection only to 127.0.0.1:8243
    if temporal operator cluster health --address 127.0.0.1:8243 >/dev/null 2>&1; then
        # Check namespace accessibility
        if temporal operator namespace list --address 127.0.0.1:8243 | grep "default" >/dev/null 2>&1; then
            status_ok "Temporal Control Plane (127.0.0.1:8243 - Dedicated Native)"
        else
            status_warn "Temporal running at 127.0.0.1:8243 but 'default' namespace is missing/unreachable"
        fi
    else
        status_warn "Temporal connectivity (native server at 127.0.0.1:8243 may be stopped)"
    fi
else
    status_err "Temporal CLI not found"
fi

# ------------------------------------------------------------------
# Lantern Relay
# ------------------------------------------------------------------
relay_ok=false
if [[ -f "$LANTERN_RUN/relay.pid" ]]; then
    PID=$(cat "$LANTERN_RUN/relay.pid")
    if kill -0 "$PID" >/dev/null 2>&1; then
        status_ok "Lantern Relay (PID $PID)"
        relay_ok=true
    else
        status_warn "Lantern Relay (stale PID file)"
    fi
elif [[ -S "$LANTERN_RUN/relay.sock" ]]; then
    status_ok "Lantern Relay (socket present)"
    relay_ok=true
fi

if [[ "$relay_ok" == "false" && "$OS" == "Darwin" ]]; then
    if launchctl list com.lantern.relay >/dev/null 2>&1; then
        status_ok "Lantern Relay (launchd loaded)"
    else
        status_warn "Lantern Relay not running"
    fi
elif [[ "$relay_ok" == "false" ]]; then
    status_warn "Lantern Relay not running"
fi

# ------------------------------------------------------------------
# git
# ------------------------------------------------------------------
if command -v git >/dev/null 2>&1; then
    status_ok "git ($(git --version 2>/dev/null || echo 'unknown version'))"
else
    status_err "git not found"
fi

# ------------------------------------------------------------------
# iTerm2 squad launcher (macOS)
# ------------------------------------------------------------------
if [[ "$OS" == "Darwin" ]]; then
    if [[ -d "/Applications/iTerm.app" ]]; then
        status_ok "iTerm2 app"
    else
        status_err "iTerm2 not installed (run: lantern-setup-iterm)"
    fi

    for script in iterm_launch.py iterm_close.py; do
        if [[ ! -x "${LANTERN_HOME}/bin/${script}" ]]; then
            status_err "iTerm script missing: ${LANTERN_HOME}/bin/${script}"
        fi
    done
    if [[ -x "${LANTERN_HOME}/bin/iterm_launch.py" ]]; then
        status_ok "iTerm layout scripts"
    fi

    PY314=""
    for candidate in python3.14 /opt/homebrew/bin/python3.14 /usr/local/bin/python3.14; do
        if command -v "$candidate" >/dev/null 2>&1 && "$candidate" -c 'import sys; exit(0 if sys.version_info >= (3, 14) else 1)' 2>/dev/null; then
            PY314=$(command -v "$candidate" 2>/dev/null || echo "$candidate")
            break
        fi
    done
    if [[ -n "$PY314" ]]; then
        status_ok "Python 3.14 ($("$PY314" --version 2>&1 | head -1))"
    else
        status_warn "Python 3.14 not found (required for kimi term)"
    fi

    ITerm_PY="${PY314:-python3}"
    if "$ITerm_PY" -c "import iterm2" >/dev/null 2>&1; then
        status_ok "iterm2 Python package"
    else
        status_err "iterm2 Python package missing (run: lantern-setup-iterm)"
    fi

    if pgrep -x iTerm2 >/dev/null 2>&1 || pgrep -x iTerm >/dev/null 2>&1; then
        if "$ITerm_PY" - <<'PY' >/dev/null 2>&1; then
import asyncio
import iterm2

async def probe(connection):
    await iterm2.async_get_app(connection)

iterm2.run_until_complete(probe)
PY
            status_ok "iTerm2 Python API"
        else
            status_warn "iTerm2 Python API not reachable (enable in Settings → General → Magic)"
        fi
    else
        status_warn "iTerm2 not running (start before startwork)"
    fi

    if command -v kimi >/dev/null 2>&1; then
        KIMI_PY=$(head -1 "$(command -v kimi)" | sed 's/^#!//')
        if [[ -x "$KIMI_PY" ]] && "$KIMI_PY" -c 'import sys; exit(0 if sys.version_info >= (3, 14) else 1)' 2>/dev/null; then
            status_ok "kimi-cli (Python 3.14+)"
        else
            status_warn "kimi-cli needs Python 3.14+ (run: lantern-setup-iterm)"
        fi
    fi
fi

log "INFO: Health check complete"
