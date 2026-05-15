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
    use crate::config::Config;
    use crate::control::{
        events::{MessageDirection, SessionMessageEvent},
        scheduler::NoopSchedulerBackend, ControlPlane, KeyValueStore, MessagePublisher,
        ProtoKeyValueStoreExt,
    };
    use crate::core::executor::ExecutionSink;
    use crate::gateway::rpc::{manifests, models};
    use crate::worker::{
        mcp_registry::McpRegistry, scheduler_auth::SchedulerRequestAuthenticator,
        WorkerEventHandler,
    };
    use async_trait::async_trait;
    use futures::stream;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;
    use serde_json::Value;
    use std::sync::Mutex;
    use tokio::sync::Mutex as AsyncMutex;
    use tokio_util::sync::CancellationToken;

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

    #[derive(Default)]
    struct MockKvStore {
        data: AsyncMutex<HashMap<(String, String), Vec<u8>>>,
    }

    #[async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, ns: &str, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self
                .data
                .lock()
                .await
                .get(&(ns.to_string(), key.to_string()))
                .cloned())
        }

        async fn set(&self, ns: &str, key: &str, value: &[u8]) -> anyhow::Result<()> {
            self.data
                .lock()
                .await
                .insert((ns.to_string(), key.to_string()), value.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            ns: &str,
            key: &str,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let full_key = (ns.to_string(), key.to_string());
            let current = data.get(&full_key).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(full_key, value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, ns: &str, key: &str) -> anyhow::Result<()> {
            self.data.lock().await.remove(&(ns.to_string(), key.to_string()));
            Ok(())
        }

        async fn list_keys(&self, ns: &str, prefix: &str) -> anyhow::Result<Vec<String>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|(stored_ns, key)| {
                    (stored_ns == ns && key.starts_with(prefix)).then(|| key.clone())
                })
                .collect::<Vec<_>>();
            keys.sort();
            Ok(keys)
        }
    }

    #[derive(Default)]
    struct MockPubSub;

    #[async_trait]
    impl MessagePublisher for MockPubSub {
        async fn publish(&self, _topic: &str, _message: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            Ok(Box::pin(stream::empty()))
        }
    }

    fn handler_with_kv(kv: Arc<MockKvStore>) -> WorkerEventHandler {
        WorkerEventHandler {
            cp: Arc::new(ControlPlane {
                kv,
                pubsub: Arc::new(MockPubSub),
                scheduler: Arc::new(NoopSchedulerBackend),
            }),
            config: Arc::new(Config {
                providers: HashMap::from([(
                    "mock".to_string(),
                    crate::config::ProviderConfig { config: None },
                )]),
                default_provider: "mock".to_string(),
                ..Config::default()
            }),
            mcp_registry: Arc::new(McpRegistry::new()),
            scheduler_authenticator: Arc::new(SchedulerRequestAuthenticator::deny_all()),
            session_cancellations: Arc::new(AsyncMutex::new(HashMap::new())),
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

    #[tokio::test]
    async fn execute_with_panic_boundary_reports_success_and_string_panic() {
        let sink = CaptureErrorSink::new();
        let ok = execute_with_panic_boundary(async { Ok("done".to_string()) }, &sink, "infra", "s1")
            .await
            .unwrap();
        assert_eq!(ok, SessionCompletionStatus::Completed);
        assert!(sink.errors().is_empty());

        let string_panic = execute_with_panic_boundary(
            async { std::panic::panic_any("owned panic".to_string()) },
            &sink,
            "infra",
            "s2",
        )
        .await
        .unwrap();
        assert_eq!(string_panic, SessionCompletionStatus::Panicked);
        assert!(sink.errors().iter().any(|err| err.contains("owned panic")));
    }

    #[tokio::test]
    async fn handle_session_control_cancels_registered_session() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv);
        let token = CancellationToken::new();
        handler
            .session_cancellations
            .lock()
            .await
            .insert("session-1".to_string(), token.clone());

        handler
            .handle_session_control(crate::control::events::SessionControlEvent {
                session_id: "session-1".to_string(),
                action: "stop_generation".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                timestamp: 0,
            })
            .await
            .expect("stop generation should succeed");

        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn handle_session_control_ignores_unknown_actions() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv);

        handler
            .handle_session_control(crate::control::events::SessionControlEvent {
                session_id: "session-1".to_string(),
                action: "noop".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                timestamp: 0,
            })
            .await
            .expect("unknown action should be ignored");
    }

    #[tokio::test]
    async fn handle_session_control_allows_missing_inflight_session() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv);

        handler
            .handle_session_control(crate::control::events::SessionControlEvent {
                session_id: "missing".to_string(),
                action: "stop_generation".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                timestamp: 0,
            })
            .await
            .expect("missing inflight session should be ignored");
    }

    #[tokio::test]
    async fn release_session_lock_sets_session_back_to_idle() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv.clone());
        let session = models::Session {
            id: "session-1".to_string(),
            agent: "assistant".to_string(),
            ns: "conic:test".to_string(),
            status: "PROCESSING".to_string(),
            created_at: 0,
            last_active: 123,
            metadata: HashMap::new(),
            labels: HashMap::new(),
        };
        kv.set_msg(
            "conic:test",
            &crate::control::keys::session("assistant", "session-1"),
            &session,
        )
        .await
        .expect("session should persist");

        handler
            .release_session_lock("conic:test", "assistant", "session-1")
            .await;

        let updated = kv
            .get_msg::<models::Session>(
                "conic:test",
                &crate::control::keys::session("assistant", "session-1"),
            )
            .await
            .expect("session should load")
            .expect("session should exist");
        assert_eq!(updated.status, "IDLE");
    }

    #[tokio::test]
    async fn handle_session_message_runs_end_to_end_and_releases_lock() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv.clone());
        let spec = manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: "assist".to_string(),
            mcp_server_refs: Vec::new(),
            capabilities: HashMap::new(),
        };

        kv.set_msg(
            "conic:test",
            &crate::control::keys::agent("assistant"),
            &models::Agent {
                name: "assistant".to_string(),
                ns: "conic:test".to_string(),
                definition: None,
                effective_spec: Some(spec),
                template_deps: Vec::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            "conic:test",
            &crate::control::keys::session("assistant", "session-1"),
            &models::Session {
                id: "session-1".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 0,
                last_active: 123,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();

        handler
            .handle_session_message(SessionMessageEvent {
                ns: "conic:test".to_string(),
                agent: "assistant".to_string(),
                session_id: "session-1".to_string(),
                message_id: "user-1".to_string(),
                direction: MessageDirection::Inbound as i32,
                message: "hello".to_string(),
                timestamp: 1,
            })
            .await
            .unwrap();

        let session = kv
            .get_msg::<models::Session>(
                "conic:test",
                &crate::control::keys::session("assistant", "session-1"),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.status, "IDLE");
        assert!(handler
            .session_cancellations
            .lock()
            .await
            .get("session-1")
            .is_none());

        let message_keys = kv
            .list_keys(
                "conic:test",
                &crate::control::keys::session_message_prefix("assistant", "session-1"),
            )
            .await
            .unwrap();
        let prefix = crate::control::keys::session_message_prefix("assistant", "session-1");
        let mut reply = None;
        for key in message_keys {
            if key.strip_prefix(&prefix).unwrap_or(&key).contains('/') {
                continue;
            }
            if let Some(message) = kv
                .get_msg::<models::SessionMessage>("conic:test", &key)
                .await
                .unwrap()
            {
                reply = Some(message);
                break;
            }
        }
        let reply = reply.expect("assistant reply should be stored");
        assert_eq!(reply.role, 2);
    }
}
