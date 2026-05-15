// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub mod agents;
pub mod config;
pub mod connectors;
pub mod control;
pub mod core;
pub mod gateway;
pub mod knowledge;
pub mod llm;
pub mod manifest;
pub mod memory;
pub mod native_tools;
pub mod orchestrator;
pub mod scheduling;
pub mod security;
pub mod skills;
pub mod worker;
pub use crate::core::executor::{
    AgentExecutor, CaptureSink, ExecutionContext, ExecutionSink, NullSink,
};
pub use crate::core::rpc::{RpcMessage, RpcRequest, RpcResponse};
pub use crate::core::task::{EncryptedResult, Task, TaskResult, TaskStatus};
pub use crate::knowledge::{KnowledgeBook, KvKnowledgeBook};
pub use crate::security::encryption::SecurityProvider;

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, OnceLock};
    use tokio::sync::Mutex as AsyncMutex;

    pub(crate) fn env_mutex() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    pub(crate) fn async_env_mutex() -> &'static AsyncMutex<()> {
        static LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| AsyncMutex::new(()))
    }
}
