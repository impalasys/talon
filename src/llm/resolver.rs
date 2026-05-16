// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use std::sync::Arc;

use crate::config::Config;
use crate::gateway::rpc::manifests::AgentSpec;
use crate::llm::LlmProvider;

/// Resolve the correct LLM provider for an agent given its spec and the
/// system configuration. Preference order:
///   1. Provider + model from AgentSpec.model_policy profile "default"
///   2. Config's default_provider
///   3. Falls back to MockLlmProvider (never returns an error for missing keys)
pub async fn resolve_llm(
    spec: &AgentSpec,
    config: &Config,
) -> Result<Arc<dyn LlmProvider + Send + Sync>> {
    let default_profile = spec
        .model_policy
        .as_ref()
        .and_then(|policy| {
            policy
                .profiles
                .iter()
                .find(|profile| profile.name == "default")
        })
        .and_then(|profile| profile.model.as_ref());
    let spec_provider = default_profile.map(|m| m.provider.as_str());
    let spec_model = default_profile.map(|m| m.name.as_str());

    let default_key = if config.default_provider.is_empty() {
        config.providers.keys().next().map(|k| k.as_str())
    } else {
        Some(config.default_provider.as_str())
    };

    let provider_name = spec_provider
        .filter(|s| !s.is_empty())
        .or(default_key)
        .unwrap_or("novita");

    tracing::debug!(provider = provider_name, "Resolved LLM provider");

    let provider_cfg = config
        .providers
        .get(provider_name)
        .ok_or_else(|| anyhow::anyhow!("LLM provider '{}' not found in config", provider_name))?;

    match &provider_cfg.config {
        Some(crate::config::proto::llm_provider_config::Config::OpenaiCompatible(generic)) => {
            use crate::config::secrets::SecretExt;
            let api_key = if let Some(secret) = &generic.api_key {
                secret.resolve().await.unwrap_or_else(|e| {
                    tracing::warn!("Failed to resolve API key for '{}': {}", provider_name, e);
                    "sk_dummy".to_string()
                })
            } else {
                std::env::var("NOVITA_API_KEY").unwrap_or_else(|_| "sk_dummy".to_string())
            };

            // Allow env-var override of base URL (useful in local dev / CI)
            let base_url = if let Ok(env_url) = std::env::var("NOVITA_BASE_URL") {
                tracing::warn!("Overriding LLM base URL with NOVITA_BASE_URL");
                env_url
            } else {
                generic.base_url.clone()
            };

            let model = spec_model
                .filter(|s| !s.is_empty())
                .unwrap_or(&generic.model)
                .to_string();

            Ok(Arc::new(crate::llm::openai::OpenAiCompatibleProvider::new(
                api_key, base_url, model,
            )))
        }
        _ => {
            tracing::warn!(
                "No recognized LLM config for provider '{}'; using MockLlmProvider",
                provider_name
            );
            Ok(Arc::new(crate::llm::mock::MockLlmProvider))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_llm;
    use crate::config::{proto, Config, ProviderConfig, Secret};
    use crate::gateway::rpc::manifests;
    use axum::{routing::post, Json, Router};
    use serde_json::json;
    use std::collections::HashMap;
    use tokio::net::TcpListener;

    fn config_with_provider(name: &str, provider: ProviderConfig) -> Config {
        Config {
            providers: HashMap::from([(name.to_string(), provider)]),
            default_provider: name.to_string(),
            ..Config::default()
        }
    }

    fn openai_compatible_provider(base_url: String, api_key: Option<Secret>) -> ProviderConfig {
        ProviderConfig {
            config: Some(proto::llm_provider_config::Config::OpenaiCompatible(
                proto::GenericConfig {
                    name: String::new(),
                    base_url,
                    model: "config-model".to_string(),
                    api_key,
                },
            )),
        }
    }

    fn spec_with_default_model(provider: &str, name: &str) -> manifests::AgentSpec {
        manifests::AgentSpec {
            features: Vec::new(),
            system_prompt: String::new(),
            mcp_server_refs: Vec::new(),
            capabilities: HashMap::new(),
            model_policy: Some(manifests::ModelPolicy {
                profiles: vec![manifests::ModelProfile {
                    name: "default".to_string(),
                    model: Some(manifests::Model {
                        provider: provider.to_string(),
                        name: name.to_string(),
                        temperature: 0.0,
                    }),
                }],
            }),
        }
    }

    #[tokio::test]
    async fn resolve_llm_uses_mock_when_provider_config_is_unrecognized() {
        let config = Config {
            providers: HashMap::from([("primary".to_string(), ProviderConfig { config: None })]),
            default_provider: "primary".to_string(),
            ..Config::default()
        };
        let spec = manifests::AgentSpec::default();

        let llm = resolve_llm(&spec, &config).await.unwrap();
        let response = llm.completion("hello").await.unwrap();
        assert_eq!(response, "Mock response for: hello");
    }

    #[tokio::test]
    async fn resolve_llm_errors_when_selected_provider_is_missing() {
        let config = Config {
            default_provider: "missing".to_string(),
            ..Config::default()
        };

        let err = match resolve_llm(&manifests::AgentSpec::default(), &config).await {
            Ok(_) => panic!("expected missing provider error"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("LLM provider 'missing' not found"));
    }

    #[tokio::test]
    async fn resolve_llm_prefers_spec_provider_and_model_for_openai_compatible() {
        let _guard = crate::test_support::env_mutex().lock().unwrap();
        unsafe {
            std::env::remove_var("NOVITA_API_KEY");
            std::env::remove_var("NOVITA_BASE_URL");
        }
        let app = Router::new().route(
            "/chat/completions",
            post(|| async {
                Json(json!({
                    "choices": [{
                        "message": {
                            "content": "resolved via spec provider",
                            "tool_calls": []
                        }
                    }]
                }))
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let config = config_with_provider(
            "secondary",
            openai_compatible_provider(
                format!("http://{addr}"),
                Some(Secret {
                    source: Some(proto::secret::Source::Plain("api-key".to_string())),
                }),
            ),
        );
        let spec = spec_with_default_model("secondary", "spec-model");
        let llm = resolve_llm(&spec, &config).await.unwrap();
        let response = llm.completion("ping").await.unwrap();
        assert_eq!(response, "resolved via spec provider");

        server.abort();
    }

    #[tokio::test]
    async fn resolve_llm_uses_env_fallbacks_for_api_key_and_base_url() {
        let _guard = crate::test_support::env_mutex().lock().unwrap();
        let app = Router::new().route(
            "/chat/completions",
            post(|| async {
                Json(json!({
                    "choices": [{
                        "message": {
                            "content": "resolved via env override",
                            "tool_calls": []
                        }
                    }]
                }))
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        unsafe {
            std::env::set_var("NOVITA_API_KEY", "env-key");
            std::env::set_var("NOVITA_BASE_URL", format!("http://{addr}"));
        }
        let config = config_with_provider(
            "primary",
            openai_compatible_provider("https://unused.example.com".to_string(), None),
        );
        let llm = resolve_llm(&manifests::AgentSpec::default(), &config)
            .await
            .unwrap();
        let response = llm.completion("ping").await.unwrap();
        assert_eq!(response, "resolved via env override");

        unsafe {
            std::env::remove_var("NOVITA_API_KEY");
            std::env::remove_var("NOVITA_BASE_URL");
        }
        server.abort();
    }
}
