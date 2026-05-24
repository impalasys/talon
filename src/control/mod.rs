// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::pin::Pin;
pub mod events;
pub mod keys;
pub mod kv;
pub mod ns;
pub mod pubsub;
pub mod scheduler;
pub mod topics;

use std::path::PathBuf;

#[async_trait::async_trait]
pub trait KeyValueStore: Send + Sync {
    /// Retrieve a raw byte sequence from the store
    async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<Vec<u8>>>;

    /// Store a raw byte sequence into the store
    async fn set(&self, namespace: &str, key: &str, value: &[u8]) -> anyhow::Result<()>;

    /// Atomically replace the current value when it matches the expected value.
    async fn compare_and_swap(
        &self,
        namespace: &str,
        key: &str,
        expected: Option<&[u8]>,
        value: &[u8],
    ) -> anyhow::Result<bool>;

    /// Delete a key from the namespace
    async fn delete(&self, namespace: &str, key: &str) -> anyhow::Result<()>;

    /// List all keys in a namespace with a given prefix
    async fn list_keys(&self, namespace: &str, prefix: &str) -> anyhow::Result<Vec<String>>;

    /// List all key/value pairs in a namespace with a given prefix.
    async fn list_entries(
        &self,
        namespace: &str,
        prefix: &str,
    ) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
        let keys = self.list_keys(namespace, prefix).await?;
        let mut entries = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(value) = self.get(namespace, &key).await? {
                entries.push((key, value));
            }
        }
        Ok(entries)
    }

    /// Delete all keys in a namespace with a given prefix.
    async fn delete_prefix(&self, namespace: &str, prefix: &str) -> anyhow::Result<()> {
        let keys = self.list_keys(namespace, prefix).await?;
        for key in keys {
            self.delete(namespace, &key).await?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
pub trait ProtoKeyValueStoreExt {
    async fn get_msg<M: prost::Message + Default>(
        &self,
        namespace: &str,
        key: &str,
    ) -> anyhow::Result<Option<M>>;
    async fn set_msg<M: prost::Message + Sync>(
        &self,
        namespace: &str,
        key: &str,
        msg: &M,
    ) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
impl<T: KeyValueStore + ?Sized> ProtoKeyValueStoreExt for T {
    async fn get_msg<M: prost::Message + Default>(
        &self,
        namespace: &str,
        key: &str,
    ) -> anyhow::Result<Option<M>> {
        match self.get(namespace, key).await? {
            Some(bytes) => Ok(Some(M::decode(bytes.as_slice())?)),
            None => Ok(None),
        }
    }

    async fn set_msg<M: prost::Message + Sync>(
        &self,
        namespace: &str,
        key: &str,
        msg: &M,
    ) -> anyhow::Result<()> {
        self.set(namespace, key, &msg.encode_to_vec()).await
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

#[derive(Clone)]
pub struct ControlPlane {
    pub kv: std::sync::Arc<dyn KeyValueStore + Send + Sync>,
    pub pubsub: std::sync::Arc<dyn MessagePublisher + Send + Sync>,
    pub scheduler: std::sync::Arc<dyn scheduler::SchedulerBackend + Send + Sync>,
}

fn message_broker_config(
    cp: &crate::config::proto::ControlPlaneConfig,
) -> anyhow::Result<&crate::config::proto::MessageBrokerConfig> {
    let mb_config = cp
        .message_broker
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("control_plane.message_broker configuration is missing"))?;
    if mb_config.driver != "gcp_pubsub" && mb_config.driver != "local_socket" {
        return Err(anyhow::anyhow!(
            "Unsupported message broker driver: {}",
            mb_config.driver
        ));
    }
    Ok(mb_config)
}

pub async fn build_control_plane(config: &crate::config::Config) -> anyhow::Result<ControlPlane> {
    use crate::config::SecretExt;

    let cp = config
        .control_plane
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("control_plane configuration is missing"))?;

    let db_config = cp
        .database
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("control_plane.database configuration is missing"))?;
    let kv: std::sync::Arc<dyn KeyValueStore + Send + Sync>;
    let scheduler_database_url: Option<String>;
    match db_config.driver.as_str() {
        "postgres" => {
            let url_secret = db_config
                .url
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Database URL secret is missing"))?;
            let pg_url: String = url_secret.resolve().await?;
            println!("Connecting to PostgresKvStore at {}...", pg_url);
            kv = std::sync::Arc::new(kv::PostgresKvStore::new(&pg_url, "talon_kv_store").await?);
            scheduler_database_url = Some(pg_url);
        }
        "sqlite" => {
            let sqlite_url = sqlite_database_url(db_config).await?;
            println!("Connecting to SqliteKvStore at {}...", sqlite_url);
            kv = std::sync::Arc::new(kv::SqliteKvStore::new(&sqlite_url, "talon_kv_store").await?);
            scheduler_database_url = Some(sqlite_url);
        }
        other => {
            return Err(anyhow::anyhow!("Unsupported database driver: {}", other));
        }
    }

    let mb_config = message_broker_config(cp)?;
    let pubsub: std::sync::Arc<dyn MessagePublisher + Send + Sync> = match mb_config.driver.as_str()
    {
        "gcp_pubsub" => {
            println!("Initializing GcpPubSubPublisher...");
            std::sync::Arc::new(pubsub::GcpPubSubPublisher::new().await?)
        }
        "local_socket" => {
            let default_root =
                if db_config.driver == "sqlite" && !db_config.data_dir.trim().is_empty() {
                    Some(PathBuf::from(db_config.data_dir.trim()))
                } else {
                    None
                };
            let socket_path = pubsub::configured_local_socket_path(default_root.as_deref());
            println!(
                "Initializing LocalSocketMessagePublisher at {}...",
                socket_path.display()
            );
            std::sync::Arc::new(pubsub::LocalSocketMessagePublisher::new(socket_path).await?)
        }
        other => {
            return Err(anyhow::anyhow!(
                "Unsupported message broker driver: {}",
                other
            ));
        }
    };

    let scheduler: std::sync::Arc<dyn scheduler::SchedulerBackend + Send + Sync> =
        match scheduler_driver().as_deref() {
            Some("local_postgres") => {
                if let Some(database_url) = scheduler_database_url.as_deref() {
                    match scheduler::LocalPostgresSchedulerBackend::new(
                        database_url,
                        std::env::var("TALON_LOCAL_SCHEDULER_TABLE").ok(),
                        std::env::var("TALON_LOCAL_SCHEDULER_TARGET_URL").ok(),
                        std::env::var("TALON_SCHEDULER_AUTH_TOKEN").ok(),
                        std::env::var("TALON_LOCAL_SCHEDULER_RUNNER")
                            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
                            .unwrap_or(false),
                    )
                    .await
                    {
                        Ok(backend) => std::sync::Arc::new(backend),
                        Err(err) => {
                            tracing::warn!(error = %err, "Failed to initialize local_postgres scheduler; using noop");
                            std::sync::Arc::new(scheduler::NoopSchedulerBackend::default())
                        }
                    }
                } else {
                    std::sync::Arc::new(scheduler::NoopSchedulerBackend::default())
                }
            }
            Some("local_sqlite") => {
                if let Some(database_url) = scheduler_database_url.as_deref() {
                    match scheduler::LocalSqliteSchedulerBackend::new(
                        database_url,
                        std::env::var("TALON_LOCAL_SCHEDULER_TABLE").ok(),
                        std::env::var("TALON_LOCAL_SCHEDULER_TARGET_URL").ok(),
                        std::env::var("TALON_SCHEDULER_AUTH_TOKEN").ok(),
                        std::env::var("TALON_LOCAL_SCHEDULER_RUNNER")
                            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
                            .unwrap_or(false),
                    )
                    .await
                    {
                        Ok(backend) => std::sync::Arc::new(backend),
                        Err(err) => {
                            tracing::warn!(error = %err, "Failed to initialize local_sqlite scheduler; using noop");
                            std::sync::Arc::new(scheduler::NoopSchedulerBackend::default())
                        }
                    }
                } else {
                    std::sync::Arc::new(scheduler::NoopSchedulerBackend::default())
                }
            }
            _ => match configured_scheduler(cp.scheduler.as_ref()) {
                Some(crate::config::proto::SchedulerConfig {
                    backend: Some(crate::config::proto::scheduler_config::Backend::CloudTasks(cfg)),
                }) => match scheduler::CloudTasksSchedulerBackend::new(&cfg).await {
                    Ok(backend) => std::sync::Arc::new(backend),
                    Err(err) => {
                        tracing::warn!(error = %err, "Failed to initialize Cloud Tasks scheduler; using noop");
                        std::sync::Arc::new(scheduler::NoopSchedulerBackend::default())
                    }
                },
                Some(crate::config::proto::SchedulerConfig { backend: None }) => {
                    std::sync::Arc::new(scheduler::NoopSchedulerBackend::default())
                }
                None => std::sync::Arc::new(scheduler::NoopSchedulerBackend::default()),
            },
        };

    Ok(ControlPlane {
        kv,
        pubsub,
        scheduler,
    })
}

fn configured_scheduler(
    cfg: Option<&crate::config::proto::SchedulerConfig>,
) -> Option<crate::config::proto::SchedulerConfig> {
    if let Some(cfg) = cfg.filter(|cfg| cfg.backend.is_some()) {
        return Some(cfg.clone());
    }

    let driver = scheduler_driver()?;
    let driver = driver.trim().to_string();
    if driver.is_empty() {
        return None;
    }

    match driver.as_str() {
        "cloud_tasks" => Some(crate::config::proto::SchedulerConfig {
            backend: Some(crate::config::proto::scheduler_config::Backend::CloudTasks(
                crate::config::proto::CloudTasksSchedulerConfig {
                    project_id: std::env::var("TALON_SCHEDULER_PROJECT_ID").unwrap_or_default(),
                    location: std::env::var("TALON_SCHEDULER_LOCATION").unwrap_or_default(),
                    queue: std::env::var("TALON_SCHEDULER_QUEUE").unwrap_or_default(),
                    target_url: std::env::var("TALON_SCHEDULER_TARGET_URL").unwrap_or_default(),
                    callback_auth: configured_scheduler_callback_auth_from_env(),
                },
            )),
        }),
        other => {
            tracing::warn!(driver = %other, "Unsupported scheduler backend configured; using noop");
            None
        }
    }
}

fn scheduler_driver() -> Option<String> {
    std::env::var("TALON_SCHEDULER_DRIVER").ok()
}

async fn sqlite_database_url(
    db_config: &crate::config::proto::DatabaseConfig,
) -> anyhow::Result<String> {
    use crate::config::SecretExt;

    if let Some(url_secret) = db_config.url.as_ref() {
        return Ok(url_secret.resolve().await?);
    }
    let data_dir = db_config.data_dir.trim();
    if data_dir.is_empty() {
        return Err(anyhow::anyhow!(
            "SQLite database requires either control_plane.database.url or control_plane.database.data_dir"
        ));
    }
    let db_path = PathBuf::from(data_dir).join("talon-control-plane.db");
    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(kv::sqlite_url_for_path(&db_path))
}

fn configured_scheduler_callback_auth_from_env(
) -> Option<crate::config::proto::SchedulerCallbackAuthConfig> {
    if let Some(token) = std::env::var("TALON_SCHEDULER_AUTH_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Some(crate::config::proto::SchedulerCallbackAuthConfig {
            auth: Some(
                crate::config::proto::scheduler_callback_auth_config::Auth::SharedSecret(
                    crate::config::Secret {
                        source: Some(crate::config::proto::secret::Source::Plain(token)),
                    },
                ),
            ),
        });
    }

    std::env::var("TALON_SCHEDULER_AUDIENCE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(
            |audience| crate::config::proto::SchedulerCallbackAuthConfig {
                auth: Some(
                    crate::config::proto::scheduler_callback_auth_config::Auth::GoogleOidc(
                        crate::config::proto::GoogleOidcAuthConfig {
                            audience,
                            service_account_email: std::env::var(
                                "TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL",
                            )
                            .unwrap_or_default(),
                        },
                    ),
                ),
            },
        )
}

#[cfg(test)]
mod tests {
    use super::{
        build_control_plane, configured_scheduler, configured_scheduler_callback_auth_from_env,
        message_broker_config, sqlite_database_url, KeyValueStore, ProtoKeyValueStoreExt,
    };
    use crate::config::proto;
    use crate::config::proto::{scheduler_callback_auth_config, scheduler_config, secret};
    use crate::gateway::rpc::models;
    use crate::test_support::MockKvStore;
    use std::collections::HashMap;
    use tempfile::tempdir;
    struct EnvGuard {
        key: &'static str,
        value: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self {
                key,
                value: previous,
            }
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::remove_var(key);
            Self {
                key,
                value: previous,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.value {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn scheduler_callback_auth_prefers_shared_secret_over_oidc() {
        let _lock = crate::test_support::env_lock();
        let _driver = EnvGuard::remove("TALON_SCHEDULER_DRIVER");
        let _token = EnvGuard::set("TALON_SCHEDULER_AUTH_TOKEN", "secret-token");
        let _aud = EnvGuard::set("TALON_SCHEDULER_AUDIENCE", "https://example.com");

        let auth = configured_scheduler_callback_auth_from_env().expect("expected auth config");
        match auth.auth.expect("expected auth variant") {
            scheduler_callback_auth_config::Auth::SharedSecret(secret) => {
                assert_eq!(
                    secret.source,
                    Some(secret::Source::Plain("secret-token".to_string()))
                );
            }
            other => panic!("expected shared secret auth, got {other:?}"),
        }
    }

    #[test]
    fn configured_scheduler_reads_cloud_tasks_from_env() {
        let _lock = crate::test_support::env_lock();
        let _driver = EnvGuard::set("TALON_SCHEDULER_DRIVER", "cloud_tasks");
        let _project = EnvGuard::set("TALON_SCHEDULER_PROJECT_ID", "talon-project");
        let _location = EnvGuard::set("TALON_SCHEDULER_LOCATION", "us-central1");
        let _queue = EnvGuard::set("TALON_SCHEDULER_QUEUE", "talon-queue");
        let _target = EnvGuard::set("TALON_SCHEDULER_TARGET_URL", "https://worker.example/fire");
        let _aud = EnvGuard::set("TALON_SCHEDULER_AUDIENCE", "https://worker.example/fire");
        let _email = EnvGuard::set(
            "TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL",
            "scheduler@example.com",
        );
        let _token = EnvGuard::remove("TALON_SCHEDULER_AUTH_TOKEN");

        let scheduler = configured_scheduler(None).expect("expected scheduler config");
        match scheduler.backend.expect("expected backend") {
            scheduler_config::Backend::CloudTasks(cfg) => {
                assert_eq!(cfg.project_id, "talon-project");
                assert_eq!(cfg.location, "us-central1");
                assert_eq!(cfg.queue, "talon-queue");
                assert_eq!(cfg.target_url, "https://worker.example/fire");
                match cfg.callback_auth.and_then(|auth| auth.auth) {
                    Some(scheduler_callback_auth_config::Auth::GoogleOidc(oidc)) => {
                        assert_eq!(oidc.audience, "https://worker.example/fire");
                        assert_eq!(oidc.service_account_email, "scheduler@example.com");
                    }
                    other => panic!("expected google oidc auth, got {other:?}"),
                }
            }
        }
    }

    #[test]
    fn configured_scheduler_prefers_explicit_config() {
        let _lock = crate::test_support::env_lock();
        let _driver = EnvGuard::set("TALON_SCHEDULER_DRIVER", "cloud_tasks");
        let explicit = proto::SchedulerConfig {
            backend: Some(scheduler_config::Backend::CloudTasks(
                proto::CloudTasksSchedulerConfig {
                    project_id: "configured-project".to_string(),
                    location: "configured-location".to_string(),
                    queue: "configured-queue".to_string(),
                    target_url: "https://configured.example/fire".to_string(),
                    callback_auth: None,
                },
            )),
        };

        let scheduler = configured_scheduler(Some(&explicit)).expect("expected scheduler config");
        match scheduler.backend.expect("expected backend") {
            scheduler_config::Backend::CloudTasks(cfg) => {
                assert_eq!(cfg.project_id, "configured-project");
                assert_eq!(cfg.location, "configured-location");
                assert_eq!(cfg.queue, "configured-queue");
                assert_eq!(cfg.target_url, "https://configured.example/fire");
            }
        }
    }

    #[test]
    fn configured_scheduler_rejects_unknown_driver() {
        let _lock = crate::test_support::env_lock();
        let _driver = EnvGuard::set("TALON_SCHEDULER_DRIVER", "unknown");
        assert!(configured_scheduler(None).is_none());
    }

    #[test]
    fn configured_scheduler_returns_none_for_missing_or_blank_driver() {
        let _lock = crate::test_support::env_lock();
        let _driver = EnvGuard::remove("TALON_SCHEDULER_DRIVER");
        assert!(configured_scheduler(None).is_none());

        let _blank = EnvGuard::set("TALON_SCHEDULER_DRIVER", "   ");
        assert!(configured_scheduler(None).is_none());
    }

    #[test]
    fn configured_scheduler_callback_auth_returns_none_for_blank_inputs_and_empty_email_default() {
        let _lock = crate::test_support::env_lock();
        let _token = EnvGuard::set("TALON_SCHEDULER_AUTH_TOKEN", "   ");
        let _aud = EnvGuard::remove("TALON_SCHEDULER_AUDIENCE");
        assert!(configured_scheduler_callback_auth_from_env().is_none());

        let _aud = EnvGuard::set("TALON_SCHEDULER_AUDIENCE", "https://worker.example/fire");
        let _email = EnvGuard::remove("TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL");
        let auth = configured_scheduler_callback_auth_from_env().expect("expected oidc auth");
        match auth.auth.expect("expected auth variant") {
            scheduler_callback_auth_config::Auth::GoogleOidc(oidc) => {
                assert_eq!(oidc.audience, "https://worker.example/fire");
                assert!(oidc.service_account_email.is_empty());
            }
            other => panic!("expected google oidc auth, got {other:?}"),
        }
    }

    #[test]
    fn env_guard_restores_previous_values_on_drop() {
        let _lock = crate::test_support::env_lock();
        std::env::set_var("TALON_TEST_RESTORE", "before");
        {
            let _guard = EnvGuard::set("TALON_TEST_RESTORE", "after");
            assert_eq!(std::env::var("TALON_TEST_RESTORE").as_deref(), Ok("after"));
        }
        assert_eq!(std::env::var("TALON_TEST_RESTORE").as_deref(), Ok("before"));
        std::env::remove_var("TALON_TEST_RESTORE");
    }

    #[tokio::test]
    async fn key_value_store_default_helpers_and_proto_round_trip_work() {
        let kv = MockKvStore::default();
        kv.set("ns", "prefix/a", b"one").await.unwrap();
        kv.set("ns", "prefix/b", b"two").await.unwrap();
        kv.set("ns", "other/c", b"three").await.unwrap();

        let mut entries = kv.list_entries("ns", "prefix/").await.unwrap();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], ("prefix/a".to_string(), b"one".to_vec()));
        assert_eq!(entries[1], ("prefix/b".to_string(), b"two".to_vec()));

        kv.delete_prefix("ns", "prefix/").await.unwrap();
        assert!(kv.get("ns", "prefix/a").await.unwrap().is_none());
        assert!(kv.get("ns", "prefix/b").await.unwrap().is_none());
        assert_eq!(
            kv.get("ns", "other/c").await.unwrap(),
            Some(b"three".to_vec())
        );

        let session = models::Session {
            id: "session-1".to_string(),
            agent: "agent".to_string(),
            ns: "ns".to_string(),
            status: "IDLE".to_string(),
            created_at: 1,
            last_active: 2,
            metadata: HashMap::new(),
            labels: HashMap::from([("env".to_string(), "test".to_string())]),
        };
        kv.set_msg("ns", "session/key", &session).await.unwrap();
        let loaded = kv
            .get_msg::<models::Session>("ns", "session/key")
            .await
            .unwrap()
            .expect("session should decode");
        assert_eq!(loaded.id, "session-1");
        assert_eq!(loaded.labels.get("env").map(String::as_str), Some("test"));
        assert!(kv
            .get_msg::<models::Session>("ns", "missing")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn build_control_plane_requires_control_plane_config() {
        let err = match build_control_plane(&proto::TalonConfig::default()).await {
            Ok(_) => panic!("expected missing control_plane error"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("control_plane configuration is missing"));
    }

    #[tokio::test]
    async fn build_control_plane_requires_database_config() {
        let config = proto::TalonConfig {
            control_plane: Some(proto::ControlPlaneConfig {
                database: None,
                message_broker: Some(proto::MessageBrokerConfig {
                    driver: "gcp_pubsub".to_string(),
                }),
                scheduler: None,
            }),
            ..Default::default()
        };

        let err = match build_control_plane(&config).await {
            Ok(_) => panic!("expected missing database error"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("control_plane.database configuration is missing"));
    }

    #[tokio::test]
    async fn build_control_plane_requires_message_broker_config() {
        let cp = proto::ControlPlaneConfig {
            database: None,
            message_broker: None,
            scheduler: None,
        };
        let err = match message_broker_config(&cp) {
            Ok(_) => panic!("expected missing message broker error"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("control_plane.message_broker configuration is missing"));
    }

    #[tokio::test]
    async fn build_control_plane_rejects_unsupported_database_driver_and_missing_url() {
        let unsupported = proto::TalonConfig {
            control_plane: Some(proto::ControlPlaneConfig {
                database: Some(proto::DatabaseConfig {
                    data_dir: String::new(),
                    driver: "badger".to_string(),
                    url: None,
                }),
                message_broker: Some(proto::MessageBrokerConfig {
                    driver: "gcp_pubsub".to_string(),
                }),
                scheduler: None,
            }),
            ..Default::default()
        };

        let err = match build_control_plane(&unsupported).await {
            Ok(_) => panic!("expected unsupported database driver error"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("Unsupported database driver: badger"));

        let missing_url = proto::TalonConfig {
            control_plane: Some(proto::ControlPlaneConfig {
                database: Some(proto::DatabaseConfig {
                    data_dir: String::new(),
                    driver: "postgres".to_string(),
                    url: None,
                }),
                message_broker: Some(proto::MessageBrokerConfig {
                    driver: "gcp_pubsub".to_string(),
                }),
                scheduler: None,
            }),
            ..Default::default()
        };

        let err = match build_control_plane(&missing_url).await {
            Ok(_) => panic!("expected missing database url error"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("Database URL secret is missing"));

        let unsupported_message_broker = proto::ControlPlaneConfig {
            database: None,
            message_broker: Some(proto::MessageBrokerConfig {
                driver: "kafka".to_string(),
            }),
            scheduler: None,
        };

        let err = match message_broker_config(&unsupported_message_broker) {
            Ok(_) => panic!("expected unsupported message broker error"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("Unsupported message broker driver: kafka"));

        let local_socket_message_broker = proto::ControlPlaneConfig {
            database: None,
            message_broker: Some(proto::MessageBrokerConfig {
                driver: "local_socket".to_string(),
            }),
            scheduler: None,
        };
        assert!(message_broker_config(&local_socket_message_broker).is_ok());
    }

    #[tokio::test]
    async fn sqlite_database_url_uses_data_dir_or_explicit_url() {
        let dir = tempdir().unwrap();
        let cfg = proto::DatabaseConfig {
            data_dir: dir.path().display().to_string(),
            driver: "sqlite".to_string(),
            url: None,
        };
        let url = sqlite_database_url(&cfg).await.unwrap();
        assert!(url.ends_with("/talon-control-plane.db"));

        let explicit = proto::DatabaseConfig {
            data_dir: String::new(),
            driver: "sqlite".to_string(),
            url: Some(crate::config::Secret {
                source: Some(secret::Source::Plain(
                    "sqlite:///tmp/explicit.db".to_string(),
                )),
            }),
        };
        assert_eq!(
            sqlite_database_url(&explicit).await.unwrap(),
            "sqlite:///tmp/explicit.db"
        );
    }

    #[tokio::test]
    async fn sqlite_database_url_requires_path_or_url() {
        let cfg = proto::DatabaseConfig {
            data_dir: "   ".to_string(),
            driver: "sqlite".to_string(),
            url: None,
        };
        let err = sqlite_database_url(&cfg).await.unwrap_err();
        assert!(err
            .to_string()
            .contains("SQLite database requires either control_plane.database.url or control_plane.database.data_dir"));
    }

    #[test]
    fn configured_scheduler_falls_back_from_empty_explicit_config_to_env() {
        let _lock = crate::test_support::env_lock();
        let _driver = EnvGuard::set("TALON_SCHEDULER_DRIVER", "cloud_tasks");
        let _project = EnvGuard::set("TALON_SCHEDULER_PROJECT_ID", "env-project");
        let _location = EnvGuard::set("TALON_SCHEDULER_LOCATION", "env-location");
        let _queue = EnvGuard::set("TALON_SCHEDULER_QUEUE", "env-queue");
        let _target = EnvGuard::set("TALON_SCHEDULER_TARGET_URL", "https://env.example/fire");
        let _token = EnvGuard::set("TALON_SCHEDULER_AUTH_TOKEN", "env-secret");

        let scheduler = configured_scheduler(Some(&proto::SchedulerConfig { backend: None }))
            .expect("expected env-backed scheduler config");
        match scheduler.backend.expect("expected backend") {
            scheduler_config::Backend::CloudTasks(cfg) => {
                assert_eq!(cfg.project_id, "env-project");
                assert_eq!(cfg.location, "env-location");
                assert_eq!(cfg.queue, "env-queue");
                assert_eq!(cfg.target_url, "https://env.example/fire");
                match cfg.callback_auth.and_then(|auth| auth.auth) {
                    Some(scheduler_callback_auth_config::Auth::SharedSecret(secret)) => {
                        assert_eq!(
                            secret.source,
                            Some(secret::Source::Plain("env-secret".to_string()))
                        );
                    }
                    other => panic!("expected shared secret auth, got {other:?}"),
                }
            }
        }
    }
}
