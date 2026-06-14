// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::Bytes,
    extract::{Host, OriginalUri, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::TimeZone;
use prost::Message;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tonic::Code;
use uuid::Uuid;

use crate::control::events::SessionMessagePartEventKind;
use crate::control::{
    events,
    keys::{self, ResourceKey},
    topics, ProtoKeyValueStoreExt,
};
use crate::gateway::auth::{self, AuthConfig, AuthMode};
use crate::gateway::rpc::manifests;
use crate::gateway::rpc::models;
use crate::gateway::server::Gateway;
use crate::scheduling;

const A2A_BLOCKING_TIMEOUT: Duration = Duration::from_secs(60);
const A2A_POLL_INTERVAL: Duration = Duration::from_millis(250);
const A2A_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentCardJson {
    name: String,
    description: String,
    version: String,
    url: String,
    protocol_version: String,
    preferred_transport: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    security_schemes: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    security: Vec<HashMap<String, Vec<String>>>,
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
    agent_card: manifests::AgentCard,
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
    #[serde(default, alias = "content")]
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
    artifacts: Vec<A2aArtifactJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    history: Vec<A2aMessageJson>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct A2aArtifactJson {
    artifact_id: String,
    name: String,
    parts: Vec<A2aPartJson>,
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

pub async fn get_agent_card(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Host(host): Host,
    Path((ns, agent)): Path<(String, String)>,
) -> Response {
    let route = match resolve_agent_card_route(&gateway, &ns, &agent).await {
        Ok(route) => route,
        Err(response) => return response,
    };
    let external_host = external_host_from_headers(&headers, &host);
    let scheme = scheme_from_headers(&headers, &external_host);
    match agent_card_json(
        &route.agent_card,
        scheme,
        &external_host,
        &route.ns,
        &route.agent,
        gateway.auth_config.as_ref(),
    ) {
        Ok(payload) => Json(payload).into_response(),
        Err(response) => response,
    }
}

pub async fn post_message_operation(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Path((ns, agent)): Path<(String, String)>,
    OriginalUri(uri): OriginalUri,
    body: Bytes,
) -> Response {
    match scoped_a2a_operation_path(uri.path(), &ns, &agent) {
        Some(path @ ("/message:send" | "/v1/message:send")) => {
            let body = match serde_json::from_slice::<SendMessageRequestJson>(&body) {
                Ok(body) => body,
                Err(err) => {
                    return a2a_error(
                        StatusCode::BAD_REQUEST,
                        format!("invalid A2A SendMessage request: {err}"),
                    );
                }
            };
            let session_hint = match a2a_session_hint(&body.message) {
                Ok(session_hint) => session_hint,
                Err(response) => return response,
            };
            if let Err(response) =
                ensure_a2a_operation_auth(&gateway, &headers, &ns, &agent, session_hint.as_deref())
            {
                return response;
            }
            let route = match resolve_agent_card_route(&gateway, &ns, &agent).await {
                Ok(route) => route,
                Err(response) => return response,
            };
            let response_encoding = if path.starts_with("/v1/") {
                A2aResponseEncoding::RestV1
            } else {
                A2aResponseEncoding::Legacy
            };
            send_message(gateway, route, body, response_encoding).await
        }
        Some("/message:stream" | "/v1/message:stream") => {
            let body = match serde_json::from_slice::<SendMessageRequestJson>(&body) {
                Ok(body) => body,
                Err(err) => {
                    return a2a_error(
                        StatusCode::BAD_REQUEST,
                        format!("invalid A2A SendMessage request: {err}"),
                    );
                }
            };
            let session_hint = match a2a_session_hint(&body.message) {
                Ok(session_hint) => session_hint,
                Err(response) => return response,
            };
            if let Err(response) =
                ensure_a2a_operation_auth(&gateway, &headers, &ns, &agent, session_hint.as_deref())
            {
                return response;
            }
            let route = match resolve_agent_card_route(&gateway, &ns, &agent).await {
                Ok(route) => route,
                Err(response) => return response,
            };
            stream_message(gateway, route, body).await
        }
        _ => {
            if let Err(response) = ensure_a2a_operation_auth(&gateway, &headers, &ns, &agent, None)
            {
                return response;
            }
            a2a_error(StatusCode::NOT_FOUND, "A2A message operation not found")
        }
    }
}

async fn stream_message(
    gateway: Arc<Gateway>,
    route: AgentCardRoute,
    body: SendMessageRequestJson,
) -> Response {
    if !is_user_role(&body.message.role) {
        return a2a_error(
            StatusCode::BAD_REQUEST,
            "A2A message.role must be ROLE_USER",
        );
    }

    let now = chrono::Utc::now();
    let (context_id, task_id, message) =
        match prepare_a2a_session_message(&body.message, now.timestamp_micros()) {
            Ok(identity) => identity,
            Err(response) => return response,
        };

    if let Err(response) = ensure_a2a_session(&gateway, &route, &context_id, &task_id).await {
        return response;
    }

    let mut receiver = match gateway
        .session_streams
        .subscribe(&route.ns, &route.agent, &context_id)
        .await
    {
        Ok(receiver) => receiver,
        Err(err) => {
            tracing::error!(%err, "Failed to subscribe to A2A stream");
            return a2a_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to subscribe to task stream",
            );
        }
    };

    if let Err(err) = scheduling::send_session_message(
        gateway.kv.as_ref(),
        gateway.pubsub.as_ref(),
        &route.ns,
        &route.agent,
        &context_id,
        message,
        now,
    )
    .await
    {
        return scheduling_error_response(err);
    }

    let stream_gateway = gateway.clone();
    let stream_route = route.clone();
    let stream_task_id = task_id.clone();
    let stream_context_id = context_id.clone();
    let stream = async_stream::stream! {
        let initial_task = match load_a2a_task_for_session(
            &stream_gateway,
            &stream_route,
            &stream_context_id,
            &stream_task_id,
        ).await {
            Ok(task) => rest_v1_task_value(task),
            Err(_) => json!({
                "id": stream_task_id,
                "contextId": stream_context_id,
                "status": { "state": "TASK_STATE_WORKING" }
            }),
        };
        yield Ok::<_, Infallible>(a2a_sse_line(json!({ "task": initial_task })));

        let timeout = tokio::time::sleep(A2A_STREAM_IDLE_TIMEOUT);
        tokio::pin!(timeout);
        let mut stream_artifact_started = false;
        let mut pending_artifact_text = String::new();

        loop {
            tokio::select! {
                _ = &mut timeout => {
                    if !pending_artifact_text.is_empty() {
                        let text = std::mem::take(&mut pending_artifact_text);
                        yield Ok::<_, Infallible>(a2a_sse_line(a2a_stream_artifact_update_value(
                            &stream_task_id,
                            &stream_context_id,
                            &text,
                            stream_artifact_started,
                            true,
                        )));
                    }
                    let final_status = match load_a2a_task_for_session(
                        &stream_gateway,
                        &stream_route,
                        &stream_context_id,
                        &stream_task_id,
                    ).await {
                        Ok(task) => rest_v1_task_status_value(task),
                        Err(_) => json!({ "state": "TASK_STATE_UNKNOWN" }),
                    };
                    yield Ok::<_, Infallible>(a2a_sse_line(json!({
                        "statusUpdate": {
                            "taskId": stream_task_id,
                            "contextId": stream_context_id,
                            "status": final_status,
                            "final": true
                        }
                    })));
                    return;
                }
                event_result = receiver.recv() => {
                    timeout.as_mut().reset(tokio::time::Instant::now() + A2A_STREAM_IDLE_TIMEOUT);
                    let Some(event_result) = event_result else {
                        break;
                    };
                    let event = match event_result {
                        Ok(event) => event,
                        Err(status) => {
                            yield Ok::<_, Infallible>(a2a_sse_line(a2a_stream_status_update_value(
                                &stream_task_id,
                                &stream_context_id,
                                "TASK_STATE_FAILED",
                                Some(status.message()),
                                true,
                            )));
                            return;
                        }
                    };

                    let part = event.part.as_ref();
                    let part_type = part.map(|part| part.part_type).unwrap_or_default();
                    let content = part.map(|part| part.content.as_str()).unwrap_or_default();
                    if event.kind == SessionMessagePartEventKind::Done as i32 {
                        break;
                    } else if event.kind == SessionMessagePartEventKind::Error as i32 {
                        if !pending_artifact_text.is_empty() {
                            let text = std::mem::take(&mut pending_artifact_text);
                            yield Ok::<_, Infallible>(a2a_sse_line(a2a_stream_artifact_update_value(
                                &stream_task_id,
                                &stream_context_id,
                                &text,
                                stream_artifact_started,
                                true,
                            )));
                        }
                        let error_text = if content.is_empty() { "Stream error" } else { content };
                        yield Ok::<_, Infallible>(a2a_sse_line(a2a_stream_status_update_value(
                            &stream_task_id,
                            &stream_context_id,
                            "TASK_STATE_FAILED",
                            Some(error_text),
                            true,
                        )));
                        return;
                    } else if part_type == models::SessionMessagePartType::Text as i32 && !content.is_empty() {
                        pending_artifact_text.push_str(content);
                        for text in a2a_drain_complete_paragraphs(&mut pending_artifact_text) {
                            yield Ok::<_, Infallible>(a2a_sse_line(a2a_stream_artifact_update_value(
                                &stream_task_id,
                                &stream_context_id,
                                &text,
                                stream_artifact_started,
                                false,
                            )));
                            stream_artifact_started = true;
                        }
                    }
                }
            }
        }

        if !pending_artifact_text.is_empty() {
            let text = std::mem::take(&mut pending_artifact_text);
            yield Ok::<_, Infallible>(a2a_sse_line(a2a_stream_artifact_update_value(
                &stream_task_id,
                &stream_context_id,
                &text,
                stream_artifact_started,
                true,
            )));
        }

        let final_status = match load_a2a_task_for_session(
            &stream_gateway,
            &stream_route,
            &stream_context_id,
            &stream_task_id,
        ).await {
            Ok(task) => rest_v1_final_task_status_value(task),
            Err(_) => json!({ "state": "TASK_STATE_COMPLETED" }),
        };
        yield Ok::<_, Infallible>(a2a_sse_line(json!({
            "statusUpdate": {
                "taskId": stream_task_id,
                "contextId": stream_context_id,
                "status": final_status,
                "final": true
            }
        })));
    };

    (
        [(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")],
        axum::body::Body::from_stream(stream),
    )
        .into_response()
}

async fn send_message(
    gateway: Arc<Gateway>,
    route: AgentCardRoute,
    body: SendMessageRequestJson,
    response_encoding: A2aResponseEncoding,
) -> Response {
    if !is_user_role(&body.message.role) {
        return a2a_error(
            StatusCode::BAD_REQUEST,
            "A2A message.role must be ROLE_USER",
        );
    }

    let now = chrono::Utc::now();
    let (context_id, task_id, message) =
        match prepare_a2a_session_message(&body.message, now.timestamp_micros()) {
            Ok(identity) => identity,
            Err(response) => return response,
        };

    if let Err(response) = ensure_a2a_session(&gateway, &route, &context_id, &task_id).await {
        return response;
    }

    if let Err(err) = scheduling::send_session_message(
        gateway.kv.as_ref(),
        gateway.pubsub.as_ref(),
        &route.ns,
        &route.agent,
        &context_id,
        message,
        now,
    )
    .await
    {
        return scheduling_error_response(err);
    }

    let task = if body.configuration.return_immediately {
        match load_a2a_task_for_session(&gateway, &route, &context_id, &task_id).await {
            Ok(task) => task,
            Err(response) => return response,
        }
    } else {
        match wait_for_a2a_task(&gateway, &route, &context_id, &task_id).await {
            Ok(task) => task,
            Err(response) => return response,
        }
    };
    match response_encoding {
        A2aResponseEncoding::Legacy => Json(task).into_response(),
        A2aResponseEncoding::RestV1 => {
            Json(json!({ "task": rest_v1_task_value(task) })).into_response()
        }
    }
}

pub async fn list_tasks(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Path((ns, agent)): Path<(String, String)>,
    OriginalUri(uri): OriginalUri,
) -> Response {
    if let Err(response) = ensure_a2a_operation_auth(&gateway, &headers, &ns, &agent, None) {
        return response;
    }
    let route = match resolve_agent_card_route(&gateway, &ns, &agent).await {
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
            let task_ids =
                match list_a2a_session_task_ids(&gateway, &route, &session_id, &session).await {
                    Ok(task_ids) => task_ids,
                    Err(response) => return response,
                };
            for task_id in task_ids {
                match load_a2a_task_for_session(&gateway, &route, &session_id, &task_id).await {
                    Ok(task) => tasks.push(task),
                    Err(response) => return response,
                }
            }
        }
    }
    if scoped_a2a_operation_path(uri.path(), &ns, &agent)
        .is_some_and(|path| path.starts_with("/v1/"))
    {
        Json(json!({
            "tasks": tasks.into_iter().map(rest_v1_task_value).collect::<Vec<_>>()
        }))
        .into_response()
    } else {
        Json(ListTasksResponseJson { tasks }).into_response()
    }
}

pub async fn get_task(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Path((ns, agent, tail)): Path<(String, String, String)>,
    OriginalUri(uri): OriginalUri,
) -> Response {
    if tail.contains('/') || tail.ends_with(":cancel") || tail.ends_with(":subscribe") {
        if let Err(response) = ensure_a2a_operation_auth(&gateway, &headers, &ns, &agent, None) {
            return response;
        }
        return a2a_error(StatusCode::NOT_FOUND, "A2A task not found");
    }
    let route = match resolve_agent_card_route(&gateway, &ns, &agent).await {
        Ok(route) => route,
        Err(response) => return response,
    };
    let task_ref = match find_a2a_task_session(&gateway, &route, &tail).await {
        Ok(task_ref) => task_ref,
        Err(response) => return response,
    };
    if let Err(response) =
        ensure_a2a_operation_auth(&gateway, &headers, &ns, &agent, Some(&task_ref.session_id))
    {
        return response;
    }
    match load_a2a_task_from_session(&gateway, &route, &task_ref, &tail).await {
        Ok(task)
            if scoped_a2a_operation_path(uri.path(), &ns, &agent)
                .is_some_and(|path| path.starts_with("/v1/")) =>
        {
            Json(rest_v1_task_value(task)).into_response()
        }
        Ok(task) => Json(task).into_response(),
        Err(response) => response,
    }
}

pub async fn post_task_operation(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Path((ns, agent, tail)): Path<(String, String, String)>,
    OriginalUri(uri): OriginalUri,
) -> Response {
    let Some(task_id) = tail.strip_suffix(":cancel") else {
        if let Err(response) = ensure_a2a_operation_auth(&gateway, &headers, &ns, &agent, None) {
            return response;
        }
        if tail.ends_with(":subscribe") {
            return unsupported_operation("Task subscription is not supported by this A2A agent");
        }
        return a2a_error(StatusCode::NOT_FOUND, "A2A task operation not found");
    };
    if task_id.is_empty() || task_id.contains('/') {
        if let Err(response) = ensure_a2a_operation_auth(&gateway, &headers, &ns, &agent, None) {
            return response;
        }
        return a2a_error(StatusCode::NOT_FOUND, "A2A task not found");
    }
    let route = match resolve_agent_card_route(&gateway, &ns, &agent).await {
        Ok(route) => route,
        Err(response) => return response,
    };
    let task_ref = match find_a2a_task_session(&gateway, &route, task_id).await {
        Ok(task_ref) => task_ref,
        Err(response) => return response,
    };
    if let Err(response) =
        ensure_a2a_operation_auth(&gateway, &headers, &ns, &agent, Some(&task_ref.session_id))
    {
        return response;
    }

    if let Err(response) = publish_stop_generation(&gateway, &route, &task_ref.session_id).await {
        return response;
    }
    if let Err(response) = mark_a2a_task_canceled(&gateway, &route, &task_ref.session_id).await {
        return response;
    }
    match load_a2a_task_for_session(&gateway, &route, &task_ref.session_id, task_id).await {
        Ok(task)
            if scoped_a2a_operation_path(uri.path(), &ns, &agent)
                .is_some_and(|path| path.starts_with("/v1/")) =>
        {
            Json(rest_v1_task_value(task)).into_response()
        }
        Ok(task) => Json(task).into_response(),
        Err(response) => response,
    }
}

pub async fn unsupported_a2a_operation(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Path((ns, agent)): Path<(String, String)>,
) -> Response {
    if let Err(response) = ensure_a2a_operation_auth(&gateway, &headers, &ns, &agent, None) {
        return response;
    }
    if let Err(response) = resolve_agent_card_route(&gateway, &ns, &agent).await {
        return response;
    }
    unsupported_operation("A2A operation is not supported by this agent")
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

fn external_host_from_headers(headers: &HeaderMap, host: &str) -> String {
    headers
        .get("x-forwarded-host")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(host)
        .to_string()
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

fn a2a_card_base_url(scheme: &str, host: &str, ns: &str, agent: &str) -> String {
    format!(
        "{}://{}/a2a/{}/{}",
        scheme,
        host.trim(),
        urlencoding::encode(ns),
        urlencoding::encode(agent)
    )
}

fn agent_card_json(
    agent_card: &manifests::AgentCard,
    scheme: &str,
    host: &str,
    ns: &str,
    agent: &str,
    auth_config: Option<&AuthConfig>,
) -> Result<AgentCardJson, Response> {
    let capabilities = agent_card.capabilities.as_ref();
    let (security_schemes, security) = agent_card_security(auth_config);
    Ok(AgentCardJson {
        name: agent_card.name.clone(),
        description: agent_card.description.clone(),
        version: agent_card.version.clone(),
        url: a2a_card_base_url(scheme, host, ns, agent),
        protocol_version: "0.3.0".to_string(),
        preferred_transport: "HTTP+JSON".to_string(),
        security_schemes,
        security,
        capabilities: AgentCardCapabilitiesJson {
            streaming: true,
            push_notifications: capabilities
                .map(|value| value.push_notifications)
                .unwrap_or(false),
            extended_agent_card: capabilities
                .map(|value| value.extended_agent_card)
                .unwrap_or(false),
        },
        default_input_modes: agent_card.default_input_modes.clone(),
        default_output_modes: agent_card.default_output_modes.clone(),
        skills: agent_card
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

fn agent_card_security(
    auth_config: Option<&AuthConfig>,
) -> (
    Option<HashMap<String, Value>>,
    Vec<HashMap<String, Vec<String>>>,
) {
    let Some(auth_config) = auth_config else {
        return (None, Vec::new());
    };
    if auth_config.mode == AuthMode::Open {
        return (None, Vec::new());
    }

    let scheme = match auth_config.mode {
        AuthMode::Open => return (None, Vec::new()),
        AuthMode::Password => json!({
            "type": "http",
            "scheme": "basic"
        }),
        AuthMode::Token => json!({
            "type": "http",
            "scheme": "bearer"
        }),
        AuthMode::Jwt => json!({
            "type": "http",
            "scheme": "bearer",
            "bearerFormat": "JWT"
        }),
    };
    let mut schemes = HashMap::new();
    schemes.insert("talon".to_string(), scheme);
    let mut requirement = HashMap::new();
    requirement.insert("talon".to_string(), Vec::new());
    (Some(schemes), vec![requirement])
}

fn ensure_a2a_operation_auth(
    gateway: &Gateway,
    headers: &HeaderMap,
    ns: &str,
    agent: &str,
    session: Option<&str>,
) -> Result<(), Response> {
    let auth_config = gateway.auth_config.clone().unwrap_or_else(AuthConfig::open);
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    auth::check_auth_header(auth_header, &auth_config, ns, Some(agent), session)
        .map_err(a2a_auth_error)
}

fn a2a_auth_error(status: tonic::Status) -> Response {
    let http_status = match status.code() {
        Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        Code::PermissionDenied => StatusCode::FORBIDDEN,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    a2a_error(http_status, status.message())
}

#[derive(Clone, Copy)]
enum A2aResponseEncoding {
    Legacy,
    RestV1,
}

fn rest_v1_task_value(task: A2aTaskJson) -> Value {
    let mut value = serde_json::to_value(task).unwrap_or_else(|_| json!({}));
    rename_message_parts_for_rest_v1(&mut value);
    value
}

fn rest_v1_task_status_value(task: A2aTaskJson) -> Value {
    let mut value = serde_json::to_value(task.status).unwrap_or_else(|_| json!({}));
    rename_message_parts_for_rest_v1(&mut value);
    value
}

fn rest_v1_final_task_status_value(task: A2aTaskJson) -> Value {
    let mut value = rest_v1_task_status_value(task);
    let state = value
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if matches!(
        state.as_str(),
        "" | "TASK_STATE_SUBMITTED" | "TASK_STATE_WORKING"
    ) {
        value["state"] = Value::String("TASK_STATE_COMPLETED".to_string());
    }
    value
}

fn a2a_sse_line(value: Value) -> String {
    format!(
        "data: {}\n\n",
        serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
    )
}

fn a2a_stream_status_update_value(
    task_id: &str,
    context_id: &str,
    state: &str,
    text: Option<&str>,
    final_event: bool,
) -> Value {
    if let Some(text) = text {
        json!({
            "statusUpdate": {
                "taskId": task_id,
                "contextId": context_id,
                "status": {
                    "state": state,
                    "message": {
                        "messageId": Uuid::now_v7().to_string(),
                        "contextId": context_id,
                        "taskId": task_id,
                        "role": "ROLE_AGENT",
                        "content": [{ "text": text }]
                    }
                },
                "final": final_event
            }
        })
    } else {
        json!({
            "statusUpdate": {
                "taskId": task_id,
                "contextId": context_id,
                "status": { "state": state },
                "final": final_event
            }
        })
    }
}

fn a2a_stream_artifact_update_value(
    task_id: &str,
    context_id: &str,
    text: &str,
    append: bool,
    last_chunk: bool,
) -> Value {
    json!({
        "artifactUpdate": {
            "taskId": task_id,
            "contextId": context_id,
            "artifact": {
                "artifactId": "response",
                "name": "response",
                "parts": [{ "text": text }]
            },
            "append": append,
            "lastChunk": last_chunk
        }
    })
}

fn a2a_drain_complete_paragraphs(buffer: &mut String) -> Vec<String> {
    let mut chunks = Vec::new();
    while let Some(end) = a2a_paragraph_boundary(buffer) {
        let chunk = buffer.drain(..end).collect::<String>();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }
    }
    chunks
}

fn a2a_paragraph_boundary(text: &str) -> Option<usize> {
    let lf = text.find("\n\n").map(|index| index + 2);
    let crlf = text.find("\r\n\r\n").map(|index| index + 4);
    match (lf, crlf) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(index), None) | (None, Some(index)) => Some(index),
        (None, None) => None,
    }
}

fn rename_message_parts_for_rest_v1(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if object.get("role").is_some() && object.get("parts").is_some() {
                let parts = object.remove("parts").unwrap_or(Value::Array(Vec::new()));
                object.insert("content".to_string(), rest_v1_content_value(parts));
            }
            for child in object.values_mut() {
                rename_message_parts_for_rest_v1(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                rename_message_parts_for_rest_v1(child);
            }
        }
        _ => {}
    }
}

fn rest_v1_content_value(parts: Value) -> Value {
    match parts {
        Value::Array(parts) => Value::Array(parts.into_iter().map(rest_v1_part_value).collect()),
        other => other,
    }
}

fn rest_v1_part_value(part: Value) -> Value {
    match part {
        Value::Object(mut object) => {
            if let Some(data) = object.remove("data") {
                object.insert("data".to_string(), json!({ "data": data }));
            }
            Value::Object(object)
        }
        other => other,
    }
}

async fn resolve_agent_card_route(
    gateway: &Arc<Gateway>,
    ns: &str,
    agent_name: &str,
) -> Result<AgentCardRoute, Response> {
    let agent = gateway
        .kv
        .get_msg::<models::Agent>(&keys::agent(ns, agent_name))
        .await
        .map_err(|err| {
            tracing::error!(%err, "Failed to fetch A2A agent");
            a2a_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load A2A agent",
            )
        })?
        .ok_or_else(|| a2a_error(StatusCode::NOT_FOUND, "agent not found"))?;
    let agent_card = agent
        .effective_spec
        .as_ref()
        .and_then(|spec| spec.a2a.as_ref())
        .and_then(|a2a| a2a.agent_card.as_ref())
        .ok_or_else(|| {
            a2a_error(
                StatusCode::NOT_FOUND,
                "agent is not published for external A2A",
            )
        })?
        .clone();
    if agent_card.name.trim().is_empty() {
        return Err(a2a_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "A2A agentCard is missing name",
        ));
    }
    if let Some(capabilities) = agent_card.capabilities.as_ref() {
        if capabilities.push_notifications || capabilities.extended_agent_card {
            return Err(a2a_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "A2A agentCard contains unsupported capabilities",
            ));
        }
    }
    Ok(AgentCardRoute {
        ns: agent.ns,
        agent: agent.name,
        agent_card,
    })
}

fn scoped_a2a_operation_path<'a>(path: &'a str, ns: &str, agent: &str) -> Option<&'a str> {
    let prefix = format!(
        "/a2a/{}/{}",
        urlencoding::encode(ns),
        urlencoding::encode(agent)
    );
    path.strip_prefix(&prefix).filter(|suffix| {
        suffix.starts_with("/message:")
            || suffix.starts_with("/v1/message:")
            || suffix.starts_with("/tasks")
            || suffix.starts_with("/v1/tasks")
            || *suffix == "/extendedAgentCard"
    })
}

fn a2a_context_id(message: &A2aMessageJson) -> String {
    message
        .context_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| Uuid::now_v7().to_string())
}

fn a2a_session_hint(message: &A2aMessageJson) -> Result<Option<String>, Response> {
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

fn prepare_a2a_session_message(
    message: &A2aMessageJson,
    timestamp: i64,
) -> Result<(String, String, crate::gateway::rpc::models::SessionMessage), Response> {
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

async fn ensure_a2a_session(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    context_id: &str,
    task_id: &str,
) -> Result<(), Response> {
    let session_key = keys::session(&route.ns, &route.agent, context_id);
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
    labels.insert("a2a.task_id".to_string(), task_id.to_string());
    labels.insert("a2a.agent".to_string(), route.agent.clone());
    let session = crate::gateway::rpc::models::Session {
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

struct A2aTaskSession {
    session_id: String,
    session: crate::gateway::rpc::models::Session,
}

async fn list_a2a_session_task_ids(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    session_id: &str,
    session: &crate::gateway::rpc::models::Session,
) -> Result<Vec<String>, Response> {
    let mut message_keys = gateway
        .kv
        .list_keys(&keys::session_message_prefix(
            &route.ns,
            &route.agent,
            session_id,
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

    let mut task_ids = Vec::new();
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

async fn find_a2a_task_session(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    task_id: &str,
) -> Result<A2aTaskSession, Response> {
    if let Ok(decoded) = decode_a2a_task_id(task_id) {
        let session_key = keys::session(&route.ns, &route.agent, &decoded.session_id);
        let Some(session) = gateway
            .kv
            .get_msg::<crate::gateway::rpc::models::Session>(&session_key)
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
            .get_msg::<crate::gateway::rpc::models::SessionMessage>(&message_key)
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
    let session_keys = gateway.kv.list_keys(&prefix).await.map_err(|err| {
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
            .get_msg::<crate::gateway::rpc::models::Session>(&key)
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
        .list_keys(&keys::session_message_prefix(
            &route.ns,
            &route.agent,
            session_id,
        ))
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

async fn load_a2a_task_for_session(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    session_id: &str,
    task_id: &str,
) -> Result<A2aTaskJson, Response> {
    let session_key = keys::session(&route.ns, &route.agent, session_id);
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

async fn load_a2a_task_from_session(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    task_ref: &A2aTaskSession,
    task_id: &str,
) -> Result<A2aTaskJson, Response> {
    let session = &task_ref.session;
    let mut message_keys = gateway
        .kv
        .list_keys(&keys::session_message_prefix(
            &route.ns,
            &route.agent,
            &task_ref.session_id,
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
            && message.role == crate::gateway::rpc::models::MessageRole::RoleUser as i32
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
            part.part_type == crate::gateway::rpc::models::SessionMessagePartType::Error as i32
        });
        let a2a_message = session_message_to_a2a_message(&message, task_id, &session);
        if message.role == crate::gateway::rpc::models::MessageRole::RoleAssistant as i32 {
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

async fn wait_for_a2a_task(
    gateway: &Arc<Gateway>,
    route: &AgentCardRoute,
    session_id: &str,
    task_id: &str,
) -> Result<A2aTaskJson, Response> {
    let deadline = Instant::now() + A2A_BLOCKING_TIMEOUT;
    loop {
        let task = load_a2a_task_for_session(gateway, route, session_id, task_id).await?;
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
    let mut labels = HashMap::new();
    if !message.message_id.trim().is_empty() {
        labels.insert("a2a.message_id".to_string(), message.message_id.clone());
    }

    Ok(crate::gateway::rpc::models::SessionMessage {
        id: Uuid::now_v7().to_string(),
        role: crate::gateway::rpc::models::MessageRole::RoleUser as i32,
        created_at: timestamp,
        labels,
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

fn session_context_id(session: &crate::gateway::rpc::models::Session) -> String {
    session
        .labels
        .get("a2a.context_id")
        .cloned()
        .unwrap_or_else(|| session.id.clone())
}

fn session_message_to_a2a_artifact(
    message: &crate::gateway::rpc::models::SessionMessage,
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
    part: &crate::gateway::rpc::models::SessionMessagePart,
) -> Option<A2aPartJson> {
    if part.part_type == crate::gateway::rpc::models::SessionMessagePartType::Usage as i32 {
        return None;
    }
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
