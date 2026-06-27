// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

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
    fn into_proto(self) -> manifests::McpServerSpec {
        manifests::McpServerSpec {
            transport: self.transport,
            target: self.target,
            args: self.args,
            headers: self.headers,
            disabled: self.disabled,
            auth_broker: self.auth_broker.map(|spec| manifests::McpAuthBrokerSpec {
                kind: spec.kind,
                url: spec.url,
                cache_ttl_seconds: spec.cache_ttl_seconds,
                audience: spec.audience,
            }),
            policy: self.policy.map(McpServerPolicyManifest::into_proto),
        }
    }

    fn from_proto(spec: &manifests::McpServerSpec) -> Self {
        Self {
            transport: spec.transport.clone(),
            target: spec.target.clone(),
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
            policy: spec
                .policy
                .as_ref()
                .map(McpServerPolicyManifest::from_proto),
        }
    }
}

impl McpServerPolicyManifest {
    fn into_proto(self) -> manifests::McpServerPolicy {
        manifests::McpServerPolicy {
            tools: self.tools.map(McpToolPolicyManifest::into_proto),
        }
    }

    fn from_proto(policy: &manifests::McpServerPolicy) -> Self {
        Self {
            tools: policy.tools.as_ref().map(McpToolPolicyManifest::from_proto),
        }
    }
}

impl McpToolPolicyManifest {
    fn into_proto(self) -> manifests::McpToolPolicy {
        manifests::McpToolPolicy {
            allowlist: self.allowlist,
        }
    }

    fn from_proto(policy: &manifests::McpToolPolicy) -> Self {
        Self {
            allowlist: policy.allowlist.clone(),
        }
    }
}
