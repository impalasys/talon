// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::events::{index_event, IndexEvent, IndexOperation, IndexSessionMessageTarget};
use crate::control::resources::ResourceStore;
use crate::control::search::{mapper, DeleteScope, Document, DocumentStore, KIND_SESSION_MESSAGE};
use crate::control::{keys, ControlPlane};
use anyhow::Result;
use std::sync::Arc;

#[derive(Clone)]
pub struct IndexController {
    cp: Arc<ControlPlane>,
    documents: Arc<dyn DocumentStore + Send + Sync>,
}

impl IndexController {
    pub fn new(cp: Arc<ControlPlane>) -> Self {
        Self {
            documents: cp.documents.clone(),
            cp,
        }
    }

    pub async fn handle_event(&self, event: IndexEvent) -> Result<()> {
        if !self.documents.is_enabled() {
            tracing::debug!("index controller skipped because document store is disabled");
            return Ok(());
        }
        match IndexOperation::try_from(event.operation).unwrap_or(IndexOperation::Upsert) {
            IndexOperation::Delete => {
                self.documents
                    .delete(&delete_scope_for_event(&event)?)
                    .await?;
            }
            IndexOperation::Unspecified | IndexOperation::Upsert => {
                let documents = self.extract_documents_for_event(&event).await?;
                if !documents.is_empty() {
                    self.documents
                        .delete(&delete_scope_for_event(&event)?)
                        .await?;
                    self.documents.upsert_documents(&documents).await?;
                }
            }
        }
        Ok(())
    }

    async fn extract_documents_for_event(&self, event: &IndexEvent) -> Result<Vec<Document>> {
        let now = chrono::Utc::now().timestamp_micros();
        match event
            .target
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("index event target is required"))?
        {
            index_event::Target::SessionMessage(target) => {
                let key = session_message_key(target);
                let Some(bytes) = self.cp.kv.get(&key).await? else {
                    return Ok(Vec::new());
                };
                let message = mapper::decode_session_message(bytes.as_slice())?;
                Ok(mapper::map_session_message(
                    &key,
                    message,
                    target.source_generation,
                    now,
                ))
            }
            index_event::Target::Resource(target) => {
                let key = keys::ResourceKey::parse_canonical(&target.resource_key)?;
                let store = ResourceStore::new(self.cp.kv.clone(), self.cp.pubsub.clone());
                let Some(resource) = store.get(&key.namespace, &key.kind, &key.name).await? else {
                    return Ok(Vec::new());
                };
                let current_generation = resource
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.generation)
                    .unwrap_or_default();
                if target.source_generation > 0 {
                    if current_generation > target.source_generation {
                        tracing::debug!(
                            resource_key = target.resource_key,
                            event_generation = target.source_generation,
                            current_generation,
                            "skipping stale resource index event"
                        );
                        return Ok(Vec::new());
                    }
                    if current_generation < target.source_generation {
                        anyhow::bail!(
                            "resource {} generation {} is behind index event generation {}",
                            target.resource_key,
                            current_generation,
                            target.source_generation
                        );
                    }
                }
                mapper::map_control_plane_resource(&key, &resource, now)
            }
            index_event::Target::Session(_) => {
                anyhow::bail!("session index target cannot be upserted")
            }
        }
    }
}

fn delete_scope_for_event(event: &IndexEvent) -> Result<DeleteScope> {
    match event
        .target
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("index event target is required"))?
    {
        index_event::Target::Resource(target) => {
            let key = keys::ResourceKey::parse_canonical(&target.resource_key)?;
            Ok(DeleteScope {
                namespace: key.namespace,
                resource_kind: key.kind,
                resource_key: target.resource_key.clone(),
                max_source_generation: target.source_generation,
                ..Default::default()
            })
        }
        index_event::Target::SessionMessage(target) => Ok(DeleteScope {
            namespace: target.namespace.clone(),
            resource_kind: KIND_SESSION_MESSAGE.to_string(),
            resource_key: session_message_key(target).canonical(),
            agent: target.agent.clone(),
            session_id: target.session_id.clone(),
            max_source_generation: target.source_generation,
            ..Default::default()
        }),
        index_event::Target::Session(target) => Ok(DeleteScope {
            namespace: target.namespace.clone(),
            resource_kind: KIND_SESSION_MESSAGE.to_string(),
            agent: target.agent.clone(),
            session_id: target.session_id.clone(),
            ..Default::default()
        }),
    }
}

fn session_message_key(target: &IndexSessionMessageTarget) -> keys::ResourceKey {
    keys::session_message(
        &target.namespace,
        &target.agent,
        &target.session_id,
        &target.message_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::events::{IndexResourceTarget, IndexSessionTarget};
    use crate::control::search;
    use crate::control::ProtoKeyValueStoreExt;
    use crate::gateway::rpc::data_proto;
    use crate::test_support::{EmptyPubSub, MockKvStore};

    fn control_plane(
        kv: Arc<MockKvStore>,
        documents: Arc<dyn DocumentStore + Send + Sync>,
    ) -> Arc<ControlPlane> {
        Arc::new(ControlPlane {
            kv,
            pubsub: Arc::new(EmptyPubSub),
            scheduler: Arc::new(crate::control::scheduler::NoopSchedulerBackend),
            objects: crate::control::object_store::default_object_store(),
            documents,
        })
    }

    #[tokio::test]
    async fn disabled_document_store_acknowledges_index_events_without_work() {
        let kv = Arc::new(MockKvStore::default());
        let controller =
            IndexController::new(control_plane(kv.clone(), search::disabled_document_store()));
        controller
            .handle_event(IndexEvent {
                id: "event-1".to_string(),
                target: Some(index_event::Target::SessionMessage(
                    IndexSessionMessageTarget {
                        namespace: "acme".to_string(),
                        agent: "support".to_string(),
                        session_id: "s1".to_string(),
                        message_id: "m1".to_string(),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn index_controller_writes_session_documents_to_document_store() {
        let kv = Arc::new(MockKvStore::default());
        let documents = search::memory_document_store();
        let cp = control_plane(kv.clone(), documents.clone());
        let key = keys::session_message("acme", "support", "s1", "m1");
        kv.set_msg(
            &key,
            &data_proto::SessionMessage {
                id: "m1".to_string(),
                role: data_proto::MessageRole::RoleUser as i32,
                created_at: 100,
                labels: [("topic".to_string(), "billing".to_string())]
                    .into_iter()
                    .collect(),
                parts: vec![data_proto::SessionMessagePart {
                    id: "000000".to_string(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: "Refund policy details".to_string(),
                    created_at: 101,
                    ..Default::default()
                }],
            },
        )
        .await
        .unwrap();
        IndexController::new(cp)
            .handle_event(IndexEvent {
                id: "event-1".to_string(),
                target: Some(index_event::Target::SessionMessage(
                    IndexSessionMessageTarget {
                        namespace: "acme".to_string(),
                        agent: "support".to_string(),
                        session_id: "s1".to_string(),
                        message_id: "m1".to_string(),
                        source_generation: 7,
                    },
                )),
                ..Default::default()
            })
            .await
            .unwrap();
        let document_id = search::document_id(
            &key.canonical(),
            search::DOCUMENT_KIND_MESSAGE_PART,
            "000000",
        );
        let document = documents
            .get_document("acme", &document_id)
            .await
            .unwrap()
            .expect("document should be indexed");
        assert_eq!(document.text, "Refund policy details");
        assert_eq!(document.document_kind, search::DOCUMENT_KIND_MESSAGE_PART);
        assert_eq!(document.source_generation, 7);
    }

    #[tokio::test]
    async fn index_controller_deletes_session_documents_from_session_target() {
        let kv = Arc::new(MockKvStore::default());
        let documents = search::memory_document_store();
        let cp = control_plane(kv.clone(), documents.clone());
        let key = keys::session_message("acme", "support", "s1", "m1");
        kv.set_msg(
            &key,
            &data_proto::SessionMessage {
                id: "m1".to_string(),
                role: data_proto::MessageRole::RoleUser as i32,
                created_at: 100,
                parts: vec![data_proto::SessionMessagePart {
                    id: "000000".to_string(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: "Refund policy details".to_string(),
                    created_at: 101,
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let controller = IndexController::new(cp);
        controller
            .handle_event(IndexEvent {
                target: Some(index_event::Target::SessionMessage(
                    IndexSessionMessageTarget {
                        namespace: "acme".to_string(),
                        agent: "support".to_string(),
                        session_id: "s1".to_string(),
                        message_id: "m1".to_string(),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            })
            .await
            .unwrap();
        let document_id = search::document_id(
            &key.canonical(),
            search::DOCUMENT_KIND_MESSAGE_PART,
            "000000",
        );
        assert!(documents
            .get_document("acme", &document_id)
            .await
            .unwrap()
            .is_some());

        controller
            .handle_event(IndexEvent {
                operation: IndexOperation::Delete as i32,
                target: Some(index_event::Target::Session(IndexSessionTarget {
                    namespace: "acme".to_string(),
                    agent: "support".to_string(),
                    session_id: "s1".to_string(),
                })),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(documents
            .get_document("acme", &document_id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn index_controller_writes_metadata_for_control_plane_resource() {
        let kv = Arc::new(MockKvStore::default());
        let documents = search::memory_document_store();
        let cp = control_plane(kv.clone(), documents.clone());
        let store = ResourceStore::new(kv.clone(), cp.pubsub.clone());
        let resource = crate::control::resource_model::agent_resource(
            "acme",
            "support",
            crate::gateway::rpc::resources_proto::AgentSpec::default(),
            [("team".to_string(), "care".to_string())]
                .into_iter()
                .collect(),
        );
        let resource = store.upsert("acme", resource).await.unwrap();
        let meta = resource.metadata.as_ref().unwrap();
        let key = keys::ResourceKey::new("acme", &[], "Agent", "support");

        IndexController::new(cp)
            .handle_event(IndexEvent {
                target: Some(index_event::Target::Resource(IndexResourceTarget {
                    resource_key: key.canonical(),
                    source_generation: meta.generation,
                })),
                ..Default::default()
            })
            .await
            .unwrap();
        let document = documents
            .get_document(
                "acme",
                &search::document_id(&key.canonical(), search::DOCUMENT_KIND_METADATA, ""),
            )
            .await
            .unwrap()
            .expect("metadata document should be indexed");
        assert_eq!(document.resource_kind, "Agent");
        assert_eq!(document.document_kind, search::DOCUMENT_KIND_METADATA);
        assert_eq!(document.source_generation, meta.generation);
        assert!(document.text.contains("support"));
    }

    #[tokio::test]
    async fn index_controller_skips_stale_resource_events() {
        let kv = Arc::new(MockKvStore::default());
        let documents = search::memory_document_store();
        let cp = control_plane(kv.clone(), documents.clone());
        let store = ResourceStore::new(kv.clone(), cp.pubsub.clone());
        store
            .upsert(
                "acme",
                crate::control::resource_model::agent_resource(
                    "acme",
                    "support",
                    crate::gateway::rpc::resources_proto::AgentSpec::default(),
                    Default::default(),
                ),
            )
            .await
            .unwrap();
        let updated = store
            .patch_spec(
                "acme",
                "Agent",
                "support",
                None,
                crate::gateway::rpc::resources_proto::ResourceSpec {
                    kind: Some(
                        crate::gateway::rpc::resources_proto::resource_spec::Kind::Agent(
                            crate::gateway::rpc::resources_proto::AgentSpec::default(),
                        ),
                    ),
                },
            )
            .await
            .unwrap();
        let current_generation = updated.metadata.as_ref().unwrap().generation;
        let key = keys::ResourceKey::new("acme", &[], "Agent", "support");
        let document_id = search::document_id(&key.canonical(), search::DOCUMENT_KIND_METADATA, "");
        let controller = IndexController::new(cp);

        controller
            .handle_event(IndexEvent {
                target: Some(index_event::Target::Resource(IndexResourceTarget {
                    resource_key: key.canonical(),
                    source_generation: current_generation,
                })),
                ..Default::default()
            })
            .await
            .unwrap();

        controller
            .handle_event(IndexEvent {
                target: Some(index_event::Target::Resource(IndexResourceTarget {
                    resource_key: key.canonical(),
                    source_generation: current_generation - 1,
                })),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            documents
                .get_document("acme", &document_id)
                .await
                .unwrap()
                .expect("current document should remain after stale upsert")
                .source_generation,
            current_generation
        );

        controller
            .handle_event(IndexEvent {
                operation: IndexOperation::Delete as i32,
                target: Some(index_event::Target::Resource(IndexResourceTarget {
                    resource_key: key.canonical(),
                    source_generation: current_generation - 1,
                })),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(documents
            .get_document("acme", &document_id)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn index_controller_retries_resource_events_ahead_of_canonical_generation() {
        let kv = Arc::new(MockKvStore::default());
        let documents = search::memory_document_store();
        let cp = control_plane(kv.clone(), documents.clone());
        let store = ResourceStore::new(kv.clone(), cp.pubsub.clone());
        let resource = store
            .upsert(
                "acme",
                crate::control::resource_model::agent_resource(
                    "acme",
                    "support",
                    crate::gateway::rpc::resources_proto::AgentSpec::default(),
                    Default::default(),
                ),
            )
            .await
            .unwrap();
        let current_generation = resource.metadata.as_ref().unwrap().generation;
        let key = keys::ResourceKey::new("acme", &[], "Agent", "support");
        let document_id = search::document_id(&key.canonical(), search::DOCUMENT_KIND_METADATA, "");

        let error = IndexController::new(cp)
            .handle_event(IndexEvent {
                target: Some(index_event::Target::Resource(IndexResourceTarget {
                    resource_key: key.canonical(),
                    source_generation: current_generation + 1,
                })),
                ..Default::default()
            })
            .await
            .expect_err("future generation should retry instead of indexing stale state");

        assert!(error
            .to_string()
            .contains("is behind index event generation"));
        assert!(documents
            .get_document("acme", &document_id)
            .await
            .unwrap()
            .is_none());
    }
}
