// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use ::protocol::data_proto;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ObjectMetadata {
    pub media_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub filename: String,
    pub content_encoding: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    pub bytes: Vec<u8>,
    pub metadata: ObjectMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedObjectUrl {
    pub url: String,
    pub expires_at_unix_seconds: i64,
    pub required_headers: HashMap<String, String>,
}

#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn put(
        &self,
        key: &str,
        bytes: &[u8],
        metadata: ObjectMetadata,
    ) -> Result<data_proto::ObjectRef>;
    async fn get(&self, key: &str) -> Result<Option<StoredObject>>;
    async fn head(&self, key: &str) -> Result<Option<ObjectMetadata>>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn signed_get_url(
        &self,
        _key: &str,
        _expires_in: Duration,
    ) -> Result<Option<SignedObjectUrl>> {
        Ok(None)
    }
    async fn signed_put_url(
        &self,
        _key: &str,
        _metadata: ObjectMetadata,
        _expires_in: Duration,
    ) -> Result<Option<SignedObjectUrl>> {
        Ok(None)
    }
}
