use anyhow::Result;
use serde_json::json;
use tracing::info;

use crate::temporal::client::{
    Payload, Payloads, SignalWorkflowExecutionRequest, TemporalGrpcClient, WorkflowExecution,
};

const DEFAULT_ENCODING: &str = "json/plain";

// NOTE: the SessionLifecycle/SessionSetupWorkflow bootstrap (SessionIdentity,
// search-attribute/payload builders, bootstrap_session_workflows) was removed —
// those workflows ran on a queue no worker polled. Session lifecycle and
// delivery are owned directly by the Rust runtime (SQLite + direct iTerm2
// injection); Temporal stays bootable but off the delivery path. Only the
// human-control + cleanup signal helpers remain here.

pub fn workflow_id_for_human_control(repo_id: &str, session_id: &str, run_id: &str) -> String {
    format!(
        "devorch/{}/{}/{}/human-control",
        repo_id, session_id, run_id
    )
}

pub fn build_human_control_signal_request(
    namespace: &str,
    workflow_id: &str,
    command: serde_json::Value,
) -> SignalWorkflowExecutionRequest {
    SignalWorkflowExecutionRequest {
        namespace: namespace.to_string(),
        workflow_execution: WorkflowExecution {
            workflow_id: workflow_id.to_string(),
            run_id: String::new(),
        },
        signal_name: "humanControlCommand".to_string(),
        input: Some(Payloads {
            payloads: vec![Payload {
                metadata: [("encoding".to_string(), DEFAULT_ENCODING.to_string())]
                    .into_iter()
                    .collect(),
                data: command.to_string(),
            }],
        }),
        identity: "lantern-human-control".to_string(),
    }
}

pub fn workflow_id_for_session_cleanup(repo_id: &str, session_id: &str, run_id: &str) -> String {
    format!("devorch/{}/{}/{}/cleanup", repo_id, session_id, run_id)
}

pub fn build_cleanup_request(
    namespace: &str,
    workflow_id: &str,
    preserve_worktrees: bool,
    closed_iterm: bool,
    released_leases: bool,
    finalized_audit: bool,
) -> SignalWorkflowExecutionRequest {
    let payload = json!({
        "preserveWorktrees": preserve_worktrees,
        "closedIterm": closed_iterm,
        "releasedLeases": released_leases,
        "finalizedAudit": finalized_audit,
    });

    SignalWorkflowExecutionRequest {
        namespace: namespace.to_string(),
        workflow_execution: WorkflowExecution {
            workflow_id: workflow_id.to_string(),
            run_id: String::new(),
        },
        signal_name: "cleanupRequested".to_string(),
        input: Some(Payloads {
            payloads: vec![Payload {
                metadata: [("encoding".to_string(), DEFAULT_ENCODING.to_string())]
                    .into_iter()
                    .collect(),
                data: payload.to_string(),
            }],
        }),
        identity: "lantern-cleanup".to_string(),
    }
}

pub async fn signal_human_control_command(
    addr: &str,
    namespace: &str,
    workflow_id: &str,
    command: serde_json::Value,
) -> Result<()> {
    let mut client = TemporalGrpcClient::connect(addr.to_string()).await?;
    let request = build_human_control_signal_request(namespace, workflow_id, command.clone());

    info!(%workflow_id, "Signaling human control command");
    match client.signal_workflow_execution(request).await {
        Ok(_) => Ok(()),
        Err(e) if e.code() == tonic::Code::NotFound => {
            info!(%workflow_id, "HumanControlWorkflow not found. Starting it on-demand...");

            let identity = command
                .get("identity")
                .ok_or_else(|| anyhow::anyhow!("missing identity in command"))?;
            let task_queue = identity
                .get("taskQueue")
                .and_then(|v| v.as_str())
                .unwrap_or("devorch");

            let start_args = vec![
                "workflow".to_string(),
                "start".to_string(),
                "--workflow-id".to_string(),
                workflow_id.to_string(),
                "--type".to_string(),
                "HumanControlWorkflow".to_string(),
                "--task-queue".to_string(),
                task_queue.to_string(),
                "--namespace".to_string(),
                namespace.to_string(),
                "--address".to_string(),
                addr.to_string(),
                "--input".to_string(),
                identity.to_string(),
            ];

            let start_output = std::process::Command::new("temporal")
                .args(&start_args)
                .output()?;

            if !start_output.status.success() {
                let stderr = String::from_utf8_lossy(&start_output.stderr);
                anyhow::bail!("failed to start HumanControlWorkflow: {}", stderr.trim());
            }

            info!(%workflow_id, "HumanControlWorkflow started successfully. Retrying signal...");
            let mut attempts = 0;
            let max_attempts = 5;
            loop {
                let retry_request =
                    build_human_control_signal_request(namespace, workflow_id, command.clone());
                match client.signal_workflow_execution(retry_request).await {
                    Ok(_) => break,
                    Err(e) if e.code() == tonic::Code::NotFound && attempts < max_attempts => {
                        attempts += 1;
                        let delay_ms = 100 * attempts;
                        info!(%workflow_id, attempt = attempts, delay_ms, "Workflow not found yet on retry. Sleeping and retrying...");
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

pub async fn signal_cleanup_request(
    addr: &str,
    namespace: &str,
    workflow_id: &str,
    preserve_worktrees: bool,
    closed_iterm: bool,
    released_leases: bool,
    finalized_audit: bool,
) -> Result<()> {
    let mut client = TemporalGrpcClient::connect(addr.to_string()).await?;
    let request = build_cleanup_request(
        namespace,
        workflow_id,
        preserve_worktrees,
        closed_iterm,
        released_leases,
        finalized_audit,
    );

    info!(%workflow_id, "Signaling cleanup workflow request");
    client.signal_workflow_execution(request).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn human_control_workflow_id_is_hard_scoped() {
        assert_eq!(
            workflow_id_for_human_control("repo-1", "navi-9", "run-1"),
            "devorch/repo-1/navi-9/run-1/human-control"
        );
    }

    #[test]
    fn human_control_signal_request_uses_payload_and_namespace() {
        let payload = json!({
            "commandId": "cmd-1",
            "action": "pause",
            "targetRole": "ai"
        });
        let request = build_human_control_signal_request(
            "default",
            "devorch/repo-1/navi-9/run-1/human-control",
            payload.clone(),
        );

        assert_eq!(request.namespace, "default");
        assert_eq!(
            request.workflow_execution.workflow_id,
            "devorch/repo-1/navi-9/run-1/human-control"
        );
        assert_eq!(request.signal_name, "humanControlCommand");
        assert_eq!(request.identity, "lantern-human-control");
        let encoded = request.input.unwrap().payloads.remove(0);
        assert_eq!(
            encoded.metadata.get("encoding").map(String::as_str),
            Some("json/plain")
        );
        assert_eq!(encoded.data, payload.to_string());
    }

    #[test]
    fn session_cleanup_workflow_id_is_hard_scoped() {
        assert_eq!(
            workflow_id_for_session_cleanup("repo-1", "navi-9", "run-1"),
            "devorch/repo-1/navi-9/run-1/cleanup"
        );
    }

    #[test]
    fn cleanup_request_uses_payload_and_signal_name() {
        let request = build_cleanup_request(
            "default",
            "devorch/repo-1/navi-9/run-1/cleanup",
            true,
            true,
            true,
            true,
        );

        assert_eq!(request.namespace, "default");
        assert_eq!(
            request.workflow_execution.workflow_id,
            "devorch/repo-1/navi-9/run-1/cleanup"
        );
        assert_eq!(request.signal_name, "cleanupRequested");
        assert_eq!(request.identity, "lantern-cleanup");
        let encoded = request.input.unwrap().payloads.remove(0);
        assert_eq!(
            encoded.metadata.get("encoding").map(String::as_str),
            Some("json/plain")
        );

        let payload: serde_json::Value = serde_json::from_str(&encoded.data).unwrap();
        assert_eq!(payload["preserveWorktrees"], true);
        assert_eq!(payload["closedIterm"], true);
        assert_eq!(payload["releasedLeases"], true);
        assert_eq!(payload["finalizedAudit"], true);
    }
}
