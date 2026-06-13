// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use prost::Message;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

use crate::control::{
    events, keys, topics, ControlPlane, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
};
use crate::gateway::rpc::models;
use crate::knowledge::KvKnowledgeBook;

const MAX_CAS_RETRIES: usize = 8;
const DEFAULT_WORKFLOW_CLAIM_TIMEOUT_SECONDS: i64 = 60;

pub const LABEL_WORKFLOW: &str = "talon.impalasys.com/workflow";
pub const LABEL_WORKFLOW_RUN: &str = "talon.impalasys.com/workflow-run";
pub const LABEL_WORKFLOW_STEP: &str = "talon.impalasys.com/workflow-step";
pub const LABEL_WORKFLOW_ATTEMPT: &str = "talon.impalasys.com/workflow-attempt";
pub const LABEL_PARENT_WORKFLOW: &str = "talon.impalasys.com/parent-workflow";
pub const LABEL_PARENT_WORKFLOW_RUN: &str = "talon.impalasys.com/parent-workflow-run";
pub const LABEL_PARENT_WORKFLOW_STEP: &str = "talon.impalasys.com/parent-workflow-step";

const STATUS_QUEUED: &str = "QUEUED";
const STATUS_RUNNING: &str = "RUNNING";
const STATUS_STARTING: &str = "STARTING";
const STATUS_WAITING_CHILDREN: &str = "WAITING_CHILDREN";
const STATUS_WAITING_RETRY: &str = "WAITING_RETRY";
const STATUS_SUSPENDED: &str = "SUSPENDED";
const STATUS_COMPLETED: &str = "COMPLETED";
const STATUS_FAILED: &str = "FAILED";
const STATUS_CANCELLED: &str = "CANCELLED";
const STATUS_SKIPPED: &str = "SKIPPED";
const STATUS_WAITING_CHILD_SESSION: &str = "WAITING_CHILD_SESSION";
const STATUS_WAITING_CHILD_WORKFLOW: &str = "WAITING_CHILD_WORKFLOW";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowWakeupPayload {
    pub namespace: String,
    pub workflow: String,
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    pub intended_fire_at: i64,
    pub reason: String,
}

#[derive(Debug)]
pub struct WorkflowClaimInProgressError;

impl std::fmt::Display for WorkflowClaimInProgressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "workflow run is already being processed")
    }
}

impl std::error::Error for WorkflowClaimInProgressError {}

#[derive(Debug)]
pub struct WorkflowInvalidArgumentError {
    message: String,
}

impl WorkflowInvalidArgumentError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for WorkflowInvalidArgumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WorkflowInvalidArgumentError {}

#[derive(Debug)]
pub struct WorkflowNotFoundError {
    message: String,
}

impl WorkflowNotFoundError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for WorkflowNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WorkflowNotFoundError {}

pub fn workflow_claim_timeout_micros() -> i64 {
    std::env::var("TALON_WORKFLOW_CLAIM_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_WORKFLOW_CLAIM_TIMEOUT_SECONDS)
        * 1_000_000
}

pub fn validate_workflow(workflow: &models::Workflow) -> Result<()> {
    if workflow.name.trim().is_empty() {
        bail!("workflow name is required");
    }
    if workflow.name.contains('/') {
        bail!("workflow name cannot contain '/'");
    }
    if workflow.ns.trim().is_empty() {
        bail!("workflow namespace is required");
    }
    let spec = workflow
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("workflow spec is required"))?;
    if spec.steps.is_empty() {
        bail!("workflow spec.steps must contain at least one step");
    }

    validate_schema_json("inputSchema", &spec.input_schema_json)?;
    validate_schema_json("outputSchema", &spec.output_schema_json)?;
    validate_json_object("output", &spec.output_json)?;

    let mut ids = HashSet::new();
    for step in &spec.steps {
        if step.id.trim().is_empty() {
            bail!("workflow step id is required");
        }
        if !ids.insert(step.id.clone()) {
            bail!("duplicate workflow step id '{}'", step.id);
        }
        match step.r#type.as_str() {
            "agent" | "tool" | "workflow" | "transform" | "pause" | "wait" => {}
            other => bail!("unsupported workflow step type '{}'", other),
        }
        match step.r#type.as_str() {
            "agent" => {
                if step.agent.trim().is_empty() {
                    bail!("agent step '{}' requires agent", step.id);
                }
                if step.prompt.trim().is_empty() {
                    bail!("agent step '{}' requires prompt", step.id);
                }
            }
            "tool" if step.tool.trim().is_empty() => {
                bail!("tool step '{}' requires tool", step.id);
            }
            "workflow" if step.workflow.trim().is_empty() => {
                bail!("workflow step '{}' requires workflow", step.id);
            }
            "pause" if step.prompt.trim().is_empty() => {
                bail!("pause step '{}' requires prompt", step.id);
            }
            "wait"
                if step.wait_duration.trim().is_empty()
                    && step.wait_until.trim().is_empty()
                    && step.resume_schema_json.trim().is_empty() =>
            {
                bail!(
                    "wait step '{}' requires duration, until, or resumeSchema",
                    step.id
                );
            }
            _ => {}
        }
        validate_json_object(&format!("step {} when", step.id), &step.when_json)?;
        validate_predicate_json(&format!("step {} when", step.id), &step.when_json)?;
        validate_json_value(&format!("step {} input", step.id), &step.input_json)?;
        validate_schema_json(
            &format!("step {} resumeSchema", step.id),
            &step.resume_schema_json,
        )?;
        validate_duration_field(&format!("step {} timeout", step.id), &step.timeout)?;
        validate_duration_field(&format!("step {} duration", step.id), &step.wait_duration)?;
        if !step.wait_until.trim().is_empty() {
            parse_timestamp_micros(&step.wait_until)
                .map_err(|err| anyhow!("step {} until is invalid: {}", step.id, err))?;
        }
        if let Some(retry) = &step.retry {
            validate_retry_policy(&step.id, retry)?;
        }
        if let Some(output) = &step.output {
            validate_schema_json(
                &format!("step {} output.schema", step.id),
                &output.schema_json,
            )?;
            match output.format.as_str() {
                "" | "text" | "json" => {}
                other => bail!(
                    "unsupported output format '{}' for step '{}'",
                    other,
                    step.id
                ),
            }
        }
        for dep in &step.after {
            if dep == &step.id {
                bail!("workflow step '{}' cannot depend on itself", step.id);
            }
        }
    }
    for step in &spec.steps {
        for dep in &step.after {
            if !ids.contains(dep) {
                bail!(
                    "workflow step '{}' depends on unknown step '{}'",
                    step.id,
                    dep
                );
            }
        }
    }
    detect_cycle(spec)?;
    Ok(())
}

pub fn validate_input(schema_json: &str, input: &Value) -> Result<()> {
    validate_basic_json_schema("input", schema_json, input)
}

pub async fn create_run(
    cp: &ControlPlane,
    workflow: &models::Workflow,
    input_json: String,
    labels: HashMap<String, String>,
) -> Result<models::WorkflowRun> {
    let input = parse_json_or(input_json.as_str(), Value::Null)?;
    let spec = workflow
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("workflow spec is required"))?;
    validate_input(&spec.input_schema_json, &input)?;

    let now = Utc::now().timestamp_micros();
    let spec_json = serde_json::to_string(spec)?;
    let run = models::WorkflowRun {
        id: uuid::Uuid::now_v7().to_string(),
        workflow: workflow.name.clone(),
        ns: workflow.ns.clone(),
        status: STATUS_QUEUED.to_string(),
        input_json: serde_json::to_string(&input)?,
        state_json: "{}".to_string(),
        output_json: String::new(),
        created_at: now,
        updated_at: now,
        labels,
        claim_expires_at: None,
        error: String::new(),
        spec_json,
        workflow_revision: now as u64,
        claim_owner: String::new(),
        claim_attempt: 0,
        last_dispatch_reason: "created".to_string(),
    };
    cp.kv
        .set_msg(&keys::workflow_run(&run.ns, &run.workflow, &run.id), &run)
        .await?;
    append_run_event(
        cp,
        &run,
        "",
        "run_started",
        "workflow run created",
        Value::Null,
    )
    .await?;
    dispatch_workflow(
        cp.pubsub.as_ref(),
        &run.ns,
        &run.workflow,
        &run.id,
        "created",
    )
    .await?;
    Ok(run)
}

pub async fn dispatch_workflow(
    pubsub: &dyn MessagePublisher,
    ns: &str,
    workflow: &str,
    run_id: &str,
    reason: &str,
) -> Result<()> {
    let event = events::WorkflowDispatchEvent {
        ns: ns.to_string(),
        workflow: workflow.to_string(),
        run_id: run_id.to_string(),
        reason: reason.to_string(),
        step_id: String::new(),
        child_session_id: String::new(),
        timestamp: Utc::now().timestamp_micros(),
    };
    pubsub
        .publish(topics::WORKFLOW_DISPATCH_TOPIC, &event.encode_to_vec())
        .await
}

pub async fn dispatch_workflow_from_session_labels(
    cp: &ControlPlane,
    session: &models::Session,
) -> Result<()> {
    let Some(workflow) = session.labels.get(LABEL_WORKFLOW) else {
        return Ok(());
    };
    let Some(run_id) = session.labels.get(LABEL_WORKFLOW_RUN) else {
        return Ok(());
    };
    let step_id = session
        .labels
        .get(LABEL_WORKFLOW_STEP)
        .cloned()
        .unwrap_or_default();
    let event = events::WorkflowDispatchEvent {
        ns: session.ns.clone(),
        workflow: workflow.clone(),
        run_id: run_id.clone(),
        reason: "child_session_completed".to_string(),
        step_id,
        child_session_id: session.id.clone(),
        timestamp: Utc::now().timestamp_micros(),
    };
    cp.pubsub
        .publish(topics::WORKFLOW_DISPATCH_TOPIC, &event.encode_to_vec())
        .await
}

async fn dispatch_parent_workflow_from_run_labels(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
) -> Result<()> {
    let Some(parent_workflow) = run.labels.get(LABEL_PARENT_WORKFLOW) else {
        return Ok(());
    };
    let Some(parent_run_id) = run.labels.get(LABEL_PARENT_WORKFLOW_RUN) else {
        return Ok(());
    };
    let step_id = run
        .labels
        .get(LABEL_PARENT_WORKFLOW_STEP)
        .cloned()
        .unwrap_or_default();
    let event = events::WorkflowDispatchEvent {
        ns: run.ns.clone(),
        workflow: parent_workflow.clone(),
        run_id: parent_run_id.clone(),
        reason: "child_workflow_completed".to_string(),
        step_id,
        child_session_id: String::new(),
        timestamp: Utc::now().timestamp_micros(),
    };
    cp.pubsub
        .publish(topics::WORKFLOW_DISPATCH_TOPIC, &event.encode_to_vec())
        .await
}

pub async fn claim_run(
    kv: &dyn KeyValueStore,
    ns: &str,
    workflow: &str,
    run_id: &str,
    now: DateTime<Utc>,
    reason: &str,
) -> Result<Option<models::WorkflowRun>> {
    let key = keys::workflow_run(ns, workflow, run_id);
    let claim_expires_at = now
        .timestamp_micros()
        .saturating_add(workflow_claim_timeout_micros());

    for _ in 0..MAX_CAS_RETRIES {
        let current = kv.get(&key).await?;
        let Some(current_bytes) = current.as_ref() else {
            return Ok(None);
        };
        let mut run = models::WorkflowRun::decode(current_bytes.as_slice())?;
        if is_terminal(&run.status) {
            return Ok(None);
        }
        if run
            .claim_expires_at
            .is_some_and(|expires| expires > now.timestamp_micros())
            && run.status == STATUS_RUNNING
        {
            return Err(WorkflowClaimInProgressError.into());
        }
        run.status = STATUS_RUNNING.to_string();
        run.updated_at = now.timestamp_micros();
        run.claim_expires_at = Some(claim_expires_at);
        run.claim_owner = workflow_claim_owner();
        run.claim_attempt = run.claim_attempt.saturating_add(1);
        run.last_dispatch_reason = reason.to_string();
        let updated = run.encode_to_vec();
        if kv
            .compare_and_swap(&key, Some(current_bytes.as_slice()), &updated)
            .await?
        {
            return Ok(Some(run));
        }
    }
    Err(anyhow!("failed to atomically claim workflow run"))
}

pub async fn advance_run(cp: &ControlPlane, mut run: models::WorkflowRun) -> Result<()> {
    let spec = load_run_spec(cp.kv.as_ref(), &run).await?;
    let mut step_runs = load_step_runs(cp.kv.as_ref(), &run).await?;

    let mut made_progress = false;
    let mut waiting = false;
    let mut suspended = false;
    let mut should_redispatch = false;
    let concurrency = workflow_concurrency(&spec);

    loop {
        let mut progressed_this_round = false;
        let mut active_steps = active_step_count(&step_runs);

        for step in &spec.steps {
            if step_runs.contains_key(&step.id) {
                let status = &step_runs[&step.id].status;
                if status == STATUS_STARTING {
                    if retry_abandoned_starting_step(cp, &run, step, &mut step_runs).await? {
                        progressed_this_round = true;
                        made_progress = true;
                        should_redispatch = true;
                    } else {
                        waiting = true;
                    }
                } else if status == STATUS_WAITING_RETRY {
                    if try_retry_step(cp, &run, step, &mut step_runs).await? {
                        progressed_this_round = true;
                        made_progress = true;
                        should_redispatch = true;
                    } else {
                        waiting = true;
                    }
                } else if status == STATUS_WAITING_CHILD_SESSION {
                    if try_complete_agent_step(cp, &run, step, &mut step_runs).await? {
                        progressed_this_round = true;
                        made_progress = true;
                        should_redispatch = true;
                    } else {
                        waiting = true;
                    }
                } else if status == STATUS_WAITING_CHILD_WORKFLOW {
                    if try_complete_workflow_step(cp, &run, step, &mut step_runs).await? {
                        progressed_this_round = true;
                        made_progress = true;
                        should_redispatch = true;
                    } else {
                        waiting = true;
                    }
                } else if status == STATUS_SUSPENDED {
                    if try_complete_resumed_step(cp, &run, step, &mut step_runs).await? {
                        progressed_this_round = true;
                        made_progress = true;
                        should_redispatch = true;
                    } else {
                        suspended = true;
                    }
                }
                continue;
            }

            if !dependencies_done(step, &step_runs) {
                continue;
            }

            let view = run_view(&run, &step_runs)?;
            if !eval_when(&step.when_json, &view)? {
                let step_run = new_step_run(step, STATUS_SKIPPED, "", Value::Null)?;
                persist_step_run(cp, &run, &step_run).await?;
                append_run_event(
                    cp,
                    &run,
                    &step.id,
                    "step_skipped",
                    "step condition evaluated false",
                    Value::Null,
                )
                .await?;
                step_runs.insert(step.id.clone(), step_run);
                progressed_this_round = true;
                made_progress = true;
                should_redispatch = true;
                continue;
            }

            if active_steps >= concurrency {
                waiting = true;
                continue;
            }

            append_run_event(
                cp,
                &run,
                &step.id,
                "step_started",
                "step started",
                Value::Null,
            )
            .await?;
            let starter = new_step_run(step, STATUS_STARTING, "", Value::Null)?;
            if !try_insert_step_run(cp, &run, &starter).await? {
                step_runs = load_step_runs(cp.kv.as_ref(), &run).await?;
                progressed_this_round = true;
                continue;
            }
            step_runs.insert(step.id.clone(), starter);
            let outcome = match start_step(cp, &run, step, &view, 1).await {
                Ok(outcome) => outcome,
                Err(err) => StepStartOutcome::Completed(failed_step(step, &err.to_string())),
            };
            match outcome {
                StepStartOutcome::Completed(mut step_run) => {
                    step_run = apply_failed_retry_policy(cp, &run, step, step_run).await?;
                    persist_step_run(cp, &run, &step_run).await?;
                    if step_run.status == STATUS_FAILED {
                        let error = step_run.error.clone();
                        append_run_event(
                            cp,
                            &run,
                            &step.id,
                            "step_failed",
                            &error,
                            json!({ "error": error }),
                        )
                        .await?;
                    } else if step_run.status == STATUS_WAITING_RETRY {
                        waiting = true;
                    } else {
                        append_run_event(
                            cp,
                            &run,
                            &step.id,
                            "step_completed",
                            "step completed",
                            parse_json_or(&step_run.output_json, Value::Null)?,
                        )
                        .await?;
                    }
                    step_runs.insert(step.id.clone(), step_run);
                    progressed_this_round = true;
                    made_progress = true;
                    should_redispatch = true;
                }
                StepStartOutcome::Waiting(step_run) => {
                    persist_step_run(cp, &run, &step_run).await?;
                    step_runs.insert(step.id.clone(), step_run);
                    waiting = true;
                    made_progress = true;
                    active_steps = active_steps.saturating_add(1);
                }
                StepStartOutcome::Suspended(step_run) => {
                    persist_step_run(cp, &run, &step_run).await?;
                    append_run_event(
                        cp,
                        &run,
                        &step.id,
                        "run_suspended",
                        "workflow run suspended",
                        parse_json_or(&step_run.suspend_json, Value::Null)?,
                    )
                    .await?;
                    step_runs.insert(step.id.clone(), step_run);
                    suspended = true;
                    made_progress = true;
                    active_steps = active_steps.saturating_add(1);
                }
            }
        }

        if !progressed_this_round {
            break;
        }
    }

    if any_failed(&step_runs) {
        run.status = STATUS_FAILED.to_string();
        run.error = first_error(&step_runs);
        append_run_event(
            cp,
            &run,
            "",
            "run_failed",
            "workflow run failed",
            json!({ "error": run.error }),
        )
        .await?;
    } else if all_terminal(&spec, &step_runs) {
        let view = run_view(&run, &step_runs)?;
        let output_template = parse_json_or(&spec.output_json, Value::Object(Map::new()))?;
        let output = render_value(&output_template, &view)?;
        validate_basic_json_schema("output", &spec.output_schema_json, &output)?;
        run.output_json = serde_json::to_string(&output)?;
        run.status = STATUS_COMPLETED.to_string();
        append_run_event(
            cp,
            &run,
            "",
            "run_completed",
            "workflow run completed",
            output,
        )
        .await?;
    } else if suspended {
        run.status = STATUS_SUSPENDED.to_string();
    } else if waiting {
        run.status = STATUS_WAITING_CHILDREN.to_string();
    } else {
        run.status = STATUS_FAILED.to_string();
        run.error = "workflow made no progress and has pending steps".to_string();
        append_run_event(
            cp,
            &run,
            "",
            "run_failed",
            &run.error,
            json!({ "error": run.error }),
        )
        .await?;
    }

    run.claim_expires_at = None;
    run.updated_at = Utc::now().timestamp_micros();
    persist_run(cp.kv.as_ref(), &run).await?;

    if is_terminal(&run.status) {
        dispatch_parent_workflow_from_run_labels(cp, &run).await?;
    }

    if made_progress && should_redispatch && has_ready_work(&spec, &step_runs)? {
        dispatch_workflow(
            cp.pubsub.as_ref(),
            &run.ns,
            &run.workflow,
            &run.id,
            "progress",
        )
        .await?;
    }

    Ok(())
}

enum StepStartOutcome {
    Completed(models::WorkflowStepRun),
    Waiting(models::WorkflowStepRun),
    Suspended(models::WorkflowStepRun),
}

async fn start_step(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step: &models::WorkflowStep,
    view: &Value,
    attempt: u32,
) -> Result<StepStartOutcome> {
    let input_template = parse_json_or(&step.input_json, Value::Null)?;
    let rendered_input = render_value(&input_template, view)?;
    match step.r#type.as_str() {
        "transform" => {
            let output = if rendered_input.is_null() {
                Value::Object(Map::new())
            } else {
                rendered_input.clone()
            };
            let mut step_run = new_step_run(
                step,
                STATUS_COMPLETED,
                &serde_json::to_string(&rendered_input)?,
                output,
            )?;
            step_run.attempt = attempt;
            Ok(StepStartOutcome::Completed(step_run))
        }
        "tool" => execute_tool_step(cp, run, step, rendered_input, attempt).await,
        "pause" | "wait" => {
            let mut step_run = new_step_run(
                step,
                STATUS_SUSPENDED,
                &serde_json::to_string(&rendered_input)?,
                Value::Null,
            )?;
            let prompt = render_template(&step.prompt, view)?;
            step_run.suspend_json = serde_json::to_string(&json!({
                "prompt": prompt,
                "input": rendered_input,
            }))?;
            step_run.attempt = attempt;
            if step.r#type == "wait" {
                if let Some(wait_until_at) = compute_wait_until_at(step)? {
                    step_run.wait_until_at = Some(wait_until_at);
                    let wakeup =
                        schedule_workflow_wakeup(cp, run, step, attempt, wait_until_at, "wait")
                            .await?;
                    step_run.wait_wakeup_handle = wakeup.handle.unwrap_or_default();
                    let mut suspend =
                        parse_json_or(&step_run.suspend_json, Value::Object(Map::new()))?;
                    if let Some(object) = suspend.as_object_mut() {
                        object.insert("until".to_string(), json!(wait_until_at));
                    }
                    step_run.suspend_json = serde_json::to_string(&suspend)?;
                }
            }
            Ok(StepStartOutcome::Suspended(step_run))
        }
        "agent" => start_agent_step(cp, run, step, view, attempt).await,
        "workflow" => start_child_workflow_step(cp, run, step, rendered_input, attempt).await,
        other => {
            let mut step_run = new_step_run(step, STATUS_FAILED, "", Value::Null)?;
            step_run.error = format!("unsupported workflow step type '{}'", other);
            Ok(StepStartOutcome::Completed(step_run))
        }
    }
}

async fn execute_tool_step(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step: &models::WorkflowStep,
    rendered_input: Value,
    attempt: u32,
) -> Result<StepStartOutcome> {
    let book = KvKnowledgeBook::new(cp.kv.clone());
    let result =
        crate::knowledge::execute_tool(&book, &run.ns, &step.tool, &rendered_input).await?;
    let Some(output) = result else {
        let mut step_run = new_step_run(
            step,
            STATUS_FAILED,
            &serde_json::to_string(&rendered_input)?,
            Value::Null,
        )?;
        step_run.attempt = attempt;
        step_run.error = format!("workflow tool '{}' is not supported", step.tool);
        return Ok(StepStartOutcome::Completed(step_run));
    };
    let output = apply_output_policy(step, &output)?;
    let mut step_run = new_step_run(
        step,
        STATUS_COMPLETED,
        &serde_json::to_string(&rendered_input)?,
        output,
    )?;
    step_run.attempt = attempt;
    Ok(StepStartOutcome::Completed(step_run))
}

async fn start_agent_step(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step: &models::WorkflowStep,
    view: &Value,
    attempt: u32,
) -> Result<StepStartOutcome> {
    if step.agent.trim().is_empty() {
        let mut step_run = new_step_run(step, STATUS_FAILED, "", Value::Null)?;
        step_run.error = "agent step requires agent".to_string();
        return Ok(StepStartOutcome::Completed(step_run));
    }
    let prompt = render_template(&step.prompt, view)?;
    let mut labels = HashMap::new();
    labels.insert(LABEL_WORKFLOW.to_string(), run.workflow.clone());
    labels.insert(LABEL_WORKFLOW_RUN.to_string(), run.id.clone());
    labels.insert(LABEL_WORKFLOW_STEP.to_string(), step.id.clone());
    labels.insert(LABEL_WORKFLOW_ATTEMPT.to_string(), attempt.to_string());
    let session_id =
        crate::scheduling::create_session_with_labels(cp, &run.ns, &step.agent, labels).await?;
    crate::scheduling::send_message(
        cp.kv.as_ref(),
        cp.pubsub.as_ref(),
        &run.ns,
        &step.agent,
        &session_id,
        &prompt,
        HashMap::new(),
        Utc::now(),
    )
    .await?;

    let mut step_run = new_step_run(step, STATUS_WAITING_CHILD_SESSION, "", Value::Null)?;
    step_run.attempt = attempt;
    step_run.child_session_id = session_id;
    apply_waiting_step_metadata(step, &mut step_run)?;
    Ok(StepStartOutcome::Waiting(step_run))
}

async fn start_child_workflow_step(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step: &models::WorkflowStep,
    rendered_input: Value,
    attempt: u32,
) -> Result<StepStartOutcome> {
    let workflow_name = if step.workflow.is_empty() {
        return Ok(StepStartOutcome::Completed(failed_step(
            step,
            "workflow step requires workflow",
        )));
    } else {
        step.workflow.clone()
    };
    let child = cp
        .kv
        .get_msg::<models::Workflow>(&keys::workflow(&run.ns, &workflow_name))
        .await?
        .ok_or_else(|| anyhow!("child workflow '{}' not found", workflow_name))?;
    let mut labels = HashMap::new();
    labels.insert(LABEL_PARENT_WORKFLOW.to_string(), run.workflow.clone());
    labels.insert(LABEL_PARENT_WORKFLOW_RUN.to_string(), run.id.clone());
    labels.insert(LABEL_PARENT_WORKFLOW_STEP.to_string(), step.id.clone());
    let child_run = create_run(cp, &child, serde_json::to_string(&rendered_input)?, labels).await?;
    let mut step_run = new_step_run(
        step,
        STATUS_WAITING_CHILD_WORKFLOW,
        &serde_json::to_string(&rendered_input)?,
        Value::Null,
    )?;
    step_run.attempt = attempt;
    step_run.child_workflow_run_id = child_run.id;
    apply_waiting_step_metadata(step, &mut step_run)?;
    Ok(StepStartOutcome::Waiting(step_run))
}

async fn try_complete_agent_step(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step: &models::WorkflowStep,
    step_runs: &mut HashMap<String, models::WorkflowStepRun>,
) -> Result<bool> {
    let Some(current) = step_runs.get(&step.id).cloned() else {
        return Ok(false);
    };
    if let Some(failed) = timed_out_step(cp, run, step, current.clone()).await? {
        persist_step_run(cp, run, &failed).await?;
        step_runs.insert(step.id.clone(), failed);
        return Ok(true);
    }
    let Some(session) = cp
        .kv
        .get_msg::<models::Session>(&keys::session(
            &run.ns,
            &step.agent,
            &current.child_session_id,
        ))
        .await?
    else {
        return Ok(false);
    };
    if session.status == "PROCESSING" {
        return Ok(false);
    }
    if session.status == "ERROR" {
        let mut failed = current;
        failed.status = STATUS_FAILED.to_string();
        failed.error = format!("child session '{}' failed", session.id);
        failed.updated_at = Utc::now().timestamp_micros();
        let failed = apply_failed_retry_policy(cp, run, step, failed).await?;
        persist_step_run(cp, run, &failed).await?;
        if failed.status == STATUS_WAITING_RETRY {
            append_run_event(
                cp,
                run,
                &step.id,
                "step_retry_scheduled",
                &failed.error,
                json!({ "error": failed.error, "nextRetryAt": failed.next_retry_at }),
            )
            .await?;
        } else {
            append_run_event(
                cp,
                run,
                &step.id,
                "step_failed",
                &failed.error,
                json!({ "error": failed.error }),
            )
            .await?;
        }
        step_runs.insert(step.id.clone(), failed);
        return Ok(true);
    }
    let text = latest_assistant_text(cp.kv.as_ref(), &run.ns, &step.agent, &session.id).await?;
    let output = match apply_output_policy(step, &text) {
        Ok(output) => output,
        Err(err) => {
            let mut failed = current;
            failed.status = STATUS_FAILED.to_string();
            failed.error = err.to_string();
            failed.updated_at = Utc::now().timestamp_micros();
            let failed = apply_failed_retry_policy(cp, run, step, failed).await?;
            persist_step_run(cp, run, &failed).await?;
            if failed.status == STATUS_WAITING_RETRY {
                append_run_event(
                    cp,
                    run,
                    &step.id,
                    "step_retry_scheduled",
                    &failed.error,
                    json!({ "error": failed.error, "nextRetryAt": failed.next_retry_at }),
                )
                .await?;
            } else {
                append_run_event(
                    cp,
                    run,
                    &step.id,
                    "step_failed",
                    &failed.error,
                    json!({ "error": failed.error }),
                )
                .await?;
            }
            step_runs.insert(step.id.clone(), failed);
            return Ok(true);
        }
    };
    let mut completed = current;
    completed.status = STATUS_COMPLETED.to_string();
    completed.output_json = serde_json::to_string(&output)?;
    completed.updated_at = Utc::now().timestamp_micros();
    persist_step_run(cp, run, &completed).await?;
    append_run_event(
        cp,
        run,
        &step.id,
        "step_completed",
        "agent step completed",
        output,
    )
    .await?;
    step_runs.insert(step.id.clone(), completed);
    Ok(true)
}

async fn try_complete_workflow_step(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step: &models::WorkflowStep,
    step_runs: &mut HashMap<String, models::WorkflowStepRun>,
) -> Result<bool> {
    let Some(current) = step_runs.get(&step.id).cloned() else {
        return Ok(false);
    };
    if let Some(failed) = timed_out_step(cp, run, step, current.clone()).await? {
        persist_step_run(cp, run, &failed).await?;
        step_runs.insert(step.id.clone(), failed);
        return Ok(true);
    }
    let Some(child) = cp
        .kv
        .get_msg::<models::WorkflowRun>(&keys::workflow_run(
            &run.ns,
            &step.workflow,
            &current.child_workflow_run_id,
        ))
        .await?
    else {
        return Ok(false);
    };
    if child.status == STATUS_COMPLETED {
        let output = parse_json_or(&child.output_json, Value::Null)?;
        let mut completed = current;
        completed.status = STATUS_COMPLETED.to_string();
        completed.output_json = serde_json::to_string(&output)?;
        completed.updated_at = Utc::now().timestamp_micros();
        persist_step_run(cp, run, &completed).await?;
        append_run_event(
            cp,
            run,
            &step.id,
            "step_completed",
            "child workflow step completed",
            output,
        )
        .await?;
        step_runs.insert(step.id.clone(), completed);
        return Ok(true);
    }
    if child.status == STATUS_FAILED || child.status == STATUS_CANCELLED {
        let mut failed = current;
        failed.status = STATUS_FAILED.to_string();
        failed.error = format!("child workflow ended with status {}", child.status);
        failed.updated_at = Utc::now().timestamp_micros();
        persist_step_run(cp, run, &failed).await?;
        step_runs.insert(step.id.clone(), failed);
        return Ok(true);
    }
    Ok(false)
}

async fn try_complete_resumed_step(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step: &models::WorkflowStep,
    step_runs: &mut HashMap<String, models::WorkflowStepRun>,
) -> Result<bool> {
    let Some(current) = step_runs.get(&step.id).cloned() else {
        return Ok(false);
    };
    if step.r#type == "wait" {
        if let Some(wait_until_at) = current.wait_until_at {
            if wait_until_at <= Utc::now().timestamp_micros() {
                let output = json!({ "firedAt": wait_until_at });
                let mut completed = current;
                completed.status = STATUS_COMPLETED.to_string();
                completed.output_json = serde_json::to_string(&output)?;
                completed.updated_at = Utc::now().timestamp_micros();
                persist_step_run(cp, run, &completed).await?;
                append_run_event(
                    cp,
                    run,
                    &step.id,
                    "step_completed",
                    "wait step completed",
                    output,
                )
                .await?;
                step_runs.insert(step.id.clone(), completed);
                return Ok(true);
            }
        }
    }
    if current.resume_json.is_empty() {
        return Ok(false);
    }
    let resume = parse_json_or(&current.resume_json, Value::Null)?;
    validate_basic_json_schema("resume", &step.resume_schema_json, &resume)?;
    let mut completed = current;
    completed.status = STATUS_COMPLETED.to_string();
    completed.output_json = serde_json::to_string(&resume)?;
    completed.updated_at = Utc::now().timestamp_micros();
    persist_step_run(cp, run, &completed).await?;
    append_run_event(
        cp,
        run,
        &step.id,
        "step_completed",
        "resumed step completed",
        resume,
    )
    .await?;
    step_runs.insert(step.id.clone(), completed);
    Ok(true)
}

pub async fn resume_run(
    cp: &ControlPlane,
    ns: &str,
    workflow: &str,
    run_id: &str,
    step_id: &str,
    resume_json: &str,
) -> Result<models::WorkflowRun> {
    let run = cp
        .kv
        .get_msg::<models::WorkflowRun>(&keys::workflow_run(ns, workflow, run_id))
        .await?
        .ok_or_else(|| WorkflowNotFoundError::new("workflow run not found"))?;
    let workflow_model = cp
        .kv
        .get_msg::<models::Workflow>(&keys::workflow(ns, workflow))
        .await?
        .ok_or_else(|| WorkflowNotFoundError::new("workflow not found"))?;
    let step = workflow_model
        .spec
        .as_ref()
        .and_then(|spec| spec.steps.iter().find(|step| step.id == step_id))
        .ok_or_else(|| {
            WorkflowNotFoundError::new(format!("workflow step '{}' not found", step_id))
        })?;
    let resume = parse_json_or(resume_json, Value::Null).map_err(|err| {
        WorkflowInvalidArgumentError::new(format!("resume must be valid JSON: {err}"))
    })?;
    validate_basic_json_schema("resume", &step.resume_schema_json, &resume)
        .map_err(|err| WorkflowInvalidArgumentError::new(err.to_string()))?;

    let mut step_run = cp
        .kv
        .get_msg::<models::WorkflowStepRun>(&keys::workflow_step_run(ns, workflow, run_id, step_id))
        .await?
        .ok_or_else(|| {
            WorkflowNotFoundError::new(format!("workflow step run '{}' not found", step_id))
        })?;
    if step_run.status != STATUS_SUSPENDED {
        return Err(WorkflowInvalidArgumentError::new(format!(
            "workflow step '{}' is not suspended",
            step_id
        ))
        .into());
    }
    step_run.resume_json = serde_json::to_string(&resume)?;
    step_run.updated_at = Utc::now().timestamp_micros();
    persist_step_run(cp, &run, &step_run).await?;
    append_run_event(
        cp,
        &run,
        step_id,
        "run_resumed",
        "workflow run resumed",
        resume,
    )
    .await?;
    dispatch_workflow(cp.pubsub.as_ref(), ns, workflow, run_id, "resumed").await?;
    Ok(run)
}

pub async fn cancel_run(
    cp: &ControlPlane,
    ns: &str,
    workflow: &str,
    run_id: &str,
) -> Result<models::WorkflowRun> {
    let mut run = cp
        .kv
        .get_msg::<models::WorkflowRun>(&keys::workflow_run(ns, workflow, run_id))
        .await?
        .ok_or_else(|| WorkflowNotFoundError::new("workflow run not found"))?;
    let step_runs = load_step_runs(cp.kv.as_ref(), &run).await?;
    let spec = load_run_spec(cp.kv.as_ref(), &run).await.ok();
    run.status = STATUS_CANCELLED.to_string();
    run.claim_expires_at = None;
    run.updated_at = Utc::now().timestamp_micros();
    persist_run(cp.kv.as_ref(), &run).await?;
    for step_run in step_runs.values() {
        if step_run.child_workflow_run_id.is_empty() {
            continue;
        }
        let Some(child_workflow) = spec
            .as_ref()
            .and_then(|spec| spec.steps.iter().find(|step| step.id == step_run.step_id))
            .map(|step| step.workflow.as_str())
            .filter(|workflow| !workflow.is_empty())
        else {
            continue;
        };
        if let Some(mut child) = cp
            .kv
            .get_msg::<models::WorkflowRun>(&keys::workflow_run(
                ns,
                child_workflow,
                &step_run.child_workflow_run_id,
            ))
            .await?
        {
            child.status = STATUS_CANCELLED.to_string();
            child.claim_expires_at = None;
            child.updated_at = Utc::now().timestamp_micros();
            persist_run(cp.kv.as_ref(), &child).await?;
            append_run_event(
                cp,
                &child,
                "",
                "run_cancelled",
                "workflow run cancelled by parent",
                Value::Null,
            )
            .await?;
        }
    }
    append_run_event(
        cp,
        &run,
        "",
        "run_cancelled",
        "workflow run cancelled",
        Value::Null,
    )
    .await?;
    Ok(run)
}

pub async fn persist_run(kv: &dyn KeyValueStore, run: &models::WorkflowRun) -> Result<()> {
    kv.set_msg(&keys::workflow_run(&run.ns, &run.workflow, &run.id), run)
        .await
}

pub async fn load_step_runs(
    kv: &dyn KeyValueStore,
    run: &models::WorkflowRun,
) -> Result<HashMap<String, models::WorkflowStepRun>> {
    let mut map = HashMap::new();
    for (_, bytes) in kv
        .list_entries(&keys::workflow_step_run_prefix(
            &run.ns,
            &run.workflow,
            &run.id,
        ))
        .await?
    {
        let step_run = models::WorkflowStepRun::decode(bytes.as_slice())?;
        map.insert(step_run.step_id.clone(), step_run);
    }
    Ok(map)
}

async fn load_run_spec(
    kv: &dyn KeyValueStore,
    run: &models::WorkflowRun,
) -> Result<models::WorkflowSpec> {
    if !run.spec_json.trim().is_empty() {
        return serde_json::from_str(&run.spec_json)
            .map_err(|err| anyhow!("workflow run spec snapshot is invalid: {}", err));
    }
    let workflow = kv
        .get_msg::<models::Workflow>(&keys::workflow(&run.ns, &run.workflow))
        .await?
        .ok_or_else(|| anyhow!("workflow '{}' not found", run.workflow))?;
    workflow
        .spec
        .ok_or_else(|| anyhow!("workflow spec is required"))
}

fn workflow_claim_owner() -> String {
    std::env::var("TALON_WORKER_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("worker-{}", uuid::Uuid::now_v7()))
}

fn workflow_concurrency(spec: &models::WorkflowSpec) -> usize {
    if spec.concurrency == 0 {
        usize::MAX
    } else {
        spec.concurrency as usize
    }
}

fn active_step_count(step_runs: &HashMap<String, models::WorkflowStepRun>) -> usize {
    step_runs
        .values()
        .filter(|run| {
            matches!(
                run.status.as_str(),
                STATUS_STARTING
                    | STATUS_WAITING_CHILD_SESSION
                    | STATUS_WAITING_CHILD_WORKFLOW
                    | STATUS_WAITING_RETRY
                    | STATUS_SUSPENDED
            )
        })
        .count()
}

fn has_ready_work(
    spec: &models::WorkflowSpec,
    step_runs: &HashMap<String, models::WorkflowStepRun>,
) -> Result<bool> {
    let now = Utc::now().timestamp_micros();
    if step_runs.values().any(|step| {
        (step.status == STATUS_WAITING_RETRY
            && step.next_retry_at.is_some_and(|retry_at| retry_at <= now))
            || (step.status == STATUS_STARTING
                && step
                    .updated_at
                    .saturating_add(workflow_claim_timeout_micros())
                    <= now)
    }) {
        return Ok(true);
    }
    for step in &spec.steps {
        if step_runs.contains_key(&step.id) {
            continue;
        }
        if dependencies_done(step, step_runs) {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn try_insert_step_run(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step_run: &models::WorkflowStepRun,
) -> Result<bool> {
    let key = keys::workflow_step_run(&run.ns, &run.workflow, &run.id, &step_run.id);
    cp.kv
        .compare_and_swap(&key, None, &step_run.encode_to_vec())
        .await
}

fn apply_waiting_step_metadata(
    step: &models::WorkflowStep,
    step_run: &mut models::WorkflowStepRun,
) -> Result<()> {
    if let Some(seconds) = parse_duration_seconds(&step.timeout)? {
        step_run.timeout_at = Some(
            Utc::now()
                .timestamp_micros()
                .saturating_add(seconds.saturating_mul(1_000_000)),
        );
    }
    Ok(())
}

async fn timed_out_step(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step: &models::WorkflowStep,
    mut step_run: models::WorkflowStepRun,
) -> Result<Option<models::WorkflowStepRun>> {
    let Some(timeout_at) = step_run.timeout_at else {
        return Ok(None);
    };
    if timeout_at > Utc::now().timestamp_micros() {
        return Ok(None);
    }
    step_run.status = STATUS_FAILED.to_string();
    step_run.error = format!("workflow step '{}' timed out", step.id);
    step_run.updated_at = Utc::now().timestamp_micros();
    Ok(Some(
        apply_failed_retry_policy(cp, run, step, step_run).await?,
    ))
}

async fn retry_abandoned_starting_step(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step: &models::WorkflowStep,
    step_runs: &mut HashMap<String, models::WorkflowStepRun>,
) -> Result<bool> {
    let Some(current) = step_runs.get(&step.id).cloned() else {
        return Ok(false);
    };
    let stale_after = workflow_claim_timeout_micros();
    if current.updated_at.saturating_add(stale_after) > Utc::now().timestamp_micros() {
        return Ok(false);
    }
    let mut failed = current;
    failed.status = STATUS_FAILED.to_string();
    failed.error = format!("workflow step '{}' was abandoned while starting", step.id);
    failed.updated_at = Utc::now().timestamp_micros();
    let failed = apply_failed_retry_policy(cp, run, step, failed).await?;
    persist_step_run(cp, run, &failed).await?;
    step_runs.insert(step.id.clone(), failed);
    Ok(true)
}

async fn try_retry_step(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step: &models::WorkflowStep,
    step_runs: &mut HashMap<String, models::WorkflowStepRun>,
) -> Result<bool> {
    let Some(current) = step_runs.get(&step.id).cloned() else {
        return Ok(false);
    };
    if current
        .next_retry_at
        .is_some_and(|retry_at| retry_at > Utc::now().timestamp_micros())
    {
        return Ok(false);
    }
    let view = run_view(run, step_runs)?;
    append_run_event(
        cp,
        run,
        &step.id,
        "step_started",
        "retry step started",
        Value::Null,
    )
    .await?;
    let attempt = current.attempt.saturating_add(1);
    let outcome = match start_step(cp, run, step, &view, attempt).await {
        Ok(outcome) => outcome,
        Err(err) => {
            StepStartOutcome::Completed(failed_step_with_attempt(step, &err.to_string(), attempt))
        }
    };
    let mut step_run = match outcome {
        StepStartOutcome::Completed(step_run)
        | StepStartOutcome::Waiting(step_run)
        | StepStartOutcome::Suspended(step_run) => step_run,
    };
    if step_run.status == STATUS_FAILED {
        step_run = apply_failed_retry_policy(cp, run, step, step_run).await?;
    }
    persist_step_run(cp, run, &step_run).await?;
    step_runs.insert(step.id.clone(), step_run);
    Ok(true)
}

async fn apply_failed_retry_policy(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step: &models::WorkflowStep,
    mut step_run: models::WorkflowStepRun,
) -> Result<models::WorkflowStepRun> {
    if step_run.status != STATUS_FAILED {
        return Ok(step_run);
    }
    let Some(retry) = &step.retry else {
        return Ok(step_run);
    };
    let max_attempts = retry.max_attempts.max(1);
    if step_run.attempt >= max_attempts {
        return Ok(step_run);
    }
    let backoff_seconds = retry_backoff_seconds(retry, step_run.attempt);
    let next_retry_at = Utc::now()
        .timestamp_micros()
        .saturating_add(backoff_seconds.saturating_mul(1_000_000));
    step_run.status = STATUS_WAITING_RETRY.to_string();
    step_run.next_retry_at = Some(next_retry_at);
    step_run.updated_at = Utc::now().timestamp_micros();
    let wakeup =
        schedule_workflow_wakeup(cp, run, step, step_run.attempt, next_retry_at, "retry").await?;
    step_run.wait_wakeup_handle = wakeup.handle.unwrap_or_default();
    append_run_event(
        cp,
        run,
        &step.id,
        "step_retry_scheduled",
        &step_run.error,
        json!({ "attempt": step_run.attempt, "nextRetryAt": next_retry_at }),
    )
    .await?;
    Ok(step_run)
}

fn retry_backoff_seconds(retry: &models::WorkflowStepRetryPolicy, attempt: u32) -> i64 {
    let initial = retry.initial_backoff_seconds.max(1);
    let max = retry.max_backoff_seconds.max(initial);
    let multiplier = if retry.multiplier > 0.0 {
        retry.multiplier
    } else {
        2.0
    };
    let exponent = attempt.saturating_sub(1) as i32;
    let value = (initial as f64 * multiplier.powi(exponent)).ceil() as i64;
    value.clamp(1, max)
}

async fn schedule_workflow_wakeup(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step: &models::WorkflowStep,
    attempt: u32,
    fire_at_micros: i64,
    reason: &str,
) -> Result<crate::control::scheduler::ScheduledWakeup> {
    let fire_at = DateTime::from_timestamp_micros(fire_at_micros)
        .ok_or_else(|| anyhow!("invalid workflow wakeup timestamp {}", fire_at_micros))?;
    let payload = WorkflowWakeupPayload {
        namespace: run.ns.clone(),
        workflow: run.workflow.clone(),
        run_id: run.id.clone(),
        step_id: step.id.clone(),
        attempt,
        intended_fire_at: fire_at_micros,
        reason: reason.to_string(),
    };
    cp.scheduler
        .schedule(crate::control::scheduler::ScheduleWakeupRequest {
            namespace: run.ns.clone(),
            schedule_id: format!("workflow/{}/{}/{}", run.workflow, run.id, step.id),
            revision: attempt as u64,
            fire_at,
            payload: serde_json::to_vec(&crate::scheduling::SchedulerFirePayload::Workflow(
                payload,
            ))?,
        })
        .await
}

fn compute_wait_until_at(step: &models::WorkflowStep) -> Result<Option<i64>> {
    if !step.wait_until.trim().is_empty() {
        return Ok(Some(parse_timestamp_micros(&step.wait_until)?));
    }
    let Some(seconds) = parse_duration_seconds(&step.wait_duration)? else {
        return Ok(None);
    };
    Ok(Some(
        Utc::now()
            .timestamp_micros()
            .saturating_add(seconds.saturating_mul(1_000_000)),
    ))
}

pub async fn handle_workflow_wakeup(
    cp: &ControlPlane,
    payload: WorkflowWakeupPayload,
) -> Result<()> {
    let Some(step_run) = cp
        .kv
        .get_msg::<models::WorkflowStepRun>(&keys::workflow_step_run(
            &payload.namespace,
            &payload.workflow,
            &payload.run_id,
            &payload.step_id,
        ))
        .await?
    else {
        return Ok(());
    };
    if step_run.attempt != payload.attempt {
        return Ok(());
    }
    dispatch_workflow(
        cp.pubsub.as_ref(),
        &payload.namespace,
        &payload.workflow,
        &payload.run_id,
        &payload.reason,
    )
    .await
}

async fn persist_step_run(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step_run: &models::WorkflowStepRun,
) -> Result<()> {
    cp.kv
        .set_msg(
            &keys::workflow_step_run(&run.ns, &run.workflow, &run.id, &step_run.id),
            step_run,
        )
        .await
}

pub async fn append_run_event(
    cp: &ControlPlane,
    run: &models::WorkflowRun,
    step_id: &str,
    event_type: &str,
    message: &str,
    payload: Value,
) -> Result<models::WorkflowRunEvent> {
    let event = models::WorkflowRunEvent {
        id: uuid::Uuid::now_v7().to_string(),
        ns: run.ns.clone(),
        workflow: run.workflow.clone(),
        run_id: run.id.clone(),
        r#type: event_type.to_string(),
        step_id: step_id.to_string(),
        message: message.to_string(),
        payload_json: serde_json::to_string(&payload)?,
        timestamp: Utc::now().timestamp_micros(),
    };
    cp.kv
        .set_msg(
            &keys::workflow_run_event(&run.ns, &run.workflow, &run.id, &event.id),
            &event,
        )
        .await?;
    cp.pubsub
        .publish(
            &topics::workflow_events_topic(&run.ns, &run.workflow, &run.id),
            &event.encode_to_vec(),
        )
        .await?;
    Ok(event)
}

fn new_step_run(
    step: &models::WorkflowStep,
    status: &str,
    input_json: &str,
    output: Value,
) -> Result<models::WorkflowStepRun> {
    let now = Utc::now().timestamp_micros();
    Ok(models::WorkflowStepRun {
        id: step.id.clone(),
        step_id: step.id.clone(),
        attempt: 1,
        status: status.to_string(),
        input_json: input_json.to_string(),
        output_json: serde_json::to_string(&output)?,
        error: String::new(),
        child_session_id: String::new(),
        child_workflow_run_id: String::new(),
        resume_json: String::new(),
        suspend_json: String::new(),
        created_at: now,
        updated_at: now,
        next_retry_at: None,
        timeout_at: None,
        wait_wakeup_handle: String::new(),
        wait_until_at: None,
    })
}

fn failed_step(step: &models::WorkflowStep, message: &str) -> models::WorkflowStepRun {
    failed_step_with_attempt(step, message, 1)
}

fn failed_step_with_attempt(
    step: &models::WorkflowStep,
    message: &str,
    attempt: u32,
) -> models::WorkflowStepRun {
    let mut step_run = new_step_run(step, STATUS_FAILED, "", Value::Null)
        .unwrap_or_else(|_| models::WorkflowStepRun::default());
    step_run.attempt = attempt;
    step_run.error = message.to_string();
    step_run
}

fn dependencies_done(
    step: &models::WorkflowStep,
    step_runs: &HashMap<String, models::WorkflowStepRun>,
) -> bool {
    step.after.iter().all(|dep| {
        step_runs
            .get(dep)
            .map(|run| run.status == STATUS_COMPLETED || run.status == STATUS_SKIPPED)
            .unwrap_or(false)
    })
}

fn all_terminal(
    spec: &models::WorkflowSpec,
    step_runs: &HashMap<String, models::WorkflowStepRun>,
) -> bool {
    spec.steps.iter().all(|step| {
        step_runs
            .get(&step.id)
            .map(|run| run.status == STATUS_COMPLETED || run.status == STATUS_SKIPPED)
            .unwrap_or(false)
    })
}

fn any_failed(step_runs: &HashMap<String, models::WorkflowStepRun>) -> bool {
    step_runs.values().any(|run| run.status == STATUS_FAILED)
}

fn first_error(step_runs: &HashMap<String, models::WorkflowStepRun>) -> String {
    step_runs
        .values()
        .find(|run| run.status == STATUS_FAILED)
        .map(|run| run.error.clone())
        .unwrap_or_default()
}

fn is_terminal(status: &str) -> bool {
    matches!(status, STATUS_COMPLETED | STATUS_FAILED | STATUS_CANCELLED)
}

fn run_view(
    run: &models::WorkflowRun,
    step_runs: &HashMap<String, models::WorkflowStepRun>,
) -> Result<Value> {
    let mut steps = Map::new();
    for (step_id, step_run) in step_runs {
        let mut step = Map::new();
        step.insert("status".to_string(), Value::String(step_run.status.clone()));
        step.insert(
            "output".to_string(),
            parse_json_or(&step_run.output_json, Value::Null)?,
        );
        step.insert(
            "resume".to_string(),
            parse_json_or(&step_run.resume_json, Value::Null)?,
        );
        step.insert(
            "suspend".to_string(),
            parse_json_or(&step_run.suspend_json, Value::Null)?,
        );
        steps.insert(step_id.clone(), Value::Object(step));
    }

    Ok(json!({
        "input": parse_json_or(&run.input_json, Value::Null)?,
        "state": parse_json_or(&run.state_json, json!({}))?,
        "steps": Value::Object(steps),
        "run": {
            "id": run.id,
            "workflow": run.workflow,
            "namespace": run.ns,
            "status": run.status,
        }
    }))
}

fn eval_when(when_json: &str, view: &Value) -> Result<bool> {
    if when_json.trim().is_empty() {
        return Ok(true);
    }
    let predicate = parse_json_or(when_json, Value::Null)?;
    eval_predicate(&predicate, view)
}

fn eval_predicate(predicate: &Value, view: &Value) -> Result<bool> {
    let Some(object) = predicate.as_object() else {
        return Ok(true);
    };
    if let Some(all) = object.get("all").and_then(Value::as_array) {
        return all
            .iter()
            .map(|p| eval_predicate(p, view))
            .try_fold(true, |acc, item| Ok(acc && item?));
    }
    if let Some(any) = object.get("any").and_then(Value::as_array) {
        return any
            .iter()
            .map(|p| eval_predicate(p, view))
            .try_fold(false, |acc, item| Ok(acc || item?));
    }
    if let Some(not) = object.get("not") {
        return Ok(!eval_predicate(not, view)?);
    }
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("predicate requires path"))?;
    let value = lookup_path(view, path);
    if let Some(exists) = object.get("exists").and_then(Value::as_bool) {
        return Ok(value.is_some() == exists);
    }
    let Some(current) = value else {
        return Ok(false);
    };
    if let Some(expected) = object.get("equals") {
        return Ok(current == expected);
    }
    if let Some(expected) = object.get("notEquals") {
        return Ok(current != expected);
    }
    if let Some(list) = object.get("in").and_then(Value::as_array) {
        return Ok(list.iter().any(|candidate| candidate == current));
    }
    if let Some(needle) = object.get("contains") {
        return Ok(match (current, needle) {
            (Value::String(haystack), Value::String(needle)) => haystack.contains(needle),
            (Value::Array(values), needle) => values.iter().any(|value| value == needle),
            _ => false,
        });
    }
    for key in ["gt", "gte", "lt", "lte"] {
        if let Some(expected) = object.get(key) {
            let left = current
                .as_f64()
                .ok_or_else(|| anyhow!("predicate {key} requires numeric path value"))?;
            let right = expected
                .as_f64()
                .ok_or_else(|| anyhow!("predicate {key} requires numeric expected value"))?;
            return Ok(match key {
                "gt" => left > right,
                "gte" => left >= right,
                "lt" => left < right,
                "lte" => left <= right,
                _ => unreachable!(),
            });
        }
    }
    Ok(true)
}

fn render_value(value: &Value, view: &Value) -> Result<Value> {
    match value {
        Value::String(s) => render_string_value(s, view),
        Value::Array(values) => values
            .iter()
            .map(|value| render_value(value, view))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| Ok((key.clone(), render_value(value, view)?)))
            .collect::<Result<Map<_, _>>>()
            .map(Value::Object),
        other => Ok(other.clone()),
    }
}

fn render_template(template: &str, view: &Value) -> Result<String> {
    match render_string_value(template, view)? {
        Value::String(text) => Ok(text),
        other => Ok(other.to_string()),
    }
}

fn render_string_value(template: &str, view: &Value) -> Result<Value> {
    if let Some(path) = whole_template_path(template) {
        return Ok(lookup_path(view, path).cloned().unwrap_or(Value::Null));
    }
    let mut output = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            bail!("unterminated template expression in '{}'", template);
        };
        let path = after[..end].trim();
        let value = lookup_path(view, path).cloned().unwrap_or(Value::Null);
        output.push_str(value_to_template_text(&value).as_str());
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    Ok(Value::String(output))
}

fn whole_template_path(template: &str) -> Option<&str> {
    let trimmed = template.trim();
    if trimmed.starts_with("${") && trimmed.ends_with('}') && trimmed.matches("${").count() == 1 {
        Some(trimmed[2..trimmed.len() - 1].trim())
    } else {
        None
    }
}

fn value_to_template_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn lookup_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let path = path.strip_prefix("$.")?;
    let mut current = root;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn apply_output_policy(step: &models::WorkflowStep, text: &str) -> Result<Value> {
    let format = step
        .output
        .as_ref()
        .map(|output| output.format.as_str())
        .filter(|format| !format.is_empty())
        .unwrap_or("text");
    let output = if format == "json" {
        serde_json::from_str::<Value>(text)
            .map_err(|err| anyhow!("step '{}' expected JSON output: {}", step.id, err))?
    } else {
        json!({ "text": text })
    };
    if let Some(policy) = &step.output {
        validate_basic_json_schema("step output", &policy.schema_json, &output)?;
    }
    Ok(output)
}

async fn latest_assistant_text(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
) -> Result<String> {
    let mut entries = kv
        .list_entries(&keys::session_message_prefix(ns, agent, session_id))
        .await?;
    entries.sort_by(|left, right| right.0.cmp(&left.0));
    for (_, bytes) in entries {
        let message = models::SessionMessage::decode(bytes.as_slice())?;
        if message.role == models::MessageRole::RoleAssistant as i32 {
            let text = message
                .parts
                .iter()
                .filter(|part| part.part_type == models::SessionMessagePartType::Text as i32)
                .map(|part| part.content.as_str())
                .collect::<String>();
            if !text.is_empty() {
                return Ok(text);
            }
        }
    }
    Ok(String::new())
}

fn parse_json_or(input: &str, default: Value) -> Result<Value> {
    if input.trim().is_empty() {
        return Ok(default);
    }
    Ok(serde_json::from_str(input)?)
}

fn validate_schema_json(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Ok(());
    }
    let parsed: Value =
        serde_json::from_str(value).with_context(|| format!("{label} must be valid JSON"))?;
    if !parsed.is_object() {
        bail!("{label} must be a JSON object");
    }
    Ok(())
}

fn validate_duration_field(label: &str, value: &str) -> Result<()> {
    parse_duration_seconds(value)
        .map(|_| ())
        .map_err(|err| anyhow!("{label} is invalid: {err}"))
}

fn validate_retry_policy(step_id: &str, retry: &models::WorkflowStepRetryPolicy) -> Result<()> {
    if retry.max_attempts == 0 {
        bail!(
            "retry.maxAttempts for step '{}' must be at least 1",
            step_id
        );
    }
    if retry.initial_backoff_seconds < 0 {
        bail!(
            "retry.initialBackoffSeconds for step '{}' must be non-negative",
            step_id
        );
    }
    if retry.max_backoff_seconds < 0 {
        bail!(
            "retry.maxBackoffSeconds for step '{}' must be non-negative",
            step_id
        );
    }
    if retry.multiplier < 0.0 {
        bail!(
            "retry.multiplier for step '{}' must be non-negative",
            step_id
        );
    }
    Ok(())
}

fn parse_timestamp_micros(value: &str) -> Result<i64> {
    let parsed = DateTime::parse_from_rfc3339(value.trim())?;
    Ok(parsed.with_timezone(&Utc).timestamp_micros())
}

fn parse_duration_seconds(value: &str) -> Result<Option<i64>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let (number, multiplier) = match value.chars().last() {
        Some('s') => (&value[..value.len() - 1], 1),
        Some('m') => (&value[..value.len() - 1], 60),
        Some('h') => (&value[..value.len() - 1], 60 * 60),
        Some('d') => (&value[..value.len() - 1], 60 * 60 * 24),
        Some(c) if c.is_ascii_digit() => (value, 1),
        _ => bail!("duration must use seconds, s, m, h, or d"),
    };
    let number = number
        .parse::<i64>()
        .map_err(|err| anyhow!("duration value must be an integer: {}", err))?;
    if number <= 0 {
        bail!("duration must be positive");
    }
    Ok(Some(number.saturating_mul(multiplier)))
}

fn validate_json_object(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Ok(());
    }
    let parsed: Value =
        serde_json::from_str(value).with_context(|| format!("{label} must be valid JSON"))?;
    if !parsed.is_object() {
        bail!("{label} must be a JSON object");
    }
    Ok(())
}

fn validate_json_value(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Ok(());
    }
    let _: Value =
        serde_json::from_str(value).with_context(|| format!("{label} must be valid JSON"))?;
    Ok(())
}

fn validate_predicate_json(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Ok(());
    }
    let predicate: Value =
        serde_json::from_str(value).with_context(|| format!("{label} must be valid JSON"))?;
    validate_predicate_shape(label, &predicate)
}

fn validate_predicate_shape(label: &str, predicate: &Value) -> Result<()> {
    let Some(object) = predicate.as_object() else {
        bail!("{label} must be an object");
    };
    if let Some(all) = object.get("all") {
        let items = all
            .as_array()
            .ok_or_else(|| anyhow!("{label}.all must be an array"))?;
        for item in items {
            validate_predicate_shape(label, item)?;
        }
        return Ok(());
    }
    if let Some(any) = object.get("any") {
        let items = any
            .as_array()
            .ok_or_else(|| anyhow!("{label}.any must be an array"))?;
        for item in items {
            validate_predicate_shape(label, item)?;
        }
        return Ok(());
    }
    if let Some(not) = object.get("not") {
        return validate_predicate_shape(label, not);
    }

    if !object.get("path").is_some_and(Value::is_string) {
        bail!("{label} predicate requires path");
    }
    let comparators = [
        "exists",
        "equals",
        "notEquals",
        "in",
        "contains",
        "gt",
        "gte",
        "lt",
        "lte",
    ];
    let present = comparators
        .iter()
        .filter(|key| object.contains_key(**key))
        .collect::<Vec<_>>();
    if present.len() != 1 {
        bail!("{label} predicate must set exactly one comparator");
    }
    match *present[0] {
        "exists" if !object.get("exists").is_some_and(Value::is_boolean) => {
            bail!("{label}.exists must be boolean");
        }
        "in" if !object.get("in").is_some_and(Value::is_array) => {
            bail!("{label}.in must be an array");
        }
        _ => {}
    }
    Ok(())
}

fn validate_basic_json_schema(label: &str, schema_json: &str, value: &Value) -> Result<()> {
    if schema_json.trim().is_empty() {
        return Ok(());
    }
    let schema: Value = serde_json::from_str(schema_json)?;
    if schema.get("type").and_then(Value::as_str) == Some("object") && !value.is_object() {
        bail!("{label} must be an object");
    }
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for field in required.iter().filter_map(Value::as_str) {
            if value.get(field).is_none() {
                bail!("{label} is missing required property '{field}'");
            }
        }
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (field, property_schema) in properties {
            let Some(field_value) = value.get(field) else {
                continue;
            };
            let Some(type_name) = property_schema.get("type").and_then(Value::as_str) else {
                continue;
            };
            let ok = match type_name {
                "string" => field_value.is_string(),
                "boolean" => field_value.is_boolean(),
                "number" => field_value.is_number(),
                "integer" => field_value.as_i64().is_some() || field_value.as_u64().is_some(),
                "object" => field_value.is_object(),
                "array" => field_value.is_array(),
                _ => true,
            };
            if !ok {
                bail!("{label}.{field} must be {type_name}");
            }
        }
    }
    Ok(())
}

fn detect_cycle(spec: &models::WorkflowSpec) -> Result<()> {
    let graph = spec
        .steps
        .iter()
        .map(|step| {
            (
                step.id.as_str(),
                step.after.iter().map(String::as_str).collect::<Vec<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for step in &spec.steps {
        visit(&step.id, &graph, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn visit<'a>(
    id: &'a str,
    graph: &HashMap<&'a str, Vec<&'a str>>,
    visiting: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
) -> Result<()> {
    if visited.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id) {
        bail!("workflow contains a dependency cycle involving '{}'", id);
    }
    for dep in graph.get(id).into_iter().flatten() {
        visit(dep, graph, visiting, visited)?;
    }
    visiting.remove(id);
    visited.insert(id);
    Ok(())
}

trait ContextExt<T> {
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String;
}

impl<T, E> ContextExt<T> for std::result::Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|err| anyhow!("{}: {}", f(), err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{proto, Config, ProviderConfig, Secret};
    use crate::control::{
        events::{MessageDirection, SessionMessageEvent, WorkflowDispatchEvent},
        scheduler::{
            NoopSchedulerBackend, ScheduleWakeupRequest, ScheduledWakeup, SchedulerBackend,
        },
        ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::manifests;
    use crate::knowledge::{KnowledgeBook, KvKnowledgeBook};
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use crate::worker::{
        mcp_registry::McpRegistry, scheduler_auth::SchedulerRequestAuthenticator,
        WorkerEventHandler,
    };
    use async_trait::async_trait;
    use axum::{routing::post, Router};
    use prost::Message;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::net::TcpListener;

    #[derive(Default)]
    struct RecordingScheduler {
        scheduled: tokio::sync::Mutex<Vec<ScheduleWakeupRequest>>,
    }

    #[async_trait]
    impl SchedulerBackend for RecordingScheduler {
        async fn schedule(&self, req: ScheduleWakeupRequest) -> Result<ScheduledWakeup> {
            self.scheduled.lock().await.push(req);
            Ok(ScheduledWakeup {
                handle: Some("recorded-wakeup".to_string()),
                armed: true,
            })
        }

        async fn cancel(&self, _handle: &str) -> Result<()> {
            Ok(())
        }
    }

    fn workflow_handler(kv: Arc<MockKvStore>, pubsub: Arc<RecordingPubSub>) -> WorkerEventHandler {
        WorkerEventHandler {
            cp: Arc::new(ControlPlane {
                kv,
                pubsub,
                scheduler: Arc::new(NoopSchedulerBackend),
                objects: crate::control::object_store::default_object_store(),
            }),
            config: Arc::new(Config {
                providers: HashMap::from([(
                    "novita".to_string(),
                    ProviderConfig {
                        config: Some(proto::llm_provider_config::Config::OpenaiCompatible(
                            proto::GenericConfig {
                                name: "novita".to_string(),
                                base_url: "https://unused.example.com".to_string(),
                                model: "test-model".to_string(),
                                api_key: Some(Secret {
                                    source: Some(proto::secret::Source::Plain(
                                        "test-key".to_string(),
                                    )),
                                }),
                            },
                        )),
                    },
                )]),
                default_provider: "novita".to_string(),
                ..Config::default()
            }),
            mcp_registry: Arc::new(McpRegistry::new()),
            scheduler_authenticator: Arc::new(SchedulerRequestAuthenticator::deny_all()),
            session_cancellations: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    fn workflow_dispatch(run: &models::WorkflowRun, reason: &str) -> WorkflowDispatchEvent {
        WorkflowDispatchEvent {
            ns: run.ns.clone(),
            workflow: run.workflow.clone(),
            run_id: run.id.clone(),
            reason: reason.to_string(),
            step_id: String::new(),
            child_session_id: String::new(),
            timestamp: Utc::now().timestamp_micros(),
        }
    }

    async fn stored_run(
        kv: &MockKvStore,
        ns: &str,
        workflow: &str,
        run_id: &str,
    ) -> models::WorkflowRun {
        kv.get_msg::<models::WorkflowRun>(&keys::workflow_run(ns, workflow, run_id))
            .await
            .expect("run should load")
            .expect("run should exist")
    }

    async fn stored_step(
        kv: &MockKvStore,
        run: &models::WorkflowRun,
        step_id: &str,
    ) -> models::WorkflowStepRun {
        kv.get_msg::<models::WorkflowStepRun>(&keys::workflow_step_run(
            &run.ns,
            &run.workflow,
            &run.id,
            step_id,
        ))
        .await
        .expect("step should load")
        .expect("step should exist")
    }

    async fn latest_session_dispatch(pubsub: &RecordingPubSub, agent: &str) -> SessionMessageEvent {
        let published = pubsub.published.lock().await;
        published
            .iter()
            .rev()
            .filter(|(topic, _)| topic == topics::SESSION_DISPATCH_TOPIC)
            .filter_map(|(_, bytes)| SessionMessageEvent::decode(bytes.as_slice()).ok())
            .find(|event| event.agent == agent)
            .expect("session dispatch should be published")
    }

    async fn workflow_event_types(
        kv: &MockKvStore,
        ns: &str,
        workflow: &str,
        run_id: &str,
    ) -> Vec<String> {
        kv.list_entries(&keys::workflow_run_event_prefix(ns, workflow, run_id))
            .await
            .expect("events should list")
            .into_iter()
            .map(|(_, bytes)| {
                models::WorkflowRunEvent::decode(bytes.as_slice())
                    .expect("event should decode")
                    .r#type
            })
            .collect()
    }

    async fn event_type_count(
        kv: &MockKvStore,
        ns: &str,
        workflow: &str,
        run_id: &str,
        event_type: &str,
    ) -> usize {
        workflow_event_types(kv, ns, workflow, run_id)
            .await
            .into_iter()
            .filter(|current| current == event_type)
            .count()
    }

    #[test]
    fn validate_workflow_rejects_duplicate_step_ids() {
        let workflow = models::Workflow {
            name: "dupe".to_string(),
            ns: "default".to_string(),
            spec: Some(models::WorkflowSpec {
                steps: vec![
                    models::WorkflowStep {
                        id: "review".to_string(),
                        r#type: "transform".to_string(),
                        ..Default::default()
                    },
                    models::WorkflowStep {
                        id: "review".to_string(),
                        r#type: "transform".to_string(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            labels: HashMap::new(),
        };

        let err = validate_workflow(&workflow).expect_err("duplicate ids should be rejected");
        assert!(err.to_string().contains("duplicate workflow step id"));

        let slash_name = models::Workflow {
            name: "bad/name".to_string(),
            ns: "default".to_string(),
            spec: Some(models::WorkflowSpec {
                steps: vec![models::WorkflowStep {
                    id: "review".to_string(),
                    r#type: "transform".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            labels: HashMap::new(),
        };
        let err = validate_workflow(&slash_name).expect_err("slash names should be rejected");
        assert!(err.to_string().contains("workflow name cannot contain '/'"));
    }

    #[tokio::test]
    async fn advance_run_completes_transform_dag_and_maps_output() {
        let kv = Arc::new(MockKvStore::new());
        let cp = ControlPlane {
            kv: kv.clone(),
            pubsub: Arc::new(RecordingPubSub::default()),
            scheduler: Arc::new(NoopSchedulerBackend),
            objects: crate::control::object_store::default_object_store(),
        };
        let workflow = models::Workflow {
            name: "copy".to_string(),
            ns: "default".to_string(),
            labels: HashMap::new(),
            spec: Some(models::WorkflowSpec {
                input_schema_json: r#"{"type":"object","required":["answer"],"properties":{"answer":{"type":"string"}}}"#.to_string(),
                output_schema_json: r#"{"type":"object","required":["answer"],"properties":{"answer":{"type":"string"}}}"#.to_string(),
                steps: vec![models::WorkflowStep {
                    id: "copy".to_string(),
                    r#type: "transform".to_string(),
                    input_json: r#"{"answer":"${$.input.answer}"}"#.to_string(),
                    ..Default::default()
                }],
                output_json: r#"{"answer":"${$.steps.copy.output.answer}"}"#.to_string(),
                ..Default::default()
            }),
        };

        validate_workflow(&workflow).expect("workflow should validate");
        kv.set_msg(&keys::workflow("default", "copy"), &workflow)
            .await
            .expect("workflow should persist");
        let run = create_run(
            &cp,
            &workflow,
            r#"{"answer":"yes"}"#.to_string(),
            HashMap::new(),
        )
        .await
        .expect("run should be created");
        let claimed = claim_run(&*kv, "default", "copy", &run.id, Utc::now(), "test")
            .await
            .expect("claim should succeed")
            .expect("run should be claimable");

        advance_run(&cp, claimed)
            .await
            .expect("workflow should advance");

        let stored = kv
            .get_msg::<models::WorkflowRun>(&keys::workflow_run("default", "copy", &run.id))
            .await
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(stored.status, STATUS_COMPLETED);
        assert_eq!(
            serde_json::from_str::<Value>(&stored.output_json).expect("output should be JSON"),
            json!({ "answer": "yes" })
        );
        let step = kv
            .get_msg::<models::WorkflowStepRun>(&keys::workflow_step_run(
                "default", "copy", &run.id, "copy",
            ))
            .await
            .expect("step should load")
            .expect("step should exist");
        assert_eq!(step.status, STATUS_COMPLETED);
    }

    #[tokio::test]
    async fn tool_step_executes_knowledge_search_and_json_policy_failure_fails_run() {
        let kv = Arc::new(MockKvStore::new());
        let cp = ControlPlane {
            kv: kv.clone(),
            pubsub: Arc::new(RecordingPubSub::default()),
            scheduler: Arc::new(NoopSchedulerBackend),
            objects: crate::control::object_store::default_object_store(),
        };
        let book = KvKnowledgeBook::new(kv.clone());
        book.write("default", "goals.md", "ship workflow support")
            .await
            .expect("knowledge should persist");
        let workflow = models::Workflow {
            name: "search".to_string(),
            ns: "default".to_string(),
            labels: HashMap::new(),
            spec: Some(models::WorkflowSpec {
                steps: vec![models::WorkflowStep {
                    id: "searchKnowledge".to_string(),
                    r#type: "tool".to_string(),
                    tool: crate::knowledge::KNOWLEDGE_SEARCH_TOOL.to_string(),
                    input_json: r#"{"query":"ship"}"#.to_string(),
                    ..Default::default()
                }],
                output_json: r#"{"result":"${$.steps.searchKnowledge.output.text}"}"#.to_string(),
                ..Default::default()
            }),
        };
        kv.set_msg(&keys::workflow("default", "search"), &workflow)
            .await
            .unwrap();
        let run = create_run(&cp, &workflow, "{}".to_string(), HashMap::new())
            .await
            .unwrap();
        let claimed = claim_run(&*kv, "default", "search", &run.id, Utc::now(), "test")
            .await
            .unwrap()
            .unwrap();

        advance_run(&cp, claimed).await.unwrap();

        let completed = stored_run(&kv, "default", "search", &run.id).await;
        assert_eq!(completed.status, STATUS_COMPLETED);
        assert!(serde_json::from_str::<Value>(&completed.output_json)
            .unwrap()
            .get("result")
            .and_then(Value::as_str)
            .unwrap()
            .contains("[default:goals.md]"));

        let failing = models::Workflow {
            name: "bad-json-output".to_string(),
            ns: "default".to_string(),
            labels: HashMap::new(),
            spec: Some(models::WorkflowSpec {
                steps: vec![models::WorkflowStep {
                    id: "searchKnowledge".to_string(),
                    r#type: "tool".to_string(),
                    tool: crate::knowledge::KNOWLEDGE_SEARCH_TOOL.to_string(),
                    input_json: r#"{"query":"ship"}"#.to_string(),
                    output: Some(models::WorkflowStepOutputPolicy {
                        format: "json".to_string(),
                        schema_json: String::new(),
                    }),
                    ..Default::default()
                }],
                output_json: "{}".to_string(),
                ..Default::default()
            }),
        };
        kv.set_msg(&keys::workflow("default", "bad-json-output"), &failing)
            .await
            .unwrap();
        let run = create_run(&cp, &failing, "{}".to_string(), HashMap::new())
            .await
            .unwrap();
        let claimed = claim_run(
            &*kv,
            "default",
            "bad-json-output",
            &run.id,
            Utc::now(),
            "test",
        )
        .await
        .unwrap()
        .unwrap();

        advance_run(&cp, claimed).await.unwrap();

        let failed = stored_run(&kv, "default", "bad-json-output", &run.id).await;
        assert_eq!(failed.status, STATUS_FAILED);
        assert!(failed.error.contains("expected JSON output"));
        assert_eq!(
            stored_step(&kv, &failed, "searchKnowledge").await.status,
            STATUS_FAILED
        );
    }

    #[tokio::test]
    async fn child_workflow_step_waits_for_child_run_and_parent_redispatch_completes() {
        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = workflow_handler(kv.clone(), pubsub);
        let child = models::Workflow {
            name: "child-copy".to_string(),
            ns: "default".to_string(),
            labels: HashMap::new(),
            spec: Some(models::WorkflowSpec {
                steps: vec![models::WorkflowStep {
                    id: "copy".to_string(),
                    r#type: "transform".to_string(),
                    input_json: r#"{"answer":"${$.input.answer}"}"#.to_string(),
                    ..Default::default()
                }],
                output_json: r#"{"answer":"${$.steps.copy.output.answer}"}"#.to_string(),
                ..Default::default()
            }),
        };
        let parent = models::Workflow {
            name: "parent".to_string(),
            ns: "default".to_string(),
            labels: HashMap::new(),
            spec: Some(models::WorkflowSpec {
                steps: vec![models::WorkflowStep {
                    id: "child".to_string(),
                    r#type: "workflow".to_string(),
                    workflow: "child-copy".to_string(),
                    input_json: r#"{"answer":"${$.input.answer}"}"#.to_string(),
                    ..Default::default()
                }],
                output_json: r#"{"answer":"${$.steps.child.output.answer}"}"#.to_string(),
                ..Default::default()
            }),
        };
        kv.set_msg(&keys::workflow("default", "child-copy"), &child)
            .await
            .unwrap();
        kv.set_msg(&keys::workflow("default", "parent"), &parent)
            .await
            .unwrap();
        let parent_run = create_run(
            &handler.cp,
            &parent,
            r#"{"answer":"from-child"}"#.to_string(),
            HashMap::new(),
        )
        .await
        .unwrap();

        handler
            .handle_workflow_dispatch(workflow_dispatch(&parent_run, "created"))
            .await
            .unwrap();
        let waiting_parent = stored_run(&kv, "default", "parent", &parent_run.id).await;
        assert_eq!(waiting_parent.status, STATUS_WAITING_CHILDREN);
        let child_step = stored_step(&kv, &waiting_parent, "child").await;
        assert_eq!(child_step.status, STATUS_WAITING_CHILD_WORKFLOW);

        let child_run = stored_run(
            &kv,
            "default",
            "child-copy",
            &child_step.child_workflow_run_id,
        )
        .await;
        handler
            .handle_workflow_dispatch(workflow_dispatch(&child_run, "created"))
            .await
            .unwrap();
        assert_eq!(
            stored_run(&kv, "default", "child-copy", &child_run.id)
                .await
                .status,
            STATUS_COMPLETED
        );

        handler
            .handle_workflow_dispatch(workflow_dispatch(&parent_run, "child_completed"))
            .await
            .unwrap();
        let completed_parent = stored_run(&kv, "default", "parent", &parent_run.id).await;
        assert_eq!(completed_parent.status, STATUS_COMPLETED);
        assert_eq!(
            serde_json::from_str::<Value>(&completed_parent.output_json).unwrap(),
            json!({ "answer": "from-child" })
        );
    }

    #[tokio::test]
    async fn run_uses_spec_snapshot_even_if_workflow_is_modified() {
        let kv = Arc::new(MockKvStore::new());
        let cp = ControlPlane {
            kv: kv.clone(),
            pubsub: Arc::new(RecordingPubSub::default()),
            scheduler: Arc::new(NoopSchedulerBackend),
            objects: crate::control::object_store::default_object_store(),
        };
        let mut workflow = models::Workflow {
            name: "snapshot".to_string(),
            ns: "default".to_string(),
            labels: HashMap::new(),
            spec: Some(models::WorkflowSpec {
                steps: vec![models::WorkflowStep {
                    id: "copy".to_string(),
                    r#type: "transform".to_string(),
                    input_json: r#"{"value":"${$.input.value}"}"#.to_string(),
                    ..Default::default()
                }],
                output_json: r#"{"value":"${$.steps.copy.output.value}","version":"original"}"#
                    .to_string(),
                ..Default::default()
            }),
        };
        kv.set_msg(&keys::workflow("default", "snapshot"), &workflow)
            .await
            .unwrap();
        let run = create_run(
            &cp,
            &workflow,
            r#"{"value":"kept"}"#.to_string(),
            HashMap::new(),
        )
        .await
        .unwrap();

        workflow.spec.as_mut().unwrap().output_json =
            r#"{"value":"mutated","version":"edited"}"#.to_string();
        kv.set_msg(&keys::workflow("default", "snapshot"), &workflow)
            .await
            .unwrap();

        let claimed = claim_run(&*kv, "default", "snapshot", &run.id, Utc::now(), "test")
            .await
            .unwrap()
            .unwrap();
        advance_run(&cp, claimed).await.unwrap();

        let completed = stored_run(&kv, "default", "snapshot", &run.id).await;
        assert_eq!(completed.status, STATUS_COMPLETED);
        assert_eq!(
            serde_json::from_str::<Value>(&completed.output_json).unwrap(),
            json!({ "value": "kept", "version": "original" })
        );
        assert!(!completed.spec_json.is_empty());
    }

    #[tokio::test]
    async fn concurrency_limit_blocks_additional_ready_agent_steps() {
        let kv = Arc::new(MockKvStore::new());
        let cp = ControlPlane {
            kv: kv.clone(),
            pubsub: Arc::new(RecordingPubSub::default()),
            scheduler: Arc::new(NoopSchedulerBackend),
            objects: crate::control::object_store::default_object_store(),
        };
        for agent in ["a", "b"] {
            kv.set_msg(
                &keys::agent("default", agent),
                &models::Agent {
                    name: agent.to_string(),
                    ns: "default".to_string(),
                    definition: None,
                    effective_spec: Some(manifests::AgentSpec {
                        features: Vec::new(),
                        model_policy: None,
                        system_prompt: "test".to_string(),
                        mcp_server_refs: Vec::new(),
                        capabilities: HashMap::new(),
                    }),
                    template_deps: Vec::new(),
                    labels: HashMap::new(),
                },
            )
            .await
            .unwrap();
        }
        let workflow = models::Workflow {
            name: "limited".to_string(),
            ns: "default".to_string(),
            labels: HashMap::new(),
            spec: Some(models::WorkflowSpec {
                steps: vec![
                    models::WorkflowStep {
                        id: "first".to_string(),
                        r#type: "agent".to_string(),
                        agent: "a".to_string(),
                        prompt: "first".to_string(),
                        ..Default::default()
                    },
                    models::WorkflowStep {
                        id: "second".to_string(),
                        r#type: "agent".to_string(),
                        agent: "b".to_string(),
                        prompt: "second".to_string(),
                        ..Default::default()
                    },
                ],
                concurrency: 1,
                ..Default::default()
            }),
        };
        validate_workflow(&workflow).unwrap();
        kv.set_msg(&keys::workflow("default", "limited"), &workflow)
            .await
            .unwrap();
        let run = create_run(&cp, &workflow, "{}".to_string(), HashMap::new())
            .await
            .unwrap();
        let claimed = claim_run(&*kv, "default", "limited", &run.id, Utc::now(), "test")
            .await
            .unwrap()
            .unwrap();
        advance_run(&cp, claimed).await.unwrap();

        let waiting = stored_run(&kv, "default", "limited", &run.id).await;
        assert_eq!(waiting.status, STATUS_WAITING_CHILDREN);
        assert_eq!(
            stored_step(&kv, &waiting, "first").await.status,
            STATUS_WAITING_CHILD_SESSION
        );
        assert!(kv
            .get_msg::<models::WorkflowStepRun>(&keys::workflow_step_run(
                "default", "limited", &run.id, "second"
            ))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn timed_wait_schedules_wakeup_and_completes_when_due() {
        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        let scheduler = Arc::new(RecordingScheduler::default());
        let cp = ControlPlane {
            kv: kv.clone(),
            pubsub: pubsub.clone(),
            scheduler: scheduler.clone(),
            objects: crate::control::object_store::default_object_store(),
        };
        let workflow = models::Workflow {
            name: "timer".to_string(),
            ns: "default".to_string(),
            labels: HashMap::new(),
            spec: Some(models::WorkflowSpec {
                steps: vec![models::WorkflowStep {
                    id: "sleep".to_string(),
                    r#type: "wait".to_string(),
                    wait_duration: "5m".to_string(),
                    ..Default::default()
                }],
                output_json: r#"{"firedAt":"${$.steps.sleep.output.firedAt}"}"#.to_string(),
                ..Default::default()
            }),
        };
        validate_workflow(&workflow).unwrap();
        kv.set_msg(&keys::workflow("default", "timer"), &workflow)
            .await
            .unwrap();
        let run = create_run(&cp, &workflow, "{}".to_string(), HashMap::new())
            .await
            .unwrap();
        let claimed = claim_run(&*kv, "default", "timer", &run.id, Utc::now(), "test")
            .await
            .unwrap()
            .unwrap();
        advance_run(&cp, claimed).await.unwrap();
        let suspended = stored_run(&kv, "default", "timer", &run.id).await;
        assert_eq!(suspended.status, STATUS_SUSPENDED);
        let mut step = stored_step(&kv, &suspended, "sleep").await;
        assert_eq!(step.status, STATUS_SUSPENDED);
        assert_eq!(step.wait_wakeup_handle, "recorded-wakeup");
        assert_eq!(scheduler.scheduled.lock().await.len(), 1);

        step.wait_until_at = Some(Utc::now().timestamp_micros().saturating_sub(1));
        kv.set_msg(
            &keys::workflow_step_run("default", "timer", &run.id, "sleep"),
            &step,
        )
        .await
        .unwrap();
        handle_workflow_wakeup(
            &cp,
            WorkflowWakeupPayload {
                namespace: "default".to_string(),
                workflow: "timer".to_string(),
                run_id: run.id.clone(),
                step_id: "sleep".to_string(),
                attempt: 1,
                intended_fire_at: step.wait_until_at.unwrap(),
                reason: "wait".to_string(),
            },
        )
        .await
        .unwrap();
        let published = pubsub.published.lock().await;
        assert!(published
            .iter()
            .any(|(topic, _)| topic == topics::WORKFLOW_DISPATCH_TOPIC));
        drop(published);
        let claimed = claim_run(&*kv, "default", "timer", &run.id, Utc::now(), "wait")
            .await
            .unwrap()
            .unwrap();
        advance_run(&cp, claimed).await.unwrap();
        let completed = stored_run(&kv, "default", "timer", &run.id).await;
        assert_eq!(completed.status, STATUS_COMPLETED);
    }

    #[tokio::test]
    async fn failed_step_with_retry_schedules_durable_wakeup() {
        let kv = Arc::new(MockKvStore::new());
        let scheduler = Arc::new(RecordingScheduler::default());
        let cp = ControlPlane {
            kv: kv.clone(),
            pubsub: Arc::new(RecordingPubSub::default()),
            scheduler: scheduler.clone(),
            objects: crate::control::object_store::default_object_store(),
        };
        let workflow = models::Workflow {
            name: "retrying".to_string(),
            ns: "default".to_string(),
            labels: HashMap::new(),
            spec: Some(models::WorkflowSpec {
                steps: vec![models::WorkflowStep {
                    id: "unsupported".to_string(),
                    r#type: "tool".to_string(),
                    tool: "missing_tool".to_string(),
                    retry: Some(models::WorkflowStepRetryPolicy {
                        max_attempts: 2,
                        initial_backoff_seconds: 1,
                        max_backoff_seconds: 5,
                        multiplier: 2.0,
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
        };
        validate_workflow(&workflow).unwrap();
        kv.set_msg(&keys::workflow("default", "retrying"), &workflow)
            .await
            .unwrap();
        let run = create_run(&cp, &workflow, "{}".to_string(), HashMap::new())
            .await
            .unwrap();
        let claimed = claim_run(&*kv, "default", "retrying", &run.id, Utc::now(), "test")
            .await
            .unwrap()
            .unwrap();
        advance_run(&cp, claimed).await.unwrap();

        let waiting = stored_run(&kv, "default", "retrying", &run.id).await;
        assert_eq!(waiting.status, STATUS_WAITING_CHILDREN);
        let step = stored_step(&kv, &waiting, "unsupported").await;
        assert_eq!(step.status, STATUS_WAITING_RETRY);
        assert_eq!(step.attempt, 1);
        assert!(step.next_retry_at.is_some());
        assert_eq!(scheduler.scheduled.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn redispatch_terminal_run_is_idempotent_and_cancelled_run_is_not_claimed() {
        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = workflow_handler(kv.clone(), pubsub);
        let workflow = models::Workflow {
            name: "once".to_string(),
            ns: "default".to_string(),
            labels: HashMap::new(),
            spec: Some(models::WorkflowSpec {
                steps: vec![models::WorkflowStep {
                    id: "copy".to_string(),
                    r#type: "transform".to_string(),
                    input_json: r#"{"answer":"${$.input.answer}"}"#.to_string(),
                    ..Default::default()
                }],
                output_json: r#"{"answer":"${$.steps.copy.output.answer}"}"#.to_string(),
                ..Default::default()
            }),
        };
        kv.set_msg(&keys::workflow("default", "once"), &workflow)
            .await
            .unwrap();
        let run = create_run(
            &handler.cp,
            &workflow,
            r#"{"answer":"yes"}"#.to_string(),
            HashMap::new(),
        )
        .await
        .unwrap();

        handler
            .handle_workflow_dispatch(workflow_dispatch(&run, "created"))
            .await
            .unwrap();
        let completed = stored_run(&kv, "default", "once", &run.id).await;
        assert_eq!(completed.status, STATUS_COMPLETED);
        let completed_events =
            event_type_count(&kv, "default", "once", &run.id, "step_completed").await;

        handler
            .handle_workflow_dispatch(workflow_dispatch(&run, "duplicate"))
            .await
            .unwrap();
        assert_eq!(
            event_type_count(&kv, "default", "once", &run.id, "step_completed").await,
            completed_events
        );

        let cancel_run_model = create_run(&handler.cp, &workflow, "{}".to_string(), HashMap::new())
            .await
            .unwrap();
        cancel_run(&handler.cp, "default", "once", &cancel_run_model.id)
            .await
            .unwrap();
        assert!(claim_run(
            handler.cp.kv.as_ref(),
            "default",
            "once",
            &cancel_run_model.id,
            Utc::now(),
            "test",
        )
        .await
        .unwrap()
        .is_none());
        assert_eq!(
            stored_run(&kv, "default", "once", &cancel_run_model.id)
                .await
                .status,
            STATUS_CANCELLED
        );
        assert!(
            workflow_event_types(&kv, "default", "once", &cancel_run_model.id)
                .await
                .iter()
                .any(|event| event == "run_cancelled")
        );
    }

    #[tokio::test]
    async fn wait_step_suspends_and_resumes_with_resume_payload_available_downstream() {
        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = workflow_handler(kv.clone(), pubsub);
        let workflow = models::Workflow {
            name: "waiter".to_string(),
            ns: "default".to_string(),
            labels: HashMap::new(),
            spec: Some(models::WorkflowSpec {
                steps: vec![
                    models::WorkflowStep {
                        id: "externalSignal".to_string(),
                        r#type: "wait".to_string(),
                        prompt: "Wait for signal".to_string(),
                        resume_schema_json:
                            r#"{"type":"object","required":["token"],"properties":{"token":{"type":"string"}}}"#
                                .to_string(),
                        ..Default::default()
                    },
                    models::WorkflowStep {
                        id: "afterWait".to_string(),
                        r#type: "transform".to_string(),
                        after: vec!["externalSignal".to_string()],
                        input_json: r#"{"token":"${$.steps.externalSignal.resume.token}"}"#.to_string(),
                        ..Default::default()
                    },
                ],
                output_json: r#"{"token":"${$.steps.afterWait.output.token}"}"#.to_string(),
                ..Default::default()
            }),
        };
        kv.set_msg(&keys::workflow("default", "waiter"), &workflow)
            .await
            .unwrap();
        let run = create_run(&handler.cp, &workflow, "{}".to_string(), HashMap::new())
            .await
            .unwrap();

        handler
            .handle_workflow_dispatch(workflow_dispatch(&run, "created"))
            .await
            .unwrap();
        let suspended = stored_run(&kv, "default", "waiter", &run.id).await;
        assert_eq!(suspended.status, STATUS_SUSPENDED);
        assert_eq!(
            stored_step(&kv, &suspended, "externalSignal").await.status,
            STATUS_SUSPENDED
        );

        resume_run(
            &handler.cp,
            "default",
            "waiter",
            &run.id,
            "externalSignal",
            r#"{"token":"ready"}"#,
        )
        .await
        .unwrap();
        handler
            .handle_workflow_dispatch(workflow_dispatch(&run, "resumed"))
            .await
            .unwrap();

        let completed = stored_run(&kv, "default", "waiter", &run.id).await;
        assert_eq!(completed.status, STATUS_COMPLETED);
        assert_eq!(
            serde_json::from_str::<Value>(&completed.output_json).unwrap(),
            json!({ "token": "ready" })
        );
    }

    #[tokio::test]
    async fn complex_workflow_yaml_runs_end_to_end_with_mock_llm_agent_and_resume() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
        let app = Router::new().route(
            "/chat/completions",
            post(|| async {
                axum::response::Response::builder()
                    .status(axum::http::StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(axum::body::Body::from(concat!(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"mock retention action from LLM\"}}]}\n\n",
                        "data: [DONE]\n"
                    )))
                    .expect("mock LLM response should build")
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fake LLM should bind");
        let addr = listener.local_addr().expect("fake LLM should have addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("fake LLM should serve");
        });
        unsafe {
            std::env::set_var("NOVITA_BASE_URL", format!("http://{addr}"));
        }

        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = workflow_handler(kv.clone(), pubsub.clone());

        let workflow = crate::manifest::parse_workflow(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Workflow
metadata:
  name: complex-retention
  namespace: customer-retention
  labels:
    app: retention
spec:
  description: Complex retention workflow with fanout, branching, pause, and agent execution.
  inputSchema:
    type: object
    required: [accountId, risk, urgency]
    properties:
      accountId:
        type: string
      risk:
        type: string
      urgency:
        type: string
  outputSchema:
    type: object
    required: [summary, action, approved]
    properties:
      summary:
        type: string
      action:
        type: string
      approved:
        type: boolean
  steps:
    - id: intake
      type: transform
      input:
        accountId: ${$.input.accountId}
        risk: ${$.input.risk}
        summary: Account ${$.input.accountId} is ${$.input.risk} risk

    - id: profile
      type: transform
      after: [intake]
      input:
        profileSummary: Profile for ${$.steps.intake.output.accountId}

    - id: policy
      type: transform
      after: [intake]
      input:
        policySummary: Policy for ${$.input.urgency}

    - id: lowTouch
      type: transform
      after: [intake]
      when:
        path: $.steps.intake.output.risk
        notEquals: high
      input:
        action: low-touch

    - id: approval
      type: pause
      after: [intake]
      when:
        path: $.steps.intake.output.risk
        equals: high
      prompt: Approve retention action for ${$.steps.intake.output.accountId}?
      resumeSchema:
        type: object
        required: [approved]
        properties:
          approved:
            type: boolean

    - id: merge
      type: transform
      after: [profile, policy]
      input:
        summary: ${$.steps.intake.output.summary}; ${$.steps.profile.output.profileSummary}; ${$.steps.policy.output.policySummary}

    - id: draftAction
      type: agent
      after: [merge, approval, lowTouch]
      when:
        any:
          - path: $.steps.intake.output.risk
            notEquals: high
          - path: $.steps.approval.resume.approved
            equals: true
      agent: campaign-writer
      prompt: |
        Draft a retention action.
        ${$.steps.merge.output.summary}

    - id: final
      type: transform
      after: [draftAction]
      input:
        action: ${$.steps.draftAction.output.text}
        approved: ${$.steps.approval.resume.approved}
  output:
    summary: ${$.steps.merge.output.summary}
    action: ${$.steps.final.output.action}
    approved: ${$.steps.final.output.approved}
"#,
        )
        .expect("complex workflow YAML should parse");
        kv.set_msg(
            &keys::workflow("customer-retention", "complex-retention"),
            &workflow,
        )
        .await
        .expect("workflow should persist");
        kv.set_msg(
            &keys::agent("customer-retention", "campaign-writer"),
            &models::Agent {
                name: "campaign-writer".to_string(),
                ns: "customer-retention".to_string(),
                definition: None,
                effective_spec: Some(manifests::AgentSpec {
                    features: Vec::new(),
                    model_policy: None,
                    system_prompt: "Write concise retention actions.".to_string(),
                    mcp_server_refs: Vec::new(),
                    capabilities: HashMap::new(),
                }),
                template_deps: Vec::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .expect("agent should persist");

        let run = create_run(
            &handler.cp,
            &workflow,
            r#"{"accountId":"acct_123","risk":"high","urgency":"normal"}"#.to_string(),
            HashMap::new(),
        )
        .await
        .expect("run should create");

        handler
            .handle_workflow_dispatch(workflow_dispatch(&run, "created"))
            .await
            .expect("first workflow dispatch should run until pause");
        let suspended = stored_run(&kv, "customer-retention", "complex-retention", &run.id).await;
        assert_eq!(suspended.status, STATUS_SUSPENDED);
        assert_eq!(
            stored_step(&kv, &suspended, "intake").await.status,
            STATUS_COMPLETED
        );
        assert_eq!(
            stored_step(&kv, &suspended, "profile").await.status,
            STATUS_COMPLETED
        );
        assert_eq!(
            stored_step(&kv, &suspended, "policy").await.status,
            STATUS_COMPLETED
        );
        assert_eq!(
            stored_step(&kv, &suspended, "merge").await.status,
            STATUS_COMPLETED
        );
        assert_eq!(
            stored_step(&kv, &suspended, "lowTouch").await.status,
            STATUS_SKIPPED
        );
        assert_eq!(
            stored_step(&kv, &suspended, "approval").await.status,
            STATUS_SUSPENDED
        );
        assert!(resume_run(
            &handler.cp,
            "customer-retention",
            "complex-retention",
            &run.id,
            "approval",
            r#"{"approved":"yes"}"#,
        )
        .await
        .expect_err("invalid resume payload should be rejected")
        .to_string()
        .contains("resume.approved must be boolean"));

        resume_run(
            &handler.cp,
            "customer-retention",
            "complex-retention",
            &run.id,
            "approval",
            r#"{"approved":true}"#,
        )
        .await
        .expect("valid resume should be accepted");
        handler
            .handle_workflow_dispatch(workflow_dispatch(&run, "resumed"))
            .await
            .expect("resumed workflow should start child agent");
        let waiting = stored_run(&kv, "customer-retention", "complex-retention", &run.id).await;
        assert_eq!(waiting.status, STATUS_WAITING_CHILDREN);
        let draft_step = stored_step(&kv, &waiting, "draftAction").await;
        assert_eq!(draft_step.status, STATUS_WAITING_CHILD_SESSION);
        let session_id = draft_step.child_session_id.clone();
        let session = kv
            .get_msg::<models::Session>(&keys::session(
                "customer-retention",
                "campaign-writer",
                &session_id,
            ))
            .await
            .expect("child session should load")
            .expect("child session should exist");
        assert_eq!(
            session.labels.get(LABEL_WORKFLOW),
            Some(&"complex-retention".to_string())
        );
        assert_eq!(session.labels.get(LABEL_WORKFLOW_RUN), Some(&run.id));
        assert_eq!(
            session.labels.get(LABEL_WORKFLOW_STEP),
            Some(&"draftAction".to_string())
        );

        let child_event = latest_session_dispatch(&pubsub, "campaign-writer").await;
        handler
            .handle_session_message(SessionMessageEvent {
                direction: MessageDirection::Inbound as i32,
                ..child_event
            })
            .await
            .expect("mock LLM child session should complete");
        handler
            .handle_workflow_dispatch(workflow_dispatch(&run, "child_session_completed"))
            .await
            .expect("parent workflow should complete after child session");

        let completed = stored_run(&kv, "customer-retention", "complex-retention", &run.id).await;
        assert_eq!(completed.status, STATUS_COMPLETED);
        assert_eq!(
            serde_json::from_str::<Value>(&completed.output_json).expect("output should decode"),
            json!({
                "summary": "Account acct_123 is high risk; Profile for acct_123; Policy for normal",
                "action": "mock retention action from LLM",
                "approved": true
            })
        );
        assert_eq!(
            serde_json::from_str::<Value>(
                &stored_step(&kv, &completed, "draftAction")
                    .await
                    .output_json
            )
            .expect("agent output should decode"),
            json!({ "text": "mock retention action from LLM" })
        );
        let event_types =
            workflow_event_types(&kv, "customer-retention", "complex-retention", &run.id).await;
        assert!(event_types.iter().any(|event| event == "run_started"));
        assert!(event_types.iter().any(|event| event == "step_skipped"));
        assert!(event_types.iter().any(|event| event == "run_suspended"));
        assert!(event_types.iter().any(|event| event == "run_resumed"));
        assert!(event_types.iter().any(|event| event == "run_completed"));

        unsafe {
            std::env::remove_var("NOVITA_BASE_URL");
        }
        server.abort();
    }

    #[tokio::test]
    async fn failed_child_agent_session_fails_step_without_using_stale_output() {
        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = workflow_handler(kv.clone(), pubsub);
        let workflow = models::Workflow {
            name: "agent-failure".to_string(),
            ns: "customer-retention".to_string(),
            labels: HashMap::new(),
            spec: Some(models::WorkflowSpec {
                steps: vec![models::WorkflowStep {
                    id: "draft".to_string(),
                    r#type: "agent".to_string(),
                    agent: "campaign-writer".to_string(),
                    prompt: "Draft an action".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
        };
        kv.set_msg(
            &keys::workflow("customer-retention", "agent-failure"),
            &workflow,
        )
        .await
        .expect("workflow should persist");
        kv.set_msg(
            &keys::agent("customer-retention", "campaign-writer"),
            &models::Agent {
                name: "campaign-writer".to_string(),
                ns: "customer-retention".to_string(),
                definition: None,
                effective_spec: Some(manifests::AgentSpec {
                    features: Vec::new(),
                    model_policy: None,
                    system_prompt: "Write concise retention actions.".to_string(),
                    mcp_server_refs: Vec::new(),
                    capabilities: HashMap::new(),
                }),
                template_deps: Vec::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .expect("agent should persist");

        let run = create_run(&handler.cp, &workflow, "{}".to_string(), HashMap::new())
            .await
            .expect("run should create");
        handler
            .handle_workflow_dispatch(workflow_dispatch(&run, "created"))
            .await
            .expect("workflow should start child session");
        let waiting = stored_run(&kv, "customer-retention", "agent-failure", &run.id).await;
        let draft_step = stored_step(&kv, &waiting, "draft").await;
        assert_eq!(draft_step.status, STATUS_WAITING_CHILD_SESSION);

        let session_key = keys::session(
            "customer-retention",
            "campaign-writer",
            &draft_step.child_session_id,
        );
        let mut session = kv
            .get_msg::<models::Session>(&session_key)
            .await
            .expect("session should load")
            .expect("session should exist");
        kv.set_msg(
            &keys::session_message(
                "customer-retention",
                "campaign-writer",
                &draft_step.child_session_id,
                "old-assistant",
            ),
            &models::SessionMessage {
                id: "old-assistant".to_string(),
                role: models::MessageRole::RoleAssistant as i32,
                created_at: 1,
                labels: HashMap::new(),
                parts: vec![models::SessionMessagePart {
                    id: "old-text".to_string(),
                    part_type: models::SessionMessagePartType::Text as i32,
                    content: "stale output from previous turn".to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                    created_at: 1,
                    object: None,
                }],
            },
        )
        .await
        .expect("stale assistant message should persist");
        session.status = "ERROR".to_string();
        kv.set_msg(&session_key, &session)
            .await
            .expect("failed child session should persist");

        handler
            .handle_workflow_dispatch(workflow_dispatch(&run, "child_session_completed"))
            .await
            .expect("workflow should process failed child session");
        let failed = stored_run(&kv, "customer-retention", "agent-failure", &run.id).await;
        let failed_step = stored_step(&kv, &failed, "draft").await;
        assert_eq!(failed_step.status, STATUS_FAILED);
        assert!(failed_step.error.contains("child session"));
        assert_ne!(
            failed_step.output_json,
            r#"{"text":"stale output from previous turn"}"#
        );
        assert_eq!(failed.status, STATUS_FAILED);
    }
}
