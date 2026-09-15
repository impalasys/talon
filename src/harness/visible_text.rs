// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! The single authoritative implementation of part-level text views.
//!
//! There are three coordinate systems. Source coordinates address the original
//! inline string or decoded text object. Visible coordinates address that
//! part's `byte_range` view. Root coordinates address the concatenation of
//! visible text parts in original part order, with no delimiters; non-text and
//! unset parts contribute no bytes. A root selection returns selection-local
//! part indexes in that ordered result.
//!
//! A range is half-open and byte-denominated, but both endpoints must be UTF-8
//! character boundaries. Exact empty ranges are valid at every root/part
//! boundary. Bounded reads make progress by backing their end up to a character
//! boundary; a cap too small for the first character is rejected. The
//! `ToolOutput.byte_range` receipt is deliberately not accepted by this module.

use crate::control::cas::CasStore;
use crate::control::object_store::ObjectMetadata;
use crate::gateway::rpc::data_proto;
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
    fn is_text(&self) -> bool {
        matches!(self, Self::Inline | Self::Object { .. })
    }
}

/// Resolves a part's text policy. A nonblank reference media type wins over a
/// disagreeing HEAD response. A blank reference must be classified by HEAD;
/// no byte sniffing or silent skip is permitted for a selection-bearing view.
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

/// Validates a part view against its decoded source text. `source` is required
/// for text parts, even object parts, so corruption and UTF-8 boundary errors
/// are reported before a selection is planned.
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
    let full = 0..source.len() as u64;
    let visible = part
        .byte_range
        .as_ref()
        .map(|range| range.start..range.end)
        .unwrap_or(full);
    validate_source_range(source, &visible)?;
    Ok(Some(visible))
}

/// Validates the portion of a persisted part view that is knowable without
/// reading an object. Inline text is checked completely; object text ranges
/// are checked when their decoded source is loaded by the reader.
pub fn validate_persisted_part(part: &ChatContentPart) -> Result<()> {
    match part.content.as_ref() {
        Some(chat_content_part::Content::Text(text)) => {
            validate_visible_part(part, &TextEligibility::Inline, Some(text), None)?;
        }
        Some(chat_content_part::Content::ObjectRef(_)) => {
            if part.byte_range.is_none() {
                return Ok(());
            }
            let eligibility = resolve_text_eligibility(part, None)?;
            if !eligibility.is_text() && part.byte_range.is_some() {
                bail!("byte_range is valid only for text content parts");
            }
        }
        None if part.byte_range.is_some() => {
            bail!("byte_range is valid only for text content parts");
        }
        None => {}
    }
    Ok(())
}

/// Returns the exact inline view selected by a content part. Callers that
/// project text use this instead of the source field so a part range can never
/// accidentally expose its prefix or suffix.
pub fn visible_inline_text(part: &ChatContentPart) -> Result<Option<&str>> {
    let Some(chat_content_part::Content::Text(source)) = part.content.as_ref() else {
        validate_persisted_part(part)?;
        return Ok(None);
    };
    let range = validate_visible_part(part, &TextEligibility::Inline, Some(source), None)?
        .expect("inline text always has a visible range");
    Ok(Some(&source[range.start as usize..range.end as usize]))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedVisibleSource {
    pub eligibility: TextEligibility,
    /// Decoded source text. Text object readers must return byte-exact UTF-8,
    /// never a lossy replacement string.
    pub text: Option<String>,
    /// The verified decoded size for an object. `ObjectRef.size_bytes` may be
    /// a stored/compressed size and is never used as a text-range bound.
    pub logical_size_bytes: Option<u64>,
    /// A canonicalized copy of the source part. In particular, a blank object
    /// media type is filled from the HEAD result once, before it can enter a
    /// selected view.
    pub canonical_part: ChatContentPart,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VisibleTextPart {
    pub source_part_index: usize,
    pub source: String,
    pub source_range: Range<u64>,
    pub root_range: Range<u64>,
    pub original_part: ChatContentPart,
}

impl VisibleTextPart {
    pub fn visible_len(&self) -> u64 {
        self.source_range.end - self.source_range.start
    }

    pub fn visible_text(&self) -> &str {
        &self.source[self.source_range.start as usize..self.source_range.end as usize]
    }

    fn with_source_range(mut self, source_range: Range<u64>) -> Self {
        self.source_range = source_range;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct VisibleTextPlan {
    /// Ordered text parts only. Its indexes are selection-local after a root
    /// selection, while `source_part_index` remains diagnostic provenance.
    pub parts: Vec<VisibleTextPart>,
    pub root_len: u64,
}

impl VisibleTextPlan {
    pub fn visible_parts(&self) -> Vec<ChatContentPart> {
        self.parts
            .iter()
            .map(|part| {
                let mut output = part.original_part.clone();
                output.byte_range = Some(ChatContentPartByteRange {
                    start: part.source_range.start,
                    end: part.source_range.end,
                });
                output
            })
            .collect()
    }
}

/// Builds the no-delimiter root segment table from visible part intervals.
/// `resolve` is the one place object callers provide metadata and decoded
/// source text; all later selection and materialization uses this plan.
pub fn plan_visible_text_with_resolver<F>(
    parts: &[ChatContentPart],
    mut resolve: F,
) -> Result<VisibleTextPlan>
where
    F: FnMut(usize, &ChatContentPart) -> Result<ResolvedVisibleSource>,
{
    let mut planned = Vec::new();
    let mut root_len = 0_u64;
    for (source_part_index, part) in parts.iter().enumerate() {
        let resolved = resolve(source_part_index, part)?;
        let Some(source_range) = validate_visible_part(
            &resolved.canonical_part,
            &resolved.eligibility,
            resolved.text.as_deref(),
            resolved.logical_size_bytes,
        )?
        else {
            continue;
        };
        let source = resolved.text.expect("validated text source");
        let visible_len = source_range.end - source_range.start;
        let root_range = root_len..root_len + visible_len;
        root_len = root_range.end;
        planned.push(VisibleTextPart {
            source_part_index,
            source,
            source_range,
            root_range,
            original_part: resolved.canonical_part,
        });
    }
    Ok(VisibleTextPlan {
        parts: planned,
        root_len,
    })
}

/// Resolves one source through the only supported object path. HEAD happens
/// only for blank media types; decode is strict UTF-8 and establishes the
/// logical size used by range validation. A missing or corrupt selected object
/// fails before a plan exists, so global root offsets cannot silently change.
pub async fn resolve_visible_part(
    cas: &CasStore,
    part: &ChatContentPart,
) -> Result<ResolvedVisibleSource> {
    let metadata = match part.content.as_ref() {
        Some(chat_content_part::Content::ObjectRef(object_ref))
            if object_ref.media_type.trim().is_empty() =>
        {
            cas.object_store()
                .head(&object_ref.key)
                .await?
                .ok_or_else(|| anyhow!("text eligibility is unavailable: object is missing"))?
        }
        _ => ObjectMetadata::default(),
    };
    let eligibility = resolve_text_eligibility(part, Some(&metadata))?;
    let mut canonical_part = part.clone();
    if let (
        Some(chat_content_part::Content::ObjectRef(original)),
        Some(chat_content_part::Content::ObjectRef(canonical)),
        TextEligibility::Object { media_type } | TextEligibility::NonText { media_type },
    ) = (
        part.content.as_ref(),
        canonical_part.content.as_mut(),
        &eligibility,
    ) {
        if original.media_type.trim().is_empty() {
            canonical.media_type = media_type.clone();
        }
    }
    match canonical_part.content.as_ref() {
        Some(chat_content_part::Content::Text(text)) => Ok(ResolvedVisibleSource {
            eligibility,
            text: Some(text.clone()),
            logical_size_bytes: None,
            canonical_part,
        }),
        Some(chat_content_part::Content::ObjectRef(object_ref))
            if matches!(eligibility, TextEligibility::Object { .. }) =>
        {
            let stored = cas
                .get_object_decoded(&object_ref.key)
                .await?
                .ok_or_else(|| anyhow!("selected text object is unavailable"))?;
            let text = String::from_utf8(stored.bytes)
                .map_err(|_| anyhow!("selected text object is not valid UTF-8"))?;
            let logical_size_bytes = stored.metadata.size_bytes;
            Ok(ResolvedVisibleSource {
                eligibility,
                text: Some(text),
                logical_size_bytes: Some(logical_size_bytes),
                canonical_part,
            })
        }
        _ => Ok(ResolvedVisibleSource {
            eligibility,
            text: None,
            logical_size_bytes: None,
            canonical_part,
        }),
    }
}

/// The authoritative production planner. Consumers supply only a CAS handle;
/// they cannot choose media classification, source bytes, or decode policy.
pub async fn plan_visible_text(
    cas: &CasStore,
    parts: &[ChatContentPart],
) -> Result<VisibleTextPlan> {
    let mut resolved = Vec::with_capacity(parts.len());
    for part in parts {
        resolved.push(resolve_visible_part(cas, part).await?);
    }
    plan_visible_text_with_resolver(parts, |index, _| Ok(resolved[index].clone()))
}

/// Composes a child visible-relative interval onto a parent source interval.
/// This is the only valid route from a nested part request to object storage.
pub fn compose_visible_range(part: &VisibleTextPart, child: Range<u64>) -> Result<Range<u64>> {
    if child.start > child.end || child.end > part.visible_len() {
        bail!("visible byte range is outside the selected part");
    }
    let composed = part.source_range.start + child.start..part.source_range.start + child.end;
    validate_source_range(&part.source, &composed)?;
    Ok(composed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleRangeRequest {
    Exact { start: u64, end: u64 },
    Bounded { start: u64, max_size: u64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct VisibleRangeSelection {
    pub plan: VisibleTextPlan,
    pub actual: Range<u64>,
    pub next_byte: Option<u64>,
}

/// Applies root coordinates to a visible plan. Zero-intersection parts are
/// omitted, making output part indexes selection-local.
pub fn select_visible_range(
    plan: &VisibleTextPlan,
    request: VisibleRangeRequest,
) -> Result<VisibleRangeSelection> {
    let actual = match request {
        VisibleRangeRequest::Exact { start, end } => {
            if end < start || end > plan.root_len || end - start > MAX_VISIBLE_TEXT_BYTES {
                bail!("exact byte range is outside the visible text stream");
            }
            if !is_root_boundary(plan, start) || !is_root_boundary(plan, end) {
                bail!("byte range endpoint is not a UTF-8 character boundary");
            }
            start..end
        }
        VisibleRangeRequest::Bounded { start, max_size } => {
            if !(1..=MAX_VISIBLE_TEXT_BYTES).contains(&max_size) || start > plan.root_len {
                bail!("bounded byte range is invalid");
            }
            if !is_root_boundary(plan, start) {
                bail!("byte range start is not a UTF-8 character boundary");
            }
            let candidate = start.saturating_add(max_size).min(plan.root_len);
            let end = preceding_root_boundary(plan, candidate);
            if end == start && start < plan.root_len {
                bail!("bounded byte range cannot include the first UTF-8 character");
            }
            start..end
        }
    };
    let next_byte = (actual.end < plan.root_len).then_some(actual.end);
    let mut selected = Vec::new();
    for part in &plan.parts {
        let start = actual.start.max(part.root_range.start);
        let end = actual.end.min(part.root_range.end);
        if start == end {
            continue;
        }
        let child = start - part.root_range.start..end - part.root_range.start;
        let source_range = compose_visible_range(part, child)?;
        let mut selected_part = part.clone().with_source_range(source_range);
        selected_part.root_range = start - actual.start..end - actual.start;
        selected.push(selected_part);
    }
    Ok(VisibleRangeSelection {
        plan: VisibleTextPlan {
            parts: selected,
            root_len: actual.end - actual.start,
        },
        actual,
        next_byte,
    })
}

/// Materializes exactly the planned view. Provider/UI callers use the returned
/// bytes as one root payload, never the original parts or `ToolOutput` receipt.
pub fn materialize_visible_parts(plan: &VisibleTextPlan) -> String {
    let mut result = String::with_capacity(plan.root_len as usize);
    for part in &plan.parts {
        result.push_str(part.visible_text());
    }
    result
}

/// Validates a deprecated root page receipt without allowing it to influence a
/// selected view. The journaled request owns its source reference and exact
/// coordinate kind, so this payload can only record the actual page outcome.
pub fn validate_page_receipt(receipt: &crate::harness::llm::ToolOutputByteRange) -> Result<()> {
    if receipt.actual_end < receipt.requested_start {
        bail!("page receipt ends before its requested start");
    }
    if let Some(next_byte) = receipt.next_byte {
        if next_byte != receipt.actual_end {
            bail!("page receipt next_byte must equal actual_end");
        }
    }
    Ok(())
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

fn is_root_boundary(plan: &VisibleTextPlan, offset: u64) -> bool {
    if offset > plan.root_len {
        return false;
    }
    if offset == 0 || offset == plan.root_len {
        return true;
    }
    plan.parts.iter().any(|part| {
        offset == part.root_range.start
            || offset == part.root_range.end
            || (offset > part.root_range.start
                && offset < part.root_range.end
                && part
                    .visible_text()
                    .is_char_boundary((offset - part.root_range.start) as usize))
    })
}

fn preceding_root_boundary(plan: &VisibleTextPlan, candidate: u64) -> u64 {
    let mut offset = candidate;
    while offset > 0 && !is_root_boundary(plan, offset) {
        offset -= 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::object_store::{InMemoryObjectStore, ObjectStore};
    use crate::harness::llm::{object_ref_part, text_part};
    use std::sync::Arc;

    fn resolve_inline_or_object(
        _index: usize,
        part: &ChatContentPart,
    ) -> Result<ResolvedVisibleSource> {
        let eligibility = resolve_text_eligibility(part, None)?;
        let text = match part.content.as_ref() {
            Some(chat_content_part::Content::Text(text)) => Some(text.clone()),
            Some(chat_content_part::Content::ObjectRef(object))
                if matches!(eligibility, TextEligibility::Object { .. }) =>
            {
                Some(
                    object
                        .metadata
                        .get("test_text")
                        .cloned()
                        .unwrap_or_default(),
                )
            }
            _ => None,
        };
        let logical_size_bytes = match part.content.as_ref() {
            Some(chat_content_part::Content::ObjectRef(_)) if text.is_some() => {
                Some(text.as_ref().expect("text source").len() as u64)
            }
            _ => None,
        };
        Ok(ResolvedVisibleSource {
            eligibility,
            text,
            logical_size_bytes,
            canonical_part: part.clone(),
        })
    }

    fn ranged_text(text: &str, start: u64, end: u64) -> ChatContentPart {
        let mut part = text_part(text);
        part.byte_range = Some(ChatContentPartByteRange { start, end });
        part
    }

    #[test]
    fn composes_nested_visible_ranges_for_inline_and_object_parts() {
        let parent = ranged_text("0123456789", 3, 8);
        let plan = plan_visible_text_with_resolver(&[parent], resolve_inline_or_object).unwrap();
        assert_eq!(compose_visible_range(&plan.parts[0], 1..4).unwrap(), 4..7);

        let mut object = data_proto::ObjectRef {
            media_type: "text/plain".to_string(),
            size_bytes: 10,
            ..Default::default()
        };
        object
            .metadata
            .insert("test_text".to_string(), "0123456789".to_string());
        let mut object_part = object_ref_part(object);
        object_part.byte_range = Some(ChatContentPartByteRange { start: 3, end: 8 });
        let object_plan =
            plan_visible_text_with_resolver(&[object_part], resolve_inline_or_object).unwrap();
        assert_eq!(
            compose_visible_range(&object_plan.parts[0], 1..4).unwrap(),
            4..7
        );
    }

    #[test]
    fn root_range_crosses_parts_without_delimiters_and_is_selection_local() {
        let image = object_ref_part(data_proto::ObjectRef {
            media_type: "image/png".to_string(),
            ..Default::default()
        });
        let plan = plan_visible_text_with_resolver(
            &[text_part("AéB"), image, text_part("XY")],
            resolve_inline_or_object,
        )
        .unwrap();
        let selected =
            select_visible_range(&plan, VisibleRangeRequest::Exact { start: 1, end: 6 }).unwrap();
        assert_eq!(materialize_visible_parts(&selected.plan), "éBXY");
        assert_eq!(selected.plan.parts.len(), 2);
        assert_eq!(selected.plan.parts[0].source_part_index, 0);
        assert_eq!(selected.plan.parts[1].source_part_index, 2);
        assert_eq!(selected.plan.parts[0].source_range, 1..4);
        assert_eq!(selected.plan.parts[1].source_range, 0..2);

        let nested = select_visible_range(
            &selected.plan,
            VisibleRangeRequest::Exact { start: 3, end: 5 },
        )
        .unwrap();
        assert_eq!(materialize_visible_parts(&nested.plan), "XY");
    }

    #[test]
    fn utf8_empty_and_bounded_progress_contracts_are_total() {
        let plan =
            plan_visible_text_with_resolver(&[text_part("é🙂")], resolve_inline_or_object).unwrap();
        assert!(
            select_visible_range(&plan, VisibleRangeRequest::Exact { start: 1, end: 2 }).is_err()
        );
        assert!(select_visible_range(
            &plan,
            VisibleRangeRequest::Bounded {
                start: 0,
                max_size: 1
            }
        )
        .is_err());
        let exact =
            select_visible_range(&plan, VisibleRangeRequest::Exact { start: 2, end: 2 }).unwrap();
        assert_eq!(materialize_visible_parts(&exact.plan), "");
        assert_eq!(exact.next_byte, Some(2));
        let end = select_visible_range(
            &plan,
            VisibleRangeRequest::Bounded {
                start: plan.root_len,
                max_size: 1,
            },
        )
        .unwrap();
        assert_eq!(end.actual, plan.root_len..plan.root_len);
        assert_eq!(end.next_byte, None);
    }

    #[test]
    fn rejects_ranged_non_text_and_requires_head_for_blank_objects() {
        let mut image = object_ref_part(data_proto::ObjectRef {
            media_type: "image/png".to_string(),
            ..Default::default()
        });
        image.byte_range = Some(ChatContentPartByteRange { start: 0, end: 0 });
        assert!(plan_visible_text_with_resolver(&[image], resolve_inline_or_object).is_err());

        let blank = object_ref_part(data_proto::ObjectRef::default());
        assert!(resolve_text_eligibility(&blank, None).is_err());
        let metadata = ObjectMetadata {
            media_type: "application/problem+json; charset=utf-8".to_string(),
            ..Default::default()
        };
        assert!(matches!(
            resolve_text_eligibility(&blank, Some(&metadata)).unwrap(),
            TextEligibility::Object { .. }
        ));
    }

    #[test]
    fn all_non_text_and_empty_source_only_allow_empty_root_selection() {
        let image = object_ref_part(data_proto::ObjectRef {
            media_type: "image/png".to_string(),
            ..Default::default()
        });
        let plan = plan_visible_text_with_resolver(&[image], resolve_inline_or_object).unwrap();
        assert_eq!(plan.root_len, 0);
        assert!(
            select_visible_range(&plan, VisibleRangeRequest::Exact { start: 0, end: 0 }).is_ok()
        );
        assert!(
            select_visible_range(&plan, VisibleRangeRequest::Exact { start: 0, end: 1 }).is_err()
        );
    }

    #[test]
    fn explicit_media_type_wins_over_conflicting_metadata() {
        let object = object_ref_part(data_proto::ObjectRef {
            media_type: "image/png".to_string(),
            ..Default::default()
        });
        let metadata = ObjectMetadata {
            media_type: "text/plain".to_string(),
            ..Default::default()
        };
        assert!(matches!(
            resolve_text_eligibility(&object, Some(&metadata)).unwrap(),
            TextEligibility::NonText { .. }
        ));
    }

    #[test]
    fn object_views_use_decoded_text_boundaries_not_stored_size() {
        let mut object = data_proto::ObjectRef {
            media_type: "text/plain".to_string(),
            size_bytes: 1,
            ..Default::default()
        };
        object
            .metadata
            .insert("test_text".to_string(), "éBC".to_string());
        let mut part = object_ref_part(object);
        part.byte_range = Some(ChatContentPartByteRange { start: 2, end: 4 });
        let plan = plan_visible_text_with_resolver(&[part], resolve_inline_or_object).unwrap();
        assert_eq!(materialize_visible_parts(&plan), "BC");
    }

    #[test]
    fn recognizes_case_parameters_and_structured_text_media_types() {
        assert!(is_text_media_type("TEXT/PLAIN; charset=utf-8"));
        assert!(is_text_media_type("application/problem+json"));
        assert!(is_text_media_type("application/soap+xml"));
        assert!(!is_text_media_type("application/octet-stream"));
    }

    #[tokio::test]
    async fn production_resolver_heads_blank_media_and_canonicalizes_the_view() {
        let store = Arc::new(InMemoryObjectStore::default());
        let cas = CasStore::new(store.clone());
        store
            .put(
                "cas/text",
                "AéB".as_bytes(),
                ObjectMetadata {
                    media_type: "text/plain; charset=utf-8".to_string(),
                    size_bytes: 0,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let part = object_ref_part(data_proto::ObjectRef {
            key: "cas/text".to_string(),
            ..Default::default()
        });
        let plan = plan_visible_text(&cas, &[part]).await.unwrap();
        assert_eq!(materialize_visible_parts(&plan), "AéB");
        let output = plan.visible_parts();
        let object = match output[0].content.as_ref() {
            Some(chat_content_part::Content::ObjectRef(object)) => object,
            _ => panic!("expected object part"),
        };
        assert_eq!(object.media_type, "text/plain; charset=utf-8");
    }

    #[tokio::test]
    async fn production_resolver_rejects_missing_and_non_utf8_text_objects() {
        let store = Arc::new(InMemoryObjectStore::default());
        let cas = CasStore::new(store.clone());
        let missing = object_ref_part(data_proto::ObjectRef {
            key: "cas/missing".to_string(),
            media_type: "text/plain".to_string(),
            ..Default::default()
        });
        assert!(plan_visible_text(&cas, &[missing]).await.is_err());

        store
            .put(
                "cas/bad-utf8",
                &[0xff],
                ObjectMetadata {
                    media_type: "text/plain".to_string(),
                    size_bytes: 0,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let invalid = object_ref_part(data_proto::ObjectRef {
            key: "cas/bad-utf8".to_string(),
            media_type: "text/plain".to_string(),
            ..Default::default()
        });
        assert!(plan_visible_text(&cas, &[invalid]).await.is_err());
    }
}
