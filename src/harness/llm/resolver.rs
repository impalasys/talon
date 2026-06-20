// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Context, Result};
use std::sync::Arc;

use crate::control::config::secrets::SecretExt;
use crate::control::config::Config;
use crate::gateway::rpc::manifests::{self, AgentSpec};
use crate::harness::llm::LlmProvider;

pub struct ResolvedLlm {
    pub provider: Arc<dyn LlmProvider + Send + Sync>,
    pub provider_key: String,
    pub model: String,
}

pub fn resolve_model_profile(policy: Option<&manifests::ModelPolicy>) -> Option<&manifests::Model> {
    policy
        .and_then(|policy| {
            policy
                .profiles
                .iter()
                .find(|profile| profile.name == "default")
                .or_else(|| {
                    policy
                        .profiles
                        .iter()
                        .find(|profile| profile.model.is_some())
                })
        })
        .and_then(|profile| profile.model.as_ref())
}

/// Resolve the correct LLM provider for an agent given its spec and the
/// system configuration. Preference order:
///   1. Provider + model from AgentSpec.model_policy profile "default"
///   2. Config's default_provider
/// Returns an error when the selected provider is missing, unsupported, or
/// cannot resolve its credentials.
pub async fn resolve_llm(spec: &AgentSpec, config: &Config) -> Result<ResolvedLlm> {
    let selected_model = resolve_model_profile(spec.model_policy.as_ref());
    let spec_provider = selected_model.map(|m| m.provider.as_str());
    let spec_model = selected_model.map(|m| m.name.as_str());

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
        .ok_or_else(|| anyhow!("LLM provider '{}' not found in config", provider_name))?;

    match &provider_cfg.config {
        Some(crate::control::config::proto::llm_provider_config::Config::Openai(openai)) => {
            let api_key = openai
                .api_key
                .as_ref()
                .context("OpenAI provider config is missing api_key")?
                .resolve()
                .await
                .with_context(|| format!("Failed to resolve API key for '{}'", provider_name))?;

            let base_url = std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
            let model = spec_model
                .filter(|s| !s.is_empty())
                .unwrap_or(&openai.model)
                .to_string();

            Ok(ResolvedLlm {
                provider: Arc::new(crate::harness::llm::openai::OpenAiCompatibleProvider::new(
                    api_key,
                    base_url,
                    model.clone(),
                )),
                provider_key: provider_name.to_string(),
                model,
            })
        }
        Some(crate::control::config::proto::llm_provider_config::Config::Anthropic(anthropic)) => {
            let api_key = anthropic
                .api_key
                .as_ref()
                .context("Anthropic provider config is missing api_key")?
                .resolve()
                .await
                .with_context(|| format!("Failed to resolve API key for '{}'", provider_name))?;
            let model = spec_model
                .filter(|s| !s.is_empty())
                .unwrap_or(&anthropic.model)
                .to_string();

            Ok(ResolvedLlm {
                provider: Arc::new(crate::harness::llm::anthropic::AnthropicProvider::new(
                    api_key,
                    model.clone(),
                )),
                provider_key: provider_name.to_string(),
                model,
            })
        }
        Some(crate::control::config::proto::llm_provider_config::Config::OpenaiCompatible(
            generic,
        )) => {
            let api_key = generic
                .api_key
                .as_ref()
                .context("OpenAI-compatible provider config is missing api_key")?
                .resolve()
                .await
                .with_context(|| format!("Failed to resolve API key for '{}'", provider_name))?;

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

            Ok(ResolvedLlm {
                provider: Arc::new(crate::harness::llm::openai::OpenAiCompatibleProvider::new(
                    api_key,
                    base_url,
                    model.clone(),
                )),
                provider_key: provider_name.to_string(),
                model,
            })
        }
        Some(crate::control::config::proto::llm_provider_config::Config::Google(_)) => {
            Err(anyhow!(
                "LLM provider '{}' uses Google config, which is not supported by this runtime yet",
                provider_name
            ))
        }
        None => Err(anyhow!(
            "LLM provider '{}' has no config; refusing to fall back to a mock provider",
            provider_name
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_llm, resolve_model_profile};
    use crate::control::config::{proto, Config, ProviderConfig, Secret};
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
                        thinking: None,
                    }),
                }],
            }),
            a2a: None,
            runtime: None,
        }
    }

    #[test]
    fn resolve_model_profile_falls_back_to_first_profile_with_model() {
        let policy = manifests::ModelPolicy {
            profiles: vec![
                manifests::ModelProfile {
                    name: "secondary".to_string(),
                    model: Some(manifests::Model {
                        provider: "openai".to_string(),
                        name: "fallback-model".to_string(),
                        temperature: 0.0,
                        thinking: None,
                    }),
                },
                manifests::ModelProfile {
                    name: "empty".to_string(),
                    model: None,
                },
            ],
        };

        let model = resolve_model_profile(Some(&policy)).unwrap();
        assert_eq!(model.name, "fallback-model");
    }

    #[tokio::test]
    async fn resolve_llm_errors_when_provider_config_is_unrecognized() {
        let config = Config {
            providers: HashMap::from([("primary".to_string(), ProviderConfig { config: None })]),
            default_provider: "primary".to_string(),
            ..Config::default()
        };
        let spec = manifests::AgentSpec::default();

        let err = match resolve_llm(&spec, &config).await {
            Ok(_) => panic!("expected provider config error"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("refusing to fall back to a mock provider"));
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
        let _guard = crate::test_support::async_env_mutex().lock().await;
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
        assert_eq!(llm.provider_key, "secondary");
        assert_eq!(llm.model, "spec-model");
        let response = llm.provider.completion("ping").await.unwrap();
        assert_eq!(response, "resolved via spec provider");

        server.abort();
    }

    #[tokio::test]
    async fn resolve_llm_uses_env_override_for_base_url() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
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
            std::env::set_var("NOVITA_BASE_URL", format!("http://{addr}"));
        }
        let config = config_with_provider(
            "primary",
            openai_compatible_provider(
                "https://unused.example.com".to_string(),
                Some(Secret {
                    source: Some(proto::secret::Source::Plain("config-key".to_string())),
                }),
            ),
        );
        let llm = resolve_llm(&manifests::AgentSpec::default(), &config)
            .await
            .unwrap();
        assert_eq!(llm.provider_key, "primary");
        assert_eq!(llm.model, "config-model");
        let response = llm.provider.completion("ping").await.unwrap();
        assert_eq!(response, "resolved via env override");

        unsafe {
            std::env::remove_var("NOVITA_BASE_URL");
        }
        server.abort();
    }
}
