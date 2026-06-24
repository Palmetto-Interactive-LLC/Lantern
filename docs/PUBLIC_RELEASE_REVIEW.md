# Public Release Security & Exposure Review

**Repository:** `Palmetto-Interactive-LLC/pi-code-orchestrator` (Lantern)
**Review type:** Pre-public-release secret, sensitive-data, and public-exposure audit
**Reviewer role:** Security specialist (`sec`)
**Date:** 2026-06-23
**Verification target:** integrated final tree = `prep/public-release-hardening` (ops hardening, `b4303df`) merged with the documentation branch (`pi-code-orchestrator-doc-3`, `231f469` + `78c9516`). Merge was clean (disjoint file sets); integrated verification commit `02abf86`.

---

## 1. Summary

The repository was audited for secrets, credentials, and sensitive/proprietary content prior to flipping visibility from **private** to **public**. The audit covered the **current working tree** and the **full 19-commit git history**.

**Zero secrets or credentials were found** — by four independent automated scanners and nine manual ripgrep pattern sweeps. The repository's standing claim of "no secrets, no credentials" is **substantiated by evidence**, not assumed.

Five findings were surfaced. The one material item (proprietary license on a repo intended for public release) is **RESOLVED** — the project now ships under **Apache-2.0** with a matching `Cargo.toml` field and README notice. The remaining four are **LOW/INFO accepted residuals** with no data-exposure risk. **No git history rewrite is required.**

A full public-OSS documentation and GitHub-hardening package has been applied (license, SECURITY.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md, CHANGELOG.md, .env.example; branch protection, secret scanning + push protection, Dependabot, SHA-pinned CodeQL/CI). A post-change re-sweep on the integrated tree confirmed it is **still clean**.

**Recommendation: GO** (one org-licensed UI toggle — GHAS code scanning — remains and should be enabled post-flip; it is not a release blocker).

---

## 2. Tools Run

All scans were executed with exact, reproducible commands. Scanner versions: gitleaks 8.30.1, trufflehog 3.95.6, ripgrep 15.1.0 (`/opt/homebrew/bin/rg`; the shell `rg` alias was bypassed). Manual sweeps iterate over `git ls-files` so only tracked content is examined.

### Automated scanners (initial audit, pubrel-2)

| # | Tool | Command | Scope | Result |
|---|------|---------|-------|--------|
| a | gitleaks | `gitleaks dir . --redact` | Current tree (2.06 MB) | **no leaks, 0 findings** |
| b | gitleaks | `gitleaks git . --redact` | **Full history — 19 commits** | **no leaks** |
| c | trufflehog | `trufflehog filesystem . --results=verified,unknown` | Current tree (365 chunks) | **0 verified, 0 unverified** |
| d | trufflehog | `trufflehog git file://<git-common-dir> --results=verified,unknown` | Full history | **0 verified, 0 unverified** |

> Note (d): this is a git **worktree**, so `.git` is a file, not a directory; trufflehog's git source was pointed at the repo's `git-common-dir` to scan real history.

### Manual ripgrep sweeps (9 pattern classes — all zero real hits)

1. API-key / token prefixes — `sk-…`, `ghp_…`, `github_pat_`, `xox[baprs]-`, `AKIA[0-9A-Z]{16}`, `AIza…`, `ya29.`, `eyJ…` (JWT), `-----BEGIN … PRIVATE KEY-----`
2. Cloud credentials — Cloudflare (`CF_API`/`CLOUDFLARE_API`), AWS (`aws_secret`/`aws_access`/`secret_access_key`), GCP (`service_account`)
3. Secret-manager refs — `op://` / 1Password, `sops`, `age1…` / `AGE-SECRET-KEY`
4. `password|secret|api_key|token` assignment literals
5. Database connection strings — `postgres://`, `mysql://`, `mongodb://`, `redis://`, `amqp://`, `DATABASE_URL=`
6. Webhooks — `hooks.slack.com`, Discord webhooks, `webhook.office.com` (Teams)
7. Emails (non-example), and external URLs (enumerated — all public)
8. Local-machine paths (`/Users/matt`, `/home/<user>`) and non-localhost IPv4
9. Sensitive keywords — internal domains/infra, customer/prospect/acquisition/finance/contract terms, `TODO`/`FIXME`/`HACK`, and profanity/unprofessional language

### Post-change re-sweep (pubrel-8, integrated tree `02abf86`)

| Tool | Command | Result |
|------|---------|--------|
| gitleaks | `gitleaks dir .` | **no leaks, 0 findings** (700 KB) |
| ripgrep | `/Users/matt` + `/home/<user>` | **none** |
| ripgrep | token/key prefixes + `BEGIN PRIVATE KEY` | **none** |
| ripgrep | non-localhost IPv4 | **none** |
| ripgrep | secret/password/token assignments | **none** |

`.env.example` verified to contain **only documentation and a commented `# RUST_LOG=debug` example** — no real values.

---

## 3. Findings Table

| ID | Finding | File / Path | Severity | Exposure risk | Status / Remediation | History rewrite |
|----|---------|-------------|----------|---------------|----------------------|-----------------|
| **F1** | Proprietary license on a repo intended for public release | `README.md`, `LICENSE`, `Cargo.toml` | ~~MEDIUM~~ → **RESOLVED** | None (legal contradiction, not a data leak) | **RESOLVED — relicensed Apache-2.0**: `LICENSE` (full Apache 2.0, 204 lines), `Cargo.toml:7 license = "Apache-2.0"`, `README.md:173` Apache-2.0 notice | N |
| **F2** | Local SSH host alias `github.com-client` | `CLAUDE.md:75-76`; `.beads/config.yaml:68` | **LOW** | Leaks the owner's personal `~/.ssh` host-alias naming; non-functional for the public (they lack the alias) but reveals nothing secret | **ACCEPTED RESIDUAL** — left intentionally: it is accurate machine documentation, and the owner's standing policy is SSH-alias-first; rewriting it to `git@github.com` would break their push/sync. Cosmetic only. | N |
| **F3** | Developer name "Matt Lucas" in tracker audit log | `.beads/interactions.jsonl` (4 status-change records) | **INFO** | None — name is already public via git commit authorship; records contain only `status` field changes, no titles/descriptions/comments | **ACCEPTED RESIDUAL** — no action; optionally drop `.beads/interactions.jsonl` from the public export if desired | N |
| **F4** | Org name `Palmetto-Interactive-LLC` in GitHub URLs | `README.md`, `CLAUDE.md`, docs | **INFO** | None — the repository's own identity; unavoidable and non-sensitive | No action | N |
| **F5** | Test fixture email `t@t.co` | `src/stopwork/mod.rs:339,400` | **INFO** | None — generic placeholder used by hermetic git unit tests | No action | N |

**Positive controls confirmed:**
- `.gitignore` correctly excludes credential-risk artifacts: `.env`/`.envrc`/`*.env` (allowing `.env.example`), `.cloud-context`, `*.db`, `.beads-credential-key`, `.beads/proxieddb/`.
- **No** tracked `.env` / `.pem` / `.key` / `credentials` files anywhere in the tree or history.
- All external URLs resolve to public destinations only: crates.io index, `127.0.0.1:8243/8244` (local Temporal), public github.com, docs.temporal.io, diataxis.fr, sh.rustup.rs, iterm2.com.
- Editor/agent hooks (`.codex/hooks.json`) invoke only `bd …`; `.agents/skills/.../openai.yaml` contains no credential strings.

---

## 4. Remediations Applied

**Licensing (resolves F1):**
- `LICENSE` — full Apache License 2.0 (204 lines), copyright "2026 Palmetto Interactive LLC".
- `Cargo.toml` — `license = "Apache-2.0"` added to package metadata.
- `README.md` — License section updated to Apache-2.0 with link to `LICENSE` (replaces "Proprietary - Palmetto Interactive LLC").

**Public OSS documentation:**
- `SECURITY.md` — vulnerability disclosure policy.
- `CONTRIBUTING.md` — dev setup (cargo build/test, install.sh), build gates (`cargo fmt --check`, clippy, test), branch/PR workflow, conventional commits, beads (`bd`) workflow.
- `CODE_OF_CONDUCT.md` — contributor code of conduct.
- `CHANGELOG.md` — change history.
- `.env.example` — documents the (none-required) env approach with a single commented example; no real values.

**GitHub repository hardening** (see §7 for the full enumeration).

---

## 5. Remaining Risks

| Risk | Severity | Disposition |
|------|----------|-------------|
| **F2** — `github.com-client` SSH alias in `CLAUDE.md` / `.beads/config.yaml` | LOW | **Accepted residual.** Machine-config detail, not a secret; rewriting would break the owner's git workflow. |
| **F3** — Developer name in `.beads/interactions.jsonl` | INFO | **Accepted residual.** Already public via commit authorship; no sensitive content. |
| **RUSTSEC-2023-0071** — `rsa` 0.9.10 "Marvin Attack" timing sidechannel, reachable via `sqlx-mysql` | MEDIUM (CVSS 5.9) | **Accepted with justification.** Flagged by qa's `cargo audit`. **No upstream fix is available.** The vulnerable code path is the **MySQL** driver's RSA handshake; **Lantern uses SQLite exclusively** (`sqlite://` connection strings only — no MySQL backend is configured or compiled into a runtime path that performs RSA). The `rsa` crate enters the dependency graph transitively through `sqlx-mysql` but is **not exercised at runtime**. Recommendation: document as an accepted risk **and** add a `cargo audit` ignore with this justification (e.g. `.cargo/audit.toml` → `[advisories] ignore = ["RUSTSEC-2023-0071"]` with a comment), and/or trim the `sqlx` MySQL feature if not needed, so CI's audit gate stays green without masking new advisories. Re-evaluate when an upstream `rsa` fix ships. |

No other unresolved risks. No customer data, financials, internal infrastructure, or proprietary strategy is exposed.

---

## 6. Git History Rewrite Needed?

**NO.**

- gitleaks scanned the **full 19-commit history** (`gitleaks git .`) → no leaks.
- trufflehog scanned the **full git history** → 0 verified / 0 unverified.
- None of F1–F5 involves a secret committed in the past; F1 was a current-tree license string (now changed forward), and F2–F5 are non-secret current-tree content.
- **No `git filter-repo` / BFG / force-push operation is required** for public release.

---

## 7. GitHub Settings — Changed and Still-Required

### Changed (applied via API + committed files; source: ops Phase 1+6)

**Repository security toggles (API):**
- `secret_scanning` = **enabled**
- `secret_scanning_push_protection` = **enabled**
- `dependabot_security_updates` = **enabled**
- Dependabot vulnerability alerts = **enabled**

**`main` branch protection (API):**
- Require a pull request before merging — **1 approval**
- Require review from **Code Owners** (CODEOWNERS)
- **No force pushes**
- **No branch deletion**
- Require **linear history**
- Required status check: **`Rust fmt`**

**Committed hardening files:**
- `.github/CODEOWNERS` — `* @Palmetto-Interactive-LLC/maintainers` (all files require maintainer review).
- `.github/dependabot.yml` — `cargo` + `github-actions` ecosystems, weekly.
- `.github/workflows/codeql.yml` — CodeQL for Rust; **all actions SHA-pinned** (`actions/checkout@11bd719…`, `github/codeql-action/*@dd903d2…`).
- `.github/workflows/ci.yml` — top-level `permissions: contents: read` (least privilege) + job-level `permissions: contents: read`; actions **SHA-pinned** (`actions/checkout@11bd719…`, `dtolnay/rust-toolchain@67ef31d…` toolchain 1.95.0).

### Still-required (cannot be set via API on this plan tier)

- **GitHub Advanced Security — Code scanning (CodeQL) default setup / GHAS enablement** is **org-license-gated** and must be turned on through the UI: **Settings → Security → Code security**. The `codeql.yml` workflow is already committed and SHA-pinned, so code scanning will begin running once GHAS is enabled. This is the **only** control not addressable via API and is **not a release blocker** — enable it immediately after the visibility flip.

---

## 8. Final GO / NO-GO Recommendation

### ✅ GO

**Rationale:**

1. **No secrets, full history.** Four independent scanners (gitleaks tree + history, trufflehog fs + git) and nine ripgrep sweeps returned **zero** credentials across the current tree **and** the complete 19-commit history. The "no secrets" claim is proven, not assumed.
2. **No history rewrite required.** History is clean; the flip to public can proceed without any force-push surgery.
3. **The one material finding is resolved.** The license contradiction (F1) is fixed — Apache-2.0 across `LICENSE`, `Cargo.toml`, and `README.md`.
4. **Residual findings are LOW/INFO with no data-exposure risk** (F2 SSH alias, F3 dev name, F4 org name, F5 test email) and are formally accepted.
5. **Public-OSS posture is complete** — license, SECURITY.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md, CHANGELOG.md, .env.example, branch protection, secret scanning + push protection, Dependabot, and SHA-pinned CodeQL/CI.
6. **Post-change re-sweep is clean** — the integrated tree was re-scanned after all doc/ops changes and remains free of secrets and local paths.

**Conditions on GO (post-flip, non-blocking):**
- Enable **GHAS code scanning** in Settings → Security → Code security (org-license action).
- Record **RUSTSEC-2023-0071** as an accepted risk (SQLite-only; MySQL/rsa path not exercised), ideally via a justified `cargo audit` ignore so the CI audit gate stays green.

There is no exposure that warrants holding the release. **Proceed to public.**
