// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Validation for OpenAI workload identity. File configuration deserializes
//! directly into generated protobuf messages; there is no separate YAML schema.

use super::proto;
use anyhow::{bail, Result};

/// Validate without reading tokens, resolving secrets, or contacting a cloud.
pub fn validate_openai_auth(config: &proto::OpenAiConfig) -> Result<()> {
    use proto::workload_identity_source::Config as Source;
    let Some(wif) = &config.workload_identity else {
        return Ok(());
    };
    if config.api_key.is_some() {
        bail!("api_key and workload_identity cannot both be configured");
    }
    required(
        "workload_identity.identity_provider_id",
        &wif.identity_provider_id,
    )?;
    required(
        "workload_identity.service_account_id",
        &wif.service_account_id,
    )?;
    match wif
        .source
        .as_ref()
        .and_then(|source| source.config.as_ref())
    {
        Some(Source::GoogleMetadata(source)) => required(
            "workload_identity.source.google_metadata.audience",
            &source.audience,
        )?,
        Some(Source::AwsSts(source)) => {
            required(
                "workload_identity.source.aws_sts.audience",
                &source.audience,
            )?;
            required("workload_identity.source.aws_sts.region", &source.region)?;
        }
        Some(Source::TokenFile(source)) => {
            required("workload_identity.source.token_file.path", &source.path)?
        }
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

// The existing provider YAML adapter tolerates extra legacy fields. Reject
// misplaced authentication explicitly so an unsupported block cannot be ignored.
// Run after merging layers, since a child may inherit its provider type.
pub(super) fn validate_field_placement(value: &serde_json::Value) -> Result<()> {
    if let Some(providers) = value
        .get("providers")
        .and_then(serde_json::Value::as_object)
    {
        for (name, provider) in providers {
            if provider.get("auth").is_some() {
                bail!("provider '{name}': unknown field `auth`; configure workload_identity directly on OpenAI");
            }
            if provider.get("workload_identity").is_some()
                && provider.get("type").and_then(serde_json::Value::as_str) != Some("openai")
            {
                bail!("provider '{name}': workload_identity is supported only by the native OpenAI provider");
            }
        }
    }
    Ok(())
}

pub(super) fn validate_config(config: &proto::TalonConfig) -> Result<()> {
    for (name, provider) in &config.providers {
        if let Some(proto::llm_provider_config::Config::Openai(openai)) = &provider.config {
            validate_openai_auth(openai)
                .map_err(|error| anyhow::anyhow!("provider '{name}': {error}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::config::{Config, ConfigExt};
    use prost::Message;

    fn yaml(provider: &str, identity: &str) -> String {
        format!("providers:\n  llm:\n    type: {provider}\n    model: demo\n{identity}")
    }

    fn identity(source: &str) -> String {
        format!("    workload_identity:\n      identity_provider_id: idp\n      service_account_id: account\n      source:\n{source}\n")
    }

    fn openai(config: &Config) -> &proto::OpenAiConfig {
        let Some(proto::llm_provider_config::Config::Openai(openai)) =
            &config.providers["llm"].config
        else {
            panic!("expected OpenAI")
        };
        openai
    }

    #[test]
    fn yaml_uses_generated_proto_fields_and_survives_binary_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        for (name, source) in [
            ("google", "        google_metadata:\n          audience: https://api.openai.com/v1"),
            ("aws", "        aws_sts:\n          audience: https://api.openai.com/v1\n          region: us-west-2"),
            ("file", "        token_file:\n          path: ./projected-token"),
        ] {
            let path = dir.path().join(format!("{name}.yaml"));
            std::fs::write(&path, yaml("openai", &identity(source))).unwrap();
            let config = Config::from_file(&path).unwrap();
            assert_eq!(config, Config::decode_binary(&config.encode_to_vec()).unwrap());
            let wif = openai(&config).workload_identity.as_ref().unwrap();
            // The YAML loader uses this exact generated message, without a conversion layer.
            let serialized = serde_yaml::to_value(wif).unwrap();
            assert!(serialized.get("identity_provider_id").is_some());
            assert!(serialized.get("mode").is_none());
            assert!(serialized.get("openai").is_none());
            assert!(serialized["source"].get("config").is_none());
            assert_eq!(*wif, serde_yaml::from_value::<proto::OpenAiWorkloadIdentityConfig>(serialized).unwrap());
            if let Some(proto::workload_identity_source::Config::TokenFile(source)) = wif.source.as_ref().unwrap().config.as_ref() {
                assert_eq!(source.path, dir.path().join("projected-token").to_str().unwrap());
            }
        }
    }

    #[test]
    fn rejects_unsupported_conflicting_missing_empty_and_unknown_settings() {
        let dir = tempfile::tempdir().unwrap();
        let valid = identity("        google_metadata:\n          audience: audience");
        let cases = [
            (yaml("anthropic", &valid), "supported only by the native OpenAI"),
            (yaml("google", &valid), "supported only by the native OpenAI"),
            (yaml("openai_compatible", &(String::from("    base_url: https://example.com\n") + &valid)), "supported only by the native OpenAI"),
            (yaml("openai", &(String::from("    api_key: key\n") + &valid)), "cannot both be configured"),
            (yaml("openai", &valid.replace("identity_provider_id: idp", "identity_provider_id: ' '")), "identity_provider_id must not be empty"),
            (yaml("openai", &valid.replace("service_account_id: account", "service_account_id: ''")), "service_account_id must not be empty"),
            (yaml("openai", &valid.replace("audience: audience", "audience: ''")), "audience must not be empty"),
            (yaml("openai", &valid.replace("      identity_provider_id: idp\n", "")), "identity_provider_id must not be empty"),
            (yaml("openai", "    auth:\n      type: workload_identity\n"), "unknown field `auth`"),
            (yaml("openai", &valid.replace("google_metadata:", "unknown_cloud:")), "unknown variant"),
            (yaml("openai", &valid.replace("          audience: audience", "          audience: audience\n          unexpected: true")), "unknown field"),
            (yaml("openai", &identity("        aws_sts:\n          audience: audience\n          region: ''")), "region must not be empty"),
            (yaml("openai", &identity("        token_file:\n          path: ''")), "path must not be empty"),
            (yaml("openai", &identity("        google_metadata:\n          audience: audience\n        token_file:\n          path: /token")), "single key"),
            (yaml("openai", &identity("        type: google_metadata\n        audience: audience")), "single key"),
            (yaml("openai", &identity("        null")), "requires an identity source"),
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
    fn inline_identity_is_validated_and_legacy_api_keys_are_preserved() {
        let _lock = crate::test_support::env_lock();
        let content = yaml(
            "openai",
            &identity(
                "        aws_sts:\n          audience: audience\n          region: us-west-2",
            ),
        );
        let _inline = crate::test_support::EnvVarGuard::set("TALON_CONFIG_INLINE_YAML", &content);
        assert!(openai(&Config::load_default().unwrap())
            .workload_identity
            .is_some());
        let _invalid = crate::test_support::EnvVarGuard::set(
            "TALON_CONFIG_INLINE_YAML",
            &content.replace("us-west-2", "''"),
        );
        assert!(Config::load_default()
            .unwrap_err()
            .to_string()
            .contains("region must not be empty"));
        let _api = crate::test_support::EnvVarGuard::set(
            "TALON_CONFIG_INLINE_YAML",
            &yaml("openai", "    api_key: existing-key\n"),
        );
        let config = Config::load_default().unwrap();
        assert!(openai(&config).api_key.is_some());
        assert!(openai(&config).workload_identity.is_none());
    }

    #[test]
    fn binary_configuration_cannot_bypass_identity_validation() {
        let mut config = Config::default();
        config.providers.insert(
            "llm".into(),
            proto::LlmProviderConfig {
                config: Some(proto::llm_provider_config::Config::Openai(
                    proto::OpenAiConfig {
                        workload_identity: Some(proto::OpenAiWorkloadIdentityConfig::default()),
                        ..Default::default()
                    },
                )),
            },
        );
        assert!(Config::decode_binary(&config.encode_to_vec())
            .unwrap_err()
            .to_string()
            .contains("identity_provider_id must not be empty"));
    }
}
