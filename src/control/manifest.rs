// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::control::resource_model::{self, ChannelResourceExt, TypedResource};
use crate::gateway::rpc::{
    manifests,
    protobuf_value::{value, ListValue, Value},
    resources_proto,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawManifest {
    pub api_version: String,
    pub kind: String,
    pub metadata: serde_yaml::Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceYamlDocument {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    #[serde(default)]
    spec: serde_yaml::Value,
    #[serde(default, skip_serializing_if = "is_empty_yaml_value")]
    status: serde_yaml::Value,
}

fn is_empty_yaml_value(value: &serde_yaml::Value) -> bool {
    match value {
        serde_yaml::Value::Null => true,
        serde_yaml::Value::Mapping(mapping) => mapping.is_empty(),
        _ => false,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesiredResourceManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    #[serde(default)]
    spec: serde_yaml::Value,
    status: Option<serde_yaml::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObjectMetaManifest {
    name: String,
    #[serde(default)]
    namespace: String,
    #[serde(default)]
    labels: HashMap<String, String>,
    #[serde(default)]
    annotations: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct AgentSpecManifest {
    features: Vec<FeatureManifest>,
    model_policy: Option<ModelPolicyManifest>,
    system_prompt: String,
    mcp_server_refs: Vec<String>,
    capabilities: Option<CapabilitiesPolicyManifest>,
    a2a: Option<A2AManifest>,
    runtime: Option<AgentRuntimeManifest>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct AgentRuntimeManifest {
    kind: String,
    acp: Option<AcpRuntimeManifest>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct AcpRuntimeManifest {
    harness_ref: String,
    command: String,
    args: Vec<String>,
    cwd: String,
    sandbox_policy_ref: String,
    persist_session: bool,
    env: HashMap<String, String>,
    permission_policy: HashMap<String, String>,
}

type CapabilitiesPolicyManifest = HashMap<String, Vec<String>>;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FeatureManifest {
    name: String,
    #[serde(rename = "type")]
    type_name: String,
    #[serde(default)]
    required: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelManifest {
    provider: String,
    name: String,
    temperature: f32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelProfileManifest {
    name: String,
    model: ModelManifest,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ModelPolicyManifest {
    profiles: Vec<ModelProfileManifest>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpServerManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    spec: McpServerSpecManifest,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct McpServerSpecManifest {
    transport: String,
    target: String,
    args: Vec<String>,
    headers: HashMap<String, String>,
    disabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    spec: AgentSpecManifest,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpServerBindingManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    spec: McpServerBindingSpecManifest,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct McpServerBindingSpecManifest {
    server_ref: String,
    args: Vec<String>,
    headers: HashMap<String, String>,
    disabled: bool,
    auth_broker: Option<McpAuthBrokerSpecManifest>,
    allowed_tool_names: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct A2AManifest {
    connections: Vec<ConnectionManifest>,
    agent_card: Option<AgentCardManifest>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ConnectionManifest {
    name: String,
    description: String,
    target: ConnectionRefManifest,
    input_modes: Vec<String>,
    output_modes: Vec<String>,
    timeout_seconds: u32,
    max_depth: u32,
    auth: Option<ConnectionAuthManifest>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ConnectionRefManifest {
    internal: Option<InternalConnectionRefManifest>,
    external: Option<ExternalConnectionRefManifest>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct InternalConnectionRefManifest {
    namespace: String,
    agent: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ExternalConnectionRefManifest {
    agent_card_url: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ConnectionAuthManifest {
    kind: String,
    secret_ref: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct AgentCardManifest {
    name: String,
    description: String,
    version: String,
    capabilities: Option<AgentCardCapabilitiesManifest>,
    default_input_modes: Vec<String>,
    default_output_modes: Vec<String>,
    skills: Vec<AgentCardSkillManifest>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct AgentCardCapabilitiesManifest {
    streaming: bool,
    push_notifications: bool,
    extended_agent_card: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct AgentCardSkillManifest {
    id: String,
    name: String,
    description: String,
    tags: Vec<String>,
    examples: Vec<String>,
    input_modes: Vec<String>,
    output_modes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct McpAuthBrokerSpecManifest {
    kind: String,
    url: String,
    cache_ttl_seconds: i32,
    audience: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NamespaceManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KnowledgeManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    spec: KnowledgeSpecManifest,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct KnowledgeSpecManifest {
    path: String,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    #[serde(default)]
    spec: ChannelSpecManifest,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ChannelSpecManifest {
    title: String,
    status: String,
    metadata: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelSubscriptionManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    spec: ChannelSubscriptionSpecManifest,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ChannelSubscriptionSpecManifest {
    channel: String,
    agent: String,
    enabled: bool,
    trigger: String,
    reply_mode: String,
    context_policy: Option<ChannelContextPolicyManifest>,
    metadata: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ChannelContextPolicyManifest {
    mode: String,
    max_messages: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    spec: WorkflowSpecManifest,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct WorkflowSpecManifest {
    description: String,
    input_schema: serde_yaml::Value,
    output_schema: serde_yaml::Value,
    steps: Vec<WorkflowStepManifest>,
    output: serde_yaml::Value,
    concurrency: u32,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct WorkflowStepManifest {
    id: String,
    #[serde(rename = "type")]
    type_name: String,
    after: Vec<String>,
    when: serde_yaml::Value,
    agent: String,
    prompt: String,
    tool: String,
    input: serde_yaml::Value,
    workflow: String,
    output: Option<WorkflowStepOutputPolicyManifest>,
    resume_schema: serde_yaml::Value,
    retry: Option<WorkflowStepRetryPolicyManifest>,
    timeout: String,
    duration: String,
    until: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct WorkflowStepOutputPolicyManifest {
    format: String,
    schema: serde_yaml::Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct WorkflowStepRetryPolicyManifest {
    #[serde(default = "default_workflow_retry_max_attempts")]
    max_attempts: u32,
    #[serde(default = "default_workflow_retry_initial_backoff_seconds")]
    initial_backoff_seconds: i64,
    #[serde(default = "default_workflow_retry_max_backoff_seconds")]
    max_backoff_seconds: i64,
    #[serde(default = "default_workflow_retry_multiplier")]
    multiplier: f64,
}

fn default_workflow_retry_max_attempts() -> u32 {
    3
}

fn default_workflow_retry_initial_backoff_seconds() -> i64 {
    1
}

fn default_workflow_retry_max_backoff_seconds() -> i64 {
    30
}

fn default_workflow_retry_multiplier() -> f64 {
    2.0
}

impl Default for WorkflowStepRetryPolicyManifest {
    fn default() -> Self {
        Self {
            max_attempts: default_workflow_retry_max_attempts(),
            initial_backoff_seconds: default_workflow_retry_initial_backoff_seconds(),
            max_backoff_seconds: default_workflow_retry_max_backoff_seconds(),
            multiplier: default_workflow_retry_multiplier(),
        }
    }
}

// ---------------------------------------------------------------------------
// Public parse API
// ---------------------------------------------------------------------------

pub fn parse_mcp_server(yaml: &str) -> Result<manifests::McpServer> {
    let server: McpServerManifest =
        serde_yaml::from_str(yaml).context("Failed to parse MCPServer YAML")?;

    if server.kind != "McpServer" {
        bail!("Expected kind 'McpServer', got '{}'", server.kind);
    }

    Ok(manifests::McpServer {
        metadata: Some(server.metadata.into_proto()),
        spec: Some(manifests::McpServerSpec {
            transport: server.spec.transport,
            target: server.spec.target,
            args: server.spec.args,
            headers: server.spec.headers,
            disabled: server.spec.disabled,
        }),
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

pub fn parse_mcp_server_binding(yaml: &str) -> Result<manifests::McpServerBinding> {
    let binding: McpServerBindingManifest =
        serde_yaml::from_str(yaml).context("Failed to parse McpServerBinding YAML")?;

    if binding.kind != "McpServerBinding" {
        bail!("Expected kind 'McpServerBinding', got '{}'", binding.kind);
    }
    if binding.metadata.namespace.trim().is_empty() {
        bail!("McpServerBinding metadata.namespace is required");
    }

    Ok(manifests::McpServerBinding {
        metadata: Some(binding.metadata.into_proto()),
        spec: Some(binding.spec.into_proto()),
        status: Some(resource_model::common_status(String::new())),
    })
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
    let spec_json = non_empty_json_object(yaml_value_to_json_string(manifest.spec)?);
    let (spec, _) = resource_spec_status_from_json(&manifest.kind, &spec_json, "{}")?;
    Ok(resources_proto::ResourceManifest {
        api_version: manifest.api_version,
        kind: manifest.kind,
        metadata: Some(metadata),
        spec: Some(spec),
    })
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
        "Template" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::Template(template_spec_from_value(spec_value)?)),
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
        "Skill" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::Skill(skill_spec_from_value(spec_value)?)),
        },
        "PermissionRequest" => resources_proto::ResourceSpec {
            kind: Some(SpecKind::PermissionRequest(
                permission_request_spec_from_value(spec_value)?,
            )),
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
        "PermissionRequest" => resources_proto::ResourceStatus {
            kind: Some(StatusKind::PermissionRequest(
                permission_request_status_from_value(status_value)?,
            )),
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
        Some(SpecKind::Template(spec)) => serde_json::to_string(&serde_json::json!({
            "kind": spec.kind,
            "metadata": spec.metadata.as_ref().map(ObjectMetaManifest::from_resource_meta),
            "spec": json_string_to_json_value(&spec.spec_json)?,
        }))?,
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
        Some(SpecKind::PermissionRequest(spec)) => serde_json::to_string(&serde_json::json!({
            "agent": spec.agent,
            "sessionId": spec.session_id,
            "action": spec.action,
            "prompt": spec.prompt,
            "payload": json_string_to_json_value(&spec.payload_json)?,
        }))?,
        Some(SpecKind::Skill(spec)) => serde_json::to_string(&serde_json::json!({
            "description": spec.description,
            "instructions": spec.instructions,
        }))?,
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
        Some(StatusKind::PermissionRequest(status)) => {
            let mut json = common_status_map(
                status.observed_generation,
                &status.phase,
                &status.conditions,
            );
            if !status.decision.is_empty() {
                json.insert(
                    "decision".to_string(),
                    serde_json::Value::String(status.decision.clone()),
                );
            }
            if !status.decided_by.is_empty() {
                json.insert(
                    "decidedBy".to_string(),
                    serde_json::Value::String(status.decided_by.clone()),
                );
            }
            if status.decided_at != 0 {
                json.insert(
                    "decidedAt".to_string(),
                    serde_json::Value::Number(status.decided_at.into()),
                );
            }
            serde_json::to_string(&serde_json::Value::Object(json))?
        }
        Some(StatusKind::Template(status))
        | Some(StatusKind::Skill(status))
        | Some(StatusKind::SandboxClass(status))
        | Some(StatusKind::SandboxPolicy(status)) => {
            serde_json::to_string(&common_status_to_json(status))?
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
            .unwrap_or(0) as u32,
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

fn permission_request_spec_from_value(
    value: serde_json::Value,
) -> Result<resources_proto::PermissionRequestSpec> {
    Ok(resources_proto::PermissionRequestSpec {
        agent: value
            .get("agent")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        session_id: value
            .get("sessionId")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        action: value
            .get("action")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        prompt: value
            .get("prompt")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        payload_json: serde_json::to_string(
            value.get("payload").unwrap_or(&serde_json::Value::Null),
        )?,
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

fn permission_request_status_from_value(
    value: serde_json::Value,
) -> Result<resources_proto::PermissionRequestStatus> {
    Ok(resources_proto::PermissionRequestStatus {
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
        decision: value
            .get("decision")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        decided_by: value
            .get("decidedBy")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        decided_at: value
            .get("decidedAt")
            .and_then(|value| value.as_i64())
            .unwrap_or(0),
    })
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

fn common_status_to_json(status: &resources_proto::CommonResourceStatus) -> serde_json::Value {
    serde_json::Value::Object(common_status_map(
        status.observed_generation,
        &status.phase,
        &status.conditions,
    ))
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

pub fn render_agent_yaml(agent: &resources_proto::Agent) -> Result<String> {
    let spec = agent
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("Agent missing spec"))?;

    let yaml_agent = AgentManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "Agent".to_string(),
        metadata: ObjectMetaManifest {
            name: agent.name().to_string(),
            namespace: agent.namespace().to_string(),
            labels: agent.labels().clone(),
            annotations: HashMap::new(),
        },
        spec: AgentSpecManifest::from_proto(spec),
    };

    serde_yaml::to_string(&yaml_agent).context("Failed to serialize Agent to YAML")
}

pub fn render_mcp_server_yaml(server: &manifests::McpServer) -> Result<String> {
    let metadata = server
        .metadata
        .as_ref()
        .ok_or_else(|| anyhow!("MCPServer missing metadata"))?;
    let spec = server
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("MCPServer missing spec"))?;

    let yaml_server = McpServerManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "McpServer".to_string(),
        metadata: ObjectMetaManifest::from_proto(metadata),
        spec: McpServerSpecManifest::from_proto(spec),
    };

    serde_yaml::to_string(&yaml_server).context("Failed to serialize MCPServer to YAML")
}

pub fn render_mcp_server_binding_yaml(binding: &manifests::McpServerBinding) -> Result<String> {
    let metadata = binding
        .metadata
        .as_ref()
        .ok_or_else(|| anyhow!("McpServerBinding missing metadata"))?;
    let spec = binding
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("McpServerBinding missing spec"))?;

    let yaml_binding = McpServerBindingManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "McpServerBinding".to_string(),
        metadata: ObjectMetaManifest::from_proto(metadata),
        spec: McpServerBindingSpecManifest::from_proto(spec),
    };

    serde_yaml::to_string(&yaml_binding).context("Failed to serialize McpServerBinding to YAML")
}

pub fn render_namespace_yaml(namespace: &resources_proto::Namespace) -> Result<String> {
    let yaml_namespace = NamespaceManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "Namespace".to_string(),
        metadata: ObjectMetaManifest {
            name: namespace.name().to_string(),
            namespace: String::new(),
            labels: namespace.labels().clone(),
            annotations: HashMap::new(),
        },
    };

    serde_yaml::to_string(&yaml_namespace).context("Failed to serialize Namespace to YAML")
}

pub fn render_agent_json(agent: &resources_proto::Agent) -> Result<serde_json::Value> {
    let spec = agent
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("Agent missing spec"))?;

    Ok(serde_json::json!({
        "name": agent.name(),
        "ns": agent.namespace(),
        "spec": AgentSpecManifest::from_proto(spec),
        "labels": agent.labels(),
    }))
}

pub fn render_knowledge_yaml(knowledge: &manifests::Knowledge) -> Result<String> {
    let metadata = knowledge
        .metadata
        .as_ref()
        .ok_or_else(|| anyhow!("Knowledge missing metadata"))?;
    let spec = knowledge
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("Knowledge missing spec"))?;

    let yaml_knowledge = KnowledgeManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "Knowledge".to_string(),
        metadata: ObjectMetaManifest::from_proto(metadata),
        spec: KnowledgeSpecManifest {
            path: spec.path.clone(),
            content: spec.content.clone(),
        },
    };

    serde_yaml::to_string(&yaml_knowledge).context("Failed to serialize Knowledge to YAML")
}

pub fn render_channel_yaml(channel: &resources_proto::Channel) -> Result<String> {
    let spec = channel
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("Channel missing spec"))?;
    let yaml_channel = ChannelManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "Channel".to_string(),
        metadata: ObjectMetaManifest {
            name: channel.name().to_string(),
            namespace: channel.namespace().to_string(),
            labels: channel.labels().clone(),
            annotations: HashMap::new(),
        },
        spec: ChannelSpecManifest {
            title: spec.title.clone(),
            status: channel.phase().to_string(),
            metadata: spec.metadata.clone(),
        },
    };

    serde_yaml::to_string(&yaml_channel).context("Failed to serialize Channel to YAML")
}

pub fn render_channel_subscription_yaml(
    subscription: &resources_proto::ChannelSubscription,
) -> Result<String> {
    let spec = subscription
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("ChannelSubscription missing spec"))?;
    let yaml_subscription = ChannelSubscriptionManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "ChannelSubscription".to_string(),
        metadata: ObjectMetaManifest {
            name: subscription.name().to_string(),
            namespace: subscription.namespace().to_string(),
            labels: subscription.labels().clone(),
            annotations: HashMap::new(),
        },
        spec: ChannelSubscriptionSpecManifest {
            channel: spec.channel.clone(),
            agent: spec.agent.clone(),
            enabled: spec.enabled,
            trigger: spec.trigger.clone(),
            reply_mode: spec.reply_mode.clone(),
            context_policy: spec.context_policy.as_ref().map(|policy| {
                ChannelContextPolicyManifest {
                    mode: policy.mode.clone(),
                    max_messages: policy.max_messages,
                }
            }),
            metadata: spec.metadata.clone(),
        },
    };

    serde_yaml::to_string(&yaml_subscription)
        .context("Failed to serialize ChannelSubscription to YAML")
}

pub fn render_workflow_yaml(workflow: &resources_proto::Workflow) -> Result<String> {
    let spec = workflow
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("Workflow missing spec"))?;
    let yaml_workflow = WorkflowManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "Workflow".to_string(),
        metadata: ObjectMetaManifest {
            name: workflow.name().to_string(),
            namespace: workflow.namespace().to_string(),
            labels: workflow.labels().clone(),
            annotations: HashMap::new(),
        },
        spec: WorkflowSpecManifest::from_proto(spec)?,
    };

    serde_yaml::to_string(&yaml_workflow).context("Failed to serialize Workflow to YAML")
}

// ---------------------------------------------------------------------------
// Manifest conversions
// ---------------------------------------------------------------------------

impl ObjectMetaManifest {
    fn into_proto(self) -> manifests::ObjectMeta {
        manifests::ObjectMeta {
            name: self.name,
            namespace: self.namespace,
            labels: self.labels,
            annotations: self.annotations,
            owner_references: Vec::new(),
            finalizers: Vec::new(),
            generation: 0,
            resource_version: String::new(),
            uid: String::new(),
            deletion_timestamp: None,
        }
    }

    fn from_proto(meta: &manifests::ObjectMeta) -> Self {
        Self {
            name: meta.name.clone(),
            namespace: meta.namespace.clone(),
            labels: meta.labels.clone(),
            annotations: meta.annotations.clone(),
        }
    }

    fn into_resource_meta(self) -> resources_proto::ResourceMeta {
        resources_proto::ResourceMeta {
            name: self.name,
            namespace: self.namespace,
            labels: self.labels,
            annotations: self.annotations,
            owner_references: Vec::new(),
            finalizers: Vec::new(),
            generation: 0,
            resource_version: String::new(),
            uid: String::new(),
            deletion_timestamp: None,
        }
    }

    fn from_resource_meta(meta: &resources_proto::ResourceMeta) -> Self {
        Self {
            name: meta.name.clone(),
            namespace: meta.namespace.clone(),
            labels: meta.labels.clone(),
            annotations: meta.annotations.clone(),
        }
    }
}

impl McpServerBindingSpecManifest {
    fn into_proto(self) -> manifests::McpServerBindingSpec {
        manifests::McpServerBindingSpec {
            server_ref: self.server_ref,
            args: self.args,
            headers: self.headers,
            disabled: self.disabled,
            auth_broker: self.auth_broker.map(|spec| manifests::McpAuthBrokerSpec {
                kind: spec.kind,
                url: spec.url,
                cache_ttl_seconds: spec.cache_ttl_seconds,
                audience: spec.audience,
            }),
            allowed_tool_names: self.allowed_tool_names,
        }
    }

    fn from_proto(spec: &manifests::McpServerBindingSpec) -> Self {
        Self {
            server_ref: spec.server_ref.clone(),
            args: spec.args.clone(),
            headers: spec.headers.clone(),
            disabled: spec.disabled,
            auth_broker: spec
                .auth_broker
                .as_ref()
                .map(|broker| McpAuthBrokerSpecManifest {
                    kind: broker.kind.clone(),
                    url: broker.url.clone(),
                    cache_ttl_seconds: broker.cache_ttl_seconds,
                    audience: broker.audience.clone(),
                }),
            allowed_tool_names: spec.allowed_tool_names.clone(),
        }
    }
}

impl A2AManifest {
    fn into_proto(self) -> Result<manifests::A2a> {
        Ok(manifests::A2a {
            connections: self
                .connections
                .into_iter()
                .map(ConnectionManifest::into_proto)
                .collect::<Result<Vec<_>>>()?,
            agent_card: self.agent_card.map(AgentCardManifest::into_proto),
        })
    }

    fn from_proto(spec: &manifests::A2a) -> Self {
        Self {
            connections: spec
                .connections
                .iter()
                .map(ConnectionManifest::from_proto)
                .collect(),
            agent_card: spec.agent_card.as_ref().map(AgentCardManifest::from_proto),
        }
    }
}

impl ConnectionManifest {
    fn into_proto(self) -> Result<manifests::Connection> {
        if self.name.trim().is_empty() {
            bail!("A2A connection name is required");
        }
        Ok(manifests::Connection {
            name: self.name,
            description: self.description,
            target: Some(self.target.into_proto()?),
            input_modes: self.input_modes,
            output_modes: self.output_modes,
            timeout_seconds: self.timeout_seconds,
            max_depth: self.max_depth,
            auth: self.auth.map(ConnectionAuthManifest::into_proto),
        })
    }

    fn from_proto(connection: &manifests::Connection) -> Self {
        Self {
            name: connection.name.clone(),
            description: connection.description.clone(),
            target: connection
                .target
                .as_ref()
                .map(ConnectionRefManifest::from_proto)
                .unwrap_or_default(),
            input_modes: connection.input_modes.clone(),
            output_modes: connection.output_modes.clone(),
            timeout_seconds: connection.timeout_seconds,
            max_depth: connection.max_depth,
            auth: connection
                .auth
                .as_ref()
                .map(ConnectionAuthManifest::from_proto),
        }
    }
}

impl ConnectionRefManifest {
    fn into_proto(self) -> Result<manifests::ConnectionRef> {
        let target = match (self.internal, self.external) {
            (Some(internal), None) => {
                if internal.namespace.trim().is_empty() || internal.agent.trim().is_empty() {
                    bail!("A2A internal target requires namespace and agent");
                }
                Some(manifests::connection_ref::Target::Internal(
                    manifests::InternalConnectionRef {
                        namespace: internal.namespace,
                        agent: internal.agent,
                    },
                ))
            }
            (None, Some(external)) => {
                if external.agent_card_url.trim().is_empty() {
                    bail!("A2A external target requires agentCardUrl");
                }
                Some(manifests::connection_ref::Target::External(
                    manifests::ExternalConnectionRef {
                        agent_card_url: external.agent_card_url,
                    },
                ))
            }
            (Some(_), Some(_)) => bail!("A2A target must set only one of internal or external"),
            (None, None) => bail!("A2A target must set one of internal or external"),
        };
        Ok(manifests::ConnectionRef { target })
    }

    fn from_proto(target: &manifests::ConnectionRef) -> Self {
        match target.target.as_ref() {
            Some(manifests::connection_ref::Target::Internal(internal)) => Self {
                internal: Some(InternalConnectionRefManifest {
                    namespace: internal.namespace.clone(),
                    agent: internal.agent.clone(),
                }),
                external: None,
            },
            Some(manifests::connection_ref::Target::External(external)) => Self {
                internal: None,
                external: Some(ExternalConnectionRefManifest {
                    agent_card_url: external.agent_card_url.clone(),
                }),
            },
            None => Self::default(),
        }
    }
}

impl ConnectionAuthManifest {
    fn into_proto(self) -> manifests::ConnectionAuth {
        manifests::ConnectionAuth {
            kind: self.kind,
            secret_ref: self.secret_ref,
        }
    }

    fn from_proto(auth: &manifests::ConnectionAuth) -> Self {
        Self {
            kind: auth.kind.clone(),
            secret_ref: auth.secret_ref.clone(),
        }
    }
}

impl AgentCardManifest {
    fn into_proto(self) -> manifests::AgentCard {
        manifests::AgentCard {
            name: self.name,
            description: self.description,
            version: self.version,
            capabilities: self
                .capabilities
                .map(AgentCardCapabilitiesManifest::into_proto),
            default_input_modes: self.default_input_modes,
            default_output_modes: self.default_output_modes,
            skills: self
                .skills
                .into_iter()
                .map(AgentCardSkillManifest::into_proto)
                .collect(),
        }
    }

    fn from_proto(spec: &manifests::AgentCard) -> Self {
        Self {
            name: spec.name.clone(),
            description: spec.description.clone(),
            version: spec.version.clone(),
            capabilities: spec
                .capabilities
                .as_ref()
                .map(AgentCardCapabilitiesManifest::from_proto),
            default_input_modes: spec.default_input_modes.clone(),
            default_output_modes: spec.default_output_modes.clone(),
            skills: spec
                .skills
                .iter()
                .map(AgentCardSkillManifest::from_proto)
                .collect(),
        }
    }
}

impl AgentCardCapabilitiesManifest {
    fn into_proto(self) -> manifests::AgentCardCapabilities {
        manifests::AgentCardCapabilities {
            streaming: self.streaming,
            push_notifications: self.push_notifications,
            extended_agent_card: self.extended_agent_card,
        }
    }

    fn from_proto(capabilities: &manifests::AgentCardCapabilities) -> Self {
        Self {
            streaming: capabilities.streaming,
            push_notifications: capabilities.push_notifications,
            extended_agent_card: capabilities.extended_agent_card,
        }
    }
}

impl AgentCardSkillManifest {
    fn into_proto(self) -> manifests::AgentCardSkill {
        manifests::AgentCardSkill {
            id: self.id,
            name: self.name,
            description: self.description,
            tags: self.tags,
            examples: self.examples,
            input_modes: self.input_modes,
            output_modes: self.output_modes,
        }
    }

    fn from_proto(skill: &manifests::AgentCardSkill) -> Self {
        Self {
            id: skill.id.clone(),
            name: skill.name.clone(),
            description: skill.description.clone(),
            tags: skill.tags.clone(),
            examples: skill.examples.clone(),
            input_modes: skill.input_modes.clone(),
            output_modes: skill.output_modes.clone(),
        }
    }
}

impl McpServerSpecManifest {
    fn from_proto(spec: &manifests::McpServerSpec) -> Self {
        Self {
            transport: spec.transport.clone(),
            target: spec.target.clone(),
            args: spec.args.clone(),
            headers: spec.headers.clone(),
            disabled: spec.disabled,
        }
    }
}

impl WorkflowSpecManifest {
    fn into_proto(self) -> Result<resources_proto::WorkflowSpec> {
        Ok(resources_proto::WorkflowSpec {
            description: self.description,
            input_schema_json: yaml_value_to_json_string(self.input_schema)?,
            output_schema_json: yaml_value_to_json_string(self.output_schema)?,
            steps: self
                .steps
                .into_iter()
                .map(WorkflowStepManifest::into_proto)
                .collect::<Result<Vec<_>>>()?,
            output_json: yaml_value_to_json_string(self.output)?,
            concurrency: self.concurrency,
        })
    }

    fn from_proto(spec: &resources_proto::WorkflowSpec) -> Result<Self> {
        Ok(Self {
            description: spec.description.clone(),
            input_schema: json_string_to_yaml_value(&spec.input_schema_json)?,
            output_schema: json_string_to_yaml_value(&spec.output_schema_json)?,
            steps: spec
                .steps
                .iter()
                .map(WorkflowStepManifest::from_proto)
                .collect::<Result<Vec<_>>>()?,
            output: json_string_to_yaml_value(&spec.output_json)?,
            concurrency: spec.concurrency,
        })
    }
}

impl WorkflowStepManifest {
    fn into_proto(self) -> Result<resources_proto::WorkflowStep> {
        Ok(resources_proto::WorkflowStep {
            id: self.id,
            r#type: self.type_name,
            after: self.after,
            when_json: yaml_value_to_json_string(self.when)?,
            agent: self.agent,
            prompt: self.prompt,
            tool: self.tool,
            input_json: yaml_value_to_json_string(self.input)?,
            workflow: self.workflow,
            output: self
                .output
                .map(WorkflowStepOutputPolicyManifest::into_proto)
                .transpose()?,
            resume_schema_json: yaml_value_to_json_string(self.resume_schema)?,
            retry: self
                .retry
                .map(WorkflowStepRetryPolicyManifest::into_proto)
                .transpose()?,
            timeout: self.timeout,
            wait_duration: self.duration,
            wait_until: self.until,
        })
    }

    fn from_proto(step: &resources_proto::WorkflowStep) -> Result<Self> {
        Ok(Self {
            id: step.id.clone(),
            type_name: step.r#type.clone(),
            after: step.after.clone(),
            when: json_string_to_yaml_value(&step.when_json)?,
            agent: step.agent.clone(),
            prompt: step.prompt.clone(),
            tool: step.tool.clone(),
            input: json_string_to_yaml_value(&step.input_json)?,
            workflow: step.workflow.clone(),
            output: step
                .output
                .as_ref()
                .map(WorkflowStepOutputPolicyManifest::from_proto)
                .transpose()?,
            resume_schema: json_string_to_yaml_value(&step.resume_schema_json)?,
            retry: step
                .retry
                .as_ref()
                .map(WorkflowStepRetryPolicyManifest::from_proto),
            timeout: step.timeout.clone(),
            duration: step.wait_duration.clone(),
            until: step.wait_until.clone(),
        })
    }
}

impl WorkflowStepOutputPolicyManifest {
    fn into_proto(self) -> Result<resources_proto::WorkflowStepOutputPolicy> {
        Ok(resources_proto::WorkflowStepOutputPolicy {
            format: self.format,
            schema_json: yaml_value_to_json_string(self.schema)?,
        })
    }

    fn from_proto(policy: &resources_proto::WorkflowStepOutputPolicy) -> Result<Self> {
        Ok(Self {
            format: policy.format.clone(),
            schema: json_string_to_yaml_value(&policy.schema_json)?,
        })
    }
}

impl WorkflowStepRetryPolicyManifest {
    fn into_proto(self) -> Result<resources_proto::WorkflowStepRetryPolicy> {
        Ok(resources_proto::WorkflowStepRetryPolicy {
            max_attempts: self.max_attempts,
            initial_backoff_seconds: self.initial_backoff_seconds,
            max_backoff_seconds: self.max_backoff_seconds,
            multiplier: self.multiplier,
        })
    }

    fn from_proto(policy: &resources_proto::WorkflowStepRetryPolicy) -> Self {
        Self {
            max_attempts: policy.max_attempts,
            initial_backoff_seconds: policy.initial_backoff_seconds,
            max_backoff_seconds: policy.max_backoff_seconds,
            multiplier: policy.multiplier,
        }
    }
}

fn yaml_value_to_json_string(value: serde_yaml::Value) -> Result<String> {
    if matches!(value, serde_yaml::Value::Null) {
        return Ok(String::new());
    }
    let json = serde_json::to_value(value).context("Failed to convert YAML value to JSON")?;
    serde_json::to_string(&json).context("Failed to serialize YAML value as JSON")
}

fn json_string_to_yaml_value(value: &str) -> Result<serde_yaml::Value> {
    if value.trim().is_empty() {
        return Ok(serde_yaml::Value::Null);
    }
    let json: serde_json::Value =
        serde_json::from_str(value).context("Failed to parse stored JSON value")?;
    serde_yaml::to_value(json).context("Failed to convert JSON value to YAML")
}

fn capabilities_policy_into_proto(
    policy: CapabilitiesPolicyManifest,
) -> std::collections::HashMap<String, ListValue> {
    policy
        .into_iter()
        .map(|(name, actions)| {
            (
                name,
                ListValue {
                    values: actions
                        .into_iter()
                        .map(|action| Value {
                            kind: Some(value::Kind::StringValue(action)),
                        })
                        .collect(),
                },
            )
        })
        .collect()
}

fn capabilities_policy_from_proto(
    policy: &std::collections::HashMap<String, ListValue>,
) -> CapabilitiesPolicyManifest {
    policy
        .iter()
        .map(|(name, actions)| {
            (
                name.clone(),
                actions
                    .values
                    .iter()
                    .filter_map(|value| match value.kind.as_ref() {
                        Some(value::Kind::StringValue(action)) => Some(action.clone()),
                        _ => None,
                    })
                    .collect(),
            )
        })
        .collect()
}

pub(crate) mod capabilities_policy_serde {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(crate) fn serialize<S>(
        policy: &std::collections::HashMap<String, ListValue>,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        capabilities_policy_from_proto(policy).serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D>(
        deserializer: D,
    ) -> std::result::Result<std::collections::HashMap<String, ListValue>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let policy = CapabilitiesPolicyManifest::deserialize(deserializer)?;
        Ok(capabilities_policy_into_proto(policy))
    }
}

impl AgentSpecManifest {
    fn into_proto(self) -> Result<manifests::AgentSpec> {
        Ok(manifests::AgentSpec {
            features: self
                .features
                .into_iter()
                .map(FeatureManifest::into_proto)
                .collect(),
            model_policy: self.model_policy.map(ModelPolicyManifest::into_proto),
            system_prompt: self.system_prompt,
            mcp_server_refs: self.mcp_server_refs,
            capabilities: self
                .capabilities
                .map(capabilities_policy_into_proto)
                .unwrap_or_default(),
            a2a: self.a2a.map(A2AManifest::into_proto).transpose()?,
            runtime: self.runtime.map(AgentRuntimeManifest::into_proto),
        })
    }

    fn from_proto(spec: &manifests::AgentSpec) -> Self {
        Self {
            features: spec
                .features
                .iter()
                .map(FeatureManifest::from_proto)
                .collect(),
            model_policy: spec
                .model_policy
                .as_ref()
                .map(ModelPolicyManifest::from_proto),
            system_prompt: spec.system_prompt.clone(),
            mcp_server_refs: spec.mcp_server_refs.clone(),
            capabilities: (!spec.capabilities.is_empty())
                .then(|| capabilities_policy_from_proto(&spec.capabilities)),
            a2a: spec.a2a.as_ref().map(A2AManifest::from_proto),
            runtime: spec.runtime.as_ref().map(AgentRuntimeManifest::from_proto),
        }
    }
}

impl AgentRuntimeManifest {
    fn into_proto(self) -> manifests::AgentRuntime {
        manifests::AgentRuntime {
            kind: self.kind,
            acp: self.acp.map(AcpRuntimeManifest::into_proto),
        }
    }

    fn from_proto(runtime: &manifests::AgentRuntime) -> Self {
        Self {
            kind: runtime.kind.clone(),
            acp: runtime.acp.as_ref().map(AcpRuntimeManifest::from_proto),
        }
    }
}

impl AcpRuntimeManifest {
    fn into_proto(self) -> manifests::AcpRuntime {
        manifests::AcpRuntime {
            harness_ref: self.harness_ref,
            command: self.command,
            args: self.args,
            cwd: self.cwd,
            sandbox_policy_ref: self.sandbox_policy_ref,
            persist_session: self.persist_session,
            env: self.env,
            permission_policy: self.permission_policy,
        }
    }

    fn from_proto(runtime: &manifests::AcpRuntime) -> Self {
        Self {
            harness_ref: runtime.harness_ref.clone(),
            command: runtime.command.clone(),
            args: runtime.args.clone(),
            cwd: runtime.cwd.clone(),
            sandbox_policy_ref: runtime.sandbox_policy_ref.clone(),
            persist_session: runtime.persist_session,
            env: runtime.env.clone(),
            permission_policy: runtime.permission_policy.clone(),
        }
    }
}

impl FeatureManifest {
    fn into_proto(self) -> manifests::Feature {
        manifests::Feature {
            name: self.name,
            r#type: self.type_name,
            required: self.required,
        }
    }

    fn from_proto(feature: &manifests::Feature) -> Self {
        Self {
            name: feature.name.clone(),
            type_name: feature.r#type.clone(),
            required: feature.required,
        }
    }
}

impl ModelManifest {
    fn into_proto(self) -> manifests::Model {
        manifests::Model {
            provider: self.provider,
            name: self.name,
            temperature: self.temperature,
            thinking: None,
        }
    }

    fn from_proto(model: &manifests::Model) -> Self {
        Self {
            provider: model.provider.clone(),
            name: model.name.clone(),
            temperature: model.temperature,
        }
    }
}

impl ModelProfileManifest {
    fn into_proto(self) -> manifests::ModelProfile {
        manifests::ModelProfile {
            name: self.name,
            model: Some(self.model.into_proto()),
        }
    }

    fn from_proto(profile: &manifests::ModelProfile) -> Self {
        Self {
            name: profile.name.clone(),
            model: profile
                .model
                .as_ref()
                .map(ModelManifest::from_proto)
                .unwrap_or(ModelManifest {
                    provider: String::new(),
                    name: String::new(),
                    temperature: 0.0,
                }),
        }
    }
}

impl ModelPolicyManifest {
    fn into_proto(self) -> manifests::ModelPolicy {
        manifests::ModelPolicy {
            profiles: self
                .profiles
                .into_iter()
                .map(ModelProfileManifest::into_proto)
                .collect(),
        }
    }

    fn from_proto(policy: &manifests::ModelPolicy) -> Self {
        Self {
            profiles: policy
                .profiles
                .iter()
                .map(ModelProfileManifest::from_proto)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::rpc::resources_proto::{resource_spec, resource_status};

    #[test]
    fn template_manifest_normalizes_nested_spec_to_json() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Template
metadata:
  name: coding-agent
  namespace: customers
spec:
  kind: Agent
  metadata:
    name: coding
  spec:
    systemPrompt: hello
"#,
        )
        .expect("template manifest parses");

        let Some(resource_spec::Kind::Template(spec)) = manifest.spec.and_then(|spec| spec.kind)
        else {
            panic!("expected Template spec");
        };
        let rendered_spec: serde_json::Value =
            serde_json::from_str(&spec.spec_json).expect("template spec JSON");
        assert_eq!(rendered_spec["systemPrompt"], "hello");
    }

    #[test]
    fn sandbox_class_manifest_normalizes_config_maps_to_json() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: SandboxClass
metadata:
  name: docker-code
  namespace: system
spec:
  provider: docker
  providerConfig:
    image: talon-zed-codex-acp:local
  credentials:
    apiKey:
      source: env
      key: E2B_API_KEY
"#,
        )
        .expect("sandbox class manifest parses");

        let Some(resource_spec::Kind::SandboxClass(spec)) =
            manifest.spec.and_then(|spec| spec.kind)
        else {
            panic!("expected SandboxClass spec");
        };
        let provider_config: serde_json::Value =
            serde_json::from_str(&spec.provider_config_json).expect("provider config JSON");
        let credentials: serde_json::Value =
            serde_json::from_str(&spec.credentials_json).expect("credentials JSON");
        assert_eq!(provider_config["image"], "talon-zed-codex-acp:local");
        assert_eq!(credentials["apiKey"]["key"], "E2B_API_KEY");
    }

    #[test]
    fn sandbox_policy_manifest_accepts_and_renders_template_spec_shape() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: SandboxPolicy
metadata:
  name: coding
  namespace: customers
spec:
  classRef:
    namespace: system
    name: docker-code
  template:
    spec:
      image: talon-zed-codex-acp:local
      workspace:
        mode: customer-repo
        mountPath: /workspace
      filesystem:
        writable:
          - /workspace
      leasePolicy:
        mode: exclusive
  quota:
    maxConcurrent: 5
"#,
        )
        .expect("sandbox policy manifest parses");

        let Some(resource_spec::Kind::SandboxPolicy(spec)) =
            manifest.spec.clone().and_then(|spec| spec.kind)
        else {
            panic!("expected SandboxPolicy spec");
        };
        assert_eq!(spec.max_concurrent, 5);
        let template = spec.template.expect("runtime template");
        assert_eq!(template.image, "talon-zed-codex-acp:local");
        assert_eq!(
            template.workspace.expect("workspace").mount_path,
            "/workspace"
        );

        let rendered = render_resource_yaml(&resources_proto::Resource {
            api_version: manifest.api_version,
            kind: manifest.kind,
            metadata: manifest.metadata,
            spec: manifest.spec,
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resource_status::Kind::SandboxPolicy(Default::default())),
            }),
        })
        .expect("render sandbox policy");
        let rendered_yaml: serde_yaml::Value =
            serde_yaml::from_str(&rendered).expect("rendered YAML parses");
        assert_eq!(
            rendered_yaml["spec"]["template"]["image"].as_str(),
            Some("talon-zed-codex-acp:local")
        );
        assert_eq!(
            rendered_yaml["spec"]["template"]["workspace"]["mountPath"].as_str(),
            Some("/workspace")
        );
        assert!(rendered_yaml.get("status").is_none());
    }

    #[test]
    fn resource_yaml_renders_common_status_conditions_when_present() {
        let rendered = render_resource_yaml(&resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Agent".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "coding".to_string(),
                namespace: "customers:acme".to_string(),
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resource_spec::Kind::Agent(Default::default())),
            }),
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resource_status::Kind::Agent(resources_proto::AgentStatus {
                    observed_generation: 3,
                    phase: "Ready".to_string(),
                    conditions: vec![resources_proto::ResourceCondition {
                        r#type: "Available".to_string(),
                        status: "True".to_string(),
                        reason: "RuntimeReady".to_string(),
                        message: "agent runtime is ready".to_string(),
                        last_transition_time: 42,
                        observed_generation: 3,
                    }],
                    last_session_id: None,
                })),
            }),
        })
        .expect("render resource YAML");

        let rendered_yaml: serde_yaml::Value =
            serde_yaml::from_str(&rendered).expect("rendered YAML parses");
        assert_eq!(
            rendered_yaml["status"]["observedGeneration"].as_u64(),
            Some(3)
        );
        assert_eq!(rendered_yaml["status"]["phase"].as_str(), Some("Ready"));
        assert_eq!(
            rendered_yaml["status"]["conditions"][0]["type"].as_str(),
            Some("Available")
        );
        assert_eq!(
            rendered_yaml["status"]["conditions"][0]["observedGeneration"].as_u64(),
            Some(3)
        );
    }

    #[test]
    fn agent_spec_serde_preserves_capabilities() {
        let spec: resources_proto::AgentSpec = serde_json::from_value(serde_json::json!({
            "capabilities": {
                "schedules": ["inspect", "create"]
            }
        }))
        .expect("deserialize AgentSpec capabilities");
        assert_eq!(spec.capabilities.len(), 1);
        let rendered = serde_json::to_value(&spec).expect("serialize AgentSpec capabilities");
        assert_eq!(
            rendered["capabilities"]["schedules"],
            serde_json::json!(["inspect", "create"])
        );
    }

    #[test]
    fn sandbox_policy_manifest_rejects_system_mount_path() {
        let error = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: SandboxPolicy
metadata:
  name: coding
  namespace: customers
spec:
  classRef:
    namespace: system
    name: docker-code
  template:
    spec:
      workspace:
        mountPath: /etc
"#,
        )
        .expect_err("system mount path should fail");
        assert!(error.to_string().contains("mountPath"));
    }

    #[test]
    fn skill_manifest_uses_typed_spec_and_renders_flat_yaml() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Skill
metadata:
  name: review
  namespace: customers
spec:
  description: Review code
  instructions: Look for regressions.
"#,
        )
        .expect("skill manifest should parse");
        let Some(resource_spec::Kind::Skill(spec)) =
            manifest.spec.clone().and_then(|spec| spec.kind)
        else {
            panic!("expected Skill spec");
        };
        assert_eq!(spec.instructions, "Look for regressions.");

        let rendered = render_resource_yaml(&resources_proto::Resource {
            api_version: manifest.api_version,
            kind: manifest.kind,
            metadata: manifest.metadata,
            spec: manifest.spec,
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resource_status::Kind::Skill(Default::default())),
            }),
        })
        .expect("skill resource should render");

        assert!(rendered.contains("kind: Skill"));
        assert!(rendered.contains("description: Review code"));
        assert!(rendered.contains("instructions: Look for regressions."));
        assert!(!rendered.contains("raw:"));
    }

    #[test]
    fn agent_manifest_rejects_invalid_acp_permission_policy() {
        let error = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: coding
  namespace: customers
spec:
  runtime:
    kind: acp
    acp:
      command: codex-acp
      sandboxPolicyRef: coding
      permissionPolicy:
        filesystemwrite: alllow
"#,
        )
        .expect_err("invalid permission policy should fail");
        assert!(error.to_string().contains("permissionPolicy"));
    }
}
