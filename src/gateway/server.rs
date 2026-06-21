// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{
    config::proto::TrustConfig, object_store::ObjectStore, scheduler::SchedulerBackend,
    ControlPlane, KeyValueStore, MessagePublisher,
};
use crate::gateway::auth::AuthConfig;
use crate::gateway::session_streams::SessionStreamHub;
use anyhow::Result;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
pub struct Gateway {
    pub auth_config: Option<AuthConfig>,
    pub trust_config: Option<TrustConfig>,
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
        Self::new_with_trust(auth_config, None, kv, pubsub, scheduler, objects)
    }

    pub fn new_with_trust(
        auth_config: Option<AuthConfig>,
        trust_config: Option<TrustConfig>,
        kv: Arc<dyn KeyValueStore + Send + Sync>,
        pubsub: Arc<dyn MessagePublisher + Send + Sync>,
        scheduler: Arc<dyn SchedulerBackend + Send + Sync>,
        objects: Arc<dyn ObjectStore + Send + Sync>,
    ) -> Self {
        let session_streams = Arc::new(SessionStreamHub::new(pubsub.clone()));
        Self {
            auth_config,
            trust_config,
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
            trust_config: self.trust_config.clone(),
            kv: self.kv.clone(),
            pubsub: self.pubsub.clone(),
            scheduler: self.scheduler.clone(),
            objects: self.objects.clone(),
            session_streams: self.session_streams.clone(),
        }
    }

    pub fn control_plane(&self) -> ControlPlane {
        ControlPlane::new(
            self.kv.clone(),
            self.pubsub.clone(),
            self.scheduler.clone(),
            self.objects.clone(),
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
        let addr = addr.parse()?;
        println!("gRPC Gateway listening on: {}", addr);

        let handler = crate::gateway::rpc::GrpcGatewayHandler {
            gateway: Arc::new(self.clone_internal()),
        };

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
                interceptor,
            ),
        );
        let auth_service = tonic_web::enable(
            crate::gateway::rpc::proto::auth_service_server::AuthServiceServer::new(handler),
        );

        Server::builder()
            .accept_http1(true)
            .layer(permissive_cors_layer())
            .add_service(namespace_service)
            .add_service(resource_service)
            .add_service(session_service)
            .add_service(channel_service)
            .add_service(workflow_service)
            .add_service(knowledge_service)
            .add_service(auth_service)
            .serve_with_shutdown(addr, shutdown)
            .await
            .map_err(|e| anyhow::anyhow!("Tonic server failed: {}", e))?;

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
