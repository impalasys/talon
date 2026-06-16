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
        async move {
            match kv.get_msg::<manifests::Skill>(&key).await {
                Ok(skill) => skill,
                Err(err) => {
                    tracing::warn!(key = %key, error = %err, "skipping unreadable namespace skill");
                    None
                }
            }
        }
    });
    let mut skills: Vec<manifests::Skill> = futures::future::join_all(fetches)
        .await
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

pub fn format_skill_catalog(skills: &[manifests::Skill]) -> String {
    let mut formatted_skills = Vec::new();

    for skill in skills {
        let Some(spec) = valid_skill_spec(skill) else {
            continue;
        };
        formatted_skills.push(format!(
            "## Skill: {}\nSource namespace: {}\nDescription: {}",
            skill_name(skill),
            skill_namespace(skill),
            spec.description.trim()
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

pub fn format_activated_skill(skill: &manifests::Skill) -> Option<String> {
    let spec = valid_skill_spec(skill)?;
    Some(format!(
        "# ACTIVATED SKILL: {}\nSource namespace: {}\nDescription: {}\n\n{}",
        skill_name(skill),
        skill_namespace(skill),
        spec.description.trim(),
        spec.instructions.trim()
    ))
}

pub fn find_effective_skill<'a>(
    skills: &'a [manifests::Skill],
    name: &str,
) -> Option<&'a manifests::Skill> {
    skills
        .iter()
        .find(|skill| valid_skill_spec(skill).is_some() && skill_name(skill) == name)
}

pub fn effective_skill_names(skills: &[manifests::Skill]) -> Vec<String> {
    skills
        .iter()
        .filter(|skill| valid_skill_spec(skill).is_some())
        .map(skill_name)
        .collect()
}

fn valid_skill_spec(skill: &manifests::Skill) -> Option<&manifests::SkillSpec> {
    let spec = skill.spec.as_ref()?;
    if skill_name(skill).trim().is_empty()
        || spec.description.trim().is_empty()
        || spec.instructions.trim().is_empty()
    {
        return None;
    }
    Some(spec)
}

pub fn skill_name(skill: &manifests::Skill) -> String {
    skill
        .metadata
        .as_ref()
        .map(|metadata| metadata.name.clone())
        .unwrap_or_default()
}

pub fn skill_namespace(skill: &manifests::Skill) -> String {
    skill
        .metadata
        .as_ref()
        .map(|metadata| metadata.namespace.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::KeyValueStore;
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

        assert_eq!(skills.len(), 1);
        assert_eq!(skill_name(&skills[0]), "review");
    }

    #[test]
    fn format_skill_catalog_omits_header_when_no_valid_skills_render() {
        let mut invalid = skill("acme", "empty", "ignored");
        invalid.spec = None;

        assert_eq!(format_skill_catalog(&[invalid]), "");
    }

    #[test]
    fn skill_catalog_and_activation_skip_empty_fields() {
        let empty_instructions = skill("acme", "empty", "");

        assert_eq!(format_skill_catalog(&[empty_instructions.clone()]), "");
        assert_eq!(
            effective_skill_names(&[empty_instructions.clone()]),
            Vec::<String>::new()
        );
        assert!(find_effective_skill(&[empty_instructions.clone()], "empty").is_none());
        assert_eq!(format_activated_skill(&empty_instructions), None);
    }
}
