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
    match kind.to_lowercase().as_str() {
        "agent" | "agents" => {
            let (ns, agent_name) = agent_lookup_target(name, namespace);
            Ok((ns, "Agent".to_string(), agent_name))
        }
        "agenttemplate" | "templates" | "template" => Ok((
            namespace
                .cloned()
                .unwrap_or_else(|| crate::control::ns::TALON_SYSTEM.to_string()),
            "Template".to_string(),
            name.to_string(),
        )),
        "mcpserver" | "mcpservers" | "mcp" => Ok((
            crate::control::ns::TALON_SYSTEM.to_string(),
            "McpServer".to_string(),
            name.to_string(),
        )),
        "worker" | "workers" => Ok((
            crate::control::ns::TALON_SYSTEM.to_string(),
            "Worker".to_string(),
            name.to_string(),
        )),
        "mcpserverbinding" | "mcpbindings" | "mcpbinding" => {
            let ns = namespace
                .cloned()
                .context("McpServerBinding requires --namespace")?;
            Ok((ns, "McpServerBinding".to_string(), name.to_string()))
        }
        "knowledge" | "knowledgeartifact" | "knowledgeartifacts" => {
            let ns = namespace.cloned().context("Knowledge requires --namespace")?;
            Ok((ns, "Knowledge".to_string(), name.to_string()))
        }
        "schedule" | "schedules" => {
            let ns = namespace.cloned().context("Schedule requires --namespace")?;
            Ok((ns, "Schedule".to_string(), name.to_string()))
        }
        "channel" | "channels" => {
            let ns = namespace.cloned().context("Channel requires --namespace")?;
            Ok((ns, "Channel".to_string(), name.to_string()))
        }
        "channelsubscription"
        | "channelsubscriptions"
        | "channel-subscription"
        | "channel-subscriptions" => {
            let ns = namespace
                .cloned()
                .context("ChannelSubscription requires --namespace")?;
            let subscription = name
                .split_once('/')
                .map(|(_, subscription)| subscription)
                .unwrap_or(name);
            Ok((
                ns,
                "ChannelSubscription".to_string(),
                subscription.to_string(),
            ))
        }
        "workflow" | "workflows" => {
            let ns = namespace.cloned().context("Workflow requires --namespace")?;
            Ok((ns, "Workflow".to_string(), name.to_string()))
        }
        "deployment" | "deployments" => {
            let ns = namespace.cloned().context("Deployment requires --namespace")?;
            Ok((ns, "Deployment".to_string(), name.to_string()))
        }
        "sandboxclass" | "sandboxclasses" | "sandbox-class" | "sandbox-classes" => Ok((
            namespace
                .cloned()
                .unwrap_or_else(|| crate::control::ns::TALON_SYSTEM.to_string()),
            "SandboxClass".to_string(),
            name.to_string(),
        )),
        "sandboxpolicy" | "sandboxpolicies" | "sandbox-policy" | "sandbox-policies" => {
            let ns = namespace
                .cloned()
                .context("SandboxPolicy requires --namespace")?;
            Ok((ns, "SandboxPolicy".to_string(), name.to_string()))
        }
        "sandbox" | "sandboxes" => {
            let ns = namespace.cloned().context("Sandbox requires --namespace")?;
            Ok((ns, "Sandbox".to_string(), name.to_string()))
        }
        "usagepolicy" | "usagepolicies" | "usage-policy" | "usage-policies" => {
            let ns = namespace
                .cloned()
                .context("UsagePolicy requires --namespace")?;
            Ok((ns, "UsagePolicy".to_string(), name.to_string()))
        }
        other => anyhow::bail!("Unsupported resource kind '{}'", other),
    }
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
}
