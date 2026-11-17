// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub use talon_storage_memory::search::disabled;
pub use talon_storage_memory::search::ephemeral;
mod events;
pub mod mapper;
pub use talon_storage_postgres::search::postgres;
pub use talon_storage_sqlite::search::sqlite;
pub mod store;

pub use disabled::{disabled_document_store, DisabledDocumentStore};
pub use ephemeral::{ephemeral_document_store, EphemeralDocumentStore};
pub(crate) use events::publish_index_event;
pub use postgres::PostgresDocumentStore;
pub use sqlite::SqliteDocumentStore;
pub use talon_contracts::search::*;
