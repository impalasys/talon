// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use prost::Message;
use serde_json::{json, Value};

use crate::control::{keys, scheduling, session_queue, ControlPlane, ProtoKeyValueStoreExt};
use crate::gateway::rpc::{data_proto, manifests};
use crate::harness::skills::registry::ToolRegistry;

const A2A_WIRE_METADATA_PREFIX: &str = "wire.a2a.talon.impalasys.com/";
const OWNER_ALIAS: &str = "owner";
const AGENT_URI_PREFIX: &str = "agent://";

pub(super) fn register(registry: &mut ToolRegistry, spec: &manifests::AgentSpec) {
    let internal_connections = crate::harness::a2a::internal_connection_names(spec);
    if !internal_connections.is_empty() {
        registry.register_builtin(
            super::AGENT_OPEN_TOOL,
            "Open a declared internal A2A agent wire and return a reusable agent:// alias.",
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
        "Send a message into an opened A2A agent wire asynchronously.",
        json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "Opened wire alias or agent:// alias, such as critic-1, agent://critic-1, or owner."
                },
                "message": {
                    "type": "string",
                    "description": "Message to enqueue for the target agent session."
                },
                "artifact_uri": {
                    "type": "string",
                    "description": "Optional artifact:// URI to grant to the target session."
                },
                "artifact_uris": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional artifact:// URIs to grant to the target session."
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
                    "description": "Opened wire alias or agent:// alias, such as critic-1, agent://critic-1, or owner."
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
    let target = crate::harness::a2a::resolve_internal_connection(spec, connection_name)?;
    let alias = match super::opt_str(args, "name") {
        Some(name) => validate_alias(name)?.to_string(),
        None => default_alias(&target.connection_name),
    };

    if let Some(existing) = load_wire(
        cp,
        current_namespace,
        current_agent,
        current_session,
        &alias,
    )
    .await?
    {
        return Ok(open_response(
            &alias,
            &target.connection_name,
            existing,
            true,
        )?);
    }

    let child_session_id = scheduling::create_session_with_labels(
        cp,
        &target.target_namespace,
        &target.target_agent,
        Default::default(),
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

    Ok(open_response(
        &alias,
        &target.connection_name,
        child,
        false,
    )?)
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
    let target = load_wire(
        cp,
        current_namespace,
        current_agent,
        current_session,
        &target_alias,
    )
    .await?
    .ok_or_else(|| anyhow!("agent wire '{}' is not open", target_alias))?;

    let artifact_uris = requested_artifact_uris(args)?;
    for artifact_uri in &artifact_uris {
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

    let queued_message = message_with_artifacts(message, &artifact_uris);
    let queued = session_queue::queue_text_message(
        cp.kv.as_ref(),
        &target.namespace,
        &target.agent,
        &target.session_id,
        session_queue::NEXT_QUEUE,
        &queued_message,
        Default::default(),
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

    Ok(serde_json::to_string_pretty(&json!({
        "accepted": true,
        "target": target_alias,
        "agentUri": format!("{AGENT_URI_PREFIX}{target_alias}"),
        "namespace": target.namespace,
        "agent": target.agent,
        "sessionId": target.session_id,
        "queue": queued.queue,
        "queueEntryId": queued.entry_id,
        "messageId": dispatched.as_ref().map(|entry| entry.message_id.as_str()),
        "dispatched": dispatched.is_some(),
        "submissionId": dispatched.as_ref().map(|entry| entry.submission_id.as_str()),
        "artifactUris": artifact_uris
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
    let submission = submission_status(cp, &target, message_id.as_deref()).await?;
    let canonical_message_exists = match message_id.as_deref() {
        Some(id) => cp
            .kv
            .get(&keys::session_message(
                &target.namespace,
                &target.agent,
                &target.session_id,
                id,
            ))
            .await?
            .is_some(),
        None => false,
    };

    Ok(serde_json::to_string_pretty(&json!({
        "target": target_alias,
        "agentUri": format!("{AGENT_URI_PREFIX}{target_alias}"),
        "namespace": target.namespace,
        "agent": target.agent,
        "sessionId": target.session_id,
        "sessionStatus": session.status,
        "lastActive": session.last_active,
        "messageId": message_id,
        "message": {
            "queued": queue["messageQueued"].as_bool().unwrap_or(false),
            "canonicalMessageExists": canonical_message_exists,
            "submission": submission,
        },
        "queue": queue,
    }))?)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentWireRef {
    namespace: String,
    agent: String,
    session_id: String,
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
    let value = value.trim();
    let alias = value.strip_prefix(AGENT_URI_PREFIX).unwrap_or(value);
    Ok(validate_alias(alias)?.to_string())
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

fn open_response(
    alias: &str,
    connection: &str,
    reference: AgentWireRef,
    reused: bool,
) -> Result<String> {
    Ok(serde_json::to_string_pretty(&json!({
        "agentUri": format!("{AGENT_URI_PREFIX}{alias}"),
        "name": alias,
        "connection": connection,
        "namespace": reference.namespace,
        "agent": reference.agent,
        "sessionId": reference.session_id,
        "reused": reused,
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

async fn queued_message_status(
    cp: &ControlPlane,
    target: &AgentWireRef,
    message_id: Option<&str>,
) -> Result<Value> {
    let prefix = keys::session_queue_prefix(
        &target.namespace,
        &target.agent,
        &target.session_id,
        session_queue::NEXT_QUEUE,
    );
    let entries = cp.kv.list_entries(&prefix, None).await?;
    let mut queued_messages = Vec::new();
    let mut matched = None;
    for (key, bytes) in entries {
        let entry_id = keys::direct_child_name(&prefix, &key).unwrap_or_default();
        let message = data_proto::SessionMessage::decode(bytes.as_slice())?;
        let item = json!({
            "entryId": entry_id,
            "messageId": message.id,
            "createdAt": message.created_at,
        });
        if message_id.is_some_and(|id| id == message.id) {
            matched = Some(item.clone());
        }
        queued_messages.push(item);
    }
    let message_queued = message_id.map_or(!queued_messages.is_empty(), |_| matched.is_some());
    Ok(json!({
        "name": session_queue::NEXT_QUEUE,
        "pendingCount": queued_messages.len(),
        "oldest": queued_messages.first(),
        "messageQueued": message_queued,
        "matchedMessage": matched,
    }))
}

async fn submission_status(
    cp: &ControlPlane,
    target: &AgentWireRef,
    message_id: Option<&str>,
) -> Result<Value> {
    let prefix =
        keys::session_submission_prefix(&target.namespace, &target.agent, &target.session_id);
    let entries = cp.kv.list_entries(&prefix, None).await?;
    let mut submissions = Vec::new();
    for (_, bytes) in entries {
        let submission = data_proto::SessionSubmission::decode(bytes.as_slice())?;
        if message_id.is_none_or(|id| submission.user_message_id == id) {
            submissions.push(submission);
        }
    }
    let latest = submissions
        .into_iter()
        .max_by_key(|submission| (submission.created_at, submission.updated_at));
    Ok(latest
        .as_ref()
        .map(session_submission_json)
        .unwrap_or(Value::Null))
}

fn session_submission_json(submission: &data_proto::SessionSubmission) -> Value {
    json!({
        "submissionId": submission.submission_id,
        "userMessageId": submission.user_message_id,
        "status": session_submission_status_name(submission.status),
        "currentPhase": session_execution_phase_name(submission.current_phase),
        "attemptId": submission.attempt_id,
        "attemptCount": submission.attempt_count,
        "claimWorkerId": submission.claim_worker_id,
        "claimExpiresAt": submission.claim_expires_at,
        "createdAt": submission.created_at,
        "updatedAt": submission.updated_at,
        "completedAt": submission.completed_at,
        "committedMessageId": submission.committed_message_id,
        "currentJournalEntryId": submission.current_journal_entry_id,
    })
}

fn session_submission_status_name(status: i32) -> &'static str {
    match status {
        value if value == data_proto::SessionSubmissionStatus::Pending as i32 => "PENDING",
        value if value == data_proto::SessionSubmissionStatus::Claimed as i32 => "CLAIMED",
        value if value == data_proto::SessionSubmissionStatus::Committed as i32 => "COMMITTED",
        value if value == data_proto::SessionSubmissionStatus::Failed as i32 => "FAILED",
        value if value == data_proto::SessionSubmissionStatus::Interrupted as i32 => "INTERRUPTED",
        _ => "UNSPECIFIED",
    }
}

fn session_execution_phase_name(phase: i32) -> &'static str {
    match phase {
        value if value == data_proto::SessionExecutionPhase::LlmResponse as i32 => "LLM_RESPONSE",
        value if value == data_proto::SessionExecutionPhase::ToolResult as i32 => "TOOL_RESULT",
        value if value == data_proto::SessionExecutionPhase::Committed as i32 => "COMMITTED",
        _ => "UNSPECIFIED",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::resources::ResourceStore;
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

    #[test]
    fn normalize_agent_uri_accepts_alias_or_uri() {
        assert_eq!(normalize_agent_uri("critic-1").unwrap(), "critic-1");
        assert_eq!(normalize_agent_uri("agent://critic-1").unwrap(), "critic-1");
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
        let child_session = opened["sessionId"].as_str().unwrap();
        assert_eq!(opened["agentUri"], "agent://critic-1");

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
        let sent: Value = serde_json::from_str(&sent).unwrap();
        assert_eq!(sent["dispatched"], true);
        assert!(kv
            .get_msg::<data_proto::SessionSubmission>(&keys::session_submission(
                "Tenant:acme:Copywriter",
                "critic-agent",
                child_session,
                sent["submissionId"].as_str().unwrap(),
            ))
            .await
            .unwrap()
            .is_some());

        let status = agent_status(
            &cp,
            "Tenant:acme:Main",
            "cmo",
            "parent-session",
            &json!({"target": "critic-1", "message_id": sent["messageId"]}),
        )
        .await
        .unwrap();
        let status: Value = serde_json::from_str(&status).unwrap();
        assert_eq!(status["sessionStatus"], "PROCESSING");
        assert_eq!(status["message"]["queued"], false);
        assert_eq!(status["message"]["canonicalMessageExists"], true);
        assert_eq!(status["message"]["submission"]["status"], "PENDING");

        let queued_while_processing = agent_send(
            &cp,
            "Tenant:acme:Main",
            "cmo",
            "parent-session",
            &json!({"target": "critic-1", "message": "Follow up."}),
        )
        .await
        .unwrap();
        let queued_while_processing: Value =
            serde_json::from_str(&queued_while_processing).unwrap();
        assert_eq!(queued_while_processing["dispatched"], false);
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
        let queued_status: Value = serde_json::from_str(&queued_status).unwrap();
        assert_eq!(queued_status["message"]["queued"], true);
        assert_eq!(queued_status["message"]["canonicalMessageExists"], false);
        assert_eq!(queued_status["queue"]["pendingCount"], 1);

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
        assert_eq!(reverse["dispatched"], true);
        assert_eq!(reverse["namespace"], "Tenant:acme:Main");
        assert_eq!(reverse["agent"], "cmo");
        assert_eq!(reverse["sessionId"], "parent-session");
    }
}
