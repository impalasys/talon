// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine};
use jsonwebtoken::{EncodingKey, Header};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use talon_client::{GatewayClientOptions, GatewayTransport, TalonGatewayClient};
use url::Url;

use crate::gateway::auth::Claims;

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

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct StoredGatewayAuth {
    pub gateway: String,
    pub access_token: String,
    pub token_type: String,
    pub expires_at: u64,
    pub subject: String,
    pub email: Option<String>,
    pub trust: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct OidcExchangeResponse {
    #[serde(alias = "accessToken")]
    access_token: String,
    #[serde(alias = "tokenType")]
    token_type: String,
    #[serde(alias = "expiresIn")]
    expires_in: u64,
    subject: String,
    email: Option<String>,
    trust: String,
}

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    id_token: String,
}

const DEFAULT_GOOGLE_CLI_CLIENT_ID: Option<&str> = option_env!("TALON_GOOGLE_CLIENT_ID");
// Official release builds can inject the Google Desktop OAuth client secret at
// compile time with TALON_GOOGLE_CLIENT_SECRET. Desktop OAuth clients are
// native-app clients, so this is a public client credential, not an
// authorization boundary. Google still may require it for token exchange; the
// real protection is PKCE, loopback redirect handling, and gateway validation
// of the resulting ID token against trust.oidc. Release packaging may obfuscate
// the value as a speed bump, but obfuscation must not be treated as security.
// See:
// https://stackoverflow.com/questions/78438540/is-google-oauth-for-native-desktop-applications-mean-to-expose-the-client-secret
const DEFAULT_GOOGLE_CLI_CLIENT_SECRET: Option<&str> = option_env!("TALON_GOOGLE_CLIENT_SECRET");

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
        .or_else(|| load_stored_gateway_auth(cli).ok().flatten().map(|auth| auth.access_token))
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

pub(crate) fn gateway_http_base(cli: &Cli) -> String {
    if let Ok(url) = std::env::var("TALON_GATEWAY_HTTP_URL") {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            return trimmed.trim_end_matches('/').to_string();
        }
    }
    if cli.gateway.ends_with(":50051") {
        return format!("{}:50052", cli.gateway.trim_end_matches(":50051"));
    }
    cli.gateway.trim_end_matches('/').to_string()
}

pub(crate) fn stored_auth_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("TALON_AUTH_FILE") {
        if !path.trim().is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    let home = std::env::var("HOME").context("HOME is required to locate Talon auth storage")?;
    Ok(PathBuf::from(home).join(".talon").join("auth.json"))
}

pub(crate) fn load_stored_gateway_auth(cli: &Cli) -> Result<Option<StoredGatewayAuth>> {
    let path = stored_auth_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read auth file {}", path.display()))?;
    let auth: StoredGatewayAuth =
        serde_json::from_str(&text).with_context(|| format!("Invalid auth file {}", path.display()))?;
    if auth.gateway != gateway_http_base(cli) {
        return Ok(None);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    if auth.expires_at <= now.saturating_add(30) {
        return Ok(None);
    }
    Ok(Some(auth))
}

pub(crate) fn save_stored_gateway_auth(auth: &StoredGatewayAuth) -> Result<()> {
    let path = stored_auth_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create auth directory {}", parent.display()))?;
    }
    let content = serde_json::to_vec_pretty(auth)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&path)
            .with_context(|| format!("Failed to write auth file {}", path.display()))?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("Failed to set auth file permissions {}", path.display()))?;
        file.write_all(&content)?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&path, &content)
            .with_context(|| format!("Failed to write auth file {}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn clear_stored_gateway_auth() -> Result<bool> {
    let path = stored_auth_path()?;
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&path)
        .with_context(|| format!("Failed to remove auth file {}", path.display()))?;
    Ok(true)
}

pub(crate) async fn exchange_oidc_id_token(
    cli: &Cli,
    id_token: &str,
    trust: Option<&str>,
    client_type: &str,
) -> Result<StoredGatewayAuth> {
    let base = gateway_http_base(cli);
    let url = format!("{base}/v1/auth/oidc/exchange");
    let mut body = serde_json::json!({
        "idToken": id_token,
        "clientType": client_type,
    });
    if let Some(trust) = trust.filter(|value| !value.trim().is_empty()) {
        body["trust"] = serde_json::Value::String(trust.to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .context("Failed to build OIDC exchange HTTP client")?;
    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("Failed to call OIDC exchange endpoint {}", url))?;
    let status = response.status();
    let text = response.text().await.context("Failed to read OIDC exchange response")?;
    if !status.is_success() {
        anyhow::bail!("OIDC exchange failed: status={} body={}", status, text.trim());
    }
    let exchanged: OidcExchangeResponse =
        serde_json::from_str(&text).context("Failed to parse OIDC exchange response")?;
    let expires_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        + exchanged.expires_in;

    Ok(StoredGatewayAuth {
        gateway: base,
        access_token: exchanged.access_token,
        token_type: exchanged.token_type,
        expires_at,
        subject: exchanged.subject,
        email: exchanged.email,
        trust: exchanged.trust,
    })
}

pub(crate) async fn login_with_google_loopback(
    cli: &Cli,
    google_client_id: Option<&str>,
    google_client_secret: Option<&str>,
    trust: Option<&str>,
) -> Result<StoredGatewayAuth> {
    let client_id = google_client_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| std::env::var("TALON_GOOGLE_CLIENT_ID").ok())
        .or_else(|| std::env::var("TALON_GOOGLE_CLI_CLIENT_ID").ok())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| DEFAULT_GOOGLE_CLI_CLIENT_ID.map(str::to_string))
        .context("TALON_GOOGLE_CLIENT_ID is required when the CLI is built without a default Google client ID")?;
    let client_secret = google_client_secret
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| std::env::var("TALON_GOOGLE_CLIENT_SECRET").ok())
        .or_else(|| std::env::var("TALON_GOOGLE_CLI_CLIENT_SECRET").ok())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            DEFAULT_GOOGLE_CLI_CLIENT_ID
                .filter(|default_client_id| client_id == *default_client_id)
                .and(DEFAULT_GOOGLE_CLI_CLIENT_SECRET)
                .map(str::to_string)
        });

    let listener = TcpListener::bind("127.0.0.1:0").context("Failed to bind loopback listener")?;
    let redirect_uri = format!("http://{}/callback", listener.local_addr()?);
    let state = random_url_token(24);
    let verifier = random_url_token(48);
    let challenge = general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut auth_url = Url::parse("https://accounts.google.com/o/oauth2/v2/auth")?;
    auth_url
        .query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", "openid email profile")
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("access_type", "offline")
        .append_pair("prompt", "select_account");

    println!("Visit this URL to log in:\n{}", auth_url);
    if let Err(err) = open_browser(auth_url.as_str()) {
        eprintln!("Could not open browser automatically: {err}");
    } else {
        println!("Opened browser for Google login.");
    }

    let code = wait_for_loopback_code(listener, &state)?;
    let id_token = exchange_google_code_for_id_token(
        &client_id,
        client_secret.as_deref(),
        &redirect_uri,
        &verifier,
        &code,
    )
    .await?;
    exchange_oidc_id_token(cli, &id_token, trust, "cli").await
}

fn random_url_token(byte_len: usize) -> String {
    let mut bytes = vec![0u8; byte_len];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
    general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "linux")]
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start"]);
        command
    };
    command.arg(url);
    command.spawn().context("Failed to open browser")?;
    Ok(())
}

fn wait_for_loopback_code(listener: TcpListener, expected_state: &str) -> Result<String> {
    let (mut stream, _) = listener.accept().context("Failed to receive OAuth callback")?;
    let mut buffer = [0u8; 4096];
    let size = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..size]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .context("Invalid OAuth callback request")?;
    let url = Url::parse(&format!("http://localhost{path}"))?;
    let params = url.query_pairs().collect::<std::collections::HashMap<_, _>>();
    let state = params.get("state").context("OAuth callback missing state")?;
    if state.as_ref() != expected_state {
        anyhow::bail!("OAuth callback state mismatch");
    }
    let code = params
        .get("code")
        .context("OAuth callback missing code")?
        .to_string();
    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nTalon CLI login complete. You can close this tab.\n";
    stream.write_all(response.as_bytes())?;
    Ok(code)
}

async fn exchange_google_code_for_id_token(
    client_id: &str,
    client_secret: Option<&str>,
    redirect_uri: &str,
    verifier: &str,
    code: &str,
) -> Result<String> {
    let form = google_token_request_form(client_id, client_secret, redirect_uri, verifier, code);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .context("Failed to build Google token HTTP client")?;
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&form)
        .send()
        .await
        .context("Failed to exchange Google OAuth code")?;
    let status = response.status();
    let text = response.text().await.context("Failed to read Google token response")?;
    if !status.is_success() {
        if text.contains("client_secret is missing") {
            anyhow::bail!(
                "Google OAuth code exchange requires this Desktop OAuth client's client_secret. \
Set TALON_GOOGLE_CLIENT_SECRET or TALON_GOOGLE_CLI_CLIENT_SECRET, or pass --google-client-secret. \
Do not use a Google Web OAuth client secret for CLI login. status={} body={}",
                status,
                text.trim()
            );
        }
        anyhow::bail!("Google OAuth code exchange failed: status={} body={}", status, text.trim());
    }
    let token: GoogleTokenResponse =
        serde_json::from_str(&text).context("Failed to parse Google token response")?;
    Ok(token.id_token)
}

fn google_token_request_form<'a>(
    client_id: &'a str,
    client_secret: Option<&'a str>,
    redirect_uri: &'a str,
    verifier: &'a str,
    code: &'a str,
) -> Vec<(&'static str, &'a str)> {
    let mut form = vec![
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
    ];
    if let Some(client_secret) = client_secret.map(str::trim).filter(|value| !value.is_empty()) {
        form.push(("client_secret", client_secret));
    }
    form
}

pub(crate) fn describe_stored_auth(cli: &Cli) -> Result<Option<String>> {
    let Some(auth) = load_stored_gateway_auth(cli)? else {
        return Ok(None);
    };
    let claims = decode_jwt_payload::<Claims>(&auth.access_token).ok();
    let subject = claims
        .as_ref()
        .map(|claims| claims.sub.as_str())
        .unwrap_or(auth.subject.as_str());
    Ok(Some(format!(
        "gateway={} subject={} email={} trust={} expires_at={}",
        auth.gateway,
        subject,
        auth.email.as_deref().unwrap_or("-"),
        auth.trust,
        auth.expires_at
    )))
}

fn decode_jwt_payload<T: for<'de> Deserialize<'de>>(token: &str) -> Result<T> {
    let payload = token
        .split('.')
        .nth(1)
        .context("stored token is not a JWT")?;
    let bytes = general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .context("stored token payload is not base64url")?;
    serde_json::from_slice(&bytes).context("stored token payload is not JSON")
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

#[cfg(test)]
mod tests {
    use super::{google_token_request_form, DEFAULT_GOOGLE_CLI_CLIENT_SECRET};

    #[test]
    fn google_token_request_form_includes_client_secret_when_present() {
        let without_secret = google_token_request_form(
            "client-id",
            None,
            "http://127.0.0.1:1234/callback",
            "verifier",
            "code",
        );
        assert!(!without_secret
            .iter()
            .any(|(key, _)| *key == "client_secret"));

        let with_secret = google_token_request_form(
            "client-id",
            Some(" client-secret "),
            "http://127.0.0.1:1234/callback",
            "verifier",
            "code",
        );
        assert!(with_secret
            .iter()
            .any(|(key, value)| *key == "client_secret" && *value == "client-secret"));
    }

    #[test]
    fn google_cli_client_secret_is_build_time_injected() {
        assert_eq!(
            DEFAULT_GOOGLE_CLI_CLIENT_SECRET,
            option_env!("TALON_GOOGLE_CLIENT_SECRET")
        );
    }
}
