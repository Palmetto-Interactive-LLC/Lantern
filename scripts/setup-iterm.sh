#!/bin/bash
# setup-iterm.sh — iTerm2 + Python API + squad styling helpers for Lantern startwork.
set -euo pipefail

LANTERN_HOME="${LANTERN_HOME:-${HOME}/.lantern}"
LANTERN_BIN="${LANTERN_BIN:-${LANTERN_HOME}/bin}"
SOURCE_DIR="${SOURCE_DIR:-}"

log() {
    echo "[$(date '+%Y-%m-%dT%H:%M:%S%z')] $*" >&2
}

find_python314() {
    for candidate in \
        python3.14 \
        /opt/homebrew/bin/python3.14 \
        /usr/local/bin/python3.14; do
        if command -v "$candidate" >/dev/null 2>&1 && "$candidate" -c 'import sys; exit(0 if sys.version_info >= (3, 14) else 1)' 2>/dev/null; then
            command -v "$candidate" 2>/dev/null || echo "$candidate"
            return 0
        fi
    done
    return 1
}

ensure_python314() {
    if PY314=$(find_python314); then
        log "INFO: Python 3.14 found: $PY314 ($($PY314 --version 2>&1))"
        echo "$PY314"
        return 0
    fi

    if [[ "$(uname -s)" != "Darwin" ]]; then
        log "WARN: Python 3.14 not found (optional; kimi Code CLI works on 3.12+). Install 3.14+ manually."
        return 1
    fi

    if ! command -v brew >/dev/null 2>&1; then
        log "WARN: Homebrew not found; cannot install Python 3.14 automatically."
        return 1
    fi

    log "INFO: Installing Python 3.14 via Homebrew..."
    brew install python@3.14
    find_python314
}

ensure_iterm2_app() {
    if [[ "$(uname -s)" != "Darwin" ]]; then
        log "INFO: iTerm2 setup skipped (macOS only)"
        return 0
    fi

    if [[ -d "/Applications/iTerm.app" ]]; then
        log "INFO: iTerm2 found at /Applications/iTerm.app"
        return 0
    fi

    if ! command -v brew >/dev/null 2>&1; then
        log "WARN: iTerm2 not installed and Homebrew unavailable. Install iTerm2 from https://iterm2.com/"
        return 1
    fi

    log "INFO: Installing iTerm2 via Homebrew..."
    brew install --cask iterm2
}

install_iterm2_python_package() {
    local py="$1"
    log "INFO: Installing iterm2 Python package for $py..."
    "$py" -m pip install --user --upgrade pip >/dev/null 2>&1 || true
    if ! "$py" -m pip install --user --upgrade 'iterm2>=2.0'; then
        # Homebrew Python may require this flag on some macOS versions.
        "$py" -m pip install --user --upgrade --break-system-packages 'iterm2>=2.0'
    fi
    "$py" -c "import iterm2; print('iterm2 ok')"
}

install_iterm_scripts() {
    local src="${SOURCE_DIR}/src/startwork"
    if [[ ! -d "$src" ]]; then
        log "WARN: Source startwork scripts not found at $src"
        return 0
    fi
    mkdir -p "$LANTERN_BIN"
    for py in iterm_launch.py iterm_close.py iterm_set_titles.py; do
        if [[ -f "$src/$py" ]]; then
            cp "$src/$py" "$LANTERN_BIN/$py"
            chmod +x "$LANTERN_BIN/$py"
        fi
    done
    log "INFO: iTerm2 layout/inject/close scripts installed to $LANTERN_BIN"
}

ensure_kimi_python314() {
    if ! command -v uv >/dev/null 2>&1; then
        log "INFO: uv not found — skipping kimi-cli Python 3.14 pin (install uv for kimi squads)"
        return 0
    fi
    if ! command -v kimi >/dev/null 2>&1 && ! uv tool list 2>/dev/null | grep -q kimi-cli; then
        log "INFO: kimi not installed — skipping kimi-cli upgrade"
        return 0
    fi
    log "INFO: Ensuring kimi-cli is installed (Kimi Code CLI for startwork kimi)"
    uv tool install kimi-cli --python 3.14 --force >/dev/null 2>&1 || \
        log "WARN: Could not upgrade kimi-cli to Python 3.14"
}

ensure_kimi_devorch_mcp() {
    if ! command -v kimi >/dev/null 2>&1; then
        return 0
    fi
    local mcp_bin="${LANTERN_BIN}/lantern"
    if [[ ! -x "$mcp_bin" ]]; then
        log "WARN: $mcp_bin missing — Kimi panes cannot call devorch MCP"
        return 0
    fi
    if kimi mcp list 2>/dev/null | grep -q devorch; then
        log "INFO: devorch MCP already registered for kimi"
        return 0
    fi
    log "INFO: Registering devorch MCP for kimi (lantern mcp)..."
    kimi mcp add devorch -- "$mcp_bin" mcp >/dev/null 2>&1 || \
        log "WARN: kimi mcp add devorch failed"
}

configure_iterm_squad_defaults() {
    local domain="com.googlecode.iterm2"
    # Tab bar on top (not left sidebar); hide tab bar for single-tab squad windows
    defaults write "$domain" TabViewType -int 0 2>/dev/null || true
    defaults write "$domain" HideTab -bool true 2>/dev/null || true
    defaults write "$domain" "Default Toolbelt Width" -int 0 2>/dev/null || true
    defaults write "$domain" ShowPaneTitles -bool false 2>/dev/null || true
    log "INFO: iTerm2 squad UI defaults (top tabs, no left sidebar, minimal toolbelt)"
}

verify_iterm_api() {
    local py="$1"
    if ! pgrep -x iTerm2 >/dev/null 2>&1 && ! pgrep -x iTerm >/dev/null 2>&1; then
        log "INFO: iTerm2 is not running — start iTerm2 before first startwork"
        log "INFO: Enable Python API: iTerm2 → Settings → General → Magic → Enable Python API"
        return 0
    fi

    log "INFO: Probing iTerm2 Python API..."
    if "$py" - <<'PY' 2>/dev/null; then
import asyncio
import iterm2

async def probe(connection):
    await iterm2.async_get_app(connection)

iterm2.run_until_complete(probe)
print("ok")
PY
        log "INFO: iTerm2 Python API connection OK"
    else
        log "WARN: iTerm2 Python API probe failed."
        log "WARN: Open iTerm2 → Settings → General → Magic → Enable Python API, then retry startwork"
    fi
}

main() {
    if [[ "$(uname -s)" != "Darwin" ]]; then
        log "INFO: iTerm2 squad launcher is macOS-only; skipping iTerm setup"
        return 0
    fi

    log "INFO: Setting up iTerm2 for Lantern squads..."
    ensure_iterm2_app || true
    configure_iterm_squad_defaults
    ensure_kimi_devorch_mcp

    install_iterm_scripts

    local py
    if ! py=$(ensure_python314); then
        py="$(command -v python3 || true)"
        log "WARN: Falling back to $py for iterm2 package"
    fi

    if [[ -n "$py" ]]; then
        install_iterm2_python_package "$py"
        ensure_kimi_python314
    verify_iterm_api "$py" || true
    else
        log "ERROR: No Python interpreter found for iterm2 package"
        return 1
    fi

    log "INFO: iTerm2 setup complete"
}

main "$@"
