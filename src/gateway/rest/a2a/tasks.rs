// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use axum::{http::StatusCode, response::Response};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::TimeZone;
use prost::Message;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::control::{
    events,
    keys::{self, ResourceKey},
    topics, ProtoKeyValueStoreExt,
};
use crate::gateway::server::Gateway;

use super::card::AgentCardRoute;
use super::types::{A2aArtifactJson, A2aMessageJson, A2aPartJson, A2aTaskJson, A2aTaskStatusJson};
use super::{a2a_error, A2A_BLOCKING_TIMEOUT, A2A_POLL_INTERVAL};

const SESSION_UPDATE_RETRIES: usize = 8;

#[derive(Clone, PartialEq, Message)]
struct A2ATaskKey {
    #[prost(oneof = "a2a_task_key::Kind", tags = "1")]
    kind: Option<a2a_task_key::Kind>,
}

mod a2a_task_key {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Kind {
        #[prost(message, tag = "1")]
        SessionMessage(super::A2ASessionMessageTaskKey),
    }
}

#[derive(Clone, PartialEq, Message)]
struct A2ASessionMessageTaskKey {
    #[prost(bytes = "vec", tag = "1")]
    session_id: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    message_id: Vec<u8>,
}

struct DecodedA2ATaskKey {
    session_id: String,
    message_id: String,
}

fn a2a_context_id(message: &A2aMessageJson) -> String {
    message
        .context_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(crate::control::uuid::session_id)
}

pub(super) fn a2a_session_hint(message: &A2aMessageJson) -> Result<Option<String>, Response> {
    if let Some(context_id) = message
        .context_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(Some(context_id.to_string()));
    }
    if let Some(task_id) = message
        .task_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return decode_a2a_task_id(task_id).map(|key| Some(key.session_id));
    }
    Ok(None)
}

pub(super) fn prepare_a2a_session_message(
    message: &A2aMessageJson,
    timestamp: i64,
) -> Result<
    (
        String,
        String,
        crate::gateway::rpc::data_proto::SessionMessage,
    ),
    Response,
> {
    let mut session_message = a2a_message_to_session_message(message, timestamp)?;
    let (context_id, task_id) = if let Some(task_id) = message
        .task_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let decoded = decode_a2a_task_id(task_id)?;
        if let Some(context_id) = message
            .context_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            if context_id != decoded.session_id {
                return Err(a2a_error(
                    StatusCode::BAD_REQUEST,
                    "A2A message.contextId does not match taskId",
                ));
            }
        }
        (decoded.session_id, task_id.to_string())
    } else {
        let context_id = a2a_context_id(message);
        let task_id = encode_a2a_task_id(&context_id, &session_message.id)?;
        (context_id, task_id)
    };

    session_message
        .labels
        .insert("a2a.task_id".to_string(), task_id.clone());
    session_message
        .labels
        .insert("a2a.context_id".to_string(), context_id.clone());
    Ok((context_id, task_id, session_message))
}

fn encode_a2a_task_id(session_id: &str, message_id: &str) -> Result<String, Response> {
    let session_uuid = Uuid::parse_str(session_id).map_err(|_| {
        a2a_error(
            StatusCode::BAD_REQUEST,
            "A2A contextId must be a Talon-generated UUID",
        )
    })?;
    let message_uuid = Uuid::parse_str(message_id).map_err(|_| {
        a2a_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to encode A2A task id",
        )
    })?;
    let key = A2ATaskKey {
        kind: Some(a2a_task_key::Kind::SessionMessage(
            A2ASessionMessageTaskKey {
                session_id: session_uuid.as_bytes().to_vec(),
                message_id: message_uuid.as_bytes().to_vec(),
            },
        )),
    };
    Ok(URL_SAFE_NO_PAD.encode(key.encode_to_vec()))
}

fn decode_a2a_task_id(task_id: &str) -> Result<DecodedA2ATaskKey, Response> {
    let bytes = URL_SAFE_NO_PAD.decode(task_id).map_err(|_| {
        a2a_error(
            StatusCode::BAD_REQUEST,
            "A2A taskId is not a valid Talon A2ATaskKey",
        )
    })?;
    let key = A2ATaskKey::decode(bytes.as_slice()).map_err(|_| {
        a2a_error(
            StatusCode::BAD_REQUEST,
            "A2A taskId is not a valid Talon A2ATaskKey",
        )
    })?;
    let Some(a2a_task_key::Kind::SessionMessage(key)) = key.kind else {
        return Err(a2a_error(
            StatusCode::BAD_REQUEST,
            "A2A taskId uses an unsupported Talon A2ATaskKey variant",
        ));
    };
    if key.session_id.len() != 16 || key.message_id.len() != 16 {
        return Err(a2a_error(
            StatusCode::BAD_REQUEST,
            "A2A taskId contains an invalid Talon A2ATaskKey UUID",
        ));
    }
    let session_uuid = Uuid::from_slice(&key.session_id).map_err(|_| {
        a2a_error(
            StatusCode::BAD_REQUEST,
            "A2A taskId contains an invalid Talon session id",
        )
    })?;
    let message_uuid = Uuid::from_slice(&key.message_id).map_err(|_| {
        a2a_error(
            StatusCode::BAD_REQUEST,
            "A2A taskId contains an invalid Talon session message id",
        )
    })?;
    Ok(DecodedA2ATaskKey {
        session_id: session_uuid.to_string(),
        message_id: message_uuid.to_string(),
    })
}

pub(super) async fn ensure_a2a_session(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    context_id: &str,
    task_id: &str,
) -> Result<(), Response> {
    let session_key = keys::session(&route.ns, &route.agent, context_id);
    if let Some(session) = gateway
        .kv
        .get_msg::<crate::gateway::rpc::data_proto::Session>(&session_key)
        .await
        .map_err(|err| {
            tracing::error!(%err, "Failed to fetch A2A session");
            a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load task")
        })?
    {
        if !session
            .labels
            .get("a2a.task")
            .is_some_and(|value| value == "true")
        {
            return Err(a2a_error(
                StatusCode::CONFLICT,
                "context id conflicts with a non-A2A session",
            ));
        }
        update_session(&gateway.kv, &session_key, |session| {
            session
                .labels
                .insert("a2a.task_id".to_string(), task_id.to_string());
        })
        .await?;
        return Ok(());
    }

    let store =
        crate::control::resources::ResourceStore::new(gateway.kv.clone(), gateway.pubsub.clone());
    if store
        .get_agent(&route.ns, &route.agent)
        .await
        .map_err(|err| {
            tracing::error!(%err, "Failed to verify A2A target agent");
            a2a_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to verify target agent",
            )
        })?
        .is_none()
    {
        return Err(a2a_error(StatusCode::NOT_FOUND, "target agent not found"));
    }

    let now = chrono::Utc::now().timestamp_micros();
    let mut labels = HashMap::new();
    labels.insert("a2a.task".to_string(), "true".to_string());
    labels.insert("a2a.context_id".to_string(), context_id.to_string());
    labels.insert("a2a.task_id".to_string(), task_id.to_string());
    labels.insert("a2a.agent".to_string(), route.agent.clone());
    let session = crate::gateway::rpc::data_proto::Session {
        id: context_id.to_string(),
        agent: route.agent.clone(),
        ns: route.ns.clone(),
        status: "IDLE".to_string(),
        created_at: now,
        last_active: now,
        metadata: HashMap::new(),
        labels,
    };
    let inserted = gateway
        .kv
        .compare_and_swap(&session_key, None, &session.encode_to_vec())
        .await
        .map_err(|err| {
            tracing::error!(%err, "Failed to create A2A session");
            a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to create task")
        })?;
    if !inserted {
        return Ok(());
    }

    let event = events::LifecycleEvent {
        resource_type: "Session".to_string(),
        name: context_id.to_string(),
        ns: route.ns.clone(),
        action: events::SystemAction::Create as i32,
        timestamp: now,
    };
    gateway
        .pubsub
        .publish(topics::RESOURCE_LIFECYCLE_TOPIC, &event.encode_to_vec())
        .await
        .map_err(|err| {
            tracing::error!(%err, "Failed to publish A2A session lifecycle event");
            a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to create task")
        })?;
    Ok(())
}

pub(super) struct A2aTaskSession {
    pub(super) session_id: String,
    pub(super) session: crate::gateway::rpc::data_proto::Session,
}

pub(super) async fn list_a2a_session_task_ids(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    session_id: &str,
    session: &crate::gateway::rpc::data_proto::Session,
) -> Result<Vec<String>, Response> {
    let message_keys = gateway
        .kv
        .list_keys(
            &keys::session_message_prefix(&route.ns, &route.agent, session_id),
            None,
        )
        .await
        .map_err(|err| {
            tracing::error!(%err, "Failed to list A2A task messages");
            a2a_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load task messages",
            )
        })?;

    let mut task_ids = Vec::new();
    for key in message_keys {
        let Some(message) = gateway
            .kv
            .get_msg::<crate::gateway::rpc::data_proto::SessionMessage>(&key)
            .await
            .map_err(|err| {
                tracing::error!(%err, "Failed to fetch A2A task message");
                a2a_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to load task message",
                )
            })?
        else {
            continue;
        };
        if let Some(task_id) = message.labels.get("a2a.task_id") {
            if task_ids.last() != Some(task_id) {
                task_ids.push(task_id.clone());
            }
        }
    }

    if task_ids.is_empty() {
        if let Some(task_id) = session.labels.get("a2a.task_id") {
            task_ids.push(task_id.clone());
        }
    }

    Ok(task_ids)
}

pub(super) async fn find_a2a_task_session(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    task_id: &str,
) -> Result<A2aTaskSession, Response> {
    if let Ok(decoded) = decode_a2a_task_id(task_id) {
        let session_key = keys::session(&route.ns, &route.agent, &decoded.session_id);
        let Some(session) = gateway
            .kv
            .get_msg::<crate::gateway::rpc::data_proto::Session>(&session_key)
            .await
            .map_err(|err| {
                tracing::error!(%err, "Failed to fetch A2A task session");
                a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load task")
            })?
        else {
            return Err(a2a_error(StatusCode::NOT_FOUND, "task not found"));
        };
        if !session
            .labels
            .get("a2a.task")
            .is_some_and(|value| value == "true")
        {
            return Err(a2a_error(StatusCode::NOT_FOUND, "task not found"));
        }
        let message_key = keys::session_message(
            &route.ns,
            &route.agent,
            &decoded.session_id,
            &decoded.message_id,
        );
        let has_anchor = gateway
            .kv
            .get_msg::<crate::gateway::rpc::data_proto::SessionMessage>(&message_key)
            .await
            .map_err(|err| {
                tracing::error!(%err, "Failed to fetch A2A task anchor message");
                a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load task")
            })?
            .is_some_and(|message| {
                message
                    .labels
                    .get("a2a.task_id")
                    .is_some_and(|value| value == task_id)
            });
        if !has_anchor {
            return Err(a2a_error(StatusCode::NOT_FOUND, "task not found"));
        }
        return Ok(A2aTaskSession {
            session_id: decoded.session_id,
            session,
        });
    }

    let prefix = keys::session_prefix(&route.ns, &route.agent);
    let session_keys = gateway.kv.list_keys(&prefix, None).await.map_err(|err| {
        tracing::error!(%err, "Failed to list A2A sessions");
        a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load task")
    })?;

    let mut fallback = None;
    for key in session_keys {
        let Some(session_id) = keys::direct_child_name(&prefix, &key) else {
            continue;
        };
        let Some(session) = gateway
            .kv
            .get_msg::<crate::gateway::rpc::data_proto::Session>(&key)
            .await
            .map_err(|err| {
                tracing::error!(%err, "Failed to fetch A2A session");
                a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load task")
            })?
        else {
            continue;
        };
        if !session
            .labels
            .get("a2a.task")
            .is_some_and(|value| value == "true")
        {
            continue;
        }
        if session
            .labels
            .get("a2a.task_id")
            .is_some_and(|value| value == task_id)
            || session_id == task_id
        {
            return Ok(A2aTaskSession {
                session_id,
                session,
            });
        }
        if fallback.is_none()
            && session_contains_a2a_task_message(gateway, route, &session_id, task_id).await?
        {
            fallback = Some(A2aTaskSession {
                session_id,
                session,
            });
        }
    }

    fallback.ok_or_else(|| a2a_error(StatusCode::NOT_FOUND, "task not found"))
}

async fn session_contains_a2a_task_message(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    session_id: &str,
    task_id: &str,
) -> Result<bool, Response> {
    let message_keys = gateway
        .kv
        .list_keys(
            &keys::session_message_prefix(&route.ns, &route.agent, session_id),
            None,
        )
        .await
        .map_err(|err| {
            tracing::error!(%err, "Failed to list A2A task messages");
            a2a_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load task messages",
            )
        })?;
    for key in message_keys {
        let Some(message) = gateway
            .kv
            .get_msg::<crate::gateway::rpc::data_proto::SessionMessage>(&key)
            .await
            .map_err(|err| {
                tracing::error!(%err, "Failed to fetch A2A task message");
                a2a_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to load task message",
                )
            })?
        else {
            continue;
        };
        if message
            .labels
            .get("a2a.task_id")
            .is_some_and(|value| value == task_id)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) async fn load_a2a_task_for_session(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    session_id: &str,
    task_id: &str,
) -> Result<A2aTaskJson, Response> {
    let session_key = keys::session(&route.ns, &route.agent, session_id);
    let session = gateway
        .kv
        .get_msg::<crate::gateway::rpc::data_proto::Session>(&session_key)
        .await
        .map_err(|err| {
            tracing::error!(%err, "Failed to fetch A2A task");
            a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load task")
        })?
        .ok_or_else(|| a2a_error(StatusCode::NOT_FOUND, "task not found"))?;
    if !session
        .labels
        .get("a2a.task")
        .is_some_and(|value| value == "true")
    {
        return Err(a2a_error(StatusCode::NOT_FOUND, "task not found"));
    }
    load_a2a_task_from_session(
        gateway,
        route,
        &A2aTaskSession {
            session_id: session_id.to_string(),
            session,
        },
        task_id,
    )
    .await
}

pub(super) async fn load_a2a_task_from_session(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    task_ref: &A2aTaskSession,
    task_id: &str,
) -> Result<A2aTaskJson, Response> {
    let session = &task_ref.session;
    let message_keys = gateway
        .kv
        .list_keys(
            &keys::session_message_prefix(&route.ns, &route.agent, &task_ref.session_id),
            None,
        )
        .await
        .map_err(|err| {
            tracing::error!(%err, "Failed to list A2A task messages");
            a2a_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load task messages",
            )
        })?;

    let mut messages = Vec::new();
    for key in message_keys {
        let Some(message) = gateway
            .kv
            .get_msg::<crate::gateway::rpc::data_proto::SessionMessage>(&key)
            .await
            .map_err(|err| {
                tracing::error!(%err, "Failed to fetch A2A task message");
                a2a_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to load task message",
                )
            })?
        else {
            continue;
        };
        messages.push(message);
    }
    messages.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    let has_task_anchor = messages.iter().any(|message| {
        message
            .labels
            .get("a2a.task_id")
            .is_some_and(|value| value == task_id)
    });
    let mut include_task_messages = !has_task_anchor;
    let mut history = Vec::new();
    let mut artifacts = Vec::new();
    let mut latest_message_has_error = false;
    let mut has_agent_response = false;
    for message in messages {
        if has_task_anchor
            && message.role == crate::gateway::rpc::data_proto::MessageRole::RoleUser as i32
        {
            include_task_messages = message
                .labels
                .get("a2a.task_id")
                .is_some_and(|value| value == task_id);
        }
        if !include_task_messages {
            continue;
        }
        latest_message_has_error = message.parts.iter().any(|part| {
            part.part_type == crate::gateway::rpc::data_proto::SessionMessagePartType::Error as i32
        });
        let a2a_message = session_message_to_a2a_message(&message, task_id, &session);
        if message.role == crate::gateway::rpc::data_proto::MessageRole::RoleAssistant as i32 {
            has_agent_response = true;
            if let Some(artifact) = session_message_to_a2a_artifact(&message) {
                artifacts.push(artifact);
            }
        }
        history.push(a2a_message);
    }

    Ok(A2aTaskJson {
        id: task_id.to_string(),
        context_id: session_context_id(session),
        status: A2aTaskStatusJson {
            state: a2a_task_state(&session, latest_message_has_error, has_agent_response),
            message: None,
            timestamp: timestamp_rfc3339(session.last_active),
        },
        artifacts,
        history,
    })
}

pub(super) async fn wait_for_a2a_task(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    session_id: &str,
    task_id: &str,
) -> Result<A2aTaskJson, Response> {
    let deadline = Instant::now() + A2A_BLOCKING_TIMEOUT;
    loop {
        let task = load_a2a_task_for_session(gateway, route, session_id, task_id).await?;
        let state = task.status.state;
        let terminal = matches!(
            state,
            "TASK_STATE_COMPLETED"
                | "TASK_STATE_FAILED"
                | "TASK_STATE_CANCELED"
                | "TASK_STATE_REJECTED"
        );
        let has_agent_message = task
            .history
            .iter()
            .any(|message| message.role == "ROLE_AGENT");
        if terminal && (state != "TASK_STATE_COMPLETED" || has_agent_message) {
            return Ok(task);
        }
        if Instant::now() >= deadline {
            return Ok(task);
        }
        tokio::time::sleep(A2A_POLL_INTERVAL).await;
    }
}

pub(super) async fn publish_stop_generation(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    task_id: &str,
) -> Result<(), Response> {
    let session_key = keys::session(&route.ns, &route.agent, task_id);
    if gateway
        .kv
        .get_msg::<crate::gateway::rpc::data_proto::Session>(&session_key)
        .await
        .map_err(|err| {
            tracing::error!(%err, "Failed to fetch A2A task before cancel");
            a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load task")
        })?
        .is_none()
    {
        return Err(a2a_error(StatusCode::NOT_FOUND, "task not found"));
    }
    let event = events::SessionControlEvent {
        session_id: task_id.to_string(),
        agent: route.agent.clone(),
        ns: route.ns.clone(),
        action: "stop_generation".to_string(),
        timestamp: chrono::Utc::now().timestamp_micros(),
    };
    gateway
        .pubsub
        .publish(topics::SESSION_CONTROL_TOPIC, &event.encode_to_vec())
        .await
        .map_err(|err| {
            tracing::error!(%err, "Failed to publish A2A cancel event");
            a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to cancel task")
        })?;
    Ok(())
}

pub(super) async fn mark_a2a_task_canceled(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    task_id: &str,
) -> Result<(), Response> {
    let key = keys::session(&route.ns, &route.agent, task_id);
    update_session(&gateway.kv, &key, |session| {
        session.status = "CANCELED".to_string();
        session.last_active = chrono::Utc::now().timestamp_micros();
        session
            .labels
            .insert("a2a.state".to_string(), "TASK_STATE_CANCELED".to_string());
    })
    .await
}

async fn update_session(
    kv: &Arc<dyn crate::control::KeyValueStore + Send + Sync>,
    key: &ResourceKey,
    mut update: impl FnMut(&mut crate::gateway::rpc::data_proto::Session),
) -> Result<(), Response> {
    for _ in 0..SESSION_UPDATE_RETRIES {
        let Some(current) = kv.get(key).await.map_err(|err| {
            tracing::error!(%err, "Failed to fetch session for update");
            a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to update task")
        })?
        else {
            return Err(a2a_error(StatusCode::NOT_FOUND, "task not found"));
        };
        let mut session = crate::gateway::rpc::data_proto::Session::decode(current.as_slice())
            .map_err(|err| {
                tracing::error!(%err, "Failed to decode session for update");
                a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to update task")
            })?;
        if !session
            .labels
            .get("a2a.task")
            .is_some_and(|value| value == "true")
        {
            return Err(a2a_error(StatusCode::NOT_FOUND, "task not found"));
        }
        update(&mut session);
        let updated = session.encode_to_vec();
        if kv
            .compare_and_swap(key, Some(current.as_slice()), &updated)
            .await
            .map_err(|err| {
                tracing::error!(%err, "Failed to update A2A session");
                a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to update task")
            })?
        {
            return Ok(());
        }
    }
    Err(a2a_error(
        StatusCode::CONFLICT,
        "task changed while applying operation; retry the request",
    ))
}

fn a2a_message_to_session_message(
    message: &A2aMessageJson,
    timestamp: i64,
) -> Result<crate::gateway::rpc::data_proto::SessionMessage, Response> {
    let parts = message
        .parts
        .iter()
        .enumerate()
        .filter_map(|(index, part)| session_part_from_a2a_part(index, part, timestamp))
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return Err(a2a_error(
            StatusCode::BAD_REQUEST,
            "A2A message.parts must contain at least one non-empty part",
        ));
    }
    let mut labels = HashMap::new();
    if !message.message_id.trim().is_empty() {
        labels.insert("a2a.message_id".to_string(), message.message_id.clone());
    }

    Ok(crate::gateway::rpc::data_proto::SessionMessage {
        id: crate::control::uuid::session_message_id(),
        role: crate::gateway::rpc::data_proto::MessageRole::RoleUser as i32,
        created_at: timestamp,
        labels,
        parts,
    })
}

fn session_part_from_a2a_part(
    index: usize,
    part: &A2aPartJson,
    timestamp: i64,
) -> Option<crate::gateway::rpc::data_proto::SessionMessagePart> {
    if let Some(text) = part.text.as_deref().filter(|text| !text.trim().is_empty()) {
        return Some(session_part(
            index,
            crate::gateway::rpc::data_proto::SessionMessagePartType::Text,
            text.to_string(),
            String::new(),
            String::new(),
            timestamp,
        ));
    }
    if let Some(data) = &part.data {
        return Some(session_part(
            index,
            crate::gateway::rpc::data_proto::SessionMessagePartType::Text,
            data.to_string(),
            "data".to_string(),
            data.to_string(),
            timestamp,
        ));
    }
    if let Some(file) = &part.file {
        return Some(session_part(
            index,
            crate::gateway::rpc::data_proto::SessionMessagePartType::File,
            String::new(),
            "file".to_string(),
            file.to_string(),
            timestamp,
        ));
    }
    None
}

fn session_part(
    index: usize,
    part_type: crate::gateway::rpc::data_proto::SessionMessagePartType,
    content: String,
    name: String,
    payload_json: String,
    timestamp: i64,
) -> crate::gateway::rpc::data_proto::SessionMessagePart {
    crate::gateway::rpc::data_proto::SessionMessagePart {
        id: format!("{index:06}"),
        part_type: part_type as i32,
        content,
        name,
        payload_json,
        created_at: timestamp,
        object: None,
    }
}

fn session_message_to_a2a_message(
    message: &crate::gateway::rpc::data_proto::SessionMessage,
    task_id: &str,
    session: &crate::gateway::rpc::data_proto::Session,
) -> A2aMessageJson {
    A2aMessageJson {
        message_id: message.id.clone(),
        role: if message.role == crate::gateway::rpc::data_proto::MessageRole::RoleUser as i32 {
            "ROLE_USER".to_string()
        } else {
            "ROLE_AGENT".to_string()
        },
        parts: message
            .parts
            .iter()
            .filter_map(session_part_to_a2a_part)
            .collect(),
        task_id: Some(
            message
                .labels
                .get("a2a.task_id")
                .cloned()
                .unwrap_or_else(|| task_id.to_string()),
        ),
        context_id: Some(
            message
                .labels
                .get("a2a.context_id")
                .cloned()
                .or_else(|| session.labels.get("a2a.context_id").cloned())
                .unwrap_or_else(|| session.id.clone()),
        ),
    }
}

fn session_context_id(session: &crate::gateway::rpc::data_proto::Session) -> String {
    session
        .labels
        .get("a2a.context_id")
        .cloned()
        .unwrap_or_else(|| session.id.clone())
}

fn session_message_to_a2a_artifact(
    message: &crate::gateway::rpc::data_proto::SessionMessage,
) -> Option<A2aArtifactJson> {
    let parts = message
        .parts
        .iter()
        .filter_map(session_part_to_a2a_part)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(A2aArtifactJson {
            artifact_id: "response".to_string(),
            name: "response".to_string(),
            parts,
        })
    }
}

fn session_part_to_a2a_part(
    part: &crate::gateway::rpc::data_proto::SessionMessagePart,
) -> Option<A2aPartJson> {
    if part.part_type == crate::gateway::rpc::data_proto::SessionMessagePartType::Usage as i32 {
        return None;
    }
    if part.part_type == crate::gateway::rpc::data_proto::SessionMessagePartType::Text as i32 {
        if part.content.is_empty() {
            None
        } else {
            Some(A2aPartJson {
                text: Some(part.content.clone()),
                data: None,
                file: None,
            })
        }
    } else if part.part_type == crate::gateway::rpc::data_proto::SessionMessagePartType::File as i32
    {
        Some(A2aPartJson {
            text: None,
            data: None,
            file: serde_json::from_str(&part.payload_json)
                .ok()
                .or_else(|| Some(json!({ "name": part.name, "uri": part.content }))),
        })
    } else if !part.payload_json.is_empty() {
        Some(A2aPartJson {
            text: None,
            data: serde_json::from_str(&part.payload_json)
                .ok()
                .or_else(|| Some(Value::String(part.payload_json.clone()))),
            file: None,
        })
    } else if !part.content.is_empty() {
        Some(A2aPartJson {
            text: Some(part.content.clone()),
            data: None,
            file: None,
        })
    } else {
        None
    }
}

fn a2a_task_state(
    session: &crate::gateway::rpc::data_proto::Session,
    latest_message_has_error: bool,
    has_agent_response: bool,
) -> &'static str {
    if session
        .labels
        .get("a2a.state")
        .is_some_and(|value| value == "TASK_STATE_CANCELED")
        || session.status == "CANCELED"
    {
        "TASK_STATE_CANCELED"
    } else if latest_message_has_error {
        "TASK_STATE_FAILED"
    } else if session.status == "PROCESSING" && !has_agent_response {
        "TASK_STATE_WORKING"
    } else {
        "TASK_STATE_COMPLETED"
    }
}

fn timestamp_rfc3339(timestamp_micros: i64) -> Option<String> {
    chrono::Utc
        .timestamp_micros(timestamp_micros)
        .single()
        .map(|value| value.to_rfc3339())
}
