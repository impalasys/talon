mod cloud_tasks;
mod local_postgres;
mod noop;

pub use cloud_tasks::CloudTasksSchedulerBackend;
pub use local_postgres::LocalPostgresSchedulerBackend;
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
