// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub mod disabled;
mod events;
pub mod mapper;
pub mod postgres;
pub mod sqlite;
pub mod store;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub use disabled::{disabled_document_store, DisabledDocumentStore};
pub(crate) use events::publish_index_event;
pub use postgres::PostgresDocumentStore;
pub use sqlite::SqliteDocumentStore;
pub use store::{
    memory_document_store, DocumentStore, DocumentStoreCapabilities, MemoryDocumentStore,
};

pub const KIND_SESSION_MESSAGE: &str = "SessionMessage";
pub const KIND_KNOWLEDGE: &str = "Knowledge";

pub const DOCUMENT_KIND_METADATA: &str = "metadata";
pub const DOCUMENT_KIND_CONTENT: &str = "content";
pub const DOCUMENT_KIND_MESSAGE_PART: &str = "part";

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ResourceRef {
    pub namespace: String,
    pub kind: String,
    pub key: String,
    pub name: String,
    pub parent_kind: String,
    pub parent_key: String,
    pub uid: String,
    pub generation: u64,
    pub resource_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Document {
    pub id: String,
    pub namespace: String,
    pub resource_kind: String,
    pub resource_key: String,
    pub document_kind: String,
    pub parent_kind: String,
    pub parent_key: String,
    pub agent: String,
    pub session_id: String,
    pub channel: String,
    pub message_id: String,
    pub run_id: String,
    pub part_id: String,
    pub part_type: String,
    pub role: String,
    pub title: String,
    pub text: String,
    pub snippet: String,
    pub labels: HashMap<String, String>,
    pub metadata_json: String,
    pub acl_scope_json: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub indexed_at: i64,
    pub source_generation: u64,
    pub embedding_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchQuery {
    pub query: String,
    pub namespaces: Vec<String>,
    pub resource_kinds: Vec<String>,
    pub agent: String,
    pub session_id: String,
    pub channel: String,
    pub role: String,
    pub part_type: String,
    pub labels: HashMap<String, String>,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub limit: usize,
    pub page_token: String,
    pub sort: SearchSort,
    pub mode: SearchMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    #[default]
    Keyword,
    Semantic,
    Hybrid,
}

impl SearchMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            SearchMode::Keyword => "keyword",
            SearchMode::Semantic => "semantic",
            SearchMode::Hybrid => "hybrid",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchSort {
    #[default]
    Relevance,
    Recency,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchResult {
    pub document: Document,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub next_page_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DeleteScope {
    pub namespace: String,
    pub resource_kind: String,
    pub resource_key: String,
    pub resource_key_prefix: String,
    pub agent: String,
    pub session_id: String,
    pub channel: String,
    pub max_source_generation: u64,
}

pub(crate) fn query_matches(query: &SearchQuery, document: &Document) -> bool {
    if !query.resource_kinds.is_empty()
        && !query
            .resource_kinds
            .iter()
            .any(|kind| kind == &document.resource_kind)
    {
        return false;
    }
    if !query.agent.is_empty() && query.agent != document.agent {
        return false;
    }
    if !query.session_id.is_empty() && query.session_id != document.session_id {
        return false;
    }
    if !query.channel.is_empty() && query.channel != document.channel {
        return false;
    }
    if !query.role.is_empty() && query.role != document.role {
        return false;
    }
    if !query.part_type.is_empty() && query.part_type != document.part_type {
        return false;
    }
    if let Some(start) = query.start_time {
        if document.created_at < start {
            return false;
        }
    }
    if let Some(end) = query.end_time {
        if document.created_at > end {
            return false;
        }
    }
    for (key, value) in &query.labels {
        if document.labels.get(key) != Some(value) {
            return false;
        }
    }
    let terms = query_terms(&query.query);
    if terms.is_empty() {
        return true;
    }
    let haystack =
        format!("{} {} {}", document.title, document.text, document.snippet).to_lowercase();
    terms.iter().all(|term| haystack.contains(term))
}

pub(crate) fn delete_matches(scope: &DeleteScope, document: &Document) -> bool {
    if !scope.resource_kind.is_empty() && scope.resource_kind != document.resource_kind {
        return false;
    }
    if !scope.resource_key.is_empty() && scope.resource_key != document.resource_key {
        return false;
    }
    if !scope.resource_key_prefix.is_empty()
        && !document
            .resource_key
            .starts_with(&scope.resource_key_prefix)
    {
        return false;
    }
    if !scope.agent.is_empty() && scope.agent != document.agent {
        return false;
    }
    if !scope.session_id.is_empty() && scope.session_id != document.session_id {
        return false;
    }
    if !scope.channel.is_empty() && scope.channel != document.channel {
        return false;
    }
    if scope.max_source_generation > 0 && document.source_generation > scope.max_source_generation {
        return false;
    }
    true
}

pub(crate) fn score_document(query: &str, document: &Document) -> f32 {
    let terms = query_terms(query);
    if terms.is_empty() {
        return 1.0;
    }
    let title = document.title.to_lowercase();
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

pub(crate) fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect()
}

pub(crate) fn page_offset(page_token: &str) -> Result<usize, anyhow::Error> {
    if page_token.trim().is_empty() {
        return Ok(0);
    }
    page_token
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("invalid search page token"))
}

pub(crate) fn next_page_token(offset: usize, limit: usize, fetched: usize) -> String {
    if fetched > limit {
        (offset + limit).to_string()
    } else {
        String::new()
    }
}

pub fn document_id(resource_key: &str, document_kind: &str, subdocument_id: &str) -> String {
    if subdocument_id.is_empty() {
        format!("{resource_key}:{document_kind}")
    } else {
        format!("{resource_key}:{document_kind}:{subdocument_id}")
    }
}

pub fn snippet(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 240 {
        return normalized;
    }
    let mut out = normalized.chars().take(237).collect::<String>();
    out.push_str("...");
    out
}

pub fn unique_namespaces(namespaces: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    namespaces
        .into_iter()
        .filter(|namespace| !namespace.trim().is_empty())
        .filter(|namespace| seen.insert(namespace.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_store_capabilities_advertise_supported_modes() {
        let keyword = DocumentStoreCapabilities::keyword_only();
        assert!(keyword.is_enabled());
        assert!(keyword.supports(SearchMode::Keyword));
        assert!(!keyword.supports(SearchMode::Semantic));
        assert!(!keyword.supports(SearchMode::Hybrid));
        assert!(keyword.require_mode(SearchMode::Keyword).is_ok());
        assert!(keyword
            .require_mode(SearchMode::Semantic)
            .unwrap_err()
            .to_string()
            .contains("semantic search is not enabled"));

        let disabled = DocumentStoreCapabilities::disabled();
        assert!(!disabled.is_enabled());
        assert!(!disabled.supports(SearchMode::Keyword));
    }

    #[tokio::test]
    async fn memory_document_store_searches_and_deletes_documents() {
        let backend = memory_document_store();
        let document = Document {
            id: "doc-1".to_string(),
            namespace: "acme".to_string(),
            resource_kind: KIND_SESSION_MESSAGE.to_string(),
            resource_key: "@Namespace/acme/Agent/support/Session/s1/@/SessionMessage/m1"
                .to_string(),
            document_kind: DOCUMENT_KIND_MESSAGE_PART.to_string(),
            agent: "support".to_string(),
            session_id: "s1".to_string(),
            title: "Support session".to_string(),
            text: "refund policy details".to_string(),
            snippet: "refund policy details".to_string(),
            created_at: 10,
            updated_at: 10,
            ..Default::default()
        };
        backend.upsert_documents(&[document]).await.unwrap();

        let response = backend
            .search(&SearchQuery {
                query: "refund".to_string(),
                namespaces: vec!["acme".to_string()],
                resource_kinds: vec![KIND_SESSION_MESSAGE.to_string()],
                agent: "support".to_string(),
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].document.id, "doc-1");

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
            .search(&SearchQuery {
                query: "refund".to_string(),
                namespaces: vec!["acme".to_string()],
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(response.results.is_empty());
    }

    #[tokio::test]
    async fn memory_document_store_paginates_and_respects_delete_generation() {
        let backend = memory_document_store();
        let documents = (0..3)
            .map(|index| Document {
                id: format!("doc-{index}"),
                namespace: "acme".to_string(),
                resource_kind: KIND_KNOWLEDGE.to_string(),
                resource_key: format!("@Namespace/acme/Knowledge/doc-{index}"),
                document_kind: DOCUMENT_KIND_METADATA.to_string(),
                title: "Knowledge".to_string(),
                text: "policy".to_string(),
                snippet: "policy".to_string(),
                updated_at: index,
                source_generation: index as u64 + 1,
                ..Default::default()
            })
            .collect::<Vec<_>>();
        backend.upsert_documents(&documents).await.unwrap();

        let response = backend
            .search(&SearchQuery {
                query: "policy".to_string(),
                namespaces: vec!["acme".to_string()],
                limit: 2,
                sort: SearchSort::Recency,
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

    #[test]
    fn query_terms_split_punctuation_for_backend_safety() {
        assert_eq!(
            query_terms("can't foo/bar a@example.com"),
            vec!["can", "t", "foo", "bar", "a", "example", "com"]
        );
    }

    #[test]
    fn snippet_normalizes_whitespace_before_truncating() {
        assert_eq!(
            snippet("  refund\n\tpolicy   details  "),
            "refund policy details"
        );
        let long = format!("{}\n{}", "a".repeat(200), "b".repeat(100));
        let result = snippet(&long);
        assert_eq!(result.chars().count(), 240);
        assert!(result.ends_with("..."));
        assert!(!result.contains('\n'));
    }

    #[tokio::test]
    async fn memory_document_store_rejects_vector_modes_without_capability() {
        let backend = memory_document_store();
        let error = backend
            .search(&SearchQuery {
                query: "refund".to_string(),
                namespaces: vec!["acme".to_string()],
                mode: SearchMode::Hybrid,
                ..Default::default()
            })
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("hybrid search is not enabled for this document store"));
    }
}
