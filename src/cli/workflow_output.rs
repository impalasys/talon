const MAX_REST_STREAM_BUFFER_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct KnowledgeManifestFile {
    spec: KnowledgeSpecFile,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct KnowledgeSpecFile {
    content: Option<String>,
    content_from_file: Option<String>,
}

fn workflow_run_json(
    run: &data_proto::WorkflowRun,
    steps: &[data_proto::WorkflowStepRun],
) -> serde_json::Value {
    json!({
        "id": run.id,
        "workflow": run.workflow,
        "ns": run.ns,
        "status": run.status,
        "input": parse_json_field(&run.input_json),
        "state": parse_json_field(&run.state_json),
        "output": parse_json_field(&run.output_json),
        "createdAt": run.created_at,
        "updatedAt": run.updated_at,
        "labels": run.labels,
        "claimExpiresAt": run.claim_expires_at,
        "error": run.error,
        "workflowRevision": run.workflow_revision,
        "claimOwner": run.claim_owner,
        "claimAttempt": run.claim_attempt,
        "lastDispatchReason": run.last_dispatch_reason,
        "steps": steps.iter().map(workflow_step_run_json).collect::<Vec<_>>(),
    })
}

fn workflow_step_run_json(step: &data_proto::WorkflowStepRun) -> serde_json::Value {
    json!({
        "id": step.id,
        "stepId": step.step_id,
        "attempt": step.attempt,
        "status": step.status,
        "input": parse_json_field(&step.input_json),
        "output": parse_json_field(&step.output_json),
        "error": step.error,
        "childSessionId": step.child_session_id,
        "childWorkflowRunId": step.child_workflow_run_id,
        "resume": parse_json_field(&step.resume_json),
        "suspend": parse_json_field(&step.suspend_json),
        "createdAt": step.created_at,
        "updatedAt": step.updated_at,
        "nextRetryAt": step.next_retry_at,
        "timeoutAt": step.timeout_at,
        "waitWakeupHandle": step.wait_wakeup_handle,
        "waitUntilAt": step.wait_until_at,
    })
}

fn workflow_event_json(event: &data_proto::WorkflowRunEvent) -> serde_json::Value {
    json!({
        "id": event.id,
        "ns": event.ns,
        "workflow": event.workflow,
        "runId": event.run_id,
        "type": event.r#type,
        "stepId": event.step_id,
        "message": event.message,
        "payload": parse_json_field(&event.payload_json),
        "timestamp": event.timestamp,
    })
}

fn parse_json_field(value: &str) -> serde_json::Value {
    if value.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(value).unwrap_or_else(|_| serde_json::Value::String(value.to_string()))
    }
}
