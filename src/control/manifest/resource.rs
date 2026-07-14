// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub fn parse_mcp_server(yaml: &str) -> Result<manifests::McpServer> {
    let server: McpServerManifest =
        serde_yaml::from_str(yaml).context("Failed to parse MCPServer YAML")?;

    if server.kind != "McpServer" {
        bail!("Expected kind 'McpServer', got '{}'", server.kind);
    }
    if server.metadata.namespace.trim().is_empty() {
        bail!("McpServer metadata.namespace is required");
    }

    Ok(manifests::McpServer {
        metadata: Some(server.metadata.into_proto()),
        spec: Some(server.spec.into_proto()),
        status: Some(resource_model::common_status(String::new())),
    })
}

pub fn parse_agent(yaml: &str) -> Result<resources_proto::Agent> {
    let agent: AgentManifest = serde_yaml::from_str(yaml).context("Failed to parse Agent YAML")?;

    if agent.kind != "Agent" {
        bail!("Expected kind 'Agent', got '{}'", agent.kind);
    }
    if agent.metadata.namespace.trim().is_empty() {
        bail!("Agent metadata.namespace is required");
    }

    Ok(resource_model::agent(
        agent.metadata.namespace,
        agent.metadata.name,
        agent.spec.into_proto()?,
        agent.metadata.labels,
    ))
}

pub fn parse_namespace(yaml: &str) -> Result<resources_proto::Namespace> {
    let namespace: NamespaceManifest =
        serde_yaml::from_str(yaml).context("Failed to parse Namespace YAML")?;

    if namespace.kind != "Namespace" {
        bail!("Expected kind 'Namespace', got '{}'", namespace.kind);
    }
    if !namespace.metadata.namespace.trim().is_empty() {
        bail!("Namespace metadata.namespace must be empty");
    }

    Ok(resource_model::namespace(
        namespace.metadata.name,
        String::new(),
        namespace.metadata.labels,
    ))
}

pub fn parse_knowledge(yaml: &str) -> Result<manifests::Knowledge> {
    let knowledge: KnowledgeManifest =
        serde_yaml::from_str(yaml).context("Failed to parse Knowledge YAML")?;

    if knowledge.kind != "Knowledge" {
        bail!("Expected kind 'Knowledge', got '{}'", knowledge.kind);
    }

    Ok(manifests::Knowledge {
        metadata: Some(knowledge.metadata.into_proto()),
        spec: Some(manifests::KnowledgeSpec {
            path: knowledge.spec.path,
            content: knowledge.spec.content,
        }),
        status: Some(resource_model::common_status(String::new())),
    })
}

pub fn parse_channel(yaml: &str) -> Result<resources_proto::Channel> {
    let channel: ChannelManifest =
        serde_yaml::from_str(yaml).context("Failed to parse Channel YAML")?;

    if channel.kind != "Channel" {
        bail!("Expected kind 'Channel', got '{}'", channel.kind);
    }
    if channel.metadata.namespace.trim().is_empty() {
        bail!("Channel metadata.namespace is required");
    }

    Ok(resource_model::channel(
        channel.metadata.namespace,
        channel.metadata.name,
        resources_proto::ChannelSpec {
            title: channel.spec.title,
            metadata: channel.spec.metadata,
        },
        resources_proto::ChannelStatus {
            observed_generation: 0,
            phase: if channel.spec.status.is_empty() {
                "open".to_string()
            } else {
                channel.spec.status
            },
            conditions: Vec::new(),
            created_at: 0,
            updated_at: 0,
        },
        channel.metadata.labels,
    ))
}

pub fn parse_channel_subscription(yaml: &str) -> Result<resources_proto::ChannelSubscription> {
    let subscription: ChannelSubscriptionManifest =
        serde_yaml::from_str(yaml).context("Failed to parse ChannelSubscription YAML")?;

    if subscription.kind != "ChannelSubscription" {
        bail!(
            "Expected kind 'ChannelSubscription', got '{}'",
            subscription.kind
        );
    }
    if subscription.metadata.namespace.trim().is_empty() {
        bail!("ChannelSubscription metadata.namespace is required");
    }

    Ok(resource_model::channel_subscription(
        subscription.metadata.namespace,
        subscription.metadata.name,
        resources_proto::ChannelSubscriptionSpec {
            channel: subscription.spec.channel,
            agent: subscription.spec.agent,
            enabled: subscription.spec.enabled,
            trigger: subscription.spec.trigger,
            context_policy: subscription.spec.context_policy.map(|policy| {
                resources_proto::ChannelContextPolicy {
                    mode: policy.mode,
                    max_messages: policy.max_messages,
                }
            }),
            reply_mode: subscription.spec.reply_mode,
            metadata: subscription.spec.metadata,
        },
        subscription.metadata.labels,
    ))
}

pub fn parse_workflow(yaml: &str) -> Result<resources_proto::Workflow> {
    let workflow: WorkflowManifest =
        serde_yaml::from_str(yaml).context("Failed to parse Workflow YAML")?;

    if workflow.kind != "Workflow" {
        bail!("Expected kind 'Workflow', got '{}'", workflow.kind);
    }
    if workflow.metadata.namespace.trim().is_empty() {
        bail!("Workflow metadata.namespace is required");
    }

    let workflow = resource_model::workflow(
        workflow.metadata.namespace,
        workflow.metadata.name,
        workflow.spec.into_proto()?,
        workflow.metadata.labels,
    );
    crate::worker::workflows::validate_workflow(&workflow)?;
    Ok(workflow)
}

pub fn parse_resource(yaml: &str) -> Result<resources_proto::Resource> {
    let manifest: ResourceYamlDocument =
        serde_yaml::from_str(yaml).context("Failed to parse resource YAML")?;
    let metadata = manifest.metadata.into_resource_meta();
    validate_mcp_server_namespace(&manifest.kind, &metadata)?;
    let spec_json = non_empty_json_object(yaml_value_to_json_string(manifest.spec)?);
    let status_json = non_empty_json_object(yaml_value_to_json_string(manifest.status)?);
    let (spec, status) = resource_spec_status_from_json(&manifest.kind, &spec_json, &status_json)?;
    Ok(resources_proto::Resource {
        api_version: manifest.api_version,
        kind: manifest.kind,
        metadata: Some(metadata),
        spec: Some(spec),
        status: Some(status),
    })
}

pub fn parse_resource_manifest(yaml: &str) -> Result<resources_proto::ResourceManifest> {
    let manifest: DesiredResourceManifest =
        serde_yaml::from_str(yaml).context("Failed to parse resource manifest YAML")?;
    if manifest.status.is_some() {
        bail!("Resource manifests cannot set status; status is controller-owned");
    }
    let metadata = manifest.metadata.into_resource_meta();
    validate_mcp_server_namespace(&manifest.kind, &metadata)?;
    let spec_json = non_empty_json_object(yaml_value_to_json_string(manifest.spec)?);
    let (spec, _) = resource_spec_status_from_json(&manifest.kind, &spec_json, "{}")?;
    Ok(resources_proto::ResourceManifest {
        api_version: manifest.api_version,
        kind: manifest.kind,
        metadata: Some(metadata),
        spec: Some(spec),
    })
}

fn validate_mcp_server_namespace(
    kind: &str,
    metadata: &resources_proto::ResourceMeta,
) -> Result<()> {
    if kind == "McpServerBinding" {
        bail!("McpServerBinding manifests are unsupported; use namespaced McpServer");
    }
    if kind == "McpServer" && metadata.namespace.trim().is_empty() {
        bail!("McpServer metadata.namespace is required");
    }
    Ok(())
}

fn non_empty_json_object(value: String) -> String {
    if value.trim().is_empty() {
        "{}".to_string()
    } else {
        value
    }
}

pub fn parse_generic_resource(yaml: &str) -> Result<resources_proto::Resource> {
    parse_resource(yaml)
}

pub fn render_resource_yaml(resource: &resources_proto::Resource) -> Result<String> {
    let metadata = resource
        .metadata
        .as_ref()
        .ok_or_else(|| anyhow!("Resource missing metadata"))?;
    let (spec, status) = resource_spec_status_to_yaml_values(resource)?;
    let yaml = ResourceYamlDocument {
        api_version: resource.api_version.clone(),
        kind: resource.kind.clone(),
        metadata: ObjectMetaManifest::from_resource_meta(metadata),
        spec,
        status,
    };
    serde_yaml::to_string(&yaml).context("Failed to serialize resource YAML")
}

pub fn render_generic_resource_yaml(resource: &resources_proto::Resource) -> Result<String> {
    render_resource_yaml(resource)
}

pub fn resource_spec_status_from_json(
    kind: &str,
    spec_json: &str,
    status_json: &str,
) -> Result<(
    resources_proto::ResourceSpec,
    resources_proto::ResourceStatus,
)> {
    use resources_proto::resource_spec::Kind as SpecKind;
    use resources_proto::resource_status::Kind as StatusKind;

    let spec_value: serde_json::Value = serde_json::from_str(spec_json)?;
    let status_value: serde_json::Value = serde_json::from_str(status_json)?;

    let spec = match kind {
        "Agent" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::Agent(agent_spec_from_value(spec_value)?)),
        },
        "Schedule" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::Schedule(schedule_spec_from_value(spec_value)?)),
        },
        "Template" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::Template(template_spec_from_value(spec_value)?)),
        },
        "Workflow" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::Workflow(
                serde_json::from_value::<WorkflowSpecManifest>(spec_value)?.into_proto()?,
            )),
        },
        "Deployment" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::Deployment(deployment_spec_from_value(
                spec_value,
            )?)),
        },
        "DeploymentReplica" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::DeploymentReplica(
                deployment_replica_spec_from_value(spec_value)?,
            )),
        },
        "SandboxClass" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::SandboxClass(sandbox_class_spec_from_value(
                spec_value,
            )?)),
        },
        "SandboxPolicy" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::SandboxPolicy(sandbox_policy_spec_from_value(
                spec_value,
            )?)),
        },
        "Sandbox" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::Sandbox(sandbox_spec_from_value(spec_value)?)),
        },
        "UsagePolicy" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::UsagePolicy(serde_json::from_value(spec_value)?)),
        },
        "ConnectorClass" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::ConnectorClass(serde_json::from_value(
                spec_value,
            )?)),
        },
        "Connector" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::Connector(serde_json::from_value(spec_value)?)),
        },
        "File" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::File(file_spec_from_value(spec_value)?)),
        },
        "Task" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::Task(serde_json::from_value(spec_value)?)),
        },
        "Skill" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::Skill(skill_spec_from_value(spec_value)?)),
        },
        "Worker" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::Worker(worker_spec_from_value(spec_value)?)),
        },
        "McpServer" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::McpServer(serde_json::from_value(spec_value)?)),
        },
        _ => resources_proto::ResourceSpec {
            kind: Some(SpecKind::Raw(resources_proto::RawResourceSpec {
                json: spec_json.to_string(),
            })),
        },
    };

    let status = match kind {
        "Agent" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::Agent(agent_status_from_value(status_value)?)),
        },
        "Schedule" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::Schedule(schedule_status_from_value(
                status_value,
            )?)),
        },
        "Workflow" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::Workflow(workflow_status_from_value(
                status_value,
            )?)),
        },
        "Deployment" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::Deployment(deployment_status_from_value(
                status_value,
            )?)),
        },
        "DeploymentReplica" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::DeploymentReplica(
                deployment_replica_status_from_value(status_value)?,
            )),
        },
        "Sandbox" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::Sandbox(sandbox_status_from_value(
                status_value,
            )?)),
        },
        "Template" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::Template(common_status_from_value(
                status_value,
            )?)),
        },
        "SandboxClass" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::SandboxClass(common_status_from_value(
                status_value,
            )?)),
        },
        "SandboxPolicy" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::SandboxPolicy(common_status_from_value(
                status_value,
            )?)),
        },
        "Skill" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::Skill(common_status_from_value(status_value)?)),
        },
        "UsagePolicy" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::UsagePolicy(serde_json::from_value(
                status_value,
            )?)),
        },
        "ConnectorClass" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::ConnectorClass(serde_json::from_value(
                status_value,
            )?)),
        },
        "Connector" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::Connector(serde_json::from_value(status_value)?)),
        },
        "File" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::File(file_status_from_value(status_value)?)),
        },
        "Task" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::Task(task_status_from_value(status_value)?)),
        },
        "Worker" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::Worker(worker_status_from_value(status_value)?)),
        },
        "McpServer" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::McpServer(common_status_from_value(
                status_value,
            )?)),
        },
        _ => resources_proto::ResourceStatus {
            kind: Some(StatusKind::Raw(resources_proto::RawResourceStatus {
                json: status_json.to_string(),
            })),
        },
    };

    Ok((spec, status))
}

fn resource_spec_status_to_yaml_values(
    resource: &resources_proto::Resource,
) -> Result<(serde_yaml::Value, serde_yaml::Value)> {
    use resources_proto::resource_spec::Kind as SpecKind;
    use resources_proto::resource_status::Kind as StatusKind;

    let spec_json = match resource.spec.as_ref().and_then(|spec| spec.kind.as_ref()) {
        Some(SpecKind::Agent(spec)) => serde_json::to_string(&AgentSpecManifest::from_proto(spec))?,
        Some(SpecKind::Schedule(spec)) => serde_json::to_string(&schedule_spec_to_json(spec))?,
        Some(SpecKind::Template(spec)) => serde_json::to_string(&serde_json::json!({
            "kind": spec.kind,
            "metadata": spec.metadata.as_ref().map(ObjectMetaManifest::from_resource_meta),
            "spec": json_string_to_json_value(&spec.spec_json)?,
        }))?,
        Some(SpecKind::Workflow(spec)) => {
            serde_json::to_string(&WorkflowSpecManifest::from_proto(spec)?)?
        }
        Some(SpecKind::Deployment(spec)) => serde_json::to_string(&serde_json::json!({
            "placement": {
                "namespaceSelector": spec.placement.as_ref().and_then(|p| p.namespace_selector.as_ref()).map(|selector| serde_json::json!({
                    "parent": selector.parent,
                    "matchLabels": selector.match_labels,
                })),
            },
            "templates": spec.templates,
        }))?,
        Some(SpecKind::DeploymentReplica(spec)) => serde_json::to_string(&serde_json::json!({
            "deploymentRef": spec.deployment_ref.as_ref().map(resource_ref_json),
            "targetNamespace": spec.target_namespace,
        }))?,
        Some(SpecKind::SandboxClass(spec)) => serde_json::to_string(&serde_json::json!({
            "provider": spec.provider,
            "providerConfig": json_string_to_json_value(&spec.provider_config_json)?,
            "credentials": json_string_to_json_value(&spec.credentials_json)?,
        }))?,
        Some(SpecKind::SandboxPolicy(spec)) => serde_json::to_string(&serde_json::json!({
            "classRef": spec.class_ref.as_ref().map(resource_ref_json),
            "template": sandbox_runtime_template_to_json_value(spec.template.as_ref()),
            "maxConcurrent": spec.max_concurrent,
        }))?,
        Some(SpecKind::Sandbox(spec)) => serde_json::to_string(&serde_json::json!({
            "policyRef": spec.policy_ref,
            "classRef": spec.class_ref.as_ref().map(resource_ref_json),
            "runtimeTemplate": sandbox_runtime_template_to_json_value(spec.runtime_template.as_ref()),
        }))?,
        Some(SpecKind::UsagePolicy(spec)) => serde_json::to_string(spec)?,
        Some(SpecKind::ConnectorClass(spec)) => serde_json::to_string(spec)?,
        Some(SpecKind::Connector(spec)) => serde_json::to_string(spec)?,
        Some(SpecKind::File(spec)) => serde_json::to_string(&FileSpecManifest::from_proto(spec))?,
        Some(SpecKind::Task(spec)) => serde_json::to_string(spec)?,
        Some(SpecKind::McpServer(spec)) => serde_json::to_string(spec)?,
        Some(SpecKind::Skill(spec)) => serde_json::to_string(&serde_json::json!({
            "description": spec.description,
            "instructions": spec.instructions,
        }))?,
        Some(SpecKind::Worker(_)) => "{}".to_string(),
        Some(SpecKind::Raw(raw)) => raw.json.clone(),
        _ => "{}".to_string(),
    };

    let status_json = match resource
        .status
        .as_ref()
        .and_then(|status| status.kind.as_ref())
    {
        Some(StatusKind::Agent(status)) => {
            let mut json = common_status_map(
                status.observed_generation,
                &status.phase,
                &status.conditions,
            );
            if let Some(last_session_id) = &status.last_session_id {
                if !last_session_id.is_empty() {
                    json.insert(
                        "lastSessionId".to_string(),
                        serde_json::Value::String(last_session_id.clone()),
                    );
                }
            }
            serde_json::to_string(&serde_json::Value::Object(json))?
        }
        Some(StatusKind::Schedule(status)) => {
            serde_json::to_string(&schedule_status_to_json(status))?
        }
        Some(StatusKind::Workflow(status)) => serde_json::to_string(&common_status_map(
            status.observed_generation,
            &status.phase,
            &status.conditions,
        ))?,
        Some(StatusKind::Deployment(status)) => {
            let mut json = common_status_map(
                status.observed_generation,
                &status.phase,
                &status.conditions,
            );
            if !status.replicas.is_empty() {
                json.insert(
                    "replicas".to_string(),
                    serde_json::Value::Array(
                        status
                            .replicas
                            .iter()
                            .map(resource_ref_json)
                            .collect::<Vec<_>>(),
                    ),
                );
            }
            if let Some(counts) = &status.replica_counts {
                json.insert("replicaCounts".to_string(), replica_counts_json(counts));
            }
            serde_json::to_string(&serde_json::Value::Object(json))?
        }
        Some(StatusKind::DeploymentReplica(status)) => {
            let mut json = common_status_map(
                status.observed_generation,
                &status.phase,
                &status.conditions,
            );
            if !status.rendered_resources.is_empty() {
                json.insert(
                    "renderedResources".to_string(),
                    serde_json::to_value(&status.rendered_resources)?,
                );
            }
            if !status.rendered_hashes.is_empty() {
                json.insert(
                    "renderedHashes".to_string(),
                    serde_json::to_value(&status.rendered_hashes)?,
                );
            }
            if !status.conflicts.is_empty() {
                json.insert(
                    "conflicts".to_string(),
                    serde_json::to_value(&status.conflicts)?,
                );
            }
            if !status.last_rendered_json.is_empty() {
                json.insert(
                    "lastRenderedJson".to_string(),
                    serde_json::to_value(&status.last_rendered_json)?,
                );
            }
            if !status.owned_json_pointers.is_empty() {
                json.insert(
                    "ownedJsonPointers".to_string(),
                    serde_json::to_value(&status.owned_json_pointers)?,
                );
            }
            serde_json::to_string(&serde_json::Value::Object(json))?
        }
        Some(StatusKind::Sandbox(status)) => {
            let mut json = common_status_map(
                status.observed_generation,
                &status.phase,
                &status.conditions,
            );
            if !status.backend_id.is_empty() {
                json.insert(
                    "backendId".to_string(),
                    serde_json::Value::String(status.backend_id.clone()),
                );
            }
            if let Some(lease) = &status.lease {
                json.insert("lease".to_string(), sandbox_lease_to_json(lease));
            }
            if !status.processes.is_empty() {
                json.insert(
                    "processes".to_string(),
                    serde_json::Value::Array(
                        status
                            .processes
                            .iter()
                            .map(sandbox_process_status_to_json)
                            .collect(),
                    ),
                );
            }
            serde_json::to_string(&serde_json::Value::Object(json))?
        }
        Some(StatusKind::Template(status))
        | Some(StatusKind::Skill(status))
        | Some(StatusKind::McpServer(status))
        | Some(StatusKind::SandboxClass(status))
        | Some(StatusKind::SandboxPolicy(status)) => {
            serde_json::to_string(&common_status_to_json(status))?
        }
        Some(StatusKind::Worker(status)) => serde_json::to_string(&worker_status_to_json(status))?,
        Some(StatusKind::UsagePolicy(status)) => serde_json::to_string(status)?,
        Some(StatusKind::ConnectorClass(status)) => serde_json::to_string(status)?,
        Some(StatusKind::Connector(status)) => serde_json::to_string(status)?,
        Some(StatusKind::File(status)) => serde_json::to_string(status)?,
        Some(StatusKind::Task(status)) => {
            serde_json::to_string(&TaskStatusManifest::from_proto(status))?
        }
        Some(StatusKind::Raw(raw)) => raw.json.clone(),
        _ => "{}".to_string(),
    };

    Ok((
        json_string_to_yaml_value(&spec_json)?,
        json_string_to_yaml_value(&status_json)?,
    ))
}

fn json_string_to_json_value(value: &str) -> Result<serde_json::Value> {
    if value.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(value).context("Failed to parse embedded JSON")
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct FileSpecManifest {
    path: String,
    media_type: String,
    #[serde(with = "crate::control::manifest::enum_serde::file_purpose")]
    purpose: i32,
    #[serde(with = "crate::control::manifest::enum_serde::file_index_policy")]
    index_policy: i32,
    #[serde(with = "crate::control::manifest::enum_serde::file_retention")]
    retention: i32,
}

impl FileSpecManifest {
    fn into_proto(self) -> resources_proto::FileSpec {
        resources_proto::FileSpec {
            path: self.path,
            media_type: self.media_type,
            purpose: self.purpose,
            index_policy: self.index_policy,
            retention: self.retention,
        }
    }

    fn from_proto(spec: &resources_proto::FileSpec) -> Self {
        Self {
            path: spec.path.clone(),
            media_type: spec.media_type.clone(),
            purpose: spec.purpose,
            index_policy: spec.index_policy,
            retention: spec.retention,
        }
    }
}

fn file_spec_from_value(value: serde_json::Value) -> Result<resources_proto::FileSpec> {
    Ok(serde_json::from_value::<FileSpecManifest>(value)?.into_proto())
}

fn file_status_from_value(value: serde_json::Value) -> Result<resources_proto::FileStatus> {
    if value.as_object().map(|object| object.is_empty()).unwrap_or(false) {
        return Ok(resources_proto::FileStatus::default());
    }
    serde_json::from_value(value).context("Failed to parse File status")
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct TaskStatusManifest {
    observed_generation: u64,
    #[serde(with = "crate::control::manifest::enum_serde::task_phase")]
    phase: i32,
    conditions: Vec<resources_proto::ResourceCondition>,
    progress_summary: String,
    result_artifacts: Vec<resources_proto::FileObjectRef>,
    output_artifact_uris: Vec<String>,
    created_at: i64,
    updated_at: i64,
    completed_at: i64,
    expires_at: i64,
    execution_ref: Option<resources_proto::TaskExecutionRef>,
}

impl TaskStatusManifest {
    fn into_proto(self) -> resources_proto::TaskStatus {
        resources_proto::TaskStatus {
            observed_generation: self.observed_generation,
            phase: self.phase,
            conditions: self.conditions,
            progress_summary: self.progress_summary,
            result_artifacts: self.result_artifacts,
            output_artifact_uris: self.output_artifact_uris,
            created_at: self.created_at,
            updated_at: self.updated_at,
            completed_at: self.completed_at,
            expires_at: self.expires_at,
            execution_ref: self.execution_ref,
        }
    }

    fn from_proto(status: &resources_proto::TaskStatus) -> Self {
        Self {
            observed_generation: status.observed_generation,
            phase: status.phase,
            conditions: status.conditions.clone(),
            progress_summary: status.progress_summary.clone(),
            result_artifacts: status.result_artifacts.clone(),
            output_artifact_uris: status.output_artifact_uris.clone(),
            created_at: status.created_at,
            updated_at: status.updated_at,
            completed_at: status.completed_at,
            expires_at: status.expires_at,
            execution_ref: status.execution_ref.clone(),
        }
    }
}

fn task_status_from_value(value: serde_json::Value) -> Result<resources_proto::TaskStatus> {
    if value.as_object().map(|object| object.is_empty()).unwrap_or(false) {
        return Ok(resources_proto::TaskStatus::default());
    }
    Ok(serde_json::from_value::<TaskStatusManifest>(value)
        .context("Failed to parse Task status")?
        .into_proto())
}

fn template_spec_from_value(value: serde_json::Value) -> Result<resources_proto::TemplateSpec> {
    let kind = value
        .get("kind")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let metadata = value
        .get("metadata")
        .cloned()
        .map(serde_json::from_value::<ObjectMetaManifest>)
        .transpose()?
        .map(ObjectMetaManifest::into_resource_meta);
    let spec_json = json_field_or_string(&value, "spec", "specJson")?;
    Ok(resources_proto::TemplateSpec {
        kind,
        metadata,
        spec_json,
    })
}

fn agent_spec_from_value(value: serde_json::Value) -> Result<resources_proto::AgentSpec> {
    let spec = serde_json::from_value::<AgentSpecManifest>(value)?;
    let spec = spec.into_proto()?;
    validate_acp_permission_policy_manifest(&spec)?;
    Ok(spec)
}

fn skill_spec_from_value(value: serde_json::Value) -> Result<resources_proto::SkillSpec> {
    let spec = serde_json::from_value::<resources_proto::SkillSpec>(value)?;
    if spec.description.trim().is_empty() {
        bail!("Skill spec.description is required");
    }
    if spec.instructions.trim().is_empty() {
        bail!("Skill spec.instructions is required");
    }
    Ok(spec)
}

fn worker_spec_from_value(value: serde_json::Value) -> Result<resources_proto::WorkerSpec> {
    Ok(serde_json::from_value::<resources_proto::WorkerSpec>(
        value,
    )?)
}

fn validate_acp_permission_policy_manifest(spec: &resources_proto::AgentSpec) -> Result<()> {
    let Some(runtime) = spec.runtime.as_ref() else {
        return Ok(());
    };
    let Some(acp) = runtime.acp.as_ref() else {
        return Ok(());
    };
    const ALLOWED_KEYS: &[&str] = &["default", "filesystemRead", "filesystemWrite", "terminal"];
    const ALLOWED_VALUES: &[&str] = &["allow", "ask", "deny"];
    for (key, value) in &acp.permission_policy {
        if !ALLOWED_KEYS.contains(&key.as_str()) {
            bail!(
                "Agent spec.runtime.acp.permissionPolicy contains unsupported key '{}'",
                key
            );
        }
        if !ALLOWED_VALUES.contains(&value.as_str()) {
            bail!(
                "Agent spec.runtime.acp.permissionPolicy.{} has unsupported value '{}'",
                key,
                value
            );
        }
    }
    Ok(())
}

fn deployment_spec_from_value(value: serde_json::Value) -> Result<resources_proto::DeploymentSpec> {
    let selector = value
        .pointer("/placement/namespaceSelector")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let parent = selector
        .get("parent")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let match_labels = selector
        .get("matchLabels")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    let templates = value
        .get("templates")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    Ok(resources_proto::DeploymentSpec {
        placement: Some(resources_proto::DeploymentPlacement {
            namespace_selector: Some(resources_proto::NamespaceSelector {
                parent,
                match_labels,
            }),
        }),
        templates,
    })
}

fn deployment_replica_spec_from_value(
    value: serde_json::Value,
) -> Result<resources_proto::DeploymentReplicaSpec> {
    Ok(resources_proto::DeploymentReplicaSpec {
        deployment_ref: value
            .get("deploymentRef")
            .map(resource_ref_from_value)
            .transpose()?,
        target_namespace: value
            .get("targetNamespace")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

fn sandbox_class_spec_from_value(
    value: serde_json::Value,
) -> Result<resources_proto::SandboxClassSpec> {
    Ok(resources_proto::SandboxClassSpec {
        provider: value
            .get("provider")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        provider_config_json: json_field_or_string(&value, "providerConfig", "providerConfigJson")?,
        credentials_json: json_field_or_string(&value, "credentials", "credentialsJson")?,
    })
}

fn sandbox_policy_spec_from_value(
    value: serde_json::Value,
) -> Result<resources_proto::SandboxPolicySpec> {
    Ok(resources_proto::SandboxPolicySpec {
        class_ref: value
            .get("classRef")
            .map(resource_ref_from_value)
            .transpose()?,
        template: Some(sandbox_runtime_template_from_value(
            value
                .pointer("/template/spec")
                .or_else(|| value.get("template"))
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        )?),
        max_concurrent: value
            .pointer("/quota/maxConcurrent")
            .or_else(|| value.get("maxConcurrent"))
            .and_then(|value| value.as_u64())
            .map(u32::try_from)
            .transpose()
            .map_err(|_| anyhow!("sandbox policy maxConcurrent exceeds u32 range"))?
            .unwrap_or(0),
    })
}

fn json_field_or_string(
    value: &serde_json::Value,
    object_key: &str,
    string_key: &str,
) -> Result<String> {
    if let Some(value) = value.get(object_key) {
        return serde_json::to_string(value).context("Failed to serialize embedded JSON field");
    }
    if let Some(value) = value.get(string_key) {
        if let Some(json) = value.as_str() {
            let _: serde_json::Value = serde_json::from_str(json)
                .with_context(|| format!("{} must contain valid JSON", string_key))?;
            return Ok(json.to_string());
        }
        return serde_json::to_string(value).context("Failed to serialize embedded JSON field");
    }
    Ok("{}".to_string())
}

fn sandbox_runtime_template_to_json_value(
    template: Option<&resources_proto::SandboxRuntimeTemplateSpec>,
) -> serde_json::Value {
    let Some(template) = template else {
        return serde_json::json!({});
    };
    serde_json::json!({
        "image": template.image,
        "workspace": template.workspace.as_ref().map(|workspace| serde_json::json!({
            "mode": workspace.mode,
            "mountPath": workspace.mount_path,
        })),
        "setup": template.setup.as_ref().map(|setup| serde_json::json!({
            "packages": setup.packages,
            "commands": setup.commands,
        })),
        "network": template.network.as_ref().map(|network| serde_json::json!({
            "mode": network.mode,
        })),
        "filesystem": template.filesystem.as_ref().map(|filesystem| serde_json::json!({
            "writable": filesystem.writable,
            "readonly": filesystem.readonly,
        })),
        "leasePolicy": template.lease_policy.as_ref().map(|lease_policy| serde_json::json!({
            "mode": lease_policy.mode,
        })),
    })
}

fn sandbox_runtime_template_from_value(
    value: serde_json::Value,
) -> Result<resources_proto::SandboxRuntimeTemplateSpec> {
    let mount_path = value
        .pointer("/workspace/mountPath")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    validate_sandbox_mount_path(&mount_path)?;
    Ok(resources_proto::SandboxRuntimeTemplateSpec {
        image: value
            .get("image")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        workspace: Some(resources_proto::SandboxWorkspaceSpec {
            mode: value
                .pointer("/workspace/mode")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            mount_path,
        }),
        setup: Some(resources_proto::SandboxSetupSpec {
            packages: value
                .pointer("/setup/packages")
                .and_then(|value| serde_json::from_value(value.clone()).ok())
                .unwrap_or_default(),
            commands: value
                .pointer("/setup/commands")
                .and_then(|value| serde_json::from_value(value.clone()).ok())
                .unwrap_or_default(),
        }),
        network: Some(resources_proto::SandboxNetworkSpec {
            mode: value
                .pointer("/network/mode")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
        }),
        filesystem: Some(resources_proto::SandboxFilesystemSpec {
            writable: value
                .pointer("/filesystem/writable")
                .and_then(|value| serde_json::from_value(value.clone()).ok())
                .unwrap_or_default(),
            readonly: value
                .pointer("/filesystem/readonly")
                .and_then(|value| serde_json::from_value(value.clone()).ok())
                .unwrap_or_default(),
        }),
        lease_policy: Some(resources_proto::SandboxLeasePolicySpec {
            mode: value
                .pointer("/leasePolicy/mode")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
        }),
    })
}

fn validate_sandbox_mount_path(mount_path: &str) -> Result<()> {
    if mount_path.is_empty() {
        return Ok(());
    }
    if !mount_path.starts_with('/') {
        bail!("SandboxPolicy template.workspace.mountPath must be absolute");
    }
    let normalized = mount_path.trim_end_matches('/');
    let forbidden = [
        "", "/bin", "/boot", "/dev", "/etc", "/lib", "/lib64", "/proc", "/root", "/run", "/sbin",
        "/sys", "/usr", "/var",
    ];
    if forbidden.contains(&normalized) {
        bail!(
            "SandboxPolicy template.workspace.mountPath '{}' is not allowed",
            mount_path
        );
    }
    Ok(())
}

fn sandbox_spec_from_value(value: serde_json::Value) -> Result<resources_proto::SandboxSpec> {
    Ok(resources_proto::SandboxSpec {
        policy_ref: value
            .get("policyRef")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        class_ref: value
            .get("classRef")
            .map(resource_ref_from_value)
            .transpose()?,
        runtime_template: Some(sandbox_runtime_template_from_value(
            value
                .get("runtimeTemplate")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        )?),
    })
}

fn schedule_spec_from_value(value: serde_json::Value) -> Result<resources_proto::ScheduleSpec> {
    Ok(resources_proto::ScheduleSpec {
        kind: value
            .get("kind")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        cron: value
            .get("cron")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        interval_seconds: value
            .get("intervalSeconds")
            .and_then(|value| value.as_u64())
            .map(u32::try_from)
            .transpose()
            .map_err(|_| anyhow!("schedule spec intervalSeconds exceeds u32 range"))?
            .unwrap_or(0),
        run_at: value
            .get("runAt")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        timezone: value
            .get("timezone")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        target: value
            .get("target")
            .map(schedule_target_from_value)
            .transpose()?,
        input_message: value
            .get("inputMessage")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        enabled: value
            .get("enabled")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        input_json: value
            .get("inputJson")
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string())
            })
            .unwrap_or_default(),
    })
}

fn schedule_target_from_value(
    value: &serde_json::Value,
) -> Result<resources_proto::ScheduleTarget> {
    Ok(resources_proto::ScheduleTarget {
        agent: value
            .get("agent")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        session_mode: value
            .get("sessionMode")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        session_id: value
            .get("sessionId")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        workflow: value
            .get("workflow")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

fn deployment_status_from_value(
    value: serde_json::Value,
) -> Result<resources_proto::DeploymentStatus> {
    Ok(resources_proto::DeploymentStatus {
        observed_generation: value
            .get("observedGeneration")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        phase: value
            .get("phase")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        conditions: conditions_from_value(&value),
        replicas: value
            .get("replicas")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| resource_ref_from_value(item).ok())
                    .collect()
            })
            .unwrap_or_default(),
        replica_counts: value
            .get("replicaCounts")
            .map(deployment_replica_counts_from_value)
            .transpose()?,
    })
}

fn deployment_replica_counts_from_value(
    value: &serde_json::Value,
) -> Result<resources_proto::DeploymentReplicaCounts> {
    Ok(resources_proto::DeploymentReplicaCounts {
        desired: value
            .get("desired")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        updated: value
            .get("updated")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        ready: value
            .get("ready")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        pending: value
            .get("pending")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        degraded: value
            .get("degraded")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
    })
}

fn schedule_status_from_value(value: serde_json::Value) -> Result<resources_proto::ScheduleStatus> {
    Ok(resources_proto::ScheduleStatus {
        observed_generation: value
            .get("observedGeneration")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        phase: value
            .get("phase")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        conditions: conditions_from_value(&value),
        revision: value
            .get("revision")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        next_run_at: value.get("nextRunAt").and_then(|value| value.as_i64()),
        backend_handle: value
            .get("backendHandle")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        backend_armed: value
            .get("backendArmed")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        last_run_at: value.get("lastRunAt").and_then(|value| value.as_i64()),
        last_session_id: value
            .get("lastSessionId")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        last_error: value
            .get("lastError")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        claimed_run_at: value.get("claimedRunAt").and_then(|value| value.as_i64()),
        claim_expires_at: value.get("claimExpiresAt").and_then(|value| value.as_i64()),
        recent_events: value
            .get("recentEvents")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default(),
    })
}

fn schedule_status_to_json(status: &resources_proto::ScheduleStatus) -> serde_json::Value {
    let mut json = common_status_map(
        status.observed_generation,
        &status.phase,
        &status.conditions,
    );
    if status.revision != 0 {
        json.insert(
            "revision".to_string(),
            serde_json::Value::Number(status.revision.into()),
        );
    }
    if let Some(next_run_at) = status.next_run_at {
        json.insert(
            "nextRunAt".to_string(),
            serde_json::Value::Number(next_run_at.into()),
        );
    }
    if let Some(backend_handle) = &status.backend_handle {
        if !backend_handle.is_empty() {
            json.insert(
                "backendHandle".to_string(),
                serde_json::Value::String(backend_handle.clone()),
            );
        }
    }
    if status.backend_armed {
        json.insert("backendArmed".to_string(), serde_json::Value::Bool(true));
    }
    if let Some(last_run_at) = status.last_run_at {
        json.insert(
            "lastRunAt".to_string(),
            serde_json::Value::Number(last_run_at.into()),
        );
    }
    if let Some(last_session_id) = &status.last_session_id {
        if !last_session_id.is_empty() {
            json.insert(
                "lastSessionId".to_string(),
                serde_json::Value::String(last_session_id.clone()),
            );
        }
    }
    if let Some(last_error) = &status.last_error {
        if !last_error.is_empty() {
            json.insert(
                "lastError".to_string(),
                serde_json::Value::String(last_error.clone()),
            );
        }
    }
    if let Some(claimed_run_at) = status.claimed_run_at {
        json.insert(
            "claimedRunAt".to_string(),
            serde_json::Value::Number(claimed_run_at.into()),
        );
    }
    if let Some(claim_expires_at) = status.claim_expires_at {
        json.insert(
            "claimExpiresAt".to_string(),
            serde_json::Value::Number(claim_expires_at.into()),
        );
    }
    if !status.recent_events.is_empty() {
        json.insert(
            "recentEvents".to_string(),
            serde_json::to_value(&status.recent_events).unwrap_or_default(),
        );
    }
    serde_json::Value::Object(json)
}

fn schedule_spec_to_json(spec: &resources_proto::ScheduleSpec) -> serde_json::Value {
    let mut json = serde_json::Map::new();
    json.insert(
        "kind".to_string(),
        serde_json::Value::String(spec.kind.clone()),
    );
    if !spec.cron.is_empty() {
        json.insert(
            "cron".to_string(),
            serde_json::Value::String(spec.cron.clone()),
        );
    }
    if spec.interval_seconds != 0 {
        json.insert(
            "intervalSeconds".to_string(),
            serde_json::Value::Number(spec.interval_seconds.into()),
        );
    }
    if !spec.run_at.is_empty() {
        json.insert(
            "runAt".to_string(),
            serde_json::Value::String(spec.run_at.clone()),
        );
    }
    if !spec.timezone.is_empty() {
        json.insert(
            "timezone".to_string(),
            serde_json::Value::String(spec.timezone.clone()),
        );
    }
    if let Some(target) = &spec.target {
        let mut target_json = serde_json::Map::new();
        if !target.agent.is_empty() {
            target_json.insert(
                "agent".to_string(),
                serde_json::Value::String(target.agent.clone()),
            );
        }
        if !target.session_mode.is_empty() {
            target_json.insert(
                "sessionMode".to_string(),
                serde_json::Value::String(target.session_mode.clone()),
            );
        }
        if !target.session_id.is_empty() {
            target_json.insert(
                "sessionId".to_string(),
                serde_json::Value::String(target.session_id.clone()),
            );
        }
        if !target.workflow.is_empty() {
            target_json.insert(
                "workflow".to_string(),
                serde_json::Value::String(target.workflow.clone()),
            );
        }
        json.insert("target".to_string(), serde_json::Value::Object(target_json));
    }
    if !spec.input_message.is_empty() {
        json.insert(
            "inputMessage".to_string(),
            serde_json::Value::String(spec.input_message.clone()),
        );
    }
    json.insert("enabled".to_string(), serde_json::Value::Bool(spec.enabled));
    if !spec.input_json.is_empty() {
        json.insert(
            "inputJson".to_string(),
            serde_json::from_str(&spec.input_json)
                .unwrap_or_else(|_| serde_json::Value::String(spec.input_json.clone())),
        );
    }
    serde_json::Value::Object(json)
}

fn deployment_replica_status_from_value(
    value: serde_json::Value,
) -> Result<resources_proto::DeploymentReplicaStatus> {
    Ok(resources_proto::DeploymentReplicaStatus {
        observed_generation: value
            .get("observedGeneration")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        phase: value
            .get("phase")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        conditions: conditions_from_value(&value),
        rendered_resources: value
            .get("renderedResources")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default(),
        rendered_hashes: value
            .get("renderedHashes")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default(),
        conflicts: value
            .get("conflicts")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default(),
        last_rendered_json: value
            .get("lastRenderedJson")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default(),
        owned_json_pointers: value
            .get("ownedJsonPointers")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default(),
    })
}

fn sandbox_status_from_value(value: serde_json::Value) -> Result<resources_proto::SandboxStatus> {
    Ok(resources_proto::SandboxStatus {
        observed_generation: value
            .get("observedGeneration")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        phase: value
            .get("phase")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        conditions: conditions_from_value(&value),
        backend_id: value
            .get("backendId")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        lease: sandbox_lease_from_value(value.get("lease")),
        processes: sandbox_processes_from_value(value.get("processes")),
    })
}

fn sandbox_lease_from_value(
    value: Option<&serde_json::Value>,
) -> Option<resources_proto::SandboxLease> {
    let value = value?;
    Some(resources_proto::SandboxLease {
        owner_kind: value
            .get("ownerKind")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        owner_agent: value
            .get("ownerAgent")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        owner_session_id: value
            .get("ownerSessionId")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        token: value
            .get("token")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        acquired_at: value
            .get("acquiredAt")
            .and_then(|value| value.as_i64())
            .unwrap_or_default(),
        expires_at: value
            .get("expiresAt")
            .and_then(|value| value.as_i64())
            .unwrap_or_default(),
        heartbeat_at: value
            .get("heartbeatAt")
            .and_then(|value| value.as_i64())
            .unwrap_or_default(),
    })
}

fn sandbox_processes_from_value(
    value: Option<&serde_json::Value>,
) -> Vec<resources_proto::SandboxProcessStatus> {
    value
        .and_then(|value| value.as_array())
        .map(|processes| {
            processes
                .iter()
                .map(|process| resources_proto::SandboxProcessStatus {
                    id: process
                        .get("id")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    command: process
                        .get("command")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    args: process
                        .get("args")
                        .and_then(|value| serde_json::from_value(value.clone()).ok())
                        .unwrap_or_default(),
                    protocol: process
                        .get("protocol")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    phase: process
                        .get("phase")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn agent_status_from_value(value: serde_json::Value) -> Result<resources_proto::AgentStatus> {
    Ok(resources_proto::AgentStatus {
        observed_generation: value
            .get("observedGeneration")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        phase: value
            .get("phase")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        conditions: conditions_from_value(&value),
        last_session_id: value
            .get("lastSessionId")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    })
}

fn worker_status_from_value(value: serde_json::Value) -> Result<resources_proto::WorkerStatus> {
    Ok(resources_proto::WorkerStatus {
        observed_generation: value
            .get("observedGeneration")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        phase: value
            .get("phase")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        conditions: conditions_from_value(&value),
        started_at: value
            .get("startedAt")
            .and_then(|value| value.as_i64())
            .unwrap_or_default(),
        heartbeat_at: value
            .get("heartbeatAt")
            .and_then(|value| value.as_i64())
            .unwrap_or_default(),
        expires_at: value
            .get("expiresAt")
            .and_then(|value| value.as_i64())
            .unwrap_or_default(),
        version: value
            .get("version")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        endpoints: value
            .get("endpoints")
            .and_then(|value| value.as_array())
            .map(|endpoints| {
                endpoints
                    .iter()
                    .cloned()
                    .map(serde_json::from_value::<resources_proto::WorkerEndpoint>)
                    .collect::<std::result::Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default(),
    })
}

fn common_status_from_value(
    value: serde_json::Value,
) -> Result<resources_proto::CommonResourceStatus> {
    Ok(resources_proto::CommonResourceStatus {
        observed_generation: value
            .get("observedGeneration")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        phase: value
            .get("phase")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        conditions: conditions_from_value(&value),
    })
}

fn workflow_status_from_value(value: serde_json::Value) -> Result<resources_proto::WorkflowStatus> {
    Ok(resources_proto::WorkflowStatus {
        observed_generation: value
            .get("observedGeneration")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        phase: value
            .get("phase")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        conditions: conditions_from_value(&value),
    })
}

fn common_status_to_json(status: &resources_proto::CommonResourceStatus) -> serde_json::Value {
    serde_json::Value::Object(common_status_map(
        status.observed_generation,
        &status.phase,
        &status.conditions,
    ))
}

fn worker_status_to_json(status: &resources_proto::WorkerStatus) -> serde_json::Value {
    let mut json = common_status_map(
        status.observed_generation,
        &status.phase,
        &status.conditions,
    );
    json.insert("startedAt".to_string(), status.started_at.into());
    json.insert("heartbeatAt".to_string(), status.heartbeat_at.into());
    json.insert("expiresAt".to_string(), status.expires_at.into());
    if !status.version.is_empty() {
        json.insert(
            "version".to_string(),
            serde_json::Value::String(status.version.clone()),
        );
    }
    if !status.endpoints.is_empty() {
        json.insert(
            "endpoints".to_string(),
            serde_json::to_value(&status.endpoints).unwrap_or_default(),
        );
    }
    serde_json::Value::Object(json)
}

fn common_status_map(
    observed_generation: u64,
    phase: &str,
    conditions: &[resources_proto::ResourceCondition],
) -> serde_json::Map<String, serde_json::Value> {
    let mut json = serde_json::Map::new();
    if observed_generation != 0 {
        json.insert(
            "observedGeneration".to_string(),
            serde_json::Value::Number(observed_generation.into()),
        );
    }
    if !phase.is_empty() {
        json.insert(
            "phase".to_string(),
            serde_json::Value::String(phase.to_string()),
        );
    }
    if !conditions.is_empty() {
        json.insert(
            "conditions".to_string(),
            serde_json::Value::Array(conditions.iter().map(condition_to_json).collect()),
        );
    }
    json
}

fn sandbox_lease_to_json(lease: &resources_proto::SandboxLease) -> serde_json::Value {
    let mut json = serde_json::Map::new();
    if !lease.owner_kind.is_empty() {
        json.insert(
            "ownerKind".to_string(),
            serde_json::Value::String(lease.owner_kind.clone()),
        );
    }
    if !lease.owner_agent.is_empty() {
        json.insert(
            "ownerAgent".to_string(),
            serde_json::Value::String(lease.owner_agent.clone()),
        );
    }
    if !lease.owner_session_id.is_empty() {
        json.insert(
            "ownerSessionId".to_string(),
            serde_json::Value::String(lease.owner_session_id.clone()),
        );
    }
    if !lease.token.is_empty() {
        json.insert(
            "token".to_string(),
            serde_json::Value::String(lease.token.clone()),
        );
    }
    if lease.acquired_at != 0 {
        json.insert(
            "acquiredAt".to_string(),
            serde_json::Value::Number(lease.acquired_at.into()),
        );
    }
    if lease.expires_at != 0 {
        json.insert(
            "expiresAt".to_string(),
            serde_json::Value::Number(lease.expires_at.into()),
        );
    }
    if lease.heartbeat_at != 0 {
        json.insert(
            "heartbeatAt".to_string(),
            serde_json::Value::Number(lease.heartbeat_at.into()),
        );
    }
    serde_json::Value::Object(json)
}

fn sandbox_process_status_to_json(
    process: &resources_proto::SandboxProcessStatus,
) -> serde_json::Value {
    let mut json = serde_json::Map::new();
    if !process.id.is_empty() {
        json.insert(
            "id".to_string(),
            serde_json::Value::String(process.id.clone()),
        );
    }
    if !process.command.is_empty() {
        json.insert(
            "command".to_string(),
            serde_json::Value::String(process.command.clone()),
        );
    }
    if !process.args.is_empty() {
        json.insert(
            "args".to_string(),
            serde_json::Value::Array(
                process
                    .args
                    .iter()
                    .map(|arg| serde_json::Value::String(arg.clone()))
                    .collect(),
            ),
        );
    }
    if !process.protocol.is_empty() {
        json.insert(
            "protocol".to_string(),
            serde_json::Value::String(process.protocol.clone()),
        );
    }
    if !process.phase.is_empty() {
        json.insert(
            "phase".to_string(),
            serde_json::Value::String(process.phase.clone()),
        );
    }
    serde_json::Value::Object(json)
}

fn conditions_from_value(value: &serde_json::Value) -> Vec<resources_proto::ResourceCondition> {
    value
        .get("conditions")
        .and_then(|value| value.as_array())
        .map(|conditions| {
            conditions
                .iter()
                .map(|condition| resources_proto::ResourceCondition {
                    r#type: condition
                        .get("type")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    status: condition
                        .get("status")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    reason: condition
                        .get("reason")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    message: condition
                        .get("message")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    last_transition_time: condition
                        .get("lastTransitionTime")
                        .and_then(|value| value.as_i64())
                        .unwrap_or_default(),
                    observed_generation: condition
                        .get("observedGeneration")
                        .and_then(|value| value.as_u64())
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn condition_to_json(condition: &resources_proto::ResourceCondition) -> serde_json::Value {
    let mut json = serde_json::Map::new();
    if !condition.r#type.is_empty() {
        json.insert(
            "type".to_string(),
            serde_json::Value::String(condition.r#type.clone()),
        );
    }
    if !condition.status.is_empty() {
        json.insert(
            "status".to_string(),
            serde_json::Value::String(condition.status.clone()),
        );
    }
    if !condition.reason.is_empty() {
        json.insert(
            "reason".to_string(),
            serde_json::Value::String(condition.reason.clone()),
        );
    }
    if !condition.message.is_empty() {
        json.insert(
            "message".to_string(),
            serde_json::Value::String(condition.message.clone()),
        );
    }
    if condition.last_transition_time != 0 {
        json.insert(
            "lastTransitionTime".to_string(),
            serde_json::Value::Number(condition.last_transition_time.into()),
        );
    }
    if condition.observed_generation != 0 {
        json.insert(
            "observedGeneration".to_string(),
            serde_json::Value::Number(condition.observed_generation.into()),
        );
    }
    serde_json::Value::Object(json)
}

fn resource_ref_from_value(value: &serde_json::Value) -> Result<resources_proto::ResourceRef> {
    Ok(resources_proto::ResourceRef {
        namespace: value
            .get("namespace")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        name: value
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

fn resource_ref_json(reference: &resources_proto::ResourceRef) -> serde_json::Value {
    serde_json::json!({
        "namespace": reference.namespace,
        "name": reference.name,
    })
}

fn replica_counts_json(counts: &resources_proto::DeploymentReplicaCounts) -> serde_json::Value {
    serde_json::json!({
        "desired": counts.desired,
        "updated": counts.updated,
        "ready": counts.ready,
        "pending": counts.pending,
        "degraded": counts.degraded,
    })
}
