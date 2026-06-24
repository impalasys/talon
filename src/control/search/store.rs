// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{
    delete_matches, next_page_token, page_offset, query_matches, score_document, search_limit,
    search_mode, search_mode_name, search_namespaces, search_sort, snippet, DeleteScope, Document,
    SearchResponse, SearchResult,
};
use crate::gateway::rpc::proto;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentStoreCapabilities {
    pub keyword: bool,
    pub vector: bool,
    pub hybrid: bool,
}

impl DocumentStoreCapabilities {
    pub const fn keyword_only() -> Self {
        Self {
            keyword: true,
            vector: false,
            hybrid: false,
        }
    }

    pub const fn disabled() -> Self {
        Self {
            keyword: false,
            vector: false,
            hybrid: false,
        }
    }

    pub const fn is_enabled(self) -> bool {
        self.keyword || self.vector || self.hybrid
    }

    pub const fn supports(self, mode: proto::SearchMode) -> bool {
        match mode {
            proto::SearchMode::Unspecified | proto::SearchMode::Keyword => self.keyword,
            proto::SearchMode::Semantic => self.vector,
            proto::SearchMode::Hybrid => self.hybrid,
        }
    }

    pub fn require_mode(self, mode: proto::SearchMode) -> Result<()> {
        if self.supports(mode) {
            Ok(())
        } else {
            Err(anyhow!(
                "{} search is not enabled for this document store",
                search_mode_name(mode)
            ))
        }
    }
}

impl Default for DocumentStoreCapabilities {
    fn default() -> Self {
        Self::keyword_only()
    }
}

#[async_trait::async_trait]
pub trait DocumentStore: Send + Sync {
    async fn upsert_documents(&self, documents: &[Document]) -> Result<()>;
    async fn delete(&self, scope: &DeleteScope) -> Result<u64>;
    async fn search(&self, query: &proto::SearchRequest) -> Result<SearchResponse>;
    async fn get_document(&self, namespace: &str, id: &str) -> Result<Option<Document>>;

    fn capabilities(&self) -> DocumentStoreCapabilities {
        DocumentStoreCapabilities::keyword_only()
    }

    fn is_enabled(&self) -> bool {
        self.capabilities().is_enabled()
    }
}

#[derive(Default)]
pub struct MemoryDocumentStore {
    documents: RwLock<Vec<Document>>,
}

pub fn memory_document_store() -> Arc<dyn DocumentStore + Send + Sync> {
    Arc::new(MemoryDocumentStore::default())
}

#[async_trait::async_trait]
impl DocumentStore for MemoryDocumentStore {
    async fn upsert_documents(&self, documents: &[Document]) -> Result<()> {
        let mut stored = self.documents.write().await;
        for document in documents {
            let document_ref = document.r#ref.as_ref().expect("document ref is required");
            let document_source = document_ref
                .source
                .as_ref()
                .expect("document source is required");
            if let Some(existing) = stored.iter_mut().find(|existing| {
                let existing_ref = existing.r#ref.as_ref().expect("document ref is required");
                let existing_source = existing_ref
                    .source
                    .as_ref()
                    .expect("document source is required");
                existing_source.namespace == document_source.namespace
                    && existing_ref.id == document_ref.id
            }) {
                *existing = document.clone();
            } else {
                stored.push(document.clone());
            }
        }
        Ok(())
    }

    async fn delete(&self, scope: &DeleteScope) -> Result<u64> {
        if scope.namespace.trim().is_empty() {
            return Ok(0);
        }
        let mut stored = self.documents.write().await;
        let before = stored.len();
        stored.retain(|document| {
            let document_ref = document.r#ref.as_ref().expect("document ref is required");
            let source = document_ref
                .source
                .as_ref()
                .expect("document source is required");
            source.namespace != scope.namespace || !delete_matches(scope, document)
        });
        Ok(before.saturating_sub(stored.len()) as u64)
    }

    async fn search(&self, query: &proto::SearchRequest) -> Result<SearchResponse> {
        self.capabilities().require_mode(search_mode(query))?;
        let stored = self.documents.read().await;
        let namespaces = search_namespaces(query);
        let mut matches = stored
            .iter()
            .filter(|document| {
                let document_ref = document.r#ref.as_ref().expect("document ref is required");
                let source = document_ref
                    .source
                    .as_ref()
                    .expect("document source is required");
                !namespaces.is_empty()
                    && namespaces
                        .iter()
                        .any(|namespace| *namespace == source.namespace)
                    && query_matches(query, document)
            })
            .cloned()
            .map(|document| SearchResult {
                snippet: snippet(&document.text),
                score: score_document(&query.query, &document),
                document,
            })
            .collect::<Vec<_>>();
        sort_results(&mut matches, search_sort(query));
        let limit = search_limit(query);
        let offset = page_offset(&query.page_token)?;
        let fetched = matches.len().saturating_sub(offset);
        let next_page_token = next_page_token(offset, limit, fetched);
        matches = matches.into_iter().skip(offset).take(limit).collect();
        Ok(SearchResponse {
            results: matches,
            next_page_token,
        })
    }

    async fn get_document(&self, namespace: &str, id: &str) -> Result<Option<Document>> {
        Ok(self
            .documents
            .read()
            .await
            .iter()
            .find(|document| {
                let document_ref = document.r#ref.as_ref().expect("document ref is required");
                let source = document_ref
                    .source
                    .as_ref()
                    .expect("document source is required");
                source.namespace == namespace && document_ref.id == id
            })
            .cloned())
    }
}

pub(crate) fn sort_results(results: &mut [SearchResult], sort: proto::SearchSort) {
    match sort {
        proto::SearchSort::Unspecified | proto::SearchSort::Relevance => {
            results.sort_by(|left, right| {
                right
                    .score
                    .partial_cmp(&left.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        let right_ref = right
                            .document
                            .r#ref
                            .as_ref()
                            .expect("document ref is required");
                        let left_ref = left
                            .document
                            .r#ref
                            .as_ref()
                            .expect("document ref is required");
                        right_ref.updated_at.cmp(&left_ref.updated_at)
                    })
            })
        }
        proto::SearchSort::Recency => results.sort_by(|left, right| {
            let right_ref = right
                .document
                .r#ref
                .as_ref()
                .expect("document ref is required");
            let left_ref = left
                .document
                .r#ref
                .as_ref()
                .expect("document ref is required");
            right_ref.updated_at.cmp(&left_ref.updated_at)
        }),
    }
}
