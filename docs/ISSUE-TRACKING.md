# Issue Tracking

Linear is Lantern's source of truth for project planning, task state, dependencies, and delivery evidence: [Palmetto Interactive Linear](https://linear.app/palmetto-interactive). GitHub issues are a public intake and discussion surface; they do not replace the linked Linear work item.

## Work Item Types

| Human type | GitHub form | GitHub label | Linear structure | Use when |
| --- | --- | --- | --- | --- |
| Bug | `bug_report.yml` | `type:bug` | Issue | Existing behavior is reproducibly wrong. |
| Feature | `feature_request.yml` | `type:feature` | Issue in a project | A new capability or meaningful behavior change is requested. |
| Epic | `epic.yml` | `type:epic` | Project with child issues | The work is large enough to break into multiple child items. |
| Issue | `issue.yml` | `type:issue` | Issue | The work is valid but not yet clearly a bug or feature. |

## Triage Convention

1. Confirm the GitHub intake has one `type:*` label when GitHub is used.
2. Create or identify the matching Linear issue. Set priority, project, owner, and dependencies there.
3. Link the GitHub issue and any pull request to the Linear issue.
4. Keep blockers and status current in Linear while the work is in progress.
5. Move the Linear issue to Done only after the implementation is merged and the stated verification is complete.

Do not create a repository-local tracker, hook, or synchronization record. Do not put secrets, vulnerability details, customer data, or private incident notes in public GitHub issues. Use private security advisories for vulnerability reports.
