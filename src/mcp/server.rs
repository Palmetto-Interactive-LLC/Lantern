use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info};

use crate::mcp::tools;

/// MCP stdio server for Lantern Relay.
/// Speaks JSON-RPC 2.0 over stdin/stdout.
pub struct McpServer {
    pool: SqlitePool,
}

impl McpServer {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Run the stdio server loop until EOF.
    pub async fn run(&self) -> anyhow::Result<()> {
        info!("MCP server starting on stdio");

        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();

        let mut stdout = tokio::io::stdout();

        while let Some(line) = lines.next_line().await? {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            debug!("MCP recv: {}", line);

            match self.handle_request(line).await {
                Ok(Some(response)) => {
                    let resp_text = serde_json::to_string(&response)?;
                    stdout.write_all(resp_text.as_bytes()).await?;
                    stdout.write_all(b"\n").await?;
                    stdout.flush().await?;
                    debug!("MCP send: {}", resp_text);
                }
                Ok(None) => {
                    // Notification — no response required.
                }
                Err(e) => {
                    error!("MCP request handling failed: {}", e);
                    let fallback = json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": {
                            "code": -32603,
                            "message": format!("Internal error: {}", e)
                        }
                    });
                    let resp_text = serde_json::to_string(&fallback)?;
                    stdout.write_all(resp_text.as_bytes()).await?;
                    stdout.write_all(b"\n").await?;
                    stdout.flush().await?;
                }
            }
        }

        info!("MCP server stdin closed");
        Ok(())
    }

    async fn handle_request(&self, line: &str) -> anyhow::Result<Option<Value>> {
        let req: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                return Ok(Some(json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    }
                })));
            }
        };

        if req.jsonrpc != "2.0" {
            return Ok(Some(make_error(
                req.id,
                -32600,
                "Invalid Request: expected jsonrpc 2.0",
            )));
        }

        // Notifications have no id; we process but don't reply.
        let is_notification = req.id.is_none();

        let result = match req.method.as_str() {
            "initialize" => Ok(make_result(
                req.id.clone(),
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name": "lantern-relay-mcp",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            )),
            "tools/list" => Ok(make_result(
                req.id.clone(),
                json!({
                    "tools": tools_schema()
                }),
            )),
            "tools/call" => {
                let params = req.params.unwrap_or(json!({}));
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

                let tool_result = match name {
                    "devorch_ping" => tools::handle_ping(&self.pool, arguments).await,
                    "devorch_ack" => tools::handle_ack(&self.pool, arguments).await,
                    "devorch_blocker" => tools::handle_blocker(&self.pool, arguments).await,
                    "devorch_query_team_state" => {
                        tools::handle_query_team_state(&self.pool, arguments).await
                    }
                    "devorch_get_setup_instructions" => {
                        tools::handle_get_setup_instructions(&self.pool, arguments).await
                    }
                    "devorch_dispatch_task" => {
                        tools::handle_dispatch_task(&self.pool, arguments).await
                    }
                    "devorch_orchestrator_inbox" => {
                        tools::handle_orchestrator_inbox(&self.pool, arguments).await
                    }
                    "devorch_report_status" => {
                        tools::handle_report_status(&self.pool, arguments).await
                    }
                    "devorch_peer_message" => {
                        tools::handle_peer_message(&self.pool, arguments).await
                    }
                    "devorch_event_subscribe" => {
                        tools::handle_event_subscribe(&self.pool, arguments).await
                    }
                    "devorch_event_publish" => {
                        tools::handle_event_publish(&self.pool, arguments).await
                    }
                    "devorch_ask_advisor" => tools::handle_ask_advisor(&self.pool, arguments).await,
                    _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
                };

                match tool_result {
                    Ok(value) => {
                        // A tool may return either a raw payload (which we wrap into an
                        // MCP content envelope here) or an already-formed envelope
                        // ({"content":[...]}, optionally with "isError"). Pass the latter
                        // through untouched so it is not double-wrapped.
                        let result = if value.get("content").is_some() {
                            value
                        } else {
                            json!({
                                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value)? }],
                                "isError": false
                            })
                        };
                        Ok(make_result(req.id.clone(), result))
                    }
                    Err(e) => Ok(make_result(
                        req.id.clone(),
                        json!({
                            "content": [{ "type": "text", "text": e.to_string() }],
                            "isError": true
                        }),
                    )),
                }
            }
            _ => Ok(make_error(
                req.id.clone(),
                -32601,
                &format!("Method not found: {}", req.method),
            )),
        };

        if is_notification {
            Ok(None)
        } else {
            result.map(Some)
        }
    }
}

// ── Tool schema (single source of truth for tools/list) ──────────────────────

fn tools_schema() -> Value {
    json!([
        {
            "name": "devorch_dispatch_task",
            "description": "Dispatch a task from the Orchestrator to a worker role.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session":     { "type": "string", "description": "The current session ID (e.g., navi-9)." },
                    "from_role":   { "type": "string", "description": "Your role (should be orchestrator)." },
                    "to_role":     {
                        "type": "string",
                        "description": "The target worker role, as listed in your setup instructions (team specialists like ai/dat/sec, or worker-1..worker-N, executor, fixer)."
                    },
                    "task_id":     { "type": "string", "description": "The task ID (e.g., issue number like 168)." },
                    "summary":     { "type": "string", "description": "A brief summary of the task." },
                    "next_action": { "type": "string", "description": "Recommended first step for the worker." },
                    "files":       { "type": "array", "items": { "type": "string" }, "description": "Relevant file paths." },
                    "priority":    { "type": "string", "enum": ["low", "normal", "high"] }
                },
                "required": ["session", "from_role", "to_role", "task_id", "summary"]
            }
        },
        {
            "name": "devorch_ping",
            "description": "Send a status-query ping to a worker role to kick them in the butt, pick up the pace, and demand an immediate status update on their progress.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session":          { "type": "string" },
                    "from_role":        { "type": "string" },
                    "to_role":          {
                        "type": "string",
                        "description": "The target worker role, as listed in your setup instructions (team specialists, worker-1..worker-N, executor, fixer)."
                    }
                },
                "required": ["session", "from_role", "to_role"]
            }
        },
        {
            "name": "devorch_ack",
            "description": "Acknowledge a ping or task assignment with a brief, high-value status update on your active progress.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session":     { "type": "string" },
                    "role":        { "type": "string" },
                    "summary":     { "type": "string", "description": "A brief status update containing what you are currently working on, your progress, and your immediate next actions. Avoid empty 'pong' or generic messages." }
                },
                "required": ["session", "role"]
            }
        },
        {
            "name": "devorch_blocker",
            "description": "Report an issue or blocker that prevents you from proceeding.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session":     { "type": "string" },
                    "role":        { "type": "string" },
                    "summary":     { "type": "string", "description": "Detailed explanation of the blocker." }
                },
                "required": ["session", "role", "summary"]
            }
        },
        {
            "name": "devorch_query_team_state",
            "description": "Get a complete snapshot of the team's current state, including active tasks and latest signals.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session":            { "type": "string" },
                    "team_id":            { "type": "string" },
                    "temporal_namespace": { "type": "string" },
                    "task_queue":         { "type": "string" },
                    "repo_id":            { "type": "string" }
                },
                "required": ["session"]
            }
        },
        {
            "name": "devorch_orchestrator_inbox",
            "description": "Fetch durable unacknowledged status transitions from the orchestrator inbox.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session":           { "type": "string", "description": "The current session ID." },
                    "clear_message_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Message IDs to clear from the inbox after reading."
                    }
                },
                "required": ["session"]
            }
        },
        {
            "name": "devorch_get_setup_instructions",
            "description": "Get your initial setup instructions and context based on your role.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session":            { "type": "string" },
                    "role":               { "type": "string" },
                    "agent":              { "type": "string" },
                    "team_id":            { "type": "string" },
                    "temporal_namespace": { "type": "string" },
                    "task_queue":         { "type": "string" },
                    "repo_id":            { "type": "string" }
                },
                "required": ["session", "role", "agent"]
            }
        },
        {
            "name": "devorch_report_status",
            "description": "Report the status of the caller's current task.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session":            { "type": "string", "description": "Current session ID" },
                    "role":               { "type": "string", "description": "Caller's role" },
                    "status":             { "type": "string", "description": "ack | complete | blocked | failed | degraded | recovered" },
                    "task_id":            { "type": "string", "description": "Optional task ID" },
                    "summary":            { "type": "string", "description": "Optional brief explanation of the progress" },
                    "validation":         { "type": "string", "description": "Optional evidence/validation details" },
                    "next_action":        { "type": "string", "description": "Optional next step recommended for the workflow" },
                    "assignment_id":      { "type": "string", "description": "Optional lease/assignment ID for generation tracking" },
                    "generation":         { "type": "integer", "description": "Optional generation counter for lease stale validation" },
                    "team_id":            { "type": "string", "description": "Optional team/session scoping field" },
                    "temporal_namespace": { "type": "string", "description": "Optional Temporal namespace scoping field" },
                    "task_queue":         { "type": "string", "description": "Optional Temporal task queue scoping field" },
                    "repo_id":            { "type": "string", "description": "Optional repo ID scoping field" }
                },
                "required": ["session", "role", "status"]
            }
        },
        {
            "name": "devorch_peer_message",
            "description": "Send a message to, or request an action from, another worker role.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session":            { "type": "string", "description": "Current session ID" },
                    "from_role":          { "type": "string", "description": "Caller's role" },
                    "to_role":            { "type": "string", "description": "Target worker role (enum)" },
                    "info":               { "type": "string", "description": "Message body" },
                    "task_id":            { "type": "string", "description": "Optional task ID" },
                    "requested_action":   { "type": "string", "description": "Optional action request" },
                    "team_id":            { "type": "string", "description": "Optional team scoping" },
                    "temporal_namespace": { "type": "string", "description": "Optional Temporal namespace scoping" },
                    "task_queue":         { "type": "string", "description": "Optional Temporal task queue scoping" },
                    "repo_id":            { "type": "string", "description": "Optional repo ID scoping" }
                },
                "required": ["session", "from_role", "to_role", "info"]
            }
        },
        {
            "name": "devorch_event_subscribe",
            "description": "Subscribe a role to an MCP event type for the session.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session":    { "type": "string", "description": "Current session ID" },
                    "role":       { "type": "string", "description": "Subscribing role" },
                    "event_type": { "type": "string", "description": "Event type to subscribe to" }
                },
                "required": ["session", "role", "event_type"]
            }
        },
        {
            "name": "devorch_event_publish",
            "description": "Publish an MCP event, fanned out to subscribed busy panes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session":    { "type": "string", "description": "Current session ID" },
                    "event_type": { "type": "string", "description": "Event type" },
                    "publisher":  { "type": "string", "description": "Publishing role/identity" },
                    "payload":    { "type": "object", "description": "Arbitrary JSON payload" }
                },
                "required": ["session", "event_type", "publisher"]
            }
        },
        {
            "name": "devorch_ask_advisor",
            "description": "Ask the session's advisor (Executor pattern only) a question and block for its reply. Use before substantive work, when stuck, and before declaring done.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session":         { "type": "string", "description": "Current session ID" },
                    "role":            { "type": "string", "description": "Your own role (e.g. executor)" },
                    "question":        { "type": "string", "description": "The question for the advisor" },
                    "context_summary": { "type": "string", "description": "Optional brief context (what you've tried, key evidence)" }
                },
                "required": ["session", "role", "question"]
            }
        }
    ])
}

#[derive(Debug, serde::Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

fn make_result(id: Option<Value>, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn make_error(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_names() -> Vec<String> {
        tools_schema()
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.get("name").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn tools_list_contains_all_tools() {
        let names = tool_names();
        assert_eq!(
            names.len(),
            12,
            "expected exactly 12 tools, got {}",
            names.len()
        );
        for expected in &[
            "devorch_dispatch_task",
            "devorch_ping",
            "devorch_ack",
            "devorch_blocker",
            "devorch_query_team_state",
            "devorch_orchestrator_inbox",
            "devorch_get_setup_instructions",
            "devorch_report_status",
            "devorch_peer_message",
            "devorch_event_subscribe",
            "devorch_event_publish",
            "devorch_ask_advisor",
        ] {
            assert!(
                names.contains(&expected.to_string()),
                "tools/list is missing '{expected}'"
            );
        }
    }

    /// Tools that return an MCP content envelope (dispatch_task / orchestrator_inbox)
    /// must NOT be re-wrapped by the server. Exercised through the real request path,
    /// which the per-tool unit tests bypass.
    #[tokio::test]
    async fn tools_call_does_not_double_wrap_content_envelope() {
        // This call goes through the scope guard, which reads the process-global
        // DEVORCH_SESSION. Hold the shared env lock and clear the var so a
        // concurrent tools test can't leak a session value that turns the inbox
        // call into a rejection (historic flake — see ENV_LOCK docs).
        let _env = crate::db::test_helpers::ENV_LOCK.lock().await;
        std::env::remove_var("DEVORCH_SESSION");
        std::env::remove_var("DEVORCH_RUN_ID");

        let (pool, _dir) = crate::db::test_helpers::create_test_pool().await;
        let srv = McpServer::new(pool);
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"devorch_orchestrator_inbox","arguments":{"session":"nope-1"}}}"#;
        let resp = srv.handle_request(line).await.unwrap().unwrap();
        let text = resp["result"]["content"][0]["text"].as_str().unwrap_or("");
        // Must be the tool's own payload ("[]" — no session rows), not a re-serialized
        // {"content":[...]} envelope.
        assert_eq!(
            text.trim(),
            "[]",
            "content envelope was double-wrapped: {text}"
        );
    }
}
