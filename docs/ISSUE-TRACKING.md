# Issue Tracking

GitHub issues are the human intake surface. Beads is the durable agent task
graph. A triaged GitHub issue should be mirrored into Beads before
implementation starts, and pull requests should reference both IDs when both
exist.

## Work Item Types

| Human type | GitHub form | GitHub label | Beads type | Use when |
| --- | --- | --- | --- | --- |
| Bug | `bug_report.yml` | `type:bug` | `bug` | Existing behavior is reproducibly wrong. |
| Feature | `feature_request.yml` | `type:feature` | `feature` | A new capability or meaningful behavior change is requested. |
| Epic | `epic.yml` | `type:epic` | `epic` | The work is large enough to break into multiple child items. |
| Issue | `issue.yml` | `type:issue` | `task` | The work is valid but not yet clearly a bug, feature, or epic. |

Beads calls the general work item type `task`; GitHub users see that same
category as `Issue` because it is the plain-language intake label.

## Triage Convention

1. Confirm the GitHub issue has exactly one `type:*` label.
2. Convert severity or urgency into Beads priority:
   `S0/P0` is critical, `S1/P1` is high, `S2/P2` is normal,
   `S3/P3` is low, and `P4` is backlog.
3. Create the matching Beads item:

   ```bash
   bd create --title="Short title" --description="GitHub issue: URL" --type=bug --priority=2
   bd create --title="Short title" --description="GitHub issue: URL" --type=feature --priority=2
   bd create --title="Short title" --description="GitHub issue: URL" --type=epic --priority=2
   bd create --title="Short title" --description="GitHub issue: URL" --type=task --priority=2
   ```

4. Link children to epics with `bd dep add CHILD_ID EPIC_ID`.
5. Mention the Beads ID in the GitHub issue or pull request.
6. Close the Beads item only when the implementation work is actually done.

Do not put secrets, vulnerability details, customer data, or private incident
notes in public GitHub issues. Use private security advisories for
vulnerability reports.
