// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::llm::provider::{
    ChatMessage, ChatRequest, ChatResponse, ChatStream, ChatStreamEvent, ChatUsage, LlmProvider,
};
use crate::gateway::rpc::manifests;
use crate::memory::Embedding;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::{stream, StreamExt};
use serde_json::json;

const DEFAULT_MAX_TOKENS: u64 = 1024;
const DEFAULT_THINKING_BUDGET_TOKENS: u64 = 1024;
const MIN_THINKING_MAX_TOKENS: u64 = 4096;

pub struct AnthropicProvider {
    pub api_key: String,
    pub model: String,
    pub http_client: reqwest::Client,
    pub api_base_url: Option<String>,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            http_client: reqwest::Client::new(),
            api_base_url: None,
        }
    }

    fn messages_url(&self) -> String {
        self.api_base_url
            .as_deref()
            .map(|base| format!("{}/v1/messages", base.trim_end_matches('/')))
            .unwrap_or_else(|| "https://api.anthropic.com/v1/messages".to_string())
    }

    fn resolve_thinking_config(
        thinking: Option<&manifests::ThinkingConfig>,
    ) -> (u64, Option<serde_json::Value>) {
        let Some(thinking) = thinking.filter(|thinking| thinking.enabled) else {
            return (DEFAULT_MAX_TOKENS, None);
        };

        let budget_tokens = thinking
            .budget_tokens
            .map(u64::from)
            .unwrap_or(DEFAULT_THINKING_BUDGET_TOKENS);
        let max_tokens = MIN_THINKING_MAX_TOKENS.max(budget_tokens.saturating_add(1));
        (
            max_tokens,
            Some(json!({
                "type": "enabled",
                "budget_tokens": budget_tokens,
            })),
        )
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
        Err(anyhow!("Anthropic does not natively support embeddings. Use an OpenAI-compatible provider for embeddings."))
    }

    async fn chat_completion(
        &self,
        request: ChatRequest,
    ) -> Result<ChatResponse> {
        // TODO: Translate OpenAI-format tool definitions to Anthropic's tool schema
        // and include them in the payload when _tools is non-empty.
        let url = self.messages_url();
        let (max_tokens, thinking_payload) =
            Self::resolve_thinking_config(request.thinking.as_ref());

        let mut payload = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": request.messages.iter().map(|m| {
                json!({
                    "role": m.role,
                    "content": m.content
                })
            }).collect::<Vec<_>>(),
        });
        if let Some(thinking_payload) = thinking_payload {
            payload["thinking"] = thinking_payload;
        }

        let resp = self
            .http_client
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err_text = resp.text().await?;
            return Err(anyhow!("Anthropic API error: {}", err_text));
        }

        let result: serde_json::Value = resp.json().await?;
        let content = result["content"][0]["text"]
            .as_str()
            .ok_or_else(|| anyhow!("Invalid Anthropic response format"))?
            .to_string();

        Ok(ChatResponse {
            content,
            tool_calls: vec![],
            usage: extract_usage(&result),
        })
    }

    async fn stream_chat_completion(&self, request: ChatRequest) -> Result<ChatStream> {
        let url = self.messages_url();
        let (max_tokens, thinking_payload) =
            Self::resolve_thinking_config(request.thinking.as_ref());

        let mut payload = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": request.messages.iter().map(|m| {
                json!({
                    "role": m.role,
                    "content": m.content
                })
            }).collect::<Vec<_>>(),
            "stream": true,
        });
        if let Some(thinking_payload) = thinking_payload {
            payload["thinking"] = thinking_payload;
        }

        let resp = self
            .http_client
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err_text = resp.text().await?;
            return Err(anyhow!("Anthropic API error: {}", err_text));
        }

        let byte_stream = resp.bytes_stream();
        let line_stream = byte_stream.map(|item| item.map_err(|e| anyhow!("Stream error: {}", e)));

        let mut buffer = String::new();
        let mut last_event = String::new();
        let mut current_usage = ChatUsage::default();

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

                    if let Some(event) = line.strip_prefix("event: ") {
                        last_event = event.to_string();
                        continue;
                    }

                    if let Some(data) = line.strip_prefix("data: ") {
                        if last_event == "content_block_delta" {
                            if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
                                if let Some(text) =
                                    value.pointer("/delta/text").and_then(|v| v.as_str())
                                {
                                    items.push(Ok(ChatStreamEvent::TextDelta(text.to_string())));
                                }
                                if let Some(thinking) =
                                    value.pointer("/delta/thinking").and_then(|v| v.as_str())
                                {
                                    items.push(Ok(ChatStreamEvent::ReasoningDelta(
                                        thinking.to_string(),
                                    )));
                                }
                            }
                        } else if last_event == "message_start" || last_event == "message_delta" {
                            if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
                                if let Some(usage) = extract_usage(&value) {
                                    if usage.input_tokens > 0 {
                                        current_usage.input_tokens = usage.input_tokens;
                                    }
                                    if usage.output_tokens > 0 {
                                        current_usage.output_tokens = usage.output_tokens;
                                    }
                                    if usage.reasoning_tokens > 0 {
                                        current_usage.reasoning_tokens = usage.reasoning_tokens;
                                    }
                                    current_usage.total_tokens =
                                        current_usage.input_tokens + current_usage.output_tokens;
                                    items.push(Ok(ChatStreamEvent::Usage(current_usage.clone())));
                                }
                            }
                        } else if last_event == "message_stop" {
                            break;
                        }
                    }
                }
                stream::iter(items)
            }
            Err(e) => stream::iter(vec![Err(e)]),
        });

        Ok(Box::pin(sse_stream))
    }

    async fn completion(&self, prompt: &str) -> Result<String> {
        self.chat_completion(
            ChatRequest {
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: prompt.to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                }],
                tools: vec![],
                thinking: None,
            },
        )
        .await
        .map(|r| r.content)
    }
}

fn extract_usage(result: &serde_json::Value) -> Option<ChatUsage> {
    let usage = result
        .get("usage")
        .or_else(|| result.pointer("/message/usage"))?;
    let input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let reasoning_tokens = usage
        .get("thinking_tokens")
        .or_else(|| usage.get("reasoning_tokens"))
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
    use axum::{routing::post, Json, Router};
    use crate::gateway::rpc::manifests::ThinkingConfig;
    use serde_json::json;
    use tokio::net::TcpListener;

    #[test]
    fn resolve_thinking_config_defaults_budget_and_max_tokens() {
        let thinking = ThinkingConfig {
            enabled: true,
            budget_tokens: None,
            effort: String::new(),
        };

        let (max_tokens, payload) = AnthropicProvider::resolve_thinking_config(Some(&thinking));

        assert_eq!(max_tokens, MIN_THINKING_MAX_TOKENS);
        assert_eq!(
            payload,
            Some(json!({
                "type": "enabled",
                "budget_tokens": DEFAULT_THINKING_BUDGET_TOKENS,
            }))
        );
    }

    #[test]
    fn resolve_thinking_config_expands_max_tokens_for_large_budget() {
        let thinking = ThinkingConfig {
            enabled: true,
            budget_tokens: Some(5000),
            effort: String::new(),
        };

        let (max_tokens, payload) = AnthropicProvider::resolve_thinking_config(Some(&thinking));

        assert_eq!(max_tokens, 5001);
        assert_eq!(
            payload,
            Some(json!({
                "type": "enabled",
                "budget_tokens": 5000,
            }))
        );
    }

    fn test_provider(base_url: String) -> AnthropicProvider {
        AnthropicProvider {
            api_key: "test-key".to_string(),
            model: "claude-test".to_string(),
            http_client: reqwest::Client::new(),
            api_base_url: Some(base_url),
        }
    }

    #[tokio::test]
    async fn test_anthropic_sse_parsing() {
        let sse_data = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\nevent: message_stop\ndata: {}\n";

        let mut buffer = String::new();
        let mut last_event = String::new();
        let mut items = Vec::new();

        let text = sse_data;
        buffer.push_str(text);
        while let Some(pos) = buffer.find('\n') {
            let line = buffer.drain(..=pos).collect::<String>();
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if let Some(event) = line.strip_prefix("event: ") {
                last_event = event.to_string();
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                if last_event == "content_block_delta" {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(text) = value.pointer("/delta/text").and_then(|v| v.as_str()) {
                            items.push(text.to_string());
                        }
                    }
                } else if last_event == "message_stop" {
                    break;
                }
            }
        }

        assert_eq!(items, vec!["hi"]);
    }

    #[tokio::test]
    async fn anthropic_usage_events_accumulate_across_stream_messages() {
        let sse_data = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10}}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":4,\"thinking_tokens\":2}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":7,\"thinking_tokens\":3}}\n\n",
            "event: message_stop\n",
            "data: {}\n"
        );

        let mut buffer = String::new();
        let mut last_event = String::new();
        let mut current_usage = ChatUsage::default();
        let mut usage_events = Vec::new();

        buffer.push_str(sse_data);
        while let Some(pos) = buffer.find('\n') {
            let line = buffer.drain(..=pos).collect::<String>();
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if let Some(event) = line.strip_prefix("event: ") {
                last_event = event.to_string();
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                if last_event == "message_start" || last_event == "message_delta" {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(usage) = extract_usage(&value) {
                            if usage.input_tokens > 0 {
                                current_usage.input_tokens = usage.input_tokens;
                            }
                            if usage.output_tokens > 0 {
                                current_usage.output_tokens = usage.output_tokens;
                            }
                            if usage.reasoning_tokens > 0 {
                                current_usage.reasoning_tokens = usage.reasoning_tokens;
                            }
                            current_usage.total_tokens =
                                current_usage.input_tokens + current_usage.output_tokens;
                            usage_events.push(current_usage.clone());
                        }
                    }
                } else if last_event == "message_stop" {
                    break;
                }
            }
        }

        assert_eq!(
            usage_events,
            vec![
                ChatUsage {
                    input_tokens: 10,
                    output_tokens: 0,
                    reasoning_tokens: 0,
                    total_tokens: 10,
                },
                ChatUsage {
                    input_tokens: 10,
                    output_tokens: 4,
                    reasoning_tokens: 2,
                    total_tokens: 14,
                },
                ChatUsage {
                    input_tokens: 10,
                    output_tokens: 7,
                    reasoning_tokens: 3,
                    total_tokens: 17,
                },
            ]
        );
    }

    #[tokio::test]
    async fn chat_completion_handles_success_error_and_invalid_format() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/v1/messages",
            post(|Json(body): Json<serde_json::Value>| async move {
                let content = body["messages"][0]["content"].as_str().unwrap_or_default();
                if content == "cause-error" {
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        Json(json!({"error":"bad request"})),
                    );
                }
                if content == "bad-format" {
                    return (
                        axum::http::StatusCode::OK,
                        Json(json!({"content":[{"type":"text"}]})),
                    );
                }
                (
                    axum::http::StatusCode::OK,
                    Json(json!({"content":[{"text":"hello from anthropic"}]})),
                )
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = test_provider(format!("http://{}", addr));
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
        }];

        let response = provider
            .chat_completion(ChatRequest {
                messages: messages.clone(),
                tools: vec![],
                thinking: None,
            })
            .await
            .unwrap();
        assert_eq!(response.content, "hello from anthropic");
        assert!(response.tool_calls.is_empty());

        let api_err = provider
            .chat_completion(ChatRequest {
                messages: vec![ChatMessage {
                    content: "cause-error".to_string(),
                    ..messages[0].clone()
                }],
                tools: vec![],
                thinking: None,
            })
            .await
            .unwrap_err();
        assert!(api_err.to_string().contains("Anthropic API error"));

        let format_err = provider
            .chat_completion(ChatRequest {
                messages: vec![ChatMessage {
                    content: "bad-format".to_string(),
                    ..messages[0].clone()
                }],
                tools: vec![],
                thinking: None,
            })
            .await
            .unwrap_err();
        assert!(format_err
            .to_string()
            .contains("Invalid Anthropic response format"));
    }

    #[tokio::test]
    async fn stream_chat_completion_and_completion_cover_http_paths() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/v1/messages",
            post(|Json(body): Json<serde_json::Value>| async move {
                let content = body["messages"][0]["content"].as_str().unwrap_or_default();
                if body["stream"].as_bool() == Some(true) {
                    if content == "stream-error" {
                        return (
                            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            "stream failed".to_string(),
                        );
                    }
                    return (
                        axum::http::StatusCode::OK,
                        "event: content_block_delta\ndata: {\"delta\":{\"text\":\"hi\"}}\n\nevent: content_block_delta\ndata: {\"delta\":{\"text\":\" there\"}}\n\nevent: message_stop\ndata: {}\n".to_string(),
                    );
                }

                (
                    axum::http::StatusCode::OK,
                    "{\"content\":[{\"text\":\"completion reply\"}]}".to_string(),
                )
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = test_provider(format!("http://{}", addr));
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "stream".to_string(),
            tool_calls: None,
            tool_call_id: None,
        }];

        let mut stream = provider
            .stream_chat_completion(ChatRequest {
                messages: messages.clone(),
                tools: vec![],
                thinking: None,
            })
            .await
            .unwrap();
        let mut deltas = Vec::new();
        while let Some(event) = stream.next().await {
            match event.unwrap() {
                ChatStreamEvent::TextDelta(text) => deltas.push(text),
                other => panic!("unexpected stream event: {:?}", other),
            }
        }
        assert_eq!(deltas, vec!["hi".to_string(), " there".to_string()]);

        let completion = provider.completion("prompt").await.unwrap();
        assert_eq!(completion, "completion reply");

        let err = match provider
            .stream_chat_completion(ChatRequest {
                messages: vec![ChatMessage {
                    content: "stream-error".to_string(),
                    ..messages[0].clone()
                }],
                tools: vec![],
                thinking: None,
            })
            .await
        {
            Ok(_) => panic!("expected stream error"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("Anthropic API error"));
    }

    #[tokio::test]
    async fn generate_embedding_reports_unsupported_provider() {
        let provider = AnthropicProvider::new("key".to_string(), "model".to_string());
        let err = provider.generate_embedding("hello").await.unwrap_err();
        assert!(err
            .to_string()
            .contains("Anthropic does not natively support embeddings"));
    }
}
