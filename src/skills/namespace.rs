// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::keys;
use crate::control::ProtoKeyValueStoreExt;
use crate::gateway::rpc::manifests;
use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;

pub async fn load_effective_skills(
    kv: Arc<dyn crate::control::KeyValueStore>,
    namespace: &str,
) -> Result<Vec<manifests::Skill>> {
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
        async move { kv.get_msg::<manifests::Skill>(&key).await }
    });
    let mut skills: Vec<manifests::Skill> = futures::future::try_join_all(fetches)
        .await?
        .into_iter()
        .flatten()
        .collect();

    skills.sort_by(|left, right| {
        skill_name(left)
            .cmp(&skill_name(right))
            .then_with(|| skill_namespace(left).cmp(&skill_namespace(right)))
    });
    Ok(skills)
}

pub fn format_skill_context(skills: &[manifests::Skill]) -> String {
    let mut formatted_skills = Vec::new();

    for skill in skills {
        let Some(spec) = skill.spec.as_ref() else {
            continue;
        };
        formatted_skills.push(format!(
            "## Skill: {}\nSource namespace: {}\nDescription: {}\n\n{}",
            skill_name(skill),
            skill_namespace(skill),
            spec.description.trim(),
            spec.instructions.trim()
        ));
    }

    if formatted_skills.is_empty() {
        return String::new();
    }

    let mut output = String::from("# INHERITED SKILLS\n");
    output.push_str(
        "These shared namespace skills are available as reusable prompt guidance. Follow any relevant skill instructions for this task.\n\n",
    );
    output.push_str(&formatted_skills.join("\n\n"));
    output
}

fn skill_name(skill: &manifests::Skill) -> String {
    skill
        .metadata
        .as_ref()
        .map(|metadata| metadata.name.clone())
        .unwrap_or_default()
}

fn skill_namespace(skill: &manifests::Skill) -> String {
    skill
        .metadata
        .as_ref()
        .map(|metadata| metadata.namespace.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn skill(ns: &str, name: &str, instructions: &str) -> manifests::Skill {
        manifests::Skill {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Skill".to_string(),
            metadata: Some(manifests::ObjectMeta {
                name: name.to_string(),
                namespace: ns.to_string(),
                labels: HashMap::new(),
                annotations: HashMap::new(),
            }),
            spec: Some(manifests::SkillSpec {
                description: format!("{} description", name),
                instructions: instructions.to_string(),
            }),
        }
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
        let rendered = format_skill_context(&skills);

        assert_eq!(skills.len(), 2);
        assert!(rendered.contains("Skill: release"));
        assert!(rendered.contains("Skill: review"));
        assert!(rendered.contains("Source namespace: acme:team"));
        assert!(rendered.contains("child"));
        assert!(!rendered.contains("parent"));
        assert!(!rendered.contains("sibling"));
    }

    #[test]
    fn format_skill_context_omits_header_when_no_valid_skills_render() {
        let mut invalid = skill("acme", "empty", "ignored");
        invalid.spec = None;

        assert_eq!(format_skill_context(&[invalid]), "");
    }
}
