// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! File configuration and validation for provider-scoped LLM authentication.

use super::proto;
use anyhow::{bail, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SerdeLlmAuth {
    ApiKey {},
    WorkloadIdentity {
        source: SerdeIdentitySource,
        openai: OpenAiMapping,
    },
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SerdeIdentitySource {
    GoogleMetadata { audience: String },
    AwsSts { audience: String, region: String },
    TokenFile { path: String },
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct OpenAiMapping {
    identity_provider_id: String,
    service_account_id: String,
}

impl From<SerdeLlmAuth> for proto::LlmAuthConfig {
    fn from(auth: SerdeLlmAuth) -> Self {
        use proto::{llm_auth_config::Mode, workload_identity_source::Config as Source};
        let mode = match auth {
            SerdeLlmAuth::ApiKey {} => Mode::ApiKey(proto::ApiKeyAuth {}),
            SerdeLlmAuth::WorkloadIdentity { source, openai } => {
                let source = match source {
                    SerdeIdentitySource::GoogleMetadata { audience } => {
                        Source::GoogleMetadata(proto::GoogleMetadataIdentitySource { audience })
                    }
                    SerdeIdentitySource::AwsSts { audience, region } => {
                        Source::AwsSts(proto::AwsStsIdentitySource { audience, region })
                    }
                    SerdeIdentitySource::TokenFile { path } => {
                        Source::TokenFile(proto::TokenFileIdentitySource { path })
                    }
                };
                Mode::WorkloadIdentity(proto::WorkloadIdentityAuth {
                    source: Some(proto::WorkloadIdentitySource {
                        config: Some(source),
                    }),
                    openai: Some(proto::OpenAiFederationConfig {
                        identity_provider_id: openai.identity_provider_id,
                        service_account_id: openai.service_account_id,
                    }),
                })
            }
        };
        Self { mode: Some(mode) }
    }
}

/// Validate without resolving secrets, reading tokens, or contacting a cloud.
/// Also called at runtime for providers constructed directly from protobufs.
pub fn validate_provider_auth(provider: &proto::LlmProviderConfig) -> Result<()> {
    use proto::{
        llm_auth_config::Mode, llm_provider_config::Config,
        workload_identity_source::Config as Source,
    };
    let Some(auth) = &provider.auth else {
        return Ok(());
    };
    let wif = match &auth.mode {
        Some(Mode::ApiKey(_)) => return Ok(()),
        Some(Mode::WorkloadIdentity(wif)) => wif,
        None => bail!("auth must select api_key or workload_identity"),
    };
    let api_key = match &provider.config {
        Some(Config::Openai(config)) => &config.api_key,
        _ => bail!("workload_identity authentication is currently supported only by the native OpenAI provider"),
    };
    if api_key.is_some() {
        bail!("api_key and workload_identity authentication cannot both be configured");
    }
    let mapping = wif
        .openai
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("workload_identity requires an openai mapping"))?;
    required(
        "auth.openai.identity_provider_id",
        &mapping.identity_provider_id,
    )?;
    required(
        "auth.openai.service_account_id",
        &mapping.service_account_id,
    )?;
    match wif
        .source
        .as_ref()
        .and_then(|source| source.config.as_ref())
    {
        Some(Source::GoogleMetadata(source)) => required("auth.source.audience", &source.audience)?,
        Some(Source::AwsSts(source)) => {
            required("auth.source.audience", &source.audience)?;
            required("auth.source.region", &source.region)?;
        }
        Some(Source::TokenFile(source)) => required("auth.source.path", &source.path)?,
        None => bail!("workload_identity requires an identity source"),
    }
    Ok(())
}

fn required(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{name} must not be empty");
    }
    Ok(())
}

pub(super) fn validate_config(config: &proto::TalonConfig) -> Result<()> {
    for (name, provider) in &config.providers {
        validate_provider_auth(provider)
            .map_err(|error| anyhow::anyhow!("provider '{name}': {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::config::{Config, ConfigExt};
    use prost::Message;

    fn yaml(provider: &str, auth: &str) -> String {
        format!("providers:\n  llm:\n    type: {provider}\n    model: demo\n{auth}")
    }

    fn auth(source: &str) -> String {
        format!("    auth:\n      type: workload_identity\n      source:\n{source}\n      openai:\n        identity_provider_id: idp\n        service_account_id: account\n")
    }

    #[test]
    fn identity_sources_load_from_yaml_and_survive_binary_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        for (name, source) in [
            ("google", "        type: google_metadata\n        audience: https://api.openai.com/v1"),
            ("aws", "        type: aws_sts\n        audience: https://api.openai.com/v1\n        region: us-west-2"),
            ("file", "        type: token_file\n        path: ./projected-token"),
        ] {
            let path = dir.path().join(format!("{name}.yaml"));
            std::fs::write(&path, yaml("openai", &auth(source))).unwrap();
            let config = Config::from_file(&path).unwrap();
            let decoded = Config::decode_binary(&config.encode_to_vec()).unwrap();
            assert_eq!(config, decoded);
            let Some(proto::llm_auth_config::Mode::WorkloadIdentity(wif)) = &decoded.providers["llm"].auth.as_ref().unwrap().mode else { panic!("expected WIF") };
            if let Some(proto::workload_identity_source::Config::TokenFile(source)) = wif.source.as_ref().unwrap().config.as_ref() {
                assert_eq!(source.path, dir.path().join("projected-token").to_str().unwrap());
            }
        }
    }

    #[test]
    fn rejects_unsupported_conflicting_missing_empty_and_unknown_auth_settings() {
        let dir = tempfile::tempdir().unwrap();
        let valid = auth("        type: google_metadata\n        audience: audience");
        let cases = [
            (yaml("anthropic", &valid), "supported only by the native OpenAI"),
            (yaml("google", &valid), "supported only by the native OpenAI"),
            (yaml("openai_compatible", &(String::from("    base_url: https://example.com\n") + &valid)), "supported only by the native OpenAI"),
            (yaml("openai", &(String::from("    api_key: key\n") + &valid)), "cannot both be configured"),
            (yaml("openai", &valid.replace("identity_provider_id: idp", "identity_provider_id: ' '")), "identity_provider_id must not be empty"),
            (yaml("openai", &valid.replace("service_account_id: account", "service_account_id: ''")), "service_account_id must not be empty"),
            (yaml("openai", &valid.replace("audience: audience", "audience: ''")), "audience must not be empty"),
            (yaml("openai", &valid.replace("      openai:\n        identity_provider_id: idp\n        service_account_id: account\n", "")), "missing field `openai`"),
            (yaml("openai", "    auth:\n      type: automatic\n"), "unknown variant"),
            (yaml("openai", &valid.replace("type: google_metadata", "type: unknown_cloud")), "unknown variant"),
            (yaml("openai", &valid.replace("        audience: audience", "        audience: audience\n        unexpected: true")), "unknown field"),
            (yaml("openai", &auth("        type: aws_sts\n        audience: audience\n        region: ''")), "region must not be empty"),
            (yaml("openai", &auth("        type: token_file\n        path: ''")), "path must not be empty"),
            (yaml("openai", "    auth: {}"), "missing field `type`"),
            (yaml("openai", "    auth:\n      type: api_key\n      source: ignored\n"), "unknown field"),
        ];
        for (index, (content, expected)) in cases.into_iter().enumerate() {
            let path = dir.path().join(format!("invalid-{index}.yaml"));
            std::fs::write(&path, content).unwrap();
            let error = Config::from_file(&path).unwrap_err().to_string();
            assert!(
                error.contains(expected),
                "case {index}: expected {expected}, got {error}"
            );
        }
    }

    #[test]
    fn inline_auth_is_validated_and_legacy_api_keys_are_preserved() {
        let _lock = crate::test_support::env_lock();
        let content = yaml(
            "openai",
            &auth("        type: aws_sts\n        audience: audience\n        region: us-west-2"),
        );
        let _inline = crate::test_support::EnvVarGuard::set("TALON_CONFIG_INLINE_YAML", &content);
        let config = Config::load_default().unwrap();
        assert!(matches!(
            config.providers["llm"].auth.as_ref().unwrap().mode,
            Some(proto::llm_auth_config::Mode::WorkloadIdentity(_))
        ));
        let _invalid = crate::test_support::EnvVarGuard::set(
            "TALON_CONFIG_INLINE_YAML",
            &content.replace("us-west-2", "''"),
        );
        assert!(Config::load_default()
            .unwrap_err()
            .to_string()
            .contains("region must not be empty"));
        for selection in ["", "    auth:\n      type: api_key\n"] {
            let _api = crate::test_support::EnvVarGuard::set(
                "TALON_CONFIG_INLINE_YAML",
                &yaml(
                    "openai",
                    &(String::from("    api_key: existing-key\n") + selection),
                ),
            );
            let config = Config::load_default().unwrap();
            let Some(proto::llm_provider_config::Config::Openai(openai)) =
                &config.providers["llm"].config
            else {
                panic!("expected OpenAI")
            };
            assert!(openai.api_key.is_some());
        }
    }

    #[test]
    fn binary_configuration_cannot_bypass_auth_validation() {
        let mut config = Config::default();
        config.providers.insert(
            "llm".into(),
            proto::LlmProviderConfig {
                config: Some(proto::llm_provider_config::Config::Openai(
                    proto::OpenAiConfig::default(),
                )),
                auth: Some(proto::LlmAuthConfig::default()),
            },
        );
        assert!(Config::decode_binary(&config.encode_to_vec())
            .unwrap_err()
            .to_string()
            .contains("must select"));
        config
            .providers
            .get_mut("llm")
            .unwrap()
            .auth
            .as_mut()
            .unwrap()
            .mode = Some(proto::llm_auth_config::Mode::WorkloadIdentity(
            proto::WorkloadIdentityAuth::default(),
        ));
        assert!(Config::decode_binary(&config.encode_to_vec())
            .unwrap_err()
            .to_string()
            .contains("requires an openai mapping"));
    }
}
