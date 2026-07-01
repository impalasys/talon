// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::security::platform_jwt;
use crate::gateway::server::Gateway;
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde_json::json;
use std::sync::Arc;
use url::Url;

const TALON_GATEWAY_BASE_URL_ENV: &str = "TALON_GATEWAY_BASE_URL";

pub fn router() -> Router<Arc<Gateway>> {
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
    let base_url = match external_base_url(&issuer) {
        Ok(base_url) => base_url,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("gateway base URL is not configured: {err}")})),
            )
                .into_response();
        }
    };
    let jwks_uri = format!("{base_url}/.well-known/jwks.json");
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
    let base_url = match external_base_url(&issuer) {
        Ok(base_url) => base_url,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("gateway base URL is not configured: {err}")})),
            )
                .into_response();
        }
    };
    Json(json!({
        "resource": base_url.clone(),
        "authorization_servers": [issuer],
        "jwks_uri": format!("{base_url}/.well-known/jwks.json"),
    }))
    .into_response()
}

fn platform_issuer() -> anyhow::Result<String> {
    platform_jwt::issuer()
}

fn external_base_url(issuer: &str) -> anyhow::Result<String> {
    let configured = std::env::var(TALON_GATEWAY_BASE_URL_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let base_url = configured.unwrap_or_else(|| issuer.trim().to_string());
    let parsed = Url::parse(&base_url).map_err(|err| {
        anyhow::anyhow!("{TALON_GATEWAY_BASE_URL_ENV} must be a valid URL: {err}")
    })?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        anyhow::bail!("{TALON_GATEWAY_BASE_URL_ENV} must use http or https");
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        anyhow::bail!("{TALON_GATEWAY_BASE_URL_ENV} must not include query or fragment");
    }
    Ok(base_url.trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{EmptyPubSub, MockKvStore};
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    const TEST_PLATFORM_ISSUER: &str = "https://talon.example.com";

    struct PlatformJwtEnvGuard {
        previous_private_key: Option<String>,
        previous_issuer: Option<String>,
        previous_gateway_base_url: Option<String>,
    }

    impl PlatformJwtEnvGuard {
        fn acquire() -> Self {
            let previous_private_key =
                std::env::var(platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV).ok();
            let previous_issuer = std::env::var(platform_jwt::TALON_PLATFORM_JWT_ISSUER_ENV).ok();
            let previous_gateway_base_url = std::env::var(TALON_GATEWAY_BASE_URL_ENV).ok();
            unsafe {
                std::env::set_var(
                    platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
                    platform_jwt::TEST_RSA_PRIVATE_KEY,
                );
                std::env::set_var(
                    platform_jwt::TALON_PLATFORM_JWT_ISSUER_ENV,
                    TEST_PLATFORM_ISSUER,
                );
                std::env::set_var(TALON_GATEWAY_BASE_URL_ENV, "https://gateway.example.com");
            }
            Self {
                previous_private_key,
                previous_issuer,
                previous_gateway_base_url,
            }
        }
    }

    impl Drop for PlatformJwtEnvGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(previous_private_key) = &self.previous_private_key {
                    std::env::set_var(
                        platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
                        previous_private_key,
                    );
                } else {
                    std::env::remove_var(platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV);
                }
                if let Some(previous_issuer) = &self.previous_issuer {
                    std::env::set_var(platform_jwt::TALON_PLATFORM_JWT_ISSUER_ENV, previous_issuer);
                } else {
                    std::env::remove_var(platform_jwt::TALON_PLATFORM_JWT_ISSUER_ENV);
                }
                if let Some(previous_gateway_base_url) = &self.previous_gateway_base_url {
                    std::env::set_var(TALON_GATEWAY_BASE_URL_ENV, previous_gateway_base_url);
                } else {
                    std::env::remove_var(TALON_GATEWAY_BASE_URL_ENV);
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
        let app = router().with_state(gateway_with_platform_jwt());

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
