// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

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
