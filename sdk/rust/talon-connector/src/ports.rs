// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::{ConnectorError, InboundEvent};
use async_trait::async_trait;
use std::time::SystemTime;

#[async_trait]
pub trait AttachmentStore: Send + Sync {
    async fn store(&self, content: Vec<u8>) -> Result<String, ConnectorError>;
}

#[async_trait]
pub trait EventSink: Send + Sync {
    async fn submit(&self, event: InboundEvent) -> Result<(), ConnectorError>;
}

#[async_trait]
pub trait ReplayGuard: Send + Sync {
    async fn reserve(&self, event_id: &str) -> Result<ReplayReservation, ConnectorError>;
    async fn release(&self, event_id: &str) -> Result<(), ConnectorError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayReservation {
    New,
    Duplicate,
}

pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}
