# Support

Lantern is an open-source local developer tool. Support is best-effort and focused on reproducible defects, installation failures, security issues, and contribution guidance.

## Supported Scope

| Area | Support level |
| --- | --- |
| Latest tagged release | Security and critical bug fixes |
| `main` branch | Active development, not guaranteed stable |
| macOS + iTerm2 launcher | Supported primary runtime |
| Linux or Windows | Not currently supported for squad launching |
| Third-party agent CLI behavior | Supported only at the Lantern integration boundary |

## Where To Ask

- Bugs and reproducible install failures: [GitHub issues](https://github.com/Palmetto-Interactive-LLC/Lantern/issues)
- Security vulnerabilities: [private vulnerability reporting](https://github.com/Palmetto-Interactive-LLC/Lantern/security/advisories/new)
- Contribution workflow: [CONTRIBUTING.md](CONTRIBUTING.md)
- Operational troubleshooting: [docs/how-to/troubleshoot-issues.md](docs/how-to/troubleshoot-issues.md)

## What To Include

For support requests, include:

- Lantern version: `lantern --version`
- macOS version and CPU architecture
- Install path used: release installer, source checkout, or local binary copy
- Relevant command output from `lantern doctor`
- The exact command that failed
- Whether iTerm2 Python API is enabled
- Agent CLI and version, if the issue involves launch or MCP registration

Do not paste credentials, API keys, agent auth files, local database contents, or private project source.
