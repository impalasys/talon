// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{proto, GrpcGatewayHandler};
use crate::control::config::proto as config_proto;
use crate::gateway::auth::{AuthConfig, AuthMode, Claims, TalonGrantClaim};
use crate::gateway::Gateway;
use jsonwebtoken::{
    decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use serde::Deserialize;

const DEFAULT_GOOGLE_JWKS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";
const TALON_ACCESS_TOKEN_TTL_SECONDS: u64 = 900;

#[derive(Debug, Deserialize, Clone)]
struct OidcIdentityClaims {
    #[serde(rename = "iss")]
    iss: String,
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

impl GrpcGatewayHandler {
    pub async fn handle_get_sso_config(
        &self,
        _req: tonic::Request<proto::GetSsoConfigRequest>,
    ) -> Result<tonic::Response<proto::GetSsoConfigResponse>, tonic::Status> {
        let client_id = std::env::var("TALON_GOOGLE_WEB_CLIENT_ID")
            .ok()
            .filter(|value| !value.trim().is_empty());
        Ok(tonic::Response::new(proto::GetSsoConfigResponse {
            google_sso_enabled: client_id.is_some(),
            google_web_client_id: client_id,
        }))
    }

    pub async fn handle_exchange_oidc_token(
        &self,
        req: tonic::Request<proto::ExchangeOidcTokenRequest>,
    ) -> Result<tonic::Response<proto::ExchangeOidcTokenResponse>, tonic::Status> {
        let request = req.into_inner();
        let id_token = request.id_token.trim();
        if id_token.is_empty() {
            return Err(tonic::Status::invalid_argument("id_token is required"));
        }

        let identity =
            verify_against_trust(&self.gateway, id_token, request.trust.as_deref()).await?;
        let access_token = mint_talon_access_token(&self.gateway, &identity)?;

        Ok(tonic::Response::new(proto::ExchangeOidcTokenResponse {
            access_token,
            token_type: "Bearer".to_string(),
            expires_in: TALON_ACCESS_TOKEN_TTL_SECONDS,
            subject: identity.claims.sub,
            email: identity.claims.email,
            trust: identity.trust_name,
            client_type: request.client_type,
        }))
    }
}

async fn verify_against_trust(
    gateway: &Gateway,
    id_token: &str,
    requested_trust: Option<&str>,
) -> Result<VerifiedOidcIdentity, tonic::Status> {
    let trust_config = gateway
        .trust_config
        .as_ref()
        .ok_or_else(|| tonic::Status::unauthenticated("OIDC trust is not configured"))?;

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
                    return Err(tonic::Status::unauthenticated(format!(
                        "trust '{}' has no grants",
                        entry.name
                    )));
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

    Err(tonic::Status::unauthenticated(last_error))
}

async fn verify_with_entry(
    entry: &config_proto::OidcTrustEntry,
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

async fn fetch_jwks(entry: &config_proto::OidcTrustEntry) -> Result<JwkSet, String> {
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

fn email_allowed(entry: &config_proto::OidcTrustEntry, claims: &OidcIdentityClaims) -> bool {
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

fn grant_claim_from_config(grant: &config_proto::OidcTrustGrant) -> TalonGrantClaim {
    let kind = match config_proto::oidc_trust_grant::Kind::try_from(grant.kind) {
        Ok(config_proto::oidc_trust_grant::Kind::Read) => "read",
        Ok(config_proto::oidc_trust_grant::Kind::Readwrite) => "readwrite",
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
) -> Result<String, tonic::Status> {
    let secret = match gateway.auth_config.as_ref() {
        Some(AuthConfig {
            mode: AuthMode::Jwt,
            jwt_secret: Some(secret),
            ..
        }) => secret,
        _ => {
            return Err(tonic::Status::internal(
                "Gateway JWT auth must be configured to issue Talon access tokens",
            ));
        }
    };

    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| tonic::Status::internal(err.to_string()))?
        .as_secs()
        + TALON_ACCESS_TOKEN_TTL_SECONDS;

    let claims = Claims {
        sub: format!("oidc:{}", identity.claims.sub),
        aud: "talon".to_string(),
        exp: exp as usize,
        oidc_issuer: Some(identity.claims.iss.clone()),
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
    .map_err(|err| tonic::Status::internal(format!("failed to mint Talon access token: {err}")))
}
