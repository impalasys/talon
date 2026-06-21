// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{data_proto, proto, GrpcGatewayHandler};
use crate::control::scheduling;
use crate::control::topics;
use crate::control::ProtoKeyValueStoreExt;
use crate::control::{events, keys, keys::ResourceParent, KeyValueStore};
use crate::gateway::session_streams::SessionStreamTarget;
use prost::Message;
use serde_json::{json, Value};
use std::sync::OnceLock;

mod watcher;

use watcher::session_parts_event_stream;

const LARGE_SESSION_PAYLOAD_WARNING_BYTES: usize = 128 * 1024;
const DEFAULT_SESSION_MESSAGES_PAGE_SIZE: usize = 50;
const MAX_SESSION_MESSAGES_PAGE_SIZE: usize = 200;
const SESSION_MESSAGE_KEY_SCAN_BATCH_SIZE: usize = 512;
const DEFAULT_SESSION_STREAM_BATCH_MAX: usize = 10_000;
const CLEAR_SESSION_CAS_RETRIES: usize = 8;

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
        message.id = uuid::Uuid::now_v7().to_string();
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
            // Cloudflare/D1 can make a just-applied resource visible through
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

        let usage_subject = crate::control::usage::UsageSubject {
            namespace: req.ns.clone(),
            agent: req.agent.clone(),
            provider: String::new(),
            model: String::new(),
        };
        crate::control::usage::check_namespace_usage(
            self.gateway.kv.as_ref(),
            &usage_subject,
            &[crate::control::usage::METRIC_AGENT_SESSIONS],
            chrono::Utc::now().timestamp(),
        )
        .await
        .map_err(|e| tonic::Status::resource_exhausted(e.to_string()))?;

        // Use ULID (UUID v7 gives time-sorted guarantees like ULID)
        let session_id = uuid::Uuid::now_v7().to_string();

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
        crate::control::usage::charge_namespace_usage(
            self.gateway.kv.as_ref(),
            &usage_subject,
            &[crate::control::usage::UsageCharge {
                metric: crate::control::usage::METRIC_AGENT_SESSIONS,
                delta: 1,
            }],
            chrono::Utc::now().timestamp(),
        )
        .await
        .map_err(|e| tonic::Status::internal(format!("Failed to charge session usage: {}", e)))?;

        let event = events::LifecycleEvent {
            resource_type: "Session".to_string(),
            name: session_id.clone(),
            ns: req.ns.clone(),
            action: events::SystemAction::Create as i32,
            timestamp: chrono::Utc::now().timestamp_micros(),
        };
        self.gateway
            .pubsub
            .publish(topics::RESOURCE_LIFECYCLE_TOPIC, &event.encode_to_vec())
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to publish event: {}", e)))?;

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
        crate::require_auth!(self, req, &req.get_ref().ns, &req.get_ref().agent);
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
        let message_key = keys::session_message(&req.ns, &req.agent, &req.session_id, &message.id);
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

        Ok(tonic::Response::new(proto::AppendSessionMessageResponse {
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

        let message_id = find_request_permission_message_id(
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

        let part = data_proto::SessionMessagePart {
            id: String::new(),
            part_type: data_proto::SessionMessagePartType::PermissionResult as i32,
            content: "Permission answered".to_string(),
            name: String::new(),
            payload_json: serde_json::to_string(&decision).unwrap_or_default(),
            created_at: now,
            object: None,
        };
        let event = events::SessionMessagePartEvent {
            session_id: req.session_id.clone(),
            kind: events::SessionMessagePartEventKind::Delta as i32,
            part: Some(part),
            timestamp: now,
            agent: req.agent.clone(),
            ns: req.ns.clone(),
            message_id,
        };
        self.gateway
            .pubsub
            .publish(
                &topics::session_part_topic_for_session(&req.session_id),
                &event.encode_to_vec(),
            )
            .await
            .map_err(|err| {
                tonic::Status::internal(format!("Failed to publish permission decision: {err}"))
            })?;

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
    ) -> std::result::Result<tonic::Response<<GrpcGatewayHandler as proto::gateway_service_server::GatewayService>::StreamSessionPartsStream>, tonic::Status>{
        crate::require_auth!(
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
        let receiver = self
            .gateway
            .session_streams
            .subscribe(&req.ns, &req.agent, &req.session_id)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to subscribe to session stream: {}", e))
            })?;

        let event_stream = session_parts_event_stream(
            receiver,
            targets,
            self.gateway.kv.clone(),
            self.gateway.pubsub.clone(),
        );

        Ok(tonic::Response::new(event_stream))
    }

    pub async fn handle_stream_session_parts_batch(
        &self,
        req: tonic::Request<proto::StreamSessionPartsBatchRequest>,
    ) -> std::result::Result<
        tonic::Response<
            <GrpcGatewayHandler as proto::gateway_service_server::GatewayService>::StreamSessionPartsBatchStream,
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
                crate::gateway::auth::check_auth(
                    req.metadata(),
                    auth_config,
                    &target.ns,
                    Some(&target.agent),
                    Some(&target.session_id),
                )?;
            }
        }

        let receiver = self
            .gateway
            .session_streams
            .subscribe_many(targets.clone())
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to subscribe to session stream: {}", e))
            })?;

        let event_stream = session_parts_event_stream(
            receiver,
            targets,
            self.gateway.kv.clone(),
            self.gateway.pubsub.clone(),
        );

        Ok(tonic::Response::new(event_stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::scheduler::NoopSchedulerBackend;
    use crate::control::ProtoKeyValueStoreExt;
    use crate::gateway::rpc::resources_proto;
    use crate::gateway::Gateway;
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use std::sync::Arc;

    fn handler(kv: Arc<MockKvStore>, pubsub: Arc<RecordingPubSub>) -> GrpcGatewayHandler {
        GrpcGatewayHandler {
            gateway: Arc::new(Gateway::new(
                None,
                kv,
                pubsub,
                Arc::new(NoopSchedulerBackend),
                crate::control::object_store::default_object_store(),
            )),
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

    #[tokio::test]
    async fn answer_session_permission_records_decision_and_publishes_result() {
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
        assert_eq!(published.len(), 1);
        let event = events::SessionMessagePartEvent::decode(published[0].1.as_slice()).unwrap();
        assert_eq!(event.session_id, session_id);
        assert_eq!(event.message_id, "message-1");
        let part = event.part.expect("event should include a part");
        assert_eq!(
            part.part_type,
            data_proto::SessionMessagePartType::PermissionResult as i32
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
            },
        )
        .await;

        handler
            .handle_create_session(tonic::Request::new(proto::CreateSessionRequest {
                ns: ns.to_string(),
                agent: "assistant".to_string(),
                labels: Default::default(),
            }))
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
            .handle_create_session(tonic::Request::new(proto::CreateSessionRequest {
                ns: ns.to_string(),
                agent: "assistant".to_string(),
                labels: Default::default(),
            }))
            .await
            .expect_err("second assistant session should be over quota");
        assert_eq!(rejected.code(), tonic::Code::ResourceExhausted);

        handler
            .handle_create_session(tonic::Request::new(proto::CreateSessionRequest {
                ns: ns.to_string(),
                agent: "other".to_string(),
                labels: Default::default(),
            }))
            .await
            .expect("agent selector should not block another agent");

        let status =
            usage_policy_status(kv.clone(), pubsub.clone(), ns, "assistant-session-limit").await;
        assert_eq!(status.hard[0].used, 1);
    }
}
