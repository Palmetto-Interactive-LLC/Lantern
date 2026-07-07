# Security Policy

## Reporting Vulnerabilities

If you discover a security vulnerability in Lantern, report it privately through GitHub private vulnerability reporting:

https://github.com/Palmetto-Interactive-LLC/Lantern/security/advisories/new

Do not open a public GitHub issue for security vulnerabilities.

Include:

1. A clear description of the vulnerability
2. Reproduction steps or a proof of concept
3. The potential impact and affected versions
4. Any known mitigations or workarounds

If GitHub private vulnerability reporting is unavailable, open a public issue that asks for a private maintainer contact channel without disclosing exploit details.

## Security Considerations

### Local-Only Operation

Lantern is designed to run locally on developer machines without requiring Lantern-owned cloud credentials. It does not need API keys or static cloud secrets to operate.

- No Lantern-owned cloud connectivity required
- No Lantern remote authentication or authorization flows
- No Lantern secret-management integration
- Agent CLIs launched by Lantern may use their own credentials and network behavior; handle those credentials according to the agent vendor's guidance

### Data Storage

- SQLite database stored locally at `~/.lantern/data/relay/lantern.db`
- Local file system access required for git worktrees and terminal management
- Lantern does not intentionally upload its SQLite state. Data may leave your machine through the agent CLIs or commands you ask agents to run.

### Temporal Integration

- Local Temporal dev server runs on `127.0.0.1:8243` (loopback only)
- Intended for local development and testing, not production use
- Docker Temporal is not supported

### Build and Distribution

- Verify checksums of downloaded binaries
- Keep your Rust toolchain and dependencies up to date
- Review the CONTRIBUTING.md guide before building from source
- Treat pull requests and dependency updates as untrusted until CI and human review have completed

## Supported Versions

Security updates will be applied to the latest stable release. Users are encouraged to upgrade promptly when new versions are released.

## Acknowledgments

We appreciate the security research community's responsible disclosure practices.
