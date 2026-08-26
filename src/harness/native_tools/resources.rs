// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::dispatch::require_global_capability;
use super::*;
use crate::control::tool_output;
use crate::harness::llm::chat_content_part;

pub(crate) fn resource_read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ref": { "type": "string", "description": "file://, artifact://, or session-local tr:// URI." },
            "namespace": { "type": "string", "description": "File namespace when reading by path. Defaults to the current namespace." },
            "path": { "type": "string", "description": "Logical File path when ref is omitted." },
            "byte_range": {
                "type": "object",
                "description": "UTF-8 byte range for a text resource. start is zero-based; end is exclusive. Supply exactly one of end or max_size.",
                "properties": {
                    "start": { "type": "integer", "minimum": 0 },
                    "end": { "type": "integer", "minimum": 0 },
                    "max_size": { "type": "integer", "minimum": 1, "maximum": 8192 }
                },
                "required": ["start"]
            }
        }
    })
}

pub(crate) fn resource_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ref": { "type": "string", "description": "Existing file:// or artifact:// URI to update." },
            "kind": { "type": "string", "enum": ["file", "artifact"], "description": "Required when ref is omitted to create a resource." },
            "namespace": { "type": "string", "description": "File namespace when creating a File. Defaults to the current namespace." },
            "path": { "type": "string", "description": "Required to create a File." },
            "title": { "type": "string", "description": "Required to create an Artifact." },
            "content": { "type": "string", "description": "Text content. Required unless content_base64 is provided." },
            "content_base64": { "type": "string", "description": "Base64 content for an Artifact." },
            "media_type": { "type": "string", "description": "Media type. Defaults to the existing or resource-specific default." },
            "purpose": { "type": "string", "description": "File purpose when creating or updating a File." },
            "index_policy": { "type": "string", "description": "File indexing policy when creating or updating a File." },
            "retention": { "type": "string", "description": "File retention when creating or updating a File." },
            "labels": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Artifact labels when creating an Artifact." },
            "metadata": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Artifact metadata when creating an Artifact." }
        }
    })
}

pub(crate) async fn read_resource_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    config: &Config,
    args: &Value,
) -> Result<ToolOutput> {
    let selection = read_selection(args)?;
    let Some(reference) = opt_str(args, "ref") else {
        require_global_capability(config, "files", "read")?;
        require_file_read(spec)?;
        return selected_resource_output(
            cp,
            read_file_tool(cp, current_namespace, args).await?,
            selection,
        )
        .await;
    };
    if reference.starts_with("tr://") {
        return read_session_tool_result(
            cp,
            current_namespace,
            current_agent,
            current_session,
            reference,
            selection,
        )
        .await;
    }
    if reference.starts_with("artifact://") {
        return selected_resource_output(
            cp,
            read_artifact(
                cp,
                current_namespace,
                current_agent,
                current_session,
                &args_with_string(args, "artifact_uri", reference)?,
            )
            .await?,
            selection,
        )
        .await;
    }
    if reference.starts_with("file://") {
        require_global_capability(config, "files", "read")?;
        require_file_read(spec)?;
        return selected_resource_output(
            cp,
            read_file_tool(
                cp,
                current_namespace,
                &args_with_string(args, "uri", reference)?,
            )
            .await?,
            selection,
        )
        .await;
    }
    Err(anyhow!(
        "read.ref must start with file://, artifact://, or tr://"
    ))
}

const MAX_RESOURCE_READ_BYTES: u64 = tool_output::TOOL_RESULT_INLINE_CONTEXT_BYTES as u64;

#[derive(Debug, Clone, Copy)]
enum ResourceSelection {
    Exact { start: u64, end: u64 },
    Bounded { start: u64, max_size: u64 },
}

fn read_selection(args: &Value) -> Result<Option<ResourceSelection>> {
    let Some(range) = args.get("byte_range") else {
        return Ok(None);
    };
    let range = range
        .as_object()
        .ok_or_else(|| anyhow!("byte_range must be an object"))?;
    let start = range
        .get("start")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("byte_range.start is required"))?;
    let end = range.get("end").and_then(Value::as_u64);
    let max_size = range.get("max_size").and_then(Value::as_u64);
    match (end, max_size) {
        (Some(end), None) if end >= start && end - start <= MAX_RESOURCE_READ_BYTES => {
            Ok(Some(ResourceSelection::Exact { start, end }))
        }
        (None, Some(max_size)) if max_size > 0 && max_size <= MAX_RESOURCE_READ_BYTES => {
            Ok(Some(ResourceSelection::Bounded { start, max_size }))
        }
        (Some(_), Some(_)) | (None, None) => Err(anyhow!(
            "byte_range requires exactly one of end or max_size"
        )),
        (Some(_), None) => Err(anyhow!(
            "byte_range.end must not precede start and must be within the 8 KiB limit"
        )),
        _ => Err(anyhow!(
            "byte_range.max_size must be within the 8 KiB limit"
        )),
    }
}

async fn selected_resource_output(
    cp: &ControlPlane,
    output: ToolOutput,
    selection: Option<ResourceSelection>,
) -> Result<ToolOutput> {
    let Some(selection) = selection else {
        return Ok(output);
    };
    let Some(object) = output.object_ref().cloned() else {
        let text = tool_output::plain_text(&output)
            .ok_or_else(|| anyhow!("byte_range is only valid for text resources"))?;
        let (text, start, end, next_byte) = select_resource_bytes(&text, selection)?;
        return Ok(ToolOutput::text(format!(
            "{text}\n[bytes {start}..{end}){}",
            next_byte
                .map(|n| format!("; next_byte={n}"))
                .unwrap_or_default()
        )));
    };
    if !tool_output::is_text_object_media_type(&object.media_type) {
        return Err(anyhow!("byte_range is only valid for text resources"));
    }
    let (start, end, next_byte) = checked_object_byte_range(cp, &object, selection).await?;
    Ok(tool_output::selected_object_byte_range_output(
        object,
        start,
        end,
        next_byte,
        format!("Read bytes {start}..{end}."),
    ))
}

fn object_byte_range(
    object: &data_proto::ObjectRef,
    selection: ResourceSelection,
) -> Result<(u64, u64, Option<u64>)> {
    let size = object
        .metadata
        .get(crate::control::cas::METADATA_UNCOMPRESSED_SIZE_BYTES)
        .and_then(|v| v.parse().ok())
        .unwrap_or(object.size_bytes);
    match selection {
        ResourceSelection::Exact { start, end } if end <= size => Ok((start, end, None)),
        ResourceSelection::Exact { .. } => Err(anyhow!("byte_range exceeds resource size")),
        ResourceSelection::Bounded { start, max_size } if start <= size => {
            let end = start.saturating_add(max_size).min(size);
            Ok((start, end, (end < size).then_some(end)))
        }
        ResourceSelection::Bounded { .. } => Err(anyhow!("byte_range.start exceeds resource size")),
    }
}

fn select_resource_bytes(
    text: &str,
    selection: ResourceSelection,
) -> Result<(String, u64, u64, Option<u64>)> {
    let bytes = text.as_bytes();
    let (start, requested_end, bounded) = match selection {
        ResourceSelection::Exact { start, end } => (start as usize, end as usize, false),
        ResourceSelection::Bounded { start, max_size } => (
            start as usize,
            start.saturating_add(max_size).min(bytes.len() as u64) as usize,
            true,
        ),
    };
    if start > bytes.len() || requested_end > bytes.len() || !text.is_char_boundary(start) {
        return Err(anyhow!(
            "byte_range is outside the resource or not on a UTF-8 boundary"
        ));
    }
    let mut end = requested_end;
    if bounded {
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
    } else if !text.is_char_boundary(end) {
        return Err(anyhow!("byte_range.end must be on a UTF-8 boundary"));
    }
    Ok((
        text[start..end].to_string(),
        start as u64,
        end as u64,
        (bounded && end < bytes.len()).then_some(end as u64),
    ))
}

async fn read_session_tool_result(
    cp: &ControlPlane,
    namespace: &str,
    agent: &str,
    session_id: &str,
    reference: &str,
    selection: Option<ResourceSelection>,
) -> Result<ToolOutput> {
    if session_id.is_empty() {
        return Err(anyhow!("tr:// references require an active session"));
    }
    let raw = reference
        .strip_prefix("tr://")
        .ok_or_else(|| anyhow!("invalid tr:// reference"))?;
    let (encoded_id, index) = match raw.rsplit_once("/parts/") {
        Some((id, index)) => (
            id,
            Some(
                index
                    .parse::<usize>()
                    .map_err(|_| anyhow!("tool result part index must be a zero-based integer"))?,
            ),
        ),
        None => (raw, None),
    };
    if encoded_id.is_empty() || encoded_id.contains('/') {
        return Err(anyhow!(
            "tr:// reference must include one encoded tool call id"
        ));
    }
    let tool_call_id = urlencoding::decode(encoded_id)
        .map_err(|_| anyhow!("tool call id is not valid percent-encoding"))?
        .into_owned();
    if crate::control::tool_output::tool_result_handle(&tool_call_id)
        != format!("tr://{encoded_id}")
    {
        return Err(anyhow!("tool call id is not canonically encoded"));
    }
    let mut found = None;
    for submission in cp
        .kv
        .list_keys(
            &keys::session_submission_prefix(namespace, agent, session_id),
            None,
        )
        .await?
    {
        for (_, bytes) in cp
            .kv
            .list_entries(
                &keys::session_journal_entry_prefix(namespace, agent, session_id, &submission.name),
                None,
            )
            .await?
        {
            let entry = data_proto::SessionJournalEntry::decode(bytes.as_slice())?;
            let Some(result) = entry
                .payload
                .as_ref()
                .and_then(|payload| payload.payload.as_ref())
                .and_then(|payload| match payload {
                    data_proto::session_journal_entry_payload::Payload::ToolResult(result) => {
                        Some(result)
                    }
                    _ => None,
                })
            else {
                continue;
            };
            if result.tool_call_id != tool_call_id {
                continue;
            }
            let output = result
                .tool_output
                .clone()
                .unwrap_or_else(|| ToolOutput::text(result.output.clone()));
            if found.replace(output).is_some() {
                return Err(anyhow!(
                    "tool result reference '{}' is ambiguous in this session",
                    tool_call_id
                ));
            }
        }
    }
    let output = found.ok_or_else(|| {
        anyhow!(
            "tool result '{}' was not found in the current session",
            tool_call_id
        )
    })?;
    let Some(index) = index else {
        if selection.is_some() {
            return Err(anyhow!(
                "byte_range requires a tr://.../parts/<index> reference"
            ));
        }
        return Ok(ToolOutput::text(if output.summary.is_empty() {
            tool_output::compact_tool_result_catalog(&tool_call_id, &output.content_parts)
        } else {
            output.summary
        }));
    };
    let part = output
        .content_parts
        .get(index)
        .ok_or_else(|| anyhow!("tool result part {index} does not exist"))?;
    match part.content.as_ref() {
        Some(chat_content_part::Content::Text(text)) => {
            let selection = selection.unwrap_or(ResourceSelection::Bounded {
                start: 0,
                max_size: MAX_RESOURCE_READ_BYTES,
            });
            let (text, start, end, next_byte) = select_resource_bytes(text, selection)?;
            Ok(ToolOutput::text(format!(
                "{text}\n[bytes {start}..{end}){}",
                next_byte
                    .map(|n| format!("; next_byte={n}"))
                    .unwrap_or_default()
            )))
        }
        Some(chat_content_part::Content::ObjectRef(object))
            if tool_output::is_text_object_media_type(&object.media_type) =>
        {
            let selection = selection.unwrap_or(ResourceSelection::Bounded {
                start: 0,
                max_size: MAX_RESOURCE_READ_BYTES,
            });
            let (start, end, next_byte) = checked_object_byte_range(cp, object, selection).await?;
            Ok(tool_output::selected_object_byte_range_output(
                object.clone(),
                start,
                end,
                next_byte,
                format!(
                    "Read bytes {start}..{end} from {}.",
                    tool_output::tool_result_part_handle(&tool_call_id, index)
                ),
            ))
        }
        Some(chat_content_part::Content::ObjectRef(object)) => {
            if selection.is_some() {
                return Err(anyhow!(
                    "byte_range is only valid for text tool-result parts"
                ));
            }
            Ok(ToolOutput::from_content_parts(
                vec![part.clone()],
                format!("Tool result part {index}: {}", object.media_type),
            ))
        }
        None => Err(anyhow!("tool result part {index} is empty")),
    }
}

async fn checked_object_byte_range(
    cp: &ControlPlane,
    object: &data_proto::ObjectRef,
    selection: ResourceSelection,
) -> Result<(u64, u64, Option<u64>)> {
    let (start, mut end, mut next_byte) = object_byte_range(object, selection)?;
    let cas = crate::control::cas::CasStore::new(cp.objects.clone());
    let bytes = cas
        .get_text_range_decoded(&object.key, start, end)
        .await?
        .ok_or_else(|| anyhow!("text object is unavailable"))?;
    match selection {
        ResourceSelection::Exact { .. } => {
            std::str::from_utf8(&bytes)
                .map_err(|_| anyhow!("byte_range start and end must be UTF-8 boundaries"))?;
        }
        ResourceSelection::Bounded { .. } => {
            let mut actual = bytes.len();
            while actual > 0 && std::str::from_utf8(&bytes[..actual]).is_err() {
                actual -= 1;
            }
            if actual == 0 && !bytes.is_empty() {
                return Err(anyhow!("byte_range.start must be a UTF-8 boundary"));
            }
            end = start + actual as u64;
            let size = object
                .metadata
                .get(crate::control::cas::METADATA_UNCOMPRESSED_SIZE_BYTES)
                .and_then(|v| v.parse().ok())
                .unwrap_or(object.size_bytes);
            next_byte = (end < size).then_some(end);
        }
    }
    Ok((start, end, next_byte))
}

pub(crate) async fn write_resource_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    config: &Config,
    args: &Value,
) -> Result<String> {
    match opt_str(args, "ref") {
        Some(reference) if reference.starts_with("tr://") => {
            Err(anyhow!("tool-result references are read-only"))
        }
        Some(reference) if reference.starts_with("artifact://") => {
            update_artifact(
                cp,
                current_namespace,
                current_agent,
                current_session,
                &args_with_string(args, "artifact_uri", reference)?,
            )
            .await
        }
        Some(reference) if reference.starts_with("file://") => {
            require_global_capability(config, "files", "update")?;
            require_capability(spec, "files", "update")?;
            update_file_tool(
                cp,
                current_namespace,
                &args_with_string(args, "uri", reference)?,
            )
            .await
        }
        Some(_) => Err(anyhow!("write.ref must start with file:// or artifact://")),
        None => match req_str(args, "kind")? {
            "artifact" => {
                create_artifact(cp, current_namespace, current_agent, current_session, args).await
            }
            "file" => {
                require_global_capability(config, "files", "create")?;
                require_capability(spec, "files", "create")?;
                create_file_tool(cp, current_namespace, args).await
            }
            kind => Err(anyhow!(
                "unsupported write.kind '{kind}'; expected file or artifact"
            )),
        },
    }
}

fn args_with_string(args: &Value, key: &str, value: &str) -> Result<Value> {
    let mut args = args.clone();
    let object = args
        .as_object_mut()
        .ok_or_else(|| anyhow!("tool arguments must be an object"))?;
    object.insert(key.to_string(), Value::String(value.to_string()));
    Ok(args)
}
