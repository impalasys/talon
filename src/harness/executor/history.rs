// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::runtime::LoopMessage;
use crate::control::cas::{decode_stored_object_bytes, object_ref_from_metadata};
use crate::control::object_store::ObjectStore;
use crate::control::tool_output::{self, ToolOutputExt};
use crate::control::{KeyValueStore, ListOptions};
use crate::gateway::rpc::data_proto;
use crate::harness::llm::{
    content_part_object_ref, object_ref_part, text_part, ChatContentPart, ToolCall,
};
use anyhow::{anyhow, Result};
use prost::Message;
use std::path::Path;

pub struct Loaded {
    pub messages: Vec<LoopMessage>,
    pub has_delegated_task: bool,
}

const SESSION_MESSAGE_LOAD_PAGE_SIZE: usize = 100;

/// Rebuild the model-visible history for a session.
///
/// Messages are read newest first in bounded KV pages. The newest compaction
/// part with a readable summary object is the boundary: its summary is injected
/// first, only parts after that marker in the marker's message are replayed,
/// and then only later messages are replayed in chronological order. If no
/// valid marker exists, every replayable message is loaded.
pub async fn load(
    kv: &dyn KeyValueStore,
    objects: &(dyn ObjectStore + Send + Sync),
    ns: &str,
    agent_id: &str,
    session_id: &str,
) -> Result<Loaded> {
    let prefix = crate::control::keys::session_message_prefix(ns, agent_id, session_id);
    let mut later_messages = Vec::new();
    let mut has_delegated_task = false;
    let mut before_name = None;

    loop {
        let page = kv
            .list_entries(
                &prefix,
                Some(
                    ListOptions::desc()
                        .before_name(before_name.as_deref())
                        .limit(SESSION_MESSAGE_LOAD_PAGE_SIZE),
                ),
            )
            .await?;
        if page.is_empty() {
            break;
        }
        before_name = page.last().map(|(key, _)| key.name.clone());

        for (_, value) in page {
            let Ok(message) = data_proto::SessionMessage::decode(value.as_slice()) else {
                continue;
            };
            if message
                .labels
                .get(crate::control::delegation::LABEL_TASK_ROLE)
                .map(String::as_str)
                == Some("delegate")
            {
                has_delegated_task = true;
            }
            if message.role == data_proto::MessageRole::RoleAssistant as i32
                && !assistant_projection_is_replayable(&message)
            {
                continue;
            }

            if let Some((marker_index, summary)) = latest_valid_compaction(&message, objects).await
            {
                let mut messages = vec![LoopMessage::text("assistant", summary)];
                let mut marker_tail = message;
                marker_tail.parts = marker_tail.parts.split_off(marker_index + 1);
                if !marker_tail.parts.is_empty() {
                    messages.extend(session_message_to_loop_messages(&marker_tail, objects).await?);
                }
                later_messages.reverse();
                for message in later_messages {
                    messages.extend(session_message_to_loop_messages(&message, objects).await?);
                }
                return Ok(Loaded {
                    messages,
                    has_delegated_task,
                });
            }

            later_messages.push(message);
        }
    }

    later_messages.reverse();
    let mut messages = Vec::new();
    for message in later_messages {
        messages.extend(session_message_to_loop_messages(&message, objects).await?);
    }

    Ok(Loaded {
        messages,
        has_delegated_task,
    })
}

async fn latest_valid_compaction(
    message: &data_proto::SessionMessage,
    objects: &(dyn ObjectStore + Send + Sync),
) -> Option<(usize, String)> {
    for (index, part) in message.parts.iter().enumerate().rev() {
        if part.part_type != data_proto::SessionMessagePartType::Compaction as i32 {
            continue;
        }
        let Some(object) = part.object.as_ref() else {
            tracing::warn!(message_id = %message.id, part_id = %part.id, "Ignoring compaction marker without a summary object");
            continue;
        };
        let result = async {
            let stored = objects
                .get(&object.key)
                .await?
                .ok_or_else(|| anyhow!("compaction summary object '{}' is missing", object.key))?;
            let bytes = decode_stored_object_bytes(&stored, &object.key)?;
            String::from_utf8(bytes).map_err(|_| anyhow!("compaction summary is not UTF-8"))
        }
        .await;
        match result {
            Ok(summary) => return Some((index, summary)),
            Err(error) => {
                tracing::warn!(message_id = %message.id, part_id = %part.id, error = %error, "Ignoring invalid compaction marker")
            }
        }
    }
    None
}

fn assistant_projection_is_replayable(message: &data_proto::SessionMessage) -> bool {
    match message
        .labels
        .get(crate::harness::sessions::SESSION_LABEL_PROJECTION_STATE)
        .map(String::as_str)
    {
        None | Some(crate::harness::sessions::SESSION_PROJECTION_STATE_COMMITTED) => true,
        Some(crate::harness::sessions::SESSION_PROJECTION_STATE_FAILED) => true,
        Some(_) => false,
    }
}

async fn session_message_to_loop_messages(
    message: &data_proto::SessionMessage,
    objects: &(dyn ObjectStore + Send + Sync),
) -> Result<Vec<LoopMessage>> {
    if message.role == data_proto::MessageRole::RoleAssistant as i32 {
        return assistant_session_message_to_loop_messages(message, objects).await;
    }

    Ok(vec![LoopMessage {
        role: match data_proto::MessageRole::try_from(message.role) {
            Ok(data_proto::MessageRole::RoleUser) => "user",
            Ok(data_proto::MessageRole::RoleSystem) => "system",
            _ => "user",
        }
        .to_string(),
        content_parts: message_content_parts(message, objects).await?,
        tool_calls: None,
        tool_call_id: None,
    }])
}

fn inferred_image_media_type(key: &str) -> Option<&'static str> {
    match Path::new(key)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

pub(crate) async fn message_content_parts(
    message: &data_proto::SessionMessage,
    objects: &(dyn ObjectStore + Send + Sync),
) -> Result<Vec<ChatContentPart>> {
    let mut content_parts = Vec::new();
    for part in &message.parts {
        content_parts.extend(message_part_content_parts(part, objects).await?);
    }
    Ok(content_parts)
}

async fn message_part_content_parts(
    part: &data_proto::SessionMessagePart,
    objects: &(dyn ObjectStore + Send + Sync),
) -> Result<Vec<ChatContentPart>> {
    if part.part_type == data_proto::SessionMessagePartType::Text as i32 {
        return Ok(if part.content.is_empty() {
            Vec::new()
        } else {
            vec![text_part(part.content.clone())]
        });
    }

    if part.part_type != data_proto::SessionMessagePartType::Image as i32 {
        return Ok(Vec::new());
    }

    let mut content_parts = Vec::new();
    if !part.content.is_empty() {
        content_parts.push(text_part(part.content.clone()));
    }

    let payload = serde_json::from_str::<serde_json::Value>(&part.payload_json)
        .unwrap_or(serde_json::Value::Null);
    if let Some(url) = payload.get("url").and_then(|value| value.as_str()) {
        content_parts.push(text_part(format!("[Image URL: {url}]")));
        return Ok(content_parts);
    }

    let Some(object) = part.object.as_ref() else {
        return Ok(content_parts);
    };
    let mut object_ref = object.clone();
    if object_ref.media_type.trim().is_empty() {
        if let Some(metadata) = objects.head(&object_ref.key).await? {
            object_ref = object_ref_from_metadata(&object_ref.key, &metadata);
        }
    }
    let mut media_type = object_ref.media_type.trim().to_string();
    if media_type.is_empty() {
        media_type = inferred_image_media_type(&object_ref.key)
            .ok_or_else(|| anyhow!("missing media type for image object '{}'", object_ref.key))?
            .to_string();
        object_ref.media_type = media_type.clone();
    }
    if !media_type.to_ascii_lowercase().starts_with("image/") {
        return Err(anyhow!(
            "unsupported media type '{}' for image object '{}'",
            media_type,
            object_ref.key
        ));
    }
    content_parts.push(object_ref_part(object_ref));
    Ok(content_parts)
}

async fn assistant_session_message_to_loop_messages(
    message: &data_proto::SessionMessage,
    objects: &(dyn ObjectStore + Send + Sync),
) -> Result<Vec<LoopMessage>> {
    let mut history = Vec::new();
    let mut content_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut tool_results = Vec::new();
    let mut seen_result_ids = std::collections::HashSet::new();

    for part in &message.parts {
        if part.part_type == data_proto::SessionMessagePartType::Text as i32
            || part.part_type == data_proto::SessionMessagePartType::Image as i32
        {
            flush_tool_batch(
                &mut history,
                &mut content_parts,
                &mut tool_calls,
                &mut tool_results,
                &mut seen_result_ids,
            );
            content_parts.extend(message_part_content_parts(part, objects).await?);
            continue;
        }

        if part.part_type == data_proto::SessionMessagePartType::ToolCall as i32 {
            if let Some(tool_call) = tool_call_from_part(part) {
                tool_calls.push(tool_call);
            }
            continue;
        }

        if part.part_type == data_proto::SessionMessagePartType::ToolResult as i32 {
            if tool_calls.is_empty() {
                continue;
            }
            if let Some(message) = tool_result_message_from_part(part, objects).await? {
                let Some(tool_call_id) = message.tool_call_id.as_deref() else {
                    continue;
                };
                let expected = tool_calls.iter().any(|call| call.id == tool_call_id);
                if expected && seen_result_ids.insert(tool_call_id.to_string()) {
                    tool_results.push(message);
                }
            }
        }
    }

    flush_tool_batch(
        &mut history,
        &mut content_parts,
        &mut tool_calls,
        &mut tool_results,
        &mut seen_result_ids,
    );
    flush_assistant_content(&mut history, &mut content_parts);
    Ok(history)
}

fn flush_assistant_content(
    history: &mut Vec<LoopMessage>,
    content_parts: &mut Vec<ChatContentPart>,
) {
    if content_parts.is_empty() {
        return;
    }
    history.push(LoopMessage {
        role: "assistant".to_string(),
        content_parts: std::mem::take(content_parts),
        tool_calls: None,
        tool_call_id: None,
    });
}

fn flush_tool_batch(
    history: &mut Vec<LoopMessage>,
    content_parts: &mut Vec<ChatContentPart>,
    tool_calls: &mut Vec<ToolCall>,
    tool_results: &mut Vec<LoopMessage>,
    seen_result_ids: &mut std::collections::HashSet<String>,
) {
    if tool_calls.is_empty() {
        return;
    }

    let result_ids = tool_results
        .iter()
        .filter_map(|result| result.tool_call_id.as_deref())
        .map(str::to_string)
        .collect::<std::collections::HashSet<_>>();
    let matched_calls = tool_calls
        .iter()
        .filter(|call| result_ids.contains(&call.id))
        .cloned()
        .collect::<Vec<_>>();
    let matched_call_ids = matched_calls
        .iter()
        .map(|call| call.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let matched_results = tool_results
        .drain(..)
        .filter(|result| {
            result
                .tool_call_id
                .as_deref()
                .is_some_and(|id| matched_call_ids.contains(id))
        })
        .collect::<Vec<_>>();

    if matched_calls.is_empty() {
        tool_calls.clear();
        seen_result_ids.clear();
        return;
    }

    history.push(LoopMessage {
        role: "assistant".to_string(),
        content_parts: std::mem::take(content_parts),
        tool_calls: Some(matched_calls),
        tool_call_id: None,
    });
    history.extend(matched_results);
    tool_calls.clear();
    seen_result_ids.clear();
}

fn tool_call_from_part(part: &data_proto::SessionMessagePart) -> Option<ToolCall> {
    let payload: serde_json::Value =
        serde_json::from_str(&part.payload_json).unwrap_or(serde_json::Value::Null);
    let tool_call_id = payload.get("tool_call_id").and_then(|v| v.as_str())?;
    if tool_call_id.is_empty() {
        return None;
    }
    let input = payload
        .get("input")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    Some(ToolCall {
        id: tool_call_id.to_string(),
        name: part.name.clone(),
        arguments: tool_arguments_json(input),
    })
}

fn tool_arguments_json(input: serde_json::Value) -> String {
    match input {
        serde_json::Value::Object(_) => {
            serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string())
        }
        _ => "{}".to_string(),
    }
}

async fn tool_result_message_from_part(
    part: &data_proto::SessionMessagePart,
    objects: &(dyn ObjectStore + Send + Sync),
) -> Result<Option<LoopMessage>> {
    let payload: serde_json::Value =
        serde_json::from_str(&part.payload_json).unwrap_or(serde_json::Value::Null);
    let Some(tool_call_id) = payload.get("tool_call_id").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    if payload.get("tool_output").is_some() {
        let Some(parsed) = tool_output::parse_tool_result_payload_json(
            &part.payload_json,
            part.object.as_ref(),
            &part.content,
        )?
        else {
            return Ok(None);
        };
        return Ok(Some(LoopMessage {
            role: "tool".to_string(),
            content_parts: materialize_tool_output_content_parts(
                parsed.tool_output.content_parts(),
                objects,
            )
            .await?,
            tool_calls: None,
            tool_call_id: Some(parsed.tool_call_id),
        }));
    }
    let inline_output = payload
        .get("output")
        .and_then(|v| v.as_str())
        .or_else(|| payload.get("output_preview").and_then(|v| v.as_str()))
        .map(str::to_string)
        .unwrap_or_else(|| part.content.clone());
    let output = if let Some(object) = part.object.as_ref() {
        let mut object_ref = object.clone();
        let Some(metadata) = objects.head(&object_ref.key).await? else {
            let mut message = LoopMessage::text("tool", unavailable_historical_tool_output());
            message.tool_call_id = Some(tool_call_id.to_string());
            return Ok(Some(message));
        };
        if object_ref.media_type.trim().is_empty() {
            object_ref = object_ref_from_metadata(&object_ref.key, &metadata);
        }
        let mut media_type = object_ref.media_type.trim().to_string();
        if media_type.is_empty() {
            if let Some(inferred) = inferred_image_media_type(&object_ref.key) {
                media_type = inferred.to_string();
                object_ref.media_type = media_type.clone();
            }
        }
        if media_type.to_ascii_lowercase().starts_with("image/") {
            let mut message = LoopMessage {
                role: "tool".to_string(),
                content_parts: vec![object_ref_part(object_ref)],
                tool_calls: None,
                tool_call_id: Some(tool_call_id.to_string()),
            };
            if !inline_output.is_empty() {
                message.content_parts.insert(0, text_part(inline_output));
            }
            return Ok(Some(message));
        }
        if media_type.to_ascii_lowercase().starts_with("video/") {
            let label = if object_ref.filename.is_empty() {
                object_ref.key.as_str()
            } else {
                object_ref.filename.as_str()
            };
            let mut message = LoopMessage::text(
                "tool",
                format!(
                    "[Video: {} ({}; {} bytes)]",
                    label, media_type, object_ref.size_bytes
                ),
            );
            message.content_parts.push(object_ref_part(object_ref));
            message.tool_call_id = Some(tool_call_id.to_string());
            return Ok(Some(message));
        }
        if !tool_output::is_text_object_media_type(&media_type) {
            let label = if object_ref.filename.is_empty() {
                object_ref.key.as_str()
            } else {
                object_ref.filename.as_str()
            };
            let summary = if inline_output.is_empty() {
                tool_output::object_ref_summary(&media_type, label, object_ref.size_bytes)
            } else {
                inline_output
            };
            let mut message = LoopMessage::text("tool", summary);
            message.content_parts.push(object_ref_part(object_ref));
            message.tool_call_id = Some(tool_call_id.to_string());
            return Ok(Some(message));
        }
        let Some(stored) = objects.get(&object.key).await? else {
            let mut message = LoopMessage::text("tool", unavailable_historical_tool_output());
            message.tool_call_id = Some(tool_call_id.to_string());
            return Ok(Some(message));
        };
        let bytes = decode_stored_object_bytes(&stored, &object.key)?;
        let output = String::from_utf8_lossy(&bytes).into_owned();
        let mut message = LoopMessage::text("tool", output);
        message.tool_call_id = Some(tool_call_id.to_string());
        return Ok(Some(message));
    } else {
        inline_output
    };
    let mut message = LoopMessage::text("tool", output);
    message.tool_call_id = Some(tool_call_id.to_string());
    Ok(Some(message))
}

/// Converts text objects in a persisted typed tool result back into text before
/// the result is replayed into model context. Non-text objects remain references
/// so provider adapters can hydrate them in their native representation. A
/// missing historical object is represented explicitly so one deleted File
/// revision cannot prevent the entire session from replaying.
async fn materialize_tool_output_content_parts(
    parts: Vec<ChatContentPart>,
    objects: &(dyn ObjectStore + Send + Sync),
) -> Result<Vec<ChatContentPart>> {
    let mut materialized = Vec::with_capacity(parts.len());
    for content_part in parts {
        let Some(mut object_ref) = content_part_object_ref(&content_part).cloned() else {
            materialized.push(content_part);
            continue;
        };
        let Some(metadata) = objects.head(&object_ref.key).await? else {
            materialized.push(text_part(unavailable_historical_tool_output()));
            continue;
        };
        if object_ref.media_type.trim().is_empty() {
            object_ref = object_ref_from_metadata(&object_ref.key, &metadata);
        }
        if tool_output::is_text_object_media_type(&object_ref.media_type) {
            let Some(stored) = objects.get(&object_ref.key).await? else {
                materialized.push(text_part(unavailable_historical_tool_output()));
                continue;
            };
            let bytes = decode_stored_object_bytes(&stored, &object_ref.key)?;
            materialized.push(text_part(String::from_utf8_lossy(&bytes).into_owned()));
        } else {
            materialized.push(object_ref_part(object_ref));
        }
    }
    Ok(materialized)
}

fn unavailable_historical_tool_output() -> String {
    "[Historical tool output is unavailable. Do not assume it reflects the current state.]"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        load, message_content_parts, session_message_to_loop_messages,
        tool_result_message_from_part,
    };
    use crate::control::object_store::{InMemoryObjectStore, ObjectMetadata, ObjectStore};
    use crate::control::tool_output::{self, ToolOutputExt};
    use crate::control::KeyValueStore;
    use crate::gateway::rpc::data_proto;
    use crate::harness::llm::{content_part_object_ref, object_ref_part, text_part, ToolOutput};
    use crate::test_support::MockKvStore;
    use prost::Message;
    use std::collections::HashMap;

    fn tool_result_part(content: String, payload_json: String) -> data_proto::SessionMessagePart {
        data_proto::SessionMessagePart {
            id: "part-1".to_string(),
            part_type: data_proto::SessionMessagePartType::ToolResult as i32,
            content,
            name: "tool".to_string(),
            payload_json,
            created_at: 0,
            object: None,
        }
    }

    fn session_text_part(id: &str, content: &str) -> data_proto::SessionMessagePart {
        data_proto::SessionMessagePart {
            id: id.to_string(),
            part_type: data_proto::SessionMessagePartType::Text as i32,
            content: content.to_string(),
            name: String::new(),
            payload_json: String::new(),
            created_at: 0,
            object: None,
        }
    }

    fn tool_call_part(
        id: &str,
        name: &str,
        input: serde_json::Value,
    ) -> data_proto::SessionMessagePart {
        data_proto::SessionMessagePart {
            id: format!("call-{id}"),
            part_type: data_proto::SessionMessagePartType::ToolCall as i32,
            content: "Tool call".to_string(),
            name: name.to_string(),
            payload_json: serde_json::json!({
                "tool_call_id": id,
                "input": input,
            })
            .to_string(),
            created_at: 0,
            object: None,
        }
    }

    fn tool_result_part_for_call(
        id: &str,
        name: &str,
        output: &str,
    ) -> data_proto::SessionMessagePart {
        data_proto::SessionMessagePart {
            id: format!("result-{id}"),
            part_type: data_proto::SessionMessagePartType::ToolResult as i32,
            content: output.to_string(),
            name: name.to_string(),
            payload_json: serde_json::json!({
                "tool_call_id": id,
                "output_preview": output,
                "output": output,
            })
            .to_string(),
            created_at: 0,
            object: None,
        }
    }

    #[tokio::test]
    async fn load_starts_after_the_newest_valid_compaction_marker() {
        let kv = MockKvStore::new();
        let objects = InMemoryObjectStore::default();
        let summary_object = objects
            .put(
                "cas/test/sessions/session/compactions/submission/000001.txt",
                b"## Task\nResearch rates.",
                ObjectMetadata {
                    media_type: "text/markdown; charset=utf-8".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let old = data_proto::SessionMessage {
            id: "000001".to_string(),
            role: data_proto::MessageRole::RoleUser as i32,
            created_at: 0,
            labels: HashMap::new(),
            parts: vec![session_text_part("000001", "old context")],
        };
        let marker = data_proto::SessionMessage {
            id: "000002".to_string(),
            role: data_proto::MessageRole::RoleAssistant as i32,
            created_at: 0,
            labels: HashMap::new(),
            parts: vec![
                data_proto::SessionMessagePart {
                    id: "000001".to_string(),
                    part_type: data_proto::SessionMessagePartType::Compaction as i32,
                    content: String::new(),
                    name: String::new(),
                    payload_json: String::new(),
                    created_at: 0,
                    object: Some(summary_object),
                },
                session_text_part("000002", "retained assistant tail"),
            ],
        };
        let recent = data_proto::SessionMessage {
            id: "000003".to_string(),
            role: data_proto::MessageRole::RoleUser as i32,
            created_at: 0,
            labels: HashMap::new(),
            parts: vec![session_text_part("000001", "recent question")],
        };
        for message in [&old, &marker, &recent] {
            kv.set(
                &crate::control::keys::session_message("ns", "agent", "session", &message.id),
                &message.encode_to_vec(),
            )
            .await
            .unwrap();
        }

        let loaded = load(&kv, &objects, "ns", "agent", "session").await.unwrap();
        assert_eq!(
            loaded
                .messages
                .iter()
                .map(|message| message.text_content())
                .collect::<Vec<_>>(),
            [
                "## Task\nResearch rates.",
                "retained assistant tail",
                "recent question"
            ]
        );
    }

    #[tokio::test]
    async fn load_replays_after_a_replaced_file_object_is_missing() {
        let kv = MockKvStore::new();
        let objects = InMemoryObjectStore::default();
        let old_file_object = objects
            .put(
                "cas/Tenant%3Aacme/files/schedule-italki/old-version",
                b"# Previous iTalki booking instructions",
                ObjectMetadata {
                    media_type: "text/markdown".to_string(),
                    filename: "schedule-italki-lesson.md".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let output = ToolOutput::from_content_parts(
            vec![object_ref_part(old_file_object.clone())],
            "[Object: schedule-italki-lesson.md]",
        );
        let assistant = data_proto::SessionMessage {
            id: "assistant-1".to_string(),
            role: data_proto::MessageRole::RoleAssistant as i32,
            created_at: 0,
            labels: HashMap::new(),
            parts: vec![
                tool_call_part(
                    "read-file-1",
                    "read_file",
                    serde_json::json!({ "path": "/skills/schedule-italki-lesson.md" }),
                ),
                data_proto::SessionMessagePart {
                    id: "result-read-file-1".to_string(),
                    part_type: data_proto::SessionMessagePartType::ToolResult as i32,
                    content: String::new(),
                    name: "read_file".to_string(),
                    payload_json: tool_output::tool_result_payload_json("read-file-1", &output)
                        .unwrap(),
                    created_at: 0,
                    object: Some(old_file_object.clone()),
                },
                session_text_part("assistant-text", "I found the booking instructions."),
            ],
        };
        kv.set(
            &crate::control::keys::session_message("ns", "agent", "session", &assistant.id),
            &assistant.encode_to_vec(),
        )
        .await
        .unwrap();

        // File updates replace the live object and remove this old revision today.
        objects.delete(&old_file_object.key).await.unwrap();

        let loaded = load(&kv, &objects, "ns", "agent", "session").await.unwrap();

        assert!(loaded.messages.iter().any(|message|
            message.text_content() == "[Historical tool output is unavailable. Do not assume it reflects the current state.]"
        ));
        assert!(loaded
            .messages
            .iter()
            .any(|message| message.text_content() == "I found the booking instructions."));
    }

    fn assistant_message(parts: Vec<data_proto::SessionMessagePart>) -> data_proto::SessionMessage {
        data_proto::SessionMessage {
            id: "assistant-1".to_string(),
            role: data_proto::MessageRole::RoleAssistant as i32,
            created_at: 0,
            labels: HashMap::new(),
            parts,
        }
    }

    #[tokio::test]
    async fn tool_result_message_prefers_raw_output_when_present() {
        let store = InMemoryObjectStore::default();
        let raw_output = format!("{{\"payload\":\"{}\"}}", "x".repeat(10_000));
        let part = tool_result_part(
            "preview".to_string(),
            serde_json::json!({
                "tool_call_id": "tool-1",
                "output_preview": "small preview",
                "output": raw_output,
            })
            .to_string(),
        );

        let message = tool_result_message_from_part(&part, &store)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(message.tool_call_id.as_deref(), Some("tool-1"));
        assert_eq!(message.text_content(), raw_output);
    }

    #[tokio::test]
    async fn tool_result_message_keeps_legacy_raw_output() {
        let store = InMemoryObjectStore::default();
        let raw_output = format!(
            "{{\"payload\":\"{}\",\"items\":[\"{}\",\"{}\"]}}",
            "x".repeat(20_000),
            "y".repeat(8_000),
            "z".repeat(8_000)
        );
        let part = tool_result_part(
            raw_output.clone(),
            serde_json::json!({
                "tool_call_id": "tool-1",
                "output": raw_output,
            })
            .to_string(),
        );

        let message = tool_result_message_from_part(&part, &store)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(message.text_content(), raw_output);
    }

    #[tokio::test]
    async fn tool_result_message_requires_tool_call_id() {
        let store = InMemoryObjectStore::default();
        let part = tool_result_part(
            "preview".to_string(),
            serde_json::json!({
                "output_preview": "small preview",
            })
            .to_string(),
        );

        assert!(tool_result_message_from_part(&part, &store)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn tool_result_message_falls_back_to_step_content_when_payload_has_no_output() {
        let store = InMemoryObjectStore::default();
        let part = tool_result_part(
            "fallback output".to_string(),
            serde_json::json!({
                "tool_call_id": "tool-1"
            })
            .to_string(),
        );

        let message = tool_result_message_from_part(&part, &store)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(message.text_content(), "fallback output");
    }

    #[tokio::test]
    async fn tool_result_message_replays_typed_tool_output_content_parts() {
        let store = InMemoryObjectStore::default();
        let object = store
            .put(
                "cas/image.png",
                b"image-bytes",
                ObjectMetadata {
                    media_type: "image/png".to_string(),
                    size_bytes: 11,
                    filename: "image.png".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let output = ToolOutput::from_content_parts(
            vec![text_part("caption"), object_ref_part(object.clone())],
            "caption",
        );
        let part = tool_result_part(
            String::new(),
            tool_output::tool_result_payload_json("tool-1", &output).unwrap(),
        );

        let message = tool_result_message_from_part(&part, &store)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(message.text_content(), "caption");
        assert_eq!(
            message
                .content_parts
                .iter()
                .find_map(content_part_object_ref)
                .map(|object| object.key.as_str()),
            Some("cas/image.png")
        );
    }

    #[tokio::test]
    async fn tool_result_message_materializes_typed_text_object_output() {
        let store = InMemoryObjectStore::default();
        let raw_output = "Parallel search returned the latest interest-rate reporting.";
        let object = store
            .put(
                "sessions/acme/support/session-1/tool-results/search-result.txt",
                raw_output.as_bytes(),
                ObjectMetadata {
                    media_type: "text/plain; charset=utf-8".to_string(),
                    size_bytes: raw_output.len() as u64,
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let output = ToolOutput::from_content_parts(vec![object_ref_part(object)], "preview");
        let part = tool_result_part(
            String::new(),
            tool_output::tool_result_payload_json("tool-1", &output).unwrap(),
        );

        let message = tool_result_message_from_part(&part, &store)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(message.tool_call_id.as_deref(), Some("tool-1"));
        assert_eq!(message.text_content(), raw_output);
        assert!(message
            .content_parts
            .iter()
            .all(|part| content_part_object_ref(part).is_none()));
    }

    #[tokio::test]
    async fn tool_result_message_hydrates_object_output() {
        let store = InMemoryObjectStore::default();
        let raw_output = "full object output".repeat(100);
        let object = store
            .put(
                "sessions/acme/support/session-1/tool-results/tool-1.txt",
                raw_output.as_bytes(),
                ObjectMetadata {
                    media_type: "text/plain; charset=utf-8".to_string(),
                    size_bytes: raw_output.len() as u64,
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let mut part = tool_result_part(
            String::new(),
            serde_json::json!({
                "tool_call_id": "tool-1",
                "output_object_key": object.key,
            })
            .to_string(),
        );
        part.object = Some(object);

        let message = tool_result_message_from_part(&part, &store)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(message.text_content(), raw_output);
    }

    #[tokio::test]
    async fn tool_result_message_infers_image_media_type_from_object_key() {
        let store = InMemoryObjectStore::default();
        let object = store
            .put(
                "sessions/acme/support/session-1/tool-results/screenshot.png",
                b"png-bytes",
                ObjectMetadata::default(),
            )
            .await
            .unwrap();
        let mut part = tool_result_part(
            String::new(),
            serde_json::json!({
                "tool_call_id": "tool-1",
                "output_object_key": object.key,
            })
            .to_string(),
        );
        part.object = Some(object.clone());

        let message = tool_result_message_from_part(&part, &store)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(message.tool_call_id.as_deref(), Some("tool-1"));
        assert_eq!(message.content_parts.len(), 1);
        assert_eq!(
            content_part_object_ref(&message.content_parts[0])
                .unwrap()
                .key,
            object.key
        );
        assert_eq!(
            content_part_object_ref(&message.content_parts[0])
                .unwrap()
                .media_type,
            "image/png"
        );
    }

    #[tokio::test]
    async fn tool_result_message_preserves_video_object_ref_on_summary() {
        let store = InMemoryObjectStore::default();
        let object = store
            .put(
                "sessions/acme/support/session-1/tool-results/clip.mp4",
                b"video-bytes",
                ObjectMetadata {
                    media_type: "video/mp4".to_string(),
                    size_bytes: 11,
                    filename: "clip.mp4".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let mut part = tool_result_part(
            String::new(),
            serde_json::json!({
                "tool_call_id": "tool-1",
                "output_object_key": object.key,
            })
            .to_string(),
        );
        part.object = Some(object.clone());

        let message = tool_result_message_from_part(&part, &store)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(message.tool_call_id.as_deref(), Some("tool-1"));
        assert!(message.text_content().contains("[Video: clip.mp4"));
        assert_eq!(
            content_part_object_ref(&message.content_parts[1])
                .unwrap()
                .key,
            object.key
        );
    }

    #[tokio::test]
    async fn tool_result_message_preserves_binary_legacy_object_ref() {
        let store = InMemoryObjectStore::default();
        let object = store
            .put(
                "sessions/acme/support/session-1/tool-results/blob.bin",
                &[0x66, 0x6f, 0xff, 0x6f],
                ObjectMetadata {
                    media_type: "application/octet-stream".to_string(),
                    size_bytes: 4,
                    filename: "blob.bin".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let mut part = tool_result_part(
            String::new(),
            serde_json::json!({
                "tool_call_id": "tool-1",
                "output_object_key": object.key,
            })
            .to_string(),
        );
        part.object = Some(object.clone());

        let message = tool_result_message_from_part(&part, &store)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(message.tool_call_id.as_deref(), Some("tool-1"));
        assert!(message.text_content().contains("[Object: blob.bin"));
        assert_eq!(
            content_part_object_ref(&message.content_parts[1])
                .unwrap()
                .key,
            object.key
        );
    }

    #[tokio::test]
    async fn tool_result_message_marks_missing_legacy_object_output_as_unavailable() {
        let store = InMemoryObjectStore::default();
        let mut part = tool_result_part(
            String::new(),
            serde_json::json!({
                "tool_call_id": "tool-1",
                "output_object_key": "missing.txt",
            })
            .to_string(),
        );
        part.object = Some(data_proto::ObjectRef {
            key: "missing.txt".to_string(),
            ..Default::default()
        });

        let message = tool_result_message_from_part(&part, &store)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(message.tool_call_id.as_deref(), Some("tool-1"));
        assert_eq!(
            message.text_content(),
            "[Historical tool output is unavailable. Do not assume it reflects the current state.]"
        );
    }

    #[tokio::test]
    async fn tool_result_message_marks_missing_typed_object_output_as_unavailable() {
        let store = InMemoryObjectStore::default();
        let output = ToolOutput::from_content_parts(
            vec![object_ref_part(data_proto::ObjectRef {
                key: "cas/Tenant%3Aacme/files/italki/old-version".to_string(),
                media_type: "text/markdown".to_string(),
                filename: "schedule-italki-lesson.md".to_string(),
                ..Default::default()
            })],
            "[Object: schedule-italki-lesson.md]",
        );
        let part = tool_result_part(
            String::new(),
            tool_output::tool_result_payload_json("read-file-1", &output).unwrap(),
        );

        let message = tool_result_message_from_part(&part, &store)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(message.tool_call_id.as_deref(), Some("read-file-1"));
        assert_eq!(
            message.text_content(),
            "[Historical tool output is unavailable. Do not assume it reflects the current state.]"
        );
    }

    #[tokio::test]
    async fn assistant_session_message_replays_interleaved_tool_cycles_in_order() {
        let store = InMemoryObjectStore::default();
        let message = assistant_message(vec![
            session_text_part("000001", "before A. "),
            tool_call_part("call-a", "search", serde_json::json!({ "q": "a" })),
            tool_result_part_for_call("call-a", "search", "result-a"),
            session_text_part("000004", "before B. "),
            tool_call_part("call-b", "fetch", serde_json::json!({ "id": "b" })),
            tool_result_part_for_call("call-b", "fetch", "result-b"),
            session_text_part("000007", "final."),
        ]);

        let history = session_message_to_loop_messages(&message, &store)
            .await
            .unwrap();

        assert_eq!(history.len(), 5);
        assert_eq!(history[0].role, "assistant");
        assert_eq!(history[0].text_content(), "before A. ");
        assert_eq!(history[0].tool_calls.as_ref().unwrap()[0].id, "call-a");
        assert_eq!(history[1].role, "tool");
        assert_eq!(history[1].tool_call_id.as_deref(), Some("call-a"));
        assert_eq!(history[1].text_content(), "result-a");
        assert_eq!(history[2].role, "assistant");
        assert_eq!(history[2].text_content(), "before B. ");
        assert_eq!(history[2].tool_calls.as_ref().unwrap()[0].id, "call-b");
        assert_eq!(history[3].role, "tool");
        assert_eq!(history[3].tool_call_id.as_deref(), Some("call-b"));
        assert_eq!(history[3].text_content(), "result-b");
        assert_eq!(history[4].role, "assistant");
        assert_eq!(history[4].text_content(), "final.");
        assert!(history[4].tool_calls.is_none());
    }

    #[tokio::test]
    async fn assistant_session_message_drops_tool_call_without_result() {
        let store = InMemoryObjectStore::default();
        let message = assistant_message(vec![
            session_text_part("000001", "before. "),
            tool_call_part(
                "call-missing",
                "search",
                serde_json::json!({ "q": "missing" }),
            ),
            session_text_part("000003", "after."),
        ]);

        let history = session_message_to_loop_messages(&message, &store)
            .await
            .unwrap();

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].role, "assistant");
        assert_eq!(history[0].text_content(), "before. after.");
        assert!(history[0].tool_calls.is_none());
    }

    #[tokio::test]
    async fn assistant_session_message_keeps_only_matched_calls_in_partial_batch() {
        let store = InMemoryObjectStore::default();
        let message = assistant_message(vec![
            session_text_part("000001", "checking. "),
            tool_call_part(
                "call-missing",
                "search",
                serde_json::json!({ "q": "missing" }),
            ),
            tool_call_part("call-ok", "fetch", serde_json::json!({ "id": "ok" })),
            tool_result_part_for_call("call-ok", "fetch", "result-ok"),
        ]);

        let history = session_message_to_loop_messages(&message, &store)
            .await
            .unwrap();

        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, "assistant");
        assert_eq!(history[0].text_content(), "checking. ");
        let calls = history[0].tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call-ok");
        assert_eq!(history[1].role, "tool");
        assert_eq!(history[1].tool_call_id.as_deref(), Some("call-ok"));
        assert_eq!(history[1].text_content(), "result-ok");
    }

    #[tokio::test]
    async fn assistant_session_message_keeps_interleaved_results_in_one_batch_until_text() {
        let store = InMemoryObjectStore::default();
        let message = assistant_message(vec![
            session_text_part("000001", "checking. "),
            tool_call_part("call-a", "search", serde_json::json!({ "q": "a" })),
            tool_result_part_for_call("call-a", "search", "result-a"),
            tool_call_part("call-b", "fetch", serde_json::json!({ "id": "b" })),
            tool_result_part_for_call("call-b", "fetch", "result-b"),
            session_text_part("000006", "done."),
        ]);

        let history = session_message_to_loop_messages(&message, &store)
            .await
            .unwrap();

        assert_eq!(history.len(), 4);
        assert_eq!(history[0].role, "assistant");
        assert_eq!(history[0].text_content(), "checking. ");
        let calls = history[0].tool_calls.as_ref().unwrap();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.id.as_str())
                .collect::<Vec<_>>(),
            vec!["call-a", "call-b"]
        );
        assert_eq!(history[1].tool_call_id.as_deref(), Some("call-a"));
        assert_eq!(history[1].text_content(), "result-a");
        assert_eq!(history[2].tool_call_id.as_deref(), Some("call-b"));
        assert_eq!(history[2].text_content(), "result-b");
        assert_eq!(history[3].role, "assistant");
        assert_eq!(history[3].text_content(), "done.");
    }

    #[tokio::test]
    async fn assistant_session_message_preserves_tool_result_order_within_batch() {
        let store = InMemoryObjectStore::default();
        let message = assistant_message(vec![
            session_text_part("000001", "checking. "),
            tool_call_part("call-a", "search", serde_json::json!({ "q": "a" })),
            tool_call_part("call-b", "fetch", serde_json::json!({ "id": "b" })),
            tool_result_part_for_call("call-b", "fetch", "result-b"),
            tool_result_part_for_call("call-a", "search", "result-a"),
            session_text_part("000006", "done."),
        ]);

        let history = session_message_to_loop_messages(&message, &store)
            .await
            .unwrap();

        assert_eq!(history.len(), 4);
        let calls = history[0].tool_calls.as_ref().unwrap();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.id.as_str())
                .collect::<Vec<_>>(),
            vec!["call-a", "call-b"]
        );
        assert_eq!(history[1].tool_call_id.as_deref(), Some("call-b"));
        assert_eq!(history[1].text_content(), "result-b");
        assert_eq!(history[2].tool_call_id.as_deref(), Some("call-a"));
        assert_eq!(history[2].text_content(), "result-a");
        assert_eq!(history[3].role, "assistant");
        assert_eq!(history[3].text_content(), "done.");
    }

    #[tokio::test]
    async fn assistant_session_message_ignores_orphan_tool_results() {
        let store = InMemoryObjectStore::default();
        let message = assistant_message(vec![
            session_text_part("000001", "before. "),
            tool_result_part_for_call("orphan", "search", "orphan-result"),
            session_text_part("000003", "after."),
        ]);

        let history = session_message_to_loop_messages(&message, &store)
            .await
            .unwrap();

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].role, "assistant");
        assert_eq!(history[0].text_content(), "before. after.");
        assert!(history[0].tool_calls.is_none());
    }

    #[tokio::test]
    async fn assistant_session_message_drops_invalid_calls_and_duplicate_results() {
        let store = InMemoryObjectStore::default();
        let mut invalid_call = tool_call_part("", "search", serde_json::json!({ "q": "bad" }));
        invalid_call.id = "invalid-call".to_string();
        let message = assistant_message(vec![
            session_text_part("000001", "checking. "),
            invalid_call,
            tool_call_part("call-ok", "fetch", serde_json::json!({ "id": "ok" })),
            tool_result_part_for_call("call-ok", "fetch", "first-result"),
            tool_result_part_for_call("call-ok", "fetch", "duplicate-result"),
        ]);

        let history = session_message_to_loop_messages(&message, &store)
            .await
            .unwrap();

        assert_eq!(history.len(), 2);
        let calls = history[0].tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call-ok");
        assert_eq!(history[1].tool_call_id.as_deref(), Some("call-ok"));
        assert_eq!(history[1].text_content(), "first-result");
    }

    #[tokio::test]
    async fn assistant_session_message_replays_missing_tool_input_as_empty_object() {
        let store = InMemoryObjectStore::default();
        let call = data_proto::SessionMessagePart {
            id: "000001".to_string(),
            part_type: data_proto::SessionMessagePartType::ToolCall as i32,
            content: "Tool call".to_string(),
            name: "list_links".to_string(),
            payload_json: serde_json::json!({
                "tool_call_id": "call-empty",
            })
            .to_string(),
            created_at: 0,
            object: None,
        };
        let message = assistant_message(vec![
            call,
            tool_result_part_for_call("call-empty", "list_links", "[]"),
        ]);

        let history = session_message_to_loop_messages(&message, &store)
            .await
            .unwrap();

        let calls = history[0].tool_calls.as_ref().unwrap();
        assert_eq!(calls[0].arguments, "{}");
    }

    #[tokio::test]
    async fn message_content_parts_infers_missing_image_media_type_from_extension() {
        let store = InMemoryObjectStore::default();
        let object = store
            .put(
                "sessions/session-1/screenshot.jpeg",
                b"jpeg-bytes",
                ObjectMetadata::default(),
            )
            .await
            .unwrap();
        let message = data_proto::SessionMessage {
            id: "msg-1".to_string(),
            role: data_proto::MessageRole::RoleUser as i32,
            created_at: 2,
            labels: HashMap::new(),
            parts: vec![data_proto::SessionMessagePart {
                id: "000001".to_string(),
                part_type: data_proto::SessionMessagePartType::Image as i32,
                content: String::new(),
                name: String::new(),
                payload_json: String::new(),
                created_at: 2,
                object: Some(object),
            }],
        };

        let parts = message_content_parts(&message, &store).await.unwrap();

        assert_eq!(parts.len(), 1);
        assert_eq!(
            content_part_object_ref(&parts[0]).unwrap().media_type,
            "image/jpeg"
        );
    }

    #[tokio::test]
    async fn message_content_parts_uses_object_ref_for_image_object() {
        let store = InMemoryObjectStore::default();
        let object = store
            .put(
                "sessions/session-1/screenshot.png",
                b"png-bytes",
                ObjectMetadata {
                    media_type: "image/png".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let message = data_proto::SessionMessage {
            id: "msg-1".to_string(),
            role: data_proto::MessageRole::RoleUser as i32,
            created_at: 2,
            labels: HashMap::new(),
            parts: vec![data_proto::SessionMessagePart {
                id: "000001".to_string(),
                part_type: data_proto::SessionMessagePartType::Image as i32,
                content: String::new(),
                name: String::new(),
                payload_json: serde_json::json!({ "detail": "low" }).to_string(),
                created_at: 2,
                object: Some(object.clone()),
            }],
        };

        let parts = message_content_parts(&message, &store).await.unwrap();

        assert_eq!(parts.len(), 1);
        assert_eq!(content_part_object_ref(&parts[0]).unwrap().key, object.key);
    }

    #[tokio::test]
    async fn message_content_parts_rejects_non_image_object_media_type() {
        let store = InMemoryObjectStore::default();
        let object = store
            .put(
                "sessions/session-1/file.txt",
                b"text",
                ObjectMetadata {
                    media_type: "text/plain".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let message = data_proto::SessionMessage {
            id: "msg-1".to_string(),
            role: data_proto::MessageRole::RoleUser as i32,
            created_at: 2,
            labels: HashMap::new(),
            parts: vec![data_proto::SessionMessagePart {
                id: "000001".to_string(),
                part_type: data_proto::SessionMessagePartType::Image as i32,
                content: String::new(),
                name: String::new(),
                payload_json: String::new(),
                created_at: 2,
                object: Some(object),
            }],
        };

        let err = message_content_parts(&message, &store).await.unwrap_err();

        assert!(err.to_string().contains(
            "unsupported media type 'text/plain' for image object 'sessions/session-1/file.txt'"
        ));
    }

    #[tokio::test]
    async fn message_content_parts_rejects_unknown_image_media_type() {
        let store = InMemoryObjectStore::default();
        let object = store
            .put(
                "sessions/session-1/upload",
                b"unknown-bytes",
                ObjectMetadata::default(),
            )
            .await
            .unwrap();
        let message = data_proto::SessionMessage {
            id: "msg-1".to_string(),
            role: data_proto::MessageRole::RoleUser as i32,
            created_at: 2,
            labels: HashMap::new(),
            parts: vec![data_proto::SessionMessagePart {
                id: "000001".to_string(),
                part_type: data_proto::SessionMessagePartType::Image as i32,
                content: String::new(),
                name: String::new(),
                payload_json: String::new(),
                created_at: 2,
                object: Some(object),
            }],
        };

        let err = message_content_parts(&message, &store).await.unwrap_err();

        assert!(err
            .to_string()
            .contains("missing media type for image object 'sessions/session-1/upload'"));
    }
}
