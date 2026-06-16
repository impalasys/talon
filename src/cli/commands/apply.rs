// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;

use crate::control::resource_model::{ChannelSubscriptionResourceExt, TypedResource};
use crate::gateway::rpc::proto::gateway_service_client::GatewayServiceClient;
use crate::gateway::rpc::proto::{
    CreateAgentRequest, CreateChannelRequest, CreateChannelSubscriptionRequest,
    CreateMcpServerRequest, CreateNamespaceKnowledgeRequest, CreateNamespaceRequest,
    CreateResourceRequest, CreateWorkflowRequest, GetChannelRequest, GetChannelSubscriptionRequest,
    ModifyAgentRequest, ModifyChannelRequest, ModifyChannelSubscriptionRequest,
};
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
        let agent_exists = if parse_raw_manifest(&content)?.kind == "Agent" {
            let agent = crate::control::manifest::parse_agent(&content)?;
            let get_path = format!(
                "/v1/ns/{}/agents/{}",
                urlencoding::encode(agent.namespace()),
                urlencoding::encode(agent.name())
            );
            rest_request_json(cli, reqwest::Method::GET, &get_path, None)
                .await
                .is_ok()
        } else {
            false
        };
        println!(
            "{}",
            rest_apply_manifest(cli, &content, agent_exists).await?
        );
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
        "Template"
            | "Deployment"
            | "DeploymentReplica"
            | "SandboxClass"
            | "SandboxPolicy"
            | "Sandbox"
    )
}

fn manifest_json_payload(content: &str) -> Result<(String, serde_json::Value)> {
    let raw = parse_raw_manifest(content)?;
    let manifest_value: serde_yaml::Value =
        serde_yaml::from_str(content).context("Failed to parse rendered manifest")?;
    match raw.kind.as_str() {
        "MCPServer" | "McpServer" => {
            Ok(("server".to_string(), json!({ "server": manifest_value })))
        }
        "Agent" => {
            let agent = crate::control::manifest::parse_agent(content)?;
            let spec = manifest_value
                .get("spec")
                .cloned()
                .context("Agent manifest missing spec")?;
            Ok((
                "agent".to_string(),
                json!({
                    "ns": agent.namespace(),
                    "name": agent.name(),
                    "labels": agent.labels(),
                    "spec": spec,
                }),
            ))
        }
        "McpServerBinding" => {
            let binding = crate::control::manifest::parse_mcp_server_binding(content)?;
            let namespace = binding
                .metadata
                .as_ref()
                .map(|meta| meta.namespace.clone())
                .filter(|namespace| !namespace.is_empty())
                .context("McpServerBinding missing metadata.namespace")?;
            Ok((
                "binding".to_string(),
                json!({
                    "ns": namespace,
                    "binding": binding,
                }),
            ))
        }
        "Namespace" => {
            let namespace = crate::control::manifest::parse_namespace(content)?;
            Ok((
                "namespace".to_string(),
                json!({
                    "name": namespace.name(),
                    "recursive": true,
                    "labels": namespace.labels(),
                }),
            ))
        }
        "Knowledge" => Ok((
            "knowledge".to_string(),
            json!({ "knowledge": manifest_value }),
        )),
        "Channel" => {
            let channel = crate::control::manifest::parse_channel(content)?;
            Ok((
                "channel".to_string(),
                json!({
                    "ns": channel.namespace(),
                    "channel": channel,
                }),
            ))
        }
        "ChannelSubscription" => {
            let subscription = crate::control::manifest::parse_channel_subscription(content)?;
            Ok((
                "subscription".to_string(),
                json!({
                    "ns": subscription.namespace(),
                    "channel": subscription.channel(),
                    "subscription": subscription,
                }),
            ))
        }
        "Workflow" => {
            let workflow = crate::control::manifest::parse_workflow(content)?;
            Ok((
                "workflow".to_string(),
                json!({
                    "ns": workflow.namespace(),
                    "workflow": workflow,
                }),
            ))
        }
        kind if is_generic_resource_kind(kind) => {
            let resource = crate::control::manifest::parse_generic_resource(content)?;
            let meta = resource
                .metadata
                .as_ref()
                .context("generic resource missing metadata")?;
            if meta.namespace.is_empty() {
                anyhow::bail!("{} metadata.namespace is required", resource.kind);
            }
            Ok((
                "resource".to_string(),
                json!({
                    "ns": meta.namespace.clone(),
                    "resource": resource_proto_json(&resource),
                }),
            ))
        }
        other => anyhow::bail!("Unsupported manifest kind '{}'", other),
    }
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

pub(super) async fn rest_apply_manifest(
    cli: &Cli,
    content: &str,
    agent_exists: bool,
) -> Result<String> {
    let (_, payload) = manifest_json_payload(content)?;
    let plan = build_rest_apply_plan(content, payload, agent_exists)?;
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
    Agent {
        ns: String,
        name: String,
        labels: HashMap<String, String>,
        spec: crate::gateway::rpc::manifests::AgentSpec,
    },
    McpServer {
        name: String,
        server: crate::gateway::rpc::manifests::McpServer,
    },
    Knowledge {
        ns: String,
        name: String,
        knowledge: crate::gateway::rpc::manifests::Knowledge,
    },
    Channel {
        ns: String,
        name: String,
        channel: resources_proto::Channel,
    },
    ChannelSubscription {
        ns: String,
        channel_name: String,
        name: String,
        subscription: resources_proto::ChannelSubscription,
    },
    Workflow {
        ns: String,
        name: String,
        workflow: resources_proto::Workflow,
    },
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

fn build_rest_apply_plan(
    content: &str,
    payload: serde_json::Value,
    agent_exists: bool,
) -> Result<RestApplyPlan> {
    let raw = parse_raw_manifest(content)?;
    match raw.kind.as_str() {
        "MCPServer" | "McpServer" => {
            let server = crate::control::manifest::parse_mcp_server(content)?;
            let meta = server
                .metadata
                .as_ref()
                .context("MCPServer missing metadata")?;
            if !meta.namespace.is_empty() {
                anyhow::bail!(
                    "MCPServer metadata.namespace is not supported; MCP servers are system resources in Sys"
                );
            }
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: "/v1/mcp-servers".to_string(),
                payload,
                success_label: format!("MCPServer '{}'", meta.name),
            })
        }
        "Agent" => {
            let agent = crate::control::manifest::parse_agent(content)?;
            let spec = payload
                .get("spec")
                .cloned()
                .context("Agent payload missing spec")?;
            let labels = payload
                .get("labels")
                .cloned()
                .context("Agent payload missing labels")?;
            let path = if agent_exists {
                format!(
                    "/v1/ns/{}/agents/{}",
                    urlencoding::encode(agent.namespace()),
                    urlencoding::encode(agent.name())
                )
            } else {
                format!("/v1/ns/{}/agents", urlencoding::encode(agent.namespace()))
            };
            let payload = if agent_exists {
                json!({
                    "ns": agent.namespace(),
                    "agent": agent.name(),
                    "labels": labels,
                    "spec": spec,
                })
            } else {
                json!({
                    "ns": agent.namespace(),
                    "name": agent.name(),
                    "labels": labels,
                    "spec": spec,
                })
            };
            Ok(RestApplyPlan {
                method: if agent_exists {
                    reqwest::Method::PUT
                } else {
                    reqwest::Method::POST
                },
                path,
                payload,
                success_label: format!("Agent '{}/{}'", agent.namespace(), agent.name()),
            })
        }
        "McpServerBinding" => {
            let binding = crate::control::manifest::parse_mcp_server_binding(content)?;
            let meta = binding
                .metadata
                .as_ref()
                .context("McpServerBinding missing metadata")?;
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: format!(
                    "/v1/namespaces/{}/mcp-bindings",
                    urlencoding::encode(&meta.namespace)
                ),
                payload: json!({ "ns": meta.namespace, "binding": binding }),
                success_label: format!("McpServerBinding '{}/{}'", meta.namespace, meta.name),
            })
        }
        "Namespace" => {
            let namespace = crate::control::manifest::parse_namespace(content)?;
            let name = namespace.name();
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: format!("/v1/namespaces/{}", urlencoding::encode(&name)),
                payload: json!({
                    "name": name,
                    "recursive": true,
                    "labels": namespace.labels(),
                }),
                success_label: format!("Namespace '{}'", name),
            })
        }
        "Knowledge" => {
            let knowledge = crate::control::manifest::parse_knowledge(content)?;
            let meta = knowledge
                .metadata
                .as_ref()
                .context("Knowledge missing metadata")?;
            if meta.namespace.is_empty() {
                anyhow::bail!("Knowledge metadata.namespace is required");
            }
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: format!(
                    "/v1/namespaces/{}/knowledge",
                    urlencoding::encode(&meta.namespace)
                ),
                payload,
                success_label: format!("Knowledge '{}/{}'", meta.namespace, meta.name),
            })
        }
        "Channel" => {
            let channel = crate::control::manifest::parse_channel(content)?;
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: format!(
                    "/v1/ns/{}/channels",
                    urlencoding::encode(channel.namespace())
                ),
                payload: json!({ "ns": channel.namespace(), "channel": channel }),
                success_label: "Channel".to_string(),
            })
        }
        "ChannelSubscription" => {
            let subscription = crate::control::manifest::parse_channel_subscription(content)?;
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: format!(
                    "/v1/ns/{}/channels/{}/subscriptions",
                    urlencoding::encode(subscription.namespace()),
                    urlencoding::encode(subscription.channel())
                ),
                payload: json!({
                    "ns": subscription.namespace(),
                    "channel": subscription.channel(),
                    "subscription": subscription,
                }),
                success_label: "ChannelSubscription".to_string(),
            })
        }
        "Workflow" => {
            let workflow = crate::control::manifest::parse_workflow(content)?;
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: format!(
                    "/v1/ns/{}/workflows",
                    urlencoding::encode(workflow.namespace())
                ),
                payload: json!({ "ns": workflow.namespace(), "workflow": workflow }),
                success_label: "Workflow".to_string(),
            })
        }
        kind if is_generic_resource_kind(kind) => {
            let resource = crate::control::manifest::parse_generic_resource(content)?;
            let meta = resource
                .metadata
                .as_ref()
                .context("generic resource missing metadata")?;
            if meta.namespace.is_empty() {
                anyhow::bail!("{} metadata.namespace is required", resource.kind);
            }
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: format!("/v2/ns/{}/resources", urlencoding::encode(&meta.namespace)),
                payload,
                success_label: format!("{} '{}/{}'", resource.kind, meta.namespace, meta.name),
            })
        }
        other => anyhow::bail!("Unsupported manifest kind '{}'", other),
    }
}

fn build_grpc_apply_plan(content: &str) -> Result<GrpcApplyPlan> {
    match parse_raw_manifest(content)?.kind.as_str() {
        "Agent" => {
            let agent = crate::control::manifest::parse_agent(content)?;
            let spec = agent.spec.clone().context("Agent spec must be provided")?;
            Ok(GrpcApplyPlan::Agent {
                ns: agent.namespace().to_string(),
                name: agent.name().to_string(),
                labels: agent.labels().clone(),
                spec,
            })
        }
        "MCPServer" | "McpServer" => {
            let server = crate::control::manifest::parse_mcp_server(content)?;
            let meta = server
                .metadata
                .as_ref()
                .context("MCPServer missing metadata")?;
            if !meta.namespace.is_empty() {
                anyhow::bail!(
                    "MCPServer metadata.namespace is not supported; MCP servers are system resources in Sys"
                );
            }
            Ok(GrpcApplyPlan::McpServer {
                name: meta.name.clone(),
                server,
            })
        }
        "Knowledge" => {
            let knowledge = crate::control::manifest::parse_knowledge(content)?;
            let meta = knowledge
                .metadata
                .as_ref()
                .context("Knowledge missing metadata")?;
            if meta.namespace.is_empty() {
                anyhow::bail!("Knowledge metadata.namespace is required");
            }
            Ok(GrpcApplyPlan::Knowledge {
                ns: meta.namespace.clone(),
                name: meta.name.clone(),
                knowledge,
            })
        }
        "Channel" => {
            let channel = crate::control::manifest::parse_channel(content)?;
            Ok(GrpcApplyPlan::Channel {
                ns: channel.namespace().to_string(),
                name: channel.name().to_string(),
                channel,
            })
        }
        "ChannelSubscription" => {
            let subscription = crate::control::manifest::parse_channel_subscription(content)?;
            Ok(GrpcApplyPlan::ChannelSubscription {
                ns: subscription.namespace().to_string(),
                channel_name: subscription.channel().to_string(),
                name: subscription.name().to_string(),
                subscription,
            })
        }
        "Workflow" => {
            let workflow = crate::control::manifest::parse_workflow(content)?;
            Ok(GrpcApplyPlan::Workflow {
                ns: workflow.namespace().to_string(),
                name: workflow.name().to_string(),
                workflow,
            })
        }
        "Namespace" => {
            let namespace = crate::control::manifest::parse_namespace(content)?;
            Ok(GrpcApplyPlan::Namespace {
                name: namespace.name().to_string(),
                labels: namespace.labels().clone(),
            })
        }
        kind if is_generic_resource_kind(kind) => {
            let resource = crate::control::manifest::parse_generic_resource(content)?;
            let meta = resource
                .metadata
                .as_ref()
                .context("generic resource missing metadata")?;
            if meta.namespace.is_empty() {
                anyhow::bail!("{} metadata.namespace is required", resource.kind);
            }
            Ok(GrpcApplyPlan::Resource {
                ns: meta.namespace.clone(),
                kind: resource.kind.clone(),
                name: meta.name.clone(),
                resource,
            })
        }
        other => anyhow::bail!("Unsupported manifest kind '{}'", other),
    }
}

fn grpc_plan_namespace_to_ensure(plan: &GrpcApplyPlan) -> Option<&str> {
    match plan {
        GrpcApplyPlan::Agent { ns, .. }
        | GrpcApplyPlan::Knowledge { ns, .. }
        | GrpcApplyPlan::Channel { ns, .. }
        | GrpcApplyPlan::ChannelSubscription { ns, .. }
        | GrpcApplyPlan::Workflow { ns, .. }
        | GrpcApplyPlan::Resource { ns, .. } => Some(ns.as_str()).filter(|ns| !ns.is_empty()),
        GrpcApplyPlan::McpServer { .. } | GrpcApplyPlan::Namespace { .. } => None,
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
        GrpcApplyPlan::Agent {
            ns,
            name,
            labels,
            spec,
        } => {
            let existing = client
                .get_agent(crate::gateway::rpc::proto::GetAgentRequest {
                    ns: ns.clone(),
                    name: name.clone(),
                })
                .await;

            match existing {
                Ok(_) => {
                    client
                        .modify_agent(ModifyAgentRequest {
                            ns: ns.clone(),
                            agent: name.clone(),
                            spec: Some(spec),
                            labels,
                        })
                        .await
                        .with_context(|| format!("Gateway rejected Agent '{}/{}'", ns, name))?;
                }
                Err(status) if status.code() == tonic::Code::NotFound => {
                    client
                        .create_agent(CreateAgentRequest {
                            ns: ns.clone(),
                            name: Some(name.clone()),
                            spec: Some(spec),
                            labels,
                        })
                        .await
                        .with_context(|| format!("Gateway rejected Agent '{}/{}'", ns, name))?;
                }
                Err(status) => return Err(status.into()),
            }
            Ok(format!("✓ Agent '{}/{}' applied successfully.", ns, name))
        }
        GrpcApplyPlan::McpServer { name, server } => {
            client
                .create_mcp_server(CreateMcpServerRequest {
                    server: Some(server),
                })
                .await
                .context("Gateway rejected MCPServer")?;
            Ok(format!("✓ MCPServer '{}' applied successfully.", name))
        }
        GrpcApplyPlan::Knowledge {
            ns,
            name,
            knowledge,
        } => {
            client
                .create_namespace_knowledge(CreateNamespaceKnowledgeRequest {
                    ns: ns.clone(),
                    knowledge: Some(knowledge),
                })
                .await
                .with_context(|| format!("Gateway rejected Knowledge '{}/{}'", ns, name))?;
            Ok(format!(
                "✓ Knowledge '{}/{}' applied successfully.",
                ns, name
            ))
        }
        GrpcApplyPlan::Channel { ns, name, channel } => {
            let existing = client
                .get_channel(GetChannelRequest {
                    ns: ns.clone(),
                    name: name.clone(),
                })
                .await;
            match existing {
                Ok(_) => {
                    client
                        .modify_channel(ModifyChannelRequest {
                            ns: ns.clone(),
                            name: name.clone(),
                            channel: Some(channel),
                        })
                        .await
                        .with_context(|| format!("Gateway rejected Channel '{}/{}'", ns, name))?;
                }
                Err(status) if status.code() == tonic::Code::NotFound => {
                    client
                        .create_channel(CreateChannelRequest {
                            ns: ns.clone(),
                            channel: Some(channel),
                        })
                        .await
                        .with_context(|| format!("Gateway rejected Channel '{}/{}'", ns, name))?;
                }
                Err(status) => return Err(status.into()),
            }
            Ok(format!("✓ Channel '{}/{}' applied successfully.", ns, name))
        }
        GrpcApplyPlan::ChannelSubscription {
            ns,
            channel_name,
            name,
            subscription,
        } => {
            let existing = client
                .get_channel_subscription(GetChannelSubscriptionRequest {
                    ns: ns.clone(),
                    channel: channel_name.clone(),
                    name: name.clone(),
                })
                .await;
            match existing {
                Ok(_) => {
                    client
                        .modify_channel_subscription(ModifyChannelSubscriptionRequest {
                            ns: ns.clone(),
                            channel: channel_name.clone(),
                            name: name.clone(),
                            subscription: Some(subscription),
                        })
                        .await
                        .with_context(|| {
                            format!(
                                "Gateway rejected ChannelSubscription '{}/{}/{}'",
                                ns, channel_name, name
                            )
                        })?;
                }
                Err(status) if status.code() == tonic::Code::NotFound => {
                    client
                        .create_channel_subscription(CreateChannelSubscriptionRequest {
                            ns: ns.clone(),
                            channel: channel_name.clone(),
                            subscription: Some(subscription),
                        })
                        .await
                        .with_context(|| {
                            format!(
                                "Gateway rejected ChannelSubscription '{}/{}/{}'",
                                ns, channel_name, name
                            )
                        })?;
                }
                Err(status) => return Err(status.into()),
            }
            Ok(format!(
                "✓ ChannelSubscription '{}/{}/{}' applied successfully.",
                ns, channel_name, name
            ))
        }
        GrpcApplyPlan::Workflow { ns, name, workflow } => {
            client
                .create_workflow(CreateWorkflowRequest {
                    ns: ns.clone(),
                    workflow: Some(workflow),
                })
                .await
                .with_context(|| format!("Gateway rejected Workflow '{}/{}'", ns, name))?;
            Ok(format!(
                "✓ Workflow '{}/{}' applied successfully.",
                ns, name
            ))
        }
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
