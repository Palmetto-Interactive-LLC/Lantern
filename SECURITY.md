# Security Policy

## Reporting Vulnerabilities

If you discover a security vulnerability in Lantern, please report it to the maintainers privately. **Do not open a public GitHub issue for security vulnerabilities.**

To report a security issue:

1. Email the details to the project maintainers
2. Include a clear description of the vulnerability, how to reproduce it, and the potential impact
3. Allow reasonable time for the maintainers to respond and develop a fix before public disclosure

## Security Considerations

### Local-Only Operation

Lantern is designed to run locally on developer machines without network connectivity or remote dependencies. It does not store credentials, API keys, or sensitive data.

- No cloud connectivity required
- No remote authentication or authorization flows
- No secrets management — configure Lantern with local machine environment only

### Data Storage

- SQLite database stored locally at `~/.lantern/data/relay/lantern.db`
- Local file system access required for git worktrees and terminal management
- No data leaves your machine unless explicitly piped through agent commands

### Temporal Integration

- Local Temporal dev server runs on `127.0.0.1:8243` (loopback only)
- Intended for local development and testing, not production use
- Docker Temporal is not supported

### Build and Distribution

- Verify checksums of downloaded binaries
- Keep your Rust toolchain and dependencies up to date
- Review the CONTRIBUTING.md guide before building from source

## Supported Versions

Security updates will be applied to the latest stable release. Users are encouraged to upgrade promptly when new versions are released.

## Acknowledgments

We appreciate the security research community's responsible disclosure practices.
