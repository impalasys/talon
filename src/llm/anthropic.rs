// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::llm::provider::{ChatMessage, ChatResponse, ChatStream, ChatStreamEvent, LlmProvider};
use crate::memory::Embedding;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::{stream, StreamExt};
use serde_json::json;

pub struct AnthropicProvider {
    pub api_key: String,
    pub model: String,
    pub http_client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            http_client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
        Err(anyhow!("Anthropic does not natively support embeddings. Use an OpenAI-compatible provider for embeddings."))
    }

    async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        _tools: Vec<crate::llm::provider::Tool>,
    ) -> Result<ChatResponse> {
        // TODO: Translate OpenAI-format tool definitions to Anthropic's tool schema
        // and include them in the payload when _tools is non-empty.
        let url = "https://api.anthropic.com/v1/messages";

        let payload = json!({
            "model": self.model,
            "max_tokens": 1024,
            "messages": messages.iter().map(|m| {
                json!({
                    "role": m.role,
                    "content": m.content
                })
            }).collect::<Vec<_>>(),
        });

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
        })
    }

    async fn stream_chat_completion(
        &self,
        _messages: Vec<ChatMessage>,
        _tools: Vec<crate::llm::provider::Tool>,
    ) -> Result<ChatStream> {
        let url = "https://api.anthropic.com/v1/messages";

        let payload = json!({
            "model": self.model,
            "max_tokens": 1024,
            "messages": _messages.iter().map(|m| {
                json!({
                    "role": m.role,
                    "content": m.content
                })
            }).collect::<Vec<_>>(),
            "stream": true,
        });

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
}
