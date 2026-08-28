// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use prost::Message;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::control::config::{global_capability_allowed, Config};
use crate::control::resource_model::{self, TypedResource};
use crate::control::resources::ResourceStore;
use crate::control::scheduling;
use crate::control::tool_output::ToolOutputExt;
use crate::control::{delegation, keys, ControlPlane, ListOptions, ProtoKeyValueStoreExt};
use crate::gateway::rpc::{
    data_proto, manifests, protobuf_value::value::Kind as ProtoValueKind, resources_proto,
};
use crate::harness::llm::ToolOutput;
use crate::harness::skills::namespace::{self, NamespaceSkill};
use crate::harness::skills::registry::ToolRegistry;
use crate::harness::skills::render::format_active_skill_context;

mod common;
pub mod artifacts;
pub mod tasks;
pub mod files;
pub mod research;
pub mod schedules;
pub mod goals;
pub mod sessions;
pub mod dispatch;
pub use dispatch::{execute_tool, execute_tool_for_session, execute_tool_for_session_output};
pub(crate) use artifacts::{create_artifact, read_artifact, update_artifact, get_artifact_metadata, grant_artifact, artifact_content_bytes, resolve_artifact_uri, artifact_json, default_access_expiry, parse_artifact_uri};
pub use files::{ensure_file_read_namespace, find_file_by_path, parse_file_uri};
pub use goals::active_goals_context;
mod registry;
use common::{has_capability_action, normalize_logical_path, opt_str, opt_u64, opt_usize, req_str, require_capability, require_file_read, string_map, string_vec};
pub use registry::{register_channel_tools, register_skill_tools, register_tools};

#[path = "../tools/a2a.rs"]
mod a2a_tools;
#[path = "../tools/artifacts.rs"]
mod artifact_tools;
#[path = "../tools/code.rs"]
mod code_tools;
#[path = "../tools/tasks.rs"]
mod task_tools;

pub const CREATE_SCHEDULE_TOOL: &str = "create_schedule";
pub const GET_SCHEDULE_TOOL: &str = "get_schedule";
pub const LIST_SCHEDULES_TOOL: &str = "list_schedules";
pub const UPDATE_SCHEDULE_TOOL: &str = "update_schedule";
pub const DELETE_SCHEDULE_TOOL: &str = "delete_schedule";
pub const CREATE_TASK_TOOL: &str = "create_task";
pub const DELEGATE_TASK_TOOL: &str = "delegate_task";
pub const AGENT_OPEN_TOOL: &str = "agent_open";
pub const AGENT_SEND_TOOL: &str = "agent_send";
pub const AGENT_STATUS_TOOL: &str = "agent_status";
pub const AGENT_WAIT_FOR_MESSAGE_TOOL: &str = "agent_wait_for_message";
pub const GET_TASK_TOOL: &str = "get_task";
pub const LIST_TASKS_TOOL: &str = "list_tasks";
pub const UPDATE_TASK_TOOL: &str = "update_task";
pub const READ_SESSION_MESSAGES_TOOL: &str = "read_session_messages";
pub const CREATE_GOAL_TOOL: &str = "create_goal";
pub const GET_GOAL_TOOL: &str = "get_goal";
pub const LIST_GOALS_TOOL: &str = "list_goals";
pub const UPDATE_GOAL_TOOL: &str = "update_goal";
pub const COMPLETE_GOAL_TOOL: &str = "complete_goal";
pub const BLOCK_GOAL_TOOL: &str = "block_goal";
pub const CHANNEL_PUBLISH_TOOL: &str = "channel_publish";
pub const CHANNEL_SKIP_REPLY_TOOL: &str = "channel_skip_reply";
pub const ACTIVATE_SKILL_TOOL: &str = "activate_skill";
pub const DEACTIVATE_SKILL_TOOL: &str = "deactivate_skill";
pub const CREATE_ARTIFACT_TOOL: &str = "create_artifact";
pub const UPDATE_ARTIFACT_TOOL: &str = "update_artifact";
pub const READ_ARTIFACT_TOOL: &str = "read_artifact";
pub const GET_ARTIFACT_METADATA_TOOL: &str = "get_artifact_metadata";
pub const GRANT_ARTIFACT_TOOL: &str = "grant_artifact";
pub const FETCH_URL_TOOL: &str = "fetch_url";
pub const WEB_SEARCH_TOOL: &str = "web_search";
pub const SEARCH_MEMORY_TOOL: &str = "search_memory";
pub const READ_MEMORY_TOOL: &str = "read_memory";
pub const LIST_MEMORY_TOOL: &str = "list_memory";
pub const CREATE_MEMORY_TOOL: &str = "create_memory";
pub const UPDATE_MEMORY_TOOL: &str = "update_memory";
pub const LIST_FILES_TOOL: &str = "list_files";
pub const READ_FILE_TOOL: &str = "read_file";
pub const GET_FILE_METADATA_TOOL: &str = "get_file_metadata";
pub const CREATE_FILE_TOOL: &str = "create_file";
pub const UPDATE_FILE_TOOL: &str = "update_file";
pub const DELETE_FILE_TOOL: &str = "delete_file";
pub const RUN_PYTHON_CODE_TOOL: &str = "run_python_code";

pub(super) const OP_READ: &str = "read";
pub(super) const OP_METADATA: &str = "metadata";
pub(super) const OP_PROMOTE: &str = "promote";
const MAX_ACCESS_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;

pub fn tool_requests_worker_stop(name: &str) -> bool {
    name == AGENT_WAIT_FOR_MESSAGE_TOOL
}

pub(crate) async fn auto_forward_a2a_final_message(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    message: &str,
    artifact_uris: &[String],
) -> Result<bool> {
    if a2a_tools::load_wire_ref(
        cp,
        current_namespace,
        current_agent,
        current_session,
        "owner",
    )
    .await?
    .is_none()
    {
        return Ok(false);
    }

    a2a_tools::send_wire_message(
        cp,
        current_namespace,
        current_agent,
        current_session,
        "owner",
        message,
        artifact_uris,
        Default::default(),
    )
    .await?;
    Ok(true)
}

pub(crate) fn artifact_uris_from_message_text(text: &str) -> Vec<String> {
    a2a_tools::artifact_uris_from_message_text(text)
}



// dispatch moved to dispatch.rs
// moved read_session_messages -> sessions.rs





// moved list_files_tool -> files.rs

// moved read_file_tool -> files.rs

// moved get_file_metadata_tool -> files.rs

// moved create_file_tool -> files.rs

// moved update_file_tool -> files.rs

// moved delete_file_tool -> files.rs


// moved list_files_by_filter -> files.rs


// moved find_file_by_path -> files.rs

struct ReadFileObject {
    object: crate::control::object_store::StoredObject,
    object_ref: data_proto::ObjectRef,
}

// moved read_file_content -> files.rs

// moved read_file_output -> files.rs

// moved read_file_object -> files.rs


// moved upsert_file -> files.rs

// moved write_file_objects -> files.rs

// moved file_from_resource -> files.rs

// moved file_name_from_file -> files.rs

// moved file_json -> files.rs


// moved file_uri -> files.rs

// moved file_location_from_args -> files.rs

// moved namespace_arg -> files.rs

// moved parse_file_uri -> files.rs

// moved ensure_file_read_namespace -> files.rs


// moved file_resource_labels -> files.rs

// moved file_purpose_label -> files.rs

// moved file_index_policy_label -> files.rs

// moved file_retention_label -> files.rs

// moved parse_file_purpose -> files.rs

// moved parse_file_index_policy -> files.rs

// moved parse_file_retention -> files.rs

// moved normalize_enum_input -> files.rs


// moved create_artifact -> artifacts.rs

// moved read_artifact -> artifacts.rs

// moved update_artifact -> artifacts.rs

// moved get_artifact_metadata -> artifacts.rs

// moved grant_artifact -> artifacts.rs

// moved fetch_url -> research.rs

// moved web_search -> research.rs

// moved upsert_schedule -> schedules.rs

// moved schedule_json -> schedules.rs

fn create_task(
    current_namespace: &str,
    current_agent: &str,
    args: &Value,
) -> Result<resources_proto::Task> {
    let namespace = opt_str(args, "namespace")
        .unwrap_or(current_namespace)
        .to_string();
    let title = req_str(args, "title")?.to_string();
    let description = req_str(args, "description")?.to_string();
    let delegate_name = req_str(args, "delegate_name")?.to_string();
    let delegate_namespace = opt_str(args, "delegate_namespace")
        .unwrap_or(current_namespace)
        .to_string();
    let task_type = opt_str(args, "type")
        .unwrap_or("agent_delegation")
        .trim()
        .to_string();
    let now = chrono::Utc::now().timestamp_micros();
    let name = unique_task_name(&title);
    let labels = HashMap::from([
        (
            delegation::LABEL_OWNER_NAME.to_string(),
            current_agent.to_string(),
        ),
        (
            delegation::LABEL_DELEGATE_NAME.to_string(),
            delegate_name.clone(),
        ),
    ]);
    let resource = resource_model::task_resource(
        namespace,
        name,
        resources_proto::TaskSpec {
            title,
            description,
            r#type: task_type,
            owner: Some(resources_proto::ResourceRef {
                namespace: current_namespace.to_string(),
                name: current_agent.to_string(),
            }),
            delegate: Some(resources_proto::ResourceRef {
                namespace: delegate_namespace.clone(),
                name: delegate_name.clone(),
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
                namespace: delegate_namespace,
                name: delegate_name,
                session_id: String::new(),
                run_id: String::new(),
            }),
        },
        labels,
    );
    task_from_resource(resource).ok_or_else(|| anyhow!("invalid Task after create"))
}

async fn delegate_task(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
    spec: &manifests::AgentSpec,
) -> Result<resources_proto::Task> {
    let namespace = opt_str(args, "namespace")
        .unwrap_or(current_namespace)
        .to_string();
    let title = req_str(args, "title")?.to_string();
    let description = req_str(args, "description")?.to_string();
    if args.get("delegate_name").is_some() || args.get("delegate_namespace").is_some() {
        return Err(anyhow!(
            "delegate_task requires a declared A2A connection; delegate_name and delegate_namespace are not accepted"
        ));
    }
    let connection_name = req_str(args, "connection")?;
    let target = crate::harness::a2a::resolve_internal_connection(spec, connection_name)?;
    let task_type = opt_str(args, "type")
        .unwrap_or("agent_delegation")
        .trim()
        .to_string();
    let name = unique_task_name(&title);
    let alias = format!("{}-1", target.connection_name);
    ensure_delegate_wire_not_busy(
        cp,
        current_namespace,
        current_agent,
        current_session,
        &alias,
    )
    .await?;
    let req = delegation::TaskDelegationRequest {
        namespace,
        name,
        title,
        description,
        task_type,
        owner_namespace: current_namespace.to_string(),
        owner_name: current_agent.to_string(),
        owner_session_id: current_session.to_string(),
        connection_name: target.connection_name.clone(),
        delegate_namespace: target.target_namespace,
        delegate_name: target.target_agent,
    };
    let task = delegation::create_delegated_task(cp, req.clone()).await?;
    let labels = delegation::task_execution_labels(&req);
    let opened = a2a_tools::open_or_reuse_wire(
        cp,
        current_namespace,
        current_agent,
        current_session,
        spec,
        &req.connection_name,
        &alias,
        labels.clone(),
    )
    .await
    .inspect_err(|err| {
        tracing::warn!(
            task_namespace = %req.namespace,
            task_name = %req.name,
            error = %err,
            "failed to open delegated Task A2A wire"
        );
    });
    let opened = match opened {
        Ok(opened) => opened,
        Err(err) => {
            let _ = delegation::mark_task_dispatch_failed(cp, &req, &err.to_string()).await;
            return Err(err);
        }
    };
    let sent = a2a_tools::send_wire_message(
        cp,
        current_namespace,
        current_agent,
        current_session,
        &opened.alias,
        &delegation::delegated_task_message(&req),
        &[],
        labels,
    )
    .await
    .inspect_err(|err| {
        tracing::warn!(
            task_namespace = %req.namespace,
            task_name = %req.name,
            error = %err,
            "failed to send delegated Task over A2A wire"
        );
    });
    let sent = match sent {
        Ok(sent) => sent,
        Err(err) => {
            let _ = delegation::mark_task_dispatch_failed(cp, &req, &err.to_string()).await;
            return Err(err);
        }
    };
    match delegation::mark_task_execution_started(
        cp,
        &req,
        &sent.reference.session_id,
        sent.submission_id.as_deref(),
    )
    .await
    {
        Ok(task) => Ok(task),
        Err(err) => {
            tracing::warn!(
                task_namespace = %req.namespace,
                task_name = %req.name,
                error = %err,
                "failed to update delegated Task execution status after A2A wire send"
            );
            Ok(task)
        }
    }
}

async fn ensure_delegate_wire_not_busy(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    alias: &str,
) -> Result<()> {
    let Some(reference) =
        a2a_tools::load_wire_ref(cp, current_namespace, current_agent, current_session, alias)
            .await?
    else {
        return Ok(());
    };
    let Some(session) = cp
        .kv
        .get_msg::<data_proto::Session>(&keys::session(
            &reference.namespace,
            &reference.agent,
            &reference.session_id,
        ))
        .await?
    else {
        return Ok(());
    };
    let Some(task_namespace) = session
        .labels
        .get(delegation::LABEL_TASK_NAMESPACE)
        .map(String::as_str)
    else {
        return Ok(());
    };
    let Some(task_name) = session
        .labels
        .get(delegation::LABEL_TASK_NAME)
        .map(String::as_str)
    else {
        return Ok(());
    };
    let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    let Some(resource) = store.get(task_namespace, "Task", task_name).await? else {
        return Ok(());
    };
    let Some(task) = task_from_resource(resource) else {
        return Ok(());
    };
    let phase = task
        .status
        .as_ref()
        .map(|status| status.phase)
        .unwrap_or(resources_proto::TaskPhase::Unspecified as i32);
    if delegate_wire_busy_phase(phase) {
        return Err(anyhow!(
            "delegate_task cannot reuse wire '{}' because delegated task '{}/{}' is still {}",
            alias,
            task_namespace,
            task_name,
            task_phase_name(phase)
        ));
    }
    Ok(())
}

fn delegate_wire_busy_phase(phase: i32) -> bool {
    matches!(
        resources_proto::TaskPhase::try_from(phase).ok(),
        Some(resources_proto::TaskPhase::Queued)
            | Some(resources_proto::TaskPhase::Running)
            | Some(resources_proto::TaskPhase::Blocked)
    )
}

fn task_resource_from_task(task: resources_proto::Task) -> resources_proto::Resource {
    let namespace = task.namespace().to_string();
    let name = task.name().to_string();
    let labels = task.labels().clone();
    resource_model::task_resource(
        namespace,
        name,
        task.spec.unwrap_or_default(),
        task.status.unwrap_or_default(),
        labels,
    )
}

async fn task_output_artifact_uris_from_args(
    cp: &ControlPlane,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<Vec<String>> {
    let mut output_artifact_uris = Vec::new();
    if let Some(uri) = opt_str(args, "output_artifact_uri") {
        output_artifact_uris.push(uri.to_string());
    }
    if let Some(values) = args.get("output_artifact_uris") {
        let Some(values) = values.as_array() else {
            return Err(anyhow!("output_artifact_uris must be an array"));
        };
        for value in values {
            let Some(uri) = value.as_str() else {
                return Err(anyhow!("output_artifact_uris must contain strings"));
            };
            output_artifact_uris.push(uri.to_string());
        }
    }
    output_artifact_uris.sort();
    output_artifact_uris.dedup();
    for uri in &output_artifact_uris {
        resolve_artifact_uri(cp, current_agent, current_session, uri, OP_READ).await?;
    }
    Ok(output_artifact_uris)
}

fn update_task_status(
    status: &mut resources_proto::TaskStatus,
    args: &Value,
    output_artifact_uris: &[String],
) -> Result<()> {
    let now = chrono::Utc::now().timestamp_micros();
    if let Some(namespace) = opt_str(args, "execution_namespace") {
        status
            .execution_ref
            .get_or_insert_with(Default::default)
            .namespace = namespace.to_string();
    }
    if let Some(name) = opt_str(args, "execution_name") {
        status
            .execution_ref
            .get_or_insert_with(Default::default)
            .name = name.to_string();
    }
    if let Some(session_id) = opt_str(args, "execution_session_id") {
        let execution = status.execution_ref.get_or_insert_with(Default::default);
        execution.kind = "AGENT_SESSION".to_string();
        execution.session_id = session_id.to_string();
    }
    if let Some(run_id) = opt_str(args, "run_id") {
        status
            .execution_ref
            .get_or_insert_with(Default::default)
            .run_id = run_id.to_string();
    }
    if let Some(phase) = opt_str(args, "phase") {
        status.phase = parse_task_phase(phase)?;
    }
    if let Some(summary) = opt_str(args, "progress_summary") {
        status.progress_summary = summary.to_string();
    }
    status.updated_at = now;
    if is_terminal_phase(status.phase) && status.completed_at == 0 {
        status.completed_at = now;
        status.expires_at = now + 90 * 24 * 60 * 60 * 1_000_000;
    }
    for uri in output_artifact_uris {
        if !status.output_artifact_uris.contains(uri) {
            status.output_artifact_uris.push(uri.clone());
        }
    }
    Ok(())
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

fn task_matches(
    task: &resources_proto::Task,
    status_group: Option<&str>,
    phase: Option<&str>,
    owner_name: Option<&str>,
    delegate_name: Option<&str>,
) -> bool {
    let spec = task.spec.as_ref();
    let current_phase = task
        .status
        .as_ref()
        .map(|status| status.phase)
        .unwrap_or_default();
    if let Some(group) = status_group {
        let matches = match group.to_ascii_lowercase().as_str() {
            "active" => is_active_phase(current_phase),
            "terminal" => is_terminal_phase(current_phase),
            _ => false,
        };
        if !matches {
            return false;
        }
    }
    if let Some(phase) = phase {
        if parse_task_phase(phase).ok() != Some(current_phase) {
            return false;
        }
    }
    if owner_name.is_some_and(|name| {
        spec.and_then(|spec| spec.owner.as_ref())
            .map(|owner| owner.name.as_str())
            != Some(name)
    }) {
        return false;
    }
    if delegate_name.is_some_and(|name| {
        spec.and_then(|spec| spec.delegate.as_ref())
            .map(|delegate| delegate.name.as_str())
            != Some(name)
    }) {
        return false;
    }
    true
}

fn task_updated_at(resource: &resources_proto::Resource) -> i64 {
    match resource
        .status
        .as_ref()
        .and_then(|status| status.kind.as_ref())
    {
        Some(resources_proto::resource_status::Kind::Task(status)) => status.updated_at,
        _ => 0,
    }
}

fn task_json(task: &resources_proto::Task) -> Value {
    let spec = task.spec.as_ref();
    let status = task.status.as_ref();
    let owner = spec.and_then(|spec| spec.owner.as_ref());
    let delegate = spec.and_then(|spec| spec.delegate.as_ref());
    let execution = status.and_then(|status| status.execution_ref.as_ref());
    json!({
        "name": task.name(),
        "namespace": task.namespace(),
        "title": spec.map(|spec| spec.title.clone()).unwrap_or_default(),
        "description": spec.map(|spec| spec.description.clone()).unwrap_or_default(),
        "type": spec.map(|spec| spec.r#type.clone()).unwrap_or_default(),
        "owner": resource_ref_json(owner),
        "delegate": resource_ref_json(delegate),
        "executionRef": execution.map(|execution| json!({
            "kind": execution.kind,
            "namespace": execution.namespace,
            "name": execution.name,
            "sessionId": execution.session_id,
            "runId": execution.run_id,
        })).unwrap_or_else(|| json!({})),
        "phase": status.map(|status| task_phase_name(status.phase)).unwrap_or("UNSPECIFIED"),
        "statusGroup": status.map(|status| {
            if is_active_phase(status.phase) {
                "ACTIVE"
            } else if is_terminal_phase(status.phase) {
                "TERMINAL"
            } else {
                "UNKNOWN"
            }
        }).unwrap_or("UNKNOWN"),
        "progressSummary": status.map(|status| status.progress_summary.clone()).unwrap_or_default(),
        "resultArtifacts": status.map(|status| {
            status.result_artifacts.iter().map(file_object_ref_json).collect::<Vec<_>>()
        }).unwrap_or_default(),
        "outputArtifactUris": status.map(|status| {
            status.output_artifact_uris.clone()
        }).unwrap_or_default(),
        "createdAt": status.map(|status| status.created_at).unwrap_or_default(),
        "updatedAt": status.map(|status| status.updated_at).unwrap_or_default(),
        "completedAt": status.map(|status| status.completed_at).unwrap_or_default(),
        "expiresAt": status.map(|status| status.expires_at).unwrap_or_default(),
        "labels": task.labels(),
    })
}

fn file_object_ref_json(reference: &resources_proto::FileObjectRef) -> Value {
    json!({
        "key": reference.key,
        "mediaType": reference.media_type,
        "sizeBytes": reference.size_bytes,
        "sha256": reference.sha256,
        "filename": reference.filename,
        "metadata": reference.metadata,
    })
}

fn resource_ref_json(reference: Option<&resources_proto::ResourceRef>) -> Value {
    reference
        .map(|reference| {
            json!({
                "namespace": reference.namespace,
                "name": reference.name,
            })
        })
        .unwrap_or_else(|| json!({}))
}

fn unique_task_name(title: &str) -> String {
    let slug = task_name_slug(title, 48);
    format!("{slug}-{}", crate::control::uuid::unique_name("tsk"))
}

fn task_name_slug(title: &str, max_chars: usize) -> String {
    let mut slug = title
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "task" } else { slug };
    let trimmed = slug.chars().take(max_chars).collect::<String>();
    trimmed.trim_matches('-').to_string()
}

fn parse_task_phase(value: &str) -> Result<i32> {
    let phase = match value.trim().to_ascii_uppercase().as_str() {
        "" | "UNSPECIFIED" => resources_proto::TaskPhase::Unspecified,
        "QUEUED" => resources_proto::TaskPhase::Queued,
        "RUNNING" => resources_proto::TaskPhase::Running,
        "BLOCKED" => resources_proto::TaskPhase::Blocked,
        "NEEDS_REVIEW" | "NEEDS-REVIEW" => resources_proto::TaskPhase::NeedsReview,
        "SUCCEEDED" | "SUCCESS" | "COMPLETED" => resources_proto::TaskPhase::Succeeded,
        "FAILED" => resources_proto::TaskPhase::Failed,
        "CANCELED" | "CANCELLED" => resources_proto::TaskPhase::Canceled,
        "EXPIRED" => resources_proto::TaskPhase::Expired,
        other => return Err(anyhow!("unsupported task phase '{}'", other)),
    };
    Ok(phase as i32)
}

fn task_phase_name(value: i32) -> &'static str {
    match resources_proto::TaskPhase::try_from(value).ok() {
        Some(resources_proto::TaskPhase::Queued) => "QUEUED",
        Some(resources_proto::TaskPhase::Running) => "RUNNING",
        Some(resources_proto::TaskPhase::Blocked) => "BLOCKED",
        Some(resources_proto::TaskPhase::NeedsReview) => "NEEDS_REVIEW",
        Some(resources_proto::TaskPhase::Succeeded) => "SUCCEEDED",
        Some(resources_proto::TaskPhase::Failed) => "FAILED",
        Some(resources_proto::TaskPhase::Canceled) => "CANCELED",
        Some(resources_proto::TaskPhase::Expired) => "EXPIRED",
        _ => "UNSPECIFIED",
    }
}

fn is_active_phase(value: i32) -> bool {
    matches!(
        resources_proto::TaskPhase::try_from(value).ok(),
        Some(resources_proto::TaskPhase::Queued)
            | Some(resources_proto::TaskPhase::Running)
            | Some(resources_proto::TaskPhase::Blocked)
            | Some(resources_proto::TaskPhase::NeedsReview)
    )
}

fn is_terminal_phase(value: i32) -> bool {
    matches!(
        resources_proto::TaskPhase::try_from(value).ok(),
        Some(resources_proto::TaskPhase::Succeeded)
            | Some(resources_proto::TaskPhase::Failed)
            | Some(resources_proto::TaskPhase::Canceled)
            | Some(resources_proto::TaskPhase::Expired)
    )
}

// moved create_goal -> goals.rs

// moved get_goal_from_args -> goals.rs

// moved load_goal -> goals.rs

// moved upsert_goal -> goals.rs

// moved list_goals -> goals.rs

// moved list_session_goals -> goals.rs

// moved goal_matches -> goals.rs

// moved update_goal_from_args -> goals.rs

// moved goal_json -> goals.rs

// moved active_goals_context -> goals.rs

// moved parse_goal_phase -> goals.rs

// moved goal_phase_name -> goals.rs

// moved goal_status_group -> goals.rs

// moved is_active_goal_phase -> goals.rs

// moved is_terminal_goal_phase -> goals.rs







// moved artifact_content_bytes -> artifacts.rs

#[derive(Debug, Clone)]
struct ArtifactUri {
    namespace: String,
    agent: String,
    session_id: String,
    artifact_id: String,
}

impl ArtifactUri {
    fn encode(&self) -> String {
        format!(
            "artifact://{}/{}/{}/{}",
            self.namespace, self.agent, self.session_id, self.artifact_id
        )
    }
}

// moved resolve_artifact_uri -> artifacts.rs

// moved artifact_json -> artifacts.rs

#[derive(Clone, Debug)]
struct ValidatedHttpUrl {
    url: url::Url,
    host: Option<String>,
    addrs: Vec<SocketAddr>,
}

// moved http_client -> research.rs

// moved validate_http_url -> research.rs

// moved validate_public_http_url -> research.rs

// moved ensure_public_ip -> research.rs

// moved is_blocked_ipv4 -> research.rs

// moved is_blocked_ipv6 -> research.rs

// moved extract_title -> research.rs

// moved compact_visible_text -> research.rs

// moved remove_tag_blocks -> research.rs

// moved decode_html_entities -> research.rs

// moved extract_duckduckgo_results -> research.rs

// moved extract_attr -> research.rs

// moved normalize_search_result_url -> research.rs

// moved default_access_expiry -> artifacts.rs

// moved access_expiry_from_ttl_seconds -> artifacts.rs


// moved parse_artifact_uri -> artifacts.rs

// moved validate_uri_segment -> artifacts.rs

// moved authorize_artifact_access -> artifacts.rs





#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::scheduler::{ScheduleWakeupRequest, ScheduledWakeup, SchedulerBackend};
    use crate::control::KeyValueStore;
    use crate::test_support::{EmptyPubSub, MockKvStore};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockScheduler {
        scheduled: Mutex<Vec<ScheduleWakeupRequest>>,
        cancelled: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl SchedulerBackend for MockScheduler {
        async fn schedule(&self, req: ScheduleWakeupRequest) -> anyhow::Result<ScheduledWakeup> {
            self.scheduled.lock().await.push(req);
            Ok(ScheduledWakeup {
                handle: Some("handle-1".to_string()),
                armed: true,
            })
        }

        async fn cancel(&self, handle: &str) -> anyhow::Result<()> {
            self.cancelled.lock().await.push(handle.to_string());
            Ok(())
        }
    }

    fn spec(capabilities: &[&str]) -> manifests::AgentSpec {
        manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: String::new(),
            post_history_prompt: String::new(),
            mcp_server_refs: Vec::new(),
            capabilities: HashMap::from([(
                "schedules".to_string(),
                crate::gateway::rpc::protobuf_value::ListValue {
                    values: capabilities
                        .iter()
                        .map(|action| crate::gateway::rpc::protobuf_value::Value {
                            kind: Some(ProtoValueKind::StringValue((*action).to_string())),
                        })
                        .collect(),
                },
            )]),
            a2a: None,
            runtime: None,
        }
    }

    fn code_spec(capabilities: &[&str]) -> manifests::AgentSpec {
        manifests::AgentSpec {
            capabilities: HashMap::from([(
                "code".to_string(),
                crate::gateway::rpc::protobuf_value::ListValue {
                    values: capabilities
                        .iter()
                        .map(|action| crate::gateway::rpc::protobuf_value::Value {
                            kind: Some(ProtoValueKind::StringValue((*action).to_string())),
                        })
                        .collect(),
                },
            )]),
            ..manifests::AgentSpec::default()
        }
    }

    fn research_spec(capabilities: &[&str]) -> manifests::AgentSpec {
        manifests::AgentSpec {
            capabilities: HashMap::from([(
                "research".to_string(),
                crate::gateway::rpc::protobuf_value::ListValue {
                    values: capabilities
                        .iter()
                        .map(|action| crate::gateway::rpc::protobuf_value::Value {
                            kind: Some(ProtoValueKind::StringValue((*action).to_string())),
                        })
                        .collect(),
                },
            )]),
            ..manifests::AgentSpec::default()
        }
    }

    fn file_spec(capabilities: &[&str]) -> manifests::AgentSpec {
        manifests::AgentSpec {
            capabilities: HashMap::from([(
                "files".to_string(),
                crate::gateway::rpc::protobuf_value::ListValue {
                    values: capabilities
                        .iter()
                        .map(|action| crate::gateway::rpc::protobuf_value::Value {
                            kind: Some(ProtoValueKind::StringValue((*action).to_string())),
                        })
                        .collect(),
                },
            )]),
            ..manifests::AgentSpec::default()
        }
    }

    fn task_spec(capabilities: &[&str]) -> manifests::AgentSpec {
        manifests::AgentSpec {
            capabilities: HashMap::from([(
                "tasks".to_string(),
                crate::gateway::rpc::protobuf_value::ListValue {
                    values: capabilities
                        .iter()
                        .map(|action| crate::gateway::rpc::protobuf_value::Value {
                            kind: Some(ProtoValueKind::StringValue((*action).to_string())),
                        })
                        .collect(),
                },
            )]),
            ..manifests::AgentSpec::default()
        }
    }

    fn task_spec_with_internal_connection(
        capabilities: &[&str],
        connection: &str,
        namespace: &str,
        agent: &str,
    ) -> manifests::AgentSpec {
        let mut spec = task_spec(capabilities);
        spec.a2a = Some(manifests::A2a {
            connections: vec![manifests::Connection {
                name: connection.to_string(),
                target: Some(manifests::ConnectionRef {
                    internal: Some(manifests::InternalConnectionRef {
                        namespace: namespace.to_string(),
                        agent: agent.to_string(),
                    }),
                    external: None,
                }),
                ..Default::default()
            }],
            agent_card: None,
        });
        spec
    }

    fn task_spec_with_external_connection(
        capabilities: &[&str],
        connection: &str,
    ) -> manifests::AgentSpec {
        let mut spec = task_spec(capabilities);
        spec.a2a = Some(manifests::A2a {
            connections: vec![manifests::Connection {
                name: connection.to_string(),
                target: Some(manifests::ConnectionRef {
                    internal: None,
                    external: Some(manifests::ExternalConnectionRef {
                        agent_card_url: "https://example.com/agent-card.json".to_string(),
                    }),
                }),
                ..Default::default()
            }],
            agent_card: None,
        });
        spec
    }

    fn goal_spec(capabilities: &[&str]) -> manifests::AgentSpec {
        manifests::AgentSpec {
            capabilities: HashMap::from([(
                "goals".to_string(),
                crate::gateway::rpc::protobuf_value::ListValue {
                    values: capabilities
                        .iter()
                        .map(|action| crate::gateway::rpc::protobuf_value::Value {
                            kind: Some(ProtoValueKind::StringValue((*action).to_string())),
                        })
                        .collect(),
                },
            )]),
            ..manifests::AgentSpec::default()
        }
    }

    fn control_plane(kv: Arc<MockKvStore>, scheduler: Arc<MockScheduler>) -> ControlPlane {
        ControlPlane::builder(kv, Arc::new(EmptyPubSub))
            .scheduler(scheduler)
            .build()
    }

    async fn seed_agent(kv: &MockKvStore, ns: &str, name: &str) {
        kv.set_msg(
            &keys::agent(ns, name),
            &resource_model::agent(ns, name, manifests::AgentSpec::default(), HashMap::new()),
        )
        .await
        .unwrap();
    }

    async fn seed_session(kv: &MockKvStore, ns: &str, agent: &str, session_id: &str) {
        let now = chrono::Utc::now().timestamp_micros();
        kv.set_msg(
            &keys::session(ns, agent, session_id),
            &data_proto::Session {
                id: session_id.to_string(),
                agent: agent.to_string(),
                ns: ns.to_string(),
                status: "IDLE".to_string(),
                created_at: now,
                last_active: now,
                metadata: HashMap::new(),
                labels: HashMap::new(),
                skill_state: None,
                context_tokens: None,
            },
        )
        .await
        .unwrap();
    }

    async fn seed_claimed_submission(kv: &MockKvStore, ns: &str, agent: &str, session_id: &str) {
        let mut submission =
            crate::harness::sessions::pending_submission("submission-1", session_id, "user-1", 1);
        submission.status = data_proto::SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        crate::harness::sessions::create_submission_if_absent(
            kv,
            ns,
            agent,
            session_id,
            &submission,
        )
        .await
        .unwrap();
    }

    async fn append_test_tool_result(
        kv: &MockKvStore,
        cp: &ControlPlane,
        ns: &str,
        agent: &str,
        session_id: &str,
        tool_name: &str,
        output: &ToolOutput,
    ) -> data_proto::ObjectRef {
        seed_claimed_submission(kv, ns, agent, session_id).await;
        let cas = crate::control::cas::CasStore::new(cp.objects.clone());
        let entry = crate::harness::sessions::append_tool_result(
            kv,
            &cas,
            ns,
            agent,
            session_id,
            "message-1",
            "part-1",
            "submission-1",
            "attempt-1",
            "call-1",
            tool_name,
            output,
            chrono::Utc::now().timestamp_micros(),
        )
        .await
        .unwrap();
        let payload = entry
            .payload
            .as_ref()
            .and_then(|payload| payload.payload.as_ref())
            .expect("journal payload");
        match payload {
            data_proto::session_journal_entry_payload::Payload::ToolResult(result) => result
                .tool_output
                .as_ref()
                .and_then(crate::control::tool_output::first_object_ref)
                .cloned()
                .expect("journaled object ref"),
            other => panic!("expected tool result payload, got {other:?}"),
        }
    }

    async fn set_session_status(
        kv: &MockKvStore,
        ns: &str,
        agent: &str,
        session_id: &str,
        status: &str,
    ) {
        let key = keys::session(ns, agent, session_id);
        let mut session = kv
            .get_msg::<data_proto::Session>(&key)
            .await
            .unwrap()
            .unwrap();
        session.status = status.to_string();
        kv.set_msg(&key, &session).await.unwrap();
    }

    async fn session_text_messages(
        kv: &MockKvStore,
        ns: &str,
        agent: &str,
        session_id: &str,
    ) -> Vec<String> {
        let entries = kv
            .list_entries(&keys::session_message_prefix(ns, agent, session_id), None)
            .await
            .unwrap();
        entries
            .into_iter()
            .filter_map(|(_, bytes)| data_proto::SessionMessage::decode(bytes.as_slice()).ok())
            .flat_map(|message| message.parts.into_iter())
            .filter(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
            .map(|part| part.content)
            .collect()
    }

    fn skill(ns: &str, name: &str, description: &str, _instructions: &str) -> NamespaceSkill {
        NamespaceSkill {
            name: name.to_string(),
            namespace: ns.to_string(),
            description: description.to_string(),
        }
    }

    fn skill_resource(
        ns: &str,
        name: &str,
        description: &str,
        _instructions: &str,
    ) -> resources_proto::Skill {
        namespace::skill_resource(ns, name, description)
    }

    #[test]
    fn register_tools_respects_capabilities() {
        let mut registry = ToolRegistry::new();
        register_tools(
            &mut registry,
            &spec(&["inspect", "create"]),
            &Config::default(),
        );

        assert!(registry.get_tool(LIST_SCHEDULES_TOOL).is_some());
        assert!(registry.get_tool(GET_SCHEDULE_TOOL).is_some());
        assert!(registry.get_tool(CREATE_SCHEDULE_TOOL).is_some());
        assert!(registry.get_tool(UPDATE_SCHEDULE_TOOL).is_none());
        assert!(registry.get_tool(DELETE_SCHEDULE_TOOL).is_none());
    }

    #[test]
    fn global_capability_gate_hides_code_tool_without_granting_it() {
        let mut disabled = Config::default();
        disabled.capabilities.insert(
            "code".to_string(),
            crate::control::config::CapabilityGate {
                actions: HashMap::from([(String::from("run"), false)]),
            },
        );
        let mut registry = ToolRegistry::new();
        register_tools(&mut registry, &code_spec(&["run"]), &disabled);
        assert!(registry.get_tool(RUN_PYTHON_CODE_TOOL).is_none());

        let mut enabled = Config::default();
        enabled.capabilities.insert(
            "code".to_string(),
            crate::control::config::CapabilityGate {
                actions: HashMap::from([(String::from("run"), true)]),
            },
        );
        let mut enabled_registry = ToolRegistry::new();
        register_tools(&mut enabled_registry, &code_spec(&["run"]), &enabled);
        assert!(enabled_registry.get_tool(RUN_PYTHON_CODE_TOOL).is_some());

        let mut ungranted_registry = ToolRegistry::new();
        register_tools(&mut ungranted_registry, &code_spec(&[]), &enabled);
        assert!(ungranted_registry.get_tool(RUN_PYTHON_CODE_TOOL).is_none());
    }

    #[test]
    fn register_research_tools_respects_capabilities() {
        let mut registry = ToolRegistry::new();
        register_tools(
            &mut registry,
            &research_spec(&["fetch_url"]),
            &Config::default(),
        );

        assert!(registry.get_tool(FETCH_URL_TOOL).is_some());
        assert!(registry.get_tool(WEB_SEARCH_TOOL).is_none());
    }

    #[test]
    fn register_file_tools_respects_capabilities() {
        let mut read_registry = ToolRegistry::new();
        register_tools(
            &mut read_registry,
            &file_spec(&["read"]),
            &Config::default(),
        );

        assert!(read_registry.get_tool(LIST_FILES_TOOL).is_some());
        assert!(read_registry.get_tool(READ_FILE_TOOL).is_some());
        assert!(read_registry.get_tool(GET_FILE_METADATA_TOOL).is_some());
        assert!(read_registry.get_tool(CREATE_FILE_TOOL).is_none());
        assert!(read_registry.get_tool(UPDATE_FILE_TOOL).is_none());
        assert!(read_registry.get_tool(DELETE_FILE_TOOL).is_none());

        let mut write_registry = ToolRegistry::new();
        register_tools(
            &mut write_registry,
            &file_spec(&["create", "update", "delete"]),
            &Config::default(),
        );

        assert!(write_registry.get_tool(CREATE_FILE_TOOL).is_some());
        assert!(write_registry.get_tool(UPDATE_FILE_TOOL).is_some());
        assert!(write_registry.get_tool(DELETE_FILE_TOOL).is_some());
        assert!(write_registry.get_tool(READ_FILE_TOOL).is_none());
    }

    #[test]
    fn file_uri_parsing_preserves_namespace_and_path() {
        let (namespace, path) = parse_file_uri(
            "file://Tenant:conic:Customers:13/content/pages/cGFnZToxNjY=/content.md",
        )
        .expect("file uri should parse");

        assert_eq!(namespace, "Tenant:conic:Customers:13");
        assert_eq!(path, "/content/pages/cGFnZToxNjY=/content.md");
    }

    #[tokio::test]
    async fn file_tools_create_read_update_list_and_delete() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let spec = file_spec(&["inspect", "read", "create", "update", "delete"]);
        let namespace = "Tenant:conic:Customers:13";
        let path = "/content/pages/cGFnZToxNjY=/content.md";
        let uri = file_uri(namespace, path);

        execute_tool_for_session(
            &cp,
            namespace,
            "cmo",
            "session-1",
            &spec,
            CREATE_FILE_TOOL,
            &json!({
                "path": path,
                "content": "# First draft",
            }),
            &Config::default(),
        )
        .await
        .expect("create should execute")
        .expect("create should return output");

        let read = execute_tool_for_session_output(
            &cp,
            namespace,
            "cmo",
            "session-1",
            &spec,
            READ_FILE_TOOL,
            &json!({ "uri": uri }),
            &Config::default(),
        )
        .await
        .expect("read should execute")
        .expect("read should return output");
        assert_eq!(read.summary(), "# First draft");
        assert!(read
            .object_ref()
            .expect("read_file should retain source object ref")
            .key
            .contains("/files/"));

        execute_tool_for_session(
            &cp,
            namespace,
            "cmo",
            "session-1",
            &spec,
            UPDATE_FILE_TOOL,
            &json!({
                "uri": uri,
                "content": "# Revised draft",
            }),
            &Config::default(),
        )
        .await
        .expect("update should execute")
        .expect("update should return output");

        let list = execute_tool_for_session(
            &cp,
            namespace,
            "cmo",
            "session-1",
            &spec,
            LIST_FILES_TOOL,
            &json!({ "namespace": "current", "prefix": "/content/pages" }),
            &Config::default(),
        )
        .await
        .expect("list should execute")
        .expect("list should return output");
        let list: Value = serde_json::from_str(&list).unwrap();
        assert_eq!(list["entries"].as_array().unwrap().len(), 1);
        assert_eq!(list["entries"][0]["uri"], uri);

        execute_tool_for_session(
            &cp,
            namespace,
            "cmo",
            "session-1",
            &spec,
            DELETE_FILE_TOOL,
            &json!({ "uri": uri }),
            &Config::default(),
        )
        .await
        .expect("delete should execute")
        .expect("delete should return output");

        let error = execute_tool_for_session(
            &cp,
            namespace,
            "cmo",
            "session-1",
            &spec,
            READ_FILE_TOOL,
            &json!({ "uri": uri }),
            &Config::default(),
        )
        .await
        .expect_err("deleted file should not read");
        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn delegate_task_schema_uses_internal_a2a_connection_enum() {
        let mut registry = ToolRegistry::new();
        register_tools(
            &mut registry,
            &task_spec_with_internal_connection(
                &["create"],
                "worker",
                "Tenant:acme:Operations",
                "support-agent",
            ),
            &Config::default(),
        );

        let tool = registry
            .get_tool(DELEGATE_TASK_TOOL)
            .expect("delegate_task should be registered");
        assert_eq!(
            tool.input_schema["properties"]["connection"]["enum"],
            json!(["worker"])
        );
        assert!(tool.input_schema["properties"]
            .as_object()
            .unwrap()
            .get("delegate_name")
            .is_none());
        assert!(tool.input_schema["properties"]
            .as_object()
            .unwrap()
            .get("delegate_namespace")
            .is_none());
    }

    #[test]
    fn delegate_task_not_registered_without_internal_a2a_connection() {
        let mut no_connection_registry = ToolRegistry::new();
        register_tools(
            &mut no_connection_registry,
            &task_spec(&["create"]),
            &Config::default(),
        );
        assert!(no_connection_registry
            .get_tool(DELEGATE_TASK_TOOL)
            .is_none());

        let mut external_registry = ToolRegistry::new();
        register_tools(
            &mut external_registry,
            &task_spec_with_external_connection(&["create"], "remote"),
            &Config::default(),
        );
        assert!(external_registry.get_tool(DELEGATE_TASK_TOOL).is_none());
    }

    #[test]
    fn agent_wire_schemas_use_internal_a2a_connection_enum_without_task_capability() {
        let mut registry = ToolRegistry::new();
        let mut spec = manifests::AgentSpec::default();
        spec.a2a = Some(manifests::A2a {
            connections: vec![manifests::Connection {
                name: "critic".to_string(),
                target: Some(manifests::ConnectionRef {
                    internal: Some(manifests::InternalConnectionRef {
                        namespace: "Tenant:acme:Copywriter".to_string(),
                        agent: "critic-agent".to_string(),
                    }),
                    external: None,
                }),
                ..Default::default()
            }],
            agent_card: None,
        });

        register_tools(&mut registry, &spec, &Config::default());

        let tool = registry
            .get_tool(AGENT_OPEN_TOOL)
            .expect("agent_open should be registered");
        assert_eq!(
            tool.input_schema["properties"]["connection"]["enum"],
            json!(["critic"])
        );
        assert!(registry.get_tool(AGENT_SEND_TOOL).is_some());
        assert!(registry.get_tool(AGENT_WAIT_FOR_MESSAGE_TOOL).is_some());
        assert!(registry.get_tool(DELEGATE_TASK_TOOL).is_none());
    }

    #[test]
    fn agent_open_not_registered_without_internal_a2a_connection() {
        let mut no_connection_registry = ToolRegistry::new();
        register_tools(
            &mut no_connection_registry,
            &manifests::AgentSpec::default(),
            &Config::default(),
        );
        assert!(no_connection_registry.get_tool(AGENT_OPEN_TOOL).is_none());
        assert!(no_connection_registry.get_tool(AGENT_SEND_TOOL).is_some());
        assert!(no_connection_registry
            .get_tool(AGENT_WAIT_FOR_MESSAGE_TOOL)
            .is_some());

        let mut external_registry = ToolRegistry::new();
        register_tools(
            &mut external_registry,
            &task_spec_with_external_connection(&["create"], "remote"),
            &Config::default(),
        );
        assert!(external_registry.get_tool(AGENT_OPEN_TOOL).is_none());
        assert!(external_registry.get_tool(AGENT_SEND_TOOL).is_some());
        assert!(external_registry
            .get_tool(AGENT_WAIT_FOR_MESSAGE_TOOL)
            .is_some());
    }

    #[test]
    fn validate_http_url_rejects_non_http_schemes() {
        assert!(validate_http_url("https://example.com/path").is_ok());
        assert!(validate_http_url("http://example.com/path").is_ok());
        assert!(validate_http_url("file:///etc/passwd").is_err());
        assert!(validate_http_url("not a url").is_err());
    }

    #[test]
    fn public_ip_validation_rejects_private_and_metadata_ranges() {
        assert!(ensure_public_ip("8.8.8.8".parse().unwrap()).is_ok());
        assert!(ensure_public_ip("10.0.0.1".parse().unwrap()).is_err());
        assert!(ensure_public_ip("127.0.0.1".parse().unwrap()).is_err());
        assert!(ensure_public_ip("169.254.169.254".parse().unwrap()).is_err());
        assert!(ensure_public_ip("::1".parse().unwrap()).is_err());
        assert!(ensure_public_ip("fc00::1".parse().unwrap()).is_err());
    }

    #[test]
    fn parse_artifact_uri_accepts_literal_namespace_segments() {
        let parsed = parse_artifact_uri(
            "artifact://Tenant:acme:Workspace:main/copywriter/session-1/artifact-1",
        )
        .unwrap();

        assert_eq!(parsed.namespace, "Tenant:acme:Workspace:main");
        assert_eq!(parsed.agent, "copywriter");
        assert_eq!(parsed.session_id, "session-1");
        assert_eq!(parsed.artifact_id, "artifact-1");
    }

    #[test]
    fn access_expiry_clamps_requested_ttl() {
        let now = chrono::Utc::now().timestamp_micros();
        let expires_at = access_expiry_from_ttl_seconds(i64::MAX);
        let max_delta = (MAX_ACCESS_TTL_SECONDS * 1_000_000) + 1_000_000;

        assert!(expires_at >= now);
        assert!(expires_at - now <= max_delta);
    }

    #[test]
    fn compact_visible_text_removes_scripts_styles_and_tags() {
        let html = r#"
            <html>
              <head>
                <title>Research &amp; Notes</title>
                <style>.hidden { display: none; }</style>
              </head>
              <body>
                <script>alert("nope")</script>
                <h1>Useful&nbsp;Heading</h1>
                <p>Visible <strong>claim</strong>.</p>
              </body>
            </html>
        "#;

        assert_eq!(extract_title(html), "Research & Notes");
        let text = compact_visible_text(html, 1_000);
        assert!(text.contains("Useful Heading"));
        assert!(text.contains("Visible"));
        assert!(text.contains("claim"));
        assert!(!text.contains("alert"));
        assert!(!text.contains("display"));
    }

    #[test]
    fn extract_duckduckgo_results_decodes_redirect_urls() {
        let html = r#"
            <a rel="nofollow" class="result__a"
               href="/l/?kh=-1&uddg=https%3A%2F%2Fexample.com%2Fpost%3Fx%3D1">
               Example &amp; Result
            </a>
            <a class="result__a" href="https://direct.example/page">
               Direct Result
            </a>
        "#;

        let results = extract_duckduckgo_results(html, 5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["title"], "Example & Result");
        assert_eq!(results[0]["url"], "https://example.com/post?x=1");
        assert_eq!(results[1]["url"], "https://direct.example/page");
    }

    #[test]
    fn register_skill_tools_uses_effective_skill_name_enum() {
        let mut registry = ToolRegistry::new();
        register_skill_tools(
            &mut registry,
            &[
                skill("acme", "review", "Review code", "parent"),
                skill("acme", "release", "Release notes", "release"),
            ],
        );

        let tool = registry
            .get_tool(ACTIVATE_SKILL_TOOL)
            .expect("activation tool should be registered");
        assert_eq!(
            tool.input_schema["properties"]["name"]["enum"],
            json!(["review", "release"])
        );
    }

    #[test]
    fn register_skill_tools_skips_empty_catalog() {
        let mut registry = ToolRegistry::new();

        register_skill_tools(&mut registry, &[]);

        assert!(registry.get_tool(ACTIVATE_SKILL_TOOL).is_none());
        assert!(registry.get_tool(DEACTIVATE_SKILL_TOOL).is_some());
    }

    #[tokio::test]
    async fn execute_tool_requires_capabilities() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let err = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &manifests::AgentSpec::default(),
            LIST_SCHEDULES_TOOL,
            &json!({}),
            &Config::default(),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("agent does not have capability"));
    }

    #[tokio::test]
    async fn deactivate_skill_requires_a_session() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let err = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &manifests::AgentSpec::default(),
            DEACTIVATE_SKILL_TOOL,
            &json!({"name": "review"}),
            &Config::default(),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("requires a session"));
    }

    #[tokio::test]
    async fn create_artifact_stores_canonical_session_owned_cas_object() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv.clone(), scheduler);
        let output = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Final draft",
                "content": "draft body",
                "media_type": "text/markdown",
                "metadata": {"source": "tool"}
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        let artifact_id = value["artifact"]["id"].as_str().unwrap();
        let artifact_uri = value["artifactUri"].as_str().unwrap();
        let object_key = value["artifact"]["objectRef"]["key"].as_str().unwrap();

        assert!(object_key.starts_with("cas/Tenant%3Aacme%3AWorkspace%3Amain/artifacts/"));
        assert!(object_key.contains(artifact_id));

        let stored = crate::control::cas::CasStore::new(cp.objects.clone())
            .get_object_decoded(object_key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.bytes, b"draft body");
        assert_eq!(stored.metadata.filename, "");
        assert_eq!(
            stored.metadata.metadata[crate::control::cas::METADATA_KIND],
            crate::control::cas::METADATA_KIND_ARTIFACT
        );
        assert_eq!(
            stored.metadata.metadata[crate::control::cas::METADATA_AGENT],
            "writer"
        );
        assert_eq!(stored.metadata.metadata["session_id"], "session-1");
        assert_eq!(stored.metadata.metadata["source"], "tool");

        let parsed_uri = parse_artifact_uri(artifact_uri).unwrap();
        assert_eq!(parsed_uri.namespace, "Tenant:acme:Workspace:main");
        assert_eq!(parsed_uri.agent, "writer");
        assert_eq!(parsed_uri.session_id, "session-1");
        assert_eq!(parsed_uri.artifact_id, artifact_id);

        let empty_create = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Missing content",
            }),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(empty_create
            .to_string()
            .contains("requires content or content_base64"));

        let read_output = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            READ_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(read_output, "draft body");

        let update_output = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            UPDATE_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
                "content": "revised body",
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let update_value: Value = serde_json::from_str(&update_output).unwrap();
        assert_eq!(update_value["artifactUri"], artifact_uri);
        let updated_object_key = update_value["artifact"]["objectRef"]["key"]
            .as_str()
            .unwrap();
        assert_ne!(updated_object_key, object_key);
        assert!(crate::control::cas::CasStore::new(cp.objects.clone())
            .get_object_decoded(object_key)
            .await
            .unwrap()
            .is_none());

        let read_updated_output = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            READ_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(read_updated_output, "revised body");

        let empty_update = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            UPDATE_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
            }),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(empty_update
            .to_string()
            .contains("requires content or content_base64"));

        execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            GRANT_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
                "target_agent": "critic",
                "target_session_id": "session-2",
                "operations": ["read", "metadata"],
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let stored_access = kv
            .get_msg::<crate::gateway::rpc::data_proto::ArtifactAccess>(&keys::artifact_access(
                "Tenant:acme:Workspace:main",
                "writer",
                "session-1",
                artifact_id,
                "critic",
                "session-2",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored_access.target_agent, "critic");
        assert_eq!(stored_access.target_session_id, "session-2");
        assert_eq!(stored_access.operations, vec!["read", "metadata"]);

        let update_denied = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "critic",
            "session-2",
            &manifests::AgentSpec::default(),
            UPDATE_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
                "content": "critic overwrite",
            }),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(update_denied.to_string().contains("only the owning"));

        let cross_namespace_update_denied = execute_tool_for_session(
            &cp,
            "Tenant:other:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            UPDATE_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
                "content": "cross-tenant overwrite",
            }),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(cross_namespace_update_denied
            .to_string()
            .contains("only the owning artifact namespace/agent/session"));
    }

    #[tokio::test]
    async fn read_artifact_returns_typed_image_output() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv.clone(), scheduler);
        let png_bytes = b"png-bytes";
        let created = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Screenshot",
                "content_base64": general_purpose::STANDARD.encode(png_bytes),
                "media_type": "image/png"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let value: Value = serde_json::from_str(&created).unwrap();
        let artifact_uri = value["artifactUri"].as_str().unwrap();

        let output = execute_tool_for_session_output(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            READ_ARTIFACT_TOOL,
            &json!({ "artifact_uri": artifact_uri }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();

        let source_object_key = output
            .object_ref()
            .as_ref()
            .expect("image output should retain source object ref")
            .key
            .clone();
        let content_parts = output.content_parts();
        assert_eq!(
            crate::harness::llm::content_part_object_ref(&content_parts[0])
                .unwrap()
                .key,
            source_object_key
        );

        let journaled_object = append_test_tool_result(
            kv.as_ref(),
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            READ_ARTIFACT_TOOL,
            &output,
        )
        .await;
        assert_eq!(journaled_object.key, source_object_key);
    }

    #[tokio::test]
    async fn read_file_output_reuses_object_ref() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv.clone(), scheduler);
        let content = "source text ".repeat(400);
        let file = upsert_file(
            &cp,
            "Tenant:acme:Workspace:main",
            None,
            "/memory/notes.txt",
            "text/plain; charset=utf-8",
            resources_proto::FilePurpose::Artifact as i32,
            resources_proto::FileIndexPolicy::Search as i32,
            resources_proto::FileRetention::Retained as i32,
            content.as_bytes(),
        )
        .await
        .unwrap();

        let output = read_file_output(&cp, &file).await.unwrap();

        assert!(output.summary().starts_with("[Object: notes.txt"));
        assert!(!output.summary().contains(&content));
        let source_object_key = output
            .object_ref()
            .expect("read_file_output should retain source object ref")
            .key
            .clone();
        let content_parts = output.content_parts();
        assert_eq!(
            crate::harness::llm::content_part_object_ref(&content_parts[0])
                .unwrap()
                .key,
            source_object_key
        );

        let journaled_object = append_test_tool_result(
            kv.as_ref(),
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            READ_MEMORY_TOOL,
            &output,
        )
        .await;
        assert_eq!(journaled_object.key, source_object_key);
    }

    #[tokio::test]
    async fn agent_send_with_artifact_uri_grants_receiver_artifact_access() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        seed_agent(kv.as_ref(), "Tenant:acme:Workspace:main", "writer").await;
        seed_agent(kv.as_ref(), "Tenant:acme:Workspace:main", "critic").await;
        seed_session(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "writer",
            "writer-session",
        )
        .await;
        let cp = control_plane(kv.clone(), scheduler);
        let writer_spec = task_spec_with_internal_connection(
            &[],
            "critic",
            "Tenant:acme:Workspace:main",
            "critic",
        );

        let opened = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "writer-session",
            &writer_spec,
            AGENT_OPEN_TOOL,
            &json!({"connection": "critic"}),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let opened: Value = serde_json::from_str(&opened).unwrap();
        assert_eq!(opened["name"], "critic-1");
        let critic_ref = a2a_tools::load_wire_ref(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "writer-session",
            "critic-1",
        )
        .await
        .unwrap()
        .expect("critic wire should exist");

        let artifact = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "writer-session",
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Draft",
                "content": "# Draft\n\nPlease review.",
                "media_type": "text/markdown"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let artifact: Value = serde_json::from_str(&artifact).unwrap();
        let artifact_uri = artifact["artifactUri"].as_str().unwrap();
        let artifact_id = artifact["artifact"]["id"].as_str().unwrap();

        let denied = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "critic",
            &critic_ref.session_id,
            &manifests::AgentSpec::default(),
            READ_ARTIFACT_TOOL,
            &json!({ "artifact_uri": artifact_uri }),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(denied.to_string().contains("artifact access denied"));

        let sent = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "writer-session",
            &writer_spec,
            AGENT_SEND_TOOL,
            &json!({
                "target": "critic-1",
                "message": "Please review the draft.",
                "artifact_uri": artifact_uri
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(!sent.contains("Please review the draft."));
        let sent: Value = serde_json::from_str(&sent).unwrap();
        if sent.get("status").is_some() {
            assert_eq!(sent["status"], "DISPATCHED");
            assert_eq!(sent["artifactCount"], 1);
            assert!(!sent.to_string().contains(artifact_uri));
        } else {
            assert_eq!(sent["dispatched"], true);
            assert_eq!(sent["artifactUris"], json!([artifact_uri]));
        }

        let access = kv
            .get_msg::<data_proto::ArtifactAccess>(&keys::artifact_access(
                "Tenant:acme:Workspace:main",
                "writer",
                "writer-session",
                artifact_id,
                "critic",
                &critic_ref.session_id,
            ))
            .await
            .unwrap()
            .expect("agent_send should grant artifact access to target session");
        assert_eq!(access.operations, vec!["read", "metadata"]);
        assert_eq!(access.granted_by_agent, "writer");
        assert_eq!(access.granted_by_session_id, "writer-session");

        let read = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "critic",
            &critic_ref.session_id,
            &manifests::AgentSpec::default(),
            READ_ARTIFACT_TOOL,
            &json!({ "artifact_uri": artifact_uri }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(read, "# Draft\n\nPlease review.");

        let messages = session_text_messages(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "critic",
            &critic_ref.session_id,
        )
        .await;
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("Please review the draft."));
        assert!(messages[0].contains("Attached artifacts:"));
        assert!(messages[0].contains(artifact_uri));
    }

    #[tokio::test]
    async fn artifact_tools_accept_large_string_content_arguments() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let large_content = format!("# Large Artifact\n\n{}", "0123456789abcdef ".repeat(700));
        assert!(
            large_content.len() > 10_000,
            "test must exercise a 10k+ string argument"
        );

        let output = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Large draft",
                "content": large_content,
                "media_type": "text/markdown"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        let artifact_uri = value["artifactUri"].as_str().unwrap();

        let read_output = execute_tool_for_session_output(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            READ_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let read_object = read_output
            .object_ref()
            .expect("large artifact read should return an object ref");
        assert_eq!(read_object.size_bytes, large_content.len() as u64);
        let stored = cp
            .objects
            .get(&read_object.key)
            .await
            .unwrap()
            .expect("large artifact object should exist");
        let actual_content = String::from_utf8(
            crate::control::cas::decode_stored_object_bytes(&stored, &read_object.key).unwrap(),
        )
        .unwrap();
        assert_eq!(actual_content, large_content);

        let large_revision = format!("# Large Revision\n\n{}", "fedcba9876543210 ".repeat(700));
        assert!(large_revision.len() > 10_000);
        execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            UPDATE_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
                "content": large_revision,
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();

        let read_revision = execute_tool_for_session_output(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            READ_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let revision_object = read_revision
            .object_ref()
            .expect("large artifact revision read should return an object ref");
        assert_eq!(revision_object.size_bytes, large_revision.len() as u64);
        let stored = cp
            .objects
            .get(&revision_object.key)
            .await
            .unwrap()
            .expect("large artifact revision object should exist");
        let actual_revision = String::from_utf8(
            crate::control::cas::decode_stored_object_bytes(&stored, &revision_object.key).unwrap(),
        )
        .unwrap();
        assert_eq!(actual_revision, large_revision);
    }

    #[tokio::test]
    async fn activate_skill_requires_a_package_entrypoint() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv.clone(), scheduler);
        kv.set_msg(
            &keys::skill("acme", "review"),
            &skill_resource("acme", "review", "Review code", "parent instructions"),
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::skill("acme:team", "review"),
            &skill_resource(
                "acme:team",
                "review",
                "Review code locally",
                "child instructions",
            ),
        )
        .await
        .unwrap();

        let error = execute_tool(
            &cp,
            "acme:team",
            "assistant",
            &manifests::AgentSpec::default(),
            ACTIVATE_SKILL_TOOL,
            &json!({"name":"review"}),
            &Config::default(),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("not available"));
    }

    #[tokio::test]
    async fn activate_skill_skips_unreadable_and_incomplete_records() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv.clone(), scheduler);
        kv.set(&keys::skill("acme", "broken"), b"not-protobuf")
            .await
            .unwrap();
        kv.set_msg(
            &keys::skill("acme", "review"),
            &skill_resource("acme", "review", "Review code", "instructions"),
        )
        .await
        .unwrap();

        let error = execute_tool(
            &cp,
            "acme",
            "assistant",
            &manifests::AgentSpec::default(),
            ACTIVATE_SKILL_TOOL,
            &json!({"name":"review"}),
            &Config::default(),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("not available"));
    }

    #[tokio::test]
    async fn activate_skill_reports_missing_or_invalid_name() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv.clone(), scheduler);

        let invalid = execute_tool(
            &cp,
            "acme",
            "assistant",
            &manifests::AgentSpec::default(),
            ACTIVATE_SKILL_TOOL,
            &json!({}),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(invalid.to_string().contains("'name' is required"));

        let missing = execute_tool(
            &cp,
            "acme",
            "assistant",
            &manifests::AgentSpec::default(),
            ACTIVATE_SKILL_TOOL,
            &json!({"name":"review"}),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(missing
            .to_string()
            .contains("skill 'review' is not available"));
    }

    #[tokio::test]
    async fn create_get_list_update_and_delete_schedule_round_trip() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        seed_agent(kv.as_ref(), "conic:test", "assistant").await;
        let cp = control_plane(kv.clone(), scheduler.clone());
        let schedule_spec = spec(&["inspect", "create", "update", "delete"]);

        let created = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &schedule_spec,
            CREATE_SCHEDULE_TOOL,
            &json!({
                "name": "nightly",
                "kind": "every",
                "interval_seconds": 600,
                "input_message": "run report",
                "labels": {"tier":"prod"},
                "enabled": true
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(created.contains("\"name\": \"nightly\""));
        assert!(created.contains("\"backendArmed\": true"));
        assert_eq!(scheduler.scheduled.lock().await.len(), 1);

        let fetched = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &schedule_spec,
            GET_SCHEDULE_TOOL,
            &json!({"name":"nightly"}),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(fetched.contains("\"name\": \"nightly\""));
        assert!(fetched.contains("\"tier\": \"prod\""));

        let listed = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &schedule_spec,
            LIST_SCHEDULES_TOOL,
            &json!({"agent":"assistant","enabled":true}),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(listed.contains("\"schedules\""));
        assert!(listed.contains("\"nightly\""));

        let updated = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &schedule_spec,
            UPDATE_SCHEDULE_TOOL,
            &json!({
                "name": "nightly",
                "input_message": "run report v2",
                "session_mode": "reuse",
                "session_id": "session-1"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(updated.contains("run report v2"));
        assert_eq!(
            scheduler.cancelled.lock().await.clone(),
            vec!["handle-1".to_string()]
        );

        let deleted = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &schedule_spec,
            DELETE_SCHEDULE_TOOL,
            &json!({"name":"nightly"}),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(deleted.contains("\"success\": true"));
        assert!(kv
            .get_msg::<resources_proto::Schedule>(&keys::schedule("conic:test", "nightly"))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn list_schedules_honors_limit_and_namespace_override() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        seed_agent(kv.as_ref(), "conic:other", "assistant").await;
        let cp = control_plane(kv.clone(), scheduler);
        let create_spec = spec(&["inspect", "create"]);

        for name in ["a", "b"] {
            execute_tool(
                &cp,
                "conic:other",
                "assistant",
                &create_spec,
                CREATE_SCHEDULE_TOOL,
                &json!({
                    "namespace": "conic:other",
                    "name": name,
                    "kind": "every",
                    "interval_seconds": 600,
                    "input_message": "run report"
                }),
                &Config::default(),
            )
            .await
            .unwrap();
        }

        let listed = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &spec(&["inspect"]),
            LIST_SCHEDULES_TOOL,
            &json!({"namespace":"conic:other","limit":1}),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(listed.matches("\"name\":").count(), 1);
    }

    #[tokio::test]
    async fn task_tools_create_update_and_list_active_work() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let spec = task_spec(&["inspect", "create", "update"]);

        let created = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            CREATE_TASK_TOOL,
            &json!({
                "title": "Prepare customer onboarding checklist",
                "description": "Create a reviewed onboarding checklist.",
                "type": "OPERATIONS",
                "delegate_namespace": "Tenant:acme:Operations",
                "delegate_name": "support-agent"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let name = created["task"]["name"].as_str().unwrap();
        assert_eq!(created["task"]["phase"], "QUEUED");
        assert_eq!(created["task"]["statusGroup"], "ACTIVE");
        assert_eq!(created["task"]["owner"]["name"], "ops-lead");
        assert_eq!(created["task"]["delegate"]["name"], "support-agent");

        let updated = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            UPDATE_TASK_TOOL,
            &json!({
                "name": name,
                "phase": "RUNNING",
                "progress_summary": "Support agent is preparing the checklist.",
                "execution_namespace": "Tenant:acme:Operations",
                "execution_name": "support-agent",
                "execution_session_id": "support-session-1"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let updated: Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(updated["task"]["phase"], "RUNNING");
        assert_eq!(updated["task"]["executionRef"]["name"], "support-agent");
        assert_eq!(
            updated["task"]["executionRef"]["sessionId"],
            "support-session-1"
        );

        let listed = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-3",
            &spec,
            LIST_TASKS_TOOL,
            &json!({
                "status_group": "active",
                "owner_name": "ops-lead"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let listed: Value = serde_json::from_str(&listed).unwrap();
        assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(listed["tasks"][0]["name"], name);
    }

    #[tokio::test]
    async fn task_tools_reject_cross_namespace_overrides() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let spec = task_spec_with_internal_connection(
            &["inspect", "create"],
            "support",
            "Tenant:acme:Operations",
            "support-agent",
        );

        let create_err = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            CREATE_TASK_TOOL,
            &json!({
                "namespace": "Tenant:other:Workspace:main",
                "title": "Prepare checklist",
                "description": "Create a reviewed checklist."
            }),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(create_err.to_string().contains("cannot target namespace"));

        let delegate_err = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "namespace": "Tenant:other:Workspace:main",
                "title": "Prepare checklist",
                "description": "Create a reviewed checklist.",
                "connection": "support"
            }),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(delegate_err.to_string().contains("cannot target namespace"));
    }

    #[tokio::test]
    async fn delegate_task_starts_delegate_session() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        seed_agent(kv.as_ref(), "Tenant:acme:Operations", "support-agent").await;
        seed_session(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
        )
        .await;
        let cp = control_plane(kv.clone(), scheduler);
        let spec = task_spec_with_internal_connection(
            &["inspect", "create"],
            "support",
            "Tenant:acme:Operations",
            "support-agent",
        );

        let created = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Prepare customer onboarding checklist",
                "description": "Create a reviewed onboarding checklist.",
                "type": "OPERATIONS",
                "connection": "support"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let task = &created["task"];
        let task_name = task["name"].as_str().unwrap();
        let child_session_id = task["executionRef"]["sessionId"].as_str().unwrap();
        assert_eq!(task["phase"], "RUNNING");
        assert_eq!(task["executionRef"]["namespace"], "Tenant:acme:Operations");
        assert_eq!(task["executionRef"]["name"], "support-agent");
        assert!(!child_session_id.is_empty());
        assert!(!task["executionRef"]["runId"].as_str().unwrap().is_empty());

        let child_session = kv
            .get_msg::<data_proto::Session>(&keys::session(
                "Tenant:acme:Operations",
                "support-agent",
                child_session_id,
            ))
            .await
            .unwrap()
            .expect("child session should exist");
        assert_eq!(
            child_session.labels.get(delegation::LABEL_TASK_NAME),
            Some(&task_name.to_string())
        );
        assert_eq!(
            child_session.labels.get(delegation::LABEL_OWNER_NAMESPACE),
            Some(&"Tenant:acme:Workspace:main".to_string())
        );
        assert_eq!(
            child_session.labels.get(delegation::LABEL_OWNER_SESSION_ID),
            Some(&"session-1".to_string())
        );
        assert_eq!(
            child_session.labels.get(delegation::LABEL_A2A_CONNECTION),
            Some(&"support".to_string())
        );
        let owner_session = kv
            .get_msg::<data_proto::Session>(&keys::session(
                "Tenant:acme:Workspace:main",
                "ops-lead",
                "session-1",
            ))
            .await
            .unwrap()
            .expect("owner session should exist");
        let expected_wire_ref = format!("Tenant:acme:Operations/support-agent/{child_session_id}");
        assert_eq!(
            owner_session
                .metadata
                .get("wire.a2a.talon.impalasys.com/support-1")
                .map(String::as_str),
            Some(expected_wire_ref.as_str())
        );

        let store = ResourceStore::new(kv.clone(), Arc::new(EmptyPubSub));
        let task_resource = store
            .get("Tenant:acme:Workspace:main", "Task", task_name)
            .await
            .unwrap()
            .expect("delegated task resource should exist");
        assert_eq!(
            task_resource
                .metadata
                .as_ref()
                .unwrap()
                .labels
                .get(delegation::LABEL_A2A_CONNECTION),
            Some(&"support".to_string())
        );
        assert_eq!(
            task_resource.metadata.as_ref().unwrap().generation,
            1,
            "delegated status updates must not bump resource generation"
        );

        let entries = kv
            .list_entries(
                &keys::session_message_prefix(
                    "Tenant:acme:Operations",
                    "support-agent",
                    child_session_id,
                ),
                None,
            )
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        let message = data_proto::SessionMessage::decode(entries[0].1.as_slice()).unwrap();
        assert_eq!(
            message.labels.get(delegation::LABEL_TASK_NAME),
            Some(&task_name.to_string())
        );
        assert_eq!(
            message.labels.get(delegation::LABEL_A2A_CONNECTION),
            Some(&"support".to_string())
        );
        assert!(message
            .parts
            .first()
            .unwrap()
            .content
            .contains("Create a reviewed onboarding checklist."));

        let listed = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            LIST_TASKS_TOOL,
            &json!({
                "status_group": "active",
                "owner_name": "ops-lead"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let listed: Value = serde_json::from_str(&listed).unwrap();
        assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn delegate_task_rejects_same_wire_while_prior_task_is_busy() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        seed_agent(kv.as_ref(), "Tenant:acme:Operations", "support-agent").await;
        seed_session(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
        )
        .await;
        let cp = control_plane(kv.clone(), scheduler);
        let spec = task_spec_with_internal_connection(
            &["inspect", "create", "update"],
            "support",
            "Tenant:acme:Operations",
            "support-agent",
        );

        let first = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Prepare first checklist",
                "description": "Create the first checklist.",
                "connection": "support"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let first: Value = serde_json::from_str(&first).unwrap();
        let first_task_name = first["task"]["name"].as_str().unwrap();
        let first_session_id = first["task"]["executionRef"]["sessionId"].as_str().unwrap();

        let busy = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Prepare second checklist",
                "description": "Create the second checklist.",
                "connection": "support"
            }),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(busy.to_string().contains("still RUNNING"));

        let listed = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            LIST_TASKS_TOOL,
            &json!({"owner_name": "ops-lead"}),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let listed: Value = serde_json::from_str(&listed).unwrap();
        assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);

        execute_tool_for_session(
            &cp,
            "Tenant:acme:Operations",
            "support-agent",
            first_session_id,
            &spec,
            UPDATE_TASK_TOOL,
            &json!({
                "namespace": "Tenant:acme:Workspace:main",
                "name": first_task_name,
                "phase": "NEEDS_REVIEW",
                "progress_summary": "Ready for review."
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();

        let second = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Prepare second checklist",
                "description": "Create the second checklist.",
                "connection": "support"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let second: Value = serde_json::from_str(&second).unwrap();
        assert_eq!(
            second["task"]["executionRef"]["sessionId"]
                .as_str()
                .unwrap(),
            first_session_id,
            "same A2A wire session should be reused once prior task is review-ready"
        );
    }

    #[tokio::test]
    async fn delegate_task_completion_ignores_stale_child_sessions() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        seed_agent(kv.as_ref(), "Tenant:acme:Operations", "support-agent").await;
        seed_session(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
        )
        .await;
        let cp = control_plane(kv.clone(), scheduler);
        let spec = task_spec_with_internal_connection(
            &["create"],
            "support",
            "Tenant:acme:Operations",
            "support-agent",
        );

        let created = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Prepare checklist",
                "description": "Create a reviewed checklist.",
                "connection": "support"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let task_name = created["task"]["name"].as_str().unwrap();
        let child_session_id = created["task"]["executionRef"]["sessionId"]
            .as_str()
            .unwrap();
        let mut stale_session = kv
            .get_msg::<data_proto::Session>(&keys::session(
                "Tenant:acme:Operations",
                "support-agent",
                child_session_id,
            ))
            .await
            .unwrap()
            .unwrap();
        stale_session.id = "stale-session".to_string();

        delegation::complete_delegated_task_from_session(
            &cp,
            &stale_session,
            delegation::DelegatedSessionCompletion::Completed,
        )
        .await
        .unwrap();

        let store = ResourceStore::new(kv.clone(), Arc::new(EmptyPubSub));
        let task_resource = store
            .get("Tenant:acme:Workspace:main", "Task", task_name)
            .await
            .unwrap()
            .unwrap();
        let phase = match task_resource.status.unwrap().kind.unwrap() {
            resources_proto::resource_status::Kind::Task(status) => status.phase,
            _ => panic!("expected Task status"),
        };
        assert_eq!(phase, resources_proto::TaskPhase::Running as i32);
        assert!(session_text_messages(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
        )
        .await
        .is_empty());
    }

    #[tokio::test]
    async fn delegate_task_failure_does_not_auto_notify_owner_session() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        seed_agent(kv.as_ref(), "Tenant:acme:Operations", "support-agent").await;
        seed_session(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
        )
        .await;
        let cp = control_plane(kv.clone(), scheduler);
        let spec = task_spec_with_internal_connection(
            &["create"],
            "support",
            "Tenant:acme:Operations",
            "support-agent",
        );

        let created = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Prepare checklist",
                "description": "Create a reviewed checklist.",
                "connection": "support"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let task_name = created["task"]["name"].as_str().unwrap();
        let child_session_id = created["task"]["executionRef"]["sessionId"]
            .as_str()
            .unwrap();
        let child_session = kv
            .get_msg::<data_proto::Session>(&keys::session(
                "Tenant:acme:Operations",
                "support-agent",
                child_session_id,
            ))
            .await
            .unwrap()
            .unwrap();

        set_session_status(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            "PROCESSING",
        )
        .await;
        let task = delegation::complete_delegated_task_from_session(
            &cp,
            &child_session,
            delegation::DelegatedSessionCompletion::Failed,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            task.status.as_ref().unwrap().phase,
            resources_proto::TaskPhase::Failed as i32
        );
        assert_eq!(task.name(), task_name);
        assert!(session_text_messages(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
        )
        .await
        .is_empty());

        let owner_messages = session_text_messages(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
        )
        .await;
        assert!(
            owner_messages.is_empty(),
            "Task completion must not auto-send owner wake messages; delegates should use agent_send owner"
        );
    }

    #[tokio::test]
    async fn delegate_task_rejects_unknown_external_and_raw_delegate_targets() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let spec = task_spec_with_internal_connection(
            &["create"],
            "support",
            "Tenant:acme:Operations",
            "support-agent",
        );

        let unknown = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "connection": "missing",
                "title": "Prepare checklist",
                "description": "Create a reviewed checklist."
            }),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(unknown.to_string().contains("valid connections: support"));

        let raw = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "connection": "support",
                "title": "Prepare checklist",
                "description": "Create a reviewed checklist.",
                "delegate_name": "support-agent"
            }),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(raw.to_string().contains("delegate_name"));

        let external_spec = task_spec_with_external_connection(&["create"], "remote");
        let external = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &external_spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "connection": "remote",
                "title": "Prepare checklist",
                "description": "Create a reviewed checklist."
            }),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(external.to_string().contains("external A2A connection"));
    }

    #[tokio::test]
    async fn nested_delegation_grants_task_output_artifacts_and_notifies_through_agent_send() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let namespace = "Tenant:acme:Workspace:main";
        seed_agent(kv.as_ref(), namespace, "router").await;
        seed_agent(kv.as_ref(), namespace, "writer").await;
        seed_session(kv.as_ref(), namespace, "owner", "owner-session").await;
        let cp = control_plane(kv.clone(), scheduler);

        let owner_spec =
            task_spec_with_internal_connection(&["create"], "router", namespace, "router");
        let parent = execute_tool_for_session(
            &cp,
            namespace,
            "owner",
            "owner-session",
            &owner_spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Review legal memo",
                "description": "Route the memo to a writing delegate and return the final artifact.",
                "connection": "router"
            }), &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let parent: Value = serde_json::from_str(&parent).unwrap();
        let parent_task_name = parent["task"]["name"].as_str().unwrap();
        let router_session_id = parent["task"]["executionRef"]["sessionId"]
            .as_str()
            .unwrap();

        let router_spec = task_spec_with_internal_connection(
            &["create", "update"],
            "writer",
            namespace,
            "writer",
        );
        let child = execute_tool_for_session(
            &cp,
            namespace,
            "router",
            router_session_id,
            &router_spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Draft legal memo",
                "description": "Prepare the final legal memo artifact.",
                "connection": "writer"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let child: Value = serde_json::from_str(&child).unwrap();
        let child_task_name = child["task"]["name"].as_str().unwrap();
        let writer_session_id = child["task"]["executionRef"]["sessionId"].as_str().unwrap();

        let writer_artifact = execute_tool_for_session(
            &cp,
            namespace,
            "writer",
            writer_session_id,
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Final memo",
                "content": "# Final Memo\n\nThe agreement should be revised.",
                "media_type": "text/markdown"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let writer_artifact: Value = serde_json::from_str(&writer_artifact).unwrap();
        let artifact_uri = writer_artifact["artifactUri"].as_str().unwrap();
        let artifact_id = writer_artifact["artifact"]["id"].as_str().unwrap();

        let writer_update = execute_tool_for_session(
            &cp,
            namespace,
            "writer",
            writer_session_id,
            &task_spec(&["update"]),
            UPDATE_TASK_TOOL,
            &json!({
                "name": child_task_name,
                "phase": "NEEDS_REVIEW",
                "progress_summary": "Final memo is ready.",
                "output_artifact_uri": artifact_uri
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let writer_update: Value = serde_json::from_str(&writer_update).unwrap();
        assert_eq!(writer_update["task"]["outputArtifactUris"][0], artifact_uri);

        let wrong_task_update = execute_tool_for_session(
            &cp,
            namespace,
            "writer",
            writer_session_id,
            &task_spec(&["update"]),
            UPDATE_TASK_TOOL,
            &json!({
                "name": parent_task_name,
                "phase": "NEEDS_REVIEW",
                "progress_summary": "Writer should not be able to update the parent task."
            }),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(wrong_task_update.to_string().contains("cannot target task"));

        let writer_session = kv
            .get_msg::<data_proto::Session>(&keys::session(namespace, "writer", writer_session_id))
            .await
            .unwrap()
            .unwrap();
        let mut stale_writer_session = writer_session.clone();
        stale_writer_session.id = "stale-writer-session".to_string();
        kv.set_msg(
            &keys::session(namespace, "writer", &stale_writer_session.id),
            &stale_writer_session,
        )
        .await
        .unwrap();
        let stale_update = execute_tool_for_session(
            &cp,
            namespace,
            "writer",
            &stale_writer_session.id,
            &task_spec(&["update"]),
            UPDATE_TASK_TOOL,
            &json!({
                "name": child_task_name,
                "phase": "NEEDS_REVIEW",
                "progress_summary": "Stale writer session should not be active."
            }),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(stale_update
            .to_string()
            .contains("not the active execution session"));

        let writer_session = kv
            .get_msg::<data_proto::Session>(&keys::session(namespace, "writer", writer_session_id))
            .await
            .unwrap()
            .unwrap();
        let child_task = delegation::complete_delegated_task_from_session(
            &cp,
            &writer_session,
            delegation::DelegatedSessionCompletion::Completed,
        )
        .await
        .unwrap()
        .unwrap();
        let child_status = child_task.status.as_ref().unwrap();
        assert!(child_status.result_artifacts.is_empty());
        assert_eq!(child_status.output_artifact_uris, vec![artifact_uri]);

        let router_access = kv
            .get_msg::<crate::gateway::rpc::data_proto::ArtifactAccess>(&keys::artifact_access(
                namespace,
                "writer",
                writer_session_id,
                artifact_id,
                "router",
                router_session_id,
            ))
            .await
            .unwrap()
            .expect("update_task output artifact should grant access to the Task owner");
        assert_eq!(
            router_access.operations,
            vec!["read", "metadata", "promote"]
        );

        execute_tool_for_session(
            &cp,
            namespace,
            "writer",
            writer_session_id,
            &manifests::AgentSpec::default(),
            AGENT_SEND_TOOL,
            &json!({
                "target": "owner",
                "message": "Final memo is ready for router review."
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();

        let router_artifact = execute_tool_for_session(
            &cp,
            namespace,
            "router",
            router_session_id,
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Router notes",
                "content": "This should not be propagated.",
                "media_type": "text/plain"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let router_artifact: Value = serde_json::from_str(&router_artifact).unwrap();
        let router_artifact_id = router_artifact["artifact"]["id"].as_str().unwrap();

        let parent_update = execute_tool_for_session(
            &cp,
            namespace,
            "router",
            router_session_id,
            &task_spec(&["update"]),
            UPDATE_TASK_TOOL,
            &json!({
                "name": parent_task_name,
                "phase": "NEEDS_REVIEW",
                "progress_summary": "Final memo is ready for owner review.",
                "output_artifact_uris": [artifact_uri]
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let parent_update: Value = serde_json::from_str(&parent_update).unwrap();
        assert_eq!(parent_update["task"]["outputArtifactUris"][0], artifact_uri);

        let router_session = kv
            .get_msg::<data_proto::Session>(&keys::session(namespace, "router", router_session_id))
            .await
            .unwrap()
            .unwrap();
        let parent_task = delegation::complete_delegated_task_from_session(
            &cp,
            &router_session,
            delegation::DelegatedSessionCompletion::Completed,
        )
        .await
        .unwrap()
        .unwrap();
        let parent_status = parent_task.status.as_ref().unwrap();
        assert!(parent_status.result_artifacts.is_empty());
        assert_eq!(parent_status.output_artifact_uris, vec![artifact_uri]);

        let owner_access = kv
            .get_msg::<crate::gateway::rpc::data_proto::ArtifactAccess>(&keys::artifact_access(
                namespace,
                "writer",
                writer_session_id,
                artifact_id,
                "owner",
                "owner-session",
            ))
            .await
            .unwrap()
            .expect("update_task output artifact should grant access to the parent Task owner");
        assert_eq!(owner_access.operations, vec!["read", "metadata", "promote"]);

        let owner_send = execute_tool_for_session(
            &cp,
            namespace,
            "router",
            router_session_id,
            &manifests::AgentSpec::default(),
            AGENT_SEND_TOOL,
            &json!({
                "target": "owner",
                "message": "Final memo is ready for owner review."
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let owner_send: Value = serde_json::from_str(&owner_send).unwrap();
        assert!(
            owner_send["status"] == "QUEUED" || owner_send["dispatched"] == false,
            "agent_send should queue while the owner session is busy: {owner_send}"
        );
        assert!(
            owner_send["messageId"].is_null()
                || owner_send["messageId"].as_str().is_some_and(str::is_empty)
        );

        let unrelated_owner_access = kv
            .get_msg::<crate::gateway::rpc::data_proto::ArtifactAccess>(&keys::artifact_access(
                namespace,
                "router",
                router_session_id,
                router_artifact_id,
                "owner",
                "owner-session",
            ))
            .await
            .unwrap();
        assert!(
            unrelated_owner_access.is_none(),
            "completion must not scan and propagate unrelated session artifacts"
        );

        set_session_status(kv.as_ref(), namespace, "owner", "owner-session", "IDLE").await;
        let dispatched = crate::control::session_queue::dispatch_next_queued_message(
            kv.as_ref(),
            cp.pubsub.as_ref(),
            namespace,
            "owner",
            "owner-session",
            crate::control::session_queue::NEXT_QUEUE,
            chrono::Utc::now(),
        )
        .await
        .unwrap()
        .expect("queued owner agent_send should dispatch after owner releases");
        assert!(!dispatched.message_id.is_empty());

        let owner_messages =
            session_text_messages(kv.as_ref(), namespace, "owner", "owner-session").await;
        assert!(owner_messages
            .iter()
            .any(|message| message.contains("Final memo is ready for owner review.")));
    }

    #[tokio::test]
    async fn delegated_session_can_update_its_owner_namespace_task_only() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let owner_namespace = "Tenant:acme:Workspace:main";
        let delegate_namespace = "Tenant:acme:Nexus:copywriter";
        seed_agent(kv.as_ref(), owner_namespace, "cmo").await;
        seed_agent(kv.as_ref(), delegate_namespace, "copywriter").await;
        seed_session(kv.as_ref(), owner_namespace, "cmo", "owner-session").await;
        let cp = control_plane(kv.clone(), scheduler);

        let owner_spec = task_spec_with_internal_connection(
            &["create"],
            "copywriter",
            delegate_namespace,
            "copywriter",
        );
        let task = execute_tool_for_session(
            &cp,
            owner_namespace,
            "cmo",
            "owner-session",
            &owner_spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Draft announcement",
                "description": "Create an announcement artifact and attach it to this task.",
                "connection": "copywriter"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let task: Value = serde_json::from_str(&task).unwrap();
        let task_name = task["task"]["name"].as_str().unwrap();
        let delegate_session_id = task["task"]["executionRef"]["sessionId"].as_str().unwrap();

        let artifact = execute_tool_for_session(
            &cp,
            delegate_namespace,
            "copywriter",
            delegate_session_id,
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Announcement",
                "content": "# Announcement\n\nThe draft is ready.",
                "media_type": "text/markdown"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let artifact: Value = serde_json::from_str(&artifact).unwrap();
        let artifact_uri = artifact["artifactUri"].as_str().unwrap();

        let updated = execute_tool_for_session(
            &cp,
            delegate_namespace,
            "copywriter",
            delegate_session_id,
            &task_spec(&["update"]),
            UPDATE_TASK_TOOL,
            &json!({
                "namespace": owner_namespace,
                "name": task_name,
                "phase": "NEEDS_REVIEW",
                "progress_summary": "Draft announcement is ready.",
                "output_artifact_uri": artifact_uri
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let updated: Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(updated["task"]["outputArtifactUris"][0], artifact_uri);

        let updated_with_task_id = execute_tool_for_session(
            &cp,
            delegate_namespace,
            "copywriter",
            delegate_session_id,
            &task_spec(&["update"]),
            UPDATE_TASK_TOOL,
            &json!({
                "name": format!("{owner_namespace}/{task_name}"),
                "phase": "NEEDS_REVIEW",
                "progress_summary": "Draft announcement is still ready.",
                "output_artifact_uri": artifact_uri
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let updated_with_task_id: Value = serde_json::from_str(&updated_with_task_id).unwrap();
        assert_eq!(
            updated_with_task_id["task"]["outputArtifactUris"][0],
            artifact_uri
        );

        let rejected = execute_tool_for_session(
            &cp,
            delegate_namespace,
            "copywriter",
            delegate_session_id,
            &task_spec(&["update"]),
            UPDATE_TASK_TOOL,
            &json!({
                "namespace": owner_namespace,
                "name": "different-task",
                "phase": "NEEDS_REVIEW"
            }),
            &Config::default(),
        )
        .await
        .unwrap_err();
        assert!(rejected.to_string().contains("cannot target task"));
    }

    #[tokio::test]
    async fn goal_tools_create_update_list_and_complete() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let spec = goal_spec(&["inspect", "create", "update"]);

        let created = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            CREATE_GOAL_TOOL,
            &json!({
                "objective": "Complete the onboarding checklist to review-ready quality.",
                "success_criteria": [
                    "Uses sourced product facts",
                    "Passes critic review"
                ],
                "max_iterations": 4
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let goal_id = created["goal"]["id"].as_str().unwrap();
        assert_eq!(created["goal"]["phase"], "RUNNING");
        assert_eq!(created["goal"]["statusGroup"], "ACTIVE");

        let context =
            active_goals_context(&cp, "Tenant:acme:Workspace:main", "ops-lead", "session-1")
                .await
                .unwrap()
                .unwrap();
        assert!(context.contains("Complete the onboarding checklist"));

        let listed = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-2",
            &spec,
            LIST_GOALS_TOOL,
            &json!({
                "status_group": "active",
                "agent": "ops-lead",
                "session_id": "session-1"
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let listed: Value = serde_json::from_str(&listed).unwrap();
        assert_eq!(listed["goals"].as_array().unwrap().len(), 1);
        assert_eq!(listed["goals"][0]["id"], goal_id);

        let listed_from_session = list_goals(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            Some("active"),
            None,
            10,
        )
        .await
        .unwrap();
        assert_eq!(listed_from_session.len(), 1);
        assert_eq!(listed_from_session[0].id, goal_id);

        let updated = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            UPDATE_GOAL_TOOL,
            &json!({
                "goal_id": goal_id,
                "phase": "NEEDS_REVIEW",
                "iteration": 2,
                "progress_summary": "Support task produced the revised checklist; draft is ready for critic review."
            }), &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let updated: Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(updated["goal"]["phase"], "NEEDS_REVIEW");
        assert_eq!(updated["goal"]["iteration"], 2);
        assert!(updated["goal"]["progressSummary"]
            .as_str()
            .unwrap()
            .contains("revised checklist"));

        let completed = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            COMPLETE_GOAL_TOOL,
            &json!({
                "goal_id": goal_id,
                "progress_summary": "Reviewer approved the final checklist."
            }),
            &Config::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let completed: Value = serde_json::from_str(&completed).unwrap();
        assert_eq!(completed["goal"]["phase"], "SUCCEEDED");
        assert_eq!(completed["goal"]["statusGroup"], "TERMINAL");

        let active_context =
            active_goals_context(&cp, "Tenant:acme:Workspace:main", "ops-lead", "session-1")
                .await
                .unwrap();
        assert!(active_context.is_none());
    }
}
