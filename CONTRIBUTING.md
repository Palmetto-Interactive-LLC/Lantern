# Contributing to Lantern

We welcome contributions to Lantern. Please read this guide to understand our development workflow, quality standards, and issue-tracking process.

## Development Environment Setup

### Prerequisites

- macOS with iTerm2
- Rust stable (install via [rustup](https://rustup.rs/))
- Temporal CLI (`brew install temporal`)
- git

### Building from Source

```bash
git clone https://github.com/Palmetto-Interactive-LLC/Lantern.git
cd Lantern
cargo build --release
```

The binary is produced at `target/release/lantern`.

### Local Installation

To test your changes with the installed service:

```bash
cargo build --release
cp target/release/lantern ~/.lantern/bin/lantern
lantern restart
```

## Build and Test Gates

All code must pass the following gates before merging to `main`:

### One-Command Verification

```bash
make verify
make security
```

`make verify` is the normal local gate for contributors. `make security` requires `cargo-audit`, `shellcheck`, `actionlint`, and `gitleaks`; run it when changing workflows, scripts, dependencies, release packaging, or security-sensitive paths.

### Code Formatting

```bash
cargo fmt --check
```

Format your code before committing:

```bash
cargo fmt
```

### Linting

```bash
cargo clippy --all-targets -- -D warnings
```

Fix any clippy warnings before submitting a PR.

### Testing

```bash
cargo test
```

All tests must pass. The default test suite uses isolated temporary SQLite databases and does not require a running Temporal server.

### CI Pipeline

GitHub Actions runs Rust formatting, strict clippy, Markdown relative-link checks, release build, tests, security scans, action linting, and CodeQL on pushes and PRs to `main`. The branch ruleset requires the `lint`, `build-test`, `secrets-scan`, `sast`, `deps-scan`, `iac-scan`, and `actions-lint` contexts before merge.

## Branch and PR Workflow

### Branch Naming

Create feature branches for your work:

```bash
git checkout -b feature/your-feature-name
git checkout -b fix/bug-description
```

### Commit Message Conventions

Write clear, descriptive commit messages following conventional commit format:

```
type(scope): brief summary

Longer description explaining the why and how, if needed.

Fixes #123  (if closing an issue)
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `chore`

Example:

```
feat(startwork): add --agent goose option for quiet orchestrator mode

Allows launching a single Goose agent without a headless ACP specialist team.
Updates squad initialization to skip team role derivation when goose mode requested.

Fixes #42
```

### Pull Request Requirements

1. **Create PR to `main` only** — feature branches must merge to `main`, not to another branch
2. **Linear history required** — rebase your branch before merging (no merge commits)
3. **Review threads resolved** — maintainers may review, but CODEOWNERS approval is not required by the current ruleset
4. **All tests pass** — local `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings` must succeed
5. **CI passes** — required GitHub Actions contexts must pass before merge

### Merging

After PR approval:

```bash
git pull --rebase origin main
git push origin your-branch
```

Use GitHub's configured squash merge path. The ruleset enforces signed commits and linear history.

## Issue Tracking with Linear

Linear is the source of truth for Lantern planning and delivery: [Palmetto Interactive Linear](https://linear.app/palmetto-interactive). GitHub issues are intake or public discussion; implementation work must be linked to a Linear issue.

### Workflow

1. **Find or create the Linear issue**: Make sure the work has clear scope, acceptance criteria, and ownership.
2. **Mark it in progress**: Record dependencies and material blockers in Linear before implementation begins.
3. **Implement**: Create a branch and commit scoped changes.
4. **Test locally**: Run all repository build and test gates before pushing.
5. **Push and PR**: Open a PR linked to the Linear issue.
6. **Close only with evidence**: After the PR is merged and verified, update the Linear issue with the delivery evidence and move it to Done.

Do not create local tracker data, hooks, or synchronization state. Do not put secrets, vulnerability details, customer data, or private incident notes in public GitHub issues. Use private security advisories for vulnerability reports.

## Documentation

Documentation follows the [Diátaxis](https://diataxis.fr/) framework. When adding or updating docs:

- **Tutorial** — learning-oriented, one guided path (`docs/tutorial/`)
- **How-to** — task-oriented, problem in the title (`docs/how-to/`)
- **Reference** — facts only, no steps (`docs/reference/`)
- **Explanation** — context and why (`docs/explanation/`)

Do not mix documentation types in a single page. Link between them instead.

For full documentation development guidance, see [How to develop and contribute](docs/how-to/develop-and-contribute.md).

## Code Review

Your PR will be reviewed by project maintainers. Expected feedback:

- Code correctness and safety
- Adherence to Rust idioms and project patterns
- Test coverage and quality gates
- Documentation completeness
- Commit message clarity

Address review feedback by pushing new commits to your branch. Do not force-push after review has started.

## Questions?

- Check [existing issues](https://github.com/Palmetto-Interactive-LLC/Lantern/issues)
- Read [SUPPORT.md](SUPPORT.md) for support scope and issue-reporting details
- Read [ROADMAP.md](ROADMAP.md) for current stable and experimental surfaces
- Read [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) for community standards
- Review [CLAUDE.md](CLAUDE.md) for project context and architecture

Thank you for contributing!
