// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub mod context_budget;
pub mod history;
pub mod rpc;
pub mod runtime;
pub mod task;

pub use context_budget::{compact_history_for_llm, tool_result_preview, ContextBudget};
pub use history::session_message_to_loop_messages;
pub use rpc::{RpcMessage, RpcRequest, RpcResponse};
pub use runtime::{
    tool_result_loop_message, AgentEvent, AgentExecutor, CaptureSink, ContextAssembler,
    ExecutionContext, ExecutionSink, LoopMessage, NullSink, RegisteredMcpTool,
};
pub use task::{EncryptedResult, Task, TaskResult, TaskStatus};
