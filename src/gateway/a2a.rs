// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::Bytes,
    extract::{Host, OriginalUri, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::TimeZone;
use prost::Message;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::control::{
    events,
    keys::{self, ResourceKey},
    topics, ProtoKeyValueStoreExt,
};
use crate::gateway::rpc::{manifests, GrpcGatewayHandler};
use crate::gateway::server::Gateway;
use crate::scheduling;

const A2A_BLOCKING_TIMEOUT: Duration = Duration::from_secs(60);
const A2A_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentCardJson {
    name: String,
    description: String,
    version: String,
    url: String,
    capabilities: AgentCardCapabilitiesJson,
    default_input_modes: Vec<String>,
    default_output_modes: Vec<String>,
    skills: Vec<AgentCardSkillJson>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentCardCapabilitiesJson {
    streaming: bool,
    push_notifications: bool,
    extended_agent_card: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentCardSkillJson {
    id: String,
    name: String,
    description: String,
    tags: Vec<String>,
    examples: Vec<String>,
    input_modes: Vec<String>,
    output_modes: Vec<String>,
}

#[derive(Clone)]
struct AgentCardRoute {
    ns: String,
    agent: String,
    card_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendMessageRequestJson {
    message: A2aMessageJson,
    #[serde(default)]
    configuration: SendMessageConfigurationJson,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendMessageConfigurationJson {
    #[serde(default)]
    return_immediately: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct A2aMessageJson {
    #[serde(default)]
    message_id: String,
    role: String,
    #[serde(default)]
    parts: Vec<A2aPartJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context_id: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct A2aPartJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file: Option<Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct A2aTaskJson {
    id: String,
    context_id: String,
    status: A2aTaskStatusJson,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    history: Vec<A2aMessageJson>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct A2aTaskStatusJson {
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<A2aMessageJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ListTasksResponseJson {
    tasks: Vec<A2aTaskJson>,
}

pub async fn get_well_known_agent_card(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Host(host): Host,
) -> Response {
    let handler = GrpcGatewayHandler { gateway };
    match handler.find_agent_card_by_hostname(&host).await {
        Ok(Some(card)) => match agent_card_json(&card, scheme_from_headers(&headers, &host), &host)
        {
            Ok(payload) => Json(payload).into_response(),
            Err(response) => response,
        },
        Ok(None) => (StatusCode::NOT_FOUND, "AgentCard not found for host").into_response(),
        Err(status) if status.code() == tonic::Code::InvalidArgument => {
            (StatusCode::BAD_REQUEST, status.message().to_string()).into_response()
        }
        Err(status) => {
            tracing::error!(%status, "Failed to find AgentCard by hostname");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load AgentCard",
            )
                .into_response()
        }
    }
}

pub async fn post_message_operation(
    State(gateway): State<Arc<Gateway>>,
    Host(host): Host,
    OriginalUri(uri): OriginalUri,
    body: Bytes,
) -> Response {
    match uri.path() {
        "/message:send" => {
            let body = match serde_json::from_slice::<SendMessageRequestJson>(&body) {
                Ok(body) => body,
                Err(err) => {
                    return a2a_error(
                        StatusCode::BAD_REQUEST,
                        format!("invalid A2A SendMessage request: {err}"),
                    );
                }
            };
            send_message(gateway, host, body).await
        }
        "/message:stream" => {
            if let Err(response) = resolve_agent_card_route(&gateway, &host).await {
                return response;
            }
            unsupported_operation("Message streaming is not supported by this AgentCard")
        }
        _ => a2a_error(StatusCode::NOT_FOUND, "A2A message operation not found"),
    }
}

async fn send_message(
    gateway: Arc<Gateway>,
    host: String,
    body: SendMessageRequestJson,
) -> Response {
    let route = match resolve_agent_card_route(&gateway, &host).await {
        Ok(route) => route,
        Err(response) => return response,
    };
    if !is_user_role(&body.message.role) {
        return a2a_error(
            StatusCode::BAD_REQUEST,
            "A2A message.role must be ROLE_USER",
        );
    }

    let task_id = body
        .message
        .task_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    let context_id = body
        .message
        .context_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| task_id.clone());

    if let Err(response) = ensure_a2a_session(&gateway, &route, &task_id, &context_id).await {
        return response;
    }

    let now = chrono::Utc::now();
    let message = match a2a_message_to_session_message(&body.message, now.timestamp_micros()) {
        Ok(message) => message,
        Err(response) => return response,
    };
    if let Err(err) = scheduling::send_session_message(
        gateway.kv.as_ref(),
        gateway.pubsub.as_ref(),
        &route.ns,
        &route.agent,
        &task_id,
        message,
        now,
    )
    .await
    {
        return scheduling_error_response(err);
    }

    let task = if body.configuration.return_immediately {
        match load_a2a_task(&gateway, &route, &task_id).await {
            Ok(task) => task,
            Err(response) => return response,
        }
    } else {
        match wait_for_a2a_task(&gateway, &route, &task_id).await {
            Ok(task) => task,
            Err(response) => return response,
        }
    };
    Json(task).into_response()
}

pub async fn list_tasks(State(gateway): State<Arc<Gateway>>, Host(host): Host) -> Response {
    let route = match resolve_agent_card_route(&gateway, &host).await {
        Ok(route) => route,
        Err(response) => return response,
    };
    let prefix = keys::session_prefix(&route.ns, &route.agent);
    let session_keys = match gateway.kv.list_keys(&prefix).await {
        Ok(keys) => keys,
        Err(err) => {
            tracing::error!(%err, "Failed to list A2A sessions");
            return a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list tasks");
        }
    };

    let mut tasks = Vec::new();
    for key in session_keys {
        let Some(session_id) = keys::direct_child_name(&prefix, &key) else {
            continue;
        };
        let Ok(Some(session)) = gateway
            .kv
            .get_msg::<crate::gateway::rpc::models::Session>(&key)
            .await
        else {
            continue;
        };
        if session
            .labels
            .get("a2a.task")
            .is_some_and(|value| value == "true")
        {
            match load_a2a_task(&gateway, &route, &session_id).await {
                Ok(task) => tasks.push(task),
                Err(response) => return response,
            }
        }
    }
    Json(ListTasksResponseJson { tasks }).into_response()
}

pub async fn get_task(
    State(gateway): State<Arc<Gateway>>,
    Host(host): Host,
    Path(tail): Path<String>,
) -> Response {
    if tail.contains('/') || tail.ends_with(":cancel") || tail.ends_with(":subscribe") {
        return a2a_error(StatusCode::NOT_FOUND, "A2A task not found");
    }
    let route = match resolve_agent_card_route(&gateway, &host).await {
        Ok(route) => route,
        Err(response) => return response,
    };
    match load_a2a_task(&gateway, &route, &tail).await {
        Ok(task) => Json(task).into_response(),
        Err(response) => response,
    }
}

pub async fn post_task_operation(
    State(gateway): State<Arc<Gateway>>,
    Host(host): Host,
    Path(tail): Path<String>,
) -> Response {
    let route = match resolve_agent_card_route(&gateway, &host).await {
        Ok(route) => route,
        Err(response) => return response,
    };
    let Some(task_id) = tail.strip_suffix(":cancel") else {
        if tail.ends_with(":subscribe") {
            return unsupported_operation("Task subscription is not supported by this AgentCard");
        }
        return a2a_error(StatusCode::NOT_FOUND, "A2A task operation not found");
    };
    if task_id.is_empty() || task_id.contains('/') {
        return a2a_error(StatusCode::NOT_FOUND, "A2A task not found");
    }

    if let Err(response) = publish_stop_generation(&gateway, &route, task_id).await {
        return response;
    }
    if let Err(response) = mark_a2a_task_canceled(&gateway, &route, task_id).await {
        return response;
    }
    match load_a2a_task(&gateway, &route, task_id).await {
        Ok(task) => Json(task).into_response(),
        Err(response) => response,
    }
}

pub async fn unsupported_a2a_operation(
    State(gateway): State<Arc<Gateway>>,
    Host(host): Host,
) -> Response {
    if let Err(response) = resolve_agent_card_route(&gateway, &host).await {
        return response;
    }
    unsupported_operation("A2A operation is not supported by this AgentCard")
}

fn scheme_from_headers(headers: &HeaderMap, host: &str) -> &'static str {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .and_then(|value| {
            if value.eq_ignore_ascii_case("http") {
                Some("http")
            } else if value.eq_ignore_ascii_case("https") {
                Some("https")
            } else {
                None
            }
        })
        .unwrap_or_else(|| if is_local_host(host) { "http" } else { "https" })
}

fn host_without_port(host: &str) -> &str {
    let host = host.trim();
    if let Some(stripped) = host.strip_prefix('[') {
        stripped
            .split_once(']')
            .map(|(inside, _rest)| inside)
            .unwrap_or(host)
    } else {
        host.rsplit_once(':')
            .and_then(|(candidate, port)| {
                (!candidate.contains(':') && port.chars().all(|ch| ch.is_ascii_digit()))
                    .then_some(candidate)
            })
            .unwrap_or(host)
    }
}

fn is_local_host(host: &str) -> bool {
    let hostname = host_without_port(host);
    hostname.eq_ignore_ascii_case("localhost") || hostname == "127.0.0.1" || hostname == "::1"
}

fn request_host_port(host: &str) -> Option<&str> {
    let host = host.trim();
    if let Some(stripped) = host.strip_prefix('[') {
        stripped
            .split_once(']')
            .and_then(|(_inside, rest)| rest.strip_prefix(':'))
            .filter(|port| port.chars().all(|ch| ch.is_ascii_digit()))
    } else {
        host.rsplit_once(':').and_then(|(candidate, port)| {
            (!candidate.contains(':') && port.chars().all(|ch| ch.is_ascii_digit())).then_some(port)
        })
    }
}

fn agent_card_json(
    card: &manifests::AgentCard,
    scheme: &str,
    host: &str,
) -> Result<AgentCardJson, Response> {
    let spec = card.spec.as_ref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "stored AgentCard is missing spec",
        )
            .into_response()
    })?;
    let capabilities = spec.capabilities.as_ref();
    let url = if let Some(port) = request_host_port(host) {
        format!("{}://{}:{}", scheme, spec.hostname, port)
    } else {
        format!("{}://{}", scheme, spec.hostname)
    };
    Ok(AgentCardJson {
        name: spec.name.clone(),
        description: spec.description.clone(),
        version: spec.version.clone(),
        url,
        capabilities: AgentCardCapabilitiesJson {
            streaming: capabilities.map(|value| value.streaming).unwrap_or(false),
            push_notifications: capabilities
                .map(|value| value.push_notifications)
                .unwrap_or(false),
            extended_agent_card: capabilities
                .map(|value| value.extended_agent_card)
                .unwrap_or(false),
        },
        default_input_modes: spec.default_input_modes.clone(),
        default_output_modes: spec.default_output_modes.clone(),
        skills: spec
            .skills
            .iter()
            .map(|skill| AgentCardSkillJson {
                id: skill.id.clone(),
                name: skill.name.clone(),
                description: skill.description.clone(),
                tags: skill.tags.clone(),
                examples: skill.examples.clone(),
                input_modes: skill.input_modes.clone(),
                output_modes: skill.output_modes.clone(),
            })
            .collect(),
    })
}

async fn resolve_agent_card_route(
    gateway: &Arc<Gateway>,
    host: &str,
) -> Result<AgentCardRoute, Response> {
    let handler = GrpcGatewayHandler {
        gateway: gateway.clone(),
    };
    let card = match handler.find_agent_card_by_hostname(host).await {
        Ok(Some(card)) => card,
        Ok(None) => {
            return Err(a2a_error(
                StatusCode::NOT_FOUND,
                "AgentCard not found for host",
            ))
        }
        Err(status) if status.code() == tonic::Code::InvalidArgument => {
            return Err(a2a_error(StatusCode::BAD_REQUEST, status.message()));
        }
        Err(status) => {
            tracing::error!(%status, "Failed to find AgentCard by hostname");
            return Err(a2a_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load AgentCard",
            ));
        }
    };
    let spec = card.spec.as_ref().ok_or_else(|| {
        a2a_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "stored AgentCard is missing spec",
        )
    })?;
    let metadata = card.metadata.as_ref().ok_or_else(|| {
        a2a_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "stored AgentCard is missing metadata",
        )
    })?;

    Ok(AgentCardRoute {
        ns: metadata.namespace.clone(),
        agent: spec.agent_ref.clone(),
        card_name: metadata.name.clone(),
    })
}

async fn ensure_a2a_session(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    task_id: &str,
    context_id: &str,
) -> Result<(), Response> {
    let session_key = keys::session(&route.ns, &route.agent, task_id);
    if let Some(session) = gateway
        .kv
        .get_msg::<crate::gateway::rpc::models::Session>(&session_key)
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
                "task id conflicts with a non-A2A session",
            ));
        }
        return Ok(());
    }

    let agent_key = keys::agent(&route.ns, &route.agent);
    if gateway
        .kv
        .get_msg::<crate::gateway::rpc::models::Agent>(&agent_key)
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
    labels.insert("a2a.agent_card".to_string(), route.card_name.clone());
    let session = crate::gateway::rpc::models::Session {
        id: task_id.to_string(),
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
        name: task_id.to_string(),
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

async fn load_a2a_task(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    task_id: &str,
) -> Result<A2aTaskJson, Response> {
    let session_key = keys::session(&route.ns, &route.agent, task_id);
    let session = gateway
        .kv
        .get_msg::<crate::gateway::rpc::models::Session>(&session_key)
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

    let mut message_keys = gateway
        .kv
        .list_keys(&keys::session_message_prefix(
            &route.ns,
            &route.agent,
            task_id,
        ))
        .await
        .map_err(|err| {
            tracing::error!(%err, "Failed to list A2A task messages");
            a2a_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load task messages",
            )
        })?;
    message_keys.sort();

    let mut messages = Vec::new();
    for key in message_keys {
        let Some(message) = gateway
            .kv
            .get_msg::<crate::gateway::rpc::models::SessionMessage>(&key)
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

    let mut history = Vec::new();
    let mut latest_message = None;
    let mut latest_message_has_error = false;
    for message in messages {
        latest_message_has_error = message.parts.iter().any(|part| {
            part.part_type == crate::gateway::rpc::models::SessionMessagePartType::Error as i32
        });
        let a2a_message = session_message_to_a2a_message(&message, task_id, &session);
        latest_message = Some(a2a_message.clone());
        history.push(a2a_message);
    }

    Ok(A2aTaskJson {
        id: task_id.to_string(),
        context_id: session
            .labels
            .get("a2a.context_id")
            .cloned()
            .unwrap_or_else(|| task_id.to_string()),
        status: A2aTaskStatusJson {
            state: a2a_task_state(&session, latest_message_has_error),
            message: latest_message,
            timestamp: timestamp_rfc3339(session.last_active),
        },
        history,
    })
}

async fn wait_for_a2a_task(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    task_id: &str,
) -> Result<A2aTaskJson, Response> {
    let deadline = Instant::now() + A2A_BLOCKING_TIMEOUT;
    loop {
        let task = load_a2a_task(gateway, route, task_id).await?;
        let terminal = matches!(
            task.status.state,
            "TASK_STATE_COMPLETED"
                | "TASK_STATE_FAILED"
                | "TASK_STATE_CANCELED"
                | "TASK_STATE_REJECTED"
        );
        let has_agent_message = task
            .history
            .iter()
            .any(|message| message.role == "ROLE_AGENT");
        if terminal && has_agent_message {
            return Ok(task);
        }
        if Instant::now() >= deadline {
            return Ok(task);
        }
        tokio::time::sleep(A2A_POLL_INTERVAL).await;
    }
}

async fn publish_stop_generation(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    task_id: &str,
) -> Result<(), Response> {
    let session_key = keys::session(&route.ns, &route.agent, task_id);
    if gateway
        .kv
        .get_msg::<crate::gateway::rpc::models::Session>(&session_key)
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

async fn mark_a2a_task_canceled(
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
    mut update: impl FnMut(&mut crate::gateway::rpc::models::Session),
) -> Result<(), Response> {
    for _ in 0..8 {
        let Some(current) = kv.get(key).await.map_err(|err| {
            tracing::error!(%err, "Failed to fetch session for update");
            a2a_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to update task")
        })?
        else {
            return Err(a2a_error(StatusCode::NOT_FOUND, "task not found"));
        };
        let mut session = crate::gateway::rpc::models::Session::decode(current.as_slice())
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
) -> Result<crate::gateway::rpc::models::SessionMessage, Response> {
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
    Ok(crate::gateway::rpc::models::SessionMessage {
        id: if message.message_id.trim().is_empty() {
            Uuid::now_v7().to_string()
        } else {
            message.message_id.clone()
        },
        role: crate::gateway::rpc::models::MessageRole::RoleUser as i32,
        created_at: timestamp,
        labels: HashMap::new(),
        parts,
    })
}

fn session_part_from_a2a_part(
    index: usize,
    part: &A2aPartJson,
    timestamp: i64,
) -> Option<crate::gateway::rpc::models::SessionMessagePart> {
    if let Some(text) = part.text.as_deref().filter(|text| !text.trim().is_empty()) {
        return Some(session_part(
            index,
            crate::gateway::rpc::models::SessionMessagePartType::Text,
            text.to_string(),
            String::new(),
            String::new(),
            timestamp,
        ));
    }
    if let Some(data) = &part.data {
        return Some(session_part(
            index,
            crate::gateway::rpc::models::SessionMessagePartType::Text,
            data.to_string(),
            "data".to_string(),
            data.to_string(),
            timestamp,
        ));
    }
    if let Some(file) = &part.file {
        return Some(session_part(
            index,
            crate::gateway::rpc::models::SessionMessagePartType::File,
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
    part_type: crate::gateway::rpc::models::SessionMessagePartType,
    content: String,
    name: String,
    payload_json: String,
    timestamp: i64,
) -> crate::gateway::rpc::models::SessionMessagePart {
    crate::gateway::rpc::models::SessionMessagePart {
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
    message: &crate::gateway::rpc::models::SessionMessage,
    task_id: &str,
    session: &crate::gateway::rpc::models::Session,
) -> A2aMessageJson {
    A2aMessageJson {
        message_id: message.id.clone(),
        role: if message.role == crate::gateway::rpc::models::MessageRole::RoleUser as i32 {
            "ROLE_USER".to_string()
        } else {
            "ROLE_AGENT".to_string()
        },
        parts: message
            .parts
            .iter()
            .filter_map(session_part_to_a2a_part)
            .collect(),
        task_id: Some(task_id.to_string()),
        context_id: Some(
            session
                .labels
                .get("a2a.context_id")
                .cloned()
                .unwrap_or_else(|| task_id.to_string()),
        ),
    }
}

fn session_part_to_a2a_part(
    part: &crate::gateway::rpc::models::SessionMessagePart,
) -> Option<A2aPartJson> {
    if part.part_type == crate::gateway::rpc::models::SessionMessagePartType::Text as i32 {
        if part.content.is_empty() {
            None
        } else {
            Some(A2aPartJson {
                text: Some(part.content.clone()),
                data: None,
                file: None,
            })
        }
    } else if part.part_type == crate::gateway::rpc::models::SessionMessagePartType::File as i32 {
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
    session: &crate::gateway::rpc::models::Session,
    latest_message_has_error: bool,
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
    } else if session.status == "PROCESSING" {
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

fn is_user_role(role: &str) -> bool {
    role.eq_ignore_ascii_case("ROLE_USER") || role.eq_ignore_ascii_case("user")
}

fn scheduling_error_response(err: anyhow::Error) -> Response {
    if err
        .downcast_ref::<scheduling::SessionCurrentlyProcessingError>()
        .is_some()
    {
        a2a_error(
            StatusCode::CONFLICT,
            "Session is currently generating a response.",
        )
    } else if err
        .downcast_ref::<scheduling::EmptyMessageError>()
        .is_some()
    {
        a2a_error(StatusCode::BAD_REQUEST, "message content is required")
    } else if err
        .downcast_ref::<scheduling::SessionNotFoundError>()
        .is_some()
    {
        a2a_error(StatusCode::NOT_FOUND, "task not found")
    } else {
        a2a_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to send message: {err}"),
        )
    }
}

fn unsupported_operation(message: impl Into<String>) -> Response {
    a2a_error(StatusCode::NOT_IMPLEMENTED, message)
}

fn a2a_error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "code": status.as_u16(),
                "message": message.into(),
            }
        })),
    )
        .into_response()
}
