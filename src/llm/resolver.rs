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
