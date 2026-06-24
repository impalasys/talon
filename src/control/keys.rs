// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::ns;
use anyhow::{anyhow, bail, Result};
use std::fmt;

const NS_PREFIX: &str = "@Namespace";
const DIRECT_CHILD_SEP: &str = "@";

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ResourceSegment {
    pub kind: String,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ResourceParent {
    pub namespace: String,
    pub parent_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ResourceKey {
    pub namespace: String,
    pub parent_path: String,
    pub kind: String,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ResourceList {
    pub parent: ResourceParent,
    pub kind: Option<String>,
}

impl ResourceSegment {
    pub fn new(kind: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            name: name.into(),
        }
    }
}

impl ResourceParent {
    pub fn root(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            parent_path: String::new(),
        }
    }

    pub fn child(&self, kind: &str, name: &str) -> Self {
        let segment = pair(kind, name);
        let parent_path = if self.parent_path.is_empty() {
            segment
        } else {
            format!("{}/{}", self.parent_path, segment)
        };
        Self {
            namespace: self.namespace.clone(),
            parent_path,
        }
    }

    pub fn list(&self, kind: Option<&str>) -> ResourceList {
        ResourceList {
            parent: self.clone(),
            kind: kind.map(str::to_string),
        }
    }
}

impl ResourceKey {
    pub fn new(namespace: &str, parent: &[(&str, &str)], kind: &str, name: &str) -> Self {
        Self {
            namespace: namespace.to_string(),
            parent_path: path(parent),
            kind: kind.to_string(),
            name: name.to_string(),
        }
    }

    pub fn as_parent(&self) -> ResourceParent {
        ResourceParent {
            namespace: self.namespace.clone(),
            parent_path: if self.parent_path.is_empty() {
                pair(&self.kind, &self.name)
            } else {
                format!("{}/{}", self.parent_path, pair(&self.kind, &self.name))
            },
        }
    }

    pub fn canonical(&self) -> String {
        if self.parent_path.is_empty() {
            format!(
                "{}/{}/{}/{}",
                NS_PREFIX,
                self.namespace,
                DIRECT_CHILD_SEP,
                pair(&self.kind, &self.name)
            )
        } else {
            format!(
                "{}/{}/{}/{}/{}",
                NS_PREFIX,
                self.namespace,
                self.parent_path,
                DIRECT_CHILD_SEP,
                pair(&self.kind, &self.name)
            )
        }
    }

    pub fn parse_canonical(key: &str) -> Result<Self> {
        let rest = key
            .strip_prefix(&format!("{NS_PREFIX}/"))
            .ok_or_else(|| anyhow!("key does not start with {NS_PREFIX}: {key}"))?;
        let (namespace, rest) = rest
            .split_once('/')
            .ok_or_else(|| anyhow!("key is missing namespace separator: {key}"))?;
        let (parent_path, leaf) =
            if let Some(leaf) = rest.strip_prefix(&format!("{DIRECT_CHILD_SEP}/")) {
                ("", leaf)
            } else {
                let sep = format!("/{DIRECT_CHILD_SEP}/");
                rest.split_once(&sep)
                    .ok_or_else(|| anyhow!("key is missing direct-child separator: {key}"))?
            };
        let segments = parse_path(leaf)?;
        if segments.len() != 1 {
            bail!("key leaf must contain exactly one kind/name segment: {key}");
        }
        let leaf = &segments[0];
        Ok(Self {
            namespace: namespace.to_string(),
            parent_path: parent_path.to_string(),
            kind: leaf.kind.clone(),
            name: leaf.name.clone(),
        })
    }

    pub fn parent_segments(&self) -> Result<Vec<ResourceSegment>> {
        parse_path(&self.parent_path)
    }
}

impl ResourceList {
    pub fn matches(&self, key: &ResourceKey) -> bool {
        key.namespace == self.parent.namespace
            && key.parent_path == self.parent.parent_path
            && self.kind.as_ref().map_or(true, |kind| key.kind == *kind)
    }

    pub fn canonical_prefix(&self) -> String {
        let base = if self.parent.parent_path.is_empty() {
            format!(
                "{}/{}/{}/",
                NS_PREFIX, self.parent.namespace, DIRECT_CHILD_SEP
            )
        } else {
            format!(
                "{}/{}/{}/{}/",
                NS_PREFIX, self.parent.namespace, self.parent.parent_path, DIRECT_CHILD_SEP
            )
        };
        match self.kind.as_deref() {
            Some(kind) => format!("{base}{kind}/"),
            None => base,
        }
    }
}

impl fmt::Display for ResourceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical())
    }
}

impl fmt::Display for ResourceList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical_prefix())
    }
}

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

fn parse_path(path: &str) -> Result<Vec<ResourceSegment>> {
    if path.is_empty() {
        return Ok(Vec::new());
    }
    let parts = path.split('/').collect::<Vec<_>>();
    let chunks = parts.chunks_exact(2);
    if !chunks.remainder().is_empty() {
        bail!("resource path must contain kind/name pairs: {path}");
    }
    chunks
        .map(|chunk| Ok(ResourceSegment::new(chunk[0], dec(chunk[1])?)))
        .collect()
}

fn resource_key(
    namespace: &str,
    parent: &[(&str, &str)],
    child_kind: &str,
    child_name: &str,
) -> ResourceKey {
    ResourceKey::new(namespace, parent, child_kind, child_name)
}

fn direct_child_prefix(
    namespace: &str,
    parent: &[(&str, &str)],
    child_kind: Option<&str>,
) -> ResourceList {
    ResourceList {
        parent: ResourceParent {
            namespace: namespace.to_string(),
            parent_path: path(parent),
        },
        kind: child_kind.map(str::to_string),
    }
}

pub fn direct_child_name(prefix: &ResourceList, key: &ResourceKey) -> Option<String> {
    prefix.matches(key).then(|| key.name.clone())
}

pub fn namespace_metadata(name: &str) -> ResourceKey {
    resource_key(ns::TALON_SYSTEM, &[], "Namespace", name)
}

pub fn namespace_metadata_prefix() -> ResourceList {
    direct_child_prefix(ns::TALON_SYSTEM, &[], Some("Namespace"))
}

pub fn namespace_ref(parent: Option<&str>, child_segment: &str) -> ResourceKey {
    let ref_namespace = parent.unwrap_or(ns::TALON_SYSTEM);
    resource_key(ref_namespace, &[], "NamespaceRef", child_segment)
}

pub fn namespace_ref_prefix(parent: Option<&str>) -> ResourceList {
    let ref_namespace = parent.unwrap_or(ns::TALON_SYSTEM);
    direct_child_prefix(ref_namespace, &[], Some("NamespaceRef"))
}

pub fn agent(namespace: &str, id: &str) -> ResourceKey {
    resource_key(namespace, &[], "Agent", id)
}

pub fn agent_prefix(namespace: &str) -> ResourceList {
    direct_child_prefix(namespace, &[], Some("Agent"))
}

pub fn session(namespace: &str, agent: &str, session_id: &str) -> ResourceKey {
    resource_key(namespace, &[("Agent", agent)], "Session", session_id)
}

pub fn session_parent(namespace: &str, agent: &str, session_id: &str) -> ResourceParent {
    session(namespace, agent, session_id).as_parent()
}

pub fn session_prefix(namespace: &str, agent: &str) -> ResourceList {
    direct_child_prefix(namespace, &[("Agent", agent)], Some("Session"))
}

pub fn session_message(
    namespace: &str,
    agent: &str,
    session_id: &str,
    message_id: &str,
) -> ResourceKey {
    resource_key(
        namespace,
        &[("Agent", agent), ("Session", session_id)],
        "SessionMessage",
        message_id,
    )
}

pub fn session_message_parent(
    namespace: &str,
    agent: &str,
    session_id: &str,
    message_id: &str,
) -> ResourceParent {
    session_message(namespace, agent, session_id, message_id).as_parent()
}

pub fn session_message_prefix(namespace: &str, agent: &str, session_id: &str) -> ResourceList {
    direct_child_prefix(
        namespace,
        &[("Agent", agent), ("Session", session_id)],
        Some("SessionMessage"),
    )
}

pub fn session_submission(
    namespace: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
) -> ResourceKey {
    resource_key(
        namespace,
        &[("Agent", agent), ("Session", session_id)],
        "SessionSubmission",
        submission_id,
    )
}

pub fn session_submission_prefix(namespace: &str, agent: &str, session_id: &str) -> ResourceList {
    direct_child_prefix(
        namespace,
        &[("Agent", agent), ("Session", session_id)],
        Some("SessionSubmission"),
    )
}

pub fn session_journal_entry(
    namespace: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    journal_entry_id: &str,
) -> ResourceKey {
    resource_key(
        namespace,
        &[
            ("Agent", agent),
            ("Session", session_id),
            ("SessionSubmission", submission_id),
        ],
        "SessionJournalEntry",
        journal_entry_id,
    )
}

pub fn session_journal_entry_prefix(
    namespace: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
) -> ResourceList {
    direct_child_prefix(
        namespace,
        &[
            ("Agent", agent),
            ("Session", session_id),
            ("SessionSubmission", submission_id),
        ],
        Some("SessionJournalEntry"),
    )
}

pub fn session_permission_decision(
    namespace: &str,
    agent: &str,
    session_id: &str,
    request_id: &str,
) -> ResourceKey {
    resource_key(
        namespace,
        &[("Agent", agent), ("Session", session_id)],
        "PermissionDecision",
        request_id,
    )
}

pub fn channel(namespace: &str, name: &str) -> ResourceKey {
    resource_key(namespace, &[], "Channel", name)
}

pub fn channel_prefix(namespace: &str) -> ResourceList {
    direct_child_prefix(namespace, &[], Some("Channel"))
}

pub fn channel_parent(namespace: &str, name: &str) -> ResourceParent {
    channel(namespace, name).as_parent()
}

pub fn channel_message(namespace: &str, channel: &str, message_id: &str) -> ResourceKey {
    resource_key(
        namespace,
        &[("Channel", channel)],
        "ChannelMessage",
        message_id,
    )
}

pub fn channel_message_prefix(namespace: &str, channel: &str) -> ResourceList {
    direct_child_prefix(namespace, &[("Channel", channel)], Some("ChannelMessage"))
}

pub fn channel_subscription(namespace: &str, channel: &str, name: &str) -> ResourceKey {
    resource_key(
        namespace,
        &[("Channel", channel)],
        "ChannelSubscription",
        name,
    )
}

pub fn channel_subscription_prefix(namespace: &str, channel: &str) -> ResourceList {
    direct_child_prefix(
        namespace,
        &[("Channel", channel)],
        Some("ChannelSubscription"),
    )
}

pub fn schedule(namespace: &str, name: &str) -> ResourceKey {
    resource_key(namespace, &[], "Schedule", name)
}

pub fn schedule_prefix(namespace: &str) -> ResourceList {
    direct_child_prefix(namespace, &[], Some("Schedule"))
}

pub fn workflow(namespace: &str, name: &str) -> ResourceKey {
    resource_key(namespace, &[], "Workflow", name)
}

pub fn workflow_prefix(namespace: &str) -> ResourceList {
    direct_child_prefix(namespace, &[], Some("Workflow"))
}

pub fn workflow_run(namespace: &str, workflow: &str, run_id: &str) -> ResourceKey {
    resource_key(namespace, &[("Workflow", workflow)], "WorkflowRun", run_id)
}

pub fn workflow_run_prefix(namespace: &str, workflow: &str) -> ResourceList {
    direct_child_prefix(namespace, &[("Workflow", workflow)], Some("WorkflowRun"))
}

pub fn workflow_step_run(
    namespace: &str,
    workflow: &str,
    run_id: &str,
    step_run_id: &str,
) -> ResourceKey {
    resource_key(
        namespace,
        &[("Workflow", workflow), ("WorkflowRun", run_id)],
        "WorkflowStepRun",
        step_run_id,
    )
}

pub fn workflow_step_run_prefix(namespace: &str, workflow: &str, run_id: &str) -> ResourceList {
    direct_child_prefix(
        namespace,
        &[("Workflow", workflow), ("WorkflowRun", run_id)],
        Some("WorkflowStepRun"),
    )
}

pub fn workflow_run_event(
    namespace: &str,
    workflow: &str,
    run_id: &str,
    event_id: &str,
) -> ResourceKey {
    resource_key(
        namespace,
        &[("Workflow", workflow), ("WorkflowRun", run_id)],
        "WorkflowRunEvent",
        event_id,
    )
}

pub fn workflow_run_event_prefix(namespace: &str, workflow: &str, run_id: &str) -> ResourceList {
    direct_child_prefix(
        namespace,
        &[("Workflow", workflow), ("WorkflowRun", run_id)],
        Some("WorkflowRunEvent"),
    )
}

pub fn mcp_server(name: &str) -> ResourceKey {
    resource_key(ns::TALON_SYSTEM, &[], "McpServer", name)
}

pub fn mcp_server_prefix() -> ResourceList {
    direct_child_prefix(ns::TALON_SYSTEM, &[], Some("McpServer"))
}

pub fn mcp_server_binding(namespace: &str, name: &str) -> ResourceKey {
    resource_key(namespace, &[], "McpServerBinding", name)
}

pub fn mcp_server_binding_prefix(namespace: &str) -> ResourceList {
    direct_child_prefix(namespace, &[], Some("McpServerBinding"))
}

pub fn agent_memory(namespace: &str, agent: &str, path: &str) -> ResourceKey {
    resource_key(namespace, &[("Agent", agent)], "Memory", path)
}

pub fn agent_memory_prefix(namespace: &str, agent: &str) -> ResourceList {
    direct_child_prefix(namespace, &[("Agent", agent)], Some("Memory"))
}

pub fn knowledge(namespace: &str, path: &str) -> ResourceKey {
    resource_key(namespace, &[], "Knowledge", path)
}

pub fn knowledge_prefix(namespace: &str) -> ResourceList {
    direct_child_prefix(namespace, &[], Some("Knowledge"))
}

pub fn knowledge_resource(namespace: &str, name: &str) -> ResourceKey {
    resource_key(namespace, &[], "KnowledgeResource", name)
}

pub fn knowledge_resource_prefix(namespace: &str) -> ResourceList {
    direct_child_prefix(namespace, &[], Some("KnowledgeResource"))
}

pub fn skill(namespace: &str, name: &str) -> ResourceKey {
    resource_key(namespace, &[], "Skill", name)
}

pub fn skill_prefix(namespace: &str) -> ResourceList {
    direct_child_prefix(namespace, &[], Some("Skill"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_keys_place_separator_before_leaf() {
        assert_eq!(
            agent("Impala:Talon", "hello-agent").canonical(),
            "@Namespace/Impala:Talon/@/Agent/hello-agent"
        );
        assert_eq!(
            session("Impala:Talon", "hello-agent", "session-id").canonical(),
            "@Namespace/Impala:Talon/Agent/hello-agent/@/Session/session-id"
        );
        assert_eq!(
            session_message("Impala:Talon", "hello-agent", "session-id", "message-id")
                .canonical(),
            "@Namespace/Impala:Talon/Agent/hello-agent/Session/session-id/@/SessionMessage/message-id"
        );
        assert_eq!(
            session_submission("Impala:Talon", "hello-agent", "session-id", "submission-id")
                .canonical(),
            "@Namespace/Impala:Talon/Agent/hello-agent/Session/session-id/@/SessionSubmission/submission-id"
        );
        assert_eq!(
            session_journal_entry(
                "Impala:Talon",
                "hello-agent",
                "session-id",
                "submission-id",
                "000001"
            )
            .canonical(),
            "@Namespace/Impala:Talon/Agent/hello-agent/Session/session-id/SessionSubmission/submission-id/@/SessionJournalEntry/000001"
        );
        assert_eq!(
            channel("Impala:Talon", "incident-123").canonical(),
            "@Namespace/Impala:Talon/@/Channel/incident-123"
        );
        assert_eq!(
            channel_message("Impala:Talon", "incident-123", "msg-1").canonical(),
            "@Namespace/Impala:Talon/Channel/incident-123/@/ChannelMessage/msg-1"
        );
        assert_eq!(
            channel_subscription("Impala:Talon", "incident-123", "researcher").canonical(),
            "@Namespace/Impala:Talon/Channel/incident-123/@/ChannelSubscription/researcher"
        );
    }

    #[test]
    fn prefixes_distinguish_direct_and_recursive_listing() {
        assert_eq!(
            session_prefix("Impala:Talon", "hello-agent").canonical_prefix(),
            "@Namespace/Impala:Talon/Agent/hello-agent/@/Session/"
        );
        assert_eq!(
            session("Impala:Talon", "hello-agent", "session-id")
                .as_parent()
                .parent_path,
            "Agent/hello-agent/Session/session-id"
        );
        assert_eq!(
            session_submission_prefix("Impala:Talon", "hello-agent", "session-id")
                .canonical_prefix(),
            "@Namespace/Impala:Talon/Agent/hello-agent/Session/session-id/@/SessionSubmission/"
        );
        assert_eq!(
            session_journal_entry_prefix("Impala:Talon", "hello-agent", "session-id", "submission-id")
                .canonical_prefix(),
            "@Namespace/Impala:Talon/Agent/hello-agent/Session/session-id/SessionSubmission/submission-id/@/SessionJournalEntry/"
        );
    }

    #[test]
    fn names_are_encoded_per_resource_segment() {
        assert_eq!(
            knowledge("quickstart", "docs/hello world.md").canonical(),
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

    #[test]
    fn mcp_keys_use_resource_store_kinds() {
        assert_eq!(
            mcp_server("conic").canonical(),
            "@Namespace/Sys/@/McpServer/conic"
        );
        assert_eq!(
            mcp_server_prefix().canonical_prefix(),
            "@Namespace/Sys/@/McpServer/"
        );
        assert_eq!(
            mcp_server_binding("Conic:Customers:13", "conic").canonical(),
            "@Namespace/Conic:Customers:13/@/McpServerBinding/conic"
        );
        assert_eq!(
            mcp_server_binding_prefix("Conic:Customers:13").canonical_prefix(),
            "@Namespace/Conic:Customers:13/@/McpServerBinding/"
        );
    }

    #[test]
    fn canonical_keys_parse_into_structured_columns() {
        let key = ResourceKey::parse_canonical(
            "@Namespace/quickstart/Agent/hello-agent/Session/s/@/SessionMessage/m%2F1",
        )
        .unwrap();
        assert_eq!(key.namespace, "quickstart");
        assert_eq!(key.parent_path, "Agent/hello-agent/Session/s");
        assert_eq!(key.kind, "SessionMessage");
        assert_eq!(key.name, "m/1");
        assert_eq!(
            key.canonical(),
            "@Namespace/quickstart/Agent/hello-agent/Session/s/@/SessionMessage/m%2F1"
        );
    }
}
