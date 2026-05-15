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
    let effective_spec = agent
        .effective_spec
        .as_ref()
        .ok_or_else(|| anyhow!("Agent missing effective_spec"))?;

    let yaml_agent = AgentYaml {
        name: &agent.name,
        ns: &agent.ns,
        definition: AgentDefinitionYaml::from_proto(definition)?,
        effective_spec: AgentSpecManifest::from_proto(effective_spec),
        template_deps: &agent.template_deps,
        labels: &agent.labels,
    };

    serde_yaml::to_string(&yaml_agent).context("Failed to serialize Agent to YAML")
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
}

impl AgentDefinitionManifest {
    fn into_proto(self) -> Result<manifests::AgentDefinition> {
        match (self.custom_spec, self.templated) {
            (Some(spec), None) => Ok(manifests::AgentDefinition {
                source: Some(manifests::agent_definition::Source::CustomSpec(
                    spec.into_proto(),
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
            replace: (!delta.replace.is_empty()).then(|| capabilities_policy_from_proto(&delta.replace)),
        }
    }
}

impl AgentSpecManifest {
    fn into_proto(self) -> manifests::AgentSpec {
        manifests::AgentSpec {
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
        }
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
    use super::{parse_agent, parse_mcp_server_binding};

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
    url: https://worker.useconic.com/mcp/talon-ops/auth
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
        assert_eq!(broker.url, "https://worker.useconic.com/mcp/talon-ops/auth");
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
}
