// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::events::{self, StepType};
use crate::gateway::rpc::{proto, GrpcGatewayHandler};
use crate::gateway::Gateway;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderName, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tonic::metadata::MetadataValue;
use uuid::Uuid;

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
    content: Option<String>,
    #[serde(default)]
    parts: Vec<UiPart>,
}

#[derive(Deserialize)]
pub struct UiPart {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    text: Option<String>,
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

fn last_message_text(messages: &[UiMessage]) -> Option<String> {
    messages.iter().rev().find_map(|message| {
        if let Some(content) = &message.content {
            if !content.trim().is_empty() {
                return Some(content.clone());
            }
        }

        let text = message
            .parts
            .iter()
            .filter(|part| part.kind.as_deref() == Some("text"))
            .filter_map(|part| part.text.as_deref())
            .collect::<String>();

        if text.trim().is_empty() {
            None
        } else {
            Some(text)
        }
    })
}

fn extract_tool_step_payload(step: &events::SessionStepEvent) -> Option<ToolStepPayload> {
    let payload: Value = serde_json::from_str(&step.payload_json).ok()?;
    let tool_call_id = payload.get("tool_call_id")?.as_str()?.to_string();
    if tool_call_id.is_empty() {
        return None;
    }

    Some(ToolStepPayload {
        tool_call_id,
        tool_name: if step.name.is_empty() {
            "tool".to_string()
        } else {
            step.name.clone()
        },
        args: payload.get("input").cloned().unwrap_or_else(|| json!({})),
        result: payload
            .get("output")
            .cloned()
            .unwrap_or_else(|| Value::String(step.content.clone())),
    })
}

fn latest_tool_step_payload<'a, I>(steps: I, step_type: i32) -> Option<ToolStepPayload>
where
    I: IntoIterator<Item = &'a events::SessionStepEvent>,
    I::IntoIter: DoubleEndedIterator,
{
    steps.into_iter()
        .rev()
        .find(|step| step.step_type == step_type)
        .and_then(extract_tool_step_payload)
}

async fn fetch_session(
    gateway: &Arc<Gateway>,
    headers: &HeaderMap,
    path: &SessionPath,
) -> Result<proto::SessionResponse, Response> {
    let request = tonic_request(
        headers,
        proto::GetSessionRequest {
            ns: path.ns.clone(),
            agent: path.agent.clone(),
            session_id: path.session_id.clone(),
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
        .find(|message| message.role == 2 && !message.content.trim().is_empty())
        .map(|message| message.content.clone())
}

fn ndjson_line(value: Value) -> Vec<u8> {
    format!("{}\n", value).into_bytes()
}

fn data_stream_line(code: &str, value: Value) -> Vec<u8> {
    format!("{code}:{}\n", value).into_bytes()
}

fn step_dedup_key(step: &events::SessionStepEvent) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        step.message_id, step.timestamp, step.step_type, step.name, step.content
    )
}

pub async fn post_chat(
    State(gateway): State<Arc<Gateway>>,
    Path(path): Path<SessionPath>,
    headers: HeaderMap,
    Json(body): Json<ChatRequestBody>,
) -> Response {
    let Some(message) = last_message_text(&body.messages) else {
        return response_with_status(StatusCode::BAD_REQUEST, "message content is required");
    };

    let send_request = match tonic_request(
        &headers,
        proto::SendMessageRequest {
            ns: path.ns.clone(),
            agent: path.agent.clone(),
            session_id: path.session_id.clone(),
            message,
            labels: Default::default(),
        },
    ) {
        Ok(request) => request,
        Err(response) => return response,
    };

    let baseline_seen_steps = fetch_session(&gateway, &headers, &path)
        .await
        .map(|response| {
            response
                .steps
                .iter()
                .map(step_dedup_key)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    if let Err(status) = gateway_handler(&gateway)
        .handle_send_message(send_request)
        .await
    {
        return map_status(status);
    }

    let gateway_for_stream = gateway.clone();
    let headers_for_stream = headers.clone();
    let path_for_stream = path;
    let stream = async_stream::stream! {
        let mut started_step = false;
        let mut started_message_id: Option<String> = None;
        let mut emitted_any_text = false;
        let mut seen_steps = baseline_seen_steps;

        for _ in 0..300 {
            let response = match fetch_session(&gateway_for_stream, &headers_for_stream, &path_for_stream).await {
                Ok(response) => response,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };

            for (step_idx, step) in response.steps.iter().enumerate() {
                let dedup_key = step_dedup_key(step);
                if !seen_steps.insert(dedup_key) {
                    continue;
                }

                if !started_step {
                    let message_id = if step.message_id.is_empty() {
                        Uuid::now_v7().to_string()
                    } else {
                        step.message_id.clone()
                    };
                    started_message_id = Some(message_id.clone());
                    started_step = true;
                    yield Ok::<_, Infallible>(data_stream_line("f", json!({ "messageId": message_id })));
                } else if started_message_id.as_deref() != Some(step.message_id.as_str()) && !step.message_id.is_empty() {
                    started_message_id = Some(step.message_id.clone());
                    yield Ok::<_, Infallible>(data_stream_line("f", json!({ "messageId": step.message_id })));
                }

                if step.step_type == StepType::Token as i32 {
                    if !step.content.is_empty() {
                        emitted_any_text = true;
                        yield Ok::<_, Infallible>(data_stream_line("0", json!(step.content)));
                    }
                } else if step.step_type == StepType::Action as i32 {
                    let payload = match extract_tool_step_payload(step) {
                        Some(payload) => Some(payload),
                        None => latest_tool_step_payload(
                            response.steps[..=step_idx].iter(),
                            StepType::Action as i32,
                        ),
                    };
                    let tool_call_id = payload
                        .as_ref()
                        .map(|payload| payload.tool_call_id.clone())
                        .unwrap_or_else(|| format!("tool-{}", Uuid::now_v7()));
                    let tool_name = payload
                        .as_ref()
                        .map(|payload| payload.tool_name.clone())
                        .unwrap_or_else(|| if step.name.is_empty() { "tool".to_string() } else { step.name.clone() });
                    let args = payload
                        .as_ref()
                        .map(|payload| payload.args.clone())
                        .unwrap_or_else(|| json!({}));
                    yield Ok::<_, Infallible>(data_stream_line("9", json!({
                        "toolCallId": tool_call_id,
                        "toolName": tool_name,
                        "args": args
                    })));
                } else if step.step_type == StepType::Observation as i32 {
                    let payload = match extract_tool_step_payload(step) {
                        Some(payload) => Some(payload),
                        None => latest_tool_step_payload(
                            response.steps[..=step_idx].iter(),
                            StepType::Observation as i32,
                        ),
                    };
                    if let Some(payload) = payload {
                        yield Ok::<_, Infallible>(data_stream_line("a", json!({
                            "toolCallId": payload.tool_call_id,
                            "result": payload.result
                        })));
                    }
                } else if step.step_type == StepType::Error as i32 {
                    let error_text = if step.content.is_empty() {
                        "Stream error".to_string()
                    } else {
                        step.content.clone()
                    };
                    yield Ok::<_, Infallible>(data_stream_line("3", json!(error_text)));
                    return;
                }
            }

            if response.state != "PROCESSING" {
                if !emitted_any_text {
                    if let Some(text) = latest_assistant_message_text(&response) {
                        if !started_step {
                            let message_id = response
                                .messages
                                .iter()
                                .rev()
                                .find(|message| message.role == 2)
                                .map(|message| message.id.clone())
                                .unwrap_or_else(|| Uuid::now_v7().to_string());
                            yield Ok::<_, Infallible>(data_stream_line("f", json!({ "messageId": message_id })));
                        }
                        yield Ok::<_, Infallible>(data_stream_line("0", json!(text)));
                    } else {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                }
                yield Ok::<_, Infallible>(data_stream_line("e", json!({
                    "finishReason": "stop",
                    "isContinued": false
                })));
                yield Ok::<_, Infallible>(data_stream_line("d", json!({
                    "finishReason": "stop"
                })));
                return;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        yield Ok::<_, Infallible>(data_stream_line("3", json!("Timed out waiting for assistant response")));
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
        proto::StreamSessionStepsRequest {
            ns: path.ns.clone(),
            agent: path.agent.clone(),
            session_id: path.session_id.clone(),
        },
    ) {
        Ok(request) => request,
        Err(response) => return response,
    };

    let response = match gateway_handler(&gateway)
        .handle_stream_session_steps(request)
        .await
    {
        Ok(response) => response,
        Err(status) => return map_status(status),
    };

    let stream = async_stream::stream! {
        let mut steps = response.into_inner();
        let mut latest_action_payload: Option<ToolStepPayload> = None;
        let mut latest_observation_payload: Option<ToolStepPayload> = None;
        while let Some(step_result) = steps.next().await {
            let step = match step_result {
                Ok(step) => step,
                Err(status) => {
                    yield Ok::<_, Infallible>(ndjson_line(json!({ "type": "error", "value": status.message() })));
                    break;
                }
            };

            if step.step_type == StepType::Token as i32 {
                if !step.content.is_empty() {
                    yield Ok::<_, Infallible>(ndjson_line(json!({ "type": "text", "value": step.content })));
                }
            } else if step.step_type == StepType::Action as i32 {
                let payload = match extract_tool_step_payload(&step) {
                    Some(payload) => {
                        latest_action_payload = Some(payload.clone());
                        Some(payload)
                    }
                    None => latest_action_payload.clone(),
                };
                let tool_call_id = payload
                    .as_ref()
                    .map(|payload| payload.tool_call_id.clone())
                    .unwrap_or_else(|| format!("tool-{}", Uuid::now_v7()));
                let tool_name = payload
                    .as_ref()
                    .map(|payload| payload.tool_name.clone())
                    .unwrap_or_else(|| if step.name.is_empty() { "tool".to_string() } else { step.name.clone() });
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
            } else if step.step_type == StepType::Observation as i32 {
                let payload = match extract_tool_step_payload(&step) {
                    Some(payload) => {
                        latest_observation_payload = Some(payload.clone());
                        Some(payload)
                    }
                    None => latest_observation_payload.clone(),
                };
                if let Some(payload) = payload {
                    yield Ok::<_, Infallible>(ndjson_line(json!({
                        "type": "tool_result",
                        "value": {
                            "toolCallId": payload.tool_call_id,
                            "result": payload.result
                        }
                    })));
                }
            } else if step.step_type == StepType::Done as i32 {
                break;
            } else if step.step_type == StepType::Error as i32 {
                let error_text = if step.content.is_empty() {
                    "Stream error".to_string()
                } else {
                    step.content
                };
                yield Ok::<_, Infallible>(ndjson_line(json!({ "type": "error", "value": error_text })));
                break;
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
    use super::{extract_tool_step_payload, last_message_text, UiMessage, UiPart};
    use crate::control::events::{SessionStepEvent, StepType};

    #[test]
    fn last_message_text_prefers_content_then_text_parts() {
        let messages = vec![
            UiMessage {
                content: Some(String::new()),
                parts: vec![UiPart {
                    kind: Some("text".to_string()),
                    text: Some("hello".to_string()),
                }],
            },
            UiMessage {
                content: Some("world".to_string()),
                parts: vec![],
            },
        ];

        assert_eq!(last_message_text(&messages).as_deref(), Some("world"));
    }

    #[test]
    fn extract_tool_step_payload_parses_tool_metadata() {
        let step = SessionStepEvent {
            session_id: "s".to_string(),
            step_type: StepType::Action as i32,
            content: String::new(),
            timestamp: 0,
            agent: "agent".to_string(),
            ns: "ns".to_string(),
            message_id: "message".to_string(),
            name: "search".to_string(),
            payload_json:
                r#"{"tool_call_id":"call-123","input":{"q":"rust"},"output":{"ok":true}}"#
                    .to_string(),
        };

        let payload = extract_tool_step_payload(&step).expect("payload should parse");
        assert_eq!(payload.tool_call_id, "call-123");
        assert_eq!(payload.tool_name, "search");
        assert_eq!(payload.args["q"], "rust");
        assert_eq!(payload.result["ok"], true);
    }
}
