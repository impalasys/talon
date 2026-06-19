// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[cfg(not(feature = "bazel"))]
pub mod data {
    pub use crate::gateway::rpc::data_proto::*;
}

#[cfg(not(feature = "bazel"))]
pub mod proto {
    tonic::include_proto!("talon.events");
}

#[cfg(feature = "bazel")]
pub mod proto {
    pub use talon_events_proto::talon::events::*;
}

pub use proto::*;
