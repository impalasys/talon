// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::object_store::{
    object_version, ObjectMetadata, ObjectStore, ObjectVersionChanged, StoredObject,
};
use crate::gateway::rpc::data_proto;
use anyhow::{anyhow, Result};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::io::{Cursor, Read, Write};
use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const TOOL_RESULT_MEDIA_TYPE: &str = "text/plain; charset=utf-8";

// Object-store metadata is the authorization and hydration contract for CAS
// objects. Keep keys centralized here so writers, readers, cleanup, and tests
// do not drift into almost-the-same string literals.
pub const METADATA_KIND: &str = "kind";
pub const METADATA_KIND_TOOL_RESULT: &str = "tool_result";
pub const METADATA_KIND_ARTIFACT: &str = "artifact";
pub const METADATA_KIND_FILE: &str = "file";
pub const METADATA_KIND_COMPACTION: &str = "compaction";
pub const METADATA_KIND_ENCRYPTED_REASONING: &str = "encrypted_reasoning";
pub const METADATA_AGENT: &str = "agent";
pub const METADATA_TOOL_CALL_ID: &str = "tool_call_id";
pub const METADATA_TOOL_NAME: &str = "tool_name";
pub const METADATA_CONTENT_ENCODING: &str = "content_encoding";
pub const METADATA_UNCOMPRESSED_SIZE_BYTES: &str = "uncompressed_size_bytes";
pub const METADATA_UNCOMPRESSED_SHA256: &str = "uncompressed_sha256";
pub const CONTENT_ENCODING_GZIP: &str = "gzip";
pub const CONTENT_ENCODING_ZSTD: &str = "zstd";

/// Logical bytes per independently-decodable frame in tool-result streams.
pub const TOOL_RESULT_SEEKABLE_FRAME_BYTES: usize = 256 * 1024;
const MAX_LOGICAL_OBJECT_BYTES: u64 = 50 * 1024 * 1024;
const MAX_TOOL_RESULT_LOGICAL_BYTES: usize = 8 * 1024 * 1024;
const TOOL_RESULT_TRUNCATION_MARKER: &[u8] =
    b"\n\n...[CONTENT TRUNCATED DUE TO TOOL RESULT SIZE LIMIT]";

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
    seek_tables: Arc<Mutex<SeekTableCache>>,
}

impl CasStore {
    pub fn new(objects: Arc<dyn ObjectStore + Send + Sync>) -> Self {
        Self {
            objects,
            seek_tables: Arc::new(Mutex::new(SeekTableCache::default())),
        }
    }

    pub fn object_store(&self) -> &(dyn ObjectStore + Send + Sync) {
        self.objects.as_ref()
    }

    async fn put_object(
        &self,
        key: &str,
        bytes: &[u8],
        metadata: ObjectMetadata,
    ) -> Result<data_proto::ObjectRef> {
        let object = self.objects.put(key, bytes, metadata).await?;
        // Cache entries are keyed by canonical object key rather than version. Drop
        // them after a successful mutation so a replacement cannot reuse old frame
        // boundaries, even when its stored or logical size is unchanged.
        self.seek_tables.lock().unwrap().remove(key);
        Ok(object)
    }

    pub fn session_object_key(
        &self,
        scope: &SessionCasScope,
        identity: &SessionObjectIdentity,
    ) -> String {
        session_object_key(scope, identity)
    }

    pub async fn put_file(
        &self,
        namespace: &str,
        file_uid: &str,
        path: &str,
        bytes: &[u8],
        media_type: &str,
    ) -> Result<data_proto::ObjectRef> {
        let sha = sha256_hex(bytes);
        let key = file_object_key(namespace, file_uid, &sha);
        let metadata = HashMap::from([
            (METADATA_KIND.to_string(), METADATA_KIND_FILE.to_string()),
            ("namespace".to_string(), namespace.to_string()),
            ("file_uid".to_string(), file_uid.to_string()),
            ("path".to_string(), path.to_string()),
        ]);
        self.put_object(
            &key,
            bytes,
            ObjectMetadata {
                media_type: media_type.to_string(),
                size_bytes: bytes.len() as u64,
                sha256: sha,
                filename: filename_for_path(path),
                content_encoding: String::new(),
                metadata,
            },
        )
        .await
    }

    pub fn signed_file_object_metadata(
        namespace: &str,
        file_uid: &str,
        path: &str,
        media_type: &str,
        sha: &str,
        size_bytes: u64,
    ) -> ObjectMetadata {
        let metadata = HashMap::from([
            (METADATA_KIND.to_string(), METADATA_KIND_FILE.to_string()),
            ("namespace".to_string(), namespace.to_string()),
            ("file_uid".to_string(), file_uid.to_string()),
            ("path".to_string(), path.to_string()),
        ]);
        ObjectMetadata {
            media_type: media_type.to_string(),
            size_bytes,
            sha256: sha.to_string(),
            filename: filename_for_path(path),
            content_encoding: String::new(),
            metadata,
        }
    }

    pub async fn signed_put_file_url(
        &self,
        namespace: &str,
        file_uid: &str,
        path: &str,
        media_type: &str,
        object_key_suffix: &str,
        sha: &str,
        size_bytes: u64,
        expires_in: Duration,
    ) -> Result<Option<crate::control::object_store::SignedObjectUrl>> {
        let key = file_object_key(namespace, file_uid, object_key_suffix);
        let metadata = Self::signed_file_object_metadata(
            namespace, file_uid, path, media_type, sha, size_bytes,
        );
        self.objects
            .signed_put_url(&key, metadata, expires_in)
            .await
    }

    pub async fn put_latest_file(
        &self,
        namespace: &str,
        path: &str,
        bytes: &[u8],
        media_type: &str,
    ) -> Result<data_proto::ObjectRef> {
        let key = latest_file_object_key(namespace, path);
        let metadata = HashMap::from([
            (METADATA_KIND.to_string(), METADATA_KIND_FILE.to_string()),
            ("namespace".to_string(), namespace.to_string()),
            ("path".to_string(), path.to_string()),
            ("latest".to_string(), "true".to_string()),
        ]);
        self.put_object(
            &key,
            bytes,
            ObjectMetadata {
                media_type: media_type.to_string(),
                size_bytes: bytes.len() as u64,
                sha256: sha256_hex(bytes),
                filename: filename_for_path(path),
                content_encoding: String::new(),
                metadata,
            },
        )
        .await
    }

    pub async fn put_artifact(
        &self,
        namespace: &str,
        agent: &str,
        session_id: &str,
        artifact_uid: &str,
        bytes: &[u8],
        media_type: &str,
        mut metadata: HashMap<String, String>,
    ) -> Result<data_proto::ObjectRef> {
        let sha = sha256_hex(bytes);
        let key = artifact_object_key(namespace, artifact_uid, &sha);
        metadata.insert(
            METADATA_KIND.to_string(),
            METADATA_KIND_ARTIFACT.to_string(),
        );
        metadata.insert("namespace".to_string(), namespace.to_string());
        metadata.insert("artifact_id".to_string(), artifact_uid.to_string());
        metadata.insert(METADATA_AGENT.to_string(), agent.to_string());
        metadata.insert("session_id".to_string(), session_id.to_string());
        self.put_object(
            &key,
            bytes,
            ObjectMetadata {
                media_type: media_type.to_string(),
                size_bytes: bytes.len() as u64,
                sha256: sha,
                filename: String::new(),
                content_encoding: String::new(),
                metadata,
            },
        )
        .await
    }

    /// Load caller-defined content as logical bytes.
    ///
    /// This decodes any CAS-managed content encoding for internal callers.
    pub async fn get_object_decoded(&self, key: &str) -> Result<Option<StoredObject>> {
        self.objects
            .get(key)
            .await?
            .map(|object| decode_stored_object(object, key))
            .transpose()
    }

    pub async fn delete_object(&self, key: &str) -> Result<()> {
        self.objects.delete(key).await?;
        self.seek_tables.lock().unwrap().remove(key);
        Ok(())
    }

    /// Store a tool result under the canonical session/message/part CAS path.
    ///
    /// CAS owns the storage representation: callers provide logical UTF-8
    /// bytes, and this method decides whether to compress them before writing
    /// the object and recording the corresponding metadata.
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
        if uncompressed_bytes.len() > MAX_LOGICAL_OBJECT_BYTES as usize {
            return Err(anyhow!(
                "tool result exceeds the maximum supported logical size"
            ));
        }
        let scope = SessionCasScope::new(ns, agent, session_id);
        let identity = SessionObjectIdentity::new(message_id, part_id);
        let logical_bytes = tool_result_logical_bytes(uncompressed_bytes);
        // Seekable encoding is intentional even if it enlarges a small object: every
        // newly-written tool result must support logical-byte range reads.
        let stored_bytes = seekable_zstd(&logical_bytes)?;
        let metadata = tool_result_metadata(&scope, tool_call_id, tool_name, &logical_bytes);

        let key = self.session_object_key(&scope, &identity);
        self.put_object(
            &key,
            &stored_bytes,
            ObjectMetadata {
                media_type: TOOL_RESULT_MEDIA_TYPE.to_string(),
                size_bytes: stored_bytes.len() as u64,
                sha256: sha256_hex(&stored_bytes),
                filename: format!("{}.txt", object_key_segment(tool_call_id)),
                content_encoding: CONTENT_ENCODING_ZSTD.to_string(),
                metadata,
            },
        )
        .await
    }

    /// Decode a logical byte range from a text tool-result object.
    ///
    /// `range` is in UTF-8 *bytes*, not stored/compressed bytes. Callers that turn
    /// this into model text must supply character boundaries; CAS returns the exact
    /// requested bytes and intentionally does not validate UTF-8 boundaries.
    pub async fn get_text_range_decoded(&self, key: &str, range: Range<u64>) -> Result<Vec<u8>> {
        // A seekable read needs a head, seek-table range, and frame range. Each
        // range is conditional on the generation returned by head; retry when a
        // concurrent overwrite wins between those requests.
        for _ in 0..3 {
            match self.get_text_range_decoded_once(key, range.clone()).await {
                Err(error) if error.downcast_ref::<ObjectVersionChanged>().is_some() => continue,
                result => return result,
            }
        }
        Err(anyhow!(
            "CAS object '{key}' changed repeatedly during a seekable range read"
        ))
    }

    async fn get_text_range_decoded_once(&self, key: &str, range: Range<u64>) -> Result<Vec<u8>> {
        let metadata = self
            .objects
            .head(key)
            .await?
            .ok_or_else(|| anyhow!("CAS object '{key}' does not exist"))?;
        let declared_logical_size = logical_size_from_metadata(key, &metadata)?;
        if let Some(logical_size) = declared_logical_size {
            validate_logical_range(&range, logical_size)?;
            if range.start == range.end {
                return Ok(Vec::new());
            }
        }

        if let (true, Some(logical_size), Some(version)) = (
            is_zstd_metadata(&metadata),
            declared_logical_size,
            object_version(&metadata),
        ) {
            if let Some(table) = self.seek_table(key, metadata.size_bytes, version).await? {
                if table.logical_size != logical_size {
                    return Err(CasRangeError::LogicalSizeMismatch {
                        metadata_size: logical_size,
                        seek_table_size: table.logical_size,
                    }
                    .into());
                }
                return self
                    .decode_seekable_range(key, &table, range, version)
                    .await;
            }
        }

        // Old objects may be gzip, a non-seekable zstd stream, or raw. They retain
        // the historical full-decode path; seekable objects never arrive here.
        let object = self
            .objects
            .get(key)
            .await?
            .ok_or_else(|| anyhow!("CAS object '{key}' disappeared during range read"))?;
        let decoded = decode_text_object_bytes(&object, key)?;
        let logical_size = declared_logical_size.unwrap_or(decoded.len() as u64);
        validate_logical_range(&range, logical_size)?;
        if decoded.len() as u64 != logical_size {
            return Err(CasRangeError::LogicalSizeMismatch {
                metadata_size: logical_size,
                seek_table_size: decoded.len() as u64,
            }
            .into());
        }
        Ok(decoded[range.start as usize..range.end as usize].to_vec())
    }

    async fn seek_table(
        &self,
        key: &str,
        stored_size: u64,
        version: &str,
    ) -> Result<Option<SeekTable>> {
        if let Some(table) = self.seek_tables.lock().unwrap().get(key, version) {
            return Ok(Some(table));
        }
        if stored_size < SEEK_FOOTER_BYTES as u64 {
            return Ok(None);
        }
        let footer = self
            .objects
            .get_range_if_version(
                key,
                stored_size - SEEK_FOOTER_BYTES as u64..stored_size,
                version,
            )
            .await?
            .ok_or_else(|| anyhow!("CAS object '{key}' disappeared during seek-table read"))?;
        let Some((frame_count, descriptor)) = parse_seek_footer(&footer) else {
            return Ok(None);
        };
        let entry_size = if descriptor & SEEK_CHECKSUM_FLAG != 0 {
            12u64
        } else {
            8u64
        };
        let payload_size = frame_count
            .checked_mul(entry_size)
            .and_then(|n| n.checked_add(SEEK_FOOTER_BYTES as u64))
            .ok_or_else(|| anyhow!("CAS object '{key}' has an oversized seek table"))?;
        let table_size = payload_size
            .checked_add(SEEKABLE_SKIPPABLE_HEADER_BYTES as u64)
            .ok_or_else(|| anyhow!("CAS object '{key}' has an oversized seek table"))?;
        if table_size > stored_size {
            return Ok(None);
        }
        let table_bytes = self
            .objects
            .get_range_if_version(key, stored_size - table_size..stored_size, version)
            .await?
            .ok_or_else(|| anyhow!("CAS object '{key}' disappeared during seek-table read"))?;
        let table = parse_seek_table(&table_bytes, frame_count, descriptor)?;
        if table.frames.last().map_or(0, |frame| frame.stored_end) != stored_size - table_size {
            return Err(anyhow!(
                "CAS object '{key}' seek table does not cover its stored frames"
            ));
        }
        self.seek_tables.lock().unwrap().insert(
            key.to_string(),
            version.to_string(),
            table.clone(),
        );
        Ok(Some(table))
    }

    async fn decode_seekable_range(
        &self,
        key: &str,
        table: &SeekTable,
        range: Range<u64>,
        version: &str,
    ) -> Result<Vec<u8>> {
        let first = table
            .frames
            .iter()
            .position(|frame| frame.logical_end > range.start)
            .ok_or_else(|| anyhow!("seek table has no frame for requested logical range"))?;
        let last = table
            .frames
            .iter()
            .rposition(|frame| frame.logical_start < range.end)
            .ok_or_else(|| anyhow!("seek table has no frame for requested logical range"))?;
        let stored = self
            .objects
            .get_range_if_version(
                key,
                table.frames[first].stored_start..table.frames[last].stored_end,
                version,
            )
            .await?
            .ok_or_else(|| anyhow!("CAS object '{key}' disappeared during frame read"))?;
        let mut decoded = Vec::new();
        for frame in &table.frames[first..=last] {
            let offset = (frame.stored_start - table.frames[first].stored_start) as usize;
            let end = offset + (frame.stored_end - frame.stored_start) as usize;
            let bytes = unzstd(&stored[offset..end], key)?;
            if bytes.len() as u64 != frame.logical_end - frame.logical_start {
                return Err(anyhow!(
                    "CAS object '{key}' seek frame decoded to an unexpected size"
                ));
            }
            decoded.extend_from_slice(&bytes);
        }
        let decoded_start = (range.start - table.frames[first].logical_start) as usize;
        let decoded_end = decoded_start + (range.end - range.start) as usize;
        Ok(decoded[decoded_start..decoded_end].to_vec())
    }

    /// Store opaque provider continuation state under the session CAS scope.
    /// The value is intentionally never decoded or logged by Talon.
    pub async fn put_encrypted_reasoning(
        &self,
        ns: &str,
        agent: &str,
        session_id: &str,
        provider: &str,
        model: &str,
        value: &str,
    ) -> Result<data_proto::ObjectRef> {
        let scope = SessionCasScope::new(ns, agent, session_id);
        let identity =
            SessionObjectIdentity::new("encrypted-reasoning", &uuid::Uuid::now_v7().to_string());
        let bytes = value.as_bytes();
        self.put_object(
            &session_object_key_with_extension(&scope, &identity, "bin"),
            bytes,
            ObjectMetadata {
                media_type: "application/octet-stream".to_string(),
                size_bytes: bytes.len() as u64,
                sha256: sha256_hex(bytes),
                filename: String::new(),
                content_encoding: String::new(),
                metadata: HashMap::from([
                    (
                        METADATA_KIND.to_string(),
                        METADATA_KIND_ENCRYPTED_REASONING.to_string(),
                    ),
                    ("namespace".to_string(), ns.to_string()),
                    (METADATA_AGENT.to_string(), agent.to_string()),
                    ("session_id".to_string(), session_id.to_string()),
                    ("provider".to_string(), provider.to_string()),
                    ("model".to_string(), model.to_string()),
                ]),
            },
        )
        .await
    }

    /// Store the immutable Markdown summary for a durable context compaction.
    /// The key intentionally uses the journal entry id so the journal and
    /// internal message marker can share one authoritative object reference.
    pub async fn put_compaction_summary(
        &self,
        ns: &str,
        agent: &str,
        session_id: &str,
        submission_id: &str,
        journal_entry_id: &str,
        summary: &str,
    ) -> Result<data_proto::ObjectRef> {
        let key = compaction_summary_object_key(ns, session_id, submission_id, journal_entry_id);
        let bytes = summary.as_bytes();
        let metadata = HashMap::from([
            (
                METADATA_KIND.to_string(),
                METADATA_KIND_COMPACTION.to_string(),
            ),
            ("namespace".to_string(), ns.to_string()),
            (METADATA_AGENT.to_string(), agent.to_string()),
            ("session_id".to_string(), session_id.to_string()),
            ("submission_id".to_string(), submission_id.to_string()),
            ("journal_entry_id".to_string(), journal_entry_id.to_string()),
        ]);
        self.put_object(
            &key,
            bytes,
            ObjectMetadata {
                media_type: "text/markdown; charset=utf-8".to_string(),
                size_bytes: bytes.len() as u64,
                sha256: sha256_hex(bytes),
                filename: format!("{}.txt", object_key_segment(journal_entry_id)),
                content_encoding: String::new(),
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

    pub async fn head_session_object_by_key(
        &self,
        key: &str,
    ) -> Result<Option<(SessionCasScope, ObjectMetadata)>> {
        let parsed = parse_session_object_key(key)?;
        let Some(metadata) = self.objects.head(key).await? else {
            return Ok(None);
        };
        let scope = session_scope_from_key_and_metadata(&parsed.scope, key, &metadata)?;
        Ok(Some((scope, metadata)))
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
    session_object_key_with_extension(scope, identity, "txt")
}

fn session_object_key_with_extension(
    scope: &SessionCasScope,
    identity: &SessionObjectIdentity,
    extension: &str,
) -> String {
    format!(
        "cas/{}/sessions/{}/messages/{}/{}.{}",
        encoded_object_key_segment(&scope.ns),
        object_key_segment(&scope.session_id),
        object_key_segment(&identity.message_id),
        object_key_segment(&identity.part_id),
        extension,
    )
}

pub fn session_object_key_prefix(scope: &SessionCasScope) -> String {
    format!(
        "cas/{}/sessions/{}/",
        encoded_object_key_segment(&scope.ns),
        object_key_segment(&scope.session_id)
    )
}

pub fn compaction_summary_object_key(
    namespace: &str,
    session_id: &str,
    submission_id: &str,
    journal_entry_id: &str,
) -> String {
    format!(
        "cas/{}/sessions/{}/compactions/{}/{}.txt",
        encoded_object_key_segment(namespace),
        object_key_segment(session_id),
        object_key_segment(submission_id),
        object_key_segment(journal_entry_id),
    )
}

pub fn file_object_key(namespace: &str, file_uid: &str, sha: &str) -> String {
    format!(
        "cas/{}/files/{}/{}",
        encoded_object_key_segment(namespace),
        object_key_segment(file_uid),
        object_key_segment(sha)
    )
}

pub fn artifact_object_key(namespace: &str, artifact_uid: &str, sha: &str) -> String {
    format!(
        "cas/{}/artifacts/{}/{}",
        encoded_object_key_segment(namespace),
        object_key_segment(artifact_uid),
        object_key_segment(sha)
    )
}

pub fn latest_file_object_key(namespace: &str, path: &str) -> String {
    format!(
        "latest/{}/files/{}",
        encoded_object_key_segment(namespace),
        path.trim_start_matches('/')
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
    if let ["cas", encoded_ns, "sessions", session_id, "compactions", submission_id, filename] =
        parts.as_slice()
    {
        let ns = urlencoding::decode(encoded_ns)
            .map_err(|err| {
                anyhow!("CAS object key namespace is not valid percent-encoding: {err}")
            })?
            .into_owned();
        let journal_entry_id = filename
            .strip_suffix(".txt")
            .ok_or_else(|| anyhow!("CAS compaction key must end with .txt"))?;
        if encoded_object_key_segment(&ns) != *encoded_ns
            || object_key_segment(session_id) != *session_id
            || object_key_segment(submission_id) != *submission_id
            || object_key_segment(journal_entry_id) != journal_entry_id
        {
            return Err(anyhow!("CAS compaction key contains unsafe characters"));
        }
        return Ok(SessionObjectKey {
            scope: SessionCasScope::new(&ns, "", session_id),
            identity: SessionObjectIdentity::new(submission_id, journal_entry_id),
        });
    }
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
    object_ref_from_metadata(key, &object.metadata)
}

pub fn object_ref_from_metadata(key: &str, metadata: &ObjectMetadata) -> data_proto::ObjectRef {
    data_proto::ObjectRef {
        key: key.to_string(),
        media_type: metadata.media_type.clone(),
        size_bytes: metadata.size_bytes,
        sha256: metadata.sha256.clone(),
        filename: metadata.filename.clone(),
        content_encoding: metadata.content_encoding.clone(),
        metadata: metadata.metadata.clone(),
    }
}

/// Return the logical object bytes for a stored CAS object.
///
/// This is the internal counterpart to the public CAS RPC, which intentionally
/// returns stored bytes so SDK callers can use signed URLs directly.
pub fn decode_stored_object_bytes(object: &StoredObject, key: &str) -> Result<Vec<u8>> {
    let encoding = if object.metadata.content_encoding.trim().is_empty() {
        object
            .metadata
            .metadata
            .get(METADATA_CONTENT_ENCODING)
            .map(|value| value.to_ascii_lowercase())
    } else {
        Some(object.metadata.content_encoding.to_ascii_lowercase())
    };
    match encoding.as_deref() {
        Some(CONTENT_ENCODING_ZSTD) => unzstd(&object.bytes, key),
        Some(CONTENT_ENCODING_GZIP) => gunzip(&object.bytes, key),
        Some(other) => Err(anyhow!(
            "CAS object '{key}' uses unsupported content encoding '{other}'"
        )),
        None => raw_object_bytes(object, key),
    }
}

fn raw_object_bytes(object: &StoredObject, key: &str) -> Result<Vec<u8>> {
    if object.bytes.len() > MAX_LOGICAL_OBJECT_BYTES as usize {
        return Err(anyhow!(
            "CAS object '{key}' exceeds the maximum supported size"
        ));
    }
    Ok(object.bytes.clone())
}

fn decode_stored_object(mut object: StoredObject, key: &str) -> Result<StoredObject> {
    object.bytes = decode_stored_object_bytes(&object, key)?;
    object.metadata.content_encoding.clear();
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
    match meta.get(METADATA_AGENT) {
        Some(actual) if actual == &scope.agent => {}
        _ => {
            return Err(anyhow!(
                "CAS object key '{key}' metadata field '{METADATA_AGENT}' does not match requested scope"
            ));
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

fn session_object_metadata(scope: &SessionCasScope) -> HashMap<String, String> {
    HashMap::from([(METADATA_AGENT.to_string(), scope.agent.clone())])
}

fn tool_result_metadata(
    scope: &SessionCasScope,
    tool_call_id: &str,
    tool_name: &str,
    uncompressed_bytes: &[u8],
) -> HashMap<String, String> {
    let mut metadata = session_object_metadata(scope);
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
    metadata
}

// Zstandard seekable format: https://github.com/facebook/zstd/blob/dev/contrib/seekable_format/zstd_seekable_compression_format.md
// Each logical frame is a standalone zstd frame, followed by a skippable seek
// table frame. We write no checksums (descriptor bit 7 is clear).
const SEEKABLE_SKIPPABLE_MAGIC: u32 = 0x184D_2A5E;
const SEEKABLE_FOOTER_MAGIC: u32 = 0x8F92_EAB1;
const SEEKABLE_SKIPPABLE_HEADER_BYTES: usize = 8;
const SEEK_FOOTER_BYTES: usize = 9;
const SEEK_CHECKSUM_FLAG: u8 = 0x80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CasRangeError {
    InvalidLogicalRange {
        start: u64,
        end: u64,
        size: u64,
    },
    InvalidLogicalSizeMetadata(String),
    LogicalSizeMismatch {
        metadata_size: u64,
        seek_table_size: u64,
    },
}

impl std::fmt::Display for CasRangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLogicalRange { start, end, size } => write!(f, "invalid logical text range [{start}..{end}) for object of {size} bytes"),
            Self::InvalidLogicalSizeMetadata(value) => write!(f, "tool-result object has invalid logical size metadata '{value}'"),
            Self::LogicalSizeMismatch { metadata_size, seek_table_size } => write!(f, "tool-result logical size metadata ({metadata_size}) does not match decoded seek-table size ({seek_table_size})"),
        }
    }
}

impl std::error::Error for CasRangeError {}

#[derive(Debug, Clone)]
struct SeekFrame {
    stored_start: u64,
    stored_end: u64,
    logical_start: u64,
    logical_end: u64,
}

#[derive(Debug, Clone)]
struct SeekTable {
    frames: Vec<SeekFrame>,
    logical_size: u64,
}

#[derive(Default)]
struct SeekTableCache {
    entries: VecDeque<((String, String), SeekTable)>,
}

impl SeekTableCache {
    fn get(&mut self, key: &str, version: &str) -> Option<SeekTable> {
        let index = self
            .entries
            .iter()
            .position(|((cached_key, cached_version), _)| {
                cached_key == key && cached_version == version
            })?;
        let entry = self.entries.remove(index).unwrap();
        let table = entry.1.clone();
        self.entries.push_front(entry);
        Some(table)
    }

    fn insert(&mut self, key: String, version: String, table: SeekTable) {
        self.entries.retain(|((cached_key, cached_version), _)| {
            cached_key != &key || cached_version != &version
        });
        self.entries.push_front(((key, version), table));
        self.entries.truncate(64);
    }

    fn remove(&mut self, key: &str) {
        self.entries
            .retain(|((cached_key, _), _)| cached_key != key);
    }
}

fn seekable_zstd(raw_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut entries = Vec::new();
    for frame in raw_bytes.chunks(TOOL_RESULT_SEEKABLE_FRAME_BYTES) {
        let encoded = zstd(frame)?;
        let compressed_size = u32::try_from(encoded.len())
            .map_err(|_| anyhow!("compressed seek frame is too large"))?;
        let logical_size =
            u32::try_from(frame.len()).map_err(|_| anyhow!("logical seek frame is too large"))?;
        output.extend_from_slice(&encoded);
        entries.push((compressed_size, logical_size));
    }
    let payload_len = entries
        .len()
        .checked_mul(8)
        .and_then(|len| len.checked_add(SEEK_FOOTER_BYTES))
        .ok_or_else(|| anyhow!("seek table is too large"))?;
    output.extend_from_slice(&SEEKABLE_SKIPPABLE_MAGIC.to_le_bytes());
    output.extend_from_slice(&(payload_len as u32).to_le_bytes());
    for (compressed_size, logical_size) in entries {
        output.extend_from_slice(&compressed_size.to_le_bytes());
        output.extend_from_slice(&logical_size.to_le_bytes());
    }
    output.extend_from_slice(
        &((raw_bytes.len().div_ceil(TOOL_RESULT_SEEKABLE_FRAME_BYTES)) as u32).to_le_bytes(),
    );
    output.push(0);
    output.extend_from_slice(&SEEKABLE_FOOTER_MAGIC.to_le_bytes());
    Ok(output)
}

fn parse_seek_footer(bytes: &[u8]) -> Option<(u64, u8)> {
    if bytes.len() != SEEK_FOOTER_BYTES
        || u32::from_le_bytes(bytes[5..9].try_into().ok()?) != SEEKABLE_FOOTER_MAGIC
    {
        return None;
    }
    Some((
        u32::from_le_bytes(bytes[0..4].try_into().ok()?) as u64,
        bytes[4],
    ))
}

fn parse_seek_table(bytes: &[u8], frame_count: u64, descriptor: u8) -> Result<SeekTable> {
    if descriptor & !SEEK_CHECKSUM_FLAG != 0 {
        return Err(anyhow!("invalid zstd seek table descriptor"));
    }
    let entry_size = if descriptor & SEEK_CHECKSUM_FLAG != 0 {
        12usize
    } else {
        8usize
    };
    let expected_payload = frame_count
        .checked_mul(entry_size as u64)
        .and_then(|n| n.checked_add(SEEK_FOOTER_BYTES as u64))
        .ok_or_else(|| anyhow!("seek table length overflows"))? as usize;
    if bytes.len() != SEEKABLE_SKIPPABLE_HEADER_BYTES + expected_payload
        || u32::from_le_bytes(bytes[0..4].try_into().unwrap()) != SEEKABLE_SKIPPABLE_MAGIC
        || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize != expected_payload
        || parse_seek_footer(&bytes[bytes.len() - SEEK_FOOTER_BYTES..])
            != Some((frame_count, descriptor))
    {
        return Err(anyhow!("invalid zstd seek table"));
    }
    let mut stored_offset = 0u64;
    let mut logical_offset = 0u64;
    let mut frames = Vec::with_capacity(frame_count as usize);
    for index in 0..frame_count as usize {
        let offset = SEEKABLE_SKIPPABLE_HEADER_BYTES + index * entry_size;
        let stored_size = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as u64;
        let logical_size =
            u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as u64;
        let frame = SeekFrame {
            stored_start: stored_offset,
            stored_end: stored_offset
                .checked_add(stored_size)
                .ok_or_else(|| anyhow!("seek table stored offsets overflow"))?,
            logical_start: logical_offset,
            logical_end: logical_offset
                .checked_add(logical_size)
                .ok_or_else(|| anyhow!("seek table logical offsets overflow"))?,
        };
        stored_offset = frame.stored_end;
        logical_offset = frame.logical_end;
        frames.push(frame);
    }
    Ok(SeekTable {
        frames,
        logical_size: logical_offset,
    })
}

fn logical_size_from_metadata(key: &str, metadata: &ObjectMetadata) -> Result<Option<u64>> {
    let Some(value) = metadata.metadata.get(METADATA_UNCOMPRESSED_SIZE_BYTES) else {
        return Ok(None);
    };
    value
        .parse()
        .map(Some)
        .map_err(|_| CasRangeError::InvalidLogicalSizeMetadata(format!("{key}: {value}")).into())
}

fn validate_logical_range(range: &Range<u64>, size: u64) -> Result<()> {
    if range.start > range.end || range.end > size {
        return Err(CasRangeError::InvalidLogicalRange {
            start: range.start,
            end: range.end,
            size,
        }
        .into());
    }
    Ok(())
}

fn is_zstd_metadata(metadata: &ObjectMetadata) -> bool {
    metadata
        .content_encoding
        .eq_ignore_ascii_case(CONTENT_ENCODING_ZSTD)
        || metadata
            .metadata
            .get(METADATA_CONTENT_ENCODING)
            .is_some_and(|value| value.eq_ignore_ascii_case(CONTENT_ENCODING_ZSTD))
}

fn decode_text_object_bytes(object: &StoredObject, key: &str) -> Result<Vec<u8>> {
    let has_encoding = !object.metadata.content_encoding.trim().is_empty()
        || object
            .metadata
            .metadata
            .contains_key(METADATA_CONTENT_ENCODING);
    if has_encoding {
        return decode_stored_object_bytes(object, key);
    }
    // Some pre-metadata CAS objects have no encoding marker. Recognize their
    // compression magic before treating the object as raw logical text.
    if object.bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        return unzstd(&object.bytes, key);
    }
    if object.bytes.starts_with(&[0x1f, 0x8b]) {
        return gunzip(&object.bytes, key);
    }
    raw_object_bytes(object, key)
}

fn tool_result_logical_bytes(raw_bytes: &[u8]) -> Cow<'_, [u8]> {
    if raw_bytes.len() <= MAX_TOOL_RESULT_LOGICAL_BYTES {
        return Cow::Borrowed(raw_bytes);
    }

    let marker_len = TOOL_RESULT_TRUNCATION_MARKER.len();
    let mut prefix_len = MAX_TOOL_RESULT_LOGICAL_BYTES.saturating_sub(marker_len);
    if let Ok(text) = std::str::from_utf8(raw_bytes) {
        while prefix_len > 0 && !text.is_char_boundary(prefix_len) {
            prefix_len -= 1;
        }
    }

    let mut out = Vec::with_capacity(prefix_len + marker_len);
    out.extend_from_slice(&raw_bytes[..prefix_len]);
    out.extend_from_slice(TOOL_RESULT_TRUNCATION_MARKER);
    Cow::Owned(out)
}

fn zstd(raw_bytes: &[u8]) -> Result<Vec<u8>> {
    Ok(zstd::stream::encode_all(Cursor::new(raw_bytes), 0)?)
}

fn gzip(raw_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(raw_bytes)?;
    Ok(encoder.finish()?)
}

fn unzstd(bytes: &[u8], key: &str) -> Result<Vec<u8>> {
    let decoder = zstd::stream::read::Decoder::new(Cursor::new(bytes))
        .map_err(|err| anyhow!("CAS object '{key}' has invalid zstd bytes: {err}"))?;
    read_limited(decoder, key, "zstd")
}

fn gunzip(bytes: &[u8], key: &str) -> Result<Vec<u8>> {
    read_limited(GzDecoder::new(bytes), key, "gzip")
}

fn read_limited(reader: impl Read, key: &str, encoding: &str) -> Result<Vec<u8>> {
    let mut decoder = reader.take(MAX_LOGICAL_OBJECT_BYTES + 1);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|err| anyhow!("CAS object '{key}' has invalid {encoding} bytes: {err}"))?;
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

fn filename_for_path(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .map(|name| {
            name.chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                        ch.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect()
        })
        .unwrap_or_else(|| "file".to_string())
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
        compaction_summary_object_key, parse_session_object_key, CasRangeError, CasStore,
        SeekTable, SeekTableCache, SessionCasScope, SessionObjectIdentity, CONTENT_ENCODING_GZIP,
        CONTENT_ENCODING_ZSTD, MAX_LOGICAL_OBJECT_BYTES, MAX_TOOL_RESULT_LOGICAL_BYTES,
        METADATA_AGENT, METADATA_CONTENT_ENCODING, METADATA_KIND, METADATA_KIND_ARTIFACT,
        METADATA_KIND_COMPACTION, METADATA_KIND_ENCRYPTED_REASONING, METADATA_KIND_FILE,
        METADATA_UNCOMPRESSED_SIZE_BYTES, TOOL_RESULT_SEEKABLE_FRAME_BYTES,
        TOOL_RESULT_TRUNCATION_MARKER,
    };
    use crate::control::object_store::{InMemoryObjectStore, ObjectMetadata, ObjectStore};
    use rand::{RngExt, SeedableRng};
    use std::sync::{Arc, Mutex};

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

    #[test]
    fn compaction_summary_keys_use_the_canonical_txt_path() {
        let key = compaction_summary_object_key(
            "team/alpha",
            "session one",
            "submission#1",
            "journal entry/1",
        );

        assert_eq!(
            key,
            "cas/team%2Falpha/sessions/session_one/compactions/submission_1/journal_entry_1.txt"
        );
    }

    #[test]
    fn parses_compaction_summary_key_and_rejects_noncanonical_fields() {
        let parsed = parse_session_object_key(
            "cas/team%2Falpha/sessions/session-1/compactions/submission-1/journal-1.txt",
        )
        .unwrap();
        assert_eq!(parsed.scope.ns, "team/alpha");
        assert_eq!(parsed.scope.session_id, "session-1");
        assert_eq!(parsed.identity.message_id, "submission-1");
        assert_eq!(parsed.identity.part_id, "journal-1");

        let err = parse_session_object_key(
            "cas/team%2falpha/sessions/session-1/compactions/submission-1/journal-1.txt",
        )
        .unwrap_err();
        assert!(err.to_string().contains("unsafe characters"));

        let err = parse_session_object_key(
            "cas/team%2Falpha/sessions/session-1/compactions/submission-1/journal-1.json",
        )
        .unwrap_err();
        assert!(err.to_string().contains("must end with .txt"));
    }

    #[tokio::test]
    async fn stores_compaction_summary_with_exact_metadata_and_session_scope() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects.clone());
        let summary = "# Compaction\n\nKeep this context.";
        let object = store
            .put_compaction_summary(
                "team/alpha",
                "agent-a",
                "session-1",
                "submission-1",
                "journal-1",
                summary,
            )
            .await
            .unwrap();

        assert_eq!(
            object.key,
            "cas/team%2Falpha/sessions/session-1/compactions/submission-1/journal-1.txt"
        );
        assert_eq!(object.media_type, "text/markdown; charset=utf-8");
        assert_eq!(object.filename, "journal-1.txt");
        assert_eq!(object.size_bytes, summary.len() as u64);
        assert!(object.content_encoding.is_empty());
        assert_eq!(object.metadata[METADATA_KIND], METADATA_KIND_COMPACTION);
        assert_eq!(object.metadata[METADATA_AGENT], "agent-a");
        assert_eq!(object.metadata["namespace"], "team/alpha");
        assert_eq!(object.metadata["session_id"], "session-1");
        assert_eq!(object.metadata["submission_id"], "submission-1");
        assert_eq!(object.metadata["journal_entry_id"], "journal-1");

        let stored = objects.get(&object.key).await.unwrap().unwrap();
        assert_eq!(stored.bytes, summary.as_bytes());
        assert_eq!(stored.metadata.media_type, object.media_type);
        assert_eq!(stored.metadata.size_bytes, object.size_bytes);
        assert_eq!(stored.metadata.sha256, object.sha256);
        assert_eq!(stored.metadata.filename, object.filename);
        assert_eq!(stored.metadata.content_encoding, object.content_encoding);
        let mut stored_user_metadata = stored.metadata.metadata;
        stored_user_metadata.remove(crate::control::object_store::OBJECT_VERSION_METADATA);
        assert_eq!(stored_user_metadata, object.metadata);

        let loaded = store
            .get_session_object(
                &SessionCasScope::new("team/alpha", "agent-a", "session-1"),
                &object.key,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.bytes, summary.as_bytes());

        let err = store
            .get_session_object(
                &SessionCasScope::new("team/alpha", "agent-a", "session-2"),
                &object.key,
            )
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("outside the requested session scope"));

        let err = store
            .get_session_object(
                &SessionCasScope::new("team/alpha", "agent-b", "session-1"),
                &object.key,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("metadata field 'agent'"));
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
            .get_session_object_by_key_decoded(&object.key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(scope, writer);
        assert_eq!(stored.bytes, b"hello");
    }

    #[tokio::test]
    async fn stores_file_objects_with_canonical_key_and_metadata() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects);
        let object = store
            .put_file(
                "Tenant:acme:Workspace:main",
                "file-1",
                "/memory/brand guide.md",
                b"draft body",
                "text/markdown",
            )
            .await
            .unwrap();

        assert!(object
            .key
            .starts_with("cas/Tenant%3Aacme%3AWorkspace%3Amain/files/file-1/"));
        assert_eq!(object.media_type, "text/markdown");
        assert_eq!(object.filename, "brand_guide.md");
        assert_eq!(object.metadata[METADATA_KIND], METADATA_KIND_FILE);
        assert_eq!(object.metadata["namespace"], "Tenant:acme:Workspace:main");
        assert_eq!(object.metadata["file_uid"], "file-1");
        assert_eq!(object.metadata["path"], "/memory/brand guide.md");

        let stored = store
            .get_object_decoded(&object.key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.bytes, b"draft body");
        assert_eq!(stored.metadata.sha256, object.sha256);
    }

    #[tokio::test]
    async fn stores_artifact_objects_with_session_ownership_metadata() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects);
        let object = store
            .put_artifact(
                "Tenant:acme:Workspace:main",
                "writer",
                "session-1",
                "artifact-1",
                b"draft body",
                "text/markdown",
                std::collections::HashMap::from([("source".to_string(), "tool".to_string())]),
            )
            .await
            .unwrap();

        assert!(object
            .key
            .starts_with("cas/Tenant%3Aacme%3AWorkspace%3Amain/artifacts/artifact-1/"));
        assert_eq!(object.filename, "");
        assert_eq!(object.metadata[METADATA_KIND], METADATA_KIND_ARTIFACT);
        assert_eq!(object.metadata[METADATA_AGENT], "writer");
        assert_eq!(object.metadata["namespace"], "Tenant:acme:Workspace:main");
        assert_eq!(object.metadata["session_id"], "session-1");
        assert_eq!(object.metadata["artifact_id"], "artifact-1");
        assert_eq!(object.metadata["source"], "tool");
    }

    #[tokio::test]
    async fn compresses_tool_result_with_zstd_when_it_saves_meaningfully() {
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
        assert_eq!(object.content_encoding, CONTENT_ENCODING_ZSTD);
        assert_eq!(object.metadata[METADATA_AGENT], "agent");
        for key_derived_field in ["namespace", "session_id", "message_id", "part_id"] {
            assert!(!object.metadata.contains_key(key_derived_field));
        }
        let stored = store
            .get_session_object_decoded(
                &SessionCasScope::new("acme", "agent", "session-1"),
                &object.key,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.bytes, raw.as_bytes());
        assert!(stored.metadata.content_encoding.is_empty());
        assert!(!stored
            .metadata
            .metadata
            .contains_key(METADATA_CONTENT_ENCODING));
    }

    #[tokio::test]
    async fn stores_incompressible_tool_result_as_seekable_zstd() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects.clone());
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let raw = (0..2 * 1024)
            .map(|_| rng.random_range(0u8..=0xff))
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

        assert_ne!(stored.bytes, raw);
        assert_eq!(stored.metadata.content_encoding, CONTENT_ENCODING_ZSTD);
        assert!(!object.metadata.contains_key(METADATA_CONTENT_ENCODING));
        assert_eq!(
            store
                .get_text_range_decoded(&object.key, 0..raw.len() as u64)
                .await
                .unwrap(),
            raw
        );
    }

    #[tokio::test]
    async fn truncates_tool_result_to_logical_storage_limit() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects);
        let raw = "x".repeat(MAX_TOOL_RESULT_LOGICAL_BYTES + 1024);

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
        let stored = store
            .get_session_object_decoded(
                &SessionCasScope::new("acme", "agent", "session-1"),
                &object.key,
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(stored.bytes.len(), MAX_TOOL_RESULT_LOGICAL_BYTES);
        assert!(stored.bytes.ends_with(TOOL_RESULT_TRUNCATION_MARKER));
        assert_eq!(
            object.metadata[METADATA_UNCOMPRESSED_SIZE_BYTES],
            MAX_TOOL_RESULT_LOGICAL_BYTES.to_string()
        );
    }

    #[tokio::test]
    async fn rejects_tool_result_above_logical_object_limit() {
        let store = CasStore::new(Arc::new(InMemoryObjectStore::default()));
        let raw = vec![b'x'; MAX_LOGICAL_OBJECT_BYTES as usize + 1];

        let err = store
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
            .unwrap_err();

        assert!(err.to_string().contains("maximum supported logical size"));
    }

    #[tokio::test]
    async fn legacy_gzip_object_decodes() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects.clone());
        let raw = b"legacy gzip payload";
        // Use the historical CAS compressor rather than constructing a fixture
        // directly, so legacy-object tests match what old writers stored.
        let gzip_bytes = super::gzip(raw).unwrap();
        let object = objects
            .put(
                "cas/acme/sessions/session-1/messages/message-1/000001.txt",
                &gzip_bytes,
                ObjectMetadata {
                    metadata: std::collections::HashMap::from([
                        (
                            METADATA_CONTENT_ENCODING.to_string(),
                            CONTENT_ENCODING_GZIP.to_string(),
                        ),
                        (METADATA_AGENT.to_string(), "agent".to_string()),
                    ]),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();

        let stored = store
            .get_session_object_decoded(
                &SessionCasScope::new("acme", "agent", "session-1"),
                &object.key,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.bytes, raw);
    }

    #[tokio::test]
    async fn encrypted_reasoning_records_provider_and_model_provenance() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects.clone());

        let object = store
            .put_encrypted_reasoning(
                "acme",
                "agent",
                "session-1",
                "openai",
                "gpt-test",
                r#"{\"type\":\"reasoning\",\"encrypted_content\":\"opaque\"}"#,
            )
            .await
            .unwrap();

        let metadata = objects.head(&object.key).await.unwrap().unwrap();
        assert_eq!(metadata.media_type, "application/octet-stream");
        assert_eq!(
            metadata.metadata.get(METADATA_KIND),
            Some(&METADATA_KIND_ENCRYPTED_REASONING.to_string())
        );
        assert_eq!(
            metadata.metadata.get("provider"),
            Some(&"openai".to_string())
        );
        assert_eq!(
            metadata.metadata.get("model"),
            Some(&"gpt-test".to_string())
        );
    }

    #[tokio::test]
    async fn corrupt_zstd_object_returns_decode_error() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects.clone());
        let object = objects
            .put(
                "cas/acme/sessions/session-1/messages/message-1/000001.txt",
                b"not zstd",
                ObjectMetadata {
                    content_encoding: CONTENT_ENCODING_ZSTD.to_string(),
                    metadata: std::collections::HashMap::from([(
                        METADATA_AGENT.to_string(),
                        "agent".to_string(),
                    )]),
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
        assert!(err.to_string().contains("invalid zstd bytes"));
    }

    #[tokio::test]
    async fn seekable_tool_results_round_trip_ranges_at_frame_boundaries() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects.clone());
        for (index, size) in [
            0,
            17,
            TOOL_RESULT_SEEKABLE_FRAME_BYTES - 1,
            TOOL_RESULT_SEEKABLE_FRAME_BYTES,
            TOOL_RESULT_SEEKABLE_FRAME_BYTES + 1,
            TOOL_RESULT_SEEKABLE_FRAME_BYTES * 2 + 19,
        ]
        .into_iter()
        .enumerate()
        {
            let raw = (0..size).map(|n| b'a' + (n % 26) as u8).collect::<Vec<_>>();
            let object = store
                .put_tool_result(
                    "acme",
                    "agent",
                    "session-1",
                    "message-1",
                    &format!("{index:06}"),
                    "call-1",
                    "search",
                    &raw,
                )
                .await
                .unwrap();
            let stored = objects.get(&object.key).await.unwrap().unwrap();
            assert_eq!(stored.metadata.content_encoding, CONTENT_ENCODING_ZSTD);
            assert_eq!(
                stored.metadata.metadata[METADATA_UNCOMPRESSED_SIZE_BYTES],
                size.to_string()
            );
            let offsets = [
                0,
                size / 2,
                size.saturating_sub(1),
                TOOL_RESULT_SEEKABLE_FRAME_BYTES.min(size),
                (TOOL_RESULT_SEEKABLE_FRAME_BYTES + 1).min(size),
                size,
            ];
            for start in offsets {
                for end in [start, (start + 1).min(size), (start + 97).min(size), size] {
                    assert_eq!(
                        store
                            .get_text_range_decoded(&object.key, start as u64..end as u64)
                            .await
                            .unwrap(),
                        raw[start..end]
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn range_reads_fallback_for_legacy_gzip_and_non_seekable_zstd() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects.clone());
        let raw = b"legacy compression payload";
        for (key, bytes, encoding) in [
            (
                "cas/acme/sessions/session-1/messages/message-1/gzip.txt",
                super::gzip(raw).unwrap(),
                CONTENT_ENCODING_GZIP,
            ),
            (
                "cas/acme/sessions/session-1/messages/message-1/zstd.txt",
                super::zstd(raw).unwrap(),
                CONTENT_ENCODING_ZSTD,
            ),
        ] {
            objects
                .put(
                    key,
                    &bytes,
                    ObjectMetadata {
                        content_encoding: encoding.to_string(),
                        metadata: std::collections::HashMap::from([(
                            METADATA_UNCOMPRESSED_SIZE_BYTES.to_string(),
                            raw.len().to_string(),
                        )]),
                        ..ObjectMetadata::default()
                    },
                )
                .await
                .unwrap();
            assert_eq!(
                store.get_text_range_decoded(key, 2..13).await.unwrap(),
                &raw[2..13]
            );
        }
    }

    #[tokio::test]
    async fn range_reads_detect_legacy_compression_magic_without_metadata() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects.clone());
        let raw = b"legacy magic payload";
        for (key, bytes) in [
            (
                "cas/acme/sessions/session-1/messages/message-1/magic-gzip.txt",
                super::gzip(raw).unwrap(),
            ),
            (
                "cas/acme/sessions/session-1/messages/message-1/magic-zstd.txt",
                super::zstd(raw).unwrap(),
            ),
        ] {
            objects
                .put(key, &bytes, ObjectMetadata::default())
                .await
                .unwrap();
            assert_eq!(
                store
                    .get_text_range_decoded(key, 1..raw.len() as u64 - 1)
                    .await
                    .unwrap(),
                &raw[1..raw.len() - 1]
            );
        }
    }

    #[tokio::test]
    async fn range_reads_return_raw_bytes_when_utf8_boundary_is_split() {
        let store = CasStore::new(Arc::new(InMemoryObjectStore::default()));
        let mut raw = vec![b'a'; TOOL_RESULT_SEEKABLE_FRAME_BYTES - 1];
        raw.extend_from_slice("étail".as_bytes());
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
        let edge = TOOL_RESULT_SEEKABLE_FRAME_BYTES as u64;
        assert_eq!(
            store
                .get_text_range_decoded(&object.key, edge - 1..edge)
                .await
                .unwrap(),
            vec![0xc3]
        );
        assert_eq!(
            store
                .get_text_range_decoded(&object.key, edge - 1..edge + 1)
                .await
                .unwrap(),
            "é".as_bytes()
        );
    }

    #[tokio::test]
    async fn seek_table_is_cached_between_range_reads() {
        #[derive(Default)]
        struct CountingStore {
            inner: InMemoryObjectStore,
            ranges: Mutex<Vec<std::ops::Range<u64>>>,
        }
        #[async_trait::async_trait]
        impl ObjectStore for CountingStore {
            async fn put(
                &self,
                key: &str,
                bytes: &[u8],
                metadata: ObjectMetadata,
            ) -> anyhow::Result<crate::gateway::rpc::data_proto::ObjectRef> {
                self.inner.put(key, bytes, metadata).await
            }
            async fn get(
                &self,
                key: &str,
            ) -> anyhow::Result<Option<crate::control::object_store::StoredObject>> {
                self.inner.get(key).await
            }
            async fn get_range(
                &self,
                key: &str,
                range: std::ops::Range<u64>,
            ) -> anyhow::Result<Option<Vec<u8>>> {
                self.inner.get_range(key, range).await
            }
            async fn get_range_if_version(
                &self,
                key: &str,
                range: std::ops::Range<u64>,
                version: &str,
            ) -> anyhow::Result<Option<Vec<u8>>> {
                self.ranges.lock().unwrap().push(range.clone());
                self.inner.get_range_if_version(key, range, version).await
            }
            async fn head(&self, key: &str) -> anyhow::Result<Option<ObjectMetadata>> {
                self.inner.head(key).await
            }
            async fn delete(&self, key: &str) -> anyhow::Result<()> {
                self.inner.delete(key).await
            }
        }
        let objects = Arc::new(CountingStore::default());
        let store = CasStore::new(objects.clone());
        let raw = vec![b'x'; TOOL_RESULT_SEEKABLE_FRAME_BYTES * 2];
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
        assert_eq!(
            store
                .get_text_range_decoded(&object.key, 3..7)
                .await
                .unwrap(),
            b"xxxx"
        );
        let stored = objects.inner.get(&object.key).await.unwrap().unwrap();
        let footer_start = stored.bytes.len() - super::SEEK_FOOTER_BYTES;
        let (frame_count, descriptor) =
            super::parse_seek_footer(&stored.bytes[footer_start..]).unwrap();
        let entry_size = if descriptor & super::SEEK_CHECKSUM_FLAG != 0 {
            12
        } else {
            8
        };
        let table_size = super::SEEKABLE_SKIPPABLE_HEADER_BYTES
            + frame_count as usize * entry_size
            + super::SEEK_FOOTER_BYTES;
        let table = super::parse_seek_table(
            &stored.bytes[stored.bytes.len() - table_size..],
            frame_count,
            descriptor,
        )
        .unwrap();
        let after_first = objects.ranges.lock().unwrap().len();
        assert_eq!(
            objects.ranges.lock().unwrap().last(),
            Some(&(table.frames[0].stored_start..table.frames[0].stored_end)),
            "single-frame read fetches only that stored frame"
        );
        assert_eq!(
            store
                .get_text_range_decoded(
                    &object.key,
                    TOOL_RESULT_SEEKABLE_FRAME_BYTES as u64 - 1
                        ..TOOL_RESULT_SEEKABLE_FRAME_BYTES as u64 + 1,
                )
                .await
                .unwrap(),
            b"xx"
        );
        assert_eq!(
            objects.ranges.lock().unwrap().len(),
            after_first + 1,
            "second read fetches a frame but not a footer/table"
        );
        assert_eq!(
            objects.ranges.lock().unwrap().last(),
            Some(&(table.frames[0].stored_start..table.frames[1].stored_end)),
            "cross-frame read fetches exactly the intersecting stored frames"
        );
    }

    #[test]
    fn seek_table_cache_evicts_the_least_recently_used_entry_at_capacity() {
        let mut cache = SeekTableCache::default();
        for index in 0..64 {
            cache.insert(
                format!("key-{index}"),
                "version".to_string(),
                SeekTable {
                    frames: Vec::new(),
                    logical_size: index,
                },
            );
        }
        assert!(cache.get("key-0", "version").is_some());
        cache.insert(
            "key-64".to_string(),
            "version".to_string(),
            SeekTable {
                frames: Vec::new(),
                logical_size: 64,
            },
        );

        assert_eq!(cache.entries.len(), 64);
        assert!(
            cache.get("key-1", "version").is_none(),
            "untouched oldest entry is evicted"
        );
        assert_eq!(cache.get("key-0", "version").unwrap().logical_size, 0);
        assert_eq!(cache.get("key-64", "version").unwrap().logical_size, 64);
    }

    #[tokio::test]
    async fn range_reads_invalidate_cached_table_after_overwrite() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects);
        let original = (0..TOOL_RESULT_SEEKABLE_FRAME_BYTES * 2)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let replacement = vec![b'z'; original.len()];
        let first = store
            .put_tool_result(
                "acme",
                "agent",
                "session-1",
                "message-1",
                "000001",
                "call-1",
                "search",
                &original,
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .get_text_range_decoded(&first.key, 17..31)
                .await
                .unwrap(),
            original[17..31]
        );

        let replacement_object = store
            .put_tool_result(
                "acme",
                "agent",
                "session-1",
                "message-1",
                "000001",
                "call-1",
                "search",
                &replacement,
            )
            .await
            .unwrap();
        assert_eq!(replacement_object.key, first.key);
        assert_eq!(
            store
                .get_text_range_decoded(&first.key, 17..31)
                .await
                .unwrap(),
            replacement[17..31]
        );
    }

    #[tokio::test]
    async fn range_read_retries_when_the_object_changes_between_table_and_frame_reads() {
        struct OverwritingStore {
            inner: Arc<InMemoryObjectStore>,
            replacement: Mutex<Option<(String, Vec<u8>, ObjectMetadata)>>,
        }
        #[async_trait::async_trait]
        impl ObjectStore for OverwritingStore {
            async fn put(
                &self,
                key: &str,
                bytes: &[u8],
                metadata: ObjectMetadata,
            ) -> anyhow::Result<crate::gateway::rpc::data_proto::ObjectRef> {
                self.inner.put(key, bytes, metadata).await
            }
            async fn get(
                &self,
                key: &str,
            ) -> anyhow::Result<Option<crate::control::object_store::StoredObject>> {
                self.inner.get(key).await
            }
            async fn get_range(
                &self,
                key: &str,
                range: std::ops::Range<u64>,
            ) -> anyhow::Result<Option<Vec<u8>>> {
                self.inner.get_range(key, range).await
            }
            async fn get_range_if_version(
                &self,
                key: &str,
                range: std::ops::Range<u64>,
                version: &str,
            ) -> anyhow::Result<Option<Vec<u8>>> {
                let bytes = self.inner.get_range_if_version(key, range, version).await?;
                let replacement = { self.replacement.lock().unwrap().take() };
                if let Some((replacement_key, replacement_bytes, replacement_metadata)) =
                    replacement
                {
                    self.inner
                        .put(&replacement_key, &replacement_bytes, replacement_metadata)
                        .await?;
                }
                Ok(bytes)
            }
            async fn head(&self, key: &str) -> anyhow::Result<Option<ObjectMetadata>> {
                self.inner.head(key).await
            }
            async fn delete(&self, key: &str) -> anyhow::Result<()> {
                self.inner.delete(key).await
            }
        }

        let inner = Arc::new(InMemoryObjectStore::default());
        let objects = Arc::new(OverwritingStore {
            inner: inner.clone(),
            replacement: Mutex::new(None),
        });
        let store = CasStore::new(objects.clone());
        let original = vec![b'a'; TOOL_RESULT_SEEKABLE_FRAME_BYTES * 2];
        let replacement = vec![b'z'; original.len()];
        let object = store
            .put_tool_result(
                "acme",
                "agent",
                "session-1",
                "message-1",
                "000001",
                "call-1",
                "search",
                &original,
            )
            .await
            .unwrap();
        let mut replacement_metadata = inner.get(&object.key).await.unwrap().unwrap().metadata;
        let replacement_bytes = super::seekable_zstd(&replacement).unwrap();
        replacement_metadata.size_bytes = replacement_bytes.len() as u64;
        replacement_metadata.sha256 = super::sha256_hex(&replacement_bytes);
        *objects.replacement.lock().unwrap() =
            Some((object.key.clone(), replacement_bytes, replacement_metadata));

        assert_eq!(
            store
                .get_text_range_decoded(&object.key, 17..31)
                .await
                .unwrap(),
            replacement[17..31],
            "a stale seek table must never be applied to replacement bytes"
        );
    }

    #[tokio::test]
    async fn range_reads_reject_invalid_logical_ranges() {
        let store = CasStore::new(Arc::new(InMemoryObjectStore::default()));
        let object = store
            .put_tool_result(
                "acme",
                "agent",
                "session-1",
                "message-1",
                "000001",
                "call-1",
                "search",
                b"logical bytes",
            )
            .await
            .unwrap();
        for range in [5..4, 0..14] {
            let error = store
                .get_text_range_decoded(&object.key, range)
                .await
                .unwrap_err();
            assert!(matches!(
                error.downcast_ref::<CasRangeError>(),
                Some(CasRangeError::InvalidLogicalRange { .. })
            ));
        }
    }

    #[tokio::test]
    async fn seek_table_cross_checks_logical_size_metadata() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let store = CasStore::new(objects.clone());
        let object = store
            .put_tool_result(
                "acme",
                "agent",
                "session-1",
                "message-1",
                "000001",
                "call-1",
                "search",
                b"seek table truth",
            )
            .await
            .unwrap();
        let mut stored = objects.get(&object.key).await.unwrap().unwrap();
        stored.metadata.metadata.insert(
            METADATA_UNCOMPRESSED_SIZE_BYTES.to_string(),
            "999".to_string(),
        );
        objects
            .put(&object.key, &stored.bytes, stored.metadata)
            .await
            .unwrap();
        let error = store
            .get_text_range_decoded(&object.key, 0..1)
            .await
            .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<CasRangeError>(),
            Some(CasRangeError::LogicalSizeMismatch { .. })
        ));
    }
}
