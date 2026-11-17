// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub mod worker;
pub use talon_contracts::{
    rpc::{RpcMessage, RpcRequest, RpcResponse},
    EncryptedResult, Task, TaskResult, TaskStatus,
};
pub use talon_security::SecurityProvider;
#[cfg(test)]
pub use talon_test_support as test_support;
pub use worker::*;
