// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

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
const GOOGLE_OIDC_HTTP_TIMEOUT: Duration = Duration::from_secs(10);

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
    pub async fn from_config(config: &crate::config::Config) -> Result<Self> {
        if let Some(cfg) = config
            .control_plane
            .as_ref()
            .and_then(|cp| cp.scheduler.as_ref())
            .and_then(|scheduler| match scheduler.backend.as_ref() {
                Some(crate::config::proto::scheduler_config::Backend::CloudTasks(cloud_tasks)) => {
                    cloud_tasks.callback_auth.as_ref()
                }
                None => None,
            })
        {
            return match cfg.auth.as_ref() {
                Some(crate::config::proto::scheduler_callback_auth_config::Auth::SharedSecret(
                    secret,
                )) => Ok(Self::shared_secret(
                    crate::config::SecretExt::resolve(secret).await?,
                )),
                Some(crate::config::proto::scheduler_callback_auth_config::Auth::GoogleOidc(
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

fn authorize_shared_secret(headers: &HeaderMap, expected_token: &str) -> Result<()> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let scheduler_header = headers
        .get(crate::control::scheduler::SCHEDULER_AUTH_HEADER)
        .and_then(|v| v.to_str().ok());

    if auth_header == Some(expected_token) || scheduler_header == Some(expected_token) {
        return Ok(());
    }

    Err(anyhow!("invalid scheduler shared secret"))
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

async fn load_google_oidc_decoding_key(kid: &str) -> Result<Arc<DecodingKey>> {
    if let Some(key) = cached_google_oidc_decoding_key(kid).await {
        return Ok(key);
    }

    let fetched = fetch_google_oidc_certs().await?;
    let mut cache = google_oidc_certs_cache().write().await;
    if let Some(cached) = cache.as_ref() {
        if cached.fetched_at.elapsed() < GOOGLE_OIDC_CERTS_TTL {
            if let Some(key) = cached.keys.get(kid) {
                return Ok(Arc::clone(key));
            }
        }
    }
    let key = fetched
        .keys
        .get(kid)
        .cloned()
        .ok_or_else(|| anyhow!("unknown Google cert kid '{}'", kid))?;
    *cache = Some(fetched);
    Ok(key)
}

async fn cached_google_oidc_decoding_key(kid: &str) -> Option<Arc<DecodingKey>> {
    let cache = google_oidc_certs_cache().read().await;
    cache.as_ref().and_then(|cached| {
        if cached.fetched_at.elapsed() < GOOGLE_OIDC_CERTS_TTL {
            cached.keys.get(kid).cloned()
        } else {
            None
        }
    })
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
    crate::security::install_jwt_crypto_provider();
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
    use super::{
        authorize_shared_secret, google_oidc_certs_cache, load_google_oidc_decoding_key, non_empty,
        CachedGoogleOidcCerts, SchedulerRequestAuthenticator,
    };
    use axum::http::{header, HeaderMap, HeaderValue};
    use jsonwebtoken::{encode, Algorithm, DecodingKey, EncodingKey, Header};
    use serde::Serialize;
    use std::sync::{Mutex, OnceLock};
    use std::{collections::HashMap, sync::Arc, time::Instant};

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
        crate::security::install_jwt_crypto_provider();
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
        crate::security::install_jwt_crypto_provider();
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
            crate::control::scheduler::SCHEDULER_AUTH_HEADER,
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
            SchedulerRequestAuthenticator::from_config(&crate::config::Config::default())
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
            SchedulerRequestAuthenticator::from_config(&crate::config::Config::default())
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
}
