// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{DeleteScope, Document, DocumentStoreCapabilities};
use crate::control::search::store::DocumentStore;
use crate::gateway::rpc::proto;
use anyhow::{anyhow, Result};
use std::sync::Arc;

#[derive(Default)]
pub struct DisabledDocumentStore;

pub fn disabled_document_store() -> Arc<dyn DocumentStore + Send + Sync> {
    Arc::new(DisabledDocumentStore)
}

#[async_trait::async_trait]
impl DocumentStore for DisabledDocumentStore {
    async fn upsert_documents(&self, _documents: &[Document]) -> Result<()> {
        Err(unavailable())
    }

    async fn delete(&self, _scope: &DeleteScope) -> Result<u64> {
        Err(unavailable())
    }

    async fn search(&self, _query: &proto::SearchRequest) -> Result<proto::SearchResponse> {
        Err(unavailable())
    }

    async fn get_document(&self, _namespace: &str, _id: &str) -> Result<Option<Document>> {
        Err(unavailable())
    }

    fn capabilities(&self) -> DocumentStoreCapabilities {
        DocumentStoreCapabilities::disabled()
    }
}

fn unavailable() -> anyhow::Error {
    anyhow!("document store is not enabled for this control plane database")
}
