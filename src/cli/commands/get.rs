// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use serde_json::json;

use super::Cli;
use crate::cli::{
    agent_lookup_target, auth_interceptor, resource_lookup_target, rest_request_json,
};
use crate::gateway::rpc::proto::gateway_service_client::GatewayServiceClient;
use crate::gateway::rpc::proto::{GetResourceRequest, ListNamespacesRequest, ListResourcesRequest};
use crate::gateway::rpc::resources_proto;

#[derive(clap::Args)]
pub(crate) struct GetCommand {
    /// Type of resource to get: agent, template, mcp-server, knowledge, schedule, channel, channel-subscription
    #[arg(value_name = "KIND")]
    pub(crate) kind: String,
    /// Name of the resource. Omit to list resources of this kind.
    ///
    /// Channel subscriptions use '<channel>/<subscription>'.
    #[arg(value_name = "NAME")]
    pub(crate) name: Option<String>,
    /// Namespace of the resource
    #[arg(short, long)]
    pub(crate) namespace: Option<String>,
    /// Output format. Defaults to table for lists and yaml for single resources.
    #[arg(short, long, value_enum)]
    pub(crate) output: Option<GetOutput>,
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum GetOutput {
    Table,
    Yaml,
    Json,
}

pub(super) async fn run(cli: &Cli, command: &GetCommand) -> Result<()> {
    let Some(name) = command.name.as_ref() else {
        let output = match command.output.unwrap_or(GetOutput::Table) {
            GetOutput::Table => {
                if cli.rest {
                    rest_list_resources_table(cli, &command.kind, command.namespace.as_ref())
                        .await?
                } else {
                    grpc_list_resources_table(cli, &command.kind, command.namespace.as_ref())
                        .await?
                }
            }
            GetOutput::Json => {
                let value = if cli.rest {
                    rest_list_resources_json(cli, &command.kind, command.namespace.as_ref()).await?
                } else {
                    grpc_list_resources_json(cli, &command.kind, command.namespace.as_ref()).await?
                };
                serde_json::to_string_pretty(&value)?
            }
            GetOutput::Yaml => anyhow::bail!("list output format 'yaml' is not supported"),
        };
        println!("{}", output);
        return Ok(());
    };

    let output = match command.output.unwrap_or(GetOutput::Yaml) {
        GetOutput::Yaml => {
            if cli.rest {
                rest_get_yaml(cli, &command.kind, name, command.namespace.as_ref()).await?
            } else {
                grpc_get_yaml(cli, &command.kind, name, command.namespace.as_ref()).await?
            }
        }
        GetOutput::Json => {
            let value = if cli.rest {
                rest_get_json(cli, &command.kind, name, command.namespace.as_ref()).await?
            } else {
                grpc_get_json(cli, &command.kind, name, command.namespace.as_ref()).await?
            };
            serde_json::to_string_pretty(&value)?
        }
        GetOutput::Table => anyhow::bail!("single resource output format 'table' is not supported"),
    };
    println!("{}", output);
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct GrpcResourceTarget {
    ns: String,
    kind: String,
    name: String,
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
                    "/v1/ns/{}/resources/Template/{}",
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
            let ns = namespace.as_ref().context("Knowledge get requires --namespace")?;
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
            let ns = namespace.as_ref().context("Schedule get requires --namespace")?;
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
            let ns = namespace.as_ref().context("Channel get requires --namespace")?;
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
            let ns = namespace.as_ref().context("Workflow get requires --namespace")?;
            Ok((
                format!(
                    "/v1/ns/{}/workflows/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "workflow",
            ))
        }
        "deployment"
        | "deployments"
        | "deploymentreplica"
        | "deploymentreplicas"
        | "deployment-replica"
        | "deployment-replicas"
        | "sandboxclass"
        | "sandboxclasses"
        | "sandbox-class"
        | "sandbox-classes"
        | "sandboxpolicy"
        | "sandboxpolicies"
        | "sandbox-policy"
        | "sandbox-policies"
        | "sandbox"
        | "sandboxes" => rest_get_resource_path(kind, name, namespace),
        other => anyhow::bail!("Unsupported resource kind '{}' for REST mode", other),
    }
}

fn rest_get_resource_path(
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<(String, &'static str)> {
    let (ns, kind, name) = resource_lookup_target(kind, name, namespace)?;
    Ok((
        format!(
            "/v1/ns/{}/resources/{}/{}",
            urlencoding::encode(&ns),
            urlencoding::encode(&kind),
            urlencoding::encode(&name)
        ),
        "resource",
    ))
}

fn grpc_get_target(
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<GrpcResourceTarget> {
    let (ns, kind, name) = resource_lookup_target(kind, name, namespace)?;
    Ok(GrpcResourceTarget { ns, kind, name })
}

async fn grpc_get_yaml(
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

async fn grpc_get_json(
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

async fn rest_get_yaml(
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

async fn rest_get_json(
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
            let resource: resources_proto::Resource =
                serde_json::from_value(value).context("Failed to decode Resource JSON")?;
            resource_manifest_json(&resource)
        }
        _ => Ok(value),
    }
}

async fn grpc_list_resources_table(
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

async fn grpc_list_resources_json(
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

async fn rest_list_resources_table(
    cli: &Cli,
    kind: &str,
    namespace: Option<&String>,
) -> Result<String> {
    match resource_list_target(kind, namespace)? {
        ResourceListTarget::Resources { ns, kind } => {
            let mut path = format!("/v1/ns/{}/resources", urlencoding::encode(&ns));
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

async fn rest_list_resources_json(
    cli: &Cli,
    kind: &str,
    namespace: Option<&String>,
) -> Result<serde_json::Value> {
    match resource_list_target(kind, namespace)? {
        ResourceListTarget::Resources { ns, kind } => {
            let mut path = format!("/v1/ns/{}/resources", urlencoding::encode(&ns));
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

fn render_rest_get_yaml(response_key: &str, value: serde_json::Value) -> Result<String> {
    match response_key {
        "resource" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let resource: resources_proto::Resource =
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
    let metadata = agent.get("metadata");
    let name = agent
        .get("name")
        .or_else(|| agent.get("agent"))
        .or_else(|| metadata.and_then(|metadata| metadata.get("name")))
        .and_then(|name| name.as_str())
        .context("Agent response missing name")?;
    let namespace = agent
        .get("ns")
        .or_else(|| metadata.and_then(|metadata| metadata.get("namespace")))
        .and_then(|namespace| namespace.as_str())
        .context("Agent response missing ns")?;
    let spec = agent
        .get("spec")
        .cloned()
        .context("Agent response missing spec")?;
    let labels = agent
        .get("labels")
        .or_else(|| metadata.and_then(|metadata| metadata.get("labels")))
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
mod tests {
    use super::*;
    use std::collections::HashMap;

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
    fn rest_get_path_supports_generic_resource_kinds() {
        let namespace = "customers:acme".to_string();
        assert_eq!(
            rest_get_path("deployment", "conic-cmo", Some(&namespace)).unwrap(),
            (
                "/v1/ns/customers%3Aacme/resources/Deployment/conic-cmo".to_string(),
                "resource"
            )
        );
        assert_eq!(
            rest_get_path("sandbox-class", "docker-codex", Some(&namespace)).unwrap(),
            (
                "/v1/ns/customers%3Aacme/resources/SandboxClass/docker-codex".to_string(),
                "resource"
            )
        );
    }

    #[test]
    fn render_rest_agent_yaml_accepts_resource_metadata_shape() {
        let yaml = render_rest_agent_yaml(json!({
            "metadata": {
                "name": "ctl",
                "namespace": "conic",
                "labels": {
                    "app": "talon",
                },
            },
            "spec": {
                "template": "conic-cmo",
            },
            "status": {
                "phase": "Ready",
            },
        }))
        .unwrap();

        assert!(yaml.contains("kind: Agent"));
        assert!(yaml.contains("name: ctl"));
        assert!(yaml.contains("namespace: conic"));
        assert!(yaml.contains("template: conic-cmo"));
        assert!(yaml.contains("app: talon"));
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
