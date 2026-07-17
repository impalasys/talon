// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use prost::Message;
use std::collections::HashMap;

use crate::control::resource_model;
use crate::control::resources::ResourceStore;
use crate::control::{keys, ControlPlane, ListOptions};
use crate::gateway::rpc::{data_proto, resources_proto};

// Task resource label: marks a Task as created through agent delegation.
pub const LABEL_DELEGATION: &str = "talon.impalasys.com/delegation";
// Task resource label: namespace of the agent that owns the delegated Task.
// Also copied onto delegate sessions/messages for wakeup routing.
pub const LABEL_OWNER_NAMESPACE: &str = "talon.impalasys.com/owner-namespace";
// Task resource label: name of the agent that owns the delegated Task.
// Also copied onto delegate sessions/messages for wakeup routing.
pub const LABEL_OWNER_NAME: &str = "talon.impalasys.com/owner-name";
// Task resource label: owner session that should receive review/failure wakeups.
// Also copied onto delegate sessions/messages for wakeup routing.
pub const LABEL_OWNER_SESSION_ID: &str = "talon.impalasys.com/owner-session-id";
// Task resource label: namespace of the agent assigned to execute the Task.
// Also copied onto owner wake messages to identify the delegate.
pub const LABEL_DELEGATE_NAMESPACE: &str = "talon.impalasys.com/delegate-namespace";
// Task resource label: name of the agent assigned to execute the Task.
// Also copied onto owner wake messages to identify the delegate.
pub const LABEL_DELEGATE_NAME: &str = "talon.impalasys.com/delegate-name";
// Task resource label: declared A2A connection used to resolve the delegate.
// Also copied onto delegate sessions/messages for traceability.
pub const LABEL_A2A_CONNECTION: &str = "talon.impalasys.com/a2a-connection";

// Delegate session/message label: namespace of the associated Task resource.
// Also copied onto owner wake messages so review messages point back to Task.
pub const LABEL_TASK_NAMESPACE: &str = "talon.impalasys.com/task-namespace";
// Delegate session/message label: name of the associated Task resource.
// Also copied onto owner wake messages so review messages point back to Task.
pub const LABEL_TASK_NAME: &str = "talon.impalasys.com/task-name";
// Session/message label: role in the delegation flow, such as delegate.
pub const LABEL_TASK_ROLE: &str = "talon.impalasys.com/task-role";

// Task status condition: tracks whether delegate execution is running,
// completed, or failed.
pub const CONDITION_DELEGATED_EXECUTION: &str = "DelegatedExecution";
#[derive(Clone, Debug)]
pub struct TaskDelegationRequest {
    pub namespace: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub task_type: String,
    pub owner_namespace: String,
    pub owner_name: String,
    pub owner_session_id: String,
    pub connection_name: String,
    pub delegate_namespace: String,
    pub delegate_name: String,
}

pub async fn create_delegated_task(
    cp: &ControlPlane,
    req: TaskDelegationRequest,
) -> Result<resources_proto::Task> {
    let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());

    let now = chrono::Utc::now().timestamp_micros();
    let task_resource = resource_model::task_resource(
        req.namespace.clone(),
        req.name.clone(),
        resources_proto::TaskSpec {
            title: req.title.clone(),
            description: req.description.clone(),
            r#type: req.task_type.clone(),
            owner: Some(resources_proto::ResourceRef {
                namespace: req.owner_namespace.clone(),
                name: req.owner_name.clone(),
            }),
            delegate: Some(resources_proto::ResourceRef {
                namespace: req.delegate_namespace.clone(),
                name: req.delegate_name.clone(),
            }),
        },
        resources_proto::TaskStatus {
            observed_generation: 0,
            phase: resources_proto::TaskPhase::Queued as i32,
            conditions: Vec::new(),
            progress_summary: "Task created; waiting for delegated execution.".to_string(),
            result_artifacts: Vec::new(),
            output_artifact_uris: Vec::new(),
            created_at: now,
            updated_at: now,
            completed_at: 0,
            expires_at: 0,
            execution_ref: Some(resources_proto::TaskExecutionRef {
                kind: "AGENT_SESSION".to_string(),
                namespace: req.delegate_namespace.clone(),
                name: req.delegate_name.clone(),
                session_id: String::new(),
                run_id: String::new(),
            }),
        },
        task_resource_labels(&req),
    );
    let created = store.upsert(&req.namespace, task_resource).await?;
    let task = task_from_resource(created).context("invalid Task after create")?;

    Ok(task)
}

pub async fn complete_delegated_task_from_session(
    cp: &ControlPlane,
    session: &data_proto::Session,
    completion_status: DelegatedSessionCompletion,
) -> Result<Option<resources_proto::Task>> {
    if session.labels.get(LABEL_TASK_ROLE).map(String::as_str) != Some("delegate") {
        return Ok(None);
    }
    let Some(task_namespace) = session.labels.get(LABEL_TASK_NAMESPACE) else {
        return Ok(None);
    };
    let Some(task_name) = session.labels.get(LABEL_TASK_NAME) else {
        return Ok(None);
    };

    let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    let task_resource = store.get(task_namespace, "Task", task_name).await?;
    let Some(task) = task_resource.and_then(task_from_resource) else {
        tracing::warn!(
            task_namespace = %task_namespace,
            task_name = %task_name,
            session = %session.id,
            "delegated session completed but Task was not found"
        );
        return Ok(None);
    };
    let current_session_id = task
        .status
        .as_ref()
        .and_then(|status| status.execution_ref.as_ref())
        .map(|execution| execution.session_id.as_str())
        .unwrap_or("");
    if current_session_id != session.id {
        tracing::info!(
            task_namespace = %task_namespace,
            task_name = %task_name,
            stale_session = %session.id,
            active_session = %current_session_id,
            "ignoring completion event from stale delegated session"
        );
        return Ok(Some(task));
    }
    let current_phase = task
        .status
        .as_ref()
        .map(|status| status.phase)
        .unwrap_or(resources_proto::TaskPhase::Unspecified as i32);
    if current_phase == resources_proto::TaskPhase::NeedsReview as i32
        && completion_status == DelegatedSessionCompletion::Completed
    {
        return Ok(Some(task));
    }
    if task_phase_is_terminal(current_phase) {
        return Ok(Some(task));
    }

    let now = chrono::Utc::now().timestamp_micros();
    let progress_summary = match completion_status {
        DelegatedSessionCompletion::Completed => {
            latest_assistant_text(cp, &session.ns, &session.agent, &session.id)
                .await?
                .map(|text| text_preview(&text, 1200))
                .unwrap_or_else(|| {
                    "Delegated execution completed; no assistant text was produced.".to_string()
                })
        }
        DelegatedSessionCompletion::Failed => {
            "Delegated session failed before completing the Task.".to_string()
        }
    };

    let mut skipped_stale = false;
    let mut skipped_existing_phase = false;
    let updated =
        patch_task_status_with(&store, task_namespace, task_name, |status, generation| {
            let current_session_id = status
                .execution_ref
                .as_ref()
                .map(|execution| execution.session_id.as_str())
                .unwrap_or("");
            if current_session_id != session.id {
                skipped_stale = true;
                return Ok(());
            }
            if status.phase == resources_proto::TaskPhase::NeedsReview as i32
                && completion_status == DelegatedSessionCompletion::Completed
            {
                skipped_existing_phase = true;
                return Ok(());
            }
            if task_phase_is_terminal(status.phase) {
                skipped_existing_phase = true;
                return Ok(());
            }
            status.updated_at = now;
            match completion_status {
                DelegatedSessionCompletion::Completed => {
                    status.phase = resources_proto::TaskPhase::NeedsReview as i32;
                    status.progress_summary = progress_summary.clone();
                    set_condition(
                        status,
                        resources_proto::ResourceCondition {
                            r#type: CONDITION_DELEGATED_EXECUTION.to_string(),
                            status: "True".to_string(),
                            reason: "SessionCompleted".to_string(),
                            message: "Delegated session completed.".to_string(),
                            last_transition_time: now,
                            observed_generation: generation,
                        },
                    );
                }
                DelegatedSessionCompletion::Failed => {
                    status.phase = resources_proto::TaskPhase::Failed as i32;
                    status.completed_at = now;
                    status.progress_summary = progress_summary.clone();
                    set_condition(
                        status,
                        resources_proto::ResourceCondition {
                            r#type: CONDITION_DELEGATED_EXECUTION.to_string(),
                            status: "False".to_string(),
                            reason: "SessionFailed".to_string(),
                            message: "Delegated session failed.".to_string(),
                            last_transition_time: now,
                            observed_generation: generation,
                        },
                    );
                }
            }
            Ok(())
        })
        .await?;
    let task = task_from_resource(updated).context("invalid Task after delegated completion")?;
    if skipped_stale {
        return Ok(Some(task));
    }
    if skipped_existing_phase {
        return Ok(Some(task));
    }
    Ok(Some(task))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DelegatedSessionCompletion {
    Completed,
    Failed,
}

pub async fn mark_task_execution_started(
    cp: &ControlPlane,
    req: &TaskDelegationRequest,
    session_id: &str,
    submission_id: Option<&str>,
) -> Result<resources_proto::Task> {
    let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    let now = chrono::Utc::now().timestamp_micros();
    let updated =
        patch_task_status_with(&store, &req.namespace, &req.name, |status, generation| {
            status.phase = resources_proto::TaskPhase::Running as i32;
            status.progress_summary = "Delegated execution started.".to_string();
            status.updated_at = now;
            status.execution_ref = Some(resources_proto::TaskExecutionRef {
                kind: "AGENT_SESSION".to_string(),
                namespace: req.delegate_namespace.clone(),
                name: req.delegate_name.clone(),
                session_id: session_id.to_string(),
                run_id: submission_id.unwrap_or_default().to_string(),
            });
            set_condition(
                status,
                resources_proto::ResourceCondition {
                    r#type: CONDITION_DELEGATED_EXECUTION.to_string(),
                    status: "Unknown".to_string(),
                    reason: "SessionRunning".to_string(),
                    message: "Delegated session is running.".to_string(),
                    last_transition_time: now,
                    observed_generation: generation,
                },
            );
            Ok(())
        })
        .await?;
    task_from_resource(updated).context("invalid Task after execution start")
}

pub async fn mark_task_dispatch_failed(
    cp: &ControlPlane,
    req: &TaskDelegationRequest,
    message: &str,
) -> Result<()> {
    let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    mark_task_failed(&store, &req.namespace, &req.name, message).await
}

async fn mark_task_failed(
    store: &ResourceStore,
    namespace: &str,
    name: &str,
    message: &str,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp_micros();
    patch_task_status_with(store, namespace, name, |status, generation| {
        status.phase = resources_proto::TaskPhase::Failed as i32;
        status.progress_summary = message.to_string();
        status.updated_at = now;
        status.completed_at = now;
        set_condition(
            status,
            resources_proto::ResourceCondition {
                r#type: CONDITION_DELEGATED_EXECUTION.to_string(),
                status: "False".to_string(),
                reason: "DispatchFailed".to_string(),
                message: message.to_string(),
                last_transition_time: now,
                observed_generation: generation,
            },
        );
        Ok(())
    })
    .await?;
    Ok(())
}

async fn patch_task_status_with<F>(
    store: &ResourceStore,
    namespace: &str,
    name: &str,
    mut update: F,
) -> Result<resources_proto::Resource>
where
    F: FnMut(&mut resources_proto::TaskStatus, u64) -> Result<()>,
{
    store
        .patch_status_with(namespace, "Task", name, None, |metadata, status| {
            let observed_generation = metadata.map(|metadata| metadata.generation).unwrap_or(0);
            let mut task_status = match status.kind.take() {
                Some(resources_proto::resource_status::Kind::Task(status)) => status,
                _ => resources_proto::TaskStatus::default(),
            };
            update(&mut task_status, observed_generation)?;
            status.kind = Some(resources_proto::resource_status::Kind::Task(task_status));
            Ok(())
        })
        .await
}

fn task_from_resource(resource: resources_proto::Resource) -> Option<resources_proto::Task> {
    let spec = match resource.spec?.kind? {
        resources_proto::resource_spec::Kind::Task(spec) => spec,
        _ => return None,
    };
    let status = match resource.status.and_then(|status| status.kind) {
        Some(resources_proto::resource_status::Kind::Task(status)) => Some(status),
        _ => None,
    };
    Some(resources_proto::Task {
        metadata: resource.metadata,
        spec: Some(spec),
        status,
    })
}

fn task_resource_labels(req: &TaskDelegationRequest) -> HashMap<String, String> {
    HashMap::from([
        (LABEL_DELEGATION.to_string(), "true".to_string()),
        (
            LABEL_OWNER_NAMESPACE.to_string(),
            req.owner_namespace.clone(),
        ),
        (LABEL_OWNER_NAME.to_string(), req.owner_name.clone()),
        (
            LABEL_OWNER_SESSION_ID.to_string(),
            req.owner_session_id.clone(),
        ),
        (
            LABEL_DELEGATE_NAMESPACE.to_string(),
            req.delegate_namespace.clone(),
        ),
        (LABEL_DELEGATE_NAME.to_string(), req.delegate_name.clone()),
        (
            LABEL_A2A_CONNECTION.to_string(),
            req.connection_name.clone(),
        ),
    ])
}

pub fn task_execution_labels(req: &TaskDelegationRequest) -> HashMap<String, String> {
    let mut labels = task_resource_labels(req);
    labels.extend([
        (LABEL_TASK_NAMESPACE.to_string(), req.namespace.clone()),
        (LABEL_TASK_NAME.to_string(), req.name.clone()),
        (LABEL_TASK_ROLE.to_string(), "delegate".to_string()),
        (
            LABEL_OWNER_SESSION_ID.to_string(),
            req.owner_session_id.clone(),
        ),
    ]);
    labels
}

fn set_condition(
    status: &mut resources_proto::TaskStatus,
    condition: resources_proto::ResourceCondition,
) {
    if let Some(existing) = status
        .conditions
        .iter_mut()
        .find(|existing| existing.r#type == condition.r#type)
    {
        *existing = condition;
    } else {
        status.conditions.push(condition);
    }
}

pub fn delegated_task_message(req: &TaskDelegationRequest) -> String {
    format!(
        "You have been assigned a Talon Task.\n\nTask: {}\nTask name: {}\nTask ID: {}/{}\nOwner: {}/{}\n\nThis Task is your durable work context. Do not rely on a final assistant response to deliver results. When the work is ready for owner review, call update_task with the Task name above, set phase to NEEDS_REVIEW, include a concise progress_summary, and attach any output artifact URI with output_artifact_uri or output_artifact_uris. Task output artifact URIs automatically grant access to the owner session. Then finish the assignment by calling agent_send with target \"owner\" and a short review-ready notification. Use the Task name above for update_task.name; the full Task ID is only for display.\n\nInstructions:\n{}",
        req.title,
        req.name,
        req.namespace,
        req.name,
        req.owner_namespace,
        req.owner_name,
        req.description
    )
}

async fn latest_assistant_text(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
) -> Result<Option<String>> {
    let prefix = keys::session_message_prefix(ns, agent, session_id);
    let mut before_name = None;
    loop {
        let entries = cp
            .kv
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
            return Ok(None);
        }
        before_name = entries.last().map(|(key, _)| key.name.clone());
        for (_, bytes) in entries {
            let message = data_proto::SessionMessage::decode(bytes.as_slice())?;
            if message.role != data_proto::MessageRole::RoleAssistant as i32 {
                continue;
            }
            let text = message
                .parts
                .iter()
                .filter(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
                .map(|part| part.content.as_str())
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
            if !text.is_empty() {
                return Ok(Some(text));
            }
        }
    }
}

fn task_phase_is_terminal(phase: i32) -> bool {
    phase == resources_proto::TaskPhase::Succeeded as i32
        || phase == resources_proto::TaskPhase::Failed as i32
        || phase == resources_proto::TaskPhase::Canceled as i32
        || phase == resources_proto::TaskPhase::Expired as i32
}

fn text_preview(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let mut preview = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        preview.push_str("...");
    }
    preview
}
