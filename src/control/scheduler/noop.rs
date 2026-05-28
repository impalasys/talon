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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn noop_scheduler_returns_default_wakeup_and_allows_cancel() {
        let backend = NoopSchedulerBackend;
        let wakeup = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "root".to_string(),
                schedule_id: "schedule".to_string(),
                revision: 1,
                fire_at: Utc::now(),
                payload: vec![1, 2, 3],
            })
            .await
            .expect("schedule should succeed");
        assert_eq!(wakeup, ScheduledWakeup::default());
        backend
            .cancel("ignored-handle")
            .await
            .expect("cancel should succeed");
    }
}
