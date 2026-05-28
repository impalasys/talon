// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{models, proto, GrpcGatewayHandler};
use crate::control::topics;
use crate::control::ProtoKeyValueStoreExt;
use crate::control::{events, keys, keys::ResourceParent, KeyValueStore};
use crate::scheduling;
use futures::future::try_join_all;
use prost::Message;

const LARGE_SESSION_PAYLOAD_WARNING_BYTES: usize = 128 * 1024;
const DEFAULT_SESSION_MESSAGES_PAGE_SIZE: usize = 50;
const MAX_SESSION_MESSAGES_PAGE_SIZE: usize = 200;
const SESSION_MESSAGE_KEY_SCAN_BATCH_SIZE: usize = 512;

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

impl GrpcGatewayHandler {
    pub async fn handle_create_session(
        &self,
        req: tonic::Request<proto::CreateSessionRequest>,
    ) -> std::result::Result<tonic::Response<proto::SessionResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns, &req.get_ref().agent);
        let req = req.into_inner();

        // 1. Verify agent exists in namespace
        let agent_db_key = keys::agent(&req.ns, &req.agent);
        let agent_exists = self
            .gateway
            .kv
            .get_msg::<models::Agent>(&agent_db_key)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to verify agent: {}", e)))?;

        if agent_exists.is_none() {
            return Err(tonic::Status::not_found(format!(
                "Agent {} not found in ns {}",
                req.agent, req.ns
            )));
        }

        // Use ULID (UUID v7 gives time-sorted guarantees like ULID)
        let session_id = uuid::Uuid::now_v7().to_string();

        let session = models::Session {
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
            steps: vec![],
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
        let step_limit = requested_limit(req.step_limit);

        let session_db_key = keys::session(&req.ns, &req.agent, &req.session_id);
        let msg_prefix = keys::session_message_prefix(&req.ns, &req.agent, &req.session_id);

        let session = self
            .gateway
            .kv
            .get_msg::<models::Session>(&session_db_key)
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

                        match models::SessionMessage::decode(bytes.as_slice()) {
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

        let mut steps = Vec::new();
        let mut remaining_steps = step_limit.unwrap_or(usize::MAX);
        for message in messages.iter().rev() {
            if remaining_steps == 0 {
                break;
            }
            let step_prefix = keys::session_message_step_prefix(
                &req.ns,
                &req.agent,
                &req.session_id,
                &message.id,
            );
            let mut step_keys = self.gateway.kv.list_keys(&step_prefix).await.map_err(|e| {
                tracing::error!(
                    ns = %req.ns,
                    agent = %req.agent,
                    session_id = %req.session_id,
                    message_id = %message.id,
                    prefix = %step_prefix,
                    error = %e,
                    "failed to list session steps"
                );
                tonic::Status::internal(format!("Failed to list session steps: {}", e))
            })?;
            step_keys.sort();
            tracing::info!(
                ns = %req.ns,
                agent = %req.agent,
                session_id = %req.session_id,
                message_id = %message.id,
                step_key_count = step_keys.len(),
                "loaded session step keys"
            );

            step_keys.reverse();
            for key in step_keys {
                if remaining_steps == 0 {
                    break;
                }
                match self.gateway.kv.get(&key).await {
                    Ok(Some(bytes)) => {
                        let payload_bytes = bytes.len();
                        if payload_bytes > LARGE_SESSION_PAYLOAD_WARNING_BYTES {
                            tracing::warn!(
                                ns = %req.ns,
                                agent = %req.agent,
                                session_id = %req.session_id,
                                message_id = %message.id,
                                key = %key,
                                payload_bytes,
                                "session step payload is unusually large"
                            );
                        }

                        match events::SessionStepEvent::decode(bytes.as_slice()) {
                            Ok(step) => {
                                steps.push(step);
                                remaining_steps = remaining_steps.saturating_sub(1);
                            }
                            Err(e) => {
                                tracing::error!(
                                    ns = %req.ns,
                                    agent = %req.agent,
                                    session_id = %req.session_id,
                                    message_id = %message.id,
                                    key = %key,
                                    payload_bytes,
                                    error = %e,
                                    "failed to decode session step"
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::warn!(
                            ns = %req.ns,
                            agent = %req.agent,
                            session_id = %req.session_id,
                            message_id = %message.id,
                            key = %key,
                            "session step key exists but value is missing"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            ns = %req.ns,
                            agent = %req.agent,
                            session_id = %req.session_id,
                            message_id = %message.id,
                            key = %key,
                            error = %e,
                            "failed to decode session step"
                        );
                    }
                }
            }
        }
        steps.reverse();
        tracing::info!(
            ns = %req.ns,
            agent = %req.agent,
            session_id = %req.session_id,
            step_count = steps.len(),
            "loaded session steps"
        );

        Ok(tonic::Response::new(proto::SessionResponse {
            session_id: session.id,
            agent: session.agent,
            state: session.status,
            messages,
            steps,
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
            .get_msg::<models::Session>(&session_db_key)
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
            // a message key, a step key, or another nested descendant.
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

                let message = match models::SessionMessage::decode(bytes.as_slice()) {
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

            let step_fetches = page_messages.into_iter().map(|message| {
                let kv = self.gateway.kv.clone();
                let ns = req.ns.clone();
                let agent = req.agent.clone();
                let session_id = req.session_id.clone();
                async move {
                    let step_prefix =
                        keys::session_message_step_prefix(&ns, &agent, &session_id, &message.id);
                    let mut step_entries = kv.list_entries(&step_prefix).await.map_err(|e| {
                        tonic::Status::internal(format!("Failed to list session steps: {}", e))
                    })?;
                    step_entries.sort_by(|left, right| left.0.cmp(&right.0));
                    let steps = step_entries
                        .into_iter()
                        .filter_map(|(step_key, step_bytes)| {
                            events::SessionStepEvent::decode(step_bytes.as_slice())
                                .map_err(|e| {
                                    tracing::error!(
                                        ns = %ns,
                                        agent = %agent,
                                        session_id = %session_id,
                                        key = %step_key,
                                        error = %e,
                                        "failed to decode session step"
                                    );
                                })
                                .ok()
                        })
                        .collect();

                    Ok::<_, tonic::Status>(proto::ListSessionMessagesResponseItem {
                        steps,
                        message: Some(message),
                    })
                }
            });
            items.extend(try_join_all(step_fetches).await?);
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
                    .get_msg::<models::Session>(&key)
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
            .get_msg::<models::Session>(&session_db_key)
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

    pub async fn handle_stream_session_steps(
        &self,
        req: tonic::Request<proto::StreamSessionStepsRequest>,
    ) -> std::result::Result<tonic::Response<<GrpcGatewayHandler as proto::gateway_service_server::GatewayService>::StreamSessionStepsStream>, tonic::Status>{
        crate::require_auth!(
            self,
            req,
            &req.get_ref().ns,
            &req.get_ref().agent,
            &req.get_ref().session_id
        );
        let req = req.into_inner();

        let receiver = self
            .gateway
            .session_streams
            .subscribe(&req.session_id)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to subscribe to session stream: {}", e))
            })?;

        let event_stream = async_stream::stream! {
            let mut receiver = receiver;
            while let Some(event) = receiver.recv().await {
                yield event;
            }
        };

        Ok(tonic::Response::new(Box::pin(event_stream)))
    }
}
