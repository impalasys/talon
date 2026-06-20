// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{data_proto, proto, GrpcGatewayHandler};
use crate::control::keys;
use crate::control::ns;
use crate::control::resources::ResourceStore;
use crate::control::search::{SearchQuery, SearchSort, DOCUMENT_KIND_CONTENT, KIND_KNOWLEDGE};
use crate::gateway::rpc::resources_proto;
use crate::harness::knowledge::KnowledgeEntry;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

fn decode_entry(namespace: &str, path: &str, bytes: &[u8]) -> KnowledgeEntry {
    serde_json::from_slice(bytes).unwrap_or_else(|_| KnowledgeEntry {
        namespace: namespace.to_string(),
        name: path.to_string(),
        path: path.to_string(),
        content: String::from_utf8_lossy(bytes).to_string(),
        updated_at: 0,
    })
}

async fn list_namespace_knowledge(
    kv: Arc<dyn crate::control::KeyValueStore>,
    pubsub: Arc<dyn crate::control::MessagePublisher>,
    namespace: &str,
) -> std::result::Result<Vec<data_proto::Knowledge>, tonic::Status> {
    let mut modules = Vec::new();
    let mut seen_paths = HashSet::new();
    let resource_store = ResourceStore::new(kv.clone(), pubsub);

    for candidate_ns in ns::ancestry(namespace) {
        let resources = resource_store
            .list(&candidate_ns, Some("Knowledge"))
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to list Knowledge resources: {e}"))
            })?;
        for resource in resources {
            let Some(spec) = resource.spec.and_then(|spec| match spec.kind {
                Some(resources_proto::resource_spec::Kind::Knowledge(spec)) => Some(spec),
                _ => None,
            }) else {
                continue;
            };
            let path = spec.path.clone();
            if path.is_empty() || !seen_paths.insert(path.clone()) {
                continue;
            }
            modules.push(data_proto::Knowledge {
                namespace: candidate_ns.clone(),
                name: resource
                    .metadata
                    .as_ref()
                    .map(|meta| meta.name.clone())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| path.clone()),
                path,
                content: spec.content,
                updated_at: 0,
            });
        }

        let prefix = keys::knowledge_prefix(&candidate_ns);
        let keys = kv.list_keys(&prefix).await.map_err(|e| {
            tonic::Status::internal(format!("Failed to list knowledge artifacts: {}", e))
        })?;

        for key in keys {
            let path = keys::direct_child_name(&prefix, &key).unwrap_or_else(|| key.name.clone());
            if !seen_paths.insert(path.clone()) {
                continue;
            }
            if let Some(bytes) = kv.get(&key).await.unwrap_or(None) {
                let entry = decode_entry(&candidate_ns, &path, &bytes);
                modules.push(data_proto::Knowledge {
                    namespace: entry.namespace,
                    name: entry.name,
                    path,
                    content: entry.content,
                    updated_at: entry.updated_at,
                });
            }
        }
    }

    Ok(modules)
}

impl GrpcGatewayHandler {
    pub async fn handle_get_knowledge(
        &self,
        req: tonic::Request<proto::GetKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::KnowledgeResponse>, tonic::Status> {
        crate::require_auth!(read, self, req, &req.get_ref().ns, &req.get_ref().agent);
        let req = req.into_inner();

        let modules = if let Some(path) = req.path.filter(|p| !p.is_empty()) {
            let modules = list_namespace_knowledge(
                self.gateway.kv.clone(),
                self.gateway.pubsub.clone(),
                &req.ns,
            )
            .await?;
            let Some(module) = modules.into_iter().find(|module| module.path == path) else {
                return Err(tonic::Status::not_found(format!(
                    "Knowledge artifact '{}' not found",
                    path
                )));
            };
            vec![module]
        } else {
            list_namespace_knowledge(
                self.gateway.kv.clone(),
                self.gateway.pubsub.clone(),
                &req.ns,
            )
            .await?
        };

        Ok(tonic::Response::new(proto::KnowledgeResponse { modules }))
    }

    pub async fn handle_search_knowledge(
        &self,
        req: tonic::Request<proto::SearchKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::SearchKnowledgeResponse>, tonic::Status> {
        crate::require_auth!(read, self, req, &req.get_ref().ns, &req.get_ref().agent);
        let req = req.into_inner();
        let mode = super::search::mode(req.mode)?;
        let sort = super::search::sort(req.sort);
        let namespaces = super::search::knowledge_namespaces(&req.ns);
        let indexed = if self.gateway.documents.is_enabled() {
            Some(
                self.gateway
                    .documents
                    .search(&SearchQuery {
                        query: req.query.clone(),
                        namespaces: namespaces.clone(),
                        resource_kinds: vec![KIND_KNOWLEDGE.to_string()],
                        limit: super::search::limit(req.limit)
                            .saturating_mul(namespaces.len().max(1)),
                        mode,
                        sort,
                        ..Default::default()
                    })
                    .await
                    .map_err(super::search::search_error)?,
            )
        } else {
            None
        };
        if let Some(indexed) = indexed.filter(|indexed| !indexed.results.is_empty()) {
            let namespace_rank = namespaces
                .iter()
                .enumerate()
                .map(|(index, namespace)| (namespace.clone(), index))
                .collect::<HashMap<_, _>>();
            let mut by_path: HashMap<String, (usize, crate::control::search::SearchResult)> =
                HashMap::new();
            for result in indexed.results {
                if result.document.document_kind != DOCUMENT_KIND_CONTENT {
                    continue;
                }
                let path =
                    serde_json::from_str::<serde_json::Value>(&result.document.metadata_json)
                        .ok()
                        .and_then(|value| {
                            value
                                .get("path")
                                .and_then(|path| path.as_str())
                                .map(str::to_string)
                        })
                        .unwrap_or_else(|| result.document.title.clone());
                let rank = *namespace_rank
                    .get(&result.document.namespace)
                    .unwrap_or(&usize::MAX);
                match by_path.get(&path) {
                    Some((current_rank, _)) if *current_rank <= rank => {}
                    _ => {
                        by_path.insert(path, (rank, result));
                    }
                }
            }
            let mut search_results = by_path
                .into_values()
                .map(|(_, result)| result)
                .collect::<Vec<_>>();
            match sort {
                SearchSort::Recency => search_results.sort_by(|left, right| {
                    right
                        .document
                        .updated_at
                        .cmp(&left.document.updated_at)
                        .then_with(|| {
                            right
                                .score
                                .partial_cmp(&left.score)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                }),
                SearchSort::Relevance => search_results.sort_by(|left, right| {
                    right
                        .score
                        .partial_cmp(&left.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| right.document.updated_at.cmp(&left.document.updated_at))
                }),
            }
            search_results.truncate(super::search::limit(req.limit));
            if search_results.is_empty() {
                // Metadata documents make workspace search richer, but the legacy
                // knowledge endpoint should return knowledge content only.
            } else {
                let legacy_results = search_results
                    .iter()
                    .map(|result| {
                        let path = serde_json::from_str::<serde_json::Value>(
                            &result.document.metadata_json,
                        )
                        .ok()
                        .and_then(|value| {
                            value
                                .get("path")
                                .and_then(|path| path.as_str())
                                .map(str::to_string)
                        })
                        .unwrap_or_else(|| result.document.title.clone());
                        data_proto::KnowledgeSearchResult {
                            namespace: result.document.namespace.clone(),
                            path,
                            snippet: result.document.snippet.clone(),
                            score: result.score,
                            timestamp: result.document.updated_at,
                        }
                    })
                    .collect();
                return Ok(tonic::Response::new(proto::SearchKnowledgeResponse {
                    results: legacy_results,
                    search_results: search_results
                        .into_iter()
                        .map(|result| proto::SearchResult {
                            document: Some(super::search::document_proto(result.document)),
                            score: result.score,
                        })
                        .collect(),
                    next_page_token: indexed.next_page_token,
                }));
            }
        }
        let modules = list_namespace_knowledge(
            self.gateway.kv.clone(),
            self.gateway.pubsub.clone(),
            &req.ns,
        )
        .await?;
        let query = req.query.to_lowercase();
        let results = modules
            .into_iter()
            .filter(|module| {
                query.is_empty()
                    || module.path.to_lowercase().contains(&query)
                    || module.content.to_lowercase().contains(&query)
            })
            .take(super::search::limit(req.limit))
            .map(|module| data_proto::KnowledgeSearchResult {
                namespace: module.namespace,
                path: module.path,
                snippet: module.content,
                score: 1.0,
                timestamp: module.updated_at,
            })
            .collect();

        Ok(tonic::Response::new(proto::SearchKnowledgeResponse {
            results,
            search_results: Vec::new(),
            next_page_token: String::new(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_entry, list_namespace_knowledge};
    use crate::control::keys::{self, ResourceKey, ResourceList};
    use crate::control::KeyValueStore;
    use crate::gateway::rpc::{proto, GrpcGatewayHandler};
    use crate::gateway::{server::Gateway, session_streams::SessionStreamHub};
    use async_trait::async_trait;
    use futures::stream;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockKvStore {
        data: Mutex<HashMap<ResourceKey, Vec<u8>>>,
    }

    #[async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, k: &ResourceKey) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.data.lock().await.get(k).cloned())
        }

        async fn set(&self, k: &ResourceKey, v: &[u8]) -> anyhow::Result<()> {
            self.data.lock().await.insert(k.clone(), v.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            k: &ResourceKey,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let current = data.get(k).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(k.clone(), value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, k: &ResourceKey) -> anyhow::Result<()> {
            self.data.lock().await.remove(k);
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
    }

    #[derive(Default)]
    struct MockPubSub;

    #[async_trait]
    impl crate::control::MessagePublisher for MockPubSub {
        async fn publish(&self, _topic: &str, _message: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            Ok(Box::pin(stream::empty()))
        }
    }

    fn handler(kv: Arc<MockKvStore>) -> GrpcGatewayHandler {
        handler_with_documents(kv, crate::control::search::memory_document_store())
    }

    fn handler_with_documents(
        kv: Arc<MockKvStore>,
        documents: Arc<dyn crate::control::search::DocumentStore + Send + Sync>,
    ) -> GrpcGatewayHandler {
        let pubsub = Arc::new(MockPubSub);
        GrpcGatewayHandler {
            gateway: Arc::new(Gateway {
                auth_config: None,
                trust_config: None,
                kv,
                pubsub: pubsub.clone(),
                scheduler: Arc::new(crate::control::scheduler::NoopSchedulerBackend),
                objects: crate::control::object_store::default_object_store(),
                documents,
                session_streams: Arc::new(SessionStreamHub::new(pubsub)),
            }),
        }
    }

    #[test]
    fn decode_entry_falls_back_to_plaintext_payload() {
        let entry = decode_entry("acme", "guide.md", b"plain text payload");
        assert_eq!(entry.namespace, "acme");
        assert_eq!(entry.name, "guide.md");
        assert_eq!(entry.path, "guide.md");
        assert_eq!(entry.content, "plain text payload");
        assert_eq!(entry.updated_at, 0);
    }

    #[tokio::test]
    async fn list_namespace_knowledge_prefers_local_entries_and_skips_duplicates() {
        let kv = Arc::new(MockKvStore::default());
        let root_entry = crate::harness::knowledge::KnowledgeEntry {
            namespace: "acme".to_string(),
            name: "guide.md".to_string(),
            path: "guide.md".to_string(),
            content: "root guide".to_string(),
            updated_at: 1,
        };
        let child_entry = crate::harness::knowledge::KnowledgeEntry {
            namespace: "acme:child".to_string(),
            name: "guide.md".to_string(),
            path: "guide.md".to_string(),
            content: "child guide".to_string(),
            updated_at: 2,
        };
        let unique_entry = crate::harness::knowledge::KnowledgeEntry {
            namespace: "acme".to_string(),
            name: "shared.md".to_string(),
            path: "shared.md".to_string(),
            content: "shared root".to_string(),
            updated_at: 3,
        };
        kv.set(
            &keys::knowledge("acme", "guide.md"),
            &serde_json::to_vec(&root_entry).unwrap(),
        )
        .await
        .unwrap();
        kv.set(
            &keys::knowledge("acme:child", "guide.md"),
            &serde_json::to_vec(&child_entry).unwrap(),
        )
        .await
        .unwrap();
        kv.set(
            &keys::knowledge("acme", "shared.md"),
            &serde_json::to_vec(&unique_entry).unwrap(),
        )
        .await
        .unwrap();

        let modules = list_namespace_knowledge(kv, Arc::new(MockPubSub), "acme:child")
            .await
            .unwrap();
        assert_eq!(modules.len(), 2);
        assert!(modules
            .iter()
            .any(|m| m.path == "guide.md" && m.content == "child guide"));
        assert!(modules
            .iter()
            .any(|m| m.path == "shared.md" && m.content == "shared root"));
    }

    #[tokio::test]
    async fn handle_get_knowledge_returns_not_found_for_missing_path() {
        let response = handler(Arc::new(MockKvStore::default()))
            .handle_get_knowledge(tonic::Request::new(proto::GetKnowledgeRequest {
                agent: "agent".to_string(),
                ns: "acme".to_string(),
                path: Some("missing.md".to_string()),
            }))
            .await
            .expect_err("missing artifact should fail");
        assert_eq!(response.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn handle_search_knowledge_returns_matches_with_paths() {
        let kv = Arc::new(MockKvStore::default());
        let entry = crate::harness::knowledge::KnowledgeEntry {
            namespace: "acme".to_string(),
            name: "guide.md".to_string(),
            path: "guide.md".to_string(),
            content: "Rust scheduling notes".to_string(),
            updated_at: 7,
        };
        kv.set(
            &keys::knowledge("acme", "guide.md"),
            &serde_json::to_vec(&entry).unwrap(),
        )
        .await
        .unwrap();

        let response = handler(kv)
            .handle_search_knowledge(tonic::Request::new(proto::SearchKnowledgeRequest {
                agent: "agent".to_string(),
                ns: "acme".to_string(),
                query: "scheduling".to_string(),
                limit: 0,
                mode: proto::SearchMode::Keyword as i32,
                sort: proto::SearchSort::Relevance as i32,
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].path, "guide.md");
        assert_eq!(response.results[0].namespace, "acme");
    }

    #[tokio::test]
    async fn handle_search_knowledge_falls_back_when_document_store_is_disabled() {
        let kv = Arc::new(MockKvStore::default());
        let entry = crate::harness::knowledge::KnowledgeEntry {
            namespace: "acme".to_string(),
            name: "guide.md".to_string(),
            path: "guide.md".to_string(),
            content: "Rust scheduling notes".to_string(),
            updated_at: 7,
        };
        kv.set(
            &keys::knowledge("acme", "guide.md"),
            &serde_json::to_vec(&entry).unwrap(),
        )
        .await
        .unwrap();

        let response =
            handler_with_documents(kv, crate::control::search::disabled_document_store())
                .handle_search_knowledge(tonic::Request::new(proto::SearchKnowledgeRequest {
                    agent: "agent".to_string(),
                    ns: "acme".to_string(),
                    query: "scheduling".to_string(),
                    limit: 0,
                    mode: proto::SearchMode::Keyword as i32,
                    sort: proto::SearchSort::Relevance as i32,
                }))
                .await
                .unwrap()
                .into_inner();

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].path, "guide.md");
        assert!(response.search_results.is_empty());
    }
}
