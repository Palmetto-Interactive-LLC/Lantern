# Releasing Lantern

Lantern uses Calendar Versioning: `YYYY.MM.PATCH`.

Examples: `2026.6.0`, `2026.6.1`, `2026.7.0`.

## Release Requirements

Before tagging:

```bash
make verify
make security
make package-smoke
make install-smoke
```

`make package-smoke` proves the release tarball layout for the current host target. `make install-smoke` runs the source installer under an isolated `HOME` and verifies the installed binary, helper commands, iTerm scripts, and launchd plist.

## Version Bump

Edit `Cargo.toml`:

```toml
[package]
version = "2026.7.0"
```

Then verify and commit:

```bash
make verify
git add Cargo.toml Cargo.lock
git commit -m "chore(release): bump version to 2026.7.0"
git push
```

## Tag

```bash
git tag v2026.7.0
git push origin v2026.7.0
```

Pushing a `v*` tag triggers `.github/workflows/release.yml`.

## Release Artifact Contract

Each macOS release tarball must contain:

- `lantern`
- `lantern-up`
- `lantern-down`
- `lantern-doctor`
- `lantern-install`
- `lantern-setup-iterm`
- `startwork`
- `stopwork`
- `launchd.plist`
- `iterm_*.py`

The release workflow builds:

- `install-lantern.sh`
- `lantern-vYYYY.M.PATCH-aarch64-apple-darwin.tar.gz`
- `lantern-vYYYY.M.PATCH-x86_64-apple-darwin.tar.gz`
- `SHA256SUMS`

Packaging is centralized in `scripts/package-release.sh`; do not duplicate tarball layout logic in workflow YAML.

## Verify Published Release

```bash
gh release view v2026.7.0 --json tagName,assets,isDraft,isPrerelease
gh run list --workflow release.yml --limit 5
```

Confirm:

- release is not a draft unless intentionally staged
- `install-lantern.sh` is attached
- both target tarballs are attached
- `SHA256SUMS` is attached
- the release workflow completed successfully

## Clean-Machine Installer Smoke

Run on a macOS host after the release is published:

```bash
tmp_home="$(mktemp -d)"
HOME="$tmp_home" sh -c 'curl -fsSL https://raw.githubusercontent.com/Palmetto-Interactive-LLC/Lantern/main/scripts/install.sh | sh'
"$tmp_home/.lantern/bin/lantern" --version
ls -1 "$tmp_home/.lantern/bin" | sort
test -f "$tmp_home/Library/LaunchAgents/com.lantern.relay.plist"
rm -rf "$tmp_home"
```

This verifies the public download path without overwriting the operator's real `~/.lantern`.

## Manual Workflow Dispatch

If the tag was pushed before the workflow was available, or a release build needs a re-run:

```bash
gh workflow run release.yml --field tag=v2026.7.0
```

## Patch Releases

For another release in the same month:

```toml
version = "2026.7.1"
```

Then repeat the release requirements, commit, tag, and verify steps.
