// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Bearer-token providers for LLM API clients.
//!
//! The shared client boundary deliberately knows only how to obtain a token;
//! identity sources acquire subject tokens; destination adapters exchange them.

use super::identity::{identity_source, WorkloadIdentitySource};
use crate::control::config::{llm_auth::validate_openai_auth, proto};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use prost::Message;
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex, OnceLock},
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

/// Exchanges an OIDC workload identity for OpenAI credentials, independently
/// of the cloud or mechanism that supplies the subject token.
pub struct OpenAiWif {
    identity_provider_id: String,
    service_account_id: String,
    source: Arc<dyn WorkloadIdentitySource>,
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
        mapping: &proto::OpenAiWorkloadIdentityConfig,
        source: Arc<dyn WorkloadIdentitySource>,
    ) -> Result<Self> {
        Self::with_source(
            mapping,
            source,
            "https://auth.openai.com/oauth/token".into(),
        )
    }

    fn with_source(
        mapping: &proto::OpenAiWorkloadIdentityConfig,
        source: Arc<dyn WorkloadIdentitySource>,
        exchange_endpoint: String,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("Failed to initialize OpenAI WIF HTTP client")?;
        Ok(Self {
            identity_provider_id: mapping.identity_provider_id.clone(),
            service_account_id: mapping.service_account_id.clone(),
            source,
            exchange_endpoint,
            refresh_buffer_seconds: 120,
            http,
            cached: Mutex::new(None),
        })
    }

    #[cfg(test)]
    fn with_endpoints(
        identity_provider_id: String,
        service_account_id: String,
        audience: String,
        metadata_endpoint: String,
        exchange_endpoint: String,
    ) -> Result<Self> {
        let source = super::identity::GoogleMetadata::new(audience, metadata_endpoint)?;
        Self::with_source(
            &proto::OpenAiWorkloadIdentityConfig {
                identity_provider_id,
                service_account_id,
                ..Default::default()
            },
            Arc::new(source),
            exchange_endpoint,
        )
    }

    async fn refresh(&self) -> Result<CachedToken> {
        let subject_token = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.source.subject_token(),
        )
        .await
        .context("Workload identity token acquisition timed out")??;
        if subject_token.trim().is_empty() {
            return Err(anyhow!(
                "Workload identity source returned an empty identity token"
            ));
        }

        let response = self
            .http
            .post(&self.exchange_endpoint)
            .json(&serde_json::json!({
                "grant_type": "urn:ietf:params:oauth:grant-type:token-exchange",
                "subject_token_type": "urn:ietf:params:oauth:token-type:jwt",
                "subject_token": subject_token,
                "identity_provider_id": self.identity_provider_id,
                "service_account_id": self.service_account_id,
            }))
            .send()
            .await
            .context("Failed to exchange workload identity token with OpenAI")?
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

/// Retain credentials across runtime builds, keyed by source and target mapping.
fn shared_openai_wif(config: &proto::OpenAiWorkloadIdentityConfig) -> Result<Arc<OpenAiWif>> {
    static PROVIDERS: OnceLock<StdMutex<HashMap<Vec<u8>, Arc<OpenAiWif>>>> = OnceLock::new();
    // These messages contain no maps: protobuf encoding is a stable full-config key.
    let key = config.encode_to_vec();
    let mut providers = PROVIDERS
        .get_or_init(StdMutex::default)
        .lock()
        .map_err(|_| anyhow!("OpenAI WIF provider cache lock poisoned"))?;
    if let Some(provider) = providers.get(&key) {
        return Ok(provider.clone());
    }
    let source = identity_source(
        config
            .source
            .as_ref()
            .context("workload_identity requires an identity source")?,
    )?;
    // Build under the lock; only successful builds are retained.
    let provider = Arc::new(OpenAiWif::new(config, source)?);
    providers.insert(key, provider.clone());
    Ok(provider)
}

/// Resolve OpenAiConfig.workload_identity; omission retains API-key behavior.
pub fn configured_workload_identity(
    openai: &proto::OpenAiConfig,
) -> Result<Option<Arc<dyn LlmCredentialProvider>>> {
    validate_openai_auth(openai)?;
    match &openai.workload_identity {
        Some(config) => Ok(Some(shared_openai_wif(config)?)),
        None => Ok(None),
    }
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
            assert!(request
                .to_ascii_lowercase()
                .contains("metadata-flavor: google"));
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

    fn configured_provider() -> proto::OpenAiConfig {
        proto::OpenAiConfig {
            workload_identity: Some(proto::OpenAiWorkloadIdentityConfig {
                source: Some(proto::WorkloadIdentitySource {
                    config: Some(proto::workload_identity_source::Config::GoogleMetadata(
                        proto::GoogleMetadataIdentitySource {
                            audience: "audience".into(),
                        },
                    )),
                }),
                identity_provider_id: "runtime-cache-test-idp".into(),
                service_account_id: "service-account".into(),
            }),
            ..Default::default()
        }
    }

    fn wif(config: &mut proto::OpenAiConfig) -> &mut proto::OpenAiWorkloadIdentityConfig {
        config.workload_identity.as_mut().unwrap()
    }

    #[test]
    fn configured_wif_shares_cached_token_across_runtime_lifetimes() {
        let mut config = configured_provider();
        let barrier = std::sync::Barrier::new(8);
        let providers = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        configured_workload_identity(&config).unwrap().unwrap()
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        for provider in &providers {
            assert!(Arc::ptr_eq(&providers[0], provider));
        }
        let provider = shared_openai_wif(wif(&mut config)).unwrap();
        *provider.cached.blocking_lock() = Some(CachedToken {
            value: "cached-across-runtimes".into(),
            refresh_at: unix_time_seconds() + 3600,
        });
        let weak = Arc::downgrade(&providers[0]);
        drop(provider);
        drop(providers);
        assert!(weak.upgrade().is_some());
        let next = configured_workload_identity(&config).unwrap().unwrap();
        assert!(Arc::ptr_eq(&weak.upgrade().unwrap(), &next));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert_eq!(
            runtime.block_on(next.bearer_token()).unwrap(),
            "cached-across-runtimes"
        );
    }

    #[test]
    fn configured_wif_cache_is_scoped_to_source_and_destination() {
        let original_config = configured_provider();
        let original = configured_workload_identity(&original_config)
            .unwrap()
            .unwrap();
        let mut resolved = vec![original.clone()];
        for field in 0..8 {
            let mut changed = original_config.clone();
            let auth = wif(&mut changed);
            match field {
                0 => auth.identity_provider_id = "other-idp".into(),
                1 => auth.service_account_id = "other-account".into(),
                2 => {
                    auth.source.as_mut().unwrap().config =
                        Some(proto::workload_identity_source::Config::GoogleMetadata(
                            proto::GoogleMetadataIdentitySource {
                                audience: "other-audience".into(),
                            },
                        ))
                }
                3 => {
                    auth.source.as_mut().unwrap().config =
                        Some(proto::workload_identity_source::Config::TokenFile(
                            proto::TokenFileIdentitySource {
                                path: "/var/run/token-a".into(),
                            },
                        ))
                }
                4 => {
                    auth.source.as_mut().unwrap().config =
                        Some(proto::workload_identity_source::Config::TokenFile(
                            proto::TokenFileIdentitySource {
                                path: "/var/run/token-b".into(),
                            },
                        ))
                }
                _ => {
                    auth.source.as_mut().unwrap().config =
                        Some(proto::workload_identity_source::Config::AwsSts(
                            proto::AwsStsIdentitySource {
                                audience: if field == 7 {
                                    "other-audience"
                                } else {
                                    "audience"
                                }
                                .into(),
                                region: if field == 6 { "us-east-1" } else { "us-west-2" }.into(),
                            },
                        ))
                }
            }
            let new = configured_workload_identity(&changed).unwrap().unwrap();
            for previous in &resolved {
                assert!(!Arc::ptr_eq(previous, &new));
            }
            assert!(Arc::ptr_eq(
                &new,
                &configured_workload_identity(&changed).unwrap().unwrap()
            ));
            resolved.push(new);
        }
        let mut invalid = original_config.clone();
        wif(&mut invalid).identity_provider_id.clear();
        for _ in 0..2 {
            assert!(configured_workload_identity(&invalid).is_err());
        }
        assert!(Arc::ptr_eq(
            &original,
            &configured_workload_identity(&original_config)
                .unwrap()
                .unwrap()
        ));
        let mut no_auth = original_config;
        no_auth.workload_identity = None;
        assert!(configured_workload_identity(&no_auth).unwrap().is_none());
    }

    #[tokio::test]
    async fn token_file_rotation_is_used_at_refresh_and_failures_do_not_reuse_stale_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projected-token");
        tokio::fs::write(&path, "subject-one\n").await.unwrap();
        let source = identity_source(&proto::WorkloadIdentitySource {
            config: Some(proto::workload_identity_source::Config::TokenFile(
                proto::TokenFileIdentitySource {
                    path: path.to_str().unwrap().into(),
                },
            )),
        })
        .unwrap();
        let subjects = Arc::new(Mutex::new(Vec::new()));
        let received = subjects.clone();
        let app = axum::Router::new().route(
            "/token",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let received = received.clone();
                async move {
                    assert_eq!(body["identity_provider_id"], "file-idp");
                    assert_eq!(body["service_account_id"], "file-account");
                    assert_eq!(
                        body["subject_token_type"],
                        "urn:ietf:params:oauth:token-type:jwt"
                    );
                    received
                        .lock()
                        .await
                        .push(body["subject_token"].as_str().unwrap().to_owned());
                    axum::Json(serde_json::json!({
                        "access_token": "exchanged-token", "token_type": "Bearer",
                        "expires_in": 3600, "scope": "api.model.request",
                    }))
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let provider = OpenAiWif::with_source(
            &proto::OpenAiWorkloadIdentityConfig {
                identity_provider_id: "file-idp".into(),
                service_account_id: "file-account".into(),
                ..Default::default()
            },
            source,
            format!("http://{address}/token"),
        )
        .unwrap();
        assert_eq!(provider.bearer_token().await.unwrap(), "exchanged-token");
        assert_eq!(provider.bearer_token().await.unwrap(), "exchanged-token");
        tokio::fs::write(&path, "subject-two\n").await.unwrap();
        provider.cached.lock().await.as_mut().unwrap().refresh_at = 0;
        assert_eq!(provider.bearer_token().await.unwrap(), "exchanged-token");
        tokio::fs::write(&path, "\n").await.unwrap();
        provider.cached.lock().await.as_mut().unwrap().refresh_at = 0;
        assert!(provider
            .bearer_token()
            .await
            .unwrap_err()
            .to_string()
            .contains("empty identity token"));
        assert_eq!(*subjects.lock().await, ["subject-one", "subject-two"]);
        server.abort();
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
                "urn:ietf:params:oauth:token-type:jwt"
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
