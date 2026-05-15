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
    use crate::gateway::rpc::models;
    use crate::gateway::rpc::{proto, GrpcGatewayHandler};
    use crate::gateway::{server::Gateway, session_streams::SessionStreamHub};
    use futures::{stream, StreamExt};
    use prost::Message;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockKvStore {
        data: Mutex<HashMap<(String, String), Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, ns: &str, k: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self
                .data
                .lock()
                .await
                .get(&(ns.to_string(), k.to_string()))
                .cloned())
        }

        async fn set(&self, ns: &str, k: &str, v: &[u8]) -> anyhow::Result<()> {
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
            self.data
                .lock()
                .await
                .remove(&(ns.to_string(), k.to_string()));
            Ok(())
        }

        async fn list_keys(&self, ns: &str, p: &str) -> anyhow::Result<Vec<String>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|(stored_ns, key)| {
                    (stored_ns == ns && key.starts_with(p)).then(|| key.clone())
                })
                .collect::<Vec<_>>();
            keys.sort();
            Ok(keys)
        }
    }

    struct MockPubSub {
        pub streams: Arc<Mutex<HashMap<String, Vec<Vec<u8>>>>>,
        pub published: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    }

    #[async_trait::async_trait]
    impl MessagePublisher for MockPubSub {
        async fn publish(&self, topic: &str, message: &[u8]) -> anyhow::Result<()> {
            self.published
                .lock()
                .await
                .push((topic.to_string(), message.to_vec()));
            Ok(())
        }

        async fn subscribe(
            &self,
            topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            let map = self.streams.lock().await;
            let data = map.get(topic).cloned().unwrap_or_default();
            Ok(Box::pin(stream::iter(data)))
        }
    }

    fn setup_mock_gateway_handler(
        kv: Arc<MockKvStore>,
        streams: Arc<Mutex<HashMap<String, Vec<Vec<u8>>>>>,
        published: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    ) -> GrpcGatewayHandler {
        let gateway = Arc::new(Gateway {
            auth_config: None,
            kv,
            pubsub: Arc::new(MockPubSub {
                streams: streams.clone(),
                published: published.clone(),
            }),
            scheduler: Arc::new(NoopSchedulerBackend),
            session_streams: Arc::new(SessionStreamHub::new(Arc::new(MockPubSub {
                streams,
                published,
            }))),
        });
        GrpcGatewayHandler { gateway }
    }

    #[tokio::test]
    async fn test_stream_session_steps() {
        let streams = Arc::new(Mutex::new(HashMap::new()));

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
            let mut map = streams.lock().await;
            map.insert(
                topic_name,
                vec![event1.encode_to_vec(), event2.encode_to_vec()],
            );
        }

        let handler = setup_mock_gateway_handler(
            Arc::new(MockKvStore::default()),
            streams,
            Arc::new(Mutex::new(Vec::new())),
        );
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
        let streams = Arc::new(Mutex::new(HashMap::new()));
        let handler = setup_mock_gateway_handler(kv.clone(), streams, Arc::new(Mutex::new(Vec::new())));

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
    async fn test_delete_session_removes_session_messages_and_steps() {
        let kv = Arc::new(MockKvStore::default());
        let streams = Arc::new(Mutex::new(HashMap::new()));
        let published = Arc::new(Mutex::new(Vec::new()));
        let handler = setup_mock_gateway_handler(kv.clone(), streams, published.clone());

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
        assert!(
            kv.get(ns, &keys::session(agent, session_id))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            kv.get(ns, &keys::session_message(agent, session_id, message_id))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            kv.get(
                ns,
                &keys::session_message_step(agent, session_id, message_id, step_id)
            )
            .await
            .unwrap()
            .is_none()
        );
        assert!(
            kv.list_keys(ns, &keys::session_message_prefix(agent, session_id))
                .await
                .unwrap()
                .is_empty()
        );

        let published = published.lock().await;
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, topics::RESOURCE_LIFECYCLE_TOPIC);
    }

    #[tokio::test]
    async fn test_stop_session_generation_publishes_session_control_event() {
        let kv = Arc::new(MockKvStore::default());
        let streams = Arc::new(Mutex::new(HashMap::new()));
        let published = Arc::new(Mutex::new(Vec::new()));
        let handler = setup_mock_gateway_handler(kv.clone(), streams, published.clone());

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

        let published = published.lock().await;
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, topics::SESSION_CONTROL_TOPIC);
        let event = crate::control::events::SessionControlEvent::decode(published[0].1.as_slice())
            .expect("expected session control event");
        assert_eq!(event.session_id, session_id);
        assert_eq!(event.agent, agent);
        assert_eq!(event.ns, ns);
        assert_eq!(event.action, "stop_generation");
    }
}
