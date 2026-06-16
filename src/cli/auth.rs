// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use base64::Engine;
use jsonwebtoken::{EncodingKey, Header};
use reqwest::header::AUTHORIZATION;
use serde::{Deserialize, Serialize};
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::{Request, Status};

use crate::gateway::rpc::proto::gateway_service_client::GatewayServiceClient;

use super::commands::Cli;

#[derive(Clone)]
pub(crate) struct AuthInterceptor {
    authorization: Option<MetadataValue<tonic::metadata::Ascii>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CliClaims {
    sub: String,
    aud: String,
    exp: u64,
    #[serde(rename = "talon:ns", skip_serializing_if = "Option::is_none")]
    ns: Option<String>,
    #[serde(rename = "talon:agent", skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    #[serde(rename = "talon:session", skip_serializing_if = "Option::is_none")]
    session: Option<String>,
    #[serde(rename = "talon:channel", skip_serializing_if = "Option::is_none")]
    channel: Option<String>,
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: Request<()>) -> std::result::Result<Request<()>, Status> {
        if let Some(auth) = &self.authorization {
            req.metadata_mut().insert("authorization", auth.clone());
        }
        Ok(req)
    }
}

pub(crate) fn resolve_gateway_password(cli: &Cli) -> Option<String> {
    cli.password
        .clone()
        .or_else(|| std::env::var("TALON_GATEWAY_PASSWORD").ok())
        .or_else(|| std::env::var("GATEWAY_PASSWORD").ok())
}

pub(crate) fn resolve_gateway_token(cli: &Cli) -> Option<String> {
    cli.token
        .clone()
        .or_else(|| std::env::var("TALON_GATEWAY_TOKEN").ok())
        .or_else(|| std::env::var("GATEWAY_TOKEN").ok())
}

pub(crate) fn resolve_gateway_jwt_secret(cli: &Cli) -> Option<String> {
    cli.jwt_secret
        .clone()
        .or_else(|| std::env::var("TALON_JWT_SECRET").ok())
        .or_else(|| std::env::var("GATEWAY_JWT_SECRET").ok())
}

pub(crate) fn mint_gateway_jwt(secret: &str) -> Result<String> {
    mint_root_jwt(secret, "talon-cli", 3600)
}

pub(crate) fn mint_scoped_jwt(
    secret: &str,
    subject: &str,
    ttl_seconds: u64,
    ns: Option<&str>,
    agent: Option<&str>,
    session: Option<&str>,
    channel: Option<&str>,
) -> Result<String> {
    let subject = subject.trim();
    if subject.is_empty() {
        anyhow::bail!("subject cannot be empty");
    }
    if ttl_seconds == 0 {
        anyhow::bail!("ttl-seconds must be greater than zero");
    }
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        + ttl_seconds;
    let claims = CliClaims {
        sub: subject.to_string(),
        aud: "talon".to_string(),
        exp,
        ns: ns.map(str::to_string),
        agent: agent.map(str::to_string),
        session: session.map(str::to_string),
        channel: channel.map(str::to_string),
    };
    jsonwebtoken::encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .context("Failed to sign Talon JWT")
}

pub(crate) fn mint_root_jwt(secret: &str, subject: &str, ttl_seconds: u64) -> Result<String> {
    mint_scoped_jwt(secret, subject, ttl_seconds, None, None, None, None)
        .context("Failed to sign Talon root JWT")
}

pub(crate) fn validate_token_part<'a>(value: &'a str, name: &str) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        anyhow::bail!("{name} cannot be empty");
    }
    Ok(value)
}

pub(crate) fn mint_agent_jwt(
    secret: &str,
    namespace: &str,
    agent: &str,
    subject: &str,
    ttl_seconds: u64,
) -> Result<String> {
    let namespace = validate_token_part(namespace, "namespace")?;
    let agent = validate_token_part(agent, "agent")?;
    mint_scoped_jwt(
        secret,
        subject,
        ttl_seconds,
        Some(namespace),
        Some(agent),
        None,
        None,
    )
    .context("Failed to sign Talon agent JWT")
}

pub(crate) fn mint_session_jwt(
    secret: &str,
    namespace: &str,
    agent: &str,
    session: &str,
    subject: &str,
    ttl_seconds: u64,
) -> Result<String> {
    let namespace = validate_token_part(namespace, "namespace")?;
    let agent = validate_token_part(agent, "agent")?;
    let session = validate_token_part(session, "session")?;
    mint_scoped_jwt(
        secret,
        subject,
        ttl_seconds,
        Some(namespace),
        Some(agent),
        Some(session),
        None,
    )
    .context("Failed to sign Talon session JWT")
}

pub(crate) fn mint_channel_jwt(
    secret: &str,
    namespace: &str,
    channel: &str,
    subject: &str,
    ttl_seconds: u64,
) -> Result<String> {
    let namespace = validate_token_part(namespace, "namespace")?;
    let channel = validate_token_part(channel, "channel")?;
    mint_scoped_jwt(
        secret,
        subject,
        ttl_seconds,
        Some(namespace),
        None,
        None,
        Some(channel),
    )
    .context("Failed to sign Talon channel JWT")
}

pub(crate) fn auth_interceptor(cli: &Cli) -> Result<AuthInterceptor> {
    let authorization = if let Some(token) = resolve_gateway_token(cli) {
        Some(
            MetadataValue::try_from(format!("Bearer {}", token))
                .context("Failed to encode bearer authorization header")?,
        )
    } else if let Some(secret) = resolve_gateway_jwt_secret(cli) {
        let token = mint_gateway_jwt(&secret)?;
        Some(
            MetadataValue::try_from(format!("Bearer {}", token))
                .context("Failed to encode JWT authorization header")?,
        )
    } else {
        resolve_gateway_password(cli)
            .map(|password| {
                let token =
                    base64::engine::general_purpose::STANDARD.encode(format!(":{}", password));
                MetadataValue::try_from(format!("Basic {}", token))
            })
            .transpose()
            .context("Failed to encode basic authorization header")?
    };

    Ok(AuthInterceptor { authorization })
}

pub(crate) async fn connect_gateway(
    cli: &Cli,
) -> Result<
    GatewayServiceClient<
        tonic::service::interceptor::InterceptedService<tonic::transport::Channel, AuthInterceptor>,
    >,
> {
    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(30))
        .connect()
        .await
        .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
    Ok(GatewayServiceClient::with_interceptor(
        channel,
        auth_interceptor(cli)?,
    ))
}

pub(crate) fn resolve_authorization_header(cli: &Cli) -> Result<Option<String>> {
    if let Some(token) = resolve_gateway_token(cli) {
        Ok(Some(format!("Bearer {}", token)))
    } else if let Some(secret) = resolve_gateway_jwt_secret(cli) {
        let token = mint_gateway_jwt(&secret)?;
        Ok(Some(format!("Bearer {}", token)))
    } else if let Some(password) = resolve_gateway_password(cli) {
        let token = base64::engine::general_purpose::STANDARD.encode(format!(":{}", password));
        Ok(Some(format!("Basic {}", token)))
    } else {
        Ok(None)
    }
}

pub(crate) fn rest_client(cli: &Cli) -> Result<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(auth) = resolve_authorization_header(cli)? {
        headers.insert(
            AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&auth)
                .context("Failed to encode REST authorization header")?,
        );
    }
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .context("Failed to build REST client")
}
