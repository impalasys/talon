// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::config::proto::{
    object_store_config, CloudflareR2ObjectStoreConfig, GcsObjectStoreConfig,
    LocalObjectStoreConfig, ObjectStoreConfig, S3ObjectStoreConfig,
};
use crate::gateway::rpc::models;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use base64::Engine as _;
use google_cloud_auth::credentials::{AccessTokenCredentials, Builder as CredentialsBuilder};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const GCS_STORAGE_SCOPE: &str = "https://www.googleapis.com/auth/devstorage.read_write";
const GCS_API_BASE: &str = "https://storage.googleapis.com";

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

pub async fn object_store_from_config(
    cfg: Option<&ObjectStoreConfig>,
    workspace_dir: &str,
) -> Result<Arc<dyn ObjectStore + Send + Sync>> {
    let default_path = PathBuf::from(workspace_dir).join(".talon").join("objects");
    let Some(cfg) = cfg.and_then(|cfg| cfg.backend.as_ref()) else {
        return Ok(Arc::new(LocalFsObjectStore::new(default_path)));
    };

    match cfg {
        object_store_config::Backend::Local(local) => Ok(Arc::new(
            LocalFsObjectStore::from_config(local, default_path),
        )),
        object_store_config::Backend::Gcs(gcs) => Ok(Arc::new(GcsObjectStore::new(gcs).await?)),
        object_store_config::Backend::S3(s3) => Ok(Arc::new(S3ObjectStore::new(s3).await?)),
        object_store_config::Backend::CloudflareR2(r2) => {
            Ok(Arc::new(CloudflareR2ObjectStore::new(r2)))
        }
    }
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

pub struct LocalFsObjectStore {
    root: PathBuf,
}

pub type LocalObjectStore = LocalFsObjectStore;

impl LocalFsObjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn from_config(cfg: &LocalObjectStoreConfig, default_path: PathBuf) -> Self {
        let path = non_empty(&cfg.path)
            .map(PathBuf::from)
            .unwrap_or(default_path);
        Self::new(path)
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
impl ObjectStore for LocalFsObjectStore {
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
        let metadata_bytes = serde_json::to_vec(&metadata)?;
        if let Some(parent) = data_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&data_path, bytes).await?;
        if let Err(err) = tokio::fs::write(&metadata_path, metadata_bytes).await {
            let _ = tokio::fs::remove_file(&data_path).await;
            return Err(err.into());
        }
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

pub struct CloudflareR2ObjectStore {
    client: reqwest::Client,
    endpoint: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudflareR2MetadataEnvelope {
    metadata: ObjectMetadata,
}

impl CloudflareR2ObjectStore {
    pub fn new(cfg: &CloudflareR2ObjectStoreConfig) -> Self {
        let endpoint = non_empty(&cfg.endpoint_url)
            .or_else(|| std::env::var("TALON_CLOUDFLARE_R2_URL").ok())
            .unwrap_or_else(|| "http://talon-r2.internal".to_string());
        Self {
            client: reqwest::Client::new(),
            endpoint: endpoint.trim_end_matches('/').to_string(),
        }
    }

    fn url(&self, key: &str) -> Result<String> {
        validate_key(key)?;
        Ok(format!(
            "{}/objects/{}",
            self.endpoint,
            urlencoding::encode(key)
        ))
    }

    fn metadata_header(metadata: &ObjectMetadata) -> Result<String> {
        Ok(
            base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(
                &CloudflareR2MetadataEnvelope {
                    metadata: metadata.clone(),
                },
            )?),
        )
    }

    fn metadata_from_header(headers: &reqwest::header::HeaderMap) -> ObjectMetadata {
        let Some(value) = headers
            .get("x-talon-object-metadata")
            .and_then(|value| value.to_str().ok())
        else {
            return ObjectMetadata::default();
        };
        base64::engine::general_purpose::STANDARD
            .decode(value)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<CloudflareR2MetadataEnvelope>(&bytes).ok())
            .map(|envelope| envelope.metadata)
            .unwrap_or_default()
    }
}

#[async_trait]
impl ObjectStore for CloudflareR2ObjectStore {
    async fn put(
        &self,
        key: &str,
        bytes: &[u8],
        mut metadata: ObjectMetadata,
    ) -> Result<models::ObjectRef> {
        metadata.size_bytes = bytes.len() as u64;
        let mut request = self
            .client
            .put(self.url(key)?)
            .header("x-talon-object-metadata", Self::metadata_header(&metadata)?)
            .body(bytes.to_vec());
        if !metadata.media_type.is_empty() {
            request = request.header(reqwest::header::CONTENT_TYPE, metadata.media_type.clone());
        }
        ensure_success(request.send().await?, "Cloudflare R2 object upload").await?;
        Ok(object_ref(key, metadata))
    }

    async fn get(&self, key: &str) -> Result<Option<StoredObject>> {
        let response = self.client.get(self.url(key)?).send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = ensure_success(response, "Cloudflare R2 object download").await?;
        let metadata = Self::metadata_from_header(response.headers());
        let bytes = response.bytes().await?.to_vec();
        Ok(Some(StoredObject {
            metadata: ObjectMetadata {
                size_bytes: bytes.len() as u64,
                ..metadata
            },
            bytes,
        }))
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let response = self.client.delete(self.url(key)?).send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        ensure_success(response, "Cloudflare R2 object delete").await?;
        Ok(())
    }
}

pub struct GcsObjectStore {
    client: reqwest::Client,
    credentials: AccessTokenCredentials,
    bucket: String,
    prefix: String,
    api_base: String,
}

impl GcsObjectStore {
    pub async fn new(cfg: &GcsObjectStoreConfig) -> Result<Self> {
        if cfg.bucket.trim().is_empty() {
            return Err(anyhow!("gcs object store requires bucket"));
        }
        let credentials = CredentialsBuilder::default()
            .with_scopes([GCS_STORAGE_SCOPE])
            .build_access_token_credentials()
            .context("failed to build Google access token credentials for GCS object store")?;
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .context("failed to build GCS object store HTTP client")?,
            credentials,
            bucket: cfg.bucket.trim().to_string(),
            prefix: normalize_prefix(&cfg.prefix)?,
            api_base: non_empty(&cfg.api_base_url).unwrap_or_else(|| GCS_API_BASE.to_string()),
        })
    }

    async fn bearer_token(&self) -> Result<String> {
        Ok(self.credentials.access_token().await?.token)
    }

    fn object_key(&self, key: &str) -> Result<String> {
        prefixed_key(&self.prefix, key)
    }
}

#[async_trait]
impl ObjectStore for GcsObjectStore {
    async fn put(
        &self,
        key: &str,
        bytes: &[u8],
        mut metadata: ObjectMetadata,
    ) -> Result<models::ObjectRef> {
        metadata.size_bytes = bytes.len() as u64;
        let object_key = self.object_key(key)?;
        let upload_url = format!(
            "{}/upload/storage/v1/b/{}/o?uploadType=media&name={}",
            self.api_base.trim_end_matches('/'),
            urlencoding::encode(&self.bucket),
            urlencoding::encode(&object_key)
        );
        let mut request = self
            .client
            .post(upload_url)
            .bearer_auth(self.bearer_token().await?)
            .body(bytes.to_vec());
        if !metadata.media_type.is_empty() {
            request = request
                .header(reqwest::header::CONTENT_TYPE, metadata.media_type.clone())
                .header("x-goog-meta-talon-media-type", metadata.media_type.clone());
        }
        if !metadata.sha256.is_empty() {
            request = request.header("x-goog-meta-talon-sha256", metadata.sha256.clone());
        }
        if !metadata.filename.is_empty() {
            request = request.header("x-goog-meta-talon-filename", metadata.filename.clone());
        }
        for (name, value) in &metadata.metadata {
            request = request.header(format!("x-goog-meta-talon-{name}"), value);
        }
        ensure_success(request.send().await?, "GCS object upload").await?;
        Ok(object_ref(key, metadata))
    }

    async fn get(&self, key: &str) -> Result<Option<StoredObject>> {
        let object_key = self.object_key(key)?;
        let url = format!(
            "{}/storage/v1/b/{}/o/{}?alt=media",
            self.api_base.trim_end_matches('/'),
            urlencoding::encode(&self.bucket),
            urlencoding::encode(&object_key)
        );
        let response = self
            .client
            .get(url)
            .bearer_auth(self.bearer_token().await?)
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = ensure_success(response, "GCS object download").await?;
        let metadata = metadata_from_headers(response.headers());
        let bytes = response.bytes().await?.to_vec();
        Ok(Some(StoredObject {
            metadata: ObjectMetadata {
                size_bytes: bytes.len() as u64,
                ..metadata
            },
            bytes,
        }))
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let object_key = self.object_key(key)?;
        let url = format!(
            "{}/storage/v1/b/{}/o/{}",
            self.api_base.trim_end_matches('/'),
            urlencoding::encode(&self.bucket),
            urlencoding::encode(&object_key)
        );
        let response = self
            .client
            .delete(url)
            .bearer_auth(self.bearer_token().await?)
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        ensure_success(response, "GCS object delete").await?;
        Ok(())
    }
}

pub struct S3ObjectStore {
    client: aws_sdk_s3::Client,
    bucket: String,
    prefix: String,
}

impl S3ObjectStore {
    pub async fn new(cfg: &S3ObjectStoreConfig) -> Result<Self> {
        if cfg.bucket.trim().is_empty() {
            return Err(anyhow!("s3 object store requires bucket"));
        }
        use aws_config::BehaviorVersion;
        let mut loader = aws_config::defaults(BehaviorVersion::latest());
        if let Some(region) = non_empty(&cfg.region) {
            loader = loader.region(aws_config::Region::new(region));
        }
        let shared_config = loader.load().await;
        let mut builder = aws_sdk_s3::config::Builder::from(&shared_config);
        if let Some(endpoint_url) = non_empty(&cfg.endpoint_url) {
            builder = builder.endpoint_url(endpoint_url);
        }
        builder = builder.force_path_style(cfg.force_path_style);
        Ok(Self {
            client: aws_sdk_s3::Client::from_conf(builder.build()),
            bucket: cfg.bucket.trim().to_string(),
            prefix: normalize_prefix(&cfg.prefix)?,
        })
    }

    fn object_key(&self, key: &str) -> Result<String> {
        prefixed_key(&self.prefix, key)
    }
}

#[async_trait]
impl ObjectStore for S3ObjectStore {
    async fn put(
        &self,
        key: &str,
        bytes: &[u8],
        mut metadata: ObjectMetadata,
    ) -> Result<models::ObjectRef> {
        metadata.size_bytes = bytes.len() as u64;
        let object_key = self.object_key(key)?;
        let mut s3_metadata = metadata.metadata.clone();
        if !metadata.media_type.is_empty() {
            s3_metadata.insert("talon-media-type".to_string(), metadata.media_type.clone());
        }
        if !metadata.sha256.is_empty() {
            s3_metadata.insert("talon-sha256".to_string(), metadata.sha256.clone());
        }
        if !metadata.filename.is_empty() {
            s3_metadata.insert("talon-filename".to_string(), metadata.filename.clone());
        }

        let mut request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(object_key)
            .body(aws_sdk_s3::primitives::ByteStream::from(bytes.to_vec()))
            .set_metadata(Some(s3_metadata));
        if !metadata.media_type.is_empty() {
            request = request.content_type(metadata.media_type.clone());
        }
        request.send().await?;
        Ok(object_ref(key, metadata))
    }

    async fn get(&self, key: &str) -> Result<Option<StoredObject>> {
        let object_key = self.object_key(key)?;
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(object_key)
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(err) if is_s3_not_found(&err) => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let metadata = metadata_from_s3_response(&response);
        let bytes = response.body.collect().await?.into_bytes().to_vec();
        Ok(Some(StoredObject {
            metadata: ObjectMetadata {
                size_bytes: bytes.len() as u64,
                ..metadata
            },
            bytes,
        }))
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let object_key = self.object_key(key)?;
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(object_key)
            .send()
            .await?;
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

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn normalize_prefix(prefix: &str) -> Result<String> {
    let Some(prefix) = non_empty(prefix) else {
        return Ok(String::new());
    };
    let prefix = prefix.trim_matches('/').to_string();
    validate_key(&prefix)?;
    Ok(prefix)
}

fn prefixed_key(prefix: &str, key: &str) -> Result<String> {
    validate_key(key)?;
    if prefix.is_empty() {
        Ok(key.to_string())
    } else {
        Ok(format!("{}/{}", prefix.trim_end_matches('/'), key))
    }
}

async fn ensure_success(response: reqwest::Response, operation: &str) -> Result<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(anyhow!("{operation} failed with HTTP {status}: {body}"))
}

fn metadata_from_headers(headers: &reqwest::header::HeaderMap) -> ObjectMetadata {
    let media_type = headers
        .get("x-goog-meta-talon-media-type")
        .or_else(|| headers.get(reqwest::header::CONTENT_TYPE))
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let sha256 = headers
        .get("x-goog-meta-talon-sha256")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let filename = headers
        .get("x-goog-meta-talon-filename")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let metadata = headers
        .iter()
        .filter_map(|(name, value)| {
            let name = name.as_str().strip_prefix("x-goog-meta-talon-")?;
            if matches!(name, "media-type" | "sha256" | "filename") {
                return None;
            }
            Some((name.to_string(), value.to_str().ok()?.to_string()))
        })
        .collect();
    ObjectMetadata {
        media_type,
        size_bytes: 0,
        sha256,
        filename,
        metadata,
    }
}

fn metadata_from_s3_response(
    response: &aws_sdk_s3::operation::get_object::GetObjectOutput,
) -> ObjectMetadata {
    let s3_metadata = response.metadata().cloned().unwrap_or_default();
    let media_type = s3_metadata
        .get("talon-media-type")
        .cloned()
        .or_else(|| response.content_type().map(ToString::to_string))
        .unwrap_or_default();
    let sha256 = s3_metadata.get("talon-sha256").cloned().unwrap_or_default();
    let filename = s3_metadata
        .get("talon-filename")
        .cloned()
        .unwrap_or_default();
    let metadata = s3_metadata
        .iter()
        .filter_map(|(key, value)| {
            if matches!(
                key.as_str(),
                "talon-media-type" | "talon-sha256" | "talon-filename"
            ) {
                None
            } else {
                Some((key.clone(), value.clone()))
            }
        })
        .collect();
    ObjectMetadata {
        media_type,
        size_bytes: 0,
        sha256,
        filename,
        metadata,
    }
}

fn is_s3_not_found(
    err: &aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::get_object::GetObjectError>,
) -> bool {
    err.as_service_error()
        .map(|err| err.is_no_such_key())
        .unwrap_or(false)
        || err
            .raw_response()
            .map(|response| response.status().as_u16() == 404)
            .unwrap_or(false)
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
    use super::{
        object_store_from_config, prefixed_key, GcsObjectStore, LocalObjectStore, ObjectMetadata,
        ObjectStore, S3ObjectStore,
    };
    use crate::config::proto::{
        object_store_config, GcsObjectStoreConfig, LocalObjectStoreConfig, ObjectStoreConfig,
        S3ObjectStoreConfig,
    };

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

    #[tokio::test]
    async fn local_object_store_cleans_up_data_when_metadata_write_fails() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalObjectStore::new(dir.path());
        let key = "sessions/session-1/image.png";
        let data_path = store.data_path(key).unwrap();
        let metadata_path = store.metadata_path(key).unwrap();
        tokio::fs::create_dir_all(&metadata_path).await.unwrap();

        let err = store
            .put(
                key,
                b"image-bytes",
                ObjectMetadata {
                    media_type: "image/png".to_string(),
                    filename: "image.png".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap_err();

        assert!(
            err.to_string().contains("directory") || err.to_string().contains("Is a directory")
        );
        assert!(!data_path.exists());
    }

    #[tokio::test]
    async fn object_store_from_config_uses_configured_local_path() {
        let dir = tempfile::tempdir().unwrap();
        let object_dir = dir.path().join("objects");
        let store = object_store_from_config(
            Some(&ObjectStoreConfig {
                backend: Some(object_store_config::Backend::Local(
                    LocalObjectStoreConfig {
                        path: object_dir.display().to_string(),
                    },
                )),
            }),
            "/unused-workspace",
        )
        .await
        .unwrap();

        store
            .put(
                "sessions/session-1/file.txt",
                b"hello",
                ObjectMetadata {
                    media_type: "text/plain".to_string(),
                    filename: "file.txt".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(
            tokio::fs::read(object_dir.join("sessions/session-1/file.txt"))
                .await
                .unwrap(),
            b"hello"
        );
    }

    #[test]
    fn prefixed_key_rejects_unsafe_object_keys() {
        assert_eq!(
            prefixed_key("tenant-a", "sessions/session-1/image.png").unwrap(),
            "tenant-a/sessions/session-1/image.png"
        );
        assert!(prefixed_key("tenant-a", "../escape").is_err());
    }

    #[tokio::test]
    async fn cloud_backends_validate_required_bucket_before_credentials() {
        let gcs = match GcsObjectStore::new(&GcsObjectStoreConfig {
            bucket: String::new(),
            prefix: String::new(),
            api_base_url: String::new(),
        })
        .await
        {
            Ok(_) => panic!("expected missing GCS bucket error"),
            Err(err) => err,
        };
        assert!(gcs.to_string().contains("requires bucket"));

        let s3 = match S3ObjectStore::new(&S3ObjectStoreConfig {
            bucket: String::new(),
            prefix: String::new(),
            region: String::new(),
            endpoint_url: String::new(),
            force_path_style: false,
        })
        .await
        {
            Ok(_) => panic!("expected missing S3 bucket error"),
            Err(err) => err,
        };
        assert!(s3.to_string().contains("requires bucket"));
    }

    #[test]
    fn s3_not_found_detection_uses_structured_errors() {
        let no_such_key = aws_sdk_s3::error::SdkError::service_error(
            aws_sdk_s3::operation::get_object::GetObjectError::NoSuchKey(
                aws_sdk_s3::types::error::NoSuchKey::builder().build(),
            ),
            aws_smithy_runtime_api::client::orchestrator::HttpResponse::new(
                aws_smithy_runtime_api::http::StatusCode::try_from(404).unwrap(),
                aws_smithy_types::body::SdkBody::empty(),
            ),
        );
        assert!(super::is_s3_not_found(&no_such_key));

        let http_not_found = aws_sdk_s3::error::SdkError::service_error(
            aws_sdk_s3::operation::get_object::GetObjectError::unhandled("missing"),
            aws_smithy_runtime_api::client::orchestrator::HttpResponse::new(
                aws_smithy_runtime_api::http::StatusCode::try_from(404).unwrap(),
                aws_smithy_types::body::SdkBody::empty(),
            ),
        );
        assert!(super::is_s3_not_found(&http_not_found));

        let internal_error = aws_sdk_s3::error::SdkError::service_error(
            aws_sdk_s3::operation::get_object::GetObjectError::unhandled("server error"),
            aws_smithy_runtime_api::client::orchestrator::HttpResponse::new(
                aws_smithy_runtime_api::http::StatusCode::try_from(500).unwrap(),
                aws_smithy_types::body::SdkBody::empty(),
            ),
        );
        assert!(!super::is_s3_not_found(&internal_error));
    }
}
