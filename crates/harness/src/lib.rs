// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub(crate) mod talon;

pub mod harness;
pub use harness::executor::{
    AgentExecutor, CaptureSink, EncryptedResult, ExecutionContext, ExecutionSink, NullSink,
    RpcMessage, RpcRequest, RpcResponse, Task, TaskResult, TaskStatus,
};
pub use harness::knowledge::{KnowledgeBook, KvKnowledgeBook};
pub use harness::*;
pub use talon::monty as monty_subprocess;
pub use talon::security::SecurityProvider;
#[cfg(test)]
pub use talon::test_support;
