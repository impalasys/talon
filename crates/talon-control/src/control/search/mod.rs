// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub use talon_storage_memory::search::disabled;
pub use talon_storage_memory::search::ephemeral;
mod events;
pub mod mapper;
pub mod store;

pub use disabled::{disabled_document_store, DisabledDocumentStore};
pub use ephemeral::{ephemeral_document_store, EphemeralDocumentStore};
pub use events::publish_index_event;
pub use talon_contracts::search::*;
