// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::search::DocumentStore;
use crate::control::{
    config::proto::TrustConfig, object_store::ObjectStore, scheduler::SchedulerBackend,
    ControlPlane, KeyValueStore, MessagePublisher,
};
use crate::gateway::auth::AuthConfig;
use anyhow::Result;
use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
    routing::RouterIntoService,
    Router,
};
use std::convert::Infallible;
use std::sync::Arc;
use tonic::body::BoxBody;
use tower::{Service, ServiceExt};
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
pub struct Gateway {
    pub auth_config: Option<AuthConfig>,
    pub trust_config: Option<TrustConfig>,
    pub kv: Arc<dyn KeyValueStore + Send + Sync>,
    pub pubsub: Arc<dyn MessagePublisher + Send + Sync>,
    pub scheduler: Arc<dyn SchedulerBackend + Send + Sync>,
    pub objects: Arc<dyn ObjectStore + Send + Sync>,
    pub documents: Arc<dyn DocumentStore + Send + Sync>,
}

impl Gateway {
    pub fn new(
        auth_config: Option<AuthConfig>,
        kv: Arc<dyn KeyValueStore + Send + Sync>,
        pubsub: Arc<dyn MessagePublisher + Send + Sync>,
        scheduler: Arc<dyn SchedulerBackend + Send + Sync>,
        objects: Arc<dyn ObjectStore + Send + Sync>,
        documents: Arc<dyn DocumentStore + Send + Sync>,
    ) -> Self {
        Self::new_with_trust(auth_config, None, kv, pubsub, scheduler, objects, documents)
    }

    pub fn new_with_trust(
        auth_config: Option<AuthConfig>,
        trust_config: Option<TrustConfig>,
        kv: Arc<dyn KeyValueStore + Send + Sync>,
        pubsub: Arc<dyn MessagePublisher + Send + Sync>,
        scheduler: Arc<dyn SchedulerBackend + Send + Sync>,
        objects: Arc<dyn ObjectStore + Send + Sync>,
        documents: Arc<dyn DocumentStore + Send + Sync>,
    ) -> Self {
        Self {
            auth_config,
            trust_config,
            kv,
            pubsub,
            scheduler,
            objects,
            documents,
        }
    }

    pub fn from_control_plane(
        auth_config: Option<AuthConfig>,
        control_plane: ControlPlane,
    ) -> Self {
        Self::new(
            auth_config,
            control_plane.kv,
            control_plane.pubsub,
            control_plane.scheduler,
            control_plane.objects,
            control_plane.documents,
        )
    }

    pub(crate) fn clone_internal(&self) -> Self {
        Self {
            auth_config: self.auth_config.clone(),
            trust_config: self.trust_config.clone(),
            kv: self.kv.clone(),
            pubsub: self.pubsub.clone(),
            scheduler: self.scheduler.clone(),
            objects: self.objects.clone(),
            documents: self.documents.clone(),
        }
    }

    pub fn control_plane(&self) -> ControlPlane {
        ControlPlane::new(
            self.kv.clone(),
            self.pubsub.clone(),
            self.scheduler.clone(),
            self.objects.clone(),
            self.documents.clone(),
        )
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
        let addr: std::net::SocketAddr = addr.parse()?;
        println!("Talon gateway listening on: {}", addr);

        let handler = crate::gateway::rpc::GrpcGatewayHandler {
            gateway: Arc::new(self.clone_internal()),
        };
        let http_gateway = handler.gateway.clone();

        let auth_config = self.auth_config.clone().unwrap_or_else(AuthConfig::open);
        let interceptor = crate::gateway::auth::TalonAuthInterceptor {
            config: auth_config,
        };

        let namespace_service = tonic_web::enable(
            crate::gateway::rpc::proto::namespace_service_server::NamespaceServiceServer::with_interceptor(
                handler.clone(),
                interceptor.clone(),
            ),
        );
        let resource_service = tonic_web::enable(
            crate::gateway::rpc::proto::resource_service_server::ResourceServiceServer::with_interceptor(
                handler.clone(),
                interceptor.clone(),
            ),
        );
        let session_service = tonic_web::enable(
            crate::gateway::rpc::proto::session_service_server::SessionServiceServer::with_interceptor(
                handler.clone(),
                interceptor.clone(),
            ),
        );
        let channel_service = tonic_web::enable(
            crate::gateway::rpc::proto::channel_service_server::ChannelServiceServer::with_interceptor(
                handler.clone(),
                interceptor.clone(),
            ),
        );
        let workflow_service = tonic_web::enable(
            crate::gateway::rpc::proto::workflow_service_server::WorkflowServiceServer::with_interceptor(
                handler.clone(),
                interceptor.clone(),
            ),
        );
        let knowledge_service = tonic_web::enable(
            crate::gateway::rpc::proto::knowledge_service_server::KnowledgeServiceServer::with_interceptor(
                handler.clone(),
                interceptor.clone(),
            ),
        );
        let search_service = tonic_web::enable(
            crate::gateway::rpc::proto::search_service_server::SearchServiceServer::with_interceptor(
                handler.clone(),
                interceptor,
            ),
        );
        let auth_service = tonic_web::enable(
            crate::gateway::rpc::proto::auth_service_server::AuthServiceServer::new(handler),
        );

        let grpc_service = Server::builder()
            .accept_http1(true)
            .add_service(namespace_service)
            .add_service(resource_service)
            .add_service(session_service)
            .add_service(channel_service)
            .add_service(workflow_service)
            .add_service(knowledge_service)
            .add_service(search_service)
            .add_service(auth_service)
            .into_service::<BoxBody>();

        let app = crate::gateway::rest::a2a::router()
            .with_state(http_gateway)
            .fallback_service(grpc_fallback_service(grpc_service))
            .layer(permissive_cors_layer());

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(|e| anyhow::anyhow!("Gateway server failed: {}", e))?;

        Ok(())
    }
}

fn grpc_fallback_service<S>(grpc_service: S) -> RouterIntoService<Body, ()>
where
    S: Service<Request<BoxBody>, Response = Response<BoxBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: std::fmt::Display + Send + Sync + 'static,
{
    Router::new()
        .fallback_service(tower::service_fn(move |request: Request<Body>| {
            let mut grpc_service = grpc_service.clone();
            async move {
                let request = request.map(tonic::body::boxed);
                let response = match grpc_service.ready().await {
                    Ok(ready_service) => ready_service.call(request).await,
                    Err(err) => {
                        tracing::error!(%err, "gRPC gateway service was not ready");
                        return Ok::<_, Infallible>(internal_server_error());
                    }
                };
                match response {
                    Ok(response) => Ok(response.into_response()),
                    Err(err) => {
                        tracing::error!(%err, "gRPC gateway service failed");
                        Ok(internal_server_error())
                    }
                }
            }
        }))
        .into_service()
}

fn internal_server_error() -> axum::response::Response {
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

fn permissive_cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any)
        .expose_headers(Any)
}
