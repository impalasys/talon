// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

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
