// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::cas::CasStore;
use crate::harness::llm::provider::{
    chat_content_part, chat_message_text, chat_stream_event, encrypted_reasoning_event,
    object_ref_fallback_text, provider_request_error, reasoning_delta_event, text_delta_event,
    tool_call_delta_event, usage_event, ChatContentPart, ChatMessage, ChatRequest, ChatResponse,
    ChatStream, ChatStreamEvent, LlmProvider, TokenCounter, ToolCallDelta,
};
use crate::harness::memory::Embedding;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use futures::{stream, Stream, StreamExt};
use serde_json::Value;
use std::{
    collections::HashSet,
    pin::Pin,
    sync::OnceLock,
    task::{Context, Poll},
};

fn object_ref_text(object_ref: &crate::gateway::rpc::data_proto::ObjectRef) -> serde_json::Value {
    serde_json::json!({
        "type": "text",
        "text": object_ref_fallback_text(object_ref),
    })
}

async fn openai_content_part(cas: &CasStore, part: &ChatContentPart) -> Result<serde_json::Value> {
    Ok(match part.content.as_ref() {
        Some(chat_content_part::Content::Text(text)) => serde_json::json!({
            "type": "text",
            "text": text,
        }),
        Some(chat_content_part::Content::ObjectRef(object_ref)) => {
            let object_ref_media_type = object_ref.media_type.trim();
            if !object_ref_media_type.is_empty()
                && !object_ref_media_type
                    .to_ascii_lowercase()
                    .starts_with("image/")
            {
                return Ok(object_ref_text(object_ref));
            }
            let Some(stored) = cas.get_object_decoded(&object_ref.key).await? else {
                return Ok(serde_json::json!({
                    "type": "text",
                    "text": format!("[Image object '{}' is missing.]", object_ref.key),
                }));
            };
            let media_type = if object_ref_media_type.is_empty() {
                stored.metadata.media_type.trim()
            } else {
                object_ref_media_type
            };
            if !media_type.to_ascii_lowercase().starts_with("image/") {
                return Ok(object_ref_text(object_ref));
            }
            serde_json::json!({
                "type": "image_url",
                "image_url": {
                    "url": format!(
                        "data:{};base64,{}",
                        media_type,
                        general_purpose::STANDARD.encode(stored.bytes)
                    ),
                },
            })
        }
        None => serde_json::json!({"type": "text", "text": ""}),
    })
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
    pub api: String,
    pub http_client: reqwest::Client,
    cas: CasStore,
}

impl OpenAiCompatibleProvider {
    pub fn new(api_key: String, base_url: String, model: String, cas: CasStore) -> Self {
        Self::with_api(api_key, base_url, model, cas, "chat_completions")
    }

    pub fn with_api(
        api_key: String,
        base_url: String,
        model: String,
        cas: CasStore,
        api: impl Into<String>,
    ) -> Self {
        Self {
            api_key,
            base_url,
            model,
            api: api.into(),
            http_client: shared_http_client(),
            cas,
        }
    }

    fn uses_responses_api(&self) -> bool {
        self.api.trim().eq_ignore_ascii_case("responses")
    }

    async fn serialize_content_parts(
        &self,
        parts: &[ChatContentPart],
    ) -> Result<Vec<serde_json::Value>> {
        let mut serialized = Vec::with_capacity(parts.len());
        for part in parts {
            serialized.push(openai_content_part(&self.cas, part).await?);
        }
        Ok(serialized)
    }

    fn tool_message_content(parts: &[ChatContentPart]) -> String {
        parts
            .iter()
            .filter_map(|part| match part.content.as_ref()? {
                chat_content_part::Content::Text(text) => Some(text.clone()),
                chat_content_part::Content::ObjectRef(object_ref) => {
                    Some(object_ref_fallback_text(object_ref))
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    async fn serialize_tool_result_media_message(
        &self,
        tool_call_id: Option<&str>,
        parts: &[ChatContentPart],
    ) -> Result<Option<serde_json::Value>> {
        let mut media_parts = Vec::new();
        for part in parts {
            if !matches!(
                part.content.as_ref(),
                Some(chat_content_part::Content::ObjectRef(_))
            ) {
                continue;
            }
            let serialized = openai_content_part(&self.cas, part).await?;
            if serialized.get("type").and_then(Value::as_str) == Some("image_url") {
                media_parts.push(serialized);
            }
        }
        if media_parts.is_empty() {
            return Ok(None);
        }

        let label = tool_call_id
            .map(|id| format!("Image result returned by tool call {id}."))
            .unwrap_or_else(|| "Image result returned by a tool call.".to_string());
        let mut content = vec![serde_json::json!({
            "type": "text",
            "text": label,
        })];
        content.extend(media_parts);
        Ok(Some(serde_json::json!({
            "role": "user",
            "content": content,
        })))
    }

    fn responses_input_content_part(part: &Value) -> Value {
        if part.get("type").and_then(Value::as_str) == Some("image_url") {
            serde_json::json!({
                "type": "input_image",
                "image_url": part.pointer("/image_url/url").cloned().unwrap_or(Value::Null),
            })
        } else {
            serde_json::json!({
                "type": "input_text",
                "text": part
                    .get("text")
                    .cloned()
                    .unwrap_or_else(|| Value::String(String::new())),
            })
        }
    }

    fn responses_message_content_part(part: &Value, role: &str) -> Value {
        if role == "assistant" {
            serde_json::json!({
                "type": "output_text",
                "text": part
                    .get("text")
                    .cloned()
                    .unwrap_or_else(|| Value::String(String::new())),
            })
        } else {
            Self::responses_input_content_part(part)
        }
    }

    async fn serialize_messages(
        &self,
        messages: Vec<ChatMessage>,
    ) -> Result<Vec<serde_json::Value>> {
        let mut serialized = Vec::with_capacity(messages.len());
        let mut pending_tool_media_messages = Vec::new();
        for message in messages {
            if message.role == "tool" {
                let tool_call_id = message.tool_call_id.as_deref();
                let mut json = serde_json::json!({
                    "role": "tool",
                    "content": Self::tool_message_content(&message.content_parts),
                });
                if let Some(tool_call_id) = tool_call_id {
                    json["tool_call_id"] = serde_json::json!(tool_call_id);
                }
                let media_message = self
                    .serialize_tool_result_media_message(tool_call_id, &message.content_parts)
                    .await?;
                serialized.push(json);
                if let Some(media_message) = media_message {
                    pending_tool_media_messages.push(media_message);
                }
                continue;
            }

            serialized.append(&mut pending_tool_media_messages);
            let content = match message.content_parts.as_slice() {
                [] => serde_json::Value::String(String::new()),
                [part] => match part.content.as_ref() {
                    Some(chat_content_part::Content::Text(text)) => {
                        serde_json::Value::String(text.clone())
                    }
                    _ => serde_json::Value::Array(
                        self.serialize_content_parts(&message.content_parts).await?,
                    ),
                },
                _ => serde_json::Value::Array(
                    self.serialize_content_parts(&message.content_parts).await?,
                ),
            };
            let mut json = serde_json::json!({
                "role": message.role,
                "content": content,
            });

            if !message.tool_calls.is_empty() {
                let openai_tool_calls: Vec<serde_json::Value> = message
                    .tool_calls
                    .into_iter()
                    .map(|tool| {
                        let arguments = Self::openai_tool_arguments(&tool.arguments);
                        serde_json::json!({
                            "id": tool.id,
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "arguments": arguments,
                            }
                        })
                    })
                    .collect();
                json["tool_calls"] = serde_json::json!(openai_tool_calls);
            }

            if let Some(tool_call_id) = message.tool_call_id {
                json["tool_call_id"] = serde_json::json!(tool_call_id);
            }

            serialized.push(json);
        }
        serialized.append(&mut pending_tool_media_messages);
        Ok(serialized)
    }

    fn openai_tool_arguments(arguments: &str) -> String {
        match serde_json::from_str::<Value>(arguments) {
            Ok(Value::Object(_)) => arguments.to_string(),
            _ => "{}".to_string(),
        }
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
            && messages
                .iter()
                .any(|m| m.role == "tool" || !m.tool_calls.is_empty())
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

    fn redact_data_urls(value: &Value) -> Value {
        match value {
            Value::String(text) if text.starts_with("data:") => {
                Value::String("<redacted-data-url>".to_string())
            }
            Value::Array(items) => Value::Array(items.iter().map(Self::redact_data_urls).collect()),
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(key, value)| (key.clone(), Self::redact_data_urls(value)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    fn payload_json_for_debug(payload: &Value) -> String {
        serde_json::to_string(&Self::redact_data_urls(payload)).unwrap_or_default()
    }

    fn sanitize_tool_schema_for_openai(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.retain(|_, child| !child.is_null());
                for child in map.values_mut() {
                    Self::sanitize_tool_schema_for_openai(child);
                }
            }
            Value::Array(items) => {
                for child in items {
                    Self::sanitize_tool_schema_for_openai(child);
                }
            }
            _ => {}
        }
    }

    fn openai_tool_parameters(schema_json: &str) -> Value {
        let mut schema = serde_json::from_str::<Value>(schema_json)
            .unwrap_or_else(|_| serde_json::json!({ "type": "object" }));
        Self::sanitize_tool_schema_for_openai(&mut schema);
        if schema.get("type").and_then(Value::as_str) == Some("object") {
            schema
        } else {
            serde_json::json!({ "type": "object" })
        }
    }

    fn compute_request_debug_stats(
        serialized_messages: &[Value],
        tools: &[crate::harness::llm::provider::Tool],
        payload: &Value,
    ) -> RequestDebugStats {
        let message_chars = serialized_messages
            .iter()
            .map(|message| serde_json::to_string(message).unwrap_or_default().len())
            .sum::<usize>();
        let tool_schema_chars = tools
            .iter()
            .map(|tool| tool.input_schema_json.len() + tool.name.len() + tool.description.len())
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
        tools: &[crate::harness::llm::provider::Tool],
        payload: &Value,
    ) {
        let stats = Self::compute_request_debug_stats(serialized_messages, tools, payload);
        let debug_requests = Self::debug_requests_enabled();
        let payload_json = if debug_requests {
            Self::payload_json_for_debug(payload)
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
        tools: &[crate::harness::llm::provider::Tool],
        payload: &Value,
        status: reqwest::StatusCode,
        err_text: &str,
    ) {
        let stats = Self::compute_request_debug_stats(serialized_messages, tools, payload);
        let debug_requests = Self::debug_requests_enabled();
        let payload_json = if debug_requests {
            Self::payload_json_for_debug(payload)
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
        if request.zero_data_retention {
            return Err(anyhow!(
                "zeroDataRetention requires the OpenAI Responses API, not Chat Completions"
            ));
        }
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let serialized_messages = self.serialize_messages(request.messages.clone()).await?;

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
                                "parameters": Self::openai_tool_parameters(&tool.input_schema_json)
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
                return Err(openai_api_error(
                    "OpenAI-compatible API request failed after retries",
                    &retry_without_tools_err_text,
                ));
            }

            return Err(openai_api_error(
                "OpenAI-compatible API request failed after retry",
                &retry_err_text,
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
            return Err(openai_api_error(
                "OpenAI-compatible API request failed after retry",
                &retry_err_text,
            ));
        }

        Err(openai_api_error(
            "OpenAI-compatible API request failed",
            &err_text,
        ))
    }

    async fn serialize_responses_input(
        &self,
        messages: Vec<ChatMessage>,
        previous_response_id: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        let messages = if previous_response_id.is_some() {
            let suffix_start = messages
                .iter()
                .rposition(|message| message.role == "assistant")
                .map(|index| index.saturating_add(1))
                .unwrap_or(0);
            messages
                .into_iter()
                .enumerate()
                .filter(|(index, message)| {
                    message.role == "system"
                        || message.role == "developer"
                        || *index >= suffix_start
                })
                .map(|(_, message)| message)
                .collect()
        } else {
            messages
        };
        let mut input = Vec::new();
        for message in messages {
            if message.role == "tool" {
                let tool_call_id = message.tool_call_id.as_deref();
                let media_message = self
                    .serialize_tool_result_media_message(tool_call_id, &message.content_parts)
                    .await?;
                let output = if let Some(media_message) = media_message {
                    let content = media_message
                        .get("content")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .map(Self::responses_input_content_part)
                        .collect::<Vec<_>>();
                    Value::Array(content)
                } else {
                    Value::String(Self::tool_message_content(&message.content_parts))
                };
                input.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": tool_call_id.unwrap_or_default(),
                    "output": output,
                }));
                continue;
            }

            let mut content = Vec::new();
            let mut has_regular_content = false;
            for part in &message.content_parts {
                has_regular_content = true;
                let part = openai_content_part(&self.cas, part).await?;
                content.push(Self::responses_message_content_part(&part, &message.role));
            }

            if !content.is_empty() || (message.tool_calls.is_empty() && has_regular_content) {
                input.push(serde_json::json!({
                    "role": message.role,
                    "content": content,
                }));
            }

            for tool_call in message.tool_calls {
                input.push(serde_json::json!({
                    "type": "function_call",
                    "call_id": tool_call.id,
                    "name": tool_call.name,
                    "arguments": Self::openai_tool_arguments(&tool_call.arguments),
                }));
            }
        }
        Ok(input)
    }

    async fn send_responses_request(
        &self,
        request: ChatRequest,
        stream: bool,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/responses", self.base_url.trim_end_matches('/'));
        let messages = request.messages;
        let previous_response_id = (!request.zero_data_retention)
            .then(|| request.previous_response_id.clone())
            .flatten();
        let build_payload = |input: Vec<serde_json::Value>, previous_response_id: Option<&str>| {
            let mut payload = serde_json::json!({
                "model": self.model,
                "input": input,
                "stream": stream,
            });
            if let Some(previous_response_id) = previous_response_id {
                payload["previous_response_id"] = serde_json::json!(previous_response_id);
            }
            if request.zero_data_retention {
                payload["store"] = serde_json::json!(false);
                payload["include"] = serde_json::json!(["reasoning.encrypted_content"]);
            }
            if !request.tools.is_empty() {
                payload["tools"] = serde_json::json!(request
                    .tools
                    .iter()
                    .map(|tool| serde_json::json!({
                        "type": "function",
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": Self::openai_tool_parameters(&tool.input_schema_json),
                        "strict": false,
                    }))
                    .collect::<Vec<_>>());
            }
            if let Some(thinking) = request
                .thinking
                .as_ref()
                .filter(|thinking| thinking.enabled)
            {
                let mut reasoning = serde_json::json!({"summary": "auto"});
                if !thinking.effort.trim().is_empty() {
                    reasoning["effort"] = serde_json::json!(thinking.effort);
                }
                payload["reasoning"] = reasoning;
            }
            payload
        };
        let input = self
            .serialize_responses_input(messages.clone(), previous_response_id.as_deref())
            .await?;
        let mut payload = build_payload(input, previous_response_id.as_deref());

        let stats_messages = payload
            .get("input")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        self.log_request_attempt(
            "responses",
            !request.tools.is_empty(),
            stream,
            &stats_messages,
            &request.tools,
            &payload,
        );
        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&payload)
            .send()
            .await?;
        if response.status().is_success() {
            Ok(response)
        } else {
            let status = response.status();
            let error = response.text().await?;
            self.log_request_failure(
                "responses",
                !request.tools.is_empty(),
                stream,
                &stats_messages,
                &request.tools,
                &payload,
                status,
                &error,
            );
            if previous_response_id.is_some()
                && (is_stale_previous_response_id(status, &error)
                    || is_missing_tool_output_for_previous_response(status, &error))
            {
                let recovery_reason = if is_stale_previous_response_id(status, &error) {
                    "stale previous_response_id"
                } else {
                    "missing function-call output for previous_response_id"
                };
                tracing::warn!(
                    model = %self.model,
                    reason = recovery_reason,
                    "Retrying Responses request without previous_response_id"
                );
                let full_input = self.serialize_responses_input(messages, None).await?;
                payload = build_payload(full_input, None);
                let retry_response = self
                    .http_client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(&payload)
                    .send()
                    .await?;
                if retry_response.status().is_success() {
                    return Ok(retry_response);
                }
                let retry_status = retry_response.status();
                let retry_error = retry_response.text().await?;
                return Err(openai_api_error(
                    "OpenAI Responses API error after previous_response_id recovery retry",
                    &retry_error,
                ))
                .map_err(|error| {
                    tracing::error!(%retry_status, error = %error, "Responses retry failed");
                    error
                });
            }
            Err(openai_api_error("OpenAI Responses API error", &error))
        }
    }
}

fn shared_http_client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

fn openai_api_error(message: &str, response_body: &str) -> anyhow::Error {
    let response = serde_json::from_str::<Value>(response_body).ok();
    let token_counter = response.as_ref().and_then(extract_usage);
    let message = response
        .as_ref()
        .and_then(|response| openai_error_message(message, response))
        .unwrap_or_else(|| message.to_string());
    provider_request_error(message, token_counter)
}

fn openai_error_message(prefix: &str, response: &Value) -> Option<String> {
    format_openai_error_message(prefix, response.get("error")?)
}

fn format_openai_error_message(prefix: &str, error: &Value) -> Option<String> {
    let error = error.as_object()?;
    let field = |name: &str| {
        error
            .get(name)
            .map(|value| match value {
                Value::String(value) => value.clone(),
                Value::Null => "null".to_string(),
                value => value.to_string(),
            })
            .unwrap_or_else(|| "null".to_string())
    };

    Some(format!(
        "{prefix} (code: {}; type: {}; message: {})",
        field("code"),
        field("type"),
        field("message"),
    ))
}

fn openai_responses_stream_error(event_type: &str, value: &Value) -> anyhow::Error {
    let response = value.get("response").unwrap_or(value);
    let error = response
        .get("error")
        .filter(|error| !error.is_null())
        .or_else(|| value.get("error").filter(|error| !error.is_null()))
        .or_else(|| {
            (value.get("code").is_some() || value.get("message").is_some()).then_some(value)
        });
    let message = error
        .and_then(|error| format_openai_error_message("OpenAI Responses stream error", error))
        .unwrap_or_else(|| {
            let incomplete_reason = response
                .pointer("/incomplete_details/reason")
                .and_then(Value::as_str)
                .unwrap_or("null");
            format!(
                "OpenAI Responses stream error (code: {incomplete_reason}; type: {event_type}; message: null)"
            )
        });
    let usage = extract_responses_usage(response).or_else(|| extract_responses_usage(value));
    provider_request_error(message, usage)
}

fn is_stale_previous_response_id(status: reqwest::StatusCode, body: &str) -> bool {
    if !matches!(
        status,
        reqwest::StatusCode::BAD_REQUEST | reqwest::StatusCode::NOT_FOUND
    ) {
        return false;
    }
    let body = body.to_ascii_lowercase();
    (body.contains("previous_response_id") || body.contains("previous response"))
        && (body.contains("invalid")
            || body.contains("expired")
            || body.contains("not found")
            || body.contains("does not exist"))
}

/// A cancelled or crashed tool turn can leave an OpenAI server-side response
/// waiting for a function-call output.  The local transcript deliberately
/// drops that incomplete interaction during recovery, so retrying from the
/// complete local history is safe.  Keep this deliberately narrow: other 400s
/// are request errors, not continuation-recovery signals.
fn is_missing_tool_output_for_previous_response(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::BAD_REQUEST
        && body
            .to_ascii_lowercase()
            .contains("no tool output found for function call")
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
        use crate::harness::llm::provider::ToolCall;

        if self.uses_responses_api() {
            if request.zero_data_retention {
                return Err(anyhow!(
                    "zeroDataRetention requires streamed OpenAI Responses execution"
                ));
            }
            let resp = self.send_responses_request(request, false).await?;
            let result: serde_json::Value = resp.json().await?;
            return parse_responses_response(&result);
        }

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

        let usage = extract_usage(&result).map(|mut counter| {
            counter.provider = "openai_compatible".to_string();
            if counter.model.is_empty() {
                counter.model = self.model.clone();
            }
            counter
        });

        Ok(ChatResponse {
            content: content.clone(),
            tool_calls,
            usage,
            encrypted_reasoning: None,
        })
    }

    #[tracing::instrument(
        name = "OpenAiCompatibleProvider.stream_chat_completion",
        skip_all,
        fields(provider_base_url = %self.base_url, model = %self.model)
    )]
    async fn stream_chat_completion(&self, request: ChatRequest) -> Result<ChatStream> {
        if self.uses_responses_api() {
            let resp = self.send_responses_request(request, true).await?;
            return Ok(parse_responses_stream(resp, tracing::Span::current()));
        }

        let resp = self.send_chat_request(request, true).await?;

        let byte_stream = resp.bytes_stream();
        let line_stream = byte_stream.map(|item| item.map_err(|e| anyhow!("Stream error: {}", e)));
        let parent_span = tracing::Span::current();
        let parse_span = parent_span.clone();

        // Simple SSE state machine
        let mut buffer = String::new();
        let mut saw_first_chunk = false;
        let stream_model = self.model.clone();
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
                                items.push(Ok(text_delta_event(content.to_string())));
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
                                items.push(Ok(ChatStreamEvent {
                                    event: Some(chat_stream_event::Event::ReasoningDelta(
                                        reasoning.to_string(),
                                    )),
                                }));
                            }
                            if let Some(tool_calls) = value
                                .pointer("/choices/0/delta/tool_calls")
                                .and_then(|v| v.as_array())
                            {
                                for call in tool_calls {
                                    let delta = ToolCallDelta {
                                        index: call["index"].as_u64().unwrap_or(0) as u32,
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
                                        items.push(Ok(tool_call_delta_event(delta)));
                                    }
                                }
                            }
                            if let Some(mut usage) = extract_usage(&value) {
                                usage.provider = "openai_compatible".to_string();
                                if usage.model.is_empty() {
                                    usage.model = stream_model.clone();
                                }
                                items.push(Ok(usage_event(usage)));
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
            messages: vec![chat_message_text("user", prompt)],
            tools: vec![],
            thinking: None,
            previous_response_id: None,
            zero_data_retention: false,
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

fn parse_responses_response(value: &Value) -> Result<ChatResponse> {
    use crate::harness::llm::provider::ToolCall;

    let mut content = String::new();
    let mut tool_calls = Vec::new();
    for item in value
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                for part in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        content.push_str(text);
                    }
                }
            }
            Some("function_call") => {
                let id = item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
                if !id.is_empty() && !name.is_empty() {
                    tool_calls.push(ToolCall {
                        id: id.to_string(),
                        name: name.to_string(),
                        arguments: item
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}")
                            .to_string(),
                    });
                }
            }
            _ => {}
        }
    }

    Ok(ChatResponse {
        content,
        tool_calls,
        usage: extract_responses_usage(value),
        encrypted_reasoning: None,
    })
}

fn encrypted_reasoning_items(value: &Value) -> Vec<String> {
    value
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("reasoning"))
        .filter(|item| {
            item.get("encrypted_content")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty())
        })
        .filter_map(|item| serde_json::to_string(item).ok())
        .collect()
}

fn extract_responses_usage(value: &Value) -> Option<TokenCounter> {
    let usage = value.get("usage").cloned().unwrap_or(Value::Null);
    let input_tokens = usage
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let output_tokens_total = usage
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let reasoning_tokens = usage
        .pointer("/output_tokens_details/reasoning_tokens")
        .or_else(|| usage.get("reasoning_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let total_tokens = usage
        .get("total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(input_tokens + output_tokens_total);
    let provider_request_id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(ToString::to_string);
    if provider_request_id.is_none()
        && (input_tokens == 0 && output_tokens_total == 0 && total_tokens == 0)
    {
        return None;
    }
    Some(TokenCounter {
        input_tokens,
        output_tokens: output_tokens_total.saturating_sub(reasoning_tokens),
        reasoning_output_tokens: reasoning_tokens,
        total_tokens,
        cached_input_tokens: usage
            .pointer("/input_tokens_details/cached_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        cache_write_tokens: usage
            .pointer("/input_tokens_details/cache_write_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        usage_available: value.get("usage").is_some()
            && (input_tokens > 0 || output_tokens_total > 0 || total_tokens > 0),
        provider_request_id,
        provider: String::new(),
        model: value
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn parse_responses_stream(response: reqwest::Response, parent_span: tracing::Span) -> ChatStream {
    let byte_stream = response.bytes_stream();
    let line_stream = byte_stream.map(|item| item.map_err(|e| anyhow!("Stream error: {}", e)));
    let parse_span = parent_span.clone();
    let mut buffer = String::new();
    let mut event_name = String::new();
    let mut text_delta_parts = HashSet::new();
    let sse_stream = line_stream.flat_map(move |result| match result {
        Ok(bytes) => {
            buffer.push_str(&String::from_utf8_lossy(&bytes));
            let mut items = Vec::new();
            while let Some(pos) = buffer.find('\n') {
                let line = buffer.drain(..=pos).collect::<String>();
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some(name) = line.strip_prefix("event: ") {
                    event_name = name.to_string();
                    continue;
                }
                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                if data == "[DONE]" {
                    break;
                }
                let Ok(value) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                let event_type = if event_name.is_empty() {
                    value
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                } else {
                    event_name.as_str()
                };
                match event_type {
                    "response.created" => {
                        if let Some(response) = value.get("response") {
                            if let Some(usage) = extract_responses_usage(response) {
                                items.push(Ok(usage_event(usage)));
                            }
                        } else if let Some(usage) = extract_responses_usage(&value) {
                            items.push(Ok(usage_event(usage)));
                        }
                    }
                    "response.output_text.delta" => {
                        if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                            text_delta_parts.insert((
                                value
                                    .get("output_index")
                                    .and_then(Value::as_u64)
                                    .unwrap_or_default(),
                                value
                                    .get("content_index")
                                    .and_then(Value::as_u64)
                                    .unwrap_or_default(),
                            ));
                            items.push(Ok(text_delta_event(delta.to_string())));
                        }
                    }
                    "response.output_text.done" => {
                        let part = (
                            value
                                .get("output_index")
                                .and_then(Value::as_u64)
                                .unwrap_or_default(),
                            value
                                .get("content_index")
                                .and_then(Value::as_u64)
                                .unwrap_or_default(),
                        );
                        if !text_delta_parts.contains(&part) {
                            if let Some(text) = value.get("text").and_then(Value::as_str) {
                                items.push(Ok(text_delta_event(text.to_string())));
                            }
                        }
                    }
                    "response.reasoning_summary_text.delta"
                    | "response.reasoning_summary_part.delta" => {
                        if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                            items.push(Ok(reasoning_delta_event(delta.to_string())));
                        }
                    }
                    "response.function_call_arguments.delta" => {
                        let delta = ToolCallDelta {
                            index: value
                                .get("output_index")
                                .and_then(Value::as_u64)
                                .unwrap_or_default() as u32,
                            id: value
                                .get("call_id")
                                .and_then(Value::as_str)
                                .map(ToString::to_string),
                            name: value
                                .get("name")
                                .and_then(Value::as_str)
                                .map(ToString::to_string),
                            arguments: value
                                .get("delta")
                                .and_then(Value::as_str)
                                .map(ToString::to_string),
                        };
                        if delta.id.is_some() || delta.name.is_some() || delta.arguments.is_some() {
                            items.push(Ok(tool_call_delta_event(delta)));
                        }
                    }
                    "response.output_item.added" => {
                        let Some(item) = value.get("item") else {
                            continue;
                        };
                        if item.get("type").and_then(Value::as_str) == Some("function_call") {
                            items.push(Ok(tool_call_delta_event(ToolCallDelta {
                                index: value
                                    .get("output_index")
                                    .and_then(Value::as_u64)
                                    .unwrap_or_default()
                                    as u32,
                                id: item
                                    .get("call_id")
                                    .and_then(Value::as_str)
                                    .map(ToString::to_string),
                                name: item
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .map(ToString::to_string),
                                arguments: None,
                            })));
                        }
                    }
                    "response.output_item.done" => {}
                    "response.completed" => {
                        if let Some(response) = value.get("response") {
                            if let Some(usage) = extract_responses_usage(response) {
                                items.push(Ok(usage_event(usage)));
                            }
                            for encrypted_reasoning in encrypted_reasoning_items(response) {
                                items.push(Ok(encrypted_reasoning_event(encrypted_reasoning)));
                            }
                        } else {
                            if let Some(usage) = extract_responses_usage(&value) {
                                items.push(Ok(usage_event(usage)));
                            }
                            for encrypted_reasoning in encrypted_reasoning_items(&value) {
                                items.push(Ok(encrypted_reasoning_event(encrypted_reasoning)));
                            }
                        }
                    }
                    "error" | "response.failed" | "response.incomplete" => {
                        items.push(Err(openai_responses_stream_error(event_type, &value)));
                    }
                    _ => {}
                }
                event_name.clear();
            }
            stream::iter(items)
        }
        Err(error) => stream::iter(vec![Err(error)]),
    });

    Box::pin(SpanInstrumentedChatStream {
        inner: Box::pin(sse_stream),
        span: parse_span,
    })
}

fn extract_usage(value: &serde_json::Value) -> Option<TokenCounter> {
    let usage = value.get("usage")?;
    if !usage.is_object() {
        return None;
    }
    let has_usage_fields = usage.get("prompt_tokens").is_some()
        || usage.get("input_tokens").is_some()
        || usage.get("completion_tokens").is_some()
        || usage.get("output_tokens").is_some()
        || usage.get("total_tokens").is_some()
        || usage.get("reasoning_tokens").is_some()
        || usage.get("thinking_tokens").is_some()
        || usage
            .pointer("/completion_tokens_details/reasoning_tokens")
            .is_some();
    if !has_usage_fields {
        return None;
    }
    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let completion_tokens = usage
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
    let output_tokens = completion_tokens.saturating_sub(reasoning_tokens);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cached_input_tokens = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_write_tokens = usage
        .pointer("/prompt_tokens_details/cache_write_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cache_write_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    Some(TokenCounter {
        input_tokens,
        output_tokens,
        reasoning_output_tokens: reasoning_tokens,
        total_tokens,
        cached_input_tokens,
        cache_write_tokens,
        usage_available: true,
        provider_request_id: value
            .get("id")
            .and_then(|v| v.as_str())
            .map(ToString::to_string),
        provider: String::new(),
        model: value
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::object_store::{InMemoryObjectStore, ObjectMetadata, ObjectStore};
    use crate::gateway::rpc::manifests::ThinkingConfig;
    use crate::harness::llm::{object_ref_part, text_part};
    use axum::{extract::State, routing::post, Json, Router};
    use std::{
        net::SocketAddr,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };
    use tokio::net::TcpListener;

    fn assistant_tool_call_message(name: &str, arguments: &str) -> ChatMessage {
        ChatMessage {
            role: "assistant".to_string(),
            content_parts: Vec::new(),
            tool_calls: vec![crate::harness::llm::provider::ToolCall {
                id: "call_1".to_string(),
                name: name.to_string(),
                arguments: arguments.to_string(),
            }],
            tool_call_id: None,
        }
    }

    fn tool_result_message(content: &str) -> ChatMessage {
        ChatMessage {
            role: "tool".to_string(),
            content_parts: vec![text_part(content.to_string())],
            tool_calls: Vec::new(),
            tool_call_id: Some("call_1".to_string()),
        }
    }

    fn test_cas_store() -> CasStore {
        CasStore::new(Arc::new(InMemoryObjectStore::default()))
    }

    fn test_provider() -> OpenAiCompatibleProvider {
        OpenAiCompatibleProvider::new(
            "test-key".to_string(),
            "http://localhost".to_string(),
            "test-model".to_string(),
            test_cas_store(),
        )
    }

    #[test]
    fn request_debug_stats_measure_payload_and_schemas() {
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": "hello"
        })];
        let tools = vec![crate::harness::llm::provider::Tool {
            name: "search".to_string(),
            description: "find things".to_string(),
            input_schema_json: serde_json::json!({
                "type": "object",
                "properties": {
                    "q": {"type": "string"}
                }
            })
            .to_string(),
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

    #[tokio::test]
    async fn serialize_messages_preserves_tool_protocol_fields() {
        let messages = vec![
            assistant_tool_call_message(
                "mcp_conic_execute_blog_post_publish",
                "{\"blogPostId\":\"page_1\"}",
            ),
            tool_result_message("{\"url\":\"https://github.com/example/repo/pull/2\"}"),
        ];

        let serialized = test_provider().serialize_messages(messages).await.unwrap();

        assert_eq!(
            serialized[0]["tool_calls"][0]["function"]["name"],
            "mcp_conic_execute_blog_post_publish"
        );
        assert_eq!(serialized[1]["tool_call_id"], "call_1");
    }

    #[tokio::test]
    async fn serialize_messages_normalizes_invalid_tool_arguments() {
        let messages = vec![assistant_tool_call_message("mcp_conic_list_links", "")];

        let serialized = test_provider().serialize_messages(messages).await.unwrap();

        assert_eq!(
            serialized[0]["tool_calls"][0]["function"]["arguments"],
            "{}"
        );
    }

    #[tokio::test]
    async fn serialize_messages_emits_multimodal_content_parts() {
        let store = Arc::new(InMemoryObjectStore::default());
        let mut object = store
            .put(
                "cas/acme/files/file-1/screenshot.png",
                b"png-bytes",
                ObjectMetadata {
                    media_type: "image/png".to_string(),
                    filename: "screenshot.png".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        object.media_type.clear();
        let provider = OpenAiCompatibleProvider::new(
            "test-key".to_string(),
            "http://localhost".to_string(),
            "test-model".to_string(),
            CasStore::new(store),
        );
        let serialized = provider
            .serialize_messages(vec![ChatMessage {
                role: "user".to_string(),
                content_parts: vec![text_part("look at this"), object_ref_part(object)],
                tool_calls: Vec::new(),
                tool_call_id: None,
            }])
            .await
            .unwrap();

        assert_eq!(serialized[0]["content"][0]["type"], "text");
        assert_eq!(serialized[0]["content"][0]["text"], "look at this");
        assert_eq!(serialized[0]["content"][1]["type"], "image_url");
        assert_eq!(
            serialized[0]["content"][1]["image_url"]["url"],
            "data:image/png;base64,cG5nLWJ5dGVz"
        );
    }

    #[tokio::test]
    async fn serialize_messages_projects_tool_image_results_as_follow_up_user_media() {
        let store = Arc::new(InMemoryObjectStore::default());
        let object = store
            .put(
                "cas/acme/files/file-1/screenshot.png",
                b"png-bytes",
                ObjectMetadata {
                    media_type: "image/png".to_string(),
                    filename: "screenshot.png".to_string(),
                    size_bytes: 9,
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let provider = OpenAiCompatibleProvider::new(
            "test-key".to_string(),
            "http://localhost".to_string(),
            "test-model".to_string(),
            CasStore::new(store),
        );

        let serialized = provider
            .serialize_messages(vec![ChatMessage {
                role: "tool".to_string(),
                content_parts: vec![object_ref_part(object)],
                tool_calls: Vec::new(),
                tool_call_id: Some("call_1".to_string()),
            }])
            .await
            .unwrap();

        assert_eq!(serialized.len(), 2);
        assert_eq!(serialized[0]["role"], "tool");
        assert_eq!(serialized[0]["tool_call_id"], "call_1");
        assert_eq!(
            serialized[0]["content"],
            "[Object reference: image/png; 9 bytes]"
        );
        assert_eq!(serialized[1]["role"], "user");
        assert_eq!(
            serialized[1]["content"][0]["text"],
            "Image result returned by tool call call_1."
        );
        assert_eq!(serialized[1]["content"][1]["type"], "image_url");
        assert_eq!(
            serialized[1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,cG5nLWJ5dGVz"
        );
    }

    #[tokio::test]
    async fn serialize_responses_input_embeds_tool_image_results_in_function_call_output() {
        let store = Arc::new(InMemoryObjectStore::default());
        let object = store
            .put(
                "cas/acme/files/file-1/screenshot.png",
                b"png-bytes",
                ObjectMetadata {
                    media_type: "image/png".to_string(),
                    filename: "screenshot.png".to_string(),
                    size_bytes: 9,
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let provider = OpenAiCompatibleProvider::with_api(
            "test-key".to_string(),
            "http://localhost".to_string(),
            "test-model".to_string(),
            CasStore::new(store),
            "responses",
        );

        let input = provider
            .serialize_responses_input(
                vec![ChatMessage {
                    role: "tool".to_string(),
                    content_parts: vec![object_ref_part(object)],
                    tool_calls: Vec::new(),
                    tool_call_id: Some("call_1".to_string()),
                }],
                None,
            )
            .await
            .unwrap();

        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_call_output");
        assert_eq!(input[0]["call_id"], "call_1");
        assert_eq!(input[0]["output"][0]["type"], "input_text");
        assert_eq!(
            input[0]["output"][0]["text"],
            "Image result returned by tool call call_1."
        );
        assert_eq!(input[0]["output"][1]["type"], "input_image");
        assert_eq!(
            input[0]["output"][1]["image_url"],
            "data:image/png;base64,cG5nLWJ5dGVz"
        );
    }

    #[tokio::test]
    async fn serialize_messages_keeps_tool_text_results_as_tool_messages_only() {
        let serialized = test_provider()
            .serialize_messages(vec![tool_result_message("plain result")])
            .await
            .unwrap();

        assert_eq!(serialized.len(), 1);
        assert_eq!(serialized[0]["role"], "tool");
        assert_eq!(serialized[0]["content"], "plain result");
        assert_eq!(serialized[0]["tool_call_id"], "call_1");
    }

    #[tokio::test]
    async fn serialize_messages_defers_tool_result_media_until_tool_batch_end() {
        let store = Arc::new(InMemoryObjectStore::default());
        let object = store
            .put(
                "cas/acme/files/file-1/screenshot.png",
                b"png-bytes",
                ObjectMetadata {
                    media_type: "image/png".to_string(),
                    filename: "screenshot.png".to_string(),
                    size_bytes: 9,
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let provider = OpenAiCompatibleProvider::new(
            "test-key".to_string(),
            "http://localhost".to_string(),
            "test-model".to_string(),
            CasStore::new(store),
        );

        let serialized = provider
            .serialize_messages(vec![
                ChatMessage {
                    role: "tool".to_string(),
                    content_parts: vec![object_ref_part(object)],
                    tool_calls: Vec::new(),
                    tool_call_id: Some("call_1".to_string()),
                },
                ChatMessage {
                    role: "tool".to_string(),
                    content_parts: vec![text_part("plain result".to_string())],
                    tool_calls: Vec::new(),
                    tool_call_id: Some("call_2".to_string()),
                },
            ])
            .await
            .unwrap();

        assert_eq!(serialized.len(), 3);
        assert_eq!(serialized[0]["role"], "tool");
        assert_eq!(serialized[0]["tool_call_id"], "call_1");
        assert_eq!(serialized[1]["role"], "tool");
        assert_eq!(serialized[1]["tool_call_id"], "call_2");
        assert_eq!(serialized[2]["role"], "user");
        assert_eq!(
            serialized[2]["content"][0]["text"],
            "Image result returned by tool call call_1."
        );
        assert_eq!(serialized[2]["content"][1]["type"], "image_url");
    }

    #[tokio::test]
    async fn serialize_messages_keeps_non_image_tool_object_refs_as_text_only() {
        let serialized = test_provider()
            .serialize_messages(vec![ChatMessage {
                role: "tool".to_string(),
                content_parts: vec![object_ref_part(
                    crate::gateway::rpc::data_proto::ObjectRef {
                        key: "cas/acme/files/file-1/report.pdf".to_string(),
                        media_type: "application/pdf".to_string(),
                        size_bytes: 3,
                        filename: "report.pdf".to_string(),
                        ..Default::default()
                    },
                )],
                tool_calls: Vec::new(),
                tool_call_id: Some("call_1".to_string()),
            }])
            .await
            .unwrap();

        assert_eq!(serialized.len(), 1);
        assert_eq!(serialized[0]["role"], "tool");
        assert_eq!(
            serialized[0]["content"],
            "[Object reference: application/pdf; 3 bytes]"
        );
    }

    #[test]
    fn debug_payload_redacts_data_urls_without_mutating_request_payload() {
        let payload = serde_json::json!({
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image_url",
                    "image_url": {
                        "url": "data:image/png;base64,c2VjcmV0",
                        "detail": "low"
                    }
                }]
            }],
            "metadata": {
                "callback": "https://example.com/data:image/png;base64/not-a-data-url"
            }
        });

        let debug_json = OpenAiCompatibleProvider::payload_json_for_debug(&payload);

        assert!(debug_json.contains("<redacted-data-url>"));
        assert!(!debug_json.contains("c2VjcmV0"));
        assert_eq!(
            payload["messages"][0]["content"][0]["image_url"]["url"],
            "data:image/png;base64,c2VjcmV0"
        );
        assert!(debug_json.contains("https://example.com/data:image/png;base64/not-a-data-url"));
    }

    #[test]
    fn supports_tool_retry_without_tools_requires_novita_internal_server_error_and_tool_history() {
        let provider = OpenAiCompatibleProvider::new(
            "key".to_string(),
            "https://api.novita.ai/v3/openai".to_string(),
            "model".to_string(),
            test_cas_store(),
        );
        let messages = vec![
            assistant_tool_call_message(
                "mcp_conic_execute_blog_post_publish",
                "{\"blogPostId\":\"page_1\"}",
            ),
            tool_result_message("{\"url\":\"https://github.com/example/repo/pull/2\"}"),
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

    #[tokio::test]
    async fn serialize_messages_omits_absent_tool_fields() {
        let serialized = test_provider()
            .serialize_messages(vec![chat_message_text("user", "hello")])
            .await
            .unwrap();

        assert_eq!(serialized[0]["role"], "user");
        assert_eq!(serialized[0]["content"], "hello");
        assert!(serialized[0].get("tool_calls").is_none());
        assert!(serialized[0].get("tool_call_id").is_none());
    }

    #[tokio::test]
    async fn serialize_messages_emits_empty_string_for_empty_content_parts() {
        let serialized = test_provider()
            .serialize_messages(vec![ChatMessage {
                role: "assistant".to_string(),
                content_parts: Vec::new(),
                tool_calls: Vec::new(),
                tool_call_id: None,
            }])
            .await
            .unwrap();

        assert_eq!(serialized[0]["content"], "");
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
            test_cas_store(),
        );
        let messages = vec![
            assistant_tool_call_message("search", "{\"q\":\"x\"}"),
            tool_result_message("{\"ok\":true}"),
        ];
        let tools = vec![crate::harness::llm::provider::Tool {
            name: "search".to_string(),
            description: "find things".to_string(),
            input_schema_json: serde_json::json!({"type": "object"}).to_string(),
        }];

        let response = provider
            .send_chat_request(
                ChatRequest {
                    messages,
                    tools,
                    thinking: None,
                    previous_response_id: None,
                    zero_data_retention: false,
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
            "id": "chatcmpl-1",
            "model": "gpt-test",
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "reasoning_tokens": 6,
                "total_tokens": 30,
                "prompt_tokens_details": {
                    "cached_tokens": 4,
                    "cache_write_tokens": 6
                }
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 14);
        assert_eq!(usage.reasoning_output_tokens, 6);
        assert_eq!(usage.cached_input_tokens, 4);
        assert_eq!(usage.cache_write_tokens, 6);
        assert_eq!(usage.total_tokens, 30);
        assert!(usage.usage_available);
        assert_eq!(usage.provider_request_id.as_deref(), Some("chatcmpl-1"));
        assert_eq!(usage.model, "gpt-test");
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
        assert_eq!(usage.output_tokens, 14);
        assert_eq!(usage.reasoning_output_tokens, 6);
        assert_eq!(usage.total_tokens, 0);
        assert!(usage.usage_available);
    }

    #[test]
    fn extract_usage_ignores_null_and_empty_usage() {
        assert!(extract_usage(&serde_json::json!({ "usage": null })).is_none());
        assert!(extract_usage(&serde_json::json!({ "usage": {} })).is_none());
    }

    #[test]
    fn openai_api_error_includes_code_type_and_message() {
        let error = openai_api_error(
            "OpenAI Responses API error",
            r#"{
                "error": {
                    "message": "You have no credits remaining.",
                    "type": "insufficient_quota",
                    "code": "credit_balance_exhausted"
                }
            }"#,
        );

        assert_eq!(
            error.to_string(),
            "OpenAI Responses API error (code: credit_balance_exhausted; type: insufficient_quota; message: You have no credits remaining.)"
        );
    }

    #[tokio::test]
    async fn send_chat_request_uses_reasoning_effort_without_implicit_output_cap() {
        let app = Router::new().route(
            "/chat/completions",
            post(|Json(payload): Json<serde_json::Value>| async move {
                assert_eq!(payload["reasoning_effort"], "high");
                assert!(payload.get("max_completion_tokens").is_none());
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
            test_cas_store(),
        );

        provider
            .chat_completion(ChatRequest {
                messages: vec![chat_message_text("user", "hi")],
                tools: vec![],
                thinking: Some(ThinkingConfig {
                    enabled: true,
                    budget_tokens: Some(2048),
                    effort: "high".to_string(),
                }),
                previous_response_id: None,
                zero_data_retention: false,
            })
            .await
            .unwrap();

        server.abort();
    }

    #[tokio::test]
    async fn send_responses_request_uses_reasoning_without_implicit_output_cap() {
        let app = Router::new().route(
            "/responses",
            post(|Json(payload): Json<serde_json::Value>| async move {
                assert_eq!(payload["reasoning"]["summary"], "auto");
                assert_eq!(payload["reasoning"]["effort"], "medium");
                assert!(payload.get("max_output_tokens").is_none());
                Json(serde_json::json!({
                    "output": [{
                        "type": "message",
                        "content": [{"type": "output_text", "text": "ok"}]
                    }]
                }))
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleProvider::with_api(
            "key".to_string(),
            format!("http://{addr}"),
            "model".to_string(),
            test_cas_store(),
            "responses",
        );

        provider
            .chat_completion(ChatRequest {
                messages: vec![chat_message_text("user", "hi")],
                tools: vec![],
                thinking: Some(ThinkingConfig {
                    enabled: true,
                    budget_tokens: None,
                    effort: "medium".to_string(),
                }),
                previous_response_id: None,
                zero_data_retention: false,
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
            test_cas_store(),
        );
        let result = provider
            .chat_completion(ChatRequest {
                messages: vec![chat_message_text("user", "hi")],
                tools: vec![],
                thinking: None,
                previous_response_id: None,
                zero_data_retention: false,
            })
            .await
            .unwrap();

        assert_eq!(result.content, "done");
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].id, "call_1");
        assert_eq!(result.tool_calls[0].name, "search");
        assert_eq!(result.tool_calls[0].arguments, "{\"q\":\"talon\"}");

        server.abort();
    }

    #[tokio::test]
    async fn chat_completion_preserves_numeric_tool_arguments() {
        let app = Router::new().route(
            "/chat/completions",
            post(|| async {
                Json(serde_json::json!({
                    "choices": [{
                        "message": {
                            "tool_calls": [{
                                "id": "call_1",
                                "function": {
                                    "name": "mcp_conic_list_links",
                                    "arguments": "{\"limit\":50,\"offset\":0}"
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
            test_cas_store(),
        );
        let result = provider
            .chat_completion(ChatRequest {
                messages: vec![chat_message_text("user", "hi")],
                tools: vec![],
                thinking: None,
                previous_response_id: None,
                zero_data_retention: false,
            })
            .await
            .unwrap();

        assert_eq!(
            result.tool_calls[0].arguments,
            "{\"limit\":50,\"offset\":0}"
        );
        let parsed: serde_json::Value = serde_json::from_str(&result.tool_calls[0].arguments)
            .expect("tool arguments should be valid JSON");
        assert_eq!(parsed["limit"], 50);
        assert_eq!(parsed["offset"], 0);
        assert!(parsed["limit"].is_number());
        assert!(!parsed["limit"].is_string());

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
            test_cas_store(),
        );
        let err = provider
            .send_chat_request(
                ChatRequest {
                    messages: vec![chat_message_text("user", "hi")],
                    tools: vec![],
                    thinking: None,
                    previous_response_id: None,
                    zero_data_retention: false,
                },
                false,
            )
            .await
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("OpenAI-compatible API request failed"));
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
            test_cas_store(),
        );

        let response = provider
            .send_chat_request(
                ChatRequest {
                    messages: vec![chat_message_text("user", "hi")],
                    tools: vec![],
                    thinking: None,
                    previous_response_id: None,
                    zero_data_retention: false,
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
            test_cas_store(),
        );
        let err = provider
            .send_chat_request(
                ChatRequest {
                    messages: vec![
                        assistant_tool_call_message("search", "{\"q\":\"x\"}"),
                        tool_result_message("{\"ok\":true}"),
                    ],
                    tools: vec![crate::harness::llm::provider::Tool {
                        name: "search".to_string(),
                        description: "find things".to_string(),
                        input_schema_json: serde_json::json!({"type": "object"}).to_string(),
                    }],
                    thinking: None,
                    previous_response_id: None,
                    zero_data_retention: false,
                },
                false,
            )
            .await
            .unwrap_err();

        let text = err.to_string();
        assert!(text.contains("OpenAI-compatible API request failed after retry"));
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
            test_cas_store(),
        );
        let mut stream = provider
            .stream_chat_completion(ChatRequest {
                messages: vec![chat_message_text("user", "hi")],
                tools: vec![],
                thinking: None,
                previous_response_id: None,
                zero_data_retention: false,
            })
            .await
            .unwrap();

        let first = stream.next().await.unwrap().unwrap();
        match first {
            ChatStreamEvent {
                event: Some(chat_stream_event::Event::TextDelta(text)),
            } => assert_eq!(text, "hello"),
            other => panic!("unexpected event: {other:?}"),
        }

        let second = stream.next().await.unwrap().unwrap();
        match second {
            ChatStreamEvent {
                event: Some(chat_stream_event::Event::ToolCallDelta(delta)),
            } => {
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
            test_cas_store(),
        );
        let mut stream = provider
            .stream_chat_completion(ChatRequest {
                messages: vec![chat_message_text("user", "hi")],
                tools: vec![],
                thinking: None,
                previous_response_id: None,
                zero_data_retention: false,
            })
            .await
            .unwrap();

        assert!(stream.next().await.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn responses_stream_surfaces_error_event_details_and_done_text() {
        let app = Router::new().route(
            "/responses",
            post(|| async {
                axum::response::Response::builder()
                    .status(axum::http::StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(axum::body::Body::from(concat!(
                        "event: response.output_text.done\n",
                        "data: {\"output_index\":0,\"content_index\":0,\"text\":\"partial reply\"}\n\n",
                        "event: error\n",
                        "data: {\"code\":\"credit_balance_exhausted\",\"type\":\"insufficient_quota\",\"message\":\"You have no credits remaining.\"}\n\n"
                    )))
                    .unwrap()
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let provider = OpenAiCompatibleProvider::with_api(
            "key".to_string(),
            format!("http://{addr}"),
            "model".to_string(),
            test_cas_store(),
            "responses",
        );
        let mut stream = provider
            .stream_chat_completion(ChatRequest {
                messages: vec![chat_message_text("user", "hi")],
                tools: vec![],
                thinking: None,
                previous_response_id: None,
                zero_data_retention: false,
            })
            .await
            .unwrap();

        let text = stream.next().await.unwrap().unwrap();
        assert!(matches!(
            text,
            ChatStreamEvent {
                event: Some(chat_stream_event::Event::TextDelta(ref text)),
            } if text == "partial reply"
        ));
        let error = stream.next().await.unwrap().unwrap_err();
        assert_eq!(
            error.to_string(),
            "OpenAI Responses stream error (code: credit_balance_exhausted; type: insufficient_quota; message: You have no credits remaining.)"
        );
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
            test_cas_store(),
        );
        let text = provider.completion("hello").await.unwrap();
        assert_eq!(text, "plain completion");
        server.abort();
    }

    #[test]
    fn openai_tool_parameters_omit_null_schema_fields() {
        let schema = OpenAiCompatibleProvider::openai_tool_parameters(
            r#"{
                "type": "object",
                "description": null,
                "properties": {
                    "urls": {
                        "type": "array",
                        "description": null,
                        "items": {
                            "type": "string",
                            "description": null
                        }
                    }
                }
            }"#,
        );

        assert_eq!(schema["type"], "object");
        assert!(schema.get("description").is_none());
        assert!(schema["properties"]["urls"].get("description").is_none());
        assert!(schema["properties"]["urls"]["items"]
            .get("description")
            .is_none());
    }

    #[test]
    fn parse_responses_response_extracts_text_tools_and_usage() {
        let response = parse_responses_response(&serde_json::json!({
            "id": "resp_1",
            "output": [
                {"type": "message", "content": [{"type": "output_text", "text": "done"}]},
                {"type": "function_call", "call_id": "call_1", "name": "lookup", "arguments": "{\"q\":\"x\"}"}
            ],
            "usage": {
                "input_tokens": 4,
                "output_tokens": 8,
                "output_tokens_details": {"reasoning_tokens": 3},
                "total_tokens": 12
            }
        }))
        .unwrap();
        assert_eq!(response.content, "done");
        assert_eq!(response.tool_calls[0].id, "call_1");
        assert_eq!(response.usage.unwrap().reasoning_output_tokens, 3);
    }

    #[tokio::test]
    async fn responses_missing_tool_output_retries_from_complete_local_history() {
        let hits = Arc::new(AtomicUsize::new(0));
        let payloads = Arc::new(std::sync::Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/responses",
                post({
                    let hits = hits.clone();
                    let payloads = payloads.clone();
                    move |Json(payload): Json<serde_json::Value>| {
                        let hits = hits.clone();
                        let payloads = payloads.clone();
                        async move {
                            payloads.lock().unwrap().push(payload);
                            if hits.fetch_add(1, Ordering::SeqCst) == 0 {
                                (
                                    axum::http::StatusCode::BAD_REQUEST,
                                    Json(serde_json::json!({
                                        "error": {
                                            "message": "No tool output found for function call call_123."
                                        }
                                    })),
                                )
                            } else {
                                (
                                    axum::http::StatusCode::OK,
                                    Json(serde_json::json!({
                                        "id": "resp_recovered",
                                        "output": [{
                                            "type": "message",
                                            "content": [{"type": "output_text", "text": "recovered"}]
                                        }]
                                    })),
                                )
                            }
                        }
                    }
                }),
            )
            .with_state(());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let provider = OpenAiCompatibleProvider::with_api(
            "key".to_string(),
            format!("http://{addr}"),
            "model".to_string(),
            test_cas_store(),
            "responses",
        );

        let response = provider
            .chat_completion(ChatRequest {
                messages: vec![
                    chat_message_text("system", "system instructions"),
                    chat_message_text("user", "old question"),
                    chat_message_text("assistant", "old answer"),
                    chat_message_text("user", "new question"),
                ],
                tools: vec![],
                thinking: None,
                previous_response_id: Some("resp_poisoned".to_string()),
                zero_data_retention: false,
            })
            .await
            .unwrap();

        assert_eq!(response.content, "recovered");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        let payloads = payloads.lock().unwrap();
        assert_eq!(payloads[0]["previous_response_id"], "resp_poisoned");
        assert!(payloads[0]["input"].to_string().contains("new question"));
        assert!(!payloads[0]["input"].to_string().contains("old question"));
        assert!(payloads[1].get("previous_response_id").is_none());
        assert!(payloads[1]["input"].to_string().contains("old question"));
        assert!(payloads[1]["input"].to_string().contains("old answer"));
        server.abort();
    }

    #[tokio::test]
    async fn responses_input_without_previous_id_keeps_full_history() {
        let provider = test_provider();
        let input = provider
            .serialize_responses_input(
                vec![
                    chat_message_text("system", "system instructions"),
                    chat_message_text("user", "old question"),
                    chat_message_text("assistant", "old answer"),
                    chat_message_text("user", "new question"),
                ],
                None,
            )
            .await
            .unwrap();

        assert_eq!(input.len(), 4);
        assert!(input
            .iter()
            .any(|item| item.to_string().contains("old question")));
        assert!(input
            .iter()
            .any(|item| item.to_string().contains("old answer")));
        assert_eq!(input[2]["content"][0]["type"], "output_text");
    }

    #[tokio::test]
    async fn responses_input_with_previous_id_contains_instructions_and_new_suffix_only() {
        let provider = test_provider();
        let messages = vec![
            chat_message_text("system", "system instructions"),
            chat_message_text("developer", "developer instructions"),
            chat_message_text("user", "old question"),
            assistant_tool_call_message("lookup", "{\"q\":\"old\"}"),
            chat_message_text("tool", "new tool output"),
            chat_message_text("user", "new question"),
        ];
        let input = provider
            .serialize_responses_input(messages, Some("resp_previous"))
            .await
            .unwrap();

        assert_eq!(input.len(), 4);
        assert_eq!(input[0]["role"], "system");
        assert_eq!(input[1]["role"], "developer");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[3]["role"], "user");
        assert!(!input
            .iter()
            .any(|item| item.to_string().contains("old question")));
    }

    #[test]
    fn responses_usage_preserves_id_without_usage_counts() {
        let usage = extract_responses_usage(&serde_json::json!({
            "id": "resp_without_usage",
            "model": "model"
        }))
        .expect("response id should be retained");
        assert_eq!(
            usage.provider_request_id.as_deref(),
            Some("resp_without_usage")
        );
        assert!(!usage.usage_available);
    }

    #[test]
    fn responses_usage_extracts_cache_write_tokens() {
        let usage = extract_responses_usage(&serde_json::json!({
            "id": "resp_cache_write",
            "model": "gpt-test",
            "usage": {
                "input_tokens": 100,
                "input_tokens_details": {
                    "cached_tokens": 20,
                    "cache_write_tokens": 80
                },
                "output_tokens": 5,
                "total_tokens": 105
            }
        }))
        .expect("usage should be present");

        assert_eq!(usage.cached_input_tokens, 20);
        assert_eq!(usage.cache_write_tokens, 80);
    }

    #[test]
    fn stale_response_id_errors_are_retryable_but_generic_errors_are_not() {
        assert!(is_stale_previous_response_id(
            reqwest::StatusCode::BAD_REQUEST,
            "previous_response_id is invalid"
        ));
        assert!(!is_stale_previous_response_id(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "previous_response_id is invalid"
        ));
        assert!(!is_stale_previous_response_id(
            reqwest::StatusCode::BAD_REQUEST,
            "rate limit exceeded"
        ));
    }

    #[test]
    fn missing_tool_output_errors_are_retryable_but_generic_errors_are_not() {
        assert!(is_missing_tool_output_for_previous_response(
            reqwest::StatusCode::BAD_REQUEST,
            "No tool output found for function call call_123."
        ));
        assert!(!is_missing_tool_output_for_previous_response(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "No tool output found for function call call_123."
        ));
        assert!(!is_missing_tool_output_for_previous_response(
            reqwest::StatusCode::BAD_REQUEST,
            "Invalid tool schema"
        ));
    }
}
