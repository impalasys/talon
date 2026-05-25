// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub mod agents;
pub mod config;
pub mod connectors;
pub mod control;
pub mod core;
pub mod gateway;
pub mod knowledge;
pub mod llm;
pub mod manifest;
pub mod memory;
pub mod native_tools;
pub mod orchestrator;
pub mod scheduling;
pub mod security;
pub mod skills;
pub mod worker;
pub use crate::core::executor::{
    AgentExecutor, CaptureSink, ExecutionContext, ExecutionSink, NullSink,
};
pub use crate::core::rpc::{RpcMessage, RpcRequest, RpcResponse};
pub use crate::core::task::{EncryptedResult, Task, TaskResult, TaskStatus};
pub use crate::knowledge::{KnowledgeBook, KvKnowledgeBook};
pub use crate::security::encryption::SecurityProvider;

pub mod test_support {
    use crate::control::{KeyValueStore, MessagePublisher};
    use futures::stream;
    use std::collections::HashMap;
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

    #[derive(Default)]
    pub struct MockKvStore {
        data: AsyncMutex<HashMap<(String, String), Vec<u8>>>,
    }

    impl MockKvStore {
        pub fn new() -> Self {
            Self::default()
        }
    }

    #[async_trait::async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self
                .data
                .lock()
                .await
                .get(&(namespace.to_string(), key.to_string()))
                .cloned())
        }

        async fn set(&self, namespace: &str, key: &str, value: &[u8]) -> anyhow::Result<()> {
            self.data
                .lock()
                .await
                .insert((namespace.to_string(), key.to_string()), value.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            namespace: &str,
            key: &str,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let storage_key = (namespace.to_string(), key.to_string());
            let current = data.get(&storage_key).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(storage_key, value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
            self.data
                .lock()
                .await
                .remove(&(namespace.to_string(), key.to_string()));
            Ok(())
        }

        async fn list_keys(&self, namespace: &str, prefix: &str) -> anyhow::Result<Vec<String>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|(ns, key)| {
                    (ns == namespace && key.starts_with(prefix)).then(|| key.clone())
                })
                .collect::<Vec<_>>();
            keys.sort();
            Ok(keys)
        }

        async fn list_keys_page(
            &self,
            namespace: &str,
            prefix: &str,
            before_key: Option<&str>,
            limit: usize,
        ) -> anyhow::Result<Vec<String>> {
            Ok(crate::control::page_keys_desc(
                self.list_keys(namespace, prefix).await?,
                before_key,
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
