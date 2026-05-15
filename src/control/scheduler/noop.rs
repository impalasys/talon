// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;

use super::{ScheduleWakeupRequest, ScheduledWakeup, SchedulerBackend};

#[derive(Default)]
pub struct NoopSchedulerBackend;

#[async_trait::async_trait]
impl SchedulerBackend for NoopSchedulerBackend {
    async fn schedule(&self, _req: ScheduleWakeupRequest) -> Result<ScheduledWakeup> {
        Ok(ScheduledWakeup::default())
    }

    async fn cancel(&self, _handle: &str) -> Result<()> {
        Ok(())
    }
}
