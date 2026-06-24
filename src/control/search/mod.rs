// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub mod d1;
pub mod disabled;
pub mod ephemeral;
mod events;
pub mod mapper;
pub mod postgres;
pub mod sqlite;
pub mod store;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::control::keys;
use crate::gateway::rpc::{data_proto, proto};

pub use d1::D1DocumentStore;
pub use disabled::{disabled_document_store, DisabledDocumentStore};
pub use ephemeral::{ephemeral_document_store, EphemeralDocumentStore};
pub(crate) use events::publish_index_event;
pub use postgres::PostgresDocumentStore;
pub use sqlite::SqliteDocumentStore;
pub use store::{DocumentStore, DocumentStoreCapabilities};

pub const KIND_SESSION_MESSAGE: &str = "SessionMessage";
pub const KIND_KNOWLEDGE: &str = "Knowledge";

pub const DOCUMENT_KIND_METADATA: &str = "metadata";
pub const DOCUMENT_KIND_CONTENT: &str = "content";
pub const DOCUMENT_KIND_MESSAGE_PART: &str = "part";

pub const ATTR_AGENT: &str = "agent";
pub const ATTR_SESSION_ID: &str = "session_id";
pub const ATTR_CHANNEL: &str = "channel";
pub const ATTR_MESSAGE_ID: &str = "message_id";
pub const ATTR_RUN_ID: &str = "run_id";
pub const ATTR_PART_ID: &str = "part_id";
pub const ATTR_PART_TYPE: &str = "part_type";
pub const ATTR_ROLE: &str = "role";

pub type Document = data_proto::Document;
pub type DocumentRef = data_proto::DocumentRef;
pub type DocumentSource = data_proto::DocumentSource;

pub fn document_ref(
    id: String,
    source: DocumentSource,
    document_kind: String,
    subdocument_id: String,
) -> DocumentRef {
    DocumentRef {
        id,
        source: Some(source),
        document_kind,
        subdocument_id,
        ..Default::default()
    }
}

pub fn document_source(
    namespace: String,
    kind: String,
    key: String,
    parent_kind: String,
    parent_key: String,
) -> DocumentSource {
    let name = keys::ResourceKey::parse_canonical(&key)
        .map(|key| key.name)
        .unwrap_or_default();
    DocumentSource {
        namespace,
        key,
        kind,
        name,
        parent_kind,
        parent_key,
        ..Default::default()
    }
}

pub fn document_attributes(
    values: impl IntoIterator<Item = (&'static str, String)>,
) -> HashMap<String, String> {
    values
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

pub fn search_namespaces(query: &proto::SearchRequest) -> Vec<&str> {
    let Some(source) = query.source.as_ref() else {
        return Vec::new();
    };
    source.namespaces.iter().map(String::as_str).collect()
}

pub fn search_limit(query: &proto::SearchRequest) -> usize {
    if query.limit <= 0 {
        10
    } else {
        (query.limit as usize).min(100)
    }
}

pub fn search_mode(query: &proto::SearchRequest) -> proto::SearchMode {
    match proto::SearchMode::try_from(query.mode).unwrap_or(proto::SearchMode::Keyword) {
        proto::SearchMode::Unspecified | proto::SearchMode::Keyword => proto::SearchMode::Keyword,
        proto::SearchMode::Semantic => proto::SearchMode::Semantic,
        proto::SearchMode::Hybrid => proto::SearchMode::Hybrid,
    }
}

pub fn search_mode_name(mode: proto::SearchMode) -> &'static str {
    match mode {
        proto::SearchMode::Unspecified | proto::SearchMode::Keyword => "keyword",
        proto::SearchMode::Semantic => "semantic",
        proto::SearchMode::Hybrid => "hybrid",
    }
}

pub fn search_sort(query: &proto::SearchRequest) -> proto::SearchSort {
    match proto::SearchSort::try_from(query.sort).unwrap_or(proto::SearchSort::Relevance) {
        proto::SearchSort::Recency => proto::SearchSort::Recency,
        _ => proto::SearchSort::Relevance,
    }
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

pub(crate) fn query_matches(query: &proto::SearchRequest, document: &Document) -> bool {
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

pub(crate) fn delete_matches(scope: &DeleteScope, document: &Document) -> bool {
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
    let original_len = text.len();
    let prefix_end = text
        .char_indices()
        .nth(1000)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    let text = &text[..prefix_end];
    let mut normalized = String::new();
    let mut chars = 0usize;
    let mut first_word = true;
    let mut truncated = prefix_end < original_len;

    'words: for word in text.split_whitespace() {
        if !first_word {
            if chars == 240 {
                truncated = true;
                break;
            }
            normalized.push(' ');
            chars += 1;
        }
        first_word = false;

        for ch in word.chars() {
            if chars == 240 {
                truncated = true;
                break 'words;
            }
            normalized.push(ch);
            chars += 1;
        }
    }

    if !truncated {
        return normalized;
    }

    let mut truncated = normalized.chars().take(237).collect::<String>();
    truncated.push_str("...");
    truncated
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

    fn test_document(
        id: String,
        source: DocumentSource,
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

    #[test]
    fn document_store_capabilities_advertise_supported_modes() {
        let keyword = DocumentStoreCapabilities::keyword_only();
        assert!(keyword.is_enabled());
        assert!(keyword.supports(proto::SearchMode::Keyword));
        assert!(!keyword.supports(proto::SearchMode::Semantic));
        assert!(!keyword.supports(proto::SearchMode::Hybrid));
        assert!(keyword.require_mode(proto::SearchMode::Keyword).is_ok());
        assert!(keyword
            .require_mode(proto::SearchMode::Semantic)
            .unwrap_err()
            .to_string()
            .contains("semantic search is not enabled"));

        let disabled = DocumentStoreCapabilities::disabled();
        assert!(!disabled.is_enabled());
        assert!(!disabled.supports(proto::SearchMode::Keyword));
    }

    #[tokio::test]
    async fn ephemeral_document_store_searches_and_deletes_documents() {
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
    async fn ephemeral_document_store_paginates_and_respects_delete_generation() {
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
    async fn ephemeral_document_store_rejects_vector_modes_without_capability() {
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
