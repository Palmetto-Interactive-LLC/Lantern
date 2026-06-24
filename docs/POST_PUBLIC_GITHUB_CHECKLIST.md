# Post-Public Activation Checklist

Features and settings that only unlock (or become free) once `pi-code-orchestrator` is set to **Public**.
Run these immediately after flipping visibility — do **not** run them while the repo is still private.

> **Legend:** 🔵 RECOMMENDED — do this at flip time. ⚪ OPTIONAL — evaluate based on project goals.

---

## 1. CodeQL / Code Scanning Default Setup 🔵 RECOMMENDED

**Why requires public:** GitHub's hosted CodeQL runner is free for public repos without a GHAS license.
Private repos require a paid GHAS seat.

**Action:** The repo already has `.github/workflows/codeql.yml` committed (SHA-pinned, Rust, auto-build).
This file activates automatically on the first push/PR after going public — **no additional action needed**.

Optionally also enable GitHub's "Default setup" via UI for a second pass with GitHub-managed config:

```
UI: Settings → Security → Code security and analysis
    → CodeQL analysis → "Default" → Enable
```

Or confirm the committed workflow is sufficient (it is — prefer the committed workflow for version control
and SHA-pinned action control over the UI-managed default).

**Verify:**
```bash
gh api repos/Palmetto-Interactive-LLC/Lantern --jq '.security_and_analysis.code_security'
# Should show: {"status":"enabled"} after GHAS or public flip
```

---

## 2. Secret Scanning + Push Protection 🔵 RECOMMENDED

**Why requires public:** Already enabled (both are currently on). For public repos these features are free
and remain active after the visibility change. Confirm they survive the flip.

**Verify immediately after going public:**
```bash
gh api repos/Palmetto-Interactive-LLC/Lantern \
  --jq '.security_and_analysis | {secret_scanning: .secret_scanning.status, push_protection: .secret_scanning_push_protection.status}'
# Both should still show "enabled"
```

If either reverts to disabled:
```bash
printf '{"security_and_analysis":{"secret_scanning":{"status":"enabled"},"secret_scanning_push_protection":{"status":"enabled"}}}' | \
  gh api -X PATCH repos/Palmetto-Interactive-LLC/Lantern --input -
```

---

## 3. Private Vulnerability Reporting 🔵 RECOMMENDED

**Why requires public:** This feature is only available on public repos. It lets security researchers
report vulnerabilities directly to maintainers via a private GitHub channel (no public issue disclosure).

**Enable:**
```
UI: Settings → Security → Private vulnerability reporting → Enable
```

Or via API:
```bash
gh api -X PUT repos/Palmetto-Interactive-LLC/Lantern/private-vulnerability-reporting 2>&1
# HTTP 204 = success
```

---

## 4. Security Policy Surfacing from SECURITY.md 🔵 RECOMMENDED

**Why requires public:** GitHub's "Security" tab surfaces `SECURITY.md` in the community health profile
and shows a "Report a vulnerability" button (backed by private vulnerability reporting, item 3) only for
public repos.

**Action:** `SECURITY.md` is already committed to the repo. No additional steps required after going
public — GitHub will surface it automatically.

**Verify:**
```
UI: https://github.com/Palmetto-Interactive-LLC/Lantern/security/policy
    → Should display SECURITY.md content
```

---

## 5. GitHub Security Advisories 🔵 RECOMMENDED

**Why requires public:** The GitHub Advisory Database integration and the ability to publish
coordinated CVEs via GitHub Security Advisories is available on public repos.

**Enable private vulnerability reporting first (item 3), then:**
```
UI: Security tab → Advisories → "New draft security advisory"
    (only visible/functional after repo is public)
```

Or list existing advisories:
```bash
gh api repos/Palmetto-Interactive-LLC/Lantern/security-advisories --jq '.[].ghsa_id'
```

---

## 6. Dependency Graph + Dependabot Alerts 🔵 RECOMMENDED

**Why requires public:** Dependency graph and Dependabot alerts are free for all public repos.
Dependabot vulnerability alerts are already enabled — confirm dependency graph is active post-flip.

**Verify:**
```bash
gh api repos/Palmetto-Interactive-LLC/Lantern/dependency-graph/sbom \
  --jq '.sbom.name' 2>&1
# Returns repo name if dependency graph is active
```

If disabled:
```
UI: Settings → Security → Code security and analysis → Dependency graph → Enable
```

---

## 7. Dependency Review Action on PRs 🔵 RECOMMENDED

**Why requires public:** The `dependency-review-action` (which blocks PRs that introduce known-vulnerable
dependencies) requires either a public repo or a GHAS license. Free on public.

**Add workflow** `.github/workflows/dependency-review.yml`:
```yaml
name: Dependency Review

on:
  pull_request:
    branches: [main]

permissions:
  contents: read
  pull-requests: write

jobs:
  dependency-review:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683 # v4.2.2
      - uses: actions/dependency-review-action@3b139cfc5fae8b618d3eae3675e383bb1769c019 # v4.5.0
```

```bash
# Add file, then:
git add .github/workflows/dependency-review.yml
git commit -m "feat(ci): add dependency-review workflow (public-repo unlock)"
git push
```

---

## 8. Community Standards / Insights Profile Completeness 🔵 RECOMMENDED

**Why requires public:** The Community Profile (Insights → Community Standards) is only populated
for public repos. GitHub checks for presence of: README, LICENSE, CONTRIBUTING, CODE_OF_CONDUCT,
SECURITY, ISSUE_TEMPLATE, PR_TEMPLATE, CODEOWNERS.

**Verify after going public:**
```
UI: https://github.com/Palmetto-Interactive-LLC/Lantern/community
    → All green checklist items
```

Current status (pre-flip): README ✅, LICENSE ✅, CONTRIBUTING ✅, CODE_OF_CONDUCT ✅,
SECURITY ✅, CODEOWNERS ✅. Missing: Pull Request template (optional).

**Add PR template (optional):**
```bash
mkdir -p .github && cat > .github/PULL_REQUEST_TEMPLATE.md << 'EOF'
## Summary

## Test plan

## Checklist
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy` clean
- [ ] `cargo test` passes
EOF
```

---

## 9. GitHub Pages ⚪ OPTIONAL

**Why requires public:** GitHub Pages is free for public repos (private requires GitHub Pro/Team).

**Enable (if docs hosting wanted):**
```
UI: Settings → Pages → Source → "Deploy from a branch"
    Branch: main, Folder: /docs (or / root)
```

Or via API:
```bash
printf '{"source":{"branch":"main","path":"/docs"}}' | \
  gh api -X POST repos/Palmetto-Interactive-LLC/Lantern/pages --input -
```

The `docs/` directory already exists with structured documentation (getting-started, reference,
how-to, tutorial, explanation). Consider adding an `index.html` or `_config.yml` for Jekyll if
enabling Pages.

---

## 10. Repo Discoverability: Topics, Description, Social Preview 🔵 RECOMMENDED

**Why requires public:** Topics improve search ranking on github.com/explore and topic pages.
Social preview images appear in link unfurls (Slack, Twitter/X, LinkedIn) — only meaningful for public.

**Set topics:**
```bash
gh api -X PUT repos/Palmetto-Interactive-LLC/Lantern/topics \
  --field names[]="rust" \
  --field names[]="cli" \
  --field names[]="orchestration" \
  --field names[]="ai-agents" \
  --field names[]="iterm2" \
  --field names[]="mcp" \
  --field names[]="temporal" \
  --field names[]="developer-tools"
```

**Set description** (already set, verify):
```bash
gh api repos/Palmetto-Interactive-LLC/Lantern --jq '.description'
```

**Social preview:**
```
UI: Settings → Social preview → Upload image (1280×640px recommended)
```

---

## 11. OpenSSF Scorecard Action ⚪ OPTIONAL

**Why requires public:** The OpenSSF Scorecard GitHub Action (supply-chain security scoring) is
free and publicly verifiable only for public repos. Produces a badge and OSSF score (0–10).

**Add workflow** `.github/workflows/scorecard.yml`:
```yaml
name: Scorecard

on:
  schedule:
    - cron: '30 1 * * 1'
  push:
    branches: [main]

permissions:
  security-events: write
  id-token: write
  contents: read
  actions: read

jobs:
  analysis:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683 # v4.2.2
        with:
          persist-credentials: false
      - uses: ossf/scorecard-action@f49aabe0b5af0936a0987cfb85d86b75b5b7b75c # v2.4.1
        with:
          results_file: scorecard-results.sarif
          results_format: sarif
          publish_results: true
      - uses: github/codeql-action/upload-sarif@dd903d2e4f5405488e5ef1422510ee31c8b32357 # v3
        with:
          sarif_file: scorecard-results.sarif
```

Add badge to README once score is published:
```markdown
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/Palmetto-Interactive-LLC/Lantern/badge)](https://scorecard.dev/viewer/?uri=github.com/Palmetto-Interactive-LLC/Lantern)
```

---

## 12. Sponsorships / FUNDING.yml ⚪ OPTIONAL

**Why requires public:** The GitHub Sponsors "Sponsor" button only appears on public repos.

**Add** `.github/FUNDING.yml`:
```yaml
# https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/displaying-a-sponsor-button-in-your-repository
github: []           # GitHub Sponsors username(s)
# open_collective: ""
# patreon: ""
# custom: ["https://palmetto.io/sponsor"]
```

---

## 13. Branch Protection / Ruleset Survives Visibility Change 🔵 RECOMMENDED

**Why requires confirmation:** GitHub branch protection rules are not always preserved when
repo visibility changes in edge cases. Verify immediately after going public.

**Verify:**
```bash
gh api repos/Palmetto-Interactive-LLC/Lantern/branches/main/protection \
  --jq '{force_push: .allow_force_pushes.enabled, deletions: .allow_deletions.enabled, linear: .required_linear_history.enabled, pr_required: .required_pull_request_reviews.required_approving_review_count, status_checks: .required_status_checks.contexts}'
# Expected: force_push=false, deletions=false, linear=true, pr_required=1, status_checks=["Rust fmt"]
```

If any rule was lost, re-apply:
```bash
printf '{
  "required_status_checks": {"strict": true, "contexts": ["Rust fmt"]},
  "enforce_admins": false,
  "required_pull_request_reviews": {"require_code_owner_reviews": true, "required_approving_review_count": 1},
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "required_linear_history": true
}' | gh api -X PUT repos/Palmetto-Interactive-LLC/Lantern/branches/main/protection --input -
```

---

## Activation Sequence (At Flip Time)

```
1. Settings → General → Danger Zone → Change visibility → Public
2. Run item 13 verify (branch protection check) immediately
3. Run item 2 verify (secret scanning still enabled)
4. Enable item 3 (private vulnerability reporting) via API
5. Run item 6 verify (dependency graph active)
6. Set item 10 topics via CLI
7. Enable item 7 (dependency-review workflow) — add file + commit + push
8. Check item 8 community profile at /community
9. Items 9, 11, 12 — evaluate based on project goals
```
