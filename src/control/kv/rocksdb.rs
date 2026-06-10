// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{
    keys::{ResourceKey, ResourceList},
    KeyValueStore,
};
use anyhow::{anyhow, bail, Result};
use rocksdb::{
    BlockBasedOptions, Cache, DBCompressionType, IteratorMode, Options, WriteOptions, DB,
};
use std::{path::Path, sync::Arc, time::Instant};
use tokio::sync::Mutex;
use tracing::{field, Instrument, Span};

fn key_bytes(key: &ResourceKey) -> Vec<u8> {
    key.canonical().into_bytes()
}

fn prefix_bytes(list: &ResourceList) -> Vec<u8> {
    list.canonical_prefix().into_bytes()
}

fn page_cursor_bytes(list: &ResourceList, kind: &str, before_name: &str) -> Vec<u8> {
    key_bytes(&ResourceKey {
        namespace: list.parent.namespace.clone(),
        parent_path: list.parent.parent_path.clone(),
        kind: kind.to_string(),
        name: before_name.to_string(),
    })
}

fn page_seek_bytes(prefix: &[u8], cursor: Option<&[u8]>) -> Vec<u8> {
    match cursor {
        Some(cursor) => cursor.to_vec(),
        None => {
            let mut seek = prefix.to_vec();
            seek.push(0xff);
            seek
        }
    }
}

fn parse_key(bytes: &[u8]) -> Result<ResourceKey> {
    ResourceKey::parse_canonical(std::str::from_utf8(bytes)?)
}

fn record_elapsed(span: &Span, started_at: Instant) {
    let elapsed_us = started_at.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
    span.record("query_elapsed_us", elapsed_us);
}

#[derive(Clone)]
pub struct RocksDbKvStore {
    db: Arc<DB>,
    path: Arc<String>,
    write_lock: Arc<Mutex<()>>,
    write_options: Arc<WriteOptions>,
}

impl RocksDbKvStore {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut options = Options::default();
        options.create_if_missing(true);
        options.set_compression_type(compression_from_env()?);
        options.set_max_background_jobs(env_i32("TALON_ROCKSDB_MAX_BACKGROUND_JOBS", 2)?);
        options.set_enable_pipelined_write(env_bool("TALON_ROCKSDB_PIPELINED_WRITE", true)?);
        options.set_allow_concurrent_memtable_write(env_bool(
            "TALON_ROCKSDB_CONCURRENT_MEMTABLE_WRITE",
            true,
        )?);
        if let Some(size) = env_mib("TALON_ROCKSDB_WRITE_BUFFER_SIZE_MB")? {
            options.set_write_buffer_size(size);
        }
        if let Some(count) = env_i32_opt("TALON_ROCKSDB_MAX_WRITE_BUFFER_NUMBER")? {
            options.set_max_write_buffer_number(count);
        }
        if let Some(size) = env_mib("TALON_ROCKSDB_BLOCK_CACHE_SIZE_MB")? {
            let cache = Cache::new_lru_cache(size);
            let mut block_options = BlockBasedOptions::default();
            block_options.set_block_cache(&cache);
            options.set_block_based_table_factory(&block_options);
        }
        if let Some(bytes) = env_mib("TALON_ROCKSDB_BYTES_PER_SYNC_MB")? {
            options.set_bytes_per_sync(bytes as u64);
        }
        if let Some(bytes) = env_mib("TALON_ROCKSDB_WAL_BYTES_PER_SYNC_MB")? {
            options.set_wal_bytes_per_sync(bytes as u64);
        }
        let mut write_options = WriteOptions::default();
        write_options.disable_wal(env_bool("TALON_ROCKSDB_DISABLE_WAL", false)?);
        let db = DB::open(&options, path)?;
        Ok(Self {
            db: Arc::new(db),
            path: Arc::new(path.display().to_string()),
            write_lock: Arc::new(Mutex::new(())),
            write_options: Arc::new(write_options),
        })
    }

    async fn spawn_blocking<T, F>(&self, operation: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<DB>) -> Result<T> + Send + 'static,
    {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || f(db))
            .await
            .map_err(|err| anyhow!("RocksDbKvStore.{operation} task failed: {err}"))?
    }
}

fn env_bool(name: &str, default: bool) -> Result<bool> {
    let Some(value) = std::env::var(name).ok() else {
        return Ok(default);
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => bail!("{name} must be a boolean, got {value:?}"),
    }
}

fn env_i32(name: &str, default: i32) -> Result<i32> {
    Ok(env_i32_opt(name)?.unwrap_or(default))
}

fn env_i32_opt(name: &str) -> Result<Option<i32>> {
    let Some(value) = std::env::var(name).ok() else {
        return Ok(None);
    };
    let parsed = value
        .trim()
        .parse::<i32>()
        .map_err(|err| anyhow!("{name} must be an integer: {err}"))?;
    if parsed <= 0 {
        bail!("{name} must be greater than zero");
    }
    Ok(Some(parsed))
}

fn env_mib(name: &str) -> Result<Option<usize>> {
    let Some(value) = std::env::var(name).ok() else {
        return Ok(None);
    };
    let parsed = value
        .trim()
        .parse::<usize>()
        .map_err(|err| anyhow!("{name} must be an integer MiB value: {err}"))?;
    if parsed == 0 {
        bail!("{name} must be greater than zero");
    }
    parsed
        .checked_mul(1024 * 1024)
        .map(Some)
        .ok_or_else(|| anyhow!("{name} value is too large"))
}

fn compression_from_env() -> Result<DBCompressionType> {
    let value = std::env::var("TALON_ROCKSDB_COMPRESSION").unwrap_or_else(|_| "lz4".to_string());
    match value.trim().to_ascii_lowercase().as_str() {
        "none" | "off" | "no" => Ok(DBCompressionType::None),
        "lz4" => Ok(DBCompressionType::Lz4),
        _ => bail!("TALON_ROCKSDB_COMPRESSION must be 'lz4' or 'none'"),
    }
}

#[async_trait::async_trait]
impl KeyValueStore for RocksDbKvStore {
    async fn get(&self, key: &ResourceKey) -> Result<Option<Vec<u8>>> {
        let encoded = key_bytes(key);
        let span = tracing::debug_span!(
            "RocksDbKvStore.get",
            "db.system" = "rocksdb",
            "db.operation" = "get",
            "talon.kv.path" = %self.path.as_str(),
            "talon.resource.kind" = %key.kind,
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
            value_bytes = field::Empty,
        );
        let span_for_body = span.clone();
        async move {
            let started_at = Instant::now();
            let value = self
                .spawn_blocking("get", move |db| Ok(db.get(encoded)?))
                .await?;
            record_elapsed(&span_for_body, started_at);
            span_for_body.record("rows_returned", u64::from(value.is_some()));
            if let Some(value) = &value {
                span_for_body.record("value_bytes", value.len() as u64);
            }
            Ok(value)
        }
        .instrument(span)
        .await
    }

    async fn set(&self, key: &ResourceKey, value: &[u8]) -> Result<()> {
        let encoded = key_bytes(key);
        let value = value.to_vec();
        let span = tracing::debug_span!(
            "RocksDbKvStore.set",
            "db.system" = "rocksdb",
            "db.operation" = "set",
            "talon.kv.path" = %self.path.as_str(),
            "talon.resource.kind" = %key.kind,
            query_elapsed_us = field::Empty,
            rows_affected = field::Empty,
            value_bytes = value.len(),
        );
        let span_for_body = span.clone();
        let write_options = Arc::clone(&self.write_options);
        let write_lock = Arc::clone(&self.write_lock);
        async move {
            let started_at = Instant::now();
            let write_guard = write_lock.lock_owned().await;
            self.spawn_blocking("set", move |db| {
                let _write_guard = write_guard;
                db.put_opt(encoded, value, &write_options)?;
                Ok(())
            })
            .await?;
            record_elapsed(&span_for_body, started_at);
            span_for_body.record("rows_affected", 1_u64);
            Ok(())
        }
        .instrument(span)
        .await
    }

    async fn compare_and_swap(
        &self,
        key: &ResourceKey,
        expected: Option<&[u8]>,
        value: &[u8],
    ) -> Result<bool> {
        let encoded = key_bytes(key);
        let expected = expected.map(Vec::from);
        let value = value.to_vec();
        let span = tracing::debug_span!(
            "RocksDbKvStore.compare_and_swap",
            "db.system" = "rocksdb",
            "db.operation" = "compare_and_swap",
            "talon.kv.path" = %self.path.as_str(),
            "talon.resource.kind" = %key.kind,
            query_elapsed_us = field::Empty,
            rows_affected = field::Empty,
            expected_present = expected.is_some(),
            value_bytes = value.len(),
        );
        let span_for_body = span.clone();
        let write_options = Arc::clone(&self.write_options);
        let write_lock = Arc::clone(&self.write_lock);
        async move {
            let started_at = Instant::now();
            let write_guard = write_lock.lock_owned().await;
            let swapped = self
                .spawn_blocking("compare_and_swap", move |db| {
                    let _write_guard = write_guard;
                    let current = db.get(&encoded)?;
                    let matches = match (current.as_deref(), expected.as_deref()) {
                        (None, None) => true,
                        (Some(current), Some(expected)) => current == expected,
                        _ => false,
                    };
                    if matches {
                        db.put_opt(encoded, value, &write_options)?;
                    }
                    Ok(matches)
                })
                .await?;
            record_elapsed(&span_for_body, started_at);
            span_for_body.record("rows_affected", u64::from(swapped));
            Ok(swapped)
        }
        .instrument(span)
        .await
    }

    async fn delete(&self, key: &ResourceKey) -> Result<()> {
        let encoded = key_bytes(key);
        let span = tracing::debug_span!(
            "RocksDbKvStore.delete",
            "db.system" = "rocksdb",
            "db.operation" = "delete",
            "talon.kv.path" = %self.path.as_str(),
            "talon.resource.kind" = %key.kind,
            query_elapsed_us = field::Empty,
            rows_affected = field::Empty,
        );
        let span_for_body = span.clone();
        let write_options = Arc::clone(&self.write_options);
        let write_lock = Arc::clone(&self.write_lock);
        async move {
            let started_at = Instant::now();
            let write_guard = write_lock.lock_owned().await;
            self.spawn_blocking("delete", move |db| {
                let _write_guard = write_guard;
                db.delete_opt(encoded, &write_options)?;
                Ok(())
            })
            .await?;
            record_elapsed(&span_for_body, started_at);
            span_for_body.record("rows_affected", 1_u64);
            Ok(())
        }
        .instrument(span)
        .await
    }

    async fn compare_and_delete(&self, key: &ResourceKey, expected: &[u8]) -> Result<bool> {
        let encoded = key_bytes(key);
        let expected = expected.to_vec();
        let span = tracing::debug_span!(
            "RocksDbKvStore.compare_and_delete",
            "db.system" = "rocksdb",
            "db.operation" = "compare_and_delete",
            "talon.kv.path" = %self.path.as_str(),
            "talon.resource.kind" = %key.kind,
            query_elapsed_us = field::Empty,
            rows_affected = field::Empty,
            expected_bytes = expected.len(),
        );
        let span_for_body = span.clone();
        let write_options = Arc::clone(&self.write_options);
        let write_lock = Arc::clone(&self.write_lock);
        async move {
            let started_at = Instant::now();
            let write_guard = write_lock.lock_owned().await;
            let deleted = self
                .spawn_blocking("compare_and_delete", move |db| {
                    let _write_guard = write_guard;
                    let current = db.get(&encoded)?;
                    let matches = current.as_deref() == Some(expected.as_slice());
                    if matches {
                        db.delete_opt(encoded, &write_options)?;
                    }
                    Ok(matches)
                })
                .await?;
            record_elapsed(&span_for_body, started_at);
            span_for_body.record("rows_affected", u64::from(deleted));
            Ok(deleted)
        }
        .instrument(span)
        .await
    }

    async fn list_keys(&self, list: &ResourceList) -> Result<Vec<ResourceKey>> {
        let prefix = prefix_bytes(list);
        let span = tracing::debug_span!(
            "RocksDbKvStore.list_keys",
            "db.system" = "rocksdb",
            "db.operation" = "list_keys",
            "talon.kv.path" = %self.path.as_str(),
            "talon.resource.kind" = list.kind.as_deref().unwrap_or("*"),
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
        );
        let span_for_body = span.clone();
        async move {
            let started_at = Instant::now();
            let mut keys = self
                .spawn_blocking("list_keys", move |db| {
                    let mut keys = Vec::new();
                    for item in
                        db.iterator(IteratorMode::From(&prefix, rocksdb::Direction::Forward))
                    {
                        let (raw_key, _) = item?;
                        if !raw_key.starts_with(&prefix) {
                            break;
                        }
                        keys.push(parse_key(&raw_key)?);
                    }
                    Ok(keys)
                })
                .await?;
            record_elapsed(&span_for_body, started_at);
            span_for_body.record("rows_returned", keys.len() as u64);
            Ok(std::mem::take(&mut keys))
        }
        .instrument(span)
        .await
    }

    async fn list_entries(&self, list: &ResourceList) -> Result<Vec<(ResourceKey, Vec<u8>)>> {
        let prefix = prefix_bytes(list);
        let span = tracing::debug_span!(
            "RocksDbKvStore.list_entries",
            "db.system" = "rocksdb",
            "db.operation" = "list_entries",
            "talon.kv.path" = %self.path.as_str(),
            "talon.resource.kind" = list.kind.as_deref().unwrap_or("*"),
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
            value_bytes = field::Empty,
        );
        let span_for_body = span.clone();
        async move {
            let started_at = Instant::now();
            let entries = self
                .spawn_blocking("list_entries", move |db| {
                    let mut entries = Vec::new();
                    let mut value_bytes = 0usize;
                    for item in
                        db.iterator(IteratorMode::From(&prefix, rocksdb::Direction::Forward))
                    {
                        let (raw_key, value) = item?;
                        if !raw_key.starts_with(&prefix) {
                            break;
                        }
                        value_bytes += value.len();
                        entries.push((parse_key(&raw_key)?, value.to_vec()));
                    }
                    Ok((entries, value_bytes))
                })
                .await?;
            let (entries, value_bytes) = entries;
            record_elapsed(&span_for_body, started_at);
            span_for_body.record("rows_returned", entries.len() as u64);
            span_for_body.record("value_bytes", value_bytes as u64);
            Ok(entries)
        }
        .instrument(span)
        .await
    }

    async fn list_keys_page(
        &self,
        list: &ResourceList,
        before_name: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ResourceKey>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let Some(kind) = &list.kind else {
            bail!("paged resource listing requires an explicit resource kind");
        };
        let prefix = prefix_bytes(list);
        let cursor = before_name.map(|before_name| page_cursor_bytes(list, kind, before_name));
        let seek_key = page_seek_bytes(&prefix, cursor.as_deref());
        let span = tracing::debug_span!(
            "RocksDbKvStore.list_keys_page",
            "db.system" = "rocksdb",
            "db.operation" = "list_keys_page",
            "talon.kv.path" = %self.path.as_str(),
            "talon.resource.kind" = %kind,
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
            limit,
        );
        let span_for_body = span.clone();
        async move {
            let started_at = Instant::now();
            let keys = self
                .spawn_blocking("list_keys_page", move |db| {
                    let mut keys = Vec::with_capacity(limit);
                    for item in
                        db.iterator(IteratorMode::From(&seek_key, rocksdb::Direction::Reverse))
                    {
                        let (raw_key, _) = item?;
                        if !raw_key.starts_with(&prefix) {
                            break;
                        }
                        if let Some(cursor) = cursor.as_deref() {
                            if raw_key.as_ref() >= cursor {
                                continue;
                            }
                        }
                        keys.push(parse_key(&raw_key)?);
                        if keys.len() >= limit {
                            break;
                        }
                    }
                    Ok(keys)
                })
                .await?;
            record_elapsed(&span_for_body, started_at);
            span_for_body.record("rows_returned", keys.len() as u64);
            Ok(keys)
        }
        .instrument(span)
        .await
    }

    async fn list_entries_page(
        &self,
        list: &ResourceList,
        before_name: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(ResourceKey, Vec<u8>)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let Some(kind) = &list.kind else {
            bail!("paged resource listing requires an explicit resource kind");
        };
        let prefix = prefix_bytes(list);
        let cursor = before_name.map(|before_name| page_cursor_bytes(list, kind, before_name));
        let seek_key = page_seek_bytes(&prefix, cursor.as_deref());
        let span = tracing::debug_span!(
            "RocksDbKvStore.list_entries_page",
            "db.system" = "rocksdb",
            "db.operation" = "list_entries_page",
            "talon.kv.path" = %self.path.as_str(),
            "talon.resource.kind" = %kind,
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
            value_bytes = field::Empty,
            limit,
        );
        let span_for_body = span.clone();
        async move {
            let started_at = Instant::now();
            let (entries, value_bytes) = self
                .spawn_blocking("list_entries_page", move |db| {
                    let mut entries = Vec::with_capacity(limit);
                    let mut value_bytes = 0usize;
                    for item in
                        db.iterator(IteratorMode::From(&seek_key, rocksdb::Direction::Reverse))
                    {
                        let (raw_key, value) = item?;
                        if !raw_key.starts_with(&prefix) {
                            break;
                        }
                        if let Some(cursor) = cursor.as_deref() {
                            if raw_key.as_ref() >= cursor {
                                continue;
                            }
                        }
                        value_bytes += value.len();
                        entries.push((parse_key(&raw_key)?, value.to_vec()));
                        if entries.len() >= limit {
                            break;
                        }
                    }
                    Ok((entries, value_bytes))
                })
                .await?;
            record_elapsed(&span_for_body, started_at);
            span_for_body.record("rows_returned", entries.len() as u64);
            span_for_body.record("value_bytes", value_bytes as u64);
            Ok(entries)
        }
        .instrument(span)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::RocksDbKvStore;
    use crate::control::{keys, KeyValueStore};
    use tempfile::{tempdir, TempDir};

    async fn test_store() -> (TempDir, RocksDbKvStore) {
        let dir = tempdir().unwrap();
        let store = RocksDbKvStore::new(dir.path().join("kv.rocksdb")).unwrap();
        (dir, store)
    }

    #[tokio::test]
    async fn get_set_delete_round_trip() {
        let (_dir, store) = test_store().await;
        let key = keys::agent("default", "agent-a");
        assert!(store.get(&key).await.unwrap().is_none());
        store.set(&key, b"value-a").await.unwrap();
        assert_eq!(
            store.get(&key).await.unwrap().as_deref(),
            Some(&b"value-a"[..])
        );
        store.delete(&key).await.unwrap();
        assert!(store.get(&key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn compare_and_swap_handles_absent_and_present_values() {
        let (_dir, store) = test_store().await;
        let key = keys::agent("default", "agent-a");

        assert!(store.compare_and_swap(&key, None, b"v1").await.unwrap());
        assert!(!store.compare_and_swap(&key, None, b"v2").await.unwrap());
        assert!(!store
            .compare_and_swap(&key, Some(b"bad"), b"v2")
            .await
            .unwrap());
        assert!(store
            .compare_and_swap(&key, Some(b"v1"), b"v2")
            .await
            .unwrap());
        assert_eq!(store.get(&key).await.unwrap().as_deref(), Some(&b"v2"[..]));
    }

    #[tokio::test]
    async fn list_operations_match_resource_ordering_and_pages() {
        let (_dir, store) = test_store().await;
        let beta = keys::session("default", "agent-a", "beta");
        let alpha = keys::session("default", "agent-a", "alpha");
        let gamma = keys::session("default", "agent-a", "gamma");
        let other = keys::session("default", "agent-b", "other");
        store.set(&beta, b"b").await.unwrap();
        store.set(&alpha, b"a").await.unwrap();
        store.set(&gamma, b"g").await.unwrap();
        store.set(&other, b"o").await.unwrap();

        let list = keys::session_prefix("default", "agent-a");
        let keys = store.list_keys(&list).await.unwrap();
        assert_eq!(
            keys.iter().map(|key| key.name.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "beta", "gamma"]
        );

        let page = store.list_keys_page(&list, None, 2).await.unwrap();
        assert_eq!(
            page.iter().map(|key| key.name.as_str()).collect::<Vec<_>>(),
            vec!["gamma", "beta"]
        );
        let next = store
            .list_entries_page(&list, Some("beta"), 2)
            .await
            .unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].0.name, "alpha");
        assert_eq!(next[0].1, b"a");
    }
}
