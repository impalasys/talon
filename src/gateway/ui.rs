// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::events::SessionMessagePartEventKind;
use crate::control::scheduling;
use crate::gateway::rpc::{data_proto, proto, GrpcGatewayHandler};
use crate::gateway::Gateway;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderName, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tonic::metadata::MetadataValue;
use uuid::Uuid;

const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
pub struct SessionPath {
    ns: String,
    agent: String,
    session_id: String,
}

#[derive(Deserialize)]
pub struct ChatRequestBody {
    messages: Vec<UiMessage>,
}

#[derive(Deserialize)]
pub struct UiMessage {
    #[serde(default)]
    parts: Vec<UiPart>,
}

#[derive(Deserialize)]
pub struct UiPart {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    object: Option<UiObjectRef>,
    #[serde(default, rename = "payloadJson", alias = "payload_json")]
    payload_json: Option<String>,
}

#[derive(Deserialize)]
pub struct UiObjectRef {
    key: String,
    #[serde(default, rename = "mediaType", alias = "media_type")]
    media_type: String,
    #[serde(default, rename = "sizeBytes", alias = "size_bytes")]
    size_bytes: u64,
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    metadata: HashMap<String, String>,
}

#[derive(Clone)]
struct ToolStepPayload {
    tool_call_id: String,
    tool_name: String,
    args: Value,
    result: Value,
}

fn gateway_handler(gateway: &Arc<Gateway>) -> GrpcGatewayHandler {
    GrpcGatewayHandler {
        gateway: gateway.clone(),
    }
}

fn tonic_request<T>(headers: &HeaderMap, inner: T) -> Result<tonic::Request<T>, Response> {
    let mut request = tonic::Request::new(inner);
    if let Some(auth_header) = headers.get(header::AUTHORIZATION) {
        let auth_str = auth_header.to_str().map_err(|_| {
            response_with_status(StatusCode::BAD_REQUEST, "Invalid authorization header")
        })?;
        let value = MetadataValue::try_from(auth_str).map_err(|_| {
            response_with_status(StatusCode::BAD_REQUEST, "Invalid authorization header")
        })?;
        request.metadata_mut().insert("authorization", value);
    }
    Ok(request)
}

fn response_with_status(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

fn map_status(status: tonic::Status) -> Response {
    let code = match status.code() {
        tonic::Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
        tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
        tonic::Code::NotFound => StatusCode::NOT_FOUND,
        tonic::Code::ResourceExhausted => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    response_with_status(code, status.message())
}

fn ui_message_has_dispatchable_part(message: &UiMessage) -> bool {
    message.parts.iter().any(|part| match part.kind.as_deref() {
        Some("text") => part
            .text
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty()),
        Some("image") => part
            .object
            .as_ref()
            .is_some_and(|object| !object.key.trim().is_empty()),
        _ => false,
    })
}

fn last_dispatchable_message(messages: &[UiMessage]) -> Option<&UiMessage> {
    messages
        .iter()
        .rev()
        .find(|message| ui_message_has_dispatchable_part(message))
}

fn ui_message_to_session_message(
    message: &UiMessage,
    now_micros: i64,
) -> data_proto::SessionMessage {
    let mut parts = Vec::new();

    for part in &message.parts {
        match part.kind.as_deref() {
            Some("text") => {
                let text = part.text.as_deref().unwrap_or_default();
                if text.trim().is_empty() {
                    continue;
                }
                parts.push(data_proto::SessionMessagePart {
                    id: String::new(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: text.to_string(),
                    name: String::new(),
                    payload_json: part.payload_json.clone().unwrap_or_default(),
                    created_at: now_micros,
                    object: None,
                });
            }
            Some("image") => {
                let Some(object) = part.object.as_ref() else {
                    continue;
                };
                if object.key.trim().is_empty() {
                    continue;
                }
                parts.push(data_proto::SessionMessagePart {
                    id: String::new(),
                    part_type: data_proto::SessionMessagePartType::Image as i32,
                    content: String::new(),
                    name: String::new(),
                    payload_json: part.payload_json.clone().unwrap_or_default(),
                    created_at: now_micros,
                    object: Some(data_proto::ObjectRef {
                        key: object.key.clone(),
                        media_type: object.media_type.clone(),
                        size_bytes: object.size_bytes,
                        sha256: object.sha256.clone(),
                        filename: object.filename.clone(),
                        metadata: object.metadata.clone(),
                    }),
                });
            }
            _ => {}
        }
    }

    data_proto::SessionMessage {
        id: Uuid::now_v7().to_string(),
        role: data_proto::MessageRole::RoleUser as i32,
        created_at: now_micros,
        labels: HashMap::new(),
        parts,
    }
}

fn extract_tool_part_payload(part: &data_proto::SessionMessagePart) -> Option<ToolStepPayload> {
    let payload: Value = serde_json::from_str(&part.payload_json).ok()?;
    let tool_call_id = payload.get("tool_call_id")?.as_str()?.to_string();
    if tool_call_id.is_empty() {
        return None;
    }

    Some(ToolStepPayload {
        tool_call_id,
        tool_name: if part.name.is_empty() {
            "tool".to_string()
        } else {
            part.name.clone()
        },
        args: payload.get("input").cloned().unwrap_or_else(|| json!({})),
        result: payload
            .get("output")
            .cloned()
            .unwrap_or_else(|| Value::String(part.content.clone())),
    })
}

#[cfg(test)]
fn latest_tool_part_payload<'a, I>(parts: I, part_type: i32) -> Option<ToolStepPayload>
where
    I: IntoIterator<Item = &'a data_proto::SessionMessagePart>,
    I::IntoIter: DoubleEndedIterator,
{
    parts
        .into_iter()
        .rev()
        .find(|part| part.part_type == part_type)
        .and_then(extract_tool_part_payload)
}

#[cfg(test)]
async fn fetch_session_metadata(
    gateway: &Arc<Gateway>,
    headers: &HeaderMap,
    path: &SessionPath,
) -> Result<proto::SessionResponse, Response> {
    // UI route guards only need session metadata here; messages are loaded
    // through the paginated message endpoint.
    fetch_session_with_limits(gateway, headers, path, Some(-1)).await
}

async fn fetch_session_with_limits(
    gateway: &Arc<Gateway>,
    headers: &HeaderMap,
    path: &SessionPath,
    message_limit: Option<i32>,
) -> Result<proto::SessionResponse, Response> {
    let request = tonic_request(
        headers,
        proto::GetSessionRequest {
            ns: path.ns.clone(),
            agent: path.agent.clone(),
            session_id: path.session_id.clone(),
            message_limit: message_limit.unwrap_or_default(),
        },
    )?;
    let response = gateway_handler(gateway)
        .handle_get_session(request)
        .await
        .map_err(map_status)?
        .into_inner();

    Ok(response)
}

fn latest_assistant_message_text(response: &proto::SessionResponse) -> Option<String> {
    response
        .messages
        .iter()
        .rev()
        .filter(|message| message.role == data_proto::MessageRole::RoleAssistant as i32)
        .find_map(|message| {
            let text = message
                .parts
                .iter()
                .filter(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
                .map(|part| part.content.as_str())
                .collect::<String>();
            if !text.trim().is_empty() {
                Some(text)
            } else {
                None
            }
        })
}

fn ndjson_line(value: Value) -> Vec<u8> {
    format!("{}\n", value).into_bytes()
}

fn data_stream_line(code: &str, value: Value) -> Vec<u8> {
    format!("{code}:{}\n", value).into_bytes()
}

#[cfg(test)]
fn stable_payload_hash(payload: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    payload.as_bytes().iter().fold(FNV_OFFSET, |hash, byte| {
        hash.wrapping_mul(FNV_PRIME) ^ u64::from(*byte)
    })
}

#[cfg(test)]
fn part_dedup_key(event: &crate::control::events::SessionMessagePartEvent) -> String {
    let part = event.part.as_ref();
    let payload_json = part
        .map(|part| part.payload_json.as_str())
        .unwrap_or_default();
    let payload_hash = stable_payload_hash(payload_json);
    let part_type = part.map(|part| part.part_type).unwrap_or_default();
    let name = part.map(|part| part.name.as_str()).unwrap_or_default();
    let content = part.map(|part| part.content.as_str()).unwrap_or_default();
    format!(
        "{}:{}:{}:{}:{}:{}",
        event.message_id, event.timestamp, part_type, name, content, payload_hash
    )
}

pub async fn post_chat(
    State(gateway): State<Arc<Gateway>>,
    Path(path): Path<SessionPath>,
    headers: HeaderMap,
    Json(body): Json<ChatRequestBody>,
) -> Response {
    let Some(ui_message) = last_dispatchable_message(&body.messages) else {
        return response_with_status(StatusCode::BAD_REQUEST, "message content is required");
    };
    let now = chrono::Utc::now();
    let session_message = ui_message_to_session_message(ui_message, now.timestamp_micros());

    let stream_request = match tonic_request(
        &headers,
        proto::StreamSessionPartsRequest {
            ns: path.ns.clone(),
            agent: path.agent.clone(),
            session_id: path.session_id.clone(),
        },
    ) {
        Ok(request) => request,
        Err(response) => return response,
    };

    let step_stream = match gateway_handler(&gateway)
        .handle_stream_session_parts(stream_request)
        .await
    {
        Ok(response) => response.into_inner(),
        Err(status) => return map_status(status),
    };

    if let Err(err) = scheduling::send_session_message(
        gateway.kv.as_ref(),
        gateway.pubsub.as_ref(),
        &path.ns,
        &path.agent,
        &path.session_id,
        session_message,
        now,
    )
    .await
    {
        let status = if err
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
            tonic::Status::internal(format!("Failed to send message: {}", err))
        };
        return map_status(status);
    }

    let gateway_for_stream = gateway.clone();
    let headers_for_stream = headers.clone();
    let path_for_stream = path;
    let stream = async_stream::stream! {
        let mut started_part = false;
        let mut started_message_id: Option<String> = None;
        let mut emitted_any_text = false;
        let mut parts = step_stream;
        let timeout = tokio::time::sleep(STREAM_IDLE_TIMEOUT);
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                _ = &mut timeout => {
                    yield Ok::<_, Infallible>(data_stream_line("3", json!("Timed out waiting for assistant response")));
                    return;
                }
                part_result = parts.next() => {
                    timeout.as_mut().reset(tokio::time::Instant::now() + STREAM_IDLE_TIMEOUT);
                    let Some(part_result) = part_result else {
                        break;
                    };
                    let event = match part_result {
                        Ok(event) => event,
                        Err(status) => {
                            yield Ok::<_, Infallible>(data_stream_line("3", json!(status.message())));
                            return;
                        }
                    };

                    let part = event.part.as_ref();
                    let part_type = part.map(|part| part.part_type).unwrap_or_default();
                    let content = part.map(|part| part.content.as_str()).unwrap_or_default();

                    if !started_part && event.kind != SessionMessagePartEventKind::Done as i32 {
                        let message_id = if event.message_id.is_empty() {
                            Uuid::now_v7().to_string()
                        } else {
                            event.message_id.clone()
                        };
                        started_message_id = Some(message_id.clone());
                        started_part = true;
                        yield Ok::<_, Infallible>(data_stream_line("f", json!({ "messageId": message_id })));
                    } else if started_message_id.as_deref() != Some(event.message_id.as_str()) && !event.message_id.is_empty() {
                        started_message_id = Some(event.message_id.clone());
                        yield Ok::<_, Infallible>(data_stream_line("f", json!({ "messageId": event.message_id })));
                    }

                    if event.kind == SessionMessagePartEventKind::Done as i32 {
                        break;
                    } else if event.kind == SessionMessagePartEventKind::Error as i32 {
                        let error_text = if content.is_empty() {
                            "Stream error".to_string()
                        } else {
                            content.to_string()
                        };
                        if !emitted_any_text {
                            yield Ok::<_, Infallible>(data_stream_line("0", json!(error_text)));
                        }
                        yield Ok::<_, Infallible>(data_stream_line("3", json!(error_text)));
                        return;
                    } else if part_type == data_proto::SessionMessagePartType::Text as i32 {
                        if !content.is_empty() {
                            emitted_any_text = true;
                            yield Ok::<_, Infallible>(data_stream_line("0", json!(content)));
                        }
                    } else if part_type == data_proto::SessionMessagePartType::Reasoning as i32 {
                        if !content.is_empty() {
                            yield Ok::<_, Infallible>(data_stream_line("g", json!(content)));
                        }
                    } else if part_type == data_proto::SessionMessagePartType::ToolCall as i32 {
                        let payload = part.and_then(extract_tool_part_payload);
                        let tool_call_id = payload
                            .as_ref()
                            .map(|payload| payload.tool_call_id.clone())
                            .unwrap_or_else(|| format!("tool-{}", Uuid::now_v7()));
                        let tool_name = payload
                            .as_ref()
                            .map(|payload| payload.tool_name.clone())
                            .unwrap_or_else(|| part.map(|part| part.name.clone()).filter(|name| !name.is_empty()).unwrap_or_else(|| "tool".to_string()));
                        let args = payload
                            .as_ref()
                            .map(|payload| payload.args.clone())
                            .unwrap_or_else(|| json!({}));
                        yield Ok::<_, Infallible>(data_stream_line("9", json!({
                            "toolCallId": tool_call_id,
                            "toolName": tool_name,
                            "args": args
                        })));
                    } else if part_type == data_proto::SessionMessagePartType::ToolResult as i32 {
                        let payload = part.and_then(extract_tool_part_payload);
                        if let Some(payload) = payload {
                            yield Ok::<_, Infallible>(data_stream_line("a", json!({
                                "toolCallId": payload.tool_call_id,
                                "result": payload.result
                            })));
                        }
                    } else if part_type
                        == data_proto::SessionMessagePartType::RequestPermission as i32
                    {
                        let payload = part
                            .and_then(|part| serde_json::from_str::<Value>(&part.payload_json).ok())
                            .unwrap_or_else(|| json!({}));
                        let request_id = payload
                            .get("requestId")
                            .and_then(|value| value.as_str())
                            .unwrap_or("request-permission");
                        yield Ok::<_, Infallible>(data_stream_line("9", json!({
                            "toolCallId": request_id,
                            "toolName": "request_permission",
                            "args": payload.get("request").cloned().unwrap_or_else(|| payload.clone())
                        })));
                    } else if part_type
                        == data_proto::SessionMessagePartType::PermissionResult as i32
                    {
                        let payload = part
                            .and_then(|part| serde_json::from_str::<Value>(&part.payload_json).ok())
                            .unwrap_or_else(|| json!({}));
                        let request_id = payload
                            .get("requestId")
                            .and_then(|value| value.as_str())
                            .unwrap_or("request-permission");
                        yield Ok::<_, Infallible>(data_stream_line("a", json!({
                            "toolCallId": request_id,
                            "result": payload.get("outcome").cloned().unwrap_or_else(|| payload.clone())
                        })));
                    } else if part_type == data_proto::SessionMessagePartType::Usage as i32 {
                        let usage = part
                            .and_then(|part| serde_json::from_str::<Value>(&part.payload_json).ok())
                            .unwrap_or_else(|| json!({}));
                        yield Ok::<_, Infallible>(data_stream_line("h", usage));
                    }
                }
            }
        }

        if !emitted_any_text {
            if let Ok(response) = fetch_session_with_limits(
                &gateway_for_stream,
                &headers_for_stream,
                &path_for_stream,
                Some(1),
            )
            .await
            {
                if let Some(text) = latest_assistant_message_text(&response) {
                    if !started_part {
                        let message_id = response
                            .messages
                            .iter()
                            .rev()
                            .find(|message| {
                                message.role == data_proto::MessageRole::RoleAssistant as i32
                            })
                            .map(|message| message.id.clone())
                            .unwrap_or_else(|| Uuid::now_v7().to_string());
                        yield Ok::<_, Infallible>(data_stream_line("f", json!({ "messageId": message_id })));
                    }
                    yield Ok::<_, Infallible>(data_stream_line("0", json!(text)));
                }
            }
        }
        yield Ok::<_, Infallible>(data_stream_line("e", json!({
            "finishReason": "stop",
            "isContinued": false
        })));
        yield Ok::<_, Infallible>(data_stream_line("d", json!({
            "finishReason": "stop"
        })));
    };

    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (HeaderName::from_static("x-vercel-ai-data-stream"), "v1"),
        ],
        axum::body::Body::from_stream(stream),
    )
        .into_response()
}

pub async fn get_chat(
    State(gateway): State<Arc<Gateway>>,
    Path(path): Path<SessionPath>,
    headers: HeaderMap,
) -> Response {
    let request = match tonic_request(
        &headers,
        proto::StreamSessionPartsRequest {
            ns: path.ns.clone(),
            agent: path.agent.clone(),
            session_id: path.session_id.clone(),
        },
    ) {
        Ok(request) => request,
        Err(response) => return response,
    };

    let response = match gateway_handler(&gateway)
        .handle_stream_session_parts(request)
        .await
    {
        Ok(response) => response,
        Err(status) => return map_status(status),
    };

    let stream = async_stream::stream! {
        let mut parts = response.into_inner();
        while let Some(part_result) = parts.next().await {
            let event = match part_result {
                Ok(event) => event,
                Err(status) => {
                    yield Ok::<_, Infallible>(ndjson_line(json!({ "type": "error", "value": status.message() })));
                    break;
                }
            };

            let part = event.part.as_ref();
            let part_type = part.map(|part| part.part_type).unwrap_or_default();
            let content = part.map(|part| part.content.as_str()).unwrap_or_default();

            if event.kind == SessionMessagePartEventKind::Done as i32 {
                break;
            } else if event.kind == SessionMessagePartEventKind::Error as i32 {
                let error_text = if content.is_empty() {
                    "Stream error".to_string()
                } else {
                    content.to_string()
                };
                yield Ok::<_, Infallible>(ndjson_line(json!({ "type": "error", "value": error_text })));
                break;
            } else if part_type == data_proto::SessionMessagePartType::Text as i32 {
                if !content.is_empty() {
                    yield Ok::<_, Infallible>(ndjson_line(json!({ "type": "text", "value": content })));
                }
            } else if part_type == data_proto::SessionMessagePartType::Reasoning as i32 {
                if !content.is_empty() {
                    yield Ok::<_, Infallible>(ndjson_line(json!({ "type": "reasoning", "value": content })));
                }
            } else if part_type == data_proto::SessionMessagePartType::ToolCall as i32 {
                let payload = part.and_then(extract_tool_part_payload);
                let tool_call_id = payload
                    .as_ref()
                    .map(|payload| payload.tool_call_id.clone())
                    .unwrap_or_else(|| format!("tool-{}", Uuid::now_v7()));
                let tool_name = payload
                    .as_ref()
                    .map(|payload| payload.tool_name.clone())
                    .unwrap_or_else(|| part.map(|part| part.name.clone()).filter(|name: &String| !name.is_empty()).unwrap_or_else(|| "tool".to_string()));
                let args = payload
                    .as_ref()
                    .map(|payload| payload.args.clone())
                    .unwrap_or_else(|| json!({}));
                yield Ok::<_, Infallible>(ndjson_line(json!({
                    "type": "tool_call",
                    "value": {
                        "toolCallId": tool_call_id,
                        "toolName": tool_name,
                        "args": args
                    }
                })));
            } else if part_type == data_proto::SessionMessagePartType::ToolResult as i32 {
                let payload = part.and_then(extract_tool_part_payload);
                if let Some(payload) = payload {
                    yield Ok::<_, Infallible>(ndjson_line(json!({
                        "type": "tool_result",
                        "value": {
                            "toolCallId": payload.tool_call_id,
                            "result": payload.result
                        }
                    })));
                }
            } else if part_type == data_proto::SessionMessagePartType::RequestPermission as i32 {
                let payload = part
                    .and_then(|part| serde_json::from_str::<Value>(&part.payload_json).ok())
                    .unwrap_or_else(|| json!({}));
                let request_id = payload
                    .get("requestId")
                    .and_then(|value| value.as_str())
                    .unwrap_or("request-permission");
                yield Ok::<_, Infallible>(ndjson_line(json!({
                    "type": "tool_call",
                    "value": {
                        "toolCallId": request_id,
                        "toolName": "request_permission",
                        "args": payload.get("request").cloned().unwrap_or_else(|| payload.clone())
                    }
                })));
            } else if part_type == data_proto::SessionMessagePartType::PermissionResult as i32 {
                let payload = part
                    .and_then(|part| serde_json::from_str::<Value>(&part.payload_json).ok())
                    .unwrap_or_else(|| json!({}));
                let request_id = payload
                    .get("requestId")
                    .and_then(|value| value.as_str())
                    .unwrap_or("request-permission");
                yield Ok::<_, Infallible>(ndjson_line(json!({
                    "type": "tool_result",
                    "value": {
                        "toolCallId": request_id,
                        "result": payload.get("outcome").cloned().unwrap_or_else(|| payload.clone())
                    }
                })));
            } else if part_type == data_proto::SessionMessagePartType::Usage as i32 {
                let usage = part
                    .and_then(|part| serde_json::from_str::<Value>(&part.payload_json).ok())
                    .unwrap_or_else(|| json!({}));
                yield Ok::<_, Infallible>(ndjson_line(json!({
                    "type": "usage",
                    "value": usage
                })));
            }
        }
    };

    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (HeaderName::from_static("x-vercel-ai-data-stream"), "v1"),
        ],
        axum::body::Body::from_stream(stream),
    )
        .into_response()
}

pub async fn delete_chat(
    State(gateway): State<Arc<Gateway>>,
    Path(path): Path<SessionPath>,
    headers: HeaderMap,
    Json(_body): Json<Value>,
) -> Response {
    let request = match tonic_request(
        &headers,
        proto::StopSessionGenerationRequest {
            ns: path.ns,
            agent: path.agent,
            session_id: path.session_id,
        },
    ) {
        Ok(request) => request,
        Err(response) => return response,
    };

    match gateway_handler(&gateway)
        .handle_stop_session_generation(request)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(status) => map_status(status),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        data_stream_line, delete_chat, extract_tool_part_payload, fetch_session_metadata, get_chat,
        latest_assistant_message_text, latest_tool_part_payload, map_status, ndjson_line,
        part_dedup_key, post_chat, tonic_request, ChatRequestBody, SessionPath, UiMessage,
        UiObjectRef, UiPart,
    };
    use crate::control::events::{
        SessionControlEvent, SessionMessagePartEvent, SessionMessagePartEventKind,
    };
    use crate::control::{
        keys::{self, ResourceKey, ResourceList},
        scheduler::NoopSchedulerBackend,
        topics, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::{data_proto, proto};
    use crate::gateway::{server::Gateway, session_streams::SessionStreamHub};
    use axum::body::to_bytes;
    use axum::extract::{Path, State};
    use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
    use axum::Json;
    use futures::stream;
    use prost::Message;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockKvStore {
        data: Mutex<HashMap<ResourceKey, Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, k: &ResourceKey) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.data.lock().await.get(k).cloned())
        }

        async fn set(&self, k: &ResourceKey, v: &[u8]) -> anyhow::Result<()> {
            self.data.lock().await.insert(k.clone(), v.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            k: &ResourceKey,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let current = data.get(k).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(k.clone(), value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, k: &ResourceKey) -> anyhow::Result<()> {
            self.data.lock().await.remove(k);
            Ok(())
        }

        async fn list_keys(&self, list: &ResourceList) -> anyhow::Result<Vec<ResourceKey>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|key| list.matches(key).then(|| key.clone()))
                .collect::<Vec<_>>();
            keys.sort();
            Ok(keys)
        }

        async fn list_keys_page(
            &self,
            list: &ResourceList,
            before_key: Option<&str>,
            limit: usize,
        ) -> anyhow::Result<Vec<ResourceKey>> {
            Ok(crate::control::page_keys_desc(
                self.list_keys(list).await?,
                before_key,
                limit,
            ))
        }
    }

    struct MockPubSub {
        streams: Arc<Mutex<HashMap<String, Vec<Vec<u8>>>>>,
        published: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    }

    #[async_trait::async_trait]
    impl MessagePublisher for MockPubSub {
        async fn publish(&self, topic: &str, message: &[u8]) -> anyhow::Result<()> {
            self.published
                .lock()
                .await
                .push((topic.to_string(), message.to_vec()));
            Ok(())
        }

        async fn subscribe(
            &self,
            topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            let data = self
                .streams
                .lock()
                .await
                .get(topic)
                .cloned()
                .unwrap_or_default();
            Ok(Box::pin(stream::iter(data)))
        }
    }

    fn setup_gateway(
        kv: Arc<MockKvStore>,
        streams: Arc<Mutex<HashMap<String, Vec<Vec<u8>>>>>,
        published: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    ) -> Arc<Gateway> {
        let pubsub = Arc::new(MockPubSub {
            streams: streams.clone(),
            published: published.clone(),
        });
        Arc::new(Gateway {
            auth_config: None,
            kv,
            pubsub: pubsub.clone(),
            scheduler: Arc::new(NoopSchedulerBackend),
            objects: crate::control::object_store::default_object_store(),
            session_streams: Arc::new(SessionStreamHub::new(pubsub)),
        })
    }

    fn message_part(
        part_type: data_proto::SessionMessagePartType,
        content: impl Into<String>,
        name: impl Into<String>,
        payload_json: impl Into<String>,
    ) -> data_proto::SessionMessagePart {
        data_proto::SessionMessagePart {
            id: String::new(),
            part_type: part_type as i32,
            content: content.into(),
            name: name.into(),
            payload_json: payload_json.into(),
            created_at: 0,
            object: None,
        }
    }

    fn part_event_for(
        session_id: &str,
        ns: &str,
        message_id: &str,
        kind: SessionMessagePartEventKind,
        part: data_proto::SessionMessagePart,
        timestamp: i64,
    ) -> SessionMessagePartEvent {
        SessionMessagePartEvent {
            session_id: session_id.to_string(),
            kind: kind as i32,
            part: Some(part),
            timestamp,
            agent: "agent".to_string(),
            ns: ns.to_string(),
            message_id: message_id.to_string(),
        }
    }

    #[test]
    fn extract_tool_part_payload_parses_tool_metadata() {
        let part = message_part(
            data_proto::SessionMessagePartType::ToolCall,
            "",
            "search",
            r#"{"tool_call_id":"call-123","input":{"q":"rust"},"output":{"ok":true}}"#,
        );

        let payload = extract_tool_part_payload(&part).expect("payload should parse");
        assert_eq!(payload.tool_call_id, "call-123");
        assert_eq!(payload.tool_name, "search");
        assert_eq!(payload.args["q"], "rust");
        assert_eq!(payload.result["ok"], true);
    }

    #[test]
    fn extract_tool_part_payload_defaults_and_rejects_invalid_payloads() {
        let fallback_part = message_part(
            data_proto::SessionMessagePartType::ToolResult,
            "fallback-result",
            "",
            r#"{"tool_call_id":"call-9"}"#,
        );
        let payload = extract_tool_part_payload(&fallback_part).expect("payload should parse");
        assert_eq!(payload.tool_name, "tool");
        assert_eq!(payload.args, json!({}));
        assert_eq!(payload.result, Value::String("fallback-result".to_string()));

        let missing_id = data_proto::SessionMessagePart {
            payload_json: r#"{"input":{"q":"rust"}}"#.to_string(),
            ..fallback_part.clone()
        };
        assert!(extract_tool_part_payload(&missing_id).is_none());

        let empty_id = data_proto::SessionMessagePart {
            payload_json: r#"{"tool_call_id":""}"#.to_string(),
            ..fallback_part.clone()
        };
        assert!(extract_tool_part_payload(&empty_id).is_none());

        let invalid_json = data_proto::SessionMessagePart {
            payload_json: "{not-json}".to_string(),
            ..fallback_part
        };
        assert!(extract_tool_part_payload(&invalid_json).is_none());
    }

    #[test]
    fn latest_tool_part_payload_returns_last_matching_entry() {
        let parts = vec![
            message_part(
                data_proto::SessionMessagePartType::ToolCall,
                "",
                "first",
                r#"{"tool_call_id":"call-1","input":{"q":"first"}}"#,
            ),
            message_part(
                data_proto::SessionMessagePartType::ToolResult,
                "",
                "obs",
                r#"{"tool_call_id":"call-1","output":{"ok":true}}"#,
            ),
            message_part(
                data_proto::SessionMessagePartType::ToolCall,
                "",
                "second",
                r#"{"tool_call_id":"call-2","input":{"q":"second"}}"#,
            ),
        ];

        let payload = latest_tool_part_payload(
            parts.iter(),
            data_proto::SessionMessagePartType::ToolCall as i32,
        )
        .unwrap();
        assert_eq!(payload.tool_call_id, "call-2");
        assert_eq!(payload.tool_name, "second");
        assert_eq!(payload.args["q"], "second");
    }

    #[test]
    fn tonic_request_rejects_non_utf8_authorization_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_bytes(&[0xFF]).expect("header value"),
        );

        let response = tonic_request(&headers, ()).expect_err("invalid auth should fail");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn tonic_request_copies_valid_authorization_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer demo-token"),
        );

        let request = tonic_request(&headers, "payload").expect("valid auth should pass");
        assert_eq!(request.get_ref(), &"payload");
        assert_eq!(
            request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer demo-token")
        );
    }

    #[test]
    fn map_status_translates_common_tonic_codes() {
        assert_eq!(
            map_status(tonic::Status::unauthenticated("no auth")).status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            map_status(tonic::Status::invalid_argument("bad")).status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            map_status(tonic::Status::not_found("gone")).status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            map_status(tonic::Status::permission_denied("nope")).status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            map_status(tonic::Status::resource_exhausted("busy")).status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            map_status(tonic::Status::internal("boom")).status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn latest_assistant_message_text_returns_last_non_empty_assistant_message() {
        let response = proto::SessionResponse {
            session_id: "s".to_string(),
            agent: "agent".to_string(),
            state: "IDLE".to_string(),
            messages: vec![
                data_proto::SessionMessage {
                    id: "m1".to_string(),
                    role: 1,
                    created_at: 1,
                    labels: HashMap::new(),
                    parts: vec![message_part(
                        data_proto::SessionMessagePartType::Text,
                        "user",
                        "",
                        "",
                    )],
                },
                data_proto::SessionMessage {
                    id: "m2".to_string(),
                    role: 2,
                    created_at: 2,
                    labels: HashMap::new(),
                    parts: Vec::new(),
                },
                data_proto::SessionMessage {
                    id: "m3".to_string(),
                    role: 2,
                    created_at: 3,
                    labels: HashMap::new(),
                    parts: vec![data_proto::SessionMessagePart {
                        id: "000000".to_string(),
                        part_type: data_proto::SessionMessagePartType::Text as i32,
                        content: "done".to_string(),
                        name: String::new(),
                        payload_json: String::new(),
                        created_at: 3,
                        object: None,
                    }],
                },
            ],
            labels: HashMap::new(),
        };

        assert_eq!(
            latest_assistant_message_text(&response).as_deref(),
            Some("done")
        );
    }

    #[test]
    fn framing_and_dedup_helpers_return_stable_output() {
        let event = part_event_for(
            "s",
            "ns",
            "msg-1",
            SessionMessagePartEventKind::Delta,
            message_part(
                data_proto::SessionMessagePartType::Text,
                "hello",
                "name",
                "",
            ),
            7,
        );

        assert_eq!(
            String::from_utf8(ndjson_line(json!({"type":"text","value":"hello"}))).unwrap(),
            "{\"type\":\"text\",\"value\":\"hello\"}\n"
        );
        assert_eq!(
            String::from_utf8(data_stream_line("0", json!("hello"))).unwrap(),
            "0:\"hello\"\n"
        );
        let key = part_dedup_key(&event);
        assert!(key.starts_with("msg-1:7:1:name:hello:"));
        assert!(!key.ends_with(':'));
        assert_eq!(key, part_dedup_key(&event));
    }

    #[tokio::test]
    async fn fetch_session_reads_seeded_session_state() {
        let kv = Arc::new(MockKvStore::default());
        kv.set_msg(
            &keys::session("default", "agent", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "agent".to_string(),
                ns: "default".to_string(),
                status: "IDLE".to_string(),
                created_at: 1,
                last_active: 2,
                metadata: HashMap::new(),
                labels: HashMap::from([("env".to_string(), "test".to_string())]),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::session_message("default", "agent", "session-1", "msg-1"),
            &data_proto::SessionMessage {
                id: "msg-1".to_string(),
                role: data_proto::MessageRole::RoleAssistant as i32,
                created_at: 3,
                labels: HashMap::new(),
                parts: vec![message_part(
                    data_proto::SessionMessagePartType::Text,
                    "history should not load",
                    "",
                    "",
                )],
            },
        )
        .await
        .unwrap();
        let gateway = setup_gateway(
            kv,
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(Vec::new())),
        );

        let response = fetch_session_metadata(
            &gateway,
            &HeaderMap::new(),
            &SessionPath {
                ns: "default".to_string(),
                agent: "agent".to_string(),
                session_id: "session-1".to_string(),
            },
        )
        .await
        .expect("session should load");

        assert_eq!(response.session_id, "session-1");
        assert_eq!(response.state, "IDLE");
        assert_eq!(response.labels.get("env").map(String::as_str), Some("test"));
        assert!(response.messages.is_empty());
    }

    #[tokio::test]
    async fn post_chat_rejects_empty_messages_before_backend_dispatch() {
        let gateway = setup_gateway(
            Arc::new(MockKvStore::default()),
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(Vec::new())),
        );

        let response = post_chat(
            State(gateway),
            Path(SessionPath {
                ns: "default".to_string(),
                agent: "agent".to_string(),
                session_id: "session-1".to_string(),
            }),
            HeaderMap::new(),
            Json(ChatRequestBody {
                messages: vec![UiMessage {
                    parts: vec![UiPart {
                        kind: Some("text".to_string()),
                        text: Some("   ".to_string()),
                        object: None,
                        payload_json: None,
                    }],
                }],
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("message content is required"));
    }

    #[tokio::test]
    async fn post_chat_persists_image_object_parts_for_dispatch() {
        let kv = Arc::new(MockKvStore::default());
        kv.set_msg(
            &keys::session("default", "agent", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "agent".to_string(),
                ns: "default".to_string(),
                status: "IDLE".to_string(),
                created_at: 1,
                last_active: 1,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        let published = Arc::new(Mutex::new(Vec::new()));
        let gateway = setup_gateway(
            kv.clone(),
            Arc::new(Mutex::new(HashMap::new())),
            published.clone(),
        );

        let response = post_chat(
            State(gateway),
            Path(SessionPath {
                ns: "default".to_string(),
                agent: "agent".to_string(),
                session_id: "session-1".to_string(),
            }),
            HeaderMap::new(),
            Json(ChatRequestBody {
                messages: vec![UiMessage {
                    parts: vec![
                        UiPart {
                            kind: Some("text".to_string()),
                            text: Some("what is in this?".to_string()),
                            object: None,
                            payload_json: None,
                        },
                        UiPart {
                            kind: Some("image".to_string()),
                            text: None,
                            object: Some(UiObjectRef {
                                key: "sessions/session-1/uploads/photo.png".to_string(),
                                media_type: "image/png".to_string(),
                                size_bytes: 123,
                                sha256: "abc123".to_string(),
                                filename: "photo.png".to_string(),
                                metadata: HashMap::new(),
                            }),
                            payload_json: None,
                        },
                    ],
                }],
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let message_keys = kv
            .list_keys(&keys::session_message_prefix(
                "default",
                "agent",
                "session-1",
            ))
            .await
            .unwrap();
        assert_eq!(message_keys.len(), 1);
        let message = kv
            .get_msg::<data_proto::SessionMessage>(&message_keys[0])
            .await
            .unwrap()
            .unwrap();
        assert_eq!(message.parts.len(), 2);
        assert_eq!(
            message.parts[0].part_type,
            data_proto::SessionMessagePartType::Text as i32
        );
        assert_eq!(message.parts[0].content, "what is in this?");
        assert_eq!(
            message.parts[1].part_type,
            data_proto::SessionMessagePartType::Image as i32
        );
        let object = message.parts[1].object.as_ref().unwrap();
        assert_eq!(object.key, "sessions/session-1/uploads/photo.png");
        assert_eq!(object.media_type, "image/png");
        assert_eq!(object.size_bytes, 123);

        let published = published.lock().await;
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, topics::SESSION_DISPATCH_TOPIC);
    }

    #[tokio::test]
    async fn post_chat_maps_missing_session_to_not_found() {
        let gateway = setup_gateway(
            Arc::new(MockKvStore::default()),
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(Vec::new())),
        );

        let response = post_chat(
            State(gateway),
            Path(SessionPath {
                ns: "default".to_string(),
                agent: "agent".to_string(),
                session_id: "missing-session".to_string(),
            }),
            HeaderMap::new(),
            Json(ChatRequestBody {
                messages: vec![UiMessage {
                    parts: vec![UiPart {
                        kind: Some("text".to_string()),
                        text: Some("hello".to_string()),
                        object: None,
                        payload_json: None,
                    }],
                }],
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("Session not found"));
    }

    #[tokio::test]
    async fn post_chat_streams_error_as_visible_text() {
        let session_id = "session-error";
        let kv = Arc::new(MockKvStore::default());
        kv.set_msg(
            &keys::session("default", "agent", session_id),
            &data_proto::Session {
                id: session_id.to_string(),
                agent: "agent".to_string(),
                ns: "default".to_string(),
                status: "IDLE".to_string(),
                created_at: 1,
                last_active: 2,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();

        let topic_name =
            topics::session_part_topic_for_shard(topics::session_part_shard(session_id));
        let streams = Arc::new(Mutex::new(HashMap::from([(
            topic_name,
            vec![part_event_for(
                session_id,
                "default",
                "assistant-1",
                SessionMessagePartEventKind::Error,
                message_part(
                    data_proto::SessionMessagePartType::Error,
                    "Error: provider overloaded",
                    "",
                    "",
                ),
                1,
            )
            .encode_to_vec()],
        )])));
        let gateway = setup_gateway(kv, streams, Arc::new(Mutex::new(Vec::new())));

        let response = post_chat(
            State(gateway),
            Path(SessionPath {
                ns: "default".to_string(),
                agent: "agent".to_string(),
                session_id: session_id.to_string(),
            }),
            HeaderMap::new(),
            Json(ChatRequestBody {
                messages: vec![UiMessage {
                    parts: vec![UiPart {
                        kind: Some("text".to_string()),
                        text: Some("hello".to_string()),
                        object: None,
                        payload_json: None,
                    }],
                }],
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains(r#"f:{"messageId":"assistant-1"}"#));
        assert!(text.contains(r#"0:"Error: provider overloaded""#));
        assert!(text.contains(r#"3:"Error: provider overloaded""#));
    }

    #[tokio::test]
    async fn get_chat_streams_tool_calls_results_and_text() {
        let session_id = "session-123";
        let topic_name =
            topics::session_part_topic_for_shard(topics::session_part_shard(session_id));
        let streams = Arc::new(Mutex::new(HashMap::from([(
            topic_name,
            vec![
                part_event_for(
                    session_id,
                    "default",
                    "msg-1",
                    SessionMessagePartEventKind::Delta,
                    message_part(
                        data_proto::SessionMessagePartType::ToolCall,
                        "",
                        "search",
                        r#"{"tool_call_id":"call-1","input":{"q":"rust"}}"#,
                    ),
                    1,
                )
                .encode_to_vec(),
                part_event_for(
                    session_id,
                    "default",
                    "msg-1",
                    SessionMessagePartEventKind::Delta,
                    message_part(
                        data_proto::SessionMessagePartType::ToolResult,
                        "fallback",
                        "search",
                        r#"{"tool_call_id":"call-1","output":{"ok":true}}"#,
                    ),
                    2,
                )
                .encode_to_vec(),
                part_event_for(
                    session_id,
                    "default",
                    "msg-1",
                    SessionMessagePartEventKind::Delta,
                    message_part(data_proto::SessionMessagePartType::Text, "Hello", "", ""),
                    3,
                )
                .encode_to_vec(),
                part_event_for(
                    session_id,
                    "default",
                    "msg-1",
                    SessionMessagePartEventKind::Done,
                    message_part(data_proto::SessionMessagePartType::Text, "", "", ""),
                    4,
                )
                .encode_to_vec(),
            ],
        )])));
        let gateway = setup_gateway(
            Arc::new(MockKvStore::default()),
            streams,
            Arc::new(Mutex::new(Vec::new())),
        );

        let response = get_chat(
            State(gateway),
            Path(SessionPath {
                ns: "default".to_string(),
                agent: "agent".to_string(),
                session_id: session_id.to_string(),
            }),
            HeaderMap::new(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains(r#""type":"tool_call""#));
        assert!(text.contains(r#""toolCallId":"call-1""#));
        assert!(text.contains(r#""type":"tool_result""#));
        assert!(text.contains(r#""ok":true"#));
        assert!(text.contains(r#""type":"text","value":"Hello""#));
    }

    #[tokio::test]
    async fn delete_chat_stops_generation_and_returns_no_content() {
        let kv = Arc::new(MockKvStore::default());
        kv.set_msg(
            &keys::session("default", "agent", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "agent".to_string(),
                ns: "default".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 1,
                last_active: 2,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        let published = Arc::new(Mutex::new(Vec::new()));
        let gateway = setup_gateway(kv, Arc::new(Mutex::new(HashMap::new())), published.clone());

        let response = delete_chat(
            State(gateway),
            Path(SessionPath {
                ns: "default".to_string(),
                agent: "agent".to_string(),
                session_id: "session-1".to_string(),
            }),
            HeaderMap::new(),
            Json(Value::Null),
        )
        .await;

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let published = published.lock().await;
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, topics::SESSION_CONTROL_TOPIC);
        let event = SessionControlEvent::decode(published[0].1.as_slice()).unwrap();
        assert_eq!(event.action, "stop_generation");
    }
}
