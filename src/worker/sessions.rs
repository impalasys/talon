// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use futures::FutureExt;
use prost::Message;
use std::panic::AssertUnwindSafe;

use super::runtime::AgentRuntime;
use super::sink::PubSubSessionSink;
use super::WorkerEventHandler;
use crate::control::events::SessionMessageEvent;
use crate::control::ProtoKeyValueStoreExt;
use crate::gateway::rpc::data_proto;
use crate::harness::executor::ExecutionSink;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

const MAX_SESSION_RELEASE_CAS_RETRIES: usize = 8;
const SESSION_RELEASE_CAS_BACKOFF_MS: u64 = 10;

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
            tracing::error!(agent = %agent, error = %format!("{:#}", e), "Execution failed");
            sink.on_error(&format!("Error: {:#}", e)).await;
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
    #[tracing::instrument(
        name = "WorkerEventHandler.handle_session_message",
        skip_all,
        fields(
            namespace = %event.ns,
            agent = %event.agent,
            session = %event.session_id,
            message_chars = event.message.len(),
        )
    )]
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
        // Pre-allocate the assistant reply slot before runtime construction so
        // provider/config errors can be recorded in the session history.
        let reply_msg_id = uuid::Uuid::now_v7().to_string();
        let reply_msg_key = crate::control::keys::session_message(
            ns,
            &event.agent,
            &event.session_id,
            &reply_msg_id,
        );
        let _ = self
            .cp
            .kv
            .set_msg(
                &reply_msg_key,
                &data_proto::SessionMessage {
                    id: reply_msg_id.clone(),
                    role: data_proto::MessageRole::RoleAssistant as i32,
                    created_at: chrono::Utc::now().timestamp_micros(),
                    labels: std::collections::HashMap::new(),
                    parts: Vec::new(),
                },
            )
            .instrument(tracing::info_span!("WorkerEventHandler.create_reply_slot"))
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

        let outcome = async {
            if is_acp_agent(&self.cp, ns, &event.agent).await? {
                let runtime = match crate::harness::acp::AcpAgentRuntime::build(
                    ns,
                    &event.agent,
                    &event.session_id,
                    &self.cp,
                    &self.config,
                )
                .instrument(tracing::info_span!("AcpAgentRuntime.build"))
                .await
                {
                    Ok(runtime) => runtime,
                    Err(err) => {
                        tracing::error!(
                            agent = %event.agent,
                            session = %event.session_id,
                            "Failed to build ACP agent runtime: {}",
                            err
                        );
                        sink.on_error(&format!("Error: {}", err)).await;
                        return Ok((SessionCompletionStatus::Errored, sink.summary()));
                    }
                };

                return execute_with_panic_boundary(
                    runtime.execute(&event.message, &sink, Some(&cancellation_token)),
                    &sink,
                    &event.agent,
                    &event.session_id,
                )
                .instrument(tracing::info_span!("AcpAgentRuntime.execute_session"))
                .await
                .map(|status| (status, sink.summary()));
            }

            // Build the fully-resolved runtime (spec, history, LLM, tools, knowledge)
            let mut runtime = match AgentRuntime::build(
                ns,
                &event.agent,
                &event.session_id,
                &self.cp,
                &self.config,
                &self.mcp_registry,
            )
            .instrument(tracing::info_span!("AgentRuntime.build"))
            .await
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    tracing::error!(
                        agent = %event.agent,
                        session = %event.session_id,
                        "Failed to build agent runtime: {}",
                        err
                    );
                    sink.on_error(&format!("Error: {}", err)).await;
                    return Ok((SessionCompletionStatus::Errored, sink.summary()));
                }
            };

            execute_with_panic_boundary(
                runtime.executor.execute(
                    &mut runtime.context,
                    &event.message,
                    &sink,
                    Some(&cancellation_token),
                ),
                &sink,
                &event.agent,
                &event.session_id,
            )
            .instrument(tracing::info_span!("WorkerEventHandler.execute_session"))
            .await
            .map(|status| (status, sink.summary()))
        }
        .await;

        self.session_cancellations
            .lock()
            .await
            .remove(&event.session_id);
        let completion_status = outcome
            .as_ref()
            .map(|(status, _)| *status)
            .unwrap_or(SessionCompletionStatus::Errored);
        self.release_session_lock(
            ns,
            &event.agent,
            &event.session_id,
            event.timestamp,
            completion_status,
        )
        .instrument(tracing::info_span!(
            "WorkerEventHandler.release_session_lock"
        ))
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

    async fn release_session_lock(
        &self,
        ns: &str,
        agent_id: &str,
        session_id: &str,
        expected_last_active: i64,
        completion_status: SessionCompletionStatus,
    ) {
        let key = crate::control::keys::session(ns, agent_id, session_id);
        let mut released_session = None;
        let mut last_error = None;
        for _ in 0..MAX_SESSION_RELEASE_CAS_RETRIES {
            let current = match self.cp.kv.get(&key).await {
                Ok(Some(current)) => current,
                Ok(None) => return,
                Err(err) => {
                    last_error = Some(err.to_string());
                    break;
                }
            };
            let mut session = match data_proto::Session::decode(current.as_slice()) {
                Ok(session) => session,
                Err(err) => {
                    last_error = Some(err.to_string());
                    break;
                }
            };
            if session.status != "PROCESSING" || session.last_active != expected_last_active {
                return;
            }
            session.status = match completion_status {
                SessionCompletionStatus::Completed => "IDLE",
                SessionCompletionStatus::Errored | SessionCompletionStatus::Panicked => "ERROR",
            }
            .to_string();
            let updated = session.encode_to_vec();
            match self
                .cp
                .kv
                .compare_and_swap(&key, Some(current.as_slice()), &updated)
                .await
            {
                Ok(true) => {
                    released_session = Some(session);
                    break;
                }
                Ok(false) => {
                    let jitter = rand::random::<u64>() % (SESSION_RELEASE_CAS_BACKOFF_MS / 2 + 1);
                    tokio::time::sleep(std::time::Duration::from_millis(
                        SESSION_RELEASE_CAS_BACKOFF_MS + jitter,
                    ))
                    .await;
                    continue;
                }
                Err(err) => {
                    last_error = Some(err.to_string());
                    break;
                }
            }
        }
        let Some(session) = released_session else {
            tracing::error!(
                namespace = %ns,
                agent = %agent_id,
                session = %session_id,
                error = last_error.as_deref().unwrap_or("compare-and-swap conflict"),
                "failed to release session lock atomically"
            );
            return;
        };
        if let Err(err) =
            crate::worker::workflows::dispatch_workflow_from_session_labels(&self.cp, &session)
                .await
        {
            tracing::warn!(
                namespace = %ns,
                agent = %agent_id,
                session = %session_id,
                error = %err,
                "failed to dispatch workflow from completed child session"
            );
        }
    }
}

async fn is_acp_agent(cp: &crate::control::ControlPlane, ns: &str, agent_id: &str) -> Result<bool> {
    let store = crate::control::resources::ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    let Some(agent) = store.get_agent(ns, agent_id).await? else {
        return Ok(false);
    };
    Ok(agent
        .spec
        .as_ref()
        .and_then(|spec| spec.runtime.as_ref())
        .map(|runtime| runtime.kind == "acp")
        .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::{execute_with_panic_boundary, SessionCompletionStatus};
    use crate::control::config::{proto, Config, ProviderConfig, Secret};
    use crate::control::{
        events::{MessageDirection, SessionMessageEvent},
        keys::{ResourceKey, ResourceList},
        scheduler::NoopSchedulerBackend,
        ControlPlane, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::{data_proto, manifests, resources_proto};
    use crate::harness::executor::ExecutionSink;
    use crate::worker::{
        mcp_registry::McpRegistry, scheduler_auth::SchedulerRequestAuthenticator,
        WorkerEventHandler,
    };
    use async_trait::async_trait;
    use axum::{routing::post, Json, Router};
    use futures::stream;
    use serde_json::json;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::Mutex;
    use tokio::net::TcpListener;
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
        async fn on_reasoning(&self, _: &str) {}
        async fn on_tool_call(&self, _: &str, _: &str, _: &Value) {}
        async fn on_tool_result(&self, _: &str, _: &str, _: &str) {}
        async fn on_usage(&self, _: &crate::harness::llm::ChatUsage) {}
        async fn on_done(&self, _: &str) {}
        async fn on_error(&self, err: &str) {
            self.errors.lock().unwrap().push(err.to_string());
        }
    }

    async fn put_agent_resource(
        kv: Arc<MockKvStore>,
        namespace: &str,
        name: &str,
        spec: resources_proto::AgentSpec,
    ) {
        let store = crate::control::resources::ResourceStore::new(
            kv,
            Arc::new(crate::test_support::RecordingPubSub::default()),
        );
        store
            .upsert(
                namespace,
                resources_proto::Resource {
                    api_version: "talon.impalasys.com/v1".to_string(),
                    kind: "Agent".to_string(),
                    metadata: Some(resources_proto::ResourceMeta {
                        name: name.to_string(),
                        namespace: namespace.to_string(),
                        labels: HashMap::new(),
                        annotations: HashMap::new(),
                        owner_references: Vec::new(),
                        finalizers: Vec::new(),
                        generation: 0,
                        resource_version: String::new(),
                        uid: String::new(),
                        deletion_timestamp: None,
                    }),
                    spec: Some(resources_proto::ResourceSpec {
                        kind: Some(resources_proto::resource_spec::Kind::Agent(spec)),
                    }),
                    status: Some(resources_proto::ResourceStatus {
                        kind: Some(resources_proto::resource_status::Kind::Agent(
                            resources_proto::AgentStatus {
                                observed_generation: 0,
                                phase: String::new(),
                                conditions: Vec::new(),
                                last_session_id: None,
                            },
                        )),
                    }),
                },
            )
            .await
            .unwrap();
    }

    #[derive(Default)]
    struct MockKvStore {
        data: AsyncMutex<HashMap<ResourceKey, Vec<u8>>>,
    }

    #[async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, key: &ResourceKey) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.data.lock().await.get(key).cloned())
        }

        async fn set(&self, key: &ResourceKey, value: &[u8]) -> anyhow::Result<()> {
            self.data.lock().await.insert(key.clone(), value.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            key: &ResourceKey,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let current = data.get(key).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(key.clone(), value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, key: &ResourceKey) -> anyhow::Result<()> {
            self.data.lock().await.remove(key);
            Ok(())
        }

        async fn list_keys(&self, list: &ResourceList) -> anyhow::Result<Vec<ResourceKey>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|key| list.matches(key).then(|| key.clone()))
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

    fn handler_with_config(kv: Arc<MockKvStore>, config: Config) -> WorkerEventHandler {
        WorkerEventHandler {
            cp: Arc::new(ControlPlane {
                kv,
                pubsub: Arc::new(MockPubSub),
                scheduler: Arc::new(NoopSchedulerBackend),
                objects: crate::control::object_store::default_object_store(),
            }),
            config: Arc::new(config),
            mcp_registry: Arc::new(McpRegistry::new()),
            scheduler_authenticator: Arc::new(SchedulerRequestAuthenticator::deny_all()),
            session_cancellations: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }

    fn handler_with_kv(kv: Arc<MockKvStore>) -> WorkerEventHandler {
        handler_with_config(
            kv,
            Config {
                providers: HashMap::from([(
                    "novita".to_string(),
                    ProviderConfig {
                        config: Some(proto::llm_provider_config::Config::OpenaiCompatible(
                            proto::GenericConfig {
                                name: "novita".to_string(),
                                base_url: "https://unused.example.com".to_string(),
                                model: "test-model".to_string(),
                                api_key: Some(Secret {
                                    source: Some(proto::secret::Source::Plain(
                                        "test-key".to_string(),
                                    )),
                                }),
                            },
                        )),
                    },
                )]),
                default_provider: "novita".to_string(),
                ..Config::default()
            },
        )
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
        let ok =
            execute_with_panic_boundary(async { Ok("done".to_string()) }, &sink, "infra", "s1")
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
        let session = data_proto::Session {
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
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &session,
        )
        .await
        .expect("session should persist");

        handler
            .release_session_lock(
                "conic:test",
                "assistant",
                "session-1",
                123,
                SessionCompletionStatus::Completed,
            )
            .await;

        let updated = kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .expect("session should load")
            .expect("session should exist");
        assert_eq!(updated.status, "IDLE");
    }

    #[tokio::test]
    async fn release_session_lock_does_not_release_stolen_lock() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv.clone());
        let session = data_proto::Session {
            id: "session-1".to_string(),
            agent: "assistant".to_string(),
            ns: "conic:test".to_string(),
            status: "PROCESSING".to_string(),
            created_at: 0,
            last_active: 456,
            metadata: HashMap::new(),
            labels: HashMap::new(),
        };
        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &session,
        )
        .await
        .expect("session should persist");

        handler
            .release_session_lock(
                "conic:test",
                "assistant",
                "session-1",
                123,
                SessionCompletionStatus::Completed,
            )
            .await;

        let updated = kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .expect("session should load")
            .expect("session should exist");
        assert_eq!(updated.status, "PROCESSING");
        assert_eq!(updated.last_active, 456);
    }

    #[tokio::test]
    async fn handle_session_message_persists_runtime_build_error_and_keeps_user_message() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_config(
            kv.clone(),
            Config {
                providers: HashMap::from([(
                    "openai".to_string(),
                    ProviderConfig {
                        config: Some(proto::llm_provider_config::Config::Openai(
                            proto::OpenAiConfig {
                                model: "gpt-test".to_string(),
                                api_key: None,
                                org_id: String::new(),
                            },
                        )),
                    },
                )]),
                default_provider: "openai".to_string(),
                ..Config::default()
            },
        );
        let spec = manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: "assist".to_string(),
            mcp_server_refs: Vec::new(),
            capabilities: HashMap::new(),
            a2a: None,
            runtime: None,
        };

        put_agent_resource(kv.clone(), "conic:test", "assistant", spec).await;
        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &data_proto::Session {
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
        kv.set_msg(
            &crate::control::keys::session_message(
                "conic:test",
                "assistant",
                "session-1",
                "user-1",
            ),
            &data_proto::SessionMessage {
                id: "user-1".to_string(),
                role: data_proto::MessageRole::RoleUser as i32,
                created_at: 1,
                labels: HashMap::new(),
                parts: vec![data_proto::SessionMessagePart {
                    id: "000000".to_string(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: "operator prompt".to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                    created_at: 1,
                    object: None,
                }],
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
                message: "operator prompt".to_string(),
                timestamp: 123,
            })
            .await
            .expect("runtime build errors should be persisted and acked");

        let session = kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.status, "ERROR");

        let message_keys = kv
            .list_keys(&crate::control::keys::session_message_prefix(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap();
        let mut user_message = None;
        let mut error_message = None;
        for key in message_keys {
            if let Some(message) = kv
                .get_msg::<data_proto::SessionMessage>(&key)
                .await
                .unwrap()
            {
                if message.role == data_proto::MessageRole::RoleUser as i32 {
                    user_message = Some(message);
                } else if message.role == data_proto::MessageRole::RoleAssistant as i32
                    && message.parts.iter().any(|part| {
                        part.part_type == data_proto::SessionMessagePartType::Error as i32
                    })
                {
                    error_message = Some(message);
                }
            }
        }

        let user_message = user_message.expect("operator message should remain persisted");
        assert_eq!(user_message.parts[0].content, "operator prompt");
        let error_message = error_message.expect("assistant error should be persisted");
        let error_part = error_message
            .parts
            .iter()
            .find(|part| part.part_type == data_proto::SessionMessagePartType::Error as i32)
            .expect("error part should exist");
        assert!(error_part
            .content
            .contains("OpenAI provider config is missing api_key"));
    }

    #[tokio::test]
    async fn handle_session_message_runs_end_to_end_and_releases_lock() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
        let app = Router::new().route(
            "/chat/completions",
            post(|| async {
                Json(json!({
                    "choices": [{
                        "message": {
                            "content": "assistant reply"
                        }
                    }]
                }))
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        unsafe {
            std::env::set_var("NOVITA_BASE_URL", format!("http://{addr}"));
        }

        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv.clone());
        let spec = manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: "assist".to_string(),
            mcp_server_refs: Vec::new(),
            capabilities: HashMap::new(),
            a2a: None,
            runtime: None,
        };

        put_agent_resource(kv.clone(), "conic:test", "assistant", spec).await;
        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &data_proto::Session {
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
                timestamp: 123,
            })
            .await
            .unwrap();

        let session = kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(
                "conic:test",
                "assistant",
                "session-1",
            ))
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
            .list_keys(&crate::control::keys::session_message_prefix(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap();
        let prefix =
            crate::control::keys::session_message_prefix("conic:test", "assistant", "session-1");
        let mut reply = None;
        for key in message_keys {
            if !prefix.matches(&key) {
                continue;
            }
            if let Some(message) = kv
                .get_msg::<data_proto::SessionMessage>(&key)
                .await
                .unwrap()
            {
                reply = Some(message);
                break;
            }
        }
        let reply = reply.expect("assistant reply should be stored");
        assert_eq!(reply.role, 2);

        unsafe {
            std::env::remove_var("NOVITA_BASE_URL");
        }
        server.abort();
    }
}
