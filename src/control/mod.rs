// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::{pin::Pin, sync::Arc};
pub mod cas;
pub mod config;
pub mod delegation;
pub mod events;
pub mod keys;
pub mod kv;
pub mod manifest;
pub mod ns;
pub mod object_store;
pub mod profiling;
pub mod pubsub;
pub mod resource_model;
pub mod resources;
pub mod scheduler;
pub mod scheduling;
pub mod search;
pub mod security;
pub mod session_queue;
pub mod telemetry;
pub mod topics;
pub mod usage;
pub mod uuid;

use std::path::PathBuf;

use keys::{ResourceKey, ResourceList};

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

#[derive(Clone)]
pub struct ControlPlane {
    pub kv: SharedKeyValueStore,
    pub pubsub: SharedMessagePublisher,
    pub scheduler: SharedSchedulerBackend,
    pub objects: SharedObjectStore,
    pub documents: SharedDocumentStore,
}

pub type SharedKeyValueStore = Arc<dyn KeyValueStore + Send + Sync>;
pub type SharedMessagePublisher = Arc<dyn MessagePublisher + Send + Sync>;
pub type SharedSchedulerBackend = Arc<dyn scheduler::SchedulerBackend + Send + Sync>;
pub type SharedObjectStore = Arc<dyn object_store::ObjectStore + Send + Sync>;
pub type SharedDocumentStore = Arc<dyn search::DocumentStore + Send + Sync>;

impl ControlPlane {
    pub fn new(
        kv: SharedKeyValueStore,
        pubsub: SharedMessagePublisher,
        scheduler: SharedSchedulerBackend,
        objects: SharedObjectStore,
        documents: SharedDocumentStore,
    ) -> Self {
        Self {
            kv,
            pubsub,
            scheduler,
            objects,
            documents,
        }
    }

    /// Start a control-plane builder with lightweight defaults for services
    /// that many tests do not exercise.
    ///
    /// Production wiring should prefer `ControlPlane::new` or
    /// `build_control_plane`; this builder defaults the scheduler and object
    /// store so tests can specify only the dependencies relevant to the case.
    pub fn builder(kv: SharedKeyValueStore, pubsub: SharedMessagePublisher) -> ControlPlaneBuilder {
        ControlPlaneBuilder {
            kv,
            pubsub,
            scheduler: Arc::new(scheduler::NoopSchedulerBackend),
            objects: object_store::default_object_store(),
            documents: search::ephemeral_document_store(),
        }
    }

    /// Build a black-hole control plane for tests that only need to satisfy an
    /// API shape. Writes and publishes are accepted, but stored data is not
    /// retained.
    pub fn noop() -> Self {
        Self::builder(Arc::new(NoopKeyValueStore), Arc::new(NoopMessagePublisher)).build()
    }
}

pub struct ControlPlaneBuilder {
    kv: SharedKeyValueStore,
    pubsub: SharedMessagePublisher,
    scheduler: SharedSchedulerBackend,
    objects: SharedObjectStore,
    documents: SharedDocumentStore,
}

impl ControlPlaneBuilder {
    pub fn scheduler(mut self, scheduler: SharedSchedulerBackend) -> Self {
        self.scheduler = scheduler;
        self
    }

    pub fn objects(mut self, objects: SharedObjectStore) -> Self {
        self.objects = objects;
        self
    }

    pub fn documents(mut self, documents: SharedDocumentStore) -> Self {
        self.documents = documents;
        self
    }

    pub fn build(self) -> ControlPlane {
        ControlPlane::new(
            self.kv,
            self.pubsub,
            self.scheduler,
            self.objects,
            self.documents,
        )
    }
}

struct NoopKeyValueStore;

#[async_trait::async_trait]
impl KeyValueStore for NoopKeyValueStore {
    async fn get(&self, _key: &ResourceKey) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(None)
    }

    async fn set(&self, _key: &ResourceKey, _value: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }

    async fn compare_and_swap(
        &self,
        _key: &ResourceKey,
        _expected: Option<&[u8]>,
        _value: &[u8],
    ) -> anyhow::Result<bool> {
        Ok(true)
    }

    async fn delete(&self, _key: &ResourceKey) -> anyhow::Result<()> {
        Ok(())
    }

    async fn list_keys(
        &self,
        _list: &ResourceList,
        _options: Option<ListOptions<'_>>,
    ) -> anyhow::Result<Vec<ResourceKey>> {
        Ok(Vec::new())
    }
}

struct NoopMessagePublisher;

#[async_trait::async_trait]
impl MessagePublisher for NoopMessagePublisher {
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

async fn ensure_builtin_namespaces(kv: &(dyn KeyValueStore + Send + Sync)) -> anyhow::Result<()> {
    let default_key = keys::namespace_metadata(ns::DEFAULT);
    if kv
        .get_msg::<crate::gateway::rpc::resources_proto::Namespace>(&default_key)
        .await?
        .is_none()
    {
        kv.set_msg(
            &default_key,
            &crate::control::resource_model::namespace(
                ns::DEFAULT,
                String::new(),
                std::collections::HashMap::new(),
            ),
        )
        .await?;
    }
    kv.set(
        &keys::namespace_ref(None, ns::DEFAULT),
        ns::DEFAULT.as_bytes(),
    )
    .await?;
    Ok(())
}

fn message_broker_config(
    cp: &crate::control::config::proto::ControlPlaneConfig,
) -> anyhow::Result<&crate::control::config::proto::MessageBrokerConfig> {
    let mb_config = cp
        .message_broker
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("control_plane.message_broker configuration is missing"))?;
    let supported = mb_config.driver == "gcp_pubsub"
        || mb_config.driver == "local_socket"
        || mb_config.driver == "sqs";
    if !supported {
        return Err(anyhow::anyhow!(
            "Unsupported message broker driver: {}",
            mb_config.driver
        ));
    }
    Ok(mb_config)
}

pub async fn build_control_plane(
    config: &crate::control::config::Config,
) -> anyhow::Result<ControlPlane> {
    use crate::control::config::SecretExt;

    let cp = config
        .control_plane
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("control_plane configuration is missing"))?;

    let db_config = cp
        .database
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("control_plane.database configuration is missing"))?;
    let kv: SharedKeyValueStore;
    let scheduler_database_url: Option<String>;
    match db_config.driver.as_str() {
        "postgres" => {
            let url_secret = db_config
                .url
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Database URL secret is missing"))?;
            let pg_url: String = url_secret.resolve().await?;
            tracing::info!("Connecting to PostgresKvStore");
            kv = Arc::new(kv::PostgresKvStore::new(&pg_url, "talon_kv_store").await?);
            scheduler_database_url = Some(pg_url);
        }
        "sqlite" => {
            let sqlite_url = sqlite_database_url(db_config).await?;
            tracing::info!(database_url = %sqlite_url, "Connecting to SqliteKvStore");
            kv = Arc::new(kv::SqliteKvStore::new(&sqlite_url, "talon_kv_store").await?);
            scheduler_database_url = Some(sqlite_url);
        }
        #[cfg(feature = "aws")]
        "dynamodb" => {
            let table = dynamodb_table_name(db_config).await?;
            tracing::info!(table = %table, "Connecting to DynamoDbKvStore");
            let store = kv::DynamoDbKvStore::from_env(table).await?;
            kv = Arc::new(store);
            scheduler_database_url = None;
        }
        #[cfg(not(feature = "aws"))]
        "dynamodb" => {
            return Err(anyhow::anyhow!(
                "DynamoDB database driver is not enabled in this build"
            ));
        }
        #[cfg(feature = "rocksdb")]
        "rocksdb" => {
            let rocksdb_path = rocksdb_database_path(db_config).await?;
            tracing::info!(path = %rocksdb_path.display(), "Connecting to RocksDbKvStore");
            kv = Arc::new(kv::RocksDbKvStore::new(&rocksdb_path)?);
            scheduler_database_url = None;
        }
        #[cfg(not(feature = "rocksdb"))]
        "rocksdb" => {
            return Err(anyhow::anyhow!(
                "RocksDB database driver is not enabled in this build"
            ));
        }
        other => {
            return Err(anyhow::anyhow!("Unsupported database driver: {}", other));
        }
    }

    let document_db_config = cp.documents.as_ref().unwrap_or(db_config);
    let documents = configured_document_store(document_db_config).await?;

    ensure_builtin_namespaces(kv.as_ref()).await?;

    let mb_config = message_broker_config(cp)?;
    let pubsub: SharedMessagePublisher = match mb_config.driver.as_str() {
        "gcp_pubsub" => {
            tracing::info!("Initializing GcpPubSubPublisher");
            Arc::new(pubsub::GcpPubSubPublisher::new().await?)
        }
        "local_socket" => {
            let default_root =
                if db_config.driver == "sqlite" && !db_config.data_dir.trim().is_empty() {
                    Some(PathBuf::from(db_config.data_dir.trim()))
                } else {
                    None
                };
            let socket_path = pubsub::configured_local_socket_path(default_root.as_deref());
            tracing::info!(
                socket_path = %socket_path.display(),
                "Initializing LocalSocketMessagePublisher"
            );
            Arc::new(pubsub::LocalSocketMessagePublisher::new(socket_path).await?)
        }
        #[cfg(feature = "aws")]
        "sqs" => {
            tracing::info!("Initializing SqsMessagePublisher");
            Arc::new(pubsub::SqsMessagePublisher::from_env().await?)
        }
        #[cfg(not(feature = "aws"))]
        "sqs" => {
            return Err(anyhow::anyhow!(
                "SQS message broker driver is not enabled in this build"
            ));
        }
        other => {
            return Err(anyhow::anyhow!(
                "Unsupported message broker driver: {}",
                other
            ));
        }
    };

    let scheduler: SharedSchedulerBackend = match scheduler_driver().as_deref() {
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
                    Ok(backend) => Arc::new(backend),
                    Err(err) => {
                        return Err(anyhow::anyhow!(
                            "Failed to initialize local_postgres scheduler: {err}"
                        ));
                    }
                }
            } else {
                Arc::new(scheduler::NoopSchedulerBackend)
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
                    Ok(backend) => Arc::new(backend),
                    Err(err) => {
                        return Err(anyhow::anyhow!(
                            "Failed to initialize local_sqlite scheduler: {err}"
                        ));
                    }
                }
            } else {
                Arc::new(scheduler::NoopSchedulerBackend)
            }
        }
        _ => match configured_scheduler(cp.scheduler.as_ref()) {
            Some(crate::control::config::proto::SchedulerConfig {
                backend:
                    Some(crate::control::config::proto::scheduler_config::Backend::CloudTasks(cfg)),
            }) => match scheduler::CloudTasksSchedulerBackend::new(&cfg).await {
                Ok(backend) => Arc::new(backend),
                Err(err) => {
                    return Err(anyhow::anyhow!(
                        "Failed to initialize Cloud Tasks scheduler: {err}"
                    ));
                }
            },
            Some(crate::control::config::proto::SchedulerConfig { backend: None }) => {
                Arc::new(scheduler::NoopSchedulerBackend)
            }
            Some(crate::control::config::proto::SchedulerConfig {
                backend:
                    Some(
                        crate::control::config::proto::scheduler_config::Backend::AwsEventbridgeScheduler(
                            cfg,
                        ),
                    ),
            }) => {
                #[cfg(feature = "aws")]
                {
                    match scheduler::AwsEventBridgeSchedulerBackend::new(&cfg).await {
                        Ok(backend) => Arc::new(backend),
                        Err(err) => {
                            return Err(anyhow::anyhow!(
                                "Failed to initialize EventBridge Scheduler backend: {err}"
                            ));
                        }
                    }
                }
                #[cfg(not(feature = "aws"))]
                {
                    let _ = cfg;
                    return Err(anyhow::anyhow!(
                        "EventBridge Scheduler backend is not enabled in this build"
                    ));
                }
            }
            None => Arc::new(scheduler::NoopSchedulerBackend),
        },
    };

    let objects =
        object_store::object_store_from_config(cp.object_store.as_ref(), &config.workspace_dir)
            .await?;

    Ok(ControlPlane::new(kv, pubsub, scheduler, objects, documents))
}

fn configured_scheduler(
    cfg: Option<&crate::control::config::proto::SchedulerConfig>,
) -> Option<crate::control::config::proto::SchedulerConfig> {
    if let Some(cfg) = cfg.filter(|cfg| cfg.backend.is_some()) {
        return Some(cfg.clone());
    }

    let driver = scheduler_driver()?;
    let driver = driver.trim().to_string();
    if driver.is_empty() {
        return None;
    }

    match driver.as_str() {
        "cloud_tasks" => Some(crate::control::config::proto::SchedulerConfig {
            backend: Some(
                crate::control::config::proto::scheduler_config::Backend::CloudTasks(
                    crate::control::config::proto::CloudTasksSchedulerConfig {
                        project_id: std::env::var("TALON_SCHEDULER_PROJECT_ID").unwrap_or_default(),
                        location: std::env::var("TALON_SCHEDULER_LOCATION").unwrap_or_default(),
                        queue: std::env::var("TALON_SCHEDULER_QUEUE").unwrap_or_default(),
                        target_url: std::env::var("TALON_SCHEDULER_TARGET_URL").unwrap_or_default(),
                        callback_auth: configured_scheduler_callback_auth_from_env(),
                    },
                ),
            ),
        }),
        "aws_eventbridge_scheduler" => Some(crate::control::config::proto::SchedulerConfig {
            backend: Some(
                crate::control::config::proto::scheduler_config::Backend::AwsEventbridgeScheduler(
                    crate::control::config::proto::AwsEventBridgeSchedulerConfig {
                        group_name: std::env::var("TALON_AWS_SCHEDULER_GROUP_NAME")
                            .unwrap_or_default(),
                        queue_url: std::env::var("TALON_AWS_SCHEDULER_QUEUE_URL")
                            .or_else(|_| std::env::var("TALON_SQS_QUEUE_URL"))
                            .unwrap_or_default(),
                        execution_role_arn: std::env::var("TALON_AWS_SCHEDULER_EXECUTION_ROLE_ARN")
                            .unwrap_or_default(),
                        schedule_name_prefix: std::env::var("TALON_AWS_SCHEDULER_NAME_PREFIX")
                            .unwrap_or_default(),
                        dlq_arn: std::env::var("TALON_AWS_SCHEDULER_DLQ_ARN").unwrap_or_default(),
                        maximum_event_age_seconds: std::env::var(
                            "TALON_AWS_SCHEDULER_MAX_EVENT_AGE_SECONDS",
                        )
                        .ok()
                        .and_then(|value| value.parse::<u32>().ok())
                        .unwrap_or_default(),
                        maximum_retry_attempts: std::env::var(
                            "TALON_AWS_SCHEDULER_MAX_RETRY_ATTEMPTS",
                        )
                        .ok()
                        .and_then(|value| value.parse::<u32>().ok()),
                        endpoint_url: std::env::var("TALON_AWS_SCHEDULER_ENDPOINT_URL")
                            .unwrap_or_default(),
                    },
                ),
            ),
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
    db_config: &crate::control::config::proto::DatabaseConfig,
) -> anyhow::Result<String> {
    use crate::control::config::SecretExt;

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

async fn configured_document_store(
    db_config: &crate::control::config::proto::DatabaseConfig,
) -> anyhow::Result<SharedDocumentStore> {
    use crate::control::config::SecretExt;

    match db_config.driver.as_str() {
        "postgres" => {
            let url_secret = db_config
                .url
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Document database URL secret is missing"))?;
            let pg_url: String = url_secret.resolve().await?;
            tracing::info!("Connecting to PostgresDocumentStore");
            Ok(Arc::new(search::PostgresDocumentStore::new(&pg_url).await?))
        }
        "sqlite" => {
            let sqlite_url = sqlite_database_url(db_config).await?;
            tracing::info!(database_url = %sqlite_url, "Connecting to SqliteDocumentStore");
            Ok(Arc::new(
                search::SqliteDocumentStore::new(&sqlite_url).await?,
            ))
        }
        "disabled" => Ok(search::disabled_document_store()),
        #[cfg(feature = "rocksdb")]
        "rocksdb" => Ok(search::disabled_document_store()),
        #[cfg(not(feature = "rocksdb"))]
        "rocksdb" => Ok(search::disabled_document_store()),
        other => Err(anyhow::anyhow!(
            "Unsupported document database driver: {}",
            other
        )),
    }
}

#[cfg(feature = "aws")]
async fn dynamodb_table_name(
    db_config: &crate::control::config::proto::DatabaseConfig,
) -> anyhow::Result<String> {
    use crate::control::config::SecretExt;

    if let Some(url_secret) = db_config.url.as_ref() {
        let table: String = url_secret.resolve().await?;
        let table = table
            .strip_prefix("dynamodb://")
            .unwrap_or(&table)
            .trim()
            .to_string();
        if !table.is_empty() {
            return Ok(table);
        }
    }
    Ok(std::env::var("TALON_DYNAMODB_TABLE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "talon_state".to_string()))
}

#[cfg(feature = "rocksdb")]
async fn rocksdb_database_path(
    db_config: &crate::control::config::proto::DatabaseConfig,
) -> anyhow::Result<PathBuf> {
    use crate::control::config::SecretExt;

    if let Some(url_secret) = db_config.url.as_ref() {
        let raw: String = url_secret.resolve().await?;
        let path = raw.strip_prefix("rocksdb://").unwrap_or(&raw);
        if path.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "RocksDB database URL must resolve to a non-empty path"
            ));
        }
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        return Ok(path);
    }
    let data_dir = db_config.data_dir.trim();
    if data_dir.is_empty() {
        return Err(anyhow::anyhow!(
            "RocksDB database requires either control_plane.database.url or control_plane.database.data_dir"
        ));
    }
    let db_path = PathBuf::from(data_dir).join("talon-control-plane.rocksdb");
    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(db_path)
}

fn configured_scheduler_callback_auth_from_env(
) -> Option<crate::control::config::proto::SchedulerCallbackAuthConfig> {
    if let Some(token) = std::env::var("TALON_SCHEDULER_AUTH_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Some(crate::control::config::proto::SchedulerCallbackAuthConfig {
            auth: Some(
                crate::control::config::proto::scheduler_callback_auth_config::Auth::SharedSecret(
                    crate::control::config::Secret {
                        source: Some(crate::control::config::proto::secret::Source::Plain(token)),
                    },
                ),
            ),
        });
    }

    std::env::var("TALON_SCHEDULER_AUDIENCE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(
            |audience| crate::control::config::proto::SchedulerCallbackAuthConfig {
                auth: Some(
                    crate::control::config::proto::scheduler_callback_auth_config::Auth::GoogleOidc(
                        crate::control::config::proto::GoogleOidcAuthConfig {
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
    #[cfg(feature = "rocksdb")]
    use super::rocksdb_database_path;
    use super::{
        build_control_plane, configured_scheduler, configured_scheduler_callback_auth_from_env,
        ensure_builtin_namespaces, message_broker_config, sqlite_database_url, KeyValueStore,
        ListOptions, ProtoKeyValueStoreExt,
    };
    use crate::control::config::proto;
    use crate::control::config::proto::{scheduler_callback_auth_config, scheduler_config, secret};
    use crate::control::keys;
    use crate::control::resource_model::{NamespaceResourceExt, TypedResource};
    use crate::gateway::rpc::{data_proto, resources_proto};
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
            other => panic!("expected Cloud Tasks scheduler, got {other:?}"),
        }
    }

    #[test]
    fn configured_scheduler_reads_aws_eventbridge_from_env() {
        let _lock = crate::test_support::env_lock();
        let _driver = EnvGuard::set("TALON_SCHEDULER_DRIVER", "aws_eventbridge_scheduler");
        let _group = EnvGuard::set("TALON_AWS_SCHEDULER_GROUP_NAME", "talon-group");
        let _queue = EnvGuard::set(
            "TALON_AWS_SCHEDULER_QUEUE_URL",
            "https://sqs.us-east-1.amazonaws.com/123/talon",
        );
        let _role = EnvGuard::set(
            "TALON_AWS_SCHEDULER_EXECUTION_ROLE_ARN",
            "arn:aws:iam::123:role/talon-scheduler",
        );
        let _prefix = EnvGuard::set("TALON_AWS_SCHEDULER_NAME_PREFIX", "talon-dev");
        let _dlq = EnvGuard::set(
            "TALON_AWS_SCHEDULER_DLQ_ARN",
            "arn:aws:sqs:us-east-1:123:dlq",
        );
        let _age = EnvGuard::set("TALON_AWS_SCHEDULER_MAX_EVENT_AGE_SECONDS", "600");
        let _retries = EnvGuard::set("TALON_AWS_SCHEDULER_MAX_RETRY_ATTEMPTS", "2");
        let _endpoint = EnvGuard::set("TALON_AWS_SCHEDULER_ENDPOINT_URL", "http://localhost:4566");

        let scheduler = configured_scheduler(None).expect("expected scheduler config");
        match scheduler.backend.expect("expected backend") {
            scheduler_config::Backend::AwsEventbridgeScheduler(cfg) => {
                assert_eq!(cfg.group_name, "talon-group");
                assert_eq!(
                    cfg.queue_url,
                    "https://sqs.us-east-1.amazonaws.com/123/talon"
                );
                assert_eq!(
                    cfg.execution_role_arn,
                    "arn:aws:iam::123:role/talon-scheduler"
                );
                assert_eq!(cfg.schedule_name_prefix, "talon-dev");
                assert_eq!(cfg.dlq_arn, "arn:aws:sqs:us-east-1:123:dlq");
                assert_eq!(cfg.maximum_event_age_seconds, 600);
                assert_eq!(cfg.maximum_retry_attempts, Some(2));
                assert_eq!(cfg.endpoint_url, "http://localhost:4566");
            }
            other => panic!("expected EventBridge Scheduler config, got {other:?}"),
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
            other => panic!("expected Cloud Tasks scheduler, got {other:?}"),
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
        let a = keys::session("ns", "agent", "a");
        let b = keys::session("ns", "agent", "b");
        let other = keys::session("ns", "other", "c");
        let list = keys::session_prefix("ns", "agent");
        kv.set(&a, b"one").await.unwrap();
        kv.set(&b, b"two").await.unwrap();
        kv.set(&other, b"three").await.unwrap();

        let entries = kv.list_entries(&list, None).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], (a.clone(), b"one".to_vec()));
        assert_eq!(entries[1], (b.clone(), b"two".to_vec()));
        assert_eq!(
            kv.list_entries(&list, Some(ListOptions::desc()))
                .await
                .unwrap(),
            vec![(b.clone(), b"two".to_vec()), (a.clone(), b"one".to_vec())]
        );
        assert_eq!(
            kv.list_entries(&list, Some(ListOptions::desc().limit(1)))
                .await
                .unwrap(),
            vec![(b.clone(), b"two".to_vec())]
        );
        assert_eq!(
            kv.list_entries(&list, Some(ListOptions::default().after_name(Some("a"))))
                .await
                .unwrap(),
            vec![(b.clone(), b"two".to_vec())]
        );
        assert_eq!(
            kv.list_entries(&list, Some(ListOptions::desc().before_name(Some("b"))))
                .await
                .unwrap(),
            vec![(a.clone(), b"one".to_vec())]
        );

        kv.delete(&a).await.unwrap();
        kv.delete(&b).await.unwrap();
        assert!(kv.get(&a).await.unwrap().is_none());
        assert!(kv.get(&b).await.unwrap().is_none());
        assert_eq!(kv.get(&other).await.unwrap(), Some(b"three".to_vec()));

        let session = data_proto::Session {
            id: "session-1".to_string(),
            agent: "agent".to_string(),
            ns: "ns".to_string(),
            status: "IDLE".to_string(),
            created_at: 1,
            last_active: 2,
            metadata: HashMap::new(),
            labels: HashMap::from([("env".to_string(), "test".to_string())]),
        };
        let session_key = keys::session("ns", "agent", "session-1");
        kv.set_msg(&session_key, &session).await.unwrap();
        let loaded = kv
            .get_msg::<data_proto::Session>(&session_key)
            .await
            .unwrap()
            .expect("session should decode");
        assert_eq!(loaded.id, "session-1");
        assert_eq!(loaded.labels.get("env").map(String::as_str), Some("test"));
        assert!(kv
            .get_msg::<data_proto::Session>(&keys::session("ns", "agent", "missing"))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn ensure_builtin_namespaces_seeds_default_without_clobbering_existing_metadata() {
        let kv = MockKvStore::default();
        ensure_builtin_namespaces(&kv).await.unwrap();

        let seeded = kv
            .get_msg::<resources_proto::Namespace>(&keys::namespace_metadata("default"))
            .await
            .unwrap()
            .expect("default namespace should be seeded");
        assert_eq!(seeded.name(), "default");
        assert!(seeded.parent().is_empty());
        assert!(!seeded.is_deleted());
        assert_eq!(
            kv.get(&keys::namespace_ref(None, "default")).await.unwrap(),
            Some(b"default".to_vec())
        );

        let mut labeled = seeded;
        labeled
            .labels_mut()
            .expect("seeded namespace metadata should exist")
            .insert("owner".to_string(), "app".to_string());
        kv.set_msg(&keys::namespace_metadata("default"), &labeled)
            .await
            .unwrap();
        ensure_builtin_namespaces(&kv).await.unwrap();

        let preserved = kv
            .get_msg::<resources_proto::Namespace>(&keys::namespace_metadata("default"))
            .await
            .unwrap()
            .expect("default namespace should still exist");
        assert_eq!(
            preserved.labels().get("owner").map(String::as_str),
            Some("app")
        );
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
                object_store: None,
                documents: None,
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
            object_store: None,
            documents: None,
        };
        let err = match message_broker_config(&cp) {
            Ok(_) => panic!("expected missing message broker error"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("control_plane.message_broker configuration is missing"));
    }

    #[test]
    fn build_control_plane_errors_when_configured_scheduler_fails_to_initialize() {
        let _lock = crate::test_support::env_lock();
        let _driver = EnvGuard::remove("TALON_SCHEDULER_DRIVER");
        let _project = EnvGuard::remove("TALON_SCHEDULER_PROJECT_ID");
        let _gcp_project = EnvGuard::remove("GCP_PROJECT_ID");
        let _location = EnvGuard::remove("TALON_SCHEDULER_LOCATION");
        let _queue = EnvGuard::remove("TALON_SCHEDULER_QUEUE");
        let _target = EnvGuard::remove("TALON_SCHEDULER_TARGET_URL");
        let dir = tempdir().unwrap();
        let config = proto::TalonConfig {
            workspace_dir: dir.path().display().to_string(),
            control_plane: Some(proto::ControlPlaneConfig {
                database: Some(proto::DatabaseConfig {
                    data_dir: dir.path().display().to_string(),
                    driver: "sqlite".to_string(),
                    url: None,
                }),
                message_broker: Some(proto::MessageBrokerConfig {
                    driver: "local_socket".to_string(),
                }),
                scheduler: Some(proto::SchedulerConfig {
                    backend: Some(scheduler_config::Backend::CloudTasks(
                        proto::CloudTasksSchedulerConfig::default(),
                    )),
                }),
                object_store: None,
                documents: None,
            }),
            ..Default::default()
        };

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let err = match runtime.block_on(build_control_plane(&config)) {
            Ok(_) => panic!("expected scheduler initialization error"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("Failed to initialize Cloud Tasks scheduler"));
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
                object_store: None,
                documents: None,
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
                object_store: None,
                documents: None,
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
            object_store: None,
            documents: None,
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
            object_store: None,
            documents: None,
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
            url: Some(crate::control::config::Secret {
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

    #[cfg(feature = "rocksdb")]
    #[tokio::test]
    async fn rocksdb_database_path_uses_data_dir_or_explicit_url() {
        let dir = tempdir().unwrap();
        let cfg = proto::DatabaseConfig {
            data_dir: dir.path().display().to_string(),
            driver: "rocksdb".to_string(),
            url: None,
        };
        let path = rocksdb_database_path(&cfg).await.unwrap();
        assert!(path.ends_with("talon-control-plane.rocksdb"));

        let explicit = proto::DatabaseConfig {
            data_dir: String::new(),
            driver: "rocksdb".to_string(),
            url: Some(crate::control::config::Secret {
                source: Some(secret::Source::Plain(
                    "rocksdb:///tmp/explicit.rocksdb".to_string(),
                )),
            }),
        };
        assert_eq!(
            rocksdb_database_path(&explicit).await.unwrap(),
            std::path::PathBuf::from("/tmp/explicit.rocksdb")
        );
    }

    #[cfg(feature = "rocksdb")]
    #[tokio::test]
    async fn rocksdb_database_path_requires_path_or_url() {
        let cfg = proto::DatabaseConfig {
            data_dir: "   ".to_string(),
            driver: "rocksdb".to_string(),
            url: None,
        };
        let err = rocksdb_database_path(&cfg).await.unwrap_err();
        assert!(err
            .to_string()
            .contains("RocksDB database requires either control_plane.database.url or control_plane.database.data_dir"));
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
            other => panic!("expected Cloud Tasks scheduler, got {other:?}"),
        }
    }
}
