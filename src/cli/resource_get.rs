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
