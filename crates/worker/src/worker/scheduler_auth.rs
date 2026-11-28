// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[allow(unused_imports)]
use crate::talon;

use anyhow::{anyhow, Result};
use axum::http::{header, HeaderMap};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

const GOOGLE_OIDC_CERTS_TTL: Duration = Duration::from_secs(60 * 60);
const GOOGLE_OIDC_MISS_REFRESH_COOLDOWN: Duration = Duration::from_secs(60);
const GOOGLE_OIDC_HTTP_TIMEOUT: Duration = Duration::from_secs(10);
pub const PUBSUB_PUSH_AUTH_HEADER: &str = "x-talon-pubsub-token";

#[derive(Clone)]
pub enum PubSubPushAuthenticator {
    SharedSecret {
        token: String,
    },
    GoogleOidc {
        audience: String,
        service_account_email: String,
    },
    DenyAll,
}

impl PubSubPushAuthenticator {
    pub fn from_env(pull_mode: bool) -> Self {
        Self::from_env_for_mode(pull_mode, |name| std::env::var(name).ok())
    }

    pub fn from_env_for_mode<F>(pull_mode: bool, get: F) -> Self
    where
        F: Fn(&str) -> Option<String>,
    {
        if pull_mode {
            return Self::deny_all();
        }

        if let Some(token) = get("TALON_PUBSUB_PUSH_AUTH_TOKEN").and_then(non_empty) {
            return Self::shared_secret(token);
        }

        if let Some(audience) = get("TALON_PUBSUB_PUSH_AUDIENCE").and_then(non_empty) {
            if let Some(service_account_email) =
                get("TALON_PUBSUB_PUSH_SERVICE_ACCOUNT_EMAIL").and_then(non_empty)
            {
                return Self::google_oidc(audience, service_account_email);
            }
            tracing::warn!(
                "TALON_PUBSUB_PUSH_AUDIENCE is set without TALON_PUBSUB_PUSH_SERVICE_ACCOUNT_EMAIL; denying Pub/Sub push requests"
            );
            return Self::deny_all();
        }

        Self::deny_all()
    }

    pub fn deny_all() -> Self {
        Self::DenyAll
    }

    pub fn shared_secret(token: String) -> Self {
        Self::SharedSecret { token }
    }

    pub fn google_oidc(audience: String, service_account_email: String) -> Self {
        match (non_empty(audience), non_empty(service_account_email)) {
            (Some(audience), Some(service_account_email)) => Self::GoogleOidc {
                audience,
                service_account_email,
            },
            _ => Self::DenyAll,
        }
    }

    pub async fn authorize(&self, headers: &HeaderMap) -> Result<()> {
        match self {
            Self::SharedSecret { token } => authorize_shared_secret_with_header(
                headers,
                token,
                PUBSUB_PUSH_AUTH_HEADER,
                "invalid pubsub push shared secret",
            ),
            Self::GoogleOidc {
                audience,
                service_account_email,
            } => {
                authorize_google_oidc(headers, audience, Some(service_account_email.as_str())).await
            }
            Self::DenyAll => Err(anyhow!(
                "pubsub push authentication is not configured or disabled in pull mode"
            )),
        }
    }
}

#[derive(Clone)]
pub enum SchedulerRequestAuthenticator {
    SharedSecret {
        token: String,
    },
    GoogleOidc {
        audience: String,
        service_account_email: Option<String>,
    },
    DenyAll,
}

impl SchedulerRequestAuthenticator {
    pub async fn from_config(config: &talon::control::config::Config) -> Result<Self> {
        if let Some(cfg) = config
            .control_plane
            .as_ref()
            .and_then(|cp| cp.scheduler.as_ref())
            .and_then(|scheduler| match scheduler.backend.as_ref() {
                Some(talon::control::config::proto::scheduler_config::Backend::CloudTasks(
                    cloud_tasks,
                )) => cloud_tasks.callback_auth.as_ref(),
                Some(
                    talon::control::config::proto::scheduler_config::Backend::AwsEventbridgeScheduler(
                        _,
                    ),
                ) => None,
                None => None,
            })
        {
            return match cfg.auth.as_ref() {
                Some(talon::control::config::proto::scheduler_callback_auth_config::Auth::SharedSecret(
                    secret,
                )) => Ok(Self::shared_secret(
                    talon::control::config::SecretExt::resolve(secret).await?,
                )),
                Some(talon::control::config::proto::scheduler_callback_auth_config::Auth::GoogleOidc(
                    oidc,
                )) => Ok(Self::google_oidc(
                    oidc.audience.clone(),
                    non_empty(oidc.service_account_email.clone()),
                )),
                None => Ok(Self::deny_all()),
            };
        }

        if let Some(token) = std::env::var("TALON_SCHEDULER_AUTH_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(Self::shared_secret(token));
        }

        if let Some(audience) = std::env::var("TALON_SCHEDULER_AUDIENCE")
            .ok()
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(Self::google_oidc(
                audience,
                std::env::var("TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL")
                    .ok()
                    .filter(|value| !value.trim().is_empty()),
            ));
        }

        Ok(Self::deny_all())
    }

    pub fn deny_all() -> Self {
        Self::DenyAll
    }

    pub fn shared_secret(token: String) -> Self {
        Self::SharedSecret { token }
    }

    pub fn google_oidc(audience: String, service_account_email: Option<String>) -> Self {
        Self::GoogleOidc {
            audience,
            service_account_email,
        }
    }

    pub async fn authorize(&self, headers: &HeaderMap) -> Result<()> {
        match self {
            Self::SharedSecret { token } => authorize_shared_secret(headers, token),
            Self::GoogleOidc {
                audience,
                service_account_email,
            } => authorize_google_oidc(headers, audience, service_account_email.as_deref()).await,
            Self::DenyAll => Err(anyhow!(
                "scheduler request authentication is not configured"
            )),
        }
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&lhs, &rhs) in a.iter().zip(b.iter()) {
        diff |= lhs ^ rhs;
    }
    diff == 0
}

fn authorize_shared_secret(headers: &HeaderMap, expected_token: &str) -> Result<()> {
    authorize_shared_secret_with_header(
        headers,
        expected_token,
        talon::control::scheduler::SCHEDULER_AUTH_HEADER,
        "invalid scheduler shared secret",
    )
}

fn authorize_shared_secret_with_header(
    headers: &HeaderMap,
    expected_token: &str,
    custom_header: &str,
    error_message: &str,
) -> Result<()> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let custom_value = headers.get(custom_header).and_then(|v| v.to_str().ok());

    if auth_header.is_some_and(|candidate| constant_time_eq(candidate, expected_token))
        || custom_value.is_some_and(|candidate| constant_time_eq(candidate, expected_token))
    {
        return Ok(());
    }

    Err(anyhow!("{error_message}"))
}

async fn authorize_google_oidc(
    headers: &HeaderMap,
    audience: &str,
    expected_email: Option<&str>,
) -> Result<()> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let token = auth_header.ok_or_else(|| anyhow!("missing scheduler bearer token"))?;
    verify_google_oidc_token(token, audience, expected_email).await
}

#[derive(Deserialize)]
struct GoogleOidcClaims {
    aud: String,
    email: Option<String>,
    email_verified: Option<bool>,
}

struct CachedGoogleOidcCerts {
    fetched_at: Instant,
    keys: HashMap<String, Arc<DecodingKey>>,
}

fn google_oidc_certs_cache() -> &'static tokio::sync::RwLock<Option<CachedGoogleOidcCerts>> {
    static CACHE: OnceLock<tokio::sync::RwLock<Option<CachedGoogleOidcCerts>>> = OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::RwLock::new(None))
}

fn google_oidc_http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(GOOGLE_OIDC_HTTP_TIMEOUT)
            .build()
            .expect("failed to build Google OIDC HTTP client")
    })
}

fn google_oidc_refresh_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn load_google_oidc_decoding_key(kid: &str) -> Result<Arc<DecodingKey>> {
    load_google_oidc_decoding_key_with(kid, fetch_google_oidc_certs).await
}

async fn load_google_oidc_decoding_key_with<F, Fut>(kid: &str, fetch: F) -> Result<Arc<DecodingKey>>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<CachedGoogleOidcCerts>>,
{
    match lookup_cached_google_oidc_key(kid).await {
        CachedCertLookup::Found(key) => return Ok(key),
        CachedCertLookup::RecentMiss => {
            return Err(anyhow!("unknown Google cert kid '{}'", kid));
        }
        CachedCertLookup::NeedsRefresh => {}
    }

    let _refresh_guard = google_oidc_refresh_lock().lock().await;
    match lookup_cached_google_oidc_key(kid).await {
        CachedCertLookup::Found(key) => return Ok(key),
        CachedCertLookup::RecentMiss => {
            return Err(anyhow!("unknown Google cert kid '{}'", kid));
        }
        CachedCertLookup::NeedsRefresh => {}
    }

    let fetched = fetch().await?;
    let mut cache = google_oidc_certs_cache().write().await;
    let key = fetched.keys.get(kid).cloned();
    // Always persist successful fetches even if the requested kid is absent so
    // repeated unknown-kid misses are bounded by GOOGLE_OIDC_MISS_REFRESH_COOLDOWN.
    *cache = Some(fetched);
    key.ok_or_else(|| anyhow!("unknown Google cert kid '{}'", kid))
}

enum CachedCertLookup {
    Found(Arc<DecodingKey>),
    RecentMiss,
    NeedsRefresh,
}

async fn lookup_cached_google_oidc_key(kid: &str) -> CachedCertLookup {
    let cache = google_oidc_certs_cache().read().await;
    let Some(cached) = cache.as_ref() else {
        return CachedCertLookup::NeedsRefresh;
    };
    let elapsed = cached.fetched_at.elapsed();
    if elapsed < GOOGLE_OIDC_CERTS_TTL {
        if let Some(key) = cached.keys.get(kid) {
            return CachedCertLookup::Found(Arc::clone(key));
        }
        if elapsed < GOOGLE_OIDC_MISS_REFRESH_COOLDOWN {
            return CachedCertLookup::RecentMiss;
        }
    }
    CachedCertLookup::NeedsRefresh
}

async fn fetch_google_oidc_certs() -> Result<CachedGoogleOidcCerts> {
    let certs = google_oidc_http_client()
        .get("https://www.googleapis.com/oauth2/v1/certs")
        .send()
        .await?
        .error_for_status()?
        .json::<HashMap<String, String>>()
        .await?;
    let keys = certs
        .into_iter()
        .map(|(kid, cert)| {
            DecodingKey::from_rsa_pem(cert.as_bytes())
                .map(|key| (kid, Arc::new(key)))
                .map_err(anyhow::Error::from)
        })
        .collect::<Result<HashMap<_, _>>>()?;

    Ok(CachedGoogleOidcCerts {
        fetched_at: Instant::now(),
        keys,
    })
}

async fn verify_google_oidc_token(
    token: &str,
    audience: &str,
    expected_email: Option<&str>,
) -> Result<()> {
    talon::control::security::install_jwt_crypto_provider();
    let header = decode_header(token)?;
    let kid = header
        .kid
        .ok_or_else(|| anyhow!("missing kid in scheduler OIDC token"))?;
    let decoding_key = load_google_oidc_decoding_key(&kid).await?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[audience]);
    validation.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);

    let decoded = decode::<GoogleOidcClaims>(token, &decoding_key, &validation)?;
    let claims = decoded.claims;
    if claims.aud != audience {
        return Err(anyhow!("unexpected scheduler OIDC audience"));
    }
    if let Some(expected_email) = expected_email {
        if claims.email.as_deref() != Some(expected_email) || claims.email_verified != Some(true) {
            return Err(anyhow!("unexpected scheduler OIDC service account"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use crate::talon;

    use super::{
        authorize_shared_secret, google_oidc_certs_cache, load_google_oidc_decoding_key,
        load_google_oidc_decoding_key_with, non_empty, CachedGoogleOidcCerts,
        PubSubPushAuthenticator, SchedulerRequestAuthenticator, GOOGLE_OIDC_MISS_REFRESH_COOLDOWN,
        PUBSUB_PUSH_AUTH_HEADER,
    };
    use axum::http::{header, HeaderMap, HeaderValue};
    use jsonwebtoken::{encode, Algorithm, DecodingKey, EncodingKey, Header};
    use serde::Serialize;
    use std::sync::{Mutex, OnceLock};
    use std::{
        collections::HashMap,
        sync::Arc,
        time::{Duration, Instant},
    };

    const TEST_RSA_PRIVATE_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----
MIICXgIBAAKBgQC72wHrmFBtmTNV6Th0MAifD5jPdnfWqdl8Z4+01QscV/6rnKhq
rFaq1UIwQET8g+Jn8EW8eus/GHLuFfirmUj4SK6GM5QhqsI/Sa97SmvUlkxzgrmO
jH79Q7HXtsahx547ZjVw711J4oA9PeHS7VLcNU+lTa+L+6WG4XpG1Qg7OQIDAQAB
AoGBAKeYg4USFadCB+e8s44NEJQAET/+HHpafFsd9brKWyhFZULr9+F7sbKlonBz
1lhGvWYnmb/HFjvrbxX0ai+GCO9XGwPBb/4ju/BHgIh6lWfC32HQs1RNSSMR4LT8
YZqzLTtRYCEco1kWACArGUEGdnnwsvzeHbnu9FhKTlnTZd8BAkEA+e2oDy04cBI0
IjospStnlHjYDYArtLumf25eT/C7PYJySfZcIKgZP/zHULK9YvWwBi7YC6S9V2ij
fYKrM8WmGQJBAMBrT204Vwk6oxPxz6d7E8LfnTn2SGzhWeomlzEeb9we1K4EvdKx
yOjFyKM9ZqNjfSTkS1roqy6wpbsukbtCoiECQFthU6NI62u+nCUvlAdIGXUlwgkd
pd1NBxFsrzsXT76rpVH5q7GdBK5qpA2TbL90CUoZcpC/SSNedPh9AE/LonECQQCD
ZANdcj5EaAzZXqJMG8fXprgGzzyPVKYANI/DE6SQa2EQ3t371Dh7ciraBOBkK1hV
66nlDsFtZWQV1+vdMdfhAkEAvQhOjtddQ5epwuJvUD/JrL8d4ulSIpYFsdEIKFqs
0wHsMqqJooCIC8H5LSwst/sL+1x3aJ/60xB77d2sZ+4YOw==
-----END RSA PRIVATE KEY-----";

    const TEST_RSA_PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----
MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQC72wHrmFBtmTNV6Th0MAifD5jP
dnfWqdl8Z4+01QscV/6rnKhqrFaq1UIwQET8g+Jn8EW8eus/GHLuFfirmUj4SK6G
M5QhqsI/Sa97SmvUlkxzgrmOjH79Q7HXtsahx547ZjVw711J4oA9PeHS7VLcNU+l
Ta+L+6WG4XpG1Qg7OQIDAQAB
-----END PUBLIC KEY-----";

    #[derive(Serialize)]
    struct TestGoogleOidcClaims<'a> {
        aud: &'a str,
        iss: &'a str,
        exp: usize,
        email: Option<&'a str>,
        email_verified: Option<bool>,
    }

    fn oidc_cache_mutex() -> &'static Mutex<()> {
        static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        MUTEX.get_or_init(|| Mutex::new(()))
    }

    async fn seed_google_oidc_cache(kid: &str) {
        talon::control::security::install_jwt_crypto_provider();
        let mut keys = HashMap::new();
        keys.insert(
            kid.to_string(),
            Arc::new(DecodingKey::from_rsa_pem(TEST_RSA_PUBLIC_KEY.as_bytes()).unwrap()),
        );
        *google_oidc_certs_cache().write().await = Some(CachedGoogleOidcCerts {
            fetched_at: Instant::now(),
            keys,
        });
    }

    async fn clear_google_oidc_cache() {
        *google_oidc_certs_cache().write().await = None;
    }

    fn signed_google_oidc_token(
        kid: &str,
        audience: &str,
        email: Option<&str>,
        email_verified: Option<bool>,
    ) -> String {
        talon::control::security::install_jwt_crypto_provider();
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.to_string());
        encode(
            &header,
            &TestGoogleOidcClaims {
                aud: audience,
                iss: "https://accounts.google.com",
                exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
                email,
                email_verified,
            },
            &EncodingKey::from_rsa_pem(TEST_RSA_PRIVATE_KEY.as_bytes()).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn non_empty_trims_and_discards_blank_values() {
        assert_eq!(non_empty(" value ".to_string()), Some("value".to_string()));
        assert_eq!(non_empty("   ".to_string()), None);
    }

    #[tokio::test]
    async fn shared_secret_authorize_accepts_bearer_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer test-token"),
        );

        SchedulerRequestAuthenticator::shared_secret("test-token".to_string())
            .authorize(&headers)
            .await
            .expect("expected shared secret auth to succeed");
    }

    #[tokio::test]
    async fn shared_secret_authorize_rejects_wrong_values() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong-token"),
        );

        let err = SchedulerRequestAuthenticator::shared_secret("test-token".to_string())
            .authorize(&headers)
            .await
            .expect_err("expected shared secret auth to fail");
        assert!(err.to_string().contains("invalid scheduler shared secret"));
    }

    #[test]
    fn authorize_shared_secret_accepts_scheduler_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            talon::control::scheduler::SCHEDULER_AUTH_HEADER,
            HeaderValue::from_static("test-token"),
        );

        authorize_shared_secret(&headers, "test-token")
            .expect("expected scheduler auth header to succeed");
    }

    #[tokio::test]
    async fn deny_all_authenticator_rejects_requests() {
        let err = SchedulerRequestAuthenticator::deny_all()
            .authorize(&HeaderMap::new())
            .await
            .expect_err("expected deny all auth to reject");
        assert!(err
            .to_string()
            .contains("scheduler request authentication is not configured"));
    }

    #[tokio::test]
    async fn from_config_prefers_shared_secret_env() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
        unsafe {
            std::env::set_var("TALON_SCHEDULER_AUTH_TOKEN", "env-secret");
            std::env::remove_var("TALON_SCHEDULER_AUDIENCE");
            std::env::remove_var("TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL");
        }

        let authenticator =
            SchedulerRequestAuthenticator::from_config(&talon::control::config::Config::default())
                .await
                .expect("authenticator should resolve");

        match authenticator {
            SchedulerRequestAuthenticator::SharedSecret { token } => {
                assert_eq!(token, "env-secret");
            }
            _ => panic!("expected shared secret authenticator"),
        }

        unsafe {
            std::env::remove_var("TALON_SCHEDULER_AUTH_TOKEN");
        }
    }

    #[tokio::test]
    async fn from_config_uses_google_oidc_env_when_no_shared_secret() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
        unsafe {
            std::env::remove_var("TALON_SCHEDULER_AUTH_TOKEN");
            std::env::set_var(
                "TALON_SCHEDULER_AUDIENCE",
                "https://worker.example.com/schedules",
            );
            std::env::set_var(
                "TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL",
                "scheduler@example.iam.gserviceaccount.com",
            );
        }

        let authenticator =
            SchedulerRequestAuthenticator::from_config(&talon::control::config::Config::default())
                .await
                .expect("authenticator should resolve");

        match authenticator {
            SchedulerRequestAuthenticator::GoogleOidc {
                audience,
                service_account_email,
            } => {
                assert_eq!(audience, "https://worker.example.com/schedules");
                assert_eq!(
                    service_account_email.as_deref(),
                    Some("scheduler@example.iam.gserviceaccount.com")
                );
            }
            _ => panic!("expected google oidc authenticator"),
        }

        unsafe {
            std::env::remove_var("TALON_SCHEDULER_AUDIENCE");
            std::env::remove_var("TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL");
        }
    }

    #[tokio::test]
    async fn google_oidc_authorize_requires_bearer_header() {
        let err = SchedulerRequestAuthenticator::google_oidc(
            "https://worker.example.com/schedules".to_string(),
            None,
        )
        .authorize(&HeaderMap::new())
        .await
        .expect_err("expected oidc auth to require bearer header");

        assert!(err.to_string().contains("missing scheduler bearer token"));
    }

    #[tokio::test]
    async fn google_oidc_authorize_accepts_valid_signed_token_from_cached_key() {
        let _guard = oidc_cache_mutex().lock().unwrap();
        clear_google_oidc_cache().await;
        let kid = "kid-valid";
        seed_google_oidc_cache(kid).await;

        let mut headers = HeaderMap::new();
        let token = signed_google_oidc_token(
            kid,
            "https://worker.example.com/schedules",
            Some("scheduler@example.iam.gserviceaccount.com"),
            Some(true),
        );
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        );

        SchedulerRequestAuthenticator::google_oidc(
            "https://worker.example.com/schedules".to_string(),
            Some("scheduler@example.iam.gserviceaccount.com".to_string()),
        )
        .authorize(&headers)
        .await
        .expect("expected cached OIDC key to validate token");

        clear_google_oidc_cache().await;
    }

    #[tokio::test]
    async fn google_oidc_authorize_rejects_wrong_service_account_and_unknown_kid() {
        let _guard = oidc_cache_mutex().lock().unwrap();
        clear_google_oidc_cache().await;
        let kid = "kid-email";
        seed_google_oidc_cache(kid).await;

        let mut headers = HeaderMap::new();
        let token = signed_google_oidc_token(
            kid,
            "https://worker.example.com/schedules",
            Some("wrong@example.iam.gserviceaccount.com"),
            Some(true),
        );
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        );

        let err = SchedulerRequestAuthenticator::google_oidc(
            "https://worker.example.com/schedules".to_string(),
            Some("scheduler@example.iam.gserviceaccount.com".to_string()),
        )
        .authorize(&headers)
        .await
        .expect_err("expected wrong service account to fail");
        assert!(err
            .to_string()
            .contains("unexpected scheduler OIDC service account"));

        let unknown_kid = signed_google_oidc_token(
            "missing-kid",
            "https://worker.example.com/schedules",
            None,
            None,
        );
        let mut unknown_headers = HeaderMap::new();
        unknown_headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", unknown_kid)).unwrap(),
        );
        let unknown_err = SchedulerRequestAuthenticator::google_oidc(
            "https://worker.example.com/schedules".to_string(),
            None,
        )
        .authorize(&unknown_headers)
        .await
        .expect_err("expected unknown kid to fail");
        assert!(unknown_err
            .to_string()
            .contains("unknown Google cert kid 'missing-kid'"));

        clear_google_oidc_cache().await;
    }

    #[tokio::test]
    async fn load_google_oidc_decoding_key_uses_fresh_cache_entries() {
        let _guard = oidc_cache_mutex().lock().unwrap();
        clear_google_oidc_cache().await;
        let kid = "kid-cache";
        seed_google_oidc_cache(kid).await;

        load_google_oidc_decoding_key(kid)
            .await
            .expect("expected cached key lookup to succeed");

        clear_google_oidc_cache().await;
    }

    #[tokio::test]
    async fn pubsub_push_authenticator_denies_by_default_and_in_pull_mode() {
        let pull_auth = PubSubPushAuthenticator::from_env_for_mode(true, |name| match name {
            "TALON_PUBSUB_PUSH_AUTH_TOKEN" => Some("push-secret".to_string()),
            _ => None,
        });
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer push-secret"),
        );
        assert!(pull_auth.authorize(&headers).await.is_err());

        let unconfigured = PubSubPushAuthenticator::from_env_for_mode(false, |_| None);
        assert!(unconfigured.authorize(&headers).await.is_err());
    }

    #[tokio::test]
    async fn pubsub_push_authenticator_accepts_shared_secret_and_google_oidc() {
        let shared = PubSubPushAuthenticator::from_env_for_mode(false, |name| match name {
            "TALON_PUBSUB_PUSH_AUTH_TOKEN" => Some("push-secret".to_string()),
            _ => None,
        });
        let mut bearer_headers = HeaderMap::new();
        bearer_headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer push-secret"),
        );
        shared.authorize(&bearer_headers).await.unwrap();

        let mut custom_headers = HeaderMap::new();
        custom_headers.insert(
            PUBSUB_PUSH_AUTH_HEADER,
            HeaderValue::from_static("push-secret"),
        );
        shared.authorize(&custom_headers).await.unwrap();

        let mut wrong_headers = HeaderMap::new();
        wrong_headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong"),
        );
        assert!(shared.authorize(&wrong_headers).await.is_err());

        let _guard = oidc_cache_mutex().lock().unwrap();
        clear_google_oidc_cache().await;
        let kid = "kid-push";
        seed_google_oidc_cache(kid).await;

        let oidc = PubSubPushAuthenticator::from_env_for_mode(false, |name| match name {
            "TALON_PUBSUB_PUSH_AUDIENCE" => Some("https://worker.example.com/pubsub/push".into()),
            "TALON_PUBSUB_PUSH_SERVICE_ACCOUNT_EMAIL" => {
                Some("pubsub@example.iam.gserviceaccount.com".into())
            }
            _ => None,
        });
        let token = signed_google_oidc_token(
            kid,
            "https://worker.example.com/pubsub/push",
            Some("pubsub@example.iam.gserviceaccount.com"),
            Some(true),
        );
        let mut oidc_headers = HeaderMap::new();
        oidc_headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        oidc.authorize(&oidc_headers).await.unwrap();
        clear_google_oidc_cache().await;
    }

    #[tokio::test]
    async fn pubsub_push_oidc_requires_service_account_email() {
        let _guard = oidc_cache_mutex().lock().unwrap();
        clear_google_oidc_cache().await;
        let kid = "kid-push-missing-email";
        seed_google_oidc_cache(kid).await;

        let token = signed_google_oidc_token(
            kid,
            "https://worker.example.com/pubsub/push",
            Some("someone@example.iam.gserviceaccount.com"),
            Some(true),
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );

        let missing_email = PubSubPushAuthenticator::from_env_for_mode(false, |name| match name {
            "TALON_PUBSUB_PUSH_AUDIENCE" => Some("https://worker.example.com/pubsub/push".into()),
            _ => None,
        });
        assert!(missing_email.authorize(&headers).await.is_err());

        let blank_email = PubSubPushAuthenticator::from_env_for_mode(false, |name| match name {
            "TALON_PUBSUB_PUSH_AUDIENCE" => Some("https://worker.example.com/pubsub/push".into()),
            "TALON_PUBSUB_PUSH_SERVICE_ACCOUNT_EMAIL" => Some("   ".into()),
            _ => None,
        });
        assert!(blank_email.authorize(&headers).await.is_err());

        clear_google_oidc_cache().await;
    }

    #[tokio::test]
    async fn google_oidc_cert_cache_stores_failed_lookups_and_rate_limits_miss_refreshes() {
        let _guard = oidc_cache_mutex().lock().unwrap();
        clear_google_oidc_cache().await;
        talon::control::security::install_jwt_crypto_provider();

        let fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let make_certs = |kid: &str| {
            let mut keys = HashMap::new();
            keys.insert(
                kid.to_string(),
                Arc::new(DecodingKey::from_rsa_pem(TEST_RSA_PUBLIC_KEY.as_bytes()).unwrap()),
            );
            CachedGoogleOidcCerts {
                fetched_at: Instant::now(),
                keys,
            }
        };

        // First miss fetches once, caches the returned keyset, and returns unknown kid.
        let err = load_google_oidc_decoding_key_with("unknown-1", || {
            let fetches = Arc::clone(&fetches);
            async move {
                fetches.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(make_certs("kid-initial"))
            }
        })
        .await
        .expect_err("unknown kid should fail");
        assert!(err.to_string().contains("unknown Google cert kid"));
        assert_eq!(fetches.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Existing key from the fetched set is immediately served from cache.
        load_google_oidc_decoding_key_with("kid-initial", || {
            let fetches = Arc::clone(&fetches);
            async move {
                fetches.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(make_certs("kid-initial"))
            }
        })
        .await
        .expect("cached key from prior miss-triggered fetch should hit");
        assert_eq!(fetches.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Subsequent unknown-kid misses within cooldown must NOT trigger another fetch.
        let err = load_google_oidc_decoding_key_with("unknown-2", || {
            let fetches = Arc::clone(&fetches);
            async move {
                fetches.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(make_certs("kid-rotated"))
            }
        })
        .await
        .expect_err("unknown kid within cooldown should fail without fetching");
        assert!(err.to_string().contains("unknown Google cert kid"));
        assert_eq!(fetches.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Once the cache is older than the miss-refresh cooldown, a miss refreshes
        // and discovers the rotated key.
        {
            let mut cache = google_oidc_certs_cache().write().await;
            cache.as_mut().unwrap().fetched_at =
                Instant::now() - GOOGLE_OIDC_MISS_REFRESH_COOLDOWN - Duration::from_secs(1);
        }
        load_google_oidc_decoding_key_with("kid-rotated", || {
            let fetches = Arc::clone(&fetches);
            async move {
                fetches.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(make_certs("kid-rotated"))
            }
        })
        .await
        .expect("rotated key should be discovered after cooldown");
        assert_eq!(fetches.load(std::sync::atomic::Ordering::SeqCst), 2);

        clear_google_oidc_cache().await;
    }
}
