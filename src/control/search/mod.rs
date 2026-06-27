// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

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
}
