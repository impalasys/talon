// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use jsonwebtoken::{
    decode, decode_header,
    jwk::{AlgorithmParameters, Jwk, JwkSet, PublicKeyUse, ThumbprintHash},
    Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::OnceLock;

pub const TALON_JWT_PRIVATE_KEY_PEM_ENV: &str = "TALON_JWT_PRIVATE_KEY_PEM";
pub const TALON_JWT_ISSUER_ENV: &str = "TALON_JWT_ISSUER";
pub const DEFAULT_JWT_ISSUER: &str = "https://talon.impala.systems";
pub const TALON_GATEWAY_AUDIENCE: &str = "talon.impala.systems";
pub const MCP_AUTH_BROKER_AUDIENCE: &str = "mcps.talon.impala.systems";
pub const TALON_OPS_AUDIENCE: &str = "talon-ops";
const MIN_RSA_MODULUS_BITS: usize = 2048;

#[cfg(test)]
pub(crate) const TEST_RSA_PRIVATE_KEY: &str = include_str!("test_rsa_private_key.pem");

#[derive(Debug, Clone)]
pub struct PlatformJwtKey {
    encoding_key: EncodingKey,
    jwk: Jwk,
}

impl PlatformJwtKey {
    pub fn from_env() -> Result<Self> {
        let pem = private_key_pem_from_env()?;
        Self::from_pem(&pem)
    }

    pub fn from_pem(pem: &str) -> Result<Self> {
        crate::control::security::install_jwt_crypto_provider();
        let normalized = normalize_pem(pem);
        let encoding_key = EncodingKey::from_rsa_pem(normalized.as_bytes())
            .map_err(|err| anyhow!("failed to parse RSA private key PEM: {err}"))?;
        let mut jwk = Jwk::from_encoding_key(&encoding_key, Algorithm::RS256)
            .map_err(|err| anyhow!("failed to derive public JWK: {err}"))?;
        validate_rsa_modulus_bits(&jwk)?;
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

pub fn issuer() -> Result<String> {
    let issuer = std::env::var(TALON_JWT_ISSUER_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_JWT_ISSUER.to_string());
    validate_issuer(&issuer)?;
    Ok(issuer)
}

fn validate_issuer(issuer: &str) -> Result<()> {
    if issuer.is_empty() {
        return Err(anyhow!("platform JWT issuer is not configured"));
    }
    let url = url::Url::parse(issuer)
        .map_err(|err| anyhow!("platform JWT issuer must be a valid URL: {err}"))?;
    if url.scheme() != "https" {
        return Err(anyhow!("platform JWT issuer must use https"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(anyhow!(
            "platform JWT issuer must not include query or fragment"
        ));
    }
    Ok(())
}

pub fn load_key() -> Result<PlatformJwtKey> {
    static KEY: OnceLock<PlatformJwtKey> = OnceLock::new();
    private_key_pem_from_env()?;
    if let Some(key) = KEY.get() {
        return Ok(key.clone());
    }
    let key = PlatformJwtKey::from_env()?;
    let _ = KEY.set(key);
    KEY.get()
        .cloned()
        .ok_or_else(|| anyhow!("failed to cache platform JWT key"))
}

fn private_key_pem_from_env() -> Result<String> {
    match std::env::var(TALON_JWT_PRIVATE_KEY_PEM_ENV) {
        Ok(value) if value.trim().is_empty() => {
            Err(anyhow!("{TALON_JWT_PRIVATE_KEY_PEM_ENV} is set but empty"))
        }
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => {
            Err(anyhow!("{TALON_JWT_PRIVATE_KEY_PEM_ENV} is required"))
        }
        Err(std::env::VarError::NotUnicode(_)) => Err(anyhow!(
            "{TALON_JWT_PRIVATE_KEY_PEM_ENV} must be valid UTF-8"
        )),
    }
}

fn normalize_pem(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.contains("\\n") && !trimmed.contains('\n') {
        trimmed.replace("\\n", "\n")
    } else {
        trimmed.to_string()
    }
}

fn validate_rsa_modulus_bits(jwk: &Jwk) -> Result<()> {
    let AlgorithmParameters::RSA(params) = &jwk.algorithm else {
        return Err(anyhow!("platform JWT key must be RSA"));
    };
    let modulus = URL_SAFE_NO_PAD
        .decode(&params.n)
        .map_err(|err| anyhow!("failed to decode RSA modulus: {err}"))?;
    let Some(first_non_zero) = modulus.iter().position(|byte| *byte != 0) else {
        return Err(anyhow!("RSA modulus is empty"));
    };
    let significant = &modulus[first_non_zero..];
    let bits = (significant.len() - 1) * 8 + (8 - significant[0].leading_zeros() as usize);
    if bits < MIN_RSA_MODULUS_BITS {
        return Err(anyhow!(
            "RSA private key modulus is too small: {bits} bits; minimum is {MIN_RSA_MODULUS_BITS} bits"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
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

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct TestClaims {
        iss: String,
        sub: String,
        aud: String,
        exp: usize,
    }

    const WEAK_RSA_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIICdgIBADANBgkqhkiG9w0BAQEFAASCAmAwggJcAgEAAoGBAOdqhKE/XSgjOkVF
2nEUhskQ2e48UoiuOZsWqfl5tkfFMKB6l/7MzRDy5C596bIj3B0VAgs8qdJEH5/U
HkXygiX+K/O2KRN5YsLLnOZai6zGCyM9cPtXXxKRkIfrN/8qUTA4EPFxefG2SMhl
urYl+mBKb3QL0Z/YNFYg25N6DM/FAgMBAAECgYEAnAVDwFcxHnOJwNSUrvKw4PSc
ibNfzcjxC6/fD1TQ4ViALWIaAe7gPxITZ4j6u2DT8UONqjfPIvVNAPwJIQ2cUmqO
XwRhutUlcf44lz0tohccryYrvSQRYBa3/FaC8VAUF3QO7/XT3fNpGuDC3ng5XGTc
iZ4BtaQShng7JiPz4w0CQQD/rlswVyZt6mDty8EPeocOzwE+n6Jx7+ch3pmURGgj
mFj5B7Xc/TDn1oY8Z8HOUXpwln7HVOxvjy+8R4VfOFNTAkEA57Rp4kd/rPFDhrKc
DO9N1uZBWLoYR2Ms8TE7hyGd+RHHV2Ei8LVlE7kER0SaEX9gMSetC10qxjtUFxUX
yp7FhwJAY964Ec7I3QBC8j+3XpNus9MZ2ltCeZzKvIkVljuOLfExN7zSRcrEUpqR
/oBMzIk4+Udfp/69B+p3K+UH7KS0rwJAZeu6V8rToqNN7MZMVnQ9bTZDsF/Lpjs9
3aqmYL6s2o6zfQBBeliQaaiM9Tx7+Q5qpbSqLcGBu0kFqFGi8YH9qQJAKfRiPR1u
ne6yhWhBMCgGWp6RJqX55eZcJJ1VR0ItGMCDDUrMACXcYm2eOoMqWGhIFKzwcsVL
GstIYNA4dYolNw==
-----END PRIVATE KEY-----"#;

    #[test]
    fn signs_and_verifies_with_public_jwk() {
        let key = PlatformJwtKey::from_pem(TEST_RSA_PRIVATE_KEY).unwrap();
        let claims = TestClaims {
            iss: "https://talon.example.com".to_string(),
            sub: "test".to_string(),
            aud: TALON_GATEWAY_AUDIENCE.to_string(),
            exp: 4_102_444_800,
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

    #[test]
    fn rejects_weak_rsa_private_keys() {
        let err = PlatformJwtKey::from_pem(WEAK_RSA_PRIVATE_KEY).unwrap_err();

        assert!(err.to_string().contains("minimum is 2048 bits"));
    }

    #[test]
    fn issuer_defaults_to_platform_url() {
        let _guard = crate::test_support::env_lock();
        let _issuer = EnvGuard::remove(TALON_JWT_ISSUER_ENV);

        assert_eq!(issuer().unwrap(), DEFAULT_JWT_ISSUER);
    }

    #[test]
    fn issuer_honors_env_override() {
        let _guard = crate::test_support::env_lock();
        let _issuer = EnvGuard::set(TALON_JWT_ISSUER_ENV, "https://talon.localhost");

        assert_eq!(issuer().unwrap(), "https://talon.localhost");
    }

    #[test]
    fn issuer_rejects_invalid_env_override() {
        let _guard = crate::test_support::env_lock();
        let _issuer = EnvGuard::set(TALON_JWT_ISSUER_ENV, "http://talon.localhost?x=1");

        let err = issuer().unwrap_err();
        assert!(err.to_string().contains("must use https"));
    }
}
