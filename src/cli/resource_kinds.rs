// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
struct ResourceKindRegistry {
    resource: Vec<ResourceKindSpec>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResourceKindSpec {
    pub(super) kind: String,
    pub(super) aliases: Vec<String>,
    pub(super) apply_route: String,
    pub(super) lookup_namespace: String,
    pub(super) list_namespace: String,
    pub(super) name_policy: String,
    pub(super) user_authorable: bool,
    pub(super) cli_lookup: bool,
    pub(super) cli_list: bool,
}

fn resource_registry() -> &'static [ResourceKindSpec] {
    static REGISTRY: OnceLock<Vec<ResourceKindSpec>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| {
            let config = include_str!("../../proto/resources.toml");
            toml::from_str::<ResourceKindRegistry>(config)
                .expect("proto/resources.toml must be valid")
                .resource
        })
        .as_slice()
}

pub(super) fn resource_kind(input: &str) -> Option<&'static ResourceKindSpec> {
    let input = input.to_ascii_lowercase();
    resource_registry().iter().find(|resource| {
        resource.kind.to_ascii_lowercase() == input
            || resource
                .aliases
                .iter()
                .any(|alias| alias.to_ascii_lowercase() == input)
    })
}

pub(super) fn canonical_resource_kind(input: &str) -> Option<&'static str> {
    resource_kind(input).map(|resource| resource.kind.as_str())
}

#[cfg(test)]
mod resource_kind_tests {
    use super::*;

    #[test]
    fn skill_is_a_namespaced_generic_resource() {
        let skill = resource_kind("skills").expect("Skill registry entry");
        assert_eq!(skill.kind, "Skill");
        assert_eq!(skill.apply_route, "generic");
        assert_eq!(skill.lookup_namespace, "required");
        assert!(skill.user_authorable);
    }

    #[test]
    fn worker_remains_internal_and_not_applyable() {
        let worker = resource_kind("worker").expect("Worker registry entry");
        assert_eq!(worker.apply_route, "internal");
        assert!(!worker.user_authorable);
    }
}
