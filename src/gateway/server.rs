// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{
    object_store::ObjectStore, scheduler::SchedulerBackend, KeyValueStore, MessagePublisher,
};
use crate::gateway::auth::AuthConfig;
use crate::gateway::session_streams::SessionStreamHub;
use anyhow::Result;
use axum::{routing::get, routing::post, Router};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
pub struct Gateway {
    pub auth_config: Option<AuthConfig>,
    pub kv: Arc<dyn KeyValueStore + Send + Sync>,
    pub pubsub: Arc<dyn MessagePublisher + Send + Sync>,
    pub scheduler: Arc<dyn SchedulerBackend + Send + Sync>,
    pub objects: Arc<dyn ObjectStore + Send + Sync>,
    pub session_streams: Arc<SessionStreamHub>,
}

impl Gateway {
    pub fn new(
        auth_config: Option<AuthConfig>,
        kv: Arc<dyn KeyValueStore + Send + Sync>,
        pubsub: Arc<dyn MessagePublisher + Send + Sync>,
        scheduler: Arc<dyn SchedulerBackend + Send + Sync>,
        objects: Arc<dyn ObjectStore + Send + Sync>,
    ) -> Self {
        let session_streams = Arc::new(SessionStreamHub::new(pubsub.clone()));
        Self {
            auth_config,
            kv,
            pubsub,
            scheduler,
            objects,
            session_streams,
        }
    }

    pub(crate) fn clone_internal(&self) -> Self {
        Self {
            auth_config: self.auth_config.clone(),
            kv: self.kv.clone(),
            pubsub: self.pubsub.clone(),
            scheduler: self.scheduler.clone(),
            objects: self.objects.clone(),
            session_streams: self.session_streams.clone(),
        }
    }

    pub fn http_ui_router(&self) -> Router {
        Router::new()
            .route(
                "/.well-known/agent-card.json",
                get(crate::gateway::a2a::get_well_known_agent_card),
            )
            .route(
                "/message:operation",
                post(crate::gateway::a2a::post_message_operation),
            )
            .route(
                "/v1/message:operation",
                post(crate::gateway::a2a::post_message_operation),
            )
            .route("/tasks", get(crate::gateway::a2a::list_tasks))
            .route("/v1/tasks", get(crate::gateway::a2a::list_tasks))
            .route(
                "/tasks/*tail",
                get(crate::gateway::a2a::get_task).post(crate::gateway::a2a::post_task_operation),
            )
            .route(
                "/v1/tasks/*tail",
                get(crate::gateway::a2a::get_task).post(crate::gateway::a2a::post_task_operation),
            )
            .route(
                "/extendedAgentCard",
                get(crate::gateway::a2a::unsupported_a2a_operation),
            )
            .route(
                "/v1/ui/ns/:ns/agents/:agent/sessions/:session_id",
                post(crate::gateway::ui::post_chat)
                    .get(crate::gateway::ui::get_chat)
                    .delete(crate::gateway::ui::delete_chat),
            )
            .with_state(Arc::new(self.clone_internal()))
            .layer(permissive_cors_layer())
    }

    pub async fn start_rpc_server(&self, addr: &str) -> Result<()> {
        self.start_rpc_server_with_shutdown(addr, std::future::pending::<()>())
            .await
    }

    pub async fn start_rpc_server_with_shutdown<F>(&self, addr: &str, shutdown: F) -> Result<()>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        use tonic::transport::Server;
        let addr = addr.parse()?;
        println!("gRPC Gateway listening on: {}", addr);

        let handler = crate::gateway::rpc::GrpcGatewayHandler {
            gateway: Arc::new(self.clone_internal()),
        };

        let auth_config = self.auth_config.clone().unwrap_or_else(AuthConfig::open);
        let interceptor = crate::gateway::auth::TalonAuthInterceptor {
            config: auth_config,
        };

        let svc = crate::gateway::rpc::proto::gateway_service_server::GatewayServiceServer::with_interceptor(handler, interceptor);
        let svc = tonic_web::enable(svc);

        Server::builder()
            .accept_http1(true)
            .layer(permissive_cors_layer())
            .add_service(svc)
            .serve_with_shutdown(addr, shutdown)
            .await
            .map_err(|e| anyhow::anyhow!("Tonic server failed: {}", e))?;

        Ok(())
    }

    pub async fn start_http_ui_server(&self, addr: &str) -> Result<()> {
        self.start_http_ui_server_with_shutdown(addr, std::future::pending::<()>())
            .await
    }

    pub async fn start_http_ui_server_with_shutdown<F>(&self, addr: &str, shutdown: F) -> Result<()>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("Gateway UI HTTP listening on {}", addr);
        axum::serve(listener, self.http_ui_router())
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(|e| anyhow::anyhow!("Gateway UI HTTP server failed: {}", e))?;
        Ok(())
    }
}

fn permissive_cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any)
        .expose_headers(Any)
}

#[cfg(test)]
mod tests {
    use super::Gateway;
    use crate::control::{
        events::{SessionMessagePartEvent, SessionMessagePartEventKind},
        keys::{ResourceKey, ResourceList},
        scheduler::NoopSchedulerBackend,
        KeyValueStore, MessagePublisher,
    };
    use crate::gateway::auth::AuthConfig;
    use crate::gateway::rpc::models::{SessionMessagePart, SessionMessagePartType};
    use axum::body::Body;
    use axum::http::{header, Method, Request, StatusCode};
    use futures::stream;
    use prost::Message;
    use std::collections::{HashMap, VecDeque};
    use std::net::SocketAddr;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tokio::time::{sleep, Duration};
    use tower::ServiceExt;

    #[derive(Default)]
    struct MockKvStore {
        data: Mutex<HashMap<ResourceKey, Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, k: &ResourceKey) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.data.lock().await.get(k).cloned())
        }

        async fn set(&self, k: &ResourceKey, v: &[u8]) -> anyhow::Result<()> {
            self.data.lock().await.insert(k.clone(), v.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            k: &ResourceKey,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let current = data.get(k).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(k.clone(), value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, k: &ResourceKey) -> anyhow::Result<()> {
            self.data.lock().await.remove(k);
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
    struct MockPubSub {
        batches: Mutex<HashMap<String, VecDeque<Vec<Vec<u8>>>>>,
    }

    #[async_trait::async_trait]
    impl MessagePublisher for MockPubSub {
        async fn publish(&self, _topic: &str, _message: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe(
            &self,
            topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            let batch = self
                .batches
                .lock()
                .await
                .get_mut(topic)
                .and_then(|entries| entries.pop_front())
                .unwrap_or_default();
            Ok(Box::pin(stream::iter(batch)))
        }
    }

    fn gateway() -> Gateway {
        Gateway::new(
            None,
            Arc::new(MockKvStore::default()),
            Arc::new(MockPubSub::default()),
            Arc::new(NoopSchedulerBackend),
            crate::control::object_store::default_object_store(),
        )
    }

    fn gateway_with_pubsub(pubsub: Arc<MockPubSub>) -> Gateway {
        Gateway::new(
            None,
            Arc::new(MockKvStore::default()),
            pubsub,
            Arc::new(NoopSchedulerBackend),
            crate::control::object_store::default_object_store(),
        )
    }

    async fn seed_namespace_and_agent(gateway: &Gateway, ns: &str, agent: &str) {
        gateway
            .kv
            .set(
                &crate::control::keys::namespace_metadata(ns),
                &crate::gateway::rpc::models::Namespace {
                    name: ns.to_string(),
                    parent: String::new(),
                    is_deleted: false,
                    deleted_at: 0,
                    labels: HashMap::new(),
                }
                .encode_to_vec(),
            )
            .await
            .unwrap();
        gateway
            .kv
            .set(
                &crate::control::keys::agent(ns, agent),
                &crate::gateway::rpc::models::Agent {
                    name: agent.to_string(),
                    ns: ns.to_string(),
                    definition: None,
                    effective_spec: None,
                    template_deps: Vec::new(),
                    labels: HashMap::new(),
                }
                .encode_to_vec(),
            )
            .await
            .unwrap();
    }

    fn agent_card(
        ns: &str,
        name: &str,
        agent_ref: &str,
        hostname: &str,
    ) -> crate::gateway::rpc::manifests::AgentCard {
        crate::gateway::rpc::manifests::AgentCard {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "AgentCard".to_string(),
            metadata: Some(crate::gateway::rpc::manifests::ObjectMeta {
                name: name.to_string(),
                namespace: ns.to_string(),
                labels: HashMap::new(),
                annotations: HashMap::new(),
            }),
            spec: Some(crate::gateway::rpc::manifests::AgentCardSpec {
                agent_ref: agent_ref.to_string(),
                hostname: hostname.to_string(),
                name: "Support Agent".to_string(),
                description: "Answers support questions.".to_string(),
                version: "1.0.0".to_string(),
                capabilities: Some(crate::gateway::rpc::manifests::AgentCardCapabilities {
                    streaming: false,
                    push_notifications: false,
                    extended_agent_card: false,
                }),
                default_input_modes: vec!["text/plain".to_string()],
                default_output_modes: vec!["text/plain".to_string()],
                skills: vec![crate::gateway::rpc::manifests::AgentCardSkill {
                    id: "answer_support_question".to_string(),
                    name: "Answer support question".to_string(),
                    description: "Answers using docs.".to_string(),
                    tags: vec!["support".to_string()],
                    examples: Vec::new(),
                    input_modes: Vec::new(),
                    output_modes: Vec::new(),
                }],
                auth: Some(crate::gateway::rpc::manifests::AgentCardAuth {
                    discovery: "public".to_string(),
                    operations: "public".to_string(),
                }),
            }),
        }
    }

    async fn seed_agent_card(
        gateway: &Gateway,
        ns: &str,
        name: &str,
        agent_ref: &str,
        hostname: &str,
    ) {
        seed_namespace_and_agent(gateway, ns, agent_ref).await;
        let handler = crate::gateway::rpc::GrpcGatewayHandler {
            gateway: Arc::new(gateway.clone()),
        };
        handler
            .handle_create_agent_card(tonic::Request::new(
                crate::gateway::rpc::proto::CreateAgentCardRequest {
                    ns: ns.to_string(),
                    card: Some(agent_card(ns, name, agent_ref, hostname)),
                },
            ))
            .await
            .unwrap();
    }

    #[test]
    fn new_preserves_auth_config_and_initializes_session_streams() {
        let gateway = Gateway::new(
            Some(AuthConfig::tokens(vec!["secret-token".to_string()])),
            Arc::new(MockKvStore::default()),
            Arc::new(MockPubSub::default()),
            Arc::new(NoopSchedulerBackend),
            crate::control::object_store::default_object_store(),
        );

        assert!(matches!(
            gateway.auth_config.as_ref().map(|cfg| &cfg.mode),
            Some(crate::gateway::auth::AuthMode::Token)
        ));
        assert!(Arc::strong_count(&gateway.session_streams) >= 1);
    }

    #[test]
    fn clone_internal_reuses_shared_dependencies() {
        let gateway = gateway();
        let cloned = gateway.clone_internal();

        assert!(Arc::ptr_eq(&gateway.kv, &cloned.kv));
        assert!(Arc::ptr_eq(&gateway.pubsub, &cloned.pubsub));
        assert!(Arc::ptr_eq(&gateway.scheduler, &cloned.scheduler));
        assert!(Arc::ptr_eq(
            &gateway.session_streams,
            &cloned.session_streams
        ));
        assert!(cloned.auth_config.is_none());
    }

    #[tokio::test]
    async fn http_ui_router_routes_supported_methods() {
        let router = gateway().http_ui_router();

        let post = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/ui/ns/default/agents/agent/sessions/session-1")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"messages":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(post.status(), StatusCode::BAD_REQUEST);

        let put = router
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri("/v1/ui/ns/default/agents/agent/sessions/session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn http_ui_router_serves_agent_card_by_host() {
        let gateway = gateway();
        seed_namespace_and_agent(&gateway, "support", "support-docs").await;
        let card = crate::gateway::rpc::manifests::AgentCard {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "AgentCard".to_string(),
            metadata: Some(crate::gateway::rpc::manifests::ObjectMeta {
                name: "support-public".to_string(),
                namespace: "support".to_string(),
                labels: HashMap::new(),
                annotations: HashMap::new(),
            }),
            spec: Some(crate::gateway::rpc::manifests::AgentCardSpec {
                agent_ref: "support-docs".to_string(),
                hostname: "support.example.com".to_string(),
                name: "Support Agent".to_string(),
                description: "Answers support questions.".to_string(),
                version: "1.0.0".to_string(),
                capabilities: Some(crate::gateway::rpc::manifests::AgentCardCapabilities {
                    streaming: false,
                    push_notifications: false,
                    extended_agent_card: false,
                }),
                default_input_modes: vec!["text/plain".to_string()],
                default_output_modes: vec!["text/plain".to_string()],
                skills: vec![crate::gateway::rpc::manifests::AgentCardSkill {
                    id: "answer_support_question".to_string(),
                    name: "Answer support question".to_string(),
                    description: "Answers using docs.".to_string(),
                    tags: vec!["support".to_string()],
                    examples: Vec::new(),
                    input_modes: Vec::new(),
                    output_modes: Vec::new(),
                }],
                auth: Some(crate::gateway::rpc::manifests::AgentCardAuth {
                    discovery: "public".to_string(),
                    operations: "public".to_string(),
                }),
            }),
        };
        let handler = crate::gateway::rpc::GrpcGatewayHandler {
            gateway: Arc::new(gateway.clone()),
        };
        handler
            .handle_create_agent_card(tonic::Request::new(
                crate::gateway::rpc::proto::CreateAgentCardRequest {
                    ns: "support".to_string(),
                    card: Some(card),
                },
            ))
            .await
            .unwrap();

        assert!(gateway
            .kv
            .get(&crate::control::keys::agent_card_hostname(
                "support.example.com"
            ))
            .await
            .unwrap()
            .is_some());

        let response = gateway
            .http_ui_router()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/.well-known/agent-card.json")
                    .header(header::HOST, "support.example.com:8080")
                    .header("x-forwarded-proto", "HTTP")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["name"], "Support Agent");
        assert_eq!(value["url"], "http://support.example.com:8080");
        assert_eq!(value["protocolVersion"], "0.3.0");
        assert_eq!(value["preferredTransport"], "HTTP+JSON");
        assert_eq!(value["capabilities"]["streaming"], true);
        assert_eq!(value["skills"][0]["id"], "answer_support_question");
        assert!(value.get("auth").is_none());

        handler
            .handle_create_agent_card(tonic::Request::new(
                crate::gateway::rpc::proto::CreateAgentCardRequest {
                    ns: "support".to_string(),
                    card: Some(agent_card(
                        "support",
                        "local-public",
                        "support-docs",
                        "localhost",
                    )),
                },
            ))
            .await
            .unwrap();

        let response = gateway
            .http_ui_router()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/.well-known/agent-card.json")
                    .header(header::HOST, "localhost:8080")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["url"], "http://localhost:8080");
    }

    #[tokio::test]
    async fn http_ui_router_serves_external_a2a_task_operations() {
        let gateway = gateway();
        seed_agent_card(
            &gateway,
            "support",
            "support-public",
            "support-docs",
            "support.example.com",
        )
        .await;
        let router = gateway.http_ui_router();

        let send = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/message:send")
                    .header(header::HOST, "support.example.com")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{
                            "message": {
                                "messageId": "msg-1",
                                "role": "ROLE_USER",
                                "taskId": "task-1",
                                "contextId": "ctx-1",
                                "parts": [{ "text": "hello from A2A" }]
                            },
                            "configuration": { "returnImmediately": true }
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(send.status(), StatusCode::OK);
        let body = axum::body::to_bytes(send.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["id"], "task-1");
        assert_eq!(value["contextId"], "ctx-1");
        assert_eq!(value["status"]["state"], "TASK_STATE_WORKING");
        assert_eq!(value["history"][0]["role"], "ROLE_USER");
        assert_eq!(value["history"][0]["parts"][0]["text"], "hello from A2A");

        let v1_send = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/message:send")
                    .header(header::HOST, "support.example.com")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{
                            "message": {
                                "messageId": "msg-2",
                                "role": "ROLE_USER",
                                "taskId": "task-2",
                                "contextId": "ctx-2",
                                "content": [{ "text": "hello from A2A SDK" }]
                            },
                            "configuration": { "returnImmediately": true }
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(v1_send.status(), StatusCode::OK);
        let body = axum::body::to_bytes(v1_send.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["task"]["id"], "task-2");
        assert_eq!(value["task"]["contextId"], "ctx-2");
        assert_eq!(value["task"]["history"][0]["role"], "ROLE_USER");
        assert_eq!(
            value["task"]["history"][0]["content"][0]["text"],
            "hello from A2A SDK"
        );
        assert!(value["task"]["history"][0].get("parts").is_none());

        let session_key = crate::control::keys::session("support", "support-docs", "task-1");
        let mut session = crate::gateway::rpc::models::Session::decode(
            gateway
                .kv
                .get(&session_key)
                .await
                .unwrap()
                .unwrap()
                .as_slice(),
        )
        .unwrap();
        session.status = "IDLE".to_string();
        session.last_active += 1;
        gateway
            .kv
            .set(&session_key, &session.encode_to_vec())
            .await
            .unwrap();
        gateway
            .kv
            .set(
                &crate::control::keys::session_message(
                    "support",
                    "support-docs",
                    "task-1",
                    "000-agent",
                ),
                &crate::gateway::rpc::models::SessionMessage {
                    id: "000-agent".to_string(),
                    role: crate::gateway::rpc::models::MessageRole::RoleAssistant as i32,
                    created_at: session.last_active,
                    labels: HashMap::new(),
                    parts: vec![crate::gateway::rpc::models::SessionMessagePart {
                        id: "000000".to_string(),
                        part_type: crate::gateway::rpc::models::SessionMessagePartType::Usage as i32,
                        content: String::new(),
                        name: String::new(),
                        payload_json: r#"{"input_tokens":0,"output_tokens":0,"reasoning_tokens":0,"total_tokens":0}"#.to_string(),
                        created_at: session.last_active,
                        object: None,
                    },
                    crate::gateway::rpc::models::SessionMessagePart {
                        id: "000001".to_string(),
                        part_type: crate::gateway::rpc::models::SessionMessagePartType::Text as i32,
                        content: "assistant reply".to_string(),
                        name: String::new(),
                        payload_json: String::new(),
                        created_at: session.last_active,
                        object: None,
                    }],
                }
                .encode_to_vec(),
            )
            .await
            .unwrap();

        let get_task = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/tasks/task-1")
                    .header(header::HOST, "support.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_task.status(), StatusCode::OK);
        let body = axum::body::to_bytes(get_task.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["id"], "task-1");
        assert_eq!(value["status"]["state"], "TASK_STATE_COMPLETED");
        assert_eq!(value["status"]["message"]["role"], "ROLE_AGENT");
        assert_eq!(
            value["status"]["message"]["parts"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            value["status"]["message"]["parts"][0]["text"],
            "assistant reply"
        );
        assert_eq!(value["history"][0]["role"], "ROLE_USER");
        assert_eq!(value["history"][1]["role"], "ROLE_AGENT");
        assert_eq!(value["history"][1]["parts"].as_array().unwrap().len(), 1);

        let get_v1_task = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/tasks/task-1")
                    .header(header::HOST, "support.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_v1_task.status(), StatusCode::OK);
        let body = axum::body::to_bytes(get_v1_task.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["status"]["message"]["role"], "ROLE_AGENT");
        assert_eq!(
            value["status"]["message"]["content"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            value["status"]["message"]["content"][0]["text"],
            "assistant reply"
        );
        assert!(value["status"]["message"]["content"][0]
            .get("data")
            .is_none());

        let list = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/tasks")
                    .header(header::HOST, "support.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list.status(), StatusCode::OK);
        let body = axum::body::to_bytes(list.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["tasks"][0]["id"], "task-1");

        let cancel = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/tasks/task-1:cancel")
                    .header(header::HOST, "support.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cancel.status(), StatusCode::OK);
        let body = axum::body::to_bytes(cancel.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["status"]["state"], "TASK_STATE_CANCELED");

        let stream = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/message:stream")
                    .header(header::HOST, "support.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stream.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn http_ui_router_streams_external_a2a_messages() {
        let task_id = "stream-task-1";
        let topic = crate::control::topics::session_part_topic_for_shard(
            crate::control::topics::session_part_shard(task_id),
        );
        let pubsub = Arc::new(MockPubSub::default());
        pubsub.batches.lock().await.insert(
            topic,
            VecDeque::from(vec![vec![
                SessionMessagePartEvent {
                    session_id: task_id.to_string(),
                    kind: SessionMessagePartEventKind::Delta as i32,
                    part: Some(SessionMessagePart {
                        id: "000000".to_string(),
                        part_type: SessionMessagePartType::Text as i32,
                        content: "streamed reply".to_string(),
                        name: String::new(),
                        payload_json: String::new(),
                        created_at: 1,
                        object: None,
                    }),
                    timestamp: 1,
                    agent: "support-docs".to_string(),
                    ns: "support".to_string(),
                    message_id: "assistant-stream-msg".to_string(),
                }
                .encode_to_vec(),
                SessionMessagePartEvent {
                    session_id: task_id.to_string(),
                    kind: SessionMessagePartEventKind::Done as i32,
                    part: None,
                    timestamp: 2,
                    agent: "support-docs".to_string(),
                    ns: "support".to_string(),
                    message_id: "assistant-stream-msg".to_string(),
                }
                .encode_to_vec(),
            ]]),
        );
        let gateway = gateway_with_pubsub(pubsub);
        seed_agent_card(
            &gateway,
            "support",
            "support-public",
            "support-docs",
            "support.example.com",
        )
        .await;

        let response = gateway
            .http_ui_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/message:stream")
                    .header(header::HOST, "support.example.com")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{
                            "message": {{
                                "messageId": "stream-user-msg",
                                "role": "ROLE_USER",
                                "taskId": "{task_id}",
                                "contextId": "stream-context-1",
                                "content": [{{ "text": "hello stream" }}]
                            }}
                        }}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/event-stream; charset=utf-8"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        let events = body
            .split("\n\n")
            .filter_map(|event| event.strip_prefix("data: "))
            .map(|data| serde_json::from_str::<serde_json::Value>(data).unwrap())
            .collect::<Vec<_>>();

        assert!(events.iter().any(|event| {
            event["statusUpdate"]["status"]["message"]["content"][0]["text"] == "streamed reply"
        }));
        assert_eq!(
            events.last().unwrap()["statusUpdate"]["final"],
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            events.last().unwrap()["statusUpdate"]["status"]["state"],
            "TASK_STATE_COMPLETED"
        );
    }

    #[tokio::test]
    async fn agent_card_rejects_non_public_discovery_auth() {
        let gateway = gateway();
        seed_namespace_and_agent(&gateway, "support", "support-docs").await;
        let mut card = agent_card(
            "support",
            "support-private",
            "support-docs",
            "private.example.com",
        );
        card.spec.as_mut().unwrap().auth = Some(crate::gateway::rpc::manifests::AgentCardAuth {
            discovery: "bearer".to_string(),
            operations: "bearer".to_string(),
        });

        let err = crate::gateway::rpc::GrpcGatewayHandler {
            gateway: Arc::new(gateway),
        }
        .handle_create_agent_card(tonic::Request::new(
            crate::gateway::rpc::proto::CreateAgentCardRequest {
                ns: "support".to_string(),
                card: Some(card),
            },
        ))
        .await
        .unwrap_err();

        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("authenticated discovery"));
    }

    #[tokio::test]
    async fn agent_card_rejects_unsupported_capabilities() {
        let gateway = gateway();
        seed_namespace_and_agent(&gateway, "support", "support-docs").await;
        let mut card = agent_card(
            "support",
            "support-public",
            "support-docs",
            "support.example.com",
        );
        card.spec.as_mut().unwrap().capabilities =
            Some(crate::gateway::rpc::manifests::AgentCardCapabilities {
                streaming: true,
                push_notifications: false,
                extended_agent_card: false,
            });

        let err = crate::gateway::rpc::GrpcGatewayHandler {
            gateway: Arc::new(gateway),
        }
        .handle_create_agent_card(tonic::Request::new(
            crate::gateway::rpc::proto::CreateAgentCardRequest {
                ns: "support".to_string(),
                card: Some(card),
            },
        ))
        .await
        .unwrap_err();

        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("streaming is not supported"));
    }

    #[tokio::test]
    async fn agent_card_hostname_claim_reuses_stale_index_but_rejects_live_owner() {
        let gateway = gateway();
        seed_namespace_and_agent(&gateway, "support", "support-docs").await;
        seed_namespace_and_agent(&gateway, "sales", "sales-docs").await;
        let handler = crate::gateway::rpc::GrpcGatewayHandler {
            gateway: Arc::new(gateway.clone()),
        };

        let stale = agent_card(
            "support",
            "deleted-card",
            "support-docs",
            "shared.example.com",
        );
        gateway
            .kv
            .set(
                &crate::control::keys::agent_card_hostname("shared.example.com"),
                &stale.encode_to_vec(),
            )
            .await
            .unwrap();
        let support_card = agent_card(
            "support",
            "support-public",
            "support-docs",
            "shared.example.com",
        );
        handler
            .handle_create_agent_card(tonic::Request::new(
                crate::gateway::rpc::proto::CreateAgentCardRequest {
                    ns: "support".to_string(),
                    card: Some(support_card),
                },
            ))
            .await
            .unwrap();

        let sales_card = agent_card("sales", "sales-public", "sales-docs", "shared.example.com");
        let err = handler
            .handle_create_agent_card(tonic::Request::new(
                crate::gateway::rpc::proto::CreateAgentCardRequest {
                    ns: "sales".to_string(),
                    card: Some(sales_card),
                },
            ))
            .await
            .unwrap_err();

        assert_eq!(err.code(), tonic::Code::AlreadyExists);
        assert!(err.message().contains("already claimed"));
    }

    #[tokio::test]
    async fn agent_card_create_does_not_persist_primary_when_hostname_claim_fails() {
        let gateway = gateway();
        seed_namespace_and_agent(&gateway, "support", "support-docs").await;
        seed_namespace_and_agent(&gateway, "sales", "sales-docs").await;
        let handler = crate::gateway::rpc::GrpcGatewayHandler {
            gateway: Arc::new(gateway.clone()),
        };
        handler
            .handle_create_agent_card(tonic::Request::new(
                crate::gateway::rpc::proto::CreateAgentCardRequest {
                    ns: "support".to_string(),
                    card: Some(agent_card(
                        "support",
                        "support-public",
                        "support-docs",
                        "shared.example.com",
                    )),
                },
            ))
            .await
            .unwrap();

        let err = handler
            .handle_create_agent_card(tonic::Request::new(
                crate::gateway::rpc::proto::CreateAgentCardRequest {
                    ns: "sales".to_string(),
                    card: Some(agent_card(
                        "sales",
                        "sales-public",
                        "sales-docs",
                        "shared.example.com",
                    )),
                },
            ))
            .await
            .unwrap_err();

        assert_eq!(err.code(), tonic::Code::AlreadyExists);
        assert!(gateway
            .kv
            .get(&crate::control::keys::agent_card("sales", "sales-public"))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn agent_card_update_cleans_old_hostname_index() {
        let gateway = gateway();
        seed_namespace_and_agent(&gateway, "support", "support-docs").await;
        let handler = crate::gateway::rpc::GrpcGatewayHandler {
            gateway: Arc::new(gateway.clone()),
        };
        handler
            .handle_create_agent_card(tonic::Request::new(
                crate::gateway::rpc::proto::CreateAgentCardRequest {
                    ns: "support".to_string(),
                    card: Some(agent_card(
                        "support",
                        "support-public",
                        "support-docs",
                        "old.example.com",
                    )),
                },
            ))
            .await
            .unwrap();
        handler
            .handle_create_agent_card(tonic::Request::new(
                crate::gateway::rpc::proto::CreateAgentCardRequest {
                    ns: "support".to_string(),
                    card: Some(agent_card(
                        "support",
                        "support-public",
                        "support-docs",
                        "new.example.com",
                    )),
                },
            ))
            .await
            .unwrap();

        assert!(gateway
            .kv
            .get(&crate::control::keys::agent_card_hostname(
                "old.example.com"
            ))
            .await
            .unwrap()
            .is_none());
        assert!(gateway
            .kv
            .get(&crate::control::keys::agent_card_hostname(
                "new.example.com"
            ))
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn agent_card_delete_cleans_hostname_index() {
        let gateway = gateway();
        seed_namespace_and_agent(&gateway, "support", "support-docs").await;
        let handler = crate::gateway::rpc::GrpcGatewayHandler {
            gateway: Arc::new(gateway.clone()),
        };
        handler
            .handle_create_agent_card(tonic::Request::new(
                crate::gateway::rpc::proto::CreateAgentCardRequest {
                    ns: "support".to_string(),
                    card: Some(agent_card(
                        "support",
                        "support-public",
                        "support-docs",
                        "support.example.com",
                    )),
                },
            ))
            .await
            .unwrap();
        handler
            .handle_delete_agent_card(tonic::Request::new(
                crate::gateway::rpc::proto::DeleteAgentCardRequest {
                    ns: "support".to_string(),
                    name: "support-public".to_string(),
                },
            ))
            .await
            .unwrap();

        assert!(gateway
            .kv
            .get(&crate::control::keys::agent_card_hostname(
                "support.example.com"
            ))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn http_ui_router_ignores_stale_agent_card_hostname_index() {
        let gateway = gateway();
        let stale_card = crate::gateway::rpc::manifests::AgentCard {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "AgentCard".to_string(),
            metadata: Some(crate::gateway::rpc::manifests::ObjectMeta {
                name: "stale-public".to_string(),
                namespace: "support".to_string(),
                labels: HashMap::new(),
                annotations: HashMap::new(),
            }),
            spec: Some(crate::gateway::rpc::manifests::AgentCardSpec {
                agent_ref: "support-docs".to_string(),
                hostname: "stale.example.com".to_string(),
                name: "Stale Agent".to_string(),
                description: String::new(),
                version: String::new(),
                capabilities: None,
                default_input_modes: Vec::new(),
                default_output_modes: Vec::new(),
                skills: Vec::new(),
                auth: None,
            }),
        };
        gateway
            .kv
            .set(
                &crate::control::keys::agent_card_hostname("stale.example.com"),
                &stale_card.encode_to_vec(),
            )
            .await
            .unwrap();

        let response = gateway
            .http_ui_router()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/.well-known/agent-card.json")
                    .header(header::HOST, "stale.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn http_ui_router_allows_browser_preflight() {
        let response = gateway()
            .http_ui_router()
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/v1/ui/ns/default/agents/agent/sessions/session-1")
                    .header(header::ORIGIN, "http://127.0.0.1:3000")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .header(
                        header::ACCESS_CONTROL_REQUEST_HEADERS,
                        "authorization,content-type",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(response.status().is_success());
        assert_eq!(
            response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&"*".parse().unwrap())
        );
    }

    #[tokio::test]
    async fn start_servers_surface_invalid_addresses() {
        let gateway = gateway();

        let rpc_err = gateway
            .start_rpc_server("definitely-not-an-address")
            .await
            .unwrap_err()
            .to_string();
        assert!(!rpc_err.is_empty());

        let http_err = gateway
            .start_http_ui_server("definitely-not-an-address")
            .await
            .unwrap_err()
            .to_string();
        assert!(!http_err.is_empty());
    }

    async fn wait_for_connect(addr: SocketAddr) {
        for _ in 0..40 {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                return;
            }
            sleep(Duration::from_millis(25)).await;
        }
        panic!("server did not start listening on {}", addr);
    }

    #[tokio::test]
    async fn start_http_ui_server_listens_on_valid_address() {
        let gateway = gateway();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let task = tokio::spawn({
            let gateway = gateway.clone();
            async move { gateway.start_http_ui_server(&addr.to_string()).await }
        });

        wait_for_connect(addr).await;
        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn start_rpc_server_listens_on_valid_address() {
        let gateway = gateway();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let task = tokio::spawn({
            let gateway = gateway.clone();
            async move { gateway.start_rpc_server(&addr.to_string()).await }
        });

        wait_for_connect(addr).await;
        task.abort();
        let _ = task.await;
    }
}
