// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::config::proto::JwtIssuerConfig;
use anyhow::{anyhow, Result};
use jsonwebtoken::{
    decode, decode_header,
    jwk::{Jwk, JwkSet, PublicKeyUse, ThumbprintHash},
    Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use serde::{de::DeserializeOwned, Serialize};

pub const TALON_JWT_PRIVATE_KEY_PEM_ENV: &str = "TALON_JWT_PRIVATE_KEY_PEM";
pub const TALON_GATEWAY_AUDIENCE: &str = "talon.impala.systems";
pub const MCP_AUTH_BROKER_AUDIENCE: &str = "mcps.talon.impala.systems";
pub const ACCESS_TOKEN_TYPE: &str = "access";
pub const MCP_AUTH_BROKER_ASSERTION_TOKEN_TYPE: &str = "mcp_auth_broker_assertion";
pub const TALON_OPS_ACCESS_TOKEN_TYPE: &str = "talon_ops_access";
pub const TALON_OPS_AUDIENCE: &str = "talon-ops";

#[cfg(test)]
pub(crate) const TEST_RSA_PRIVATE_KEY: &str = include_str!("test_rsa_private_key.pem");

#[derive(Debug, Clone)]
pub struct PlatformJwtKey {
    encoding_key: EncodingKey,
    jwk: Jwk,
}

impl PlatformJwtKey {
    pub fn from_env() -> Result<Self> {
        let pem = std::env::var(TALON_JWT_PRIVATE_KEY_PEM_ENV)
            .map_err(|_| anyhow!("{TALON_JWT_PRIVATE_KEY_PEM_ENV} is required"))?;
        Self::from_pem(&pem)
    }

    pub fn from_pem(pem: &str) -> Result<Self> {
        crate::control::security::install_jwt_crypto_provider();
        let normalized = normalize_pem(pem);
        let encoding_key = EncodingKey::from_rsa_pem(normalized.as_bytes())
            .map_err(|err| anyhow!("failed to parse RSA private key PEM: {err}"))?;
        let mut jwk = Jwk::from_encoding_key(&encoding_key, Algorithm::RS256)
            .map_err(|err| anyhow!("failed to derive public JWK: {err}"))?;
        let kid = jwk.thumbprint(ThumbprintHash::SHA256);
        jwk.common.key_id = Some(kid);
        jwk.common.public_key_use = Some(PublicKeyUse::Signature);
        Ok(Self { encoding_key, jwk })
    }

    pub fn kid(&self) -> &str {
        self.jwk
            .common
            .key_id
            .as_deref()
            .expect("platform JWK is always assigned a kid")
    }

    pub fn jwks(&self) -> JwkSet {
        JwkSet {
            keys: vec![self.jwk.clone()],
        }
    }

    pub fn sign<T: Serialize>(&self, claims: &T) -> Result<String> {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(self.kid().to_string());
        jsonwebtoken::encode(&header, claims, &self.encoding_key)
            .map_err(|err| anyhow!("failed to sign platform JWT: {err}"))
    }

    pub fn verify<T: DeserializeOwned + Clone>(
        &self,
        token: &str,
        issuer: &str,
        audience: &str,
    ) -> Result<T> {
        let header = decode_header(token).map_err(|err| anyhow!("invalid JWT header: {err}"))?;
        if header.alg != Algorithm::RS256 {
            return Err(anyhow!(
                "unsupported platform JWT algorithm: {:?}",
                header.alg
            ));
        }
        if header.kid.as_deref() != Some(self.kid()) {
            return Err(anyhow!("platform JWT kid does not match configured key"));
        }
        let decoding_key = DecodingKey::from_jwk(&self.jwk)
            .map_err(|err| anyhow!("failed to build platform JWT decoding key: {err}"))?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[audience]);
        validation.set_issuer(&[issuer]);
        decode::<T>(token, &decoding_key, &validation)
            .map(|data| data.claims)
            .map_err(|err| anyhow!("invalid platform JWT: {err}"))
    }
}

pub fn configured_issuer(config: &JwtIssuerConfig) -> Result<&str> {
    let issuer = config.issuer.trim();
    if issuer.is_empty() {
        return Err(anyhow!("platform JWT issuer is not configured"));
    }
    Ok(issuer)
}

pub fn load_key() -> Result<PlatformJwtKey> {
    PlatformJwtKey::from_env()
}

fn normalize_pem(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.contains("\\n") && !trimmed.contains('\n') {
        trimmed.replace("\\n", "\n")
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct TestClaims {
        iss: String,
        sub: String,
        aud: String,
        exp: usize,
        #[serde(rename = "talon:token_type")]
        token_type: String,
    }

    #[test]
    fn signs_and_verifies_with_public_jwk() {
        let key = PlatformJwtKey::from_pem(TEST_RSA_PRIVATE_KEY).unwrap();
        let claims = TestClaims {
            iss: "https://talon.example.com".to_string(),
            sub: "test".to_string(),
            aud: TALON_GATEWAY_AUDIENCE.to_string(),
            exp: 4_102_444_800,
            token_type: ACCESS_TOKEN_TYPE.to_string(),
        };

        let token = key.sign(&claims).unwrap();
        let verified: TestClaims = key
            .verify(&token, "https://talon.example.com", TALON_GATEWAY_AUDIENCE)
            .unwrap();

        assert_eq!(verified, claims);
        assert_eq!(key.jwks().keys.len(), 1);
        let jwks_json = serde_json::to_value(key.jwks()).unwrap();
        assert!(jwks_json["keys"][0].get("d").is_none());
        assert!(jwks_json["keys"][0].get("p").is_none());
        assert_eq!(jwks_json["keys"][0]["kid"], key.kid());
    }

    #[test]
    fn rejects_wrong_audience() {
        let key = PlatformJwtKey::from_pem(TEST_RSA_PRIVATE_KEY).unwrap();
        let claims = TestClaims {
            iss: "https://talon.example.com".to_string(),
            sub: "test".to_string(),
            aud: TALON_GATEWAY_AUDIENCE.to_string(),
            exp: 4_102_444_800,
            token_type: ACCESS_TOKEN_TYPE.to_string(),
        };

        let token = key.sign(&claims).unwrap();
        let err = key
            .verify::<TestClaims>(
                &token,
                "https://talon.example.com",
                MCP_AUTH_BROKER_AUDIENCE,
            )
            .unwrap_err();

        assert!(err.to_string().contains("InvalidAudience"));
    }
}
