// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::cas::decode_stored_object_bytes;
use crate::control::object_store::ObjectStore;
use crate::gateway::rpc::data_proto;
use anyhow::{anyhow, Result};
use std::sync::OnceLock;

const DEFAULT_TOOL_RESULT_OBJECT_THRESHOLD_BYTES: usize = 2 * 1024;

pub fn tool_result_payload_json(
    tool_call_id: &str,
    inline_output: Option<&str>,
    object: Option<&data_proto::ObjectRef>,
) -> String {
    let mut value = serde_json::json!({
        "tool_call_id": tool_call_id,
    });
    if let Some(object) = object {
        value["output_object_key"] = serde_json::Value::String(object.key.clone());
    } else if let Some(output) = inline_output {
        value["output"] = serde_json::Value::String(output.to_string());
    }
    serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
}

pub async fn hydrate_tool_result(
    objects: &(dyn ObjectStore + Send + Sync),
    object: Option<&data_proto::ObjectRef>,
    inline_output: &str,
) -> Result<String> {
    let Some(object) = object else {
        return Ok(inline_output.to_string());
    };
    let stored = objects
        .get(&object.key)
        .await?
        .ok_or_else(|| anyhow!("tool result object '{}' is missing", object.key))?;
    let bytes = decode_stored_object_bytes(&stored, &object.key)?;
    String::from_utf8(bytes).map_err(|err| {
        anyhow!(
            "tool result object '{}' is not valid UTF-8: {err}",
            object.key
        )
    })
}

pub fn tool_result_object_threshold_bytes() -> usize {
    static CACHE: OnceLock<usize> = OnceLock::new();
    *CACHE.get_or_init(parse_tool_result_object_threshold_bytes)
}

fn parse_tool_result_object_threshold_bytes() -> usize {
    match std::env::var("TALON_SESSION_TOOL_RESULT_OBJECT_THRESHOLD_BYTES") {
        Ok(raw) => match raw.trim().parse::<usize>() {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(
                    value = %raw,
                    error = %error,
                    default_bytes = DEFAULT_TOOL_RESULT_OBJECT_THRESHOLD_BYTES,
                    "Ignoring invalid TALON_SESSION_TOOL_RESULT_OBJECT_THRESHOLD_BYTES"
                );
                DEFAULT_TOOL_RESULT_OBJECT_THRESHOLD_BYTES
            }
        },
        Err(_) => DEFAULT_TOOL_RESULT_OBJECT_THRESHOLD_BYTES,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        hydrate_tool_result, tool_result_object_threshold_bytes, tool_result_payload_json,
    };
    use crate::control::cas::CasStore;
    use crate::control::object_store::{InMemoryObjectStore, ObjectStore};
    use std::sync::Arc;

    #[tokio::test]
    async fn small_tool_result_stays_inline() {
        let store = Arc::new(InMemoryObjectStore::default());
        let cas = CasStore::new(store.clone());
        let object = cas
            .put_tool_result_if_raw_at_least(
                "acme",
                "support",
                "session-1",
                "message-1",
                "000001",
                "call-1",
                "search",
                b"small result",
                tool_result_object_threshold_bytes(),
            )
            .await
            .unwrap();

        assert!(object.is_none());
        let payload: serde_json::Value = serde_json::from_str(&tool_result_payload_json(
            "call-1",
            Some("small result"),
            None,
        ))
        .unwrap();
        assert_eq!(payload["output"], "small result");
        assert!(payload.get("output_preview").is_none());
        assert!(payload.get("output_object_key").is_none());
    }

    #[tokio::test]
    async fn large_compressible_tool_result_is_written_to_gzip_object() {
        let store = Arc::new(InMemoryObjectStore::default());
        let cas = CasStore::new(store.clone());
        let raw = "x".repeat(3 * 1024);
        let object = cas
            .put_tool_result_if_raw_at_least(
                "acme",
                "support",
                "session-1",
                "message-1",
                "000001",
                "call-1",
                "search",
                raw.as_bytes(),
                tool_result_object_threshold_bytes(),
            )
            .await
            .unwrap()
            .expect("large result should have object ref");

        assert_eq!(
            object.key,
            "cas/acme/sessions/session-1/messages/message-1/000001.txt"
        );
        assert_eq!(object.media_type, "text/plain; charset=utf-8");
        assert!(object.size_bytes < raw.len() as u64);
        assert_eq!(object.metadata["kind"], "tool_result");
        assert_eq!(object.metadata["message_id"], "message-1");
        assert_eq!(object.metadata["part_id"], "000001");
        assert_eq!(object.metadata["tool_call_id"], "call-1");
        assert_eq!(object.metadata["content_encoding"], "gzip");
        assert_eq!(
            object.metadata["uncompressed_size_bytes"],
            raw.len().to_string()
        );
        let stored = store.get(&object.key).await.unwrap().unwrap();
        assert!(stored.bytes.len() < raw.len());
        let hydrated = hydrate_tool_result(store.as_ref(), Some(&object), "")
            .await
            .unwrap();
        assert_eq!(hydrated, raw);
        let payload: serde_json::Value =
            serde_json::from_str(&tool_result_payload_json("call-1", None, Some(&object))).unwrap();
        assert!(payload.get("output").is_none());
        assert!(payload.get("output_preview").is_none());
        assert_eq!(payload["output_object_key"], object.key);
    }

    #[test]
    fn object_key_segments_are_sanitized_deterministically() {
        let store = CasStore::new(Arc::new(InMemoryObjectStore::default()));
        assert_eq!(
            store.session_object_key(
                &crate::control::cas::SessionCasScope::new("team/alpha", "agent", "session one"),
                &crate::control::cas::SessionObjectIdentity::new("message#1", "../part id"),
            ),
            "cas/team%2Falpha/sessions/session_one/messages/message_1/.._part_id.txt"
        );
    }

    #[tokio::test]
    async fn object_metadata_filename_uses_sanitized_tool_call_id() {
        let store = Arc::new(InMemoryObjectStore::default());
        let cas = CasStore::new(store);
        let object = cas
            .put_tool_result_if_raw_at_least(
                "acme",
                "support",
                "session-1",
                "message-1",
                "000001",
                "../tool call",
                "search",
                "x".repeat(3 * 1024).as_bytes(),
                tool_result_object_threshold_bytes(),
            )
            .await
            .unwrap()
            .expect("large result should have object ref");

        assert_eq!(object.filename, ".._tool_call.txt");
    }
}
