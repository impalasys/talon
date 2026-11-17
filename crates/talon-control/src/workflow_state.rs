// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::events;

use crate::control::keys;

use crate::control::topics;

use crate::control::ControlPlane;

use crate::control::KeyValueStore;

use crate::control::ListOptions;

use crate::control::MessagePublisher;

use crate::control::ProtoKeyValueStoreExt;
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use talon_fanout::{FanoutHub, WorkflowFanoutKey};
use talon_protocol::{
    data_proto, resource_model::TypedResource, resources_proto, workflow_validation::*,
};
pub const MAX_CAS_RETRIES: usize = 8;
pub const DEFAULT_WORKFLOW_CLAIM_TIMEOUT_SECONDS: i64 = 60;
pub const WORKFLOW_OUTPUT_PREVIEW_CHARS: usize = 600;

pub const LABEL_WORKFLOW: &str = "talon.impalasys.com/workflow";
pub const LABEL_WORKFLOW_RUN: &str = "talon.impalasys.com/workflow-run";
pub const LABEL_WORKFLOW_STEP: &str = "talon.impalasys.com/workflow-step";
pub const LABEL_WORKFLOW_ATTEMPT: &str = "talon.impalasys.com/workflow-attempt";
pub const LABEL_PARENT_WORKFLOW: &str = "talon.impalasys.com/parent-workflow";
pub const LABEL_PARENT_WORKFLOW_RUN: &str = "talon.impalasys.com/parent-workflow-run";
pub const LABEL_PARENT_WORKFLOW_STEP: &str = "talon.impalasys.com/parent-workflow-step";

pub const STATUS_QUEUED: &str = "QUEUED";
pub const STATUS_RUNNING: &str = "RUNNING";
pub const STATUS_STARTING: &str = "STARTING";
pub const STATUS_WAITING_CHILDREN: &str = "WAITING_CHILDREN";
pub const STATUS_WAITING_RETRY: &str = "WAITING_RETRY";
pub const STATUS_SUSPENDED: &str = "SUSPENDED";
pub const STATUS_COMPLETED: &str = "COMPLETED";
pub const STATUS_FAILED: &str = "FAILED";
pub const STATUS_CANCELLED: &str = "CANCELLED";
pub const STATUS_SKIPPED: &str = "SKIPPED";
pub const STATUS_WAITING_CHILD_SESSION: &str = "WAITING_CHILD_SESSION";
pub const STATUS_WAITING_CHILD_WORKFLOW: &str = "WAITING_CHILD_WORKFLOW";

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
    pub fn new(message: impl Into<String>) -> Self {
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
    pub fn new(message: impl Into<String>) -> Self {
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

pub async fn create_run(
    cp: &ControlPlane,
    workflow: &resources_proto::Workflow,
    input_json: String,
    labels: HashMap<String, String>,
) -> Result<data_proto::WorkflowRun> {
    let input = parse_json_or(input_json.as_str(), Value::Null)?;
    let spec = workflow
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("workflow spec is required"))?;
    validate_input(&spec.input_schema_json, &input)?;

    let now = Utc::now().timestamp_micros();
    let spec_json = serde_json::to_string(spec)?;
    let run = data_proto::WorkflowRun {
        id: crate::control::uuid::v7(),
        workflow: workflow.name().to_string(),
        ns: workflow.namespace().to_string(),
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
    session: &data_proto::Session,
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

pub async fn resume_run(
    cp: &ControlPlane,
    ns: &str,
    workflow: &str,
    run_id: &str,
    step_id: &str,
    resume_json: &str,
) -> Result<data_proto::WorkflowRun> {
    let run = cp
        .kv
        .get_msg::<data_proto::WorkflowRun>(&keys::workflow_run(ns, workflow, run_id))
        .await?
        .ok_or_else(|| WorkflowNotFoundError::new("workflow run not found"))?;
    let workflow_model = cp
        .kv
        .get_msg::<resources_proto::Workflow>(&keys::workflow(ns, workflow))
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
        .get_msg::<data_proto::WorkflowStepRun>(&keys::workflow_step_run(
            ns, workflow, run_id, step_id,
        ))
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
) -> Result<data_proto::WorkflowRun> {
    let mut run = cp
        .kv
        .get_msg::<data_proto::WorkflowRun>(&keys::workflow_run(ns, workflow, run_id))
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
            .get_msg::<data_proto::WorkflowRun>(&keys::workflow_run(
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

pub async fn persist_run(kv: &dyn KeyValueStore, run: &data_proto::WorkflowRun) -> Result<()> {
    kv.set_msg(&keys::workflow_run(&run.ns, &run.workflow, &run.id), run)
        .await
}

pub async fn load_step_runs(
    kv: &dyn KeyValueStore,
    run: &data_proto::WorkflowRun,
) -> Result<HashMap<String, data_proto::WorkflowStepRun>> {
    let mut map = HashMap::new();
    for (_, bytes) in kv
        .list_entries(
            &keys::workflow_step_run_prefix(&run.ns, &run.workflow, &run.id),
            None,
        )
        .await?
    {
        let step_run = data_proto::WorkflowStepRun::decode(bytes.as_slice())?;
        map.insert(step_run.step_id.clone(), step_run);
    }
    Ok(map)
}

pub async fn load_run_spec(
    kv: &dyn KeyValueStore,
    run: &data_proto::WorkflowRun,
) -> Result<resources_proto::WorkflowSpec> {
    if !run.spec_json.trim().is_empty() {
        return serde_json::from_str(&run.spec_json)
            .map_err(|err| anyhow!("workflow run spec snapshot is invalid: {}", err));
    }
    let workflow = kv
        .get_msg::<resources_proto::Workflow>(&keys::workflow(&run.ns, &run.workflow))
        .await?
        .ok_or_else(|| anyhow!("workflow '{}' not found", run.workflow))?;
    workflow
        .spec
        .ok_or_else(|| anyhow!("workflow spec is required"))
}

pub async fn persist_step_run(
    cp: &ControlPlane,
    run: &data_proto::WorkflowRun,
    step_run: &data_proto::WorkflowStepRun,
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
    run: &data_proto::WorkflowRun,
    step_id: &str,
    event_type: &str,
    message: &str,
    payload: Value,
) -> Result<data_proto::WorkflowRunEvent> {
    append_run_event_with_fanout(cp, run, step_id, event_type, message, payload, None).await
}

pub async fn append_run_event_with_fanout(
    cp: &ControlPlane,
    run: &data_proto::WorkflowRun,
    step_id: &str,
    event_type: &str,
    message: &str,
    payload: Value,
    fanout_hub: Option<&Arc<FanoutHub>>,
) -> Result<data_proto::WorkflowRunEvent> {
    let event = data_proto::WorkflowRunEvent {
        id: crate::control::uuid::v7(),
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
    if let Some(fanout_hub) = fanout_hub {
        let key = WorkflowFanoutKey::new(run.ns.clone(), run.workflow.clone(), run.id.clone());
        fanout_hub.publish_workflow_event(&key, event.clone()).await;
    }
    Ok(event)
}

pub fn parse_json_or(input: &str, default: Value) -> Result<Value> {
    if input.trim().is_empty() {
        return Ok(default);
    }
    Ok(serde_json::from_str(input)?)
}

use prost::Message;
