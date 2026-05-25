// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[cfg(test)]
mod tests {
    use crate::control::{
        events::{SessionStepEvent, StepType},
        keys,
        scheduler::NoopSchedulerBackend,
        topics, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::{manifests, models};
    use crate::gateway::rpc::{proto, GrpcGatewayHandler};
    use crate::gateway::server::Gateway;
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use futures::{stream, StreamExt};
    use prost::Message;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    struct FailingPubSub {
        fail_publish: bool,
        fail_subscribe: bool,
    }

    #[async_trait::async_trait]
    impl MessagePublisher for FailingPubSub {
        async fn publish(&self, _topic: &str, _message: &[u8]) -> anyhow::Result<()> {
            if self.fail_publish {
                anyhow::bail!("publish failed");
            }
            Ok(())
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            if self.fail_subscribe {
                anyhow::bail!("subscribe failed");
            }
            Ok(Box::pin(stream::empty()))
        }
    }

    #[derive(Default)]
    struct FailingKvStore {
        data: Mutex<HashMap<(String, String), Vec<u8>>>,
        fail_list_prefix: Option<String>,
        fail_get_key: Option<String>,
        fail_set_prefix: Option<String>,
        fail_delete_key: Option<String>,
        extra_list_keys: Vec<String>,
    }

    #[async_trait::async_trait]
    impl KeyValueStore for FailingKvStore {
        async fn get(&self, ns: &str, k: &str) -> anyhow::Result<Option<Vec<u8>>> {
            if self.fail_get_key.as_deref() == Some(k) {
                anyhow::bail!("get failed for {}", k);
            }
            Ok(self
                .data
                .lock()
                .await
                .get(&(ns.to_string(), k.to_string()))
                .cloned())
        }

        async fn set(&self, ns: &str, k: &str, v: &[u8]) -> anyhow::Result<()> {
            if self
                .fail_set_prefix
                .as_deref()
                .is_some_and(|prefix| k.starts_with(prefix))
            {
                anyhow::bail!("set failed for {}", k);
            }
            self.data
                .lock()
                .await
                .insert((ns.to_string(), k.to_string()), v.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            ns: &str,
            k: &str,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let key = (ns.to_string(), k.to_string());
            let current = data.get(&key).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(key, value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, ns: &str, k: &str) -> anyhow::Result<()> {
            if self.fail_delete_key.as_deref() == Some(k) {
                anyhow::bail!("delete failed for {}", k);
            }
            self.data
                .lock()
                .await
                .remove(&(ns.to_string(), k.to_string()));
            Ok(())
        }

        async fn list_keys(&self, ns: &str, p: &str) -> anyhow::Result<Vec<String>> {
            if self
                .fail_list_prefix
                .as_deref()
                .is_some_and(|prefix| p.starts_with(prefix))
            {
                anyhow::bail!("list failed for {}", p);
            }

            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|(stored_ns, key)| {
                    (stored_ns == ns && key.starts_with(p)).then(|| key.clone())
                })
                .collect::<Vec<_>>();
            keys.extend(self.extra_list_keys.iter().cloned());
            keys.sort();
            Ok(keys)
        }
    }

    fn setup_mock_gateway_handler(
        kv: Arc<MockKvStore>,
        pubsub: Arc<RecordingPubSub>,
    ) -> GrpcGatewayHandler {
        let gateway = Arc::new(Gateway::new(
            None,
            kv,
            pubsub,
            Arc::new(NoopSchedulerBackend),
        ));
        GrpcGatewayHandler { gateway }
    }

    fn setup_gateway_handler_with(
        kv: Arc<dyn KeyValueStore + Send + Sync>,
        pubsub: Arc<dyn MessagePublisher + Send + Sync>,
    ) -> GrpcGatewayHandler {
        let gateway = Arc::new(Gateway::new(
            None,
            kv,
            pubsub,
            Arc::new(NoopSchedulerBackend),
        ));
        GrpcGatewayHandler { gateway }
    }

    fn custom_agent_definition() -> manifests::AgentDefinition {
        manifests::AgentDefinition {
            source: Some(manifests::agent_definition::Source::CustomSpec(
                manifests::AgentSpec {
                    features: Vec::new(),
                    model_policy: Some(manifests::ModelPolicy {
                        profiles: vec![manifests::ModelProfile {
                            name: "default".to_string(),
                            model: Some(manifests::Model {
                                provider: "mock".to_string(),
                                name: "gpt-5".to_string(),
                                temperature: 0.0,
                                thinking: None,
                            }),
                        }],
                    }),
                    system_prompt: "You are helpful.".to_string(),
                    mcp_server_refs: Vec::new(),
                    capabilities: HashMap::new(),
                },
            )),
        }
    }

    async fn seed_agent(kv: &Arc<MockKvStore>, ns: &str, name: &str) {
        kv.set_msg(
            ns,
            &keys::agent(name),
            &models::Agent {
                name: name.to_string(),
                ns: ns.to_string(),
                definition: Some(custom_agent_definition()),
                effective_spec: None,
                template_deps: Vec::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_create_session_requires_existing_agent_and_persists_labels() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = setup_mock_gateway_handler(kv.clone(), pubsub.clone());

        let missing = handler
            .handle_create_session(tonic::Request::new(proto::CreateSessionRequest {
                agent: "missing".to_string(),
                ns: "default".to_string(),
                labels: HashMap::new(),
            }))
            .await
            .expect_err("missing agent should fail");
        assert_eq!(missing.code(), tonic::Code::NotFound);

        seed_agent(&kv, "default", "test-agent").await;

        let response = handler
            .handle_create_session(tonic::Request::new(proto::CreateSessionRequest {
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                labels: HashMap::from([("team".to_string(), "ops".to_string())]),
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(response.agent, "test-agent");
        assert_eq!(response.state, "ACTIVE");
        assert_eq!(response.labels.get("team").map(String::as_str), Some("ops"));

        let stored = kv
            .get_msg::<models::Session>(
                "default",
                &keys::session("test-agent", &response.session_id),
            )
            .await
            .unwrap()
            .expect("session should be stored");
        assert_eq!(stored.status, "IDLE");
        assert_eq!(stored.labels.get("team").map(String::as_str), Some("ops"));

        let published = pubsub.published.lock().await;
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, topics::RESOURCE_LIFECYCLE_TOPIC);
    }

    #[tokio::test]
    async fn test_create_session_surfaces_publish_failure() {
        let kv = Arc::new(MockKvStore::default());
        seed_agent(&kv, "default", "test-agent").await;
        let handler = setup_gateway_handler_with(
            kv,
            Arc::new(FailingPubSub {
                fail_publish: true,
                fail_subscribe: false,
            }),
        );

        let err = handler
            .handle_create_session(tonic::Request::new(proto::CreateSessionRequest {
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                labels: HashMap::new(),
            }))
            .await
            .expect_err("publish failure should surface");

        assert_eq!(err.code(), tonic::Code::Internal);
        assert!(err.message().contains("Failed to publish event"));
    }

    #[tokio::test]
    async fn test_create_session_surfaces_save_failure() {
        let session_key_prefix = keys::session("test-agent", "");
        let kv = Arc::new(FailingKvStore {
            data: Mutex::new(HashMap::from([(
                ("default".to_string(), keys::agent("test-agent")),
                models::Agent {
                    name: "test-agent".to_string(),
                    ns: "default".to_string(),
                    definition: Some(custom_agent_definition()),
                    effective_spec: None,
                    template_deps: Vec::new(),
                    labels: HashMap::new(),
                }
                .encode_to_vec(),
            )])),
            fail_list_prefix: None,
            fail_get_key: None,
            fail_set_prefix: Some(session_key_prefix),
            fail_delete_key: None,
            extra_list_keys: Vec::new(),
        });
        let handler = setup_gateway_handler_with(
            kv,
            Arc::new(FailingPubSub {
                fail_publish: false,
                fail_subscribe: false,
            }),
        );

        let err = handler
            .handle_create_session(tonic::Request::new(proto::CreateSessionRequest {
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                labels: HashMap::new(),
            }))
            .await
            .expect_err("save failure should surface");

        assert_eq!(err.code(), tonic::Code::Internal);
        assert!(err.message().contains("Failed to save session state"));
    }

    #[tokio::test]
    async fn test_get_session_skips_nested_missing_and_invalid_payloads() {
        let kv = Arc::new(MockKvStore::default());
        let handler = setup_mock_gateway_handler(kv.clone(), Arc::new(RecordingPubSub::default()));

        let ns = "default";
        let agent = "test-agent";
        let session_id = "session-1";
        let message_id = "msg-1";

        kv.set_msg(
            ns,
            &keys::session(agent, session_id),
            &models::Session {
                id: session_id.to_string(),
                agent: agent.to_string(),
                ns: ns.to_string(),
                status: "PROCESSING".to_string(),
                created_at: 100,
                last_active: 250,
                metadata: HashMap::new(),
                labels: HashMap::from([("env".to_string(), "test".to_string())]),
            },
        )
        .await
        .unwrap();

        kv.set_msg(
            ns,
            &keys::session_message(agent, session_id, message_id),
            &models::SessionMessage {
                id: message_id.to_string(),
                role: 2,
                content: "assistant reply".to_string(),
                created_at: 150,
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();

        kv.set(
            ns,
            &format!(
                "{}/nested",
                keys::session_message(agent, session_id, message_id)
            ),
            b"should-be-ignored",
        )
        .await
        .unwrap();
        kv.set(
            ns,
            &keys::session_message(agent, session_id, "msg-invalid"),
            b"not-protobuf",
        )
        .await
        .unwrap();
        kv.set(
            ns,
            &keys::session_message_step(agent, session_id, message_id, "step-valid"),
            &SessionStepEvent {
                session_id: session_id.to_string(),
                step_type: StepType::Token as i32,
                content: "hello".to_string(),
                timestamp: 200,
                agent: agent.to_string(),
                ns: ns.to_string(),
                message_id: message_id.to_string(),
                name: String::new(),
                payload_json: String::new(),
            }
            .encode_to_vec(),
        )
        .await
        .unwrap();
        kv.set(
            ns,
            &keys::session_message_step(agent, session_id, message_id, "step-invalid"),
            b"not-step",
        )
        .await
        .unwrap();

        let response = handler
            .handle_get_session(tonic::Request::new(proto::GetSessionRequest {
                session_id: session_id.to_string(),
                agent: agent.to_string(),
                ns: ns.to_string(),
                message_limit: 0,
                step_limit: 0,
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(response.session_id, session_id);
        assert_eq!(response.state, "PROCESSING");
        assert_eq!(response.labels.get("env").map(String::as_str), Some("test"));
        assert_eq!(response.messages.len(), 1);
        assert_eq!(response.messages[0].content, "assistant reply");
        assert_eq!(response.steps.len(), 1);
        assert_eq!(response.steps[0].content, "hello");
    }

    #[tokio::test]
    async fn test_get_session_surfaces_not_found_and_list_errors() {
        let missing = setup_mock_gateway_handler(
            Arc::new(MockKvStore::default()),
            Arc::new(RecordingPubSub::default()),
        );
        let err = missing
            .handle_get_session(tonic::Request::new(proto::GetSessionRequest {
                session_id: "missing".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                message_limit: 0,
                step_limit: 0,
            }))
            .await
            .expect_err("missing session should fail");
        assert_eq!(err.code(), tonic::Code::NotFound);

        let kv = Arc::new(FailingKvStore {
            data: Mutex::new(HashMap::from([(
                (
                    "default".to_string(),
                    keys::session("test-agent", "session-1"),
                ),
                models::Session {
                    id: "session-1".to_string(),
                    agent: "test-agent".to_string(),
                    ns: "default".to_string(),
                    status: "IDLE".to_string(),
                    created_at: 1,
                    last_active: 2,
                    metadata: HashMap::new(),
                    labels: HashMap::new(),
                }
                .encode_to_vec(),
            )])),
            fail_list_prefix: Some(keys::session_message_prefix("test-agent", "session-1")),
            fail_get_key: None,
            fail_set_prefix: None,
            fail_delete_key: None,
            extra_list_keys: Vec::new(),
        });
        let handler = setup_gateway_handler_with(kv, Arc::new(RecordingPubSub::default()));

        let err = handler
            .handle_get_session(tonic::Request::new(proto::GetSessionRequest {
                session_id: "session-1".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                message_limit: 0,
                step_limit: 0,
            }))
            .await
            .expect_err("list failure should surface");
        assert_eq!(err.code(), tonic::Code::Internal);
        assert!(err.message().contains("Failed to list session messages"));
    }

    #[tokio::test]
    async fn test_get_session_surfaces_step_list_failure() {
        let kv = Arc::new(FailingKvStore {
            data: Mutex::new(HashMap::from([
                (
                    (
                        "default".to_string(),
                        keys::session("test-agent", "session-1"),
                    ),
                    models::Session {
                        id: "session-1".to_string(),
                        agent: "test-agent".to_string(),
                        ns: "default".to_string(),
                        status: "IDLE".to_string(),
                        created_at: 1,
                        last_active: 2,
                        metadata: HashMap::new(),
                        labels: HashMap::new(),
                    }
                    .encode_to_vec(),
                ),
                (
                    (
                        "default".to_string(),
                        keys::session_message("test-agent", "session-1", "msg-1"),
                    ),
                    models::SessionMessage {
                        id: "msg-1".to_string(),
                        role: 1,
                        content: "hello".to_string(),
                        created_at: 3,
                        labels: HashMap::new(),
                    }
                    .encode_to_vec(),
                ),
            ])),
            fail_list_prefix: Some(keys::session_message_step_prefix(
                "test-agent",
                "session-1",
                "msg-1",
            )),
            fail_get_key: None,
            fail_set_prefix: None,
            fail_delete_key: None,
            extra_list_keys: Vec::new(),
        });
        let handler = setup_gateway_handler_with(kv, Arc::new(RecordingPubSub::default()));

        let err = handler
            .handle_get_session(tonic::Request::new(proto::GetSessionRequest {
                session_id: "session-1".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                message_limit: 0,
                step_limit: 0,
            }))
            .await
            .expect_err("step list failure should surface");
        assert_eq!(err.code(), tonic::Code::Internal);
        assert!(err.message().contains("Failed to list session steps"));
    }

    #[tokio::test]
    async fn test_get_session_applies_message_and_step_limits() {
        let ns = "default";
        let agent = "test-agent";
        let session_id = "session-limited";
        let kv = Arc::new(MockKvStore::default());
        let handler = setup_mock_gateway_handler(kv.clone(), Arc::new(RecordingPubSub::default()));

        kv.set_msg(
            ns,
            &keys::session(agent, session_id),
            &models::Session {
                id: session_id.to_string(),
                agent: agent.to_string(),
                ns: ns.to_string(),
                status: "IDLE".to_string(),
                created_at: 1,
                last_active: 2,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();

        for index in 1..=3 {
            let message_id = format!("msg-{index}");
            kv.set_msg(
                ns,
                &keys::session_message(agent, session_id, &message_id),
                &models::SessionMessage {
                    id: message_id.clone(),
                    role: 2,
                    content: format!("assistant-{index}"),
                    created_at: index as i64,
                    labels: HashMap::new(),
                },
            )
            .await
            .unwrap();

            for step_index in 1..=2 {
                kv.set(
                    ns,
                    &keys::session_message_step(
                        agent,
                        session_id,
                        &message_id,
                        &format!("step-{step_index}"),
                    ),
                    &SessionStepEvent {
                        session_id: session_id.to_string(),
                        step_type: StepType::Token as i32,
                        content: format!("step-{index}-{step_index}"),
                        timestamp: (index * 10 + step_index) as i64,
                        agent: agent.to_string(),
                        ns: ns.to_string(),
                        message_id: message_id.clone(),
                        name: String::new(),
                        payload_json: String::new(),
                    }
                    .encode_to_vec(),
                )
                .await
                .unwrap();
            }
        }

        let response = handler
            .handle_get_session(tonic::Request::new(proto::GetSessionRequest {
                session_id: session_id.to_string(),
                agent: agent.to_string(),
                ns: ns.to_string(),
                message_limit: 2,
                step_limit: 2,
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(
            response
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            vec!["assistant-2", "assistant-3"]
        );
        assert_eq!(
            response
                .steps
                .iter()
                .map(|step| step.content.as_str())
                .collect::<Vec<_>>(),
            vec!["step-3-1", "step-3-2"]
        );
    }

    #[tokio::test]
    async fn test_list_session_messages_paginates_and_nests_steps() {
        let ns = "default";
        let agent = "test-agent";
        let session_id = "session-paged";
        let kv = Arc::new(MockKvStore::default());
        let handler = setup_mock_gateway_handler(kv.clone(), Arc::new(RecordingPubSub::default()));

        kv.set_msg(
            ns,
            &keys::session(agent, session_id),
            &models::Session {
                id: session_id.to_string(),
                agent: agent.to_string(),
                ns: ns.to_string(),
                status: "IDLE".to_string(),
                created_at: 1,
                last_active: 2,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();

        for index in 1..=3 {
            let message_id = format!("019f0000-0000-7000-8000-00000000000{index}");
            kv.set_msg(
                ns,
                &keys::session_message(agent, session_id, &message_id),
                &models::SessionMessage {
                    id: message_id.clone(),
                    role: if index == 2 { 1 } else { 2 },
                    content: format!("message-{index}"),
                    created_at: index as i64,
                    labels: HashMap::new(),
                },
            )
            .await
            .unwrap();

            kv.set(
                ns,
                &keys::session_message_step(agent, session_id, &message_id, "000001"),
                &SessionStepEvent {
                    session_id: session_id.to_string(),
                    step_type: StepType::Token as i32,
                    content: format!("step-{index}"),
                    timestamp: index as i64,
                    agent: agent.to_string(),
                    ns: ns.to_string(),
                    message_id: message_id.clone(),
                    name: String::new(),
                    payload_json: String::new(),
                }
                .encode_to_vec(),
            )
            .await
            .unwrap();
        }

        let newest_page = handler
            .handle_list_session_messages(tonic::Request::new(proto::ListSessionMessagesRequest {
                session_id: session_id.to_string(),
                agent: agent.to_string(),
                ns: ns.to_string(),
                page_size: 2,
                before_message_id: None,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(newest_page.has_more);
        assert_eq!(
            newest_page.next_before_message_id.as_deref(),
            Some("019f0000-0000-7000-8000-000000000002")
        );
        assert_eq!(newest_page.items.len(), 2);
        assert_eq!(
            newest_page.items[0]
                .message
                .as_ref()
                .map(|message| message.content.as_str()),
            Some("message-2")
        );
        assert!(newest_page.items[0].steps.is_empty());
        assert_eq!(
            newest_page.items[1]
                .message
                .as_ref()
                .map(|message| message.content.as_str()),
            Some("message-3")
        );
        assert_eq!(
            newest_page.items[1]
                .steps
                .iter()
                .map(|step| step.content.as_str())
                .collect::<Vec<_>>(),
            vec!["step-3"]
        );

        let older_page = handler
            .handle_list_session_messages(tonic::Request::new(proto::ListSessionMessagesRequest {
                session_id: session_id.to_string(),
                agent: agent.to_string(),
                ns: ns.to_string(),
                page_size: 2,
                before_message_id: Some("019f0000-0000-7000-8000-000000000002".to_string()),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!older_page.has_more);
        assert_eq!(older_page.items.len(), 1);
        assert_eq!(
            older_page.items[0]
                .message
                .as_ref()
                .map(|message| message.content.as_str()),
            Some("message-1")
        );
        assert_eq!(
            older_page.items[0]
                .steps
                .iter()
                .map(|step| step.content.as_str())
                .collect::<Vec<_>>(),
            vec!["step-1"]
        );
    }

    #[tokio::test]
    async fn test_list_session_messages_allows_unknown_cursor() {
        let kv = Arc::new(MockKvStore::default());
        let handler = setup_mock_gateway_handler(kv.clone(), Arc::new(RecordingPubSub::default()));

        kv.set_msg(
            "default",
            &keys::session("test-agent", "session-1"),
            &models::Session {
                id: "session-1".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                status: "IDLE".to_string(),
                created_at: 1,
                last_active: 2,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();

        let response = handler
            .handle_list_session_messages(tonic::Request::new(proto::ListSessionMessagesRequest {
                session_id: "session-1".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                page_size: 10,
                before_message_id: Some("missing".to_string()),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(response.items.is_empty());
        assert!(!response.has_more);
    }

    #[tokio::test]
    async fn test_send_message_maps_common_errors() {
        let kv = Arc::new(MockKvStore::default());
        let handler = setup_mock_gateway_handler(kv.clone(), Arc::new(RecordingPubSub::default()));

        let empty = handler
            .handle_send_message(tonic::Request::new(proto::SendMessageRequest {
                session_id: "missing".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                message: "   ".to_string(),
                labels: HashMap::new(),
            }))
            .await
            .expect_err("empty message should fail");
        assert_eq!(empty.code(), tonic::Code::InvalidArgument);

        let not_found = handler
            .handle_send_message(tonic::Request::new(proto::SendMessageRequest {
                session_id: "missing".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                message: "hello".to_string(),
                labels: HashMap::new(),
            }))
            .await
            .expect_err("missing session should fail");
        assert_eq!(not_found.code(), tonic::Code::NotFound);

        kv.set_msg(
            "default",
            &keys::session("test-agent", "busy-session"),
            &models::Session {
                id: "busy-session".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 100,
                last_active: chrono::Utc::now().timestamp_micros(),
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();

        let busy = handler
            .handle_send_message(tonic::Request::new(proto::SendMessageRequest {
                session_id: "busy-session".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                message: "hello".to_string(),
                labels: HashMap::new(),
            }))
            .await
            .expect_err("processing session should fail");
        assert_eq!(busy.code(), tonic::Code::ResourceExhausted);

        kv.set_msg(
            "default",
            &keys::session("test-agent", "idle-session"),
            &models::Session {
                id: "idle-session".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                status: "IDLE".to_string(),
                created_at: 100,
                last_active: chrono::Utc::now().timestamp_micros(),
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();

        let sent = handler
            .handle_send_message(tonic::Request::new(proto::SendMessageRequest {
                session_id: "idle-session".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                message: "hello".to_string(),
                labels: HashMap::from([("source".to_string(), "test".to_string())]),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(sent.session_id, "idle-session");

        let stored_message_keys = kv
            .list_keys(
                "default",
                &keys::session_message_prefix("test-agent", "idle-session"),
            )
            .await
            .unwrap();
        assert_eq!(stored_message_keys.len(), 1);
    }

    #[tokio::test]
    async fn test_stream_session_steps() {
        let pubsub = Arc::new(RecordingPubSub::default());

        let session_id = "test-session-123";
        let topic_name =
            topics::session_step_topic_for_shard(topics::session_step_shard(session_id));

        let event1 = SessionStepEvent {
            session_id: session_id.to_string(),
            step_type: StepType::Action as i32,
            content: "Tool call".to_string(),
            timestamp: 1000,
            agent: "test-agent".to_string(),
            ns: "default".to_string(),
            message_id: "msg-123".to_string(),
            name: "knowledge_search".to_string(),
            payload_json: "{\"query\":\"talon\"}".to_string(),
        };

        let event2 = SessionStepEvent {
            session_id: session_id.to_string(),
            step_type: StepType::Token as i32,
            content: "Hello, ".to_string(),
            timestamp: 2000,
            agent: "test-agent".to_string(),
            ns: "default".to_string(),
            message_id: "msg-123".to_string(),
            name: "".to_string(),
            payload_json: "".to_string(),
        };

        {
            let mut map = pubsub.streams.lock().await;
            map.insert(
                topic_name,
                vec![event1.encode_to_vec(), event2.encode_to_vec()],
            );
        }

        let handler = setup_mock_gateway_handler(Arc::new(MockKvStore::default()), pubsub);
        let req = tonic::Request::new(proto::StreamSessionStepsRequest {
            session_id: session_id.to_string(),
            agent: "test-agent".to_string(),
            ns: "default".to_string(),
        });

        let response = handler.handle_stream_session_steps(req).await.unwrap();
        let mut stream = response.into_inner();

        let e1 = stream.next().await.unwrap().unwrap();
        assert_eq!(e1.step_type, StepType::Action as i32);
        assert_eq!(e1.name, "knowledge_search");

        let e2 = stream.next().await.unwrap().unwrap();
        assert_eq!(e2.step_type, StepType::Token as i32);
        assert_eq!(e2.content, "Hello, ");

        let e3 = stream.next().await;
        assert!(e3.is_none());
    }

    #[tokio::test]
    async fn test_list_sessions_returns_updated_at_sorted_desc() {
        let kv = Arc::new(MockKvStore::default());
        let handler = setup_mock_gateway_handler(kv.clone(), Arc::new(RecordingPubSub::default()));

        let ns = "default";
        let agent = "test-agent";
        let older_session = models::Session {
            id: "session-old".to_string(),
            agent: agent.to_string(),
            ns: ns.to_string(),
            status: "IDLE".to_string(),
            created_at: 100,
            last_active: 200,
            metadata: HashMap::new(),
            labels: HashMap::new(),
        };
        let newer_session = models::Session {
            id: "session-new".to_string(),
            agent: agent.to_string(),
            ns: ns.to_string(),
            status: "IDLE".to_string(),
            created_at: 300,
            last_active: 400,
            metadata: HashMap::new(),
            labels: HashMap::new(),
        };

        kv.set_msg(ns, &keys::session(agent, &older_session.id), &older_session)
            .await
            .unwrap();
        kv.set_msg(ns, &keys::session(agent, &newer_session.id), &newer_session)
            .await
            .unwrap();
        kv.set(
            ns,
            &keys::session_message(agent, &newer_session.id, "msg-1"),
            b"nested-message-should-be-skipped",
        )
        .await
        .unwrap();

        let response = handler
            .handle_list_sessions(tonic::Request::new(proto::ListSessionsRequest {
                agent: agent.to_string(),
                ns: ns.to_string(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(response.session_ids.len(), 2);
        assert_eq!(response.sessions.len(), 2);
        assert_eq!(response.sessions[0].session_id, "session-new");
        assert_eq!(response.sessions[0].updated_at, 400);
        assert!(response.sessions[0].labels.is_empty());
        assert_eq!(response.sessions[1].session_id, "session-old");
        assert_eq!(response.sessions[1].updated_at, 200);
        assert!(response.sessions[1].labels.is_empty());
    }

    #[tokio::test]
    async fn test_list_sessions_surfaces_list_and_get_failures_and_skips_missing_metadata() {
        let handler = setup_gateway_handler_with(
            Arc::new(FailingKvStore {
                data: Mutex::new(HashMap::new()),
                fail_list_prefix: Some(keys::session_prefix("test-agent")),
                fail_get_key: None,
                fail_set_prefix: None,
                fail_delete_key: None,
                extra_list_keys: Vec::new(),
            }),
            Arc::new(FailingPubSub {
                fail_publish: false,
                fail_subscribe: false,
            }),
        );
        let err = handler
            .handle_list_sessions(tonic::Request::new(proto::ListSessionsRequest {
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
            }))
            .await
            .expect_err("list failure should surface");
        assert_eq!(err.code(), tonic::Code::Internal);
        assert!(err.message().contains("Failed to list sessions"));

        let session_key = keys::session("test-agent", "session-1");
        let kv = Arc::new(FailingKvStore {
            data: Mutex::new(HashMap::from([
                (
                    ("default".to_string(), session_key.clone()),
                    b"ignored".to_vec(),
                ),
                (
                    (
                        "default".to_string(),
                        keys::session("test-agent", "session-2"),
                    ),
                    models::Session {
                        id: "session-2".to_string(),
                        agent: "test-agent".to_string(),
                        ns: "default".to_string(),
                        status: "IDLE".to_string(),
                        created_at: 1,
                        last_active: 9,
                        metadata: HashMap::new(),
                        labels: HashMap::new(),
                    }
                    .encode_to_vec(),
                ),
            ])),
            fail_list_prefix: None,
            fail_get_key: Some(session_key.clone()),
            fail_set_prefix: None,
            fail_delete_key: None,
            extra_list_keys: Vec::new(),
        });
        let handler = setup_gateway_handler_with(
            kv,
            Arc::new(FailingPubSub {
                fail_publish: false,
                fail_subscribe: false,
            }),
        );
        let err = handler
            .handle_list_sessions(tonic::Request::new(proto::ListSessionsRequest {
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
            }))
            .await
            .expect_err("get failure should surface");
        assert_eq!(err.code(), tonic::Code::Internal);
        assert!(err.message().contains("Failed to fetch session metadata"));

        let kv = Arc::new(FailingKvStore {
            data: Mutex::new(HashMap::new()),
            fail_list_prefix: None,
            fail_get_key: None,
            fail_set_prefix: None,
            fail_delete_key: None,
            extra_list_keys: vec![keys::session("test-agent", "session-ghost")],
        });
        let handler = setup_gateway_handler_with(
            kv,
            Arc::new(FailingPubSub {
                fail_publish: false,
                fail_subscribe: false,
            }),
        );
        let response = handler
            .handle_list_sessions(tonic::Request::new(proto::ListSessionsRequest {
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(response.session_ids.len(), 1);
        assert_eq!(response.session_ids[0], "session-ghost");
        assert!(response.sessions.is_empty());
    }

    #[tokio::test]
    async fn test_delete_session_removes_session_messages_and_steps() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = setup_mock_gateway_handler(kv.clone(), pubsub.clone());

        let ns = "default";
        let agent = "test-agent";
        let session_id = "session-1";
        let message_id = "msg-1";
        let step_id = "step-1";

        let session = models::Session {
            id: session_id.to_string(),
            agent: agent.to_string(),
            ns: ns.to_string(),
            status: "IDLE".to_string(),
            created_at: 100,
            last_active: 200,
            metadata: HashMap::new(),
            labels: HashMap::new(),
        };
        let message = models::SessionMessage {
            id: message_id.to_string(),
            role: 1,
            content: "hello".to_string(),
            created_at: 300,
            labels: HashMap::new(),
        };
        let step = crate::control::events::SessionStepEvent {
            session_id: session_id.to_string(),
            step_type: StepType::Action as i32,
            content: "tool".to_string(),
            timestamp: 400,
            agent: agent.to_string(),
            ns: ns.to_string(),
            message_id: message_id.to_string(),
            name: "search".to_string(),
            payload_json: "{}".to_string(),
        };

        kv.set_msg(ns, &keys::session(agent, session_id), &session)
            .await
            .unwrap();
        kv.set_msg(
            ns,
            &keys::session_message(agent, session_id, message_id),
            &message,
        )
        .await
        .unwrap();
        kv.set_msg(
            ns,
            &keys::session_message_step(agent, session_id, message_id, step_id),
            &step,
        )
        .await
        .unwrap();

        let response = handler
            .handle_delete_session(tonic::Request::new(proto::DeleteSessionRequest {
                session_id: session_id.to_string(),
                agent: agent.to_string(),
                ns: ns.to_string(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(response.success);
        assert!(kv
            .get(ns, &keys::session(agent, session_id))
            .await
            .unwrap()
            .is_none());
        assert!(kv
            .get(ns, &keys::session_message(agent, session_id, message_id))
            .await
            .unwrap()
            .is_none());
        assert!(kv
            .get(
                ns,
                &keys::session_message_step(agent, session_id, message_id, step_id)
            )
            .await
            .unwrap()
            .is_none());
        assert!(kv
            .list_keys(ns, &keys::session_message_prefix(agent, session_id))
            .await
            .unwrap()
            .is_empty());

        let published = pubsub.published.lock().await;
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, topics::RESOURCE_LIFECYCLE_TOPIC);
    }

    #[tokio::test]
    async fn test_delete_session_surfaces_delete_and_publish_failures() {
        let kv = Arc::new(FailingKvStore {
            data: Mutex::new(HashMap::new()),
            fail_list_prefix: Some(keys::session_message_prefix("test-agent", "session-1")),
            fail_get_key: None,
            fail_set_prefix: None,
            fail_delete_key: None,
            extra_list_keys: Vec::new(),
        });
        let handler = setup_gateway_handler_with(
            kv,
            Arc::new(FailingPubSub {
                fail_publish: false,
                fail_subscribe: false,
            }),
        );

        let err = handler
            .handle_delete_session(tonic::Request::new(proto::DeleteSessionRequest {
                session_id: "session-1".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
            }))
            .await
            .expect_err("delete descendants failure should surface");
        assert_eq!(err.code(), tonic::Code::Internal);
        assert!(err
            .message()
            .contains("Failed to delete session descendants"));

        let kv = Arc::new(MockKvStore::default());
        kv.set_msg(
            "default",
            &keys::session("test-agent", "session-1"),
            &models::Session {
                id: "session-1".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                status: "IDLE".to_string(),
                created_at: 1,
                last_active: 2,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        let handler = setup_gateway_handler_with(
            kv,
            Arc::new(FailingPubSub {
                fail_publish: true,
                fail_subscribe: false,
            }),
        );

        let err = handler
            .handle_delete_session(tonic::Request::new(proto::DeleteSessionRequest {
                session_id: "session-1".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
            }))
            .await
            .expect_err("publish failure should surface");
        assert_eq!(err.code(), tonic::Code::Internal);
        assert!(err.message().contains("Failed to publish event"));

        let kv = Arc::new(FailingKvStore {
            data: Mutex::new(HashMap::new()),
            fail_list_prefix: None,
            fail_get_key: None,
            fail_set_prefix: None,
            fail_delete_key: Some(keys::session("test-agent", "session-1")),
            extra_list_keys: Vec::new(),
        });
        let handler = setup_gateway_handler_with(
            kv,
            Arc::new(FailingPubSub {
                fail_publish: false,
                fail_subscribe: false,
            }),
        );
        let err = handler
            .handle_delete_session(tonic::Request::new(proto::DeleteSessionRequest {
                session_id: "session-1".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
            }))
            .await
            .expect_err("delete failure should surface");
        assert_eq!(err.code(), tonic::Code::Internal);
        assert!(err.message().contains("Failed to delete session"));
    }

    #[tokio::test]
    async fn test_stop_session_generation_publishes_session_control_event() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let handler = setup_mock_gateway_handler(kv.clone(), pubsub.clone());

        let ns = "default";
        let agent = "test-agent";
        let session_id = "session-1";
        let session = models::Session {
            id: session_id.to_string(),
            agent: agent.to_string(),
            ns: ns.to_string(),
            status: "PROCESSING".to_string(),
            created_at: 100,
            last_active: 200,
            metadata: HashMap::new(),
            labels: HashMap::new(),
        };
        kv.set_msg(ns, &keys::session(agent, session_id), &session)
            .await
            .unwrap();

        let response = handler
            .handle_stop_session_generation(tonic::Request::new(
                proto::StopSessionGenerationRequest {
                    session_id: session_id.to_string(),
                    agent: agent.to_string(),
                    ns: ns.to_string(),
                },
            ))
            .await
            .unwrap()
            .into_inner();

        assert!(response.success);

        let published = pubsub.published.lock().await;
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, topics::SESSION_CONTROL_TOPIC);
        let event = crate::control::events::SessionControlEvent::decode(published[0].1.as_slice())
            .expect("expected session control event");
        assert_eq!(event.session_id, session_id);
        assert_eq!(event.agent, agent);
        assert_eq!(event.ns, ns);
        assert_eq!(event.action, "stop_generation");
    }

    #[tokio::test]
    async fn test_stop_session_generation_requires_existing_session() {
        let handler = setup_mock_gateway_handler(
            Arc::new(MockKvStore::default()),
            Arc::new(RecordingPubSub::default()),
        );

        let err = handler
            .handle_stop_session_generation(tonic::Request::new(
                proto::StopSessionGenerationRequest {
                    session_id: "missing".to_string(),
                    agent: "test-agent".to_string(),
                    ns: "default".to_string(),
                },
            ))
            .await
            .expect_err("missing session should fail");

        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_stop_session_generation_surfaces_publish_failure() {
        let kv = Arc::new(MockKvStore::default());
        kv.set_msg(
            "default",
            &keys::session("test-agent", "session-1"),
            &models::Session {
                id: "session-1".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 1,
                last_active: 2,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        let handler = setup_gateway_handler_with(
            kv,
            Arc::new(FailingPubSub {
                fail_publish: true,
                fail_subscribe: false,
            }),
        );

        let err = handler
            .handle_stop_session_generation(tonic::Request::new(
                proto::StopSessionGenerationRequest {
                    session_id: "session-1".to_string(),
                    agent: "test-agent".to_string(),
                    ns: "default".to_string(),
                },
            ))
            .await
            .expect_err("publish failure should surface");

        assert_eq!(err.code(), tonic::Code::Internal);
        assert!(err.message().contains("Failed to publish stop event"));
    }

    #[tokio::test]
    async fn test_stream_session_steps_surfaces_subscribe_failure() {
        let handler = setup_gateway_handler_with(
            Arc::new(MockKvStore::default()),
            Arc::new(FailingPubSub {
                fail_publish: false,
                fail_subscribe: true,
            }),
        );

        let err = match handler
            .handle_stream_session_steps(tonic::Request::new(proto::StreamSessionStepsRequest {
                session_id: "session-1".to_string(),
                agent: "test-agent".to_string(),
                ns: "default".to_string(),
            }))
            .await
        {
            Ok(_) => panic!("subscribe failure should surface"),
            Err(err) => err,
        };

        assert_eq!(err.code(), tonic::Code::Internal);
        assert!(err
            .message()
            .contains("Failed to subscribe to session stream"));
    }
}
