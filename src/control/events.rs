// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub mod data {
    pub use crate::gateway::rpc::data_proto::*;
}

pub mod proto {
    tonic::include_proto!("talon.events");
}

pub use proto::*;
