// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::llm::provider::{
    ChatMessage, ChatResponse, ChatStream, ChatStreamEvent, LlmProvider, ToolCallDelta,
};
use crate::memory::Embedding;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::{stream, StreamExt};
use serde_json::Value;

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
            http_client: reqwest::Client::new(),
        }
    }

    fn serialize_messages(messages: Vec<ChatMessage>) -> Vec<serde_json::Value> {
        messages
            .into_iter()
            .map(|message| {
                let mut json = serde_json::json!({
                    "role": message.role,
                    "content": message.content,
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

    async fn send_chat_request(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<crate::llm::provider::Tool>,
        stream: bool,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let serialized_messages = Self::serialize_messages(messages.clone());

        let build_payload = |include_tools: bool| {
            let mut payload = serde_json::json!({
                "model": self.model,
                "messages": serialized_messages,
            });

            if stream {
                payload["stream"] = serde_json::json!(true);
            }

            if include_tools && !tools.is_empty() {
                let openai_tools: Vec<serde_json::Value> = tools
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

            payload
        };

        let initial_include_tools = !tools.is_empty();
        let initial_payload = build_payload(initial_include_tools);
        self.log_request_attempt(
            "initial",
            initial_include_tools,
            stream,
            &serialized_messages,
            &tools,
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
            &tools,
            &initial_payload,
            initial_status,
            &err_text,
        );
        if self.supports_tool_retry_without_tools(&messages, &err_text, initial_include_tools) {
            let retry_serialized_messages = serialized_messages.clone();
            let retry_payload = {
                let mut payload = serde_json::json!({
                    "model": self.model,
                    "messages": retry_serialized_messages,
                });
                if stream {
                    payload["stream"] = serde_json::json!(true);
                }
                payload
            };
            self.log_request_attempt(
                "retry_without_tools",
                false,
                stream,
                &retry_serialized_messages,
                &tools,
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
                &retry_serialized_messages,
                &tools,
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

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
        Ok(vec![0.0; 768])
    }

    async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<crate::llm::provider::Tool>,
    ) -> Result<ChatResponse> {
        use crate::llm::provider::ToolCall;

        let resp = self.send_chat_request(messages, tools, false).await?;

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
        })
    }

    async fn stream_chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<crate::llm::provider::Tool>,
    ) -> Result<ChatStream> {
        let resp = self.send_chat_request(messages, tools, true).await?;

        let byte_stream = resp.bytes_stream();
        let line_stream = byte_stream.map(|item| item.map_err(|e| anyhow!("Stream error: {}", e)));

        // Simple SSE state machine
        let mut buffer = String::new();
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
                        break;
                    }
                    if let Some(data) = line.strip_prefix("data: ") {
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(content) = value
                                .pointer("/choices/0/delta/content")
                                .and_then(|v| v.as_str())
                            {
                                items.push(Ok(ChatStreamEvent::TextDelta(content.to_string())));
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
            vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
                tool_calls: None,
                tool_call_id: None,
            }],
            vec![],
        )
        .await
        .map(|r| r.content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
