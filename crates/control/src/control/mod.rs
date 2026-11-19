// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[allow(unused_imports)]
use crate::talon;

use std::{pin::Pin, sync::Arc};
pub mod cas;
pub mod config;
pub mod delegation;
pub mod events;
pub use talon::identifiers::keys;
pub mod manifest;
pub use talon::identifiers::ns;
pub mod object_store;
pub mod resource_model;
pub mod resources;
pub mod scheduler;
pub mod scheduling;
pub mod search;
pub mod security;
pub mod session_queue;
pub mod skills;
pub mod tool_output;
pub mod topics;
pub mod usage;
pub mod uuid;

use std::path::PathBuf;

use keys::{ResourceKey, ResourceList};

pub use talon::contracts::{
    apply_list_options_to_entries, apply_list_options_to_keys, KeyValueBatchOperation,
    KeyValueStore, ListOptions, MessagePublisher, Order, ProtoKeyValueStoreExt,
};

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
