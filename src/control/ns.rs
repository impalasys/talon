// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

/// Core system namespace for Talon Control Plane resources.
/// Similar to Kubernetes' `kube-system`, this namespace is strictly for
/// internal platform logic, template artifacts, and manifests that are globally accessible
/// but shouldn't collide with dynamic tenant/workspace namespaces.
pub const TALON_SYSTEM: &str = "Sys";

/// Built-in root-level namespace for quickstarts, local development, and
/// single-tenant applications.
pub const DEFAULT: &str = "default";

/// Standard domain prefix.
/// In Kubernetes, while namespaces are short (e.g. `kube-system`), domains are
/// usually reserved for API Groups (e.g. `manifests.talon.impalasys.com/v1`) or
/// metadata labels (e.g. `talon.impalasys.com/template-name="my-template"`).
pub const TALON_DOMAIN: &str = "talon.impalasys.com";

/// Returns the namespace ancestry chain from most specific to least specific.
/// Example: `conic:wks:13` => `["conic:wks:13", "conic:wks", "conic"]`
pub fn ancestry(namespace: &str) -> Vec<String> {
    let mut current = namespace.trim();
    let mut chain = Vec::new();

    while !current.is_empty() {
        chain.push(current.to_string());
        let Some((parent, _)) = current.rsplit_once(':') else {
            break;
        };
        current = parent;
    }

    chain
}

#[cfg(test)]
mod tests {
    use super::ancestry;

    #[test]
    fn ancestry_walks_namespace_tree() {
        assert_eq!(
            ancestry("conic:wks:13"),
            vec!["conic:wks:13", "conic:wks", "conic"]
        );
        assert_eq!(ancestry("conic"), vec!["conic"]);
        assert!(ancestry("").is_empty());
    }
}
