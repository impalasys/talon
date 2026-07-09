// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::runtime::LoopMessage;
use crate::control::object_store::ObjectStore;
use crate::gateway::rpc::data_proto;
use crate::harness::llm::{image_data_part, image_url_part, text_part, ChatContentPart, ToolCall};
use crate::harness::tool_results::hydrate_tool_result;
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use std::path::Path;

pub async fn session_message_to_loop_messages(
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
    let detail = payload
        .get("detail")
        .and_then(|value| value.as_str())
        .map(ToString::to_string);
    if let Some(url) = payload.get("url").and_then(|value| value.as_str()) {
        content_parts.push(image_url_part(url.to_string(), detail));
        return Ok(content_parts);
    }

    let Some(object) = part.object.as_ref() else {
        return Ok(content_parts);
    };
    let stored = objects.get(&object.key).await?.ok_or_else(|| {
        anyhow!(
            "object '{}' referenced by message part is missing",
            object.key
        )
    })?;
    let mut media_type = if object.media_type.trim().is_empty() {
        stored.metadata.media_type.trim().to_string()
    } else {
        object.media_type.trim().to_string()
    };
    if media_type.is_empty() {
        media_type = inferred_image_media_type(&object.key)
            .ok_or_else(|| anyhow!("missing media type for image object '{}'", object.key))?
            .to_string();
    }
    if !media_type.to_ascii_lowercase().starts_with("image/") {
        return Err(anyhow!(
            "unsupported media type '{}' for image object '{}'",
            media_type,
            object.key
        ));
    }
    content_parts.push(image_data_part(
        media_type,
        general_purpose::STANDARD.encode(stored.bytes),
        detail,
    ));
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
        arguments: serde_json::to_string(&input).unwrap_or_else(|_| "null".to_string()),
    })
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
    let inline_output = payload
        .get("output")
        .and_then(|v| v.as_str())
        .or_else(|| payload.get("output_preview").and_then(|v| v.as_str()))
        .map(str::to_string)
        .unwrap_or_else(|| part.content.clone());
    let output = hydrate_tool_result(objects, part.object.as_ref(), &inline_output).await?;
    let mut message = LoopMessage::text("tool", output);
    message.tool_call_id = Some(tool_call_id.to_string());
    Ok(Some(message))
}

#[cfg(test)]
mod tests {
    use super::{
        message_content_parts, session_message_to_loop_messages, tool_result_message_from_part,
    };
    use crate::control::object_store::{InMemoryObjectStore, ObjectMetadata, ObjectStore};
    use crate::gateway::rpc::data_proto;
    use crate::harness::llm::image_data_part;
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
            "preview".to_string(),
            serde_json::json!({
                "tool_call_id": "tool-1",
                "output_preview": "preview",
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
    async fn tool_result_message_errors_when_object_is_missing() {
        let store = InMemoryObjectStore::default();
        let mut part = tool_result_part(
            "preview".to_string(),
            serde_json::json!({
                "tool_call_id": "tool-1",
                "output_preview": "preview",
                "output_object_key": "missing.txt",
            })
            .to_string(),
        );
        part.object = Some(data_proto::ObjectRef {
            key: "missing.txt".to_string(),
            ..Default::default()
        });

        let err = tool_result_message_from_part(&part, &store)
            .await
            .unwrap_err();

        assert!(err.to_string().contains("missing"));
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

        assert_eq!(
            parts,
            vec![image_data_part(
                "image/jpeg",
                "anBlZy1ieXRlcw==",
                None::<String>
            )]
        );
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
