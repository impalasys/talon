// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Shared storage, messaging and execution interfaces.

use std::{pin::Pin, sync::Arc};
use talon_identifiers::keys::{ResourceKey, ResourceList};
pub mod llm;
pub mod object_store;
pub mod rpc;
pub mod scheduler;
pub mod search;
pub mod task;
pub use task::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Order {
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ListOptions<'a> {
    pub order: Order,
    pub limit: Option<usize>,
    pub before_name: Option<&'a str>,
    pub after_name: Option<&'a str>,
}

impl Default for ListOptions<'_> {
    fn default() -> Self {
        Self {
            order: Order::Asc,
            limit: None,
            before_name: None,
            after_name: None,
        }
    }
}

impl<'a> ListOptions<'a> {
    pub fn asc() -> Self {
        Self::default()
    }

    pub fn desc() -> Self {
        Self {
            order: Order::Desc,
            ..Self::default()
        }
    }

    pub fn ordered(order: Order) -> Self {
        Self {
            order,
            ..Self::default()
        }
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn before_name(mut self, before_name: Option<&'a str>) -> Self {
        self.before_name = before_name;
        self
    }

    pub fn after_name(mut self, after_name: Option<&'a str>) -> Self {
        self.after_name = after_name;
        self
    }
}

pub fn apply_list_options_to_keys(
    mut keys: Vec<ResourceKey>,
    options: ListOptions<'_>,
) -> Vec<ResourceKey> {
    if options.limit == Some(0) {
        return Vec::new();
    }

    keys.retain(|key| {
        options
            .before_name
            .map_or(true, |cursor| key.name.as_str() < cursor)
            && options
                .after_name
                .map_or(true, |cursor| key.name.as_str() > cursor)
    });
    if options.order == Order::Desc {
        keys.sort_by(|left, right| right.cmp(left));
    } else {
        keys.sort();
    }
    if let Some(limit) = options.limit {
        keys.truncate(limit);
    }
    keys
}

pub fn apply_list_options_to_entries(
    mut entries: Vec<(ResourceKey, Vec<u8>)>,
    options: ListOptions<'_>,
) -> Vec<(ResourceKey, Vec<u8>)> {
    if options.limit == Some(0) {
        return Vec::new();
    }

    entries.retain(|(key, _)| {
        options
            .before_name
            .map_or(true, |cursor| key.name.as_str() < cursor)
            && options
                .after_name
                .map_or(true, |cursor| key.name.as_str() > cursor)
    });
    if options.order == Order::Desc {
        entries.sort_by(|left, right| right.0.cmp(&left.0));
    } else {
        entries.sort_by(|left, right| left.0.cmp(&right.0));
    }
    if let Some(limit) = options.limit {
        entries.truncate(limit);
    }
    entries
}

#[async_trait::async_trait]
pub trait KeyValueStore: Send + Sync {
    /// Retrieve a raw byte sequence from the store
    async fn get(&self, key: &ResourceKey) -> anyhow::Result<Option<Vec<u8>>>;

    /// Store a raw byte sequence into the store
    async fn set(&self, key: &ResourceKey, value: &[u8]) -> anyhow::Result<()>;

    /// Atomically replace the current value when it matches the expected value.
    async fn compare_and_swap(
        &self,
        key: &ResourceKey,
        expected: Option<&[u8]>,
        value: &[u8],
    ) -> anyhow::Result<bool>;

    /// Delete a key.
    async fn delete(&self, key: &ResourceKey) -> anyhow::Result<()>;

    /// List direct children matching a resource parent and optional kind.
    async fn list_keys(
        &self,
        list: &ResourceList,
        options: Option<ListOptions<'_>>,
    ) -> anyhow::Result<Vec<ResourceKey>>;

    /// List all matching direct child key/value pairs.
    async fn list_entries(
        &self,
        list: &ResourceList,
        options: Option<ListOptions<'_>>,
    ) -> anyhow::Result<Vec<(ResourceKey, Vec<u8>)>> {
        let keys = self.list_keys(list, options).await?;
        let mut entries = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(value) = self.get(&key).await? {
                entries.push((key, value));
            }
        }
        Ok(entries)
    }
}

#[async_trait::async_trait]
pub trait ProtoKeyValueStoreExt {
    async fn get_msg<M: prost::Message + Default>(
        &self,
        key: &ResourceKey,
    ) -> anyhow::Result<Option<M>>;
    async fn set_msg<M: prost::Message + Sync>(
        &self,
        key: &ResourceKey,
        msg: &M,
    ) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
impl<T: KeyValueStore + ?Sized> ProtoKeyValueStoreExt for T {
    async fn get_msg<M: prost::Message + Default>(
        &self,
        key: &ResourceKey,
    ) -> anyhow::Result<Option<M>> {
        match self.get(key).await? {
            Some(bytes) => Ok(Some(M::decode(bytes.as_slice())?)),
            None => Ok(None),
        }
    }

    async fn set_msg<M: prost::Message + Sync>(
        &self,
        key: &ResourceKey,
        msg: &M,
    ) -> anyhow::Result<()> {
        self.set(key, &msg.encode_to_vec()).await
    }
}

#[async_trait::async_trait]
pub trait MessagePublisher: Send + Sync {
    /// Publish a raw byte payload to a topic
    async fn publish(&self, topic: &str, message: &[u8]) -> anyhow::Result<()>;

    /// Subscribe to a topic and return a stream of raw byte payloads
    async fn subscribe(
        &self,
        topic: &str,
    ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>>;
}
