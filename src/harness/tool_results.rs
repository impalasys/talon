// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::object_store::{ObjectMetadata, ObjectStore};
use crate::gateway::rpc::data_proto;
use crate::harness::executor::compaction::tool_result_preview;
use anyhow::{anyhow, Result};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::OnceLock;

const DEFAULT_TOOL_RESULT_OBJECT_THRESHOLD_BYTES: usize = 2 * 1024;
const MIN_GZIP_SAVINGS_PERCENT: usize = 10;
const TOOL_RESULT_MEDIA_TYPE: &str = "text/plain; charset=utf-8";

#[derive(Debug, Clone, PartialEq)]
pub struct StoredToolResult {
    pub part_id: String,
    pub output: String,
    pub preview: String,
    pub object: Option<data_proto::ObjectRef>,
}

impl StoredToolResult {
    pub fn payload_json(&self, tool_call_id: &str) -> String {
        let mut value = serde_json::json!({
            "tool_call_id": tool_call_id,
            "output_preview": self.preview,
        });
        if let Some(object) = self.object.as_ref() {
            value["output_object_key"] = serde_json::Value::String(object.key.clone());
        } else {
            value["output"] = serde_json::Value::String(self.output.clone());
        }
        serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
    }
}

pub async fn store_tool_result(
    objects: &(dyn ObjectStore + Send + Sync),
    ns: &str,
    agent: &str,
    session_id: &str,
    message_id: &str,
    part_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    result: &str,
) -> Result<StoredToolResult> {
    let preview = tool_result_preview(result);
    if result.len() < tool_result_object_threshold_bytes() {
        return Ok(StoredToolResult {
            part_id: part_id.to_string(),
            output: result.to_string(),
            preview,
            object: None,
        });
    }

    let key = tool_result_object_key(ns, session_id, message_id, part_id);
    let raw_bytes = result.as_bytes();
    let (bytes, content_encoding) = compressed_object_bytes(raw_bytes)?;
    let mut metadata = HashMap::new();
    metadata.insert("kind".to_string(), "tool_result".to_string());
    metadata.insert("namespace".to_string(), ns.to_string());
    metadata.insert("agent".to_string(), agent.to_string());
    metadata.insert("session_id".to_string(), session_id.to_string());
    metadata.insert("message_id".to_string(), message_id.to_string());
    metadata.insert("part_id".to_string(), part_id.to_string());
    metadata.insert("tool_call_id".to_string(), tool_call_id.to_string());
    metadata.insert("tool_name".to_string(), tool_name.to_string());
    metadata.insert(
        "uncompressed_size_bytes".to_string(),
        raw_bytes.len().to_string(),
    );
    metadata.insert("uncompressed_sha256".to_string(), sha256_hex(raw_bytes));
    if let Some(content_encoding) = content_encoding {
        metadata.insert("content_encoding".to_string(), content_encoding.to_string());
    }

    let object = objects
        .put(
            &key,
            &bytes,
            ObjectMetadata {
                media_type: TOOL_RESULT_MEDIA_TYPE.to_string(),
                size_bytes: bytes.len() as u64,
                sha256: sha256_hex(&bytes),
                filename: format!("{}.txt", object_key_segment(tool_call_id)),
                metadata,
            },
        )
        .await?;

    Ok(StoredToolResult {
        part_id: part_id.to_string(),
        output: preview.clone(),
        preview,
        object: Some(object),
    })
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
    let bytes = if stored
        .metadata
        .metadata
        .get("content_encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("gzip"))
        || object
            .metadata
            .get("content_encoding")
            .is_some_and(|value| value.eq_ignore_ascii_case("gzip"))
    {
        gunzip(&stored.bytes, &object.key)?
    } else {
        stored.bytes
    };
    String::from_utf8(bytes).map_err(|err| {
        anyhow!(
            "tool result object '{}' is not valid UTF-8: {err}",
            object.key
        )
    })
}

fn tool_result_object_threshold_bytes() -> usize {
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

fn tool_result_object_key(ns: &str, session_id: &str, message_id: &str, part_id: &str) -> String {
    format!(
        "cas/{}/sessions/{}/messages/{}/{}.txt",
        encoded_object_key_segment(ns),
        object_key_segment(session_id),
        object_key_segment(message_id),
        object_key_segment(part_id)
    )
}

fn encoded_object_key_segment(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}

fn object_key_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn compressed_object_bytes(raw_bytes: &[u8]) -> Result<(Vec<u8>, Option<&'static str>)> {
    let gzipped = gzip(raw_bytes)?;
    if (gzipped.len() as u64) * 100
        < (raw_bytes.len() as u64) * (100 - MIN_GZIP_SAVINGS_PERCENT) as u64
    {
        Ok((gzipped, Some("gzip")))
    } else {
        Ok((raw_bytes.to_vec(), None))
    }
}

fn gzip(raw_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(raw_bytes)?;
    Ok(encoder.finish()?)
}

fn gunzip(bytes: &[u8], key: &str) -> Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(bytes);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|err| anyhow!("tool result object '{key}' has invalid gzip bytes: {err}"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::store_tool_result;
    use crate::control::object_store::{InMemoryObjectStore, ObjectStore};
    use rand::{Rng, SeedableRng};

    #[tokio::test]
    async fn small_tool_result_stays_inline() {
        let store = InMemoryObjectStore::default();
        let result = store_tool_result(
            &store,
            "acme",
            "support",
            "session-1",
            "message-1",
            "000001",
            "call-1",
            "search",
            "small result",
        )
        .await
        .unwrap();

        assert_eq!(result.output, "small result");
        assert_eq!(result.preview, "small result");
        assert!(result.object.is_none());
    }

    #[tokio::test]
    async fn large_compressible_tool_result_is_written_to_gzip_object() {
        let store = InMemoryObjectStore::default();
        let raw = "x".repeat(3 * 1024);
        let result = store_tool_result(
            &store,
            "acme",
            "support",
            "session-1",
            "message-1",
            "000001",
            "call-1",
            "search",
            &raw,
        )
        .await
        .unwrap();

        let object = result.object.expect("large result should have object ref");
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
        let hydrated = super::hydrate_tool_result(&store, Some(&object), "")
            .await
            .unwrap();
        assert_eq!(hydrated, raw);
        assert_eq!(result.output, result.preview);
    }

    #[test]
    fn incompressible_bytes_are_kept_raw() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let raw = (0..2 * 1024)
            .map(|_| rng.gen_range(0u8..=0xff))
            .collect::<Vec<_>>();
        let (bytes, encoding) = super::compressed_object_bytes(&raw).unwrap();

        assert_eq!(bytes, raw);
        assert!(encoding.is_none());
    }

    #[test]
    fn object_key_segments_are_sanitized_deterministically() {
        assert_eq!(
            super::tool_result_object_key("team/alpha", "session one", "message#1", "../part id"),
            "cas/team%2Falpha/sessions/session_one/messages/message_1/.._part_id.txt"
        );
    }

    #[tokio::test]
    async fn object_metadata_filename_uses_sanitized_tool_call_id() {
        let store = InMemoryObjectStore::default();
        let result = store_tool_result(
            &store,
            "acme",
            "support",
            "session-1",
            "message-1",
            "000001",
            "../tool call",
            "search",
            &"x".repeat(3 * 1024),
        )
        .await
        .unwrap();

        let object = result.object.expect("large result should have object ref");
        assert_eq!(object.filename, ".._tool_call.txt");
    }

    #[tokio::test]
    async fn corrupt_gzip_object_returns_error() {
        let store = InMemoryObjectStore::default();
        let object = store
            .put(
                "cas/acme/sessions/session-1/messages/message-1/000001.txt",
                b"not gzip",
                crate::control::object_store::ObjectMetadata {
                    metadata: std::collections::HashMap::from([(
                        "content_encoding".to_string(),
                        "gzip".to_string(),
                    )]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let err = super::hydrate_tool_result(&store, Some(&object), "")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid gzip bytes"));
    }
}
