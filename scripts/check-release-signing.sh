#!/bin/bash
# Verify that this checkout can create and verify an SSH-signed release tag.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TIMEOUT_SECONDS="${SIGNING_TIMEOUT_SECONDS:-15}"

die() {
  echo "error: $*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

run_with_timeout() {
  perl -e '$SIG{ALRM}=sub{die "timed out\n"}; alarm shift @ARGV; exec @ARGV' \
    "$TIMEOUT_SECONDS" "$@"
}

need_cmd git
need_cmd gh
need_cmd perl
need_cmd ssh-keygen

cd "$ROOT"

gpg_format="$(git config --get gpg.format || true)"
[[ "$gpg_format" == "ssh" ]] || die "git config gpg.format must be ssh"

tag_signing="$(git config --bool --get tag.gpgsign || true)"
[[ "$tag_signing" == "true" ]] || die "git config tag.gpgsign must be true"

signing_key="$(git config --path --get user.signingkey || true)"
[[ -n "$signing_key" ]] || die "git config user.signingkey is not set"
[[ -f "$signing_key" ]] || die "user.signingkey does not point to a file: $signing_key"

allowed_signers="$(git config --path --get gpg.ssh.allowedSignersFile || true)"
[[ -n "$allowed_signers" ]] || die "git config gpg.ssh.allowedSignersFile is not set"
[[ -f "$allowed_signers" ]] || die "allowed signers file does not exist: $allowed_signers"

email="$(git config --get user.email || true)"
[[ -n "$email" ]] || die "git config user.email is not set"
if ! grep -F -- "$email" "$allowed_signers" >/dev/null; then
  die "allowed signers file does not contain the current Git email: $email"
fi

github_login="$(gh api user --jq '.login')"
key_fingerprint="$(ssh-keygen -lf "$signing_key" | awk '{print $2}')"
github_has_key="$(
  gh api "users/${github_login}/ssh_signing_keys" --jq '.[].key' \
    | while IFS= read -r github_key; do
        tmp_key="$(mktemp "${TMPDIR:-/tmp}/lantern-gh-signing-key.XXXXXX")"
        printf '%s\n' "$github_key" > "$tmp_key"
        if ssh-keygen -lf "$tmp_key" | awk '{print $2}' | grep -Fx -- "$key_fingerprint" >/dev/null; then
          rm -f "$tmp_key"
          printf 'yes\n'
          exit 0
        fi
        rm -f "$tmp_key"
      done
)"
[[ "$github_has_key" == "yes" ]] || die "GitHub user ${github_login} does not list user.signingkey as an SSH signing key"

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/lantern-release-signing.XXXXXX")"
tag="lantern-signing-smoke-$(date +%s)-$$"

cleanup() {
  git tag -d "$tag" >/dev/null 2>&1 || true
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

payload="$tmp_dir/payload"
printf 'lantern release signing smoke\n' > "$payload"
run_with_timeout ssh-keygen -Y sign -f "$signing_key" -n git "$payload" >/dev/null
rm -f "$payload.sig"

run_with_timeout git tag -s -m "Lantern signing smoke" "$tag" HEAD
git tag -v "$tag" >/dev/null

echo "release signing OK for $email (${github_login})"
