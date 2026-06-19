// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use futures::StreamExt;
use serde_json::json;
use std::collections::HashMap;
use std::fs;

use super::{Cli, RunOutcome};
use crate::cli::{
    connect_gateway, rest_client, rest_grpc_error_details, rest_request_json,
};
use crate::gateway::rpc::data_proto;
use crate::gateway::rpc::proto::{
    CancelWorkflowRunRequest, CreateWorkflowRunRequest, GetWorkflowRunRequest,
    ListWorkflowRunsRequest, ResumeWorkflowRunRequest, StreamWorkflowEventsRequest,
};

const MAX_REST_STREAM_BUFFER_BYTES: usize = 10 * 1024 * 1024;

#[derive(Args)]
pub(crate) struct WorkflowCommand {
    #[command(subcommand)]
    command: WorkflowCommands,
}

#[derive(Subcommand)]
enum WorkflowCommands {
    /// Create a workflow run.
    RunCreate {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        #[arg(long, conflicts_with = "input_file")]
        input: Option<String>,
        #[arg(long, conflicts_with = "input")]
        input_file: Option<String>,
    },
    /// Get one workflow run and its step runs.
    RunGet {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        run_id: String,
    },
    /// List workflow runs.
    RunList {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        #[arg(long, default_value_t = 0)]
        page_size: i32,
        #[arg(long, default_value = "")]
        before_run_id: String,
    },
    /// Resume a suspended workflow step.
    RunResume {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        run_id: String,
        step_id: String,
        #[arg(long, conflicts_with = "resume_file")]
        resume: Option<String>,
        #[arg(long, conflicts_with = "resume")]
        resume_file: Option<String>,
    },
    /// Cancel a workflow run.
    RunCancel {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        run_id: String,
    },
    /// Stream workflow run events.
    RunEvents {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        run_id: String,
    },
}

pub(super) async fn run(cli: &Cli, command: &WorkflowCommand) -> Result<RunOutcome> {
    match &command.command {
        WorkflowCommands::RunCreate {
            namespace,
            workflow,
            input,
            input_file,
        } => {
            let value =
                workflow_run_create(cli, namespace, workflow, read_json_arg(input, input_file)?)
                    .await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        WorkflowCommands::RunGet {
            namespace,
            workflow,
            run_id,
        } => {
            let value = workflow_run_get(cli, namespace, workflow, run_id).await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        WorkflowCommands::RunList {
            namespace,
            workflow,
            page_size,
            before_run_id,
        } => {
            let value =
                workflow_run_list(cli, namespace, workflow, *page_size, before_run_id).await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        WorkflowCommands::RunResume {
            namespace,
            workflow,
            run_id,
            step_id,
            resume,
            resume_file,
        } => {
            let value = workflow_run_resume(
                cli,
                namespace,
                workflow,
                run_id,
                step_id,
                read_json_arg(resume, resume_file)?,
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        WorkflowCommands::RunCancel {
            namespace,
            workflow,
            run_id,
        } => {
            let value = workflow_run_cancel(cli, namespace, workflow, run_id).await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        WorkflowCommands::RunEvents {
            namespace,
            workflow,
            run_id,
        } => {
            workflow_run_events(cli, namespace, workflow, run_id).await?;
        }
    }
    Ok(RunOutcome { exit_code: None })
}

fn read_json_arg(value: &Option<String>, file: &Option<String>) -> Result<String> {
    let raw = if let Some(file) = file {
        fs::read_to_string(file).with_context(|| format!("Failed to read JSON file '{}'", file))?
    } else {
        value.clone().unwrap_or_else(|| "{}".to_string())
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).context("Argument must be valid JSON")?;
    serde_json::to_string(&parsed).context("Failed to normalize JSON argument")
}

async fn workflow_run_create(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    input_json: String,
) -> Result<serde_json::Value> {
    if cli.rest {
        return rest_request_json(
            cli,
            reqwest::Method::POST,
            &format!(
                "/v1/ns/{}/workflows/{}/runs",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow)
            ),
            Some(json!({ "ns": namespace, "workflow": workflow, "inputJson": input_json })),
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .create_workflow_run(CreateWorkflowRunRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            input_json,
            labels: HashMap::new(),
        })
        .await
        .context("Failed to create workflow run")?
        .into_inner();
    let run = response.run.context("Workflow run missing from response")?;
    Ok(workflow_run_json(&run, &response.steps))
}

async fn workflow_run_get(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
) -> Result<serde_json::Value> {
    if cli.rest {
        return rest_request_json(
            cli,
            reqwest::Method::GET,
            &format!(
                "/v1/ns/{}/workflows/{}/runs/{}",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow),
                urlencoding::encode(run_id)
            ),
            None,
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .get_workflow_run(GetWorkflowRunRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            run_id: run_id.to_string(),
        })
        .await
        .context("Failed to get workflow run")?
        .into_inner();
    let run = response.run.context("Workflow run missing from response")?;
    Ok(workflow_run_json(&run, &response.steps))
}

async fn workflow_run_list(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    page_size: i32,
    before_run_id: &str,
) -> Result<serde_json::Value> {
    if cli.rest {
        let mut query = Vec::new();
        if page_size != 0 {
            query.push(format!("page_size={page_size}"));
        }
        if !before_run_id.is_empty() {
            query.push(format!(
                "before_run_id={}",
                urlencoding::encode(before_run_id)
            ));
        }
        let query = if query.is_empty() {
            String::new()
        } else {
            format!("?{}", query.join("&"))
        };
        return rest_request_json(
            cli,
            reqwest::Method::GET,
            &format!(
                "/v1/ns/{}/workflows/{}/runs{}",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow),
                query
            ),
            None,
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .list_workflow_runs(ListWorkflowRunsRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            page_size,
            before_run_id: before_run_id.to_string(),
        })
        .await
        .context("Failed to list workflow runs")?
        .into_inner();
    Ok(json!({
        "runs": response.runs.iter().map(|run| workflow_run_json(run, &[])).collect::<Vec<_>>(),
        "hasMore": response.has_more,
        "nextBeforeRunId": response.next_before_run_id,
    }))
}

async fn workflow_run_resume(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
    step_id: &str,
    resume_json: String,
) -> Result<serde_json::Value> {
    if cli.rest {
        return rest_request_json(
            cli,
            reqwest::Method::POST,
            &format!(
                "/v1/ns/{}/workflows/{}/runs/{}:resume",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow),
                urlencoding::encode(run_id)
            ),
            Some(json!({
                "ns": namespace,
                "workflow": workflow,
                "runId": run_id,
                "stepId": step_id,
                "resumeJson": resume_json,
            })),
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .resume_workflow_run(ResumeWorkflowRunRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            run_id: run_id.to_string(),
            step_id: step_id.to_string(),
            resume_json,
        })
        .await
        .context("Failed to resume workflow run")?
        .into_inner();
    let run = response.run.context("Workflow run missing from response")?;
    Ok(workflow_run_json(&run, &response.steps))
}

async fn workflow_run_cancel(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
) -> Result<serde_json::Value> {
    if cli.rest {
        return rest_request_json(
            cli,
            reqwest::Method::POST,
            &format!(
                "/v1/ns/{}/workflows/{}/runs/{}:cancel",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow),
                urlencoding::encode(run_id)
            ),
            Some(json!({ "ns": namespace, "workflow": workflow, "runId": run_id })),
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .cancel_workflow_run(CancelWorkflowRunRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            run_id: run_id.to_string(),
        })
        .await
        .context("Failed to cancel workflow run")?
        .into_inner();
    let run = response.run.context("Workflow run missing from response")?;
    Ok(workflow_run_json(&run, &response.steps))
}

async fn workflow_run_events(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
) -> Result<()> {
    if cli.rest {
        rest_stream_workflow_events(cli, namespace, workflow, run_id).await?;
        return Ok(());
    }
    let mut client = connect_gateway(cli).await?;
    let mut stream = client
        .stream_workflow_events(StreamWorkflowEventsRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            run_id: run_id.to_string(),
        })
        .await
        .context("Failed to stream workflow events")?
        .into_inner();
    while let Some(event) = stream
        .message()
        .await
        .context("Failed to read workflow event")?
    {
        println!("{}", serde_json::to_string(&workflow_event_json(&event))?);
    }
    Ok(())
}

async fn rest_stream_workflow_events(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
) -> Result<()> {
    let client = rest_client(cli)?;
    let path = format!(
        "/v1/ns/{}/workflows/{}/runs/{}/stream",
        urlencoding::encode(namespace),
        urlencoding::encode(workflow),
        urlencoding::encode(run_id)
    );
    let url = format!("{}{}", cli.gateway.trim_end_matches('/'), path);
    let response = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to call REST endpoint {}", url))?;
    let status = response.status();
    let headers = response.headers().clone();
    if !status.is_success() {
        let text = response
            .text()
            .await
            .with_context(|| format!("Failed to read REST response body from {}", url))?;
        anyhow::bail!(
            "REST {} {} failed: status={} body={}{}",
            path,
            url,
            status,
            text.trim(),
            rest_grpc_error_details(&headers)
        );
    }

    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("Failed to read REST stream from {}", url))?;
        buffer.extend_from_slice(&chunk);
        ensure_rest_stream_buffer_within_limit(buffer.len())?;
        let mut last_index = 0;
        while let Some(newline) = buffer[last_index..].iter().position(|byte| *byte == b'\n') {
            let absolute_newline = last_index + newline;
            let line = String::from_utf8_lossy(&buffer[last_index..absolute_newline]);
            print_stream_event_line(line.trim_end_matches('\r'))?;
            last_index = absolute_newline + 1;
        }
        if last_index > 0 {
            buffer.drain(..last_index);
        }
    }
    if !buffer.is_empty() {
        let line = String::from_utf8_lossy(&buffer);
        print_stream_event_line(line.trim_end_matches('\r'))?;
    }
    Ok(())
}

fn ensure_rest_stream_buffer_within_limit(buffer_len: usize) -> Result<()> {
    if buffer_len > MAX_REST_STREAM_BUFFER_BYTES {
        anyhow::bail!(
            "REST stream exceeded maximum buffer limit of {} bytes without a newline",
            MAX_REST_STREAM_BUFFER_BYTES
        );
    }
    Ok(())
}

fn print_stream_event_line(line: &str) -> Result<()> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') || line.starts_with("event:") {
        return Ok(());
    }
    let payload = line.strip_prefix("data:").map(str::trim).unwrap_or(line);
    if payload.is_empty() || payload == "[DONE]" {
        return Ok(());
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
        println!("{}", serde_json::to_string(&json)?);
    } else {
        println!("{}", payload);
    }
    Ok(())
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
