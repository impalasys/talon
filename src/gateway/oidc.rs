// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{
    decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::control::config::proto;
use crate::gateway::{
    auth::{AuthConfig, AuthMode, Claims, TalonGrantClaim},
    server::Gateway,
};

const DEFAULT_GOOGLE_JWKS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";
const TALON_ACCESS_TOKEN_TTL_SECONDS: u64 = 900;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OidcExchangeRequest {
    pub id_token: String,
    #[serde(default)]
    pub trust: Option<String>,
    #[serde(default)]
    pub client_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OidcExchangeResponse {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_in: u64,
    pub subject: String,
    pub email: Option<String>,
    pub trust: String,
    pub client_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthConfigResponse {
    pub google_sso_enabled: bool,
    pub google_web_client_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct OidcIdentityClaims {
    #[serde(rename = "iss")]
    _iss: String,
    sub: String,
    #[serde(rename = "aud")]
    _aud: serde_json::Value,
    #[serde(rename = "exp")]
    _exp: usize,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<bool>,
    #[serde(default)]
    hd: Option<String>,
}

#[derive(Debug)]
struct VerifiedOidcIdentity {
    trust_name: String,
    claims: OidcIdentityClaims,
    grants: Vec<TalonGrantClaim>,
}

pub async fn get_auth_config() -> Response {
    let client_id = std::env::var("TALON_GOOGLE_WEB_CLIENT_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());

    Json(AuthConfigResponse {
        google_sso_enabled: client_id.is_some(),
        google_web_client_id: client_id,
    })
    .into_response()
}

pub async fn exchange_oidc_token(
    State(gateway): State<Arc<Gateway>>,
    Json(request): Json<OidcExchangeRequest>,
) -> Response {
    match exchange_oidc_token_inner(&gateway, request).await {
        Ok(response) => Json(response).into_response(),
        Err((status, message)) => {
            (status, Json(serde_json::json!({ "error": message }))).into_response()
        }
    }
}

async fn exchange_oidc_token_inner(
    gateway: &Gateway,
    request: OidcExchangeRequest,
) -> Result<OidcExchangeResponse, (StatusCode, String)> {
    let id_token = request.id_token.trim();
    if id_token.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "idToken is required".to_string()));
    }

    let identity = verify_against_trust(gateway, id_token, request.trust.as_deref()).await?;
    let access_token = mint_talon_access_token(gateway, &identity)?;

    Ok(OidcExchangeResponse {
        access_token,
        token_type: "Bearer",
        expires_in: TALON_ACCESS_TOKEN_TTL_SECONDS,
        subject: identity.claims.sub,
        email: identity.claims.email,
        trust: identity.trust_name,
        client_type: request.client_type,
    })
}

async fn verify_against_trust(
    gateway: &Gateway,
    id_token: &str,
    requested_trust: Option<&str>,
) -> Result<VerifiedOidcIdentity, (StatusCode, String)> {
    let trust_config = gateway.trust_config.as_ref().ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "OIDC trust is not configured".to_string(),
        )
    })?;

    let mut last_error = "OIDC token did not match configured trust".to_string();
    for entry in &trust_config.oidc {
        if requested_trust.is_some_and(|name| name != entry.name) {
            continue;
        }
        match verify_with_entry(entry, id_token).await {
            Ok(claims) => {
                let grants = entry
                    .grants
                    .iter()
                    .map(grant_claim_from_config)
                    .collect::<Vec<_>>();
                if grants.is_empty() {
                    return Err((
                        StatusCode::UNAUTHORIZED,
                        format!("trust '{}' has no grants", entry.name),
                    ));
                }
                return Ok(VerifiedOidcIdentity {
                    trust_name: entry.name.clone(),
                    claims,
                    grants,
                });
            }
            Err(err) => last_error = err,
        }
    }

    Err((StatusCode::UNAUTHORIZED, last_error))
}

async fn verify_with_entry(
    entry: &proto::OidcTrustEntry,
    id_token: &str,
) -> Result<OidcIdentityClaims, String> {
    if entry.audiences.is_empty() {
        return Err(format!("trust '{}' has no audiences", entry.name));
    }

    let header =
        decode_header(id_token).map_err(|err| format!("invalid OIDC token header: {err}"))?;
    let kid = header
        .kid
        .as_deref()
        .ok_or_else(|| "OIDC token header missing kid".to_string())?;
    let jwks = fetch_jwks(entry).await?;
    let jwk = jwks
        .keys
        .iter()
        .find(|key| key.common.key_id.as_deref() == Some(kid))
        .ok_or_else(|| "OIDC signing key not found in JWKS".to_string())?;
    let decoding_key = DecodingKey::from_jwk(jwk)
        .map_err(|err| format!("failed to build OIDC decoding key: {err}"))?;

    let mut validation = Validation::new(header.alg);
    validation.set_audience(
        &entry
            .audiences
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    validation.set_issuer(&[entry.issuer.as_str()]);
    validation.leeway = entry.clock_skew_seconds as u64;
    reject_unsupported_algorithm(header.alg)?;

    let claims = decode::<OidcIdentityClaims>(id_token, &decoding_key, &validation)
        .map_err(|err| format!("invalid OIDC token: {err}"))?
        .claims;

    let uses_email_policy = !entry.allowed_emails.is_empty() || !entry.allowed_domains.is_empty();
    if uses_email_policy && claims.email_verified != Some(true) {
        return Err("OIDC email is not verified".to_string());
    }
    if !email_allowed(entry, &claims) {
        return Err("OIDC identity is not allowed by email/domain policy".to_string());
    }

    Ok(claims)
}

fn reject_unsupported_algorithm(algorithm: Algorithm) -> Result<(), String> {
    match algorithm {
        Algorithm::RS256
        | Algorithm::RS384
        | Algorithm::RS512
        | Algorithm::ES256
        | Algorithm::ES384 => Ok(()),
        _ => Err(format!("unsupported OIDC signing algorithm: {algorithm:?}")),
    }
}

async fn fetch_jwks(entry: &proto::OidcTrustEntry) -> Result<JwkSet, String> {
    let url = if !entry.jwks_url.trim().is_empty() {
        entry.jwks_url.trim().to_string()
    } else if entry.issuer == "https://accounts.google.com" {
        DEFAULT_GOOGLE_JWKS_URL.to_string()
    } else {
        format!(
            "{}/.well-known/jwks.json",
            entry.issuer.trim_end_matches('/')
        )
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|err| format!("failed to build OIDC JWKS client: {err}"))?;

    client
        .get(&url)
        .send()
        .await
        .map_err(|err| format!("failed to fetch OIDC JWKS: {err}"))?
        .error_for_status()
        .map_err(|err| format!("OIDC JWKS request failed: {err}"))?
        .json::<JwkSet>()
        .await
        .map_err(|err| format!("failed to parse OIDC JWKS: {err}"))
}

fn email_allowed(entry: &proto::OidcTrustEntry, claims: &OidcIdentityClaims) -> bool {
    if entry.allowed_emails.is_empty() && entry.allowed_domains.is_empty() {
        return true;
    }

    if let Some(email) = claims.email.as_deref() {
        if entry
            .allowed_emails
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(email))
        {
            return true;
        }
    }

    let hosted_domain = claims.hd.as_deref();
    let email_domain = claims
        .email
        .as_deref()
        .and_then(|email| email.split_once('@').map(|(_, domain)| domain));
    entry.allowed_domains.iter().any(|allowed| {
        hosted_domain.is_some_and(|domain| allowed.eq_ignore_ascii_case(domain))
            || email_domain.is_some_and(|domain| allowed.eq_ignore_ascii_case(domain))
    })
}

fn grant_claim_from_config(grant: &proto::OidcTrustGrant) -> TalonGrantClaim {
    let kind = match proto::oidc_trust_grant::Kind::try_from(grant.kind) {
        Ok(proto::oidc_trust_grant::Kind::Read) => "read",
        Ok(proto::oidc_trust_grant::Kind::Readwrite) => "readwrite",
        _ => "unspecified",
    };

    TalonGrantClaim {
        kind: kind.to_string(),
        namespace: non_empty(&grant.namespace),
        agent: non_empty(&grant.agent),
        session: non_empty(&grant.session),
        channel: non_empty(&grant.channel),
    }
}

fn non_empty(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.trim().to_string())
}

fn mint_talon_access_token(
    gateway: &Gateway,
    identity: &VerifiedOidcIdentity,
) -> Result<String, (StatusCode, String)> {
    let secret = match gateway.auth_config.as_ref() {
        Some(AuthConfig {
            mode: AuthMode::Jwt,
            jwt_secret: Some(secret),
            ..
        }) => secret,
        _ => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Gateway JWT auth must be configured to issue Talon access tokens".to_string(),
            ));
        }
    };

    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .as_secs()
        + TALON_ACCESS_TOKEN_TTL_SECONDS;

    let claims = Claims {
        sub: format!("oidc:{}", identity.claims.sub),
        aud: "talon".to_string(),
        exp: exp as usize,
        ns: None,
        agent: None,
        session: None,
        channel: None,
        grants: identity.grants.clone(),
    };

    jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to mint Talon access token: {err}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> proto::OidcTrustEntry {
        proto::OidcTrustEntry {
            name: "google".to_string(),
            issuer: "https://accounts.google.com".to_string(),
            audiences: vec!["client".to_string()],
            allowed_domains: Vec::new(),
            allowed_emails: Vec::new(),
            jwks_url: String::new(),
            clock_skew_seconds: 0,
            grants: Vec::new(),
        }
    }

    fn claims(email: Option<&str>, verified: Option<bool>, hd: Option<&str>) -> OidcIdentityClaims {
        OidcIdentityClaims {
            _iss: "https://accounts.google.com".to_string(),
            sub: "subject".to_string(),
            _aud: serde_json::json!("client"),
            _exp: 10_000_000_000,
            email: email.map(str::to_string),
            email_verified: verified,
            hd: hd.map(str::to_string),
        }
    }

    #[test]
    fn email_policy_allows_empty_policy_and_exact_email() {
        let mut entry = entry();
        assert!(email_allowed(&entry, &claims(None, None, None)));

        entry.allowed_emails = vec!["alice@impala.systems".to_string()];
        assert!(email_allowed(
            &entry,
            &claims(Some("alice@impala.systems"), Some(true), None)
        ));
        assert!(!email_allowed(
            &entry,
            &claims(Some("bob@impala.systems"), Some(true), None)
        ));
    }

    #[test]
    fn email_policy_allows_hosted_or_email_domain() {
        let mut entry = entry();
        entry.allowed_domains = vec!["impala.systems".to_string()];

        assert!(email_allowed(
            &entry,
            &claims(
                Some("alice@other.example"),
                Some(true),
                Some("impala.systems")
            )
        ));
        assert!(email_allowed(
            &entry,
            &claims(Some("alice@impala.systems"), Some(true), None)
        ));
        assert!(!email_allowed(
            &entry,
            &claims(
                Some("alice@other.example"),
                Some(true),
                Some("other.example")
            )
        ));
    }

    #[test]
    fn grant_claim_from_config_maps_readwrite_selectors() {
        let grant = proto::OidcTrustGrant {
            kind: proto::oidc_trust_grant::Kind::Readwrite as i32,
            namespace: "Support".to_string(),
            agent: "retention-reviewer".to_string(),
            session: String::new(),
            channel: String::new(),
        };

        assert_eq!(
            grant_claim_from_config(&grant),
            TalonGrantClaim {
                kind: "readwrite".to_string(),
                namespace: Some("Support".to_string()),
                agent: Some("retention-reviewer".to_string()),
                session: None,
                channel: None,
            }
        );
    }
}
