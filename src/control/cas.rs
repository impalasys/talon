// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::object_store::{ObjectMetadata, ObjectStore, StoredObject};
use crate::gateway::rpc::data_proto;
use anyhow::{anyhow, Result};
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;

const TOOL_RESULT_MEDIA_TYPE: &str = "text/plain; charset=utf-8";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCasScope {
    pub ns: String,
    pub agent: String,
    pub session_id: String,
}

impl SessionCasScope {
    pub fn new(ns: &str, agent: &str, session_id: &str) -> Self {
        Self {
            ns: ns.to_string(),
            agent: agent.to_string(),
            session_id: session_id.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionObjectIdentity {
    pub message_id: String,
    pub part_id: String,
}

impl SessionObjectIdentity {
    pub fn new(message_id: &str, part_id: &str) -> Self {
        Self {
            message_id: message_id.to_string(),
            part_id: part_id.to_string(),
        }
    }
}

#[derive(Clone)]
pub struct CasStore {
    objects: Arc<dyn ObjectStore + Send + Sync>,
}

impl CasStore {
    pub fn new(objects: Arc<dyn ObjectStore + Send + Sync>) -> Self {
        Self { objects }
    }

    pub fn object_store(&self) -> &(dyn ObjectStore + Send + Sync) {
        self.objects.as_ref()
    }

    pub fn session_object_key(
        &self,
        scope: &SessionCasScope,
        identity: &SessionObjectIdentity,
    ) -> String {
        session_object_key(scope, identity)
    }

    pub async fn put_tool_result(
        &self,
        scope: &SessionCasScope,
        identity: &SessionObjectIdentity,
        tool_call_id: &str,
        tool_name: &str,
        stored_bytes: &[u8],
        uncompressed_bytes: &[u8],
        content_encoding: Option<&str>,
    ) -> Result<data_proto::ObjectRef> {
        let mut metadata = session_object_metadata(scope, identity);
        metadata.insert("kind".to_string(), "tool_result".to_string());
        metadata.insert("tool_call_id".to_string(), tool_call_id.to_string());
        metadata.insert("tool_name".to_string(), tool_name.to_string());
        metadata.insert(
            "uncompressed_size_bytes".to_string(),
            uncompressed_bytes.len().to_string(),
        );
        metadata.insert(
            "uncompressed_sha256".to_string(),
            sha256_hex(uncompressed_bytes),
        );
        if let Some(content_encoding) = content_encoding {
            metadata.insert("content_encoding".to_string(), content_encoding.to_string());
        }

        self.objects
            .put(
                &self.session_object_key(scope, identity),
                stored_bytes,
                ObjectMetadata {
                    media_type: TOOL_RESULT_MEDIA_TYPE.to_string(),
                    size_bytes: stored_bytes.len() as u64,
                    sha256: sha256_hex(stored_bytes),
                    filename: format!("{}.txt", object_key_segment(tool_call_id)),
                    metadata,
                },
            )
            .await
    }

    pub async fn get_session_object(
        &self,
        scope: &SessionCasScope,
        key: &str,
    ) -> Result<Option<StoredObject>> {
        ensure_session_key_scope(scope, key)?;
        let Some(object) = self.objects.get(key).await? else {
            return Ok(None);
        };
        ensure_session_metadata_scope(scope, key, &object.metadata)?;
        Ok(Some(object))
    }
}

pub fn session_object_key(scope: &SessionCasScope, identity: &SessionObjectIdentity) -> String {
    format!(
        "cas/{}/sessions/{}/messages/{}/{}.txt",
        encoded_object_key_segment(&scope.ns),
        object_key_segment(&scope.session_id),
        object_key_segment(&identity.message_id),
        object_key_segment(&identity.part_id)
    )
}

pub fn session_object_key_prefix(scope: &SessionCasScope) -> String {
    format!(
        "cas/{}/sessions/{}/",
        encoded_object_key_segment(&scope.ns),
        object_key_segment(&scope.session_id)
    )
}

pub fn ensure_session_key_scope(scope: &SessionCasScope, key: &str) -> Result<()> {
    if !key.starts_with(&session_object_key_prefix(scope)) {
        return Err(anyhow!(
            "CAS object key is outside the requested session scope"
        ));
    }
    Ok(())
}

pub fn logical_object_bytes(object: &StoredObject, key: &str) -> Result<Vec<u8>> {
    if object
        .metadata
        .metadata
        .get("content_encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("gzip"))
    {
        let mut decoder = GzDecoder::new(object.bytes.as_slice());
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|err| anyhow!("CAS object '{key}' has invalid gzip bytes: {err}"))?;
        Ok(out)
    } else {
        Ok(object.bytes.clone())
    }
}

pub fn object_ref_from_stored_object(key: &str, object: &StoredObject) -> data_proto::ObjectRef {
    data_proto::ObjectRef {
        key: key.to_string(),
        media_type: object.metadata.media_type.clone(),
        size_bytes: object.metadata.size_bytes,
        sha256: object.metadata.sha256.clone(),
        filename: object.metadata.filename.clone(),
        metadata: object.metadata.metadata.clone(),
    }
}

fn ensure_session_metadata_scope(
    scope: &SessionCasScope,
    key: &str,
    metadata: &ObjectMetadata,
) -> Result<()> {
    let meta = &metadata.metadata;
    for (field, expected) in [
        ("namespace", scope.ns.as_str()),
        ("agent", scope.agent.as_str()),
        ("session_id", scope.session_id.as_str()),
    ] {
        if let Some(actual) = meta.get(field) {
            if actual != expected {
                return Err(anyhow!(
                    "CAS object key '{key}' metadata field '{field}' does not match requested scope"
                ));
            }
        }
    }
    Ok(())
}

fn session_object_metadata(
    scope: &SessionCasScope,
    identity: &SessionObjectIdentity,
) -> HashMap<String, String> {
    HashMap::from([
        ("namespace".to_string(), scope.ns.clone()),
        ("agent".to_string(), scope.agent.clone()),
        ("session_id".to_string(), scope.session_id.clone()),
        ("message_id".to_string(), identity.message_id.clone()),
        ("part_id".to_string(), identity.part_id.clone()),
    ])
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

#[cfg(test)]
mod tests {
    use super::{CasStore, SessionCasScope, SessionObjectIdentity};
    use crate::control::object_store::InMemoryObjectStore;
    use std::sync::Arc;

    #[test]
    fn session_object_keys_are_stable_and_session_scoped() {
        let store = CasStore::new(Arc::new(InMemoryObjectStore::default()));
        let key = store.session_object_key(
            &SessionCasScope::new("team/alpha", "agent", "session one"),
            &SessionObjectIdentity::new("message#1", "../part id"),
        );
        assert_eq!(
            key,
            "cas/team%2Falpha/sessions/session_one/messages/message_1/.._part_id.txt"
        );
    }

    #[tokio::test]
    async fn rejects_keys_outside_session_scope() {
        let store = CasStore::new(Arc::new(InMemoryObjectStore::default()));
        let err = store
            .get_session_object(
                &SessionCasScope::new("acme", "agent", "session-1"),
                "cas/acme/sessions/session-2/messages/message-1/000001.txt",
            )
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("outside the requested session scope"));
    }

    #[tokio::test]
    async fn rejects_metadata_from_different_agent() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects);
        let writer = SessionCasScope::new("acme", "agent-a", "session-1");
        let identity = SessionObjectIdentity::new("message-1", "000001");
        let object = store
            .put_tool_result(
                &writer, &identity, "call-1", "search", b"hello", b"hello", None,
            )
            .await
            .unwrap();

        let err = store
            .get_session_object(
                &SessionCasScope::new("acme", "agent-b", "session-1"),
                &object.key,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("metadata field 'agent'"));
    }
}
