// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub mod cli;
pub mod control;
pub use talon_gateway::gateway;
pub use talon_gateway::require_auth;
pub use talon_harness::harness;
#[doc(hidden)]
pub use talon_monty as monty_subprocess;
pub mod worker;
pub use crate::control::security::encryption::SecurityProvider;
pub use crate::harness::executor::{
    AgentExecutor, CaptureSink, EncryptedResult, ExecutionContext, ExecutionSink, NullSink,
    RpcMessage, RpcRequest, RpcResponse, Task, TaskResult, TaskStatus,
};
pub use crate::harness::knowledge::{KnowledgeBook, KvKnowledgeBook};

pub use talon_test_support as test_support;
