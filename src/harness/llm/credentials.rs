// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Bearer-token providers for LLM API clients.
//!
//! The shared client boundary deliberately knows only how to obtain a token;
//! cloud identity details belong in provider implementations such as `OpenAiWif`.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

#[async_trait]
pub trait LlmCredentialProvider: Send + Sync {
    async fn bearer_token(&self) -> Result<String>;
}

pub struct StaticApiKey(String);

impl StaticApiKey {
    pub fn new(api_key: String) -> Self {
        Self(api_key)
    }
}

#[async_trait]
impl LlmCredentialProvider for StaticApiKey {
    async fn bearer_token(&self) -> Result<String> {
        Ok(self.0.clone())
    }
}

/// OpenAI WIF credential provider for Google Cloud metadata-server ID tokens.
///
/// Operators must first create an OpenAI Workload Identity Provider trusting
/// `https://accounts.google.com`, then map this workload's stable `sub` claim
/// to an OpenAI project service account. The mapping must explicitly grant the
/// API permissions this workload needs (for model calls, `api.model.request`);
/// an empty permissions list inherits the mapped service account's roles.
/// See https://developers.openai.com/api/docs/guides/workload-identity-federation/google-cloud.
pub struct OpenAiWif {
    identity_provider_id: String,
    service_account_id: String,
    audience: String,
    metadata_endpoint: String,
    exchange_endpoint: String,
    refresh_buffer_seconds: u64,
    http: reqwest::Client,
    cached: Mutex<Option<CachedToken>>,
}

struct CachedToken {
    value: String,
    refresh_at: u64,
}

#[derive(Deserialize)]
struct ExchangeResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    #[serde(default)]
    scope: String,
}

impl OpenAiWif {
    pub fn new(
        identity_provider_id: String,
        service_account_id: String,
        audience: String,
    ) -> Result<Self> {
        Self::with_endpoints(
            identity_provider_id,
            service_account_id,
            audience,
            "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/identity".to_string(),
            "https://auth.openai.com/oauth/token".to_string(),
        )
    }

    fn with_endpoints(
        identity_provider_id: String,
        service_account_id: String,
        audience: String,
        metadata_endpoint: String,
        exchange_endpoint: String,
    ) -> Result<Self> {
        for (name, value) in [
            ("OPENAI_IDENTITY_PROVIDER_ID", &identity_provider_id),
            ("OPENAI_SERVICE_ACCOUNT_ID", &service_account_id),
            ("OPENAI_WIF_AUDIENCE", &audience),
        ] {
            if value.trim().is_empty() {
                return Err(anyhow!(
                    "{name} must not be empty when OpenAI WIF is enabled"
                ));
            }
        }
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("Failed to initialize OpenAI WIF HTTP client")?;
        Ok(Self {
            identity_provider_id,
            service_account_id,
            audience,
            metadata_endpoint,
            exchange_endpoint,
            refresh_buffer_seconds: 120,
            http,
            cached: Mutex::new(None),
        })
    }

    async fn refresh(&self) -> Result<CachedToken> {
        let mut metadata_url = reqwest::Url::parse(&self.metadata_endpoint)
            .context("Invalid Google metadata token endpoint")?;
        metadata_url
            .query_pairs_mut()
            .append_pair("audience", &self.audience)
            .append_pair("format", "full");
        let identity_response = self
            .http
            .get(metadata_url)
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .context("Failed to request Google metadata identity token")?
            .error_for_status()
            .context("Google metadata identity token request was rejected")?;
        let subject_token = identity_response
            .text()
            .await
            .context("Failed to read Google metadata identity token")?;
        let subject_token = subject_token.trim();
        if subject_token.is_empty() {
            return Err(anyhow!(
                "Google metadata server returned an empty identity token"
            ));
        }

        let response = self
            .http
            .post(&self.exchange_endpoint)
            .json(&serde_json::json!({
                "grant_type": "urn:ietf:params:oauth:grant-type:token-exchange",
                "subject_token_type": "urn:ietf:params:oauth:token-type:id_token",
                "subject_token": subject_token,
                "identity_provider_id": self.identity_provider_id,
                "service_account_id": self.service_account_id,
            }))
            .send()
            .await
            .context("Failed to exchange Google identity token with OpenAI")?
            .error_for_status()
            .context("OpenAI workload identity token exchange was rejected")?;
        let token: ExchangeResponse = response
            .json()
            .await
            .context("OpenAI returned an invalid workload identity token response")?;
        if token.access_token.is_empty() || !token.token_type.eq_ignore_ascii_case("bearer") {
            return Err(anyhow!(
                "OpenAI returned an invalid workload identity access token"
            ));
        }
        // OpenAI omits `scope` when the mapping has no explicit permission
        // restriction. Reject that response to prevent inheriting all mapped
        // service-account roles; model requests require api.model.request.
        let permissions: Vec<_> = token.scope.split_ascii_whitespace().collect();
        if !permissions.contains(&"api.model.request") {
            return Err(anyhow!(
                "OpenAI WIF mapping must explicitly grant api.model.request"
            ));
        }
        if token.expires_in == 0 || token.expires_in > 3600 {
            return Err(anyhow!(
                "OpenAI WIF access-token lifetime must be between 1 and 3600 seconds"
            ));
        }
        let refresh_at = unix_time_seconds()
            .checked_add(token.expires_in)
            .ok_or_else(|| anyhow!("OpenAI WIF access-token expiration overflowed"))?;
        let refresh_buffer = self.refresh_buffer_seconds.min(token.expires_in / 2);
        Ok(CachedToken {
            value: token.access_token,
            refresh_at: refresh_at.saturating_sub(refresh_buffer),
        })
    }
}

#[async_trait]
impl LlmCredentialProvider for OpenAiWif {
    async fn bearer_token(&self) -> Result<String> {
        let mut cached = self.cached.lock().await;
        let now = unix_time_seconds();
        if let Some(token) = cached.as_ref() {
            // Refresh 120 seconds early, capped at half the token lifetime so
            // short-lived (but valid) responses don't cause refresh loops.
            if token.refresh_at > now {
                return Ok(token.value.clone());
            }
        }
        // Hold the lock across refresh so concurrent requests share one exchange.
        let token = self.refresh().await?;
        let value = token.value.clone();
        *cached = Some(token);
        Ok(value)
    }
}

fn unix_time_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn configured_openai_wif() -> Result<Option<Arc<dyn LlmCredentialProvider>>> {
    let enabled = match std::env::var("TALON_OPENAI_WIF_ENABLED") {
        Ok(value) if value.eq_ignore_ascii_case("true") => true,
        Ok(value) if value.eq_ignore_ascii_case("false") => false,
        Ok(_) => {
            return Err(anyhow!(
                "TALON_OPENAI_WIF_ENABLED must be 'true' or 'false'"
            ))
        }
        Err(std::env::VarError::NotPresent) => false,
        Err(error) => return Err(error.into()),
    };
    if !enabled {
        return Ok(None);
    }
    let required = |name: &'static str| -> Result<String> {
        std::env::var(name)
            .map_err(|_| anyhow!("{name} is required when TALON_OPENAI_WIF_ENABLED=true"))
    };
    Ok(Some(Arc::new(OpenAiWif::new(
        required("OPENAI_IDENTITY_PROVIDER_ID")?,
        required("OPENAI_SERVICE_ACCOUNT_ID")?,
        required("OPENAI_WIF_AUDIENCE")?,
    )?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    async fn fake_endpoint(response: &'static str, expected_path: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 8192];
            let count = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.starts_with(&format!("GET {expected_path}?")));
            assert!(request.contains("audience=https%3A%2F%2Fapi.openai.com%2Fv1"));
            assert!(request.contains("format=full"));
            let content_length = response.len();
            stream.write_all(format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {content_length}\r\nConnection: close\r\n\r\n{response}"
            ).as_bytes()).await.unwrap();
        });
        format!("http://{address}{expected_path}")
    }

    #[tokio::test]
    async fn static_provider_returns_api_key_unchanged() {
        let provider = StaticApiKey::new("  key-bytes\n".to_string());
        assert_eq!(provider.bearer_token().await.unwrap(), "  key-bytes\n");
    }

    #[test]
    fn wif_disabled_path_returns_no_override() {
        let _lock = crate::test_support::env_lock();
        let _enabled = crate::test_support::EnvVarGuard::set("TALON_OPENAI_WIF_ENABLED", "false");
        assert!(configured_openai_wif().unwrap().is_none());
    }

    #[test]
    fn enabled_wif_requires_all_openai_mapping_settings() {
        let _lock = crate::test_support::env_lock();
        let _enabled = crate::test_support::EnvVarGuard::set("TALON_OPENAI_WIF_ENABLED", "true");
        let _provider_id = crate::test_support::EnvVarGuard::remove("OPENAI_IDENTITY_PROVIDER_ID");
        let _service_account_id =
            crate::test_support::EnvVarGuard::remove("OPENAI_SERVICE_ACCOUNT_ID");
        let _audience = crate::test_support::EnvVarGuard::remove("OPENAI_WIF_AUDIENCE");
        let error = match configured_openai_wif() {
            Ok(_) => panic!("expected required WIF configuration error"),
            Err(error) => error,
        };
        assert!(error
            .to_string()
            .contains("OPENAI_IDENTITY_PROVIDER_ID is required"));
    }

    #[tokio::test]
    async fn wif_requests_metadata_and_exchange_caches_token() {
        let metadata = fake_endpoint("oidc-subject-token", "/identity").await;
        let exchange_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let exchange_address = exchange_listener.local_addr().unwrap();
        let exchanges = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let exchanges_in_server = exchanges.clone();
        tokio::spawn(async move {
            let (mut stream, _) = exchange_listener.accept().await.unwrap();
            let mut request = vec![0; 8192];
            let count = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.starts_with("POST /oauth/token "));
            let body = request.split("\r\n\r\n").nth(1).unwrap();
            let body: Value = serde_json::from_str(body).unwrap();
            assert_eq!(body["subject_token"], "oidc-subject-token");
            assert_eq!(
                body["subject_token_type"],
                "urn:ietf:params:oauth:token-type:id_token"
            );
            exchanges_in_server.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let response = r#"{"access_token":"openai-short-token","token_type":"Bearer","expires_in":3600,"scope":"api.model.request"}"#;
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
        });
        let provider = OpenAiWif::with_endpoints(
            "idp".into(),
            "service-account".into(),
            "https://api.openai.com/v1".into(),
            metadata,
            format!("http://{exchange_address}/oauth/token"),
        )
        .unwrap();
        assert_eq!(provider.bearer_token().await.unwrap(), "openai-short-token");
        assert_eq!(provider.bearer_token().await.unwrap(), "openai-short-token");
        assert_eq!(exchanges.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn wif_exchange_failure_fails_closed() {
        let metadata = fake_endpoint("oidc-subject-token", "/identity").await;
        let exchange_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let exchange_address = exchange_listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = exchange_listener.accept().await.unwrap();
            let mut request = vec![0; 8192];
            let _ = stream.read(&mut request).await.unwrap();
            let body = r#"{"error":"mapping_not_found"}"#;
            stream.write_all(format!("HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        });
        let provider = OpenAiWif::with_endpoints(
            "idp".into(),
            "service-account".into(),
            "https://api.openai.com/v1".into(),
            metadata,
            format!("http://{exchange_address}/oauth/token"),
        )
        .unwrap();
        let error = provider.bearer_token().await.unwrap_err();
        assert!(error
            .to_string()
            .contains("OpenAI workload identity token exchange"));
    }

    #[tokio::test]
    async fn wif_rejects_exchange_without_explicit_model_permission() {
        let metadata = fake_endpoint("oidc-subject-token", "/identity").await;
        let exchange_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let exchange_address = exchange_listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = exchange_listener.accept().await.unwrap();
            let mut request = vec![0; 8192];
            let _ = stream.read(&mut request).await.unwrap();
            let body =
                r#"{"access_token":"unrestricted-token","token_type":"Bearer","expires_in":3600}"#;
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        });
        let provider = OpenAiWif::with_endpoints(
            "idp".into(),
            "service-account".into(),
            "https://api.openai.com/v1".into(),
            metadata,
            format!("http://{exchange_address}/oauth/token"),
        )
        .unwrap();
        let error = provider.bearer_token().await.unwrap_err();
        assert!(error
            .to_string()
            .contains("must explicitly grant api.model.request"));
    }

    #[tokio::test]
    async fn wif_refreshes_before_expiration() {
        let exchange_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let exchange_address = exchange_listener.local_addr().unwrap();
        let metadata_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let metadata_address = metadata_listener.local_addr().unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let exchange_calls = calls.clone();
        tokio::spawn(async move {
            for index in 0..2 {
                let (mut stream, _) = exchange_listener.accept().await.unwrap();
                let mut request = vec![0; 8192];
                let _ = stream.read(&mut request).await.unwrap();
                exchange_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let response = format!("{{\"access_token\":\"token-{index}\",\"token_type\":\"Bearer\",\"expires_in\":4,\"scope\":\"api.model.request\"}}");
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
            }
        });
        tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = metadata_listener.accept().await.unwrap();
                let mut request = vec![0; 4096];
                let _ = stream.read(&mut request).await.unwrap();
                let response = "oidc-subject-token";
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
            }
        });
        let mut provider = OpenAiWif::with_endpoints(
            "idp".into(),
            "service-account".into(),
            "audience".into(),
            format!("http://{metadata_address}/identity"),
            format!("http://{exchange_address}/oauth/token"),
        )
        .unwrap();
        assert_eq!(provider.bearer_token().await.unwrap(), "token-0");
        provider.refresh_buffer_seconds = 2;
        tokio::time::sleep(std::time::Duration::from_millis(2_100)).await;
        assert_eq!(provider.bearer_token().await.unwrap(), "token-1");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
