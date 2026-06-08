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
        keys::{ResourceKey, ResourceList},
        scheduler::NoopSchedulerBackend,
        KeyValueStore, MessagePublisher,
    };
    use crate::gateway::auth::AuthConfig;
    use axum::body::Body;
    use axum::http::{header, Method, Request, StatusCode};
    use futures::stream;
    use prost::Message;
    use std::collections::HashMap;
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
    struct MockPubSub;

    #[async_trait::async_trait]
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

    fn gateway() -> Gateway {
        Gateway::new(
            None,
            Arc::new(MockKvStore::default()),
            Arc::new(MockPubSub),
            Arc::new(NoopSchedulerBackend),
            crate::control::object_store::default_object_store(),
        )
    }

    #[test]
    fn new_preserves_auth_config_and_initializes_session_streams() {
        let gateway = Gateway::new(
            Some(AuthConfig::tokens(vec!["secret-token".to_string()])),
            Arc::new(MockKvStore::default()),
            Arc::new(MockPubSub),
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
        gateway
            .kv
            .set(
                &crate::control::keys::namespace_metadata("support"),
                &crate::gateway::rpc::models::Namespace {
                    name: "support".to_string(),
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
                &crate::control::keys::agent_card("support", "support-public"),
                &crate::gateway::rpc::manifests::AgentCard {
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
                            streaming: true,
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
                        auth: None,
                    }),
                }
                .encode_to_vec(),
            )
            .await
            .unwrap();

        let response = gateway
            .http_ui_router()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/.well-known/agent-card.json")
                    .header(header::HOST, "support.example.com")
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
        assert_eq!(value["capabilities"]["streaming"], true);
        assert_eq!(value["skills"][0]["id"], "answer_support_question");
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
