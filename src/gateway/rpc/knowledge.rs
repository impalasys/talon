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
