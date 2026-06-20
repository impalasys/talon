// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use base64::Engine;
use jsonwebtoken::{EncodingKey, Header};
use serde::{Deserialize, Serialize};
use talon_client::{GatewayClientOptions, GatewayTransport, TalonGatewayClient};

use super::commands::Cli;

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

fn resolve_authorization_header(cli: &Cli) -> Result<Option<String>> {
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

fn grpc_web_enabled(cli: &Cli) -> bool {
    cli.grpc_web
        || std::env::var("TALON_GRPC_WEB")
            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false)
}

pub(crate) async fn connect_gateway(cli: &Cli) -> Result<TalonGatewayClient> {
    let mut options = GatewayClientOptions::new(cli.gateway.clone());
    options.transport = if grpc_web_enabled(cli) {
        GatewayTransport::GrpcWeb
    } else {
        GatewayTransport::Grpc
    };
    options.authorization = resolve_authorization_header(cli)?;
    TalonGatewayClient::connect_with_options(options)
        .await
        .map_err(|err| anyhow::anyhow!("Could not connect to gateway at {}: {}", cli.gateway, err))
}
