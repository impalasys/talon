// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

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
        pub mod gateway {
            include!("generated/talon.gateway.rs");
        }
    }
}

pub use generated::talon::*;
