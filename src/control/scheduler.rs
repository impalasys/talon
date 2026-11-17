// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[cfg(feature = "aws")]
mod aws_eventbridge;
mod cloud_tasks;
mod local_postgres;
mod local_sqlite;
mod noop;

#[cfg(feature = "aws")]
pub use aws_eventbridge::AwsEventBridgeSchedulerBackend;
pub use cloud_tasks::CloudTasksSchedulerBackend;
pub use local_postgres::LocalPostgresSchedulerBackend;
pub use local_sqlite::LocalSqliteSchedulerBackend;
pub use noop::NoopSchedulerBackend;

pub use talon_contracts::scheduler::*;
