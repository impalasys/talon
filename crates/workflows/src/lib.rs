// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub(crate) mod talon;

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use prost::Message;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use talon::control::events;
use talon::control::keys;
use talon::control::resource_model::TypedResource;
use talon::control::topics;
use talon::control::ControlPlane;
use talon::control::KeyValueStore;
use talon::control::ListOptions;
use talon::control::MessagePublisher;
use talon::control::ProtoKeyValueStoreExt;
use talon::fanout::FanoutHub;
use talon::fanout::WorkflowFanoutKey;
use talon::harness::knowledge::KvKnowledgeBook;
use talon::protocol::data_proto;
use talon::protocol::resources_proto;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct WorkflowReplyOutput {
    text: String,
    silent: bool,
    attachments: Vec<data_proto::ObjectRef>,
}

pub use talon::control::workflow_state::*;

pub fn workflow_claim_timeout_micros() -> i64 {
    std::env::var("TALON_WORKFLOW_CLAIM_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_WORKFLOW_CLAIM_TIMEOUT_SECONDS)
        * 1_000_000
}

async fn dispatch_parent_workflow_from_run_labels(
    cp: &ControlPlane,
    run: &data_proto::WorkflowRun,
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
) -> Result<Option<data_proto::WorkflowRun>> {
    let key = keys::workflow_run(ns, workflow, run_id);
    let claim_expires_at = now
        .timestamp_micros()
        .saturating_add(workflow_claim_timeout_micros());

    for _ in 0..MAX_CAS_RETRIES {
        let current = kv.get(&key).await?;
        let Some(current_bytes) = current.as_ref() else {
            return Ok(None);
        };
        let mut run = data_proto::WorkflowRun::decode(current_bytes.as_slice())?;
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

pub async fn advance_run(cp: &ControlPlane, run: data_proto::WorkflowRun) -> Result<()> {
    advance_run_inner(cp, run, None).await
}

pub async fn advance_run_with_fanout(
    cp: &ControlPlane,
    run: data_proto::WorkflowRun,
    fanout_hub: Arc<FanoutHub>,
) -> Result<()> {
    advance_run_inner(cp, run, Some(&fanout_hub)).await
}

async fn advance_run_inner(
    cp: &ControlPlane,
    mut run: data_proto::WorkflowRun,
    fanout_hub: Option<&Arc<FanoutHub>>,
) -> Result<()> {
    let claimed_run = run.clone();
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
                    if retry_abandoned_starting_step(cp, &run, step, &mut step_runs, fanout_hub)
                        .await?
                    {
                        progressed_this_round = true;
                        made_progress = true;
                        should_redispatch = true;
                    } else {
                        waiting = true;
                    }
                } else if status == STATUS_WAITING_RETRY {
                    if try_retry_step(cp, &run, step, &mut step_runs, fanout_hub).await? {
                        progressed_this_round = true;
                        made_progress = true;
                        should_redispatch = true;
                    } else {
                        waiting = true;
                    }
                } else if status == STATUS_WAITING_CHILD_SESSION {
                    if try_complete_agent_step(cp, &run, step, &mut step_runs, fanout_hub).await? {
                        progressed_this_round = true;
                        made_progress = true;
                        should_redispatch = true;
                    } else {
                        waiting = true;
                    }
                } else if status == STATUS_WAITING_CHILD_WORKFLOW {
                    if try_complete_workflow_step(cp, &run, step, &mut step_runs, fanout_hub)
                        .await?
                    {
                        progressed_this_round = true;
                        made_progress = true;
                        should_redispatch = true;
                    } else {
                        waiting = true;
                    }
                } else if status == STATUS_SUSPENDED {
                    if try_complete_resumed_step(cp, &run, step, &mut step_runs, fanout_hub).await?
                    {
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
                append_run_event_with_fanout(
                    cp,
                    &run,
                    &step.id,
                    "step_skipped",
                    "step condition evaluated false",
                    Value::Null,
                    fanout_hub,
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

            append_run_event_with_fanout(
                cp,
                &run,
                &step.id,
                "step_started",
                "step started",
                Value::Null,
                fanout_hub,
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
                    step_run =
                        apply_failed_retry_policy(cp, &run, step, step_run, fanout_hub).await?;
                    persist_step_run(cp, &run, &step_run).await?;
                    if step_run.status == STATUS_FAILED {
                        let error = step_run.error.clone();
                        append_run_event_with_fanout(
                            cp,
                            &run,
                            &step.id,
                            "step_failed",
                            &error,
                            json!({ "error": error }),
                            fanout_hub,
                        )
                        .await?;
                    } else if step_run.status == STATUS_WAITING_RETRY {
                        waiting = true;
                    } else {
                        append_run_event_with_fanout(
                            cp,
                            &run,
                            &step.id,
                            "step_completed",
                            "step completed",
                            parse_json_or(&step_run.output_json, Value::Null)?,
                            fanout_hub,
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
                    append_run_event_with_fanout(
                        cp,
                        &run,
                        &step.id,
                        "run_suspended",
                        "workflow run suspended",
                        parse_json_or(&step_run.suspend_json, Value::Null)?,
                        fanout_hub,
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

    let final_run_event = if any_failed(&step_runs) {
        run.status = STATUS_FAILED.to_string();
        run.error = first_error(&step_runs);
        Some((
            "run_failed".to_string(),
            "workflow run failed".to_string(),
            json!({ "error": run.error }),
        ))
    } else if all_terminal(&spec, &step_runs) {
        let view = run_view(&run, &step_runs)?;
        let output_template = parse_json_or(&spec.output_json, Value::Object(Map::new()))?;
        let output = render_value(&output_template, &view)?;
        validate_basic_json_schema("output", &spec.output_schema_json, &output)?;
        run.output_json = serde_json::to_string(&output)?;
        run.status = STATUS_COMPLETED.to_string();
        Some((
            "run_completed".to_string(),
            "workflow run completed".to_string(),
            output,
        ))
    } else if suspended {
        run.status = STATUS_SUSPENDED.to_string();
        None
    } else if waiting {
        run.status = STATUS_WAITING_CHILDREN.to_string();
        None
    } else {
        run.status = STATUS_FAILED.to_string();
        run.error = "workflow made no progress and has pending steps".to_string();
        Some((
            "run_failed".to_string(),
            run.error.clone(),
            json!({ "error": run.error }),
        ))
    };

    run.claim_expires_at = None;
    run.updated_at = Utc::now().timestamp_micros();
    if !persist_claimed_run(cp.kv.as_ref(), &claimed_run, &run).await? {
        tracing::warn!(
            namespace = %run.ns,
            workflow = %run.workflow,
            run_id = %run.id,
            claim_owner = %claimed_run.claim_owner,
            claim_attempt = claimed_run.claim_attempt,
            "Skipping workflow run persistence because the claim is no longer current"
        );
        return Ok(());
    }

    if let Some((event_type, message, payload)) = final_run_event {
        append_run_event_with_fanout(cp, &run, "", &event_type, &message, payload, fanout_hub)
            .await?;
    }

    if run.status == STATUS_COMPLETED {
        dispatch_workflow_reply(cp, &run, fanout_hub).await?;
    }

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
    Completed(data_proto::WorkflowStepRun),
    Waiting(data_proto::WorkflowStepRun),
    Suspended(data_proto::WorkflowStepRun),
}

async fn start_step(
    cp: &ControlPlane,
    run: &data_proto::WorkflowRun,
    step: &resources_proto::WorkflowStep,
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
    run: &data_proto::WorkflowRun,
    step: &resources_proto::WorkflowStep,
    rendered_input: Value,
    attempt: u32,
) -> Result<StepStartOutcome> {
    let book = KvKnowledgeBook::new(cp.kv.clone());
    let result =
        talon::harness::knowledge::execute_tool(&book, &run.ns, &step.tool, &rendered_input)
            .await?;
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
    run: &data_proto::WorkflowRun,
    step: &resources_proto::WorkflowStep,
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
        talon::control::scheduling::create_session_with_labels(cp, &run.ns, &step.agent, labels)
            .await?;
    talon::control::scheduling::send_message(
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
    run: &data_proto::WorkflowRun,
    step: &resources_proto::WorkflowStep,
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
        .get_msg::<resources_proto::Workflow>(&keys::workflow(&run.ns, &workflow_name))
        .await?
        .ok_or_else(|| anyhow!("child workflow '{}' not found", workflow_name))?;
    let mut labels = run.labels.clone();
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

async fn dispatch_workflow_reply(
    cp: &ControlPlane,
    run: &data_proto::WorkflowRun,
    fanout_hub: Option<&Arc<FanoutHub>>,
) -> Result<()> {
    let output = parse_json_or(&run.output_json, Value::Object(Map::new()))?;
    let Some(reply_value) = output.get("reply").cloned() else {
        return Ok(());
    };
    let reply: WorkflowReplyOutput = serde_json::from_value(reply_value)
        .map_err(|err| anyhow!("workflow reply output is invalid: {}", err))?;
    if reply.silent || (reply.text.trim().is_empty() && reply.attachments.is_empty()) {
        return Ok(());
    }

    let delivery_id = format!("workflow-reply-{}", run.id);
    let reply_mode = run
        .labels
        .get(talon::harness::connector::LABEL_CONNECTOR_REPLY_MODE)
        .map(|value| value.as_str())
        .unwrap_or_default();
    let result = if run
        .labels
        .contains_key(talon::harness::connector::LABEL_CONNECTOR_REGISTRATION)
    {
        talon::harness::connector::deliver_connector_reply_from_labels(
            cp,
            &run.labels,
            &run.ns,
            &delivery_id,
            &reply.text,
            reply.attachments,
            reply_mode,
        )
        .await
    } else {
        return Ok(());
    };

    match result {
        Ok(()) => {
            append_run_event_with_fanout(
                cp,
                run,
                "",
                "reply_sent",
                "workflow reply dispatched",
                json!({ "deliveryId": delivery_id }),
                fanout_hub,
            )
            .await?;
        }
        Err(err) => {
            append_run_event_with_fanout(
                cp,
                run,
                "",
                "reply_failed",
                "workflow reply dispatch failed",
                json!({ "error": err.to_string() }),
                fanout_hub,
            )
            .await?;
        }
    }
    Ok(())
}

async fn try_complete_agent_step(
    cp: &ControlPlane,
    run: &data_proto::WorkflowRun,
    step: &resources_proto::WorkflowStep,
    step_runs: &mut HashMap<String, data_proto::WorkflowStepRun>,
    fanout_hub: Option<&Arc<FanoutHub>>,
) -> Result<bool> {
    let Some(current) = step_runs.get(&step.id).cloned() else {
        return Ok(false);
    };
    if let Some(failed) = timed_out_step(cp, run, step, current.clone(), fanout_hub).await? {
        persist_step_run(cp, run, &failed).await?;
        step_runs.insert(step.id.clone(), failed);
        return Ok(true);
    }
    let Some(session) = cp
        .kv
        .get_msg::<data_proto::Session>(&keys::session(
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
        let failed = apply_failed_retry_policy(cp, run, step, failed, fanout_hub).await?;
        persist_step_run(cp, run, &failed).await?;
        if failed.status == STATUS_WAITING_RETRY {
            append_run_event_with_fanout(
                cp,
                run,
                &step.id,
                "step_retry_scheduled",
                &failed.error,
                json!({ "error": failed.error, "nextRetryAt": failed.next_retry_at }),
                fanout_hub,
            )
            .await?;
        } else {
            append_run_event_with_fanout(
                cp,
                run,
                &step.id,
                "step_failed",
                &failed.error,
                json!({ "error": failed.error }),
                fanout_hub,
            )
            .await?;
        }
        step_runs.insert(step.id.clone(), failed);
        return Ok(true);
    }
    let text = latest_assistant_text(cp.kv.as_ref(), &run.ns, &step.agent, &session.id).await?;
    let output_result = match apply_output_policy_with_repair(step, &text) {
        Ok(output_result) => output_result,
        Err(err) => {
            let assistant_text_preview = text_preview(&text, WORKFLOW_OUTPUT_PREVIEW_CHARS);
            let assistant_text_length = text.chars().count();
            tracing::warn!(
                namespace = %run.ns,
                workflow = %run.workflow,
                run_id = %run.id,
                step_id = %step.id,
                agent = %step.agent,
                session_id = %session.id,
                attempt = current.attempt,
                assistant_text_length,
                assistant_text_preview = %assistant_text_preview,
                error = %err,
                "workflow step output parsing failed"
            );
            let mut failed = current;
            failed.status = STATUS_FAILED.to_string();
            failed.error = err.to_string();
            failed.updated_at = Utc::now().timestamp_micros();
            let failed = apply_failed_retry_policy(cp, run, step, failed, fanout_hub).await?;
            persist_step_run(cp, run, &failed).await?;
            if failed.status == STATUS_WAITING_RETRY {
                append_run_event_with_fanout(
                    cp,
                    run,
                    &step.id,
                    "step_retry_scheduled",
                    &failed.error,
                    json!({
                        "error": failed.error,
                        "nextRetryAt": failed.next_retry_at,
                        "assistantTextPreview": assistant_text_preview,
                        "assistantTextLength": assistant_text_length,
                    }),
                    fanout_hub,
                )
                .await?;
            } else {
                append_run_event_with_fanout(
                    cp,
                    run,
                    &step.id,
                    "step_failed",
                    &failed.error,
                    json!({
                        "error": failed.error,
                        "assistantTextPreview": assistant_text_preview,
                        "assistantTextLength": assistant_text_length,
                    }),
                    fanout_hub,
                )
                .await?;
            }
            step_runs.insert(step.id.clone(), failed);
            return Ok(true);
        }
    };
    let output = output_result.output;
    let output_repair = output_result.repair;
    let mut completed = current;
    completed.status = STATUS_COMPLETED.to_string();
    completed.output_json = serde_json::to_string(&output)?;
    completed.updated_at = Utc::now().timestamp_micros();
    persist_step_run(cp, run, &completed).await?;
    let mut event_payload = output.clone();
    if let Some(repair) = output_repair {
        event_payload = json!({
            "output": output,
            "outputRepair": repair,
        });
    }
    append_run_event_with_fanout(
        cp,
        run,
        &step.id,
        "step_completed",
        "agent step completed",
        event_payload,
        fanout_hub,
    )
    .await?;
    step_runs.insert(step.id.clone(), completed);
    Ok(true)
}

async fn try_complete_workflow_step(
    cp: &ControlPlane,
    run: &data_proto::WorkflowRun,
    step: &resources_proto::WorkflowStep,
    step_runs: &mut HashMap<String, data_proto::WorkflowStepRun>,
    fanout_hub: Option<&Arc<FanoutHub>>,
) -> Result<bool> {
    let Some(current) = step_runs.get(&step.id).cloned() else {
        return Ok(false);
    };
    if let Some(failed) = timed_out_step(cp, run, step, current.clone(), fanout_hub).await? {
        persist_step_run(cp, run, &failed).await?;
        step_runs.insert(step.id.clone(), failed);
        return Ok(true);
    }
    let Some(child) = cp
        .kv
        .get_msg::<data_proto::WorkflowRun>(&keys::workflow_run(
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
        append_run_event_with_fanout(
            cp,
            run,
            &step.id,
            "step_completed",
            "child workflow step completed",
            output,
            fanout_hub,
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
    run: &data_proto::WorkflowRun,
    step: &resources_proto::WorkflowStep,
    step_runs: &mut HashMap<String, data_proto::WorkflowStepRun>,
    fanout_hub: Option<&Arc<FanoutHub>>,
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
                append_run_event_with_fanout(
                    cp,
                    run,
                    &step.id,
                    "step_completed",
                    "wait step completed",
                    output,
                    fanout_hub,
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
    append_run_event_with_fanout(
        cp,
        run,
        &step.id,
        "step_completed",
        "resumed step completed",
        resume,
        fanout_hub,
    )
    .await?;
    step_runs.insert(step.id.clone(), completed);
    Ok(true)
}

async fn persist_claimed_run(
    kv: &dyn KeyValueStore,
    claimed: &data_proto::WorkflowRun,
    updated: &data_proto::WorkflowRun,
) -> Result<bool> {
    let key = keys::workflow_run(&claimed.ns, &claimed.workflow, &claimed.id);
    for _ in 0..MAX_CAS_RETRIES {
        let Some(current_bytes) = kv.get(&key).await? else {
            return Ok(false);
        };
        let current = data_proto::WorkflowRun::decode(current_bytes.as_slice())?;
        if current.status != STATUS_RUNNING
            || current.claim_owner != claimed.claim_owner
            || current.claim_attempt != claimed.claim_attempt
            || current.claim_expires_at != claimed.claim_expires_at
        {
            return Ok(false);
        }
        if kv
            .compare_and_swap(
                &key,
                Some(current_bytes.as_slice()),
                &updated.encode_to_vec(),
            )
            .await?
        {
            return Ok(true);
        }
    }
    Err(anyhow!(
        "failed to persist workflow run after concurrent modifications"
    ))
}

fn workflow_claim_owner() -> String {
    talon::control::registration::worker_id()
}

fn workflow_concurrency(spec: &resources_proto::WorkflowSpec) -> usize {
    if spec.concurrency == 0 {
        usize::MAX
    } else {
        spec.concurrency as usize
    }
}

fn active_step_count(step_runs: &HashMap<String, data_proto::WorkflowStepRun>) -> usize {
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
    spec: &resources_proto::WorkflowSpec,
    step_runs: &HashMap<String, data_proto::WorkflowStepRun>,
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
    run: &data_proto::WorkflowRun,
    step_run: &data_proto::WorkflowStepRun,
) -> Result<bool> {
    let key = keys::workflow_step_run(&run.ns, &run.workflow, &run.id, &step_run.id);
    cp.kv
        .compare_and_swap(&key, None, &step_run.encode_to_vec())
        .await
}

fn apply_waiting_step_metadata(
    step: &resources_proto::WorkflowStep,
    step_run: &mut data_proto::WorkflowStepRun,
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
    run: &data_proto::WorkflowRun,
    step: &resources_proto::WorkflowStep,
    mut step_run: data_proto::WorkflowStepRun,
    fanout_hub: Option<&Arc<FanoutHub>>,
) -> Result<Option<data_proto::WorkflowStepRun>> {
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
        apply_failed_retry_policy(cp, run, step, step_run, fanout_hub).await?,
    ))
}

async fn retry_abandoned_starting_step(
    cp: &ControlPlane,
    run: &data_proto::WorkflowRun,
    step: &resources_proto::WorkflowStep,
    step_runs: &mut HashMap<String, data_proto::WorkflowStepRun>,
    fanout_hub: Option<&Arc<FanoutHub>>,
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
    let failed = apply_failed_retry_policy(cp, run, step, failed, fanout_hub).await?;
    persist_step_run(cp, run, &failed).await?;
    step_runs.insert(step.id.clone(), failed);
    Ok(true)
}

async fn try_retry_step(
    cp: &ControlPlane,
    run: &data_proto::WorkflowRun,
    step: &resources_proto::WorkflowStep,
    step_runs: &mut HashMap<String, data_proto::WorkflowStepRun>,
    fanout_hub: Option<&Arc<FanoutHub>>,
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
    append_run_event_with_fanout(
        cp,
        run,
        &step.id,
        "step_started",
        "retry step started",
        Value::Null,
        fanout_hub,
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
        step_run = apply_failed_retry_policy(cp, run, step, step_run, fanout_hub).await?;
    }
    persist_step_run(cp, run, &step_run).await?;
    step_runs.insert(step.id.clone(), step_run);
    Ok(true)
}

async fn apply_failed_retry_policy(
    cp: &ControlPlane,
    run: &data_proto::WorkflowRun,
    step: &resources_proto::WorkflowStep,
    mut step_run: data_proto::WorkflowStepRun,
    fanout_hub: Option<&Arc<FanoutHub>>,
) -> Result<data_proto::WorkflowStepRun> {
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
    append_run_event_with_fanout(
        cp,
        run,
        &step.id,
        "step_retry_scheduled",
        &step_run.error,
        json!({ "attempt": step_run.attempt, "nextRetryAt": next_retry_at }),
        fanout_hub,
    )
    .await?;
    Ok(step_run)
}

fn retry_backoff_seconds(retry: &resources_proto::WorkflowStepRetryPolicy, attempt: u32) -> i64 {
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
    run: &data_proto::WorkflowRun,
    step: &resources_proto::WorkflowStep,
    attempt: u32,
    fire_at_micros: i64,
    reason: &str,
) -> Result<talon::control::scheduler::ScheduledWakeup> {
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
        .schedule(talon::control::scheduler::ScheduleWakeupRequest {
            namespace: run.ns.clone(),
            schedule_id: format!("workflow/{}/{}/{}", run.workflow, run.id, step.id),
            revision: attempt as u64,
            fire_at,
            payload: serde_json::to_vec(
                &talon::control::scheduling::SchedulerFirePayload::Workflow(payload),
            )?,
        })
        .await
}

fn compute_wait_until_at(step: &resources_proto::WorkflowStep) -> Result<Option<i64>> {
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
        .get_msg::<data_proto::WorkflowStepRun>(&keys::workflow_step_run(
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

fn new_step_run(
    step: &resources_proto::WorkflowStep,
    status: &str,
    input_json: &str,
    output: Value,
) -> Result<data_proto::WorkflowStepRun> {
    let now = Utc::now().timestamp_micros();
    Ok(data_proto::WorkflowStepRun {
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

fn failed_step(step: &resources_proto::WorkflowStep, message: &str) -> data_proto::WorkflowStepRun {
    failed_step_with_attempt(step, message, 1)
}

fn failed_step_with_attempt(
    step: &resources_proto::WorkflowStep,
    message: &str,
    attempt: u32,
) -> data_proto::WorkflowStepRun {
    let mut step_run = new_step_run(step, STATUS_FAILED, "", Value::Null)
        .unwrap_or_else(|_| data_proto::WorkflowStepRun::default());
    step_run.attempt = attempt;
    step_run.error = message.to_string();
    step_run
}

fn dependencies_done(
    step: &resources_proto::WorkflowStep,
    step_runs: &HashMap<String, data_proto::WorkflowStepRun>,
) -> bool {
    step.after.iter().all(|dep| {
        step_runs
            .get(dep)
            .map(|run| run.status == STATUS_COMPLETED || run.status == STATUS_SKIPPED)
            .unwrap_or(false)
    })
}

fn all_terminal(
    spec: &resources_proto::WorkflowSpec,
    step_runs: &HashMap<String, data_proto::WorkflowStepRun>,
) -> bool {
    spec.steps.iter().all(|step| {
        step_runs
            .get(&step.id)
            .map(|run| run.status == STATUS_COMPLETED || run.status == STATUS_SKIPPED)
            .unwrap_or(false)
    })
}

fn any_failed(step_runs: &HashMap<String, data_proto::WorkflowStepRun>) -> bool {
    step_runs.values().any(|run| run.status == STATUS_FAILED)
}

fn first_error(step_runs: &HashMap<String, data_proto::WorkflowStepRun>) -> String {
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
    run: &data_proto::WorkflowRun,
    step_runs: &HashMap<String, data_proto::WorkflowStepRun>,
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

#[derive(Debug)]
struct OutputPolicyResult {
    output: Value,
    repair: Option<&'static str>,
}

fn apply_output_policy(step: &resources_proto::WorkflowStep, text: &str) -> Result<Value> {
    Ok(apply_output_policy_with_repair(step, text)?.output)
}

fn apply_output_policy_with_repair(
    step: &resources_proto::WorkflowStep,
    text: &str,
) -> Result<OutputPolicyResult> {
    let format = step
        .output
        .as_ref()
        .map(|output| output.format.as_str())
        .filter(|format| !format.is_empty())
        .unwrap_or("text");
    let (output, repair) = if format == "json" {
        parse_json_output(text)
            .map_err(|err| anyhow!("step '{}' expected JSON output: {}", step.id, err))?
    } else {
        (json!({ "text": text }), None)
    };
    if let Some(policy) = &step.output {
        validate_basic_json_schema("step output", &policy.schema_json, &output)?;
    }
    Ok(OutputPolicyResult { output, repair })
}

fn parse_json_output(text: &str) -> Result<(Value, Option<&'static str>)> {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => return Ok((value, None)),
        Err(raw_err) => {
            for candidate in fenced_json_candidates(text) {
                if let Ok(value) = serde_json::from_str::<Value>(&candidate) {
                    return Ok((value, Some("fenced_json")));
                }
            }
            for candidate in balanced_json_candidates(text) {
                if let Ok(value) = serde_json::from_str::<Value>(&candidate) {
                    return Ok((value, Some("balanced_json")));
                }
            }
            Err(raw_err.into())
        }
    }
}

fn fenced_json_candidates(text: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after_start = &rest[start + 3..];
        let Some(header_end) = after_start.find('\n') else {
            break;
        };
        let fence_label = after_start[..header_end].trim().to_ascii_lowercase();
        let after_header = &after_start[header_end + 1..];
        let Some(end) = after_header.find("```") else {
            break;
        };
        if fence_label.is_empty() || fence_label == "json" {
            candidates.push(after_header[..end].trim().to_string());
        }
        rest = &after_header[end + 3..];
    }
    candidates
}

fn balanced_json_candidates(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut candidates = Vec::new();
    for (start, byte) in bytes.iter().enumerate() {
        if *byte != b'{' && *byte != b'[' {
            continue;
        }
        let opening = *byte;
        let closing = if opening == b'{' { b'}' } else { b']' };
        let mut stack = vec![closing];
        let mut in_string = false;
        let mut escaped = false;
        for idx in start + 1..bytes.len() {
            let current = bytes[idx];
            if in_string {
                if escaped {
                    escaped = false;
                } else if current == b'\\' {
                    escaped = true;
                } else if current == b'"' {
                    in_string = false;
                }
                continue;
            }
            match current {
                b'"' => in_string = true,
                b'{' => stack.push(b'}'),
                b'[' => stack.push(b']'),
                b'}' | b']' => {
                    if stack.pop() != Some(current) {
                        break;
                    }
                    if stack.is_empty() {
                        candidates.push(text[start..=idx].trim().to_string());
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    candidates
}

fn text_preview(text: &str, max_chars: usize) -> String {
    let mut preview: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        preview.push_str("...");
    }
    preview
}

async fn latest_assistant_text(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
) -> Result<String> {
    let prefix = keys::session_message_prefix(ns, agent, session_id);
    let mut before_name = None;
    loop {
        let entries = kv
            .list_entries(
                &prefix,
                Some(
                    ListOptions::desc()
                        .before_name(before_name.as_deref())
                        .limit(64),
                ),
            )
            .await?;
        if entries.is_empty() {
            return Ok(String::new());
        }
        before_name = entries.last().map(|(key, _)| key.name.clone());
        for (_, bytes) in entries {
            let message = data_proto::SessionMessage::decode(bytes.as_slice())?;
            if message.role == data_proto::MessageRole::RoleAssistant as i32 {
                let text = message
                    .parts
                    .iter()
                    .filter(|part| {
                        part.part_type == data_proto::SessionMessagePartType::Text as i32
                    })
                    .map(|part| part.content.as_str())
                    .collect::<String>();
                if !text.is_empty() {
                    return Ok(text);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use crate::talon;

    use super::*;
    use async_trait::async_trait;
    use axum::{routing::post, Json, Router};
    use prost::Message;
    use serde_json::json;
    use std::sync::Arc;
    use talon::control::config::proto;
    use talon::control::config::Config;
    use talon::control::config::ProviderConfig;
    use talon::control::config::Secret;
    use talon::control::events::MessageDirection;
    use talon::control::events::SessionDispatchEvent;
    use talon::control::events::WorkflowDispatchEvent;
    use talon::control::scheduler::ScheduleWakeupRequest;
    use talon::control::scheduler::ScheduledWakeup;
    use talon::control::scheduler::SchedulerBackend;
    use talon::control::ProtoKeyValueStoreExt;
    use talon::harness::knowledge::KnowledgeBook;
    use talon::harness::knowledge::KvKnowledgeBook;
    use talon::protocol::manifests;
    use talon::test_support::workflows::*;
    use talon::test_support::MockKvStore;
    use talon::test_support::RecordingPubSub;
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

    #[tokio::test]
    async fn create_run_persists_started_event_and_publishes_only_workflow_dispatch() {
        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        let cp = ControlPlane::builder(kv.clone(), pubsub.clone()).build();
        let workflow = workflow_fixture(
            "default",
            "dispatch-only",
            resources_proto::WorkflowSpec {
                steps: vec![resources_proto::WorkflowStep {
                    id: "copy".to_string(),
                    r#type: "transform".to_string(),
                    input_json: r#"{"ok":true}"#.to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );

        let run = create_run(&cp, &workflow, "{}".to_string(), HashMap::new())
            .await
            .unwrap();

        let published = pubsub.published.lock().await;
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, topics::WORKFLOW_DISPATCH_TOPIC);
        drop(published);

        let events = kv
            .list_entries(
                &keys::workflow_run_event_prefix("default", "dispatch-only", &run.id),
                None,
            )
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let started = data_proto::WorkflowRunEvent::decode(events[0].1.as_slice()).unwrap();
        assert_eq!(started.r#type, "run_started");
    }

    #[test]
    fn json_output_policy_accepts_raw_json() {
        let (value, repair) = parse_json_output(r#"{"drafts":[]}"#).unwrap();
        assert_eq!(value, json!({"drafts": []}));
        assert_eq!(repair, None);
    }

    #[test]
    fn json_output_policy_repairs_fenced_json() {
        let (value, repair) = parse_json_output(
            r#"Here is the result:

```json
{"drafts":[{"draftId":1,"status":"drafted"}]}
```

Done."#,
        )
        .unwrap();
        assert_eq!(
            value,
            json!({"drafts": [{"draftId": 1, "status": "drafted"}]})
        );
        assert_eq!(repair, Some("fenced_json"));
    }

    #[test]
    fn json_output_policy_repairs_unlabeled_fenced_json() {
        let (value, repair) = parse_json_output(
            r#"```
[{"draftId":1,"status":"drafted"}]
```"#,
        )
        .unwrap();
        assert_eq!(value, json!([{"draftId": 1, "status": "drafted"}]));
        assert_eq!(repair, Some("fenced_json"));
    }

    #[test]
    fn json_output_policy_uses_first_valid_fence() {
        let (value, repair) = parse_json_output(
            r#"```json
{not valid}
```

```json
{"ok":true}
```"#,
        )
        .unwrap();
        assert_eq!(value, json!({"ok": true}));
        assert_eq!(repair, Some("fenced_json"));
    }

    #[test]
    fn json_output_policy_repairs_balanced_json() {
        let (value, repair) =
            parse_json_output(r#"I found this result: {"ok":true,"items":[1,2]}."#).unwrap();
        assert_eq!(value, json!({"ok": true, "items": [1, 2]}));
        assert_eq!(repair, Some("balanced_json"));
    }

    #[test]
    fn json_output_policy_still_rejects_unrecoverable_prose() {
        let err = parse_json_output("I cannot produce a draft for this input.").unwrap_err();
        assert!(err.to_string().contains("expected value"));
    }

    #[test]
    fn json_output_policy_validates_schema_after_repair() {
        let step = resources_proto::WorkflowStep {
            id: "generate_drafts".to_string(),
            output: Some(resources_proto::WorkflowStepOutputPolicy {
                format: "json".to_string(),
                schema_json: r#"{"type":"object","required":["drafts"]}"#.to_string(),
            }),
            ..Default::default()
        };
        let err = apply_output_policy_with_repair(
            &step,
            r#"```json
{"notDrafts":[]}
```"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("missing required property"));
    }

    #[test]
    fn validate_workflow_rejects_duplicate_step_ids() {
        let workflow = workflow_fixture(
            "default",
            "dupe",
            resources_proto::WorkflowSpec {
                steps: vec![
                    resources_proto::WorkflowStep {
                        id: "review".to_string(),
                        r#type: "transform".to_string(),
                        ..Default::default()
                    },
                    resources_proto::WorkflowStep {
                        id: "review".to_string(),
                        r#type: "transform".to_string(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        );

        let err = validate_workflow(&workflow).expect_err("duplicate ids should be rejected");
        assert!(err.to_string().contains("duplicate workflow step id"));

        let slash_name = workflow_fixture(
            "default",
            "bad/name",
            resources_proto::WorkflowSpec {
                steps: vec![resources_proto::WorkflowStep {
                    id: "review".to_string(),
                    r#type: "transform".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let err = validate_workflow(&slash_name).expect_err("slash names should be rejected");
        assert!(err.to_string().contains("workflow name cannot contain '/'"));
    }

    #[tokio::test]
    async fn advance_run_completes_transform_dag_and_maps_output() {
        let kv = Arc::new(MockKvStore::new());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(RecordingPubSub::default())).build();
        let workflow = workflow_fixture(
            "default",
            "copy",
            resources_proto::WorkflowSpec {
                input_schema_json: r#"{"type":"object","required":["answer"],"properties":{"answer":{"type":"string"}}}"#.to_string(),
                output_schema_json: r#"{"type":"object","required":["answer"],"properties":{"answer":{"type":"string"}}}"#.to_string(),
                steps: vec![resources_proto::WorkflowStep {
                    id: "copy".to_string(),
                    r#type: "transform".to_string(),
                    input_json: r#"{"answer":"${$.input.answer}"}"#.to_string(),
                    ..Default::default()
                }],
                output_json: r#"{"answer":"${$.steps.copy.output.answer}"}"#.to_string(),
                ..Default::default()
            },
        );

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
            .get_msg::<data_proto::WorkflowRun>(&keys::workflow_run("default", "copy", &run.id))
            .await
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(stored.status, STATUS_COMPLETED);
        assert_eq!(
            serde_json::from_str::<Value>(&stored.output_json).expect("output should be JSON"),
            json!({ "answer": "yes" })
        );
        let step = kv
            .get_msg::<data_proto::WorkflowStepRun>(&keys::workflow_step_run(
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
        let cp = ControlPlane::builder(kv.clone(), Arc::new(RecordingPubSub::default())).build();
        let book = KvKnowledgeBook::new(kv.clone());
        book.write("default", "goals.md", "ship workflow support")
            .await
            .expect("knowledge should persist");
        let workflow = workflow_fixture(
            "default",
            "search",
            resources_proto::WorkflowSpec {
                steps: vec![resources_proto::WorkflowStep {
                    id: "searchKnowledge".to_string(),
                    r#type: "tool".to_string(),
                    tool: talon::harness::knowledge::KNOWLEDGE_SEARCH_TOOL.to_string(),
                    input_json: r#"{"query":"ship"}"#.to_string(),
                    ..Default::default()
                }],
                output_json: r#"{"result":"${$.steps.searchKnowledge.output.text}"}"#.to_string(),
                ..Default::default()
            },
        );
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

        let failing = workflow_fixture(
            "default",
            "bad-json-output",
            resources_proto::WorkflowSpec {
                steps: vec![resources_proto::WorkflowStep {
                    id: "searchKnowledge".to_string(),
                    r#type: "tool".to_string(),
                    tool: talon::harness::knowledge::KNOWLEDGE_SEARCH_TOOL.to_string(),
                    input_json: r#"{"query":"ship"}"#.to_string(),
                    output: Some(resources_proto::WorkflowStepOutputPolicy {
                        format: "json".to_string(),
                        schema_json: String::new(),
                    }),
                    ..Default::default()
                }],
                output_json: "{}".to_string(),
                ..Default::default()
            },
        );
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
    async fn run_uses_spec_snapshot_even_if_workflow_is_modified() {
        let kv = Arc::new(MockKvStore::new());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(RecordingPubSub::default())).build();
        let mut workflow = workflow_fixture(
            "default",
            "snapshot",
            resources_proto::WorkflowSpec {
                steps: vec![resources_proto::WorkflowStep {
                    id: "copy".to_string(),
                    r#type: "transform".to_string(),
                    input_json: r#"{"value":"${$.input.value}"}"#.to_string(),
                    ..Default::default()
                }],
                output_json: r#"{"value":"${$.steps.copy.output.value}","version":"original"}"#
                    .to_string(),
                ..Default::default()
            },
        );
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
        let cp = ControlPlane::builder(kv.clone(), Arc::new(RecordingPubSub::default())).build();
        for agent in ["a", "b"] {
            kv.set_msg(
                &keys::agent("default", agent),
                &agent_fixture(
                    "default",
                    agent,
                    manifests::AgentSpec {
                        features: Vec::new(),
                        model_policy: None,
                        system_prompt: "test".to_string(),
                        post_history_prompt: String::new(),
                        mcp_server_refs: Vec::new(),
                        capabilities: HashMap::new(),
                        a2a: None,
                        runtime: None,
                    },
                ),
            )
            .await
            .unwrap();
        }
        let workflow = workflow_fixture(
            "default",
            "limited",
            resources_proto::WorkflowSpec {
                steps: vec![
                    resources_proto::WorkflowStep {
                        id: "first".to_string(),
                        r#type: "agent".to_string(),
                        agent: "a".to_string(),
                        prompt: "first".to_string(),
                        ..Default::default()
                    },
                    resources_proto::WorkflowStep {
                        id: "second".to_string(),
                        r#type: "agent".to_string(),
                        agent: "b".to_string(),
                        prompt: "second".to_string(),
                        ..Default::default()
                    },
                ],
                concurrency: 1,
                ..Default::default()
            },
        );
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
            .get_msg::<data_proto::WorkflowStepRun>(&keys::workflow_step_run(
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
        let cp = ControlPlane::builder(kv.clone(), pubsub.clone())
            .scheduler(scheduler.clone())
            .build();
        let workflow = workflow_fixture(
            "default",
            "timer",
            resources_proto::WorkflowSpec {
                steps: vec![resources_proto::WorkflowStep {
                    id: "sleep".to_string(),
                    r#type: "wait".to_string(),
                    wait_duration: "5m".to_string(),
                    ..Default::default()
                }],
                output_json: r#"{"firedAt":"${$.steps.sleep.output.firedAt}"}"#.to_string(),
                ..Default::default()
            },
        );
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
        let cp = ControlPlane::builder(kv.clone(), Arc::new(RecordingPubSub::default()))
            .scheduler(scheduler.clone())
            .build();
        let workflow = workflow_fixture(
            "default",
            "retrying",
            resources_proto::WorkflowSpec {
                steps: vec![resources_proto::WorkflowStep {
                    id: "unsupported".to_string(),
                    r#type: "tool".to_string(),
                    tool: "missing_tool".to_string(),
                    retry: Some(resources_proto::WorkflowStepRetryPolicy {
                        max_attempts: 2,
                        initial_backoff_seconds: 1,
                        max_backoff_seconds: 5,
                        multiplier: 2.0,
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
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
}

// The parser and runtime share the same pure validation rules.
pub use talon::protocol::workflow_validation::*;
