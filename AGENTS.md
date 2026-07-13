# Agent Instructions

## Issue Tracking

Linear is the source of truth for planning and delivery.

- Work from a linked Linear issue and keep its status, dependencies, and delivery evidence current.
- Do not create or rely on a repository-local task database, tracker hook, or sync ref.
- Keep GitHub issues and pull requests linked to the corresponding Linear issue when they are used.

## Non-Interactive Shell Commands

**ALWAYS use non-interactive flags** with file operations to avoid hanging on confirmation prompts.

Shell commands like `cp`, `mv`, and `rm` may be aliased to include `-i` (interactive) mode on some systems, causing the agent to hang indefinitely waiting for y/n input.

**Use these forms instead:**
```bash
# Force overwrite without prompting
cp -f source dest           # NOT: cp source dest
mv -f source dest           # NOT: mv source dest
rm -f file                  # NOT: rm file

# For recursive operations
rm -rf directory            # NOT: rm -r directory
cp -rf source dest          # NOT: cp -r source dest
```

**Other commands that may prompt:**
- `scp` - use `-o BatchMode=yes` for non-interactive
- `ssh` - use `-o BatchMode=yes` to fail instead of prompting
- `apt-get` - use `-y` flag
- `brew` - use `HOMEBREW_NO_AUTO_UPDATE=1` env var

## Review Guidelines

Treat this repository as security-sensitive production software. Reviews should prioritize correctness, least privilege, release integrity, and whether the repository stays within the paid GitHub Team baseline without silently depending on GHAS or Enterprise-only features.

- Flag any secret, token, account ID, ARN, real cluster name, or static cloud credential committed to the repo as P0.
- Flag any `AWS_ACCESS_KEY_ID` or `AWS_SECRET_ACCESS_KEY` usage for deploys as P0; cloud deploy authentication should use OIDC role/federation flows.
- Flag any unpinned GitHub Action (`uses:` not pinned to a full 40-character commit SHA with a version comment) as P0.
- Flag broad IAM/cloud permissions such as `iam:*`, `*:*`, wildcard resources without a documented rationale, or production trust policies that accept every repo ref as P0.
- Flag workflows that omit top-level `permissions: {}` or grant broader job permissions than the job uses as P1.
- Flag any use of `pull_request_target` for model-driven review or code execution as P0.
- Flag missing tests or verification for new scripts, rulesets, workflow behavior, infrastructure changes, or release changes as P1.
- Flag direct human-review requirements that would deadlock a solo developer, such as required approving reviews or required CODEOWNERS review, as P1.
- Flag production deployment paths that bypass protected `main`, immutable GitHub Releases with `v*` tags, environment branch/tag policies, or required status checks as P1.
- Flag logging of secrets, PII, cloud tokens, OIDC tokens, or full GitHub event payloads as P1.

<!-- BEGIN GITHUB SSH TRANSPORT POLICY v:1 -->
## GitHub SSH Transport Policy

GitHub Git transport is SSH-only through the configured per-account host aliases.
Before any GitHub operation, run `git remote -v` and use that remote exactly for
`git fetch`, `git pull`, and `git push`.

Allowed canonical GitHub SSH aliases:

- `git@github-meridian7:...`
- `git@github-palmetto:...`
- `git@github-personal:...`
- `git@github-shelterfitness:...`

Never rewrite a GitHub remote to HTTPS. Never use `https://github.com/...` for
Git transport. Never use direct `git@github.com:...`. Treat legacy duplicate
aliases such as `github.com-client`, `github.com-work`, and `github.com-primary`
as drift and normalize them to the canonical aliases above.

`gh` and GitHub API auth are separate from Git transport. A broken or wrong
`gh` account does not block branch fetch/push when SSH works. Use `gh` only for
PR/API operations, and prove Git access with `git ls-remote <remote-url>` or
`ssh -T git@<alias>`, not with `gh auth status`.

Reference: `/Users/matt/Development/AGENT-GITHUB-MODEL.md`.
<!-- END GITHUB SSH TRANSPORT POLICY -->
