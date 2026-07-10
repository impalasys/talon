// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{connectors as connector_rpc, data_proto, proto, GrpcGatewayHandler};
use crate::control::cas::{session_object_key_prefix, SessionCasScope};
use crate::control::scheduling;
use crate::control::topics;
use crate::control::ProtoKeyValueStoreExt;
use crate::control::{events, keys, keys::ResourceParent, KeyValueStore};
use prost::Message;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::OnceLock;

pub(crate) mod watcher;

use watcher::{session_parts_event_stream, session_submission_event_stream, SessionStreamTarget};

const LARGE_SESSION_PAYLOAD_WARNING_BYTES: usize = 128 * 1024;
const DEFAULT_SESSION_MESSAGES_PAGE_SIZE: usize = 50;
const MAX_SESSION_MESSAGES_PAGE_SIZE: usize = 200;
const SESSION_MESSAGE_KEY_SCAN_BATCH_SIZE: usize = 512;
const DEFAULT_SESSION_STREAM_BATCH_MAX: usize = 10_000;
const CLEAR_SESSION_CAS_RETRIES: usize = 8;
const LABEL_CONNECTOR_DELIVERY_ERROR: &str = "talon.impalasys.com/connector-delivery-error";
const LABEL_CONNECTOR_DELIVERY_STATUS: &str = "talon.impalasys.com/connector-delivery-status";
const RESERVED_CONNECTOR_LABEL_PREFIX: &str = "talon.impalasys.com/connector-";
const RESERVED_CONNECTOR_MATCH_LABEL_PREFIX: &str = "talon.impalasys.com/connector-match/";
const RESERVED_EXTERNAL_LABEL_PREFIX: &str = "talon.impalasys.com/external-";

fn is_mutable_connector_delivery_label(key: &str) -> bool {
    key == LABEL_CONNECTOR_DELIVERY_STATUS || key == LABEL_CONNECTOR_DELIVERY_ERROR
}

fn is_reserved_connector_routing_label(key: &str) -> bool {
    let is_connector_or_external = key.starts_with(RESERVED_CONNECTOR_LABEL_PREFIX)
        || key.starts_with(RESERVED_EXTERNAL_LABEL_PREFIX)
        || key.starts_with(RESERVED_CONNECTOR_MATCH_LABEL_PREFIX);
    is_connector_or_external && !is_mutable_connector_delivery_label(key)
}

fn merge_update_session_message_labels(
    existing: &std::collections::HashMap<String, String>,
    requested: std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    let mut labels = requested;
    for (key, value) in existing {
        if is_reserved_connector_routing_label(key) {
            labels.insert(key.clone(), value.clone());
        }
    }
    labels
}

// Session creation charges namespace/agent usage; provider/model are only used
// by LLM metrics and intentionally stay empty here.
fn namespace_usage_subject(
    ns: &str,
    agent: &str,
    rate_limit_key: Option<String>,
) -> crate::control::usage::UsageSubject {
    crate::control::usage::UsageSubject {
        namespace: ns.to_string(),
        agent: agent.to_string(),
        provider: String::new(),
        model: String::new(),
        rate_limit_key,
    }
}

// Attach the authenticated request identity to the session create charge when
// one is available. Identity-scoped policies require this rate-limit key.
fn session_usage_subject_from_request<T>(
    req: &tonic::Request<T>,
    ns: &str,
    agent: &str,
) -> crate::control::usage::UsageSubject {
    namespace_usage_subject(
        ns,
        agent,
        crate::gateway::auth::rate_limit_key_from_request(req),
    )
}

// Charge the successful session creation. If quota admission fails after the
// session row was written, delete that row so rejected creates do not leave
// visible sessions behind.
async fn charge_session_quota_or_delete(
    gateway: &crate::gateway::Gateway,
    session_key: &keys::ResourceKey,
    subject: &crate::control::usage::UsageSubject,
    now_seconds: i64,
) -> std::result::Result<(), tonic::Status> {
    let result = crate::control::usage::charge_namespace_usage_under_limit(
        gateway.kv.as_ref(),
        subject,
        &[crate::control::usage::UsageCharge {
            metric: crate::control::usage::METRIC_AGENT_SESSIONS,
            delta: 1,
        }],
        now_seconds,
    )
    .await;

    if let Err(err) = result {
        if let Err(delete_err) = gateway.kv.delete(session_key).await {
            tracing::warn!(
                key = %session_key,
                error = %delete_err,
                "failed to roll back session after quota admission failure"
            );
            return Err(tonic::Status::internal(
                "Failed to roll back session after quota admission failure",
            ));
        }
        return Err(if crate::control::usage::is_quota_exceeded_error(&err) {
            tonic::Status::resource_exhausted(err.to_string())
        } else {
            tonic::Status::internal(format!("Failed to charge session usage: {}", err))
        });
    }

    Ok(())
}

// Undo a session create after it was already charged. This is only for create
// failure rollback, not normal DeleteSession, because `agent.sessions` counts
// successful creates in the rate-limit window.
async fn rollback_created_session(
    gateway: &crate::gateway::Gateway,
    session_key: &keys::ResourceKey,
    subject: &crate::control::usage::UsageSubject,
    now_seconds: i64,
) -> std::result::Result<(), tonic::Status> {
    if let Err(delete_err) = gateway.kv.delete(session_key).await {
        tracing::warn!(
            key = %session_key,
            error = %delete_err,
            "failed to delete session while rolling back create_session"
        );
        return Err(tonic::Status::internal(
            "Failed to delete session while rolling back create_session",
        ));
    }
    if let Err(refund_err) = crate::control::usage::refund_namespace_usage(
        gateway.kv.as_ref(),
        subject,
        &[crate::control::usage::UsageCharge {
            metric: crate::control::usage::METRIC_AGENT_SESSIONS,
            delta: 1,
        }],
        now_seconds,
    )
    .await
    {
        tracing::warn!(
            key = %session_key,
            error = %refund_err,
            "failed to refund session quota while rolling back create_session"
        );
        return Err(tonic::Status::internal(
            "Failed to refund session quota while rolling back create_session",
        ));
    }
    Ok(())
}

async fn delete_descendants(kv: &dyn KeyValueStore, parent: ResourceParent) -> anyhow::Result<()> {
    let mut stack = vec![parent];
    while let Some(parent) = stack.pop() {
        let list = parent.list(None);
        let children = kv.list_keys(&list).await?;
        for child in children {
            stack.push(child.as_parent());
            kv.delete(&child).await?;
        }
    }
    Ok(())
}

async fn collect_session_tool_result_object_keys(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
) -> anyhow::Result<Vec<String>> {
    let expected_prefix = session_object_key_prefix(&SessionCasScope::new(ns, agent, session_id));
    let mut keys_to_delete = HashSet::new();
    for key in kv
        .list_keys(&keys::session_message_prefix(ns, agent, session_id))
        .await?
    {
        let message = match kv.get_msg::<data_proto::SessionMessage>(&key).await {
            Ok(Some(message)) => message,
            Ok(None) => continue,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    namespace = %ns,
                    agent = %agent,
                    session_id = %session_id,
                    key = %key,
                    "failed to fetch or decode session message while collecting tool result objects for deletion"
                );
                continue;
            }
        };
        for part in message.parts {
            if part.part_type == data_proto::SessionMessagePartType::ToolResult as i32 {
                collect_tool_result_object_key(
                    part.object.as_ref(),
                    &expected_prefix,
                    &mut keys_to_delete,
                );
            }
        }
    }

    for submission_key in kv
        .list_keys(&keys::session_submission_prefix(ns, agent, session_id))
        .await?
    {
        for (_, bytes) in kv
            .list_entries(&keys::session_journal_entry_prefix(
                ns,
                agent,
                session_id,
                &submission_key.name,
            ))
            .await?
        {
            let entry = match data_proto::SessionJournalEntry::decode(bytes.as_slice()) {
                Ok(entry) => entry,
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        namespace = %ns,
                        agent = %agent,
                        session_id = %session_id,
                        submission_id = %submission_key.name,
                        "failed to decode session journal entry while collecting tool result objects for deletion"
                    );
                    continue;
                }
            };
            let object = entry
                .payload
                .as_ref()
                .and_then(|payload| payload.payload.as_ref())
                .and_then(|payload| match payload {
                    data_proto::session_journal_entry_payload::Payload::ToolResult(result) => {
                        result.object.as_ref()
                    }
                    _ => None,
                });
            collect_tool_result_object_key(object, &expected_prefix, &mut keys_to_delete);
        }
    }

    Ok(keys_to_delete.into_iter().collect())
}

fn collect_tool_result_object_key(
    object: Option<&data_proto::ObjectRef>,
    expected_prefix: &str,
    keys_to_delete: &mut HashSet<String>,
) {
    let Some(object) = object else {
        return;
    };
    let is_tool_result = object
        .metadata
        .get("kind")
        .is_some_and(|kind| kind == "tool_result")
        || object.key.contains("/tool-results/")
        || object.key.starts_with("cas/");
    if is_tool_result && object.key.starts_with(expected_prefix) && !object.key.trim().is_empty() {
        keys_to_delete.insert(object.key.clone());
    }
}

fn requested_limit(limit: i32) -> Option<usize> {
    match limit {
        value if value < 0 => Some(0),
        0 => None,
        value => Some(value as usize),
    }
}

fn validated_page_size(page_size: i32) -> std::result::Result<usize, tonic::Status> {
    if page_size < 0 {
        return Err(tonic::Status::invalid_argument(
            "page_size must be non-negative",
        ));
    }
    let page_size = if page_size == 0 {
        DEFAULT_SESSION_MESSAGES_PAGE_SIZE
    } else {
        page_size as usize
    };
    Ok(page_size.min(MAX_SESSION_MESSAGES_PAGE_SIZE))
}

fn stream_session_batch_max() -> usize {
    static CACHE: OnceLock<usize> = OnceLock::new();

    *CACHE.get_or_init(|| {
        std::env::var("TALON_STREAM_SESSION_PARTS_BATCH_MAX")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_SESSION_STREAM_BATCH_MAX)
    })
}

fn normalize_appended_session_message(
    mut message: data_proto::SessionMessage,
) -> data_proto::SessionMessage {
    let now_micros = chrono::Utc::now().timestamp_micros();
    if message.id.is_empty() {
        message.id = crate::control::uuid::session_message_id();
    }
    if message.role == data_proto::MessageRole::RoleUnspecified as i32 {
        message.role = data_proto::MessageRole::RoleUser as i32;
    }
    if message.created_at == 0 {
        message.created_at = now_micros;
    }
    for (index, part) in message.parts.iter_mut().enumerate() {
        if part.id.is_empty() {
            part.id = format!("{index:06}");
        }
        if part.created_at == 0 {
            part.created_at = message.created_at;
        }
    }
    message
}

fn normalize_session_message_parts(
    parts: &mut [data_proto::SessionMessagePart],
    message_created_at: i64,
) {
    for (index, part) in parts.iter_mut().enumerate() {
        if part.id.is_empty() {
            part.id = format!("{index:06}");
        }
        if part.created_at == 0 {
            part.created_at = message_created_at;
        }
    }
}

fn request_permission_part_id(part: &data_proto::SessionMessagePart) -> Option<String> {
    if part.part_type != data_proto::SessionMessagePartType::RequestPermission as i32 {
        return None;
    }
    serde_json::from_str::<Value>(&part.payload_json)
        .ok()
        .and_then(|payload| {
            payload
                .get("requestId")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
}

async fn find_request_permission_message_id(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    request_id: &str,
) -> std::result::Result<Option<String>, tonic::Status> {
    let prefix = keys::session_message_prefix(ns, agent, session_id);
    let mut message_keys = kv.list_keys(&prefix).await.map_err(|err| {
        tonic::Status::internal(format!("Failed to list session messages: {err}"))
    })?;
    message_keys.sort();
    for key in message_keys {
        let Some(bytes) = kv.get(&key).await.map_err(|err| {
            tonic::Status::internal(format!("Failed to fetch session message: {err}"))
        })?
        else {
            continue;
        };
        let Ok(message) = data_proto::SessionMessage::decode(bytes.as_slice()) else {
            continue;
        };
        if message
            .parts
            .iter()
            .any(|part| request_permission_part_id(part).as_deref() == Some(request_id))
        {
            return Ok(Some(message.id));
        }
    }
    Ok(None)
}

fn parse_session_stream_target(
    name: &str,
) -> std::result::Result<SessionStreamTarget, tonic::Status> {
    let key = keys::ResourceKey::parse_canonical(name).map_err(|err| {
        tonic::Status::invalid_argument(format!("invalid session resource name {name:?}: {err}"))
    })?;
    if key.kind != "Session" {
        return Err(tonic::Status::invalid_argument(format!(
            "session resource name must identify a Session, got {}",
            key.kind
        )));
    }

    let parent_segments = key.parent_segments().map_err(|err| {
        tonic::Status::invalid_argument(format!("invalid session parent in {name:?}: {err}"))
    })?;
    if parent_segments.len() != 1 || parent_segments[0].kind != "Agent" {
        return Err(tonic::Status::invalid_argument(format!(
            "session resource name must be under an Agent parent: {name}"
        )));
    }

    Ok(SessionStreamTarget::new(
        key.namespace,
        parent_segments[0].name.clone(),
        key.name,
    ))
}

async fn acquire_clear_session_lock(
    kv: &dyn KeyValueStore,
    key: &keys::ResourceKey,
    now_micros: i64,
) -> std::result::Result<(), tonic::Status> {
    for _ in 0..CLEAR_SESSION_CAS_RETRIES {
        let current = kv
            .get(key)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to fetch session: {}", e)))?;
        let Some(current_bytes) = current.as_ref() else {
            return Err(tonic::Status::not_found("Session not found"));
        };
        let mut session = data_proto::Session::decode(current_bytes.as_slice())
            .map_err(|e| tonic::Status::internal(format!("Failed to decode session: {}", e)))?;

        if session.status == "PROCESSING"
            && now_micros.saturating_sub(session.last_active)
                <= scheduling::session_processing_timeout_micros()
        {
            return Err(tonic::Status::resource_exhausted(
                "Session is currently generating a response.",
            ));
        }

        session.status = "PROCESSING".to_string();
        session.last_active = now_micros;
        let updated = session.encode_to_vec();
        if kv
            .compare_and_swap(key, Some(current_bytes.as_slice()), &updated)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to acquire session lock: {}", e))
            })?
        {
            return Ok(());
        }
    }

    Err(tonic::Status::internal(
        "Failed to atomically acquire session lock",
    ))
}

async fn release_clear_session_lock(
    kv: &dyn KeyValueStore,
    key: &keys::ResourceKey,
    expected_last_active: i64,
) -> std::result::Result<(), tonic::Status> {
    for _ in 0..CLEAR_SESSION_CAS_RETRIES {
        let current = kv
            .get(key)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to fetch session: {}", e)))?;
        let Some(current_bytes) = current.as_ref() else {
            return Err(tonic::Status::not_found("Session not found"));
        };
        let mut session = data_proto::Session::decode(current_bytes.as_slice())
            .map_err(|e| tonic::Status::internal(format!("Failed to decode session: {}", e)))?;

        if session.status != "PROCESSING" || session.last_active != expected_last_active {
            return Err(tonic::Status::internal(
                "Session changed while clearing context",
            ));
        }

        session.status = "IDLE".to_string();
        session.last_active = expected_last_active;
        let updated = session.encode_to_vec();
        if kv
            .compare_and_swap(key, Some(current_bytes.as_slice()), &updated)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to release session lock: {}", e))
            })?
        {
            return Ok(());
        }
    }

    Err(tonic::Status::internal(
        "Failed to atomically release session lock",
    ))
}

impl GrpcGatewayHandler {
    pub async fn handle_create_session(
        &self,
        req: tonic::Request<proto::CreateSessionRequest>,
    ) -> std::result::Result<tonic::Response<proto::SessionResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns, &req.get_ref().agent);
        let session_usage_subject =
            session_usage_subject_from_request(&req, &req.get_ref().ns, &req.get_ref().agent);
        let req = req.into_inner();

        // 1. Verify agent exists in namespace
        let store = crate::control::resources::ResourceStore::new(
            self.gateway.kv.clone(),
            self.gateway.pubsub.clone(),
        );
        let mut agent_exists = store
            .get_agent(&req.ns, &req.agent)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to verify agent: {}", e)))?;
        if agent_exists.is_none() {
            // Some stores can make a just-applied resource visible through
            // prefix listing before an exact point lookup observes it.
            agent_exists = store
                .list(&req.ns, Some("Agent"))
                .await
                .map_err(|e| tonic::Status::internal(format!("Failed to list agents: {}", e)))?
                .into_iter()
                .find(|resource| {
                    resource
                        .metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata.name == req.agent)
                })
                .map(crate::control::resources::agent_from_resource)
                .transpose()
                .map_err(|e| tonic::Status::internal(format!("Failed to decode agent: {}", e)))?;
        }

        if agent_exists.is_none() {
            return Err(tonic::Status::not_found(format!(
                "Agent {} not found in ns {}",
                req.agent, req.ns
            )));
        }

        // Use ULID (UUID v7 gives time-sorted guarantees like ULID)
        let session_id = crate::control::uuid::session_id();
        let session_usage_now = chrono::Utc::now().timestamp();

        let session = data_proto::Session {
            id: session_id.clone(),
            agent: req.agent.clone(),
            ns: req.ns.clone(),
            status: "IDLE".to_string(),
            created_at: chrono::Utc::now().timestamp_micros(),
            last_active: chrono::Utc::now().timestamp_micros(),
            metadata: std::collections::HashMap::new(),
            labels: req.labels.clone(),
        };

        let session_db_key = keys::session(&req.ns, &req.agent, &session_id);

        self.gateway
            .kv
            .set_msg(&session_db_key, &session)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to save session state: {}", e)))?;
        charge_session_quota_or_delete(
            &self.gateway,
            &session_db_key,
            &session_usage_subject,
            session_usage_now,
        )
        .await?;

        let event = events::LifecycleEvent {
            resource_type: "Session".to_string(),
            name: session_id.clone(),
            ns: req.ns.clone(),
            action: events::SystemAction::Create as i32,
            timestamp: chrono::Utc::now().timestamp_micros(),
        };
        if let Err(e) = self
            .gateway
            .pubsub
            .publish(topics::RESOURCE_LIFECYCLE_TOPIC, &event.encode_to_vec())
            .await
        {
            rollback_created_session(
                &self.gateway,
                &session_db_key,
                &session_usage_subject,
                session_usage_now,
            )
            .await?;
            return Err(tonic::Status::internal(format!(
                "Failed to publish event: {}",
                e
            )));
        }

        Ok(tonic::Response::new(proto::SessionResponse {
            session_id,
            agent: req.agent,
            state: "ACTIVE".to_string(),
            messages: vec![],
            labels: req.labels,
        }))
    }

    pub async fn handle_get_session(
        &self,
        req: tonic::Request<proto::GetSessionRequest>,
    ) -> std::result::Result<tonic::Response<proto::SessionResponse>, tonic::Status> {
        crate::require_auth!(
            read,
            self,
            req,
            &req.get_ref().ns,
            &req.get_ref().agent,
            &req.get_ref().session_id
        );
        let req = req.into_inner();
        let message_limit = requested_limit(req.message_limit);

        let session_db_key = keys::session(&req.ns, &req.agent, &req.session_id);
        let msg_prefix = keys::session_message_prefix(&req.ns, &req.agent, &req.session_id);

        let session = self
            .gateway
            .kv
            .get_msg::<data_proto::Session>(&session_db_key)
            .await
            .map_err(|e| {
                tracing::error!(
                    ns = %req.ns,
                    agent = %req.agent,
                    session_id = %req.session_id,
                    key = %session_db_key,
                    error = %e,
                    "failed to fetch session metadata"
                );
                tonic::Status::internal(format!("Failed to fetch session metadata: {}", e))
            })?
            .ok_or_else(|| {
                tracing::warn!(
                    ns = %req.ns,
                    agent = %req.agent,
                    session_id = %req.session_id,
                    key = %session_db_key,
                    "session not found"
                );
                tonic::Status::not_found("Session not found")
            })?;

        let mut messages = Vec::new();
        if message_limit != Some(0) {
            let msg_keys = if let Some(limit) = message_limit {
                let mut page_before_name: Option<String> = None;
                let mut keys = Vec::with_capacity(limit);
                while keys.len() < limit {
                    let page = self
                        .gateway
                        .kv
                        .list_keys_page(
                            &msg_prefix,
                            page_before_name.as_deref(),
                            SESSION_MESSAGE_KEY_SCAN_BATCH_SIZE,
                        )
                        .await
                        .map_err(|e| {
                            tracing::error!(
                                ns = %req.ns,
                                agent = %req.agent,
                                session_id = %req.session_id,
                                prefix = %msg_prefix,
                                error = %e,
                                "failed to page session message keys"
                            );
                            tonic::Status::internal(format!(
                                "Failed to list session messages: {}",
                                e
                            ))
                        })?;
                    if page.is_empty() {
                        break;
                    }
                    page_before_name = page.last().map(|key| key.name.clone());
                    for key in page {
                        keys.push(key);
                        if keys.len() >= limit {
                            break;
                        }
                    }
                }
                keys.sort();
                keys
            } else {
                let mut keys = self.gateway.kv.list_keys(&msg_prefix).await.map_err(|e| {
                    tracing::error!(
                        ns = %req.ns,
                        agent = %req.agent,
                        session_id = %req.session_id,
                        prefix = %msg_prefix,
                        error = %e,
                        "failed to list session messages"
                    );
                    tonic::Status::internal(format!("Failed to list session messages: {}", e))
                })?;
                keys.sort();
                keys
            };
            tracing::info!(
                ns = %req.ns,
                agent = %req.agent,
                session_id = %req.session_id,
                message_key_count = msg_keys.len(),
                "loaded session message keys"
            );

            for key in &msg_keys {
                match self.gateway.kv.get(key).await {
                    Ok(Some(bytes)) => {
                        let payload_bytes = bytes.len();
                        if payload_bytes > LARGE_SESSION_PAYLOAD_WARNING_BYTES {
                            tracing::warn!(
                                ns = %req.ns,
                                agent = %req.agent,
                                session_id = %req.session_id,
                                key = %key,
                                payload_bytes,
                                "session message payload is unusually large"
                            );
                        }

                        match data_proto::SessionMessage::decode(bytes.as_slice()) {
                            Ok(msg) => messages.push(msg),
                            Err(e) => {
                                tracing::error!(
                                    ns = %req.ns,
                                    agent = %req.agent,
                                    session_id = %req.session_id,
                                    key = %key,
                                    payload_bytes,
                                    error = %e,
                                    "failed to decode session message"
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::warn!(
                            ns = %req.ns,
                            agent = %req.agent,
                            session_id = %req.session_id,
                            key = %key,
                            "session message key exists but value is missing"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            ns = %req.ns,
                            agent = %req.agent,
                            session_id = %req.session_id,
                            key = %key,
                            error = %e,
                            "failed to decode session message"
                        );
                    }
                }
            }
        }
        tracing::info!(
            ns = %req.ns,
            agent = %req.agent,
            session_id = %req.session_id,
            message_count = messages.len(),
            "loaded session messages"
        );

        Ok(tonic::Response::new(proto::SessionResponse {
            session_id: session.id,
            agent: session.agent,
            state: session.status,
            messages,
            labels: session.labels,
        }))
    }

    pub async fn handle_list_session_messages(
        &self,
        req: tonic::Request<proto::ListSessionMessagesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListSessionMessagesResponse>, tonic::Status>
    {
        crate::require_auth!(
            read,
            self,
            req,
            &req.get_ref().ns,
            &req.get_ref().agent,
            &req.get_ref().session_id
        );
        let req = req.into_inner();
        let page_size = validated_page_size(req.page_size)?;
        let session_db_key = keys::session(&req.ns, &req.agent, &req.session_id);
        let msg_prefix = keys::session_message_prefix(&req.ns, &req.agent, &req.session_id);

        let session = self
            .gateway
            .kv
            .get_msg::<data_proto::Session>(&session_db_key)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to fetch session metadata: {}", e))
            })?
            .ok_or_else(|| tonic::Status::not_found("Session not found"))?;

        let before_name = req
            .before_message_id
            .as_deref()
            .filter(|before_message_id| !before_message_id.is_empty())
            .map(str::to_string);

        // Fetch one extra message so the response can expose has_more and an
        // exclusive before_message_id cursor without requiring a separate count.
        let target_message_count = page_size + 1;
        let mut scan_before_name = before_name;
        let mut items = Vec::with_capacity(target_message_count);

        while items.len() < target_message_count {
            let entries = self
                .gateway
                .kv
                .list_entries_page(
                    &msg_prefix,
                    scan_before_name.as_deref(),
                    SESSION_MESSAGE_KEY_SCAN_BATCH_SIZE,
                )
                .await
                .map_err(|e| {
                    tonic::Status::internal(format!("Failed to list session messages: {}", e))
                })?;

            if entries.is_empty() {
                break;
            }

            // Continue from the last key returned, regardless of whether it was
            // a message key or another nested descendant.
            scan_before_name = entries.last().map(|(key, _)| key.name.clone());

            let remaining = target_message_count.saturating_sub(items.len());
            let mut page_messages = Vec::with_capacity(remaining);
            for (key, bytes) in entries {
                if page_messages.len() >= remaining {
                    break;
                }
                let payload_bytes = bytes.len();
                if payload_bytes > LARGE_SESSION_PAYLOAD_WARNING_BYTES {
                    tracing::warn!(
                        ns = %req.ns,
                        agent = %req.agent,
                        session_id = %req.session_id,
                        key = %key,
                        payload_bytes,
                        "session message payload is unusually large"
                    );
                }

                let message = match data_proto::SessionMessage::decode(bytes.as_slice()) {
                    Ok(message) => message,
                    Err(e) => {
                        tracing::error!(
                            ns = %req.ns,
                            agent = %req.agent,
                            session_id = %req.session_id,
                            key = %key,
                            payload_bytes,
                            error = %e,
                            "failed to decode session message"
                        );
                        continue;
                    }
                };

                page_messages.push(message);
            }

            items.extend(page_messages.into_iter().map(|message| {
                proto::ListSessionMessagesResponseItem {
                    message: Some(message),
                }
            }));
        }

        let has_more = items.len() > page_size;
        if has_more {
            items.truncate(page_size);
        }

        items.reverse();
        let next_before_message_id = if has_more {
            items
                .first()
                .and_then(|item| item.message.as_ref())
                .map(|message| message.id.clone())
        } else {
            None
        };

        Ok(tonic::Response::new(proto::ListSessionMessagesResponse {
            session_id: session.id,
            agent: session.agent,
            state: session.status,
            items,
            has_more,
            next_before_message_id,
        }))
    }

    pub async fn handle_list_sessions(
        &self,
        req: tonic::Request<proto::ListSessionsRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListSessionsResponse>, tonic::Status> {
        crate::require_auth!(read, self, req, &req.get_ref().ns, &req.get_ref().agent);
        let req = req.into_inner();

        let session_prefix = keys::session_prefix(&req.ns, &req.agent);

        let keys = self
            .gateway
            .kv
            .list_keys(&session_prefix)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to list sessions: {}", e)))?;

        let mut session_ids = Vec::new();
        let mut sessions = Vec::new();
        for key in keys {
            if let Some(session_id) = keys::direct_child_name(&session_prefix, &key) {
                session_ids.push(session_id.clone());

                let session = self
                    .gateway
                    .kv
                    .get_msg::<data_proto::Session>(&key)
                    .await
                    .map_err(|e| {
                        tonic::Status::internal(format!("Failed to fetch session metadata: {}", e))
                    })?;

                if let Some(session) = session {
                    sessions.push(proto::SessionListItem {
                        session_id,
                        updated_at: session.last_active,
                        labels: session.labels,
                    });
                }
            }
        }

        sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));

        Ok(tonic::Response::new(proto::ListSessionsResponse {
            session_ids,
            sessions,
        }))
    }

    pub async fn handle_delete_session(
        &self,
        req: tonic::Request<proto::DeleteSessionRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteSessionResponse>, tonic::Status> {
        crate::require_auth!(
            self,
            req,
            &req.get_ref().ns,
            &req.get_ref().agent,
            &req.get_ref().session_id
        );
        let req = req.into_inner();

        let session_db_key = keys::session(&req.ns, &req.agent, &req.session_id);
        let tool_result_object_keys = collect_session_tool_result_object_keys(
            self.gateway.kv.as_ref(),
            &req.ns,
            &req.agent,
            &req.session_id,
        )
        .await
        .map_err(|e| {
            tonic::Status::internal(format!(
                "Failed to collect session tool result objects: {}",
                e
            ))
        })?;

        delete_descendants(
            self.gateway.kv.as_ref(),
            keys::session_parent(&req.ns, &req.agent, &req.session_id),
        )
        .await
        .map_err(|e| {
            tonic::Status::internal(format!("Failed to delete session descendants: {}", e))
        })?;

        self.gateway
            .kv
            .delete(&session_db_key)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to delete session: {}", e)))?;
        if let Err(error) = crate::control::search::publish_index_event(
            self.gateway.pubsub.as_ref(),
            crate::control::events::IndexEvent {
                operation: crate::control::events::IndexOperation::Delete as i32,
                key: session_db_key.canonical(),
                ..Default::default()
            },
        )
        .await
        {
            tracing::warn!(
                error = %error,
                namespace = %req.ns,
                agent = %req.agent,
                session_id = %req.session_id,
            "failed to publish search delete event for deleted session"
            );
        }

        for object_key in tool_result_object_keys {
            if let Err(error) = self.gateway.objects.delete(&object_key).await {
                tracing::warn!(
                    error = %error,
                    namespace = %req.ns,
                    agent = %req.agent,
                    session_id = %req.session_id,
                    object_key = %object_key,
                    "failed to delete tool result object for deleted session"
                );
            }
        }

        let event = events::LifecycleEvent {
            resource_type: "Session".to_string(),
            name: req.session_id,
            ns: req.ns,
            action: events::SystemAction::Delete as i32,
            timestamp: chrono::Utc::now().timestamp_micros(),
        };
        self.gateway
            .pubsub
            .publish(topics::RESOURCE_LIFECYCLE_TOPIC, &event.encode_to_vec())
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to publish event: {}", e)))?;

        Ok(tonic::Response::new(proto::DeleteSessionResponse {
            success: true,
        }))
    }

    pub async fn handle_clear_session(
        &self,
        req: tonic::Request<proto::ClearSessionRequest>,
    ) -> std::result::Result<tonic::Response<proto::ClearSessionResponse>, tonic::Status> {
        crate::require_auth!(
            self,
            req,
            &req.get_ref().ns,
            &req.get_ref().agent,
            &req.get_ref().session_id
        );
        let req = req.into_inner();

        let session_db_key = keys::session(&req.ns, &req.agent, &req.session_id);
        let now_micros = chrono::Utc::now().timestamp_micros();
        acquire_clear_session_lock(self.gateway.kv.as_ref(), &session_db_key, now_micros).await?;

        if let Err(e) = delete_descendants(
            self.gateway.kv.as_ref(),
            keys::session_parent(&req.ns, &req.agent, &req.session_id),
        )
        .await
        {
            if let Err(release_err) =
                release_clear_session_lock(self.gateway.kv.as_ref(), &session_db_key, now_micros)
                    .await
            {
                tracing::warn!(
                    key = %session_db_key,
                    error = %release_err,
                    "failed to release session lock after clear_session error"
                );
            }
            return Err(tonic::Status::internal(format!(
                "Failed to clear session descendants: {}",
                e
            )));
        }

        release_clear_session_lock(self.gateway.kv.as_ref(), &session_db_key, now_micros).await?;
        if let Err(error) = crate::control::search::publish_index_event(
            self.gateway.pubsub.as_ref(),
            crate::control::events::IndexEvent {
                operation: crate::control::events::IndexOperation::Delete as i32,
                key: session_db_key.canonical(),
                ..Default::default()
            },
        )
        .await
        {
            tracing::warn!(
                error = %error,
                namespace = %req.ns,
                agent = %req.agent,
                session_id = %req.session_id,
                "failed to publish search delete event for cleared session"
            );
        }

        Ok(tonic::Response::new(proto::ClearSessionResponse {
            success: true,
        }))
    }

    pub async fn handle_send_message(
        &self,
        req: tonic::Request<proto::SendMessageRequest>,
    ) -> std::result::Result<tonic::Response<proto::SendMessageResponse>, tonic::Status> {
        crate::require_auth!(
            self,
            req,
            &req.get_ref().ns,
            &req.get_ref().agent,
            &req.get_ref().session_id
        );
        let req = req.into_inner();

        println!(
            "!! Gateway received SendMessage request: session_id={}, message={}",
            req.session_id, req.message
        );

        scheduling::send_message(
            self.gateway.kv.as_ref(),
            self.gateway.pubsub.as_ref(),
            &req.ns,
            &req.agent,
            &req.session_id,
            &req.message,
            req.labels,
            chrono::Utc::now(),
        )
        .await
        .map_err(|e| {
            if e.downcast_ref::<scheduling::SessionCurrentlyProcessingError>()
                .is_some()
            {
                tonic::Status::resource_exhausted("Session is currently generating a response.")
            } else if e.downcast_ref::<scheduling::EmptyMessageError>().is_some() {
                tonic::Status::invalid_argument("message content is required")
            } else if e
                .downcast_ref::<scheduling::SessionNotFoundError>()
                .is_some()
            {
                tonic::Status::not_found("Session not found")
            } else {
                tonic::Status::internal(format!("Failed to send message: {}", e))
            }
        })?;

        Ok(tonic::Response::new(proto::SendMessageResponse {
            reply: "".to_string(), // In async design, reply is polled or streamed later
            session_id: req.session_id,
        }))
    }

    pub async fn handle_append_session_message(
        &self,
        req: tonic::Request<proto::AppendSessionMessageRequest>,
    ) -> std::result::Result<tonic::Response<proto::AppendSessionMessageResponse>, tonic::Status>
    {
        crate::require_auth!(
            self,
            req,
            &req.get_ref().ns,
            &req.get_ref().agent,
            &req.get_ref().session_id
        );
        let req = req.into_inner();

        let session_db_key = keys::session(&req.ns, &req.agent, &req.session_id);
        let mut session = self
            .gateway
            .kv
            .get_msg::<data_proto::Session>(&session_db_key)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to fetch session: {}", e)))?;
        let session = session
            .as_mut()
            .ok_or_else(|| tonic::Status::not_found("Session not found"))?;

        let message = normalize_appended_session_message(
            req.message
                .ok_or_else(|| tonic::Status::invalid_argument("message is required"))?,
        );
        if message.parts.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "message must contain at least one part",
            ));
        }
        let should_deliver_connector_reply =
            message.role == data_proto::MessageRole::RoleAssistant as i32;
        let message_key = keys::session_message(&req.ns, &req.agent, &req.session_id, &message.id);
        let message_id = message.id.clone();
        let inserted = self
            .gateway
            .kv
            .compare_and_swap(&message_key, None, &message.encode_to_vec())
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to append message: {}", e)))?;
        if !inserted {
            return Err(tonic::Status::already_exists(
                "Session message already exists",
            ));
        }

        session.last_active = message.created_at;
        self.gateway
            .kv
            .set_msg(&session_db_key, session)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to update session: {}", e)))?;
        if let Err(error) = crate::control::search::publish_index_event(
            self.gateway.pubsub.as_ref(),
            crate::control::events::IndexEvent {
                operation: crate::control::events::IndexOperation::Upsert as i32,
                key: message_key.canonical(),
                ..Default::default()
            },
        )
        .await
        {
            tracing::warn!(
                error = %error,
                namespace = %req.ns,
                agent = %req.agent,
                session_id = %req.session_id,
                message_id = %message.id,
                "failed to publish search index event for appended session message"
            );
        }

        if should_deliver_connector_reply {
            if let Err(error) = connector_rpc::maybe_deliver_connector_session_message(
                &self.gateway.control_plane(),
                &req.ns,
                &req.agent,
                &req.session_id,
                &message_id,
            )
            .await
            {
                tracing::warn!(
                    error = %error,
                    namespace = %req.ns,
                    agent = %req.agent,
                    session_id = %req.session_id,
                    message_id = %message_id,
                    "failed to deliver appended connector session message"
                );
                return Err(tonic::Status::internal(format!(
                    "Failed to deliver connector session message: {error}"
                )));
            }
        }

        Ok(tonic::Response::new(proto::AppendSessionMessageResponse {
            session_id: req.session_id,
            message: Some(message),
        }))
    }

    pub async fn handle_update_session_message(
        &self,
        req: tonic::Request<proto::UpdateSessionMessageRequest>,
    ) -> std::result::Result<tonic::Response<proto::UpdateSessionMessageResponse>, tonic::Status>
    {
        crate::require_auth!(
            self,
            req,
            &req.get_ref().ns,
            &req.get_ref().agent,
            &req.get_ref().session_id
        );
        let req = req.into_inner();
        if req.message_id.trim().is_empty() {
            return Err(tonic::Status::invalid_argument("message_id is required"));
        }

        let session_db_key = keys::session(&req.ns, &req.agent, &req.session_id);
        let mut session = self
            .gateway
            .kv
            .get_msg::<data_proto::Session>(&session_db_key)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to fetch session: {}", e)))?
            .ok_or_else(|| tonic::Status::not_found("Session not found"))?;

        let message_key =
            keys::session_message(&req.ns, &req.agent, &req.session_id, &req.message_id);
        let mut message = self
            .gateway
            .kv
            .get_msg::<data_proto::SessionMessage>(&message_key)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to fetch session message: {}", e))
            })?
            .ok_or_else(|| tonic::Status::not_found("Session message not found"))?;

        if message.role != data_proto::MessageRole::RoleUser as i32
            && message.role != data_proto::MessageRole::RoleAssistant as i32
        {
            return Err(tonic::Status::failed_precondition(
                "Only user and assistant session messages can be updated",
            ));
        }
        if req.parts.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "message must contain at least one part",
            ));
        }

        message.parts = req.parts;
        message.labels = merge_update_session_message_labels(&message.labels, req.labels);
        normalize_session_message_parts(&mut message.parts, message.created_at);

        self.gateway
            .kv
            .set_msg(&message_key, &message)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to update session message: {}", e))
            })?;

        session.last_active = chrono::Utc::now().timestamp_micros();
        self.gateway
            .kv
            .set_msg(&session_db_key, &session)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to update session: {}", e)))?;

        if let Err(error) = crate::control::search::publish_index_event(
            self.gateway.pubsub.as_ref(),
            crate::control::events::IndexEvent {
                operation: crate::control::events::IndexOperation::Upsert as i32,
                key: message_key.canonical(),
                ..Default::default()
            },
        )
        .await
        {
            tracing::warn!(
                error = %error,
                namespace = %req.ns,
                agent = %req.agent,
                session_id = %req.session_id,
                message_id = %message.id,
                "failed to publish search index event for updated session message"
            );
        }

        if message.role == data_proto::MessageRole::RoleAssistant as i32 {
            if let Err(error) = connector_rpc::maybe_deliver_connector_session_message(
                &self.gateway.control_plane(),
                &req.ns,
                &req.agent,
                &req.session_id,
                &message.id,
            )
            .await
            {
                tracing::warn!(
                    error = %error,
                    namespace = %req.ns,
                    agent = %req.agent,
                    session_id = %req.session_id,
                    message_id = %message.id,
                    "failed to process connector delivery for updated session message"
                );
                return Err(tonic::Status::internal(format!(
                    "Failed to process connector delivery for updated session message: {error}"
                )));
            }
            if let Some(updated_message) = self
                .gateway
                .kv
                .get_msg::<data_proto::SessionMessage>(&message_key)
                .await
                .map_err(|e| {
                    tonic::Status::internal(format!(
                        "Failed to reload updated session message: {}",
                        e
                    ))
                })?
            {
                message = updated_message;
            }
        }

        Ok(tonic::Response::new(proto::UpdateSessionMessageResponse {
            session_id: req.session_id,
            message: Some(message),
        }))
    }

    pub async fn handle_answer_session_permission(
        &self,
        req: tonic::Request<proto::AnswerSessionPermissionRequest>,
    ) -> std::result::Result<tonic::Response<proto::AnswerSessionPermissionResponse>, tonic::Status>
    {
        crate::require_auth!(
            self,
            req,
            &req.get_ref().ns,
            &req.get_ref().agent,
            &req.get_ref().session_id
        );
        let req = req.into_inner();
        if req.request_id.trim().is_empty() {
            return Err(tonic::Status::invalid_argument("request_id is required"));
        }
        let outcome = match req.outcome.as_str() {
            "" | "selected" => "selected",
            "cancelled" => "cancelled",
            value => {
                return Err(tonic::Status::invalid_argument(format!(
                    "unsupported permission outcome '{value}'"
                )))
            }
        };
        if outcome == "selected" && req.option_id.trim().is_empty() {
            return Err(tonic::Status::invalid_argument(
                "option_id is required when outcome is selected",
            ));
        }

        let session_key = keys::session(&req.ns, &req.agent, &req.session_id);
        let session_exists = self
            .gateway
            .kv
            .get_msg::<data_proto::Session>(&session_key)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to fetch session: {err}")))?
            .is_some();
        if !session_exists {
            return Err(tonic::Status::not_found("Session not found"));
        }

        find_request_permission_message_id(
            self.gateway.kv.as_ref(),
            &req.ns,
            &req.agent,
            &req.session_id,
            &req.request_id,
        )
        .await?
        .ok_or_else(|| tonic::Status::not_found("permission request not found"))?;

        let now = chrono::Utc::now().timestamp_micros();
        let outcome_json = if outcome == "selected" {
            json!({
                "outcome": "selected",
                "optionId": req.option_id,
            })
        } else {
            json!({
                "outcome": "cancelled",
            })
        };
        let decision = json!({
            "requestId": req.request_id,
            "status": outcome,
            "outcome": outcome_json,
            "decidedBy": if req.decided_by.trim().is_empty() {
                "user"
            } else {
                req.decided_by.as_str()
            },
            "decidedAt": now,
        });
        let decision_bytes = serde_json::to_vec(&decision).map_err(|err| {
            tonic::Status::internal(format!("Failed to encode permission decision: {err}"))
        })?;
        let decision_key = keys::session_permission_decision(
            &req.ns,
            &req.agent,
            &req.session_id,
            &req.request_id,
        );
        let inserted = self
            .gateway
            .kv
            .compare_and_swap(&decision_key, None, &decision_bytes)
            .await
            .map_err(|err| {
                tonic::Status::internal(format!("Failed to store permission decision: {err}"))
            })?;
        if !inserted {
            return Err(tonic::Status::already_exists(
                "permission request was already answered",
            ));
        }

        Ok(tonic::Response::new(
            proto::AnswerSessionPermissionResponse {
                session_id: req.session_id,
                request_id: req.request_id,
                outcome: outcome.to_string(),
                option_id: req.option_id,
            },
        ))
    }

    pub async fn handle_stop_session_generation(
        &self,
        req: tonic::Request<proto::StopSessionGenerationRequest>,
    ) -> std::result::Result<tonic::Response<proto::StopSessionGenerationResponse>, tonic::Status>
    {
        crate::require_auth!(
            self,
            req,
            &req.get_ref().ns,
            &req.get_ref().agent,
            &req.get_ref().session_id
        );
        let req = req.into_inner();

        let session_db_key = keys::session(&req.ns, &req.agent, &req.session_id);
        let session = self
            .gateway
            .kv
            .get_msg::<data_proto::Session>(&session_db_key)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to fetch session: {}", e)))?;

        if session.is_none() {
            return Err(tonic::Status::not_found("Session not found"));
        }

        let event = events::SessionControlEvent {
            session_id: req.session_id,
            agent: req.agent,
            ns: req.ns,
            action: "stop_generation".to_string(),
            timestamp: chrono::Utc::now().timestamp_micros(),
        };
        self.gateway
            .pubsub
            .publish(topics::SESSION_CONTROL_TOPIC, &event.encode_to_vec())
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to publish stop event: {}", e)))?;

        Ok(tonic::Response::new(proto::StopSessionGenerationResponse {
            success: true,
        }))
    }

    pub async fn handle_stream_session_parts(
        &self,
        req: tonic::Request<proto::StreamSessionPartsRequest>,
    ) -> std::result::Result<
        tonic::Response<
            <GrpcGatewayHandler as proto::session_service_server::SessionService>::StreamPartsStream,
        >,
        tonic::Status,
    >{
        crate::require_auth!(
            read,
            self,
            req,
            &req.get_ref().ns,
            &req.get_ref().agent,
            &req.get_ref().session_id
        );
        let req = req.into_inner();

        let targets = vec![SessionStreamTarget::new(
            req.ns.clone(),
            req.agent.clone(),
            req.session_id.clone(),
        )];
        let event_stream = session_parts_event_stream(
            targets,
            self.gateway.kv.clone(),
            self.gateway.pubsub.clone(),
            self.gateway.worker_connections.clone(),
        );

        Ok(tonic::Response::new(event_stream))
    }

    pub async fn handle_stream_session_parts_batch(
        &self,
        req: tonic::Request<proto::StreamSessionPartsBatchRequest>,
    ) -> std::result::Result<
        tonic::Response<
            <GrpcGatewayHandler as proto::session_service_server::SessionService>::StreamPartsBatchStream,
        >,
        tonic::Status,
    >{
        let batch_max = stream_session_batch_max();
        let request = req.get_ref();
        if request.session_names.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "session_names must contain at least one session",
            ));
        }
        if request.session_names.len() > batch_max {
            return Err(tonic::Status::invalid_argument(format!(
                "session_names contains {} sessions, maximum is {}",
                request.session_names.len(),
                batch_max
            )));
        }

        let targets = request
            .session_names
            .iter()
            .map(|name| parse_session_stream_target(name))
            .collect::<std::result::Result<Vec<_>, _>>()?;

        if let Some(auth_config) = &self.gateway.auth_config {
            for target in &targets {
                crate::gateway::auth::check_auth_for_operation(
                    req.metadata(),
                    auth_config,
                    crate::gateway::auth::AuthzOperation::Read,
                    &target.ns,
                    Some(&target.agent),
                    Some(&target.session_id),
                )?;
            }
        }

        let event_stream = session_parts_event_stream(
            targets,
            self.gateway.kv.clone(),
            self.gateway.pubsub.clone(),
            self.gateway.worker_connections.clone(),
        );

        Ok(tonic::Response::new(event_stream))
    }

    pub async fn handle_submit_session_turn(
        &self,
        req: tonic::Request<proto::SubmitSessionTurnRequest>,
    ) -> std::result::Result<
        tonic::Response<
            <GrpcGatewayHandler as proto::session_service_server::SessionService>::SubmitTurnStream,
        >,
        tonic::Status,
    > {
        crate::require_auth!(
            self,
            req,
            &req.get_ref().ns,
            &req.get_ref().agent,
            &req.get_ref().session_id
        );
        let req = req.into_inner();
        let mut message = req
            .message
            .ok_or_else(|| tonic::Status::invalid_argument("message is required"))?;
        message.labels.extend(req.labels);
        let message = normalize_appended_session_message(message);
        let now = chrono::Utc::now();
        let submission_id = scheduling::send_session_message(
            self.gateway.kv.as_ref(),
            self.gateway.pubsub.as_ref(),
            &req.ns,
            &req.agent,
            &req.session_id,
            message,
            now,
        )
        .await
        .map_err(map_session_submit_error)?;
        let target =
            SessionStreamTarget::new(req.ns.clone(), req.agent.clone(), req.session_id.clone());

        let event_stream = session_submission_event_stream(
            target,
            submission_id,
            self.gateway.kv.clone(),
            self.gateway.pubsub.clone(),
            self.gateway.worker_connections.clone(),
        );
        Ok(tonic::Response::new(event_stream))
    }
}

fn map_session_submit_error(err: anyhow::Error) -> tonic::Status {
    if err
        .downcast_ref::<scheduling::SessionCurrentlyProcessingError>()
        .is_some()
    {
        tonic::Status::resource_exhausted("Session is currently generating a response.")
    } else if err
        .downcast_ref::<scheduling::EmptyMessageError>()
        .is_some()
    {
        tonic::Status::invalid_argument("message content is required")
    } else if err
        .downcast_ref::<scheduling::SessionNotFoundError>()
        .is_some()
    {
        tonic::Status::not_found("Session not found")
    } else {
        tonic::Status::internal(format!("Failed to submit session turn: {}", err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::object_store::{
        InMemoryObjectStore, ObjectMetadata, ObjectStore, StoredObject,
    };
    use crate::control::ControlPlane;
    use crate::control::MessagePublisher;
    use crate::control::ProtoKeyValueStoreExt;
    use crate::gateway::rpc::resources_proto;
    use crate::gateway::Gateway;
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use anyhow::Result;
    use futures::stream;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;

    fn handler(kv: Arc<MockKvStore>, pubsub: Arc<RecordingPubSub>) -> GrpcGatewayHandler {
        let control_plane = ControlPlane::builder(kv, pubsub).build();
        GrpcGatewayHandler {
            gateway: Arc::new(Gateway::from_control_plane(None, control_plane)),
        }
    }

    fn handler_with_objects(
        kv: Arc<MockKvStore>,
        pubsub: Arc<RecordingPubSub>,
        objects: Arc<dyn ObjectStore + Send + Sync>,
    ) -> GrpcGatewayHandler {
        let control_plane = ControlPlane::builder(kv, pubsub).objects(objects).build();
        GrpcGatewayHandler {
            gateway: Arc::new(Gateway::from_control_plane(None, control_plane)),
        }
    }

    struct FailingDeleteObjectStore {
        inner: InMemoryObjectStore,
    }

    #[async_trait::async_trait]
    impl ObjectStore for FailingDeleteObjectStore {
        async fn put(
            &self,
            key: &str,
            bytes: &[u8],
            metadata: ObjectMetadata,
        ) -> Result<data_proto::ObjectRef> {
            self.inner.put(key, bytes, metadata).await
        }

        async fn get(&self, key: &str) -> Result<Option<StoredObject>> {
            self.inner.get(key).await
        }

        async fn delete(&self, _key: &str) -> Result<()> {
            Err(anyhow::anyhow!("delete failed"))
        }
    }

    struct FailingPubSub;

    #[async_trait::async_trait]
    impl MessagePublisher for FailingPubSub {
        async fn publish(&self, _topic: &str, _message: &[u8]) -> anyhow::Result<()> {
            Err(anyhow::anyhow!("publish failed"))
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            Ok(Box::pin(stream::empty()))
        }
    }

    fn failing_publish_handler(kv: Arc<MockKvStore>) -> GrpcGatewayHandler {
        let control_plane = ControlPlane::builder(kv, Arc::new(FailingPubSub)).build();
        GrpcGatewayHandler {
            gateway: Arc::new(Gateway::from_control_plane(None, control_plane)),
        }
    }

    async fn put_agent(kv: Arc<MockKvStore>, pubsub: Arc<RecordingPubSub>, ns: &str, name: &str) {
        let store = crate::control::resources::ResourceStore::new(kv, pubsub);
        store
            .upsert(
                ns,
                resources_proto::Resource {
                    api_version: "talon.impalasys.com/v1".to_string(),
                    kind: "Agent".to_string(),
                    metadata: Some(resources_proto::ResourceMeta {
                        name: name.to_string(),
                        namespace: ns.to_string(),
                        ..Default::default()
                    }),
                    spec: Some(resources_proto::ResourceSpec {
                        kind: Some(resources_proto::resource_spec::Kind::Agent(
                            resources_proto::AgentSpec::default(),
                        )),
                    }),
                    status: None,
                },
            )
            .await
            .unwrap();
    }

    async fn put_usage_policy(
        kv: Arc<MockKvStore>,
        pubsub: Arc<RecordingPubSub>,
        ns: &str,
        name: &str,
        limit: resources_proto::UsageLimit,
    ) {
        let store = crate::control::resources::ResourceStore::new(kv, pubsub);
        store
            .upsert(
                ns,
                resources_proto::Resource {
                    api_version: "talon.impalasys.com/v1".to_string(),
                    kind: "UsagePolicy".to_string(),
                    metadata: Some(resources_proto::ResourceMeta {
                        name: name.to_string(),
                        namespace: ns.to_string(),
                        ..Default::default()
                    }),
                    spec: Some(resources_proto::ResourceSpec {
                        kind: Some(resources_proto::resource_spec::Kind::UsagePolicy(
                            resources_proto::UsagePolicySpec {
                                namespace_scope: "self".to_string(),
                                hard: vec![limit],
                            },
                        )),
                    }),
                    status: None,
                },
            )
            .await
            .unwrap();
    }

    async fn usage_policy_status(
        kv: Arc<MockKvStore>,
        pubsub: Arc<RecordingPubSub>,
        ns: &str,
        name: &str,
    ) -> resources_proto::UsagePolicyStatus {
        let store = crate::control::resources::ResourceStore::new(kv, pubsub);
        let resource = store
            .get(ns, "UsagePolicy", name)
            .await
            .unwrap()
            .expect("UsagePolicy should exist");
        match resource.status.unwrap().kind.unwrap() {
            resources_proto::resource_status::Kind::UsagePolicy(status) => status,
            _ => panic!("expected UsagePolicy status"),
        }
    }

    async fn seed_session(kv: &MockKvStore, ns: &str, agent: &str, session_id: &str) {
        kv.set_msg(
            &keys::session(ns, agent, session_id),
            &data_proto::Session {
                id: session_id.to_string(),
                agent: agent.to_string(),
                ns: ns.to_string(),
                status: "READY".to_string(),
                created_at: 1,
                last_active: 1,
                metadata: Default::default(),
                labels: Default::default(),
            },
        )
        .await
        .unwrap();
    }

    fn tool_result_metadata(
        ns: &str,
        agent: &str,
        session_id: &str,
        message_id: &str,
        tool_call_id: &str,
    ) -> ObjectMetadata {
        ObjectMetadata {
            media_type: "text/plain; charset=utf-8".to_string(),
            filename: format!("{tool_call_id}.txt"),
            metadata: HashMap::from([
                ("kind".to_string(), "tool_result".to_string()),
                ("namespace".to_string(), ns.to_string()),
                ("agent".to_string(), agent.to_string()),
                ("session_id".to_string(), session_id.to_string()),
                ("message_id".to_string(), message_id.to_string()),
                ("tool_call_id".to_string(), tool_call_id.to_string()),
                ("tool_name".to_string(), "shell".to_string()),
            ]),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn delete_session_removes_tool_result_objects_from_session_messages() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = handler(kv.clone(), pubsub);
        let ns = "conic";
        let agent = "coding";
        let session_id = "session-1";
        let message_id = "message-1";
        let tool_call_id = "tool-1";
        let object_key = "cas/conic/sessions/session-1/messages/message-1/part-1.txt";
        seed_session(kv.as_ref(), ns, agent, session_id).await;
        let object = handler
            .gateway
            .objects
            .put(
                object_key,
                b"large tool output",
                tool_result_metadata(ns, agent, session_id, message_id, tool_call_id),
            )
            .await
            .unwrap();
        kv.set_msg(
            &keys::session_message(ns, agent, session_id, message_id),
            &data_proto::SessionMessage {
                id: message_id.to_string(),
                role: data_proto::MessageRole::RoleAssistant as i32,
                created_at: 1,
                labels: Default::default(),
                parts: vec![data_proto::SessionMessagePart {
                    id: "part-1".to_string(),
                    part_type: data_proto::SessionMessagePartType::ToolResult as i32,
                    content: "large tool".to_string(),
                    name: "shell".to_string(),
                    payload_json: json!({
                        "tool_call_id": tool_call_id,
                        "output_preview": "large tool",
                        "output_object_key": object.key,
                    })
                    .to_string(),
                    created_at: 1,
                    object: Some(object),
                }],
            },
        )
        .await
        .unwrap();

        handler
            .handle_delete_session(tonic::Request::new(proto::DeleteSessionRequest {
                ns: ns.to_string(),
                agent: agent.to_string(),
                session_id: session_id.to_string(),
            }))
            .await
            .unwrap();

        assert!(handler
            .gateway
            .objects
            .get(object_key)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn delete_session_does_not_remove_tool_result_objects_outside_session_prefix() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = handler(kv.clone(), pubsub);
        let ns = "conic";
        let agent = "coding";
        let session_id = "session-1";
        let message_id = "message-1";
        let tool_call_id = "tool-1";
        let foreign_object_key = "cas/conic/sessions/session-2/messages/message-1/part-1.txt";
        seed_session(kv.as_ref(), ns, agent, session_id).await;
        let object = handler
            .gateway
            .objects
            .put(
                foreign_object_key,
                b"large tool output",
                tool_result_metadata(ns, agent, "session-2", message_id, tool_call_id),
            )
            .await
            .unwrap();
        kv.set_msg(
            &keys::session_message(ns, agent, session_id, message_id),
            &data_proto::SessionMessage {
                id: message_id.to_string(),
                role: data_proto::MessageRole::RoleAssistant as i32,
                created_at: 1,
                labels: Default::default(),
                parts: vec![data_proto::SessionMessagePart {
                    id: "part-1".to_string(),
                    part_type: data_proto::SessionMessagePartType::ToolResult as i32,
                    content: String::new(),
                    name: "shell".to_string(),
                    payload_json: json!({
                        "tool_call_id": tool_call_id,
                        "output_object_key": object.key,
                    })
                    .to_string(),
                    created_at: 1,
                    object: Some(object),
                }],
            },
        )
        .await
        .unwrap();

        handler
            .handle_delete_session(tonic::Request::new(proto::DeleteSessionRequest {
                ns: ns.to_string(),
                agent: agent.to_string(),
                session_id: session_id.to_string(),
            }))
            .await
            .unwrap();

        assert!(handler
            .gateway
            .objects
            .get(foreign_object_key)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn delete_session_removes_tool_result_objects_from_journal_only_entries() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = handler(kv.clone(), pubsub);
        let ns = "conic";
        let agent = "coding";
        let session_id = "session-1";
        let message_id = "message-1";
        let submission_id = "submission-1";
        let tool_call_id = "tool-1";
        let object_key = "cas/conic/sessions/session-1/messages/message-1/part-1.txt";
        seed_session(kv.as_ref(), ns, agent, session_id).await;
        let object = handler
            .gateway
            .objects
            .put(
                object_key,
                b"large journal-only tool output",
                tool_result_metadata(ns, agent, session_id, message_id, tool_call_id),
            )
            .await
            .unwrap();
        kv.set_msg(
            &keys::session_submission(ns, agent, session_id, submission_id),
            &crate::harness::sessions::pending_submission(submission_id, session_id, "user-1", 1),
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::session_journal_entry(ns, agent, session_id, submission_id, "000001"),
            &data_proto::SessionJournalEntry {
                journal_entry_id: "000001".to_string(),
                submission_id: submission_id.to_string(),
                attempt_id: "attempt-1".to_string(),
                phase: data_proto::SessionExecutionPhase::ToolResult as i32,
                created_at: 1,
                updated_at: 1,
                committed_at: None,
                committed_message_id: None,
                payload: Some(data_proto::SessionJournalEntryPayload {
                    payload: Some(
                        data_proto::session_journal_entry_payload::Payload::ToolResult(
                            data_proto::SessionJournalEntryPayloadToolResult {
                                tool_call_id: tool_call_id.to_string(),
                                name: "shell".to_string(),
                                output: "large journal".to_string(),
                                object: Some(object),
                            },
                        ),
                    ),
                }),
            },
        )
        .await
        .unwrap();

        handler
            .handle_delete_session(tonic::Request::new(proto::DeleteSessionRequest {
                ns: ns.to_string(),
                agent: agent.to_string(),
                session_id: session_id.to_string(),
            }))
            .await
            .unwrap();

        assert!(handler
            .gateway
            .objects
            .get(object_key)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn delete_session_warns_but_succeeds_when_tool_result_object_delete_fails() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let objects = Arc::new(FailingDeleteObjectStore {
            inner: InMemoryObjectStore::default(),
        });
        let handler = handler_with_objects(kv.clone(), pubsub, objects.clone());
        let ns = "conic";
        let agent = "coding";
        let session_id = "session-1";
        let message_id = "message-1";
        let tool_call_id = "tool-1";
        let object_key = "cas/conic/sessions/session-1/messages/message-1/part-1.txt";
        seed_session(kv.as_ref(), ns, agent, session_id).await;
        let object = objects
            .put(
                object_key,
                b"large tool output",
                tool_result_metadata(ns, agent, session_id, message_id, tool_call_id),
            )
            .await
            .unwrap();
        kv.set_msg(
            &keys::session_message(ns, agent, session_id, message_id),
            &data_proto::SessionMessage {
                id: message_id.to_string(),
                role: data_proto::MessageRole::RoleAssistant as i32,
                created_at: 1,
                labels: Default::default(),
                parts: vec![data_proto::SessionMessagePart {
                    id: "part-1".to_string(),
                    part_type: data_proto::SessionMessagePartType::ToolResult as i32,
                    content: "large tool".to_string(),
                    name: "shell".to_string(),
                    payload_json: json!({
                        "tool_call_id": tool_call_id,
                        "output_preview": "large tool",
                        "output_object_key": object.key,
                    })
                    .to_string(),
                    created_at: 1,
                    object: Some(object),
                }],
            },
        )
        .await
        .unwrap();

        handler
            .handle_delete_session(tonic::Request::new(proto::DeleteSessionRequest {
                ns: ns.to_string(),
                agent: agent.to_string(),
                session_id: session_id.to_string(),
            }))
            .await
            .unwrap();

        assert!(kv
            .get(&keys::session(ns, agent, session_id))
            .await
            .unwrap()
            .is_none());
        assert!(objects.get(object_key).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn delete_session_continues_when_session_message_is_corrupt() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = handler(kv.clone(), pubsub);
        let ns = "conic";
        let agent = "coding";
        let session_id = "session-1";
        seed_session(kv.as_ref(), ns, agent, session_id).await;
        kv.set(
            &keys::session_message(ns, agent, session_id, "message-1"),
            b"not a protobuf message",
        )
        .await
        .unwrap();

        handler
            .handle_delete_session(tonic::Request::new(proto::DeleteSessionRequest {
                ns: ns.to_string(),
                agent: agent.to_string(),
                session_id: session_id.to_string(),
            }))
            .await
            .unwrap();

        assert!(kv
            .get(&keys::session(ns, agent, session_id))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn delete_session_continues_when_journal_entry_is_corrupt() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = handler(kv.clone(), pubsub);
        let ns = "conic";
        let agent = "coding";
        let session_id = "session-1";
        let submission_id = "submission-1";
        seed_session(kv.as_ref(), ns, agent, session_id).await;
        kv.set_msg(
            &keys::session_submission(ns, agent, session_id, submission_id),
            &crate::harness::sessions::pending_submission(submission_id, session_id, "user-1", 1),
        )
        .await
        .unwrap();
        kv.set(
            &keys::session_journal_entry(ns, agent, session_id, submission_id, "000001"),
            b"not a protobuf message",
        )
        .await
        .unwrap();

        handler
            .handle_delete_session(tonic::Request::new(proto::DeleteSessionRequest {
                ns: ns.to_string(),
                agent: agent.to_string(),
                session_id: session_id.to_string(),
            }))
            .await
            .unwrap();

        assert!(kv
            .get(&keys::session(ns, agent, session_id))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn update_session_message_replaces_parts_and_labels_only_for_editable_roles() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = handler(kv.clone(), pubsub);
        kv.set_msg(
            &keys::session("conic:test", "assistant", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                status: "READY".to_string(),
                created_at: 10,
                last_active: 20,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::session_message("conic:test", "assistant", "session-1", "assistant-1"),
            &data_proto::SessionMessage {
                id: "assistant-1".to_string(),
                role: data_proto::MessageRole::RoleAssistant as i32,
                created_at: 123,
                labels: HashMap::from([
                    ("old".to_string(), "label".to_string()),
                    (
                        "talon.impalasys.com/connector-registration".to_string(),
                        "Namespace/conic/ConnectorClass/slack".to_string(),
                    ),
                    (
                        "talon.impalasys.com/connector-match/teamId".to_string(),
                        "team-1".to_string(),
                    ),
                    (
                        "talon.impalasys.com/external-conversation".to_string(),
                        "thread-1".to_string(),
                    ),
                    (
                        "talon.impalasys.com/connector-delivery-status".to_string(),
                        "pending_review".to_string(),
                    ),
                ]),
                parts: vec![data_proto::SessionMessagePart {
                    id: "old-part".to_string(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: "old text".to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                    created_at: 124,
                    object: None,
                }],
            },
        )
        .await
        .unwrap();

        let response = handler
            .handle_update_session_message(tonic::Request::new(
                proto::UpdateSessionMessageRequest {
                    ns: "conic:test".to_string(),
                    agent: "assistant".to_string(),
                    session_id: "session-1".to_string(),
                    message_id: "assistant-1".to_string(),
                    parts: vec![data_proto::SessionMessagePart {
                        id: String::new(),
                        part_type: data_proto::SessionMessagePartType::Text as i32,
                        content: "edited text".to_string(),
                        name: String::new(),
                        payload_json: String::new(),
                        created_at: 0,
                        object: None,
                    }],
                    labels: HashMap::from([
                        ("new".to_string(), "label".to_string()),
                        (
                            "talon.impalasys.com/connector-registration".to_string(),
                            "Namespace/other/ConnectorClass/evil".to_string(),
                        ),
                        (
                            "talon.impalasys.com/connector-match/teamId".to_string(),
                            "team-2".to_string(),
                        ),
                        (
                            "talon.impalasys.com/external-conversation".to_string(),
                            "thread-2".to_string(),
                        ),
                        (
                            "talon.impalasys.com/connector-delivery-status".to_string(),
                            "delivery_requested".to_string(),
                        ),
                    ]),
                },
            ))
            .await
            .unwrap()
            .into_inner();

        let message = response.message.expect("updated message");
        assert_eq!(message.id, "assistant-1");
        assert_eq!(message.role, data_proto::MessageRole::RoleAssistant as i32);
        assert_eq!(message.created_at, 123);
        assert_eq!(message.labels.get("new").map(String::as_str), Some("label"));
        assert!(!message.labels.contains_key("old"));
        assert_eq!(
            message
                .labels
                .get("talon.impalasys.com/connector-registration")
                .map(String::as_str),
            Some("Namespace/conic/ConnectorClass/slack")
        );
        assert_eq!(
            message
                .labels
                .get("talon.impalasys.com/connector-match/teamId")
                .map(String::as_str),
            Some("team-1")
        );
        assert_eq!(
            message
                .labels
                .get("talon.impalasys.com/external-conversation")
                .map(String::as_str),
            Some("thread-1")
        );
        assert_eq!(
            message
                .labels
                .get("talon.impalasys.com/connector-delivery-status")
                .map(String::as_str),
            Some("delivery_requested")
        );
        assert_eq!(message.parts[0].id, "000000");
        assert_eq!(message.parts[0].created_at, 123);
        assert_eq!(message.parts[0].content, "edited text");

        kv.set_msg(
            &keys::session_message("conic:test", "assistant", "session-1", "system-1"),
            &data_proto::SessionMessage {
                id: "system-1".to_string(),
                role: data_proto::MessageRole::RoleSystem as i32,
                created_at: 200,
                labels: HashMap::new(),
                parts: vec![data_proto::SessionMessagePart {
                    id: "000000".to_string(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: "system".to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                    created_at: 200,
                    object: None,
                }],
            },
        )
        .await
        .unwrap();

        let error = handler
            .handle_update_session_message(tonic::Request::new(
                proto::UpdateSessionMessageRequest {
                    ns: "conic:test".to_string(),
                    agent: "assistant".to_string(),
                    session_id: "session-1".to_string(),
                    message_id: "system-1".to_string(),
                    parts: vec![data_proto::SessionMessagePart {
                        id: "000000".to_string(),
                        part_type: data_proto::SessionMessagePartType::Text as i32,
                        content: "blocked".to_string(),
                        name: String::new(),
                        payload_json: String::new(),
                        created_at: 200,
                        object: None,
                    }],
                    labels: HashMap::new(),
                },
            ))
            .await
            .expect_err("system messages cannot be updated");
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn answer_session_permission_records_decision_without_publishing_result() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = handler(kv.clone(), pubsub.clone());
        let ns = "conic";
        let agent = "coding";
        let session_id = "session-1";
        let request_id = "request-1";
        kv.set_msg(
            &keys::session(ns, agent, session_id),
            &data_proto::Session {
                id: session_id.to_string(),
                agent: agent.to_string(),
                ns: ns.to_string(),
                status: "PROCESSING".to_string(),
                created_at: 1,
                last_active: 2,
                metadata: std::collections::HashMap::new(),
                labels: std::collections::HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::session_message(ns, agent, session_id, "message-1"),
            &data_proto::SessionMessage {
                id: "message-1".to_string(),
                role: data_proto::MessageRole::RoleAssistant as i32,
                created_at: 2,
                labels: std::collections::HashMap::new(),
                parts: vec![data_proto::SessionMessagePart {
                    id: "part-1".to_string(),
                    part_type: data_proto::SessionMessagePartType::RequestPermission as i32,
                    content: "Permission requested".to_string(),
                    name: "terminal".to_string(),
                    payload_json: json!({
                        "requestId": request_id,
                        "action": "terminal",
                    })
                    .to_string(),
                    created_at: 2,
                    object: None,
                }],
            },
        )
        .await
        .unwrap();

        let response = handler
            .handle_answer_session_permission(tonic::Request::new(
                proto::AnswerSessionPermissionRequest {
                    session_id: session_id.to_string(),
                    agent: agent.to_string(),
                    ns: ns.to_string(),
                    request_id: request_id.to_string(),
                    outcome: "selected".to_string(),
                    option_id: "approved".to_string(),
                    decided_by: "operator".to_string(),
                },
            ))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(response.request_id, request_id);
        assert_eq!(response.outcome, "selected");

        let decision_bytes = kv
            .get(&keys::session_permission_decision(
                ns, agent, session_id, request_id,
            ))
            .await
            .unwrap()
            .expect("decision should be stored");
        let decision: Value = serde_json::from_slice(&decision_bytes).unwrap();
        assert_eq!(decision["requestId"], request_id);
        assert_eq!(decision["outcome"]["optionId"], "approved");

        let published = pubsub.published.lock().await;
        assert!(
            published.is_empty(),
            "permission answers are gateway-to-worker decisions; the worker emits stream events"
        );
        drop(published);

        let duplicate = handler
            .handle_answer_session_permission(tonic::Request::new(
                proto::AnswerSessionPermissionRequest {
                    session_id: session_id.to_string(),
                    agent: agent.to_string(),
                    ns: ns.to_string(),
                    request_id: request_id.to_string(),
                    outcome: "selected".to_string(),
                    option_id: "approved".to_string(),
                    decided_by: "operator".to_string(),
                },
            ))
            .await
            .unwrap_err();
        assert_eq!(duplicate.code(), tonic::Code::AlreadyExists);
    }

    #[tokio::test]
    async fn create_session_enforces_agent_scoped_usage_policy() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = handler(kv.clone(), pubsub.clone());
        let ns = "conic:test";
        let oidc_request = |agent: &str, subject: &str| {
            let mut request = tonic::Request::new(proto::CreateSessionRequest {
                ns: ns.to_string(),
                agent: agent.to_string(),
                labels: Default::default(),
            });
            request
                .extensions_mut()
                .insert(crate::gateway::auth::Claims {
                    iss: None,
                    sub: format!("oidc:{subject}"),
                    aud: "talon".to_string(),
                    iat: None,
                    exp: 10_000_000_000,
                    ns: None,
                    agent: None,
                    session: None,
                    channel: None,
                    origins: Vec::new(),
                    grants: Vec::new(),
                });
            request
        };

        put_agent(kv.clone(), pubsub.clone(), ns, "assistant").await;
        put_agent(kv.clone(), pubsub.clone(), ns, "other").await;
        put_usage_policy(
            kv.clone(),
            pubsub.clone(),
            ns,
            "assistant-session-limit",
            resources_proto::UsageLimit {
                selector: Some(resources_proto::UsageSelector {
                    agent: "assistant".to_string(),
                    provider: String::new(),
                    model: String::new(),
                }),
                metric: crate::control::usage::METRIC_AGENT_SESSIONS.to_string(),
                max: 1,
                window: "1h".to_string(),
                subject_scope: "all".to_string(),
            },
        )
        .await;

        handler
            .handle_create_session(oidc_request("assistant", "first-subject"))
            .await
            .unwrap();

        let status =
            usage_policy_status(kv.clone(), pubsub.clone(), ns, "assistant-session-limit").await;
        assert_eq!(status.hard.len(), 1);
        assert_eq!(
            status.hard[0].metric,
            crate::control::usage::METRIC_AGENT_SESSIONS
        );
        assert_eq!(status.hard[0].used, 1);
        assert_eq!(status.hard[0].remaining, 0);
        assert!(status.hard[0].exceeded);

        let rejected = handler
            .handle_create_session(oidc_request("assistant", "second-subject"))
            .await
            .expect_err("second assistant session should be over all quota");
        assert_eq!(rejected.code(), tonic::Code::ResourceExhausted);

        handler
            .handle_create_session(oidc_request("other", "second-subject"))
            .await
            .expect("agent selector should not block another agent");

        let status =
            usage_policy_status(kv.clone(), pubsub.clone(), ns, "assistant-session-limit").await;
        assert_eq!(status.hard[0].used, 1);
    }

    #[tokio::test]
    async fn create_session_can_scope_session_quota_by_oidc_subject() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = handler(kv.clone(), pubsub.clone());
        let ns = "conic:identity";
        let oidc_request = |subject: &str| {
            let mut request = tonic::Request::new(proto::CreateSessionRequest {
                ns: ns.to_string(),
                agent: "assistant".to_string(),
                labels: Default::default(),
            });
            request
                .extensions_mut()
                .insert(crate::gateway::auth::Claims {
                    iss: None,
                    sub: format!("oidc:{subject}"),
                    aud: "talon".to_string(),
                    iat: None,
                    exp: 10_000_000_000,
                    ns: None,
                    agent: None,
                    session: None,
                    channel: None,
                    origins: Vec::new(),
                    grants: Vec::new(),
                });
            request
        };

        put_agent(kv.clone(), pubsub.clone(), ns, "assistant").await;
        put_usage_policy(
            kv.clone(),
            pubsub.clone(),
            ns,
            "assistant-session-limit",
            resources_proto::UsageLimit {
                selector: Some(resources_proto::UsageSelector {
                    agent: "assistant".to_string(),
                    provider: String::new(),
                    model: String::new(),
                }),
                metric: crate::control::usage::METRIC_AGENT_SESSIONS.to_string(),
                max: 1,
                window: "1h".to_string(),
                subject_scope: "identity".to_string(),
            },
        )
        .await;

        handler
            .handle_create_session(oidc_request("first-subject"))
            .await
            .expect("first subject should get its own session quota");
        handler
            .handle_create_session(oidc_request("second-subject"))
            .await
            .expect("second subject should have an independent session quota");

        let rejected = handler
            .handle_create_session(oidc_request("first-subject"))
            .await
            .expect_err("same subject should be over its identity-scoped session quota");
        assert_eq!(rejected.code(), tonic::Code::ResourceExhausted);
    }

    #[tokio::test]
    async fn create_session_rolls_back_session_and_quota_when_publish_fails() {
        let kv = Arc::new(MockKvStore::default());
        let setup_pubsub = Arc::new(RecordingPubSub::default());
        let handler = failing_publish_handler(kv.clone());
        let ns = "conic:rollback";
        put_agent(kv.clone(), setup_pubsub.clone(), ns, "assistant").await;
        put_usage_policy(
            kv.clone(),
            setup_pubsub.clone(),
            ns,
            "assistant-session-limit",
            resources_proto::UsageLimit {
                selector: Some(resources_proto::UsageSelector {
                    agent: "assistant".to_string(),
                    provider: String::new(),
                    model: String::new(),
                }),
                metric: crate::control::usage::METRIC_AGENT_SESSIONS.to_string(),
                max: 1,
                window: "1h".to_string(),
                subject_scope: "all".to_string(),
            },
        )
        .await;

        let err = handler
            .handle_create_session(tonic::Request::new(proto::CreateSessionRequest {
                ns: ns.to_string(),
                agent: "assistant".to_string(),
                labels: Default::default(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::Internal);

        let session_keys = kv
            .list_keys(&keys::session_prefix(ns, "assistant"))
            .await
            .unwrap();
        assert!(session_keys.is_empty());

        let status =
            usage_policy_status(kv.clone(), setup_pubsub, ns, "assistant-session-limit").await;
        assert_eq!(status.hard[0].used, 0);
    }
}
