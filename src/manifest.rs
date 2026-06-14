// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::gateway::rpc::{
    manifests, models,
    protobuf_value::{value, ListValue, Value},
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
struct AgentTemplateManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    definition: AgentDefinitionManifest,
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentDefinitionManifest {
    #[serde(default)]
    custom_spec: Option<AgentSpecManifest>,
    #[serde(default)]
    templated: Option<TemplatedAgentSpecManifest>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TemplatedAgentSpecManifest {
    template_name: String,
    #[serde(default)]
    delta: AgentSpecDeltaManifest,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AgentSpecDeltaManifest {
    #[serde(default)]
    model_policy: Option<ModelPolicyDeltaManifest>,
    #[serde(default)]
    system_prompt: Option<PromptDeltaManifest>,
    #[serde(default)]
    features: Option<FeatureSetDeltaManifest>,
    #[serde(default)]
    mcp_server_refs: Option<StringListDeltaManifest>,
    #[serde(default)]
    capabilities: Option<CapabilitiesPolicyDeltaManifest>,
    #[serde(default)]
    a2a: Option<A2AManifest>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptDeltaManifest {
    #[serde(default)]
    replace: Option<String>,
    #[serde(default)]
    prepend: Option<String>,
    #[serde(default)]
    append: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct FeatureSetDeltaManifest {
    upsert: Vec<FeatureManifest>,
    remove: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct StringListDeltaManifest {
    replace: Vec<String>,
    add: Vec<String>,
    remove: Vec<String>,
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
}

type CapabilitiesPolicyManifest = HashMap<String, Vec<String>>;

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct CapabilitiesPolicyDeltaManifest {
    replace: Option<CapabilitiesPolicyManifest>,
}

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

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ModelPolicyDeltaManifest {
    upsert: Vec<ModelProfileManifest>,
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
    definition: AgentDefinitionManifest,
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
    auth: Option<AgentCardAuthManifest>,
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
struct AgentCardAuthManifest {
    discovery: String,
    operations: String,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentYaml<'a> {
    name: &'a str,
    ns: &'a str,
    definition: AgentDefinitionYaml,
    effective_spec: AgentSpecManifest,
    template_deps: &'a [String],
    labels: &'a HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Public parse API
// ---------------------------------------------------------------------------

pub fn parse_agent_template(yaml: &str) -> Result<manifests::AgentTemplate> {
    let template: AgentTemplateManifest =
        serde_yaml::from_str(yaml).context("Failed to parse AgentTemplate YAML")?;

    if template.kind != "AgentTemplate" {
        bail!("Expected kind 'AgentTemplate', got '{}'", template.kind);
    }

    Ok(manifests::AgentTemplate {
        api_version: template.api_version,
        kind: template.kind,
        metadata: Some(template.metadata.into_proto()),
        definition: Some(template.definition.into_proto()?),
    })
}

pub fn parse_mcp_server(yaml: &str) -> Result<manifests::McpServer> {
    let server: McpServerManifest =
        serde_yaml::from_str(yaml).context("Failed to parse MCPServer YAML")?;

    if server.kind != "McpServer" {
        bail!("Expected kind 'McpServer', got '{}'", server.kind);
    }

    Ok(manifests::McpServer {
        api_version: server.api_version,
        kind: server.kind,
        metadata: Some(server.metadata.into_proto()),
        spec: Some(manifests::McpServerSpec {
            transport: server.spec.transport,
            target: server.spec.target,
            args: server.spec.args,
            headers: server.spec.headers,
            disabled: server.spec.disabled,
        }),
    })
}

pub fn parse_agent(yaml: &str) -> Result<models::Agent> {
    let agent: AgentManifest = serde_yaml::from_str(yaml).context("Failed to parse Agent YAML")?;

    if agent.kind != "Agent" {
        bail!("Expected kind 'Agent', got '{}'", agent.kind);
    }
    if agent.metadata.namespace.trim().is_empty() {
        bail!("Agent metadata.namespace is required");
    }

    Ok(models::Agent {
        name: agent.metadata.name,
        ns: agent.metadata.namespace,
        definition: Some(agent.definition.into_proto()?),
        effective_spec: None,
        template_deps: Vec::new(),
        labels: agent.metadata.labels,
    })
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
        api_version: binding.api_version,
        kind: binding.kind,
        metadata: Some(binding.metadata.into_proto()),
        spec: Some(binding.spec.into_proto()),
    })
}

pub fn parse_namespace(yaml: &str) -> Result<models::Namespace> {
    let namespace: NamespaceManifest =
        serde_yaml::from_str(yaml).context("Failed to parse Namespace YAML")?;

    if namespace.kind != "Namespace" {
        bail!("Expected kind 'Namespace', got '{}'", namespace.kind);
    }
    if !namespace.metadata.namespace.trim().is_empty() {
        bail!("Namespace metadata.namespace must be empty");
    }

    Ok(models::Namespace {
        name: namespace.metadata.name,
        parent: String::new(),
        is_deleted: false,
        deleted_at: 0,
        labels: namespace.metadata.labels,
    })
}

pub fn parse_knowledge(yaml: &str) -> Result<manifests::Knowledge> {
    let knowledge: KnowledgeManifest =
        serde_yaml::from_str(yaml).context("Failed to parse Knowledge YAML")?;

    if knowledge.kind != "Knowledge" {
        bail!("Expected kind 'Knowledge', got '{}'", knowledge.kind);
    }

    Ok(manifests::Knowledge {
        api_version: knowledge.api_version,
        kind: knowledge.kind,
        metadata: Some(knowledge.metadata.into_proto()),
        spec: Some(manifests::KnowledgeSpec {
            path: knowledge.spec.path,
            content: knowledge.spec.content,
        }),
    })
}

pub fn parse_channel(yaml: &str) -> Result<models::Channel> {
    let channel: ChannelManifest =
        serde_yaml::from_str(yaml).context("Failed to parse Channel YAML")?;

    if channel.kind != "Channel" {
        bail!("Expected kind 'Channel', got '{}'", channel.kind);
    }
    if channel.metadata.namespace.trim().is_empty() {
        bail!("Channel metadata.namespace is required");
    }

    Ok(models::Channel {
        name: channel.metadata.name,
        ns: channel.metadata.namespace,
        title: channel.spec.title,
        status: if channel.spec.status.is_empty() {
            "open".to_string()
        } else {
            channel.spec.status
        },
        created_at: 0,
        updated_at: 0,
        metadata: channel.spec.metadata,
        labels: channel.metadata.labels,
    })
}

pub fn parse_channel_subscription(yaml: &str) -> Result<models::ChannelSubscription> {
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

    Ok(models::ChannelSubscription {
        name: subscription.metadata.name,
        ns: subscription.metadata.namespace,
        channel: subscription.spec.channel,
        agent: subscription.spec.agent,
        enabled: subscription.spec.enabled,
        trigger: subscription.spec.trigger,
        reply_mode: subscription.spec.reply_mode,
        context_policy: subscription.spec.context_policy.map(|policy| {
            models::ChannelContextPolicy {
                mode: policy.mode,
                max_messages: policy.max_messages,
            }
        }),
        metadata: subscription.spec.metadata,
        labels: subscription.metadata.labels,
    })
}

pub fn parse_workflow(yaml: &str) -> Result<models::Workflow> {
    let workflow: WorkflowManifest =
        serde_yaml::from_str(yaml).context("Failed to parse Workflow YAML")?;

    if workflow.kind != "Workflow" {
        bail!("Expected kind 'Workflow', got '{}'", workflow.kind);
    }
    if workflow.metadata.namespace.trim().is_empty() {
        bail!("Workflow metadata.namespace is required");
    }

    let workflow = models::Workflow {
        name: workflow.metadata.name,
        ns: workflow.metadata.namespace,
        spec: Some(workflow.spec.into_proto()?),
        labels: workflow.metadata.labels,
    };
    crate::workflows::validate_workflow(&workflow)?;
    Ok(workflow)
}

pub fn render_agent_template_yaml(template: &manifests::AgentTemplate) -> Result<String> {
    let metadata = template
        .metadata
        .as_ref()
        .ok_or_else(|| anyhow!("AgentTemplate missing metadata"))?;
    let definition = template
        .definition
        .as_ref()
        .ok_or_else(|| anyhow!("AgentTemplate missing definition"))?;

    let yaml_template = AgentTemplateManifest {
        api_version: template.api_version.clone(),
        kind: template.kind.clone(),
        metadata: ObjectMetaManifest::from_proto(metadata),
        definition: AgentDefinitionManifest::from_proto(definition)?,
    };

    serde_yaml::to_string(&yaml_template).context("Failed to serialize AgentTemplate to YAML")
}

pub fn render_agent_yaml(agent: &models::Agent) -> Result<String> {
    let definition = agent
        .definition
        .as_ref()
        .ok_or_else(|| anyhow!("Agent missing definition"))?;

    let yaml_agent = AgentManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "Agent".to_string(),
        metadata: ObjectMetaManifest {
            name: agent.name.clone(),
            namespace: agent.ns.clone(),
            labels: agent.labels.clone(),
            annotations: HashMap::new(),
        },
        definition: AgentDefinitionManifest::from_proto(definition)?,
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
        api_version: server.api_version.clone(),
        kind: server.kind.clone(),
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
        api_version: binding.api_version.clone(),
        kind: binding.kind.clone(),
        metadata: ObjectMetaManifest::from_proto(metadata),
        spec: McpServerBindingSpecManifest::from_proto(spec),
    };

    serde_yaml::to_string(&yaml_binding).context("Failed to serialize McpServerBinding to YAML")
}

pub fn render_namespace_yaml(namespace: &models::Namespace) -> Result<String> {
    let yaml_namespace = NamespaceManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "Namespace".to_string(),
        metadata: ObjectMetaManifest {
            name: namespace.name.clone(),
            namespace: String::new(),
            labels: namespace.labels.clone(),
            annotations: HashMap::new(),
        },
    };

    serde_yaml::to_string(&yaml_namespace).context("Failed to serialize Namespace to YAML")
}

pub fn render_agent_json(agent: &models::Agent) -> Result<serde_json::Value> {
    let definition = agent
        .definition
        .as_ref()
        .ok_or_else(|| anyhow!("Agent missing definition"))?;
    let effective_spec = agent
        .effective_spec
        .as_ref()
        .ok_or_else(|| anyhow!("Agent missing effective_spec"))?;

    let json_agent = AgentYaml {
        name: &agent.name,
        ns: &agent.ns,
        definition: AgentDefinitionYaml::from_proto(definition)?,
        effective_spec: AgentSpecManifest::from_proto(effective_spec),
        template_deps: &agent.template_deps,
        labels: &agent.labels,
    };

    serde_json::to_value(&json_agent).context("Failed to serialize Agent to JSON")
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
        api_version: knowledge.api_version.clone(),
        kind: knowledge.kind.clone(),
        metadata: ObjectMetaManifest::from_proto(metadata),
        spec: KnowledgeSpecManifest {
            path: spec.path.clone(),
            content: spec.content.clone(),
        },
    };

    serde_yaml::to_string(&yaml_knowledge).context("Failed to serialize Knowledge to YAML")
}

pub fn render_channel_yaml(channel: &models::Channel) -> Result<String> {
    let yaml_channel = ChannelManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "Channel".to_string(),
        metadata: ObjectMetaManifest {
            name: channel.name.clone(),
            namespace: channel.ns.clone(),
            labels: channel.labels.clone(),
            annotations: HashMap::new(),
        },
        spec: ChannelSpecManifest {
            title: channel.title.clone(),
            status: channel.status.clone(),
            metadata: channel.metadata.clone(),
        },
    };

    serde_yaml::to_string(&yaml_channel).context("Failed to serialize Channel to YAML")
}

pub fn render_channel_subscription_yaml(
    subscription: &models::ChannelSubscription,
) -> Result<String> {
    let yaml_subscription = ChannelSubscriptionManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "ChannelSubscription".to_string(),
        metadata: ObjectMetaManifest {
            name: subscription.name.clone(),
            namespace: subscription.ns.clone(),
            labels: subscription.labels.clone(),
            annotations: HashMap::new(),
        },
        spec: ChannelSubscriptionSpecManifest {
            channel: subscription.channel.clone(),
            agent: subscription.agent.clone(),
            enabled: subscription.enabled,
            trigger: subscription.trigger.clone(),
            reply_mode: subscription.reply_mode.clone(),
            context_policy: subscription.context_policy.as_ref().map(|policy| {
                ChannelContextPolicyManifest {
                    mode: policy.mode.clone(),
                    max_messages: policy.max_messages,
                }
            }),
            metadata: subscription.metadata.clone(),
        },
    };

    serde_yaml::to_string(&yaml_subscription)
        .context("Failed to serialize ChannelSubscription to YAML")
}

pub fn render_workflow_yaml(workflow: &models::Workflow) -> Result<String> {
    let spec = workflow
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("Workflow missing spec"))?;
    let yaml_workflow = WorkflowManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "Workflow".to_string(),
        metadata: ObjectMetaManifest {
            name: workflow.name.clone(),
            namespace: workflow.ns.clone(),
            labels: workflow.labels.clone(),
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
            auth: self.auth.map(AgentCardAuthManifest::into_proto),
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
            auth: spec.auth.as_ref().map(AgentCardAuthManifest::from_proto),
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

impl AgentCardAuthManifest {
    fn into_proto(self) -> manifests::AgentCardAuth {
        manifests::AgentCardAuth {
            discovery: self.discovery,
            operations: self.operations,
        }
    }

    fn from_proto(auth: &manifests::AgentCardAuth) -> Self {
        Self {
            discovery: auth.discovery.clone(),
            operations: auth.operations.clone(),
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
    fn into_proto(self) -> Result<models::WorkflowSpec> {
        Ok(models::WorkflowSpec {
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

    fn from_proto(spec: &models::WorkflowSpec) -> Result<Self> {
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
    fn into_proto(self) -> Result<models::WorkflowStep> {
        Ok(models::WorkflowStep {
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

    fn from_proto(step: &models::WorkflowStep) -> Result<Self> {
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
    fn into_proto(self) -> Result<models::WorkflowStepOutputPolicy> {
        Ok(models::WorkflowStepOutputPolicy {
            format: self.format,
            schema_json: yaml_value_to_json_string(self.schema)?,
        })
    }

    fn from_proto(policy: &models::WorkflowStepOutputPolicy) -> Result<Self> {
        Ok(Self {
            format: policy.format.clone(),
            schema: json_string_to_yaml_value(&policy.schema_json)?,
        })
    }
}

impl WorkflowStepRetryPolicyManifest {
    fn into_proto(self) -> Result<models::WorkflowStepRetryPolicy> {
        Ok(models::WorkflowStepRetryPolicy {
            max_attempts: self.max_attempts,
            initial_backoff_seconds: self.initial_backoff_seconds,
            max_backoff_seconds: self.max_backoff_seconds,
            multiplier: self.multiplier,
        })
    }

    fn from_proto(policy: &models::WorkflowStepRetryPolicy) -> Self {
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

impl AgentDefinitionManifest {
    fn into_proto(self) -> Result<manifests::AgentDefinition> {
        match (self.custom_spec, self.templated) {
            (Some(spec), None) => Ok(manifests::AgentDefinition {
                source: Some(manifests::agent_definition::Source::CustomSpec(
                    spec.into_proto()?,
                )),
            }),
            (None, Some(templated)) => Ok(manifests::AgentDefinition {
                source: Some(manifests::agent_definition::Source::Templated(
                    templated.into_proto()?,
                )),
            }),
            (Some(_), Some(_)) => {
                bail!("AgentDefinition must set only one of customSpec or templated")
            }
            (None, None) => bail!("AgentDefinition must set one of customSpec or templated"),
        }
    }

    fn from_proto(definition: &manifests::AgentDefinition) -> Result<Self> {
        match definition.source.as_ref() {
            Some(manifests::agent_definition::Source::CustomSpec(spec)) => Ok(Self {
                custom_spec: Some(AgentSpecManifest::from_proto(spec)),
                templated: None,
            }),
            Some(manifests::agent_definition::Source::Templated(templated)) => Ok(Self {
                custom_spec: None,
                templated: Some(TemplatedAgentSpecManifest::from_proto(templated)?),
            }),
            None => bail!("AgentDefinition missing source"),
        }
    }
}

impl TemplatedAgentSpecManifest {
    fn into_proto(self) -> Result<manifests::TemplatedAgentSpec> {
        if self.template_name.trim().is_empty() {
            bail!("TemplatedAgentSpec.templateName is required");
        }

        Ok(manifests::TemplatedAgentSpec {
            template_name: self.template_name,
            delta: Some(self.delta.into_proto()?),
        })
    }

    fn from_proto(templated: &manifests::TemplatedAgentSpec) -> Result<Self> {
        Ok(Self {
            template_name: templated.template_name.clone(),
            delta: AgentSpecDeltaManifest::from_proto(templated.delta.as_ref()),
        })
    }
}

impl AgentSpecDeltaManifest {
    fn into_proto(self) -> Result<manifests::AgentSpecDelta> {
        Ok(manifests::AgentSpecDelta {
            model_policy: self.model_policy.map(ModelPolicyDeltaManifest::into_proto),
            system_prompt: self
                .system_prompt
                .map(PromptDeltaManifest::into_proto)
                .transpose()?,
            features: self.features.map(FeatureSetDeltaManifest::into_proto),
            mcp_server_refs: self
                .mcp_server_refs
                .map(StringListDeltaManifest::into_proto),
            capabilities: self
                .capabilities
                .map(CapabilitiesPolicyDeltaManifest::into_proto),
            a2a: self.a2a.map(A2AManifest::into_proto).transpose()?,
        })
    }

    fn from_proto(delta: Option<&manifests::AgentSpecDelta>) -> Self {
        let Some(delta) = delta else {
            return Self::default();
        };

        Self {
            model_policy: delta
                .model_policy
                .as_ref()
                .map(ModelPolicyDeltaManifest::from_proto),
            system_prompt: delta
                .system_prompt
                .as_ref()
                .map(PromptDeltaManifest::from_proto),
            features: delta
                .features
                .as_ref()
                .map(FeatureSetDeltaManifest::from_proto),
            mcp_server_refs: delta
                .mcp_server_refs
                .as_ref()
                .map(StringListDeltaManifest::from_proto),
            capabilities: delta
                .capabilities
                .as_ref()
                .map(CapabilitiesPolicyDeltaManifest::from_proto),
            a2a: delta.a2a.as_ref().map(A2AManifest::from_proto),
        }
    }
}

impl PromptDeltaManifest {
    fn into_proto(self) -> Result<manifests::PromptDelta> {
        let operation = match (self.replace, self.prepend, self.append) {
            (Some(value), None, None) => Some(manifests::prompt_delta::Operation::Replace(value)),
            (None, Some(value), None) => Some(manifests::prompt_delta::Operation::Prepend(value)),
            (None, None, Some(value)) => Some(manifests::prompt_delta::Operation::Append(value)),
            (None, None, None) => None,
            _ => bail!("PromptDelta must set only one of replace, prepend, or append"),
        };

        Ok(manifests::PromptDelta { operation })
    }

    fn from_proto(delta: &manifests::PromptDelta) -> Self {
        match delta.operation.as_ref() {
            Some(manifests::prompt_delta::Operation::Replace(value)) => Self {
                replace: Some(value.clone()),
                prepend: None,
                append: None,
            },
            Some(manifests::prompt_delta::Operation::Prepend(value)) => Self {
                replace: None,
                prepend: Some(value.clone()),
                append: None,
            },
            Some(manifests::prompt_delta::Operation::Append(value)) => Self {
                replace: None,
                prepend: None,
                append: Some(value.clone()),
            },
            None => Self {
                replace: None,
                prepend: None,
                append: None,
            },
        }
    }
}

impl FeatureSetDeltaManifest {
    fn into_proto(self) -> manifests::FeatureSetDelta {
        manifests::FeatureSetDelta {
            upsert: self
                .upsert
                .into_iter()
                .map(FeatureManifest::into_proto)
                .collect(),
            remove: self.remove,
        }
    }

    fn from_proto(delta: &manifests::FeatureSetDelta) -> Self {
        Self {
            upsert: delta
                .upsert
                .iter()
                .map(FeatureManifest::from_proto)
                .collect(),
            remove: delta.remove.clone(),
        }
    }
}

impl StringListDeltaManifest {
    fn into_proto(self) -> manifests::StringListDelta {
        manifests::StringListDelta {
            replace: self.replace,
            add: self.add,
            remove: self.remove,
        }
    }

    fn from_proto(delta: &manifests::StringListDelta) -> Self {
        Self {
            replace: delta.replace.clone(),
            add: delta.add.clone(),
            remove: delta.remove.clone(),
        }
    }
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

impl CapabilitiesPolicyDeltaManifest {
    fn into_proto(self) -> manifests::CapabilitiesPolicyDelta {
        manifests::CapabilitiesPolicyDelta {
            replace: self
                .replace
                .map(capabilities_policy_into_proto)
                .unwrap_or_default(),
        }
    }

    fn from_proto(delta: &manifests::CapabilitiesPolicyDelta) -> Self {
        Self {
            replace: (!delta.replace.is_empty())
                .then(|| capabilities_policy_from_proto(&delta.replace)),
        }
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

impl ModelPolicyDeltaManifest {
    fn into_proto(self) -> manifests::ModelPolicyDelta {
        manifests::ModelPolicyDelta {
            upsert: self
                .upsert
                .into_iter()
                .map(ModelProfileManifest::into_proto)
                .collect(),
        }
    }

    fn from_proto(delta: &manifests::ModelPolicyDelta) -> Self {
        Self {
            upsert: delta
                .upsert
                .iter()
                .map(ModelProfileManifest::from_proto)
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentDefinitionYaml {
    #[serde(skip_serializing_if = "Option::is_none")]
    custom_spec: Option<AgentSpecManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    templated: Option<TemplatedAgentSpecYaml>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TemplatedAgentSpecYaml {
    template_name: String,
    delta: AgentSpecDeltaManifest,
}

impl AgentDefinitionYaml {
    fn from_proto(definition: &manifests::AgentDefinition) -> Result<Self> {
        match definition.source.as_ref() {
            Some(manifests::agent_definition::Source::CustomSpec(spec)) => Ok(Self {
                custom_spec: Some(AgentSpecManifest::from_proto(spec)),
                templated: None,
            }),
            Some(manifests::agent_definition::Source::Templated(templated)) => Ok(Self {
                custom_spec: None,
                templated: Some(TemplatedAgentSpecYaml {
                    template_name: templated.template_name.clone(),
                    delta: AgentSpecDeltaManifest::from_proto(templated.delta.as_ref()),
                }),
            }),
            None => bail!("AgentDefinition missing source"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::rpc::{manifests, models};
    use std::collections::HashMap;

    #[test]
    fn parse_agent_manifest_supports_internal_agent() {
        let agent = parse_agent(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: ctl
  namespace: conic
  labels:
    visibility: internal
definition:
  customSpec:
    systemPrompt: test
    mcpServerRefs:
      - conic
      - talon-ops
"#,
        )
        .expect("agent manifest should parse");

        assert_eq!(agent.name, "ctl");
        assert_eq!(agent.ns, "conic");
        assert_eq!(
            agent.labels.get("visibility").map(String::as_str),
            Some("internal")
        );
        let definition = agent.definition.expect("agent definition should exist");
        let source = definition
            .source
            .expect("agent definition source should exist");
        match source {
            crate::gateway::rpc::manifests::agent_definition::Source::CustomSpec(spec) => {
                assert_eq!(spec.mcp_server_refs, vec!["conic", "talon-ops"]);
            }
            _ => panic!("expected customSpec"),
        }
    }

    #[test]
    fn parse_agent_manifest_supports_a2a_connections() {
        let agent = parse_agent(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: ctl
  namespace: conic
definition:
  customSpec:
    systemPrompt: test
    a2a:
      connections:
        - name: billing_investigator
          description: Investigates billing issues.
          target:
            internal:
              namespace: billing
              agent: invoice-agent
          inputModes:
            - text/plain
          outputModes:
            - text/plain
          timeoutSeconds: 120
          maxDepth: 3
        - name: policy_researcher
          description: Researches policy questions.
          target:
            external:
              agentCardUrl: https://policy.example.com/.well-known/agent-card.json
          auth:
            kind: bearer
            secretRef: policy-token
"#,
        )
        .expect("agent manifest should parse");

        let definition = agent.definition.expect("agent definition should exist");
        match definition
            .source
            .expect("agent definition source should exist")
        {
            crate::gateway::rpc::manifests::agent_definition::Source::CustomSpec(spec) => {
                let a2a = spec.a2a.expect("a2a spec should exist");
                assert_eq!(a2a.connections.len(), 2);
                assert_eq!(a2a.connections[0].name, "billing_investigator");
                assert_eq!(a2a.connections[0].input_modes, vec!["text/plain"]);
                assert_eq!(a2a.connections[0].max_depth, 3);
                match a2a.connections[0]
                    .target
                    .as_ref()
                    .and_then(|target| target.target.as_ref())
                {
                    Some(manifests::connection_ref::Target::Internal(target)) => {
                        assert_eq!(target.namespace, "billing");
                        assert_eq!(target.agent, "invoice-agent");
                    }
                    other => panic!("expected internal target, got {other:?}"),
                }
                match a2a.connections[1]
                    .target
                    .as_ref()
                    .and_then(|target| target.target.as_ref())
                {
                    Some(manifests::connection_ref::Target::External(target)) => {
                        assert_eq!(
                            target.agent_card_url,
                            "https://policy.example.com/.well-known/agent-card.json"
                        );
                    }
                    other => panic!("expected external target, got {other:?}"),
                }
            }
            other => panic!("expected customSpec, got {other:?}"),
        }
    }

    #[test]
    fn parse_and_render_agent_manifest_with_a2a_agent_card() {
        let agent = parse_agent(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: support-docs
  namespace: support
definition:
  customSpec:
    systemPrompt: test
    modelPolicy:
      profiles:
        - name: default
          model:
            provider: test
            name: test
            temperature: 0
    a2a:
      agentCard:
        name: Support Agent
        description: Answers support questions.
        version: 1.0.0
        capabilities:
          streaming: true
          pushNotifications: false
          extendedAgentCard: false
        defaultInputModes:
          - text/plain
        defaultOutputModes:
          - text/plain
        skills:
          - id: answer_support_question
            name: Answer support question
            description: Answers using docs.
            tags:
              - support
"#,
        )
        .expect("Agent manifest should parse");

        let definition = agent.definition.as_ref().expect("definition should exist");
        let agent_card = match definition.source.as_ref().expect("source should exist") {
            manifests::agent_definition::Source::CustomSpec(spec) => spec
                .a2a
                .as_ref()
                .and_then(|a2a| a2a.agent_card.as_ref())
                .expect("agent card should exist"),
            other => panic!("expected customSpec, got {other:?}"),
        };
        assert!(agent_card
            .capabilities
            .as_ref()
            .is_some_and(|caps| caps.streaming));
        assert_eq!(agent_card.skills[0].id, "answer_support_question");

        let rendered = render_agent_yaml(&agent).expect("Agent yaml should render");
        let reparsed = parse_agent(&rendered).expect("rendered agent should parse");
        assert_eq!(reparsed.name, "support-docs");
        assert!(rendered.contains("agentCard:"));
        assert!(rendered.contains("answer_support_question"));
    }

    #[test]
    fn parse_mcp_server_binding_manifest_supports_auth_broker() {
        let binding = parse_mcp_server_binding(
            r#"
apiVersion: talon.impalasys.com/v1
kind: McpServerBinding
metadata:
  name: talon-ops
  namespace: conic
spec:
  serverRef: talon-ops
  authBroker:
    kind: http_bearer
    url: https://worker.example.com/mcp/talon-ops/auth
    cacheTtlSeconds: 3300
    audience: talon-ops
"#,
        )
        .expect("binding manifest should parse");

        let metadata = binding.metadata.expect("binding metadata should exist");
        assert_eq!(metadata.name, "talon-ops");
        assert_eq!(metadata.namespace, "conic");

        let spec = binding.spec.expect("binding spec should exist");
        assert_eq!(spec.server_ref, "talon-ops");
        let broker = spec.auth_broker.expect("auth broker should exist");
        assert_eq!(broker.kind, "http_bearer");
        assert_eq!(broker.url, "https://worker.example.com/mcp/talon-ops/auth");
        assert_eq!(broker.cache_ttl_seconds, 3300);
        assert_eq!(broker.audience, "talon-ops");
    }

    #[test]
    fn parse_namespace_manifest_supports_root_namespace_labels() {
        let namespace = super::parse_namespace(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Namespace
metadata:
  name: conic
  labels:
    visibility: internal
"#,
        )
        .expect("namespace manifest should parse");

        assert_eq!(namespace.name, "conic");
        assert_eq!(
            namespace.labels.get("visibility").map(String::as_str),
            Some("internal")
        );
    }

    #[test]
    fn parse_and_render_channel_manifests() {
        let channel = parse_channel(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Channel
metadata:
  name: incident-123
  namespace: conic
  labels:
    team: platform
spec:
  title: Checkout latency incident
  status: open
  metadata:
    severity: sev2
"#,
        )
        .expect("channel manifest should parse");
        assert_eq!(channel.name, "incident-123");
        assert_eq!(channel.ns, "conic");
        assert_eq!(
            channel.metadata.get("severity").map(String::as_str),
            Some("sev2")
        );

        let rendered = render_channel_yaml(&channel).expect("channel yaml should render");
        let reparsed = parse_channel(&rendered).expect("rendered channel should parse");
        assert_eq!(reparsed.title, "Checkout latency incident");

        let subscription = parse_channel_subscription(
            r#"
apiVersion: talon.impalasys.com/v1
kind: ChannelSubscription
metadata:
  name: incident-researcher
  namespace: conic
spec:
  channel: incident-123
  agent: researcher
  enabled: true
  trigger: mention
  replyMode: none
  contextPolicy:
    mode: recent_public
    maxMessages: 20
"#,
        )
        .expect("channel subscription manifest should parse");
        assert_eq!(subscription.channel, "incident-123");
        assert_eq!(subscription.agent, "researcher");
        assert!(subscription.enabled);
        assert_eq!(subscription.reply_mode, "none");
        assert_eq!(
            subscription
                .context_policy
                .as_ref()
                .map(|policy| policy.max_messages),
            Some(20)
        );

        let rendered = render_channel_subscription_yaml(&subscription)
            .expect("channel subscription yaml should render");
        let reparsed = parse_channel_subscription(&rendered)
            .expect("rendered channel subscription should parse");
        assert_eq!(reparsed.trigger, "mention");
        assert_eq!(reparsed.reply_mode, "none");
    }

    #[test]
    fn parse_agent_template_supports_templated_definition_delta() {
        let template = parse_agent_template(
            r#"
apiVersion: talon.impalasys.com/v1
kind: AgentTemplate
metadata:
  name: assistant
definition:
  templated:
    templateName: base
    delta:
      systemPrompt:
        append: " extra context"
      mcpServerRefs:
        add:
          - talon-ops
"#,
        )
        .expect("template manifest should parse");

        let definition = template
            .definition
            .expect("template definition should exist");
        match definition.source.expect("template source should exist") {
            manifests::agent_definition::Source::Templated(templated) => {
                assert_eq!(templated.template_name, "base");
                let delta = templated.delta.expect("delta should exist");
                assert!(delta.system_prompt.is_some());
                assert!(delta.mcp_server_refs.is_some());
            }
            other => panic!("expected templated definition, got {other:?}"),
        }
    }

    #[test]
    fn parse_agent_rejects_missing_namespace() {
        let error = parse_agent(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: ctl
definition:
  customSpec:
    systemPrompt: test
"#,
        )
        .expect_err("missing namespace should fail");

        assert!(error.to_string().contains("metadata.namespace is required"));
    }

    #[test]
    fn parse_namespace_rejects_nested_namespace_field() {
        let error = parse_namespace(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Namespace
metadata:
  name: child
  namespace: parent
"#,
        )
        .expect_err("namespace metadata.namespace should be empty");

        assert!(error.to_string().contains("must be empty"));
    }

    #[test]
    fn parse_mcp_server_and_knowledge_round_trip() {
        let server = parse_mcp_server(
            r#"
apiVersion: talon.impalasys.com/v1
kind: McpServer
metadata:
  name: github
spec:
  transport: stdio
  target: gh
  args:
    - api
  headers:
    Authorization: Bearer token
  disabled: true
"#,
        )
        .expect("mcp server manifest should parse");

        let spec = server.spec.expect("server spec");
        assert_eq!(spec.transport, "stdio");
        assert_eq!(spec.target, "gh");
        assert_eq!(spec.args, vec!["api"]);
        assert_eq!(
            spec.headers.get("Authorization").map(String::as_str),
            Some("Bearer token")
        );
        assert!(spec.disabled);

        let knowledge = parse_knowledge(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Knowledge
metadata:
  name: handbook
  namespace: conic
spec:
  path: /docs/handbook
  content: hello
"#,
        )
        .expect("knowledge manifest should parse");
        let rendered = render_knowledge_yaml(&knowledge).expect("knowledge yaml should render");

        assert!(rendered.contains("kind: Knowledge"));
        assert!(rendered.contains("path: /docs/handbook"));
        assert!(rendered.contains("content: hello"));
    }

    #[test]
    fn render_agent_template_yaml_round_trips_custom_spec() {
        let template = manifests::AgentTemplate {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "AgentTemplate".to_string(),
            metadata: Some(manifests::ObjectMeta {
                name: "assistant".to_string(),
                namespace: String::new(),
                labels: std::collections::HashMap::from([(
                    "visibility".to_string(),
                    "internal".to_string(),
                )]),
                annotations: std::collections::HashMap::new(),
            }),
            definition: Some(manifests::AgentDefinition {
                source: Some(manifests::agent_definition::Source::CustomSpec(
                    manifests::AgentSpec {
                        features: vec![manifests::Feature {
                            name: "search".to_string(),
                            r#type: "builtin".to_string(),
                            required: true,
                        }],
                        model_policy: Some(manifests::ModelPolicy {
                            profiles: vec![manifests::ModelProfile {
                                name: "default".to_string(),
                                model: Some(manifests::Model {
                                    provider: "openai".to_string(),
                                    name: "gpt-4.1".to_string(),
                                    temperature: 0.2,
                                    thinking: None,
                                }),
                            }],
                        }),
                        system_prompt: "be useful".to_string(),
                        mcp_server_refs: vec!["talon-ops".to_string()],
                        capabilities: std::collections::HashMap::new(),
                        a2a: None,
                    },
                )),
            }),
        };

        let rendered = render_agent_template_yaml(&template).expect("template yaml should render");
        let reparsed = parse_agent_template(&rendered).expect("rendered template should parse");
        let reparsed_meta = reparsed.metadata.expect("metadata should exist");

        assert_eq!(reparsed_meta.name, "assistant");
        assert_eq!(
            reparsed_meta.labels.get("visibility").map(String::as_str),
            Some("internal")
        );
    }

    #[test]
    fn render_agent_yaml_round_trips_manifest_and_json_includes_runtime_fields() {
        let agent = models::Agent {
            name: "ctl".to_string(),
            ns: "conic".to_string(),
            definition: Some(manifests::AgentDefinition {
                source: Some(manifests::agent_definition::Source::Templated(
                    manifests::TemplatedAgentSpec {
                        template_name: "assistant".to_string(),
                        delta: Some(manifests::AgentSpecDelta {
                            model_policy: None,
                            system_prompt: Some(manifests::PromptDelta {
                                operation: Some(manifests::prompt_delta::Operation::Append(
                                    " extra".to_string(),
                                )),
                            }),
                            features: None,
                            mcp_server_refs: None,
                            capabilities: None,
                            a2a: None,
                        }),
                    },
                )),
            }),
            effective_spec: Some(manifests::AgentSpec {
                features: vec![manifests::Feature {
                    name: "search".to_string(),
                    r#type: "builtin".to_string(),
                    required: false,
                }],
                model_policy: Some(manifests::ModelPolicy {
                    profiles: vec![manifests::ModelProfile {
                        name: "default".to_string(),
                        model: Some(manifests::Model {
                            provider: "openai".to_string(),
                            name: "gpt-4.1".to_string(),
                            temperature: 0.1,
                            thinking: None,
                        }),
                    }],
                }),
                system_prompt: "test".to_string(),
                mcp_server_refs: vec!["talon-ops".to_string()],
                capabilities: std::collections::HashMap::from([(
                    "schedules".to_string(),
                    crate::gateway::rpc::protobuf_value::ListValue {
                        values: vec![crate::gateway::rpc::protobuf_value::Value {
                            kind: Some(
                                crate::gateway::rpc::protobuf_value::value::Kind::StringValue(
                                    "read".to_string(),
                                ),
                            ),
                        }],
                    },
                )]),
                a2a: None,
            }),
            template_deps: vec!["assistant".to_string()],
            labels: std::collections::HashMap::from([(
                "visibility".to_string(),
                "internal".to_string(),
            )]),
        };

        let yaml = render_agent_yaml(&agent).expect("agent yaml should render");
        let json = render_agent_json(&agent).expect("agent json should render");
        let reparsed = parse_agent(&yaml).expect("rendered agent manifest should parse");

        assert!(yaml.contains("apiVersion: talon.impalasys.com/v1"));
        assert!(yaml.contains("kind: Agent"));
        assert!(yaml.contains("namespace: conic"));
        assert!(yaml.contains("templateName: assistant"));
        assert!(yaml.contains("append: ' extra'"));
        assert_eq!(reparsed.name, "ctl");
        assert_eq!(reparsed.ns, "conic");
        assert_eq!(json["name"], "ctl");
        assert_eq!(json["ns"], "conic");
        assert_eq!(json["effectiveSpec"]["systemPrompt"], "test");
        assert_eq!(json["templateDeps"][0], "assistant");
        assert_eq!(json["labels"]["visibility"], "internal");
    }

    #[test]
    fn render_helpers_require_mandatory_fields() {
        let missing_definition = models::Agent {
            name: "ctl".to_string(),
            ns: "conic".to_string(),
            definition: None,
            effective_spec: None,
            template_deps: Vec::new(),
            labels: std::collections::HashMap::new(),
        };
        assert!(render_agent_yaml(&missing_definition).is_err());
        assert!(render_agent_json(&missing_definition).is_err());

        let missing_knowledge = manifests::Knowledge {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Knowledge".to_string(),
            metadata: None,
            spec: None,
        };
        assert!(render_knowledge_yaml(&missing_knowledge).is_err());
    }

    #[test]
    fn parse_agent_template_rejects_conflicting_definition_sources() {
        let error = parse_agent_template(
            r#"
apiVersion: talon.impalasys.com/v1
kind: AgentTemplate
metadata:
  name: assistant
definition:
  customSpec:
    systemPrompt: test
  templated:
    templateName: base
"#,
        )
        .expect_err("conflicting definition sources should fail");

        assert!(error
            .to_string()
            .contains("must set only one of customSpec or templated"));
    }

    #[test]
    fn parse_helpers_reject_wrong_kinds_and_missing_required_sources() {
        let wrong_agent = parse_agent(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Namespace
metadata:
  name: ctl
  namespace: conic
definition:
  customSpec:
    systemPrompt: hi
"#,
        )
        .expect_err("wrong kind should fail");
        assert!(wrong_agent.to_string().contains("Expected kind 'Agent'"));

        let wrong_server = parse_mcp_server(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: github
spec:
  transport: streamable_http
  target: https://example.com
"#,
        )
        .expect_err("wrong kind should fail");
        assert!(wrong_server
            .to_string()
            .contains("Expected kind 'McpServer'"));

        let wrong_binding = parse_mcp_server_binding(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: github
  namespace: conic
spec:
  serverRef: github
"#,
        )
        .expect_err("wrong kind should fail");
        assert!(wrong_binding
            .to_string()
            .contains("Expected kind 'McpServerBinding'"));

        let wrong_knowledge = parse_knowledge(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: notes
spec:
  path: docs/a
  content: hi
"#,
        )
        .expect_err("wrong kind should fail");
        assert!(wrong_knowledge
            .to_string()
            .contains("Expected kind 'Knowledge'"));

        let missing_definition = parse_agent_template(
            r#"
apiVersion: talon.impalasys.com/v1
kind: AgentTemplate
metadata:
  name: assistant
definition: {}
"#,
        )
        .expect_err("missing definition source should fail");
        assert!(missing_definition
            .to_string()
            .contains("must set one of customSpec or templated"));
    }

    #[test]
    fn templated_and_prompt_delta_manifests_validate_remaining_branches() {
        let empty_template_name = TemplatedAgentSpecManifest {
            template_name: "   ".to_string(),
            delta: AgentSpecDeltaManifest::default(),
        }
        .into_proto()
        .expect_err("blank template name should fail");
        assert!(empty_template_name
            .to_string()
            .contains("templateName is required"));

        let prompt_conflict = PromptDeltaManifest {
            replace: Some("a".to_string()),
            prepend: Some("b".to_string()),
            append: None,
        }
        .into_proto()
        .expect_err("multiple prompt delta operations should fail");
        assert!(prompt_conflict
            .to_string()
            .contains("set only one of replace, prepend, or append"));

        let prompt_none = PromptDeltaManifest {
            replace: None,
            prepend: None,
            append: None,
        }
        .into_proto()
        .expect("empty prompt delta should still serialize");
        assert!(prompt_none.operation.is_none());
    }

    #[test]
    fn capabilities_policy_and_agent_spec_round_trip_non_string_actions() {
        let proto = capabilities_policy_into_proto(HashMap::from([(
            "sessions".to_string(),
            vec!["inspect".to_string(), "read:messages".to_string()],
        )]));
        let mut with_non_string = proto.clone();
        with_non_string.insert(
            "schedules".to_string(),
            ListValue {
                values: vec![Value {
                    kind: Some(value::Kind::NumberValue(1.0)),
                }],
            },
        );

        let manifest = capabilities_policy_from_proto(&with_non_string);
        assert_eq!(
            manifest.get("sessions").cloned(),
            Some(vec!["inspect".to_string(), "read:messages".to_string()])
        );
        assert_eq!(manifest.get("schedules").cloned(), Some(Vec::new()));

        let delta = CapabilitiesPolicyDeltaManifest {
            replace: Some(HashMap::from([(
                "sessions".to_string(),
                vec!["inspect".to_string()],
            )])),
        };
        let round_trip = CapabilitiesPolicyDeltaManifest::from_proto(&delta.into_proto());
        assert_eq!(
            round_trip
                .replace
                .as_ref()
                .and_then(|m| m.get("sessions"))
                .cloned(),
            Some(vec!["inspect".to_string()])
        );

        let spec = AgentSpecManifest {
            features: vec![FeatureManifest {
                name: "search".to_string(),
                type_name: "builtin".to_string(),
                required: false,
            }],
            model_policy: Some(ModelPolicyManifest {
                profiles: vec![ModelProfileManifest {
                    name: "default".to_string(),
                    model: ModelManifest {
                        provider: "openai".to_string(),
                        name: "gpt-5".to_string(),
                        temperature: 0.2,
                    },
                }],
            }),
            system_prompt: "Base".to_string(),
            mcp_server_refs: vec!["github".to_string()],
            capabilities: Some(HashMap::from([(
                "sessions".to_string(),
                vec!["inspect".to_string()],
            )])),
            a2a: None,
        };
        let proto = spec.into_proto().expect("spec should convert");
        let round_trip = AgentSpecManifest::from_proto(&proto);
        assert_eq!(
            round_trip
                .capabilities
                .as_ref()
                .and_then(|m| m.get("sessions"))
                .cloned(),
            Some(vec!["inspect".to_string()])
        );
    }

    #[test]
    fn render_helpers_require_proto_sources_for_agent_template() {
        let missing_definition_source = render_agent_template_yaml(&manifests::AgentTemplate {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "AgentTemplate".to_string(),
            metadata: Some(manifests::ObjectMeta {
                name: "assistant".to_string(),
                namespace: String::new(),
                labels: HashMap::new(),
                annotations: HashMap::new(),
            }),
            definition: Some(manifests::AgentDefinition { source: None }),
        })
        .expect_err("missing proto source should fail");
        assert!(missing_definition_source
            .to_string()
            .contains("AgentDefinition missing source"));

        let missing_metadata = render_agent_template_yaml(&manifests::AgentTemplate {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "AgentTemplate".to_string(),
            metadata: None,
            definition: Some(manifests::AgentDefinition {
                source: Some(manifests::agent_definition::Source::CustomSpec(
                    manifests::AgentSpec::default(),
                )),
            }),
        })
        .expect_err("missing metadata should fail");
        assert!(missing_metadata.to_string().contains("missing metadata"));
    }

    #[test]
    fn proto_conversion_helpers_cover_missing_defaults_and_errors() {
        let err = AgentDefinitionManifest {
            custom_spec: None,
            templated: None,
        }
        .into_proto()
        .unwrap_err();
        assert!(err.to_string().contains("must set one"));

        let err = AgentDefinitionManifest::from_proto(&manifests::AgentDefinition { source: None })
            .unwrap_err();
        assert!(err.to_string().contains("missing source"));

        let err = TemplatedAgentSpecManifest {
            template_name: " ".to_string(),
            delta: AgentSpecDeltaManifest::default(),
        }
        .into_proto()
        .unwrap_err();
        assert!(err.to_string().contains("templateName is required"));

        let templated = TemplatedAgentSpecManifest::from_proto(&manifests::TemplatedAgentSpec {
            template_name: "template".to_string(),
            delta: None,
        })
        .expect("templated proto should deserialize");
        assert_eq!(templated.template_name, "template");
        assert!(templated.delta.model_policy.is_none());
        assert!(templated.delta.system_prompt.is_none());
        assert!(templated.delta.features.is_none());
        assert!(templated.delta.mcp_server_refs.is_none());
        assert!(templated.delta.capabilities.is_none());

        let yaml_err =
            AgentDefinitionYaml::from_proto(&manifests::AgentDefinition { source: None })
                .unwrap_err();
        assert!(yaml_err.to_string().contains("missing source"));

        let profile = ModelProfileManifest::from_proto(&manifests::ModelProfile {
            name: "blank".to_string(),
            model: None,
        });
        assert_eq!(profile.name, "blank");
        assert_eq!(profile.model.provider, "");
        assert_eq!(profile.model.name, "");
        assert_eq!(profile.model.temperature, 0.0);

        let spec = AgentSpecManifest::from_proto(&manifests::AgentSpec {
            features: vec![],
            model_policy: None,
            system_prompt: "prompt".to_string(),
            mcp_server_refs: vec!["server".to_string()],
            capabilities: HashMap::new(),
            a2a: None,
        });
        assert!(spec.capabilities.is_none());
        assert_eq!(spec.system_prompt, "prompt");
        assert_eq!(spec.mcp_server_refs, vec!["server".to_string()]);
    }

    #[test]
    fn prompt_and_capability_helpers_cover_all_proto_shapes() {
        let prepend = PromptDeltaManifest::from_proto(&manifests::PromptDelta {
            operation: Some(manifests::prompt_delta::Operation::Prepend(
                "before".to_string(),
            )),
        });
        assert_eq!(prepend.prepend.as_deref(), Some("before"));
        assert!(prepend.replace.is_none());
        assert!(prepend.append.is_none());

        let append = PromptDeltaManifest::from_proto(&manifests::PromptDelta {
            operation: Some(manifests::prompt_delta::Operation::Append(
                "after".to_string(),
            )),
        });
        assert_eq!(append.append.as_deref(), Some("after"));
        assert!(append.replace.is_none());
        assert!(append.prepend.is_none());

        let replace =
            CapabilitiesPolicyDeltaManifest::from_proto(&manifests::CapabilitiesPolicyDelta {
                replace: HashMap::from([(
                    "tools".to_string(),
                    ListValue {
                        values: vec![
                            Value {
                                kind: Some(value::Kind::StringValue("read".to_string())),
                            },
                            Value {
                                kind: Some(value::Kind::BoolValue(true)),
                            },
                        ],
                    },
                )]),
            });
        assert_eq!(
            replace.replace,
            Some(HashMap::from([(
                "tools".to_string(),
                vec!["read".to_string()],
            )]))
        );

        let empty =
            CapabilitiesPolicyDeltaManifest::from_proto(&manifests::CapabilitiesPolicyDelta {
                replace: HashMap::new(),
            });
        assert!(empty.replace.is_none());
    }

    #[test]
    fn parse_and_render_workflow_manifest_supports_dag_syntax() {
        let workflow = parse_workflow(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Workflow
metadata:
  name: retention-review
  namespace: customer-retention
  labels:
    app: retention
spec:
  description: Review an account.
  inputSchema:
    type: object
    required: [accountId]
    properties:
      accountId:
        type: string
  outputSchema:
    type: object
    properties:
      summary:
        type: string
  steps:
    - id: review
      type: transform
      input:
        summary: Review ${$.input.accountId}
    - id: approval
      type: pause
      after: [review]
      when:
        path: $.steps.review.output.summary
        contains: acct_
      prompt: Approve ${$.input.accountId}?
      resumeSchema:
        type: object
        required: [approved]
        properties:
          approved:
            type: boolean
    - id: timedWait
      type: wait
      after: [approval]
      duration: 5m
      timeout: 1h
      retry:
        maxAttempts: 3
        initialBackoffSeconds: 1
        maxBackoffSeconds: 30
        multiplier: 2.0
    - id: defaultRetry
      type: wait
      after: [timedWait]
      duration: 1s
      retry: {}
  output:
    summary: ${$.steps.review.output.summary}
"#,
        )
        .expect("workflow manifest should parse");

        assert_eq!(workflow.name, "retention-review");
        assert_eq!(workflow.ns, "customer-retention");
        let spec = workflow.spec.as_ref().expect("spec should be present");
        assert_eq!(spec.steps.len(), 4);
        assert_eq!(spec.steps[1].after, vec!["review".to_string()]);
        assert_eq!(spec.steps[2].wait_duration, "5m");
        assert_eq!(spec.steps[2].timeout, "1h");
        assert_eq!(spec.steps[2].retry.as_ref().unwrap().max_attempts, 3);
        let default_retry = spec.steps[3].retry.as_ref().unwrap();
        assert_eq!(default_retry.max_attempts, 3);
        assert_eq!(default_retry.initial_backoff_seconds, 1);
        assert_eq!(default_retry.max_backoff_seconds, 30);
        assert_eq!(default_retry.multiplier, 2.0);

        let rendered = render_workflow_yaml(&workflow).expect("workflow should render");
        assert!(rendered.contains("kind: Workflow"));
        assert!(rendered.contains("retention-review"));
        assert!(rendered.contains("duration: 5m"));
        assert!(rendered.contains("maxAttempts: 3"));
    }

    #[test]
    fn parse_workflow_rejects_invalid_workflow_shapes() {
        let base = |steps: &str| {
            format!(
                r#"
apiVersion: talon.impalasys.com/v1
kind: Workflow
metadata:
  name: invalid
  namespace: default
spec:
  steps:
{steps}
  output: {{}}
"#
            )
        };

        let duplicate = parse_workflow(&base(
            r#"
    - id: same
      type: transform
    - id: same
      type: transform
"#,
        ))
        .unwrap_err();
        assert!(duplicate.to_string().contains("duplicate workflow step id"));

        let unknown_dep = parse_workflow(&base(
            r#"
    - id: second
      type: transform
      after: [missing]
"#,
        ))
        .unwrap_err();
        assert!(unknown_dep.to_string().contains("depends on unknown step"));

        let unsupported = parse_workflow(&base(
            r#"
    - id: nope
      type: foreach
"#,
        ))
        .unwrap_err();
        assert!(unsupported
            .to_string()
            .contains("unsupported workflow step type"));

        let invalid_predicate = parse_workflow(&base(
            r#"
    - id: badWhen
      type: transform
      when:
        path: $.input.flag
"#,
        ))
        .unwrap_err();
        assert!(invalid_predicate
            .to_string()
            .contains("predicate must set exactly one comparator"));

        let invalid_schema = parse_workflow(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Workflow
metadata:
  name: invalid-schema
  namespace: default
spec:
  inputSchema: []
  steps:
    - id: ok
      type: transform
  output: {}
"#,
        )
        .unwrap_err();
        assert!(invalid_schema
            .to_string()
            .contains("inputSchema must be a JSON object"));
    }
}
