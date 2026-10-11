// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Session-local, model-readable handles for prior tool-result parts.

use crate::control::cas::{CasStore, SessionCasScope};
use crate::control::tool_output::{self, ToolOutputExt, ToolResultPayload};
use crate::control::{keys, ControlPlane, ProtoKeyValueStoreExt};
use crate::gateway::rpc::{
    data_proto,
    harness_proto::{chat_content_part, ByteRange, ChatContentPart, ToolOutput},
};
use anyhow::{anyhow, bail, Result};
use prost::Message;
use serde_json::json;
use std::ops::Range;

pub const MAX_TOOL_RESULT_READ_BYTES: u64 = 8 * 1024;
const MAX_CATALOG_BYTES: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolResultHandle {
    pub tool_call_id: String,
    pub part_index: Option<usize>,
}

pub fn tool_result_catalog_uri(tool_call_id: &str) -> String {
    format!("tr://{}", urlencoding::encode(tool_call_id))
}

pub fn tool_result_part_uri(tool_call_id: &str, part_index: usize) -> String {
    format!(
        "{}/parts/{part_index}",
        tool_result_catalog_uri(tool_call_id)
    )
}

/// Replace stored object references with their model-facing catalog while
/// preserving ordinary inline results unchanged.
pub fn tool_result_context_output(tool_call_id: &str, output: &ToolOutput) -> Result<ToolOutput> {
    if !output
        .content_parts
        .iter()
        .any(|part| match part.content.as_ref() {
            Some(chat_content_part::Content::ObjectRef(object)) => {
                tool_output::is_text_object_media_type(&object.media_type)
                    && object
                        .metadata
                        .get(crate::control::cas::METADATA_KIND)
                        .map(String::as_str)
                        == Some(crate::control::cas::METADATA_KIND_TOOL_RESULT)
            }
            _ => false,
        })
    {
        return Ok(output.clone());
    }
    let payload = ToolResultPayload {
        tool_call_id: tool_call_id.to_string(),
        tool_output: output.clone(),
    };
    let catalog = catalog_output(tool_call_id, &payload)?;
    let preview = output.summary.trim();
    let text = if preview.is_empty() {
        catalog.summary
    } else {
        format!("{preview}\n{}", catalog.summary)
    };
    Ok(ToolOutput::text(text))
}

pub fn parse_tool_result_handle(uri: &str) -> Result<ToolResultHandle> {
    let rest = uri
        .strip_prefix("tr://")
        .ok_or_else(|| anyhow!("tool result ref must use the tr:// scheme"))?;
    let (encoded_call_id, part_index) = match rest.split_once("/parts/") {
        Some((call_id, index)) => {
            if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
                bail!("tool result part index must be a non-negative integer");
            }
            let parsed = index.parse::<usize>()?;
            if parsed.to_string() != index {
                bail!("tool result part index is not canonical");
            }
            (call_id, Some(parsed))
        }
        None => (rest, None),
    };
    if encoded_call_id.is_empty() || encoded_call_id.contains('/') {
        bail!("tool result handle has an invalid tool-call ID");
    }
    let tool_call_id = urlencoding::decode(encoded_call_id)
        .map_err(|error| anyhow!("tool result handle has invalid percent-encoding: {error}"))?
        .into_owned();
    if tool_call_id.is_empty() || urlencoding::encode(&tool_call_id) != encoded_call_id {
        bail!("tool result handle is not canonical");
    }
    if encoded_call_id.len() > MAX_CATALOG_BYTES / 2 {
        bail!("tool result handle is too long");
    }
    Ok(ToolResultHandle {
        tool_call_id,
        part_index,
    })
}

/// Resolve a handle only through records belonging to the supplied session.
/// Identical transcript and journal copies represent one result; duplicates in
/// either source or conflicting copies are rejected as ambiguous.
pub async fn read_tool_result_handle(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    uri: &str,
    byte_range: Option<ByteRange>,
) -> Result<ToolOutput> {
    let handle = parse_tool_result_handle(uri)?;
    let payload =
        resolve_tool_result_payload(cp, ns, agent, session_id, &handle.tool_call_id).await?;
    let Some(part_index) = handle.part_index else {
        if byte_range.is_some() {
            bail!("byte_range can only be used with a tr:// part handle");
        }
        return catalog_output(&handle.tool_call_id, &payload);
    };
    let part = payload
        .tool_output
        .content_parts
        .get(part_index)
        .ok_or_else(|| anyhow!("tool result part index {part_index} does not exist"))?;
    materialize_part(
        cp,
        ns,
        agent,
        session_id,
        &handle.tool_call_id,
        part,
        byte_range,
    )
    .await
}

pub async fn read_tool_result_handle_bounded(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    uri: &str,
    start: u64,
    max_size: u64,
) -> Result<ToolOutput> {
    if max_size == 0 || max_size > MAX_TOOL_RESULT_READ_BYTES {
        bail!("tool result reads are limited to 8192 bytes");
    }
    let handle = parse_tool_result_handle(uri)?;
    let part_index = handle
        .part_index
        .ok_or_else(|| anyhow!("byte_range can only be used with a tr:// part handle"))?;
    let payload =
        resolve_tool_result_payload(cp, ns, agent, session_id, &handle.tool_call_id).await?;
    let part = payload
        .tool_output
        .content_parts
        .get(part_index)
        .ok_or_else(|| anyhow!("tool result part index {part_index} does not exist"))?;
    let total = match part.content.as_ref() {
        Some(chat_content_part::Content::Text(text)) => text.len() as u64,
        Some(chat_content_part::Content::ObjectRef(object)) => object
            .metadata
            .get("uncompressed_size_bytes")
            .and_then(|size| size.parse().ok())
            .unwrap_or(object.size_bytes),
        None => bail!("tool result part has no content"),
    };
    if start > total {
        bail!("byte range exceeds the tool result part size");
    }
    let end = start.saturating_add(max_size).min(total);
    materialize_part(
        cp,
        ns,
        agent,
        session_id,
        &handle.tool_call_id,
        part,
        Some(ByteRange { start, end }),
    )
    .await
}

async fn resolve_tool_result_payload(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    tool_call_id: &str,
) -> Result<ToolResultPayload> {
    let mut transcript: Vec<ToolResultPayload> = Vec::new();
    for key in cp
        .kv
        .list_keys(&keys::session_message_prefix(ns, agent, session_id), None)
        .await?
    {
        let Some(message) = cp.kv.get_msg::<data_proto::SessionMessage>(&key).await? else {
            continue;
        };
        for part in message
            .parts
            .iter()
            .filter(|part| part.part_type == data_proto::SessionMessagePartType::ToolResult as i32)
        {
            if let Some(payload) = tool_output::parse_tool_result_payload_json(
                &part.payload_json,
                part.object.as_ref(),
                &part.content,
            )? {
                if payload.tool_call_id == tool_call_id {
                    transcript.push(payload);
                }
            }
        }
    }

    let mut journal: Vec<ToolResultPayload> = Vec::new();
    for submission in cp
        .kv
        .list_keys(
            &keys::session_submission_prefix(ns, agent, session_id),
            None,
        )
        .await?
    {
        for (_, bytes) in cp
            .kv
            .list_entries(
                &keys::session_journal_entry_prefix(ns, agent, session_id, &submission.name),
                None,
            )
            .await?
        {
            let entry = data_proto::SessionJournalEntry::decode(bytes.as_slice())?;
            let Some(data_proto::session_journal_entry_payload::Payload::ToolResult(result)) =
                entry.payload.and_then(|payload| payload.payload)
            else {
                continue;
            };
            if result.tool_call_id != tool_call_id {
                continue;
            }
            let output = result.tool_output.unwrap_or_else(|| {
                let content = if result.output.is_empty() {
                    String::new()
                } else {
                    result.output.clone()
                };
                #[allow(deprecated)]
                ToolOutput {
                    content_parts: vec![crate::harness::llm::text_part(content.clone())],
                    summary: content,
                    byte_range: None,
                }
            });
            journal.push(ToolResultPayload {
                tool_call_id: result.tool_call_id,
                tool_output: output,
            });
        }
    }

    ensure_unique_source(&transcript, "canonical transcript")?;
    ensure_unique_source(&journal, "submission journal")?;
    match (transcript.first(), journal.first()) {
        (Some(transcript), Some(journal)) if transcript != journal => {
            bail!("tool result handle resolves to conflicting session records")
        }
        (Some(transcript), _) => Ok(transcript.clone()),
        (_, Some(journal)) => Ok(journal.clone()),
        _ => bail!("tool result handle was not found in this session"),
    }
}

fn ensure_unique_source(records: &[ToolResultPayload], source: &str) -> Result<()> {
    if records.len() > 1 {
        bail!("tool result handle is ambiguous in the {source}");
    }
    Ok(())
}

fn catalog_output(tool_call_id: &str, payload: &ToolResultPayload) -> Result<ToolOutput> {
    let uri = tool_result_catalog_uri(tool_call_id);
    let mut parts = Vec::new();
    let total_parts = payload.tool_output.content_parts.len();
    let mut truncated = false;
    for (index, part) in payload.tool_output.content_parts.iter().enumerate() {
        let Some(content) = part.content.as_ref() else {
            continue;
        };
        let (media_type, size_bytes) = match content {
            chat_content_part::Content::Text(text) => ("text/plain".to_string(), text.len() as u64),
            chat_content_part::Content::ObjectRef(object) => {
                let logical_size = object
                    .metadata
                    .get("uncompressed_size_bytes")
                    .and_then(|size| size.parse().ok())
                    .unwrap_or(object.size_bytes);
                (object.media_type.clone(), logical_size)
            }
        };
        let entry = json!({"index": index, "uri": tool_result_part_uri(tool_call_id, index), "media_type": media_type, "size_bytes": size_bytes});
        let mut proposed_parts = parts.clone();
        proposed_parts.push(entry.clone());
        let proposed = json!({"uri": uri, "parts": proposed_parts, "total_parts": total_parts, "truncated": false});
        if serde_json::to_string(&proposed)?.len() > MAX_CATALOG_BYTES {
            truncated = true;
            break;
        }
        parts.push(entry);
    }
    Ok(crate::harness::llm::ToolOutput::text(
        serde_json::to_string(
            &json!({"uri": uri, "parts": parts, "total_parts": total_parts, "truncated": truncated}),
        )?,
    ))
}

async fn materialize_part(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    tool_call_id: &str,
    part: &ChatContentPart,
    byte_range: Option<ByteRange>,
) -> Result<ToolOutput> {
    let range = validate_requested_range(byte_range)?;
    let bytes = match part
        .content
        .as_ref()
        .ok_or_else(|| anyhow!("tool result part has no content"))?
    {
        chat_content_part::Content::Text(text) => slice_text(text.as_bytes(), range.as_ref())?,
        chat_content_part::Content::ObjectRef(object) => {
            if !tool_output::is_text_object_media_type(&object.media_type) {
                bail!("tool result part is not text-readable");
            }
            let cas = CasStore::new(cp.objects.clone());
            let scope = SessionCasScope::new(ns, agent, session_id);
            let logical = if let Some(range) = range.as_ref() {
                let (object_scope, metadata) =
                    cas.head_session_object_by_key(&object.key)
                        .await?
                        .ok_or_else(|| anyhow!("tool result content is no longer available"))?;
                if object_scope != scope
                    || metadata
                        .metadata
                        .get(crate::control::cas::METADATA_KIND)
                        .map(String::as_str)
                        != Some(crate::control::cas::METADATA_KIND_TOOL_RESULT)
                    || metadata
                        .metadata
                        .get(crate::control::cas::METADATA_TOOL_CALL_ID)
                        .map(String::as_str)
                        != Some(tool_call_id)
                {
                    bail!("tool result object does not belong to this session handle");
                }
                cas.get_text_range_decoded(&object.key, range.clone())
                    .await?
            } else {
                let object = cas
                    .get_session_object_decoded(&scope, &object.key)
                    .await?
                    .ok_or_else(|| anyhow!("tool result content is no longer available"))?;
                if object
                    .metadata
                    .metadata
                    .get(crate::control::cas::METADATA_KIND)
                    .map(String::as_str)
                    != Some(crate::control::cas::METADATA_KIND_TOOL_RESULT)
                    || object
                        .metadata
                        .metadata
                        .get(crate::control::cas::METADATA_TOOL_CALL_ID)
                        .map(String::as_str)
                        != Some(tool_call_id)
                {
                    bail!("tool result object does not belong to this session handle");
                }
                object.bytes
            };
            if std::str::from_utf8(&logical).is_err() {
                bail!("tool result range is not valid UTF-8");
            }
            logical
        }
    };
    let mut text = String::from_utf8(bytes)?;
    let mut output = crate::harness::llm::ToolOutput::text(text.clone());
    if let Some(range) = range {
        let total = match part.content.as_ref().unwrap() {
            chat_content_part::Content::Text(text) => text.len() as u64,
            chat_content_part::Content::ObjectRef(object) => object
                .metadata
                .get("uncompressed_size_bytes")
                .and_then(|size| size.parse().ok())
                .unwrap_or(object.size_bytes),
        };
        let continuation = if range.end < total {
            format!(
                "\n[Read bytes {}..{} of {}; continue at byte {}.]",
                range.start, range.end, total, range.end
            )
        } else {
            format!(
                "\n[Read bytes {}..{} of {}; end of part.]",
                range.start, range.end, total
            )
        };
        text.push_str(&continuation);
        output = crate::harness::llm::ToolOutput::text(text);
        #[allow(deprecated)]
        {
            output.byte_range = Some(crate::harness::llm::ToolOutputByteRange {
                start: range.start,
                end: range.end,
                next_byte: (range.end < total).then_some(range.end),
            });
        }
    }
    Ok(output)
}

fn validate_requested_range(range: Option<ByteRange>) -> Result<Option<Range<u64>>> {
    let Some(range) = range else { return Ok(None) };
    if range.end < range.start {
        bail!("byte range end must be greater than or equal to start");
    }
    if range.end - range.start > MAX_TOOL_RESULT_READ_BYTES {
        bail!("tool result reads are limited to 8192 bytes");
    }
    Ok(Some(range.start..range.end))
}

fn slice_text(bytes: &[u8], range: Option<&Range<u64>>) -> Result<Vec<u8>> {
    let Some(range) = range else {
        return Ok(bytes.to_vec());
    };
    let start = usize::try_from(range.start)?;
    let end = usize::try_from(range.end)?;
    if end > bytes.len() || start > end {
        bail!("byte range exceeds the tool result part size");
    }
    let text = std::str::from_utf8(bytes)?;
    if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        bail!("byte range endpoints must be UTF-8 boundaries");
    }
    Ok(bytes[start..end].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::object_store::InMemoryObjectStore;
    use crate::control::{ControlPlane, ProtoKeyValueStoreExt};
    use crate::gateway::rpc::data_proto::{
        session_journal_entry_payload::Payload, SessionJournalEntry, SessionJournalEntryPayload,
        SessionJournalEntryPayloadToolResult, SessionMessage, SessionMessagePart,
        SessionMessagePartType,
    };
    use crate::test_support::{EmptyPubSub, MockKvStore};
    use std::sync::Arc;

    #[test]
    fn tr_handles_round_trip_and_require_canonical_encoding() {
        let uri = tool_result_part_uri("call/with space", 3);
        assert_eq!(
            parse_tool_result_handle(&uri).unwrap(),
            ToolResultHandle {
                tool_call_id: "call/with space".to_string(),
                part_index: Some(3),
            }
        );
        assert!(parse_tool_result_handle("tr://call%2fwith%20space/parts/3").is_err());
        assert!(parse_tool_result_handle("tr://call/parts/03").is_err());
    }

    #[test]
    fn range_reads_are_limited_and_require_utf8_boundaries() {
        assert!(validate_requested_range(Some(ByteRange {
            start: 0,
            end: MAX_TOOL_RESULT_READ_BYTES + 1,
        }))
        .is_err());
        assert_eq!(
            slice_text("aéz".as_bytes(), Some(&(1..3))).unwrap(),
            "é".as_bytes()
        );
        assert!(slice_text("aéz".as_bytes(), Some(&(2..3))).is_err());
    }

    #[test]
    fn model_context_uses_handles_without_exposing_cas_keys() {
        let output = ToolOutput::from_content_parts(
            vec![crate::harness::llm::object_ref_part(
                data_proto::ObjectRef {
                    key: "cas/acme/sessions/session-1/messages/message-1/result.txt".to_string(),
                    media_type: "text/plain; charset=utf-8".to_string(),
                    size_bytes: 4096,
                    metadata: std::collections::HashMap::from([
                        (
                            crate::control::cas::METADATA_KIND.to_string(),
                            crate::control::cas::METADATA_KIND_TOOL_RESULT.to_string(),
                        ),
                        (
                            crate::control::cas::METADATA_UNCOMPRESSED_SIZE_BYTES.to_string(),
                            "12000".to_string(),
                        ),
                    ]),
                    ..Default::default()
                },
            )],
            "Stored text result (12000 bytes)",
        );
        let projected = tool_result_context_output("call-1", &output).unwrap();
        assert!(projected.summary.contains("tr://call-1/parts/0"));
        assert!(projected.summary.contains("12000"));
        assert!(!projected.summary.contains("cas/"));
    }

    #[tokio::test]
    async fn catalog_and_result_remain_resolvable_after_a_compaction_record() {
        let kv = Arc::new(MockKvStore::new());
        let cp = ControlPlane::builder(kv, Arc::new(EmptyPubSub))
            .objects(Arc::new(InMemoryObjectStore::default()))
            .build();
        let ns = "acme";
        let agent = "assistant";
        let session = "session-1";
        let call_id = "call/one";
        let output = ToolOutput::text("original result");
        let payload_json = tool_output::tool_result_payload_json(call_id, &output).unwrap();

        cp.kv
            .set_msg(
                &keys::session_message(ns, agent, session, "message-1"),
                &SessionMessage {
                    id: "message-1".to_string(),
                    role: data_proto::MessageRole::RoleAssistant as i32,
                    parts: vec![SessionMessagePart {
                        id: "result-1".to_string(),
                        part_type: SessionMessagePartType::ToolResult as i32,
                        name: "lookup".to_string(),
                        payload_json: payload_json.clone(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        cp.kv
            .set_msg(
                &keys::session_message(ns, agent, session, "message-2"),
                &SessionMessage {
                    id: "message-2".to_string(),
                    role: data_proto::MessageRole::RoleAssistant as i32,
                    parts: vec![SessionMessagePart {
                        id: "compaction-1".to_string(),
                        part_type: SessionMessagePartType::Compaction as i32,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        cp.kv
            .set_msg(
                &keys::session_submission(ns, agent, session, "submission-1"),
                &data_proto::SessionSubmission {
                    submission_id: "submission-1".to_string(),
                    session_id: session.to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        cp.kv
            .set_msg(
                &keys::session_journal_entry(ns, agent, session, "submission-1", "entry-1"),
                &SessionJournalEntry {
                    submission_id: "submission-1".to_string(),
                    journal_entry_id: "entry-1".to_string(),
                    payload: Some(SessionJournalEntryPayload {
                        payload: Some(Payload::ToolResult(SessionJournalEntryPayloadToolResult {
                            tool_call_id: call_id.to_string(),
                            name: "lookup".to_string(),
                            output: "original result".to_string(),
                            tool_output: Some(output),
                            ..Default::default()
                        })),
                    }),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let catalog = read_tool_result_handle(
            &cp,
            ns,
            agent,
            session,
            &tool_result_catalog_uri(call_id),
            None,
        )
        .await
        .unwrap();
        assert!(catalog.summary.contains(&tool_result_part_uri(call_id, 0)));
        let result = read_tool_result_handle(
            &cp,
            ns,
            agent,
            session,
            &tool_result_part_uri(call_id, 0),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            tool_output::plain_text(&result).as_deref(),
            Some("original result")
        );
    }
}
