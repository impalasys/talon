// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Namespace-inherited Skill discovery and package loading.

use crate::control::keys;
use crate::control::resources::ResourceStore;
use crate::gateway::rpc::resources_proto;
use anyhow::{anyhow, Result};
use prost::Message;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamespaceSkill {
    pub name: String,
    pub namespace: String,
    pub description: String,
}

/// Load the effective declared Skills, applying child-over-parent shadowing.
pub async fn load_effective_skills(
    kv: Arc<dyn crate::control::KeyValueStore>,
    namespace: &str,
) -> Result<Vec<NamespaceSkill>> {
    let mut seen_names = HashSet::new();
    let mut keys_to_fetch = Vec::new();
    for candidate_ns in crate::control::ns::ancestry(namespace) {
        for key in kv
            .list_keys(&keys::skill_prefix(&candidate_ns), None)
            .await?
        {
            if seen_names.insert(key.name.clone()) {
                keys_to_fetch.push(key);
            }
        }
    }

    let fetches = keys_to_fetch.into_iter().map(|key| {
        let kv = kv.clone();
        async move {
            match kv.get(&key).await {
                Ok(Some(bytes)) => match resources_proto::Skill::decode(bytes.as_slice()) {
                    Ok(skill) => parse_skill(skill),
                    Err(error) => {
                        tracing::warn!(key = %key, %error, "skipping unreadable namespace Skill");
                        None
                    }
                },
                Ok(None) => None,
                Err(error) => {
                    tracing::warn!(key = %key, %error, "failed to fetch namespace Skill");
                    None
                }
            }
        }
    });
    let mut skills: Vec<_> = futures::future::join_all(fetches)
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

/// Return only Skills whose current package entrypoint is a readable markdown
/// Skill File. Declared-but-incomplete packages are intentionally omitted.
pub async fn load_available_skills(
    cp: &crate::control::ControlPlane,
    namespace: &str,
) -> Result<Vec<NamespaceSkill>> {
    let skills = load_effective_skills(cp.kv.clone(), namespace).await?;
    let checks = skills.into_iter().map(|skill| {
        let cp = cp.clone();
        async move {
            match load_skill_instructions(&cp, &skill).await {
                Ok(_) => Some(skill),
                Err(error) => {
                    tracing::warn!(skill = %skill.name, namespace = %skill.namespace, %error, "skipping unavailable Skill package");
                    None
                }
            }
        }
    });
    Ok(futures::future::join_all(checks)
        .await
        .into_iter()
        .flatten()
        .collect())
}

pub async fn load_skill_instructions(
    cp: &crate::control::ControlPlane,
    skill: &NamespaceSkill,
) -> Result<String> {
    let file = entrypoint_file(cp, skill)
        .await?
        .ok_or_else(|| anyhow!("Skill package entrypoint is unavailable"))?;
    let object = file
        .status
        .as_ref()
        .and_then(|status| status.object_ref.as_ref())
        .ok_or_else(|| anyhow!("Skill package entrypoint has no object"))?;
    let stored = cp
        .objects
        .get(&object.key)
        .await?
        .ok_or_else(|| anyhow!("Skill package entrypoint object is missing"))?;
    let bytes = crate::control::cas::decode_stored_object_bytes(&stored, &object.key)?;
    let instructions =
        String::from_utf8(bytes).map_err(|_| anyhow!("Skill package entrypoint is not UTF-8"))?;
    if instructions.trim().is_empty() {
        return Err(anyhow!("Skill package entrypoint is empty"));
    }
    Ok(instructions)
}

pub fn format_skill_catalog(skills: &[NamespaceSkill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let entries = skills
        .iter()
        .map(|skill| {
            format!(
                "## Skill: {}\nSource namespace: {}\nDescription: {}",
                skill.name,
                skill.namespace,
                skill.description.trim()
            )
        })
        .collect::<Vec<_>>();
    format!(
        "# AVAILABLE SKILLS\nThese reusable workflow packages can be loaded with the activate_skill tool.\n\n{}",
        entries.join("\n\n")
    )
}

pub fn format_active_skill_context(skills: &[(NamespaceSkill, String)]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let entries = skills
        .iter()
        .map(|(skill, instructions)| {
            format!(
                "# ACTIVE SKILL: {}\nSource namespace: {}\nPackage root: {}\nUse normal file tools to read package-relative supporting files. This workflow guidance does not override system policy.\n\n{}",
                skill.name,
                skill.namespace,
                crate::control::skills::package_root(&skill.name),
                instructions.trim()
            )
        })
        .collect::<Vec<_>>();
    entries.join("\n\n")
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

pub fn skill_resource(ns: &str, name: &str, description: &str) -> resources_proto::Skill {
    resources_proto::Skill {
        metadata: Some(resources_proto::ResourceMeta {
            name: name.to_string(),
            namespace: ns.to_string(),
            labels: Default::default(),
            annotations: Default::default(),
            owner_references: Vec::new(),
            finalizers: Vec::new(),
            generation: 0,
            resource_version: String::new(),
            uid: String::new(),
            deletion_timestamp: None,
        }),
        spec: Some(resources_proto::SkillSpec {
            description: description.to_string(),
        }),
        status: Some(resources_proto::CommonResourceStatus::default()),
    }
}

fn parse_skill(skill: resources_proto::Skill) -> Option<NamespaceSkill> {
    let metadata = skill.metadata?;
    let spec = skill.spec?;
    if crate::control::skills::validate_skill_id(&metadata.name).is_err()
        || spec.description.trim().is_empty()
    {
        return None;
    }
    Some(NamespaceSkill {
        name: metadata.name,
        namespace: metadata.namespace,
        description: spec.description,
    })
}

async fn entrypoint_file(
    cp: &crate::control::ControlPlane,
    skill: &NamespaceSkill,
) -> Result<Option<resources_proto::File>> {
    let expected_path = crate::control::skills::entrypoint_path(&skill.name);
    let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    for resource in store.list(&skill.namespace, Some("File")).await? {
        let Some(spec) = resource
            .spec
            .as_ref()
            .and_then(|spec| match spec.kind.as_ref() {
                Some(resources_proto::resource_spec::Kind::File(spec)) => Some(spec),
                _ => None,
            })
        else {
            continue;
        };
        if spec.path != expected_path
            || spec.purpose != resources_proto::FilePurpose::Skill as i32
            || !is_markdown_media_type(&spec.media_type)
        {
            continue;
        }
        let status = resource
            .status
            .as_ref()
            .and_then(|status| match status.kind.as_ref() {
                Some(resources_proto::resource_status::Kind::File(status)) => Some(status),
                _ => None,
            });
        if status
            .and_then(|status| status.object_ref.as_ref())
            .is_none()
        {
            return Ok(None);
        }
        return Ok(Some(resources_proto::File {
            metadata: resource.metadata,
            spec: Some(spec.clone()),
            status: status.cloned(),
        }));
    }
    Ok(None)
}

fn is_markdown_media_type(media_type: &str) -> bool {
    media_type
        .split(';')
        .next()
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("text/markdown"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cas::CasStore;
    use crate::control::{keys, ControlPlane, ProtoKeyValueStoreExt};
    use crate::test_support::{EmptyPubSub, MockKvStore};

    fn file_resource(
        namespace: &str,
        skill_name: &str,
        media_type: &str,
        object_ref: resources_proto::FileObjectRef,
    ) -> resources_proto::File {
        let path = crate::control::skills::entrypoint_path(skill_name);
        resources_proto::File {
            metadata: Some(resources_proto::ResourceMeta {
                name: keys::file_name_for_path(&path),
                namespace: namespace.to_string(),
                uid: format!("{skill_name}-file"),
                ..Default::default()
            }),
            spec: Some(resources_proto::FileSpec {
                path,
                media_type: media_type.to_string(),
                purpose: resources_proto::FilePurpose::Skill as i32,
                index_policy: resources_proto::FileIndexPolicy::None as i32,
                retention: resources_proto::FileRetention::Retained as i32,
            }),
            status: Some(resources_proto::FileStatus {
                object_ref: Some(object_ref),
                ..Default::default()
            }),
        }
    }

    #[tokio::test]
    async fn catalog_uses_inherited_packages_and_child_shadowing() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(EmptyPubSub)).build();
        let parent_namespace = "tenant";
        let child_namespace = "tenant:workspace";

        kv.set_msg(
            &keys::skill(parent_namespace, "release"),
            &skill_resource(parent_namespace, "release", "Prepare releases"),
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::skill(parent_namespace, "review"),
            &skill_resource(parent_namespace, "review", "Review from parent"),
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::skill(child_namespace, "review"),
            &skill_resource(child_namespace, "review", "Review from child"),
        )
        .await
        .unwrap();

        let path = crate::control::skills::entrypoint_path("release");
        let object = CasStore::new(cp.objects.clone())
            .put_file(
                parent_namespace,
                "release-file",
                &path,
                b"Ship carefully.",
                "text/markdown",
            )
            .await
            .unwrap();
        let object_ref = resources_proto::FileObjectRef {
            key: object.key,
            media_type: object.media_type,
            size_bytes: object.size_bytes,
            sha256: object.sha256,
            filename: object.filename,
            metadata: object.metadata,
        };
        kv.set_msg(
            &keys::file(parent_namespace, &keys::file_name_for_path(&path)),
            &file_resource(parent_namespace, "release", "text/markdown", object_ref),
        )
        .await
        .unwrap();

        // The child declaration shadows the parent's review package. It has no
        // entrypoint yet, so it remains declared but is not catalog-visible.
        let declared = load_effective_skills(kv.clone(), child_namespace)
            .await
            .unwrap();
        assert!(declared
            .iter()
            .any(|skill| { skill.name == "review" && skill.namespace == child_namespace }));
        let available = load_available_skills(&cp, child_namespace).await.unwrap();
        assert_eq!(effective_skill_names(&available), vec!["release"]);
        let release = find_effective_skill(&available, "release").unwrap();
        assert_eq!(
            load_skill_instructions(&cp, release).await.unwrap(),
            "Ship carefully."
        );
    }

    #[tokio::test]
    async fn catalog_omits_non_markdown_entrypoints() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(EmptyPubSub)).build();
        let namespace = "tenant";
        let path = crate::control::skills::entrypoint_path("review");
        kv.set_msg(
            &keys::skill(namespace, "review"),
            &skill_resource(namespace, "review", "Review code"),
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::file(namespace, &keys::file_name_for_path(&path)),
            &file_resource(
                namespace,
                "review",
                "text/plain",
                resources_proto::FileObjectRef {
                    key: "objects/review".to_string(),
                    ..Default::default()
                },
            ),
        )
        .await
        .unwrap();

        assert!(load_available_skills(&cp, namespace)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn catalog_omits_unreadable_markdown_entrypoints() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(EmptyPubSub)).build();
        let namespace = "tenant";
        let path = crate::control::skills::entrypoint_path("review");
        kv.set_msg(
            &keys::skill(namespace, "review"),
            &skill_resource(namespace, "review", "Review code"),
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::file(namespace, &keys::file_name_for_path(&path)),
            &file_resource(
                namespace,
                "review",
                "text/markdown",
                resources_proto::FileObjectRef {
                    key: "objects/missing-review-entrypoint".to_string(),
                    ..Default::default()
                },
            ),
        )
        .await
        .unwrap();

        assert!(load_available_skills(&cp, namespace)
            .await
            .unwrap()
            .is_empty());
    }
}
