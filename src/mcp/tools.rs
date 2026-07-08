use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use tracing::{info, warn};

use crate::db::queries;
use crate::db::queries::DiscoveryCard;
use crate::delivery::inject;
use crate::types::{generate_id, Acknowledgement, PeerMessage, StatusReport};

const WORKER_ROLES: &[&str] = &["ai", "dat", "sec", "ops", "plt", "ui", "doc", "qa"];

/// Push statuses that get mirrored to the orchestrator pane as a notification.
const PUSH_STATUSES: &[&str] = &[
    "ack",
    "complete",
    "blocked",
    "failed",
    "degraded",
    "recovered",
];

// ── byte-exact injected templates (parity with the former TS workflows) ───────

/// `[orchestrator:dispatch]` prompt → target worker pane (formatDispatchPrompt).
fn format_dispatch_prompt(
    message_id: &str,
    task_id: &str,
    from_role: &str,
    summary: &str,
    cards: &[DiscoveryCard],
    files: &[String],
    next_action: Option<&str>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("[orchestrator:dispatch]".to_string());
    lines.push(format!("message_id={message_id}"));
    lines.push(format!("task_id={task_id}"));
    let from = if from_role.is_empty() {
        "orchestrator"
    } else {
        from_role
    };
    lines.push(format!("from={from}"));
    lines.push(String::new());
    lines.push(summary.to_string());

    if !cards.is_empty() {
        lines.push(String::new());
        lines.push("Active Blackboard Discovery Cards (Shared Insights):".to_string());
        for card in cards {
            lines.push(format!(
                "  - [{} by {}] {}",
                card.category.to_uppercase(),
                card.role,
                card.summary
            ));
            lines.push(format!("    Solution: {}", card.solution));
        }
    }
    if !files.is_empty() {
        lines.push(String::new());
        lines.push("Files:".to_string());
        for f in files {
            lines.push(format!("  - {f}"));
        }
    }
    if let Some(na) = next_action {
        lines.push(String::new());
        lines.push(format!("Next: {na}"));
    }
    if !from_role.is_empty() && from_role != "orchestrator" {
        lines.push(String::new());
        lines.push(format!("(from {from_role})"));
    }
    lines.push(String::new());
    lines.push("First acknowledge receipt:".to_string());
    lines.push(format!(
        "  signal --status ack --task {task_id} --summary \"received\""
    ));
    lines.push(String::new());
    lines.push("When done:".to_string());
    lines.push(format!(
        "  signal --status complete --task {task_id} --summary \"...\" --evidence \"...\""
    ));
    lines.push(String::new());
    lines.push("If blocked:".to_string());
    lines.push(format!(
        "  signal --status blocked --task {task_id} --summary \"...\" --blocker \"...\""
    ));
    lines.join("\n")
}

/// `[worker:message]` peer prompt → target worker pane (formatPeerPrompt).
fn format_peer_prompt(
    message_id: &str,
    from: &str,
    to: &str,
    task_id: Option<&str>,
    info: &str,
    requested_action: Option<&str>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("[worker:message]".to_string());
    lines.push(format!("message_id={message_id}"));
    lines.push(format!("from={from}"));
    lines.push(format!("to={to}"));
    if let Some(t) = task_id {
        lines.push(format!("task_id={t}"));
    }
    lines.push(String::new());
    lines.push(info.to_string());
    if let Some(ra) = requested_action {
        lines.push(String::new());
        lines.push(format!("Requested: {ra}"));
    }
    lines.join("\n")
}

/// Orchestrator-pane status-notification line (push-status transition).
fn format_status_line(
    role: &str,
    status: &str,
    task_id: Option<&str>,
    summary: Option<&str>,
    evidence: Option<&str>,
    next_action: Option<&str>,
) -> String {
    let mut parts: Vec<String> = vec![format!("[{role} {status}]")];
    if let Some(t) = task_id {
        parts.push(format!("task={t}"));
    }
    if let Some(s) = summary {
        parts.push(s.to_string());
    }
    if let Some(e) = evidence {
        parts.push(format!("evidence={e}"));
    }
    if let Some(n) = next_action {
        parts.push(format!("next={n}"));
    }
    format!("{}\n", parts.join(" "))
}

/// Orchestrator-pane peer line (peer message to == orchestrator). `→` = U+2192.
fn format_orchestrator_peer_line(
    from: &str,
    task_id: Option<&str>,
    info: &str,
    requested_action: Option<&str>,
) -> String {
    let mut parts: Vec<String> = vec![format!("[peer {from} \u{2192} orchestrator]")];
    if let Some(t) = task_id {
        parts.push(format!("task={t}"));
    }
    parts.push(info.to_string());
    if let Some(ra) = requested_action {
        parts.push(format!("action={ra}"));
    }
    format!("{}\n", parts.join(" "))
}

/// MCP event broadcast text → busy subscribed worker pane.
fn format_mcp_event(event_type: &str, publisher: &str, payload: &Value) -> String {
    format!(
        "\n\n>>> [MCP EVENT BROADCAST] <<<\nEvent: {event_type} from {publisher}\nPayload: {}\n\n",
        payload
    )
}

/// Best-effort direct injection into a role's pane. A missing pane is logged and
/// swallowed (parity with the TS `catch { /* degraded pane */ }` blocks): SQLite
/// is already the source of truth, so delivery failure must never be fatal.
async fn try_inject(pool: &SqlitePool, session: &str, role: &str, text: &str) {
    if let Err(e) = inject::deliver_to_role(pool, session, role, text).await {
        warn!(session = %session, role = %role, error = %e, "pane injection failed (degraded pane)");
    }
}

/// Resolve the SQLite `sessions.id` for a `name-N` session string, if present.
async fn resolve_session_id(pool: &SqlitePool, session: &str) -> Option<String> {
    let (project_slug, slot) = parse_session(session).ok()?;
    sqlx::query_as::<_, (String,)>(
        "SELECT id FROM sessions WHERE project_slug = ? AND slot_number = ?",
    )
    .bind(&project_slug)
    .bind(slot)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .map(|(id,)| id)
}

fn enforce_scope(args: &Value, source: &str) -> anyhow::Result<()> {
    let team_id = args
        .get("team_id")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("session").and_then(|v| v.as_str()));
    let repo_id = args.get("repo_id").and_then(|v| v.as_str());
    let temporal_namespace = args.get("temporal_namespace").and_then(|v| v.as_str());
    let task_queue = args.get("task_queue").and_then(|v| v.as_str());

    let active_session = std::env::var("DEVORCH_SESSION").ok();
    let active_run_id =
        std::env::var("DEVORCH_RUN_ID").unwrap_or_else(|_| "unknown-run".to_string());

    // Load config to check temporal namespace
    let config = crate::config::Config::load().ok();
    let active_namespace = config
        .as_ref()
        .map(|c| c.temporal_namespace.as_str())
        .unwrap_or("default");

    let team_id_str = team_id.unwrap_or("N/A");
    let queue_str = task_queue.unwrap_or("N/A");

    if team_id.is_none() {
        log_rejection(
            "N/A",
            &active_run_id,
            "N/A",
            source,
            "team_id (or session) is required",
        );
        anyhow::bail!("Rejection: team_id is required.");
    }
    if temporal_namespace.is_none() && task_queue.is_none() {
        log_rejection(
            team_id_str,
            &active_run_id,
            "N/A",
            source,
            "temporal_namespace or task_queue is required",
        );
        anyhow::bail!("Rejection: temporal_namespace or task_queue is required.");
    }
    if repo_id.is_none() {
        log_rejection(
            team_id_str,
            &active_run_id,
            queue_str,
            source,
            "repo_id is required",
        );
        anyhow::bail!("Rejection: repo_id is required.");
    }

    if let Some(ref active) = active_session {
        if team_id_str != active {
            log_rejection(
                team_id_str,
                &active_run_id,
                queue_str,
                source,
                &format!("Mismatched team_id: expected {}", active),
            );
            anyhow::bail!(
                "Rejection: team_id '{}' does not match active team/session '{}'",
                team_id_str,
                active
            );
        }
    }

    if let Some(ns) = temporal_namespace {
        if ns != active_namespace {
            log_rejection(
                team_id_str,
                &active_run_id,
                queue_str,
                source,
                &format!(
                    "Mismatched temporal_namespace: expected {}",
                    active_namespace
                ),
            );
            anyhow::bail!(
                "Rejection: temporal_namespace '{}' does not match active namespace '{}'",
                ns,
                active_namespace
            );
        }
    }

    info!(
        team_id = %team_id_str,
        repo_id = %repo_id.unwrap(),
        source = %source,
        "Verified scope for active team/session"
    );

    Ok(())
}

fn log_rejection(
    team_id: &str,
    run_id: &str,
    task_queue: &str,
    source: &str,
    rejection_reason: &str,
) {
    tracing::error!(
        event = "mcp_message_rejected",
        team_id = %team_id,
        workflow_id = %format!("session:{}:orchestrator", team_id),
        run_id = %run_id,
        task_queue = %task_queue,
        source = %source,
        rejection_reason = %rejection_reason,
        "MCP message rejected"
    );
}

/// Tool: devorch_report_status
///
/// Input: `{ session, role, status, task_id?, summary?, validation?, next_action?,
///           assignment_id?, generation?, team_id?, temporal_namespace?, task_queue?, repo_id? }`
pub async fn handle_report_status(pool: &SqlitePool, args: Value) -> anyhow::Result<Value> {
    enforce_scope(&args, "devorch_report_status")?;
    let report: StatusReport = serde_json::from_value(args)?;

    // 1. Look up agent by session + role
    let agents = queries::get_agents_for_session(pool, &report.session).await?;
    let agent = agents
        .into_iter()
        .find(|a| a.role == report.role)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "agent not found for session {} role {}",
                report.session,
                report.role
            )
        })?;

    // 2. Generation validation
    if let (Some(ref assignment_id), Some(generation)) = (&report.assignment_id, report.generation)
    {
        let row = sqlx::query(
            "SELECT generation FROM leases WHERE work_item_id = ? ORDER BY created_at DESC LIMIT 1",
        )
        .bind(assignment_id)
        .fetch_optional(pool)
        .await?;

        if let Some(r) = row {
            let db_gen: i64 = r.try_get("generation")?;
            if db_gen != generation {
                return Err(anyhow::anyhow!("stale generation, re-read active.md"));
            }
        }
    }

    // 3. Resolve work item
    let mut work_item_id: Option<String> = report.assignment_id.clone();

    if work_item_id.is_none() {
        if let Some(ref task_id) = report.task_id {
            let row = sqlx::query(
                "SELECT id FROM work_items WHERE task_id = ? AND target_agent_id = ? AND status IN ('leased','delivered','acked','in_progress')"
            )
            .bind(task_id)
            .bind(&agent.id)
            .fetch_optional(pool)
            .await?;

            if let Some(r) = row {
                work_item_id = Some(r.try_get("id")?);
            }
        }
    }

    // 4. Update work item + acknowledgement
    if let Some(ref wi_id) = work_item_id {
        // Map the report vocabulary (ack/complete/blocked/failed/degraded/
        // recovered) onto the work_items.status CHECK set. No bare passthrough —
        // ack/degraded/recovered are NOT valid work_items statuses and would
        // trip "CHECK constraint failed".
        let work_status = match report.status.as_str() {
            "complete" => "done_claimed",
            "failed" => "stale",
            "ack" => "acked",
            "blocked" => "blocked",
            "degraded" | "recovered" => "in_progress",
            _ => "in_progress",
        };

        queries::update_work_item_status(pool, wi_id, work_status).await?;

        let ack = Acknowledgement {
            id: generate_id("ack"),
            work_item_id: wi_id.clone(),
            agent_id: agent.id.clone(),
            ack_type: report.status.clone(),
            generation: report.generation.unwrap_or(1),
            received_at: Utc::now(),
        };
        queries::insert_acknowledgement(pool, &ack).await?;
    }

    // 5. Update agent status.
    // report.status is the workflow-facing transition vocabulary
    // (ack/complete/blocked/failed/degraded/recovered); agents.status has a
    // CHECK constraint on the operational set (idle/busy/degraded/dead/paused/
    // recovering/failed). Project the report onto a valid agent status.
    let agent_status = match report.status.as_str() {
        "ack" => "busy",
        "complete" => "idle",
        "blocked" => "degraded",
        "failed" => "failed",
        "degraded" => "degraded",
        "recovered" => "busy",
        _ => "busy",
    };
    queries::update_agent_status(pool, &agent.id, agent_status).await?;

    // 5b. Advance the nudge clock and busy/active-task tracking. A status report
    // counts as a signal (silence-clock reset). complete/failed clear the active
    // task; everything else keeps the agent busy on its task.
    match report.status.as_str() {
        "complete" | "failed" => {
            let _ = queries::clear_agent_active_task(pool, &agent.id).await;
            let _ = queries::mark_agent_signal(pool, &agent.id, None, false).await;
        }
        _ => {
            let _ =
                queries::mark_agent_signal(pool, &agent.id, report.task_id.as_deref(), true).await;
        }
    }

    // 6. Durable SQLite inbox write (single source of truth) — replaces the former
    // orch.stateTransition Temporal signal.
    let now = Utc::now().to_rfc3339();
    if let Some(session_id) = resolve_session_id(pool, &report.session).await {
        let message_id = generate_id("trans");
        if let Err(e) = sqlx::query(
            "INSERT INTO inbox_messages \
             (message_id, session_id, task_id, role, status, summary, evidence, next_action, received_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&message_id)
        .bind(&session_id)
        .bind(&report.task_id)
        .bind(&report.role)
        .bind(&report.status)
        .bind(&report.summary)
        .bind(&report.validation)
        .bind(&report.next_action)
        .bind(&now)
        .execute(pool)
        .await {
            warn!(error = %e, message_id = %message_id, "Failed to persist to inbox_messages");
        }
    }

    // 6b. On a push status, inject the orchestrator-pane notification line.
    if PUSH_STATUSES.contains(&report.status.as_str()) {
        let line = format_status_line(
            &report.role,
            &report.status,
            report.task_id.as_deref(),
            report.summary.as_deref(),
            report.validation.as_deref(),
            report.next_action.as_deref(),
        );
        try_inject(pool, &report.session, "orchestrator", &line).await;
    }

    // 6c. Auto-heal: re-decompose on TypeScript-shaped blockers, resume parents
    // when an auto-fix completes, and post discovery cards on terminal statuses.
    handle_auto_heal(pool, &report).await;

    // 7. Log event
    let payload = serde_json::to_string(&report).ok();
    queries::log_event(
        pool,
        &report.session,
        Some(&agent.id),
        "status_report",
        payload.as_deref(),
    )
    .await?;

    Ok(json!({
        "status": "ok",
        "agent_id": agent.id,
        "work_item_updated": work_item_id.is_some()
    }))
}

/// Persist a dispatch row + inject the dispatch prompt to the target pane. Shared
/// by `devorch_dispatch_task` and the auto-heal re-decompose/resume paths so all
/// dispatches are durable and rendered identically (with blackboard cards).
struct DispatchEnvelope<'a> {
    session: &'a str,
    from_role: &'a str,
    to_role: &'a str,
    task_id: &'a str,
    summary: &'a str,
    files: &'a [String],
    next_action: Option<&'a str>,
    priority: &'a str,
}

async fn dispatch_and_inject(
    pool: &SqlitePool,
    dispatch: DispatchEnvelope<'_>,
) -> anyhow::Result<String> {
    let DispatchEnvelope {
        session,
        from_role,
        to_role,
        task_id,
        summary,
        files,
        next_action,
        priority,
    } = dispatch;
    let message_id = format!(
        "dispatch-{}-{}",
        uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("x"),
        Utc::now().timestamp_millis()
    );
    let created_at = Utc::now().to_rfc3339();
    let files_json = serde_json::to_string(files)?;

    sqlx::query(
        "INSERT INTO dispatches \
         (message_id, session_id, task_id, from_role, to_role, summary, files, next_action, priority, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&message_id)
    .bind(session)
    .bind(task_id)
    .bind(from_role)
    .bind(to_role)
    .bind(summary)
    .bind(&files_json)
    .bind(next_action)
    .bind(priority)
    .bind(&created_at)
    .execute(pool)
    .await?;

    // Load shared blackboard cards for the session and render the prompt.
    let cards = queries::list_discovery_cards(pool, session)
        .await
        .unwrap_or_default();
    let prompt = format_dispatch_prompt(
        &message_id,
        task_id,
        from_role,
        summary,
        &cards,
        files,
        next_action,
    );

    // Mark the target agent busy on this task (advances the nudge clock).
    if let Ok(agents) = queries::get_agents_for_session(pool, session).await {
        if let Some(agent) = agents.into_iter().find(|a| a.role == to_role) {
            let _ = queries::mark_agent_signal(pool, &agent.id, Some(task_id), true).await;
            let _ = queries::update_agent_status(pool, &agent.id, "busy").await;
        }
    }

    try_inject(pool, session, to_role, &prompt).await;
    Ok(message_id)
}

/// Auto-heal driven entirely off the just-applied status report (native parity
/// with the former orchestrator/execution-window workflows):
///   - blocked + TS-shaped blocker → re-decompose into an `auto-fix-…` task to `ai`
///   - complete on an `auto-fix-…` task → resume the (still blocked) parent
///   - complete/blocked/failed → post a discovery card to the shared blackboard
async fn handle_auto_heal(pool: &SqlitePool, report: &StatusReport) {
    let session = &report.session;

    match report.status.as_str() {
        "blocked" => {
            if let Some(task_id) = report.task_id.as_deref() {
                let blocker_text = report
                    .validation
                    .as_deref()
                    .or(report.summary.as_deref())
                    .unwrap_or("")
                    .to_lowercase();
                if blocker_text.contains("typescript")
                    || blocker_text.contains("typecheck")
                    || blocker_text.contains("exactoptional")
                {
                    let sub_task =
                        format!("auto-fix-{}-{}", task_id, Utc::now().timestamp_millis());
                    let evidence = report
                        .validation
                        .as_deref()
                        .or(report.summary.as_deref())
                        .unwrap_or("");
                    let summary = format!(
                        "[AUTOMATED TROUBLESHOOTING] Role \"{}\" is BLOCKED on task \"{}\" due to TypeScript/compilation errors:\n\n{}\n\nPlease inspect the workspace, fix any type/compilation errors, ensure package typecheck passes, and signal complete.",
                        report.role, task_id, evidence
                    );
                    if let Err(e) = dispatch_and_inject(
                        pool,
                        DispatchEnvelope {
                            session,
                            from_role: "orchestrator",
                            to_role: "ai",
                            task_id: &sub_task,
                            summary: &summary,
                            files: &[],
                            next_action: None,
                            priority: "high",
                        },
                    )
                    .await
                    {
                        warn!(error = %e, "auto re-decompose dispatch failed");
                    }
                }
            }
        }
        "complete" => {
            if let Some(task_id) = report.task_id.as_deref() {
                if let Some(stripped) = task_id.strip_prefix("auto-fix-") {
                    // parentTaskId = task_id.split('-')[2..len-1].join('-'): drop the
                    // `auto-fix-` prefix and the trailing `-{millis}` component.
                    let parent = match stripped.rsplit_once('-') {
                        Some((parent, _millis)) if !parent.is_empty() => parent.to_string(),
                        _ => String::new(),
                    };
                    if !parent.is_empty() {
                        // Only resume if the parent is currently blocked.
                        let blocked_role: Option<String> = sqlx::query_scalar(
                            "SELECT target_role FROM work_items \
                             WHERE task_id = ? AND session_id = ? AND status = 'blocked' \
                             ORDER BY created_at DESC LIMIT 1",
                        )
                        .bind(&parent)
                        .bind(session)
                        .fetch_optional(pool)
                        .await
                        .ok()
                        .flatten();

                        if let Some(parent_role) = blocked_role {
                            // Clear the parent's blocked work item(s).
                            let _ = sqlx::query(
                                "UPDATE work_items SET status = 'in_progress' \
                                 WHERE task_id = ? AND session_id = ? AND status = 'blocked'",
                            )
                            .bind(&parent)
                            .bind(session)
                            .execute(pool)
                            .await;

                            let resume = "[RESUMED TASK] The blocker has been automatically resolved. Please rerun validation and proceed to completion.";
                            if let Err(e) = dispatch_and_inject(
                                pool,
                                DispatchEnvelope {
                                    session,
                                    from_role: "orchestrator",
                                    to_role: &parent_role,
                                    task_id: &parent,
                                    summary: resume,
                                    files: &[],
                                    next_action: None,
                                    priority: "high",
                                },
                            )
                            .await
                            {
                                warn!(error = %e, "auto-resume dispatch failed");
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }

    // Post a discovery card on terminal statuses (parity with emitTransition).
    if matches!(report.status.as_str(), "complete" | "blocked" | "failed") {
        let card = DiscoveryCard {
            card_id: format!(
                "card-{}-{}-{}",
                report.task_id.as_deref().unwrap_or("task"),
                report.role,
                Utc::now().timestamp_millis()
            ),
            role: report.role.clone(),
            category: if report.status == "blocked" {
                "typescript".to_string()
            } else {
                "environment".to_string()
            },
            summary: format!(
                "{}: {}",
                report.status.to_uppercase(),
                report.summary.as_deref().unwrap_or("")
            ),
            solution: report
                .validation
                .as_deref()
                .unwrap_or("No details provided.")
                .to_string(),
        };
        if let Err(e) = queries::insert_discovery_card(pool, session, &card).await {
            warn!(error = %e, "failed to post discovery card");
        }
    }
}

/// Tool: devorch_peer_message
///
/// Input: `{ session, from_role, to_role, task_id?, info, requested_action?, team_id?, temporal_namespace?, task_queue?, repo_id? }`
pub async fn handle_peer_message(pool: &SqlitePool, args: Value) -> anyhow::Result<Value> {
    enforce_scope(&args, "devorch_peer_message")?;
    let msg: PeerMessage = serde_json::from_value(args)?;

    // Find target agent id for richer logging
    let agents = queries::get_agents_for_session(pool, &msg.session).await?;
    let target_agent_id = agents
        .iter()
        .find(|a| a.role == msg.to_role)
        .map(|a| a.id.clone());

    // Queue / log the message
    let payload = serde_json::to_string(&msg).ok();
    queries::log_event(
        pool,
        &msg.session,
        target_agent_id.as_deref(),
        "peer_message",
        payload.as_deref(),
    )
    .await?;

    if !agents.iter().any(|a| a.role == msg.to_role) {
        anyhow::bail!("no agent for session {} role {}", msg.session, msg.to_role);
    }

    // SQLite is the source of truth; delivery is a direct iTerm2 injection.
    let message_id = generate_id("peer");
    let now = Utc::now().to_rfc3339();

    if msg.to_role == "orchestrator" {
        // The orchestrator reads its inbox AND gets a live pane notification line.
        if let Some(session_id) = resolve_session_id(pool, &msg.session).await {
            let summary = format!("peer {} \u{2192} orchestrator: {}", msg.from_role, msg.info);
            let _ = sqlx::query(
                "INSERT INTO inbox_messages \
                 (message_id, session_id, task_id, role, status, summary, evidence, next_action, received_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&message_id)
            .bind(&session_id)
            .bind(&msg.task_id)
            .bind(&msg.from_role) // sender is the role reporting
            .bind("info")
            .bind(&summary)
            .bind::<Option<&str>>(None)
            .bind(&msg.requested_action)
            .bind(&now)
            .execute(pool)
            .await;
        }
        let line = format_orchestrator_peer_line(
            &msg.from_role,
            msg.task_id.as_deref(),
            &msg.info,
            msg.requested_action.as_deref(),
        );
        try_inject(pool, &msg.session, "orchestrator", &line).await;
    } else {
        // Worker peer: inject the [worker:message] prompt and advance its clock.
        let prompt = format_peer_prompt(
            &message_id,
            &msg.from_role,
            &msg.to_role,
            msg.task_id.as_deref(),
            &msg.info,
            msg.requested_action.as_deref(),
        );
        if let Some(agent_id) = target_agent_id.as_deref() {
            let _ = queries::mark_agent_signal(pool, agent_id, None, true).await;
        }
        try_inject(pool, &msg.session, &msg.to_role, &prompt).await;
    }

    Ok(ok_text(format!(
        "Peer message {message_id} routed from {} to {}.",
        msg.from_role, msg.to_role
    )))
}

/// Tool: devorch_query_team_state
///
/// Input: `{ session, team_id?, temporal_namespace?, task_queue?, repo_id? }`
pub async fn handle_query_team_state(pool: &SqlitePool, args: Value) -> anyhow::Result<Value> {
    enforce_scope(&args, "devorch_query_team_state")?;
    #[derive(Deserialize)]
    struct Args {
        session: String,
    }
    let parsed: Args = serde_json::from_value(args)?;

    let agents = queries::get_agents_for_session(pool, &parsed.session).await?;

    let rows = sqlx::query(
        "SELECT task_id, target_role, status, summary
         FROM work_items
         WHERE session_id = ? AND status IN ('leased', 'delivered', 'acked', 'in_progress', 'blocked')"
    )
    .bind(&parsed.session)
    .fetch_all(pool)
    .await?;

    let mut lines = vec![
        "=== TEAM STATE ===".to_string(),
        format!("Session: {}", parsed.session),
        "".to_string(),
        "AGENTS:".to_string(),
        format!(
            "{:<12} {:<12} {:<12} {:<20}",
            "ROLE", "KIND", "STATUS", "LAST_SEEN"
        ),
    ];

    for agent in &agents {
        let last_seen = agent
            .last_seen_at
            .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "-".to_string());
        lines.push(format!(
            "{:<12} {:<12} {:<12} {:<20}",
            agent.role, agent.agent_kind, agent.status, last_seen
        ));
    }

    lines.push("".to_string());
    lines.push("ACTIVE WORK:".to_string());
    lines.push(format!(
        "{:<12} {:<12} {:<12} {:<30}",
        "TASK_ID", "ROLE", "STATUS", "SUMMARY"
    ));

    for row in &rows {
        let task_id: String = row.try_get("task_id")?;
        let target_role: String = row.try_get("target_role")?;
        let status: String = row.try_get("status")?;
        let summary: String = row.try_get("summary")?;
        // Truncate by char, not byte: free-text summaries may contain multi-byte
        // UTF-8 (emoji, accents). Slicing `&summary[..30]` panics when byte 30 is
        // not a char boundary.
        let summary_trunc: String = summary.chars().take(30).collect();
        lines.push(format!(
            "{:<12} {:<12} {:<12} {:<30}",
            task_id, target_role, status, summary_trunc
        ));
    }

    Ok(json!({
        "status": "ok",
        "report": lines.join("\n")
    }))
}

/// Tool: devorch_get_setup_instructions
///
/// Input: `{ session, role, agent, team_id?, temporal_namespace?, task_queue?, repo_id? }`
pub async fn handle_get_setup_instructions(
    _pool: &SqlitePool,
    args: Value,
) -> anyhow::Result<Value> {
    enforce_scope(&args, "devorch_get_setup_instructions")?;
    #[derive(Deserialize)]
    struct Args {
        session: String,
        role: String,
        agent: String,
    }
    let parsed: Args = serde_json::from_value(args)?;

    // Pattern-aware instructions: non-team `LaunchPattern`s (see
    // `startwork::patterns::LaunchPattern`) get their own prompt content from
    // `crate::prompts`, keyed off `DEVORCH_PATTERN` (exported to every pane by
    // `startwork::launch`). Team stays on the inline prompts below, unchanged.
    if let Some("simple") = std::env::var("DEVORCH_PATTERN").ok().as_deref() {
        return Ok(json!({
            "status": "ok",
            "instructions": crate::prompts::simple::instructions(
                &parsed.role,
                &parsed.agent,
                &parsed.session,
            )
        }));
    }

    let instructions = if parsed.role == "orchestrator" {
        format!(
            "You are the orchestrator for session {} (agent: {}).\n\
             You lead a LIVE TEAM of specialist workers, each in its own pane with a domain skill:\n\
               ai=AI/agents, dat=data/DB, sec=security, ops=infra/CI/CD, plt=platform/backend, ui=frontend, doc=docs/GitHub, qa=testing.\n\
             DELEGATE — DO NOT DO THE WORK YOURSELF: You never write code, edit files, or run the analysis yourself, and you do NOT use any built-in Task/subagent/agent tool. Decompose the goal and assign each piece to the right specialist via `devorch_dispatch_task` (to_role=<role>). The specialists do the work; you route, sequence dependencies, and integrate results.\n\
             CRITICAL COMMUNICATION RULES:\n\
             1. DO NOT POLL OR RESEARCH: Never call `devorch_orchestrator_inbox` or `devorch_query_team_state`. Do not run bash commands like `find` or `ls` to check team state. Responses are pushed directly into your terminal.\n\
             2. WAIT IDLE: After sending pings or assigning a task, you MUST wait completely idle. Stop calling tools entirely and do not run any fallback tools.\n\
             3. PINGING: To ping a worker, you MUST use `devorch_ping`. You may ping multiple workers in parallel to kick them in the butt and demand a brief progress update.\n\
             4. ASSIGNING: Use `devorch_dispatch_task` to assign work to the right specialist role.\n\
             5. ZERO CHAT / CONCISENESS: Be extremely silent, concise, and professional. Never engage in conversational chit-chat, explain your thought process to the user, or print greetings/pleasantries. Only output necessary commands and minimal structured status updates. Reign in all verbal chatter.",
            parsed.session, parsed.agent
        )
    } else {
        format!(
            "You are the {} specialist worker (agent: {}) for session {}.\n\
             Use your {} domain skill and expertise for everything assigned to you — you are the expert for this area.\n\
             CRITICAL COMMUNICATION RULES:\n\
             1. WAIT IDLE: Do not search for tasks or poll. Wait patiently for the Orchestrator to assign you tasks or ping you via terminal push notifications.\n\
             2. RESPONDING TO PINGS: When you receive a status ping, it is a direct kick in the butt to pick up the pace and provide a brief status update. You MUST immediately acknowledge it by calling `devorch_ack` and providing a meaningful summary of your progress (e.g. what you are active on, key hurdles, next action). Never just say 'pong' or 'Task acknowledged'.\n\
             3. DELIVERING: When you finish an assigned task, call `devorch_report_status` with status `complete` and a short validation note so the Orchestrator can integrate and reassign.\n\
             4. BLOCKERS: If you encounter an issue, call `devorch_blocker` and explain the issue.\n\
             5. ZERO CHAT / CONCISENESS: Be extremely silent, concise, and professional. Never engage in conversational chit-chat, explain your thought process to the user, or print greetings/pleasantries. Only output necessary commands, signals, and minimal structured status updates. Reign in all verbal chatter.",
            parsed.role, parsed.agent, parsed.session, parsed.role
        )
    };

    Ok(json!({
        "status": "ok",
        "instructions": instructions
    }))
}

// ── helpers for the new tools ─────────────────────────────────────────────────

fn ok_text(text: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": text.into() }] })
}

fn err_text(text: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": text.into() }], "isError": true })
}

fn require_str<'a>(params: &'a Value, key: &str) -> Result<&'a str, Value> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| err_text(format!("missing required field: {key}")))
}

fn parse_session(session: &str) -> anyhow::Result<(String, i64)> {
    let last_dash = session.rfind('-').ok_or_else(|| {
        anyhow::anyhow!("invalid session format: expected 'name-N' (e.g. navi-9)")
    })?;
    let slug = &session[..last_dash];
    let slot: i64 = session[last_dash + 1..]
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid session format: slot must be a number"))?;
    Ok((slug.to_string(), slot))
}

// ── tool 5: devorch_dispatch_task ─────────────────────────────────────────────

pub async fn handle_dispatch_task(pool: &SqlitePool, args: Value) -> anyhow::Result<Value> {
    // Lightweight session scope guard: if DEVORCH_SESSION is set, session must match.
    if let Some(session) = args.get("session").and_then(Value::as_str) {
        if let Ok(active) = std::env::var("DEVORCH_SESSION") {
            if !active.is_empty() && session != active {
                let run_id =
                    std::env::var("DEVORCH_RUN_ID").unwrap_or_else(|_| "unknown-run".to_string());
                log_rejection(
                    session,
                    &run_id,
                    "N/A",
                    "devorch_dispatch_task",
                    &format!(
                        "session '{}' does not match active DEVORCH_SESSION '{}'",
                        session, active
                    ),
                );
                return Ok(err_text(format!(
                    "Rejection: session '{}' does not match active DEVORCH_SESSION '{}'",
                    session, active
                )));
            }
        }
    }
    match dispatch_task_inner(pool, &args).await {
        Ok(v) => Ok(v),
        Err(e) => Ok(err_text(format!("Failed to dispatch: {e}"))),
    }
}

async fn dispatch_task_inner(pool: &SqlitePool, params: &Value) -> anyhow::Result<Value> {
    let session = require_str(params, "session").map_err(|e| anyhow::anyhow!("{e}"))?;
    let from_role = require_str(params, "from_role").map_err(|e| anyhow::anyhow!("{e}"))?;
    let to_role = require_str(params, "to_role").map_err(|e| anyhow::anyhow!("{e}"))?;
    let task_id = require_str(params, "task_id").map_err(|e| anyhow::anyhow!("{e}"))?;
    let summary = require_str(params, "summary").map_err(|e| anyhow::anyhow!("{e}"))?;

    let next_action = params.get("next_action").and_then(Value::as_str);
    let priority = params
        .get("priority")
        .and_then(Value::as_str)
        .unwrap_or("normal");
    let files: Vec<String> = params
        .get("files")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    if !WORKER_ROLES.contains(&to_role) {
        anyhow::bail!("to_role must be one of: {}", WORKER_ROLES.join(", "));
    }

    // Create a leasable work_item so devorch_report_status can resolve the task
    // later. Best-effort: a missing session row (tests) leaves work_item absent,
    // which the report path tolerates.
    if let Ok(agents) = queries::get_agents_for_session(pool, session).await {
        if let Some(agent) = agents.into_iter().find(|a| a.role == to_role) {
            let files_json = serde_json::to_string(&files)?;
            let wi_id = generate_id("wi");
            let _ = sqlx::query(
                "INSERT INTO work_items \
                 (id, session_id, target_role, target_agent_id, task_id, summary, files, next_action, priority, status, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'delivered', ?)"
            )
            .bind(&wi_id)
            .bind(session)
            .bind(to_role)
            .bind(&agent.id)
            .bind(task_id)
            .bind(summary)
            .bind(&files_json)
            .bind(next_action)
            .bind(priority)
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await;
        }
    }

    // SQLite-first persist of the dispatch row, then DIRECT pane injection of the
    // [orchestrator:dispatch] prompt (with shared blackboard cards) — no Temporal.
    let message_id = dispatch_and_inject(
        pool,
        DispatchEnvelope {
            session,
            from_role,
            to_role,
            task_id,
            summary,
            files: &files,
            next_action,
            priority,
        },
    )
    .await?;

    Ok(ok_text(format!(
        "Dispatched task {task_id} to {to_role}. Message ID: {message_id}"
    )))
}

// ── tool 6: devorch_orchestrator_inbox ───────────────────────────────────────

pub async fn handle_orchestrator_inbox(pool: &SqlitePool, args: Value) -> anyhow::Result<Value> {
    // Lightweight session scope guard: if DEVORCH_SESSION is set, session must match.
    if let Some(session) = args.get("session").and_then(Value::as_str) {
        if let Ok(active) = std::env::var("DEVORCH_SESSION") {
            if !active.is_empty() && session != active {
                let run_id =
                    std::env::var("DEVORCH_RUN_ID").unwrap_or_else(|_| "unknown-run".to_string());
                log_rejection(
                    session,
                    &run_id,
                    "N/A",
                    "devorch_orchestrator_inbox",
                    &format!(
                        "session '{}' does not match active DEVORCH_SESSION '{}'",
                        session, active
                    ),
                );
                return Ok(err_text(format!(
                    "Rejection: session '{}' does not match active DEVORCH_SESSION '{}'",
                    session, active
                )));
            }
        }
    }
    match orchestrator_inbox_inner(pool, &args).await {
        Ok(v) => Ok(v),
        Err(e) => Ok(err_text(format!("Failed to fetch orchestrator inbox: {e}"))),
    }
}

async fn orchestrator_inbox_inner(pool: &SqlitePool, params: &Value) -> anyhow::Result<Value> {
    let session = require_str(params, "session").map_err(|e| anyhow::anyhow!("{e}"))?;

    let clear_ids: Vec<String> = params
        .get("clear_message_ids")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    // 1. Clear messages if requested — SQLite only (no Temporal hop).
    if !clear_ids.is_empty() {
        for id in &clear_ids {
            let _ = sqlx::query("UPDATE inbox_messages SET cleared = 1 WHERE message_id = ?")
                .bind(id)
                .execute(pool)
                .await;
        }
    }

    // 2. Make SQLite the single source of truth for the inbox.
    // The Temporal orchestratorInbox query was removed to avoid non-determinism
    // and loss of messages like 'ack'.

    // 3. SQLite fallback: uncleared inbox_messages for this session.
    let (project_slug, slot) = parse_session(session)?;
    let session_row: Option<(String,)> =
        sqlx::query_as("SELECT id FROM sessions WHERE project_slug = ? AND slot_number = ?")
            .bind(&project_slug)
            .bind(slot)
            .fetch_optional(pool)
            .await?;

    let session_id = match session_row {
        Some((id,)) => id,
        None => {
            return Ok(ok_text("[]"));
        }
    };

    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        String,
        Option<String>,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    )> = sqlx::query_as(
        "SELECT message_id, task_id, role, status, summary, evidence, next_action, received_at \
             FROM inbox_messages \
             WHERE session_id = ? AND cleared = 0 \
             ORDER BY received_at ASC",
    )
    .bind(&session_id)
    .fetch_all(pool)
    .await?;

    let inbox: Vec<Value> = rows
        .into_iter()
        .map(
            |(message_id, task_id, role, status, summary, evidence, next_action, timestamp)| {
                json!({
                    "message_id": message_id,
                    "task_id": task_id,
                    "role": role,
                    "status": status,
                    "summary": summary,
                    "evidence": evidence,
                    "next_action": next_action,
                    "timestamp": timestamp,
                })
            },
        )
        .collect();

    Ok(ok_text(
        serde_json::to_string_pretty(&inbox).unwrap_or_else(|_| "[]".to_string()),
    ))
}

pub async fn handle_ping(pool: &SqlitePool, args: Value) -> anyhow::Result<Value> {
    enforce_scope(&args, "devorch_ping")?;
    let mut mapped_args = args.clone();
    mapped_args["info"] = json!("ping");
    mapped_args["requested_action"] = json!("status_update");
    handle_peer_message(pool, mapped_args).await
}

pub async fn handle_ack(pool: &SqlitePool, args: Value) -> anyhow::Result<Value> {
    enforce_scope(&args, "devorch_ack")?;
    let mut mapped_args = args.clone();
    mapped_args["status"] = json!("ack");

    let summary = mapped_args
        .get("summary")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .unwrap_or("");

    if summary.is_empty()
        || summary.eq_ignore_ascii_case("pong")
        || summary.eq_ignore_ascii_case("Task acknowledged")
    {
        // Look up the active work item for this agent/role to provide a meaningful status update
        let role = mapped_args
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let session = mapped_args
            .get("session")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let mut custom_summary = "Awaiting task assignment. Ready for next sprint.".to_string();

        if !role.is_empty() && !session.is_empty() {
            if let Ok(agents) = queries::get_agents_for_session(pool, session).await {
                if let Some(agent) = agents.into_iter().find(|a| a.role == role) {
                    let active_work = sqlx::query(
                        "SELECT task_id, summary FROM work_items WHERE target_agent_id = ? AND status IN ('leased','delivered','acked','in_progress','blocked') ORDER BY created_at DESC LIMIT 1"
                    )
                    .bind(&agent.id)
                    .fetch_optional(pool)
                    .await;

                    if let Ok(Some(row)) = active_work {
                        let task_id: String = row.try_get("task_id").unwrap_or_default();
                        let task_summary: String = row.try_get("summary").unwrap_or_default();
                        custom_summary = format!(
                            "Acknowledged status ping. Active on task #{} ({})",
                            task_id, task_summary
                        );
                    }
                }
            }
        }

        mapped_args["summary"] = json!(custom_summary);
    }

    handle_report_status(pool, mapped_args).await
}

pub async fn handle_blocker(pool: &SqlitePool, args: Value) -> anyhow::Result<Value> {
    enforce_scope(&args, "devorch_blocker")?;
    let mut mapped_args = args.clone();
    mapped_args["status"] = json!("blocked");
    if mapped_args.get("summary").is_none() {
        mapped_args["summary"] = json!("Encountered a blocker");
    }
    handle_report_status(pool, mapped_args).await
}

/// Tool: devorch_event_subscribe — register `role`'s interest in `event_type`.
///
/// Input: `{ session, role, event_type }`
pub async fn handle_event_subscribe(pool: &SqlitePool, args: Value) -> anyhow::Result<Value> {
    let session = require_str(&args, "session").map_err(|e| anyhow::anyhow!("{e}"))?;
    let role = require_str(&args, "role").map_err(|e| anyhow::anyhow!("{e}"))?;
    let event_type = require_str(&args, "event_type").map_err(|e| anyhow::anyhow!("{e}"))?;

    queries::add_subscription(pool, session, event_type, role).await?;
    Ok(ok_text(format!(
        "Subscribed {role} to event '{event_type}' for {session}."
    )))
}

/// Tool: devorch_event_publish — fan out an MCP event to subscribed busy panes.
///
/// Input: `{ session, event_type, publisher, payload? }`
pub async fn handle_event_publish(pool: &SqlitePool, args: Value) -> anyhow::Result<Value> {
    let session = require_str(&args, "session").map_err(|e| anyhow::anyhow!("{e}"))?;
    let event_type = require_str(&args, "event_type").map_err(|e| anyhow::anyhow!("{e}"))?;
    let publisher = require_str(&args, "publisher").map_err(|e| anyhow::anyhow!("{e}"))?;
    let payload = args.get("payload").cloned().unwrap_or(Value::Null);

    let fanout = publish_event(pool, session, event_type, publisher, &payload).await?;
    Ok(ok_text(format!(
        "Published '{event_type}' from {publisher}; injected to {fanout} subscribed busy pane(s)."
    )))
}

/// Fan out an MCP event to every subscribed role whose pane is busy. Returns the
/// number of panes injected. Reusable by an internal publish path too.
pub async fn publish_event(
    pool: &SqlitePool,
    session: &str,
    event_type: &str,
    publisher: &str,
    payload: &Value,
) -> anyhow::Result<usize> {
    let roles = queries::list_subscribed_roles(pool, session, event_type).await?;
    if roles.is_empty() {
        return Ok(0);
    }
    let agents = queries::get_agents_for_session(pool, session).await?;
    let text = format_mcp_event(event_type, publisher, payload);

    let mut count = 0;
    for role in roles {
        // Only deliver to busy subscribed panes (parity with the TS broadcast,
        // which targeted active execution windows).
        let is_busy = agents
            .iter()
            .find(|a| a.role == role)
            .map(|a| a.status == "busy")
            .unwrap_or(false);
        if is_busy {
            try_inject(pool, session, &role, &text).await;
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // All tests that read or write DEVORCH_SESSION / DEVORCH_RUN_ID must hold this
    // lock for their entire duration. Env vars are process-global; without
    // serialisation concurrent tests race on reads inside the scope guard. The
    // lock is crate-wide (shared with mcp::server::tests) so env-reading tests in
    // other modules serialise against the env-mutating tests here.
    use crate::db::test_helpers::ENV_LOCK;

    #[tokio::test]
    async fn test_scope_isolation_mismatched_team() {
        let _env = ENV_LOCK.lock().await;
        std::env::set_var("DEVORCH_SESSION", "m7-lantern-1");
        std::env::set_var("DEVORCH_RUN_ID", "m7-lantern-1-20260523");

        let payload = json!({
            "team_id": "m7-navi-35",
            "temporal_namespace": "default",
            "repo_id": "m7-navi"
        });

        let res = enforce_scope(&payload, "test_source");
        std::env::remove_var("DEVORCH_SESSION");
        std::env::remove_var("DEVORCH_RUN_ID");
        assert!(res.is_err(), "Should reject mismatched team");
        let err = res.unwrap_err().to_string();
        assert!(
            err.contains("does not match active team/session"),
            "Expected mismatched team error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_scope_isolation_matching_team() {
        let _env = ENV_LOCK.lock().await;
        std::env::set_var("DEVORCH_SESSION", "m7-lantern-1");
        std::env::set_var("DEVORCH_RUN_ID", "m7-lantern-1-20260523");

        let payload = json!({
            "team_id": "m7-lantern-1",
            "temporal_namespace": "default",
            "repo_id": "m7-lantern"
        });

        let res = enforce_scope(&payload, "test_source");
        std::env::remove_var("DEVORCH_SESSION");
        std::env::remove_var("DEVORCH_RUN_ID");
        assert!(
            res.is_ok(),
            "Should accept matching team and correct parameters: {:?}",
            res
        );
    }

    #[tokio::test]
    async fn test_scope_isolation_missing_team_id() {
        let _env = ENV_LOCK.lock().await;
        std::env::set_var("DEVORCH_SESSION", "m7-lantern-1");

        let payload = json!({
            "temporal_namespace": "default",
            "repo_id": "m7-lantern"
        });

        let res = enforce_scope(&payload, "test_source");
        std::env::remove_var("DEVORCH_SESSION");
        assert!(res.is_err(), "Should reject missing team_id");
        let err = res.unwrap_err().to_string();
        assert!(
            err.contains("team_id is required"),
            "Expected missing team_id error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_scope_isolation_missing_namespace_and_queue() {
        let _env = ENV_LOCK.lock().await;
        std::env::set_var("DEVORCH_SESSION", "m7-lantern-1");

        let payload = json!({
            "team_id": "m7-lantern-1",
            "repo_id": "m7-lantern"
        });

        let res = enforce_scope(&payload, "test_source");
        std::env::remove_var("DEVORCH_SESSION");
        assert!(res.is_err(), "Should reject missing namespace/queue");
        let err = res.unwrap_err().to_string();
        assert!(
            err.contains("temporal_namespace or task_queue is required"),
            "Expected missing namespace error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_scope_isolation_missing_repo_id() {
        let _env = ENV_LOCK.lock().await;
        std::env::set_var("DEVORCH_SESSION", "m7-lantern-1");

        let payload = json!({
            "team_id": "m7-lantern-1",
            "temporal_namespace": "default"
        });

        let res = enforce_scope(&payload, "test_source");
        std::env::remove_var("DEVORCH_SESSION");
        assert!(res.is_err(), "Should reject missing repo_id");
        let err = res.unwrap_err().to_string();
        assert!(
            err.contains("repo_id is required"),
            "Expected missing repo_id error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_scope_isolation_mismatched_namespace() {
        let _env = ENV_LOCK.lock().await;
        std::env::set_var("DEVORCH_SESSION", "m7-lantern-1");

        let payload = json!({
            "team_id": "m7-lantern-1",
            "temporal_namespace": "mismatched-namespace",
            "repo_id": "m7-lantern"
        });

        let res = enforce_scope(&payload, "test_source");
        std::env::remove_var("DEVORCH_SESSION");
        assert!(res.is_err(), "Should reject mismatched namespace");
        let err = res.unwrap_err().to_string();
        assert!(
            err.contains("does not match active namespace"),
            "Expected mismatched namespace error, got: {}",
            err
        );
    }

    // ── dispatch_task tests ────────────────────────────────────────────────────

    async fn seed_active_session(pool: &SqlitePool, session_id: &str, slug: &str, slot: i64) {
        sqlx::query(
            "INSERT OR IGNORE INTO machines (id, created_at) VALUES ('m1', datetime('now'))",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions (id, machine_id, project_slug, slot_number, status, created_at) \
             VALUES (?, 'm1', ?, ?, 'active', datetime('now'))",
        )
        .bind(session_id)
        .bind(slug)
        .bind(slot)
        .execute(pool)
        .await
        .unwrap();
    }

    /// devorch_dispatch_task must persist the dispatch row to SQLite even if
    /// Temporal is unreachable — the work item must survive a Temporal outage.
    #[tokio::test]
    async fn dispatch_task_persists_work_item_to_sqlite() {
        let _env = ENV_LOCK.lock().await;
        std::env::remove_var("DEVORCH_SESSION");
        let (pool, _dir) = crate::db::test_helpers::create_test_pool().await;
        seed_active_session(&pool, "sess-1", "persist-test", 9).await;

        let params = json!({
            "session": "persist-test-9",
            "from_role": "orchestrator",
            "to_role": "ai",
            "task_id": "issue-168",
            "summary": "Implement the auth module",
            "priority": "high",
            "files": ["src/auth/mod.rs"],
            "next_action": "Read the existing session code first"
        });

        // Call inner fn directly; Temporal is not running so signal fails,
        // but the SQLite write happens first.
        let _ = dispatch_task_inner(&pool, &params).await;
        std::env::remove_var("DEVORCH_SESSION");

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM dispatches WHERE task_id = 'issue-168' AND to_role = 'ai'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(
            count, 1,
            "dispatch must be persisted to SQLite even when Temporal signal fails"
        );
    }

    /// Validates that the correct fields are stored.
    #[tokio::test]
    async fn dispatch_task_stores_correct_fields() {
        let _env = ENV_LOCK.lock().await;
        std::env::remove_var("DEVORCH_SESSION");
        let (pool, _dir) = crate::db::test_helpers::create_test_pool().await;
        seed_active_session(&pool, "sess-2", "fields-test", 3).await;

        let params = json!({
            "session": "fields-test-3",
            "from_role": "orchestrator",
            "to_role": "dat",
            "task_id": "issue-42",
            "summary": "Build the pipeline",
            "priority": "normal"
        });

        let _ = dispatch_task_inner(&pool, &params).await;

        let row: Option<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT task_id, from_role, to_role, summary, priority FROM dispatches WHERE task_id = 'issue-42'"
        )
        .fetch_optional(&pool)
        .await
        .unwrap();

        let row = row.expect("dispatch row should exist");
        assert_eq!(row.0, "issue-42");
        assert_eq!(row.1, "orchestrator");
        assert_eq!(row.2, "dat");
        assert_eq!(row.3, "Build the pipeline");
        assert_eq!(row.4, "normal");
    }

    /// Invalid to_role must be rejected without touching SQLite.
    #[tokio::test]
    async fn dispatch_task_rejects_invalid_to_role() {
        let _env = ENV_LOCK.lock().await;
        std::env::remove_var("DEVORCH_SESSION");
        let (pool, _dir) = crate::db::test_helpers::create_test_pool().await;

        let params = json!({
            "session": "navi-1",
            "from_role": "orchestrator",
            "to_role": "nonexistent_role",
            "task_id": "t1",
            "summary": "test"
        });

        let result = dispatch_task_inner(&pool, &params).await;
        assert!(result.is_err(), "should reject unknown to_role");
    }

    // ── orchestrator_inbox tests ───────────────────────────────────────────────

    /// devorch_orchestrator_inbox must return inbound items from the SQLite
    /// projection when Temporal is unavailable.
    #[tokio::test]
    async fn orchestrator_inbox_returns_inbound_items_from_sqlite() {
        let _env = ENV_LOCK.lock().await;
        std::env::remove_var("DEVORCH_SESSION");
        let (pool, _dir) = crate::db::test_helpers::create_test_pool().await;
        seed_active_session(&pool, "sess-inbox", "proj", 1).await;

        for (mid, role, status) in [("msg-1", "ai", "complete"), ("msg-2", "dat", "blocked")] {
            sqlx::query(
                "INSERT INTO inbox_messages \
                 (message_id, session_id, role, status, summary, received_at) \
                 VALUES (?, 'sess-inbox', ?, ?, 'test summary', datetime('now'))",
            )
            .bind(mid)
            .bind(role)
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
        }

        let params = json!({ "session": "proj-1" });
        // Temporal is not running; orchestrator_inbox_inner falls back to SQLite.
        let result = orchestrator_inbox_inner(&pool, &params).await.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap_or("");

        assert!(
            text.contains("msg-1"),
            "inbox must contain msg-1; got: {text}"
        );
        assert!(
            text.contains("msg-2"),
            "inbox must contain msg-2; got: {text}"
        );
        assert!(
            text.contains("complete") || text.contains("blocked"),
            "inbox must contain status values"
        );
    }

    /// Cleared messages must not appear in the inbox.
    #[tokio::test]
    async fn orchestrator_inbox_excludes_cleared_messages() {
        let _env = ENV_LOCK.lock().await;
        std::env::remove_var("DEVORCH_SESSION");
        let (pool, _dir) = crate::db::test_helpers::create_test_pool().await;
        seed_active_session(&pool, "sess-clr", "proj2", 2).await;

        sqlx::query(
            "INSERT INTO inbox_messages \
             (message_id, session_id, role, status, cleared, received_at) \
             VALUES ('cleared-msg', 'sess-clr', 'ai', 'complete', 1, datetime('now'))",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO inbox_messages \
             (message_id, session_id, role, status, cleared, received_at) \
             VALUES ('active-msg', 'sess-clr', 'sec', 'acked', 0, datetime('now'))",
        )
        .execute(&pool)
        .await
        .unwrap();

        let params = json!({ "session": "proj2-2" });
        let result = orchestrator_inbox_inner(&pool, &params).await.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap_or("");

        assert!(
            !text.contains("cleared-msg"),
            "cleared messages must not appear in inbox"
        );
        assert!(
            text.contains("active-msg"),
            "uncleared messages must appear in inbox"
        );
    }

    // ── dispatch_task session scope guard ─────────────────────────────────────
    // These tests call handle_dispatch_task (the public wrapper) which enforces the
    // session scope guard. ENV_LOCK serialises all env-mutating tests to prevent races.

    /// handle_dispatch_task must reject a session that doesn't match DEVORCH_SESSION.
    #[tokio::test]
    async fn dispatch_task_rejects_mismatched_session() {
        let _env = ENV_LOCK.lock().await;
        let (pool, _dir) = crate::db::test_helpers::create_test_pool().await;
        std::env::set_var("DEVORCH_SESSION", "scopeguard-test-77");

        let params = json!({
            "session": "wrong-session-99",
            "from_role": "orchestrator",
            "to_role": "ai",
            "task_id": "t-scope-1",
            "summary": "should be rejected"
        });

        let result = handle_dispatch_task(&pool, params).await.unwrap();
        std::env::remove_var("DEVORCH_SESSION");
        // The wrapper converts scope rejection to isError response, not Err().
        let is_error = result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(
            is_error,
            "should produce an isError response for mismatched session"
        );
        let text = result["content"][0]["text"].as_str().unwrap_or("");
        assert!(
            text.contains("does not match active DEVORCH_SESSION"),
            "expected scope rejection text, got: {text}"
        );

        // Must not have written to SQLite.
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM dispatches WHERE task_id = 't-scope-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0, "rejected dispatch must not be persisted");
    }

    /// handle_dispatch_task must accept a session that matches DEVORCH_SESSION.
    #[tokio::test]
    async fn dispatch_task_accepts_matching_session() {
        let _env = ENV_LOCK.lock().await;
        let (pool, _dir) = crate::db::test_helpers::create_test_pool().await;
        seed_active_session(&pool, "sess-sg", "scopeguard", 77).await;
        std::env::set_var("DEVORCH_SESSION", "scopeguard-77");

        let params = json!({
            "session": "scopeguard-77",
            "from_role": "orchestrator",
            "to_role": "ai",
            "task_id": "t-scope-2",
            "summary": "should be accepted"
        });

        // Temporal is not running so the signal will fail, but SQLite write must succeed.
        let _ = handle_dispatch_task(&pool, params).await;
        std::env::remove_var("DEVORCH_SESSION");

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM dispatches WHERE task_id = 't-scope-2'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "matching session must be persisted");
    }

    /// handle_dispatch_task must allow any session when DEVORCH_SESSION is unset.
    #[tokio::test]
    async fn dispatch_task_allows_any_session_when_env_unset() {
        let _env = ENV_LOCK.lock().await;
        std::env::remove_var("DEVORCH_SESSION");
        let (pool, _dir) = crate::db::test_helpers::create_test_pool().await;
        seed_active_session(&pool, "sess-free", "free", 1).await;

        let params = json!({
            "session": "free-1",
            "from_role": "orchestrator",
            "to_role": "ops",
            "task_id": "t-scope-3",
            "summary": "no env guard active"
        });

        let _ = handle_dispatch_task(&pool, params).await;

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM dispatches WHERE task_id = 't-scope-3'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "should persist when DEVORCH_SESSION is not set");
    }

    // ── orchestrator_inbox session scope guard ────────────────────────────────

    /// handle_orchestrator_inbox must reject a session that doesn't match DEVORCH_SESSION.
    #[tokio::test]
    async fn orchestrator_inbox_rejects_mismatched_session() {
        let _env = ENV_LOCK.lock().await;
        let (pool, _dir) = crate::db::test_helpers::create_test_pool().await;
        std::env::set_var("DEVORCH_SESSION", "inbox-guard-88");

        let params = json!({ "session": "wrong-inbox-99" });
        let result = handle_orchestrator_inbox(&pool, params).await.unwrap();
        std::env::remove_var("DEVORCH_SESSION");

        let is_error = result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(
            is_error,
            "should produce an isError response for mismatched session"
        );
        let text = result["content"][0]["text"].as_str().unwrap_or("");
        assert!(
            text.contains("does not match active DEVORCH_SESSION"),
            "expected scope rejection text, got: {text}"
        );
    }

    /// handle_orchestrator_inbox must accept a matching session.
    #[tokio::test]
    async fn orchestrator_inbox_accepts_matching_session() {
        let _env = ENV_LOCK.lock().await;
        std::env::remove_var("DEVORCH_SESSION");
        let (pool, _dir) = crate::db::test_helpers::create_test_pool().await;
        seed_active_session(&pool, "sess-ib88", "inboxmatch", 88).await;
        std::env::set_var("DEVORCH_SESSION", "inboxmatch-88");

        let params = json!({ "session": "inboxmatch-88" });
        // Temporal is down; falls back to SQLite. Must not error on scope.
        let result = handle_orchestrator_inbox(&pool, params).await;
        std::env::remove_var("DEVORCH_SESSION");

        assert!(
            result.is_ok(),
            "matching session must not be rejected: {:?}",
            result
        );
        let is_error = result
            .as_ref()
            .unwrap()
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(
            !is_error,
            "matching session must not produce an isError response"
        );
    }

    // ── report_status → orch.stateTransition ─────────────────────────────────

    /// handle_report_status must persist locally and NOT panic when Temporal is down.
    /// Verifies the stateTransition signal path (signal will fail; local write must succeed).
    #[tokio::test]
    async fn report_status_persists_and_tolerates_temporal_down() {
        let _env = ENV_LOCK.lock().await;
        std::env::remove_var("DEVORCH_SESSION");
        let (pool, _dir) = crate::db::test_helpers::create_test_pool().await;
        seed_active_session(&pool, "sess-rs", "rstest", 5).await;

        // Register an agent so handle_report_status can find it.
        // session_id must match report.session ("rstest-5") because get_agents_for_session
        // queries WHERE session_id = report.session directly.
        // worktree_path and branch are NOT NULL in the schema.
        sqlx::query(
            "INSERT INTO agents (id, session_id, role, agent_kind, worktree_path, branch, status, created_at) \
             VALUES ('agent-rs-1', 'rstest-5', 'ai', 'claude', '/tmp/rstest', 'main', 'idle', datetime('now'))"
        )
        .execute(&pool)
        .await
        .unwrap();

        let args = json!({
            "session": "rstest-5",
            "role": "ai",
            // Use "failed" — a valid agent status per the agents table CHECK constraint.
            "status": "failed",
            "task_id": "t-rs-1",
            "summary": "blocked on missing dependency",
            "validation": "no upstream branch found",
            "next_action": "orchestrator should unblock",
            // scope fields required by enforce_scope
            "team_id": "rstest-5",
            "temporal_namespace": "default",
            "repo_id": "rstest"
        });

        // Must not panic. Temporal signal will fail (no server) — that's expected.
        let result = handle_report_status(&pool, args).await;
        assert!(
            result.is_ok(),
            "report_status must succeed even when Temporal is down: {:?}",
            result
        );

        // Agent status must be updated locally.
        let status: String =
            sqlx::query_scalar("SELECT status FROM agents WHERE id = 'agent-rs-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "failed", "agent status must be persisted locally");

        // Event must be logged — log_event uses report.session ("rstest-5") as session_id.
        let event_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE session_id = 'rstest-5' AND event_type = 'status_report'"
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(event_count, 1, "status_report event must be logged");
    }

    // ── parse_session ──────────────────────────────────────────────────────────

    #[test]
    fn parse_session_splits_correctly() {
        let (slug, slot) = parse_session("navi-9").unwrap();
        assert_eq!(slug, "navi");
        assert_eq!(slot, 9);

        let (slug, slot) = parse_session("m7-navi-42").unwrap();
        assert_eq!(slug, "m7-navi");
        assert_eq!(slot, 42);
    }

    #[test]
    fn parse_session_rejects_invalid() {
        assert!(parse_session("nosession").is_err());
        assert!(parse_session("name-xyz").is_err());
    }
}
