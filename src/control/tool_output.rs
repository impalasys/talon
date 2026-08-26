// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::cas::CasStore;
use crate::gateway::rpc::data_proto;
use crate::harness::llm::{
    chat_content_part, object_ref_part, text_part, ChatContentPart, ToolOutput,
    ToolOutputByteRange, ToolOutputLineSelection,
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const TOOL_RESULT_OBJECT_THRESHOLD_BYTES: usize = 2 * 1024;
pub const TOOL_RESULT_INLINE_CONTEXT_BYTES: usize = 8 * 1024;
pub const TOOL_RESULT_DURABLE_SUMMARY_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq)]
pub struct ToolResultPayload {
    pub tool_call_id: String,
    pub tool_output: ToolOutput,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolOutputStorageContext<'a> {
    pub ns: &'a str,
    pub agent: &'a str,
    pub session_id: &'a str,
    pub message_id: &'a str,
    pub part_id: &'a str,
    pub tool_call_id: &'a str,
    pub tool_name: &'a str,
}

pub trait ToolOutputExt {
    fn text(text: impl Into<String>) -> Self;
    fn from_source_object(
        bytes: Vec<u8>,
        media_type: impl Into<String>,
        filename: impl Into<String>,
        object_ref: data_proto::ObjectRef,
    ) -> Self;
    fn from_content_parts(content_parts: Vec<ChatContentPart>, summary: impl Into<String>) -> Self;
    fn summary(&self) -> String;
    fn content_parts(&self) -> Vec<ChatContentPart>;
    fn object_ref(&self) -> Option<&data_proto::ObjectRef>;
}

impl ToolOutputExt for ToolOutput {
    fn text(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            content_parts: vec![text_part(text.clone())],
            summary: text,
            line_selection: None,
            byte_range: None,
        }
    }

    fn from_source_object(
        bytes: Vec<u8>,
        media_type: impl Into<String>,
        filename: impl Into<String>,
        mut object_ref: data_proto::ObjectRef,
    ) -> Self {
        let media_type = media_type.into();
        let filename = filename.into();
        if object_ref.media_type.trim().is_empty() {
            object_ref.media_type = media_type.clone();
        }
        if object_ref.filename.trim().is_empty() {
            object_ref.filename = filename.clone();
        }
        let summary = if is_text_object_media_type(&media_type)
            && bytes.len() < TOOL_RESULT_OBJECT_THRESHOLD_BYTES
        {
            String::from_utf8_lossy(&bytes).to_string()
        } else {
            object_ref_summary(&media_type, &filename, bytes.len() as u64)
        };
        Self {
            content_parts: vec![object_ref_part(object_ref)],
            summary,
            line_selection: None,
            byte_range: None,
        }
    }

    fn from_content_parts(content_parts: Vec<ChatContentPart>, summary: impl Into<String>) -> Self {
        Self {
            content_parts,
            summary: summary.into(),
            line_selection: None,
            byte_range: None,
        }
    }

    fn summary(&self) -> String {
        summary(self)
    }

    fn content_parts(&self) -> Vec<ChatContentPart> {
        self.content_parts.clone()
    }

    fn object_ref(&self) -> Option<&data_proto::ObjectRef> {
        first_object_ref(self)
    }
}

pub fn is_text_object_media_type(media_type: &str) -> bool {
    let media_type = media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    media_type.starts_with("text/")
        || matches!(
            media_type.as_str(),
            "application/json"
                | "application/yaml"
                | "application/x-yaml"
                | "application/toml"
                | "application/xml"
                | "application/javascript"
                | "application/x-javascript"
        )
        || media_type.ends_with("+json")
        || media_type.ends_with("+xml")
}

pub fn first_object_ref(output: &ToolOutput) -> Option<&data_proto::ObjectRef> {
    output
        .content_parts
        .iter()
        .find_map(|part| match part.content.as_ref()? {
            chat_content_part::Content::ObjectRef(object_ref) => Some(object_ref),
            _ => None,
        })
}

pub fn plain_text(output: &ToolOutput) -> Option<String> {
    if output.content_parts.is_empty() {
        return None;
    }
    output
        .content_parts
        .iter()
        .all(|part| matches!(part.content, Some(chat_content_part::Content::Text(_))))
        .then(|| {
            output
                .content_parts
                .iter()
                .filter_map(|part| match part.content.as_ref()? {
                    chat_content_part::Content::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("")
        })
}

pub fn summary(output: &ToolOutput) -> String {
    if !output.summary.is_empty() {
        return output.summary.clone();
    }
    if let Some(text) = plain_text(output) {
        return text;
    }
    first_object_ref(output)
        .map(|object_ref| {
            object_ref_summary(
                &object_ref.media_type,
                &object_ref.filename,
                object_ref.size_bytes,
            )
        })
        .unwrap_or_default()
}

pub fn display_text(output: &ToolOutput) -> String {
    plain_text(output).unwrap_or_else(|| {
        serde_json::to_string(&tool_output_json(output)).unwrap_or_else(|_| summary(output))
    })
}

pub fn tool_result_handle(tool_call_id: &str) -> String {
    format!("tr://{}", urlencoding::encode(tool_call_id))
}

pub fn tool_result_part_handle(tool_call_id: &str, index: usize) -> String {
    format!("{}/parts/{index}", tool_result_handle(tool_call_id))
}

pub fn is_tool_result_object_ref(object_ref: &data_proto::ObjectRef) -> bool {
    object_ref
        .metadata
        .get(crate::control::cas::METADATA_KIND)
        .is_some_and(|kind| kind == crate::control::cas::METADATA_KIND_TOOL_RESULT)
}

pub fn compact_tool_result_catalog(tool_call_id: &str, parts: &[ChatContentPart]) -> String {
    let entries = parts
        .iter()
        .enumerate()
        .map(|(index, part)| match part.content.as_ref() {
            Some(chat_content_part::Content::Text(text)) => {
                format!("parts/{index}: text/plain ({} bytes)", text.len())
            }
            Some(chat_content_part::Content::ObjectRef(object)) => {
                format!("parts/{index}: {} ({} bytes)", object.media_type, object.size_bytes)
            }
            None => format!("parts/{index}: empty"),
        })
        .collect::<Vec<_>>()
        .join(", ");
    truncate_utf8(
        &format!(
            "Tool result catalog {}: {}. Use read with ref '{}' to inspect a part.",
            tool_result_handle(tool_call_id), entries, tool_result_handle(tool_call_id)
        ),
        TOOL_RESULT_DURABLE_SUMMARY_BYTES,
    )
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes { return value.to_string(); }
    let mut end = max_bytes.saturating_sub(3);
    while end > 0 && !value.is_char_boundary(end) { end -= 1; }
    format!("{}...", &value[..end])
}

pub async fn normalize_for_session_storage(
    cas: &CasStore,
    ctx: ToolOutputStorageContext<'_>,
    output: &ToolOutput,
) -> Result<ToolOutput> {
    let mut content_parts = Vec::with_capacity(output.content_parts.len());
    let mut stored_large_text = false;
    for (index, part) in output.content_parts.iter().enumerate() {
        let Some(chat_content_part::Content::Text(text)) = part.content.as_ref() else {
            content_parts.push(part.clone());
            continue;
        };
        if text.as_bytes().len() < TOOL_RESULT_OBJECT_THRESHOLD_BYTES {
            content_parts.push(part.clone());
            continue;
        }
        let object_part_id = if output.content_parts.len() == 1 {
            ctx.part_id.to_string()
        } else {
            format!("{}-{}", ctx.part_id, index)
        };
        let object_ref = cas
            .put_tool_result(
                ctx.ns,
                ctx.agent,
                ctx.session_id,
                ctx.message_id,
                &object_part_id,
                ctx.tool_call_id,
                ctx.tool_name,
                text.as_bytes(),
            )
            .await?;
        content_parts.push(object_ref_part(object_ref));
        stored_large_text = true;
    }
    let summary = if stored_large_text {
        compact_tool_result_catalog(ctx.tool_call_id, &content_parts)
    } else {
        output.summary.clone()
    };
    Ok(ToolOutput {
        content_parts,
        summary,
        line_selection: output.line_selection.clone(),
        byte_range: output.byte_range.clone(),
    })
}

pub fn tool_result_payload_json(tool_call_id: &str, output: &ToolOutput) -> Result<String> {
    serde_json::to_string(&json!({
        "tool_call_id": tool_call_id,
        "tool_output": tool_output_json(output),
    }))
    .map_err(Into::into)
}

pub fn parse_tool_result_payload_json(
    payload_json: &str,
    part_object: Option<&data_proto::ObjectRef>,
    part_content: &str,
) -> Result<Option<ToolResultPayload>> {
    let payload = serde_json::from_str::<Value>(payload_json).unwrap_or(Value::Null);
    let Some(tool_call_id) = payload.get("tool_call_id").and_then(Value::as_str) else {
        return Ok(None);
    };
    if tool_call_id.is_empty() {
        return Ok(None);
    }
    if let Some(tool_output) = payload.get("tool_output") {
        return Ok(Some(ToolResultPayload {
            tool_call_id: tool_call_id.to_string(),
            tool_output: parse_tool_output_json(tool_output)?,
        }));
    }
    Ok(Some(ToolResultPayload {
        tool_call_id: tool_call_id.to_string(),
        tool_output: legacy_tool_output(&payload, part_object, part_content)?,
    }))
}

pub fn tool_output_json(output: &ToolOutput) -> Value {
    json!({
        "summary": output.summary,
        "content_parts": output.content_parts.iter().map(content_part_json).collect::<Vec<_>>(),
        "line_selection": output.line_selection.as_ref().map(|selection| json!({
            "start_line": selection.start_line,
            "end_line": selection.end_line,
        })),
        "byte_range": output.byte_range.as_ref().map(|range| json!({
            "start": range.start,
            "end": range.end,
            "next_byte": range.next_byte,
        })),
    })
}

pub fn text_from_payload_json(payload_json: &str) -> Option<String> {
    parse_tool_result_payload_json(payload_json, None, "")
        .ok()
        .flatten()
        .and_then(|payload| plain_text(&payload.tool_output))
}

fn parse_tool_output_json(value: &Value) -> Result<ToolOutput> {
    let summary = value
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let content_parts = value
        .get("content_parts")
        .or_else(|| value.get("contentParts"))
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .map(parse_content_part_json)
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(ToolOutput {
        content_parts,
        summary,
        line_selection: value
            .get("line_selection")
            .or_else(|| value.get("lineSelection"))
            .and_then(Value::as_object)
            .map(|selection| ToolOutputLineSelection {
                start_line: selection
                    .get("start_line")
                    .or_else(|| selection.get("startLine"))
                    .and_then(Value::as_u64)
                    .unwrap_or_default(),
                end_line: selection
                    .get("end_line")
                    .or_else(|| selection.get("endLine"))
                    .and_then(Value::as_u64)
                    .unwrap_or_default(),
            }),
        byte_range: value
            .get("byte_range")
            .or_else(|| value.get("byteRange"))
            .and_then(Value::as_object)
            .map(|range| ToolOutputByteRange {
                start: range
                    .get("start")
                    .and_then(Value::as_u64)
                    .unwrap_or_default(),
                end: range.get("end").and_then(Value::as_u64).unwrap_or_default(),
                next_byte: range
                    .get("next_byte")
                    .or_else(|| range.get("nextByte"))
                    .and_then(Value::as_u64),
            }),
    })
}

fn legacy_tool_output(
    payload: &Value,
    part_object: Option<&data_proto::ObjectRef>,
    part_content: &str,
) -> Result<ToolOutput> {
    let inline_output = payload
        .get("output")
        .and_then(Value::as_str)
        .or_else(|| payload.get("output_preview").and_then(Value::as_str))
        .unwrap_or(part_content);
    if let Some(object_ref) = part_object {
        let mut content_parts = Vec::new();
        if !inline_output.is_empty() {
            content_parts.push(text_part(inline_output.to_string()));
        }
        content_parts.push(object_ref_part(object_ref.clone()));
        return Ok(ToolOutput {
            content_parts,
            summary: inline_output.to_string(),
            line_selection: None,
            byte_range: None,
        });
    }
    Ok(ToolOutput::text(inline_output.to_string()))
}

fn content_part_json(part: &ChatContentPart) -> Value {
    match part.content.as_ref() {
        Some(chat_content_part::Content::Text(text)) => json!({
            "type": "text",
            "text": text,
        }),
        Some(chat_content_part::Content::ObjectRef(object_ref)) => json!({
            "type": "object_ref",
            "object_ref": object_ref_json(object_ref),
        }),
        None => json!({
            "type": "empty",
        }),
    }
}

fn parse_content_part_json(value: &Value) -> Result<ChatContentPart> {
    match value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "text" => Ok(text_part(
            value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        )),
        "object_ref" => {
            let object_ref = value
                .get("object_ref")
                .or_else(|| value.get("objectRef"))
                .ok_or_else(|| anyhow!("object_ref content part is missing object_ref"))?;
            Ok(object_ref_part(parse_object_ref_json(object_ref)?))
        }
        "empty" | "" => Ok(ChatContentPart { content: None }),
        _ => Ok(ChatContentPart { content: None }),
    }
}

fn object_ref_json(object_ref: &data_proto::ObjectRef) -> Value {
    json!({
        "key": object_ref.key,
        "media_type": object_ref.media_type,
        "size_bytes": object_ref.size_bytes,
        "sha256": object_ref.sha256,
        "filename": object_ref.filename,
        "metadata": object_ref.metadata,
        "content_encoding": object_ref.content_encoding,
    })
}

fn parse_object_ref_json(value: &Value) -> Result<data_proto::ObjectRef> {
    #[derive(Serialize, Deserialize)]
    struct ObjectRefJson {
        #[serde(default)]
        key: String,
        #[serde(default, alias = "mediaType")]
        media_type: String,
        #[serde(default, alias = "sizeBytes")]
        size_bytes: u64,
        #[serde(default)]
        sha256: String,
        #[serde(default)]
        filename: String,
        #[serde(default)]
        metadata: HashMap<String, String>,
        #[serde(default, alias = "contentEncoding")]
        content_encoding: String,
    }
    let parsed: ObjectRefJson = serde_json::from_value(value.clone())?;
    Ok(data_proto::ObjectRef {
        key: parsed.key,
        media_type: parsed.media_type,
        size_bytes: parsed.size_bytes,
        sha256: parsed.sha256,
        filename: parsed.filename,
        metadata: parsed.metadata,
        content_encoding: parsed.content_encoding,
    })
}

pub(crate) fn object_ref_summary(media_type: &str, filename: &str, size_bytes: u64) -> String {
    let normalized_media_type = media_type.trim().to_ascii_lowercase();
    let label = if normalized_media_type.starts_with("image/") {
        "Image"
    } else if normalized_media_type.starts_with("video/") {
        "Video"
    } else {
        "Object"
    };
    let display_filename = if filename.trim().is_empty() {
        "unnamed"
    } else {
        filename
    };
    format!(
        "[{label}: {display_filename} ({}; {} bytes)]",
        media_type, size_bytes
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::object_store::{InMemoryObjectStore, ObjectStore};
    use std::sync::Arc;

    fn object_ref(key: &str, media_type: &str) -> data_proto::ObjectRef {
        data_proto::ObjectRef {
            key: key.to_string(),
            media_type: media_type.to_string(),
            size_bytes: 12,
            sha256: "abc123".to_string(),
            filename: "asset.bin".to_string(),
            metadata: HashMap::new(),
            content_encoding: String::new(),
        }
    }

    #[test]
    fn text_output_payload_round_trips() {
        let output = ToolOutput::text("result");
        let json = tool_result_payload_json("call-1", &output).unwrap();
        let payload = parse_tool_result_payload_json(&json, None, "")
            .unwrap()
            .unwrap();

        assert_eq!(payload.tool_call_id, "call-1");
        assert_eq!(plain_text(&payload.tool_output).as_deref(), Some("result"));
    }

    #[test]
    fn byte_range_payload_round_trips_without_catalog_metadata() {
        let output = ToolOutput {
            content_parts: vec![object_ref_part(object_ref("cas/text", "text/plain"))],
            summary: "Read bytes 3..8.".to_string(),
            line_selection: None,
            byte_range: Some(ToolOutputByteRange {
                start: 3,
                end: 8,
                next_byte: Some(8),
            }),
        };
        let json = tool_result_payload_json("call-1", &output).unwrap();
        let payload = parse_tool_result_payload_json(&json, None, "")
            .unwrap()
            .unwrap();

        assert_eq!(
            payload.tool_output.byte_range,
            Some(ToolOutputByteRange {
                start: 3,
                end: 8,
                next_byte: Some(8),
            })
        );
        assert!(!json.contains("section_readable"));
        assert!(!json.contains("captured_size_bytes"));
    }

    #[test]
    fn object_ref_payload_round_trips() {
        let output = ToolOutput::from_content_parts(
            vec![object_ref_part(object_ref("cas/image", "image/png"))],
            "",
        );
        let json = tool_result_payload_json("call-1", &output).unwrap();
        let payload = parse_tool_result_payload_json(&json, None, "")
            .unwrap()
            .unwrap();

        assert_eq!(
            first_object_ref(&payload.tool_output).map(|object| object.key.as_str()),
            Some("cas/image")
        );
        assert_eq!(payload.tool_output.content_parts.len(), 1);
    }

    #[test]
    fn mixed_content_parts_round_trip_without_flattening() {
        let output = ToolOutput::from_content_parts(
            vec![
                text_part("caption"),
                object_ref_part(object_ref("cas/image", "image/png")),
                object_ref_part(object_ref("cas/video", "video/mp4")),
            ],
            "caption",
        );
        let json = tool_result_payload_json("call-1", &output).unwrap();
        let payload = parse_tool_result_payload_json(&json, None, "")
            .unwrap()
            .unwrap();

        assert_eq!(payload.tool_output.content_parts.len(), 3);
        assert!(plain_text(&payload.tool_output).is_none());
    }

    #[test]
    fn legacy_output_payload_decodes() {
        let payload = parse_tool_result_payload_json(
            r#"{"tool_call_id":"call-1","output":"legacy"}"#,
            None,
            "",
        )
        .unwrap()
        .unwrap();

        assert_eq!(plain_text(&payload.tool_output).as_deref(), Some("legacy"));
    }

    #[test]
    fn legacy_object_payload_decodes_with_part_object() {
        let object = object_ref("cas/object", "image/png");
        let payload = parse_tool_result_payload_json(
            r#"{"tool_call_id":"call-1","output_object_key":"cas/object"}"#,
            Some(&object),
            "",
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            first_object_ref(&payload.tool_output).map(|object| object.key.as_str()),
            Some("cas/object")
        );
    }

    #[test]
    fn media_type_parameters_are_ignored_for_text_classification() {
        assert!(is_text_object_media_type("application/json; charset=utf-8"));
        assert!(is_text_object_media_type(
            " application/vnd.test+json ; charset=utf-8"
        ));
        assert!(is_text_object_media_type("text/plain; charset=utf-8"));
        assert!(!is_text_object_media_type("image/png; charset=binary"));
    }

    #[test]
    fn source_text_object_summary_is_bounded_for_large_content() {
        let content = "x".repeat(TOOL_RESULT_OBJECT_THRESHOLD_BYTES);
        let output = ToolOutput::from_source_object(
            content.into_bytes(),
            "text/plain; charset=utf-8",
            "large.txt",
            object_ref("cas/large-text", "text/plain; charset=utf-8"),
        );

        assert_eq!(
            output.summary(),
            format!(
                "[Object: large.txt (text/plain; charset=utf-8; {} bytes)]",
                TOOL_RESULT_OBJECT_THRESHOLD_BYTES
            )
        );
        assert_eq!(
            first_object_ref(&output).map(|object| object.key.as_str()),
            Some("cas/large-text")
        );
    }

    #[test]
    fn unknown_content_part_types_decode_as_empty_parts() {
        let payload = parse_tool_result_payload_json(
            r#"{"tool_call_id":"call-1","tool_output":{"summary":"","content_parts":[{"type":"future","value":"x"}]}}"#,
            None,
            "",
        )
        .unwrap()
        .unwrap();

        assert_eq!(payload.tool_output.content_parts.len(), 1);
        assert!(payload.tool_output.content_parts[0].content.is_none());
    }

    #[tokio::test]
    async fn large_text_normalization_stores_text_once_in_cas() {
        let store = Arc::new(InMemoryObjectStore::default());
        let cas = CasStore::new(store.clone());
        let output = ToolOutput::text("x".repeat(TOOL_RESULT_OBJECT_THRESHOLD_BYTES));
        let normalized = normalize_for_session_storage(
            &cas,
            ToolOutputStorageContext {
                ns: "ns",
                agent: "agent",
                session_id: "session",
                message_id: "message",
                part_id: "part",
                tool_call_id: "call",
                tool_name: "tool",
            },
            &output,
        )
        .await
        .unwrap();

        let object = first_object_ref(&normalized).unwrap();
        assert!(object.key.contains("/messages/message/part.txt"));
        assert!(store.get(&object.key).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn large_text_normalization_replaces_large_summary() {
        let store = Arc::new(InMemoryObjectStore::default());
        let cas = CasStore::new(store);
        let content = "x".repeat(TOOL_RESULT_OBJECT_THRESHOLD_BYTES);
        let output = ToolOutput::text(content);
        let normalized = normalize_for_session_storage(
            &cas,
            ToolOutputStorageContext {
                ns: "ns",
                agent: "agent",
                session_id: "session",
                message_id: "message",
                part_id: "part",
                tool_call_id: "call",
                tool_name: "tool",
            },
            &output,
        )
        .await
        .unwrap();

        assert!(normalized.summary.len() < TOOL_RESULT_OBJECT_THRESHOLD_BYTES);
        assert!(normalized.summary.starts_with("Tool result catalog tr://call:"));
        assert!(normalized.summary.contains("parts/0: text/plain"));
        let payload = tool_result_payload_json("call", &normalized).unwrap();
        assert!(payload.len() < TOOL_RESULT_OBJECT_THRESHOLD_BYTES);
        assert!(!payload.contains(&"x".repeat(128)));
    }

    #[tokio::test]
    async fn object_ref_normalization_does_not_store_again() {
        let store = Arc::new(InMemoryObjectStore::default());
        let cas = CasStore::new(store.clone());
        let output = ToolOutput::from_content_parts(
            vec![object_ref_part(object_ref("cas/existing", "image/png"))],
            "",
        );
        let normalized = normalize_for_session_storage(
            &cas,
            ToolOutputStorageContext {
                ns: "ns",
                agent: "agent",
                session_id: "session",
                message_id: "message",
                part_id: "part",
                tool_call_id: "call",
                tool_name: "tool",
            },
            &output,
        )
        .await
        .unwrap();

        assert_eq!(
            first_object_ref(&normalized).map(|object| object.key.as_str()),
            Some("cas/existing")
        );
        assert!(store.get("cas/existing").await.unwrap().is_none());
    }
}
