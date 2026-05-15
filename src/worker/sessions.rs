// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use futures::FutureExt;
use std::panic::AssertUnwindSafe;

use super::runtime::AgentRuntime;
use super::sink::PubSubSessionSink;
use super::WorkerEventHandler;
use crate::control::events::SessionMessageEvent;
use crate::control::ProtoKeyValueStoreExt;
use crate::core::executor::ExecutionSink;
use crate::gateway::rpc::models;
use tokio_util::sync::CancellationToken;

async fn execute_with_panic_boundary<F>(
    future: F,
    sink: &dyn ExecutionSink,
    agent: &str,
    session_id: &str,
) -> Result<SessionCompletionStatus>
where
    F: std::future::Future<Output = Result<String>>,
{
    match AssertUnwindSafe(future).catch_unwind().await {
        Ok(Ok(_)) => Ok(SessionCompletionStatus::Completed),
        Ok(Err(e)) => {
            tracing::error!(agent = %agent, "Execution failed: {}", e);
            sink.on_error(&format!("Error: {}", e)).await;
            Ok(SessionCompletionStatus::Errored)
        }
        Err(panic) => {
            let panic_msg = if let Some(msg) = panic.downcast_ref::<&str>() {
                (*msg).to_string()
            } else if let Some(msg) = panic.downcast_ref::<String>() {
                msg.clone()
            } else {
                "unknown panic".to_string()
            };
            tracing::error!(
                agent = %agent,
                session = %session_id,
                "Execution panicked: {}",
                panic_msg
            );
            sink.on_error(&format!("Error: execution panicked: {}", panic_msg))
                .await;
            Ok(SessionCompletionStatus::Panicked)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionCompletionStatus {
    Completed,
    Errored,
    Panicked,
}

impl WorkerEventHandler {
    pub async fn handle_session_message(&self, event: SessionMessageEvent) -> Result<()> {
        tracing::info!(
            agent = %event.agent,
            session = %event.session_id,
            "Handling session message"
        );

        let ns = &event.ns;
        let cancellation_token = CancellationToken::new();
        self.session_cancellations
            .lock()
            .await
            .insert(event.session_id.clone(), cancellation_token.clone());
        let outcome = async {
            // Build the fully-resolved runtime (spec, history, LLM, tools, knowledge)
            let mut runtime = AgentRuntime::build(
                ns,
                &event.agent,
                &event.session_id,
                &self.cp,
                &self.config,
                &self.mcp_registry,
            )
            .await?;

            // Pre-allocate the assistant reply slot in KV
            let reply_msg_id = uuid::Uuid::now_v7().to_string();
            let reply_msg_key = crate::control::keys::session_message(
                &event.agent,
                &event.session_id,
                &reply_msg_id,
            );
            let _ = self
                .cp
                .kv
                .set_msg(
                    ns,
                    &reply_msg_key,
                    &models::SessionMessage {
                        id: reply_msg_id.clone(),
                        role: 2, // ASSISTANT
                        content: String::new(),
                        created_at: chrono::Utc::now().timestamp_micros(),
                        labels: std::collections::HashMap::new(),
                    },
                )
                .await;

            let sink = PubSubSessionSink::new(
                self.cp.kv.clone(),
                self.cp.pubsub.clone(),
                event.ns.clone(),
                event.session_id.clone(),
                event.agent.clone(),
                reply_msg_id,
                reply_msg_key,
            );

            execute_with_panic_boundary(
                runtime
                    .executor
                    .execute(
                        &mut runtime.context,
                        &event.message,
                        &sink,
                        Some(&cancellation_token),
                    ),
                &sink,
                &event.agent,
                &event.session_id,
            )
            .await
            .map(|status| (status, sink.summary()))
        }
        .await;

        self.session_cancellations
            .lock()
            .await
            .remove(&event.session_id);
        self.release_session_lock(ns, &event.agent, &event.session_id)
            .await;

        if let Ok((status, summary)) = &outcome {
            tracing::info!(
                agent = %event.agent,
                session = %event.session_id,
                status = ?status,
                duration_ms = summary.duration_ms,
                input_token_chunks = summary.input_token_chunks,
                input_token_chars = summary.input_token_chars,
                published_token_batches = summary.published_token_batches,
                published_token_chars = summary.published_token_chars,
                tool_calls = summary.tool_calls,
                tool_results = summary.tool_results,
                "Session message completed"
            );
        }

        outcome.map(|_| ())
    }

    pub async fn handle_session_control(
        &self,
        event: crate::control::events::SessionControlEvent,
    ) -> Result<()> {
        if event.action != "stop_generation" {
            tracing::warn!(
                session = %event.session_id,
                action = %event.action,
                "Ignoring unknown session control action"
            );
            return Ok(());
        }

        let cancellations = self.session_cancellations.lock().await;
        if let Some(token) = cancellations.get(&event.session_id) {
            tracing::info!(
                namespace = %event.ns,
                agent = %event.agent,
                session = %event.session_id,
                "Cancelling in-flight session generation"
            );
            token.cancel();
        } else {
            tracing::info!(
                namespace = %event.ns,
                agent = %event.agent,
                session = %event.session_id,
                "Session stop requested, but no in-flight generation was registered"
            );
        }
        Ok(())
    }

    async fn release_session_lock(&self, ns: &str, agent_id: &str, session_id: &str) {
        let key = crate::control::keys::session(agent_id, session_id);
        if let Ok(Some(mut session)) = self.cp.kv.get_msg::<models::Session>(ns, &key).await {
            session.status = "IDLE".to_string();
            let _ = self.cp.kv.set_msg(ns, &key, &session).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{execute_with_panic_boundary, SessionCompletionStatus};
    use crate::core::executor::ExecutionSink;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::sync::Mutex;

    struct CaptureErrorSink {
        errors: Mutex<Vec<String>>,
    }

    impl CaptureErrorSink {
        fn new() -> Self {
            Self {
                errors: Mutex::new(Vec::new()),
            }
        }

        fn errors(&self) -> Vec<String> {
            self.errors.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ExecutionSink for CaptureErrorSink {
        async fn on_token(&self, _: &str) {}
        async fn on_tool_call(&self, _: &str, _: &str, _: &Value) {}
        async fn on_tool_result(&self, _: &str, _: &str, _: &str) {}
        async fn on_done(&self, _: &str) {}
        async fn on_error(&self, err: &str) {
            self.errors.lock().unwrap().push(err.to_string());
        }
    }

    #[tokio::test]
    async fn execute_with_panic_boundary_reports_panic_to_sink() {
        let sink = CaptureErrorSink::new();

        let result = execute_with_panic_boundary(
            async { panic!("unicode excerpt panic") },
            &sink,
            "infra",
            "session-1",
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(
            sink.errors(),
            vec!["Error: execution panicked: unicode excerpt panic".to_string()]
        );
        assert_eq!(result.unwrap(), SessionCompletionStatus::Panicked);
    }

    #[tokio::test]
    async fn execute_with_panic_boundary_reports_regular_error_to_sink() {
        let sink = CaptureErrorSink::new();

        let result = execute_with_panic_boundary(
            async { Err(anyhow::anyhow!("tool failed")) },
            &sink,
            "infra",
            "session-1",
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(sink.errors(), vec!["Error: tool failed".to_string()]);
        assert_eq!(result.unwrap(), SessionCompletionStatus::Errored);
    }
}
