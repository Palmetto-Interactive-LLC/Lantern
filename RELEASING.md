# Releasing Lantern

## Versioning scheme

Lantern uses **CalVer**: `YYYY.MM.PATCH`

| Component | Meaning |
|-----------|---------|
| `YYYY` | Four-digit year (e.g. `2026`) |
| `MM` | Month without zero-padding (e.g. `6` for June, `12` for December) |
| `PATCH` | Starts at `0`; increments for each additional release in the same month |

Examples: `2026.6.0`, `2026.6.1`, `2026.7.0`

This is valid [semver](https://semver.org/) (`YYYY.MM.PATCH` maps to `major.minor.patch`). Tools like `cargo` and `gh` handle it without modification.

## Cutting a release

### 1. Bump the version

Edit `Cargo.toml`:

```toml
[package]
version = "2026.7.0"   # ← new version
```

Run `cargo build --release` locally to update `Cargo.lock`, then commit:

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore(release): bump version to 2026.7.0"
git push
```

### 2. Tag and push

```bash
git tag v2026.7.0
git push origin v2026.7.0
```

Pushing the tag triggers `.github/workflows/release.yml`, which:

1. Builds `lantern` for both `aarch64-apple-darwin` (Apple Silicon) and `x86_64-apple-darwin` (Intel)
2. Packages each as `lantern-v2026.7.0-<target>.tar.gz`
3. Generates `SHA256SUMS`
4. Creates the GitHub Release with auto-generated notes and attaches all assets

### 3. Verify the release

```bash
gh release view v2026.7.0
```

Check that both `.tar.gz` assets and `SHA256SUMS` are attached and that the CI run passed.

### 4. Test the installer

```bash
curl -fsSL https://raw.githubusercontent.com/Palmetto-Interactive-LLC/pi-code-orchestrator/main/scripts/install.sh | sh
```

---

## Workflow dispatch (manual trigger)

If the tag was pushed before the workflow was in place, or you need to re-run the release build:

```bash
gh workflow run release.yml --field tag=v2026.7.0
```

---

## Patch releases within a month

If a critical fix ships later in June 2026:

```
version = "2026.6.1"   # in Cargo.toml
git tag v2026.6.1
git push origin v2026.6.1
```

---

## CI gates (must be green before tagging)

```bash
cargo fmt --check
cargo clippy --all-targets
cargo build --release
cargo test
```

All four are enforced by `.github/workflows/ci.yml` on every push to `main` and every PR.
