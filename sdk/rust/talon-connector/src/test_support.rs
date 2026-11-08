// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::{Clock, ConnectorError, EventSink, InboundEvent, ReplayGuard, ReplayReservation};
use async_trait::async_trait;
use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::SystemTime;

pub struct FixedClock(pub SystemTime);

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}

#[derive(Default)]
pub struct InMemoryReplayGuard {
    event_ids: Mutex<BTreeSet<String>>,
}

#[async_trait]
impl ReplayGuard for InMemoryReplayGuard {
    async fn reserve(&self, event_id: &str) -> Result<ReplayReservation, ConnectorError> {
        let mut event_ids = self.event_ids.lock().expect("replay guard lock poisoned");
        if event_ids.insert(event_id.to_string()) {
            Ok(ReplayReservation::New)
        } else {
            Ok(ReplayReservation::Duplicate)
        }
    }

    async fn release(&self, event_id: &str) -> Result<(), ConnectorError> {
        self.event_ids
            .lock()
            .expect("replay guard lock poisoned")
            .remove(event_id);
        Ok(())
    }
}

#[derive(Default)]
pub struct RecordingEventSink {
    events: Mutex<Vec<InboundEvent>>,
}

impl RecordingEventSink {
    pub fn events(&self) -> Vec<InboundEvent> {
        self.events
            .lock()
            .expect("event sink lock poisoned")
            .clone()
    }
}

#[async_trait]
impl EventSink for RecordingEventSink {
    async fn submit(&self, event: InboundEvent) -> Result<(), ConnectorError> {
        self.events
            .lock()
            .expect("event sink lock poisoned")
            .push(event);
        Ok(())
    }
}
