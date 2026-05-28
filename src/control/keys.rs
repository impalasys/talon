// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::ns;
use anyhow::{anyhow, Result};

const NS_PREFIX: &str = "@Namespace";
const DIRECT_CHILD_SEP: &str = "@";

fn enc(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}

fn dec(value: &str) -> Result<String> {
    urlencoding::decode(value)
        .map(|value| value.into_owned())
        .map_err(|err| anyhow!("failed to decode key segment '{value}': {err}"))
}

fn pair(kind: &str, name: &str) -> String {
    format!("{}/{}", kind, enc(name))
}

fn path(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(kind, name)| pair(kind, name))
        .collect::<Vec<_>>()
        .join("/")
}

fn resource_key(
    namespace: &str,
    parent: &[(&str, &str)],
    child_kind: &str,
    child_name: &str,
) -> String {
    if parent.is_empty() {
        format!(
            "{}/{}/{}/{}",
            NS_PREFIX,
            namespace,
            DIRECT_CHILD_SEP,
            pair(child_kind, child_name)
        )
    } else {
        format!(
            "{}/{}/{}/{}/{}",
            NS_PREFIX,
            namespace,
            path(parent),
            DIRECT_CHILD_SEP,
            pair(child_kind, child_name)
        )
    }
}

fn direct_child_prefix(
    namespace: &str,
    parent: &[(&str, &str)],
    child_kind: Option<&str>,
) -> String {
    let base = if parent.is_empty() {
        format!("{}/{}/{}/", NS_PREFIX, namespace, DIRECT_CHILD_SEP)
    } else {
        format!(
            "{}/{}/{}/{}/",
            NS_PREFIX,
            namespace,
            path(parent),
            DIRECT_CHILD_SEP
        )
    };
    match child_kind {
        Some(kind) => format!("{base}{kind}/"),
        None => base,
    }
}

pub fn recursive_prefix(namespace: &str, parent: &[(&str, &str)]) -> String {
    if parent.is_empty() {
        format!("{}/{}/", NS_PREFIX, namespace)
    } else {
        format!("{}/{}/{}/", NS_PREFIX, namespace, path(parent))
    }
}

pub fn direct_child_name(prefix: &str, key: &str) -> Option<String> {
    let suffix = key.strip_prefix(prefix)?;
    if suffix.is_empty() || suffix.contains('/') {
        return None;
    }
    dec(suffix).ok()
}

pub fn namespace_metadata(name: &str) -> String {
    resource_key(ns::TALON_SYSTEM, &[], "Namespace", name)
}

pub fn namespace_metadata_prefix() -> String {
    direct_child_prefix(ns::TALON_SYSTEM, &[], Some("Namespace"))
}

pub fn namespace_ref(parent: Option<&str>, child_segment: &str) -> String {
    let ref_namespace = parent.unwrap_or(ns::TALON_SYSTEM);
    resource_key(ref_namespace, &[], "NamespaceRef", child_segment)
}

pub fn namespace_ref_prefix(parent: Option<&str>) -> String {
    let ref_namespace = parent.unwrap_or(ns::TALON_SYSTEM);
    direct_child_prefix(ref_namespace, &[], Some("NamespaceRef"))
}

pub fn agent(namespace: &str, id: &str) -> String {
    resource_key(namespace, &[], "Agent", id)
}

pub fn agent_prefix(namespace: &str) -> String {
    direct_child_prefix(namespace, &[], Some("Agent"))
}

pub fn session(namespace: &str, agent: &str, session_id: &str) -> String {
    resource_key(namespace, &[("Agent", agent)], "Session", session_id)
}

pub fn session_prefix(namespace: &str, agent: &str) -> String {
    direct_child_prefix(namespace, &[("Agent", agent)], Some("Session"))
}

pub fn session_message(namespace: &str, agent: &str, session_id: &str, message_id: &str) -> String {
    resource_key(
        namespace,
        &[("Agent", agent), ("Session", session_id)],
        "SessionMessage",
        message_id,
    )
}

pub fn session_message_prefix(namespace: &str, agent: &str, session_id: &str) -> String {
    direct_child_prefix(
        namespace,
        &[("Agent", agent), ("Session", session_id)],
        Some("SessionMessage"),
    )
}

pub fn session_message_step(
    namespace: &str,
    agent: &str,
    session_id: &str,
    message_id: &str,
    step_id: &str,
) -> String {
    resource_key(
        namespace,
        &[
            ("Agent", agent),
            ("Session", session_id),
            ("SessionMessage", message_id),
        ],
        "SessionStep",
        step_id,
    )
}

pub fn session_message_step_prefix(
    namespace: &str,
    agent: &str,
    session_id: &str,
    message_id: &str,
) -> String {
    direct_child_prefix(
        namespace,
        &[
            ("Agent", agent),
            ("Session", session_id),
            ("SessionMessage", message_id),
        ],
        Some("SessionStep"),
    )
}

pub fn schedule(namespace: &str, name: &str) -> String {
    resource_key(namespace, &[], "Schedule", name)
}

pub fn schedule_prefix(namespace: &str) -> String {
    direct_child_prefix(namespace, &[], Some("Schedule"))
}

pub fn agent_template(name: &str) -> String {
    resource_key(ns::TALON_SYSTEM, &[], "AgentTemplate", name)
}

pub fn agent_template_prefix() -> String {
    direct_child_prefix(ns::TALON_SYSTEM, &[], Some("AgentTemplate"))
}

pub fn mcp_server(name: &str) -> String {
    resource_key(ns::TALON_SYSTEM, &[], "MCPServer", name)
}

pub fn mcp_server_prefix() -> String {
    direct_child_prefix(ns::TALON_SYSTEM, &[], Some("MCPServer"))
}

pub fn mcp_server_binding(namespace: &str, name: &str) -> String {
    resource_key(namespace, &[], "MCPServerBinding", name)
}

pub fn mcp_server_binding_prefix(namespace: &str) -> String {
    direct_child_prefix(namespace, &[], Some("MCPServerBinding"))
}

pub fn agent_memory(namespace: &str, agent: &str, path: &str) -> String {
    resource_key(namespace, &[("Agent", agent)], "Memory", path)
}

pub fn agent_memory_prefix(namespace: &str, agent: &str) -> String {
    direct_child_prefix(namespace, &[("Agent", agent)], Some("Memory"))
}

pub fn knowledge(namespace: &str, path: &str) -> String {
    resource_key(namespace, &[], "Knowledge", path)
}

pub fn knowledge_prefix(namespace: &str) -> String {
    direct_child_prefix(namespace, &[], Some("Knowledge"))
}

pub fn knowledge_resource(namespace: &str, name: &str) -> String {
    resource_key(namespace, &[], "KnowledgeResource", name)
}

pub fn knowledge_resource_prefix(namespace: &str) -> String {
    direct_child_prefix(namespace, &[], Some("KnowledgeResource"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_keys_place_separator_before_leaf() {
        assert_eq!(
            agent("Impala:Talon", "hello-agent"),
            "@Namespace/Impala:Talon/@/Agent/hello-agent"
        );
        assert_eq!(
            session("Impala:Talon", "hello-agent", "session-id"),
            "@Namespace/Impala:Talon/Agent/hello-agent/@/Session/session-id"
        );
        assert_eq!(
            session_message("Impala:Talon", "hello-agent", "session-id", "message-id"),
            "@Namespace/Impala:Talon/Agent/hello-agent/Session/session-id/@/SessionMessage/message-id"
        );
    }

    #[test]
    fn prefixes_distinguish_direct_and_recursive_listing() {
        assert_eq!(
            session_prefix("Impala:Talon", "hello-agent"),
            "@Namespace/Impala:Talon/Agent/hello-agent/@/Session/"
        );
        assert_eq!(
            recursive_prefix("Impala:Talon", &[("Agent", "hello-agent")]),
            "@Namespace/Impala:Talon/Agent/hello-agent/"
        );
    }

    #[test]
    fn names_are_encoded_per_resource_segment() {
        assert_eq!(
            knowledge("quickstart", "docs/hello world.md"),
            "@Namespace/quickstart/@/Knowledge/docs%2Fhello%20world.md"
        );
        assert_eq!(
            direct_child_name(
                &knowledge_prefix("quickstart"),
                &knowledge("quickstart", "docs/hello world.md")
            ),
            Some("docs/hello world.md".to_string())
        );
    }
}
