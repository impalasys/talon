// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Complete a pending turn between the eligibility read and session-lock read.
use super::{keys, KeyValueBatchOperation, KeyValueStore, ListOptions, ProtoKeyValueStoreExt};
use crate::talon::protocol::data_proto;
use crate::test_support::MockKvStore;
use anyhow::Result;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub(crate) struct CompleteBeforeSessionRead {
    pub inner: Arc<MockKvStore>,
    pub session: keys::ResourceKey,
    pub submission: keys::ResourceKey,
    pub armed: AtomicBool,
}
#[async_trait::async_trait]
impl KeyValueStore for CompleteBeforeSessionRead {
    async fn get(&self, key: &keys::ResourceKey) -> Result<Option<Vec<u8>>> {
        if key == &self.session && self.armed.swap(false, Ordering::SeqCst) {
            let mut submission: data_proto::SessionSubmission =
                self.inner.get_msg(&self.submission).await?.unwrap();
            submission.status = data_proto::SessionSubmissionStatus::Committed as i32;
            self.inner.set_msg(&self.submission, &submission).await?;
            let mut session: data_proto::Session =
                self.inner.get_msg(&self.session).await?.unwrap();
            session.status = "IDLE".into();
            self.inner.set_msg(&self.session, &session).await?;
        }
        self.inner.get(key).await
    }
    async fn set(&self, key: &keys::ResourceKey, value: &[u8]) -> Result<()> {
        self.inner.set(key, value).await
    }
    async fn compare_and_swap(
        &self,
        key: &keys::ResourceKey,
        expected: Option<&[u8]>,
        value: &[u8],
    ) -> Result<bool> {
        self.inner.compare_and_swap(key, expected, value).await
    }
    async fn compare_and_delete(&self, key: &keys::ResourceKey, expected: &[u8]) -> Result<bool> {
        self.inner.compare_and_delete(key, expected).await
    }
    async fn compare_and_swap_batch(&self, operations: &[KeyValueBatchOperation]) -> Result<bool> {
        self.inner.compare_and_swap_batch(operations).await
    }
    async fn delete(&self, key: &keys::ResourceKey) -> Result<()> {
        self.inner.delete(key).await
    }
    async fn list_keys(
        &self,
        list: &keys::ResourceList,
        options: Option<ListOptions<'_>>,
    ) -> Result<Vec<keys::ResourceKey>> {
        self.inner.list_keys(list, options).await
    }
}
