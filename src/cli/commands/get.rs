// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use serde_json::json;

use super::Cli;
use crate::cli::{connect_gateway, resource_lookup_target, to_internal_resource};
use crate::gateway::rpc::resources_proto;
use talon_client::v1::{GetResourceRequest, ListNamespacesRequest, ListResourcesRequest};

#[derive(clap::Args)]
pub(crate) struct GetCommand {
    /// Type of resource to get: agent, template, mcp-server, worker, knowledge, schedule, channel, channel-subscription
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
                list_resources_table(cli, &command.kind, command.namespace.as_ref()).await?
            }
            GetOutput::Json => {
                let value =
                    list_resources_json(cli, &command.kind, command.namespace.as_ref())
                        .await?;
                serde_json::to_string_pretty(&value)?
            }
            GetOutput::Yaml => anyhow::bail!("list output format 'yaml' is not supported"),
        };
        println!("{}", output);
        return Ok(());
    };

    let output = match command.output.unwrap_or(GetOutput::Yaml) {
        GetOutput::Yaml => {
            get_yaml(cli, &command.kind, name, command.namespace.as_ref()).await?
        }
        GetOutput::Json => {
            let value = get_json(cli, &command.kind, name, command.namespace.as_ref()).await?;
            serde_json::to_string_pretty(&value)?
        }
        GetOutput::Table => anyhow::bail!("single resource output format 'table' is not supported"),
    };
    println!("{}", output);
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct ResourceTarget {
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
            ns: ns_or_default(),
            kind: Some("McpServer".to_string()),
        }),
        "worker" | "workers" => Ok(ResourceListTarget::Resources {
            ns: system_ns(),
            kind: Some("Worker".to_string()),
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
        "usagepolicy" | "usagepolicies" | "usage-policy" | "usage-policies" => {
            Ok(ResourceListTarget::Resources {
                ns: ns_or_default(),
                kind: Some("UsagePolicy".to_string()),
            })
        }
        other => anyhow::bail!("Unsupported resource kind '{}'", other),
    }
}

fn get_target(
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<ResourceTarget> {
    let (ns, kind, name) = resource_lookup_target(kind, name, namespace)?;
    Ok(ResourceTarget { ns, kind, name })
}

async fn get_yaml(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let mut client = connect_gateway(cli).await?;

    let target = get_target(kind, name, namespace)?;
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
    let resource = to_internal_resource(&resource)?;
    crate::control::manifest::render_resource_yaml(&resource)
}

async fn get_json(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<serde_json::Value> {
    let mut client = connect_gateway(cli).await?;

    let target = get_target(kind, name, namespace)?;
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
    let resource = to_internal_resource(&resource)?;
    resource_manifest_json(&resource)
}

async fn list_resources_table(
    cli: &Cli,
    kind: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let mut client = connect_gateway(cli).await?;

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
            let resources = resources
                .iter()
                .map(to_internal_resource)
                .collect::<Result<Vec<_>>>()?;
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

async fn list_resources_json(
    cli: &Cli,
    kind: &str,
    namespace: Option<&String>,
) -> Result<serde_json::Value> {
    let mut client = connect_gateway(cli).await?;

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
            let resources = resources
                .iter()
                .map(to_internal_resource)
                .collect::<Result<Vec<_>>>()?;
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
    namespaces: &[talon_client::v1::NamespaceResponse],
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
        | StatusKind::Knowledge(status)
        | StatusKind::Skill(status)
        | StatusKind::Template(status)
        | StatusKind::SandboxClass(status)
        | StatusKind::SandboxPolicy(status) => Some(status.phase.clone()),
        StatusKind::Worker(status) => Some(status.phase.clone()),
        StatusKind::Namespace(status) => Some(status.phase.clone()),
        StatusKind::Session(status) => Some(status.phase.clone()),
        StatusKind::Deployment(status) => Some(status.phase.clone()),
        StatusKind::DeploymentReplica(status) => Some(status.phase.clone()),
        StatusKind::Sandbox(status) => Some(status.phase.clone()),
        StatusKind::UsagePolicy(status) => Some(status.phase.clone()),
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
