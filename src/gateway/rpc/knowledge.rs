// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{models, proto, GrpcGatewayHandler};
use crate::control::keys;
use crate::control::ns;
use crate::knowledge::{KnowledgeBook, KnowledgeEntry, KvKnowledgeBook};
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
    namespace: &str,
) -> std::result::Result<Vec<models::Knowledge>, tonic::Status> {
    let prefix = keys::knowledge_prefix();
    let mut modules = Vec::new();
    let mut seen_paths = HashSet::new();

    for candidate_ns in ns::ancestry(namespace) {
        let keys = kv.list_keys(&candidate_ns, prefix).await.map_err(|e| {
            tonic::Status::internal(format!("Failed to list knowledge artifacts: {}", e))
        })?;

        for key in keys {
            let path = key.strip_prefix(prefix).unwrap_or(&key).to_string();
            if !seen_paths.insert(path.clone()) {
                continue;
            }
            if let Some(bytes) = kv.get(&candidate_ns, &key).await.unwrap_or(None) {
                let entry = decode_entry(&candidate_ns, &path, &bytes);
                modules.push(models::Knowledge {
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
        crate::require_auth!(self, req, &req.get_ref().ns, &req.get_ref().agent);
        let req = req.into_inner();
        let book = KvKnowledgeBook::new(self.gateway.kv.clone());

        let modules = if let Some(path) = req.path.filter(|p| !p.is_empty()) {
            let Some(entry) = book.get(&req.ns, &path).await.map_err(|e| {
                tonic::Status::internal(format!("Failed to fetch knowledge artifact: {}", e))
            })?
            else {
                return Err(tonic::Status::not_found(format!(
                    "Knowledge artifact '{}' not found",
                    path
                )));
            };
            let entry_path = entry.path();

            vec![models::Knowledge {
                namespace: entry.namespace,
                name: entry.name,
                path: entry_path,
                content: entry.content,
                updated_at: entry.updated_at,
            }]
        } else {
            list_namespace_knowledge(self.gateway.kv.clone(), &req.ns).await?
        };

        Ok(tonic::Response::new(proto::KnowledgeResponse { modules }))
    }

    pub async fn handle_search_knowledge(
        &self,
        req: tonic::Request<proto::SearchKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::SearchKnowledgeResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns, &req.get_ref().agent);
        let req = req.into_inner();
        let book = KvKnowledgeBook::new(self.gateway.kv.clone());

        let results = book
            .search(&req.ns, &req.query, 5)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to search knowledge: {}", e)))?
            .into_iter()
            .map(|result| models::KnowledgeSearchResult {
                namespace: result.namespace,
                path: result.path,
                snippet: result.excerpt,
                score: 1.0,
                timestamp: result.updated_at,
            })
            .collect();

        Ok(tonic::Response::new(proto::SearchKnowledgeResponse {
            results,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_entry, list_namespace_knowledge};
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
        data: Mutex<HashMap<(String, String), Vec<u8>>>,
    }

    #[async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, ns: &str, k: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self
                .data
                .lock()
                .await
                .get(&(ns.to_string(), k.to_string()))
                .cloned())
        }

        async fn set(&self, ns: &str, k: &str, v: &[u8]) -> anyhow::Result<()> {
            self.data
                .lock()
                .await
                .insert((ns.to_string(), k.to_string()), v.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            ns: &str,
            k: &str,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let key = (ns.to_string(), k.to_string());
            let current = data.get(&key).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(key, value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, ns: &str, k: &str) -> anyhow::Result<()> {
            self.data.lock().await.remove(&(ns.to_string(), k.to_string()));
            Ok(())
        }

        async fn list_keys(&self, ns: &str, p: &str) -> anyhow::Result<Vec<String>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|(stored_ns, key)| {
                    (stored_ns == ns && key.starts_with(p)).then(|| key.clone())
                })
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
        let pubsub = Arc::new(MockPubSub);
        GrpcGatewayHandler {
            gateway: Arc::new(Gateway {
                auth_config: None,
                kv,
                pubsub: pubsub.clone(),
                scheduler: Arc::new(crate::control::scheduler::NoopSchedulerBackend),
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
        let root_entry = crate::knowledge::KnowledgeEntry {
            namespace: "acme".to_string(),
            name: "guide.md".to_string(),
            path: "guide.md".to_string(),
            content: "root guide".to_string(),
            updated_at: 1,
        };
        let child_entry = crate::knowledge::KnowledgeEntry {
            namespace: "acme:child".to_string(),
            name: "guide.md".to_string(),
            path: "guide.md".to_string(),
            content: "child guide".to_string(),
            updated_at: 2,
        };
        let unique_entry = crate::knowledge::KnowledgeEntry {
            namespace: "acme".to_string(),
            name: "shared.md".to_string(),
            path: "shared.md".to_string(),
            content: "shared root".to_string(),
            updated_at: 3,
        };
        kv.set(
            "acme",
            "Knowledge/guide.md",
            &serde_json::to_vec(&root_entry).unwrap(),
        )
        .await
        .unwrap();
        kv.set(
            "acme:child",
            "Knowledge/guide.md",
            &serde_json::to_vec(&child_entry).unwrap(),
        )
        .await
        .unwrap();
        kv.set(
            "acme",
            "Knowledge/shared.md",
            &serde_json::to_vec(&unique_entry).unwrap(),
        )
        .await
        .unwrap();

        let modules = list_namespace_knowledge(kv, "acme:child").await.unwrap();
        assert_eq!(modules.len(), 2);
        assert!(modules.iter().any(|m| m.path == "guide.md" && m.content == "child guide"));
        assert!(modules.iter().any(|m| m.path == "shared.md" && m.content == "shared root"));
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
        let entry = crate::knowledge::KnowledgeEntry {
            namespace: "acme".to_string(),
            name: "guide.md".to_string(),
            path: "guide.md".to_string(),
            content: "Rust scheduling notes".to_string(),
            updated_at: 7,
        };
        kv.set(
            "acme",
            "Knowledge/guide.md",
            &serde_json::to_vec(&entry).unwrap(),
        )
        .await
        .unwrap();

        let response = handler(kv)
            .handle_search_knowledge(tonic::Request::new(proto::SearchKnowledgeRequest {
                agent: "agent".to_string(),
                ns: "acme".to_string(),
                query: "scheduling".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].path, "guide.md");
        assert_eq!(response.results[0].namespace, "acme");
    }
}
