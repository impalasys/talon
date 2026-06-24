// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::events::{IndexEvent, IndexOperation};
use crate::control::search::{mapper, DeleteScope, DocumentStore, KIND_SESSION_MESSAGE};
use crate::control::{keys, ns, ControlPlane};
use anyhow::Result;
use std::sync::Arc;

#[derive(Clone)]
pub struct IndexController {
    documents: Arc<dyn DocumentStore + Send + Sync>,
    mapper: mapper::DocumentMapper,
}

impl IndexController {
    pub fn new(cp: Arc<ControlPlane>) -> Self {
        Self {
            documents: cp.documents.clone(),
            mapper: mapper::DocumentMapper::new(cp.clone()),
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
                for scope in delete_scopes_for_key(&key, event.generation)? {
                    self.documents.delete(&scope).await?;
                }
            }
            IndexOperation::Unspecified | IndexOperation::Upsert => {
                let now = chrono::Utc::now().timestamp_micros();
                let documents = self.mapper.map_key(&key, event.generation, now).await?;
                if !documents.is_empty() {
                    self.documents
                        .delete(&replace_scope_for_key(&key, event.generation)?)
                        .await?;
                    self.documents.upsert_documents(&documents).await?;
                }
            }
        }
        Ok(())
    }
}

fn replace_scope_for_key(key: &keys::ResourceKey, generation: u64) -> Result<DeleteScope> {
    exact_scope_for_key(key, generation)
}

fn delete_scopes_for_key(key: &keys::ResourceKey, generation: u64) -> Result<Vec<DeleteScope>> {
    match key.kind.as_str() {
        "Session" => Ok(vec![session_scope_for_key(key)]),
        "Namespace" if key.namespace == ns::TALON_SYSTEM => Ok(vec![
            exact_scope_for_key(key, generation)?,
            DeleteScope {
                namespace: key.name.clone(),
                ..Default::default()
            },
        ]),
        _ => Ok(vec![exact_scope_for_key(key, generation)?]),
    }
}

fn exact_scope_for_key(key: &keys::ResourceKey, generation: u64) -> Result<DeleteScope> {
    let mut scope = DeleteScope {
        namespace: key.namespace.clone(),
        resource_kind: key.kind.clone(),
        resource_key: key.canonical(),
        max_source_generation: generation,
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
    use crate::control::resources::ResourceStore;
    use crate::control::search::{self, Document};
    use crate::control::ProtoKeyValueStoreExt;
    use crate::gateway::rpc::data_proto;
    use crate::test_support::{EmptyPubSub, MockKvStore};

    fn control_plane(
        kv: Arc<MockKvStore>,
        documents: Arc<dyn DocumentStore + Send + Sync>,
    ) -> Arc<ControlPlane> {
        Arc::new(
            ControlPlane::builder(kv, Arc::new(EmptyPubSub))
                .documents(documents)
                .build(),
        )
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
        let documents = search::ephemeral_document_store();
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
                generation: 7,
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
        let document_ref = document.r#ref.as_ref().expect("document ref");
        assert_eq!(document.text, "Refund policy details");
        assert_eq!(
            document_ref.document_kind,
            search::DOCUMENT_KIND_MESSAGE_PART
        );
        assert_eq!(document_ref.generation, 7);
    }

    #[tokio::test]
    async fn index_controller_deletes_session_documents_from_session_target() {
        let kv = Arc::new(MockKvStore::default());
        let documents = search::ephemeral_document_store();
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
        let documents = search::ephemeral_document_store();
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
                generation: meta.generation,
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
        let document_ref = document.r#ref.as_ref().expect("document ref");
        let source = document_ref.source.as_ref().expect("document source");
        assert_eq!(source.kind, "Agent");
        assert_eq!(document_ref.document_kind, search::DOCUMENT_KIND_METADATA);
        assert_eq!(document_ref.generation, meta.generation);
        assert!(document.text.contains("support"));
    }

    #[tokio::test]
    async fn index_controller_skips_stale_resource_events() {
        let kv = Arc::new(MockKvStore::default());
        let documents = search::ephemeral_document_store();
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
                generation: current_generation,
                ..Default::default()
            })
            .await
            .unwrap();

        controller
            .handle_event(IndexEvent {
                key: key.canonical(),
                generation: current_generation - 1,
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
                .r#ref
                .as_ref()
                .expect("document ref")
                .generation,
            current_generation
        );

        controller
            .handle_event(IndexEvent {
                operation: IndexOperation::Delete as i32,
                key: key.canonical(),
                generation: current_generation - 1,
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
        let documents = search::ephemeral_document_store();
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
                generation: current_generation + 1,
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
        let documents = search::ephemeral_document_store();
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
                    r#ref: Some(search::DocumentRef {
                        id: namespace_doc_id.clone(),
                        source: Some(search::document_source(
                            ns::TALON_SYSTEM.to_string(),
                            "Namespace".to_string(),
                            namespace_key.canonical(),
                            String::new(),
                            String::new(),
                        )),
                        document_kind: search::DOCUMENT_KIND_METADATA.to_string(),
                        title: "Namespace/acme".to_string(),
                        ..Default::default()
                    }),
                    text: "acme namespace".to_string(),
                },
                Document {
                    r#ref: Some(search::DocumentRef {
                        id: agent_doc_id.clone(),
                        source: Some(search::document_source(
                            "acme".to_string(),
                            "Agent".to_string(),
                            agent_key.canonical(),
                            String::new(),
                            String::new(),
                        )),
                        document_kind: search::DOCUMENT_KIND_METADATA.to_string(),
                        title: "Agent/support".to_string(),
                        ..Default::default()
                    }),
                    text: "support agent".to_string(),
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
