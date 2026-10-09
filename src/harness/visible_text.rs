// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Part-level visible-text policy and UTF-8 range validation.
//!
//! A content part's `byte_range` is a half-open source interval. It is valid
//! only for inline text or text-media ObjectRefs; endpoints are UTF-8 character
//! boundaries. Higher layers compose and materialize these validated views.

use crate::control::object_store::ObjectMetadata;
use crate::harness::llm::{chat_content_part, ChatContentPart, ChatContentPartByteRange};
use anyhow::{anyhow, bail, Result};
use std::ops::Range;

pub const CONTENT_VIEW_VERSION: u32 = 1;
pub const MAX_VISIBLE_TEXT_BYTES: u64 = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextEligibility {
    Inline,
    Object { media_type: String },
    NonText { media_type: String },
    Unset,
}

impl TextEligibility {
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Inline | Self::Object { .. })
    }
}

/// Resolves text eligibility without inspecting payload bytes. A declared,
/// nonblank ObjectRef media type wins over metadata; blank media types require
/// a HEAD result so selection-bearing views cannot silently change shape.
pub fn resolve_text_eligibility(
    part: &ChatContentPart,
    metadata: Option<&ObjectMetadata>,
) -> Result<TextEligibility> {
    match part.content.as_ref() {
        Some(chat_content_part::Content::Text(_)) => Ok(TextEligibility::Inline),
        Some(chat_content_part::Content::ObjectRef(object_ref)) => {
            let media_type = if object_ref.media_type.trim().is_empty() {
                metadata
                    .map(|metadata| metadata.media_type.trim())
                    .filter(|media_type| !media_type.is_empty())
                    .ok_or_else(|| {
                        anyhow!("text eligibility is unavailable: object media type is blank")
                    })?
                    .to_string()
            } else {
                object_ref.media_type.trim().to_string()
            };
            if is_text_media_type(&media_type) {
                Ok(TextEligibility::Object { media_type })
            } else {
                Ok(TextEligibility::NonText { media_type })
            }
        }
        None => Ok(TextEligibility::Unset),
    }
}

/// Validates a part view against decoded text. Object callers must supply the
/// logical decoded size: stored/compressed ObjectRef size is not a range bound.
pub fn validate_visible_part(
    part: &ChatContentPart,
    eligibility: &TextEligibility,
    source: Option<&str>,
    logical_size_bytes: Option<u64>,
) -> Result<Option<Range<u64>>> {
    if !eligibility.is_text() {
        if part.byte_range.is_some() {
            bail!("byte_range is valid only for text content parts");
        }
        return Ok(None);
    }
    let source = source.ok_or_else(|| anyhow!("text source is unavailable"))?;
    if matches!(eligibility, TextEligibility::Object { .. })
        && logical_size_bytes != Some(source.len() as u64)
    {
        bail!("text object logical size does not match decoded source");
    }
    let range = part
        .byte_range
        .as_ref()
        .map(|range| range.start..range.end)
        .unwrap_or(0..source.len() as u64);
    validate_source_range(source, &range)?;
    Ok(Some(range))
}

/// Checks persisted ranges that are knowable without reading an ObjectRef.
pub fn validate_persisted_part(part: &ChatContentPart) -> Result<()> {
    match part.content.as_ref() {
        Some(chat_content_part::Content::Text(text)) => {
            validate_visible_part(part, &TextEligibility::Inline, Some(text), None)?;
        }
        Some(chat_content_part::Content::ObjectRef(_)) if part.byte_range.is_some() => {
            let eligibility = resolve_text_eligibility(part, None)?;
            if !eligibility.is_text() {
                bail!("byte_range is valid only for text content parts");
            }
        }
        None if part.byte_range.is_some() => {
            bail!("byte_range is valid only for text content parts")
        }
        _ => {}
    }
    Ok(())
}

/// Returns exactly the selected inline portion. Every inline projection goes
/// through this function, preventing accidental prefix/suffix disclosure.
pub fn visible_inline_text(part: &ChatContentPart) -> Result<Option<&str>> {
    let Some(chat_content_part::Content::Text(source)) = part.content.as_ref() else {
        validate_persisted_part(part)?;
        return Ok(None);
    };
    let range = validate_visible_part(part, &TextEligibility::Inline, Some(source), None)?
        .expect("inline text always has a visible range");
    Ok(Some(&source[range.start as usize..range.end as usize]))
}

pub fn is_text_media_type(media_type: &str) -> bool {
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

fn validate_source_range(source: &str, range: &Range<u64>) -> Result<()> {
    if range.start > range.end || range.end > source.len() as u64 {
        bail!("byte range is outside the text source");
    }
    if !source.is_char_boundary(range.start as usize)
        || !source.is_char_boundary(range.end as usize)
    {
        bail!("byte range endpoint is not a UTF-8 character boundary");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::rpc::data_proto;
    use crate::harness::llm::{object_ref_part, text_part};

    #[test]
    fn validates_inline_utf8_views_and_rejects_invalid_ranges() {
        let mut part = text_part("AéB");
        part.byte_range = Some(ChatContentPartByteRange { start: 1, end: 3 });
        assert_eq!(visible_inline_text(&part).unwrap(), Some("é"));
        part.byte_range = Some(ChatContentPartByteRange { start: 2, end: 3 });
        assert!(visible_inline_text(&part).is_err());
    }

    #[test]
    fn accepts_text_media_and_rejects_ranged_non_text() {
        let mut image = object_ref_part(data_proto::ObjectRef {
            media_type: "image/png".to_string(),
            ..Default::default()
        });
        image.byte_range = Some(ChatContentPartByteRange { start: 0, end: 0 });
        assert!(validate_persisted_part(&image).is_err());
        let blank = object_ref_part(data_proto::ObjectRef::default());
        let metadata = ObjectMetadata {
            media_type: "application/problem+json; charset=utf-8".to_string(),
            ..Default::default()
        };
        assert!(matches!(
            resolve_text_eligibility(&blank, Some(&metadata)).unwrap(),
            TextEligibility::Object { .. }
        ));
        assert!(is_text_media_type("TEXT/PLAIN; charset=utf-8"));
        assert!(!is_text_media_type("application/octet-stream"));
    }

    #[test]
    fn validates_text_object_against_decoded_logical_size() {
        let mut part = object_ref_part(data_proto::ObjectRef {
            media_type: "text/plain".to_string(),
            size_bytes: 1,
            ..Default::default()
        });
        part.byte_range = Some(ChatContentPartByteRange { start: 0, end: 3 });
        let eligibility = resolve_text_eligibility(&part, None).unwrap();
        assert!(validate_visible_part(&part, &eligibility, Some("Aé"), Some(3)).is_ok());
        assert!(validate_visible_part(&part, &eligibility, Some("Aé"), Some(1)).is_err());
        assert!(validate_visible_part(&part, &eligibility, None, Some(3)).is_err());
    }
}
