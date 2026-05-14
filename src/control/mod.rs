use std::pin::Pin;
pub mod events;
pub mod keys;
pub mod kv;
pub mod ns;
pub mod pubsub;
pub mod scheduler;
pub mod topics;

use serde::{de::DeserializeOwned, Serialize};

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
    if db_config.driver != "postgres" {
        return Err(anyhow::anyhow!(
            "Unsupported database driver: {}",
            db_config.driver
        ));
    }
    let url_secret = db_config
        .url
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Database URL secret is missing"))?;
    let pg_url: String = url_secret.resolve().await?;
    println!("Connecting to PostgresKvStore at {}...", pg_url);
    let kv = std::sync::Arc::new(kv::PostgresKvStore::new(&pg_url, "talon_kv_store").await?);

    let mb_config = cp
        .message_broker
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("control_plane.message_broker configuration is missing"))?;
    if mb_config.driver != "gcp_pubsub" {
        return Err(anyhow::anyhow!(
            "Unsupported message broker driver: {}",
            mb_config.driver
        ));
    }
    println!("Initializing GcpPubSubPublisher...");
    let pubsub = std::sync::Arc::new(pubsub::GcpPubSubPublisher::new().await?);

    let scheduler: std::sync::Arc<dyn scheduler::SchedulerBackend + Send + Sync> =
        if matches!(scheduler_driver().as_deref(), Some("local_postgres")) {
            match scheduler::LocalPostgresSchedulerBackend::new(
                &pg_url,
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
            match configured_scheduler(cp.scheduler.as_ref()) {
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
            }
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
