// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub(super) fn agent_lookup_target(name: &str, namespace: Option<&String>) -> (String, String) {
    let mut parts = name.splitn(2, '/');
    let ns_part = parts.next().unwrap_or("default");
    let agent_name = parts.next().unwrap_or(ns_part);
    let (mut final_ns, final_name) = if agent_name == ns_part {
        ("default".to_string(), ns_part.to_string())
    } else {
        (ns_part.to_string(), agent_name.to_string())
    };
    if let Some(n) = namespace {
        final_ns = n.clone();
    }
    (final_ns, final_name)
}

pub(super) fn resource_lookup_target(
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<(String, String, String)> {
    let resource = crate::cli::resource_kind(kind)
        .with_context(|| format!("Unsupported resource kind '{}'", kind))?;
    if !resource.cli_lookup {
        anyhow::bail!("Unsupported resource kind '{}'", kind);
    }

    let final_namespace = match resource.lookup_namespace.as_str() {
        "agent" => agent_lookup_target(name, namespace).0,
        "default" => namespace.cloned().unwrap_or_else(|| "default".to_string()),
        "required" => namespace
            .cloned()
            .with_context(|| format!("{} requires --namespace", resource.kind))?,
        "system" => namespace
            .cloned()
            .unwrap_or_else(|| crate::control::ns::TALON_SYSTEM.to_string()),
        "system_fixed" => crate::control::ns::TALON_SYSTEM.to_string(),
        policy => anyhow::bail!(
            "Unsupported lookup namespace policy '{}'; check resource registry",
            policy
        ),
    };

    let final_name = match resource.name_policy.as_str() {
        "agent" => agent_lookup_target(name, namespace).1,
        "channel_subscription" => name
            .split_once('/')
            .map(|(_, subscription)| subscription)
            .unwrap_or(name)
            .to_string(),
        "plain" => name.to_string(),
        policy => anyhow::bail!(
            "Unsupported name policy '{}'; check resource registry",
            policy
        ),
    };

    Ok((final_namespace, resource.kind.clone(), final_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_template_lookup_honors_explicit_namespace() {
        let namespace = "customers:source".to_string();

        assert_eq!(
            resource_lookup_target("template", "coding-sandbox-policy", Some(&namespace)).unwrap(),
            (
                "customers:source".to_string(),
                "Template".to_string(),
                "coding-sandbox-policy".to_string(),
            )
        );
    }

    #[test]
    fn single_sandbox_class_lookup_honors_explicit_namespace() {
        let namespace = "Example".to_string();

        assert_eq!(
            resource_lookup_target("sandboxclass", "docker-codex", Some(&namespace)).unwrap(),
            (
                "Example".to_string(),
                "SandboxClass".to_string(),
                "docker-codex".to_string(),
            )
        );
    }

    #[test]
    fn skill_lookup_requires_namespace_and_accepts_plural_alias() {
        let namespace = "customers:source".to_string();

        assert_eq!(
            resource_lookup_target("skills", "review", Some(&namespace)).unwrap(),
            (
                "customers:source".to_string(),
                "Skill".to_string(),
                "review".to_string(),
            )
        );
        assert!(resource_lookup_target("skill", "review", None).is_err());
    }
}
