// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub mod cli;
pub mod control;
pub mod gateway;
pub mod harness;
pub mod worker;
pub use crate::control::security::encryption::SecurityProvider;
pub use crate::harness::executor::{
    AgentExecutor, CaptureSink, EncryptedResult, ExecutionContext, ExecutionSink, NullSink,
    RpcMessage, RpcRequest, RpcResponse, Task, TaskResult, TaskStatus,
};
pub use crate::harness::knowledge::{KnowledgeBook, KvKnowledgeBook};

pub mod test_support {
    use crate::control::keys::{ResourceKey, ResourceList};
    #[cfg(test)]
    use crate::control::security::platform_jwt;
    use crate::control::{KeyValueStore, MessagePublisher};
    use futures::stream;
    use std::collections::HashMap;
    #[cfg(test)]
    use std::ffi::OsString;
    use std::pin::Pin;
    use std::process::Command;
    use std::sync::{Mutex, OnceLock};
    use tokio::sync::Mutex as AsyncMutex;

    pub fn async_env_mutex() -> &'static AsyncMutex<()> {
        static LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| AsyncMutex::new(()))
    }

    pub fn env_lock() -> tokio::sync::MutexGuard<'static, ()> {
        async_env_mutex().blocking_lock()
    }

    #[cfg(test)]
    pub const TEST_PLATFORM_JWT_ISSUER: &str = "https://talon.example.com";

    #[cfg(test)]
    pub struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    #[cfg(test)]
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

    #[cfg(test)]
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

    #[cfg(test)]
    pub struct PlatformJwtEnvGuard {
        previous_private_key: Option<OsString>,
        previous_issuer: Option<OsString>,
        _guard: tokio::sync::MutexGuard<'static, ()>,
    }

    #[cfg(test)]
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
            let previous_private_key =
                std::env::var_os(platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV);
            let previous_issuer = std::env::var_os(platform_jwt::TALON_JWT_ISSUER_ENV);
            unsafe {
                std::env::set_var(
                    platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
                    platform_jwt::TEST_RSA_PRIVATE_KEY,
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

    #[cfg(test)]
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

        async fn delete(&self, key: &ResourceKey) -> anyhow::Result<()> {
            self.data.lock().await.remove(key);
            Ok(())
        }

        async fn list_keys(&self, list: &ResourceList) -> anyhow::Result<Vec<ResourceKey>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|key| list.matches(key).then(|| key.clone()))
                .collect::<Vec<_>>();
            keys.sort();
            Ok(keys)
        }

        async fn list_keys_page(
            &self,
            list: &ResourceList,
            before_name: Option<&str>,
            limit: usize,
        ) -> anyhow::Result<Vec<ResourceKey>> {
            Ok(crate::control::page_keys_desc(
                self.list_keys(list).await?,
                before_name,
                limit,
            ))
        }

        async fn list_entries_page(
            &self,
            list: &ResourceList,
            before_name: Option<&str>,
            limit: usize,
        ) -> anyhow::Result<Vec<(ResourceKey, Vec<u8>)>> {
            Ok(crate::control::page_entries_desc(
                self.list_entries(list).await?,
                before_name,
                limit,
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
            let name = format!("{}-{}", prefix, uuid::Uuid::now_v7());
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
}
