// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Host, OriginalUri, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures::StreamExt;
use serde_json::{json, Value};
use tonic::Code;

use crate::control::scheduling;
use crate::control::{
    events::SessionMessagePartEventKind,
    keys::{self},
    ProtoKeyValueStoreExt,
};
use crate::gateway::auth::{self, AuthConfig};
use crate::gateway::rpc::data_proto;
use crate::gateway::rpc::sessions::watcher::SessionStreamTarget;
use crate::gateway::server::Gateway;

use super::card::{
    agent_card_json, external_host_from_headers, resolve_agent_card_route, scheme_from_headers,
    AgentCardRoute,
};
use super::tasks::{
    a2a_session_hint, ensure_a2a_session, find_a2a_task_session, list_a2a_session_task_ids,
    load_a2a_task_for_session, load_a2a_task_from_session, mark_a2a_task_canceled,
    prepare_a2a_session_message, publish_stop_generation, wait_for_a2a_task,
};
use super::types::{
    A2aResponseEncoding, A2aTaskJson, ListTasksResponseJson, SendMessageRequestJson,
};
use super::{a2a_error, A2A_STREAM_IDLE_TIMEOUT};

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
    let scheme = scheme_from_headers(&headers);
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

    let mut receiver = crate::gateway::rpc::sessions::watcher::session_parts_event_stream(
        vec![SessionStreamTarget::new(
            route.ns.clone(),
            route.agent.clone(),
            context_id.clone(),
        )],
        gateway.kv.clone(),
        gateway.pubsub.clone(),
        gateway.worker_connections.clone(),
    );
    let (event_sender, mut event_receiver) = tokio::sync::mpsc::channel(32);
    let watcher_cancel = tokio_util::sync::CancellationToken::new();
    let watcher_cancel_task = watcher_cancel.clone();
    let _watcher_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = watcher_cancel_task.cancelled() => break,
                event = receiver.next() => {
                    let Some(event) = event else {
                        break;
                    };
                    if event_sender.send(event).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

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
        watcher_cancel.cancel();
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
                    watcher_cancel.cancel();
                    return;
                }
                event_result = event_receiver.recv() => {
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
                            watcher_cancel.cancel();
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
                        watcher_cancel.cancel();
                        return;
                    } else if part_type == data_proto::SessionMessagePartType::Text as i32 && !content.is_empty() {
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
        watcher_cancel.cancel();
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
            .get_msg::<crate::gateway::rpc::data_proto::Session>(&key)
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

fn ensure_a2a_operation_auth(
    gateway: &Gateway,
    headers: &HeaderMap,
    ns: &str,
    agent: &str,
    session: Option<&str>,
) -> Result<(), Response> {
    let auth_config = gateway
        .auth_config
        .clone()
        .unwrap_or_else(AuthConfig::jwt_platform);
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    auth::check_auth_header_for_operation_with_origin(
        auth_header,
        origin,
        &auth_config,
        auth::AuthzOperation::ReadWrite,
        ns,
        Some(agent),
        session,
    )
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
                        "messageId": crate::control::uuid::session_message_id(),
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
                object.insert("data".to_string(), data);
            }
            Value::Object(object)
        }
        other => other,
    }
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
