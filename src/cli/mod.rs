// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::resource_model::{self, ChannelSubscriptionResourceExt, TypedResource};
use crate::gateway::rpc::manifests::{Knowledge, KnowledgeSpec, ObjectMeta};
use crate::gateway::rpc::proto::gateway_service_client::GatewayServiceClient;
use crate::gateway::rpc::proto::{
    CancelWorkflowRunRequest, CreateResourceRequest, CreateWorkflowRunRequest,
    DeleteResourceRequest, GetResourceRequest, GetWorkflowRunRequest, ListNamespacesRequest,
    ListResourcesRequest, ListWorkflowRunsRequest, ResumeWorkflowRunRequest,
    StreamWorkflowEventsRequest,
};
use crate::gateway::rpc::{data_proto, resources_proto};
use anyhow::{Context, Result};
use clap::Parser;
use futures::StreamExt;
use minijinja::{context, Environment, UndefinedBehavior};
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

mod auth;
mod commands;

use commands::Cli;

pub(super) use auth::{
    auth_interceptor, connect_gateway, mint_agent_jwt, mint_channel_jwt, mint_root_jwt,
    mint_session_jwt, resolve_gateway_jwt_secret, resolve_gateway_password, rest_client,
};

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

fn rest_grpc_error_details(headers: &reqwest::header::HeaderMap) -> String {
    let grpc_status = headers
        .get("grpc-status")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty());
    let grpc_message = headers
        .get("grpc-message")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            urlencoding::decode(value)
                .map(|decoded| decoded.into_owned())
                .unwrap_or_else(|_| value.to_string())
        });

    match (grpc_status, grpc_message.as_deref()) {
        (Some(status), Some(message)) => {
            format!(" grpc-status={} grpc-message={}", status, message)
        }
        (Some(status), None) => format!(" grpc-status={}", status),
        (None, Some(message)) => format!(" grpc-message={}", message),
        (None, None) => String::new(),
    }
}

pub(super) async fn rest_request_json(
    cli: &Cli,
    method: reqwest::Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    let client = rest_client(cli)?;
    let url = format!("{}{}", cli.gateway.trim_end_matches('/'), path);
    let mut request = client.request(method, &url);
    if let Some(payload) = body {
        request = request
            .header(CONTENT_TYPE, "application/json")
            .json(&payload);
    }
    let response = request
        .send()
        .await
        .with_context(|| format!("Failed to call REST endpoint {}", url))?;
    let status = response.status();
    let headers = response.headers().clone();
    let text = response
        .text()
        .await
        .with_context(|| format!("Failed to read REST response body from {}", url))?;
    if !status.is_success() {
        anyhow::bail!(
            "REST {} {} failed: status={} body={}{}",
            path,
            url,
            status,
            text.trim(),
            rest_grpc_error_details(&headers)
        );
    }
    if text.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(&text)
        .with_context(|| format!("Failed to parse REST response JSON from {}", url))
}

fn agent_lookup_target(name: &str, namespace: Option<&String>) -> (String, String) {
    let mut parts = name.splitn(2, '/');
    let ns_part = parts.next().unwrap_or("default");
    let agent_name = parts.next().unwrap_or(ns_part);
    let (mut final_ns, final_name) = if agent_name == ns_part {
        ("default".to_string(), ns_part.to_string())
    } else {
        (ns_part.to_string(), agent_name.to_string())
    };
    if let Some(n) = namespace {
        final_ns = n.clone();
    }
    (final_ns, final_name)
}

fn rest_get_path(
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<(String, &'static str)> {
    match kind.to_lowercase().as_str() {
        "agenttemplate" | "templates" | "template" => {
            let ns = namespace
                .cloned()
                .unwrap_or_else(|| crate::control::ns::TALON_SYSTEM.to_string());
            Ok((
                format!(
                    "/v2/ns/{}/resources/Template/{}",
                    urlencoding::encode(&ns),
                    urlencoding::encode(name)
                ),
                "resource",
            ))
        }
        "mcpserver" | "mcpservers" | "mcp" => Ok((
            format!("/v1/mcp-servers/{}", urlencoding::encode(name)),
            "server",
        )),
        "agent" | "agents" => {
            let (ns, agent_name) = agent_lookup_target(name, namespace);
            Ok((
                format!(
                    "/v1/ns/{}/agents/{}",
                    urlencoding::encode(&ns),
                    urlencoding::encode(&agent_name)
                ),
                "agent",
            ))
        }
        "mcpserverbinding" | "mcpbindings" | "mcpbinding" => {
            let ns = namespace
                .as_ref()
                .context("namespace is required for McpServerBinding get")?;
            Ok((
                format!(
                    "/v1/namespaces/{}/mcp-bindings/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "binding",
            ))
        }
        "namespace" | "namespaces" => Ok((
            format!("/v1/namespaces/{}", urlencoding::encode(name)),
            "namespace",
        )),
        "knowledge" | "knowledgeartifact" | "knowledgeartifacts" => {
            let ns = namespace
                .as_ref()
                .context("Knowledge get requires --namespace")?;
            Ok((
                format!(
                    "/v1/namespaces/{}/knowledge/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "knowledge",
            ))
        }
        "schedule" | "schedules" => {
            let ns = namespace
                .as_ref()
                .context("Schedule get requires --namespace")?;
            Ok((
                format!(
                    "/v1/ns/{}/schedules/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "schedule",
            ))
        }
        "channel" | "channels" => {
            let ns = namespace
                .as_ref()
                .context("Channel get requires --namespace")?;
            Ok((
                format!(
                    "/v1/ns/{}/channels/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "channel",
            ))
        }
        "channelsubscription"
        | "channelsubscriptions"
        | "channel-subscription"
        | "channel-subscriptions" => {
            let ns = namespace
                .as_ref()
                .context("ChannelSubscription get requires --namespace")?;
            let (channel, subscription) = name
                .split_once('/')
                .context("ChannelSubscription name must be '<channel>/<subscription>'")?;
            Ok((
                format!(
                    "/v1/ns/{}/channels/{}/subscriptions/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(channel),
                    urlencoding::encode(subscription)
                ),
                "subscription",
            ))
        }
        "workflow" | "workflows" => {
            let ns = namespace
                .as_ref()
                .context("Workflow get requires --namespace")?;
            Ok((
                format!(
                    "/v1/ns/{}/workflows/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "workflow",
            ))
        }
        other => anyhow::bail!("Unsupported resource kind '{}' for REST mode", other),
    }
}

fn rest_delete_path(kind: &str, name: &str, namespace: Option<&String>) -> Result<String> {
    if matches!(kind.to_lowercase().as_str(), "namespace" | "namespaces") {
        return Ok(format!("/v1/namespaces/{}", urlencoding::encode(name)));
    }
    let (ns, resource_kind, resource_name) = resource_lookup_target(kind, name, namespace)?;
    Ok(format!(
        "/v2/ns/{}/resources/{}/{}",
        urlencoding::encode(&ns),
        urlencoding::encode(&resource_kind),
        urlencoding::encode(&resource_name)
    ))
}

pub(super) fn render_json_payload(content: &str) -> Result<serde_json::Value> {
    let raw = parse_raw_manifest(content)?;
    let manifest_value: serde_yaml::Value =
        serde_yaml::from_str(content).context("Failed to parse rendered manifest")?;
    match raw.kind.as_str() {
        "MCPServer" | "McpServer" => Ok(json!({ "server": manifest_value })),
        "Agent" => Ok(json!({ "agent": manifest_value })),
        "McpServerBinding" => {
            let binding = crate::control::manifest::parse_mcp_server_binding(content)?;
            let namespace = binding
                .metadata
                .as_ref()
                .map(|meta| meta.namespace.clone())
                .filter(|namespace| !namespace.is_empty())
                .context("McpServerBinding missing metadata.namespace")?;
            Ok(json!({
                "ns": namespace,
                "binding": binding,
            }))
        }
        "Namespace" => {
            let namespace = crate::control::manifest::parse_namespace(content)?;
            Ok(json!({
                "name": namespace.name(),
                "recursive": true,
                "labels": namespace.labels(),
            }))
        }
        "Knowledge" => Ok(json!({ "knowledge": manifest_value })),
        "Channel" => {
            let channel = crate::control::manifest::parse_channel(content)?;
            Ok(json!({ "ns": channel.namespace(), "channel": channel }))
        }
        "ChannelSubscription" => {
            let subscription = crate::control::manifest::parse_channel_subscription(content)?;
            Ok(json!({
                "ns": subscription.namespace(),
                "channel": subscription.channel(),
                "subscription": subscription,
            }))
        }
        "Workflow" => {
            let workflow = crate::control::manifest::parse_workflow(content)?;
            Ok(json!({ "ns": workflow.namespace(), "workflow": workflow }))
        }
        other => anyhow::bail!("Unsupported manifest kind '{}'", other),
    }
}

pub(super) async fn rest_get_yaml(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let (path, response_key) = rest_get_path(kind, name, namespace)?;
    let resp = rest_request_json(cli, reqwest::Method::GET, &path, None)
        .await
        .with_context(|| format!("Failed to fetch {} '{}'", kind, name))?;
    let value = if response_key == "namespace" {
        resp
    } else {
        resp.get(response_key)
            .cloned()
            .or_else(|| (response_key == "card" && resp.get("cards").is_some()).then_some(resp))
            .with_context(|| format!("REST response missing {}", response_key))?
    };
    render_rest_get_yaml(response_key, value)
        .with_context(|| format!("Failed to serialize {} YAML", kind))
}

pub(super) async fn rest_get_json(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<serde_json::Value> {
    let (path, response_key) = rest_get_path(kind, name, namespace)?;
    let resp = rest_request_json(cli, reqwest::Method::GET, &path, None)
        .await
        .with_context(|| format!("Failed to fetch {} '{}'", kind, name))?;
    let value = if response_key == "namespace" {
        resp
    } else {
        resp.get(response_key)
            .cloned()
            .or_else(|| (response_key == "card" && resp.get("cards").is_some()).then_some(resp))
            .with_context(|| format!("REST response missing {}", response_key))?
    };
    match response_key {
        "resource" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let resource: crate::gateway::rpc::resources_proto::Resource =
                serde_json::from_value(value).context("Failed to decode Resource JSON")?;
            resource_manifest_json(&resource)
        }
        _ => Ok(value),
    }
}

fn render_rest_get_yaml(response_key: &str, value: serde_json::Value) -> Result<String> {
    match response_key {
        "resource" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let resource: crate::gateway::rpc::resources_proto::Resource =
                serde_json::from_value(value).context("Failed to decode Resource JSON")?;
            crate::control::manifest::render_resource_yaml(&resource)
        }
        "agent" => render_rest_agent_yaml(value),
        "namespace" => render_rest_namespace_yaml(value),
        "server" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let server: crate::gateway::rpc::manifests::McpServer =
                serde_json::from_value(value).context("Failed to decode MCPServer JSON")?;
            crate::control::manifest::render_mcp_server_yaml(&server)
        }
        "binding" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let binding: crate::gateway::rpc::manifests::McpServerBinding =
                serde_json::from_value(value).context("Failed to decode McpServerBinding JSON")?;
            crate::control::manifest::render_mcp_server_binding_yaml(&binding)
        }
        "knowledge" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let knowledge: crate::gateway::rpc::manifests::Knowledge =
                serde_json::from_value(value).context("Failed to decode Knowledge JSON")?;
            crate::control::manifest::render_knowledge_yaml(&knowledge)
        }
        "schedule" => serde_yaml::to_string(&value).context("Failed to serialize Schedule YAML"),
        "channel" => {
            let mut value = value;
            normalize_json_int64_fields(
                &mut value,
                &["createdAt", "created_at", "updatedAt", "updated_at"],
            )?;
            let channel: resources_proto::Channel =
                serde_json::from_value(value).context("Failed to decode Channel JSON")?;
            crate::control::manifest::render_channel_yaml(&channel)
        }
        "subscription" => {
            let subscription: resources_proto::ChannelSubscription = serde_json::from_value(value)
                .context("Failed to decode ChannelSubscription JSON")?;
            crate::control::manifest::render_channel_subscription_yaml(&subscription)
        }
        "workflow" => {
            let workflow: resources_proto::Workflow =
                serde_json::from_value(value).context("Failed to decode Workflow JSON")?;
            crate::control::manifest::render_workflow_yaml(&workflow)
        }
        other => anyhow::bail!("Unsupported REST response resource '{}'", other),
    }
}

fn normalize_manifest_metadata_maps(value: &mut serde_json::Value) {
    let Some(metadata) = value
        .get_mut("metadata")
        .and_then(|metadata| metadata.as_object_mut())
    else {
        return;
    };

    for key in ["labels", "annotations"] {
        if metadata.get(key).is_some_and(|value| value.is_null()) {
            metadata.insert(key.to_string(), json!({}));
        }
    }
}

fn normalize_json_int64_fields(value: &mut serde_json::Value, fields: &[&str]) -> Result<()> {
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };

    for field in fields {
        let Some(field_value) = object.get_mut(*field) else {
            continue;
        };
        let Some(raw) = field_value.as_str() else {
            continue;
        };
        let parsed = raw
            .parse::<i64>()
            .with_context(|| format!("Failed to parse {field} as int64"))?;
        *field_value = serde_json::Value::Number(parsed.into());
    }

    Ok(())
}

fn render_rest_agent_yaml(agent: serde_json::Value) -> Result<String> {
    let name = agent
        .get("name")
        .or_else(|| agent.get("agent"))
        .and_then(|name| name.as_str())
        .context("Agent response missing name")?;
    let namespace = agent
        .get("ns")
        .and_then(|namespace| namespace.as_str())
        .context("Agent response missing ns")?;
    let spec = agent
        .get("spec")
        .cloned()
        .context("Agent response missing spec")?;
    let labels = agent
        .get("labels")
        .filter(|labels| !labels.is_null())
        .cloned()
        .unwrap_or_else(|| json!({}));

    serde_yaml::to_string(&json!({
        "apiVersion": "talon.impalasys.com/v1",
        "kind": "Agent",
        "metadata": {
            "name": name,
            "namespace": namespace,
            "labels": labels,
        },
        "spec": spec,
    }))
    .context("Failed to serialize Agent YAML")
}

fn render_rest_namespace_yaml(namespace: serde_json::Value) -> Result<String> {
    let name = namespace
        .get("name")
        .and_then(|name| name.as_str())
        .context("Namespace response missing name")?;
    let labels = namespace
        .get("labels")
        .filter(|labels| !labels.is_null())
        .cloned()
        .unwrap_or_else(|| json!({}));

    serde_yaml::to_string(&json!({
        "apiVersion": "talon.impalasys.com/v1",
        "kind": "Namespace",
        "metadata": {
            "name": name,
            "labels": labels,
        },
    }))
    .context("Failed to serialize Namespace YAML")
}

pub(super) async fn rest_delete_resource(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let path = rest_delete_path(kind, name, namespace)?;
    rest_request_json(cli, reqwest::Method::DELETE, &path, None)
        .await
        .with_context(|| format!("Failed to delete {} '{}'", kind, name))?;
    Ok(format!("✓ {} '{}' deleted successfully.", kind, name))
}

fn knowledge_resource_name(path: &str) -> String {
    path.to_string()
}

fn build_knowledge(namespace: &str, path: &str, content: String) -> Knowledge {
    Knowledge {
        metadata: Some(ObjectMeta {
            name: knowledge_resource_name(path),
            namespace: namespace.to_string(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
            owner_references: Vec::new(),
            finalizers: Vec::new(),
            generation: 0,
            resource_version: String::new(),
            uid: String::new(),
            deletion_timestamp: None,
        }),
        spec: Some(KnowledgeSpec {
            path: path.to_string(),
            content,
        }),
        status: Some(resource_model::common_status(String::new())),
    }
}

fn knowledge_resource_manifest_proto(
    knowledge: &Knowledge,
) -> Result<resources_proto::ResourceManifest> {
    Ok(resources_proto::ResourceManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "Knowledge".to_string(),
        metadata: knowledge.metadata.clone(),
        spec: Some(resources_proto::ResourceSpec {
            kind: Some(resources_proto::resource_spec::Kind::Knowledge(
                knowledge.spec.clone().context("Knowledge missing spec")?,
            )),
        }),
    })
}

fn knowledge_from_resource_proto(resource: resources_proto::Resource) -> Option<Knowledge> {
    let spec = resource.spec.and_then(|spec| match spec.kind {
        Some(resources_proto::resource_spec::Kind::Knowledge(spec)) => Some(spec),
        _ => None,
    })?;
    let status = resource.status.and_then(|status| match status.kind {
        Some(resources_proto::resource_status::Kind::Knowledge(status)) => Some(status),
        _ => None,
    });
    Some(Knowledge {
        metadata: resource.metadata,
        spec: Some(spec),
        status,
    })
}

fn knowledge_resource_manifest_json(knowledge: &Knowledge) -> serde_json::Value {
    json!({
        "apiVersion": "talon.impalasys.com/v1",
        "kind": "Knowledge",
        "metadata": knowledge.metadata,
        "spec": {
            "knowledge": knowledge.spec,
        },
    })
}

fn knowledge_from_resource_json(resource: serde_json::Value) -> Result<Option<Knowledge>> {
    let metadata = resource.get("metadata").cloned();
    let spec = resource
        .get("spec")
        .and_then(|spec| spec.get("knowledge"))
        .cloned();
    let status = resource
        .get("status")
        .and_then(|status| status.get("knowledge"))
        .cloned();
    let Some(spec) = spec else {
        return Ok(None);
    };
    Ok(Some(Knowledge {
        metadata: metadata
            .map(serde_json::from_value)
            .transpose()
            .context("Failed to decode Knowledge metadata")?,
        spec: Some(serde_json::from_value(spec).context("Failed to decode Knowledge spec")?),
        status: status
            .map(serde_json::from_value)
            .transpose()
            .context("Failed to decode Knowledge status")?,
    }))
}

pub(super) fn read_knowledge_content(
    file: &Option<String>,
    content: &Option<String>,
) -> Result<String> {
    match (file, content) {
        (Some(path), None) => fs::read_to_string(path)
            .with_context(|| format!("Failed to read knowledge content from '{}'", path)),
        (None, Some(value)) => Ok(value.clone()),
        (Some(_), Some(_)) => anyhow::bail!("Specify only one of --file or --content"),
        (None, None) => anyhow::bail!("One of --file or --content is required"),
    }
}

fn relative_knowledge_path(root: &Path, file: &Path) -> Result<String> {
    let relative = file.strip_prefix(root).with_context(|| {
        format!(
            "Knowledge file '{}' is not inside '{}'",
            file.display(),
            root.display()
        )
    })?;
    let path = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    if path.is_empty() {
        anyhow::bail!("Knowledge path cannot be empty for '{}'", file.display());
    }
    Ok(path)
}

fn collect_markdown_files(dir: &Path) -> Result<Vec<PathBuf>> {
    fn walk(current: &Path, acc: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(current)
            .with_context(|| format!("Failed to read directory '{}'", current.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walk(&path, acc)?;
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
            {
                acc.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    walk(dir, &mut files)?;
    files.sort();
    Ok(files)
}

pub(super) async fn knowledge_get(
    cli: &Cli,
    namespace: &str,
    path: &str,
) -> Result<Option<Knowledge>> {
    let name = knowledge_resource_name(path);
    if cli.rest {
        let resp = rest_request_json(
            cli,
            reqwest::Method::GET,
            &format!(
                "/v2/ns/{}/resources/Knowledge/{}",
                urlencoding::encode(namespace),
                urlencoding::encode(&name)
            ),
            None,
        )
        .await?;
        let Some(resource) = resp.get("resource").cloned() else {
            return Ok(None);
        };
        Ok(knowledge_from_resource_json(resource)?)
    } else {
        let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
            .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
            .connect()
            .await
            .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
        let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);
        let response = client
            .get_resource(GetResourceRequest {
                ns: namespace.to_string(),
                kind: "Knowledge".to_string(),
                name,
            })
            .await;
        match response {
            Ok(resp) => Ok(resp
                .into_inner()
                .resource
                .and_then(knowledge_from_resource_proto)),
            Err(status) if status.code() == tonic::Code::NotFound => Ok(None),
            Err(status) => Err(status).context(format!(
                "Failed to fetch Knowledge '{}/{}'",
                namespace, path
            )),
        }
    }
}

pub(super) async fn knowledge_set(
    cli: &Cli,
    namespace: &str,
    path: &str,
    content: String,
) -> Result<()> {
    let knowledge = build_knowledge(namespace, path, content);
    if cli.rest {
        rest_request_json(
            cli,
            reqwest::Method::POST,
            &format!("/v2/ns/{}/resources", urlencoding::encode(namespace)),
            Some(json!({
                "ns": namespace,
                "manifest": knowledge_resource_manifest_json(&knowledge),
            })),
        )
        .await
        .with_context(|| format!("Failed to write Knowledge '{}/{}'", namespace, path))?;
    } else {
        let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
            .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
            .connect()
            .await
            .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
        let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);
        client
            .create_resource(CreateResourceRequest {
                ns: namespace.to_string(),
                manifest: Some(knowledge_resource_manifest_proto(&knowledge)?),
            })
            .await
            .with_context(|| format!("Failed to write Knowledge '{}/{}'", namespace, path))?;
    }
    Ok(())
}

pub(super) async fn knowledge_delete(cli: &Cli, namespace: &str, path: &str) -> Result<()> {
    let name = knowledge_resource_name(path);
    if cli.rest {
        rest_request_json(
            cli,
            reqwest::Method::DELETE,
            &format!(
                "/v2/ns/{}/resources/Knowledge/{}",
                urlencoding::encode(namespace),
                urlencoding::encode(&name)
            ),
            None,
        )
        .await
        .with_context(|| format!("Failed to delete Knowledge '{}/{}'", namespace, path))?;
    } else {
        let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
            .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
            .connect()
            .await
            .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
        let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);
        client
            .delete_resource(DeleteResourceRequest {
                ns: namespace.to_string(),
                kind: "Knowledge".to_string(),
                name,
            })
            .await
            .with_context(|| format!("Failed to delete Knowledge '{}/{}'", namespace, path))?;
    }
    Ok(())
}

async fn knowledge_list(cli: &Cli, namespace: &str) -> Result<Vec<Knowledge>> {
    if cli.rest {
        let resp = rest_request_json(
            cli,
            reqwest::Method::GET,
            &format!(
                "/v2/ns/{}/resources?kind=Knowledge",
                urlencoding::encode(namespace)
            ),
            None,
        )
        .await?;
        let resources = resp
            .get("resources")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
        let resources = resources.as_array().cloned().unwrap_or_default();
        resources
            .into_iter()
            .map(knowledge_from_resource_json)
            .collect::<Result<Vec<_>>>()
            .map(|items| items.into_iter().flatten().collect())
    } else {
        let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
            .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
            .connect()
            .await
            .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
        let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);
        Ok(client
            .list_resources(ListResourcesRequest {
                ns: namespace.to_string(),
                kind: Some("Knowledge".to_string()),
            })
            .await
            .with_context(|| format!("Failed to list Knowledge for '{}'", namespace))?
            .into_inner()
            .resources
            .into_iter()
            .filter_map(knowledge_from_resource_proto)
            .collect())
    }
}

pub(super) fn read_json_arg(value: &Option<String>, file: &Option<String>) -> Result<String> {
    let raw = if let Some(file) = file {
        fs::read_to_string(file).with_context(|| format!("Failed to read JSON file '{}'", file))?
    } else {
        value.clone().unwrap_or_else(|| "{}".to_string())
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).context("Argument must be valid JSON")?;
    serde_json::to_string(&parsed).context("Failed to normalize JSON argument")
}

pub(super) async fn workflow_run_create(
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

pub(super) async fn workflow_run_get(
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

pub(super) async fn workflow_run_list(
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

pub(super) async fn workflow_run_resume(
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

pub(super) async fn workflow_run_cancel(
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

pub(super) async fn workflow_run_events(
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

pub(super) async fn sync_knowledge_dir(
    cli: &Cli,
    namespace: &str,
    dir: &str,
) -> Result<(usize, Vec<String>)> {
    let root = Path::new(dir);
    let files = collect_markdown_files(root)?;
    let existing: Vec<Knowledge> = knowledge_list(cli, namespace).await?;
    let existing_paths = existing
        .into_iter()
        .filter_map(|knowledge| knowledge.spec.map(|spec| spec.path))
        .collect::<std::collections::HashSet<_>>();
    let mut synced_paths = Vec::new();

    for file in files {
        let knowledge_path = relative_knowledge_path(root, &file)?;
        let content = fs::read_to_string(&file)
            .with_context(|| format!("Failed to read knowledge file '{}'", file.display()))?;
        knowledge_set(cli, namespace, &knowledge_path, content).await?;
        synced_paths.push(knowledge_path);
    }

    let unsynced_existing = existing_paths
        .into_iter()
        .filter(|path| !synced_paths.iter().any(|synced| synced == path))
        .collect::<Vec<_>>();

    Ok((synced_paths.len(), unsynced_existing))
}

#[derive(Debug, PartialEq, Eq)]
struct GrpcResourceTarget {
    ns: String,
    kind: String,
    name: String,
}

fn resource_lookup_target(
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<(String, String, String)> {
    match kind.to_lowercase().as_str() {
        "agent" | "agents" => {
            let (ns, agent_name) = agent_lookup_target(name, namespace);
            Ok((ns, "Agent".to_string(), agent_name))
        }
        "agenttemplate" | "templates" | "template" => Ok((
            namespace
                .cloned()
                .unwrap_or_else(|| crate::control::ns::TALON_SYSTEM.to_string()),
            "Template".to_string(),
            name.to_string(),
        )),
        "mcpserver" | "mcpservers" | "mcp" => Ok((
            crate::control::ns::TALON_SYSTEM.to_string(),
            "McpServer".to_string(),
            name.to_string(),
        )),
        "mcpserverbinding" | "mcpbindings" | "mcpbinding" => {
            let ns = namespace
                .cloned()
                .context("McpServerBinding requires --namespace")?;
            Ok((ns, "McpServerBinding".to_string(), name.to_string()))
        }
        "knowledge" | "knowledgeartifact" | "knowledgeartifacts" => {
            let ns = namespace
                .cloned()
                .context("Knowledge requires --namespace")?;
            Ok((ns, "Knowledge".to_string(), name.to_string()))
        }
        "schedule" | "schedules" => {
            let ns = namespace
                .cloned()
                .context("Schedule requires --namespace")?;
            Ok((ns, "Schedule".to_string(), name.to_string()))
        }
        "channel" | "channels" => {
            let ns = namespace.cloned().context("Channel requires --namespace")?;
            Ok((ns, "Channel".to_string(), name.to_string()))
        }
        "channelsubscription"
        | "channelsubscriptions"
        | "channel-subscription"
        | "channel-subscriptions" => {
            let ns = namespace
                .cloned()
                .context("ChannelSubscription requires --namespace")?;
            let subscription = name
                .split_once('/')
                .map(|(_, subscription)| subscription)
                .unwrap_or(name);
            Ok((
                ns,
                "ChannelSubscription".to_string(),
                subscription.to_string(),
            ))
        }
        "workflow" | "workflows" => {
            let ns = namespace
                .cloned()
                .context("Workflow requires --namespace")?;
            Ok((ns, "Workflow".to_string(), name.to_string()))
        }
        "deployment" | "deployments" => {
            let ns = namespace
                .cloned()
                .context("Deployment requires --namespace")?;
            Ok((ns, "Deployment".to_string(), name.to_string()))
        }
        "sandboxclass" | "sandboxclasses" | "sandbox-class" | "sandbox-classes" => Ok((
            namespace
                .cloned()
                .unwrap_or_else(|| crate::control::ns::TALON_SYSTEM.to_string()),
            "SandboxClass".to_string(),
            name.to_string(),
        )),
        "sandboxpolicy" | "sandboxpolicies" | "sandbox-policy" | "sandbox-policies" => {
            let ns = namespace
                .cloned()
                .context("SandboxPolicy requires --namespace")?;
            Ok((ns, "SandboxPolicy".to_string(), name.to_string()))
        }
        "sandbox" | "sandboxes" => {
            let ns = namespace.cloned().context("Sandbox requires --namespace")?;
            Ok((ns, "Sandbox".to_string(), name.to_string()))
        }
        other => anyhow::bail!("Unsupported resource kind '{}'", other),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ResourceListTarget {
    Resources { ns: String, kind: Option<String> },
    Namespaces { parent: Option<String> },
}

fn resource_list_target(kind: &str, namespace: Option<&String>) -> Result<ResourceListTarget> {
    let ns_or_default = || namespace.cloned().unwrap_or_else(|| "default".to_string());
    let system_ns = || crate::control::ns::TALON_SYSTEM.to_string();
    match kind.to_lowercase().as_str() {
        "resource" | "resources" | "all" => Ok(ResourceListTarget::Resources {
            ns: ns_or_default(),
            kind: None,
        }),
        "namespace" | "namespaces" => Ok(ResourceListTarget::Namespaces {
            parent: namespace.cloned(),
        }),
        "agent" | "agents" => Ok(ResourceListTarget::Resources {
            ns: ns_or_default(),
            kind: Some("Agent".to_string()),
        }),
        "agenttemplate" | "templates" | "template" => Ok(ResourceListTarget::Resources {
            ns: namespace.cloned().unwrap_or_else(system_ns),
            kind: Some("Template".to_string()),
        }),
        "mcpserver" | "mcpservers" | "mcp" => Ok(ResourceListTarget::Resources {
            ns: system_ns(),
            kind: Some("McpServer".to_string()),
        }),
        "mcpserverbinding" | "mcpbindings" | "mcpbinding" => Ok(ResourceListTarget::Resources {
            ns: ns_or_default(),
            kind: Some("McpServerBinding".to_string()),
        }),
        "knowledge" | "knowledgeartifact" | "knowledgeartifacts" => {
            Ok(ResourceListTarget::Resources {
                ns: ns_or_default(),
                kind: Some("Knowledge".to_string()),
            })
        }
        "schedule" | "schedules" => Ok(ResourceListTarget::Resources {
            ns: ns_or_default(),
            kind: Some("Schedule".to_string()),
        }),
        "channel" | "channels" => Ok(ResourceListTarget::Resources {
            ns: ns_or_default(),
            kind: Some("Channel".to_string()),
        }),
        "channelsubscription"
        | "channelsubscriptions"
        | "channel-subscription"
        | "channel-subscriptions" => Ok(ResourceListTarget::Resources {
            ns: ns_or_default(),
            kind: Some("ChannelSubscription".to_string()),
        }),
        "workflow" | "workflows" => Ok(ResourceListTarget::Resources {
            ns: ns_or_default(),
            kind: Some("Workflow".to_string()),
        }),
        "deployment" | "deployments" => Ok(ResourceListTarget::Resources {
            ns: ns_or_default(),
            kind: Some("Deployment".to_string()),
        }),
        "deploymentreplica"
        | "deploymentreplicas"
        | "deployment-replica"
        | "deployment-replicas" => Ok(ResourceListTarget::Resources {
            ns: ns_or_default(),
            kind: Some("DeploymentReplica".to_string()),
        }),
        "sandboxclass" | "sandboxclasses" | "sandbox-class" | "sandbox-classes" => {
            Ok(ResourceListTarget::Resources {
                ns: ns_or_default(),
                kind: Some("SandboxClass".to_string()),
            })
        }
        "sandboxpolicy" | "sandboxpolicies" | "sandbox-policy" | "sandbox-policies" => {
            Ok(ResourceListTarget::Resources {
                ns: ns_or_default(),
                kind: Some("SandboxPolicy".to_string()),
            })
        }
        "sandbox" | "sandboxes" => Ok(ResourceListTarget::Resources {
            ns: ns_or_default(),
            kind: Some("Sandbox".to_string()),
        }),
        other => anyhow::bail!("Unsupported resource kind '{}'", other),
    }
}

pub(super) fn sdk_methods_from_dir(dir: &str) -> Result<Vec<String>> {
    let mut class_methods = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().unwrap_or_default() != "yaml" {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        let raw = parse_raw_manifest(&content)?;
        if raw.kind == "Agent" {
            let agent = crate::control::manifest::parse_agent(&content)?;
            let method_name = format!("create{}", to_camel_case(&agent.name()));
            class_methods.push(format!(
                r#"  async {method_name}(workspaceId: string): Promise<any> {{
    return fetch(`${{this.endpoint}}/api/agents`, {{
      method: "POST",
      headers: {{ "Content-Type": "application/json" }},
      body: JSON.stringify({{ agent: "{raw_name}", namespace: workspaceId, inputs: {{}} }})
    }}).then(r => r.json());
  }}"#,
                method_name = method_name,
                raw_name = agent.name(),
            ));
        }
    }
    Ok(class_methods)
}

fn grpc_get_target(
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<GrpcResourceTarget> {
    let (ns, kind, name) = resource_lookup_target(kind, name, namespace)?;
    Ok(GrpcResourceTarget { ns, kind, name })
}

pub(super) async fn grpc_get_yaml(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
        .connect()
        .await
        .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
    let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);

    let target = grpc_get_target(kind, name, namespace)?;
    let resp = client
        .get_resource(GetResourceRequest {
            ns: target.ns.clone(),
            kind: target.kind.clone(),
            name: target.name.clone(),
        })
        .await
        .with_context(|| {
            format!(
                "Failed to fetch {} '{}/{}'",
                target.kind, target.ns, target.name
            )
        })?;
    let resource = resp.into_inner().resource.context("Resource not found.")?;
    crate::control::manifest::render_resource_yaml(&resource)
}

pub(super) async fn grpc_get_json(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<serde_json::Value> {
    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
        .connect()
        .await
        .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
    let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);

    let target = grpc_get_target(kind, name, namespace)?;
    let resp = client
        .get_resource(GetResourceRequest {
            ns: target.ns.clone(),
            kind: target.kind.clone(),
            name: target.name.clone(),
        })
        .await
        .with_context(|| {
            format!(
                "Failed to fetch {} '{}/{}'",
                target.kind, target.ns, target.name
            )
        })?;
    let resource = resp.into_inner().resource.context("Resource not found.")?;
    resource_manifest_json(&resource)
}

pub(super) async fn grpc_list_resources_table(
    cli: &Cli,
    kind: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
        .connect()
        .await
        .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
    let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);

    match resource_list_target(kind, namespace)? {
        ResourceListTarget::Resources { ns, kind } => {
            let resources = client
                .list_resources(ListResourcesRequest {
                    ns: ns.clone(),
                    kind,
                })
                .await
                .with_context(|| format!("Failed to list resources in '{}'", ns))?
                .into_inner()
                .resources;
            Ok(render_resource_list_table(&resources))
        }
        ResourceListTarget::Namespaces { parent } => {
            let namespaces = client
                .list_namespaces(ListNamespacesRequest { parent })
                .await
                .context("Failed to list namespaces")?
                .into_inner()
                .namespaces;
            Ok(render_namespace_list_table_from_proto(&namespaces))
        }
    }
}

pub(super) async fn grpc_list_resources_json(
    cli: &Cli,
    kind: &str,
    namespace: Option<&String>,
) -> Result<serde_json::Value> {
    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
        .connect()
        .await
        .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
    let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);

    match resource_list_target(kind, namespace)? {
        ResourceListTarget::Resources { ns, kind } => {
            let resources = client
                .list_resources(ListResourcesRequest {
                    ns: ns.clone(),
                    kind,
                })
                .await
                .with_context(|| format!("Failed to list resources in '{}'", ns))?
                .into_inner()
                .resources;
            resources_list_json(resources)
        }
        ResourceListTarget::Namespaces { parent } => {
            let namespaces = client
                .list_namespaces(ListNamespacesRequest { parent })
                .await
                .context("Failed to list namespaces")?
                .into_inner()
                .namespaces;
            Ok(json!({
                "namespaces": namespaces.into_iter().map(|namespace| {
                    json!({
                        "name": namespace.name,
                        "parent": namespace.parent,
                        "isDeleted": namespace.is_deleted,
                        "deletedAt": namespace.deleted_at,
                        "labels": namespace.labels,
                    })
                }).collect::<Vec<_>>()
            }))
        }
    }
}

pub(super) async fn rest_list_resources_table(
    cli: &Cli,
    kind: &str,
    namespace: Option<&String>,
) -> Result<String> {
    match resource_list_target(kind, namespace)? {
        ResourceListTarget::Resources { ns, kind } => {
            let mut path = format!("/v2/ns/{}/resources", urlencoding::encode(&ns));
            if let Some(kind) = kind {
                path.push_str(&format!("?kind={}", urlencoding::encode(&kind)));
            }
            let resp = rest_request_json(cli, reqwest::Method::GET, &path, None)
                .await
                .with_context(|| format!("Failed to list resources in '{}'", ns))?;
            let resources = resp
                .get("resources")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|mut value| {
                    normalize_manifest_metadata_maps(&mut value);
                    serde_json::from_value::<resources_proto::Resource>(value)
                        .context("Failed to decode Resource JSON")
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(render_resource_list_table(&resources))
        }
        ResourceListTarget::Namespaces { parent } => {
            let path = match parent {
                Some(parent) => format!("/v1/namespaces?parent={}", urlencoding::encode(&parent)),
                None => "/v1/namespaces".to_string(),
            };
            let resp = rest_request_json(cli, reqwest::Method::GET, &path, None)
                .await
                .context("Failed to list namespaces")?;
            Ok(render_namespace_list_table_from_json(&resp))
        }
    }
}

pub(super) async fn rest_list_resources_json(
    cli: &Cli,
    kind: &str,
    namespace: Option<&String>,
) -> Result<serde_json::Value> {
    match resource_list_target(kind, namespace)? {
        ResourceListTarget::Resources { ns, kind } => {
            let mut path = format!("/v2/ns/{}/resources", urlencoding::encode(&ns));
            if let Some(kind) = kind {
                path.push_str(&format!("?kind={}", urlencoding::encode(&kind)));
            }
            let resp = rest_request_json(cli, reqwest::Method::GET, &path, None)
                .await
                .with_context(|| format!("Failed to list resources in '{}'", ns))?;
            let resources = resp
                .get("resources")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|mut value| {
                    normalize_manifest_metadata_maps(&mut value);
                    serde_json::from_value::<resources_proto::Resource>(value)
                        .context("Failed to decode Resource JSON")
                })
                .collect::<Result<Vec<_>>>()?;
            resources_list_json(resources)
        }
        ResourceListTarget::Namespaces { parent } => {
            let path = match parent {
                Some(parent) => format!("/v1/namespaces?parent={}", urlencoding::encode(&parent)),
                None => "/v1/namespaces".to_string(),
            };
            rest_request_json(cli, reqwest::Method::GET, &path, None)
                .await
                .context("Failed to list namespaces")
        }
    }
}

fn render_resource_list_table(resources: &[resources_proto::Resource]) -> String {
    let mut rows = vec![vec![
        "KIND".to_string(),
        "NAMESPACE".to_string(),
        "NAME".to_string(),
        "PHASE".to_string(),
    ]];
    for resource in resources {
        let metadata = resource.metadata.as_ref();
        rows.push(vec![
            resource.kind.clone(),
            metadata
                .map(|meta| meta.namespace.clone())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "-".to_string()),
            metadata
                .map(|meta| meta.name.clone())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "-".to_string()),
            resource_status_phase(resource)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "-".to_string()),
        ]);
    }
    render_table(rows)
}

fn render_namespace_list_table_from_proto(
    namespaces: &[crate::gateway::rpc::proto::NamespaceResponse],
) -> String {
    let mut rows = vec![vec![
        "NAME".to_string(),
        "PARENT".to_string(),
        "DELETED".to_string(),
    ]];
    for namespace in namespaces {
        rows.push(vec![
            namespace.name.clone(),
            namespace.parent.clone().unwrap_or_else(|| "-".to_string()),
            namespace.is_deleted.to_string(),
        ]);
    }
    render_table(rows)
}

fn render_namespace_list_table_from_json(value: &serde_json::Value) -> String {
    let mut rows = vec![vec![
        "NAME".to_string(),
        "PARENT".to_string(),
        "DELETED".to_string(),
    ]];
    for namespace in value
        .get("namespaces")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
    {
        rows.push(vec![
            namespace
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("-")
                .to_string(),
            namespace
                .get("parent")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .unwrap_or("-")
                .to_string(),
            namespace
                .get("isDeleted")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
                .to_string(),
        ]);
    }
    render_table(rows)
}

fn resources_list_json(resources: Vec<resources_proto::Resource>) -> Result<serde_json::Value> {
    let resources = resources
        .iter()
        .map(resource_manifest_json)
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({ "resources": resources }))
}

fn resource_manifest_json(resource: &resources_proto::Resource) -> Result<serde_json::Value> {
    let yaml = crate::control::manifest::render_resource_yaml(resource)?;
    let yaml_value: serde_yaml::Value =
        serde_yaml::from_str(&yaml).context("Failed to parse rendered resource YAML")?;
    serde_json::to_value(yaml_value).context("Failed to convert rendered resource YAML to JSON")
}

fn resource_status_phase(resource: &resources_proto::Resource) -> Option<String> {
    use resources_proto::resource_status::Kind as StatusKind;
    match resource.status.as_ref()?.kind.as_ref()? {
        StatusKind::Agent(status) => Some(status.phase.clone()),
        StatusKind::Workflow(status) => Some(status.phase.clone()),
        StatusKind::Schedule(status) => {
            if let Some(error) = &status.last_error {
                if !error.is_empty() {
                    return Some("error".to_string());
                }
            }
            Some(if status.backend_armed {
                "armed".to_string()
            } else {
                "pending".to_string()
            })
        }
        StatusKind::Channel(status) => Some(status.phase.clone()),
        StatusKind::ChannelSubscription(status)
        | StatusKind::McpServer(status)
        | StatusKind::McpServerBinding(status)
        | StatusKind::Knowledge(status)
        | StatusKind::Skill(status)
        | StatusKind::Template(status)
        | StatusKind::SandboxClass(status)
        | StatusKind::SandboxPolicy(status) => Some(status.phase.clone()),
        StatusKind::Namespace(status) => Some(status.phase.clone()),
        StatusKind::Session(status) => Some(status.phase.clone()),
        StatusKind::Deployment(status) => Some(status.phase.clone()),
        StatusKind::DeploymentReplica(status) => Some(status.phase.clone()),
        StatusKind::Sandbox(status) => Some(status.phase.clone()),
        StatusKind::Raw(status) => serde_json::from_str::<serde_json::Value>(&status.json)
            .ok()
            .and_then(|value| {
                value
                    .get("phase")
                    .and_then(|phase| phase.as_str())
                    .map(str::to_string)
            }),
    }
}

fn render_table(rows: Vec<Vec<String>>) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0usize; column_count];
    for row in &rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.len());
        }
    }
    rows.into_iter()
        .map(|row| {
            row.into_iter()
                .enumerate()
                .map(|(index, cell)| {
                    if index + 1 == column_count {
                        cell
                    } else {
                        format!("{cell:<width$}", width = widths[index])
                    }
                })
                .collect::<Vec<_>>()
                .join("  ")
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod get_list_tests {
    use super::*;

    #[test]
    fn resource_list_target_supports_kubectl_style_aliases() {
        let namespace = "customers:acme".to_string();
        assert_eq!(
            resource_list_target("agents", Some(&namespace)).unwrap(),
            ResourceListTarget::Resources {
                ns: "customers:acme".to_string(),
                kind: Some("Agent".to_string()),
            }
        );
        assert_eq!(
            resource_list_target("sandbox-policies", Some(&namespace)).unwrap(),
            ResourceListTarget::Resources {
                ns: "customers:acme".to_string(),
                kind: Some("SandboxPolicy".to_string()),
            }
        );
        assert_eq!(
            resource_list_target("sandboxclasses", Some(&namespace)).unwrap(),
            ResourceListTarget::Resources {
                ns: "customers:acme".to_string(),
                kind: Some("SandboxClass".to_string()),
            }
        );
        assert_eq!(
            resource_list_target("resources", Some(&namespace)).unwrap(),
            ResourceListTarget::Resources {
                ns: "customers:acme".to_string(),
                kind: None,
            }
        );
        assert_eq!(
            resource_list_target("namespaces", Some(&namespace)).unwrap(),
            ResourceListTarget::Namespaces {
                parent: Some("customers:acme".to_string()),
            }
        );
    }

    #[test]
    fn single_template_lookup_honors_explicit_namespace() {
        let namespace = "customers:source".to_string();

        assert_eq!(
            resource_lookup_target("template", "coding-sandbox-policy", Some(&namespace)).unwrap(),
            (
                "customers:source".to_string(),
                "Template".to_string(),
                "coding-sandbox-policy".to_string(),
            )
        );
        assert_eq!(
            rest_delete_path("template", "coding-sandbox-policy", Some(&namespace)).unwrap(),
            "/v2/ns/customers%3Asource/resources/Template/coding-sandbox-policy"
        );
    }

    #[test]
    fn single_sandbox_class_lookup_honors_explicit_namespace() {
        let namespace = "Example".to_string();

        assert_eq!(
            resource_lookup_target("sandboxclass", "docker-codex", Some(&namespace)).unwrap(),
            (
                "Example".to_string(),
                "SandboxClass".to_string(),
                "docker-codex".to_string(),
            )
        );
        assert_eq!(
            rest_delete_path("sandboxclass", "docker-codex", Some(&namespace)).unwrap(),
            "/v2/ns/Example/resources/SandboxClass/docker-codex"
        );
    }

    #[test]
    fn render_resource_list_table_includes_kind_namespace_name_and_phase() {
        let resources = vec![resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Agent".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "coding".to_string(),
                namespace: "customers:acme".to_string(),
                labels: HashMap::new(),
                annotations: HashMap::new(),
                owner_references: Vec::new(),
                finalizers: Vec::new(),
                generation: 1,
                resource_version: "1".to_string(),
                uid: "uid".to_string(),
                deletion_timestamp: None,
            }),
            spec: None,
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resources_proto::resource_status::Kind::Agent(
                    resources_proto::AgentStatus {
                        observed_generation: 1,
                        phase: "Ready".to_string(),
                        conditions: Vec::new(),
                        last_session_id: None,
                    },
                )),
            }),
        }];

        let table = render_resource_list_table(&resources);

        assert!(table.contains("KIND"));
        assert!(table.contains("NAMESPACE"));
        assert!(table.contains("Agent"));
        assert!(table.contains("customers:acme"));
        assert!(table.contains("coding"));
        assert!(table.contains("Ready"));
    }
}

pub(super) async fn grpc_delete_resource(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
        .connect()
        .await
        .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
    let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);

    let (ns, kind, name) = resource_lookup_target(kind, name, namespace)?;
    client
        .delete_resource(DeleteResourceRequest {
            ns: ns.clone(),
            kind: kind.clone(),
            name: name.clone(),
        })
        .await
        .with_context(|| format!("Failed to delete {} '{}/{}'", kind, ns, name))?;
    Ok(format!(
        "✓ {} '{}/{}' deleted successfully.",
        kind, ns, name
    ))
}

pub async fn main() -> Result<()> {
    crate::control::security::install_jwt_crypto_provider();
    let mut cli = Cli::parse();

    if let Ok(env_gateway) = std::env::var("TALON_GATEWAY") {
        cli.gateway = env_gateway;
    }
    if cli.password.is_none() {
        cli.password = resolve_gateway_password(&cli);
    }

    let outcome = commands::run_cli(&cli).await?;
    if let Some(code) = outcome.exit_code {
        std::process::exit(code);
    }

    Ok(())
}

pub(super) fn parse_raw_manifest(content: &str) -> Result<crate::control::manifest::RawManifest> {
    serde_yaml::from_str(content).context("Failed to parse manifest YAML")
}

pub(super) fn render_manifest_file(file: &str, vars: &[String]) -> Result<String> {
    let content = fs::read_to_string(file)
        .with_context(|| format!("Failed to read manifest file: {}", file))?;
    let vars = parse_vars(vars)?;
    let rendered = render_manifest_template(&content, &vars)
        .with_context(|| format!("Failed to render manifest file: {}", file))?;
    resolve_manifest_sources(file, &rendered)
}

fn resolve_manifest_sources(file: &str, rendered: &str) -> Result<String> {
    let raw = parse_raw_manifest(rendered)?;
    if raw.kind != "Knowledge" {
        return Ok(rendered.to_string());
    }

    let mut manifest: serde_yaml::Value =
        serde_yaml::from_str(rendered).context("Failed to parse rendered Knowledge manifest")?;
    let file_manifest: KnowledgeManifestFile = serde_yaml::from_str(rendered)
        .context("Failed to parse Knowledge manifest source directives")?;

    let content = match (
        file_manifest.spec.content.clone(),
        file_manifest.spec.content_from_file.clone(),
    ) {
        (Some(content), None) => content,
        (None, Some(path)) => {
            let base_dir = Path::new(file).parent().unwrap_or_else(|| Path::new("."));
            let full_path = canonicalize_manifest_path(base_dir, &path);
            fs::read_to_string(&full_path).with_context(|| {
                format!(
                    "Failed to read Knowledge contentFromFile '{}'",
                    full_path.display()
                )
            })?
        }
        (Some(_), Some(_)) => {
            anyhow::bail!("Knowledge manifest spec can set only one of content or contentFromFile")
        }
        (None, None) => {
            anyhow::bail!("Knowledge manifest spec must set one of content or contentFromFile")
        }
    };

    if let Some(spec) = manifest
        .get_mut("spec")
        .and_then(|value| value.as_mapping_mut())
    {
        spec.remove(&serde_yaml::Value::String("contentFromFile".to_string()));
        spec.insert(
            serde_yaml::Value::String("content".to_string()),
            serde_yaml::Value::String(content),
        );
    }

    serde_yaml::to_string(&manifest).context("Failed to serialize resolved Knowledge manifest")
}

fn canonicalize_manifest_path(base_dir: &Path, raw_path: &str) -> PathBuf {
    let path = Path::new(raw_path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

fn parse_vars(entries: &[String]) -> Result<HashMap<String, String>> {
    let mut vars = HashMap::new();
    for entry in entries {
        let (key, value) = entry
            .split_once('=')
            .with_context(|| format!("Invalid --var '{}', expected KEY=VALUE", entry))?;
        if key.is_empty() {
            anyhow::bail!("Invalid --var '{}', key cannot be empty", entry);
        }
        vars.insert(key.to_string(), value.to_string());
    }
    Ok(vars)
}

fn render_manifest_template(template: &str, vars: &HashMap<String, String>) -> Result<String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.add_template("manifest", template)
        .context("Failed to compile manifest template")?;
    let rendered = env
        .get_template("manifest")
        .context("Missing manifest template")?
        .render(context! { vars => vars })
        .context("Failed to render manifest template")?;
    Ok(rendered)
}

fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '-' || c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod cli_render_tests {
    use super::*;

    #[test]
    fn render_manifest_template_renders_template_outer_vars_and_preserves_raw_inner_vars() {
        let mut vars = HashMap::new();
        vars.insert("source_ns".to_string(), "customers".to_string());
        let rendered = render_manifest_template(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Template
metadata:
  name: coding-agent
  namespace: "{{ vars.source_ns }}"
spec:
  kind: Agent
  spec:
    systemPrompt: |
      You are the coding agent for {% raw %}{{ namespace.name }}{% endraw %}.
"#,
            &vars,
        )
        .expect("template renders");

        assert!(rendered.contains("namespace: \"customers\""));
        assert!(rendered.contains("{{ namespace.name }}"));
    }

    #[test]
    fn render_manifest_template_fails_on_undefined_vars() {
        let vars = HashMap::new();
        render_manifest_template("name: {{ vars.missing }}", &vars)
            .expect_err("undefined var should fail");
    }
}
