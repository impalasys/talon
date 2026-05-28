// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};

use crate::control::keys;

pub(super) fn namespaced_key(namespace: &str, key: &str) -> Result<String> {
    let namespace = match namespace {
        "talon-system:ns" => "Sys:ns",
        "talon-system:ns:internal" => "Sys:ns:internal",
        namespace => namespace,
    };

    if namespace == "Sys:ns" {
        let name = key
            .strip_prefix("Namespace/")
            .ok_or_else(|| anyhow!("unknown legacy namespace metadata key '{key}'"))?;
        return Ok(keys::namespace_metadata(name).canonical());
    }

    if namespace == "Sys:ns:internal" {
        let child = key
            .strip_prefix("NamespaceRef/")
            .ok_or_else(|| anyhow!("unknown legacy root namespace edge key '{key}'"))?;
        return Ok(keys::namespace_ref(None, child).canonical());
    }

    if let Some(parent) = namespace.strip_suffix(":ns:internal") {
        let full_child = key
            .strip_prefix("NamespaceRef/")
            .ok_or_else(|| anyhow!("unknown legacy namespace edge key '{key}'"))?;
        let child = full_child.rsplit(':').next().unwrap_or(full_child);
        return Ok(keys::namespace_ref(Some(parent), child).canonical());
    }

    let parts = key.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["Agent", agent_name] => Ok(keys::agent(namespace, agent_name).canonical()),
        ["Agent", agent_name, "Session", session_id] => {
            Ok(keys::session(namespace, agent_name, session_id).canonical())
        }
        ["Agent", agent_name, "Session", session_id, "Messages", message] => {
            Ok(keys::session_message(namespace, agent_name, session_id, message).canonical())
        }
        ["Agent", agent_name, "Session", session_id, "Messages", message, "Steps", step] => Ok(
            keys::session_message_step(namespace, agent_name, session_id, message, step)
                .canonical(),
        ),
        ["Schedule", name] => Ok(keys::schedule(namespace, name).canonical()),
        ["AgentTemplate", name] => Ok(keys::agent_template(name).canonical()),
        ["McpServer", name] => Ok(keys::mcp_server(name).canonical()),
        ["McpServerBinding", name] => Ok(keys::mcp_server_binding(namespace, name).canonical()),
        ["KnowledgeResource", name] => Ok(keys::knowledge_resource(namespace, name).canonical()),
        ["Agent", agent, "Memory", rest @ ..] if !rest.is_empty() => {
            Ok(keys::agent_memory(namespace, agent, &rest.join("/")).canonical())
        }
        ["Knowledge", rest @ ..] if !rest.is_empty() => {
            Ok(keys::knowledge(namespace, &rest.join("/")).canonical())
        }
        _ => Err(anyhow!(
            "unknown legacy key shape namespace='{namespace}' key='{key}'"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::namespaced_key;

    #[test]
    fn legacy_keys_convert_to_ordered_keys() {
        assert_eq!(
            namespaced_key("acme", "Agent/agent-1/Session/s1/Messages/m1").unwrap(),
            crate::control::keys::session_message("acme", "agent-1", "s1", "m1").canonical()
        );
        assert_eq!(
            namespaced_key("Sys:ns", "Namespace/acme:team").unwrap(),
            crate::control::keys::namespace_metadata("acme:team").canonical()
        );
        assert_eq!(
            namespaced_key("acme:ns:internal", "NamespaceRef/acme:team").unwrap(),
            crate::control::keys::namespace_ref(Some("acme"), "team").canonical()
        );
        assert_eq!(
            namespaced_key("talon-system:ns", "Namespace/quickstart").unwrap(),
            crate::control::keys::namespace_metadata("quickstart").canonical()
        );
        assert_eq!(
            namespaced_key("talon-system:ns:internal", "NamespaceRef/quickstart").unwrap(),
            crate::control::keys::namespace_ref(None, "quickstart").canonical()
        );
    }
}
