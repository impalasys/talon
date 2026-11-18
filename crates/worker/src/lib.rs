// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub(crate) mod talon;

pub mod worker;
pub use talon::contracts::{
    rpc::{RpcMessage, RpcRequest, RpcResponse},
    EncryptedResult, Task, TaskResult, TaskStatus,
};
pub use talon::security::SecurityProvider;
#[cfg(test)]
pub use talon::test_support;
pub use worker::*;
