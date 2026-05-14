use crate::control::events::{self, StepType};
use crate::gateway::rpc::{proto, GrpcGatewayHandler};
use crate::gateway::Gateway;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
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

async fn fetch_latest_tool_step(
    gateway: &Arc<Gateway>,
    headers: &HeaderMap,
    path: &SessionPath,
    step_type: i32,
) -> Result<Option<ToolStepPayload>, Response> {
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

    Ok(response
        .steps
        .iter()
        .rev()
        .find(|step| step.step_type == step_type)
        .and_then(extract_tool_step_payload))
}

fn sse_json(value: Value) -> Result<Event, Infallible> {
    Ok(Event::default().data(value.to_string()))
}

fn ndjson_line(value: Value) -> Vec<u8> {
    format!("{}\n", value).into_bytes()
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

    let request = match tonic_request(
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

    if let Err(status) = gateway_handler(&gateway).handle_send_message(request).await {
        return map_status(status);
    }

    let gateway_for_stream = gateway.clone();
    let headers_for_stream = headers.clone();
    let path_for_stream = path;
    let message_id = Uuid::now_v7().to_string();
    let text_part_id = Uuid::now_v7().to_string();

    let stream = async_stream::stream! {
        yield sse_json(json!({ "type": "start", "messageId": message_id.clone() }));
        yield sse_json(json!({ "type": "start-step" }));

        let request = match tonic_request(
            &headers_for_stream,
            proto::StreamSessionStepsRequest {
                ns: path_for_stream.ns.clone(),
                agent: path_for_stream.agent.clone(),
                session_id: path_for_stream.session_id.clone(),
            },
        ) {
            Ok(request) => request,
            Err(response) => {
                let body = format!("{:?}", response);
                yield sse_json(json!({ "type": "error", "errorText": body }));
                yield sse_json(json!({ "type": "finish-step" }));
                yield sse_json(json!({ "type": "finish" }));
                yield Ok(Event::default().data("[DONE]"));
                return;
            }
        };

        let response = match gateway_handler(&gateway_for_stream).handle_stream_session_steps(request).await {
            Ok(response) => response,
            Err(status) => {
                yield sse_json(json!({ "type": "error", "errorText": status.message() }));
                yield sse_json(json!({ "type": "finish-step" }));
                yield sse_json(json!({ "type": "finish" }));
                yield Ok(Event::default().data("[DONE]"));
                return;
            }
        };

        let mut steps = response.into_inner();
        let mut text_started = false;

        while let Some(step_result) = steps.next().await {
            let step = match step_result {
                Ok(step) => step,
                Err(status) => {
                    yield sse_json(json!({ "type": "error", "errorText": status.message() }));
                    break;
                }
            };

            if step.step_type == StepType::Token as i32 {
                if step.content.is_empty() {
                    continue;
                }
                if !text_started {
                    text_started = true;
                    yield sse_json(json!({ "type": "text-start", "id": text_part_id }));
                }
                yield sse_json(json!({ "type": "text-delta", "id": text_part_id, "delta": step.content }));
            } else if step.step_type == StepType::Action as i32 {
                let payload = match extract_tool_step_payload(&step) {
                    Some(payload) => Some(payload),
                    None => match fetch_latest_tool_step(
                        &gateway_for_stream,
                        &headers_for_stream,
                        &path_for_stream,
                        StepType::Action as i32,
                    ).await {
                        Ok(payload) => payload,
                        Err(response) => {
                            yield sse_json(json!({ "type": "error", "errorText": format!("{:?}", response) }));
                            None
                        }
                    }
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

                yield sse_json(json!({
                    "type": "tool-input-available",
                    "toolCallId": tool_call_id,
                    "toolName": tool_name,
                    "input": args,
                    "dynamic": true
                }));
            } else if step.step_type == StepType::Observation as i32 {
                let payload = match extract_tool_step_payload(&step) {
                    Some(payload) => Some(payload),
                    None => match fetch_latest_tool_step(
                        &gateway_for_stream,
                        &headers_for_stream,
                        &path_for_stream,
                        StepType::Observation as i32,
                    ).await {
                        Ok(payload) => payload,
                        Err(response) => {
                            yield sse_json(json!({ "type": "error", "errorText": format!("{:?}", response) }));
                            None
                        }
                    }
                };

                if let Some(payload) = payload {
                    yield sse_json(json!({
                        "type": "tool-output-available",
                        "toolCallId": payload.tool_call_id,
                        "output": payload.result,
                        "dynamic": true
                    }));
                }
            } else if step.step_type == StepType::Done as i32 {
                break;
            } else if step.step_type == StepType::Error as i32 {
                let error_text = if step.content.is_empty() {
                    "Stream error".to_string()
                } else {
                    step.content
                };
                yield sse_json(json!({ "type": "error", "errorText": error_text }));
                break;
            }
        }

        if text_started {
            yield sse_json(json!({ "type": "text-end", "id": text_part_id }));
        }
        yield sse_json(json!({ "type": "finish-step" }));
        yield sse_json(json!({ "type": "finish" }));
        yield Ok(Event::default().data("[DONE]"));
    };

    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    response.headers_mut().insert(
        HeaderName::from_static("x-vercel-ai-ui-message-stream"),
        HeaderValue::from_static("v1"),
    );
    response
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

    let gateway_for_stream = gateway.clone();
    let headers_for_stream = headers.clone();
    let path_for_stream = path;

    let stream = async_stream::stream! {
        let mut steps = response.into_inner();
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
                    Some(payload) => Some(payload),
                    None => fetch_latest_tool_step(
                        &gateway_for_stream,
                        &headers_for_stream,
                        &path_for_stream,
                        StepType::Action as i32,
                    ).await.ok().flatten(),
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
                    Some(payload) => Some(payload),
                    None => fetch_latest_tool_step(
                        &gateway_for_stream,
                        &headers_for_stream,
                        &path_for_stream,
                        StepType::Observation as i32,
                    ).await.ok().flatten(),
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
