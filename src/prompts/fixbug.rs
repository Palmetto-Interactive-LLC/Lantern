//! Kickoff instructions injected into the `fixer` pane for `DEVORCH_PATTERN=fixbug`.
//!
//! Adapted from Anthropic's issue-to-pr maintainer-bot workflow: fetch and
//! read the issue, explore before editing, implement the fix, run tests,
//! open a PR, watch CI, address review/CI feedback deliberately (never
//! blind-retry), get human approval via `devorch_blocker` before merging,
//! then independently re-verify after merge.

/// Resolved context for the issue a `fixbug` session is targeting. `title`/
/// `body`/`url` are `None` when resolution failed (network error, unknown
/// id, `gh`/`bd` not available, etc.) — in that case `resolution_note`
/// explains what was tried so the fixer knows to look the issue up itself.
#[derive(Debug, Clone, Default)]
pub struct IssueContext {
    /// The raw `--issue` value as given on the CLI (number, URL, or `bd-*` id).
    pub raw_ref: String,
    pub title: Option<String>,
    pub body: Option<String>,
    pub url: Option<String>,
    /// Set when automatic resolution via `gh`/`bd` failed; explains why.
    pub resolution_note: Option<String>,
}

/// Build the kickoff prompt injected as the fixer pane's initial message.
pub fn kickoff_instructions(issue: &IssueContext, branch: &str, repo_id: &str) -> String {
    let issue_block = match (&issue.title, &issue.body, &issue.url) {
        (Some(title), body, url) => {
            let url_line = url
                .as_deref()
                .map(|u| format!("URL: {u}\n"))
                .unwrap_or_default();
            let body_text = body.as_deref().unwrap_or("(no description provided)");
            format!(
                "Issue reference: {raw}\n{url_line}Title: {title}\n\nBody:\n{body_text}",
                raw = issue.raw_ref
            )
        }
        _ => {
            let note = issue
                .resolution_note
                .as_deref()
                .unwrap_or("no resolution was attempted");
            format!(
                "Issue reference: {raw} (could not auto-resolve title/body: {note}).\n\
                 Look it up yourself before proceeding — try `gh issue view {raw}` or `bd show {raw}`.",
                raw = issue.raw_ref
            )
        }
    };

    format!(
        "You are the FIXER for a `fixbug` session on repo `{repo_id}`, working in this \
         worktree on branch `{branch}`. Your job: fix one issue, open a PR, and see it \
         through review — do not stop at \"PR opened\".\n\n\
         {issue_block}\n\n\
         Workflow:\n\
         1. FETCH AND READ: re-fetch the issue yourself if anything above is thin (`gh issue view` \
            for a GitHub issue, `bd show` for a beads id) — get the full title, body, and any \
            comments before you touch code.\n\
         2. EXPLORE FIRST: read the affected files and their callers/tests before editing. Do not \
            guess at a fix from the issue text alone — reproduce or trace the bug in the code.\n\
         3. IMPLEMENT: make the fix in this worktree, on this branch, matching existing style. Keep \
            the diff scoped to the bug — no drive-by refactors.\n\
         4. TEST: run this repo's real test/build/lint commands (see CLAUDE.md) and make sure they \
            pass before opening a PR.\n\
         5. OPEN A PR: push this branch and open a PR with `gh pr create`, describing the root cause \
            and the fix, and linking the issue.\n\
         6. WATCH CI: run `gh pr checks --watch`. If CI fails or a reviewer requests changes, read \
            exactly what they said and address that specific feedback — never retry blindly or \
            re-push hoping a flake clears without understanding why it failed.\n\
         7. BLOCKER BEFORE MERGE: before merging, you MUST call the `devorch_blocker` MCP tool \
            summarizing the PR (URL, what changed, CI status) and wait for a human to approve the \
            merge. Do not merge on your own judgment.\n\
         8. AFTER MERGE: once merged, independently re-verify — re-run `gh pr checks` and print the \
            final PR state (merged, checks green) as proof. Do not report done from memory of an \
            earlier check.\n\n\
         Work only in this worktree. If you get stuck, call `devorch_blocker` and explain exactly \
         what's blocking you."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_issue_embeds_title_body_url() {
        let issue = IssueContext {
            raw_ref: "42".to_string(),
            title: Some("Panes leak worktrees".to_string()),
            body: Some("Repro: ...".to_string()),
            url: Some("https://github.com/org/repo/issues/42".to_string()),
            resolution_note: None,
        };
        let text = kickoff_instructions(&issue, "proj-fix-42-1", "proj");
        assert!(text.contains("Panes leak worktrees"));
        assert!(text.contains("Repro: ..."));
        assert!(text.contains("https://github.com/org/repo/issues/42"));
        assert!(text.contains("devorch_blocker"));
        assert!(text.contains("proj-fix-42-1"));
    }

    #[test]
    fn unresolved_issue_falls_back_to_raw_ref_with_note() {
        let issue = IssueContext {
            raw_ref: "bd-99".to_string(),
            resolution_note: Some("bd show exited non-zero".to_string()),
            ..Default::default()
        };
        let text = kickoff_instructions(&issue, "proj-fix-bd-99-1", "proj");
        assert!(text.contains("bd-99"));
        assert!(text.contains("bd show exited non-zero"));
        assert!(text.contains("could not auto-resolve"));
    }
}
