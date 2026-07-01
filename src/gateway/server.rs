// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::search::DocumentStore;
use crate::control::security::platform_jwt;
use crate::control::{
    config::proto::TrustConfig, object_store::ObjectStore, scheduler::SchedulerBackend,
    ControlPlane, KeyValueStore, MessagePublisher,
};
use crate::gateway::auth::AuthConfig;
use anyhow::Result;
use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, HeaderValue, Request, Response, StatusCode},
    response::IntoResponse,
    routing::{get, RouterIntoService},
    Json, Router,
};
use serde_json::json;
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
                interceptor.clone(),
            ),
        );
        let connector_service = tonic_web::enable(
            crate::gateway::rpc::proto::connector_service_server::ConnectorServiceServer::with_interceptor(
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
            .add_service(connector_service)
            .add_service(auth_service)
            .into_service::<BoxBody>();

        let app = well_known_router()
            .merge(crate::gateway::rest::a2a::router())
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

fn well_known_router() -> Router<Arc<Gateway>> {
    Router::new()
        .route("/.well-known/jwks.json", get(jwks))
        .route(
            "/.well-known/oauth-authorization-server",
            get(authorization_server_metadata),
        )
        .route(
            "/.well-known/openid-configuration",
            get(authorization_server_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource/*tail",
            get(protected_resource_metadata),
        )
}

async fn jwks(State(gateway): State<Arc<Gateway>>) -> axum::response::Response {
    let _ = gateway;
    if std::env::var(platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV)
        .ok()
        .is_none_or(|value| value.trim().is_empty())
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    match platform_jwt::load_key() {
        Ok(key) => Json(key.jwks()).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("platform JWT key is not configured: {err}")})),
        )
            .into_response(),
    }
}

async fn authorization_server_metadata(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let _ = gateway;
    let issuer = match platform_issuer() {
        Ok(issuer) => issuer,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("platform JWT issuer is not configured: {err}")})),
            )
                .into_response();
        }
    };
    let jwks_uri = format!("{}/.well-known/jwks.json", external_base_url(&headers));
    Json(json!({
        "issuer": issuer,
        "jwks_uri": jwks_uri,
        "response_types_supported": [],
        "grant_types_supported": [],
        "token_endpoint_auth_methods_supported": [],
    }))
    .into_response()
}

async fn protected_resource_metadata(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let _ = gateway;
    let issuer = match platform_issuer() {
        Ok(issuer) => issuer,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("platform JWT issuer is not configured: {err}")})),
            )
                .into_response();
        }
    };
    Json(json!({
        "resource": external_base_url(&headers),
        "authorization_servers": [issuer],
        "jwks_uri": format!("{}/.well-known/jwks.json", external_base_url(&headers)),
    }))
    .into_response()
}

fn platform_issuer() -> anyhow::Result<String> {
    platform_jwt::issuer()
}

#[cfg(test)]
mod well_known_tests {
    use super::*;
    use crate::test_support::{EmptyPubSub, MockKvStore};
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    const TEST_PLATFORM_ISSUER: &str = "https://talon.example.com";

    struct PlatformJwtEnvGuard {
        previous_private_key: Option<String>,
        previous_issuer: Option<String>,
    }

    impl PlatformJwtEnvGuard {
        fn acquire() -> Self {
            let previous_private_key = std::env::var(
                crate::control::security::platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
            )
            .ok();
            let previous_issuer = std::env::var(
                crate::control::security::platform_jwt::TALON_PLATFORM_JWT_ISSUER_ENV,
            )
            .ok();
            unsafe {
                std::env::set_var(
                    crate::control::security::platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
                    crate::control::security::platform_jwt::TEST_RSA_PRIVATE_KEY,
                );
                std::env::set_var(
                    crate::control::security::platform_jwt::TALON_PLATFORM_JWT_ISSUER_ENV,
                    TEST_PLATFORM_ISSUER,
                );
            }
            Self {
                previous_private_key,
                previous_issuer,
            }
        }
    }

    impl Drop for PlatformJwtEnvGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(previous_private_key) = &self.previous_private_key {
                    std::env::set_var(
                        crate::control::security::platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
                        previous_private_key,
                    );
                } else {
                    std::env::remove_var(
                        crate::control::security::platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
                    );
                }
                if let Some(previous_issuer) = &self.previous_issuer {
                    std::env::set_var(
                        crate::control::security::platform_jwt::TALON_PLATFORM_JWT_ISSUER_ENV,
                        previous_issuer,
                    );
                } else {
                    std::env::remove_var(
                        crate::control::security::platform_jwt::TALON_PLATFORM_JWT_ISSUER_ENV,
                    );
                }
            }
        }
    }

    fn gateway_with_platform_jwt() -> Arc<Gateway> {
        let cp = crate::control::ControlPlane::builder(
            Arc::new(MockKvStore::default()),
            Arc::new(EmptyPubSub),
        )
        .build();
        Arc::new(Gateway::new_with_trust(
            None,
            None,
            cp.kv,
            cp.pubsub,
            cp.scheduler,
            cp.objects,
            cp.documents,
        ))
    }

    async fn json_response(app: Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let response = app
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("host", "gateway.example.com")
                    .header("x-forwarded-proto", "https")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&body).unwrap();
        (status, json)
    }

    #[tokio::test]
    async fn well_known_endpoints_publish_public_platform_jwks_and_metadata() {
        let _env_lock = crate::test_support::async_env_mutex().lock().await;
        let _guard = PlatformJwtEnvGuard::acquire();
        let app = well_known_router().with_state(gateway_with_platform_jwt());

        let (status, jwks) = json_response(app.clone(), "/.well-known/jwks.json").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(jwks["keys"].as_array().unwrap().len(), 1);
        assert_eq!(jwks["keys"][0]["use"], "sig");
        assert_eq!(jwks["keys"][0]["alg"], "RS256");
        assert!(jwks["keys"][0].get("n").is_some());
        assert!(jwks["keys"][0].get("e").is_some());
        assert!(jwks["keys"][0].get("d").is_none());
        assert!(jwks["keys"][0].get("p").is_none());

        let (status, metadata) =
            json_response(app.clone(), "/.well-known/openid-configuration").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(metadata["issuer"], TEST_PLATFORM_ISSUER);
        assert_eq!(
            metadata["jwks_uri"],
            "https://gateway.example.com/.well-known/jwks.json"
        );

        let (status, protected) =
            json_response(app, "/.well-known/oauth-protected-resource/talon").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(protected["authorization_servers"][0], TEST_PLATFORM_ISSUER);
        assert_eq!(
            protected["jwks_uri"],
            "https://gateway.example.com/.well-known/jwks.json"
        );
    }
}

fn external_base_url(headers: &HeaderMap) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| value.eq_ignore_ascii_case("http") || value.eq_ignore_ascii_case("https"))
        .unwrap_or("https");
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("localhost");
    format!("{scheme}://{host}")
}

fn grpc_fallback_service<S>(grpc_service: S) -> RouterIntoService<Body, ()>
where
    S: Service<Request<BoxBody>, Response = Response<BoxBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: std::fmt::Display + Send + Sync + 'static,
{
    Router::new()
        .fallback_service(tower::service_fn(move |mut request: Request<Body>| {
            let mut grpc_service = grpc_service.clone();
            async move {
                mark_grpc_web_origin_scope(&mut request);
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

fn mark_grpc_web_origin_scope(request: &mut Request<Body>) {
    request
        .headers_mut()
        .remove(crate::gateway::auth::TALON_GRPC_WEB_REQUEST_METADATA);
    request
        .headers_mut()
        .remove(crate::gateway::auth::TALON_GRPC_WEB_ORIGIN_METADATA);

    let is_grpc_web = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|content_type| content_type.starts_with("application/grpc-web"));
    if !is_grpc_web {
        return;
    }

    request.headers_mut().insert(
        crate::gateway::auth::TALON_GRPC_WEB_REQUEST_METADATA,
        HeaderValue::from_static("true"),
    );
    if let Some(origin) = request.headers().get(header::ORIGIN).cloned() {
        request
            .headers_mut()
            .insert(crate::gateway::auth::TALON_GRPC_WEB_ORIGIN_METADATA, origin);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_grpc_web_origin_scope_marks_only_grpc_web_requests() {
        let mut grpc_web = Request::builder()
            .header(header::CONTENT_TYPE, "application/grpc-web+proto")
            .header(header::ORIGIN, "https://app.example.com")
            .body(Body::empty())
            .unwrap();
        mark_grpc_web_origin_scope(&mut grpc_web);
        assert_eq!(
            grpc_web
                .headers()
                .get(crate::gateway::auth::TALON_GRPC_WEB_REQUEST_METADATA)
                .and_then(|value| value.to_str().ok()),
            Some("true")
        );
        assert_eq!(
            grpc_web
                .headers()
                .get(crate::gateway::auth::TALON_GRPC_WEB_ORIGIN_METADATA)
                .and_then(|value| value.to_str().ok()),
            Some("https://app.example.com")
        );

        let mut native_grpc = Request::builder()
            .header(header::CONTENT_TYPE, "application/grpc")
            .header(header::ORIGIN, "https://app.example.com")
            .header(
                crate::gateway::auth::TALON_GRPC_WEB_REQUEST_METADATA,
                "true",
            )
            .header(
                crate::gateway::auth::TALON_GRPC_WEB_ORIGIN_METADATA,
                "https://evil.example.com",
            )
            .body(Body::empty())
            .unwrap();
        mark_grpc_web_origin_scope(&mut native_grpc);
        assert!(native_grpc
            .headers()
            .get(crate::gateway::auth::TALON_GRPC_WEB_REQUEST_METADATA)
            .is_none());
        assert!(native_grpc
            .headers()
            .get(crate::gateway::auth::TALON_GRPC_WEB_ORIGIN_METADATA)
            .is_none());
    }
}
