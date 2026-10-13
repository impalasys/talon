// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;
use crate::harness::llm::{chat_content_part, ByteRange, ToolOutputByteRange};
use anyhow::{anyhow, bail, Result};

pub fn register(registry: &mut ToolRegistry, _spec: &manifests::AgentSpec) {
    // Artifact reads and writes are session scoped and were available through
    // the legacy artifact tools without file capabilities. Dispatch applies
    // the resource-specific authorization after inspecting the URI/kind.
    registry.register_builtin(
            READ_RESOURCE_TOOL,
            "Read a file, artifact, or prior tool-result part by its URI. Text reads default to an 8192-byte page; use byte_range to select a UTF-8 byte interval.",
            json!({
                "type": "object",
                "properties": {
                    "ref": { "type": "string", "description": "file://, artifact://, or session-local tr:// resource URI." },
                    "byte_range": {
                        "type": "object",
                        "properties": {
                            "start": { "type": "integer", "minimum": 0 },
                            "end": { "type": "integer", "minimum": 0 },
                            "max_size": { "type": "integer", "minimum": 1, "maximum": 8192 }
                        },
                        "required": ["start"]
                    }
                },
                "required": ["ref"]
            }),
    );
    registry.register_builtin(
            WRITE_RESOURCE_TOOL,
            "Create or update a file or session artifact. Set kind for creation, or ref to update an existing resource.",
            json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["file", "artifact"] },
                    "ref": { "type": "string", "description": "Existing file:// or artifact:// resource URI to update." },
                    "path": { "type": "string", "description": "Required when creating a file." },
                    "title": { "type": "string", "description": "Required when creating an artifact." },
                    "content": { "type": "string" },
                    "media_type": { "type": "string" },
                    "purpose": { "type": "string" },
                    "index_policy": { "type": "string" },
                    "retention": { "type": "string" }
                },
                "required": ["content"]
            }),
    );
}

pub async fn read(
    cp: &ControlPlane,
    namespace: &str,
    agent: &str,
    session: &str,
    args: &Value,
) -> Result<ToolOutput> {
    let reference = req_str(args, "ref")?;
    if reference.starts_with("tr://") {
        if !reference.contains("/parts/") && args.get("byte_range").is_none() {
            return crate::control::tool_result_handles::read_tool_result_handle(
                cp, namespace, agent, session, reference, None,
            )
            .await;
        }
        if let Some(range) = args.get("byte_range") {
            if range.get("max_size").and_then(Value::as_u64).is_some() {
                let start = range.get("start").and_then(Value::as_u64).unwrap_or(0);
                let max_size = range.get("max_size").and_then(Value::as_u64).unwrap_or(0);
                return crate::control::tool_result_handles::read_tool_result_handle_bounded(
                    cp, namespace, agent, session, reference, start, max_size,
                )
                .await;
            }
            return crate::control::tool_result_handles::read_tool_result_handle(
                cp,
                namespace,
                agent,
                session,
                reference,
                parse_exact_range(args)?,
            )
            .await;
        }
        return crate::control::tool_result_handles::read_tool_result_handle_bounded(
            cp, namespace, agent, session, reference, 0, 8192,
        )
        .await;
    }
    let mut output = if reference.starts_with("file://") {
        let mut forwarded = args.clone();
        forwarded["uri"] = json!(reference);
        crate::harness::native_tools::files::read_file_tool(cp, namespace, &forwarded).await?
    } else if reference.starts_with("artifact://") {
        crate::harness::native_tools::artifacts::read_artifact(
            cp,
            namespace,
            agent,
            session,
            &json!({"artifact_uri": reference}),
        )
        .await?
    } else {
        bail!("read ref must use file://, artifact://, or tr://")
    };
    select_range(&mut output, args)?;
    Ok(output)
}

pub async fn write(
    cp: &ControlPlane,
    namespace: &str,
    agent: &str,
    session: &str,
    args: &Value,
) -> Result<String> {
    let content = req_str(args, "content")?;
    let mut forwarded = args.clone();
    forwarded["content"] = json!(content);
    if let Some(reference) = opt_str(args, "ref") {
        if reference.starts_with("file://") {
            forwarded["uri"] = json!(reference);
            crate::harness::native_tools::files::update_file_tool(cp, namespace, &forwarded).await
        } else if reference.starts_with("artifact://") {
            forwarded["artifact_uri"] = json!(reference);
            crate::harness::native_tools::artifacts::update_artifact(
                cp, namespace, agent, session, &forwarded,
            )
            .await
        } else {
            bail!("write ref must use file:// or artifact://")
        }
    } else {
        match opt_str(args, "kind") {
            Some("file") => {
                crate::harness::native_tools::files::create_file_tool(cp, namespace, args).await
            }
            Some("artifact") => {
                crate::harness::native_tools::artifacts::create_artifact(
                    cp, namespace, agent, session, args,
                )
                .await
            }
            _ => Err(anyhow!(
                "write requires kind=file|artifact when ref is omitted"
            )),
        }
    }
}

fn parse_exact_range(args: &Value) -> Result<Option<ByteRange>> {
    let Some(value) = args.get("byte_range") else {
        return Ok(None);
    };
    let range = value
        .as_object()
        .ok_or_else(|| anyhow!("byte_range must be an object"))?;
    let start = range
        .get("start")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("byte_range.start must be a non-negative integer"))?;
    let end = range
        .get("end")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("tr:// reads require an exact byte_range.end"))?;
    Ok(Some(ByteRange { start, end }))
}

fn select_range(output: &mut ToolOutput, args: &Value) -> Result<()> {
    let range_value = args.get("byte_range");
    let start = range_value
        .and_then(|value| value.get("start"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let requested_end = range_value
        .and_then(|value| value.get("end"))
        .and_then(Value::as_u64);
    let max_size = range_value
        .and_then(|value| value.get("max_size"))
        .and_then(Value::as_u64);
    let bounded = requested_end.is_none();
    if range_value.is_some() && requested_end.is_none() && max_size.is_none() {
        bail!("byte_range requires end or max_size");
    }
    if max_size.is_some_and(|size| size == 0 || size > 8192) {
        bail!("byte_range.max_size must be between 1 and 8192 bytes");
    }
    if requested_end.is_some_and(|end| end.saturating_sub(start) > 8192) {
        bail!("byte_range is limited to 8192 bytes");
    }
    let default_page = range_value.is_none();
    let limit = max_size.unwrap_or(8192);
    let part = output
        .content_parts
        .first_mut()
        .ok_or_else(|| anyhow!("resource has no content"))?;
    match part.content.as_mut() {
        Some(chat_content_part::Content::Text(text)) => {
            let original = text.clone();
            let bytes_len = original.len();
            let end = if bounded {
                start.saturating_add(limit).min(bytes_len as u64)
            } else {
                requested_end.expect("checked exact end")
            };
            if start > end || end > bytes_len as u64 {
                bail!("byte_range is outside the resource");
            }
            let mut actual_end = end as usize;
            if !bounded && !original.is_char_boundary(actual_end) {
                bail!("byte_range is not valid UTF-8");
            }
            while actual_end > start as usize && !original.is_char_boundary(actual_end) {
                actual_end -= 1;
            }
            if !original.is_char_boundary(start as usize) {
                bail!("byte_range is not valid UTF-8");
            }
            let selected = original[start as usize..actual_end].to_string();
            *text = selected.clone();
            if !default_page {
                output.byte_range = Some(ToolOutputByteRange {
                    start,
                    end: actual_end as u64,
                    next_byte: (bounded && actual_end < bytes_len).then_some(actual_end as u64),
                });
            }
            output.summary = selected;
        }
        Some(chat_content_part::Content::ObjectRef(object)) => {
            let text_type =
                crate::control::tool_output::is_text_object_media_type(&object.media_type);
            if !text_type {
                if range_value.is_some() {
                    bail!("byte_range is only supported for text resources");
                }
                return Ok(());
            }
            let logical_size = object.size_bytes;
            let end = if bounded {
                start.saturating_add(limit).min(logical_size)
            } else {
                requested_end.expect("checked exact end")
            };
            if start > end || end > logical_size {
                bail!("byte_range is outside the resource");
            }
            output.byte_range = Some(ToolOutputByteRange {
                start,
                end,
                next_byte: (bounded && end < logical_size).then_some(end),
            });
        }
        None => bail!("resource has no content"),
    }
    Ok(())
}
