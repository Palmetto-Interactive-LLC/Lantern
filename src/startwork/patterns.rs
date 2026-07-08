//! Shared launch-pattern contract for `startwork`.
//!
//! This is the single source of truth for the four ways a squad can be
//! launched (Team Orchestrator / Executor / Simple Orchestrator / Fix a Bug):
//! the model menus, per-pane `RoleSpec`s, and `LayoutSpec` geometry. `menu.rs`
//! resolves a `PatternConfig` from CLI flags or interactive prompts; `mod.rs`
//! dispatches on it to actually launch panes.
//!
//! `TeamOrchestrator` must stay behavior-identical to the legacy all-panes
//! grid — it reuses `GRID_ORDER`, `TEAM_LABELS`, and `TEAM_COLORS` from
//! `super` rather than redefining them.

use super::{GRID_ORDER, TEAM_COLORS, TEAM_LABELS};

/// Agent CLI families launchable in a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Codex,
    Gemini,
    Kimi,
    Goose,
}

impl AgentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            // Gemini-family models launch through the Antigravity CLI; "agy"
            // is the agent-kind string every downstream launch path expects.
            AgentKind::Gemini => "agy",
            AgentKind::Kimi => "kimi",
            AgentKind::Goose => "goose",
        }
    }

    /// Parse a CLI-facing agent token (`--agent`, trailing positional, or a
    /// menu selection) into an `AgentKind`. Case-insensitive. `gemini`,
    /// `agy`, and `agi` all mean the Antigravity-launched Gemini family.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "claude" => Some(AgentKind::Claude),
            "codex" => Some(AgentKind::Codex),
            "gemini" | "agy" | "agi" => Some(AgentKind::Gemini),
            "kimi" => Some(AgentKind::Kimi),
            "goose" => Some(AgentKind::Goose),
            _ => None,
        }
    }
}

/// A concrete model pick surfaced in the executor/worker/fixer/orchestrator
/// menus (see `executor_model_menu()` / `orchestrator_model_menu()`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub agent: AgentKind,
    pub model_id: String,
    pub effort: String,
    pub label: String,
}

impl ModelChoice {
    fn new(agent: AgentKind, model_id: &str, effort: &str, label: &str) -> Self {
        Self {
            agent,
            model_id: model_id.to_string(),
            effort: effort.to_string(),
            label: label.to_string(),
        }
    }

    /// Antigravity (`agy`) selects models by display name (e.g. "Gemini 3.1
    /// Pro (High)"), not by dotted model id — mirror the naming convention
    /// the team grid's agy launch arm already uses for `ANTIGRAVITY_MODEL`.
    pub fn antigravity_model(&self) -> &str {
        &self.label
    }
}

/// Prefer the self-updating model registry's cached manifest
/// (`~/.lantern/data/models_cache.json`, see `models_registry`) when it has
/// entries for `tier`, falling back to `None` so callers can use their
/// compiled-in table. Never touches the network — `load_menu_override()` is
/// a plain file read.
fn menu_override_for_tier(tier: &str) -> Option<Vec<ModelChoice>> {
    let manifest = crate::models_registry::load_menu_override()?;
    let entries: Vec<ModelChoice> = manifest
        .models
        .into_iter()
        .filter(|m| m.tier == tier)
        .filter_map(|m| {
            let agent = AgentKind::parse(&m.agent)?;
            Some(ModelChoice::new(agent, &m.model_id, &m.effort, &m.label))
        })
        .collect();
    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

/// Model menu shared by the executor / worker / fixer picks. First entry is
/// the default ("Sonnet 5 High"). Prefers the cached model registry manifest
/// when present (see `menu_override_for_tier`).
pub fn executor_model_menu() -> Vec<ModelChoice> {
    if let Some(menu) = menu_override_for_tier("executor") {
        return menu;
    }
    vec![
        ModelChoice::new(
            AgentKind::Claude,
            "claude-sonnet-5",
            "high",
            "Sonnet 5 High",
        ),
        ModelChoice::new(AgentKind::Claude, "claude-haiku-4-5", "high", "Haiku High"),
        ModelChoice::new(AgentKind::Codex, "gpt-5.5", "high", "GPT 5.5 High"),
        ModelChoice::new(AgentKind::Codex, "gpt-5.5", "medium", "GPT 5.5 Medium"),
        ModelChoice::new(
            AgentKind::Codex,
            "gpt-5.3-codex-spark",
            "medium",
            "GPT 5.3 Codex Spark",
        ),
        ModelChoice::new(
            AgentKind::Gemini,
            "gemini-3.5-flash",
            "high",
            "Gemini 3.5 Flash (High)",
        ),
        ModelChoice::new(
            AgentKind::Gemini,
            "gemini-3.1-pro",
            "high",
            "Gemini 3.1 Pro (High)",
        ),
    ]
}

/// Orchestrator menu for `SimpleOrchestrator`'s `orch` pick. First entry is
/// the default ("Fable 5 XHIGH"). Prefers the cached model registry manifest
/// when present (see `menu_override_for_tier`).
pub fn orchestrator_model_menu() -> Vec<ModelChoice> {
    if let Some(menu) = menu_override_for_tier("orchestrator") {
        return menu;
    }
    vec![
        ModelChoice::new(
            AgentKind::Claude,
            "claude-fable-5",
            "xhigh",
            "Fable 5 XHIGH",
        ),
        ModelChoice::new(
            AgentKind::Claude,
            "claude-opus-4-8",
            "xhigh",
            "Opus 4.8 XHIGH",
        ),
    ]
}

/// The implicit advisor riding alongside an `Executor` pane — always Fable 5
/// XHIGH, never user-selectable.
pub fn advisor_model() -> ModelChoice {
    ModelChoice::new(
        AgentKind::Claude,
        "claude-fable-5",
        "xhigh",
        "Fable 5 XHIGH",
    )
}

/// Find a model in `menu` by exact label (case-insensitive) or model id.
pub fn find_model<'a>(menu: &'a [ModelChoice], query: &str) -> Option<&'a ModelChoice> {
    let q = query.trim();
    menu.iter()
        .find(|m| m.label.eq_ignore_ascii_case(q) || m.model_id == q)
}

/// The four launch patterns `startwork` can produce.
///
/// Non-team variants' fields are read by `PatternConfig::pattern_slug()`'s
/// match arms (by discriminant) but not yet destructured field-by-field —
/// that lands with the executor/simple/fixbug launch implementations other
/// agents are adding on top of this foundation commit.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum LaunchPattern {
    TeamOrchestrator {
        agent: AgentKind,
    },
    /// Single executor pane + an implicit Fable 5 XHIGH advisor pane.
    Executor {
        executor: ModelChoice,
    },
    SimpleOrchestrator {
        orch: ModelChoice,
        workers: u8,
        worker_model: ModelChoice,
    },
    FixABug {
        issue: String,
        fixer: ModelChoice,
    },
}

/// One pane's static configuration. Consumed by the (forthcoming) per-pattern
/// launch implementations; not yet read outside tests in this foundation
/// commit.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RoleSpec {
    pub role: String,
    pub label: String,
    pub color: (u8, u8, u8),
    pub model: ModelChoice,
    pub needs_worktree: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LayoutColumn {
    pub weight: u32,
    pub rows: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LayoutSpec {
    pub columns: Vec<LayoutColumn>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PatternConfig {
    pub pattern: LaunchPattern,
    pub roles: Vec<RoleSpec>,
    pub layout: LayoutSpec,
}

impl PatternConfig {
    #[allow(dead_code)]
    pub fn pane_count(&self) -> usize {
        self.roles.len()
    }

    pub fn pattern_slug(&self) -> &'static str {
        match self.pattern {
            LaunchPattern::TeamOrchestrator { .. } => "team",
            LaunchPattern::Executor { .. } => "executor",
            LaunchPattern::SimpleOrchestrator { .. } => "simple",
            LaunchPattern::FixABug { .. } => "fixbug",
        }
    }

    /// Current 4x2+1 team grid: 3 columns (orch+input stacked in column 1,
    /// then two 4-worker columns), matching `GRID_ORDER` and
    /// `iterm_launch.py`'s layout exactly.
    pub fn team(agent: AgentKind) -> PatternConfig {
        let roles = GRID_ORDER
            .iter()
            .map(|&role| {
                let label = TEAM_LABELS
                    .iter()
                    .find(|(r, _)| *r == role)
                    .map(|(_, l)| *l)
                    .unwrap_or(role);
                let color = TEAM_COLORS
                    .iter()
                    .find(|(r, _)| *r == role)
                    .map(|(_, c)| (c[0], c[1], c[2]))
                    .unwrap_or((40, 40, 40));
                RoleSpec {
                    role: role.to_string(),
                    label: label.to_string(),
                    color,
                    model: team_role_model(agent, role),
                    needs_worktree: role != "inp",
                }
            })
            .collect();

        let layout = LayoutSpec {
            columns: vec![
                LayoutColumn {
                    weight: 33,
                    rows: vec!["orch".to_string(), "inp".to_string()],
                },
                LayoutColumn {
                    weight: 33,
                    rows: vec![
                        "ai".to_string(),
                        "dat".to_string(),
                        "plt".to_string(),
                        "doc".to_string(),
                    ],
                },
                LayoutColumn {
                    weight: 34,
                    rows: vec![
                        "sec".to_string(),
                        "ops".to_string(),
                        "ui".to_string(),
                        "qa".to_string(),
                    ],
                },
            ],
        };

        PatternConfig {
            pattern: LaunchPattern::TeamOrchestrator { agent },
            roles,
            layout,
        }
    }

    /// One executor worktree pane (70%) + a non-worktree advisor pane (30%),
    /// plus the input router stacked under the executor column (same
    /// placement `team()` uses for `inp` today).
    pub fn executor(executor: ModelChoice) -> PatternConfig {
        let advisor = advisor_model();
        let roles = vec![
            RoleSpec {
                role: "executor".to_string(),
                label: "EXEC".to_string(),
                color: (30, 32, 35),
                model: executor.clone(),
                needs_worktree: true,
            },
            RoleSpec {
                role: "advisor".to_string(),
                label: "ADVISOR".to_string(),
                color: (45, 27, 83),
                model: advisor.clone(),
                needs_worktree: false,
            },
            RoleSpec {
                role: "inp".to_string(),
                label: "INPUT".to_string(),
                color: (45, 45, 45),
                model: advisor,
                needs_worktree: false,
            },
        ];
        let layout = LayoutSpec {
            columns: vec![
                LayoutColumn {
                    weight: 70,
                    rows: vec!["executor".to_string(), "inp".to_string()],
                },
                LayoutColumn {
                    weight: 30,
                    rows: vec!["advisor".to_string()],
                },
            ],
        };
        PatternConfig {
            pattern: LaunchPattern::Executor { executor },
            roles,
            layout,
        }
    }

    /// One orchestrator pane (33%) + N worker worktree panes (1..=10, default
    /// 4), split into a single worker column when N<=5 or two balanced
    /// columns otherwise, plus the input router stacked under `orch`.
    pub fn simple(orch: ModelChoice, workers: u8, worker_model: ModelChoice) -> PatternConfig {
        let workers = workers.clamp(1, 10);
        let mut roles = vec![RoleSpec {
            role: "orch".to_string(),
            label: "ORCH".to_string(),
            color: (30, 32, 35),
            model: orch.clone(),
            needs_worktree: true,
        }];
        for i in 1..=workers {
            roles.push(RoleSpec {
                role: format!("worker-{i}"),
                label: format!("W{i}"),
                color: worker_color(i),
                model: worker_model.clone(),
                needs_worktree: true,
            });
        }
        roles.push(RoleSpec {
            role: "inp".to_string(),
            label: "INPUT".to_string(),
            color: (45, 45, 45),
            model: worker_model.clone(),
            needs_worktree: false,
        });

        let worker_rows: Vec<String> = (1..=workers).map(|i| format!("worker-{i}")).collect();
        let mut columns = vec![LayoutColumn {
            weight: 33,
            rows: vec!["orch".to_string(), "inp".to_string()],
        }];
        if workers <= 5 {
            columns.push(LayoutColumn {
                weight: 67,
                rows: worker_rows,
            });
        } else {
            let first_n = worker_rows.len().div_ceil(2); // front column gets the extra row
            let (first, second) = worker_rows.split_at(first_n);
            let w1 = 67 / 2;
            let w2 = 67 - w1;
            columns.push(LayoutColumn {
                weight: w1,
                rows: first.to_vec(),
            });
            columns.push(LayoutColumn {
                weight: w2,
                rows: second.to_vec(),
            });
        }

        PatternConfig {
            pattern: LaunchPattern::SimpleOrchestrator {
                orch,
                workers,
                worker_model,
            },
            roles,
            layout: LayoutSpec { columns },
        }
    }

    /// One fixer worktree pane, full width, with the input router stacked
    /// underneath.
    pub fn fixbug(issue: String, fixer: ModelChoice) -> PatternConfig {
        let roles = vec![
            RoleSpec {
                role: "fixer".to_string(),
                label: "FIXER".to_string(),
                color: (7, 57, 25),
                model: fixer.clone(),
                needs_worktree: true,
            },
            RoleSpec {
                role: "inp".to_string(),
                label: "INPUT".to_string(),
                color: (45, 45, 45),
                model: fixer.clone(),
                needs_worktree: false,
            },
        ];
        let layout = LayoutSpec {
            columns: vec![LayoutColumn {
                weight: 100,
                rows: vec!["fixer".to_string(), "inp".to_string()],
            }],
        };
        PatternConfig {
            pattern: LaunchPattern::FixABug { issue, fixer },
            roles,
            layout,
        }
    }
}

/// Legacy per-role model mapping for the team pattern's `RoleSpec.model`
/// metadata (informational only — the actual team launch path still uses
/// `get_model_for_role`/`build_agent_command` in `mod.rs`, unchanged, so this
/// has no effect on today's behavior).
fn team_role_model(agent: AgentKind, role: &str) -> ModelChoice {
    let (model_id, effort): (&str, &str) = match agent {
        AgentKind::Claude => match role {
            "orch" | "ai" | "sec" => ("opus", "high"),
            "doc" => ("haiku", "high"),
            _ => ("sonnet", "high"),
        },
        AgentKind::Codex => match role {
            "orch" | "ai" | "sec" => ("gpt-5.5", "xhigh"),
            _ => ("gpt-5.4-mini", "low"),
        },
        AgentKind::Gemini => match role {
            "orch" | "ai" | "sec" => ("gemini-3.1-pro", "low"),
            "doc" => ("gpt-oss-120b", "medium"),
            _ => ("gemini-3.5-flash", "medium"),
        },
        AgentKind::Kimi => ("kimi-code/kimi-for-coding", "medium"),
        AgentKind::Goose => ("opus", "medium"),
    };
    ModelChoice::new(agent, model_id, effort, role)
}

/// Cycles through the existing team worker palette (skipping orch/input) so
/// `simple()` worker panes get visually distinct colors without inventing a
/// new palette.
fn worker_color(index: u8) -> (u8, u8, u8) {
    const PALETTE: &[(u8, u8, u8)] = &[
        (62, 49, 0),
        (45, 27, 83),
        (0, 17, 51),
        (0, 53, 58),
        (7, 57, 25),
        (78, 24, 24),
        (70, 28, 0),
        (80, 0, 80),
        (30, 60, 60),
        (60, 30, 60),
    ];
    let i = (index.saturating_sub(1)) as usize % PALETTE.len();
    PALETTE[i]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_matches_grid_order() {
        let cfg = PatternConfig::team(AgentKind::Claude);
        assert_eq!(cfg.pane_count(), GRID_ORDER.len());
        assert_eq!(cfg.pattern_slug(), "team");
        let roles: Vec<&str> = cfg.roles.iter().map(|r| r.role.as_str()).collect();
        assert_eq!(roles, GRID_ORDER.to_vec());
        assert_eq!(cfg.layout.columns.len(), 3);
        assert_eq!(cfg.layout.columns[0].rows, vec!["orch", "inp"]);
    }

    #[test]
    fn executor_layout_is_70_30_plus_input() {
        let cfg = PatternConfig::executor(default_test_model());
        assert_eq!(cfg.pattern_slug(), "executor");
        assert_eq!(cfg.pane_count(), 3);
        assert_eq!(cfg.layout.columns[0].weight, 70);
        assert_eq!(cfg.layout.columns[1].weight, 30);
        assert_eq!(cfg.layout.columns[0].rows, vec!["executor", "inp"]);
    }

    #[test]
    fn simple_defaults_to_single_worker_column_at_n_le_5() {
        let cfg = PatternConfig::simple(default_test_model(), 4, default_test_model());
        assert_eq!(cfg.pattern_slug(), "simple");
        assert_eq!(cfg.pane_count(), 1 + 4 + 1);
        assert_eq!(cfg.layout.columns.len(), 2);
        assert_eq!(
            cfg.layout.columns[1].rows,
            vec!["worker-1", "worker-2", "worker-3", "worker-4"]
        );
    }

    #[test]
    fn simple_splits_into_two_columns_above_5_workers() {
        let cfg = PatternConfig::simple(default_test_model(), 8, default_test_model());
        assert_eq!(cfg.layout.columns.len(), 3);
        assert_eq!(
            cfg.layout.columns[1].rows.len() + cfg.layout.columns[2].rows.len(),
            8
        );
    }

    #[test]
    fn simple_clamps_worker_count_to_1_10() {
        let cfg = PatternConfig::simple(default_test_model(), 99, default_test_model());
        assert_eq!(cfg.pane_count(), 1 + 10 + 1);
    }

    #[test]
    fn fixbug_is_full_width_plus_input() {
        let cfg = PatternConfig::fixbug("ISSUE-1".to_string(), default_test_model());
        assert_eq!(cfg.pattern_slug(), "fixbug");
        assert_eq!(cfg.pane_count(), 2);
        assert_eq!(cfg.layout.columns.len(), 1);
        assert_eq!(cfg.layout.columns[0].weight, 100);
        assert_eq!(cfg.layout.columns[0].rows, vec!["fixer", "inp"]);
    }

    #[test]
    fn model_menus_have_expected_defaults() {
        let menu = executor_model_menu();
        assert_eq!(menu[0].label, "Sonnet 5 High");
        let orch_menu = orchestrator_model_menu();
        assert_eq!(orch_menu[0].label, "Fable 5 XHIGH");
        assert_eq!(advisor_model().label, "Fable 5 XHIGH");
    }

    #[test]
    fn find_model_matches_label_case_insensitively() {
        let menu = executor_model_menu();
        assert_eq!(
            find_model(&menu, "sonnet 5 high").unwrap().model_id,
            "claude-sonnet-5"
        );
        assert!(find_model(&menu, "gpt-5.5").is_some()); // matches by model_id (two entries)
        assert!(find_model(&menu, "nonexistent").is_none());
    }

    fn default_test_model() -> ModelChoice {
        executor_model_menu().into_iter().next().unwrap()
    }
}
