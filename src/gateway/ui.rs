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
            message_limit: 0,
            step_limit: 0,
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

fn stable_payload_hash(payload: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    payload.as_bytes().iter().fold(FNV_OFFSET, |hash, byte| {
        hash.wrapping_mul(FNV_PRIME) ^ u64::from(*byte)
    })
}

fn step_dedup_key(step: &events::SessionStepEvent) -> String {
    let payload_hash = stable_payload_hash(&step.payload_json);
    format!(
        "{}:{}:{}:{}:{}:{}",
        step.message_id,
        step.timestamp,
        step.step_type,
        step.name,
        step.content,
        payload_hash
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
                } else if step.step_type == StepType::Reasoning as i32 {
                    if !step.content.is_empty() {
                        yield Ok::<_, Infallible>(data_stream_line("g", json!(step.content)));
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
                } else if step.step_type == StepType::Usage as i32 {
                    let usage = serde_json::from_str::<Value>(&step.payload_json)
                        .unwrap_or_else(|_| json!({}));
                    yield Ok::<_, Infallible>(data_stream_line("h", usage));
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
            } else if step.step_type == StepType::Reasoning as i32 {
                if !step.content.is_empty() {
                    yield Ok::<_, Infallible>(ndjson_line(json!({ "type": "reasoning", "value": step.content })));
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
            } else if step.step_type == StepType::Usage as i32 {
                let usage = serde_json::from_str::<Value>(&step.payload_json)
                    .unwrap_or_else(|_| json!({}));
                yield Ok::<_, Infallible>(ndjson_line(json!({
                    "type": "usage",
                    "value": usage
                })));
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
    use super::{
        data_stream_line, delete_chat, extract_tool_step_payload, fetch_session, get_chat,
        last_message_text, latest_assistant_message_text, latest_tool_step_payload, map_status,
        ndjson_line, post_chat, step_dedup_key, tonic_request, ChatRequestBody, SessionPath,
        UiMessage, UiPart,
    };
    use crate::control::events::{SessionControlEvent, SessionStepEvent, StepType};
    use crate::control::{
        keys,
        scheduler::NoopSchedulerBackend,
        topics, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::{models, proto};
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
        data: Mutex<HashMap<(String, String), Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, ns: &str, k: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self
                .data
                .lock()
                .await
                .get(&(ns.to_string(), k.to_string()))
                .cloned())
        }

        async fn set(&self, ns: &str, k: &str, v: &[u8]) -> anyhow::Result<()> {
            self.data
                .lock()
                .await
                .insert((ns.to_string(), k.to_string()), v.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            ns: &str,
            k: &str,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let key = (ns.to_string(), k.to_string());
            let current = data.get(&key).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(key, value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, ns: &str, k: &str) -> anyhow::Result<()> {
            self.data
                .lock()
                .await
                .remove(&(ns.to_string(), k.to_string()));
            Ok(())
        }

        async fn list_keys(&self, ns: &str, p: &str) -> anyhow::Result<Vec<String>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|(stored_ns, key)| {
                    (stored_ns == ns && key.starts_with(p)).then(|| key.clone())
                })
                .collect::<Vec<_>>();
            keys.sort();
            Ok(keys)
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
            session_streams: Arc::new(SessionStreamHub::new(pubsub)),
        })
    }

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
    fn last_message_text_uses_text_parts_when_content_is_empty() {
        let messages = vec![UiMessage {
            content: Some("   ".to_string()),
            parts: vec![
                UiPart {
                    kind: Some("text".to_string()),
                    text: Some("hello".to_string()),
                },
                UiPart {
                    kind: Some("text".to_string()),
                    text: Some(" world".to_string()),
                },
            ],
        }];

        assert_eq!(last_message_text(&messages).as_deref(), Some("hello world"));
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

    #[test]
    fn extract_tool_step_payload_defaults_and_rejects_invalid_payloads() {
        let fallback_step = SessionStepEvent {
            session_id: "s".to_string(),
            step_type: StepType::Observation as i32,
            content: "fallback-result".to_string(),
            timestamp: 0,
            agent: "agent".to_string(),
            ns: "ns".to_string(),
            message_id: "message".to_string(),
            name: String::new(),
            payload_json: r#"{"tool_call_id":"call-9"}"#.to_string(),
        };
        let payload = extract_tool_step_payload(&fallback_step).expect("payload should parse");
        assert_eq!(payload.tool_name, "tool");
        assert_eq!(payload.args, json!({}));
        assert_eq!(payload.result, Value::String("fallback-result".to_string()));

        let missing_id = SessionStepEvent {
            payload_json: r#"{"input":{"q":"rust"}}"#.to_string(),
            ..fallback_step.clone()
        };
        assert!(extract_tool_step_payload(&missing_id).is_none());

        let empty_id = SessionStepEvent {
            payload_json: r#"{"tool_call_id":""}"#.to_string(),
            ..fallback_step.clone()
        };
        assert!(extract_tool_step_payload(&empty_id).is_none());

        let invalid_json = SessionStepEvent {
            payload_json: "{not-json}".to_string(),
            ..fallback_step
        };
        assert!(extract_tool_step_payload(&invalid_json).is_none());
    }

    #[test]
    fn latest_tool_step_payload_returns_last_matching_entry() {
        let steps = vec![
            SessionStepEvent {
                session_id: "s".to_string(),
                step_type: StepType::Action as i32,
                content: String::new(),
                timestamp: 1,
                agent: "agent".to_string(),
                ns: "ns".to_string(),
                message_id: "msg-1".to_string(),
                name: "first".to_string(),
                payload_json: r#"{"tool_call_id":"call-1","input":{"q":"first"}}"#.to_string(),
            },
            SessionStepEvent {
                session_id: "s".to_string(),
                step_type: StepType::Observation as i32,
                content: String::new(),
                timestamp: 2,
                agent: "agent".to_string(),
                ns: "ns".to_string(),
                message_id: "msg-1".to_string(),
                name: "obs".to_string(),
                payload_json: r#"{"tool_call_id":"call-1","output":{"ok":true}}"#.to_string(),
            },
            SessionStepEvent {
                session_id: "s".to_string(),
                step_type: StepType::Action as i32,
                content: String::new(),
                timestamp: 3,
                agent: "agent".to_string(),
                ns: "ns".to_string(),
                message_id: "msg-2".to_string(),
                name: "second".to_string(),
                payload_json: r#"{"tool_call_id":"call-2","input":{"q":"second"}}"#.to_string(),
            },
        ];

        let payload = latest_tool_step_payload(steps.iter(), StepType::Action as i32).unwrap();
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
                models::SessionMessage {
                    id: "m1".to_string(),
                    role: 1,
                    content: "user".to_string(),
                    created_at: 1,
                    labels: HashMap::new(),
                },
                models::SessionMessage {
                    id: "m2".to_string(),
                    role: 2,
                    content: String::new(),
                    created_at: 2,
                    labels: HashMap::new(),
                },
                models::SessionMessage {
                    id: "m3".to_string(),
                    role: 2,
                    content: "done".to_string(),
                    created_at: 3,
                    labels: HashMap::new(),
                },
            ],
            steps: Vec::new(),
            labels: HashMap::new(),
        };

        assert_eq!(
            latest_assistant_message_text(&response).as_deref(),
            Some("done")
        );
    }

    #[test]
    fn framing_and_dedup_helpers_return_stable_output() {
        let step = SessionStepEvent {
            session_id: "s".to_string(),
            step_type: StepType::Token as i32,
            content: "hello".to_string(),
            timestamp: 7,
            agent: "agent".to_string(),
            ns: "ns".to_string(),
            message_id: "msg-1".to_string(),
            name: "name".to_string(),
            payload_json: String::new(),
        };

        assert_eq!(
            String::from_utf8(ndjson_line(json!({"type":"text","value":"hello"}))).unwrap(),
            "{\"type\":\"text\",\"value\":\"hello\"}\n"
        );
        assert_eq!(
            String::from_utf8(data_stream_line("0", json!("hello"))).unwrap(),
            "0:\"hello\"\n"
        );
        let key = step_dedup_key(&step);
        assert!(key.starts_with("msg-1:7:1:name:hello:"));
        assert!(!key.ends_with(':'));
        assert_eq!(key, step_dedup_key(&step));
    }

    #[tokio::test]
    async fn fetch_session_reads_seeded_session_state() {
        let kv = Arc::new(MockKvStore::default());
        kv.set_msg(
            "default",
            &keys::session("agent", "session-1"),
            &models::Session {
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
        let gateway = setup_gateway(
            kv,
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(Vec::new())),
        );

        let response = fetch_session(
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
                    content: None,
                    parts: vec![UiPart {
                        kind: Some("text".to_string()),
                        text: Some("   ".to_string()),
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
                    content: Some("hello".to_string()),
                    parts: vec![],
                }],
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("Session not found"));
    }

    #[tokio::test]
    async fn get_chat_streams_tool_calls_results_and_text() {
        let session_id = "session-123";
        let topic_name =
            topics::session_step_topic_for_shard(topics::session_step_shard(session_id));
        let streams = Arc::new(Mutex::new(HashMap::from([(
            topic_name,
            vec![
                SessionStepEvent {
                    session_id: session_id.to_string(),
                    step_type: StepType::Action as i32,
                    content: String::new(),
                    timestamp: 1,
                    agent: "agent".to_string(),
                    ns: "default".to_string(),
                    message_id: "msg-1".to_string(),
                    name: "search".to_string(),
                    payload_json: r#"{"tool_call_id":"call-1","input":{"q":"rust"}}"#.to_string(),
                }
                .encode_to_vec(),
                SessionStepEvent {
                    session_id: session_id.to_string(),
                    step_type: StepType::Observation as i32,
                    content: "fallback".to_string(),
                    timestamp: 2,
                    agent: "agent".to_string(),
                    ns: "default".to_string(),
                    message_id: "msg-1".to_string(),
                    name: "search".to_string(),
                    payload_json: r#"{"tool_call_id":"call-1","output":{"ok":true}}"#.to_string(),
                }
                .encode_to_vec(),
                SessionStepEvent {
                    session_id: session_id.to_string(),
                    step_type: StepType::Token as i32,
                    content: "Hello".to_string(),
                    timestamp: 3,
                    agent: "agent".to_string(),
                    ns: "default".to_string(),
                    message_id: "msg-1".to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                }
                .encode_to_vec(),
                SessionStepEvent {
                    session_id: session_id.to_string(),
                    step_type: StepType::Done as i32,
                    content: String::new(),
                    timestamp: 4,
                    agent: "agent".to_string(),
                    ns: "default".to_string(),
                    message_id: "msg-1".to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                }
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
            "default",
            &keys::session("agent", "session-1"),
            &models::Session {
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
