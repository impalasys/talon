// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::ns;
use anyhow::{anyhow, bail, Result};
use sha2::{Digest, Sha256};
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

pub fn connector_registration_id(class_namespace: &str, class_name: &str) -> String {
    format!(
        "Namespace/{}/ConnectorClass/{}",
        enc(class_namespace),
        enc(class_name)
    )
}

pub fn parse_connector_registration_id(registration_id: &str) -> Result<(String, String)> {
    let parts = registration_id.split('/').collect::<Vec<_>>();
    if parts.len() != 4 || parts[0] != "Namespace" || parts[2] != "ConnectorClass" {
        bail!("connector registration id must be Namespace/<namespace>/ConnectorClass/<name>");
    }
    let namespace = dec(parts[1])?;
    let class_name = dec(parts[3])?;
    if namespace.trim().is_empty() || class_name.trim().is_empty() {
        bail!("connector registration id namespace and ConnectorClass name are required");
    }
    Ok((namespace, class_name))
}

pub fn connector_route(class_namespace: &str, class_name: &str, name: &str) -> ResourceKey {
    resource_key(
        class_namespace,
        &[("ConnectorClass", class_name)],
        "Route",
        name,
    )
}

pub fn connector_route_prefix(class_namespace: &str, class_name: &str) -> ResourceList {
    direct_child_prefix(
        class_namespace,
        &[("ConnectorClass", class_name)],
        Some("Route"),
    )
}

pub fn connector_event(class_namespace: &str, class_name: &str, event_id: &str) -> ResourceKey {
    resource_key(
        class_namespace,
        &[("ConnectorClass", class_name)],
        "Event",
        event_id,
    )
}

pub fn connector_event_prefix(class_namespace: &str, class_name: &str) -> ResourceList {
    direct_child_prefix(
        class_namespace,
        &[("ConnectorClass", class_name)],
        Some("Event"),
    )
}

pub fn connector_session(class_namespace: &str, class_name: &str, name: &str) -> ResourceKey {
    resource_key(
        class_namespace,
        &[("ConnectorClass", class_name)],
        "Session",
        name,
    )
}

pub fn connector_session_prefix(class_namespace: &str, class_name: &str) -> ResourceList {
    direct_child_prefix(
        class_namespace,
        &[("ConnectorClass", class_name)],
        Some("Session"),
    )
}

pub fn agent(namespace: &str, id: &str) -> ResourceKey {
    resource_key(namespace, &[], "Agent", id)
}

pub fn agent_prefix(namespace: &str) -> ResourceList {
    direct_child_prefix(namespace, &[], Some("Agent"))
}

pub fn file(namespace: &str, name: &str) -> ResourceKey {
    resource_key(namespace, &[], "File", name)
}

pub fn file_name_for_path(path: &str) -> String {
    let slug = path
        .trim_matches('/')
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(48)
        .collect::<String>();
    let hash = format!("{:x}", Sha256::digest(path.as_bytes()));
    format!(
        "{}-{}",
        if slug.is_empty() { "file" } else { &slug },
        &hash[..12]
    )
}

pub fn file_prefix(namespace: &str) -> ResourceList {
    direct_child_prefix(namespace, &[], Some("File"))
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

pub fn artifact(namespace: &str, agent: &str, session_id: &str, artifact_id: &str) -> ResourceKey {
    resource_key(
        namespace,
        &[("Agent", agent), ("Session", session_id)],
        "Artifact",
        artifact_id,
    )
}

pub fn artifact_prefix(namespace: &str, agent: &str, session_id: &str) -> ResourceList {
    direct_child_prefix(
        namespace,
        &[("Agent", agent), ("Session", session_id)],
        Some("Artifact"),
    )
}

pub fn artifact_access_name(target_agent: &str, target_session_id: &str) -> String {
    format!("{target_agent}:{target_session_id}")
}

pub fn artifact_access(
    namespace: &str,
    agent: &str,
    session_id: &str,
    artifact_id: &str,
    target_agent: &str,
    target_session_id: &str,
) -> ResourceKey {
    resource_key(
        namespace,
        &[
            ("Agent", agent),
            ("Session", session_id),
            ("Artifact", artifact_id),
        ],
        "ArtifactAccess",
        &artifact_access_name(target_agent, target_session_id),
    )
}

pub fn artifact_access_prefix(
    namespace: &str,
    agent: &str,
    session_id: &str,
    artifact_id: &str,
) -> ResourceList {
    direct_child_prefix(
        namespace,
        &[
            ("Agent", agent),
            ("Session", session_id),
            ("Artifact", artifact_id),
        ],
        Some("ArtifactAccess"),
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

pub fn async_a2a_wakeup(
    namespace: &str,
    agent: &str,
    session_id: &str,
    wakeup_id: &str,
) -> ResourceKey {
    resource_key(
        namespace,
        &[("Agent", agent), ("Session", session_id)],
        "AsyncA2AWakeup",
        wakeup_id,
    )
}

pub fn async_a2a_wakeup_prefix(namespace: &str, agent: &str, session_id: &str) -> ResourceList {
    direct_child_prefix(
        namespace,
        &[("Agent", agent), ("Session", session_id)],
        Some("AsyncA2AWakeup"),
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

pub fn mcp_server(namespace: &str, name: &str) -> ResourceKey {
    resource_key(namespace, &[], "McpServer", name)
}

pub fn mcp_server_prefix(namespace: &str) -> ResourceList {
    direct_child_prefix(namespace, &[], Some("McpServer"))
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
        assert_eq!(
            file("Impala:Talon", "brand-guidelines-md-7f3a").canonical(),
            "@Namespace/Impala:Talon/@/File/brand-guidelines-md-7f3a"
        );
        assert_eq!(
            connector_route("Impala:Talon", "slack", "team\u{1f}teamId=T123").canonical(),
            "@Namespace/Impala:Talon/ConnectorClass/slack/@/Route/team%1FteamId%3DT123"
        );
    }

    #[test]
    fn connector_registration_id_round_trips_encoded_segments() {
        let registration_id = connector_registration_id("customer/acme", "slack:prod");
        assert_eq!(
            registration_id,
            "Namespace/customer%2Facme/ConnectorClass/slack%3Aprod"
        );
        assert_eq!(
            parse_connector_registration_id(&registration_id).unwrap(),
            ("customer/acme".to_string(), "slack:prod".to_string())
        );
        assert!(parse_connector_registration_id("reg_abc").is_err());
        assert!(parse_connector_registration_id("Namespace/acme/Connector/slack").is_err());
        assert!(parse_connector_registration_id("Namespace//ConnectorClass/slack").is_err());
        assert!(parse_connector_registration_id("Namespace/acme/ConnectorClass/").is_err());
    }

    #[test]
    fn file_names_are_derived_from_full_logical_path() {
        let acme = file_name_for_path("/memory/acme/brand-guidelines.md");
        let conic = file_name_for_path("/memory/conic/brand-guidelines.md");
        assert_ne!(acme, conic);
        assert!(acme.starts_with("memory-acme-brand-guidelines-md-"));
        assert!(conic.starts_with("memory-conic-brand-guidelines-md-"));
        assert_eq!(acme.rsplit_once('-').unwrap().1.len(), 12);
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
        assert_eq!(
            file_prefix("Impala:Talon").canonical_prefix(),
            "@Namespace/Impala:Talon/@/File/"
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
            mcp_server("Conic:Customers:13", "conic").canonical(),
            "@Namespace/Conic:Customers:13/@/McpServer/conic"
        );
        assert_eq!(
            mcp_server_prefix("Conic:Customers:13").canonical_prefix(),
            "@Namespace/Conic:Customers:13/@/McpServer/"
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
