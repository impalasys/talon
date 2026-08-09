// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::harness::executor::compaction::{ContextMetrics, ModelContextLimits};
use crate::harness::llm::{
    chat_content_part, ChatContentPart, ChatMessage, ChatRequest, TokenCounter, Tool, ToolCall,
};
use opentelemetry::KeyValue;
use serde_json::{json, Value};
use tracing::{field, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;

const MAX_TELEMETRY_FIELD_CHARS: usize = 12_000;

pub fn tenant_slug(namespace: &str) -> Option<&str> {
    namespace
        .strip_prefix("Tenant:")
        .and_then(|rest| rest.split(':').next())
        .filter(|slug| !slug.is_empty())
}

pub fn genai_provider_name(provider_key: &str) -> String {
    let normalized = provider_key.trim().to_ascii_lowercase();
    if normalized.contains("anthropic") {
        "anthropic".to_string()
    } else if normalized.contains("openai") {
        "openai".to_string()
    } else {
        normalized
    }
}

pub fn agent_span(namespace: &str, agent_id: &str, session_id: &str) -> Span {
    let otel_name = format!("invoke_agent {agent_id}");
    let span = tracing::info_span!(
        "invoke_agent",
        "otel.name" = otel_name.as_str(),
        "gen_ai.operation.name" = "invoke_agent",
        "gen_ai.agent.name" = agent_id,
        "gen_ai.conversation.id" = session_id,
        "talon.namespace" = namespace,
        "talon.tenant.slug" = field::Empty,
        "talon.session.id" = session_id,
        "talon.agent.name" = agent_id,
        "error.type" = field::Empty,
    );
    if let Some(slug) = tenant_slug(namespace) {
        span.record("talon.tenant.slug", slug);
    }
    span
}

pub fn chat_span(
    namespace: &str,
    agent_id: &str,
    session_id: &str,
    provider_key: &str,
    model: &str,
    reasoning_level: Option<&str>,
) -> Span {
    let provider = genai_provider_name(provider_key);
    let otel_name = format!("chat {model}");
    let span = tracing::info_span!(
        "chat",
        "otel.name" = otel_name.as_str(),
        "gen_ai.operation.name" = "chat",
        "gen_ai.provider.name" = provider.as_str(),
        "gen_ai.request.model" = model,
        "gen_ai.request.stream" = true,
        "gen_ai.request.reasoning.level" = field::Empty,
        "gen_ai.conversation.id" = session_id,
        "gen_ai.input.messages" = field::Empty,
        "gen_ai.tool.definitions" = field::Empty,
        "gen_ai.output.messages" = field::Empty,
        "gen_ai.usage.input_tokens" = field::Empty,
        "gen_ai.usage.input_tokens.cached" = field::Empty,
        "gen_ai.usage.input_tokens.cache_write" = field::Empty,
        "gen_ai.usage.output_tokens" = field::Empty,
        "gen_ai.usage.output_tokens.reasoning" = field::Empty,
        "gen_ai.usage.total_tokens" = field::Empty,
        "gen_ai.response.time_to_first_chunk" = field::Empty,
        "talon.namespace" = namespace,
        "talon.tenant.slug" = field::Empty,
        "talon.session.id" = session_id,
        "talon.agent.name" = agent_id,
        "talon.llm.provider_key" = provider_key,
        "error.type" = field::Empty,
    );
    if let Some(slug) = tenant_slug(namespace) {
        span.record("talon.tenant.slug", slug);
    }
    if let Some(level) = reasoning_level.filter(|level| !level.trim().is_empty()) {
        span.record("gen_ai.request.reasoning.level", level);
    }
    span
}

pub fn record_chat_operation_details(
    span: &Span,
    request: &ChatRequest,
    model_limits: ModelContextLimits,
    context_metrics: ContextMetrics,
) {
    if span.is_disabled() {
        return;
    }

    let mut attributes = vec![
        KeyValue::new("gen_ai.operation.name", "chat"),
        KeyValue::new(
            "talon.context.request_message_count",
            request.messages.len() as i64,
        ),
        KeyValue::new("talon.context.tool_count", request.tools.len() as i64),
        KeyValue::new(
            "talon.context.transcript_messages_before",
            context_metrics.transcript_messages_before as i64,
        ),
        KeyValue::new(
            "talon.context.transcript_messages_after",
            context_metrics.transcript_messages_after as i64,
        ),
        KeyValue::new(
            "talon.context.transcript_message_parts_before",
            context_metrics.transcript_message_parts_before as i64,
        ),
        KeyValue::new(
            "talon.context.transcript_message_parts_after",
            context_metrics.transcript_message_parts_after as i64,
        ),
        KeyValue::new(
            "talon.context.transcript_message_parts_removed",
            context_metrics
                .transcript_message_parts_before
                .saturating_sub(context_metrics.transcript_message_parts_after) as i64,
        ),
        KeyValue::new(
            "talon.context.transcript_messages_omitted",
            context_metrics.transcript_messages_omitted as i64,
        ),
        KeyValue::new(
            "talon.context.estimated_chars_before",
            context_metrics.estimated_chars_before as i64,
        ),
        KeyValue::new(
            "talon.context.estimated_chars_after",
            context_metrics.estimated_chars_after as i64,
        ),
        KeyValue::new(
            "talon.context.estimated_chars_removed",
            context_metrics
                .estimated_chars_before
                .saturating_sub(context_metrics.estimated_chars_after) as i64,
        ),
        KeyValue::new(
            "talon.context.estimated_input_tokens",
            (context_metrics.estimated_chars_after / 4) as i64,
        ),
        KeyValue::new(
            "talon.context.tool_schema_chars",
            context_metrics.tool_schema_chars as i64,
        ),
        KeyValue::new(
            "talon.context.compaction_applied",
            context_metrics.compaction_applied,
        ),
        KeyValue::new(
            "talon.context.reserved_output_tokens",
            model_limits.reserved_output_tokens() as i64,
        ),
    ];
    if let Some(context_window) = model_limits.context_window_tokens {
        attributes.push(KeyValue::new(
            "talon.context.context_window_tokens",
            context_window as i64,
        ));
    }
    if let Some(max_output) = model_limits.max_output_tokens {
        attributes.push(KeyValue::new(
            "talon.context.max_output_tokens",
            max_output as i64,
        ));
    }
    if let Some(long_context) = model_limits.long_context_tokens {
        attributes.push(KeyValue::new(
            "talon.context.long_context_tokens",
            long_context as i64,
        ));
    }
    span.add_event("gen_ai.client.inference.operation.details", attributes);
}

pub fn tool_span(
    namespace: &str,
    agent_id: &str,
    session_id: &str,
    tool_call: &ToolCall,
    tool_type: &str,
) -> Span {
    let arguments = serialize_json_or_string(&tool_call.arguments);
    let otel_name = format!("execute_tool {}", tool_call.name);
    let span = tracing::info_span!(
        "execute_tool",
        "otel.name" = otel_name.as_str(),
        "gen_ai.operation.name" = "execute_tool",
        "gen_ai.tool.name" = tool_call.name.as_str(),
        "gen_ai.tool.call.id" = tool_call.id.as_str(),
        "gen_ai.tool.type" = tool_type,
        "gen_ai.tool.call.arguments" = arguments.as_str(),
        "gen_ai.tool.call.result" = field::Empty,
        "talon.namespace" = namespace,
        "talon.tenant.slug" = field::Empty,
        "talon.session.id" = session_id,
        "talon.agent.name" = agent_id,
        "error.type" = field::Empty,
    );
    if let Some(slug) = tenant_slug(namespace) {
        span.record("talon.tenant.slug", slug);
    }
    span
}

pub fn record_usage(span: &Span, usage: &TokenCounter) {
    span.record("gen_ai.usage.input_tokens", usage.input_tokens);
    span.record(
        "gen_ai.usage.input_tokens.cached",
        usage.cached_input_tokens,
    );
    span.record(
        "gen_ai.usage.input_tokens.cache_write",
        usage.cache_write_tokens,
    );
    span.record("gen_ai.usage.output_tokens", usage.output_tokens);
    span.record(
        "gen_ai.usage.output_tokens.reasoning",
        usage.reasoning_output_tokens,
    );
    span.record("gen_ai.usage.total_tokens", usage.total_tokens);
}

pub fn record_time_to_first_chunk(span: &Span, seconds: f64) {
    span.record("gen_ai.response.time_to_first_chunk", seconds);
}

pub fn record_chat_output(span: &Span, content: &str, tool_calls: &[ToolCall]) {
    let output = serialize_output_messages_json(content, tool_calls);
    span.record("gen_ai.output.messages", output.as_str());
}

pub fn record_tool_result(span: &Span, result: &str) {
    let output = serialize_json_or_string(&truncate_middle(result, MAX_TELEMETRY_FIELD_CHARS));
    span.record("gen_ai.tool.call.result", output.as_str());
}

pub fn record_error(span: &Span, error: &anyhow::Error) {
    span.record("error.type", low_cardinality_error_text(&error.to_string()));
}

pub fn record_error_text(span: &Span, error: &str) {
    span.record("error.type", low_cardinality_error_text(error));
}

pub fn serialize_messages_json(messages: &[ChatMessage]) -> String {
    serialize_json_array(messages.iter().map(message_value))
}

pub fn serialize_output_messages_json(content: &str, tool_calls: &[ToolCall]) -> String {
    let mut parts = Vec::new();
    if !content.is_empty() {
        parts.push(json!({
            "type": "text",
            "content": content,
        }));
    }
    for call in tool_calls {
        parts.push(tool_call_part_value(call));
    }
    serialize_json_array([json!({
        "role": "assistant",
        "parts": parts,
    })])
}

pub fn serialize_tool_definitions_json(tools: &[Tool]) -> String {
    serialize_json_array(tools.iter().map(|tool| {
        let parameters =
            serde_json::from_str::<Value>(&tool.input_schema_json).unwrap_or_else(|_| {
                if tool.input_schema_json.trim().is_empty() {
                    json!({})
                } else {
                    Value::String(tool.input_schema_json.clone())
                }
            });
        json!({
            "type": "function",
            "name": tool.name,
            "description": tool.description,
            "parameters": parameters,
        })
    }))
}

pub fn serialize_json_or_string(value: &str) -> String {
    serde_json::from_str::<Value>(value)
        .unwrap_or_else(|_| Value::String(value.to_string()))
        .to_string()
}

fn truncate_middle(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return text.to_string();
    }

    if max_chars <= 32 {
        return chars.into_iter().take(max_chars).collect();
    }

    let omitted = chars.len() - max_chars;
    let marker = format!("\n...[{omitted} chars omitted]...\n");
    let marker_len = marker.chars().count();
    if marker_len >= max_chars {
        return chars.into_iter().take(max_chars).collect();
    }

    let remaining = max_chars - marker_len;
    let prefix_len = remaining * 2 / 3;
    let suffix_len = remaining.saturating_sub(prefix_len);
    let prefix = chars.iter().take(prefix_len).collect::<String>();
    let suffix = chars
        .iter()
        .skip(chars.len().saturating_sub(suffix_len))
        .collect::<String>();
    format!("{prefix}{marker}{suffix}")
}

fn serialize_json_array(values: impl IntoIterator<Item = Value>) -> String {
    Value::Array(values.into_iter().collect()).to_string()
}

fn message_value(message: &ChatMessage) -> Value {
    let parts = if message.role == "tool" {
        let tool_call_id = message.tool_call_id.as_deref().unwrap_or_default();
        message
            .content_parts
            .iter()
            .filter_map(|part| match part.content.as_ref()? {
                chat_content_part::Content::Text(text) => Some(json!({
                    "type": "tool_call_response",
                    "id": tool_call_id,
                    "result": text,
                })),
                _ => None,
            })
            .collect()
    } else {
        let mut parts = content_parts_value(&message.content_parts);
        for call in &message.tool_calls {
            parts.push(tool_call_part_value(call));
        }
        if let Some(tool_call_id) = &message.tool_call_id {
            for part in &mut parts {
                if let Some(object) = part.as_object_mut() {
                    object.insert("id".to_string(), Value::String(tool_call_id.clone()));
                }
            }
        }
        parts
    };
    json!({
        "role": message.role,
        "parts": parts,
    })
}

fn content_parts_value(parts: &[ChatContentPart]) -> Vec<Value> {
    parts
        .iter()
        .filter_map(|part| match part.content.as_ref() {
            Some(chat_content_part::Content::Text(text)) => Some(json!({
                "type": "text",
                "content": text,
            })),
            Some(chat_content_part::Content::ObjectRef(object)) => Some(json!({
                "type": "object",
                "object_key": object.key,
                "media_type": object.media_type,
                "size_bytes": object.size_bytes,
                "filename": object.filename,
            })),
            None => None,
        })
        .collect()
}

fn tool_call_part_value(call: &ToolCall) -> Value {
    let arguments = serde_json::from_str::<Value>(&call.arguments)
        .unwrap_or_else(|_| Value::String(call.arguments.clone()));
    json!({
        "type": "tool_call",
        "id": call.id,
        "name": call.name,
        "arguments": arguments,
    })
}

fn low_cardinality_error_text(error: &str) -> &'static str {
    let error_lc = error.to_ascii_lowercase();
    if error_lc.contains("cancel") || error_lc.contains("interrupt") {
        "cancelled"
    } else if error_lc.contains("timeout") {
        "timeout"
    } else {
        "_OTHER"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::rpc::data_proto;
    use crate::harness::llm::{
        chat_message_text, object_ref_part, text_part, ChatMessage, Tool, ToolCall,
    };

    #[test]
    fn tenant_slug_uses_first_segment_after_tenant_prefix() {
        assert_eq!(tenant_slug("Tenant:acme"), Some("acme"));
        assert_eq!(tenant_slug("Tenant:acme:prod"), Some("acme"));
        assert_eq!(tenant_slug("conic:wks:13"), None);
        assert_eq!(tenant_slug("Tenant:"), None);
    }

    #[test]
    fn provider_name_maps_known_provider_families() {
        assert_eq!(genai_provider_name("prod-openai"), "openai");
        assert_eq!(genai_provider_name("Anthropic"), "anthropic");
        assert_eq!(genai_provider_name("novita"), "novita");
    }

    #[test]
    fn messages_serialize_content_and_tool_shapes() {
        let message = ChatMessage {
            role: "assistant".to_string(),
            content_parts: vec![
                text_part("hello"),
                object_ref_part(data_proto::ObjectRef {
                    key: "cas/acme/files/file-1/sha".to_string(),
                    media_type: "image/png".to_string(),
                    size_bytes: 123,
                    filename: "image.png".to_string(),
                    ..Default::default()
                }),
            ],
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "lookup".to_string(),
                arguments: r#"{"query":"plan"}"#.to_string(),
            }],
            tool_call_id: None,
        };

        let json = serialize_messages_json(&[message]);
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["role"], "assistant");
        assert_eq!(value[0]["parts"][0]["content"], "hello");
        assert_eq!(
            value[0]["parts"][1]["object_key"],
            "cas/acme/files/file-1/sha"
        );
        assert_eq!(value[0]["parts"][2]["arguments"]["query"], "plan");
    }

    #[test]
    fn tool_definitions_parse_json_schema_when_possible() {
        let json = serialize_tool_definitions_json(&[Tool {
            name: "lookup".to_string(),
            description: "Search".to_string(),
            input_schema_json: r#"{"type":"object"}"#.to_string(),
        }]);
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["name"], "lookup");
        assert_eq!(value[0]["parameters"]["type"], "object");
    }

    #[test]
    fn tool_definitions_use_empty_object_for_blank_schema() {
        let json = serialize_tool_definitions_json(&[
            Tool {
                name: "blank".to_string(),
                description: "Blank".to_string(),
                input_schema_json: String::new(),
            },
            Tool {
                name: "whitespace".to_string(),
                description: "Whitespace".to_string(),
                input_schema_json: "  \n\t".to_string(),
            },
        ]);
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["parameters"], json!({}));
        assert_eq!(value[1]["parameters"], json!({}));
    }

    #[test]
    fn json_or_string_preserves_structured_json_and_plain_strings() {
        assert_eq!(serialize_json_or_string(r#"{"ok":true}"#), r#"{"ok":true}"#);
        assert_eq!(serialize_json_or_string("plain"), r#""plain""#);
    }

    #[test]
    fn output_messages_include_text_and_tool_calls() {
        let json = serialize_output_messages_json(
            "done",
            &[ToolCall {
                id: "call-1".to_string(),
                name: "lookup".to_string(),
                arguments: r#"{"x":1}"#.to_string(),
            }],
        );
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["role"], "assistant");
        assert_eq!(value[0]["parts"][0]["content"], "done");
        assert_eq!(value[0]["parts"][1]["name"], "lookup");
    }

    #[test]
    fn tool_response_message_records_tool_call_id() {
        let mut message = chat_message_text("tool", "result");
        message.tool_call_id = Some("call-1".to_string());
        let json = serialize_messages_json(&[message]);
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["parts"][0]["type"], "tool_call_response");
        assert_eq!(value[0]["parts"][0]["id"], "call-1");
        assert_eq!(value[0]["parts"][0]["result"], "result");
    }

    #[test]
    fn error_classification_is_case_insensitive() {
        assert_eq!(low_cardinality_error_text("Cancelled by user"), "cancelled");
        assert_eq!(low_cardinality_error_text("Timeout occurred"), "timeout");
        assert_eq!(low_cardinality_error_text("unknown failure"), "_OTHER");
    }
}
