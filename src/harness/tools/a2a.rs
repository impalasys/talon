// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use prost::Message;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::control::{keys, scheduling, session_queue, ControlPlane, ProtoKeyValueStoreExt};
use crate::gateway::rpc::{data_proto, manifests};
use crate::harness::skills::registry::ToolRegistry;

const A2A_WIRE_METADATA_PREFIX: &str = "wire.a2a.talon.impalasys.com/";
const OWNER_ALIAS: &str = "owner";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct OpenedAgentWire {
    pub alias: String,
    pub connection: String,
    pub reference: AgentWireRef,
    pub reused: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SentAgentWireMessage {
    pub target_alias: String,
    pub reference: AgentWireRef,
    pub queue: String,
    pub queue_entry_id: String,
    pub message_id: Option<String>,
    pub dispatched: bool,
    pub submission_id: Option<String>,
    pub artifact_uris: Vec<String>,
}

pub(super) fn register(registry: &mut ToolRegistry, spec: &manifests::AgentSpec) {
    let internal_connections = crate::harness::a2a::internal_connection_names(spec);
    if !internal_connections.is_empty() {
        registry.register_builtin(
            super::AGENT_OPEN_TOOL,
            "Open a declared internal A2A agent wire and return a reusable wire name.",
            json!({
                "type": "object",
                "properties": {
                    "connection": {
                        "type": "string",
                        "description": "Declared internal A2A connection name.",
                        "enum": internal_connections
                    },
                    "name": {
                        "type": "string",
                        "description": "Optional wire alias. Defaults to <connection>-1, such as critic-1."
                    }
                },
                "required": ["connection"]
            }),
        );
    }

    registry.register_builtin(
        super::AGENT_SEND_TOOL,
        "Send a message into an opened A2A agent wire asynchronously. Child agents are expected to use target \"owner\" to respond back and finish assigned work.",
        json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "Opened wire name, such as critic-1. In child sessions, owner is the session that opened this wire and should receive completion/review replies."
                },
                "message": {
                    "type": "string",
                    "description": "Message to enqueue for the target agent session. Talon adds a sender prefix such as @owner or @critic-1 for the receiver."
                },
                "artifact_uri": {
                    "type": "string",
                    "description": "Optional artifact:// URI to grant to the target session for ad hoc sharing. Delegated Task outputs grant owner access through update_task output_artifact_uri."
                },
                "artifact_uris": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional artifact:// URIs to grant to the target session for ad hoc sharing. Delegated Task outputs grant owner access through update_task output_artifact_uris."
                }
            },
            "required": ["target", "message"]
        }),
    );

    registry.register_builtin(
        super::AGENT_STATUS_TOOL,
        "Inspect an opened A2A agent wire and optionally a message previously returned by agent_send.",
        json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "Opened wire name, such as critic-1. In child sessions, owner refers to the session that opened this wire."
                },
                "message_id": {
                    "type": "string",
                    "description": "Optional messageId returned by agent_send."
                }
            },
            "required": ["target"]
        }),
    );
}

pub(super) async fn execute(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    name: &str,
    args: &Value,
) -> Result<Option<String>> {
    match name {
        super::AGENT_OPEN_TOOL => agent_open(
            cp,
            current_namespace,
            current_agent,
            current_session,
            spec,
            args,
        )
        .await
        .map(Some),
        super::AGENT_SEND_TOOL => {
            agent_send(cp, current_namespace, current_agent, current_session, args)
                .await
                .map(Some)
        }
        super::AGENT_STATUS_TOOL => {
            agent_status(cp, current_namespace, current_agent, current_session, args)
                .await
                .map(Some)
        }
        _ => Ok(None),
    }
}

async fn agent_open(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    args: &Value,
) -> Result<String> {
    let connection_name = super::req_str(args, "connection")?;
    let alias = match super::opt_str(args, "name") {
        Some(name) => validate_alias(name)?.to_string(),
        None => default_alias(connection_name),
    };
    let opened = open_or_reuse_wire(
        cp,
        current_namespace,
        current_agent,
        current_session,
        spec,
        connection_name,
        &alias,
        Default::default(),
    )
    .await?;

    Ok(open_response(opened)?)
}

async fn agent_send(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<String> {
    let target_alias = normalize_agent_uri(super::req_str(args, "target")?)?;
    let message = super::req_str(args, "message")?;
    let artifact_uris = requested_artifact_uris(args)?;
    let sent = send_wire_message(
        cp,
        current_namespace,
        current_agent,
        current_session,
        &target_alias,
        message,
        &artifact_uris,
        Default::default(),
    )
    .await?;

    Ok(serde_json::to_string_pretty(&json!({
        "accepted": true,
        "target": sent.target_alias,
        "status": if sent.dispatched { "DISPATCHED" } else { "QUEUED" },
        "messageId": sent.message_id,
        "artifactCount": sent.artifact_uris.len()
    }))?)
}

async fn agent_status(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<String> {
    let target_alias = normalize_agent_uri(super::req_str(args, "target")?)?;
    let message_id = super::opt_str(args, "message_id").map(str::to_string);
    let target = load_wire(
        cp,
        current_namespace,
        current_agent,
        current_session,
        &target_alias,
    )
    .await?
    .ok_or_else(|| anyhow!("agent wire '{}' is not open", target_alias))?;

    let session = cp
        .kv
        .get_msg::<data_proto::Session>(&keys::session(
            &target.namespace,
            &target.agent,
            &target.session_id,
        ))
        .await?
        .ok_or_else(|| anyhow!("target agent session '{}' not found", target.session_id))?;
    let queue = queued_message_status(cp, &target, message_id.as_deref()).await?;
    let active = active_message_id(cp, &target, message_id.as_deref()).await?;
    let pending = queue.pending_count;
    let status = wire_status(&session, pending, active.as_deref());
    let message_ids = status_message_ids(active.as_deref(), &queue.pending_entry_ids);
    let detail = if message_ids.is_empty() {
        format!(
            "{target_alias} has no active or pending messages. If you are waiting for a reply, please standby and do not poll agent_status."
        )
    } else {
        format!(
            "{target_alias} is currently {} your message(s) {}. If you are waiting for a reply, please standby and do not poll agent_status.",
            status_phrase(&status),
            message_ids.join(", ")
        )
    };
    let summary = json!({
        "status": status,
        "pending": pending,
        "active": active,
    });

    Ok(format!("{}\n{}", serde_json::to_string(&summary)?, detail))
}

pub(super) async fn open_or_reuse_wire(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    connection_name: &str,
    alias: &str,
    labels: HashMap<String, String>,
) -> Result<OpenedAgentWire> {
    let target = crate::harness::a2a::resolve_internal_connection(spec, connection_name)?;
    let alias = validate_alias(alias)?.to_string();
    if let Some(existing) = load_wire(
        cp,
        current_namespace,
        current_agent,
        current_session,
        &alias,
    )
    .await?
    {
        refresh_wire_session_labels(cp, &existing, labels).await?;
        return Ok(OpenedAgentWire {
            alias,
            connection: target.connection_name,
            reference: existing,
            reused: true,
        });
    }

    let child_session_id = scheduling::create_session_with_labels(
        cp,
        &target.target_namespace,
        &target.target_agent,
        labels,
    )
    .await?;
    let child = AgentWireRef {
        namespace: target.target_namespace.clone(),
        agent: target.target_agent.clone(),
        session_id: child_session_id,
    };
    let owner = AgentWireRef {
        namespace: current_namespace.to_string(),
        agent: current_agent.to_string(),
        session_id: current_session.to_string(),
    };
    upsert_session_wire(
        cp,
        current_namespace,
        current_agent,
        current_session,
        &alias,
        &child,
    )
    .await?;
    upsert_session_wire(
        cp,
        &target.target_namespace,
        &target.target_agent,
        &child.session_id,
        OWNER_ALIAS,
        &owner,
    )
    .await?;

    Ok(OpenedAgentWire {
        alias,
        connection: target.connection_name,
        reference: child,
        reused: false,
    })
}

pub(super) async fn load_wire_ref(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    alias: &str,
) -> Result<Option<AgentWireRef>> {
    let alias = normalize_agent_uri(alias)?;
    load_wire(
        cp,
        current_namespace,
        current_agent,
        current_session,
        &alias,
    )
    .await
}

pub(super) async fn send_wire_message(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    target_alias: &str,
    message: &str,
    artifact_uris: &[String],
    labels: HashMap<String, String>,
) -> Result<SentAgentWireMessage> {
    let target_alias = normalize_agent_uri(target_alias)?;
    let target = load_wire(
        cp,
        current_namespace,
        current_agent,
        current_session,
        &target_alias,
    )
    .await?
    .ok_or_else(|| anyhow!("agent wire '{}' is not open", target_alias))?;

    for artifact_uri in artifact_uris {
        grant_artifact_to_session(
            cp,
            current_agent,
            current_session,
            artifact_uri,
            &target.agent,
            &target.session_id,
        )
        .await?;
    }

    let sender_alias = sender_alias_for_target(
        cp,
        current_namespace,
        current_agent,
        current_session,
        &target,
    )
    .await?;
    let queued_message = wire_message(&sender_alias, message, artifact_uris);
    let queued = session_queue::queue_text_message(
        cp.kv.as_ref(),
        &target.namespace,
        &target.agent,
        &target.session_id,
        session_queue::NEXT_QUEUE,
        &queued_message,
        labels,
        chrono::Utc::now(),
    )
    .await?;
    let dispatched = session_queue::dispatch_next_queued_message(
        cp.kv.as_ref(),
        cp.pubsub.as_ref(),
        &target.namespace,
        &target.agent,
        &target.session_id,
        session_queue::NEXT_QUEUE,
        chrono::Utc::now(),
    )
    .await?;

    Ok(SentAgentWireMessage {
        target_alias,
        reference: target,
        queue: queued.queue,
        queue_entry_id: queued.entry_id,
        message_id: dispatched.as_ref().map(|entry| entry.message_id.clone()),
        dispatched: dispatched.is_some(),
        submission_id: dispatched.map(|entry| entry.submission_id),
        artifact_uris: artifact_uris.to_vec(),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AgentWireRef {
    pub namespace: String,
    pub agent: String,
    pub session_id: String,
}

fn metadata_key(alias: &str) -> String {
    format!("{A2A_WIRE_METADATA_PREFIX}{alias}")
}

fn default_alias(connection: &str) -> String {
    format!("{connection}-1")
}

fn validate_alias(alias: &str) -> Result<&str> {
    let alias = alias.trim();
    if alias.is_empty()
        || alias.contains('/')
        || alias.chars().any(char::is_control)
        || alias.starts_with('-')
    {
        return Err(anyhow!("agent wire alias is invalid"));
    }
    Ok(alias)
}

fn normalize_agent_uri(value: &str) -> Result<String> {
    Ok(validate_alias(value)?.to_string())
}

fn encode_wire_ref(reference: &AgentWireRef) -> String {
    format!(
        "{}/{}/{}",
        reference.namespace, reference.agent, reference.session_id
    )
}

fn decode_wire_ref(value: &str) -> Result<AgentWireRef> {
    let parts = value.split('/').collect::<Vec<_>>();
    let [namespace, agent, session_id] = parts.as_slice() else {
        return Err(anyhow!("agent wire metadata is malformed"));
    };
    if [namespace, agent, session_id]
        .iter()
        .any(|part| part.trim().is_empty() || part.chars().any(char::is_control))
    {
        return Err(anyhow!("agent wire metadata contains invalid values"));
    }
    Ok(AgentWireRef {
        namespace: (*namespace).to_string(),
        agent: (*agent).to_string(),
        session_id: (*session_id).to_string(),
    })
}

async fn load_wire(
    cp: &ControlPlane,
    namespace: &str,
    agent: &str,
    session_id: &str,
    alias: &str,
) -> Result<Option<AgentWireRef>> {
    let session = cp
        .kv
        .get_msg::<data_proto::Session>(&keys::session(namespace, agent, session_id))
        .await?
        .ok_or_else(|| anyhow!("session '{}' not found", session_id))?;
    session
        .metadata
        .get(&metadata_key(alias))
        .map(|value| decode_wire_ref(value))
        .transpose()
}

async fn upsert_session_wire(
    cp: &ControlPlane,
    namespace: &str,
    agent: &str,
    session_id: &str,
    alias: &str,
    reference: &AgentWireRef,
) -> Result<()> {
    let key = keys::session(namespace, agent, session_id);
    for _ in 0..8 {
        let current = cp
            .kv
            .get(&key)
            .await?
            .ok_or_else(|| anyhow!("session '{}' not found", session_id))?;
        let mut session = data_proto::Session::decode(current.as_slice())?;
        session
            .metadata
            .insert(metadata_key(alias), encode_wire_ref(reference));
        if cp
            .kv
            .compare_and_swap(&key, Some(current.as_slice()), &session.encode_to_vec())
            .await?
        {
            return Ok(());
        }
    }
    Err(anyhow!("failed to update session A2A wire metadata"))
}

async fn refresh_wire_session_labels(
    cp: &ControlPlane,
    reference: &AgentWireRef,
    labels: HashMap<String, String>,
) -> Result<()> {
    if labels.is_empty() {
        return Ok(());
    }
    let key = keys::session(
        &reference.namespace,
        &reference.agent,
        &reference.session_id,
    );
    for _ in 0..8 {
        let current = cp
            .kv
            .get(&key)
            .await?
            .ok_or_else(|| anyhow!("session '{}' not found", reference.session_id))?;
        let mut session = data_proto::Session::decode(current.as_slice())?;
        session.labels.extend(labels.clone());
        if cp
            .kv
            .compare_and_swap(&key, Some(current.as_slice()), &session.encode_to_vec())
            .await?
        {
            return Ok(());
        }
    }
    Err(anyhow!("failed to update session A2A wire labels"))
}

fn open_response(opened: OpenedAgentWire) -> Result<String> {
    Ok(serde_json::to_string_pretty(&json!({
        "name": opened.alias,
        "connection": opened.connection,
        "status": if opened.reused { "OPEN" } else { "CREATED" },
        "reused": opened.reused,
        "reverseTarget": OWNER_ALIAS
    }))?)
}

fn requested_artifact_uris(args: &Value) -> Result<Vec<String>> {
    let mut uris = Vec::new();
    if let Some(uri) = super::opt_str(args, "artifact_uri") {
        uris.push(uri.to_string());
    }
    if let Some(values) = args.get("artifact_uris") {
        let Some(values) = values.as_array() else {
            return Err(anyhow!("artifact_uris must be an array"));
        };
        for value in values {
            let Some(uri) = value.as_str() else {
                return Err(anyhow!("artifact_uris must contain strings"));
            };
            uris.push(uri.to_string());
        }
    }
    if let Some(message) = super::opt_str(args, "message") {
        uris.extend(artifact_uris_from_text(message));
    }
    uris.sort();
    uris.dedup();
    Ok(uris)
}

async fn grant_artifact_to_session(
    cp: &ControlPlane,
    current_agent: &str,
    current_session: &str,
    artifact_uri: &str,
    target_agent: &str,
    target_session_id: &str,
) -> Result<()> {
    let (uri, _) = super::resolve_artifact_uri(
        cp,
        current_agent,
        current_session,
        artifact_uri,
        super::OP_READ,
    )
    .await?;
    let access = data_proto::ArtifactAccess {
        target_agent: target_agent.to_string(),
        target_session_id: target_session_id.to_string(),
        operations: vec![super::OP_READ.to_string(), super::OP_METADATA.to_string()],
        expires_at: super::default_access_expiry(),
        granted_by_agent: current_agent.to_string(),
        granted_by_session_id: current_session.to_string(),
        created_at: chrono::Utc::now().timestamp_micros(),
    };
    cp.kv
        .set_msg(
            &keys::artifact_access(
                &uri.namespace,
                &uri.agent,
                &uri.session_id,
                &uri.artifact_id,
                target_agent,
                target_session_id,
            ),
            &access,
        )
        .await?;
    Ok(())
}

fn artifact_uris_from_text(text: &str) -> Vec<String> {
    let mut uris = text
        .split_whitespace()
        .filter_map(|token| {
            token
                .trim_matches(|ch: char| {
                    matches!(ch, ',' | '.' | ')' | ']' | '"' | '\'' | '`' | ';' | ':')
                })
                .strip_prefix("artifact://")
                .map(|tail| format!("artifact://{tail}"))
        })
        .collect::<Vec<_>>();
    uris.sort();
    uris.dedup();
    uris
}

async fn sender_alias_for_target(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    target: &AgentWireRef,
) -> Result<String> {
    let Some(session) = cp
        .kv
        .get_msg::<data_proto::Session>(&keys::session(
            &target.namespace,
            &target.agent,
            &target.session_id,
        ))
        .await?
    else {
        return Ok(current_agent.to_string());
    };
    let current = AgentWireRef {
        namespace: current_namespace.to_string(),
        agent: current_agent.to_string(),
        session_id: current_session.to_string(),
    };
    for (key, value) in &session.metadata {
        let Some(alias) = key.strip_prefix(A2A_WIRE_METADATA_PREFIX) else {
            continue;
        };
        if decode_wire_ref(value).ok().as_ref() == Some(&current) {
            return Ok(alias.to_string());
        }
    }
    Ok(current_agent.to_string())
}

fn wire_message(sender_alias: &str, message: &str, artifact_uris: &[String]) -> String {
    let message = message_with_artifacts(message, artifact_uris);
    format!("From @{sender_alias}:\n\n{message}")
}

fn message_with_artifacts(message: &str, artifact_uris: &[String]) -> String {
    if artifact_uris.is_empty() {
        return message.to_string();
    }

    let mut message = message.trim_end().to_string();
    message.push_str("\n\nAttached artifacts:");
    for artifact_uri in artifact_uris {
        message.push_str("\n- ");
        message.push_str(artifact_uri);
    }
    message
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct QueuedWireStatus {
    pending_count: usize,
    pending_entry_ids: Vec<String>,
}

async fn queued_message_status(
    cp: &ControlPlane,
    target: &AgentWireRef,
    message_id: Option<&str>,
) -> Result<QueuedWireStatus> {
    let prefix = keys::session_queue_prefix(
        &target.namespace,
        &target.agent,
        &target.session_id,
        session_queue::NEXT_QUEUE,
    );
    let entries = cp.kv.list_entries(&prefix, None).await?;
    let mut pending_entry_ids = Vec::new();
    for (key, bytes) in entries {
        let entry_id = keys::direct_child_name(&prefix, &key).unwrap_or_default();
        let message = data_proto::SessionMessage::decode(bytes.as_slice())?;
        if message_id.is_none_or(|id| id == message.id || id == entry_id) {
            pending_entry_ids.push(entry_id);
        }
    }
    Ok(QueuedWireStatus {
        pending_count: pending_entry_ids.len(),
        pending_entry_ids,
    })
}

async fn active_message_id(
    cp: &ControlPlane,
    target: &AgentWireRef,
    message_id: Option<&str>,
) -> Result<Option<String>> {
    let prefix =
        keys::session_submission_prefix(&target.namespace, &target.agent, &target.session_id);
    let entries = cp.kv.list_entries(&prefix, None).await?;
    let mut submissions = Vec::new();
    for (_, bytes) in entries {
        let submission = data_proto::SessionSubmission::decode(bytes.as_slice())?;
        if message_id.is_none_or(|id| submission.user_message_id == id)
            && !session_submission_is_terminal(&submission)
        {
            submissions.push(submission);
        }
    }
    Ok(submissions
        .into_iter()
        .max_by_key(|submission| (submission.created_at, submission.updated_at))
        .map(|submission| submission.user_message_id))
}

fn session_submission_is_terminal(submission: &data_proto::SessionSubmission) -> bool {
    submission.status == data_proto::SessionSubmissionStatus::Committed as i32
        || submission.status == data_proto::SessionSubmissionStatus::Failed as i32
        || submission.status == data_proto::SessionSubmissionStatus::Interrupted as i32
}

fn wire_status(session: &data_proto::Session, pending: usize, active: Option<&str>) -> String {
    if active.is_some() || session.status == "PROCESSING" {
        "PROCESSING".to_string()
    } else if pending > 0 {
        "PENDING".to_string()
    } else {
        session.status.clone()
    }
}

fn status_phrase(status: &str) -> &'static str {
    match status {
        "PENDING" => "yet to process",
        "PROCESSING" => "processing",
        _ => "not processing",
    }
}

fn status_message_ids(active: Option<&str>, pending_entry_ids: &[String]) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(active) = active {
        ids.push(active.to_string());
    }
    ids.extend(pending_entry_ids.iter().cloned());
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{resources::ResourceStore, KeyValueStore};
    use crate::gateway::rpc::resources_proto;
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use std::sync::Arc;

    async fn put_agent(kv: Arc<MockKvStore>, namespace: &str, name: &str) {
        let store = ResourceStore::new(kv, Arc::new(RecordingPubSub::default()));
        store
            .upsert(
                namespace,
                resources_proto::Resource {
                    api_version: "talon.impalasys.com/v1".to_string(),
                    kind: "Agent".to_string(),
                    metadata: Some(resources_proto::ResourceMeta {
                        name: name.to_string(),
                        namespace: namespace.to_string(),
                        ..Default::default()
                    }),
                    spec: Some(resources_proto::ResourceSpec {
                        kind: Some(resources_proto::resource_spec::Kind::Agent(
                            resources_proto::AgentSpec::default(),
                        )),
                    }),
                    status: Some(resources_proto::ResourceStatus {
                        kind: Some(resources_proto::resource_status::Kind::Agent(
                            resources_proto::AgentStatus::default(),
                        )),
                    }),
                },
            )
            .await
            .unwrap();
    }

    async fn put_session(kv: &MockKvStore, namespace: &str, agent: &str, session_id: &str) {
        kv.set_msg(
            &keys::session(namespace, agent, session_id),
            &data_proto::Session {
                id: session_id.to_string(),
                agent: agent.to_string(),
                ns: namespace.to_string(),
                status: "IDLE".to_string(),
                created_at: 1,
                last_active: 1,
                metadata: Default::default(),
                labels: Default::default(),
            },
        )
        .await
        .unwrap();
    }

    fn spec_with_critic() -> manifests::AgentSpec {
        manifests::AgentSpec {
            a2a: Some(manifests::A2a {
                connections: vec![manifests::Connection {
                    name: "critic".to_string(),
                    target: Some(manifests::ConnectionRef {
                        internal: Some(manifests::InternalConnectionRef {
                            namespace: "Tenant:acme:Copywriter".to_string(),
                            agent: "critic-agent".to_string(),
                        }),
                        external: None,
                    }),
                    ..Default::default()
                }],
                agent_card: None,
            }),
            ..Default::default()
        }
    }

    fn status_json(output: &str) -> Value {
        serde_json::from_str(output.lines().next().unwrap()).unwrap()
    }

    #[test]
    fn normalize_agent_uri_accepts_wire_name() {
        assert_eq!(normalize_agent_uri("critic-1").unwrap(), "critic-1");
        assert!(normalize_agent_uri("agent://critic-1").is_err());
    }

    #[test]
    fn default_alias_uses_connection_name() {
        assert_eq!(default_alias("critic"), "critic-1");
    }

    #[test]
    fn wire_ref_round_trips() {
        let reference = AgentWireRef {
            namespace: "Tenant:conic:Nexus".to_string(),
            agent: "critic".to_string(),
            session_id: "session-1".to_string(),
        };
        assert_eq!(
            decode_wire_ref(&encode_wire_ref(&reference)).unwrap(),
            reference
        );
    }

    #[test]
    fn artifact_uris_from_text_extracts_and_deduplicates_uris() {
        let uris = artifact_uris_from_text(
            "Pass: artifact://Tenant:acme:Ops/writer/session-1/draft, \
             duplicate artifact://Tenant:acme:Ops/writer/session-1/draft. \
             Also artifact://Tenant:acme:Ops/writer/session-1/notes)",
        );

        assert_eq!(
            uris,
            vec![
                "artifact://Tenant:acme:Ops/writer/session-1/draft",
                "artifact://Tenant:acme:Ops/writer/session-1/notes",
            ]
        );
    }

    #[test]
    fn message_with_artifacts_appends_visible_uris() {
        let message = message_with_artifacts(
            "Done.",
            &[
                "artifact://Tenant:acme:Ops/writer/session-1/draft".to_string(),
                "artifact://Tenant:acme:Ops/writer/session-1/notes".to_string(),
            ],
        );

        assert_eq!(
            message,
            "Done.\n\nAttached artifacts:\n- artifact://Tenant:acme:Ops/writer/session-1/draft\n- artifact://Tenant:acme:Ops/writer/session-1/notes"
        );
    }

    #[tokio::test]
    async fn agent_open_and_send_wire_forward_and_reverse_wires() {
        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        let cp = ControlPlane::builder(kv.clone(), pubsub.clone()).build();
        put_agent(kv.clone(), "Tenant:acme:Main", "cmo").await;
        put_agent(kv.clone(), "Tenant:acme:Copywriter", "critic-agent").await;
        put_session(kv.as_ref(), "Tenant:acme:Main", "cmo", "parent-session").await;

        let opened = agent_open(
            &cp,
            "Tenant:acme:Main",
            "cmo",
            "parent-session",
            &spec_with_critic(),
            &json!({"connection": "critic"}),
        )
        .await
        .unwrap();
        let opened: Value = serde_json::from_str(&opened).unwrap();
        assert_eq!(opened["name"], "critic-1");
        assert!(opened.get("agentUri").is_none());
        assert_eq!(opened["status"], "CREATED");
        let child_ref = load_wire_ref(&cp, "Tenant:acme:Main", "cmo", "parent-session", "critic-1")
            .await
            .unwrap()
            .expect("wire should be stored on parent session");
        let child_session = child_ref.session_id.as_str();

        let parent = kv
            .get_msg::<data_proto::Session>(&keys::session(
                "Tenant:acme:Main",
                "cmo",
                "parent-session",
            ))
            .await
            .unwrap()
            .unwrap();
        let expected_child_ref = format!("Tenant:acme:Copywriter/critic-agent/{child_session}");
        assert_eq!(
            parent
                .metadata
                .get("wire.a2a.talon.impalasys.com/critic-1")
                .map(String::as_str),
            Some(expected_child_ref.as_str())
        );
        let child = kv
            .get_msg::<data_proto::Session>(&keys::session(
                "Tenant:acme:Copywriter",
                "critic-agent",
                child_session,
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            child
                .metadata
                .get("wire.a2a.talon.impalasys.com/owner")
                .map(String::as_str),
            Some("Tenant:acme:Main/cmo/parent-session")
        );
        assert!(child.labels.is_empty());

        let sent = agent_send(
            &cp,
            "Tenant:acme:Main",
            "cmo",
            "parent-session",
            &json!({"target": "critic-1", "message": "Review this."}),
        )
        .await
        .unwrap();
        assert!(!sent.contains("Review this."));
        let sent: Value = serde_json::from_str(&sent).unwrap();
        assert_eq!(sent["target"], "critic-1");
        assert!(sent.get("agentUri").is_none());
        assert_eq!(sent["status"], "DISPATCHED");
        assert_eq!(sent["artifactCount"], 0);
        let submissions = kv
            .list_entries(
                &keys::session_submission_prefix(
                    "Tenant:acme:Copywriter",
                    "critic-agent",
                    child_session,
                ),
                None,
            )
            .await
            .unwrap();
        assert_eq!(submissions.len(), 1);
        let child_messages = kv
            .list_entries(
                &keys::session_message_prefix(
                    "Tenant:acme:Copywriter",
                    "critic-agent",
                    child_session,
                ),
                None,
            )
            .await
            .unwrap();
        assert_eq!(child_messages.len(), 1);
        let child_message =
            data_proto::SessionMessage::decode(child_messages[0].1.as_slice()).unwrap();
        assert_eq!(
            child_message.parts.first().unwrap().content,
            "From @owner:\n\nReview this."
        );

        let status = agent_status(
            &cp,
            "Tenant:acme:Main",
            "cmo",
            "parent-session",
            &json!({"target": "critic-1", "message_id": sent["messageId"]}),
        )
        .await
        .unwrap();
        assert!(status.contains("please standby and do not poll agent_status"));
        let status = status_json(&status);
        assert_eq!(status["status"], "PROCESSING");
        assert_eq!(status["pending"], 0);
        assert_eq!(status["active"], sent["messageId"]);

        let queued_while_processing = agent_send(
            &cp,
            "Tenant:acme:Main",
            "cmo",
            "parent-session",
            &json!({"target": "critic-1", "message": "Follow up."}),
        )
        .await
        .unwrap();
        assert!(!queued_while_processing.contains("Follow up."));
        let queued_while_processing: Value =
            serde_json::from_str(&queued_while_processing).unwrap();
        assert_eq!(queued_while_processing["status"], "QUEUED");
        assert!(queued_while_processing["messageId"].is_null());
        let queued_status = agent_status(
            &cp,
            "Tenant:acme:Main",
            "cmo",
            "parent-session",
            &json!({
                "target": "critic-1"
            }),
        )
        .await
        .unwrap();
        assert!(queued_status.contains("please standby and do not poll agent_status"));
        let queued_status = status_json(&queued_status);
        assert_eq!(queued_status["status"], "PROCESSING");
        assert_eq!(queued_status["pending"], 1);
        assert_eq!(queued_status["active"], sent["messageId"]);

        let mut child = kv
            .get_msg::<data_proto::Session>(&keys::session(
                "Tenant:acme:Copywriter",
                "critic-agent",
                child_session,
            ))
            .await
            .unwrap()
            .unwrap();
        child.status = "IDLE".to_string();
        kv.set_msg(
            &keys::session("Tenant:acme:Copywriter", "critic-agent", child_session),
            &child,
        )
        .await
        .unwrap();

        let reverse = agent_send(
            &cp,
            "Tenant:acme:Copywriter",
            "critic-agent",
            child_session,
            &json!({"target": "owner", "message": "Looks good."}),
        )
        .await
        .unwrap();
        let reverse: Value = serde_json::from_str(&reverse).unwrap();
        assert_eq!(reverse["status"], "DISPATCHED");
        let parent_submissions = kv
            .list_entries(
                &keys::session_submission_prefix("Tenant:acme:Main", "cmo", "parent-session"),
                None,
            )
            .await
            .unwrap();
        assert_eq!(parent_submissions.len(), 1);
        let parent_messages = kv
            .list_entries(
                &keys::session_message_prefix("Tenant:acme:Main", "cmo", "parent-session"),
                None,
            )
            .await
            .unwrap();
        assert_eq!(parent_messages.len(), 1);
        let parent_message =
            data_proto::SessionMessage::decode(parent_messages[0].1.as_slice()).unwrap();
        assert_eq!(
            parent_message.parts.first().unwrap().content,
            "From @critic-1:\n\nLooks good."
        );
    }
}
