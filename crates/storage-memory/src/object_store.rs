// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub use ::contracts::object_store::*;
use ::protocol::data_proto;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::Arc,
};
const LEGACY_CONTENT_ENCODING_METADATA: &str = "content_encoding";
#[derive(Default)]
pub struct InMemoryObjectStore {
    objects: tokio::sync::RwLock<HashMap<String, StoredObject>>,
}

#[async_trait]
impl ObjectStore for InMemoryObjectStore {
    async fn put(
        &self,
        key: &str,
        bytes: &[u8],
        mut metadata: ObjectMetadata,
    ) -> Result<data_proto::ObjectRef> {
        validate_key(key)?;
        metadata.size_bytes = bytes.len() as u64;
        let metadata = normalize_object_metadata(metadata);
        self.objects.write().await.insert(
            key.to_string(),
            StoredObject {
                bytes: bytes.to_vec(),
                metadata: metadata.clone(),
            },
        );
        Ok(object_ref(key, metadata))
    }

    async fn get(&self, key: &str) -> Result<Option<StoredObject>> {
        validate_key(key)?;
        Ok(self.objects.read().await.get(key).cloned())
    }

    async fn head(&self, key: &str) -> Result<Option<ObjectMetadata>> {
        validate_key(key)?;
        Ok(self
            .objects
            .read()
            .await
            .get(key)
            .map(|object| object.metadata.clone()))
    }

    async fn delete(&self, key: &str) -> Result<()> {
        validate_key(key)?;
        self.objects.write().await.remove(key);
        Ok(())
    }
}

pub fn default_object_store() -> Arc<dyn ObjectStore + Send + Sync> {
    Arc::new(InMemoryObjectStore::default())
}

pub fn object_ref(key: &str, metadata: ObjectMetadata) -> data_proto::ObjectRef {
    data_proto::ObjectRef {
        key: key.to_string(),
        media_type: metadata.media_type,
        size_bytes: metadata.size_bytes,
        sha256: metadata.sha256,
        filename: metadata.filename,
        content_encoding: metadata.content_encoding,
        metadata: metadata.metadata,
    }
}

pub fn normalize_object_metadata(mut metadata: ObjectMetadata) -> ObjectMetadata {
    if metadata.content_encoding.trim().is_empty() {
        if let Some(content_encoding) = metadata.metadata.remove(LEGACY_CONTENT_ENCODING_METADATA) {
            metadata.content_encoding = content_encoding;
        }
    } else {
        metadata.metadata.remove(LEGACY_CONTENT_ENCODING_METADATA);
    }
    metadata
}

pub fn validate_key(key: &str) -> Result<()> {
    if key.trim().is_empty() {
        return Err(anyhow!("object key is required"));
    }
    let _ = safe_relative_path(key)?;
    Ok(())
}

pub fn safe_relative_path(key: &str) -> Result<PathBuf> {
    let path = Path::new(key);
    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(anyhow!(
                    "object key must be relative and cannot contain '..'"
                ));
            }
        }
    }
    if safe.as_os_str().is_empty() {
        return Err(anyhow!("object key is required"));
    }
    Ok(safe)
}
