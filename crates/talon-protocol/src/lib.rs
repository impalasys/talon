// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Internal protobuf messages and serialization shared by Talon services.

pub mod serialization;

pub mod generated {
    pub mod config {
        tonic::include_proto!("talon.config");
    }
    pub mod data {
        tonic::include_proto!("talon.data");
    }
    pub mod harness {
        tonic::include_proto!("talon.harness");
    }
    pub mod external {
        tonic::include_proto!("talon.external");
    }
    pub mod events {
        tonic::include_proto!("talon.events");
    }
    pub mod resources {
        tonic::include_proto!("talon.resources");
    }
    pub mod proto {
        tonic::include_proto!("talon.v1");
    }
    pub mod worker_proto {
        tonic::include_proto!("talon.worker.v1");
    }
}

pub mod proto {
    pub use super::generated::proto::*;
}

pub mod data {
    pub use super::generated::data::*;
}

pub mod data_proto {
    pub use super::generated::data::*;
}

pub mod harness_proto {
    pub use super::generated::harness::*;
}

pub mod external_proto {
    pub use super::generated::external::*;
}

pub mod resources_proto {
    pub use super::generated::resources::*;
}

pub mod worker_proto {
    pub use super::generated::worker_proto::*;
}

pub mod manifests {
    pub use super::resources_proto::*;
    pub type ObjectMeta = super::resources_proto::ResourceMeta;
}

pub mod protobuf_value {
    pub use prost_types::{value, ListValue, Value};
}

pub use generated::events;

pub mod manifest;
pub mod resource_model;
pub mod workflow_validation;
