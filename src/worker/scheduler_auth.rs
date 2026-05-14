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
