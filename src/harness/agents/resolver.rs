// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashSet;

use anyhow::{anyhow, bail, Context, Result};

use crate::gateway::rpc::{
    manifests,
    protobuf_value::{value::Kind as ProtoValueKind, ListValue},
};

pub fn resolve_agent_spec(spec: manifests::AgentSpec) -> Result<manifests::AgentSpec> {
    validate_agent_spec(&spec)?;
    Ok(spec)
}

pub fn validate_agent_spec(spec: &manifests::AgentSpec) -> Result<()> {
    let model_policy = spec
        .model_policy
        .as_ref()
        .ok_or_else(|| anyhow!("AgentSpec.model_policy is required"))?;
    validate_model_policy(model_policy, "AgentSpec.model_policy")?;

    let mut feature_names = HashSet::new();
    for feature in &spec.features {
        let name = feature.name.trim();
        if name.is_empty() {
            bail!("AgentSpec.features entries must include a non-empty name");
        }
        if !feature_names.insert(name.to_string()) {
            bail!("Duplicate feature '{}' in AgentSpec", name);
        }
    }

    let mut seen_mcp_refs = HashSet::new();
    for mcp_ref in &spec.mcp_server_refs {
        let name = mcp_ref.trim();
        if name.is_empty() {
            bail!("AgentSpec.mcp_server_refs entries must be non-empty");
        }
        if !seen_mcp_refs.insert(name.to_string()) {
            bail!("Duplicate MCP server ref '{}' in AgentSpec", name);
        }
    }

    if !spec.capabilities.is_empty() {
        validate_capabilities_policy(&spec.capabilities, "AgentSpec.capabilities")?;
    }

    if let Some(a2a) = spec.a2a.as_ref() {
        validate_a2a(a2a)?;
    }

    if let Some(runtime) = spec.runtime.as_ref() {
        validate_agent_runtime(runtime)?;
    }

    Ok(())
}

fn validate_agent_runtime(runtime: &manifests::AgentRuntime) -> Result<()> {
    match runtime.kind.as_str() {
        "" | "llm" | "native" => Ok(()),
        "acp" => {
            let acp = runtime
                .acp
                .as_ref()
                .ok_or_else(|| anyhow!("AgentRuntime kind 'acp' requires acp config"))?;
            if acp.command.trim().is_empty() && acp.harness_ref.trim().is_empty() {
                bail!("AgentRuntime.acp requires command or harnessRef");
            }
            if acp.sandbox_policy_ref.trim().is_empty() {
                bail!("AgentRuntime.acp.sandboxPolicyRef is required");
            }
            validate_acp_permission_policy(&acp.permission_policy)?;
            Ok(())
        }
        other => bail!("Unsupported AgentRuntime.kind '{}'", other),
    }
}

fn validate_acp_permission_policy(
    policy: &std::collections::HashMap<String, String>,
) -> Result<()> {
    const ALLOWED_KEYS: &[&str] = &["default", "filesystemRead", "filesystemWrite", "terminal"];
    const ALLOWED_VALUES: &[&str] = &["allow", "ask", "deny"];

    for (key, value) in policy {
        if !ALLOWED_KEYS.contains(&key.as_str()) {
            bail!(
                "AgentRuntime.acp.permissionPolicy contains unsupported key '{}'",
                key
            );
        }
        if !ALLOWED_VALUES.contains(&value.as_str()) {
            bail!(
                "AgentRuntime.acp.permissionPolicy.{} has unsupported value '{}'",
                key,
                value
            );
        }
    }
    Ok(())
}

fn validate_a2a(a2a: &manifests::A2a) -> Result<()> {
    if let Some(agent_card) = a2a.agent_card.as_ref() {
        validate_a2a_agent_card(agent_card)?;
    }

    let mut seen_connections = HashSet::new();
    for connection in &a2a.connections {
        let name = connection.name.trim();
        if name.is_empty() {
            bail!("A2A connection name is required");
        }
        if !seen_connections.insert(name.to_string()) {
            bail!("Duplicate A2A connection '{}'", name);
        }

        let target = connection
            .target
            .as_ref()
            .and_then(|target| target.target.as_ref())
            .ok_or_else(|| anyhow!("A2A connection '{}' must set a target", name))?;
        match target {
            manifests::connection_ref::Target::Internal(internal) => {
                if internal.namespace.trim().is_empty() || internal.agent.trim().is_empty() {
                    bail!(
                        "A2A connection '{}' internal target requires namespace and agent",
                        name
                    );
                }
            }
            manifests::connection_ref::Target::External(external) => {
                let url = external.agent_card_url.trim();
                if url.is_empty() {
                    bail!(
                        "A2A connection '{}' external target requires agent_card_url",
                        name
                    );
                }
                let parsed = url::Url::parse(url).with_context(|| {
                    format!(
                        "A2A connection '{}' external agent_card_url must be an absolute URL",
                        name
                    )
                })?;
                if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
                    bail!(
                        "A2A connection '{}' external agent_card_url must be an http(s) URL with a host",
                        name
                    );
                }
            }
        }

        if let Some(auth) = connection.auth.as_ref() {
            let kind = auth.kind.trim();
            match kind {
                "" | "none" => {
                    if !auth.secret_ref.trim().is_empty() {
                        bail!(
                            "A2A connection '{}' auth.secret_ref requires auth.kind 'bearer'",
                            name
                        );
                    }
                }
                "bearer" => {
                    if auth.secret_ref.trim().is_empty() {
                        bail!("A2A connection '{}' bearer auth requires secret_ref", name);
                    }
                }
                other => bail!(
                    "A2A connection '{}' auth.kind must be 'none' or 'bearer'; got '{}'",
                    name,
                    other
                ),
            }
        }
    }

    Ok(())
}

fn validate_a2a_agent_card(agent_card: &manifests::AgentCard) -> Result<()> {
    if agent_card.name.trim().is_empty() {
        bail!("A2A agentCard name is required");
    }
    if let Some(capabilities) = agent_card.capabilities.as_ref() {
        if capabilities.push_notifications {
            bail!("A2A agentCard capabilities.pushNotifications is not supported yet");
        }
        if capabilities.extended_agent_card {
            bail!("A2A agentCard capabilities.extendedAgentCard is not supported yet");
        }
    }
    Ok(())
}

fn validate_capabilities_policy(
    policy: &std::collections::HashMap<String, ListValue>,
    path: &str,
) -> Result<()> {
    for (capability, actions) in policy {
        let capability_trimmed = capability.trim();
        if capability_trimmed.is_empty() {
            bail!("{path} capability names must be non-empty");
        }
        if capability != capability_trimmed {
            bail!("{path} capability names must be trimmed");
        }
        let capability = capability_trimmed;
        for action in actions
            .values
            .iter()
            .map(|value| match value.kind.as_ref() {
                Some(ProtoValueKind::StringValue(action)) => Ok(action.as_str()),
                _ => bail!("{path}['{capability}'] actions must be strings"),
            })
        {
            let action = action?;
            let action_trimmed = action.trim();
            if action_trimmed.is_empty() {
                bail!("{path}['{capability}'] actions must be non-empty");
            }
            if action_trimmed != action {
                bail!("{path}['{capability}'] actions must be trimmed");
            }
            if !is_allowed_capability_action(capability, action_trimmed) {
                bail!("{path}['{capability}'] contains unsupported action '{action_trimmed}'");
            }
        }
    }
    Ok(())
}

fn is_allowed_capability_action(capability: &str, action: &str) -> bool {
    match capability {
        "schedules" => matches!(
            action,
            "inspect"
                | "create"
                | "update"
                | "delete"
                | "create:new"
                | "create:reuse"
                | "create:fresh"
                | "create:named"
        ),
        "sessions" => matches!(action, "inspect" | "read:messages" | "send_message"),
        "search" => matches!(
            action,
            "workspace" | "sessions" | "knowledge" | "open_result"
        ),
        _ => false,
    }
}

fn validate_model_policy(policy: &manifests::ModelPolicy, path: &str) -> Result<()> {
    let mut seen_profiles = HashSet::new();
    let mut has_default = false;

    for profile in &policy.profiles {
        let name = profile.name.trim();
        if name.is_empty() {
            bail!("{path} entries must include a non-empty name");
        }
        if !seen_profiles.insert(name.to_string()) {
            bail!("Duplicate model profile '{}' in {}", name, path);
        }
        if name == "default" {
            has_default = true;
        }

        let model = profile
            .model
            .as_ref()
            .ok_or_else(|| anyhow!("{path}['{}'].model is required", profile.name))?;
        validate_model(model, format!("{path}['{}'].model", profile.name).as_str())?;
    }

    if !has_default {
        bail!("{path} must include a 'default' profile");
    }

    Ok(())
}

fn validate_model(model: &manifests::Model, path: &str) -> Result<()> {
    if model.provider.trim().is_empty() {
        bail!("{path}.provider is required");
    }
    if model.name.trim().is_empty() {
        bail!("{path}.name is required");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn feature(name: &str, type_: &str) -> manifests::Feature {
        manifests::Feature {
            name: name.to_string(),
            r#type: type_.to_string(),
            required: false,
        }
    }

    fn model_profile(profile_name: &str, model_name: &str) -> manifests::ModelProfile {
        manifests::ModelProfile {
            name: profile_name.to_string(),
            model: Some(manifests::Model {
                provider: "openai".to_string(),
                name: model_name.to_string(),
                temperature: 0.2,
                thinking: None,
            }),
        }
    }

    fn model_policy(profiles: Vec<(&str, &str)>) -> manifests::ModelPolicy {
        manifests::ModelPolicy {
            profiles: profiles
                .into_iter()
                .map(|(profile_name, model_name)| model_profile(profile_name, model_name))
                .collect(),
        }
    }

    fn valid_agent_spec() -> manifests::AgentSpec {
        manifests::AgentSpec {
            features: vec![],
            model_policy: Some(model_policy(vec![("default", "gpt-5")])),
            system_prompt: "Base".to_string(),
            mcp_server_refs: vec![],
            capabilities: HashMap::new(),
            a2a: None,
            runtime: None,
        }
    }

    #[test]
    fn resolve_agent_spec_returns_valid_spec() {
        let spec = valid_agent_spec();
        let resolved = resolve_agent_spec(spec.clone()).expect("spec should validate");
        assert_eq!(resolved.system_prompt, spec.system_prompt);
    }

    #[test]
    fn validate_agent_spec_rejects_duplicate_or_invalid_fields() {
        let duplicate_feature = validate_agent_spec(&manifests::AgentSpec {
            features: vec![feature("search", "builtin"), feature("search", "builtin")],
            model_policy: Some(model_policy(vec![("default", "gpt-5")])),
            system_prompt: "Base".to_string(),
            mcp_server_refs: vec![],
            capabilities: HashMap::new(),
            a2a: None,
            runtime: None,
        })
        .unwrap_err();
        assert!(duplicate_feature.to_string().contains("Duplicate feature"));

        let blank_mcp_ref = validate_agent_spec(&manifests::AgentSpec {
            features: vec![],
            model_policy: Some(model_policy(vec![("default", "gpt-5")])),
            system_prompt: "Base".to_string(),
            mcp_server_refs: vec![" ".to_string()],
            capabilities: HashMap::new(),
            a2a: None,
            runtime: None,
        })
        .unwrap_err();
        assert!(blank_mcp_ref.to_string().contains("must be non-empty"));

        let duplicate_mcp_ref = validate_agent_spec(&manifests::AgentSpec {
            features: vec![],
            model_policy: Some(model_policy(vec![("default", "gpt-5")])),
            system_prompt: "Base".to_string(),
            mcp_server_refs: vec!["github".to_string(), "github".to_string()],
            capabilities: HashMap::new(),
            a2a: None,
            runtime: None,
        })
        .unwrap_err();
        assert!(duplicate_mcp_ref
            .to_string()
            .contains("Duplicate MCP server ref"));
    }

    #[test]
    fn validate_agent_spec_rejects_invalid_a2a_connections() {
        let mut missing_target = valid_agent_spec();
        missing_target.a2a = Some(manifests::A2a {
            connections: vec![manifests::Connection {
                name: "policy".to_string(),
                ..Default::default()
            }],
            agent_card: None,
        });
        let err = validate_agent_spec(&missing_target).unwrap_err();
        assert!(err.to_string().contains("must set a target"));

        let mut invalid_url = valid_agent_spec();
        invalid_url.a2a = Some(manifests::A2a {
            connections: vec![manifests::Connection {
                name: "external".to_string(),
                target: Some(manifests::ConnectionRef {
                    target: Some(manifests::connection_ref::Target::External(
                        manifests::ExternalConnectionRef {
                            agent_card_url: "file:///tmp/agent-card.json".to_string(),
                        },
                    )),
                }),
                ..Default::default()
            }],
            agent_card: None,
        });
        let err = validate_agent_spec(&invalid_url).unwrap_err();
        assert!(err.to_string().contains("http(s) URL"));
    }

    #[test]
    fn validate_model_policy_and_capabilities_reject_invalid_entries() {
        let missing_model = validate_model_policy(
            &manifests::ModelPolicy {
                profiles: vec![manifests::ModelProfile {
                    name: "default".to_string(),
                    model: None,
                }],
            },
            "policy",
        )
        .unwrap_err();
        assert!(missing_model.to_string().contains(".model is required"));

        let duplicate_profile = validate_model_policy(
            &manifests::ModelPolicy {
                profiles: vec![
                    model_profile("default", "gpt-5"),
                    model_profile("default", "gpt-5-mini"),
                ],
            },
            "policy",
        )
        .unwrap_err();
        assert!(duplicate_profile
            .to_string()
            .contains("Duplicate model profile"));

        let invalid_capability = validate_capabilities_policy(
            &HashMap::from([(
                "bad".to_string(),
                ListValue {
                    values: vec![protobuf_string("inspect")],
                },
            )]),
            "caps",
        )
        .unwrap_err();
        assert!(invalid_capability
            .to_string()
            .contains("unsupported action"));
    }

    fn protobuf_string(value: &str) -> crate::gateway::rpc::protobuf_value::Value {
        crate::gateway::rpc::protobuf_value::Value {
            kind: Some(ProtoValueKind::StringValue(value.to_string())),
        }
    }
}
