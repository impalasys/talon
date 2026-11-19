// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Shared fixtures and environment guards for Talon tests.

pub(crate) mod talon;

pub const TEST_RSA_PRIVATE_KEY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src/control/security/test_rsa_private_key.pem"
));

use futures::stream;
use std::collections::HashMap;
use std::ffi::OsString;
use std::pin::Pin;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use talon::contracts::auth as platform_jwt;
use talon::contracts::{KeyValueStore, ListOptions, MessagePublisher};
use talon::identifiers::keys::{ResourceKey, ResourceList};
use tokio::sync::Mutex as AsyncMutex;

pub fn async_env_mutex() -> &'static AsyncMutex<()> {
    static LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| AsyncMutex::new(()))
}

pub fn env_lock() -> tokio::sync::MutexGuard<'static, ()> {
    async_env_mutex().blocking_lock()
}

pub const TEST_PLATFORM_JWT_ISSUER: &str = "https://talon.example.com";

pub struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    pub fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }

    pub fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

pub struct PlatformJwtEnvGuard {
    previous_private_key: Option<OsString>,
    previous_issuer: Option<OsString>,
    _guard: tokio::sync::MutexGuard<'static, ()>,
}

impl PlatformJwtEnvGuard {
    pub async fn acquire() -> Self {
        let guard = async_env_mutex().lock().await;
        Self::from_guard(guard)
    }

    pub fn acquire_blocking() -> Self {
        let guard = env_lock();
        Self::from_guard(guard)
    }

    fn from_guard(guard: tokio::sync::MutexGuard<'static, ()>) -> Self {
        let previous_private_key = std::env::var_os(platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV);
        let previous_issuer = std::env::var_os(platform_jwt::TALON_JWT_ISSUER_ENV);
        unsafe {
            std::env::set_var(
                platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
                TEST_RSA_PRIVATE_KEY,
            );
            std::env::set_var(platform_jwt::TALON_JWT_ISSUER_ENV, TEST_PLATFORM_JWT_ISSUER);
        }
        Self {
            previous_private_key,
            previous_issuer,
            _guard: guard,
        }
    }
}

impl Drop for PlatformJwtEnvGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(previous_private_key) = &self.previous_private_key {
                std::env::set_var(
                    platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
                    previous_private_key,
                );
            } else {
                std::env::remove_var(platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV);
            }
            if let Some(previous_issuer) = &self.previous_issuer {
                std::env::set_var(platform_jwt::TALON_JWT_ISSUER_ENV, previous_issuer);
            } else {
                std::env::remove_var(platform_jwt::TALON_JWT_ISSUER_ENV);
            }
        }
    }
}

#[derive(Default)]
pub struct MockKvStore {
    data: AsyncMutex<HashMap<ResourceKey, Vec<u8>>>,
}

impl MockKvStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl KeyValueStore for MockKvStore {
    async fn get(&self, key: &ResourceKey) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.data.lock().await.get(key).cloned())
    }

    async fn set(&self, key: &ResourceKey, value: &[u8]) -> anyhow::Result<()> {
        self.data.lock().await.insert(key.clone(), value.to_vec());
        Ok(())
    }

    async fn compare_and_swap(
        &self,
        key: &ResourceKey,
        expected: Option<&[u8]>,
        value: &[u8],
    ) -> anyhow::Result<bool> {
        let mut data = self.data.lock().await;
        let current = data.get(key).cloned();
        let matches = match (current.as_deref(), expected) {
            (None, None) => true,
            (Some(current), Some(expected)) => current == expected,
            _ => false,
        };
        if matches {
            data.insert(key.clone(), value.to_vec());
        }
        Ok(matches)
    }

    async fn compare_and_swap_batch(
        &self,
        operations: &[talon::contracts::KeyValueBatchOperation],
    ) -> anyhow::Result<bool> {
        talon::contracts::validate_key_value_batch(operations)?;
        let mut data = self.data.lock().await;
        if operations.iter().any(|operation| {
            data.get(&operation.key).map(Vec::as_slice) != operation.expected.as_deref()
        }) {
            return Ok(false);
        }
        for operation in operations {
            match &operation.value {
                Some(value) => {
                    data.insert(operation.key.clone(), value.clone());
                }
                None => {
                    data.remove(&operation.key);
                }
            }
        }
        Ok(true)
    }

    async fn compare_and_delete(&self, key: &ResourceKey, expected: &[u8]) -> anyhow::Result<bool> {
        let mut data = self.data.lock().await;
        if data.get(key).map(Vec::as_slice) != Some(expected) {
            return Ok(false);
        }
        data.remove(key);
        Ok(true)
    }

    async fn delete(&self, key: &ResourceKey) -> anyhow::Result<()> {
        self.data.lock().await.remove(key);
        Ok(())
    }

    async fn list_keys(
        &self,
        list: &ResourceList,
        options: Option<ListOptions<'_>>,
    ) -> anyhow::Result<Vec<ResourceKey>> {
        let keys = self
            .data
            .lock()
            .await
            .keys()
            .filter_map(|key| list.matches(key).then(|| key.clone()))
            .collect::<Vec<_>>();
        Ok(talon::contracts::apply_list_options_to_keys(
            keys,
            options.unwrap_or_default(),
        ))
    }
}

#[derive(Default)]
pub struct EmptyPubSub;

#[async_trait::async_trait]
impl MessagePublisher for EmptyPubSub {
    async fn publish(&self, _topic: &str, _message: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }

    async fn subscribe(
        &self,
        _topic: &str,
    ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
        Ok(Box::pin(futures::stream::empty()))
    }
}

#[derive(Default)]
pub struct RecordingPubSub {
    pub streams: AsyncMutex<HashMap<String, Vec<Vec<u8>>>>,
    pub published: AsyncMutex<Vec<(String, Vec<u8>)>>,
}

#[async_trait::async_trait]
impl MessagePublisher for RecordingPubSub {
    async fn publish(&self, topic: &str, message: &[u8]) -> anyhow::Result<()> {
        self.published
            .lock()
            .await
            .push((topic.to_string(), message.to_vec()));
        Ok(())
    }

    async fn subscribe(
        &self,
        topic: &str,
    ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
        let data = self
            .streams
            .lock()
            .await
            .get(topic)
            .cloned()
            .unwrap_or_default();
        Ok(Box::pin(stream::iter(data)))
    }
}

fn docker_test_mutex() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub fn docker_test_guard() -> std::sync::MutexGuard<'static, ()> {
    docker_test_mutex()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct PostgresContainer {
    name: String,
    port: u16,
}

impl PostgresContainer {
    pub fn start(prefix: &str) -> Self {
        let name = talon::contracts::uuid::unique_name(prefix);
        let run = Command::new("docker")
            .args([
                "run",
                "-d",
                "--rm",
                "--name",
                &name,
                "-e",
                "POSTGRES_USER=talon",
                "-e",
                "POSTGRES_PASSWORD=password",
                "-e",
                "POSTGRES_DB=talon",
                "-p",
                "127.0.0.1::5432",
                "postgres:15-alpine",
            ])
            .output()
            .expect("docker run should succeed");
        assert!(
            run.status.success(),
            "docker run failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );

        let inspect = Command::new("docker")
            .args([
                "inspect",
                "-f",
                "{{(index (index .NetworkSettings.Ports \"5432/tcp\") 0).HostPort}}",
                &name,
            ])
            .output()
            .expect("docker inspect should succeed");
        assert!(
            inspect.status.success(),
            "docker inspect failed: {}",
            String::from_utf8_lossy(&inspect.stderr)
        );
        let port = String::from_utf8_lossy(&inspect.stdout)
            .trim()
            .parse::<u16>()
            .expect("host port should parse");

        let max_attempts = std::env::var("TALON_TEST_PG_READY_ATTEMPTS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|attempts| *attempts > 0)
            .unwrap_or(60);

        for _ in 0..max_attempts {
            let ready = Command::new("docker")
                .args(["exec", &name, "pg_isready", "-U", "talon", "-d", "talon"])
                .output()
                .expect("docker exec should succeed");
            if ready.status.success() {
                return Self { name, port };
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        panic!("postgres container did not become ready");
    }

    pub fn database_url(&self) -> String {
        format!("postgres://talon:password@127.0.0.1:{}/talon", self.port)
    }
}

impl Drop for PostgresContainer {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.name])
            .output();
    }
}

pub mod workflows;

/// Shared storage conformance: conditions and mutations are one transaction.
pub async fn assert_atomic_batch_conformance(store: &dyn KeyValueStore) {
    use talon::contracts::KeyValueBatchOperation as Op;
    let a = ResourceKey::new("batch-conformance", &[], "Test", "a");
    let b = ResourceKey::new("batch-conformance", &[], "Test", "b");
    let c = ResourceKey::new("batch-conformance", &[], "Test", "c");
    store.set(&a, b"a0").await.unwrap();
    store.set(&b, b"b0").await.unwrap();
    assert!(!store
        .compare_and_swap_batch(&[
            Op::new(a.clone(), Some(b"a0"), Some(b"a1")),
            Op::new(b.clone(), Some(b"wrong"), None),
            Op::new(c.clone(), None, Some(b"new")),
        ])
        .await
        .unwrap());
    assert_eq!(store.get(&a).await.unwrap().as_deref(), Some(&b"a0"[..]));
    assert_eq!(store.get(&b).await.unwrap().as_deref(), Some(&b"b0"[..]));
    assert!(store.get(&c).await.unwrap().is_none());
    assert!(store
        .compare_and_swap_batch(&[
            Op::new(a.clone(), Some(b"a0"), Some(b"a1")),
            Op::new(b.clone(), Some(b"b0"), None),
            Op::new(c.clone(), None, None),
        ])
        .await
        .unwrap());
    assert!(store.get(&b).await.unwrap().is_none());
    assert!(store.get(&c).await.unwrap().is_none());
    assert!(store
        .compare_and_swap_batch(&[
            Op::new(a.clone(), Some(b"a1"), Some(b"a1")),
            Op::new(c.clone(), None, Some(b"c1")),
        ])
        .await
        .unwrap());
    assert!(!store
        .compare_and_swap_batch(&[
            Op::new(a.clone(), Some(b"a1"), Some(b"corrupt")),
            Op::new(c.clone(), None, None),
        ])
        .await
        .unwrap());
    assert!(store
        .compare_and_swap_batch(&[
            Op::new(a.clone(), Some(b"a1"), None),
            Op::new(a.clone(), Some(b"a1"), Some(b"duplicate")),
        ])
        .await
        .is_err());
    let left = [
        Op::new(a.clone(), Some(b"a1"), Some(b"left")),
        Op::new(c.clone(), Some(b"c1"), Some(b"left")),
    ];
    let right = [
        Op::new(a.clone(), Some(b"a1"), Some(b"right")),
        Op::new(c.clone(), Some(b"c1"), Some(b"right")),
    ];
    let (left, right) = tokio::join!(
        store.compare_and_swap_batch(&left),
        store.compare_and_swap_batch(&right)
    );
    assert_ne!(left.unwrap(), right.unwrap());
    assert_eq!(store.get(&a).await.unwrap(), store.get(&c).await.unwrap());
    // A single-key writer must participate in the same serialization order.
    let current = store.get(&a).await.unwrap().unwrap();
    let batch = [
        Op::new(a.clone(), Some(&current), Some(b"batch")),
        Op::new(c.clone(), Some(&current), Some(b"batch")),
    ];
    let (batch, single) = tokio::join!(
        store.compare_and_swap_batch(&batch),
        store.compare_and_swap(&a, Some(&current), b"single")
    );
    assert_ne!(batch.unwrap(), single.unwrap());
    let a_value = store.get(&a).await.unwrap().unwrap();
    let c_value = store.get(&c).await.unwrap().unwrap();
    if a_value == b"batch" {
        assert_eq!(c_value, b"batch");
    } else {
        assert_eq!(a_value, b"single");
        assert_eq!(c_value, current);
    }
}

/// Run asynchronous storage conformance fixtures without adding a runtime to each backend's production dependencies.
pub fn run_async_test<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}
