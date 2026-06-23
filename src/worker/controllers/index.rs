// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::events::{IndexEvent, IndexOperation};
use crate::control::resources::ResourceStore;
use crate::control::search::{mapper, DeleteScope, Document, DocumentStore, KIND_SESSION_MESSAGE};
use crate::control::{keys, ns, ControlPlane};
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
        if event.prefix {
            anyhow::bail!("prefix index events are reserved but not supported yet");
        }
        let key = keys::ResourceKey::parse_canonical(&event.key)?;
        match IndexOperation::try_from(event.operation).unwrap_or(IndexOperation::Upsert) {
            IndexOperation::Delete => {
                for scope in delete_scopes_for_key(&key, event.source_generation)? {
                    self.documents.delete(&scope).await?;
                }
            }
            IndexOperation::Unspecified | IndexOperation::Upsert => {
                let documents = self
                    .extract_documents_for_key(&key, event.source_generation)
                    .await?;
                if !documents.is_empty() {
                    self.documents
                        .delete(&replace_scope_for_key(&key, event.source_generation)?)
                        .await?;
                    self.documents.upsert_documents(&documents).await?;
                }
            }
        }
        Ok(())
    }

    async fn extract_documents_for_key(
        &self,
        key: &keys::ResourceKey,
        source_generation: u64,
    ) -> Result<Vec<Document>> {
        let now = chrono::Utc::now().timestamp_micros();
        match key.kind.as_str() {
            "SessionMessage" => {
                let Some(bytes) = self.cp.kv.get(key).await? else {
                    return Ok(Vec::new());
                };
                let message = mapper::decode_session_message(bytes.as_slice())?;
                Ok(mapper::map_session_message(
                    key,
                    message,
                    source_generation,
                    now,
                ))
            }
            "Session" => {
                anyhow::bail!("session index key cannot be upserted")
            }
            _ => {
                let store = ResourceStore::new(self.cp.kv.clone(), self.cp.pubsub.clone());
                let Some(resource) = store.get(&key.namespace, &key.kind, &key.name).await? else {
                    return Ok(Vec::new());
                };
                let current_generation = resource
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.generation)
                    .unwrap_or_default();
                if source_generation > 0 {
                    if current_generation > source_generation {
                        tracing::debug!(
                            resource_key = key.canonical(),
                            event_generation = source_generation,
                            current_generation,
                            "skipping stale resource index event"
                        );
                        return Ok(Vec::new());
                    }
                    if current_generation < source_generation {
                        anyhow::bail!(
                            "resource {} generation {} is behind index event generation {}",
                            key.canonical(),
                            current_generation,
                            source_generation
                        );
                    }
                }
                mapper::map_control_plane_resource(key, &resource, now)
            }
        }
    }
}

fn replace_scope_for_key(key: &keys::ResourceKey, source_generation: u64) -> Result<DeleteScope> {
    exact_scope_for_key(key, source_generation)
}

fn delete_scopes_for_key(
    key: &keys::ResourceKey,
    source_generation: u64,
) -> Result<Vec<DeleteScope>> {
    match key.kind.as_str() {
        "Session" => Ok(vec![session_scope_for_key(key)]),
        "Namespace" if key.namespace == ns::TALON_SYSTEM => Ok(vec![
            exact_scope_for_key(key, source_generation)?,
            DeleteScope {
                namespace: key.name.clone(),
                ..Default::default()
            },
        ]),
        _ => Ok(vec![exact_scope_for_key(key, source_generation)?]),
    }
}

fn exact_scope_for_key(key: &keys::ResourceKey, source_generation: u64) -> Result<DeleteScope> {
    let mut scope = DeleteScope {
        namespace: key.namespace.clone(),
        resource_kind: key.kind.clone(),
        resource_key: key.canonical(),
        max_source_generation: source_generation,
        ..Default::default()
    };
    if key.kind == "SessionMessage" {
        scope.agent = parent_segment(key, "Agent");
        scope.session_id = parent_segment(key, "Session");
    }
    Ok(scope)
}

fn session_scope_for_key(key: &keys::ResourceKey) -> DeleteScope {
    DeleteScope {
        namespace: key.namespace.clone(),
        resource_kind: KIND_SESSION_MESSAGE.to_string(),
        agent: parent_segment(key, "Agent"),
        session_id: key.name.clone(),
        ..Default::default()
    }
}

fn parent_segment(key: &keys::ResourceKey, kind: &str) -> String {
    key.parent_segments()
        .ok()
        .and_then(|segments| {
            segments
                .into_iter()
                .find(|segment| segment.kind == kind)
                .map(|segment| segment.name)
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
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
                key: keys::session_message("acme", "support", "s1", "m1").canonical(),
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
                key: key.canonical(),
                source_generation: 7,
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
                key: key.canonical(),
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
                key: keys::session("acme", "support", "s1").canonical(),
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
                key: key.canonical(),
                source_generation: meta.generation,
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
                key: key.canonical(),
                source_generation: current_generation,
                ..Default::default()
            })
            .await
            .unwrap();

        controller
            .handle_event(IndexEvent {
                key: key.canonical(),
                source_generation: current_generation - 1,
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
                key: key.canonical(),
                source_generation: current_generation - 1,
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
                key: key.canonical(),
                source_generation: current_generation + 1,
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

    #[tokio::test]
    async fn index_controller_deletes_namespace_scope_from_namespace_key() {
        let kv = Arc::new(MockKvStore::default());
        let documents = search::memory_document_store();
        let cp = control_plane(kv.clone(), documents.clone());
        let namespace_key = keys::namespace_metadata("acme");
        let namespace_doc_id = search::document_id(
            &namespace_key.canonical(),
            search::DOCUMENT_KIND_METADATA,
            "",
        );
        let agent_key = keys::agent("acme", "support");
        let agent_doc_id =
            search::document_id(&agent_key.canonical(), search::DOCUMENT_KIND_METADATA, "");
        documents
            .upsert_documents(&[
                Document {
                    id: namespace_doc_id.clone(),
                    namespace: ns::TALON_SYSTEM.to_string(),
                    resource_kind: "Namespace".to_string(),
                    resource_key: namespace_key.canonical(),
                    document_kind: search::DOCUMENT_KIND_METADATA.to_string(),
                    title: "Namespace/acme".to_string(),
                    text: "acme namespace".to_string(),
                    ..Default::default()
                },
                Document {
                    id: agent_doc_id.clone(),
                    namespace: "acme".to_string(),
                    resource_kind: "Agent".to_string(),
                    resource_key: agent_key.canonical(),
                    document_kind: search::DOCUMENT_KIND_METADATA.to_string(),
                    title: "Agent/support".to_string(),
                    text: "support agent".to_string(),
                    ..Default::default()
                },
            ])
            .await
            .unwrap();

        IndexController::new(cp)
            .handle_event(IndexEvent {
                operation: IndexOperation::Delete as i32,
                key: namespace_key.canonical(),
                ..Default::default()
            })
            .await
            .unwrap();

        assert!(documents
            .get_document(ns::TALON_SYSTEM, &namespace_doc_id)
            .await
            .unwrap()
            .is_none());
        assert!(documents
            .get_document("acme", &agent_doc_id)
            .await
            .unwrap()
            .is_none());
    }
}
