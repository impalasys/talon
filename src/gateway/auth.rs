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
}

pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, Status> {
    crate::security::install_jwt_crypto_provider();
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

pub fn check_auth(
    metadata: &MetadataMap,
    auth_config: &AuthConfig,
    ns: &str,
    agent: Option<&str>,
    session: Option<&str>,
) -> Result<(), Status> {
    match auth_config.mode {
        AuthMode::Open => Ok(()),
        AuthMode::Token => {
            let auth_header = metadata
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| Status::unauthenticated("Missing authorization header"))?;

            let token = auth_header
                .strip_prefix("Bearer ")
                .ok_or_else(|| Status::unauthenticated("Missing bearer token"))?;

            if auth_config.tokens.iter().any(|t| t == token) {
                Ok(())
            } else {
                Err(Status::unauthenticated("Invalid token"))
            }
        }
        AuthMode::Jwt => {
            let auth_header = metadata
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| Status::unauthenticated("Missing authorization header"))?;

            if check_basic_password(auth_header, auth_config.jwt_secret.as_ref())? {
                return Ok(());
            }

            let token = auth_header
                .strip_prefix("Bearer ")
                .ok_or_else(|| Status::unauthenticated("Missing bearer token"))?;

            let secret = auth_config
                .jwt_secret
                .as_ref()
                .ok_or_else(|| Status::internal("JWT secret not configured"))?;
            let claims = verify_jwt(token, secret)?;

            // Hierarchical Scope Validation
            if let Some(allowed_ns) = &claims.ns {
                if allowed_ns != ns {
                    return Err(Status::permission_denied(format!(
                        "Token scope restricted to namespace: {}",
                        allowed_ns
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
        AuthMode::Password => {
            let auth_header = metadata
                .get("authorization")
                .and_then(|v| v.to_str().ok())
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
                let token = auth_header
                    .strip_prefix("Bearer ")
                    .ok_or_else(|| Status::unauthenticated("Missing bearer token"))?;
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

                let token = auth_header
                    .strip_prefix("Bearer ")
                    .ok_or_else(|| Status::unauthenticated("Missing bearer token"))?;
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
                    if auth_str.starts_with("Bearer ") {
                        let token = &auth_str[7..];
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
                    if auth_str.starts_with("Bearer ") {
                        let token = &auth_str[7..];
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
    use jsonwebtoken::{encode, EncodingKey, Header};
    use tonic::metadata::MetadataMap;

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
        crate::security::install_jwt_crypto_provider();
        let claims = Claims {
            sub: "user123".to_string(),
            aud: "talon".to_string(),
            exp: 10000000000, // far future
            ns: ns.map(|s| s.to_string()),
            agent: agent.map(|s| s.to_string()),
            session: session.map(|s| s.to_string()),
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
        // Fail: wrong ns
        assert!(check_auth(&metadata, &config, "other-ns", None, None).is_err());

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
}
