// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::gateway::rpc::models;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ObjectMetadata {
    pub media_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub filename: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    pub bytes: Vec<u8>,
    pub metadata: ObjectMetadata,
}

#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn put(
        &self,
        key: &str,
        bytes: &[u8],
        metadata: ObjectMetadata,
    ) -> Result<models::ObjectRef>;
    async fn get(&self, key: &str) -> Result<Option<StoredObject>>;
    async fn delete(&self, key: &str) -> Result<()>;
}

pub fn default_object_store() -> Arc<dyn ObjectStore + Send + Sync> {
    Arc::new(InMemoryObjectStore::default())
}

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
    ) -> Result<models::ObjectRef> {
        validate_key(key)?;
        metadata.size_bytes = bytes.len() as u64;
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

    async fn delete(&self, key: &str) -> Result<()> {
        validate_key(key)?;
        self.objects.write().await.remove(key);
        Ok(())
    }
}

pub struct LocalObjectStore {
    root: PathBuf,
}

impl LocalObjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn data_path(&self, key: &str) -> Result<PathBuf> {
        Ok(self.root.join(safe_relative_path(key)?))
    }

    fn metadata_path(&self, key: &str) -> Result<PathBuf> {
        let mut path = self.data_path(key)?;
        let filename = path
            .file_name()
            .ok_or_else(|| anyhow!("object key must include a file name"))?
            .to_string_lossy()
            .to_string();
        path.set_file_name(format!("{filename}.metadata.json"));
        Ok(path)
    }
}

#[async_trait]
impl ObjectStore for LocalObjectStore {
    async fn put(
        &self,
        key: &str,
        bytes: &[u8],
        mut metadata: ObjectMetadata,
    ) -> Result<models::ObjectRef> {
        validate_key(key)?;
        metadata.size_bytes = bytes.len() as u64;
        let data_path = self.data_path(key)?;
        let metadata_path = self.metadata_path(key)?;
        if let Some(parent) = data_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&data_path, bytes).await?;
        tokio::fs::write(&metadata_path, serde_json::to_vec(&metadata)?).await?;
        Ok(object_ref(key, metadata))
    }

    async fn get(&self, key: &str) -> Result<Option<StoredObject>> {
        validate_key(key)?;
        let data_path = self.data_path(key)?;
        let metadata_path = self.metadata_path(key)?;
        let bytes = match tokio::fs::read(&data_path).await {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let mut metadata = match tokio::fs::read(&metadata_path).await {
            Ok(bytes) => serde_json::from_slice::<ObjectMetadata>(&bytes)?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => ObjectMetadata::default(),
            Err(err) => return Err(err.into()),
        };
        metadata.size_bytes = bytes.len() as u64;
        Ok(Some(StoredObject { bytes, metadata }))
    }

    async fn delete(&self, key: &str) -> Result<()> {
        validate_key(key)?;
        let data_path = self.data_path(key)?;
        let metadata_path = self.metadata_path(key)?;
        for path in [data_path, metadata_path] {
            match tokio::fs::remove_file(path).await {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            }
        }
        Ok(())
    }
}

fn object_ref(key: &str, metadata: ObjectMetadata) -> models::ObjectRef {
    models::ObjectRef {
        key: key.to_string(),
        media_type: metadata.media_type,
        size_bytes: metadata.size_bytes,
        sha256: metadata.sha256,
        filename: metadata.filename,
        metadata: metadata.metadata,
    }
}

fn validate_key(key: &str) -> Result<()> {
    if key.trim().is_empty() {
        return Err(anyhow!("object key is required"));
    }
    let _ = safe_relative_path(key)?;
    Ok(())
}

fn safe_relative_path(key: &str) -> Result<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::{LocalObjectStore, ObjectMetadata, ObjectStore};

    #[tokio::test]
    async fn local_object_store_round_trips_bytes_and_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalObjectStore::new(dir.path());
        let object = store
            .put(
                "sessions/session-1/image.png",
                b"image-bytes",
                ObjectMetadata {
                    media_type: "image/png".to_string(),
                    filename: "image.png".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(object.key, "sessions/session-1/image.png");
        assert_eq!(object.media_type, "image/png");
        assert_eq!(object.size_bytes, 11);

        let stored = store
            .get("sessions/session-1/image.png")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.bytes, b"image-bytes");
        assert_eq!(stored.metadata.media_type, "image/png");
        assert_eq!(stored.metadata.filename, "image.png");
    }

    #[tokio::test]
    async fn local_object_store_rejects_parent_paths() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalObjectStore::new(dir.path());
        let err = store
            .put("../escape", b"bytes", ObjectMetadata::default())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("cannot contain '..'"));
    }
}
