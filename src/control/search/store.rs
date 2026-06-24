// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{search_mode_name, DeleteScope, Document};
use crate::gateway::rpc::proto;
use anyhow::{anyhow, Result};

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
    async fn search(&self, query: &proto::SearchRequest) -> Result<proto::SearchResponse>;
    async fn get_document(&self, namespace: &str, id: &str) -> Result<Option<Document>>;

    fn capabilities(&self) -> DocumentStoreCapabilities {
        DocumentStoreCapabilities::keyword_only()
    }

    fn is_enabled(&self) -> bool {
        self.capabilities().is_enabled()
    }
}

pub(crate) fn sort_results(results: &mut [proto::SearchResult], sort: proto::SearchSort) {
    match sort {
        proto::SearchSort::Unspecified | proto::SearchSort::Relevance => {
            results.sort_by(|left, right| {
                right
                    .score
                    .partial_cmp(&left.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        let right_ref = right.document.as_ref().expect("document ref is required");
                        let left_ref = left.document.as_ref().expect("document ref is required");
                        right_ref.updated_at.cmp(&left_ref.updated_at)
                    })
            })
        }
        proto::SearchSort::Recency => results.sort_by(|left, right| {
            let right_ref = right.document.as_ref().expect("document ref is required");
            let left_ref = left.document.as_ref().expect("document ref is required");
            right_ref.updated_at.cmp(&left_ref.updated_at)
        }),
    }
}
