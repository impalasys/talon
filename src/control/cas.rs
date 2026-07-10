// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::object_store::{ObjectMetadata, ObjectStore, StoredObject};
use crate::gateway::rpc::data_proto;
use anyhow::{anyhow, Result};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

pub const TOOL_RESULT_MEDIA_TYPE: &str = "text/plain; charset=utf-8";

// Object-store metadata is the authorization and hydration contract for CAS
// objects. Keep keys centralized here so writers, readers, cleanup, and tests
// do not drift into almost-the-same string literals.
pub const METADATA_KIND: &str = "kind";
pub const METADATA_KIND_TOOL_RESULT: &str = "tool_result";
pub const METADATA_NAMESPACE: &str = "namespace";
pub const METADATA_AGENT: &str = "agent";
pub const METADATA_SESSION_ID: &str = "session_id";
pub const METADATA_MESSAGE_ID: &str = "message_id";
pub const METADATA_PART_ID: &str = "part_id";
pub const METADATA_TOOL_CALL_ID: &str = "tool_call_id";
pub const METADATA_TOOL_NAME: &str = "tool_name";
pub const METADATA_CONTENT_ENCODING: &str = "content_encoding";
pub const METADATA_UNCOMPRESSED_SIZE_BYTES: &str = "uncompressed_size_bytes";
pub const METADATA_UNCOMPRESSED_SHA256: &str = "uncompressed_sha256";
pub const CONTENT_ENCODING_GZIP: &str = "gzip";

const MIN_GZIP_SAVINGS_PERCENT: usize = 10;
const MAX_LOGICAL_OBJECT_BYTES: u64 = 50 * 1024 * 1024;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionObjectKey {
    pub scope: SessionCasScope,
    pub identity: SessionObjectIdentity,
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

    /// Store a tool result under the canonical session/message/part CAS path.
    ///
    /// CAS owns the storage representation: callers provide logical UTF-8
    /// bytes, and this method decides whether to gzip them before writing the
    /// object and recording the corresponding metadata.
    pub async fn put_tool_result(
        &self,
        ns: &str,
        agent: &str,
        session_id: &str,
        message_id: &str,
        part_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        uncompressed_bytes: &[u8],
    ) -> Result<data_proto::ObjectRef> {
        let scope = SessionCasScope::new(ns, agent, session_id);
        let identity = SessionObjectIdentity::new(message_id, part_id);
        let (stored_bytes, content_encoding) = compressed_object_bytes(uncompressed_bytes)?;
        let metadata = tool_result_metadata(
            &scope,
            &identity,
            tool_call_id,
            tool_name,
            uncompressed_bytes,
            content_encoding,
        );

        self.objects
            .put(
                &self.session_object_key(&scope, &identity),
                &stored_bytes,
                ObjectMetadata {
                    media_type: TOOL_RESULT_MEDIA_TYPE.to_string(),
                    size_bytes: stored_bytes.len() as u64,
                    sha256: sha256_hex(&stored_bytes),
                    filename: format!("{}.txt", object_key_segment(tool_call_id)),
                    metadata,
                },
            )
            .await
    }

    /// Store a tool result only after the logical value crosses a raw-byte
    /// threshold. Tool results use this policy so large raw outputs never land
    /// back in session rows just because they compress well.
    pub async fn put_tool_result_if_raw_at_least(
        &self,
        ns: &str,
        agent: &str,
        session_id: &str,
        message_id: &str,
        part_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        uncompressed_bytes: &[u8],
        threshold_bytes: usize,
    ) -> Result<Option<data_proto::ObjectRef>> {
        if uncompressed_bytes.len() < threshold_bytes {
            return Ok(None);
        }
        self.put_tool_result(
            ns,
            agent,
            session_id,
            message_id,
            part_id,
            tool_call_id,
            tool_name,
            uncompressed_bytes,
        )
        .await
        .map(Some)
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

    /// Load a session object as logical bytes for internal callers.
    ///
    /// This preserves the same scope checks as `get_session_object`, then
    /// decodes any CAS-managed content encoding before returning.
    pub async fn get_session_object_decoded(
        &self,
        scope: &SessionCasScope,
        key: &str,
    ) -> Result<Option<StoredObject>> {
        self.get_session_object(scope, key)
            .await?
            .map(|object| decode_stored_object(object, key))
            .transpose()
    }

    pub async fn get_session_object_by_key(
        &self,
        key: &str,
    ) -> Result<Option<(SessionCasScope, StoredObject)>> {
        let parsed = parse_session_object_key(key)?;
        let Some(object) = self.objects.get(key).await? else {
            return Ok(None);
        };
        let scope = session_scope_from_key_and_metadata(&parsed.scope, key, &object.metadata)?;
        Ok(Some((scope, object)))
    }

    /// Parse, authorize-by-metadata, and load a session object as logical bytes.
    ///
    /// Use this for internal replay/recovery paths that receive only a CAS key.
    /// The public RPC intentionally uses `get_session_object_by_key` instead so
    /// SDK callers can fetch the stored bytes or signed URL directly.
    pub async fn get_session_object_by_key_decoded(
        &self,
        key: &str,
    ) -> Result<Option<(SessionCasScope, StoredObject)>> {
        self.get_session_object_by_key(key)
            .await?
            .map(|(scope, object)| decode_stored_object(object, key).map(|object| (scope, object)))
            .transpose()
    }

    pub async fn signed_get_url(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> Result<Option<crate::control::object_store::SignedObjectUrl>> {
        self.objects.signed_get_url(key, expires_in).await
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

pub fn parse_session_object_key(key: &str) -> Result<SessionObjectKey> {
    let parts: Vec<&str> = key.split('/').collect();
    let ["cas", encoded_ns, "sessions", session_id, "messages", message_id, filename] =
        parts.as_slice()
    else {
        return Err(anyhow!("CAS object key is not a session object key"));
    };
    let ns = urlencoding::decode(encoded_ns)
        .map_err(|err| anyhow!("CAS object key namespace is not valid percent-encoding: {err}"))?
        .into_owned();
    if encoded_object_key_segment(&ns) != *encoded_ns {
        return Err(anyhow!("CAS object key namespace is not canonical"));
    }
    let part_id = filename
        .strip_suffix(".txt")
        .ok_or_else(|| anyhow!("CAS object key must end with .txt"))?;
    for (field, value) in [
        ("session_id", *session_id),
        ("message_id", *message_id),
        ("part_id", part_id),
    ] {
        if object_key_segment(value) != value {
            return Err(anyhow!(
                "CAS object key field '{field}' contains unsafe characters"
            ));
        }
    }
    Ok(SessionObjectKey {
        scope: SessionCasScope::new(&ns, "", session_id),
        identity: SessionObjectIdentity::new(message_id, part_id),
    })
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

/// Return the logical object bytes for a stored CAS object.
///
/// This is the internal counterpart to the public CAS RPC, which intentionally
/// returns stored bytes so SDK callers can use signed URLs directly.
pub fn decode_stored_object_bytes(object: &StoredObject, key: &str) -> Result<Vec<u8>> {
    if object
        .metadata
        .metadata
        .get(METADATA_CONTENT_ENCODING)
        .is_some_and(|value| value.eq_ignore_ascii_case(CONTENT_ENCODING_GZIP))
    {
        gunzip(&object.bytes, key)
    } else {
        Ok(object.bytes.clone())
    }
}

fn decode_stored_object(mut object: StoredObject, key: &str) -> Result<StoredObject> {
    object.bytes = decode_stored_object_bytes(&object, key)?;
    object.metadata.metadata.remove(METADATA_CONTENT_ENCODING);
    object.metadata.size_bytes = object.bytes.len() as u64;
    object.metadata.sha256 = object
        .metadata
        .metadata
        .get(METADATA_UNCOMPRESSED_SHA256)
        .cloned()
        .unwrap_or_else(|| sha256_hex(&object.bytes));
    Ok(object)
}

fn ensure_session_metadata_scope(
    scope: &SessionCasScope,
    key: &str,
    metadata: &ObjectMetadata,
) -> Result<()> {
    let meta = &metadata.metadata;
    for (field, expected) in [
        (METADATA_NAMESPACE, scope.ns.as_str()),
        (METADATA_AGENT, scope.agent.as_str()),
        (METADATA_SESSION_ID, scope.session_id.as_str()),
    ] {
        match meta.get(field) {
            Some(actual) if actual == expected => {}
            _ => {
                return Err(anyhow!(
                    "CAS object key '{key}' metadata field '{field}' does not match requested scope"
                ));
            }
        }
    }
    Ok(())
}

fn session_scope_from_key_and_metadata(
    key_scope: &SessionCasScope,
    key: &str,
    metadata: &ObjectMetadata,
) -> Result<SessionCasScope> {
    let meta = &metadata.metadata;
    for (field, expected) in [
        (METADATA_NAMESPACE, key_scope.ns.as_str()),
        (METADATA_SESSION_ID, key_scope.session_id.as_str()),
    ] {
        if let Some(actual) = meta.get(field) {
            if actual != expected {
                return Err(anyhow!(
                    "CAS object key '{key}' metadata field '{field}' does not match key scope"
                ));
            }
        }
    }
    let agent = meta
        .get(METADATA_AGENT)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("CAS object key '{key}' metadata is missing agent"))?;
    Ok(SessionCasScope::new(
        &key_scope.ns,
        agent,
        &key_scope.session_id,
    ))
}

fn session_object_metadata(
    scope: &SessionCasScope,
    identity: &SessionObjectIdentity,
) -> HashMap<String, String> {
    HashMap::from([
        (METADATA_NAMESPACE.to_string(), scope.ns.clone()),
        (METADATA_AGENT.to_string(), scope.agent.clone()),
        (METADATA_SESSION_ID.to_string(), scope.session_id.clone()),
        (METADATA_MESSAGE_ID.to_string(), identity.message_id.clone()),
        (METADATA_PART_ID.to_string(), identity.part_id.clone()),
    ])
}

fn tool_result_metadata(
    scope: &SessionCasScope,
    identity: &SessionObjectIdentity,
    tool_call_id: &str,
    tool_name: &str,
    uncompressed_bytes: &[u8],
    content_encoding: Option<&str>,
) -> HashMap<String, String> {
    let mut metadata = session_object_metadata(scope, identity);
    metadata.insert(
        METADATA_KIND.to_string(),
        METADATA_KIND_TOOL_RESULT.to_string(),
    );
    metadata.insert(METADATA_TOOL_CALL_ID.to_string(), tool_call_id.to_string());
    metadata.insert(METADATA_TOOL_NAME.to_string(), tool_name.to_string());
    metadata.insert(
        METADATA_UNCOMPRESSED_SIZE_BYTES.to_string(),
        uncompressed_bytes.len().to_string(),
    );
    metadata.insert(
        METADATA_UNCOMPRESSED_SHA256.to_string(),
        sha256_hex(uncompressed_bytes),
    );
    if let Some(content_encoding) = content_encoding {
        metadata.insert(
            METADATA_CONTENT_ENCODING.to_string(),
            content_encoding.to_string(),
        );
    }
    metadata
}

fn compressed_object_bytes(raw_bytes: &[u8]) -> Result<(Vec<u8>, Option<&'static str>)> {
    let gzipped = gzip(raw_bytes)?;
    if (gzipped.len() as u64) * 100
        < (raw_bytes.len() as u64) * (100 - MIN_GZIP_SAVINGS_PERCENT) as u64
    {
        Ok((gzipped, Some(CONTENT_ENCODING_GZIP)))
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
    let mut decoder = GzDecoder::new(bytes).take(MAX_LOGICAL_OBJECT_BYTES + 1);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|err| anyhow!("CAS object '{key}' has invalid gzip bytes: {err}"))?;
    if out.len() > MAX_LOGICAL_OBJECT_BYTES as usize {
        return Err(anyhow!(
            "CAS object '{key}' expands beyond the maximum supported size"
        ));
    }
    Ok(out)
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
    use super::{
        parse_session_object_key, CasStore, SessionCasScope, SessionObjectIdentity,
        CONTENT_ENCODING_GZIP, METADATA_AGENT, METADATA_CONTENT_ENCODING, METADATA_NAMESPACE,
        METADATA_SESSION_ID,
    };
    use crate::control::object_store::{InMemoryObjectStore, ObjectMetadata, ObjectStore};
    use rand::{Rng, SeedableRng};
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
        let object = store
            .put_tool_result(
                "acme",
                "agent-a",
                "session-1",
                "message-1",
                "000001",
                "call-1",
                "search",
                b"hello",
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

    #[test]
    fn parses_session_scope_from_cas_key() {
        let parsed = parse_session_object_key(
            "cas/team%2Falpha/sessions/session-1/messages/message-1/part.txt",
        )
        .unwrap();
        assert_eq!(parsed.scope.ns, "team/alpha");
        assert_eq!(parsed.scope.session_id, "session-1");
        assert_eq!(parsed.identity.message_id, "message-1");
        assert_eq!(parsed.identity.part_id, "part");
    }

    #[test]
    fn rejects_non_canonical_cas_key_namespaces() {
        let err = parse_session_object_key(
            "cas/team%2falpha/sessions/session-1/messages/message-1/part.txt",
        )
        .unwrap_err();
        assert!(err.to_string().contains("namespace is not canonical"));
    }

    #[tokio::test]
    async fn derives_scope_from_key_and_stored_metadata() {
        let writer = SessionCasScope::new("acme", "agent-a", "session-1");
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects.clone());
        let object = store
            .put_tool_result(
                "acme",
                "agent-a",
                "session-1",
                "message-1",
                "000001",
                "call-1",
                "search",
                b"hello",
            )
            .await
            .unwrap();

        let (scope, stored) = store
            .get_session_object_by_key(&object.key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(scope, writer);
        assert_eq!(stored.bytes, b"hello");
    }

    #[tokio::test]
    async fn rejects_stored_metadata_that_disagrees_with_key_scope() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects.clone());
        objects
            .put(
                "cas/acme/sessions/session-1/messages/message-1/000001.txt",
                b"hello",
                ObjectMetadata {
                    metadata: std::collections::HashMap::from([
                        ("namespace".to_string(), "acme".to_string()),
                        ("agent".to_string(), "agent".to_string()),
                        ("session_id".to_string(), "session-2".to_string()),
                    ]),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();

        let err = store
            .get_session_object_by_key("cas/acme/sessions/session-1/messages/message-1/000001.txt")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("does not match key scope"));
    }

    #[tokio::test]
    async fn compresses_tool_result_when_gzip_saves_meaningfully() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects.clone());
        let raw = "x".repeat(3 * 1024);

        let object = store
            .put_tool_result(
                "acme",
                "agent",
                "session-1",
                "message-1",
                "000001",
                "call-1",
                "search",
                raw.as_bytes(),
            )
            .await
            .unwrap();

        assert!(object.size_bytes < raw.len() as u64);
        assert_eq!(
            object.metadata[METADATA_CONTENT_ENCODING],
            CONTENT_ENCODING_GZIP
        );
        let stored = store
            .get_session_object_decoded(
                &SessionCasScope::new("acme", "agent", "session-1"),
                &object.key,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.bytes, raw.as_bytes());
        assert!(!stored
            .metadata
            .metadata
            .contains_key(METADATA_CONTENT_ENCODING));
    }

    #[tokio::test]
    async fn keeps_incompressible_tool_result_raw() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects.clone());
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let raw = (0..2 * 1024)
            .map(|_| rng.gen_range(0u8..=0xff))
            .collect::<Vec<_>>();

        let object = store
            .put_tool_result(
                "acme",
                "agent",
                "session-1",
                "message-1",
                "000001",
                "call-1",
                "search",
                &raw,
            )
            .await
            .unwrap();
        let stored = objects.get(&object.key).await.unwrap().unwrap();

        assert_eq!(stored.bytes, raw);
        assert!(!object.metadata.contains_key(METADATA_CONTENT_ENCODING));
    }

    #[tokio::test]
    async fn corrupt_gzip_object_returns_decode_error() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects.clone());
        let object = objects
            .put(
                "cas/acme/sessions/session-1/messages/message-1/000001.txt",
                b"not gzip",
                ObjectMetadata {
                    metadata: std::collections::HashMap::from([
                        (
                            METADATA_CONTENT_ENCODING.to_string(),
                            CONTENT_ENCODING_GZIP.to_string(),
                        ),
                        (METADATA_NAMESPACE.to_string(), "acme".to_string()),
                        (METADATA_AGENT.to_string(), "agent".to_string()),
                        (METADATA_SESSION_ID.to_string(), "session-1".to_string()),
                    ]),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();

        let err = store
            .get_session_object_decoded(
                &SessionCasScope::new("acme", "agent", "session-1"),
                &object.key,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid gzip bytes"));
    }
}
