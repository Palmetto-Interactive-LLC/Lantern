#!/bin/bash
# Build release tarballs from already-built macOS target binaries.
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: scripts/package-release.sh <tag> [target ...]

Example:
  scripts/package-release.sh v2026.7.0 aarch64-apple-darwin x86_64-apple-darwin

Set DIST_DIR to override the output directory. Defaults to ./dist.
EOF
}

if [[ $# -lt 1 ]]; then
  usage
  exit 2
fi

TAG="$1"
shift || true

if [[ $# -eq 0 ]]; then
  set -- aarch64-apple-darwin x86_64-apple-darwin
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-${ROOT}/dist}"
HOST_TARGET="$(rustc -vV | awk '/^host:/ {print $2}')"

mkdir -p "$DIST_DIR"
: > "${DIST_DIR}/SHA256SUMS"

checksum() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1"
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1"
  else
    echo "No SHA256 tool found" >&2
    return 1
  fi
}

install_asset() {
  local src="$1"
  local dest="$2"
  [[ -f "$src" ]] || {
    echo "missing release asset: $src" >&2
    exit 1
  }
  cp "$src" "$dest"
}

for target in "$@"; do
  binary="${ROOT}/target/${target}/release/lantern"
  if [[ ! -x "$binary" && "$target" == "$HOST_TARGET" ]]; then
    binary="${ROOT}/target/release/lantern"
  fi
  [[ -x "$binary" ]] || {
    echo "missing built binary for ${target}: ${binary}" >&2
    echo "run: cargo build --release --target ${target}" >&2
    exit 1
  }

  asset="lantern-${TAG}-${target}"
  stage="$(mktemp -d "${TMPDIR:-/tmp}/${asset}.XXXXXX")"
  trap 'rm -rf "$stage"' RETURN

  install_asset "$binary" "${stage}/lantern"
  install_asset "${ROOT}/scripts/lantern-up.sh" "${stage}/lantern-up"
  install_asset "${ROOT}/scripts/lantern-down.sh" "${stage}/lantern-down"
  install_asset "${ROOT}/scripts/lantern-doctor.sh" "${stage}/lantern-doctor"
  install_asset "${ROOT}/scripts/install.sh" "${stage}/lantern-install"
  install_asset "${ROOT}/scripts/setup-iterm.sh" "${stage}/lantern-setup-iterm"
  install_asset "${ROOT}/scripts/startwork.sh" "${stage}/startwork"
  install_asset "${ROOT}/scripts/stopwork.sh" "${stage}/stopwork"
  install_asset "${ROOT}/scripts/launchd.plist" "${stage}/launchd.plist"
  cp "${ROOT}"/src/startwork/iterm_*.py "$stage/"
  chmod +x "${stage}/lantern" "${stage}"/lantern-* "${stage}/startwork" \
    "${stage}/stopwork" "${stage}"/iterm_*.py

  tarball="${DIST_DIR}/${asset}.tar.gz"
  tar -czf "$tarball" -C "$stage" .
  (cd "$DIST_DIR" && checksum "$(basename "$tarball")") >> "${DIST_DIR}/SHA256SUMS"
  rm -rf "$stage"
  trap - RETURN
  echo "packaged ${tarball}"
done
