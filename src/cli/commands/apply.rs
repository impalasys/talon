// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;

use crate::control::ns;
use crate::control::resource_model::TypedResource;
use crate::gateway::rpc::proto::gateway_service_client::GatewayServiceClient;
use crate::gateway::rpc::proto::{CreateNamespaceRequest, CreateResourceRequest};
use crate::gateway::rpc::resources_proto;

use super::Cli;
use crate::cli::{auth_interceptor, parse_raw_manifest, render_manifest_file, rest_request_json};

#[derive(clap::Args)]
pub(crate) struct ApplyCommand {
    #[arg(short, long)]
    pub(crate) file: String,
    /// Template variables in KEY=VALUE form.
    #[arg(long = "var", value_name = "KEY=VALUE")]
    pub(crate) vars: Vec<String>,
}

pub(super) async fn run(cli: &Cli, command: &ApplyCommand) -> Result<()> {
    let content = render_manifest_file(&command.file, &command.vars)?;

    if cli.rest {
        rest_ensure_manifest_namespace(cli, &content).await?;
        println!("{}", rest_apply_manifest(cli, &content).await?);
        return Ok(());
    }

    println!("{}", grpc_apply_manifest(cli, &content).await?);
    Ok(())
}

fn namespace_to_ensure(content: &str) -> Result<Option<String>> {
    let raw = parse_raw_manifest(content)?;
    if matches!(raw.kind.as_str(), "Namespace" | "MCPServer" | "McpServer") {
        return Ok(None);
    }
    Ok(raw
        .metadata
        .get("namespace")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|namespace| !namespace.is_empty())
        .map(str::to_string))
}

async fn rest_ensure_manifest_namespace(cli: &Cli, content: &str) -> Result<()> {
    let Some(namespace) = namespace_to_ensure(content)? else {
        return Ok(());
    };
    rest_request_json(
        cli,
        reqwest::Method::POST,
        &format!("/v1/namespaces/{}", urlencoding::encode(&namespace)),
        Some(json!({
            "name": namespace.clone(),
            "recursive": true,
            "labels": {},
        })),
    )
    .await
    .with_context(|| format!("Gateway rejected implicit Namespace '{}'", namespace))?;
    Ok(())
}

fn is_generic_resource_kind(kind: &str) -> bool {
    matches!(
        kind,
        "Agent"
            | "McpServer"
            | "MCPServer"
            | "McpServerBinding"
            | "Knowledge"
            | "Channel"
            | "ChannelSubscription"
            | "Schedule"
            | "Workflow"
            | "Template"
            | "Deployment"
            | "DeploymentReplica"
            | "SandboxClass"
            | "SandboxPolicy"
            | "Sandbox"
    )
}

fn resource_from_manifest(
    content: &str,
) -> Result<(String, String, String, resources_proto::Resource)> {
    use resources_proto::resource_spec::Kind as SpecKind;
    use resources_proto::resource_status::Kind as StatusKind;

    let raw = parse_raw_manifest(content)?;
    let mut resource = match raw.kind.as_str() {
        "MCPServer" | "McpServer" => {
            let mut server = crate::control::manifest::parse_mcp_server(content)?;
            let meta = server
                .metadata
                .as_mut()
                .context("MCPServer missing metadata")?;
            if meta.namespace.is_empty() {
                meta.namespace = ns::TALON_SYSTEM.to_string();
            }
            resources_proto::Resource {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "McpServer".to_string(),
                metadata: server.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::McpServer(
                        server.spec.clone().context("MCPServer missing spec")?,
                    )),
                }),
                status: Some(resources_proto::ResourceStatus {
                    kind: Some(StatusKind::McpServer(
                        resources_proto::CommonResourceStatus::default(),
                    )),
                }),
            }
        }
        "Agent" => {
            let agent = crate::control::manifest::parse_agent(content)?;
            resources_proto::Resource {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "Agent".to_string(),
                metadata: agent.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::Agent(
                        agent.spec.clone().context("Agent missing spec")?,
                    )),
                }),
                status: Some(resources_proto::ResourceStatus {
                    kind: Some(StatusKind::Agent(agent.status.clone().unwrap_or_default())),
                }),
            }
        }
        "McpServerBinding" => {
            let binding = crate::control::manifest::parse_mcp_server_binding(content)?;
            resources_proto::Resource {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "McpServerBinding".to_string(),
                metadata: binding.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::McpServerBinding(
                        binding
                            .spec
                            .clone()
                            .context("McpServerBinding missing spec")?,
                    )),
                }),
                status: Some(resources_proto::ResourceStatus {
                    kind: Some(StatusKind::McpServerBinding(
                        resources_proto::CommonResourceStatus::default(),
                    )),
                }),
            }
        }
        "Knowledge" => {
            let knowledge = crate::control::manifest::parse_knowledge(content)?;
            resources_proto::Resource {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "Knowledge".to_string(),
                metadata: knowledge.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::Knowledge(
                        knowledge.spec.clone().context("Knowledge missing spec")?,
                    )),
                }),
                status: Some(resources_proto::ResourceStatus {
                    kind: Some(StatusKind::Knowledge(
                        resources_proto::CommonResourceStatus::default(),
                    )),
                }),
            }
        }
        "Channel" => {
            let channel = crate::control::manifest::parse_channel(content)?;
            resources_proto::Resource {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "Channel".to_string(),
                metadata: channel.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::Channel(
                        channel.spec.clone().context("Channel missing spec")?,
                    )),
                }),
                status: Some(resources_proto::ResourceStatus {
                    kind: Some(StatusKind::Channel(
                        channel.status.clone().unwrap_or_default(),
                    )),
                }),
            }
        }
        "ChannelSubscription" => {
            let subscription = crate::control::manifest::parse_channel_subscription(content)?;
            resources_proto::Resource {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "ChannelSubscription".to_string(),
                metadata: subscription.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::ChannelSubscription(
                        subscription
                            .spec
                            .clone()
                            .context("ChannelSubscription missing spec")?,
                    )),
                }),
                status: Some(resources_proto::ResourceStatus {
                    kind: Some(StatusKind::ChannelSubscription(
                        resources_proto::CommonResourceStatus::default(),
                    )),
                }),
            }
        }
        "Workflow" => {
            let workflow = crate::control::manifest::parse_workflow(content)?;
            resources_proto::Resource {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "Workflow".to_string(),
                metadata: workflow.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::Workflow(
                        workflow.spec.clone().context("Workflow missing spec")?,
                    )),
                }),
                status: Some(resources_proto::ResourceStatus {
                    kind: Some(StatusKind::Workflow(
                        workflow.status.clone().unwrap_or_default(),
                    )),
                }),
            }
        }
        kind if is_generic_resource_kind(kind) => {
            crate::control::manifest::parse_resource(content)?
        }
        other => anyhow::bail!("Unsupported manifest kind '{}'", other),
    };
    let meta = resource
        .metadata
        .as_mut()
        .context("resource missing metadata")?;
    if meta.namespace.is_empty() {
        meta.namespace = ns::TALON_SYSTEM.to_string();
    }
    Ok((
        meta.namespace.clone(),
        resource.kind.clone(),
        meta.name.clone(),
        resource,
    ))
}

fn resource_proto_json(resource: &resources_proto::Resource) -> serde_json::Value {
    json!({
        "apiVersion": resource.api_version,
        "kind": resource.kind,
        "metadata": resource.metadata.as_ref().map(resource_meta_proto_json),
        "spec": resource.spec.as_ref().map(resource_spec_proto_json),
        "status": resource.status.as_ref().map(resource_status_proto_json),
    })
}

fn resource_meta_proto_json(meta: &resources_proto::ResourceMeta) -> serde_json::Value {
    json!({
        "name": meta.name,
        "namespace": meta.namespace,
        "labels": meta.labels,
        "annotations": meta.annotations,
        "ownerReferences": meta.owner_references.iter().map(owner_reference_proto_json).collect::<Vec<_>>(),
        "finalizers": meta.finalizers,
        "generation": meta.generation,
        "resourceVersion": meta.resource_version,
        "uid": meta.uid,
        "deletionTimestamp": meta.deletion_timestamp,
    })
}

fn owner_reference_proto_json(reference: &resources_proto::OwnerReference) -> serde_json::Value {
    json!({
        "apiVersion": reference.api_version,
        "kind": reference.kind,
        "namespace": reference.namespace,
        "name": reference.name,
        "uid": reference.uid,
        "controller": reference.controller,
        "blockOwnerDeletion": reference.block_owner_deletion,
    })
}

fn resource_ref_proto_json(reference: &resources_proto::ResourceRef) -> serde_json::Value {
    json!({
        "namespace": reference.namespace,
        "name": reference.name,
    })
}

fn resource_spec_proto_json(spec: &resources_proto::ResourceSpec) -> serde_json::Value {
    use resources_proto::resource_spec::Kind;

    match spec.kind.as_ref() {
        Some(Kind::Template(spec)) => json!({
            "template": {
                "kind": spec.kind,
                "metadata": spec.metadata.as_ref().map(resource_meta_proto_json),
                "specJson": spec.spec_json,
            }
        }),
        Some(Kind::Deployment(spec)) => json!({
            "deployment": {
                "placement": spec.placement.as_ref().map(|placement| json!({
                    "namespaceSelector": placement.namespace_selector.as_ref().map(|selector| json!({
                        "parent": selector.parent,
                        "matchLabels": selector.match_labels,
                    })),
                })),
                "templates": spec.templates,
            }
        }),
        Some(Kind::DeploymentReplica(spec)) => json!({
            "deploymentReplica": {
                "deploymentRef": spec.deployment_ref.as_ref().map(resource_ref_proto_json),
                "targetNamespace": spec.target_namespace,
            }
        }),
        Some(Kind::SandboxClass(spec)) => json!({
            "sandboxClass": {
                "provider": spec.provider,
                "providerConfigJson": spec.provider_config_json,
                "credentialsJson": spec.credentials_json,
            }
        }),
        Some(Kind::SandboxPolicy(spec)) => json!({
            "sandboxPolicy": {
                "classRef": spec.class_ref.as_ref().map(resource_ref_proto_json),
                "template": spec.template.as_ref().map(sandbox_runtime_template_proto_json),
                "maxConcurrent": spec.max_concurrent,
            }
        }),
        Some(Kind::Sandbox(spec)) => json!({
            "sandbox": {
                "policyRef": spec.policy_ref,
                "classRef": spec.class_ref.as_ref().map(resource_ref_proto_json),
                "runtimeTemplateJson": spec.runtime_template_json,
            }
        }),
        Some(Kind::PermissionRequest(spec)) => json!({
            "permissionRequest": {
                "agent": spec.agent,
                "sessionId": spec.session_id,
                "action": spec.action,
                "prompt": spec.prompt,
                "payloadJson": spec.payload_json,
            }
        }),
        Some(Kind::Raw(spec)) => json!({ "raw": { "json": spec.json } }),
        Some(Kind::Agent(_))
        | Some(Kind::Workflow(_))
        | Some(Kind::Schedule(_))
        | Some(Kind::Channel(_))
        | Some(Kind::ChannelSubscription(_))
        | Some(Kind::McpServer(_))
        | Some(Kind::McpServerBinding(_))
        | Some(Kind::Knowledge(_))
        | Some(Kind::Namespace(_))
        | Some(Kind::Session(_))
        | None => json!({}),
    }
}

fn sandbox_runtime_template_proto_json(
    spec: &resources_proto::SandboxRuntimeTemplateSpec,
) -> serde_json::Value {
    json!({
        "image": spec.image,
        "workspace": spec.workspace.as_ref().map(|workspace| json!({
            "mode": workspace.mode,
            "mountPath": workspace.mount_path,
        })),
        "setup": spec.setup.as_ref().map(|setup| json!({
            "packages": setup.packages,
            "commands": setup.commands,
        })),
        "network": spec.network.as_ref().map(|network| json!({
            "mode": network.mode,
        })),
        "filesystem": spec.filesystem.as_ref().map(|filesystem| json!({
            "writable": filesystem.writable,
            "readonly": filesystem.readonly,
        })),
        "leasePolicy": spec.lease_policy.as_ref().map(|lease_policy| json!({
            "mode": lease_policy.mode,
        })),
    })
}

fn resource_status_proto_json(status: &resources_proto::ResourceStatus) -> serde_json::Value {
    use resources_proto::resource_status::Kind;

    match status.kind.as_ref() {
        Some(Kind::Template(status)) => json!({
            "template": common_status_proto_json(status),
        }),
        Some(Kind::SandboxClass(status)) => json!({
            "sandboxClass": common_status_proto_json(status),
        }),
        Some(Kind::SandboxPolicy(status)) => json!({
            "sandboxPolicy": common_status_proto_json(status),
        }),
        Some(Kind::Deployment(status)) => json!({
            "deployment": {
                "observedGeneration": status.observed_generation,
                "phase": status.phase,
                "conditions": status.conditions.iter().map(condition_proto_json).collect::<Vec<_>>(),
                "replicas": status.replicas.iter().map(resource_ref_proto_json).collect::<Vec<_>>(),
            }
        }),
        Some(Kind::DeploymentReplica(status)) => json!({
            "deploymentReplica": {
                "renderedResources": status.rendered_resources,
                "renderedHashes": status.rendered_hashes,
                "conflicts": status.conflicts,
                "lastRenderedJson": status.last_rendered_json,
                "ownedJsonPointers": status.owned_json_pointers,
                "phase": status.phase,
            }
        }),
        Some(Kind::Sandbox(status)) => json!({
            "sandbox": {
                "observedGeneration": status.observed_generation,
                "phase": status.phase,
                "conditions": status.conditions.iter().map(condition_proto_json).collect::<Vec<_>>(),
                "backendId": status.backend_id,
                "lease": status.lease.as_ref().map(sandbox_lease_proto_json),
                "processes": status.processes.iter().map(sandbox_process_status_proto_json).collect::<Vec<_>>(),
            }
        }),
        Some(Kind::PermissionRequest(status)) => json!({
            "permissionRequest": {
                "observedGeneration": status.observed_generation,
                "phase": status.phase,
                "conditions": status.conditions.iter().map(condition_proto_json).collect::<Vec<_>>(),
                "decision": status.decision,
                "decidedBy": status.decided_by,
                "decidedAt": status.decided_at,
            }
        }),
        Some(Kind::Raw(status)) => json!({ "raw": { "json": status.json } }),
        Some(Kind::Agent(_))
        | Some(Kind::Workflow(_))
        | Some(Kind::Schedule(_))
        | Some(Kind::Channel(_))
        | Some(Kind::ChannelSubscription(_))
        | Some(Kind::McpServer(_))
        | Some(Kind::McpServerBinding(_))
        | Some(Kind::Knowledge(_))
        | Some(Kind::Namespace(_))
        | Some(Kind::Session(_))
        | None => json!({}),
    }
}

fn common_status_proto_json(status: &resources_proto::CommonResourceStatus) -> serde_json::Value {
    json!({
        "observedGeneration": status.observed_generation,
        "phase": status.phase,
        "conditions": status.conditions.iter().map(condition_proto_json).collect::<Vec<_>>(),
    })
}

fn condition_proto_json(condition: &resources_proto::ResourceCondition) -> serde_json::Value {
    json!({
        "type": condition.r#type,
        "status": condition.status,
        "reason": condition.reason,
        "message": condition.message,
        "lastTransitionTime": condition.last_transition_time,
        "observedGeneration": condition.observed_generation,
    })
}

fn sandbox_lease_proto_json(lease: &resources_proto::SandboxLease) -> serde_json::Value {
    json!({
        "ownerKind": lease.owner_kind,
        "ownerAgent": lease.owner_agent,
        "ownerSessionId": lease.owner_session_id,
        "token": lease.token,
        "acquiredAt": lease.acquired_at,
        "expiresAt": lease.expires_at,
        "heartbeatAt": lease.heartbeat_at,
    })
}

fn sandbox_process_status_proto_json(
    status: &resources_proto::SandboxProcessStatus,
) -> serde_json::Value {
    json!({
        "id": status.id,
        "command": status.command,
        "args": status.args,
        "protocol": status.protocol,
        "phase": status.phase,
    })
}

pub(super) async fn rest_apply_manifest(cli: &Cli, content: &str) -> Result<String> {
    let plan = build_rest_apply_plan(content)?;
    rest_request_json(cli, plan.method, &plan.path, Some(plan.payload))
        .await
        .with_context(|| format!("Gateway rejected {}", plan.success_label))?;
    Ok(format!("✓ {} applied successfully.", plan.success_label))
}

#[derive(Debug)]
struct RestApplyPlan {
    method: reqwest::Method,
    path: String,
    payload: serde_json::Value,
    success_label: String,
}

#[derive(Debug)]
enum GrpcApplyPlan {
    Namespace {
        name: String,
        labels: HashMap<String, String>,
    },
    Resource {
        ns: String,
        kind: String,
        name: String,
        resource: resources_proto::Resource,
    },
}

fn build_rest_apply_plan(content: &str) -> Result<RestApplyPlan> {
    let raw = parse_raw_manifest(content)?;
    if raw.kind == "Namespace" {
        let namespace = crate::control::manifest::parse_namespace(content)?;
        let name = namespace.name();
        return Ok(RestApplyPlan {
            method: reqwest::Method::POST,
            path: format!("/v1/namespaces/{}", urlencoding::encode(&name)),
            payload: json!({
                "name": name,
                "recursive": true,
                "labels": namespace.labels(),
            }),
            success_label: format!("Namespace '{}'", name),
        });
    }
    let (ns, kind, name, resource) = resource_from_manifest(content)?;
    Ok(RestApplyPlan {
        method: reqwest::Method::POST,
        path: format!("/v2/ns/{}/resources", urlencoding::encode(&ns)),
        payload: json!({
            "ns": ns,
            "resource": resource_proto_json(&resource),
        }),
        success_label: format!("{} '{}/{}'", kind, ns, name),
    })
}

fn build_grpc_apply_plan(content: &str) -> Result<GrpcApplyPlan> {
    match parse_raw_manifest(content)?.kind.as_str() {
        "Namespace" => {
            let namespace = crate::control::manifest::parse_namespace(content)?;
            Ok(GrpcApplyPlan::Namespace {
                name: namespace.name().to_string(),
                labels: namespace.labels().clone(),
            })
        }
        _ => {
            let (ns, kind, name, resource) = resource_from_manifest(content)?;
            Ok(GrpcApplyPlan::Resource {
                ns,
                kind,
                name,
                resource,
            })
        }
    }
}

fn grpc_plan_namespace_to_ensure(plan: &GrpcApplyPlan) -> Option<&str> {
    match plan {
        GrpcApplyPlan::Resource { ns, .. } => Some(ns.as_str())
            .filter(|ns| !ns.is_empty())
            .filter(|ns| *ns != crate::control::ns::TALON_SYSTEM),
        GrpcApplyPlan::Namespace { .. } => None,
    }
}

pub(super) async fn grpc_apply_manifest(cli: &Cli, content: &str) -> Result<String> {
    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
        .connect()
        .await
        .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
    let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);
    let plan = build_grpc_apply_plan(content)?;
    if let Some(namespace) = grpc_plan_namespace_to_ensure(&plan) {
        client
            .create_namespace(CreateNamespaceRequest {
                name: namespace.to_string(),
                recursive: true,
                labels: HashMap::new(),
            })
            .await
            .with_context(|| format!("Gateway rejected implicit Namespace '{}'", namespace))?;
    }

    match plan {
        GrpcApplyPlan::Namespace { name, labels } => {
            client
                .create_namespace(CreateNamespaceRequest {
                    name: name.clone(),
                    recursive: true,
                    labels,
                })
                .await
                .with_context(|| format!("Gateway rejected Namespace '{}'", name))?;
            Ok(format!("✓ Namespace '{}' applied successfully.", name))
        }
        GrpcApplyPlan::Resource {
            ns,
            kind,
            name,
            resource,
        } => {
            client
                .create_resource(CreateResourceRequest {
                    ns: ns.clone(),
                    resource: Some(resource),
                })
                .await
                .with_context(|| format!("Gateway rejected {} '{}/{}'", kind, ns, name))?;
            Ok(format!(
                "✓ {} '{}/{}' applied successfully.",
                kind, ns, name
            ))
        }
    }
}
