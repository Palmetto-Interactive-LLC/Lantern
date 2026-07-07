#!/bin/sh
# Lantern installer — downloads the latest release binary from GitHub Releases.
#
# Usage (curl):
#   curl -fsSL https://raw.githubusercontent.com/Palmetto-Interactive-LLC/Lantern/main/scripts/install.sh | sh
#
# Usage (local, from a source checkout):
#   ./scripts/install.sh
#
# When run from a source checkout (Cargo.toml present in parent directory) the
# script builds from source instead of downloading a pre-built binary. Pass
# LANTERN_FORCE_DOWNLOAD=1 to always download regardless of local source.

set -eu

REPO="Palmetto-Interactive-LLC/Lantern"
LANTERN_BIN="${HOME}/.lantern/bin"
LANTERN_HOME="${HOME}/.lantern"
LANTERN_DATA="${HOME}/.lantern/data"
LANTERN_LOGS="${HOME}/.lantern/logs"
LANTERN_CONFIG="${HOME}/.lantern/config"
LANTERN_RUN="${HOME}/.lantern/run"
HOSTNAME_SHORT="$(hostname -s)"

log() { printf '[lantern-install] %s\n' "$*"; }
die() { log "ERROR: $*"; exit 1; }

install_executable() {
  src="$1"
  dest="$2"
  [ -f "$src" ] || die "Required install asset missing: $src"
  cp "$src" "$dest"
  chmod +x "$dest"
}

install_optional_executable() {
  src="$1"
  dest="$2"
  if [ -f "$src" ]; then
    cp "$src" "$dest"
    chmod +x "$dest"
  fi
}

install_launchd_plist() {
  src="$1"
  [ "$OS" = "Darwin" ] || return 0
  [ -f "$src" ] || die "Required launchd plist missing: $src"
  plist_dir="${HOME}/Library/LaunchAgents"
  plist="${plist_dir}/com.lantern.relay.plist"
  mkdir -p "$plist_dir"
  sed \
    -e "s#{{LANTERN_BIN}}#${LANTERN_BIN}#g" \
    -e "s#{{LANTERN_LOGS}}#${LANTERN_LOGS}#g" \
    -e "s#{{LANTERN_HOME}}#${LANTERN_HOME}#g" \
    -e "s#{{HOSTNAME}}#${HOSTNAME_SHORT}#g" \
    "$src" > "$plist"
  log "Installed launchd plist: $plist"
}

# ── OS / arch check ──────────────────────────────────────────────────────────
OS="$(uname -s)"
ARCH="$(uname -m)"

[ "$OS" = "Darwin" ] || die "Lantern requires macOS. Got: $OS"

case "$ARCH" in
  arm64|aarch64) RUST_TARGET="aarch64-apple-darwin" ;;
  x86_64)        RUST_TARGET="x86_64-apple-darwin" ;;
  *)             die "Unsupported architecture: $ARCH" ;;
esac

# ── Detect source checkout ───────────────────────────────────────────────────
SCRIPT_DIR=""
# Resolve script location when run as a file (not via pipe)
if [ -n "${BASH_SOURCE:-}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "$BASH_SOURCE")" && pwd)"
elif [ -f "$0" ] && [ "$0" != "sh" ] && [ "$0" != "-sh" ]; then
  SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
fi

SOURCE_DIR=""
if [ -n "$SCRIPT_DIR" ] && [ -f "${SCRIPT_DIR}/../Cargo.toml" ]; then
  SOURCE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
fi

# ── Directories ──────────────────────────────────────────────────────────────
log "Creating ~/.lantern directory structure"
mkdir -p "$LANTERN_BIN" "${LANTERN_DATA}/temporal" "${LANTERN_DATA}/relay" \
         "$LANTERN_LOGS" "$LANTERN_CONFIG" "$LANTERN_RUN"

# ── Build from source OR download ────────────────────────────────────────────
if [ -n "$SOURCE_DIR" ] && [ "${LANTERN_FORCE_DOWNLOAD:-0}" != "1" ]; then
  # ── Source build ──────────────────────────────────────────────────────────
  log "Source checkout detected at $SOURCE_DIR — building from source"

  if ! command -v cargo >/dev/null 2>&1 || ! cargo --version >/dev/null 2>&1; then
    if command -v rustup >/dev/null 2>&1; then
      log "Rust toolchain not configured. Installing stable toolchain via rustup..."
      rustup default stable
    else
      log "Rust not found. Installing via rustup..."
      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    fi
    if [ -f "${HOME}/.cargo/env" ]; then
      # shellcheck source=/dev/null
      . "${HOME}/.cargo/env"
    fi
  fi
  log "Rust: $(rustc --version)"

  cd "$SOURCE_DIR"
  env -u CARGO_TARGET_DIR cargo build --release

  BUILT="$SOURCE_DIR/target/release/lantern"
  HELP="$("$BUILT" --help 2>&1 || true)"
  for sub in mcp relay up; do
    echo "$HELP" | grep -qw "$sub" || die "Built binary missing subcommand '$sub' — refusing to install"
  done
  log "Smoke-test passed (mcp/relay/up present)"

  cp "$BUILT" "$LANTERN_BIN/lantern"
  chmod +x "$LANTERN_BIN/lantern"

  # Copy iTerm2 Python helpers
  for py in "$SOURCE_DIR"/src/startwork/iterm_*.py; do
    [ -f "$py" ] || continue
    cp "$py" "$LANTERN_BIN/$(basename "$py")"
    chmod +x "$LANTERN_BIN/$(basename "$py")"
  done
  log "iTerm2 helpers installed"

  install_executable "$SOURCE_DIR/scripts/lantern-up.sh" "$LANTERN_BIN/lantern-up"
  install_executable "$SOURCE_DIR/scripts/lantern-down.sh" "$LANTERN_BIN/lantern-down"
  install_executable "$SOURCE_DIR/scripts/lantern-doctor.sh" "$LANTERN_BIN/lantern-doctor"
  install_executable "$SOURCE_DIR/scripts/install.sh" "$LANTERN_BIN/lantern-install"
  install_executable "$SOURCE_DIR/scripts/setup-iterm.sh" "$LANTERN_BIN/lantern-setup-iterm"
  install_executable "$SOURCE_DIR/scripts/startwork.sh" "$LANTERN_BIN/startwork"
  install_executable "$SOURCE_DIR/scripts/stopwork.sh" "$LANTERN_BIN/stopwork"
  install_launchd_plist "$SOURCE_DIR/scripts/launchd.plist"
  log "Lantern helper commands installed"

else
  # ── Download pre-built binary ──────────────────────────────────────────────
  log "Fetching latest release from github.com/${REPO}"

  if ! command -v curl >/dev/null 2>&1; then
    die "curl is required for download installation"
  fi

  RELEASE_JSON="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest")"
  VERSION="$(printf '%s' "$RELEASE_JSON" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
  [ -n "$VERSION" ] || die "Could not determine latest release version"
  log "Latest release: $VERSION"

  ASSET_NAME="lantern-${VERSION}-${RUST_TARGET}.tar.gz"
  ASSET_URL="$(printf '%s' "$RELEASE_JSON" | grep "browser_download_url" | grep "${ASSET_NAME}" | head -1 | sed 's/.*"browser_download_url": *"\([^"]*\)".*/\1/')"
  [ -n "$ASSET_URL" ] || die "Release asset not found: $ASSET_NAME"

  SHA256_URL="$(printf '%s' "$RELEASE_JSON" | grep "browser_download_url" | grep "SHA256SUMS" | head -1 | sed 's/.*"browser_download_url": *"\([^"]*\)".*/\1/')"

  TMPDIR_LANTERN="$(mktemp -d)"
  trap 'rm -rf "$TMPDIR_LANTERN"' EXIT

  log "Downloading $ASSET_NAME"
  curl -fsSL "$ASSET_URL" -o "${TMPDIR_LANTERN}/${ASSET_NAME}"

  # SHA256 verification
  if [ -n "$SHA256_URL" ]; then
    log "Verifying SHA256"
    curl -fsSL "$SHA256_URL" -o "${TMPDIR_LANTERN}/SHA256SUMS"
    cd "$TMPDIR_LANTERN"
    # Extract only the line for our asset
    grep "$ASSET_NAME" SHA256SUMS > "${ASSET_NAME}.sha256" || die "SHA256SUMS entry not found for $ASSET_NAME"
    if command -v shasum >/dev/null 2>&1; then
      shasum -a 256 -c "${ASSET_NAME}.sha256" || die "SHA256 verification failed"
    elif command -v sha256sum >/dev/null 2>&1; then
      sha256sum -c "${ASSET_NAME}.sha256" || die "SHA256 verification failed"
    else
      log "WARN: No sha256 tool found — skipping verification"
    fi
    log "SHA256 verified"
    cd - >/dev/null
  else
    log "WARN: No SHA256SUMS in release — skipping verification"
  fi

  tar -xzf "${TMPDIR_LANTERN}/${ASSET_NAME}" -C "$TMPDIR_LANTERN"
  install_executable "${TMPDIR_LANTERN}/lantern" "$LANTERN_BIN/lantern"
  install_executable "${TMPDIR_LANTERN}/lantern-up" "$LANTERN_BIN/lantern-up"
  install_executable "${TMPDIR_LANTERN}/lantern-down" "$LANTERN_BIN/lantern-down"
  install_executable "${TMPDIR_LANTERN}/lantern-doctor" "$LANTERN_BIN/lantern-doctor"
  install_executable "${TMPDIR_LANTERN}/lantern-install" "$LANTERN_BIN/lantern-install"
  install_executable "${TMPDIR_LANTERN}/lantern-setup-iterm" "$LANTERN_BIN/lantern-setup-iterm"
  install_executable "${TMPDIR_LANTERN}/startwork" "$LANTERN_BIN/startwork"
  install_executable "${TMPDIR_LANTERN}/stopwork" "$LANTERN_BIN/stopwork"
  install_launchd_plist "${TMPDIR_LANTERN}/launchd.plist"
  for py in "${TMPDIR_LANTERN}"/iterm_*.py; do
    install_optional_executable "$py" "$LANTERN_BIN/$(basename "$py")"
  done
fi

# Ad-hoc codesign so Gatekeeper allows execution from ~/.lantern/bin
if command -v codesign >/dev/null 2>&1; then
  codesign -s - -f "$LANTERN_BIN/lantern" 2>/dev/null || \
    log "WARN: codesign failed — you may need to allow $LANTERN_BIN/lantern in Privacy & Security"
fi

# ── Config ───────────────────────────────────────────────────────────────────
cat > "$LANTERN_CONFIG/lantern.toml" <<EOF
machine_id = "${HOSTNAME_SHORT}"
temporal_address = "127.0.0.1:8243"
temporal_namespace = "default"
reconciliation_interval_secs = 5
ack_timeout_secs = 30
ack_retry_interval_secs = 30
stale_threshold_secs = 300
EOF

# ── PATH ─────────────────────────────────────────────────────────────────────
if [ -f "${HOME}/.zshrc" ] && ! grep -q "${LANTERN_BIN}" "${HOME}/.zshrc" 2>/dev/null; then
  printf "\n# Lantern\nexport PATH=\"%s:\$PATH\"\n" "$LANTERN_BIN" >> "${HOME}/.zshrc"
  log "Added $LANTERN_BIN to PATH in ~/.zshrc"
fi
if [ -f "${HOME}/.bashrc" ] && ! grep -q "${LANTERN_BIN}" "${HOME}/.bashrc" 2>/dev/null; then
  printf "\n# Lantern\nexport PATH=\"%s:\$PATH\"\n" "$LANTERN_BIN" >> "${HOME}/.bashrc"
fi

# ── Done ─────────────────────────────────────────────────────────────────────
INSTALLED_VERSION="$("$LANTERN_BIN/lantern" --version 2>/dev/null || echo 'unknown')"
log "Installed: $INSTALLED_VERSION → $LANTERN_BIN/lantern"
printf '\n'
printf '  Next steps:\n'
printf '    1. Reload your shell:  source ~/.zshrc\n'
printf '    2. Install Temporal:   brew install temporal\n'
printf '    3. Start services:     lantern up\n'
printf '    4. Health check:       lantern doctor\n'
printf '    5. Launch a squad:     lantern startwork <project> <slot>\n'
printf '\n'
printf '  Docs: https://github.com/%s\n' "$REPO"
