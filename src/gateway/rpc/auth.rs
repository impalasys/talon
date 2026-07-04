// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{data_proto, proto, GrpcGatewayHandler};
use crate::control::config::proto as config_proto;
use crate::control::security::platform_jwt;
use crate::control::{keys, ns};
use crate::gateway::auth::{
    self as gateway_auth, AuthConfig, AuthMode, AuthzOperation, Claims, TalonGrantClaim,
};
use crate::gateway::Gateway;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use prost::Message;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

const DEFAULT_GOOGLE_JWKS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";
const TALON_ACCESS_TOKEN_TTL_SECONDS: u64 = 900;
const TALON_API_KEY_ACCESS_TOKEN_MAX_TTL_SECONDS: u64 = 3600;
const API_KEY_PREFIX: &str = "talon_sk_v1_";
const API_KEY_SECRET_BYTES: usize = 32;

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

type StoredApiKey = data_proto::ApiKeyRecord;

struct ParsedApiKey<'a> {
    id: &'a str,
    secret: &'a str,
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
        if auth_config.mode != AuthMode::Jwt {
            return Err(tonic::Status::unauthenticated(
                "JWT auth is required to mint Talon access tokens",
            ));
        }

        let parent_claims = gateway_auth::jwt_claims_from_metadata(&metadata, auth_config)?
            .ok_or_else(|| tonic::Status::unauthenticated("Bearer JWT is required"))?;
        ensure_scope_authorized(auth_config, &metadata, &parent_claims, &scope)?;

        let (expires_in, expires_at) = delegated_expiration(&parent_claims, scope.expires_in)?;
        let origins = delegated_origins(&parent_claims, &scope.origins)?;
        let now = unix_seconds()?;
        let claims = Claims {
            iss: Some(platform_issuer()?),
            sub: format!("delegated:{}", parent_claims.sub),
            aud: platform_jwt::TALON_GATEWAY_AUDIENCE.to_string(),
            iat: Some(now as usize),
            exp: expires_at as usize,
            ns: Some(scope.namespace),
            agent: scope.agent,
            session: scope.session,
            channel: scope.channel,
            origins,
            grants: Vec::new(),
        };

        let access_token = mint_platform_access_token(self.gateway.as_ref(), &claims)?;

        Ok(tonic::Response::new(proto::MintAccessTokenResponse {
            access_token,
            token_type: "Bearer".to_string(),
            expires_in,
            expires_at,
        }))
    }

    pub async fn handle_create_api_key(
        &self,
        req: tonic::Request<proto::CreateApiKeyRequest>,
    ) -> Result<tonic::Response<proto::CreateApiKeyResponse>, tonic::Status> {
        ensure_root_jwt(self.gateway.as_ref(), req.metadata())?;
        let request = req.into_inner();
        let name = request.name.trim().to_string();
        if name.is_empty() {
            return Err(tonic::Status::invalid_argument("name is required"));
        }
        let grants = request
            .grants
            .into_iter()
            .map(grant_from_proto)
            .collect::<Result<Vec<_>, _>>()?;
        if grants.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "at least one grant is required",
            ));
        }
        for grant in &grants {
            validate_grant(grant)?;
        }
        if let Some(expires_at) = request.expires_at {
            let now = unix_seconds()?;
            if expires_at <= now {
                return Err(tonic::Status::invalid_argument(
                    "expires_at must be in the future",
                ));
            }
        }

        let id = crate::control::uuid::auth_record_id();
        let secret = random_url_token(API_KEY_SECRET_BYTES);
        let raw_key = format!("{API_KEY_PREFIX}{id}_{secret}");
        let prefix = raw_key
            .chars()
            .take(API_KEY_PREFIX.len() + 8)
            .collect::<String>();
        let now = unix_seconds()?;
        let record = StoredApiKey {
            id: id.clone(),
            name,
            prefix,
            secret_hash: hash_api_key_secret(&secret),
            grants: grants.iter().map(grant_to_data_proto).collect(),
            created_at: now,
            last_used_at: 0,
            expires_at: request.expires_at,
            revoked_at: None,
        };
        write_api_key(self.gateway.as_ref(), &record).await?;

        Ok(tonic::Response::new(proto::CreateApiKeyResponse {
            api_key: Some(api_key_info(&record)),
            secret: raw_key,
        }))
    }

    pub async fn handle_list_api_keys(
        &self,
        req: tonic::Request<proto::ListApiKeysRequest>,
    ) -> Result<tonic::Response<proto::ListApiKeysResponse>, tonic::Status> {
        ensure_root_jwt(self.gateway.as_ref(), req.metadata())?;
        let entries = self
            .gateway
            .kv
            .list_entries(&api_key_list())
            .await
            .map_err(|err| tonic::Status::internal(format!("failed to list API keys: {err}")))?;
        let mut api_keys = Vec::with_capacity(entries.len());
        for (key, bytes) in entries {
            match decode_api_key_record(&bytes) {
                Ok(record) => api_keys.push(api_key_info(&record)),
                Err(err) => {
                    tracing::warn!(
                        api_key_id = %key.name,
                        error = %err,
                        "Skipping invalid API key record while listing API keys"
                    );
                }
            }
        }
        api_keys.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(tonic::Response::new(proto::ListApiKeysResponse {
            api_keys,
        }))
    }

    pub async fn handle_revoke_api_key(
        &self,
        req: tonic::Request<proto::RevokeApiKeyRequest>,
    ) -> Result<tonic::Response<proto::RevokeApiKeyResponse>, tonic::Status> {
        ensure_root_jwt(self.gateway.as_ref(), req.metadata())?;
        let id = req.into_inner().id.trim().to_string();
        if id.is_empty() {
            return Err(tonic::Status::invalid_argument("id is required"));
        }
        let now = unix_seconds()?;
        let Some((record, ())) = update_api_key_record(self.gateway.as_ref(), &id, |mut record| {
            if record.revoked_at.is_none() {
                record.revoked_at = Some(now);
            }
            Ok((record, ()))
        })
        .await?
        else {
            return Err(tonic::Status::not_found("API key not found"));
        };
        Ok(tonic::Response::new(proto::RevokeApiKeyResponse {
            api_key: Some(api_key_info(&record)),
        }))
    }

    pub async fn handle_exchange_api_key(
        &self,
        req: tonic::Request<proto::ExchangeApiKeyRequest>,
    ) -> Result<tonic::Response<proto::ExchangeApiKeyResponse>, tonic::Status> {
        let request = req.into_inner();
        let parsed = parse_api_key(request.api_key.trim())?;
        let now = unix_seconds()?;
        let requested_grant = request.grant;
        let Some((record, effective_grant)) =
            update_api_key_record(self.gateway.as_ref(), parsed.id, |mut record| {
                validate_api_key_exchange_record(&record, parsed.secret, now)?;
                let effective_grant = effective_api_key_grant(&record, requested_grant.clone())?;
                record.last_used_at = now;
                Ok((record, effective_grant))
            })
            .await?
        else {
            return Err(tonic::Status::unauthenticated("Invalid API key"));
        };
        let (expires_in, expires_at) = api_key_access_token_expiration(request.expires_in)?;
        let claims = Claims {
            iss: Some(platform_issuer()?),
            sub: format!("api_key:{}", record.id),
            aud: platform_jwt::TALON_GATEWAY_AUDIENCE.to_string(),
            iat: Some(now as usize),
            exp: expires_at as usize,
            ns: effective_grant.namespace.clone(),
            agent: effective_grant.agent.clone(),
            session: effective_grant.session.clone(),
            channel: effective_grant.channel.clone(),
            origins: Vec::new(),
            grants: vec![effective_grant],
        };
        let access_token = mint_platform_access_token(self.gateway.as_ref(), &claims)?;
        Ok(tonic::Response::new(proto::ExchangeApiKeyResponse {
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

fn platform_issuer() -> Result<String, tonic::Status> {
    platform_jwt::issuer().map_err(|err| {
        tonic::Status::internal(format!("Platform JWT issuer is not configured: {err}"))
    })
}

fn mint_platform_access_token(
    _gateway: &Gateway,
    claims: &Claims,
) -> Result<String, tonic::Status> {
    let issuer = platform_issuer()?;
    if claims.iss.as_deref() != Some(issuer.as_str())
        || claims.aud != platform_jwt::TALON_GATEWAY_AUDIENCE
    {
        return Err(tonic::Status::internal(
            "Talon access token claims do not match the platform token profile",
        ));
    }
    let key = platform_jwt::load_key().map_err(|err| {
        tonic::Status::internal(format!("Platform JWT key is not configured: {err}"))
    })?;
    key.sign(claims)
        .map_err(|err| tonic::Status::internal(format!("failed to mint Talon access token: {err}")))
}

fn ensure_root_jwt(
    gateway: &Gateway,
    metadata: &tonic::metadata::MetadataMap,
) -> Result<Claims, tonic::Status> {
    let auth_config = gateway
        .auth_config
        .as_ref()
        .ok_or_else(|| tonic::Status::unauthenticated("JWT auth is not configured"))?;
    if auth_config.mode != AuthMode::Jwt {
        return Err(tonic::Status::unauthenticated(
            "JWT auth is required to manage API keys",
        ));
    }
    let claims = gateway_auth::jwt_claims_from_metadata(metadata, auth_config)?
        .ok_or_else(|| tonic::Status::unauthenticated("Root bearer JWT is required"))?;
    if claims.ns.as_deref().is_some_and(|value| !value.is_empty())
        || claims
            .agent
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        || claims
            .session
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        || claims
            .channel
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        || !claims.grants.is_empty()
    {
        return Err(tonic::Status::permission_denied(
            "Root JWT is required to manage API keys",
        ));
    }
    Ok(claims)
}

fn api_key_key(id: &str) -> keys::ResourceKey {
    keys::ResourceKey::new(ns::TALON_SYSTEM, &[("Auth", "api-keys")], "ApiKey", id)
}

fn api_key_list() -> keys::ResourceList {
    keys::ResourceParent::root(ns::TALON_SYSTEM)
        .child("Auth", "api-keys")
        .list(Some("ApiKey"))
}

async fn write_api_key(gateway: &Gateway, record: &StoredApiKey) -> Result<(), tonic::Status> {
    let bytes = record.encode_to_vec();
    gateway
        .kv
        .set(&api_key_key(&record.id), &bytes)
        .await
        .map_err(|err| tonic::Status::internal(format!("failed to write API key: {err}")))
}

async fn update_api_key_record<R>(
    gateway: &Gateway,
    id: &str,
    mut update: impl FnMut(StoredApiKey) -> Result<(StoredApiKey, R), tonic::Status>,
) -> Result<Option<(StoredApiKey, R)>, tonic::Status> {
    let key = api_key_key(id);
    loop {
        let Some(current_bytes) = gateway
            .kv
            .get(&key)
            .await
            .map_err(|err| tonic::Status::internal(format!("failed to read API key: {err}")))?
        else {
            return Ok(None);
        };
        let current = decode_api_key_record(&current_bytes)?;
        let (updated, output) = update(current)?;
        let updated_bytes = updated.encode_to_vec();
        if updated_bytes == current_bytes {
            return Ok(Some((updated, output)));
        }
        let swapped = gateway
            .kv
            .compare_and_swap(&key, Some(current_bytes.as_slice()), &updated_bytes)
            .await
            .map_err(|err| tonic::Status::internal(format!("failed to update API key: {err}")))?;
        if swapped {
            return Ok(Some((updated, output)));
        }
    }
}

fn decode_api_key_record(bytes: &[u8]) -> Result<StoredApiKey, tonic::Status> {
    StoredApiKey::decode(bytes)
        .map_err(|err| tonic::Status::internal(format!("failed to decode API key: {err}")))
}

fn api_key_info(record: &StoredApiKey) -> proto::ApiKeyInfo {
    proto::ApiKeyInfo {
        id: record.id.clone(),
        name: record.name.clone(),
        prefix: record.prefix.clone(),
        grants: record.grants.clone(),
        created_at: record.created_at,
        last_used_at: record.last_used_at,
        expires_at: record.expires_at,
        revoked_at: record.revoked_at,
    }
}

fn grant_from_proto(grant: data_proto::ApiKeyGrant) -> Result<TalonGrantClaim, tonic::Status> {
    let grant = TalonGrantClaim {
        kind: grant.kind.trim().to_ascii_lowercase(),
        namespace: trim_optional(grant.namespace),
        agent: trim_optional(grant.agent),
        session: trim_optional(grant.session),
        channel: trim_optional(grant.channel),
    };
    validate_grant(&grant)?;
    Ok(grant)
}

fn grant_to_data_proto(grant: &TalonGrantClaim) -> data_proto::ApiKeyGrant {
    data_proto::ApiKeyGrant {
        kind: grant.kind.clone(),
        namespace: grant.namespace.clone(),
        agent: grant.agent.clone(),
        session: grant.session.clone(),
        channel: grant.channel.clone(),
    }
}

fn grant_from_data_proto(
    grant: &data_proto::ApiKeyGrant,
) -> Result<TalonGrantClaim, tonic::Status> {
    let grant = TalonGrantClaim {
        kind: grant.kind.trim().to_ascii_lowercase(),
        namespace: trim_optional(grant.namespace.clone()),
        agent: trim_optional(grant.agent.clone()),
        session: trim_optional(grant.session.clone()),
        channel: trim_optional(grant.channel.clone()),
    };
    validate_grant(&grant)?;
    Ok(grant)
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn validate_grant(grant: &TalonGrantClaim) -> Result<(), tonic::Status> {
    match grant.kind.as_str() {
        "read" | "readwrite" => {}
        _ => {
            return Err(tonic::Status::invalid_argument(
                "grant kind must be read or readwrite",
            ));
        }
    }
    if grant.session.is_some() && (grant.namespace.is_none() || grant.agent.is_none()) {
        return Err(tonic::Status::invalid_argument(
            "session grant requires namespace and agent",
        ));
    }
    if grant.channel.is_some()
        && (grant.namespace.is_none() || grant.agent.is_some() || grant.session.is_some())
    {
        return Err(tonic::Status::invalid_argument(
            "channel grant requires namespace and cannot combine with agent or session",
        ));
    }
    if (grant.agent.is_some() || grant.channel.is_some()) && grant.namespace.is_none() {
        return Err(tonic::Status::invalid_argument(
            "resource grant requires namespace",
        ));
    }
    Ok(())
}

fn effective_api_key_grant(
    record: &StoredApiKey,
    requested: Option<data_proto::ApiKeyGrant>,
) -> Result<TalonGrantClaim, tonic::Status> {
    if record.grants.is_empty() {
        return Err(tonic::Status::permission_denied("API key has no grants"));
    }
    let Some(requested) = requested else {
        if record.grants.len() == 1 {
            return grant_from_data_proto(&record.grants[0]);
        }
        return Err(tonic::Status::invalid_argument(
            "grant is required when API key has multiple grants",
        ));
    };
    let requested = grant_from_proto(requested)?;
    for allowed in &record.grants {
        let allowed = grant_from_data_proto(allowed)?;
        if grant_allows_requested(&allowed, &requested) {
            return Ok(requested);
        }
    }
    Err(tonic::Status::permission_denied(
        "Requested grant is broader than the API key grant",
    ))
}

fn grant_allows_requested(allowed: &TalonGrantClaim, requested: &TalonGrantClaim) -> bool {
    if !kind_allows_requested(&allowed.kind, &requested.kind) {
        return false;
    }
    if !namespace_allows_requested(allowed.namespace.as_deref(), requested.namespace.as_deref()) {
        return false;
    }
    if !optional_selector_allows(allowed.agent.as_deref(), requested.agent.as_deref()) {
        return false;
    }
    if !optional_selector_allows(allowed.session.as_deref(), requested.session.as_deref()) {
        return false;
    }
    if !optional_selector_allows(allowed.channel.as_deref(), requested.channel.as_deref()) {
        return false;
    }
    true
}

fn kind_allows_requested(allowed: &str, requested: &str) -> bool {
    allowed == "readwrite" || requested == "read"
}

fn namespace_allows_requested(allowed: Option<&str>, requested: Option<&str>) -> bool {
    let Some(allowed) = allowed.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    matches!(requested, Some(requested) if gateway_auth::namespace_scope_allows(allowed, requested))
}

fn optional_selector_allows(allowed: Option<&str>, requested: Option<&str>) -> bool {
    let Some(allowed) = allowed.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    matches!(requested, Some(requested) if requested == allowed)
}

fn validate_api_key_exchange_record(
    record: &StoredApiKey,
    secret: &str,
    now: u64,
) -> Result<(), tonic::Status> {
    if !constant_time_eq(
        hash_api_key_secret(secret).as_bytes(),
        record.secret_hash.as_bytes(),
    ) {
        return Err(tonic::Status::unauthenticated("Invalid API key"));
    }
    if record.revoked_at.is_some() {
        return Err(tonic::Status::unauthenticated("API key is revoked"));
    }
    if record
        .expires_at
        .is_some_and(|expires_at| expires_at <= now)
    {
        return Err(tonic::Status::unauthenticated("API key is expired"));
    }
    Ok(())
}

fn api_key_access_token_expiration(requested_expires_in: u64) -> Result<(u64, u64), tonic::Status> {
    let expires_in = if requested_expires_in == 0 {
        TALON_ACCESS_TOKEN_TTL_SECONDS
    } else {
        requested_expires_in
    };
    if expires_in == 0 {
        return Err(tonic::Status::invalid_argument(
            "expires_in must be positive",
        ));
    }
    if expires_in > TALON_API_KEY_ACCESS_TOKEN_MAX_TTL_SECONDS {
        return Err(tonic::Status::permission_denied(
            "Requested token expiry exceeds API key token maximum",
        ));
    }
    Ok((expires_in, unix_seconds()? + expires_in))
}

fn parse_api_key(value: &str) -> Result<ParsedApiKey<'_>, tonic::Status> {
    let rest = value
        .strip_prefix(API_KEY_PREFIX)
        .ok_or_else(|| tonic::Status::unauthenticated("Invalid API key"))?;
    let (id, secret) = rest
        .split_once('_')
        .ok_or_else(|| tonic::Status::unauthenticated("Invalid API key"))?;
    if id.is_empty() || secret.is_empty() {
        return Err(tonic::Status::unauthenticated("Invalid API key"));
    }
    Ok(ParsedApiKey { id, secret })
}

fn random_url_token(byte_len: usize) -> String {
    let mut bytes = vec![0u8; byte_len];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_api_key_secret(secret: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(secret.as_bytes()))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in left.iter().zip(right.iter()) {
        diff |= left ^ right;
    }
    diff == 0
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
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| tonic::Status::internal(err.to_string()))?
        .as_secs()
        + TALON_ACCESS_TOKEN_TTL_SECONDS;
    let now = unix_seconds()?;

    let claims = Claims {
        iss: Some(platform_issuer()?),
        sub: format!("oidc:{}", identity.claims.sub),
        aud: platform_jwt::TALON_GATEWAY_AUDIENCE.to_string(),
        iat: Some(now as usize),
        exp: exp as usize,
        ns: None,
        agent: None,
        session: None,
        channel: None,
        origins: Vec::new(),
        grants: identity.grants.clone(),
    };

    mint_platform_access_token(gateway, &claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{ControlPlane, KeyValueStore};
    use crate::gateway::auth::check_auth;
    use crate::test_support::{
        EmptyPubSub, MockKvStore, PlatformJwtEnvGuard, TEST_PLATFORM_JWT_ISSUER,
    };
    use std::sync::Arc;

    fn handler() -> GrpcGatewayHandler {
        handler_with_kv(Arc::new(MockKvStore::default()))
    }

    fn handler_with_kv(kv: Arc<MockKvStore>) -> GrpcGatewayHandler {
        let control_plane = ControlPlane::builder(kv, Arc::new(EmptyPubSub)).build();
        let ControlPlane {
            kv,
            pubsub,
            scheduler,
            objects,
            documents,
        } = control_plane;
        GrpcGatewayHandler {
            gateway: Arc::new(Gateway::new_with_trust(
                Some(jwt_auth_config()),
                None,
                kv,
                pubsub,
                scheduler,
                objects,
                documents,
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

    fn auth_request<T>(token: &str, request: T) -> tonic::Request<T> {
        let mut req = tonic::Request::new(request);
        req.metadata_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
        req
    }

    fn token(mut claims: Claims) -> String {
        if claims.exp == 0 {
            claims.exp = (unix_seconds().unwrap() + 3600) as usize;
        }
        claims.iss = Some(TEST_PLATFORM_JWT_ISSUER.to_string());
        claims.aud = platform_jwt::TALON_GATEWAY_AUDIENCE.to_string();
        claims.iat = Some(unix_seconds().unwrap() as usize);
        platform_jwt::PlatformJwtKey::from_pem(platform_jwt::TEST_RSA_PRIVATE_KEY)
            .unwrap()
            .sign(&claims)
            .unwrap()
    }

    fn claims(ns: Option<&str>, agent: Option<&str>, session: Option<&str>) -> Claims {
        Claims {
            iss: None,
            sub: "tenant-admin".to_string(),
            aud: "talon".to_string(),
            iat: None,
            exp: 0,
            ns: ns.map(str::to_string),
            agent: agent.map(str::to_string),
            session: session.map(str::to_string),
            channel: None,
            origins: Vec::new(),
            grants: Vec::new(),
        }
    }

    fn jwt_auth_config() -> AuthConfig {
        AuthConfig::jwt_platform()
    }

    fn verify_platform_access_token(token: &str) -> Claims {
        let key =
            platform_jwt::PlatformJwtKey::from_pem(platform_jwt::TEST_RSA_PRIVATE_KEY).unwrap();
        let claims: Claims = key
            .verify(
                token,
                TEST_PLATFORM_JWT_ISSUER,
                platform_jwt::TALON_GATEWAY_AUDIENCE,
            )
            .unwrap();
        claims
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

    fn proto_grant(
        kind: &str,
        namespace: Option<&str>,
        agent: Option<&str>,
        session: Option<&str>,
        channel: Option<&str>,
    ) -> data_proto::ApiKeyGrant {
        data_proto::ApiKeyGrant {
            kind: kind.to_string(),
            namespace: namespace.map(str::to_string),
            agent: agent.map(str::to_string),
            session: session.map(str::to_string),
            channel: channel.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn mint_access_token_allows_descendant_namespace_and_agent_narrowing() {
        let _env_guard = PlatformJwtEnvGuard::acquire().await;
        let parent = token(claims(Some("Tenant:acme"), None, None));
        let handler = handler();
        let mut request = mint_request("Tenant:acme:child");
        request.agent = Some("assistant".to_string());

        let response = handler
            .handle_mint_access_token(bearer_request(&parent, request))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(response.token_type, "Bearer");
        assert_eq!(response.expires_in, 60);
        let minted = verify_platform_access_token(&response.access_token);
        assert_eq!(minted.sub, "delegated:tenant-admin");
        assert_eq!(minted.ns.as_deref(), Some("Tenant:acme:child"));
        assert_eq!(minted.agent.as_deref(), Some("assistant"));
        assert!(minted.grants.is_empty());

        let config = jwt_auth_config();
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
        let _env_guard = PlatformJwtEnvGuard::acquire().await;
        let parent = token(claims(Some("Tenant:acme"), Some("assistant"), None));
        let handler = handler();

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
        let _env_guard = PlatformJwtEnvGuard::acquire().await;
        let mut parent_claims = claims(Some("Tenant:acme"), None, None);
        parent_claims.exp = (unix_seconds().unwrap() + 30) as usize;
        let parent = token(parent_claims);
        let handler = handler();
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
        let _env_guard = PlatformJwtEnvGuard::acquire().await;
        let mut parent_claims = claims(Some("Tenant:acme"), None, None);
        parent_claims.origins = vec!["https://app.example.com".to_string()];
        let parent = token(parent_claims);
        let handler = handler();

        let mut request = mint_request("Tenant:acme");
        request.origins = vec!["https://APP.example.com:443".to_string()];
        let response = handler
            .handle_mint_access_token(bearer_request(&parent, request))
            .await
            .unwrap()
            .into_inner();
        let minted = verify_platform_access_token(&response.access_token);
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
        let _env_guard = PlatformJwtEnvGuard::acquire().await;
        let handler = handler();
        let mut parent_claims = claims(None, None, None);
        parent_claims.sub = "oidc:user123".to_string();
        parent_claims.grants = vec![TalonGrantClaim {
            kind: "readwrite".to_string(),
            namespace: Some("Tenant:acme".to_string()),
            agent: Some("assistant".to_string()),
            session: None,
            channel: None,
        }];
        let parent = token(parent_claims.clone());

        let mut request = mint_request("Tenant:acme:child");
        request.agent = Some("assistant".to_string());
        handler
            .handle_mint_access_token(bearer_request(&parent, request))
            .await
            .unwrap();

        parent_claims.grants[0].kind = "read".to_string();
        let parent = token(parent_claims);
        let mut request = mint_request("Tenant:acme:child");
        request.agent = Some("assistant".to_string());
        let err = handler
            .handle_mint_access_token(bearer_request(&parent, request))
            .await
            .expect_err("read-only grant must not mint write-capable token");
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn api_key_create_exchange_and_revoke_are_scoped() {
        let _env_guard = PlatformJwtEnvGuard::acquire().await;
        let handler = handler();
        let root = token(claims(None, None, None));

        let created = handler
            .handle_create_api_key(auth_request(
                &root,
                proto::CreateApiKeyRequest {
                    name: "deploy bot".to_string(),
                    grants: vec![proto_grant(
                        "readwrite",
                        Some("Tenant:acme"),
                        None,
                        None,
                        None,
                    )],
                    expires_at: None,
                },
            ))
            .await
            .unwrap()
            .into_inner();
        let info = created.api_key.as_ref().unwrap();
        assert_eq!(info.name, "deploy bot");
        assert_eq!(info.grants.len(), 1);
        assert!(!created.secret.is_empty());

        let exchanged = handler
            .handle_exchange_api_key(tonic::Request::new(proto::ExchangeApiKeyRequest {
                api_key: created.secret.clone(),
                grant: None,
                expires_in: 60,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(exchanged.token_type, "Bearer");
        let minted = verify_platform_access_token(&exchanged.access_token);
        assert_eq!(minted.sub, format!("api_key:{}", info.id));
        assert_eq!(minted.ns.as_deref(), Some("Tenant:acme"));
        assert_eq!(minted.grants.len(), 1);
        assert_eq!(minted.grants[0].kind, "readwrite");

        let config = jwt_auth_config();
        let mut metadata = tonic::metadata::MetadataMap::new();
        metadata.insert(
            "authorization",
            format!("Bearer {}", exchanged.access_token)
                .parse()
                .unwrap(),
        );
        assert!(check_auth(&metadata, &config, "Tenant:acme:child", None, None).is_ok());
        assert!(check_auth(&metadata, &config, "Tenant:other", None, None).is_err());

        handler
            .handle_revoke_api_key(auth_request(
                &root,
                proto::RevokeApiKeyRequest {
                    id: info.id.clone(),
                },
            ))
            .await
            .unwrap();
        let err = handler
            .handle_exchange_api_key(tonic::Request::new(proto::ExchangeApiKeyRequest {
                api_key: created.secret,
                grant: None,
                expires_in: 60,
            }))
            .await
            .expect_err("revoked API key must not exchange");
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn api_key_management_requires_root_jwt() {
        let _env_guard = PlatformJwtEnvGuard::acquire().await;
        let handler = handler();
        let scoped = token(claims(Some("Tenant:acme"), None, None));

        let err = handler
            .handle_create_api_key(auth_request(
                &scoped,
                proto::CreateApiKeyRequest {
                    name: "scoped".to_string(),
                    grants: vec![proto_grant("read", Some("Tenant:acme"), None, None, None)],
                    expires_at: None,
                },
            ))
            .await
            .expect_err("scoped JWT must not create API keys");
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn api_key_list_skips_invalid_records() {
        let _env_guard = PlatformJwtEnvGuard::acquire().await;
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv.clone());
        let root = token(claims(None, None, None));

        handler
            .handle_create_api_key(auth_request(
                &root,
                proto::CreateApiKeyRequest {
                    name: "valid".to_string(),
                    grants: vec![proto_grant("read", Some("Tenant:acme"), None, None, None)],
                    expires_at: None,
                },
            ))
            .await
            .unwrap();
        kv.set(&api_key_key("corrupt"), b"not a protobuf record")
            .await
            .unwrap();

        let response = handler
            .handle_list_api_keys(auth_request(&root, proto::ListApiKeysRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(response.api_keys.len(), 1);
        assert_eq!(response.api_keys[0].name, "valid");
    }

    #[tokio::test]
    async fn api_key_exchange_requires_explicit_grant_for_multiple_grants_and_allows_narrowing() {
        let _env_guard = PlatformJwtEnvGuard::acquire().await;
        let handler = handler();
        let root = token(claims(None, None, None));

        let created = handler
            .handle_create_api_key(auth_request(
                &root,
                proto::CreateApiKeyRequest {
                    name: "multi".to_string(),
                    grants: vec![
                        proto_grant("readwrite", Some("Tenant:acme"), None, None, None),
                        proto_grant("read", Some("Tenant:other"), None, None, None),
                    ],
                    expires_at: None,
                },
            ))
            .await
            .unwrap()
            .into_inner();

        let err = handler
            .handle_exchange_api_key(tonic::Request::new(proto::ExchangeApiKeyRequest {
                api_key: created.secret.clone(),
                grant: None,
                expires_in: 60,
            }))
            .await
            .expect_err("multiple grants require explicit requested grant");
        assert_eq!(err.code(), tonic::Code::InvalidArgument);

        let narrowed = handler
            .handle_exchange_api_key(tonic::Request::new(proto::ExchangeApiKeyRequest {
                api_key: created.secret.clone(),
                grant: Some(proto_grant(
                    "read",
                    Some("Tenant:acme:child"),
                    Some("assistant"),
                    None,
                    None,
                )),
                expires_in: 60,
            }))
            .await
            .unwrap()
            .into_inner();
        let claims = verify_platform_access_token(&narrowed.access_token);
        assert_eq!(claims.grants[0].kind, "read");
        assert_eq!(
            claims.grants[0].namespace.as_deref(),
            Some("Tenant:acme:child")
        );
        assert_eq!(claims.grants[0].agent.as_deref(), Some("assistant"));

        let err = handler
            .handle_exchange_api_key(tonic::Request::new(proto::ExchangeApiKeyRequest {
                api_key: created.secret,
                grant: Some(proto_grant(
                    "readwrite",
                    Some("Tenant:other"),
                    None,
                    None,
                    None,
                )),
                expires_in: 60,
            }))
            .await
            .expect_err("read grant must not mint readwrite token");
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[test]
    fn jwt_decode_accepts_grants_and_legacy_talon_grants() {
        let value = serde_json::json!({
            "sub": "api_key:test",
            "aud": "talon",
            "exp": 10000000000_u64,
            "grants": [
                {"kind": "read", "namespace": "Tenant:acme"}
            ],
            "talon:grants": [
                {"kind": "readwrite", "namespace": "Tenant:ops"}
            ]
        });
        let claims: Claims = serde_json::from_value(value).unwrap();
        assert_eq!(claims.grants.len(), 2);
        assert_eq!(claims.grants[0].kind, "read");
        assert_eq!(claims.grants[1].kind, "readwrite");
    }
}
