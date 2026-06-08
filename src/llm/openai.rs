// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::llm::provider::{
    ChatContentPart, ChatMessage, ChatRequest, ChatResponse, ChatStream, ChatStreamEvent,
    ChatUsage, LlmProvider, ToolCallDelta,
};
use crate::memory::Embedding;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::{stream, Stream, StreamExt};
use serde_json::Value;
use std::{
    pin::Pin,
    sync::OnceLock,
    task::{Context, Poll},
};

const DEFAULT_THINKING_BUDGET_TOKENS: u32 = 1024;
const THINKING_COMPLETION_BUFFER_TOKENS: u32 = 4096;

fn openai_content_part(part: ChatContentPart) -> serde_json::Value {
    match part {
        ChatContentPart::Text { text } => serde_json::json!({
            "type": "text",
            "text": text,
        }),
        ChatContentPart::ImageUrl { url, detail } => {
            let mut image_url = serde_json::json!({ "url": url });
            if let Some(detail) = detail {
                image_url["detail"] = serde_json::Value::String(detail);
            }
            serde_json::json!({
                "type": "image_url",
                "image_url": image_url,
            })
        }
        ChatContentPart::ImageData {
            media_type,
            data_base64,
            detail,
        } => {
            let mut image_url = serde_json::json!({
                "url": format!("data:{media_type};base64,{data_base64}"),
            });
            if let Some(detail) = detail {
                image_url["detail"] = serde_json::Value::String(detail);
            }
            serde_json::json!({
                "type": "image_url",
                "image_url": image_url,
            })
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RequestDebugStats {
    message_count: usize,
    tool_count: usize,
    message_chars: usize,
    tool_schema_chars: usize,
    payload_chars: usize,
}

pub struct OpenAiCompatibleProvider {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub http_client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            api_key,
            base_url,
            model,
            http_client: shared_http_client(),
        }
    }

    fn serialize_messages(messages: Vec<ChatMessage>) -> Vec<serde_json::Value> {
        messages
            .into_iter()
            .map(|message| {
                let content = if message.content_parts.is_empty() {
                    serde_json::Value::String(message.content)
                } else {
                    serde_json::Value::Array(
                        message
                            .content_parts
                            .into_iter()
                            .map(openai_content_part)
                            .collect(),
                    )
                };
                let mut json = serde_json::json!({
                    "role": message.role,
                    "content": content,
                });

                if let Some(tool_calls) = message.tool_calls {
                    let openai_tool_calls: Vec<serde_json::Value> = tool_calls
                        .into_iter()
                        .map(|tool| {
                            serde_json::json!({
                                "id": tool.id,
                                "type": "function",
                                "function": {
                                    "name": tool.name,
                                    "arguments": tool.arguments,
                                }
                            })
                        })
                        .collect();
                    json["tool_calls"] = serde_json::json!(openai_tool_calls);
                }

                if let Some(tool_call_id) = message.tool_call_id {
                    json["tool_call_id"] = serde_json::json!(tool_call_id);
                }

                json
            })
            .collect()
    }

    fn supports_tool_retry_without_tools(
        &self,
        messages: &[ChatMessage],
        err_text: &str,
        tools_were_sent: bool,
    ) -> bool {
        tools_were_sent
            && self.base_url.contains("novita.ai")
            && err_text.contains("internal_server_error")
            && messages.iter().any(|m| {
                m.role == "tool" || m.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty())
            })
    }

    fn supports_stream_options_retry(&self, stream: bool, err_text: &str) -> bool {
        stream && {
            let lower = err_text.to_ascii_lowercase();
            lower.contains("stream_options")
                || lower.contains("include_usage")
                || lower.contains("unknown field")
                || lower.contains("unknown parameter")
                || lower.contains("unexpected field")
        }
    }

    fn debug_requests_enabled() -> bool {
        std::env::var("TALON_LLM_DEBUG_REQUESTS")
            .ok()
            .map(|value| {
                let normalized = value.trim().to_ascii_lowercase();
                matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
            })
            .unwrap_or(false)
    }

    fn truncate_for_log(text: &str, max_chars: usize) -> String {
        let chars = text.chars().collect::<Vec<_>>();
        if chars.len() <= max_chars {
            return text.to_string();
        }
        chars.into_iter().take(max_chars).collect::<String>()
    }

    fn compute_request_debug_stats(
        serialized_messages: &[Value],
        tools: &[crate::llm::provider::Tool],
        payload: &Value,
    ) -> RequestDebugStats {
        let message_chars = serialized_messages
            .iter()
            .map(|message| serde_json::to_string(message).unwrap_or_default().len())
            .sum::<usize>();
        let tool_schema_chars = tools
            .iter()
            .map(|tool| {
                serde_json::to_string(&tool.input_schema)
                    .unwrap_or_default()
                    .len()
                    + tool.name.len()
                    + tool.description.len()
            })
            .sum::<usize>();
        let payload_chars = serde_json::to_string(payload).unwrap_or_default().len();

        RequestDebugStats {
            message_count: serialized_messages.len(),
            tool_count: tools.len(),
            message_chars,
            tool_schema_chars,
            payload_chars,
        }
    }

    fn log_request_attempt(
        &self,
        attempt: &str,
        include_tools: bool,
        stream: bool,
        serialized_messages: &[Value],
        tools: &[crate::llm::provider::Tool],
        payload: &Value,
    ) {
        let stats = Self::compute_request_debug_stats(serialized_messages, tools, payload);
        let debug_requests = Self::debug_requests_enabled();
        let payload_json = if debug_requests {
            serde_json::to_string(payload).unwrap_or_default()
        } else {
            String::new()
        };

        tracing::info!(
            provider_base_url = %self.base_url,
            model = %self.model,
            attempt,
            include_tools,
            stream,
            message_count = stats.message_count,
            tool_count = stats.tool_count,
            message_chars = stats.message_chars,
            tool_schema_chars = stats.tool_schema_chars,
            payload_chars = stats.payload_chars,
            payload_json = if payload_json.is_empty() {
                None
            } else {
                Some(payload_json.as_str())
            },
            payload_preview = if payload_json.is_empty() {
                None
            } else {
                Some(Self::truncate_for_log(&payload_json, 4_000))
            },
            "Sending OpenAI-compatible LLM request"
        );
    }

    fn log_request_failure(
        &self,
        attempt: &str,
        include_tools: bool,
        stream: bool,
        serialized_messages: &[Value],
        tools: &[crate::llm::provider::Tool],
        payload: &Value,
        status: reqwest::StatusCode,
        err_text: &str,
    ) {
        let stats = Self::compute_request_debug_stats(serialized_messages, tools, payload);
        let debug_requests = Self::debug_requests_enabled();
        let payload_json = if debug_requests {
            serde_json::to_string(payload).unwrap_or_default()
        } else {
            String::new()
        };

        tracing::warn!(
            provider_base_url = %self.base_url,
            model = %self.model,
            attempt,
            include_tools,
            stream,
            status = %status,
            message_count = stats.message_count,
            tool_count = stats.tool_count,
            message_chars = stats.message_chars,
            tool_schema_chars = stats.tool_schema_chars,
            payload_chars = stats.payload_chars,
            error_text_full = if debug_requests {
                Some(err_text)
            } else {
                None
            },
            error_text = %Self::truncate_for_log(err_text, 4_000),
            payload_json = if payload_json.is_empty() {
                None
            } else {
                Some(payload_json.as_str())
            },
            payload_preview = if payload_json.is_empty() {
                None
            } else {
                Some(Self::truncate_for_log(&payload_json, 4_000))
            },
            "OpenAI-compatible LLM request failed"
        );
    }

    #[tracing::instrument(
        name = "OpenAiCompatibleProvider.send_chat_request",
        skip_all,
        fields(
            provider_base_url = %self.base_url,
            model = %self.model,
            stream,
            message_count = request.messages.len(),
            tool_count = request.tools.len(),
        )
    )]
    async fn send_chat_request(
        &self,
        request: ChatRequest,
        stream: bool,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let serialized_messages = Self::serialize_messages(request.messages.clone());

        let build_payload = |include_tools: bool, include_stream_options: bool| {
            let mut payload = serde_json::json!({
                "model": self.model,
                "messages": serialized_messages,
            });

            if stream {
                payload["stream"] = serde_json::json!(true);
                if include_stream_options {
                    payload["stream_options"] = serde_json::json!({
                        "include_usage": true
                    });
                }
            }

            if include_tools && !request.tools.is_empty() {
                let openai_tools: Vec<serde_json::Value> = request
                    .tools
                    .iter()
                    .map(|tool| {
                        serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.input_schema
                            }
                        })
                    })
                    .collect();
                payload["tools"] = serde_json::json!(openai_tools);
                payload["tool_choice"] = serde_json::json!("auto");
            }

            if let Some(thinking) = request
                .thinking
                .as_ref()
                .filter(|thinking| thinking.enabled)
            {
                if !thinking.effort.trim().is_empty() {
                    payload["reasoning_effort"] = serde_json::json!(thinking.effort);
                }
                let budget_tokens = thinking
                    .budget_tokens
                    .unwrap_or(DEFAULT_THINKING_BUDGET_TOKENS);
                let max_completion_tokens =
                    budget_tokens.saturating_add(THINKING_COMPLETION_BUFFER_TOKENS);
                payload["max_completion_tokens"] = serde_json::json!(max_completion_tokens);
            }

            payload
        };

        let initial_include_tools = !request.tools.is_empty();
        let initial_payload = build_payload(initial_include_tools, true);
        self.log_request_attempt(
            "initial",
            initial_include_tools,
            stream,
            &serialized_messages,
            &request.tools,
            &initial_payload,
        );
        let initial_resp = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&initial_payload)
            .send()
            .await?;

        if initial_resp.status().is_success() {
            return Ok(initial_resp);
        }

        let initial_status = initial_resp.status();
        let err_text = initial_resp.text().await?;
        self.log_request_failure(
            "initial",
            initial_include_tools,
            stream,
            &serialized_messages,
            &request.tools,
            &initial_payload,
            initial_status,
            &err_text,
        );
        if self.supports_stream_options_retry(stream, &err_text) {
            let retry_payload = build_payload(initial_include_tools, false);
            self.log_request_attempt(
                "retry_without_stream_options",
                initial_include_tools,
                stream,
                &serialized_messages,
                &request.tools,
                &retry_payload,
            );
            let retry_resp = self
                .http_client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&retry_payload)
                .send()
                .await?;

            if retry_resp.status().is_success() {
                return Ok(retry_resp);
            }

            let retry_status = retry_resp.status();
            let retry_err_text = retry_resp.text().await?;
            self.log_request_failure(
                "retry_without_stream_options",
                initial_include_tools,
                stream,
                &serialized_messages,
                &request.tools,
                &retry_payload,
                retry_status,
                &retry_err_text,
            );

            if self.supports_tool_retry_without_tools(
                &request.messages,
                &retry_err_text,
                initial_include_tools,
            ) {
                let retry_without_tools_payload = build_payload(false, false);
                self.log_request_attempt(
                    "retry_without_stream_options_or_tools",
                    false,
                    stream,
                    &serialized_messages,
                    &request.tools,
                    &retry_without_tools_payload,
                );
                let retry_without_tools_resp = self
                    .http_client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(&retry_without_tools_payload)
                    .send()
                    .await?;

                if retry_without_tools_resp.status().is_success() {
                    return Ok(retry_without_tools_resp);
                }

                let retry_without_tools_status = retry_without_tools_resp.status();
                let retry_without_tools_err_text = retry_without_tools_resp.text().await?;
                self.log_request_failure(
                    "retry_without_stream_options_or_tools",
                    false,
                    stream,
                    &serialized_messages,
                    &request.tools,
                    &retry_without_tools_payload,
                    retry_without_tools_status,
                    &retry_without_tools_err_text,
                );
                return Err(anyhow!(
                    "OpenAI API error after retry_without_stream_options_or_tools: initial={}, retry_without_stream_options={}, retry_without_stream_options_or_tools={}",
                    err_text,
                    retry_err_text,
                    retry_without_tools_err_text
                ));
            }

            return Err(anyhow!(
                "OpenAI API error after retry_without_stream_options: initial={}, retry_without_stream_options={}",
                err_text,
                retry_err_text
            ));
        }
        if self.supports_tool_retry_without_tools(
            &request.messages,
            &err_text,
            initial_include_tools,
        ) {
            let retry_payload = build_payload(false, true);
            self.log_request_attempt(
                "retry_without_tools",
                false,
                stream,
                &serialized_messages,
                &request.tools,
                &retry_payload,
            );
            let retry_resp = self
                .http_client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&retry_payload)
                .send()
                .await?;

            if retry_resp.status().is_success() {
                return Ok(retry_resp);
            }

            let retry_status = retry_resp.status();
            let retry_err_text = retry_resp.text().await?;
            self.log_request_failure(
                "retry_without_tools",
                false,
                stream,
                &serialized_messages,
                &request.tools,
                &retry_payload,
                retry_status,
                &retry_err_text,
            );
            return Err(anyhow!(
                "OpenAI API error after Novita retry_without_tools: initial={}, retry_without_tools={}",
                err_text,
                retry_err_text
            ));
        }

        Err(anyhow!("OpenAI API error: {}", err_text))
    }
}

fn shared_http_client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
        Ok(vec![0.0; 768])
    }

    #[tracing::instrument(
        name = "OpenAiCompatibleProvider.chat_completion",
        skip_all,
        fields(provider_base_url = %self.base_url, model = %self.model)
    )]
    async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse> {
        use crate::llm::provider::ToolCall;

        let resp = self.send_chat_request(request, false).await?;

        let result: serde_json::Value = resp.json().await?;
        let message = &result["choices"][0]["message"];

        // Extract text content (may be null/missing when the model only returns tool_calls)
        let content = message["content"].as_str().unwrap_or("").to_string();

        // Parse native tool calls if present
        let tool_calls = if let Some(calls) = message["tool_calls"].as_array() {
            calls
                .iter()
                .filter_map(|c| {
                    Some(ToolCall {
                        id: c["id"].as_str()?.to_string(),
                        name: c["function"]["name"].as_str()?.to_string(),
                        arguments: c["function"]["arguments"].as_str()?.to_string(),
                    })
                })
                .collect()
        } else {
            vec![]
        };

        Ok(ChatResponse {
            content,
            tool_calls,
            usage: extract_usage(&result),
        })
    }

    #[tracing::instrument(
        name = "OpenAiCompatibleProvider.stream_chat_completion",
        skip_all,
        fields(provider_base_url = %self.base_url, model = %self.model)
    )]
    async fn stream_chat_completion(&self, request: ChatRequest) -> Result<ChatStream> {
        let resp = self.send_chat_request(request, true).await?;

        let byte_stream = resp.bytes_stream();
        let line_stream = byte_stream.map(|item| item.map_err(|e| anyhow!("Stream error: {}", e)));
        let parent_span = tracing::Span::current();
        let parse_span = parent_span.clone();

        // Simple SSE state machine
        let mut buffer = String::new();
        let mut saw_first_chunk = false;
        let sse_stream = line_stream.flat_map(move |result| match result {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                buffer.push_str(&text);
                let mut items = Vec::new();
                while let Some(pos) = buffer.find('\n') {
                    let line = buffer.drain(..=pos).collect::<String>();
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if line == "data: [DONE]" {
                        parse_span
                            .in_scope(|| tracing::info!("OpenAI-compatible LLM stream completed"));
                        break;
                    }
                    if let Some(data) = line.strip_prefix("data: ") {
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
                            if !saw_first_chunk {
                                saw_first_chunk = true;
                                parse_span.in_scope(|| {
                                    tracing::info!("OpenAI-compatible LLM stream first chunk")
                                });
                            }
                            if let Some(content) = value
                                .pointer("/choices/0/delta/content")
                                .and_then(|v| v.as_str())
                            {
                                items.push(Ok(ChatStreamEvent::TextDelta(content.to_string())));
                            }
                            if let Some(reasoning) = value
                                .pointer("/choices/0/delta/reasoning")
                                .and_then(|v| v.as_str())
                                .or_else(|| {
                                    value
                                        .pointer("/choices/0/delta/reasoning_content")
                                        .and_then(|v| v.as_str())
                                })
                            {
                                items.push(Ok(ChatStreamEvent::ReasoningDelta(
                                    reasoning.to_string(),
                                )));
                            }
                            if let Some(tool_calls) = value
                                .pointer("/choices/0/delta/tool_calls")
                                .and_then(|v| v.as_array())
                            {
                                for call in tool_calls {
                                    let delta = ToolCallDelta {
                                        index: call["index"].as_u64().unwrap_or(0) as usize,
                                        id: call["id"].as_str().map(ToString::to_string),
                                        name: call
                                            .pointer("/function/name")
                                            .and_then(|v| v.as_str())
                                            .map(ToString::to_string),
                                        arguments: call
                                            .pointer("/function/arguments")
                                            .and_then(|v| v.as_str())
                                            .map(ToString::to_string),
                                    };
                                    if delta.id.is_some()
                                        || delta.name.is_some()
                                        || delta.arguments.is_some()
                                    {
                                        items.push(Ok(ChatStreamEvent::ToolCallDelta(delta)));
                                    }
                                }
                            }
                            if let Some(usage) = extract_usage(&value) {
                                items.push(Ok(ChatStreamEvent::Usage(usage)));
                            }
                        }
                    }
                }
                stream::iter(items)
            }
            Err(e) => stream::iter(vec![Err(e)]),
        });

        Ok(Box::pin(SpanInstrumentedChatStream {
            inner: Box::pin(sse_stream),
            span: parent_span,
        }))
    }

    async fn completion(&self, prompt: &str) -> Result<String> {
        self.chat_completion(ChatRequest {
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
                content_parts: Vec::new(),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: vec![],
            thinking: None,
        })
        .await
        .map(|r| r.content)
    }
}

struct SpanInstrumentedChatStream {
    inner: ChatStream,
    span: tracing::Span,
}

impl Stream for SpanInstrumentedChatStream {
    type Item = Result<ChatStreamEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let _entered = this.span.enter();
        this.inner.as_mut().poll_next(cx)
    }
}

fn extract_usage(value: &serde_json::Value) -> Option<ChatUsage> {
    let usage = value.get("usage")?;
    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let reasoning_tokens = usage
        .get("reasoning_tokens")
        .or_else(|| usage.get("thinking_tokens"))
        .or_else(|| usage.pointer("/completion_tokens_details/reasoning_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(input_tokens + output_tokens);

    Some(ChatUsage {
        input_tokens,
        output_tokens,
        reasoning_tokens,
        total_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::rpc::manifests::ThinkingConfig;
    use axum::{extract::State, routing::post, Json, Router};
    use std::{
        net::SocketAddr,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };
    use tokio::net::TcpListener;

    #[test]
    fn request_debug_stats_measure_payload_and_schemas() {
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": "hello"
        })];
        let tools = vec![crate::llm::provider::Tool {
            name: "search".to_string(),
            description: "find things".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "q": {"type": "string"}
                }
            }),
        }];
        let payload = serde_json::json!({
            "model": "minimax/minimax-m2.7",
            "messages": messages,
            "tools": [{
                "type": "function",
                "function": {
                    "name": "search",
                    "description": "find things",
                    "parameters": {"type":"object"}
                }
            }]
        });

        let stats = OpenAiCompatibleProvider::compute_request_debug_stats(
            payload["messages"].as_array().unwrap(),
            &tools,
            &payload,
        );

        assert_eq!(stats.message_count, 1);
        assert_eq!(stats.tool_count, 1);
        assert!(stats.message_chars > 0);
        assert!(stats.tool_schema_chars > 0);
        assert!(stats.payload_chars >= stats.message_chars);
    }

    #[test]
    fn serialize_messages_preserves_tool_protocol_fields() {
        let messages = vec![
            ChatMessage {
                role: "assistant".to_string(),
                content: "".to_string(),
                content_parts: Vec::new(),
                tool_calls: Some(vec![crate::llm::provider::ToolCall {
                    id: "call_1".to_string(),
                    name: "mcp_conic_create_github_pr".to_string(),
                    arguments: "{\"title\":\"x\"}".to_string(),
                }]),
                tool_call_id: None,
            },
            ChatMessage {
                role: "tool".to_string(),
                content: "{\"url\":\"https://github.com/example/repo/pull/2\"}".to_string(),
                content_parts: Vec::new(),
                tool_calls: None,
                tool_call_id: Some("call_1".to_string()),
            },
        ];

        let serialized = OpenAiCompatibleProvider::serialize_messages(messages);

        assert_eq!(
            serialized[0]["tool_calls"][0]["function"]["name"],
            "mcp_conic_create_github_pr"
        );
        assert_eq!(serialized[1]["tool_call_id"], "call_1");
    }

    #[test]
    fn serialize_messages_emits_multimodal_content_parts() {
        let serialized = OpenAiCompatibleProvider::serialize_messages(vec![ChatMessage {
            role: "user".to_string(),
            content: "fallback text".to_string(),
            content_parts: vec![
                ChatContentPart::Text {
                    text: "look at this".to_string(),
                },
                ChatContentPart::ImageData {
                    media_type: "image/png".to_string(),
                    data_base64: "cG5nLWJ5dGVz".to_string(),
                    detail: Some("low".to_string()),
                },
            ],
            tool_calls: None,
            tool_call_id: None,
        }]);

        assert_eq!(serialized[0]["content"][0]["type"], "text");
        assert_eq!(serialized[0]["content"][0]["text"], "look at this");
        assert_eq!(serialized[0]["content"][1]["type"], "image_url");
        assert_eq!(
            serialized[0]["content"][1]["image_url"]["url"],
            "data:image/png;base64,cG5nLWJ5dGVz"
        );
        assert_eq!(serialized[0]["content"][1]["image_url"]["detail"], "low");
    }

    #[test]
    fn supports_tool_retry_without_tools_requires_novita_internal_server_error_and_tool_history() {
        let provider = OpenAiCompatibleProvider::new(
            "key".to_string(),
            "https://api.novita.ai/v3/openai".to_string(),
            "model".to_string(),
        );
        let messages = vec![
            ChatMessage {
                role: "assistant".to_string(),
                content: "".to_string(),
                content_parts: Vec::new(),
                tool_calls: Some(vec![crate::llm::provider::ToolCall {
                    id: "call_1".to_string(),
                    name: "mcp_conic_create_github_pr".to_string(),
                    arguments: "{\"title\":\"x\"}".to_string(),
                }]),
                tool_call_id: None,
            },
            ChatMessage {
                role: "tool".to_string(),
                content: "{\"url\":\"https://github.com/example/repo/pull/2\"}".to_string(),
                content_parts: Vec::new(),
                tool_calls: None,
                tool_call_id: Some("call_1".to_string()),
            },
        ];

        assert!(provider.supports_tool_retry_without_tools(
            &messages,
            "{\"message\":\"internal_server_error\"}",
            true
        ));
        assert!(!provider.supports_tool_retry_without_tools(
            &messages,
            "{\"message\":\"invalid_request_error\"}",
            true
        ));
        assert!(!provider.supports_tool_retry_without_tools(
            &messages,
            "{\"message\":\"internal_server_error\"}",
            false
        ));
    }

    #[test]
    fn debug_requests_enabled_parses_truthy_values() {
        let _guard = crate::test_support::env_lock();
        unsafe {
            std::env::remove_var("TALON_LLM_DEBUG_REQUESTS");
        }
        assert!(!OpenAiCompatibleProvider::debug_requests_enabled());

        for value in ["1", "true", "YES", " on "] {
            unsafe {
                std::env::set_var("TALON_LLM_DEBUG_REQUESTS", value);
            }
            assert!(OpenAiCompatibleProvider::debug_requests_enabled());
        }

        unsafe {
            std::env::set_var("TALON_LLM_DEBUG_REQUESTS", "false");
            std::env::remove_var("TALON_LLM_DEBUG_REQUESTS");
        }
    }

    #[test]
    fn truncate_for_log_preserves_short_strings_and_trims_long_strings() {
        assert_eq!(
            OpenAiCompatibleProvider::truncate_for_log("hello", 10),
            "hello"
        );
        assert_eq!(
            OpenAiCompatibleProvider::truncate_for_log("abcdef", 3),
            "abc"
        );
    }

    #[test]
    fn serialize_messages_omits_absent_tool_fields() {
        let serialized = OpenAiCompatibleProvider::serialize_messages(vec![ChatMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
            content_parts: Vec::new(),
            tool_calls: None,
            tool_call_id: None,
        }]);

        assert_eq!(serialized[0]["role"], "user");
        assert!(serialized[0].get("tool_calls").is_none());
        assert!(serialized[0].get("tool_call_id").is_none());
    }

    #[tokio::test]
    async fn test_openai_sse_parsing() {
        let sse_data = "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\ndata: [DONE]\n";

        let mut buffer = String::new();
        let mut items = Vec::new();

        // Simulating the flat_map logic
        let text = sse_data;
        buffer.push_str(text);
        while let Some(pos) = buffer.find('\n') {
            let line = buffer.drain(..=pos).collect::<String>();
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line == "data: [DONE]" {
                break;
            }
            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(content) = value
                        .pointer("/choices/0/delta/content")
                        .and_then(|v| v.as_str())
                    {
                        items.push(content.to_string());
                    }
                }
            }
        }

        assert_eq!(items, vec!["hello", " world"]);
    }

    #[tokio::test]
    async fn send_chat_request_retries_without_tools_for_novita() {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route(
                "/novita.ai/chat/completions",
                post(
                    move |State(hits): State<Arc<AtomicUsize>>,
                          Json(payload): Json<serde_json::Value>| async move {
                        let hit = hits.fetch_add(1, Ordering::SeqCst);
                        if hit == 0 {
                            assert!(payload.get("tools").is_some());
                            (
                                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                Json(serde_json::json!({
                                    "message": "internal_server_error"
                                })),
                            )
                        } else {
                            assert!(payload.get("tools").is_none());
                            (
                                axum::http::StatusCode::OK,
                                Json(serde_json::json!({
                                    "choices": [{
                                        "message": {
                                            "content": "retried-ok"
                                        }
                                    }]
                                })),
                            )
                        }
                    },
                ),
            )
            .with_state(hits.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleProvider::new(
            "key".to_string(),
            format!("http://{addr}/novita.ai"),
            "model".to_string(),
        );
        let messages = vec![
            ChatMessage {
                role: "assistant".to_string(),
                content: "".to_string(),
                content_parts: Vec::new(),
                tool_calls: Some(vec![crate::llm::provider::ToolCall {
                    id: "call_1".to_string(),
                    name: "search".to_string(),
                    arguments: "{\"q\":\"x\"}".to_string(),
                }]),
                tool_call_id: None,
            },
            ChatMessage {
                role: "tool".to_string(),
                content: "{\"ok\":true}".to_string(),
                content_parts: Vec::new(),
                tool_calls: None,
                tool_call_id: Some("call_1".to_string()),
            },
        ];
        let tools = vec![crate::llm::provider::Tool {
            name: "search".to_string(),
            description: "find things".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        }];

        let response = provider
            .send_chat_request(
                ChatRequest {
                    messages,
                    tools,
                    thinking: None,
                },
                false,
            )
            .await
            .unwrap();
        let payload: serde_json::Value = response.json().await.unwrap();
        assert_eq!(payload["choices"][0]["message"]["content"], "retried-ok");
        assert_eq!(hits.load(Ordering::SeqCst), 2);

        server.abort();
    }

    #[test]
    fn extract_usage_does_not_double_count_reasoning_tokens() {
        let usage = extract_usage(&serde_json::json!({
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "reasoning_tokens": 6
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.reasoning_tokens, 6);
        assert_eq!(usage.total_tokens, 30);
    }

    #[test]
    fn extract_usage_accepts_thinking_tokens_fallback() {
        let usage = extract_usage(&serde_json::json!({
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "thinking_tokens": 6
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.reasoning_tokens, 6);
        assert_eq!(usage.total_tokens, 30);
    }

    #[tokio::test]
    async fn send_chat_request_uses_reasoning_effort_field() {
        let app = Router::new().route(
            "/chat/completions",
            post(|Json(payload): Json<serde_json::Value>| async move {
                assert_eq!(payload["reasoning_effort"], "high");
                assert_eq!(payload["max_completion_tokens"], 6144);
                assert!(payload.get("reasoning").is_none());
                Json(serde_json::json!({
                    "choices": [{
                        "message": {
                            "content": "ok"
                        }
                    }]
                }))
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleProvider::new(
            "key".to_string(),
            format!("http://{addr}"),
            "model".to_string(),
        );

        provider
            .chat_completion(ChatRequest {
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: "hi".to_string(),
                    content_parts: Vec::new(),
                    tool_calls: None,
                    tool_call_id: None,
                }],
                tools: vec![],
                thinking: Some(ThinkingConfig {
                    enabled: true,
                    budget_tokens: Some(2048),
                    effort: "high".to_string(),
                }),
            })
            .await
            .unwrap();

        server.abort();
    }

    #[tokio::test]
    async fn send_chat_request_defaults_completion_budget_for_thinking() {
        let app = Router::new().route(
            "/chat/completions",
            post(|Json(payload): Json<serde_json::Value>| async move {
                assert_eq!(payload["reasoning_effort"], "medium");
                assert_eq!(payload["max_completion_tokens"], 5120);
                Json(serde_json::json!({
                    "choices": [{
                        "message": {
                            "content": "ok"
                        }
                    }]
                }))
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleProvider::new(
            "key".to_string(),
            format!("http://{addr}"),
            "model".to_string(),
        );

        provider
            .chat_completion(ChatRequest {
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: "hi".to_string(),
                    content_parts: Vec::new(),
                    tool_calls: None,
                    tool_call_id: None,
                }],
                tools: vec![],
                thinking: Some(ThinkingConfig {
                    enabled: true,
                    budget_tokens: None,
                    effort: "medium".to_string(),
                }),
            })
            .await
            .unwrap();

        server.abort();
    }

    #[tokio::test]
    async fn chat_completion_parses_text_and_tool_calls_from_response() {
        let app = Router::new().route(
            "/chat/completions",
            post(|| async {
                Json(serde_json::json!({
                    "choices": [{
                        "message": {
                            "content": "done",
                            "tool_calls": [{
                                "id": "call_1",
                                "function": {
                                    "name": "search",
                                    "arguments": "{\"q\":\"talon\"}"
                                }
                            }]
                        }
                    }]
                }))
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleProvider::new(
            "key".to_string(),
            format!("http://{addr}"),
            "model".to_string(),
        );
        let result = provider
            .chat_completion(ChatRequest {
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: "hi".to_string(),
                    content_parts: Vec::new(),
                    tool_calls: None,
                    tool_call_id: None,
                }],
                tools: vec![],
                thinking: None,
            })
            .await
            .unwrap();

        assert_eq!(result.content, "done");
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].id, "call_1");
        assert_eq!(result.tool_calls[0].name, "search");

        server.abort();
    }

    #[tokio::test]
    async fn send_chat_request_surfaces_non_retryable_error() {
        let app = Router::new().route(
            "/chat/completions",
            post(|| async {
                (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "message": "bad request"
                    })),
                )
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleProvider::new(
            "key".to_string(),
            format!("http://{addr}"),
            "model".to_string(),
        );
        let err = provider
            .send_chat_request(
                ChatRequest {
                    messages: vec![ChatMessage {
                        role: "user".to_string(),
                        content: "hi".to_string(),
                        content_parts: Vec::new(),
                        tool_calls: None,
                        tool_call_id: None,
                    }],
                    tools: vec![],
                    thinking: None,
                },
                false,
            )
            .await
            .unwrap_err();

        assert!(err.to_string().contains("OpenAI API error"));
        assert!(err.to_string().contains("bad request"));
        server.abort();
    }

    #[tokio::test]
    async fn send_chat_request_retries_without_stream_options_when_rejected() {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/chat/completions",
            post({
                let hits = hits.clone();
                move |Json(payload): Json<serde_json::Value>| {
                    let hits = hits.clone();
                    async move {
                        let hit = hits.fetch_add(1, Ordering::SeqCst);
                        if hit == 0 {
                            assert_eq!(
                                payload["stream_options"],
                                serde_json::json!({ "include_usage": true })
                            );
                            (
                                axum::http::StatusCode::BAD_REQUEST,
                                Json(serde_json::json!({
                                    "message": "unknown field stream_options"
                                })),
                            )
                        } else {
                            assert!(payload.get("stream_options").is_none());
                            (
                                axum::http::StatusCode::OK,
                                Json(serde_json::json!({
                                    "choices": [{
                                        "message": {
                                            "content": "retried-ok"
                                        }
                                    }]
                                })),
                            )
                        }
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleProvider::new(
            "key".to_string(),
            format!("http://{addr}"),
            "model".to_string(),
        );

        let response = provider
            .send_chat_request(
                ChatRequest {
                    messages: vec![ChatMessage {
                        role: "user".to_string(),
                        content: "hi".to_string(),
                        content_parts: Vec::new(),
                        tool_calls: None,
                        tool_call_id: None,
                    }],
                    tools: vec![],
                    thinking: None,
                },
                true,
            )
            .await
            .unwrap();

        let payload: serde_json::Value = response.json().await.unwrap();
        assert_eq!(payload["choices"][0]["message"]["content"], "retried-ok");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[tokio::test]
    async fn send_chat_request_surfaces_failed_novita_retry() {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route(
                "/novita.ai/chat/completions",
                post(
                    move |State(hits): State<Arc<AtomicUsize>>,
                          Json(payload): Json<serde_json::Value>| async move {
                        let hit = hits.fetch_add(1, Ordering::SeqCst);
                        if hit == 0 {
                            assert!(payload.get("tools").is_some());
                            (
                                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                Json(serde_json::json!({
                                    "message": "internal_server_error"
                                })),
                            )
                        } else {
                            assert!(payload.get("tools").is_none());
                            (
                                axum::http::StatusCode::BAD_GATEWAY,
                                Json(serde_json::json!({
                                    "message": "retry still failed"
                                })),
                            )
                        }
                    },
                ),
            )
            .with_state(hits.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleProvider::new(
            "key".to_string(),
            format!("http://{addr}/novita.ai"),
            "model".to_string(),
        );
        let err = provider
            .send_chat_request(
                ChatRequest {
                    messages: vec![
                        ChatMessage {
                            role: "assistant".to_string(),
                            content: "".to_string(),
                            content_parts: Vec::new(),
                            tool_calls: Some(vec![crate::llm::provider::ToolCall {
                                id: "call_1".to_string(),
                                name: "search".to_string(),
                                arguments: "{\"q\":\"x\"}".to_string(),
                            }]),
                            tool_call_id: None,
                        },
                        ChatMessage {
                            role: "tool".to_string(),
                            content: "{\"ok\":true}".to_string(),
                            content_parts: Vec::new(),
                            tool_calls: None,
                            tool_call_id: Some("call_1".to_string()),
                        },
                    ],
                    tools: vec![crate::llm::provider::Tool {
                        name: "search".to_string(),
                        description: "find things".to_string(),
                        input_schema: serde_json::json!({"type": "object"}),
                    }],
                    thinking: None,
                },
                false,
            )
            .await
            .unwrap_err();

        let text = err.to_string();
        assert!(text.contains("retry_without_tools"));
        assert!(text.contains("internal_server_error"));
        assert!(text.contains("retry still failed"));
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[tokio::test]
    async fn stream_chat_completion_emits_text_and_tool_call_deltas() {
        let payloads = Arc::new(std::sync::Mutex::new(Vec::new()));
        let app = Router::new().route(
            "/chat/completions",
            post({
                let payloads = payloads.clone();
                move |Json(payload): Json<serde_json::Value>| {
                    let payloads = payloads.clone();
                    async move {
                        payloads.lock().unwrap().push(payload);
                axum::response::Response::builder()
                    .status(axum::http::StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(axum::body::Body::from(
                        concat!(
                            "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
                            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"search\",\"arguments\":\"{\\\"q\\\":\\\"talon\\\"}\"}}]}}]}\n\n",
                            "data: [DONE]\n"
                        ),
                    ))
                    .unwrap()
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleProvider::new(
            "key".to_string(),
            format!("http://{addr}"),
            "model".to_string(),
        );
        let mut stream = provider
            .stream_chat_completion(ChatRequest {
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: "hi".to_string(),
                    content_parts: Vec::new(),
                    tool_calls: None,
                    tool_call_id: None,
                }],
                tools: vec![],
                thinking: None,
            })
            .await
            .unwrap();

        let first = stream.next().await.unwrap().unwrap();
        match first {
            ChatStreamEvent::TextDelta(text) => assert_eq!(text, "hello"),
            other => panic!("unexpected event: {other:?}"),
        }

        let second = stream.next().await.unwrap().unwrap();
        match second {
            ChatStreamEvent::ToolCallDelta(delta) => {
                assert_eq!(delta.index, 0);
                assert_eq!(delta.id.as_deref(), Some("call_1"));
                assert_eq!(delta.name.as_deref(), Some("search"));
                assert_eq!(delta.arguments.as_deref(), Some("{\"q\":\"talon\"}"));
            }
            other => panic!("unexpected event: {other:?}"),
        }

        assert!(stream.next().await.is_none());
        let recorded_payloads = payloads.lock().unwrap();
        assert_eq!(
            recorded_payloads[0]["stream_options"],
            serde_json::json!({ "include_usage": true })
        );
        server.abort();
    }

    #[tokio::test]
    async fn stream_chat_completion_surfaces_stream_errors() {
        let app = Router::new().route(
            "/chat/completions",
            post(|| async {
                axum::response::Response::builder()
                    .status(axum::http::StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(axum::body::Body::from("data: {not-json}\n\n"))
                    .unwrap()
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleProvider::new(
            "key".to_string(),
            format!("http://{addr}"),
            "model".to_string(),
        );
        let mut stream = provider
            .stream_chat_completion(ChatRequest {
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: "hi".to_string(),
                    content_parts: Vec::new(),
                    tool_calls: None,
                    tool_call_id: None,
                }],
                tools: vec![],
                thinking: None,
            })
            .await
            .unwrap();

        assert!(stream.next().await.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn completion_returns_chat_content() {
        let app = Router::new().route(
            "/chat/completions",
            post(|| async {
                Json(serde_json::json!({
                    "choices": [{
                        "message": {
                            "content": "plain completion"
                        }
                    }]
                }))
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleProvider::new(
            "key".to_string(),
            format!("http://{addr}"),
            "model".to_string(),
        );
        let text = provider.completion("hello").await.unwrap();
        assert_eq!(text, "plain completion");
        server.abort();
    }
}
