// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

// Gateway Authentication - token/password auth middleware for the Gateway
// This file is owned by: Agent 4 (Gateway Auth & Tools)

use crate::gateway::server::Gateway;
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose, Engine as _};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tonic::{metadata::MetadataMap, Status};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthzOperation {
    Read,
    ReadWrite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuthMode {
    Open,
    Password,
    Token,
    Jwt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub mode: AuthMode,
    pub password: Option<String>,
    pub tokens: Vec<String>,
    pub jwt_secret: Option<String>,
}

impl AuthConfig {
    pub fn open() -> Self {
        Self {
            mode: AuthMode::Open,
            password: None,
            tokens: Vec::new(),
            jwt_secret: None,
        }
    }

    pub fn password(password: String) -> Self {
        Self {
            mode: AuthMode::Password,
            password: Some(password),
            tokens: Vec::new(),
            jwt_secret: None,
        }
    }

    pub fn tokens(tokens: Vec<String>) -> Self {
        Self {
            mode: AuthMode::Token,
            password: None,
            tokens,
            jwt_secret: None,
        }
    }

    pub fn jwt(secret: String) -> Self {
        Self {
            mode: AuthMode::Jwt,
            password: None,
            tokens: Vec::new(),
            jwt_secret: Some(secret),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub aud: String,
    pub exp: usize,
    #[serde(rename = "talon:ns")]
    pub ns: Option<String>,
    #[serde(rename = "talon:agent")]
    pub agent: Option<String>,
    #[serde(rename = "talon:session")]
    pub session: Option<String>,
    #[serde(rename = "talon:channel")]
    pub channel: Option<String>,
    #[serde(rename = "talon:grants", default)]
    pub grants: Vec<TalonGrantClaim>,
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

pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, Status> {
    crate::control::security::install_jwt_crypto_provider();
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&["talon"]);

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_ref()),
        &validation,
    )
    .map_err(|e| Status::unauthenticated(format!("Invalid token: {}", e)))?;

    Ok(token_data.claims)
}

fn basic_password_from_auth_header(auth_header: &str) -> Result<Option<String>, Status> {
    if !auth_header.starts_with("Basic ") {
        return Ok(None);
    }

    let base64_str = &auth_header[6..];
    let decoded = general_purpose::STANDARD
        .decode(base64_str)
        .map_err(|_| Status::unauthenticated("Invalid base64"))?;
    let decoded_str =
        String::from_utf8(decoded).map_err(|_| Status::unauthenticated("Invalid utf8"))?;

    Ok(decoded_str
        .split_once(':')
        .map(|(_, pass)| pass.to_string()))
}

fn check_basic_password(auth_header: &str, expected_pass: Option<&String>) -> Result<bool, Status> {
    let Some(expected_pass) = expected_pass else {
        return Ok(false);
    };
    Ok(matches!(
        basic_password_from_auth_header(auth_header)?,
        Some(pass) if pass == *expected_pass
    ))
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
    check_auth_header_for_operation(auth_header, auth_config, operation, ns, agent, session)
}

/// Verify and return bearer JWT claims for callers that need to derive an
/// effective read scope from the token after normal authorization succeeds.
///
/// This helper is intentionally not an authorization check by itself. Use
/// `check_auth_for_operation` or `check_channel_auth_for_operation` first, then
/// call this when the caller needs claim details such as `talon:ns`, scoped
/// agent/session/channel values, or `talon:grants`. Search uses this to narrow a
/// broad query to the caller's authorized source filters before querying the
/// `DocumentStore`.
///
/// Returns `Ok(None)` for non-JWT auth modes and for JWT mode's basic-password
/// fallback, because there are no bearer JWT claims to inspect in those cases.
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
    if check_basic_password(auth_header, auth_config.jwt_secret.as_ref())? {
        return Ok(None);
    }
    let token = bearer_token(auth_header)?;
    let secret = auth_config
        .jwt_secret
        .as_ref()
        .ok_or_else(|| Status::internal("JWT secret not configured"))?;
    verify_jwt(token, secret).map(Some)
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
    match auth_config.mode {
        AuthMode::Open => Ok(()),
        AuthMode::Token => {
            let auth_header = auth_header
                .ok_or_else(|| Status::unauthenticated("Missing authorization header"))?;

            let token = bearer_token(auth_header)?;

            if auth_config.tokens.iter().any(|t| t == token) {
                Ok(())
            } else {
                Err(Status::unauthenticated("Invalid token"))
            }
        }
        AuthMode::Jwt => {
            let auth_header = auth_header
                .ok_or_else(|| Status::unauthenticated("Missing authorization header"))?;

            if check_basic_password(auth_header, auth_config.jwt_secret.as_ref())? {
                return Ok(());
            }

            let token = bearer_token(auth_header)?;

            let secret = auth_config
                .jwt_secret
                .as_ref()
                .ok_or_else(|| Status::internal("JWT secret not configured"))?;
            let claims = verify_jwt(token, secret)?;

            check_claim_scope(&claims, operation, ns, agent, session, None)
        }
        AuthMode::Password => {
            let auth_header = auth_header
                .ok_or_else(|| Status::unauthenticated("Missing authorization header"))?;

            if !auth_header.starts_with("Basic ") {
                return Err(Status::unauthenticated(
                    "Invalid auth scheme, expected Basic",
                ));
            }
            if check_basic_password(auth_header, auth_config.password.as_ref())? {
                return Ok(());
            }
            Err(Status::unauthenticated("Invalid password"))
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
    match auth_config.mode {
        AuthMode::Jwt => {
            let auth_header = metadata
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| Status::unauthenticated("Missing authorization header"))?;

            if check_basic_password(auth_header, auth_config.jwt_secret.as_ref())? {
                return Ok(());
            }

            let token = bearer_token(auth_header)?;

            let secret = auth_config
                .jwt_secret
                .as_ref()
                .ok_or_else(|| Status::internal("JWT secret not configured"))?;
            let claims = verify_jwt(token, secret)?;
            check_claim_scope(&claims, operation, ns, None, None, Some(channel))
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
) -> Result<(), Status> {
    if claims.grants.is_empty() && claims.sub.starts_with("oidc:") {
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
            AuthMode::Password => {
                if !auth_header.starts_with("Basic ") {
                    return Err(Status::unauthenticated(
                        "Invalid auth scheme, expected Basic",
                    ));
                }
                if check_basic_password(auth_header, self.config.password.as_ref())? {
                    return Ok(request);
                }
                Err(Status::unauthenticated("Invalid password"))
            }
            AuthMode::Token => {
                let token = bearer_token(auth_header)?;
                if self.config.tokens.iter().any(|t| t == token) {
                    Ok(request)
                } else {
                    Err(Status::unauthenticated("Invalid static token"))
                }
            }
            AuthMode::Jwt => {
                if check_basic_password(auth_header, self.config.jwt_secret.as_ref())? {
                    return Ok(request);
                }

                let token = bearer_token(auth_header)?;
                let secret = self
                    .config
                    .jwt_secret
                    .as_ref()
                    .ok_or_else(|| Status::internal("JWT secret not configured"))?;
                let claims = verify_jwt(token, secret)?;

                // Attach claims to request extensions so handlers can access them
                request.extensions_mut().insert(claims);
                Ok(request)
            }
            _ => Ok(request),
        }
    }
}

// Legacy Axum auth layer (keeping for now, but focus is on tonic)
pub async fn auth_layer(State(state): State<Arc<Gateway>>, req: Request, next: Next) -> Response {
    let auth_config = match &state.auth_config {
        Some(config) => config,
        None => return next.run(req).await,
    };

    match auth_config.mode {
        AuthMode::Open => next.run(req).await,
        AuthMode::Password => {
            if let Some(auth_header) = req.headers().get(header::AUTHORIZATION) {
                if let Ok(auth_str) = auth_header.to_str() {
                    if matches!(
                        check_basic_password(auth_str, auth_config.password.as_ref()),
                        Ok(true)
                    ) {
                        return next.run(req).await;
                    }
                }
            }
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
        AuthMode::Token => {
            if let Some(auth_header) = req.headers().get(header::AUTHORIZATION) {
                if let Ok(auth_str) = auth_header.to_str() {
                    if let Ok(token) = bearer_token(auth_str) {
                        if auth_config.tokens.iter().any(|t| t == token) {
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
        AuthMode::Jwt => {
            if let Some(auth_header) = req.headers().get(header::AUTHORIZATION) {
                if let Ok(auth_str) = auth_header.to_str() {
                    if matches!(
                        check_basic_password(auth_str, auth_config.jwt_secret.as_ref()),
                        Ok(true)
                    ) {
                        return next.run(req).await;
                    }
                    if let Ok(token) = bearer_token(auth_str) {
                        if let Some(secret) = &auth_config.jwt_secret {
                            if verify_jwt(token, secret).is_ok() {
                                return next.run(req).await;
                            }
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
    use crate::test_support::{EmptyPubSub, MockKvStore};
    use axum::{
        body::Body,
        http::{Request as HttpRequest, StatusCode as HttpStatusCode},
        middleware::from_fn_with_state,
        routing::get,
        Router,
    };
    use jsonwebtoken::{encode, EncodingKey, Header};
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

        let pass = AuthConfig::password("test".to_string());
        assert_eq!(pass.mode, AuthMode::Password);
        assert_eq!(pass.password, Some("test".to_string()));

        let token = AuthConfig::tokens(vec!["t1".to_string()]);
        assert_eq!(token.mode, AuthMode::Token);
        assert_eq!(token.tokens, vec!["t1".to_string()]);

        let jwt = AuthConfig::jwt("secret".to_string());
        assert_eq!(jwt.mode, AuthMode::Jwt);
        assert_eq!(jwt.jwt_secret, Some("secret".to_string()));
    }

    fn create_token(
        secret: &str,
        ns: Option<&str>,
        agent: Option<&str>,
        session: Option<&str>,
    ) -> String {
        crate::control::security::install_jwt_crypto_provider();
        let claims = Claims {
            sub: "user123".to_string(),
            aud: "talon".to_string(),
            exp: 10000000000, // far future
            ns: ns.map(|s| s.to_string()),
            agent: agent.map(|s| s.to_string()),
            session: session.map(|s| s.to_string()),
            channel: None,
            grants: Vec::new(),
        };
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_ref()),
        )
        .unwrap()
    }

    #[test]
    fn test_jwt_verification() {
        let secret = "test-secret";
        let token = create_token(secret, Some("ns1"), Some("agent1"), None);

        let claims = verify_jwt(&token, secret).unwrap();
        assert_eq!(claims.ns, Some("ns1".to_string()));
        assert_eq!(claims.agent, Some("agent1".to_string()));
    }

    #[test]
    fn test_check_auth_static_token() {
        let config = AuthConfig::tokens(vec!["valid-token".to_string()]);
        let mut metadata = MetadataMap::new();

        // Fail: Missing
        let res = check_auth(&metadata, &config, "any", None, None);
        assert!(res.is_err());

        // Fail: Invalid
        metadata.insert("authorization", "Bearer invalid".parse().unwrap());
        let res = check_auth(&metadata, &config, "any", None, None);
        assert!(res.is_err());

        // Success
        metadata.insert("authorization", "Bearer valid-token".parse().unwrap());
        let res = check_auth(&metadata, &config, "any", None, None);
        assert!(res.is_ok());
    }

    #[test]
    fn test_check_auth_jwt_scopes() {
        let secret = "test-secret";
        let config = AuthConfig::jwt(secret.to_string());
        let mut metadata = MetadataMap::new();

        // 1. Namespace scope
        let token = create_token(secret, Some("my-ns"), None, None);
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
        let token = create_token(secret, Some("my-ns"), Some("agent-42"), None);
        metadata.insert(
            "authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );

        // Success: matching agent
        assert!(check_auth(&metadata, &config, "my-ns", Some("agent-42"), None).is_ok());
        // Fail: wrong agent
        assert!(check_auth(&metadata, &config, "my-ns", Some("agent-99"), None).is_err());

        // 3. Session scope
        let token = create_token(secret, Some("my-ns"), Some("agent-42"), Some("sess-1"));
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

    fn create_channel_token(secret: &str, ns: Option<&str>, channel: Option<&str>) -> String {
        crate::control::security::install_jwt_crypto_provider();
        let claims = Claims {
            sub: "channel-client".to_string(),
            aud: "talon".to_string(),
            exp: 10000000000,
            ns: ns.map(|s| s.to_string()),
            agent: None,
            session: None,
            channel: channel.map(|s| s.to_string()),
            grants: Vec::new(),
        };
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_ref()),
        )
        .unwrap()
    }

    fn create_grant_token(secret: &str, grants: Vec<TalonGrantClaim>) -> String {
        crate::control::security::install_jwt_crypto_provider();
        let claims = Claims {
            sub: "oidc:user123".to_string(),
            aud: "talon".to_string(),
            exp: 10000000000,
            ns: None,
            agent: None,
            session: None,
            channel: None,
            grants,
        };
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_ref()),
        )
        .unwrap()
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
    fn test_grant_read_allows_reads_but_denies_writes() {
        let secret = "test-secret";
        let token = create_grant_token(
            secret,
            vec![TalonGrantClaim {
                kind: "read".to_string(),
                namespace: Some("ops".to_string()),
                agent: None,
                session: None,
                channel: None,
            }],
        );
        let config = AuthConfig::jwt(secret.to_string());
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
        let secret = "test-secret";
        let token = create_grant_token(secret, Vec::new());
        let config = AuthConfig::jwt(secret.to_string());
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
        let secret = "test-secret";
        let token = create_grant_token(
            secret,
            vec![TalonGrantClaim {
                kind: "readwrite".to_string(),
                namespace: Some("ops".to_string()),
                agent: Some("triage".to_string()),
                session: Some("session-1".to_string()),
                channel: None,
            }],
        );
        let config = AuthConfig::jwt(secret.to_string());
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
        let secret = "test-secret";
        let config = AuthConfig::jwt(secret.to_string());
        let mut metadata = MetadataMap::new();
        let token = create_channel_token(secret, Some("ops"), Some("incident-room"));
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
        let secret = "test-secret";
        let config = AuthConfig::jwt(secret.to_string());
        let mut metadata = MetadataMap::new();
        let token = create_channel_token(secret, None, Some("incident-room"));
        metadata.insert(
            "authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );

        assert!(check_channel_auth(&metadata, &config, "ops", "incident-room").is_err());
    }

    #[test]
    fn test_basic_password_helpers_cover_invalid_and_matching_inputs() {
        let encoded = general_purpose::STANDARD.encode(":secret");
        let auth_header = format!("Basic {encoded}");
        assert_eq!(
            basic_password_from_auth_header(&auth_header).unwrap(),
            Some("secret".to_string())
        );
        assert_eq!(
            basic_password_from_auth_header("Bearer token").unwrap(),
            None
        );
        assert!(basic_password_from_auth_header("Basic !!!").is_err());

        assert!(check_basic_password(&auth_header, Some(&"secret".to_string())).unwrap());
        assert!(!check_basic_password(&auth_header, Some(&"wrong".to_string())).unwrap());
        assert!(!check_basic_password(&auth_header, None).unwrap());
    }

    #[test]
    fn test_check_auth_password_and_jwt_basic_fallback() {
        let password = "pw".to_string();
        let mut metadata = MetadataMap::new();
        let encoded = general_purpose::STANDARD.encode(":pw");
        metadata.insert("authorization", format!("Basic {encoded}").parse().unwrap());

        assert!(check_auth(
            &metadata,
            &AuthConfig::password(password.clone()),
            "ns",
            None,
            None
        )
        .is_ok());
        assert!(check_auth(&metadata, &AuthConfig::jwt(password), "ns", None, None).is_ok());
    }

    #[test]
    fn test_talon_auth_interceptor_covers_password_token_and_jwt_modes() {
        let mut password_interceptor = TalonAuthInterceptor {
            config: AuthConfig::password("pw".to_string()),
        };
        let mut password_request = tonic::Request::new(());
        let encoded = general_purpose::STANDARD.encode(":pw");
        password_request
            .metadata_mut()
            .insert("authorization", format!("Basic {encoded}").parse().unwrap());
        assert!(password_interceptor.call(password_request).is_ok());

        let mut token_interceptor = TalonAuthInterceptor {
            config: AuthConfig::tokens(vec!["good".to_string()]),
        };
        let mut token_request = tonic::Request::new(());
        token_request
            .metadata_mut()
            .insert("authorization", "bearer good".parse().unwrap());
        assert!(token_interceptor.call(token_request).is_ok());

        let secret = "jwt-secret";
        let token = create_token(secret, Some("ns"), Some("agent"), Some("session"));
        let mut jwt_interceptor = TalonAuthInterceptor {
            config: AuthConfig::jwt(secret.to_string()),
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

        let mut wrong_scheme = TalonAuthInterceptor {
            config: AuthConfig::password("pw".to_string()),
        };
        let mut wrong_scheme_request = tonic::Request::new(());
        wrong_scheme_request
            .metadata_mut()
            .insert("authorization", "Bearer nope".parse().unwrap());
        assert!(wrong_scheme.call(wrong_scheme_request).is_err());

        let mut missing_bearer = TalonAuthInterceptor {
            config: AuthConfig::tokens(vec!["good".to_string()]),
        };
        let mut missing_bearer_request = tonic::Request::new(());
        missing_bearer_request
            .metadata_mut()
            .insert("authorization", "Basic Zm9vOmJhcg==".parse().unwrap());
        assert!(missing_bearer.call(missing_bearer_request).is_err());

        let mut invalid_jwt = TalonAuthInterceptor {
            config: AuthConfig::jwt("jwt-secret".to_string()),
        };
        let mut invalid_jwt_request = tonic::Request::new(());
        invalid_jwt_request
            .metadata_mut()
            .insert("authorization", "Bearer not-a-token".parse().unwrap());
        assert!(invalid_jwt.call(invalid_jwt_request).is_err());
    }

    #[tokio::test]
    async fn test_auth_layer_enforces_password_token_and_jwt_modes() {
        async fn ok_handler() -> &'static str {
            "ok"
        }

        let password_gateway = gateway_with_auth(Some(AuthConfig::password("pw".to_string())));
        let password_app = Router::new()
            .route("/", get(ok_handler))
            .layer(from_fn_with_state(password_gateway.clone(), auth_layer))
            .with_state(password_gateway);

        let unauthorized = password_app
            .clone()
            .oneshot(HttpRequest::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), HttpStatusCode::UNAUTHORIZED);

        let encoded = general_purpose::STANDARD.encode(":pw");
        let authorized = password_app
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header(header::AUTHORIZATION, format!("Basic {encoded}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(authorized.status(), HttpStatusCode::OK);

        let token_gateway = gateway_with_auth(Some(AuthConfig::tokens(vec!["good".to_string()])));
        let token_app = Router::new()
            .route("/", get(ok_handler))
            .layer(from_fn_with_state(token_gateway.clone(), auth_layer))
            .with_state(token_gateway);
        let token_res = token_app
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header(header::AUTHORIZATION, "Bearer good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(token_res.status(), HttpStatusCode::OK);

        let secret = "jwt-secret";
        let jwt_gateway = gateway_with_auth(Some(AuthConfig::jwt(secret.to_string())));
        let jwt_app = Router::new()
            .route("/", get(ok_handler))
            .layer(from_fn_with_state(jwt_gateway.clone(), auth_layer))
            .with_state(jwt_gateway);
        let token = create_token(secret, Some("ns"), None, None);
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
