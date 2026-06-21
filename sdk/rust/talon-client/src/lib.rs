// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

mod client;

pub mod generated {
    pub mod talon {
        pub mod config {
            include!("generated/talon.config.rs");
        }
        pub mod data {
            include!("generated/talon.data.rs");
        }
        pub mod harness {
            include!("generated/talon.harness.rs");
        }
        pub mod resources {
            include!("generated/talon.resources.rs");
        }
        pub mod events {
            include!("generated/talon.events.rs");
        }
        pub mod v1 {
            include!("generated/talon.v1.rs");
        }
    }
}

pub use client::{
    GatewayClientOptions, GatewayTransport, GrpcWebTalonClient, NativeTalonClient, TalonClient,
    TalonClientset,
};
pub use generated::talon::*;
