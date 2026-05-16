// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashSet;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;

use crate::control::{keys, ns, KeyValueStore, ProtoKeyValueStoreExt};
use crate::gateway::rpc::{
    manifests,
    protobuf_value::{value::Kind as ProtoValueKind, ListValue},
};

const MAX_TEMPLATE_DEPTH: usize = 16;

#[derive(Debug)]
pub struct ResolvedAgentDefinition {
    pub effective_spec: manifests::AgentSpec,
    pub template_deps: Vec<String>,
}

#[async_trait]
pub trait AgentTemplateLoader {
    async fn load_template(&self, name: &str) -> Result<Option<manifests::AgentTemplate>>;
}

pub struct KvTemplateLoader<'a> {
    kv: &'a (dyn KeyValueStore + Send + Sync),
}

impl<'a> KvTemplateLoader<'a> {
    pub fn new(kv: &'a (dyn KeyValueStore + Send + Sync)) -> Self {
        Self { kv }
    }
}

#[async_trait]
impl AgentTemplateLoader for KvTemplateLoader<'_> {
    async fn load_template(&self, name: &str) -> Result<Option<manifests::AgentTemplate>> {
        self.kv
            .get_msg::<manifests::AgentTemplate>(ns::TALON_SYSTEM, &keys::agent_template(name))
            .await
    }
}

pub async fn resolve_agent_definition(
    kv: &(dyn KeyValueStore + Send + Sync),
    definition: &manifests::AgentDefinition,
) -> Result<ResolvedAgentDefinition> {
    let loader = KvTemplateLoader::new(kv);
    resolve_agent_definition_with_loader(&loader, definition).await
}

pub async fn resolve_agent_definition_with_loader<L: AgentTemplateLoader + Sync>(
    loader: &L,
    definition: &manifests::AgentDefinition,
) -> Result<ResolvedAgentDefinition> {
    let mut current = definition.clone();
    let mut deltas = Vec::new();
    let mut template_deps = Vec::new();
    let mut seen_templates = HashSet::new();

    let base_spec = loop {
        let source = current
            .source
            .ok_or_else(|| anyhow!("AgentDefinition must provide a source"))?;

        match source {
            manifests::agent_definition::Source::CustomSpec(spec) => break spec,
            manifests::agent_definition::Source::Templated(templated) => {
                let template_name = templated.template_name.trim();
                if template_name.is_empty() {
                    bail!("TemplatedAgentSpec.template_name is required");
                }
                if template_deps.len() >= MAX_TEMPLATE_DEPTH {
                    bail!(
                        "Template inheritance depth exceeded maximum of {}",
                        MAX_TEMPLATE_DEPTH
                    );
                }
                if !seen_templates.insert(template_name.to_string()) {
                    bail!("Template inheritance cycle detected at '{}'", template_name);
                }

                template_deps.push(template_name.to_string());
                deltas.push(templated.delta.unwrap_or_default());

                let template = loader
                    .load_template(template_name)
                    .await?
                    .ok_or_else(|| anyhow!("AgentTemplate '{}' not found", template_name))?;

                current = template.definition.ok_or_else(|| {
                    anyhow!("AgentTemplate '{}' is missing definition", template_name)
                })?;
            }
        }
    };

    let mut effective_spec = base_spec;
    for delta in deltas.iter().rev() {
        apply_agent_spec_delta(&mut effective_spec, delta)?;
    }
    validate_agent_spec(&effective_spec)?;

    Ok(ResolvedAgentDefinition {
        effective_spec,
        template_deps,
    })
}

fn apply_agent_spec_delta(
    spec: &mut manifests::AgentSpec,
    delta: &manifests::AgentSpecDelta,
) -> Result<()> {
    if let Some(model_policy_delta) = &delta.model_policy {
        apply_model_policy_delta(
            spec.model_policy.get_or_insert_with(Default::default),
            model_policy_delta,
        )?;
    }

    if let Some(prompt_delta) = &delta.system_prompt {
        match prompt_delta.operation.as_ref() {
            Some(manifests::prompt_delta::Operation::Replace(replace)) => {
                spec.system_prompt = replace.clone();
            }
            Some(manifests::prompt_delta::Operation::Prepend(prepend)) => {
                spec.system_prompt = format!("{}{}", prepend, spec.system_prompt);
            }
            Some(manifests::prompt_delta::Operation::Append(append)) => {
                spec.system_prompt = format!("{}{}", spec.system_prompt, append);
            }
            None => {}
        }
    }

    if let Some(feature_delta) = &delta.features {
        apply_feature_delta(&mut spec.features, feature_delta)?;
    }

    if let Some(mcp_delta) = &delta.mcp_server_refs {
        apply_string_list_delta(&mut spec.mcp_server_refs, mcp_delta)?;
    }

    if let Some(capabilities_delta) = &delta.capabilities {
        apply_capabilities_policy_delta(&mut spec.capabilities, capabilities_delta)?;
    }

    validate_agent_spec(spec)?;

    Ok(())
}

fn apply_model_policy_delta(
    policy: &mut manifests::ModelPolicy,
    delta: &manifests::ModelPolicyDelta,
) -> Result<()> {
    for profile in &delta.upsert {
        let name = profile.name.trim();
        if name.is_empty() {
            bail!("ModelPolicyDelta.upsert entries must include a non-empty name");
        }
        let model = profile.model.as_ref().ok_or_else(|| {
            anyhow!(
                "ModelPolicyDelta.upsert entry '{}' is missing model",
                profile.name
            )
        })?;
        validate_model(
            model,
            format!("ModelPolicyDelta.upsert['{}']", profile.name).as_str(),
        )?;

        if let Some(existing) = policy.profiles.iter_mut().find(|p| p.name == profile.name) {
            *existing = profile.clone();
        } else {
            policy.profiles.push(profile.clone());
        }
    }

    Ok(())
}

fn apply_feature_delta(
    features: &mut Vec<manifests::Feature>,
    delta: &manifests::FeatureSetDelta,
) -> Result<()> {
    let removals: HashSet<&str> = delta.remove.iter().map(String::as_str).collect();
    features.retain(|feature| !removals.contains(feature.name.as_str()));

    for feature in &delta.upsert {
        if feature.name.trim().is_empty() {
            bail!("Feature upserts must include a non-empty name");
        }

        if let Some(existing) = features.iter_mut().find(|f| f.name == feature.name) {
            *existing = feature.clone();
        } else {
            features.push(feature.clone());
        }
    }

    Ok(())
}

fn apply_string_list_delta(
    values: &mut Vec<String>,
    delta: &manifests::StringListDelta,
) -> Result<()> {
    if !delta.replace.is_empty() && (!delta.add.is_empty() || !delta.remove.is_empty()) {
        bail!("StringListDelta.replace cannot be combined with add/remove");
    }

    if !delta.replace.is_empty() {
        *values = delta.replace.clone();
    }

    if !delta.remove.is_empty() {
        let removals: HashSet<&str> = delta.remove.iter().map(String::as_str).collect();
        values.retain(|value| !removals.contains(value.as_str()));
    }

    for value in &delta.add {
        if !values.iter().any(|existing| existing == value) {
            values.push(value.clone());
        }
    }

    Ok(())
}

fn apply_capabilities_policy_delta(
    policy: &mut std::collections::HashMap<String, ListValue>,
    delta: &manifests::CapabilitiesPolicyDelta,
) -> Result<()> {
    if !delta.replace.is_empty() {
        let replace = &delta.replace;
        validate_capabilities_policy(replace, "CapabilitiesPolicyDelta.replace")?;
        *policy = replace.clone();
    }
    Ok(())
}

fn validate_agent_spec(spec: &manifests::AgentSpec) -> Result<()> {
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
        for action in actions.values.iter().map(|value| match value.kind.as_ref() {
            Some(ProtoValueKind::StringValue(action)) => Ok(action.as_str()),
            _ => bail!("{path}['{capability}'] actions must be strings"),
        }) {
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
        "sessions" => matches!(
            action,
            "inspect" | "read:messages" | "read:steps" | "send_message"
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

    struct MapTemplateLoader {
        templates: HashMap<String, manifests::AgentTemplate>,
    }

    #[async_trait]
    impl AgentTemplateLoader for MapTemplateLoader {
        async fn load_template(&self, name: &str) -> Result<Option<manifests::AgentTemplate>> {
            Ok(self.templates.get(name).cloned())
        }
    }

    fn feature(name: &str, type_: &str) -> manifests::Feature {
        manifests::Feature {
            name: name.to_string(),
            r#type: type_.to_string(),
            required: false,
        }
    }

    fn model(name: &str) -> manifests::Model {
        manifests::Model {
            provider: "openai".to_string(),
            name: name.to_string(),
            temperature: 0.2,
            thinking: None,
        }
    }

    fn model_profile(profile_name: &str, model_name: &str) -> manifests::ModelProfile {
        manifests::ModelProfile {
            name: profile_name.to_string(),
            model: Some(model(model_name)),
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

    fn custom_spec_definition(spec: manifests::AgentSpec) -> manifests::AgentDefinition {
        manifests::AgentDefinition {
            source: Some(manifests::agent_definition::Source::CustomSpec(spec)),
        }
    }

    fn templated_definition(
        template_name: &str,
        delta: manifests::AgentSpecDelta,
    ) -> manifests::AgentDefinition {
        manifests::AgentDefinition {
            source: Some(manifests::agent_definition::Source::Templated(
                manifests::TemplatedAgentSpec {
                    template_name: template_name.to_string(),
                    delta: Some(delta),
                },
            )),
        }
    }

    fn template(name: &str, definition: manifests::AgentDefinition) -> manifests::AgentTemplate {
        manifests::AgentTemplate {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "AgentTemplate".to_string(),
            metadata: Some(manifests::ObjectMeta {
                name: name.to_string(),
                namespace: ns::TALON_SYSTEM.to_string(),
                labels: HashMap::new(),
                annotations: HashMap::new(),
            }),
            definition: Some(definition),
        }
    }

    #[tokio::test]
    async fn resolves_template_inheritance_with_deltas() {
        let base_spec = manifests::AgentSpec {
            features: vec![feature("search", "builtin"), feature("draft", "builtin")],
            model_policy: Some(model_policy(vec![("default", "gpt-5")])),
            system_prompt: "Base".to_string(),
            mcp_server_refs: vec!["conic-api".to_string()],
            capabilities: std::collections::HashMap::new(),
        };

        let mut templates = HashMap::new();
        templates.insert(
            "marketing-base".to_string(),
            template("marketing-base", custom_spec_definition(base_spec)),
        );
        templates.insert(
            "seo-agent".to_string(),
            template(
                "seo-agent",
                templated_definition(
                    "marketing-base",
                    manifests::AgentSpecDelta {
                        model_policy: Some(manifests::ModelPolicyDelta {
                            upsert: vec![model_profile("interactive", "gpt-5-fast")],
                        }),
                        system_prompt: Some(manifests::PromptDelta {
                            operation: Some(manifests::prompt_delta::Operation::Append(
                                "\nFocus on SEO.".to_string(),
                            )),
                        }),
                        features: Some(manifests::FeatureSetDelta {
                            upsert: vec![feature("optimize", "builtin")],
                            remove: vec!["draft".to_string()],
                        }),
                        ..Default::default()
                    },
                ),
            ),
        );

        let loader = MapTemplateLoader { templates };
        let resolved = resolve_agent_definition_with_loader(
            &loader,
            &templated_definition(
                "seo-agent",
                manifests::AgentSpecDelta {
                    system_prompt: Some(manifests::PromptDelta {
                        operation: Some(manifests::prompt_delta::Operation::Append(
                            "\nUse enterprise tone.".to_string(),
                        )),
                    }),
                    mcp_server_refs: Some(manifests::StringListDelta {
                        add: vec!["ahrefs".to_string()],
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
        )
        .await
        .unwrap();

        assert_eq!(resolved.template_deps, vec!["seo-agent", "marketing-base"]);
        assert_eq!(
            resolved.effective_spec.system_prompt,
            "Base\nFocus on SEO.\nUse enterprise tone."
        );
        let profiles = resolved
            .effective_spec
            .model_policy
            .as_ref()
            .unwrap()
            .profiles
            .iter()
            .map(|profile| {
                (
                    profile.name.as_str(),
                    profile.model.as_ref().unwrap().name.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            profiles,
            vec![("default", "gpt-5"), ("interactive", "gpt-5-fast")]
        );
        assert_eq!(
            resolved.effective_spec.mcp_server_refs,
            vec!["conic-api", "ahrefs"]
        );
        assert_eq!(
            resolved
                .effective_spec
                .features
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            vec!["search", "optimize"]
        );
    }

    #[tokio::test]
    async fn empty_delta_behaves_like_template_pointer() {
        let loader = MapTemplateLoader {
            templates: HashMap::from([(
                "marketing-base".to_string(),
                template(
                    "marketing-base",
                    custom_spec_definition(manifests::AgentSpec {
                        features: vec![],
                        model_policy: Some(model_policy(vec![("default", "gpt-5-mini")])),
                        system_prompt: "Base".to_string(),
                        mcp_server_refs: vec!["conic-api".to_string()],
                        capabilities: std::collections::HashMap::new(),
                    }),
                ),
            )]),
        };

        let resolved = resolve_agent_definition_with_loader(
            &loader,
            &templated_definition("marketing-base", manifests::AgentSpecDelta::default()),
        )
        .await
        .unwrap();

        assert_eq!(resolved.effective_spec.system_prompt, "Base");
        assert_eq!(resolved.effective_spec.mcp_server_refs, vec!["conic-api"]);
    }

    #[tokio::test]
    async fn detects_template_cycles() {
        let loader = MapTemplateLoader {
            templates: HashMap::from([
                (
                    "a".to_string(),
                    template(
                        "a",
                        templated_definition("b", manifests::AgentSpecDelta::default()),
                    ),
                ),
                (
                    "b".to_string(),
                    template(
                        "b",
                        templated_definition("a", manifests::AgentSpecDelta::default()),
                    ),
                ),
            ]),
        };

        let err = resolve_agent_definition_with_loader(
            &loader,
            &templated_definition("a", manifests::AgentSpecDelta::default()),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("cycle"));
    }

    #[tokio::test]
    async fn errors_on_missing_template() {
        let loader = MapTemplateLoader {
            templates: HashMap::new(),
        };

        let err = resolve_agent_definition_with_loader(
            &loader,
            &templated_definition("missing", manifests::AgentSpecDelta::default()),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn validates_string_list_delta_usage() {
        let mut spec = manifests::AgentSpec {
            features: vec![],
            model_policy: Some(model_policy(vec![("default", "gpt-5")])),
            system_prompt: "Base".to_string(),
            mcp_server_refs: vec!["conic-api".to_string()],
            capabilities: std::collections::HashMap::new(),
        };

        let err = apply_agent_spec_delta(
            &mut spec,
            &manifests::AgentSpecDelta {
                mcp_server_refs: Some(manifests::StringListDelta {
                    replace: vec!["alt".to_string()],
                    add: vec!["another".to_string()],
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("cannot be combined"));
    }

    #[tokio::test]
    async fn model_policy_delta_overwrites_and_adds_profiles() {
        let resolved = resolve_agent_definition_with_loader(
            &MapTemplateLoader {
                templates: HashMap::new(),
            },
            &custom_spec_definition(manifests::AgentSpec {
                features: vec![],
                model_policy: Some(model_policy(vec![
                    ("default", "gpt-5"),
                    ("background", "gpt-5-mini"),
                ])),
                system_prompt: "Base".to_string(),
                mcp_server_refs: vec![],
                capabilities: std::collections::HashMap::new(),
            }),
        )
        .await
        .unwrap();

        let mut effective_spec = resolved.effective_spec;
        apply_agent_spec_delta(
            &mut effective_spec,
            &manifests::AgentSpecDelta {
                model_policy: Some(manifests::ModelPolicyDelta {
                    upsert: vec![
                        model_profile("background", "gpt-5-background"),
                        model_profile("interactive", "gpt-5-fast"),
                    ],
                }),
                ..Default::default()
            },
        )
        .unwrap();

        let profiles = effective_spec
            .model_policy
            .as_ref()
            .unwrap()
            .profiles
            .iter()
            .map(|profile| {
                (
                    profile.name.as_str(),
                    profile.model.as_ref().unwrap().name.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            profiles,
            vec![
                ("default", "gpt-5"),
                ("background", "gpt-5-background"),
                ("interactive", "gpt-5-fast"),
            ]
        );
    }

    #[tokio::test]
    async fn rejects_model_policy_without_default_profile() {
        let err = resolve_agent_definition_with_loader(
            &MapTemplateLoader {
                templates: HashMap::new(),
            },
            &custom_spec_definition(manifests::AgentSpec {
                features: vec![],
                model_policy: Some(model_policy(vec![("interactive", "gpt-5-fast")])),
                system_prompt: "Base".to_string(),
                mcp_server_refs: vec![],
                capabilities: std::collections::HashMap::new(),
            }),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("'default' profile"));
    }

    #[tokio::test]
    async fn validates_missing_source_template_name_and_template_definition() {
        let loader = MapTemplateLoader {
            templates: HashMap::from([(
                "missing-def".to_string(),
                manifests::AgentTemplate {
                    api_version: "talon.impalasys.com/v1".to_string(),
                    kind: "AgentTemplate".to_string(),
                    metadata: None,
                    definition: None,
                },
            )]),
        };

        let missing_source = resolve_agent_definition_with_loader(
            &loader,
            &manifests::AgentDefinition { source: None },
        )
        .await
        .unwrap_err();
        assert!(missing_source.to_string().contains("provide a source"));

        let missing_name = resolve_agent_definition_with_loader(
            &loader,
            &templated_definition("   ", manifests::AgentSpecDelta::default()),
        )
        .await
        .unwrap_err();
        assert!(missing_name.to_string().contains("template_name is required"));

        let missing_definition = resolve_agent_definition_with_loader(
            &loader,
            &templated_definition("missing-def", manifests::AgentSpecDelta::default()),
        )
        .await
        .unwrap_err();
        assert!(missing_definition.to_string().contains("missing definition"));
    }

    #[tokio::test]
    async fn enforces_template_depth_limit() {
        let mut templates = HashMap::new();
        for idx in 0..=MAX_TEMPLATE_DEPTH {
            let name = format!("t{idx}");
            let definition = if idx == MAX_TEMPLATE_DEPTH {
                custom_spec_definition(manifests::AgentSpec {
                    features: vec![],
                    model_policy: Some(model_policy(vec![("default", "gpt-5")])),
                    system_prompt: "Base".to_string(),
                    mcp_server_refs: vec![],
                    capabilities: HashMap::new(),
                })
            } else {
                templated_definition(&format!("t{}", idx + 1), manifests::AgentSpecDelta::default())
            };
            templates.insert(name.clone(), template(&name, definition));
        }

        let err = resolve_agent_definition_with_loader(
            &MapTemplateLoader { templates },
            &templated_definition("t0", manifests::AgentSpecDelta::default()),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("depth exceeded"));
    }

    #[test]
    fn apply_prompt_feature_and_capability_deltas_cover_remaining_branches() {
        let mut spec = manifests::AgentSpec {
            features: vec![feature("search", "builtin"), feature("draft", "builtin")],
            model_policy: Some(model_policy(vec![("default", "gpt-5")])),
            system_prompt: "Base".to_string(),
            mcp_server_refs: vec!["conic-api".to_string()],
            capabilities: HashMap::from([(
                "schedules".to_string(),
                ListValue {
                    values: vec![protobuf_string("inspect")],
                },
            )]),
        };

        apply_agent_spec_delta(
            &mut spec,
            &manifests::AgentSpecDelta {
                system_prompt: Some(manifests::PromptDelta {
                    operation: Some(manifests::prompt_delta::Operation::Replace(
                        "Reset".to_string(),
                    )),
                }),
                features: Some(manifests::FeatureSetDelta {
                    upsert: vec![feature("search", "mcp"), feature("plan", "builtin")],
                    remove: vec!["draft".to_string()],
                }),
                mcp_server_refs: Some(manifests::StringListDelta {
                    replace: vec!["alt".to_string()],
                    ..Default::default()
                }),
                capabilities: Some(manifests::CapabilitiesPolicyDelta {
                    replace: HashMap::from([(
                        "sessions".to_string(),
                        ListValue {
                            values: vec![
                                protobuf_string("inspect"),
                                protobuf_string("read:messages"),
                            ],
                        },
                    )]),
                }),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(spec.system_prompt, "Reset");
        assert_eq!(
            spec.features.iter().map(|f| (f.name.as_str(), f.r#type.as_str())).collect::<Vec<_>>(),
            vec![("search", "mcp"), ("plan", "builtin")]
        );
        assert_eq!(spec.mcp_server_refs, vec!["alt".to_string()]);
        assert!(spec.capabilities.contains_key("sessions"));

        apply_agent_spec_delta(
            &mut spec,
            &manifests::AgentSpecDelta {
                system_prompt: Some(manifests::PromptDelta {
                    operation: Some(manifests::prompt_delta::Operation::Prepend(
                        "Prefix: ".to_string(),
                    )),
                }),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(spec.system_prompt, "Prefix: Reset");

        let duplicate_ref = apply_agent_spec_delta(
            &mut spec,
            &manifests::AgentSpecDelta {
                mcp_server_refs: Some(manifests::StringListDelta {
                    replace: vec!["alt".to_string(), "alt".to_string()],
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(duplicate_ref
            .to_string()
            .contains("Duplicate MCP server ref"));
    }

    #[test]
    fn validate_agent_spec_rejects_duplicate_or_invalid_fields() {
        let duplicate_feature = validate_agent_spec(&manifests::AgentSpec {
            features: vec![feature("search", "builtin"), feature("search", "builtin")],
            model_policy: Some(model_policy(vec![("default", "gpt-5")])),
            system_prompt: "Base".to_string(),
            mcp_server_refs: vec![],
            capabilities: HashMap::new(),
        })
        .unwrap_err();
        assert!(duplicate_feature.to_string().contains("Duplicate feature"));

        let blank_mcp_ref = validate_agent_spec(&manifests::AgentSpec {
            features: vec![],
            model_policy: Some(model_policy(vec![("default", "gpt-5")])),
            system_prompt: "Base".to_string(),
            mcp_server_refs: vec![" ".to_string()],
            capabilities: HashMap::new(),
        })
        .unwrap_err();
        assert!(blank_mcp_ref.to_string().contains("must be non-empty"));

        let duplicate_mcp_ref = validate_agent_spec(&manifests::AgentSpec {
            features: vec![],
            model_policy: Some(model_policy(vec![("default", "gpt-5")])),
            system_prompt: "Base".to_string(),
            mcp_server_refs: vec!["github".to_string(), "github".to_string()],
            capabilities: HashMap::new(),
        })
        .unwrap_err();
        assert!(duplicate_mcp_ref.to_string().contains("Duplicate MCP server ref"));
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
        assert!(duplicate_profile.to_string().contains("Duplicate model profile"));

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
        assert!(invalid_capability.to_string().contains("unsupported action"));

        let invalid_action_type = validate_capabilities_policy(
            &HashMap::from([(
                "sessions".to_string(),
                ListValue {
                    values: vec![crate::gateway::rpc::protobuf_value::Value {
                        kind: Some(ProtoValueKind::NumberValue(3.0)),
                    }],
                },
            )]),
            "caps",
        )
        .unwrap_err();
        assert!(invalid_action_type.to_string().contains("actions must be strings"));

        let untrimmed_action = validate_capabilities_policy(
            &HashMap::from([(
                "sessions".to_string(),
                ListValue {
                    values: vec![protobuf_string(" inspect ")],
                },
            )]),
            "caps",
        )
        .unwrap_err();
        assert!(untrimmed_action.to_string().contains("must be trimmed"));
    }

    #[test]
    fn validate_model_and_delta_entries_reject_missing_fields() {
        let missing_provider = validate_model(
            &manifests::Model {
                provider: " ".to_string(),
                name: "gpt-5".to_string(),
                temperature: 0.2,
                thinking: None,
            },
            "model",
        )
        .unwrap_err();
        assert!(missing_provider.to_string().contains(".provider is required"));

        let missing_name = validate_model(
            &manifests::Model {
                provider: "openai".to_string(),
                name: " ".to_string(),
                temperature: 0.2,
                thinking: None,
            },
            "model",
        )
        .unwrap_err();
        assert!(missing_name.to_string().contains(".name is required"));

        let mut spec = manifests::AgentSpec {
            features: vec![],
            model_policy: Some(model_policy(vec![("default", "gpt-5")])),
            system_prompt: "Base".to_string(),
            mcp_server_refs: vec![],
            capabilities: HashMap::new(),
        };

        let empty_profile_name = apply_agent_spec_delta(
            &mut spec,
            &manifests::AgentSpecDelta {
                model_policy: Some(manifests::ModelPolicyDelta {
                    upsert: vec![manifests::ModelProfile {
                        name: " ".to_string(),
                        model: Some(model("gpt-5")),
                    }],
                }),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(empty_profile_name.to_string().contains("non-empty name"));

        let missing_model = apply_agent_spec_delta(
            &mut spec,
            &manifests::AgentSpecDelta {
                model_policy: Some(manifests::ModelPolicyDelta {
                    upsert: vec![manifests::ModelProfile {
                        name: "interactive".to_string(),
                        model: None,
                    }],
                }),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(missing_model.to_string().contains("missing model"));

        let blank_feature = apply_agent_spec_delta(
            &mut spec,
            &manifests::AgentSpecDelta {
                features: Some(manifests::FeatureSetDelta {
                    upsert: vec![feature(" ", "builtin")],
                    remove: vec![],
                }),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(blank_feature.to_string().contains("non-empty name"));
    }

    fn protobuf_string(value: &str) -> crate::gateway::rpc::protobuf_value::Value {
        crate::gateway::rpc::protobuf_value::Value {
            kind: Some(ProtoValueKind::StringValue(value.to_string())),
        }
    }

}
