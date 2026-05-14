use crate::control::{scheduler::SchedulerBackend, KeyValueStore, MessagePublisher};
use crate::gateway::auth::AuthConfig;
use crate::gateway::session_streams::SessionStreamHub;
use anyhow::Result;
use std::sync::Arc;

pub struct Gateway {
    pub auth_config: Option<AuthConfig>,
    pub kv: Arc<dyn KeyValueStore + Send + Sync>,
    pub pubsub: Arc<dyn MessagePublisher + Send + Sync>,
    pub scheduler: Arc<dyn SchedulerBackend + Send + Sync>,
    pub session_streams: Arc<SessionStreamHub>,
}

impl Gateway {
    pub fn new(
        auth_config: Option<AuthConfig>,
        kv: Arc<dyn KeyValueStore + Send + Sync>,
        pubsub: Arc<dyn MessagePublisher + Send + Sync>,
        scheduler: Arc<dyn SchedulerBackend + Send + Sync>,
    ) -> Self {
        let session_streams = Arc::new(SessionStreamHub::new(pubsub.clone()));
        Self {
            auth_config,
            kv,
            pubsub,
            scheduler,
            session_streams,
        }
    }

    pub(crate) fn clone_internal(&self) -> Self {
        Self {
            auth_config: self.auth_config.clone(),
            kv: self.kv.clone(),
            pubsub: self.pubsub.clone(),
            scheduler: self.scheduler.clone(),
            session_streams: self.session_streams.clone(),
        }
    }

    pub async fn start_rpc_server(&self, addr: &str) -> Result<()> {
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

        let cors = tower_http::cors::CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any)
            .allow_methods(tower_http::cors::Any)
            .expose_headers(tower_http::cors::Any);

        Server::builder()
            .accept_http1(true)
            .layer(cors)
            .add_service(svc)
            .serve(addr)
            .await
            .map_err(|e| anyhow::anyhow!("Tonic server failed: {}", e))?;

        Ok(())
    }
}
