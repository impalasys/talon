// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{proto, GrpcGatewayHandler};
use crate::control::config::proto as config_proto;
use crate::gateway::auth::{
    self as gateway_auth, AuthConfig, AuthMode, AuthzOperation, Claims, TalonGrantClaim,
};
use crate::gateway::Gateway;
use jsonwebtoken::{
    decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use serde::Deserialize;
use std::collections::HashSet;

const DEFAULT_GOOGLE_JWKS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";
const TALON_ACCESS_TOKEN_TTL_SECONDS: u64 = 900;

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

struct DelegatedTokenScope {
    namespace: String,
    agent: Option<String>,
    session: Option<String>,
    channel: Option<String>,
    expires_in: u64,
    origins: Vec<String>,
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

    pub async fn handle_mint_access_token(
        &self,
        req: tonic::Request<proto::MintAccessTokenRequest>,
    ) -> Result<tonic::Response<proto::MintAccessTokenResponse>, tonic::Status> {
        let metadata = req.metadata().clone();
        let request = req.into_inner();
        let scope = DelegatedTokenScope::from_request(request)?;

        let auth_config = self
            .gateway
            .auth_config
            .as_ref()
            .ok_or_else(|| tonic::Status::unauthenticated("JWT auth is not configured"))?;
        let secret = match auth_config {
            AuthConfig {
                mode: AuthMode::Jwt,
                jwt_secret: Some(secret),
                ..
            } => secret,
            _ => {
                return Err(tonic::Status::unauthenticated(
                    "JWT auth is required to mint Talon access tokens",
                ));
            }
        };

        let parent_claims = gateway_auth::jwt_claims_from_metadata(&metadata, auth_config)?
            .ok_or_else(|| tonic::Status::unauthenticated("Bearer JWT is required"))?;
        ensure_scope_authorized(auth_config, &metadata, &parent_claims, &scope)?;

        let (expires_in, expires_at) = delegated_expiration(&parent_claims, scope.expires_in)?;
        let origins = delegated_origins(&parent_claims, &scope.origins)?;
        let claims = Claims {
            sub: format!("delegated:{}", parent_claims.sub),
            aud: "talon".to_string(),
            exp: expires_at as usize,
            ns: Some(scope.namespace),
            agent: scope.agent,
            session: scope.session,
            channel: scope.channel,
            origins,
            grants: Vec::new(),
        };

        let access_token = jsonwebtoken::encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .map_err(|err| {
            tonic::Status::internal(format!("failed to mint Talon access token: {err}"))
        })?;

        Ok(tonic::Response::new(proto::MintAccessTokenResponse {
            access_token,
            token_type: "Bearer".to_string(),
            expires_in,
            expires_at,
        }))
    }
}

impl DelegatedTokenScope {
    fn from_request(request: proto::MintAccessTokenRequest) -> Result<Self, tonic::Status> {
        let namespace = request.namespace.trim().to_string();
        if namespace.is_empty() {
            return Err(tonic::Status::invalid_argument("namespace is required"));
        }

        let agent = non_empty_optional(request.agent);
        let session = non_empty_optional(request.session);
        let channel = non_empty_optional(request.channel);
        if session.is_some() && agent.is_none() {
            return Err(tonic::Status::invalid_argument(
                "session scope requires agent scope",
            ));
        }
        if channel.is_some() && (agent.is_some() || session.is_some()) {
            return Err(tonic::Status::invalid_argument(
                "channel scope cannot be combined with agent or session scope",
            ));
        }

        Ok(Self {
            namespace,
            agent,
            session,
            channel,
            expires_in: request.expires_in,
            origins: request.origins,
        })
    }
}

fn non_empty_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn ensure_scope_authorized(
    auth_config: &AuthConfig,
    metadata: &tonic::metadata::MetadataMap,
    parent_claims: &Claims,
    scope: &DelegatedTokenScope,
) -> Result<(), tonic::Status> {
    if !parent_claims.grants.is_empty() {
        if let Some(channel) = scope.channel.as_deref() {
            gateway_auth::check_channel_auth_for_operation(
                metadata,
                auth_config,
                AuthzOperation::ReadWrite,
                &scope.namespace,
                channel,
            )?;
        } else {
            gateway_auth::check_auth_for_operation(
                metadata,
                auth_config,
                AuthzOperation::ReadWrite,
                &scope.namespace,
                scope.agent.as_deref(),
                scope.session.as_deref(),
            )?;
        }
        return Ok(());
    }

    if parent_claims.sub.starts_with("oidc:") {
        return Err(tonic::Status::permission_denied(
            "OIDC token does not include any Talon grants",
        ));
    }
    if !claim_scope_allows_delegation(parent_claims, scope) {
        return Err(tonic::Status::permission_denied(
            "Requested token scope is broader than the authenticating token",
        ));
    }
    Ok(())
}

fn claim_scope_allows_delegation(claims: &Claims, scope: &DelegatedTokenScope) -> bool {
    let Some(allowed_ns) = claims.ns.as_deref().map(str::trim) else {
        return claim_resource_scope_allows_delegation(claims, scope);
    };
    if allowed_ns.is_empty() || !gateway_auth::namespace_scope_allows(allowed_ns, &scope.namespace)
    {
        return false;
    }
    claim_resource_scope_allows_delegation(claims, scope)
}

fn claim_resource_scope_allows_delegation(claims: &Claims, scope: &DelegatedTokenScope) -> bool {
    if !narrow_optional_scope(claims.agent.as_deref(), scope.agent.as_deref()) {
        return false;
    }
    if !narrow_optional_scope(claims.session.as_deref(), scope.session.as_deref()) {
        return false;
    }
    if !narrow_optional_scope(claims.channel.as_deref(), scope.channel.as_deref()) {
        return false;
    }
    if claims
        .channel
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return scope.agent.is_none() && scope.session.is_none();
    }
    if claims
        .agent
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return scope.channel.is_none();
    }
    true
}

fn narrow_optional_scope(parent: Option<&str>, child: Option<&str>) -> bool {
    let Some(parent) = parent.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    matches!(child, Some(child) if child == parent)
}

fn delegated_expiration(
    parent_claims: &Claims,
    requested_expires_in: u64,
) -> Result<(u64, u64), tonic::Status> {
    let now = unix_seconds()?;
    let parent_exp = parent_claims.exp as u64;
    if parent_exp <= now {
        return Err(tonic::Status::unauthenticated(
            "Authenticating token is expired",
        ));
    }
    let parent_remaining = parent_exp - now;
    let expires_in = if requested_expires_in == 0 {
        TALON_ACCESS_TOKEN_TTL_SECONDS.min(parent_remaining)
    } else {
        requested_expires_in
    };
    if expires_in == 0 {
        return Err(tonic::Status::invalid_argument(
            "expires_in must be positive",
        ));
    }
    if expires_in > parent_remaining {
        return Err(tonic::Status::permission_denied(
            "Requested token expiry is later than the authenticating token expiry",
        ));
    }
    Ok((expires_in, now + expires_in))
}

fn delegated_origins(
    parent_claims: &Claims,
    requested_origins: &[String],
) -> Result<Vec<String>, tonic::Status> {
    let requested = requested_origins
        .iter()
        .map(|origin| origin.trim())
        .filter(|origin| !origin.is_empty())
        .collect::<Vec<_>>();
    if requested.is_empty() {
        return Ok(parent_claims.origins.clone());
    }

    let parent_origins = parent_claims
        .origins
        .iter()
        .map(|origin| {
            gateway_auth::normalize_origin(origin).map_err(|message| {
                tonic::Status::permission_denied(format!(
                    "Authenticating token contains invalid origin scope: {message}"
                ))
            })
        })
        .collect::<Result<HashSet<_>, _>>()?;

    let mut origins = Vec::with_capacity(requested.len());
    for origin in requested {
        let normalized = gateway_auth::normalize_origin(origin).map_err(|message| {
            tonic::Status::invalid_argument(format!("Invalid origin: {message}"))
        })?;
        if !parent_origins.is_empty() && !parent_origins.contains(&normalized) {
            return Err(tonic::Status::permission_denied(
                "Requested origin scope is broader than the authenticating token",
            ));
        }
        origins.push(normalized);
    }
    origins.sort();
    origins.dedup();
    Ok(origins)
}

fn unix_seconds() -> Result<u64, tonic::Status> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| tonic::Status::internal(err.to_string()))
        .map(|duration| duration.as_secs())
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
        ns: None,
        agent: None,
        session: None,
        channel: None,
        origins: Vec::new(),
        grants: identity.grants.clone(),
    };

    jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|err| tonic::Status::internal(format!("failed to mint Talon access token: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ControlPlane;
    use crate::gateway::auth::{check_auth, verify_jwt};
    use crate::test_support::{EmptyPubSub, MockKvStore};
    use jsonwebtoken::{encode, EncodingKey, Header};
    use std::sync::Arc;

    fn handler(secret: &str) -> GrpcGatewayHandler {
        let control_plane =
            ControlPlane::builder(Arc::new(MockKvStore::default()), Arc::new(EmptyPubSub)).build();
        GrpcGatewayHandler {
            gateway: Arc::new(Gateway::from_control_plane(
                Some(AuthConfig::jwt(secret.to_string())),
                control_plane,
            )),
        }
    }

    fn bearer_request(
        token: &str,
        request: proto::MintAccessTokenRequest,
    ) -> tonic::Request<proto::MintAccessTokenRequest> {
        let mut req = tonic::Request::new(request);
        req.metadata_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
        req
    }

    fn token(secret: &str, mut claims: Claims) -> String {
        crate::control::security::install_jwt_crypto_provider();
        if claims.exp == 0 {
            claims.exp = (unix_seconds().unwrap() + 3600) as usize;
        }
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret.as_ref()),
        )
        .unwrap()
    }

    fn claims(ns: Option<&str>, agent: Option<&str>, session: Option<&str>) -> Claims {
        Claims {
            sub: "tenant-admin".to_string(),
            aud: "talon".to_string(),
            exp: 0,
            ns: ns.map(str::to_string),
            agent: agent.map(str::to_string),
            session: session.map(str::to_string),
            channel: None,
            origins: Vec::new(),
            grants: Vec::new(),
        }
    }

    fn mint_request(namespace: &str) -> proto::MintAccessTokenRequest {
        proto::MintAccessTokenRequest {
            namespace: namespace.to_string(),
            agent: None,
            session: None,
            channel: None,
            expires_in: 60,
            origins: Vec::new(),
        }
    }

    #[tokio::test]
    async fn mint_access_token_allows_descendant_namespace_and_agent_narrowing() {
        let secret = "delegate-secret";
        let parent = token(secret, claims(Some("Tenant:acme"), None, None));
        let handler = handler(secret);
        let mut request = mint_request("Tenant:acme:child");
        request.agent = Some("assistant".to_string());

        let response = handler
            .handle_mint_access_token(bearer_request(&parent, request))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(response.token_type, "Bearer");
        assert_eq!(response.expires_in, 60);
        let minted = verify_jwt(&response.access_token, secret).unwrap();
        assert_eq!(minted.sub, "delegated:tenant-admin");
        assert_eq!(minted.ns.as_deref(), Some("Tenant:acme:child"));
        assert_eq!(minted.agent.as_deref(), Some("assistant"));
        assert!(minted.grants.is_empty());

        let config = AuthConfig::jwt(secret.to_string());
        let mut metadata = tonic::metadata::MetadataMap::new();
        metadata.insert(
            "authorization",
            format!("Bearer {}", response.access_token).parse().unwrap(),
        );
        assert!(check_auth(
            &metadata,
            &config,
            "Tenant:acme:child",
            Some("assistant"),
            None
        )
        .is_ok());
        assert!(check_auth(
            &metadata,
            &config,
            "Tenant:acme:other",
            Some("assistant"),
            None
        )
        .is_err());
    }

    #[tokio::test]
    async fn mint_access_token_rejects_scope_widening() {
        let secret = "delegate-secret";
        let parent = token(secret, claims(Some("Tenant:acme"), Some("assistant"), None));
        let handler = handler(secret);

        let sibling = handler
            .handle_mint_access_token(bearer_request(&parent, mint_request("Tenant:acme2")))
            .await
            .expect_err("sibling namespace should be rejected");
        assert_eq!(sibling.code(), tonic::Code::PermissionDenied);

        let descendant_without_agent = handler
            .handle_mint_access_token(bearer_request(&parent, mint_request("Tenant:acme:child")))
            .await
            .expect_err("dropping parent agent scope should be rejected");
        assert_eq!(
            descendant_without_agent.code(),
            tonic::Code::PermissionDenied
        );

        let mut other_agent = mint_request("Tenant:acme:child");
        other_agent.agent = Some("other".to_string());
        let err = handler
            .handle_mint_access_token(bearer_request(&parent, other_agent))
            .await
            .expect_err("changing parent agent scope should be rejected");
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn mint_access_token_rejects_later_expiry() {
        let secret = "delegate-secret";
        let mut parent_claims = claims(Some("Tenant:acme"), None, None);
        parent_claims.exp = (unix_seconds().unwrap() + 30) as usize;
        let parent = token(secret, parent_claims);
        let handler = handler(secret);
        let mut request = mint_request("Tenant:acme");
        request.expires_in = 31;

        let err = handler
            .handle_mint_access_token(bearer_request(&parent, request))
            .await
            .expect_err("delegated token must not outlive parent");
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn mint_access_token_honors_parent_origin_scope() {
        let secret = "delegate-secret";
        let mut parent_claims = claims(Some("Tenant:acme"), None, None);
        parent_claims.origins = vec!["https://app.example.com".to_string()];
        let parent = token(secret, parent_claims);
        let handler = handler(secret);

        let mut request = mint_request("Tenant:acme");
        request.origins = vec!["https://APP.example.com:443".to_string()];
        let response = handler
            .handle_mint_access_token(bearer_request(&parent, request))
            .await
            .unwrap()
            .into_inner();
        let minted = verify_jwt(&response.access_token, secret).unwrap();
        assert_eq!(minted.origins, vec!["https://app.example.com"]);

        let mut request = mint_request("Tenant:acme");
        request.origins = vec!["https://other.example.com".to_string()];
        let err = handler
            .handle_mint_access_token(bearer_request(&parent, request))
            .await
            .expect_err("origin widening should be rejected");
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn mint_access_token_accepts_readwrite_grants_but_rejects_read_grants() {
        let secret = "delegate-secret";
        let handler = handler(secret);
        let mut parent_claims = claims(None, None, None);
        parent_claims.sub = "oidc:user123".to_string();
        parent_claims.grants = vec![TalonGrantClaim {
            kind: "readwrite".to_string(),
            namespace: Some("Tenant:acme".to_string()),
            agent: Some("assistant".to_string()),
            session: None,
            channel: None,
        }];
        let parent = token(secret, parent_claims.clone());

        let mut request = mint_request("Tenant:acme:child");
        request.agent = Some("assistant".to_string());
        handler
            .handle_mint_access_token(bearer_request(&parent, request))
            .await
            .unwrap();

        parent_claims.grants[0].kind = "read".to_string();
        let parent = token(secret, parent_claims);
        let mut request = mint_request("Tenant:acme:child");
        request.agent = Some("assistant".to_string());
        let err = handler
            .handle_mint_access_token(bearer_request(&parent, request))
            .await
            .expect_err("read-only grant must not mint write-capable token");
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }
}
