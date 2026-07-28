// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Shared harness runtime data shapes.
//!
//! These types sit between tool execution, the executor loop, session replay,
//! and durable sink/journal persistence. Keep provider- or tool-specific
//! behavior in those modules; this module owns the common output contract.

use crate::gateway::rpc::data_proto;
use crate::harness::llm::{chat_content_part, object_ref_part, text_part, ChatContentPart};

#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    content_parts: Vec<ChatContentPart>,
    summary: String,
}

pub(crate) fn is_text_object_media_type(media_type: &str) -> bool {
    let media_type = media_type.trim().to_ascii_lowercase();
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

impl ToolOutput {
    pub fn text(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            content_parts: vec![text_part(text.clone())],
            summary: text,
        }
    }

    pub fn from_source_object(
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
        let summary = if is_text_object_media_type(&media_type) {
            String::from_utf8_lossy(&bytes).to_string()
        } else {
            let label = if media_type.trim().to_ascii_lowercase().starts_with("image/") {
                "Image"
            } else if media_type.trim().to_ascii_lowercase().starts_with("video/") {
                "Video"
            } else {
                "Object"
            };
            let display_filename = if filename.trim().is_empty() {
                "unnamed".to_string()
            } else {
                filename
            };
            format!(
                "[{label}: {display_filename} ({}; {} bytes)]",
                media_type,
                bytes.len()
            )
        };
        Self {
            content_parts: vec![object_ref_part(object_ref)],
            summary,
        }
    }

    pub fn from_content_parts(
        content_parts: Vec<ChatContentPart>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            content_parts,
            summary: summary.into(),
        }
    }

    pub fn summary(&self) -> String {
        self.summary.clone()
    }

    pub fn serialized_output(&self) -> String {
        if let Some(text) = self.inline_summary() {
            return text.to_string();
        }

        let content_parts = self
            .content_parts
            .iter()
            .map(serialized_content_part)
            .collect::<Vec<_>>();
        serde_json::to_string(&serde_json::json!({
            "summary": self.summary,
            "contentParts": content_parts,
        }))
        .unwrap_or_else(|_| self.summary.clone())
    }

    pub fn content_parts(&self) -> Vec<ChatContentPart> {
        self.content_parts.clone()
    }

    pub fn object_ref(&self) -> Option<&data_proto::ObjectRef> {
        self.content_parts
            .iter()
            .find_map(|part| match part.content.as_ref()? {
                chat_content_part::Content::ObjectRef(object_ref) => Some(object_ref),
                _ => None,
            })
    }

    pub fn inline_summary(&self) -> Option<&str> {
        self.content_parts
            .iter()
            .all(|part| {
                matches!(
                    part.content.as_ref(),
                    Some(chat_content_part::Content::Text(_))
                )
            })
            .then_some(self.summary.as_str())
    }
}

fn serialized_content_part(part: &ChatContentPart) -> serde_json::Value {
    match part.content.as_ref() {
        Some(chat_content_part::Content::Text(text)) => serde_json::json!({
            "type": "text",
            "text": text,
        }),
        Some(chat_content_part::Content::ObjectRef(object_ref)) => serde_json::json!({
            "type": "object_ref",
            "objectRef": {
                "key": object_ref.key.as_str(),
                "mediaType": object_ref.media_type.as_str(),
                "sizeBytes": object_ref.size_bytes,
                "sha256": object_ref.sha256.as_str(),
                "filename": object_ref.filename.as_str(),
                "metadata": &object_ref.metadata,
                "contentEncoding": object_ref.content_encoding.as_str(),
            },
        }),
        None => serde_json::json!({
            "type": "empty",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::ToolOutput;
    use crate::gateway::rpc::data_proto;
    use crate::harness::llm::object_ref_part;
    use serde_json::Value;
    use std::collections::HashMap;

    #[test]
    fn serialized_output_preserves_plain_text_outputs() {
        assert_eq!(ToolOutput::text("result").serialized_output(), "result");
    }

    #[test]
    fn empty_text_output_keeps_empty_text_part() {
        let output = ToolOutput::text("");

        assert_eq!(
            output.content_parts(),
            vec![crate::harness::llm::text_part("")]
        );
        assert_eq!(output.serialized_output(), "");
    }

    #[test]
    fn serialized_output_includes_object_ref_when_summary_is_empty() {
        let output = ToolOutput::from_content_parts(
            vec![object_ref_part(data_proto::ObjectRef {
                key: "cas/test-image".to_string(),
                media_type: "image/png".to_string(),
                size_bytes: 12,
                sha256: "abc123".to_string(),
                filename: "image.png".to_string(),
                metadata: HashMap::new(),
                content_encoding: String::new(),
            })],
            "",
        );

        let value: Value = serde_json::from_str(&output.serialized_output()).unwrap();
        assert_eq!(value["summary"], "");
        assert_eq!(value["contentParts"][0]["type"], "object_ref");
        assert_eq!(
            value["contentParts"][0]["objectRef"]["key"],
            "cas/test-image"
        );
        assert_eq!(
            value["contentParts"][0]["objectRef"]["mediaType"],
            "image/png"
        );
    }
}
