// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

mod cf_alarms;
mod cloud_tasks;
mod local_postgres;
mod local_sqlite;
mod noop;

pub use cf_alarms::CfAlarmsSchedulerBackend;
pub use cloud_tasks::CloudTasksSchedulerBackend;
pub use local_postgres::LocalPostgresSchedulerBackend;
pub use local_sqlite::LocalSqliteSchedulerBackend;
pub use noop::NoopSchedulerBackend;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const SCHEDULER_AUTH_HEADER: &str = "X-Talon-Scheduler-Token";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleWakeupRequest {
    pub namespace: String,
    pub schedule_id: String,
    pub revision: u64,
    pub fire_at: DateTime<Utc>,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledWakeup {
    pub handle: Option<String>,
    pub armed: bool,
}

#[async_trait::async_trait]
pub trait SchedulerBackend: Send + Sync {
    async fn schedule(&self, req: ScheduleWakeupRequest) -> Result<ScheduledWakeup>;
    async fn cancel(&self, handle: &str) -> Result<()>;
}
