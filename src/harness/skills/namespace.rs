// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::keys;
use crate::control::ProtoKeyValueStoreExt;
use crate::gateway::rpc::resources_proto;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamespaceSkill {
    pub name: String,
    pub namespace: String,
    pub description: String,
    pub instructions: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
struct NamespaceSkillSpec {
    description: String,
    instructions: String,
}

pub async fn load_effective_skills(
    kv: Arc<dyn crate::control::KeyValueStore>,
    namespace: &str,
) -> Result<Vec<NamespaceSkill>> {
    let mut seen_names = HashSet::new();
    let mut keys_to_fetch = Vec::new();

    for candidate_ns in crate::control::ns::ancestry(namespace) {
        let prefix = keys::skill_prefix(&candidate_ns);
        let mut skill_keys = kv.list_keys(&prefix).await?;
        skill_keys.sort();

        for key in skill_keys {
            let name = key.name.clone();
            if seen_names.insert(name) {
                keys_to_fetch.push(key);
            }
        }
    }

    let fetches = keys_to_fetch.into_iter().map(|key| {
        let kv = kv.clone();
        async move {
            match kv.get_msg::<resources_proto::Resource>(&key).await {
                Ok(Some(resource)) => parse_skill_resource(resource),
                Ok(None) => None,
                Err(err) => {
                    tracing::warn!(key = %key, error = %err, "skipping unreadable namespace skill");
                    None
                }
            }
        }
    });
    let mut skills: Vec<NamespaceSkill> = futures::future::join_all(fetches)
        .await
        .into_iter()
        .flatten()
        .collect();

    skills.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.namespace.cmp(&right.namespace))
    });
    Ok(skills)
}

pub fn format_skill_catalog(skills: &[NamespaceSkill]) -> String {
    let mut formatted_skills = Vec::new();

    for skill in skills {
        formatted_skills.push(format!(
            "## Skill: {}\nSource namespace: {}\nDescription: {}",
            skill.name,
            skill.namespace,
            skill.description.trim()
        ));
    }

    if formatted_skills.is_empty() {
        return String::new();
    }

    let mut output = String::from("# AVAILABLE SKILLS\n");
    output.push_str(
        "These shared namespace skills are available as reusable prompt guidance. Call the activate_skill tool to load full instructions before using a relevant skill.\n\n",
    );
    output.push_str(&formatted_skills.join("\n\n"));
    output
}

pub fn format_activated_skill(skill: &NamespaceSkill) -> Option<String> {
    Some(format!(
        "# ACTIVATED SKILL: {}\nSource namespace: {}\nDescription: {}\n\n{}",
        skill.name,
        skill.namespace,
        skill.description.trim(),
        skill.instructions.trim()
    ))
}

pub fn find_effective_skill<'a>(
    skills: &'a [NamespaceSkill],
    name: &str,
) -> Option<&'a NamespaceSkill> {
    skills.iter().find(|skill| skill.name == name)
}

pub fn effective_skill_names(skills: &[NamespaceSkill]) -> Vec<String> {
    skills.iter().map(|skill| skill.name.clone()).collect()
}

fn parse_skill_resource(resource: resources_proto::Resource) -> Option<NamespaceSkill> {
    if resource.kind != "Skill" {
        return None;
    }
    let metadata = resource.metadata?;
    let spec = match resource.spec?.kind? {
        resources_proto::resource_spec::Kind::Raw(raw) => raw,
        _ => return None,
    };
    let spec: NamespaceSkillSpec = serde_json::from_str(&spec.json).ok()?;
    if metadata.name.trim().is_empty()
        || spec.description.trim().is_empty()
        || spec.instructions.trim().is_empty()
    {
        return None;
    }
    Some(NamespaceSkill {
        name: metadata.name,
        namespace: metadata.namespace,
        description: spec.description,
        instructions: spec.instructions,
    })
}

pub fn skill_resource(
    ns: &str,
    name: &str,
    description: &str,
    instructions: &str,
) -> resources_proto::Resource {
    resources_proto::Resource {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "Skill".to_string(),
        metadata: Some(resources_proto::ResourceMeta {
            name: name.to_string(),
            namespace: ns.to_string(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
            owner_references: Vec::new(),
            finalizers: Vec::new(),
            generation: 0,
            resource_version: String::new(),
            uid: String::new(),
            deletion_timestamp: None,
        }),
        spec: Some(resources_proto::ResourceSpec {
            kind: Some(resources_proto::resource_spec::Kind::Raw(
                resources_proto::RawResourceSpec {
                    json: serde_json::to_string(&NamespaceSkillSpec {
                        description: description.to_string(),
                        instructions: instructions.to_string(),
                    })
                    .expect("skill spec should serialize"),
                },
            )),
        }),
        status: None,
    }
}

pub fn skill_name(skill: &NamespaceSkill) -> String {
    skill.name.clone()
}

pub fn skill_namespace(skill: &NamespaceSkill) -> String {
    skill.namespace.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::KeyValueStore;

    fn skill(ns: &str, name: &str, instructions: &str) -> resources_proto::Resource {
        skill_resource(ns, name, &format!("{} description", name), instructions)
    }

    #[tokio::test]
    async fn effective_skills_inherit_and_shadow_by_namespace_ancestry() {
        let kv = Arc::new(crate::test_support::MockKvStore::default());
        kv.set_msg(
            &keys::skill("acme", "review"),
            &skill("acme", "review", "parent"),
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::skill("acme", "release"),
            &skill("acme", "release", "release"),
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::skill("acme:team", "review"),
            &skill("acme:team", "review", "child"),
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::skill("acme:sibling", "sibling"),
            &skill("acme:sibling", "sibling", "sibling"),
        )
        .await
        .unwrap();

        let skills = load_effective_skills(kv, "acme:team:agent").await.unwrap();
        let rendered = format_skill_catalog(&skills);

        assert_eq!(skills.len(), 2);
        assert!(rendered.contains("Skill: release"));
        assert!(rendered.contains("Skill: review"));
        assert!(rendered.contains("Source namespace: acme:team"));
        assert!(rendered.contains("review description"));
        assert!(!rendered.contains("child"));
        assert!(!rendered.contains("parent"));
        assert!(!rendered.contains("sibling"));

        let activated = format_activated_skill(find_effective_skill(&skills, "review").unwrap())
            .expect("valid skill should activate");
        assert!(activated.contains("ACTIVATED SKILL: review"));
        assert!(activated.contains("Source namespace: acme:team"));
        assert!(activated.contains("child"));
        assert!(!activated.contains("parent"));
    }

    #[tokio::test]
    async fn effective_skills_skip_unreadable_records() {
        let kv = Arc::new(crate::test_support::MockKvStore::default());
        kv.set_msg(
            &keys::skill("acme", "review"),
            &skill("acme", "review", "valid"),
        )
        .await
        .unwrap();
        kv.set(&keys::skill("acme", "broken"), b"not-protobuf")
            .await
            .unwrap();

        let skills = load_effective_skills(kv, "acme").await.unwrap();

        assert_eq!(effective_skill_names(&skills), vec!["review"]);
    }

    #[test]
    fn invalid_skills_are_not_parsed() {
        let mut missing_spec = skill_resource("acme", "review", "Review code", "instructions");
        missing_spec.spec = None;
        assert!(parse_skill_resource(missing_spec).is_none());

        let empty_instructions = skill_resource("acme", "review", "Review code", "");
        assert!(parse_skill_resource(empty_instructions).is_none());
    }
}
