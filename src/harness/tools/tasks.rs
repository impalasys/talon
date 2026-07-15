// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::control::resource_model::TypedResource;
use crate::control::resources::ResourceStore;
use crate::control::ControlPlane;
use crate::control::{keys, ProtoKeyValueStoreExt};
use crate::gateway::rpc::data_proto;
use crate::gateway::rpc::{manifests, resources_proto};
use crate::harness::skills::registry::ToolRegistry;

fn task_namespace<'a>(args: &'a Value, current_namespace: &'a str) -> Result<&'a str> {
    let namespace = super::opt_str(args, "namespace").unwrap_or(current_namespace);
    if namespace != current_namespace {
        return Err(anyhow!(
            "task tools cannot target namespace '{}' from agent namespace '{}'",
            namespace,
            current_namespace
        ));
    }
    Ok(namespace)
}

fn split_task_id(value: &str) -> Option<(&str, &str)> {
    let (namespace, name) = value.split_once('/')?;
    let namespace = namespace.trim();
    let name = name.trim();
    if namespace.is_empty() || name.is_empty() || name.contains('/') {
        return None;
    }
    Some((namespace, name))
}

async fn authorized_update_task_namespace(
    cp: &ControlPlane,
    args: &Value,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
) -> Result<(String, String)> {
    let raw_name = super::req_str(args, "name")?;
    let explicit_namespace = super::opt_str(args, "namespace");
    let (namespace, name) = if let Some((id_namespace, id_name)) = split_task_id(raw_name) {
        if let Some(namespace) = explicit_namespace {
            if namespace != id_namespace {
                return Err(anyhow!(
                    "task namespace '{}' does not match Task ID namespace '{}'",
                    namespace,
                    id_namespace
                ));
            }
        }
        (id_namespace, id_name)
    } else {
        (explicit_namespace.unwrap_or(current_namespace), raw_name)
    };

    if namespace == current_namespace {
        return Ok((namespace.to_string(), name.to_string()));
    }

    let session = cp
        .kv
        .get_msg::<data_proto::Session>(&keys::session(
            current_namespace,
            current_agent,
            current_session,
        ))
        .await?;
    let Some(session) = session else {
        return Err(anyhow!(
            "task tools cannot target namespace '{}' from agent namespace '{}'",
            namespace,
            current_namespace
        ));
    };

    let is_delegate = session
        .labels
        .get(crate::control::delegation::LABEL_TASK_ROLE)
        .map(String::as_str)
        == Some("delegate");
    let assigned_namespace = session
        .labels
        .get(crate::control::delegation::LABEL_TASK_NAMESPACE)
        .map(String::as_str);
    let assigned_name = session
        .labels
        .get(crate::control::delegation::LABEL_TASK_NAME)
        .map(String::as_str);
    if is_delegate && assigned_namespace == Some(namespace) && assigned_name == Some(name) {
        return Ok((namespace.to_string(), name.to_string()));
    }

    Err(anyhow!(
        "task tools cannot target namespace '{}' from agent namespace '{}'",
        namespace,
        current_namespace
    ))
}

pub(super) fn register(registry: &mut ToolRegistry, spec: &manifests::AgentSpec) {
    if super::has_capability_action(spec, "tasks", "inspect") {
        registry.register_builtin(
            super::LIST_TASKS_TOOL,
            "List durable Talon Tasks in a namespace. Use this to rediscover delegated work across sessions.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Namespace to inspect. Defaults to the current agent namespace if omitted." },
                    "status_group": { "type": "string", "description": "Optional group: active or terminal." },
                    "phase": { "type": "string", "description": "Optional phase filter such as RUNNING, NEEDS_REVIEW, SUCCEEDED, FAILED, or CANCELED." },
                    "owner_name": { "type": "string", "description": "Optional owner agent resource name filter." },
                    "delegate_name": { "type": "string", "description": "Optional delegate agent resource name filter." },
                    "limit": { "type": "integer", "description": "Optional maximum number of results to return." }
                }
            }),
        );
        registry.register_builtin(
            super::GET_TASK_TOOL,
            "Get one durable Talon Task by name.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Namespace containing the task. Defaults to the current agent namespace if omitted." },
                    "name": { "type": "string", "description": "Task resource name." }
                },
                "required": ["name"]
            }),
        );
    }

    if super::has_capability_action(spec, "tasks", "create") {
        registry.register_builtin(
            super::CREATE_TASK_TOOL,
            "Create a durable caller-owned Task record without starting delegate execution.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Namespace that owns the task. Defaults to the current agent namespace if omitted." },
                    "title": { "type": "string", "description": "Short human-readable task title." },
                    "description": { "type": "string", "description": "Brief or acceptance criteria for the task." },
                    "type": { "type": "string", "description": "Optional caller-defined classifier such as agent_delegation or human_review. Talon does not interpret it." },
                    "delegate_namespace": { "type": "string", "description": "Namespace of the worker agent." },
                    "delegate_name": { "type": "string", "description": "Worker agent resource name." }
                },
                "required": ["title", "description", "delegate_name"]
            }),
        );
    }

    let internal_connections = crate::harness::a2a::internal_connection_names(spec);
    if super::has_capability_action(spec, "tasks", "create") && !internal_connections.is_empty() {
        registry.register_builtin(
            super::DELEGATE_TASK_TOOL,
            "Create a durable Task and start a declared A2A internal connection in a linked child session.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Namespace that owns the task. Defaults to the current agent namespace if omitted." },
                    "connection": {
                        "type": "string",
                        "description": "Declared A2A connection name to delegate through.",
                        "enum": internal_connections
                    },
                    "title": { "type": "string", "description": "Short human-readable task title." },
                    "description": { "type": "string", "description": "Full task brief, success criteria, and context for the delegate." },
                    "type": { "type": "string", "description": "Optional caller-defined classifier. Defaults to agent_delegation." }
                },
                "required": ["connection", "title", "description"]
            }),
        );
    }

    if super::has_capability_action(spec, "tasks", "update") {
        registry.register_builtin(
            super::UPDATE_TASK_TOOL,
            "Update Task status after delegation progress, review, completion, or failure.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Namespace containing the task. Defaults to the current agent namespace if omitted." },
                    "name": { "type": "string", "description": "Task resource name." },
                    "phase": { "type": "string", "description": "Optional phase: QUEUED, RUNNING, BLOCKED, NEEDS_REVIEW, SUCCEEDED, FAILED, CANCELED, or EXPIRED." },
                    "progress_summary": { "type": "string", "description": "Short current state or result summary." },
                    "output_artifact_uri": { "type": "string", "description": "Artifact URI to attach as a Task output." },
                    "output_artifact_uris": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Artifact URIs to attach as Task outputs."
                    },
                    "execution_namespace": { "type": "string", "description": "Optional execution namespace." },
                    "execution_name": { "type": "string", "description": "Optional execution agent resource name." },
                    "execution_session_id": { "type": "string", "description": "Optional child session id." },
                    "run_id": { "type": "string", "description": "Optional workflow or run id." }
                },
                "required": ["name"]
            }),
        );
    }
}

pub(super) async fn execute(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    name: &str,
    args: &Value,
) -> Result<Option<String>> {
    match name {
        super::LIST_TASKS_TOOL => {
            super::require_capability(spec, "tasks", "inspect")?;
            let namespace = task_namespace(args, current_namespace)?;
            let status_group = super::opt_str(args, "status_group");
            let phase = super::opt_str(args, "phase");
            let owner_name = super::opt_str(args, "owner_name");
            let delegate_name = super::opt_str(args, "delegate_name");
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(100) as usize;
            let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
            let mut resources = store.list(namespace, Some("Task")).await?;
            resources.sort_by(|a, b| super::task_updated_at(b).cmp(&super::task_updated_at(a)));
            let mut tasks = Vec::new();
            for resource in resources {
                let Some(task) = super::task_from_resource(resource) else {
                    continue;
                };
                if !super::task_matches(&task, status_group, phase, owner_name, delegate_name) {
                    continue;
                }
                tasks.push(super::task_json(&task));
                if tasks.len() >= limit {
                    break;
                }
            }
            Ok(Some(serde_json::to_string_pretty(
                &json!({ "tasks": tasks }),
            )?))
        }
        super::GET_TASK_TOOL => {
            super::require_capability(spec, "tasks", "inspect")?;
            let namespace = task_namespace(args, current_namespace)?;
            let name = super::req_str(args, "name")?;
            let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
            let resource = store
                .get(namespace, "Task", name)
                .await?
                .ok_or_else(|| anyhow!("task '{}' not found", name))?;
            let task =
                super::task_from_resource(resource).ok_or_else(|| anyhow!("invalid Task"))?;
            Ok(Some(serde_json::to_string_pretty(&json!({
                "task": super::task_json(&task)
            }))?))
        }
        super::CREATE_TASK_TOOL => {
            super::require_capability(spec, "tasks", "create")?;
            task_namespace(args, current_namespace)?;
            let task = super::create_task(current_namespace, current_agent, args)?;
            let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
            let namespace = task.namespace().to_string();
            let resource = store
                .upsert(&namespace, super::task_resource_from_task(task))
                .await?;
            let task =
                super::task_from_resource(resource).ok_or_else(|| anyhow!("invalid Task"))?;
            Ok(Some(serde_json::to_string_pretty(&json!({
                "task": super::task_json(&task)
            }))?))
        }
        super::DELEGATE_TASK_TOOL => {
            super::require_capability(spec, "tasks", "create")?;
            task_namespace(args, current_namespace)?;
            let task = super::delegate_task(
                cp,
                current_namespace,
                current_agent,
                current_session,
                args,
                spec,
            )
            .await?;
            Ok(Some(serde_json::to_string_pretty(&json!({
                "task": super::task_json(&task)
            }))?))
        }
        super::UPDATE_TASK_TOOL => {
            super::require_capability(spec, "tasks", "update")?;
            let (namespace, name) = authorized_update_task_namespace(
                cp,
                args,
                current_namespace,
                current_agent,
                current_session,
            )
            .await?;
            let output_artifact_uris = super::task_output_artifact_uris_from_args(
                cp,
                current_agent,
                current_session,
                args,
            )
            .await?;
            let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
            let resource = store
                .patch_status_with(&namespace, "Task", &name, None, |_, status| {
                    let mut task_status = match status.kind.take() {
                        Some(resources_proto::resource_status::Kind::Task(status)) => status,
                        _ => resources_proto::TaskStatus::default(),
                    };
                    super::update_task_status(&mut task_status, args, &output_artifact_uris)?;
                    status.kind = Some(resources_proto::resource_status::Kind::Task(task_status));
                    Ok(())
                })
                .await?;
            let task =
                super::task_from_resource(resource).ok_or_else(|| anyhow!("invalid Task"))?;
            Ok(Some(serde_json::to_string_pretty(&json!({
                "task": super::task_json(&task)
            }))?))
        }
        _ => Ok(None),
    }
}
