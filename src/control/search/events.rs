// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::events::{IndexEvent, IndexOperation};
use crate::control::{topics, MessagePublisher};
use anyhow::Result;
use prost::Message;

pub(crate) async fn publish_index_event(
    pubsub: &(dyn MessagePublisher + Send + Sync),
    mut event: IndexEvent,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp_micros();
    if event.id.is_empty() {
        event.id = crate::control::uuid::event_id();
    }
    if event.operation == IndexOperation::Unspecified as i32 {
        event.operation = IndexOperation::Upsert as i32;
    }
    if event.key.trim().is_empty() {
        anyhow::bail!("index event key is required");
    }
    if event.created_at == 0 {
        event.created_at = now;
    }
    event.updated_at = now;
    let payload = event.encode_to_vec();
    pubsub.publish(topics::INDEX_EVENTS_TOPIC, &payload).await
}
