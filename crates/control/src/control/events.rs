// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[allow(unused_imports)]
use crate::talon;

pub use talon::protocol::events::*;
pub use talon::protocol::generated::events as proto;
pub mod data {
    #[allow(unused_imports)]
    use crate::talon;

    pub use talon::protocol::data_proto::*;
}
