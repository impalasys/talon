// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::store::{sort_results, DocumentStore};
use super::{
    next_page_token, page_offset, query_terms, search_limit, search_mode, search_namespaces,
    search_sort, snippet, DeleteScope, Document, ATTR_AGENT, ATTR_CHANNEL, ATTR_SESSION_ID,
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
    let terms = query_terms(query);
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

fn query_matches(query: &proto::SearchRequest, document: &Document) -> bool {
    let document_ref = document.r#ref.as_ref().expect("document ref is required");
    let source = document_ref
        .source
        .as_ref()
        .expect("document source is required");
    if let Some(query_source) = query.source.as_ref() {
        if !query_source.key.is_empty() && query_source.key != source.key {
            return false;
        }
        if !query_source.key_prefix.is_empty() && !source.key.starts_with(&query_source.key_prefix)
        {
            return false;
        }
        if !query_source.parent_key.is_empty() && query_source.parent_key != source.parent_key {
            return false;
        }
        if !query_source.kinds.is_empty()
            && !query_source.kinds.iter().any(|kind| kind == &source.kind)
        {
            return false;
        }
    }
    for (key, value) in &query.attributes {
        if document_ref.attributes.get(key) != Some(value) {
            return false;
        }
    }
    if let Some(start) = query.start_time {
        if document_ref.created_at < start {
            return false;
        }
    }
    if let Some(end) = query.end_time {
        if document_ref.created_at > end {
            return false;
        }
    }
    for (key, value) in &query.labels {
        if document_ref.labels.get(key) != Some(value) {
            return false;
        }
    }
    let terms = query_terms(&query.query);
    if terms.is_empty() {
        return true;
    }
    let haystack = format!("{} {}", document_ref.title, document.text).to_lowercase();
    terms.iter().all(|term| haystack.contains(term))
}

fn delete_matches(scope: &DeleteScope, document: &Document) -> bool {
    let document_ref = document.r#ref.as_ref().expect("document ref is required");
    let source = document_ref
        .source
        .as_ref()
        .expect("document source is required");
    if !scope.resource_kind.is_empty() && scope.resource_kind != source.kind {
        return false;
    }
    if !scope.resource_key.is_empty() && scope.resource_key != source.key {
        return false;
    }
    if !scope.resource_key_prefix.is_empty() && !source.key.starts_with(&scope.resource_key_prefix)
    {
        return false;
    }
    if !scope.agent.is_empty()
        && document_ref.attributes.get(ATTR_AGENT).map(String::as_str) != Some(scope.agent.as_str())
    {
        return false;
    }
    if !scope.session_id.is_empty()
        && document_ref
            .attributes
            .get(ATTR_SESSION_ID)
            .map(String::as_str)
            != Some(scope.session_id.as_str())
    {
        return false;
    }
    if !scope.channel.is_empty()
        && document_ref
            .attributes
            .get(ATTR_CHANNEL)
            .map(String::as_str)
            != Some(scope.channel.as_str())
    {
        return false;
    }
    if scope.max_source_generation > 0 && document_ref.generation > scope.max_source_generation {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::search::{
        document_attributes, document_source, DocumentRef, DOCUMENT_KIND_MESSAGE_PART,
        DOCUMENT_KIND_METADATA, KIND_KNOWLEDGE, KIND_SESSION_MESSAGE,
    };

    fn test_document(
        id: String,
        source: crate::control::search::DocumentSource,
        document_kind: String,
        text: String,
    ) -> Document {
        Document {
            r#ref: Some(DocumentRef {
                id,
                source: Some(source),
                document_kind,
                ..Default::default()
            }),
            text,
        }
    }

    #[tokio::test]
    async fn searches_and_deletes_documents() {
        let backend = ephemeral_document_store();
        let mut document = test_document(
            "doc-1".to_string(),
            document_source(
                "acme".to_string(),
                KIND_SESSION_MESSAGE.to_string(),
                "@Namespace/acme/Agent/support/Session/s1/@/SessionMessage/m1".to_string(),
                "Session".to_string(),
                "@Namespace/acme/Agent/support/@/Session/s1".to_string(),
            ),
            DOCUMENT_KIND_MESSAGE_PART.to_string(),
            "refund policy details".to_string(),
        );
        let reference = document.r#ref.as_mut().unwrap();
        reference.attributes = document_attributes([
            (ATTR_AGENT, "support".to_string()),
            (ATTR_SESSION_ID, "s1".to_string()),
        ]);
        reference.title = "Support session".to_string();
        reference.created_at = 10;
        reference.updated_at = 10;
        backend.upsert_documents(&[document]).await.unwrap();

        let response = backend
            .search(&proto::SearchRequest {
                query: "refund".to_string(),
                source: Some(proto::SearchSourceFilter {
                    namespaces: vec!["acme".to_string()],
                    kinds: vec![KIND_SESSION_MESSAGE.to_string()],
                    ..Default::default()
                }),
                attributes: document_attributes([(ATTR_AGENT, "support".to_string())]),
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(response.results.len(), 1);
        assert_eq!(
            response.results[0]
                .document
                .as_ref()
                .expect("document ref")
                .id,
            "doc-1"
        );

        let deleted = backend
            .delete(&DeleteScope {
                namespace: "acme".to_string(),
                resource_kind: KIND_SESSION_MESSAGE.to_string(),
                session_id: "s1".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(deleted, 1);

        let response = backend
            .search(&proto::SearchRequest {
                query: "refund".to_string(),
                source: Some(proto::SearchSourceFilter {
                    namespaces: vec!["acme".to_string()],
                    ..Default::default()
                }),
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(response.results.is_empty());
    }

    #[tokio::test]
    async fn paginates_and_respects_delete_generation() {
        let backend = ephemeral_document_store();
        let documents = (0..3)
            .map(|index| {
                let mut document = test_document(
                    format!("doc-{index}"),
                    document_source(
                        "acme".to_string(),
                        KIND_KNOWLEDGE.to_string(),
                        format!("@Namespace/acme/Knowledge/doc-{index}"),
                        String::new(),
                        String::new(),
                    ),
                    DOCUMENT_KIND_METADATA.to_string(),
                    "policy".to_string(),
                );
                let reference = document.r#ref.as_mut().unwrap();
                reference.title = "Knowledge".to_string();
                reference.updated_at = index;
                reference.generation = index as u64 + 1;
                document
            })
            .collect::<Vec<_>>();
        backend.upsert_documents(&documents).await.unwrap();

        let response = backend
            .search(&proto::SearchRequest {
                query: "policy".to_string(),
                source: Some(proto::SearchSourceFilter {
                    namespaces: vec!["acme".to_string()],
                    ..Default::default()
                }),
                limit: 2,
                sort: proto::SearchSort::Recency as i32,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(response.results.len(), 2);
        assert_eq!(response.next_page_token, "2");

        let deleted = backend
            .delete(&DeleteScope {
                namespace: "acme".to_string(),
                resource_kind: KIND_KNOWLEDGE.to_string(),
                max_source_generation: 2,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(deleted, 2);
        assert!(backend
            .get_document("acme", "doc-2")
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn rejects_vector_modes_without_capability() {
        let backend = ephemeral_document_store();
        let error = backend
            .search(&proto::SearchRequest {
                query: "refund".to_string(),
                source: Some(proto::SearchSourceFilter {
                    namespaces: vec!["acme".to_string()],
                    ..Default::default()
                }),
                mode: proto::SearchMode::Hybrid as i32,
                ..Default::default()
            })
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("hybrid search is not enabled for this document store"));
    }
}
