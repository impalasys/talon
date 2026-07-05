// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

// Gateway Authentication - platform JWT auth middleware for the Gateway
// This file is owned by: Agent 4 (Gateway Auth & Tools)

use crate::control::security::platform_jwt;
use crate::gateway::server::Gateway;
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tonic::{metadata::MetadataMap, Status};
use url::Url;

pub(crate) const TALON_GRPC_WEB_REQUEST_METADATA: &str = "x-talon-grpc-web-request";
pub(crate) const TALON_GRPC_WEB_ORIGIN_METADATA: &str = "x-talon-grpc-web-origin";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthzOperation {
    Read,
    ReadWrite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuthMode {
    Open,
    Jwt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub mode: AuthMode,
}

impl AuthConfig {
    pub fn open() -> Self {
        Self {
            mode: AuthMode::Open,
        }
    }

    pub fn jwt_platform() -> Self {
        Self {
            mode: AuthMode::Jwt,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct Claims {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    pub sub: String,
    pub aud: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iat: Option<usize>,
    pub exp: usize,
    #[serde(rename = "talon:ns")]
    pub ns: Option<String>,
    #[serde(rename = "talon:agent")]
    pub agent: Option<String>,
    #[serde(rename = "talon:session")]
    pub session: Option<String>,
    #[serde(rename = "talon:channel")]
    pub channel: Option<String>,
    #[serde(
        rename = "talon:origins",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub origins: Vec<String>,
    #[serde(rename = "grants", default)]
    pub grants: Vec<TalonGrantClaim>,
}

impl<'de> Deserialize<'de> for Claims {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawClaims {
            #[serde(default)]
            iss: Option<String>,
            sub: String,
            aud: String,
            #[serde(default)]
            iat: Option<usize>,
            exp: usize,
            #[serde(rename = "talon:ns")]
            ns: Option<String>,
            #[serde(rename = "talon:agent")]
            agent: Option<String>,
            #[serde(rename = "talon:session")]
            session: Option<String>,
            #[serde(rename = "talon:channel")]
            channel: Option<String>,
            #[serde(rename = "talon:origins", default)]
            origins: Vec<String>,
            #[serde(rename = "grants", default)]
            grants: Vec<TalonGrantClaim>,
            #[serde(rename = "talon:grants", default)]
            legacy_grants: Vec<TalonGrantClaim>,
        }

        let mut raw = RawClaims::deserialize(deserializer)?;
        raw.grants.extend(raw.legacy_grants);
        Ok(Self {
            iss: raw.iss,
            sub: raw.sub,
            aud: raw.aud,
            iat: raw.iat,
            exp: raw.exp,
            ns: raw.ns,
            agent: raw.agent,
            session: raw.session,
            channel: raw.channel,
            origins: raw.origins,
            grants: raw.grants,
        })
    }
}

pub fn rate_limit_key_from_request<T>(request: &tonic::Request<T>) -> Option<String> {
    let subject = request.extensions().get::<Claims>()?.sub.trim();
    if subject.is_empty() {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(subject.as_bytes());
    Some(format!("sub:{:x}", hasher.finalize()))
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct TalonGrantClaim {
    pub kind: String,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
}

pub fn verify_platform_access_jwt(token: &str) -> Result<Claims, Status> {
    let key = platform_jwt::load_key()
        .map_err(|err| Status::internal(format!("Platform JWT key is not configured: {err}")))?;
    let issuer = platform_jwt::issuer()
        .map_err(|err| Status::internal(format!("Platform JWT issuer is not configured: {err}")))?;
    let claims = key
        .verify::<Claims>(token, &issuer, platform_jwt::TALON_GATEWAY_AUDIENCE)
        .map_err(|err| Status::unauthenticated(format!("Invalid token: {err}")))?;
    Ok(claims)
}

fn verify_bearer_jwt(token: &str, _auth_config: &AuthConfig) -> Result<Claims, Status> {
    verify_platform_access_jwt(token)
}

fn bearer_token(auth_header: &str) -> Result<&str, Status> {
    match auth_header.get(..7) {
        Some(scheme) if auth_header.len() > 7 && scheme.eq_ignore_ascii_case("bearer ") => {
            Ok(&auth_header[7..])
        }
        _ => Err(Status::unauthenticated("Missing bearer token")),
    }
}

pub fn check_auth(
    metadata: &MetadataMap,
    auth_config: &AuthConfig,
    ns: &str,
    agent: Option<&str>,
    session: Option<&str>,
) -> Result<(), Status> {
    check_auth_for_operation(
        metadata,
        auth_config,
        AuthzOperation::ReadWrite,
        ns,
        agent,
        session,
    )
}

pub fn check_auth_for_operation(
    metadata: &MetadataMap,
    auth_config: &AuthConfig,
    operation: AuthzOperation,
    ns: &str,
    agent: Option<&str>,
    session: Option<&str>,
) -> Result<(), Status> {
    let auth_header = metadata.get("authorization").and_then(|v| v.to_str().ok());
    let origin_scope = metadata_origin_scope(metadata);
    check_auth_header_for_operation_with_origin_scope(
        auth_header,
        origin_scope,
        auth_config,
        operation,
        ns,
        agent,
        session,
    )
}

/// Verify and return bearer JWT claims for callers that need to derive an
/// effective read scope from the token after normal authorization succeeds.
///
/// This helper is intentionally not an authorization check by itself. Use
/// `check_auth_for_operation` or `check_channel_auth_for_operation` first, then
/// call this when the caller needs claim details such as `talon:ns`, scoped
/// agent/session/channel values, or `talon:grants`. Search uses this to narrow a
/// broad query to the caller's authorized source filters before querying the
/// `DocumentStore`. It still enforces token validity conditions that require
/// request metadata, such as `talon:origins`.
///
/// Returns `Ok(None)` for non-JWT auth modes, because there are no bearer JWT
/// claims to inspect in those cases.
pub fn jwt_claims_from_metadata(
    metadata: &MetadataMap,
    auth_config: &AuthConfig,
) -> Result<Option<Claims>, Status> {
    if auth_config.mode != AuthMode::Jwt {
        return Ok(None);
    }
    let auth_header = metadata
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Status::unauthenticated("Missing authorization header"))?;
    let token = bearer_token(auth_header)?;
    let claims = verify_bearer_jwt(token, auth_config)?;
    check_origin_scope(&claims, metadata_origin_scope(metadata))?;
    Ok(Some(claims))
}

pub fn check_auth_header(
    auth_header: Option<&str>,
    auth_config: &AuthConfig,
    ns: &str,
    agent: Option<&str>,
    session: Option<&str>,
) -> Result<(), Status> {
    check_auth_header_for_operation(
        auth_header,
        auth_config,
        AuthzOperation::ReadWrite,
        ns,
        agent,
        session,
    )
}

pub fn check_auth_header_for_operation(
    auth_header: Option<&str>,
    auth_config: &AuthConfig,
    operation: AuthzOperation,
    ns: &str,
    agent: Option<&str>,
    session: Option<&str>,
) -> Result<(), Status> {
    check_auth_header_for_operation_with_origin_scope(
        auth_header,
        OriginScope::Ignore,
        auth_config,
        operation,
        ns,
        agent,
        session,
    )
}

pub fn check_auth_header_for_operation_with_origin(
    auth_header: Option<&str>,
    origin: Option<&str>,
    auth_config: &AuthConfig,
    operation: AuthzOperation,
    ns: &str,
    agent: Option<&str>,
    session: Option<&str>,
) -> Result<(), Status> {
    check_auth_header_for_operation_with_origin_scope(
        auth_header,
        OriginScope::Enforce(origin),
        auth_config,
        operation,
        ns,
        agent,
        session,
    )
}

fn check_auth_header_for_operation_with_origin_scope(
    auth_header: Option<&str>,
    origin_scope: OriginScope<'_>,
    auth_config: &AuthConfig,
    operation: AuthzOperation,
    ns: &str,
    agent: Option<&str>,
    session: Option<&str>,
) -> Result<(), Status> {
    match auth_config.mode {
        AuthMode::Open => Ok(()),
        AuthMode::Jwt => {
            let auth_header = auth_header
                .ok_or_else(|| Status::unauthenticated("Missing authorization header"))?;

            let token = bearer_token(auth_header)?;

            let claims = verify_bearer_jwt(token, auth_config)?;

            check_claim_scope(&claims, operation, ns, agent, session, None, origin_scope)
        }
    }
}

pub fn check_channel_auth(
    metadata: &MetadataMap,
    auth_config: &AuthConfig,
    ns: &str,
    channel: &str,
) -> Result<(), Status> {
    check_channel_auth_for_operation(
        metadata,
        auth_config,
        AuthzOperation::ReadWrite,
        ns,
        channel,
    )
}

pub fn check_channel_auth_for_operation(
    metadata: &MetadataMap,
    auth_config: &AuthConfig,
    operation: AuthzOperation,
    ns: &str,
    channel: &str,
) -> Result<(), Status> {
    let origin_scope = metadata_origin_scope(metadata);
    match auth_config.mode {
        AuthMode::Jwt => {
            let auth_header = metadata
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| Status::unauthenticated("Missing authorization header"))?;

            let token = bearer_token(auth_header)?;

            let claims = verify_bearer_jwt(token, auth_config)?;
            check_claim_scope(
                &claims,
                operation,
                ns,
                None,
                None,
                Some(channel),
                origin_scope,
            )
        }
        _ => check_auth_for_operation(metadata, auth_config, operation, ns, None, None),
    }
}

fn check_claim_scope(
    claims: &Claims,
    operation: AuthzOperation,
    ns: &str,
    agent: Option<&str>,
    session: Option<&str>,
    channel: Option<&str>,
    origin_scope: OriginScope<'_>,
) -> Result<(), Status> {
    check_origin_scope(claims, origin_scope)?;

    if claims.grants.is_empty()
        && claims.sub.starts_with("oidc:")
        && claims.ns.is_none()
        && claims.agent.is_none()
        && claims.session.is_none()
        && claims.channel.is_none()
    {
        return Err(Status::permission_denied(
            "OIDC token does not include any Talon grants",
        ));
    }

    if !claims.grants.is_empty() {
        if claims
            .grants
            .iter()
            .any(|grant| grant_allows(grant, operation, ns, agent, session, channel))
        {
            return Ok(());
        }
        return Err(Status::permission_denied(
            "Token grants do not allow this operation",
        ));
    }

    if let Some(allowed_ns) = &claims.ns {
        if !namespace_scope_allows(allowed_ns, ns) {
            return Err(Status::permission_denied(format!(
                "Token scope restricted to namespace: {}",
                allowed_ns
            )));
        }
    }

    if let Some(allowed_channel) = &claims.channel {
        if allowed_channel.is_empty() {
            return Err(Status::permission_denied(
                "Token scope restricted to an empty channel",
            ));
        }
        let allowed_ns = claims.ns.as_deref().filter(|value| !value.is_empty());
        if allowed_ns.is_none() {
            return Err(Status::permission_denied(
                "Channel-scoped token must include talon:ns",
            ));
        }
        let target_channel = channel.ok_or_else(|| {
            Status::permission_denied(
                "Token restricted to a specific channel, but this request is not a channel message operation",
            )
        })?;
        if allowed_channel != target_channel {
            return Err(Status::permission_denied(format!(
                "Token scope restricted to channel: {}",
                allowed_channel
            )));
        }
    }

    if let Some(allowed_agent) = &claims.agent {
        if let Some(target_agent) = agent {
            if allowed_agent != target_agent && !allowed_agent.is_empty() {
                return Err(Status::permission_denied(format!(
                    "Token scope restricted to agent: {}",
                    allowed_agent
                )));
            }
        }
    }

    if let Some(allowed_session) = &claims.session {
        let target_session = session.ok_or_else(|| {
            Status::permission_denied(
                "Token restricted to specific session, but none specified in request",
            )
        })?;
        if allowed_session != target_session && !allowed_session.is_empty() {
            return Err(Status::permission_denied(format!(
                "Token scope restricted to session: {}",
                allowed_session
            )));
        }
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum OriginScope<'a> {
    Ignore,
    Enforce(Option<&'a str>),
}

fn metadata_origin_scope(metadata: &MetadataMap) -> OriginScope<'_> {
    if metadata.get(TALON_GRPC_WEB_REQUEST_METADATA).is_none() {
        return OriginScope::Ignore;
    }
    OriginScope::Enforce(
        metadata
            .get(TALON_GRPC_WEB_ORIGIN_METADATA)
            .and_then(|value| value.to_str().ok()),
    )
}

fn check_origin_scope(claims: &Claims, origin_scope: OriginScope<'_>) -> Result<(), Status> {
    if claims.origins.is_empty() {
        return Ok(());
    }
    let OriginScope::Enforce(origin) = origin_scope else {
        return Ok(());
    };

    let origin = origin.ok_or_else(|| {
        Status::permission_denied(
            "Token scope restricted to origins, but request has no Origin header",
        )
    })?;
    let origin = normalize_origin(origin).map_err(|message| {
        Status::permission_denied(format!("Invalid request Origin: {message}"))
    })?;

    for allowed in &claims.origins {
        let allowed = normalize_origin(allowed).map_err(|message| {
            Status::permission_denied(format!("Token contains invalid origin scope: {message}"))
        })?;
        if allowed == origin {
            return Ok(());
        }
    }

    Err(Status::permission_denied(format!(
        "Token scope restricted to origins: {}",
        claims.origins.join(", ")
    )))
}

pub(crate) fn normalize_origin(value: &str) -> Result<String, String> {
    let value = value.trim();
    let url = Url::parse(value).map_err(|err| err.to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("origin scheme must be http or https".to_string());
    }
    if url.username() != "" || url.password().is_some() {
        return Err("origin must not include credentials".to_string());
    }
    if url.host_str().is_none() {
        return Err("origin must include a host".to_string());
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err("origin must not include a path, query, or fragment".to_string());
    }
    Ok(url.origin().ascii_serialization())
}

fn grant_allows(
    grant: &TalonGrantClaim,
    operation: AuthzOperation,
    ns: &str,
    agent: Option<&str>,
    session: Option<&str>,
    channel: Option<&str>,
) -> bool {
    let kind = grant.kind.trim();
    match (kind, operation) {
        ("read", AuthzOperation::Read) | ("readwrite", _) => {}
        _ => return false,
    }

    if !namespace_selector_matches(grant.namespace.as_deref(), ns) {
        return false;
    }
    if !selector_matches(grant.agent.as_deref(), agent) {
        return false;
    }
    if !selector_matches(grant.session.as_deref(), session) {
        return false;
    }
    if !selector_matches(grant.channel.as_deref(), channel) {
        return false;
    }

    true
}

fn selector_matches(allowed: Option<&str>, target: Option<&str>) -> bool {
    let Some(allowed) = allowed.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    matches!(target, Some(target) if allowed == target)
}

fn namespace_selector_matches(allowed: Option<&str>, target: &str) -> bool {
    let Some(allowed) = allowed.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    namespace_scope_allows(allowed, target)
}

pub(crate) fn namespace_scope_allows(allowed: &str, target: &str) -> bool {
    let allowed = allowed.trim();
    if allowed.is_empty() {
        return target.trim().is_empty();
    }
    target == allowed
        || target
            .strip_prefix(allowed)
            .is_some_and(|suffix| suffix.starts_with(':'))
}

/// gRPC Interceptor that extracts and verifies JWTs, attaching claims to the request.
#[derive(Clone)]
pub struct TalonAuthInterceptor {
    pub config: AuthConfig,
}

impl tonic::service::Interceptor for TalonAuthInterceptor {
    fn call(&mut self, mut request: tonic::Request<()>) -> Result<tonic::Request<()>, Status> {
        if self.config.mode == AuthMode::Open {
            return Ok(request);
        }

        let auth_header = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Status::unauthenticated("Missing authorization header"))?;

        match self.config.mode {
            AuthMode::Jwt => {
                let token = bearer_token(auth_header)?;
                let claims = verify_bearer_jwt(token, &self.config)?;

                // Attach claims to request extensions so handlers can access them
                request.extensions_mut().insert(claims);
                Ok(request)
            }
            AuthMode::Open => Ok(request),
        }
    }
}

// Axum auth layer for REST surfaces served by the gateway.
pub async fn auth_layer(State(state): State<Arc<Gateway>>, req: Request, next: Next) -> Response {
    let auth_config = match &state.auth_config {
        Some(config) => config,
        None => return next.run(req).await,
    };

    match auth_config.mode {
        AuthMode::Open => next.run(req).await,
        AuthMode::Jwt => {
            let origin = req
                .headers()
                .get(header::ORIGIN)
                .and_then(|value| value.to_str().ok());
            if let Some(auth_header) = req.headers().get(header::AUTHORIZATION) {
                if let Ok(auth_str) = auth_header.to_str() {
                    if let Ok(token) = bearer_token(auth_str) {
                        if verify_bearer_jwt(token, auth_config)
                            .and_then(|claims| {
                                check_origin_scope(&claims, OriginScope::Enforce(origin))?;
                                Ok(claims)
                            })
                            .is_ok()
                        {
                            return next.run(req).await;
                        }
                    }
                }
            }
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::server::Gateway;
    use crate::test_support::{
        EmptyPubSub, MockKvStore, PlatformJwtEnvGuard, TEST_PLATFORM_JWT_ISSUER,
    };
    use axum::{
        body::Body,
        http::{Request as HttpRequest, StatusCode as HttpStatusCode},
        middleware::from_fn_with_state,
        routing::get,
        Router,
    };
    use std::sync::Arc;
    use tonic::metadata::MetadataMap;
    use tonic::service::Interceptor;
    use tower::ServiceExt;

    fn gateway_with_auth(auth_config: Option<AuthConfig>) -> Arc<Gateway> {
        let control_plane = crate::control::ControlPlane::builder(
            Arc::new(MockKvStore::default()),
            Arc::new(EmptyPubSub),
        )
        .build();
        Arc::new(Gateway::from_control_plane(auth_config, control_plane))
    }

    #[test]
    fn test_auth_config_builders() {
        let open = AuthConfig::open();
        assert_eq!(open.mode, AuthMode::Open);

        let jwt = AuthConfig::jwt_platform();
        assert_eq!(jwt.mode, AuthMode::Jwt);
    }

    fn create_token(ns: Option<&str>, agent: Option<&str>, session: Option<&str>) -> String {
        sign_platform_claims(&Claims {
            iss: Some(TEST_PLATFORM_JWT_ISSUER.to_string()),
            sub: "user123".to_string(),
            aud: platform_jwt::TALON_GATEWAY_AUDIENCE.to_string(),
            iat: Some(1),
            exp: 10000000000, // far future
            ns: ns.map(|s| s.to_string()),
            agent: agent.map(|s| s.to_string()),
            session: session.map(|s| s.to_string()),
            channel: None,
            origins: Vec::new(),
            grants: Vec::new(),
        })
    }

    fn create_origin_token(origins: Vec<&str>) -> String {
        sign_platform_claims(&Claims {
            iss: Some(TEST_PLATFORM_JWT_ISSUER.to_string()),
            sub: "browser-client".to_string(),
            aud: platform_jwt::TALON_GATEWAY_AUDIENCE.to_string(),
            iat: Some(1),
            exp: 10000000000,
            ns: None,
            agent: None,
            session: None,
            channel: None,
            origins: origins.into_iter().map(str::to_string).collect(),
            grants: Vec::new(),
        })
    }

    fn jwt_auth_config() -> AuthConfig {
        AuthConfig::jwt_platform()
    }

    fn platform_claims(audience: &str) -> Claims {
        Claims {
            iss: Some(TEST_PLATFORM_JWT_ISSUER.to_string()),
            sub: "talon".to_string(),
            aud: audience.to_string(),
            iat: Some(1),
            exp: 10000000000,
            ns: Some("ns".to_string()),
            agent: None,
            session: None,
            channel: None,
            origins: Vec::new(),
            grants: Vec::new(),
        }
    }

    fn sign_platform_claims(claims: &Claims) -> String {
        platform_jwt::PlatformJwtKey::from_pem(platform_jwt::TEST_RSA_PRIVATE_KEY)
            .unwrap()
            .sign(claims)
            .unwrap()
    }

    fn metadata_with_bearer(token: &str) -> MetadataMap {
        let mut metadata = MetadataMap::new();
        metadata.insert("authorization", format!("Bearer {token}").parse().unwrap());
        metadata
    }

    #[tokio::test]
    async fn platform_gateway_auth_accepts_access_profile_and_rejects_broker_assertion() {
        let _guard = PlatformJwtEnvGuard::acquire().await;
        let config = jwt_auth_config();

        let access_token =
            sign_platform_claims(&platform_claims(platform_jwt::TALON_GATEWAY_AUDIENCE));
        let access = jwt_claims_from_metadata(&metadata_with_bearer(&access_token), &config)
            .unwrap()
            .unwrap();
        assert_eq!(access.aud, platform_jwt::TALON_GATEWAY_AUDIENCE);

        let broker_assertion =
            sign_platform_claims(&platform_claims(platform_jwt::MCP_AUTH_BROKER_AUDIENCE));
        assert!(
            jwt_claims_from_metadata(&metadata_with_bearer(&broker_assertion), &config).is_err()
        );
    }

    #[test]
    fn test_check_auth_jwt_scopes() {
        let _guard = PlatformJwtEnvGuard::acquire_blocking();
        let config = jwt_auth_config();
        let mut metadata = MetadataMap::new();

        // 1. Namespace scope
        let token = create_token(Some("my-ns"), None, None);
        metadata.insert(
            "authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );

        // Success: matching ns
        assert!(check_auth(&metadata, &config, "my-ns", None, None).is_ok());
        // Success: descendant namespace
        assert!(check_auth(&metadata, &config, "my-ns:child", None, None).is_ok());
        // Fail: wrong ns
        assert!(check_auth(&metadata, &config, "other-ns", None, None).is_err());
        // Fail: prefix sibling must not match
        assert!(check_auth(&metadata, &config, "my-ns2", None, None).is_err());

        // 2. Agent scope
        let token = create_token(Some("my-ns"), Some("agent-42"), None);
        metadata.insert(
            "authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );

        // Success: matching agent
        assert!(check_auth(&metadata, &config, "my-ns", Some("agent-42"), None).is_ok());
        // Fail: wrong agent
        assert!(check_auth(&metadata, &config, "my-ns", Some("agent-99"), None).is_err());

        // 3. Session scope
        let token = create_token(Some("my-ns"), Some("agent-42"), Some("sess-1"));
        metadata.insert(
            "authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );

        // Success: matching session
        assert!(check_auth(
            &metadata,
            &config,
            "my-ns",
            Some("agent-42"),
            Some("sess-1")
        )
        .is_ok());
        // Fail: wrong session
        assert!(check_auth(
            &metadata,
            &config,
            "my-ns",
            Some("agent-42"),
            Some("sess-2")
        )
        .is_err());
    }

    #[test]
    fn test_origin_scoped_jwt_is_enforced_only_for_grpc_web_metadata() {
        let _guard = PlatformJwtEnvGuard::acquire_blocking();
        let config = jwt_auth_config();
        let token = create_origin_token(vec!["https://app.example.com"]);
        let mut metadata = auth_metadata(&token);

        assert!(check_auth(&metadata, &config, "any", None, None).is_ok());

        metadata.insert("origin", "https://evil.example.com".parse().unwrap());
        assert!(check_auth(&metadata, &config, "any", None, None).is_ok());

        metadata.insert(TALON_GRPC_WEB_REQUEST_METADATA, "true".parse().unwrap());
        assert!(check_auth(&metadata, &config, "any", None, None).is_err());

        metadata.insert(
            TALON_GRPC_WEB_ORIGIN_METADATA,
            "https://evil.example.com".parse().unwrap(),
        );
        assert!(check_auth(&metadata, &config, "any", None, None).is_err());

        metadata.insert(
            TALON_GRPC_WEB_ORIGIN_METADATA,
            "https://APP.example.com:443".parse().unwrap(),
        );
        assert!(check_auth(&metadata, &config, "any", None, None).is_ok());

        let claims = jwt_claims_from_metadata(&metadata, &config)
            .unwrap()
            .unwrap();
        assert_eq!(claims.origins, vec!["https://app.example.com"]);
    }

    fn create_channel_token(ns: Option<&str>, channel: Option<&str>) -> String {
        sign_platform_claims(&Claims {
            iss: Some(TEST_PLATFORM_JWT_ISSUER.to_string()),
            sub: "channel-client".to_string(),
            aud: platform_jwt::TALON_GATEWAY_AUDIENCE.to_string(),
            iat: Some(1),
            exp: 10000000000,
            ns: ns.map(|s| s.to_string()),
            agent: None,
            session: None,
            channel: channel.map(|s| s.to_string()),
            origins: Vec::new(),
            grants: Vec::new(),
        })
    }

    fn create_grant_token(grants: Vec<TalonGrantClaim>) -> String {
        sign_platform_claims(&Claims {
            iss: Some(TEST_PLATFORM_JWT_ISSUER.to_string()),
            sub: "oidc:user123".to_string(),
            aud: platform_jwt::TALON_GATEWAY_AUDIENCE.to_string(),
            iat: Some(1),
            exp: 10000000000,
            ns: None,
            agent: None,
            session: None,
            channel: None,
            origins: Vec::new(),
            grants,
        })
    }

    fn auth_metadata(token: &str) -> MetadataMap {
        let mut metadata = MetadataMap::new();
        metadata.insert(
            "authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );
        metadata
    }

    #[test]
    fn rate_limit_key_uses_claim_subject_from_request_extensions() {
        fn request_with_subject(subject: &str) -> tonic::Request<()> {
            let mut request = tonic::Request::new(());
            request.extensions_mut().insert(Claims {
                iss: None,
                sub: subject.to_string(),
                aud: "talon".to_string(),
                iat: None,
                exp: 10000000000,
                ns: None,
                agent: None,
                session: None,
                channel: None,
                origins: Vec::new(),
                grants: Vec::new(),
            });
            request
        }

        let request = request_with_subject("oidc:user123:browser");
        let key = rate_limit_key_from_request(&request).expect("subject rate limit key");
        assert!(key.starts_with("sub:"));
        assert_eq!(key.len(), "sub:".len() + 64);

        let padded = request_with_subject("  oidc:user123:browser  ");
        assert_eq!(rate_limit_key_from_request(&padded), Some(key.clone()));

        let different_subject = request_with_subject("oidc:user123:other-client");
        assert_ne!(rate_limit_key_from_request(&different_subject), Some(key));

        let blank_subject = request_with_subject("   ");
        assert!(rate_limit_key_from_request(&blank_subject).is_none());
    }

    #[test]
    fn test_grant_read_allows_reads_but_denies_writes() {
        let _guard = PlatformJwtEnvGuard::acquire_blocking();
        let token = create_grant_token(vec![TalonGrantClaim {
            kind: "read".to_string(),
            namespace: Some("ops".to_string()),
            agent: None,
            session: None,
            channel: None,
        }]);
        let config = jwt_auth_config();
        let metadata = auth_metadata(&token);

        assert!(check_auth_for_operation(
            &metadata,
            &config,
            AuthzOperation::Read,
            "ops",
            Some("triage"),
            None
        )
        .is_ok());
        assert!(check_auth_for_operation(
            &metadata,
            &config,
            AuthzOperation::ReadWrite,
            "ops",
            Some("triage"),
            None
        )
        .is_err());
        assert!(check_auth_for_operation(
            &metadata,
            &config,
            AuthzOperation::Read,
            "ops:tenant-1",
            Some("triage"),
            None
        )
        .is_ok());
        assert!(check_auth_for_operation(
            &metadata,
            &config,
            AuthzOperation::Read,
            "ops-prod",
            Some("triage"),
            None
        )
        .is_err());
        assert!(check_auth_for_operation(
            &metadata,
            &config,
            AuthzOperation::Read,
            "other",
            Some("triage"),
            None
        )
        .is_err());
    }

    #[test]
    fn test_oidc_token_without_grants_is_denied() {
        let _guard = PlatformJwtEnvGuard::acquire_blocking();
        let token = create_grant_token(Vec::new());
        let config = jwt_auth_config();
        let metadata = auth_metadata(&token);

        assert!(check_auth_for_operation(
            &metadata,
            &config,
            AuthzOperation::Read,
            "ops",
            None,
            None
        )
        .is_err());
    }

    #[test]
    fn test_grant_readwrite_matches_session_scope() {
        let _guard = PlatformJwtEnvGuard::acquire_blocking();
        let token = create_grant_token(vec![TalonGrantClaim {
            kind: "readwrite".to_string(),
            namespace: Some("ops".to_string()),
            agent: Some("triage".to_string()),
            session: Some("session-1".to_string()),
            channel: None,
        }]);
        let config = jwt_auth_config();
        let metadata = auth_metadata(&token);

        assert!(check_auth_for_operation(
            &metadata,
            &config,
            AuthzOperation::ReadWrite,
            "ops",
            Some("triage"),
            Some("session-1")
        )
        .is_ok());
        assert!(check_auth_for_operation(
            &metadata,
            &config,
            AuthzOperation::ReadWrite,
            "ops",
            Some("triage"),
            Some("session-2")
        )
        .is_err());
    }

    #[test]
    fn test_channel_scoped_jwt_only_authorizes_matching_channel_operations() {
        let _guard = PlatformJwtEnvGuard::acquire_blocking();
        let config = jwt_auth_config();
        let mut metadata = MetadataMap::new();
        let token = create_channel_token(Some("ops"), Some("incident-room"));
        metadata.insert(
            "authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );

        assert!(check_channel_auth(&metadata, &config, "ops", "incident-room").is_ok());
        metadata.insert(
            "authorization",
            format!("bearer {}", token).parse().unwrap(),
        );
        assert!(check_channel_auth(&metadata, &config, "ops", "incident-room").is_ok());
        assert!(check_channel_auth(&metadata, &config, "ops", "other-room").is_err());
        assert!(check_channel_auth(&metadata, &config, "other", "incident-room").is_err());
        assert!(check_auth(&metadata, &config, "ops", None, None).is_err());
    }

    #[test]
    fn test_channel_scoped_jwt_requires_namespace() {
        let _guard = PlatformJwtEnvGuard::acquire_blocking();
        let config = jwt_auth_config();
        let mut metadata = MetadataMap::new();
        let token = create_channel_token(None, Some("incident-room"));
        metadata.insert(
            "authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );

        assert!(check_channel_auth(&metadata, &config, "ops", "incident-room").is_err());
    }

    #[test]
    fn test_talon_auth_interceptor_covers_platform_jwt_mode() {
        let _guard = PlatformJwtEnvGuard::acquire_blocking();
        let token = create_token(Some("ns"), Some("agent"), Some("session"));
        let mut jwt_interceptor = TalonAuthInterceptor {
            config: jwt_auth_config(),
        };
        let mut jwt_request = tonic::Request::new(());
        jwt_request
            .metadata_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
        let jwt_request = jwt_interceptor.call(jwt_request).unwrap();
        let claims = jwt_request.extensions().get::<Claims>().unwrap();
        assert_eq!(claims.ns.as_deref(), Some("ns"));
        assert_eq!(claims.agent.as_deref(), Some("agent"));
        assert_eq!(claims.session.as_deref(), Some("session"));

        let oidc_claims = Claims {
            iss: Some(TEST_PLATFORM_JWT_ISSUER.to_string()),
            sub: "oidc:user123".to_string(),
            aud: platform_jwt::TALON_GATEWAY_AUDIENCE.to_string(),
            iat: Some(1),
            exp: 10000000000,
            ns: None,
            agent: None,
            session: None,
            channel: None,
            origins: Vec::new(),
            grants: vec![TalonGrantClaim {
                kind: "readwrite".to_string(),
                namespace: Some("ns".to_string()),
                agent: None,
                session: None,
                channel: None,
            }],
        };
        let oidc_token = sign_platform_claims(&oidc_claims);
        let mut oidc_request = tonic::Request::new(());
        oidc_request.metadata_mut().insert(
            "authorization",
            format!("Bearer {oidc_token}").parse().unwrap(),
        );
        let oidc_request = jwt_interceptor.call(oidc_request).unwrap();
        let key = rate_limit_key_from_request(&oidc_request).expect("subject rate limit key");
        assert!(key.starts_with("sub:"));
        assert_eq!(key.len(), "sub:".len() + 64);

        let mut invalid_jwt = TalonAuthInterceptor {
            config: jwt_auth_config(),
        };
        let mut invalid_jwt_request = tonic::Request::new(());
        invalid_jwt_request
            .metadata_mut()
            .insert("authorization", "Bearer not-a-token".parse().unwrap());
        assert!(invalid_jwt.call(invalid_jwt_request).is_err());
    }

    #[tokio::test]
    async fn test_auth_layer_enforces_platform_jwt_mode() {
        let _guard = PlatformJwtEnvGuard::acquire().await;
        async fn ok_handler() -> &'static str {
            "ok"
        }

        let jwt_gateway = gateway_with_auth(Some(jwt_auth_config()));
        let jwt_app = Router::new()
            .route("/", get(ok_handler))
            .layer(from_fn_with_state(jwt_gateway.clone(), auth_layer))
            .with_state(jwt_gateway);
        let token = create_token(Some("ns"), None, None);
        let jwt_res = jwt_app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(jwt_res.status(), HttpStatusCode::OK);

        let open_gateway = gateway_with_auth(Some(AuthConfig::open()));
        let open_app = Router::new()
            .route("/", get(ok_handler))
            .layer(from_fn_with_state(open_gateway.clone(), auth_layer))
            .with_state(open_gateway);
        let open_res = open_app
            .oneshot(HttpRequest::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(open_res.status(), HttpStatusCode::OK);

        let none_gateway = gateway_with_auth(None);
        let none_app = Router::new()
            .route("/", get(ok_handler))
            .layer(from_fn_with_state(none_gateway.clone(), auth_layer))
            .with_state(none_gateway);
        let none_res = none_app
            .oneshot(HttpRequest::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(none_res.status(), HttpStatusCode::OK);

        let jwt_bad = jwt_app
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header(header::AUTHORIZATION, "Bearer invalid")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(jwt_bad.status(), HttpStatusCode::UNAUTHORIZED);
    }
}
