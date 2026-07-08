//! Resolve a `PatternConfig` for `startwork`: interactive `inquire` menus when
//! attached to a tty, non-interactive flags otherwise.

use std::io::IsTerminal;

use anyhow::{bail, Context, Result};
use inquire::{CustomType, Select, Text};

use super::patterns::{
    executor_model_menu, find_model, orchestrator_model_menu, AgentKind, ModelChoice, PatternConfig,
};

/// Non-interactive bypass flags threaded from clap on the `Startwork` command.
#[derive(Debug, Default, Clone)]
pub struct PatternCliArgs {
    pub pattern: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub orch_model: Option<String>,
    pub workers: Option<u8>,
    pub issue: Option<String>,
}

/// Resolve a `PatternConfig`:
/// - `--pattern` given: fully non-interactive, errors on missing required flags.
/// - no `--pattern`, stdin not a tty: defaults to `team + claude` (today's
///   scripted `startwork` behavior).
/// - no `--pattern`, stdin is a tty: interactive `inquire` menus.
pub fn resolve_pattern(args: PatternCliArgs) -> Result<PatternConfig> {
    if let Some(pattern) = args.pattern.clone() {
        return resolve_non_interactive(&pattern, &args);
    }

    if !std::io::stdin().is_terminal() {
        let agent = args
            .agent
            .as_deref()
            .and_then(AgentKind::parse)
            .unwrap_or(AgentKind::Claude);
        return Ok(PatternConfig::team(agent));
    }

    resolve_interactive()
}

fn resolve_non_interactive(pattern: &str, args: &PatternCliArgs) -> Result<PatternConfig> {
    match pattern.to_ascii_lowercase().as_str() {
        "team" => {
            let agent = match args.agent.as_deref() {
                Some(raw) => AgentKind::parse(raw)
                    .with_context(|| format!("--pattern team: unknown --agent '{raw}'"))?,
                None => AgentKind::Claude,
            };
            Ok(PatternConfig::team(agent))
        }
        "executor" => {
            let model = resolve_model(args.model.as_deref(), &executor_model_menu())?;
            Ok(PatternConfig::executor(model))
        }
        "simple" => {
            let orch = resolve_model(args.orch_model.as_deref(), &orchestrator_model_menu())?;
            let workers = args.workers.unwrap_or(4);
            if !(1..=10).contains(&workers) {
                bail!("--workers must be between 1 and 10 (got {workers})");
            }
            let worker_model = resolve_model(args.model.as_deref(), &executor_model_menu())?;
            Ok(PatternConfig::simple(orch, workers, worker_model))
        }
        "fixbug" => {
            let issue = args
                .issue
                .clone()
                .filter(|s| !s.trim().is_empty())
                .context("--pattern fixbug requires --issue <ref>")?;
            let fixer = resolve_model(args.model.as_deref(), &executor_model_menu())?;
            Ok(PatternConfig::fixbug(issue, fixer))
        }
        other => bail!("unknown --pattern '{other}' (expected team|executor|simple|fixbug)"),
    }
}

fn resolve_model(query: Option<&str>, menu: &[ModelChoice]) -> Result<ModelChoice> {
    match query {
        None => Ok(menu[0].clone()),
        Some(q) => find_model(menu, q).cloned().with_context(|| {
            format!(
                "unknown model '{q}' (expected one of: {})",
                menu_labels(menu)
            )
        }),
    }
}

fn menu_labels(menu: &[ModelChoice]) -> String {
    menu.iter()
        .map(|m| m.label.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Bold name + dim description, the shared look of every startwork menu row.
fn menu_row(name: &str, desc: &str) -> String {
    format!("{BOLD}{name}{RESET}  {DIM}{desc}{RESET}")
}

/// Present a styled `Select` and return the chosen index (options are
/// ANSI-styled strings, so selection is by index, not by string match).
fn select_index(prompt: &str, rows: Vec<String>) -> Result<usize> {
    Ok(Select::new(prompt, rows)
        .raw_prompt()
        .with_context(|| format!("'{prompt}' selection cancelled"))?
        .index)
}

fn resolve_interactive() -> Result<PatternConfig> {
    let pattern_rows = vec![
        menu_row(
            "Team Orchestrator",
            "1 orchestrator + 8 specialist workers — the full 10-pane grid",
        ),
        menu_row(
            "Executor",
            "one executor pane guided by a quiet Fable 5 advisor",
        ),
        menu_row(
            "Simple Orchestrator",
            "you drive the orchestrator; it delegates to 1-10 identical workers",
        ),
        menu_row(
            "Fix a Bug",
            "one maintainer agent takes an issue through PR, CI, and review to merge",
        ),
    ];

    match select_index("Launch pattern:", pattern_rows)? {
        0 => {
            let agent_rows = vec![
                menu_row("Claude", "Claude Code CLI"),
                menu_row("Codex", "OpenAI Codex CLI"),
                menu_row("Gemini", "Antigravity (agy) CLI"),
            ];
            let agent = match select_index("Agent:", agent_rows)? {
                1 => AgentKind::Codex,
                2 => AgentKind::Gemini,
                _ => AgentKind::Claude,
            };
            Ok(PatternConfig::team(agent))
        }
        1 => {
            let model = select_model("Executor model:", executor_model_menu())?;
            Ok(PatternConfig::executor(model))
        }
        2 => {
            let orch = select_model("Orchestrator model:", orchestrator_model_menu())?;
            let workers = CustomType::<u8>::new("Number of workers (1-10):")
                .with_default(4)
                .with_validator(|v: &u8| {
                    if (1..=10).contains(v) {
                        Ok(inquire::validator::Validation::Valid)
                    } else {
                        Ok(inquire::validator::Validation::Invalid(
                            "must be between 1 and 10".into(),
                        ))
                    }
                })
                .prompt()
                .context("worker count prompt cancelled")?;
            let worker_model = select_model("Worker model:", executor_model_menu())?;
            Ok(PatternConfig::simple(orch, workers, worker_model))
        }
        3 => {
            let issue = Text::new("Issue reference:")
                .with_help_message("GitHub issue number/URL, or a beads id (e.g. lan-abc)")
                .prompt()
                .context("issue prompt cancelled")?;
            let fixer = select_model("Fixer model:", executor_model_menu())?;
            Ok(PatternConfig::fixbug(issue, fixer))
        }
        other => bail!("unexpected pattern menu selection index {other}"),
    }
}

fn select_model(prompt: &str, menu: Vec<ModelChoice>) -> Result<ModelChoice> {
    let rows: Vec<String> = menu
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let default_tag = if i == 0 { "  (default)" } else { "" };
            menu_row(
                &m.label,
                &format!("{} @ {}{}", m.model_id, m.effort, default_tag),
            )
        })
        .collect();
    let idx = select_index(prompt, rows)?;
    menu.into_iter()
        .nth(idx)
        .context("selected model not found in menu")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_pattern_flag_defaults_to_team_claude_when_not_tty() {
        // This test always runs non-interactively (no tty in CI), so it
        // exercises the "no --pattern, non-tty" branch directly.
        let cfg = resolve_pattern(PatternCliArgs::default()).expect("resolve default");
        assert_eq!(cfg.pattern_slug(), "team");
    }

    #[test]
    fn pattern_flag_team_honors_agent() {
        let cfg = resolve_pattern(PatternCliArgs {
            pattern: Some("team".to_string()),
            agent: Some("codex".to_string()),
            ..Default::default()
        })
        .expect("resolve team+codex");
        assert_eq!(cfg.pattern_slug(), "team");
    }

    #[test]
    fn pattern_flag_executor_defaults_model() {
        let cfg = resolve_pattern(PatternCliArgs {
            pattern: Some("executor".to_string()),
            ..Default::default()
        })
        .expect("resolve executor");
        assert_eq!(cfg.pattern_slug(), "executor");
    }

    #[test]
    fn pattern_flag_simple_defaults_workers_to_4() {
        let cfg = resolve_pattern(PatternCliArgs {
            pattern: Some("simple".to_string()),
            ..Default::default()
        })
        .expect("resolve simple");
        assert_eq!(cfg.pane_count(), 1 + 4 + 1);
    }

    #[test]
    fn pattern_flag_simple_rejects_out_of_range_workers() {
        let err = resolve_pattern(PatternCliArgs {
            pattern: Some("simple".to_string()),
            workers: Some(11),
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.to_string().contains("--workers"));
    }

    #[test]
    fn pattern_flag_fixbug_requires_issue() {
        let err = resolve_pattern(PatternCliArgs {
            pattern: Some("fixbug".to_string()),
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.to_string().contains("--issue"));
    }

    #[test]
    fn pattern_flag_fixbug_with_issue_resolves() {
        let cfg = resolve_pattern(PatternCliArgs {
            pattern: Some("fixbug".to_string()),
            issue: Some("ISSUE-42".to_string()),
            ..Default::default()
        })
        .expect("resolve fixbug");
        assert_eq!(cfg.pattern_slug(), "fixbug");
    }

    #[test]
    fn unknown_model_label_errors_with_menu_listing() {
        let err = resolve_pattern(PatternCliArgs {
            pattern: Some("executor".to_string()),
            model: Some("Not A Real Model".to_string()),
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.to_string().contains("Sonnet 5 High"));
    }

    #[test]
    fn unknown_pattern_slug_errors() {
        let err = resolve_pattern(PatternCliArgs {
            pattern: Some("bogus".to_string()),
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.to_string().contains("unknown --pattern"));
    }
}
