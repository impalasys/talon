// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use talon_client::data::ApiKeyGrant;
use talon_client::v1::{ExchangeApiKeyRequest, ExchangeOidcTokenRequest};
use talon_client::{GatewayClientOptions, GatewayTransport, TalonClient};
use url::Url;

use crate::control::security::platform_jwt;
use crate::gateway::auth::{Claims, TalonGrantClaim};

use super::commands::Cli;

pub(crate) const DEFAULT_TOKEN_TTL: &str = "5min";

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct StoredGatewayAuth {
    pub gateway: String,
    pub access_token: String,
    pub token_type: String,
    pub expires_at: u64,
    pub subject: String,
    pub email: Option<String>,
    pub trust: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_hash: Option<String>,
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

pub(crate) fn resolve_gateway_api_key(cli: &Cli) -> Option<String> {
    cli.api_key
        .clone()
        .or_else(|| std::env::var("TALON_API_KEY").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn parse_api_key_grant(value: &str) -> Result<ApiKeyGrant> {
    let mut parts = value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty());
    let kind = parts
        .next()
        .context("grant must start with read or readwrite")?
        .to_ascii_lowercase();
    if kind != "read" && kind != "readwrite" {
        anyhow::bail!("grant kind must be read or readwrite");
    }
    let mut grant = ApiKeyGrant {
        kind,
        namespace: None,
        agent: None,
        session: None,
        channel: None,
    };
    for part in parts {
        let (key, value) = part
            .split_once('=')
            .with_context(|| format!("grant selector '{part}' must be key=value"))?;
        let value = value.trim();
        if value.is_empty() {
            anyhow::bail!("grant selector '{key}' cannot be empty");
        }
        match key.trim() {
            "namespace" | "ns" => grant.namespace = Some(value.to_string()),
            "agent" => grant.agent = Some(value.to_string()),
            "session" => grant.session = Some(value.to_string()),
            "channel" => grant.channel = Some(value.to_string()),
            other => anyhow::bail!("unsupported grant selector '{other}'"),
        }
    }
    Ok(grant)
}

impl StoredGatewayAuth {
    fn matches_api_key(&self, api_key: &str, grant: Option<&str>) -> bool {
        self.auth_source.as_deref() == Some("api_key")
            && self.credential_hash.as_deref() == Some(api_key_cache_hash(api_key, grant).as_str())
    }
}

pub(crate) fn gateway_http_base(cli: &Cli) -> String {
    if let Ok(url) = std::env::var("TALON_GATEWAY_URL") {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            return trimmed.trim_end_matches('/').to_string();
        }
    }
    gateway_http_base_from_endpoint(&cli.gateway_url())
}

fn gateway_http_base_from_endpoint(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else if trimmed.starts_with("localhost:")
        || trimmed.starts_with("127.")
        || trimmed.starts_with("0.0.0.0:")
        || trimmed.starts_with("[::1]:")
    {
        format!("http://{trimmed}")
    } else {
        format!("https://{trimmed}")
    }
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
    let auth: StoredGatewayAuth = serde_json::from_str(&text)
        .with_context(|| format!("Invalid auth file {}", path.display()))?;
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
    let mut client = TalonClient::connect_with_options(GatewayClientOptions {
        endpoint: base.clone(),
        transport: GatewayTransport::Grpc,
        authorization: None,
        api_key: None,
        connect_timeout: Some(std::time::Duration::from_secs(5)),
        request_timeout: Some(std::time::Duration::from_secs(20)),
    })
    .await
    .map_err(|err| anyhow::anyhow!("{err}"))
    .context("Failed to connect to Talon AuthService")?;
    let exchanged = client
        .exchange_oidc_token(ExchangeOidcTokenRequest {
            id_token: id_token.to_string(),
            trust: trust
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            client_type: Some(client_type.to_string()),
        })
        .await
        .context("OIDC exchange failed")?
        .into_inner();
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
        auth_source: Some("oidc".to_string()),
        credential_hash: None,
    })
}

pub(crate) async fn exchange_api_key(cli: &Cli, api_key: &str) -> Result<StoredGatewayAuth> {
    let base = gateway_http_base(cli);
    let requested_grant = cli
        .api_key_grant
        .as_deref()
        .map(parse_api_key_grant)
        .transpose()?;
    let mut client = TalonClient::connect_with_options(GatewayClientOptions {
        endpoint: base.clone(),
        transport: GatewayTransport::Grpc,
        authorization: None,
        api_key: None,
        connect_timeout: Some(std::time::Duration::from_secs(5)),
        request_timeout: Some(std::time::Duration::from_secs(20)),
    })
    .await
    .map_err(|err| anyhow::anyhow!("{err}"))
    .context("Failed to connect to Talon AuthService")?;
    let exchanged = client
        .exchange_api_key(ExchangeApiKeyRequest {
            api_key: api_key.to_string(),
            grant: requested_grant,
            expires_in: 0,
        })
        .await
        .context("API key exchange failed")?
        .into_inner();
    let expires_at = exchanged.expires_at;
    let claims = decode_jwt_payload::<Claims>(&exchanged.access_token).ok();
    Ok(StoredGatewayAuth {
        gateway: base,
        access_token: exchanged.access_token,
        token_type: exchanged.token_type,
        expires_at,
        subject: claims
            .as_ref()
            .map(|claims| claims.sub.clone())
            .unwrap_or_else(|| "api_key".to_string()),
        email: None,
        trust: "api-key".to_string(),
        auth_source: Some("api_key".to_string()),
        credential_hash: Some(api_key_cache_hash(api_key, cli.api_key_grant.as_deref())),
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
    let (mut stream, _) = listener
        .accept()
        .context("Failed to receive OAuth callback")?;
    let mut buffer = [0u8; 4096];
    let size = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..size]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .context("Invalid OAuth callback request")?;
    let url = Url::parse(&format!("http://localhost{path}"))?;
    let params = url
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    let state = params
        .get("state")
        .context("OAuth callback missing state")?;
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
    let text = response
        .text()
        .await
        .context("Failed to read Google token response")?;
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
        anyhow::bail!(
            "Google OAuth code exchange failed: status={} body={}",
            status,
            text.trim()
        );
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
    if let Some(client_secret) = client_secret
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
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

pub(crate) struct LocalPlatformTokenScope<'a> {
    pub(crate) namespace: Option<&'a str>,
    pub(crate) agent: Option<&'a str>,
    pub(crate) session: Option<&'a str>,
    pub(crate) channel: Option<&'a str>,
}

pub(crate) fn mint_local_platform_access_jwt(
    private_key_pem: &str,
    subject: &str,
    ttl_seconds: u64,
    scope: LocalPlatformTokenScope<'_>,
    origins: &[String],
    grants: &[ApiKeyGrant],
) -> Result<String> {
    let issuer = platform_jwt::issuer()?;
    let subject = validate_token_part(subject, "subject")?;
    let namespace = validate_optional_token_part(scope.namespace, "namespace")?;
    let agent = validate_optional_token_part(scope.agent, "agent")?;
    let session = validate_optional_token_part(scope.session, "session")?;
    let channel = validate_optional_token_part(scope.channel, "channel")?;
    if agent.is_some() && namespace.is_none() {
        anyhow::bail!("agent scope requires namespace scope");
    }
    if session.is_some() && (namespace.is_none() || agent.is_none()) {
        anyhow::bail!("session scope requires namespace and agent scope");
    }
    if channel.is_some() && namespace.is_none() {
        anyhow::bail!("channel scope requires namespace scope");
    }
    if ttl_seconds == 0 {
        anyhow::bail!("ttl-seconds must be greater than zero");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let exp = now.checked_add(ttl_seconds).context("ttl is too large")?;
    let claims = Claims {
        iss: Some(issuer),
        sub: subject.to_string(),
        aud: platform_jwt::TALON_GATEWAY_AUDIENCE.to_string(),
        iat: Some(now as usize),
        exp: exp as usize,
        ns: namespace.map(str::to_string),
        agent: agent.map(str::to_string),
        session: session.map(str::to_string),
        channel: channel.map(str::to_string),
        origins: validate_origins(origins)?,
        grants: grants
            .iter()
            .map(validate_local_platform_grant)
            .collect::<Result<Vec<_>>>()?,
    };
    platform_jwt::PlatformJwtKey::from_pem(private_key_pem)?
        .sign(&claims)
        .context("Failed to sign Talon platform access JWT")
}

fn validate_local_platform_grant(grant: &ApiKeyGrant) -> Result<TalonGrantClaim> {
    let kind = validate_token_part(&grant.kind, "grant kind")?;
    if kind != "read" && kind != "readwrite" {
        anyhow::bail!("grant kind must be read or readwrite");
    }
    let namespace = validate_optional_token_part(grant.namespace.as_deref(), "grant namespace")?;
    let agent = validate_optional_token_part(grant.agent.as_deref(), "grant agent")?;
    let session = validate_optional_token_part(grant.session.as_deref(), "grant session")?;
    let channel = validate_optional_token_part(grant.channel.as_deref(), "grant channel")?;
    if agent.is_some() && namespace.is_none() {
        anyhow::bail!("grant agent scope requires namespace scope");
    }
    if session.is_some() && (namespace.is_none() || agent.is_none()) {
        anyhow::bail!("grant session scope requires namespace and agent scope");
    }
    if channel.is_some() && namespace.is_none() {
        anyhow::bail!("grant channel scope requires namespace scope");
    }
    Ok(TalonGrantClaim {
        kind: kind.to_string(),
        namespace: namespace.map(str::to_string),
        agent: agent.map(str::to_string),
        session: session.map(str::to_string),
        channel: channel.map(str::to_string),
    })
}

pub(crate) fn parse_ttl_seconds(value: &str) -> Result<u64> {
    let value = value.trim();
    if value.is_empty() {
        anyhow::bail!("ttl cannot be empty");
    }

    let split_at = value
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(value.len());
    let (amount, unit) = value.split_at(split_at);
    if amount.is_empty() {
        anyhow::bail!("ttl must start with a number");
    }
    let amount = amount
        .parse::<u64>()
        .context("ttl amount must be a number")?;
    if amount == 0 {
        anyhow::bail!("ttl must be greater than zero");
    }

    let multiplier = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => 1,
        "m" | "min" | "mins" | "minute" | "minutes" => 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => 60 * 60,
        "d" | "day" | "days" => 24 * 60 * 60,
        "w" | "wk" | "wks" | "week" | "weeks" => 7 * 24 * 60 * 60,
        "mo" | "mos" | "month" | "months" => 30 * 24 * 60 * 60,
        "y" | "yr" | "yrs" | "year" | "years" => 365 * 24 * 60 * 60,
        _ => anyhow::bail!(
            "unsupported ttl unit '{}'; use s, min, h, d, wk, mo, or yr",
            unit
        ),
    };
    amount.checked_mul(multiplier).context("ttl is too large")
}

pub(crate) fn resolve_token_ttl_seconds(ttl: &str, ttl_seconds: Option<u64>) -> Result<u64> {
    if let Some(ttl_seconds) = ttl_seconds {
        if ttl_seconds == 0 {
            anyhow::bail!("ttl-seconds must be greater than zero");
        }
        if ttl.trim() != DEFAULT_TOKEN_TTL {
            anyhow::bail!("use either --ttl or --ttl-seconds, not both");
        }
        return Ok(ttl_seconds);
    }
    parse_ttl_seconds(ttl)
}

pub(crate) fn validate_token_part<'a>(value: &'a str, name: &str) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        anyhow::bail!("{name} cannot be empty");
    }
    Ok(value)
}

fn validate_optional_token_part<'a>(value: Option<&'a str>, name: &str) -> Result<Option<&'a str>> {
    value
        .map(|value| validate_token_part(value, name))
        .transpose()
}

fn validate_origins(origins: &[String]) -> Result<Vec<String>> {
    origins
        .iter()
        .map(|origin| {
            let parsed = Url::parse(origin.trim())
                .with_context(|| format!("invalid origin '{}'", origin.trim()))?;
            if !matches!(parsed.scheme(), "http" | "https") {
                anyhow::bail!("origin '{}' must use http or https", origin.trim());
            }
            if !parsed.username().is_empty() || parsed.password().is_some() {
                anyhow::bail!("origin '{}' must not include credentials", origin.trim());
            }
            if parsed.host_str().is_none() {
                anyhow::bail!("origin '{}' must include a host", origin.trim());
            }
            if parsed.path() != "/" || parsed.query().is_some() || parsed.fragment().is_some() {
                anyhow::bail!(
                    "origin '{}' must not include a path, query, or fragment",
                    origin.trim()
                );
            }
            Ok(parsed.origin().ascii_serialization())
        })
        .collect()
}

async fn resolve_authorization_header(cli: &Cli) -> Result<Option<String>> {
    if let Some(token) = cli
        .token
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        Ok(Some(format!("Bearer {}", token)))
    } else if let Some(api_key) = resolve_gateway_api_key(cli) {
        if let Some(auth) = load_stored_gateway_auth(cli)?
            .filter(|auth| auth.matches_api_key(&api_key, cli.api_key_grant.as_deref()))
        {
            return Ok(Some(format!("Bearer {}", auth.access_token)));
        }
        let auth = exchange_api_key(cli, &api_key).await?;
        save_stored_gateway_auth(&auth)?;
        Ok(Some(format!("Bearer {}", auth.access_token)))
    } else if let Some(auth) = load_stored_gateway_auth(cli)? {
        Ok(Some(format!("Bearer {}", auth.access_token)))
    } else {
        Ok(None)
    }
}

fn api_key_cache_hash(api_key: &str, grant: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(api_key.trim().as_bytes());
    hasher.update(b"\0");
    hasher.update(grant.map(str::trim).unwrap_or("").as_bytes());
    general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize())
}

pub(crate) async fn connect_gateway(cli: &Cli) -> Result<TalonClient> {
    let gateway = cli.gateway_url();
    let mut options = GatewayClientOptions::new(gateway.clone());
    options.transport = if cli.grpc_web_enabled() {
        GatewayTransport::GrpcWeb
    } else {
        GatewayTransport::Grpc
    };
    options.authorization = resolve_authorization_header(cli).await?;
    TalonClient::connect_with_options(options)
        .await
        .map_err(|err| anyhow::anyhow!("{err}"))
        .with_context(|| format!("Could not connect to gateway at {gateway}"))
}

#[cfg(test)]
mod tests {
    use super::{
        api_key_cache_hash, gateway_http_base, gateway_http_base_from_endpoint,
        google_token_request_form, mint_local_platform_access_jwt, parse_ttl_seconds,
        resolve_token_ttl_seconds, StoredGatewayAuth, DEFAULT_GOOGLE_CLI_CLIENT_SECRET,
        LocalPlatformTokenScope, DEFAULT_TOKEN_TTL,
    };
    use crate::cli::commands::Cli;
    use crate::control::security::platform_jwt;
    use crate::gateway::auth::Claims;
    use clap::Parser;
    use std::ffi::OsString;
    use talon_client::data::ApiKeyGrant;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(previous) = &self.previous {
                    std::env::set_var(self.key, previous);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

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

    #[test]
    fn gateway_http_base_defaults_hosted_gateways_to_https() {
        assert_eq!(
            gateway_http_base_from_endpoint("talon.impala.systems"),
            "https://talon.impala.systems"
        );
    }

    #[test]
    fn gateway_http_base_keeps_local_gateways_on_http() {
        assert_eq!(
            gateway_http_base_from_endpoint("localhost:50051"),
            "http://localhost:50051"
        );
        assert_eq!(
            gateway_http_base_from_endpoint("127.0.0.1:50051"),
            "http://127.0.0.1:50051"
        );
        assert_eq!(
            gateway_http_base_from_endpoint("[::1]:50051"),
            "http://[::1]:50051"
        );
    }

    #[test]
    fn gateway_http_base_uses_cli_gateway_inference() {
        let _guard = crate::test_support::env_lock();
        std::env::remove_var("TALON_GATEWAY");
        std::env::remove_var("TALON_GATEWAY_URL");
        std::env::remove_var("TALON_GRPC_WEB");
        let cli = Cli::parse_from([
            "talon-cli",
            "--gateway",
            "localhost:50051",
            "auth",
            "whoami",
        ]);

        assert_eq!(gateway_http_base(&cli), "http://localhost:50051");
    }

    #[test]
    fn parse_ttl_seconds_accepts_compact_units() {
        assert_eq!(parse_ttl_seconds("5min").unwrap(), 300);
        assert_eq!(parse_ttl_seconds("1wk").unwrap(), 604800);
        assert_eq!(parse_ttl_seconds("3mo").unwrap(), 7776000);
        assert_eq!(parse_ttl_seconds("1yr").unwrap(), 31536000);
        assert_eq!(parse_ttl_seconds("42").unwrap(), 42);
    }

    #[test]
    fn resolve_token_ttl_seconds_keeps_legacy_seconds_escape_hatch() {
        assert_eq!(
            resolve_token_ttl_seconds(DEFAULT_TOKEN_TTL, None).unwrap(),
            300
        );
        assert_eq!(
            resolve_token_ttl_seconds(DEFAULT_TOKEN_TTL, Some(123)).unwrap(),
            123
        );
        assert!(resolve_token_ttl_seconds("1wk", Some(123)).is_err());
        assert!(parse_ttl_seconds("0min").is_err());
        assert!(parse_ttl_seconds("1fortnight").is_err());
    }

    #[test]
    fn local_platform_access_jwt_sets_gateway_profile_and_scope() {
        let _env_lock = crate::test_support::env_lock();
        let issuer = "https://talon.example.com";
        let _issuer_guard = EnvVarGuard::set(platform_jwt::TALON_JWT_ISSUER_ENV, issuer);
        let token = mint_local_platform_access_jwt(
            platform_jwt::TEST_RSA_PRIVATE_KEY,
            "tenant-client",
            60,
            LocalPlatformTokenScope {
                namespace: Some("customers:acme"),
                agent: None,
                session: None,
                channel: None,
            },
            &[],
            &[],
        )
        .unwrap();
        let claims = platform_jwt::PlatformJwtKey::from_pem(platform_jwt::TEST_RSA_PRIVATE_KEY)
            .unwrap()
            .verify::<Claims>(&token, issuer, platform_jwt::TALON_GATEWAY_AUDIENCE)
            .unwrap();

        assert_eq!(claims.sub, "tenant-client");
        assert_eq!(claims.iss.as_deref(), Some(issuer));
        assert_eq!(claims.aud, platform_jwt::TALON_GATEWAY_AUDIENCE);
        assert_eq!(claims.ns.as_deref(), Some("customers:acme"));
        assert_eq!(claims.agent, None);
        assert_eq!(claims.session, None);
        assert_eq!(claims.channel, None);
    }

    #[test]
    fn local_platform_access_jwt_rejects_empty_or_incomplete_scope() {
        let _env_lock = crate::test_support::env_lock();
        let mint = |scope| {
            mint_local_platform_access_jwt(
                platform_jwt::TEST_RSA_PRIVATE_KEY,
                "tenant-client",
                60,
                scope,
                &[],
                &[],
            )
        };

        assert!(mint(LocalPlatformTokenScope {
            namespace: Some(" "),
            agent: None,
            session: None,
            channel: None,
        })
        .is_err());
        assert!(mint(LocalPlatformTokenScope {
            namespace: None,
            agent: Some("assistant"),
            session: None,
            channel: None,
        })
        .is_err());
        assert!(mint(LocalPlatformTokenScope {
            namespace: Some("customers:acme"),
            agent: None,
            session: Some("session-1"),
            channel: None,
        })
        .is_err());
        assert!(mint(LocalPlatformTokenScope {
            namespace: None,
            agent: None,
            session: None,
            channel: Some("incident-room"),
        })
        .is_err());
    }

    #[test]
    fn local_platform_access_jwt_rejects_invalid_grant_selectors() {
        let _env_lock = crate::test_support::env_lock();
        let invalid_grant = ApiKeyGrant {
            kind: "readwrite".to_string(),
            namespace: None,
            agent: Some("assistant".to_string()),
            session: None,
            channel: None,
        };

        let err = mint_local_platform_access_jwt(
            platform_jwt::TEST_RSA_PRIVATE_KEY,
            "tenant-client",
            60,
            LocalPlatformTokenScope {
                namespace: None,
                agent: None,
                session: None,
                channel: None,
            },
            &[],
            &[invalid_grant],
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("grant agent scope requires namespace scope"));
    }

    #[test]
    fn stored_api_key_auth_matches_only_same_api_key() {
        let auth = StoredGatewayAuth {
            gateway: "http://localhost:50051".to_string(),
            access_token: "token".to_string(),
            token_type: "Bearer".to_string(),
            expires_at: 1,
            subject: "api_key:test".to_string(),
            email: None,
            trust: "api-key".to_string(),
            auth_source: Some("api_key".to_string()),
            credential_hash: Some(api_key_cache_hash("talon_sk_v1_id_secret", None)),
        };

        assert!(auth.matches_api_key("talon_sk_v1_id_secret", None));
        assert!(!auth.matches_api_key("talon_sk_v1_id_secret", Some("read")));
        assert!(!auth.matches_api_key("talon_sk_v1_id_other", None));

        let mut oidc_auth = auth;
        oidc_auth.auth_source = Some("oidc".to_string());
        assert!(!oidc_auth.matches_api_key("talon_sk_v1_id_secret", None));
    }
}
