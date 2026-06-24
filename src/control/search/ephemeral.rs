// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::store::{sort_results, DocumentStore};
use super::{
    delete_matches, next_page_token, page_offset, query_matches, search_limit, search_mode,
    search_namespaces, search_sort, snippet, DeleteScope, Document,
};
use crate::gateway::rpc::proto;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct EphemeralDocumentStore {
    documents: RwLock<Vec<Document>>,
}

pub fn ephemeral_document_store() -> Arc<dyn DocumentStore + Send + Sync> {
    Arc::new(EphemeralDocumentStore::default())
}

#[async_trait::async_trait]
impl DocumentStore for EphemeralDocumentStore {
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

    async fn search(&self, query: &proto::SearchRequest) -> Result<proto::SearchResponse> {
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
            .map(|document| proto::SearchResult {
                document: document.r#ref.clone(),
                snippet: snippet(&document.text),
                score: ephemeral_score_document(&query.query, &document),
            })
            .collect::<Vec<_>>();
        sort_results(&mut matches, search_sort(query));
        let limit = search_limit(query);
        let offset = page_offset(&query.page_token)?;
        let fetched = matches.len().saturating_sub(offset);
        let next_page_token = next_page_token(offset, limit, fetched);
        matches = matches.into_iter().skip(offset).take(limit).collect();
        Ok(proto::SearchResponse {
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

fn ephemeral_score_document(query: &str, document: &Document) -> f32 {
    let document_ref = document.r#ref.as_ref().expect("document ref is required");
    let terms = super::query_terms(query);
    if terms.is_empty() {
        return 1.0;
    }
    let title = document_ref.title.to_lowercase();
    let text = document.text.to_lowercase();
    let mut score = 0.0;
    for term in terms {
        if title.contains(&term) {
            score += 3.0;
        }
        score += text.matches(&term).count() as f32;
    }
    score.max(0.1)
}
