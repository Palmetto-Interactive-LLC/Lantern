#!/bin/bash
set -euo pipefail

LANTERN_HOME="${HOME}/.lantern"
LANTERN_BIN="${LANTERN_HOME}/bin"
LANTERN_DATA="${LANTERN_HOME}/data"
LANTERN_LOGS="${LANTERN_HOME}/logs"
LANTERN_CONFIG="${LANTERN_HOME}/config"
LANTERN_RUN="${LANTERN_HOME}/run"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

log() {
    echo "[$(date '+%Y-%m-%dT%H:%M:%S%z')] $*"
}

trap 'log "ERROR: Install failed on line $LINENO"' ERR

OS=$(uname -s)
ARCH=$(uname -m)
HOSTNAME=$(hostname -s)

log "INFO: Starting Lantern install on $OS ($ARCH)"

# Create directory structure
log "INFO: Creating Lantern directory structure at $LANTERN_HOME"
mkdir -p "$LANTERN_BIN" "$LANTERN_DATA/temporal" \
    "$LANTERN_DATA/relay" "$LANTERN_LOGS" "$LANTERN_CONFIG" "$LANTERN_RUN"

# ------------------------------------------------------------------
# Rust toolchain
# ------------------------------------------------------------------
RUST_OK=false
if command -v rustc >/dev/null 2>&1 && command -v cargo >/dev/null 2>&1; then
    if rustc --version >/dev/null 2>&1 && cargo --version >/dev/null 2>&1; then
        log "INFO: Rust found: $(rustc --version)"
        RUST_OK=true
    fi
fi

if [[ "$RUST_OK" != "true" ]]; then
    log "INFO: Rust not found or not functional. Installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    # shellcheck source=/dev/null
    source "${HOME}/.cargo/env"
    log "INFO: Rust installed: $(rustc --version)"
fi

# ------------------------------------------------------------------
# Temporal CLI
# ------------------------------------------------------------------
if command -v temporal >/dev/null 2>&1; then
    log "INFO: Temporal CLI found: $(temporal --version 2>&1 | head -n1 || echo 'unknown')"
else
    log "INFO: Downloading Temporal CLI..."
    TEMPORAL_VERSION=$(curl -sL https://api.github.com/repos/temporalio/cli/releases/latest | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    if [[ -z "$TEMPORAL_VERSION" ]]; then
        TEMPORAL_VERSION="latest"
    fi
    log "INFO: Temporal CLI version: $TEMPORAL_VERSION"

    TEMPORAL_OS=""
    if [[ "$OS" == "Darwin" ]]; then
        TEMPORAL_OS="darwin"
    elif [[ "$OS" == "Linux" ]]; then
        TEMPORAL_OS="linux"
    fi

    TEMPORAL_ARCH=""
    case "$ARCH" in
        x86_64|amd64) TEMPORAL_ARCH="amd64" ;;
        arm64|aarch64) TEMPORAL_ARCH="arm64" ;;
        *) log "ERROR: Unsupported architecture $ARCH for Temporal CLI"; exit 1 ;;
    esac

    TEMPORAL_TARBALL="temporal_cli_${TEMPORAL_VERSION}_${TEMPORAL_OS}_${TEMPORAL_ARCH}.tar.gz"
    TEMPORAL_URL="https://github.com/temporalio/cli/releases/download/${TEMPORAL_VERSION}/${TEMPORAL_TARBALL}"

    log "INFO: Downloading from $TEMPORAL_URL"
    curl -fsSL "$TEMPORAL_URL" -o "/tmp/${TEMPORAL_TARBALL}"
    tar -xzf "/tmp/${TEMPORAL_TARBALL}" -C /tmp temporal 2>/dev/null || \
        tar -xzf "/tmp/${TEMPORAL_TARBALL}" -C /tmp
    mv -f /tmp/temporal "$LANTERN_BIN/temporal"
    chmod +x "$LANTERN_BIN/temporal"
    rm -f "/tmp/${TEMPORAL_TARBALL}"
    log "INFO: Temporal CLI installed to $LANTERN_BIN/temporal"
fi

# ------------------------------------------------------------------
# Build lantern from source
# ------------------------------------------------------------------
if [[ -f "$SOURCE_DIR/Cargo.toml" ]]; then
    # Guard 1 (identity): refuse to build unless SOURCE_DIR is the canonical
    # lantern crate. A bad branch reset once pointed the build at a divergent
    # tree and installed a lantern with no `mcp` subcommand, breaking every
    # agent's MCP client. Any repo with a stray Cargo.toml must not qualify.
    if ! grep -Eq '^name[[:space:]]*=[[:space:]]*"lantern"' "$SOURCE_DIR/Cargo.toml"; then
        log "ERROR: $SOURCE_DIR/Cargo.toml is not the 'lantern' crate. Refusing to build."
        log "ERROR: Run lantern-install from a pi-code-orchestrator checkout."
        exit 1
    fi
    log "INFO: Building lantern from source..."
    cd "$SOURCE_DIR"
    # Always build into the repo's target/ (not a sandbox CARGO_TARGET_DIR).
    env -u CARGO_TARGET_DIR cargo build --release
    # Guard 2 (smoke test): the freshly built artifact must expose the core
    # orchestration subcommands BEFORE it is allowed to clobber the installed
    # binary. Catches a source that compiles but lacks `mcp`/`relay`, so a bad
    # build can never overwrite a working install.
    built="$SOURCE_DIR/target/release/lantern"
    help_out="$("$built" --help 2>&1 || true)"
    for sub in mcp relay up; do
        if ! grep -qw "$sub" <<<"$help_out"; then
            log "ERROR: Built binary lacks the '$sub' subcommand — refusing to install."
            log "ERROR: Source at $SOURCE_DIR is wrong or out of date; installed binary left untouched."
            exit 1
        fi
    done
    log "INFO: Build smoke-test passed (mcp/relay/up present)"
    cp "$built" "$LANTERN_BIN/lantern"
    chmod +x "$LANTERN_BIN/lantern"
    # Ad-hoc sign so macOS Gatekeeper allows execution from ~/.lantern/bin
    if [[ "$OS" == "Darwin" ]]; then
        codesign -s - -f "$LANTERN_BIN/lantern" 2>/dev/null || \
            log "WARN: codesign failed; you may need to allow $LANTERN_BIN/lantern in Privacy & Security"
    fi
    log "INFO: lantern binary installed to $LANTERN_BIN/lantern"

    # iTerm2 Python helpers — copy every iterm_*.py the crate ships.
    for py in "$SOURCE_DIR"/src/startwork/iterm_*.py; do
        [[ -f "$py" ]] || continue
        cp "$py" "$LANTERN_BIN/$(basename "$py")"
        chmod +x "$LANTERN_BIN/$(basename "$py")"
    done
    log "INFO: iTerm2 Python helpers installed to $LANTERN_BIN"

    if [[ "$OS" == "Darwin" && -x "$SCRIPT_DIR/setup-iterm.sh" ]]; then
        SOURCE_DIR="$SOURCE_DIR" LANTERN_HOME="$LANTERN_HOME" LANTERN_BIN="$LANTERN_BIN" \
            "$SCRIPT_DIR/setup-iterm.sh"
    fi
else
    log "WARN: Source directory $SOURCE_DIR does not contain Cargo.toml. Skipping build."
    log "WARN: Please build manually: cargo build --release && cp target/release/lantern $LANTERN_BIN/"
fi

# Copy helper scripts
cp "$SCRIPT_DIR/lantern-up.sh" "$LANTERN_BIN/lantern-up"
cp "$SCRIPT_DIR/lantern-down.sh" "$LANTERN_BIN/lantern-down"
cp "$SCRIPT_DIR/lantern-doctor.sh" "$LANTERN_BIN/lantern-doctor"
cp "$SCRIPT_DIR/install.sh" "$LANTERN_BIN/lantern-install"
cp "$SCRIPT_DIR/setup-iterm.sh" "$LANTERN_BIN/lantern-setup-iterm"
cp "$SCRIPT_DIR/startwork.sh" "$LANTERN_BIN/startwork"
cp "$SCRIPT_DIR/stopwork.sh" "$LANTERN_BIN/stopwork"
chmod +x "$LANTERN_BIN/startwork"
chmod +x "$LANTERN_BIN/stopwork"
mkdir -p "${HOME}/.local/bin"
ln -sf "$LANTERN_BIN/startwork" "${HOME}/.local/bin/startwork"
log "INFO: Linked ~/.local/bin/startwork → $LANTERN_BIN/startwork"
ln -sf "$LANTERN_BIN/stopwork" "${HOME}/.local/bin/stopwork"
log "INFO: Linked ~/.local/bin/stopwork → $LANTERN_BIN/stopwork"
chmod +x "$LANTERN_BIN/lantern-up" "$LANTERN_BIN/lantern-down" "$LANTERN_BIN/lantern-doctor" "$LANTERN_BIN/lantern-install" "$LANTERN_BIN/lantern-setup-iterm"
log "INFO: Shell commands installed to $LANTERN_BIN"

# ------------------------------------------------------------------
# Config files
# ------------------------------------------------------------------
cat > "$LANTERN_CONFIG/lantern.toml" <<EOF
machine_id = "${HOSTNAME}"
temporal_address = "127.0.0.1:8243"
temporal_namespace = "default"
reconciliation_interval_secs = 5
ack_timeout_secs = 30
ack_retry_interval_secs = 30
stale_threshold_secs = 300
EOF
log "INFO: Created lantern.toml"

# ------------------------------------------------------------------
# PATH
# ------------------------------------------------------------------
if ! grep -q "${LANTERN_BIN}" "$HOME/.zshrc" 2>/dev/null; then
    echo "" >> "$HOME/.zshrc"
    echo "# Lantern" >> "$HOME/.zshrc"
    echo "export PATH=\"${LANTERN_BIN}:\$PATH\"" >> "$HOME/.zshrc"
    log "INFO: Added $LANTERN_BIN to PATH in ~/.zshrc"
else
    log "INFO: $LANTERN_BIN already in PATH"
fi

# ------------------------------------------------------------------
# launchd (macOS)
# ------------------------------------------------------------------
if [[ "$OS" == "Darwin" ]]; then
    PLIST_PATH="$HOME/Library/LaunchAgents/com.lantern.relay.plist"
    mkdir -p "$HOME/Library/LaunchAgents"
    log "INFO: Installing launchd plist to $PLIST_PATH"
    sed \
        -e "s|{{LANTERN_BIN}}|${LANTERN_BIN}|g" \
        -e "s|{{HOSTNAME}}|${HOSTNAME}|g" \
        -e "s|{{LANTERN_LOGS}}|${LANTERN_LOGS}|g" \
        -e "s|{{LANTERN_HOME}}|${LANTERN_HOME}|g" \
        "$SCRIPT_DIR/launchd.plist" > "$PLIST_PATH"
    launchctl unload "$PLIST_PATH" 2>/dev/null || true
    launchctl load -w "$PLIST_PATH"
    log "INFO: launchd service loaded"
fi

log "INFO: Install complete. Running lantern-doctor..."
exec "$LANTERN_BIN/lantern-doctor"
