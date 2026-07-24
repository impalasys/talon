// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Context, Result};
use std::sync::Arc;

use crate::control::config::secrets::SecretExt;
use crate::control::config::{proto, Config, Secret};
use crate::control::ControlPlane;
use crate::gateway::rpc::manifests::{self, AgentSpec};
use crate::harness::llm::LlmProvider;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};

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
    resolve_llm_with_credentials(spec, config, None).await
}

pub async fn resolve_llm_for_namespace(
    spec: &AgentSpec,
    config: &Config,
    cp: &ControlPlane,
    namespace: &str,
) -> Result<ResolvedLlm> {
    resolve_llm_with_credentials(
        spec,
        config,
        Some(TenantCredentialContext { cp, namespace }),
    )
    .await
}

#[derive(Clone, Copy)]
struct TenantCredentialContext<'a> {
    cp: &'a ControlPlane,
    namespace: &'a str,
}

async fn resolve_llm_with_credentials(
    spec: &AgentSpec,
    config: &Config,
    credentials: Option<TenantCredentialContext<'_>>,
) -> Result<ResolvedLlm> {
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
        Some(proto::llm_provider_config::Config::Openai(openai)) => {
            let api_key = resolve_provider_api_key(
                provider_name,
                openai.api_key.as_ref(),
                credentials,
                "OpenAI provider config is missing api_key",
            )
            .await?;

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
        Some(proto::llm_provider_config::Config::Anthropic(anthropic)) => {
            let api_key = resolve_provider_api_key(
                provider_name,
                anthropic.api_key.as_ref(),
                credentials,
                "Anthropic provider config is missing api_key",
            )
            .await?;
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
        Some(proto::llm_provider_config::Config::OpenaiCompatible(generic)) => {
            let api_key = resolve_provider_api_key(
                provider_name,
                generic.api_key.as_ref(),
                credentials,
                "OpenAI-compatible provider config is missing api_key",
            )
            .await?;

            let base_url = generic.base_url.clone();

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
        Some(proto::llm_provider_config::Config::Google(_)) => Err(anyhow!(
            "LLM provider '{}' uses Google config, which is not supported by this runtime yet",
            provider_name
        )),
        None => Err(anyhow!(
            "LLM provider '{}' has no config; refusing to fall back to a mock provider",
            provider_name
        )),
    }
}

async fn resolve_provider_api_key(
    provider_name: &str,
    configured: Option<&Secret>,
    credentials: Option<TenantCredentialContext<'_>>,
    missing_config_message: &'static str,
) -> Result<String> {
    if let Some(credentials) = credentials {
        if let Some(tenant_namespace) = tenant_root_namespace(credentials.namespace) {
            return resolve_tenant_provider_api_key(
                credentials.cp,
                &tenant_namespace,
                provider_name,
            )
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "Tenant namespace '{}' requires Secret '{}' in namespace '{}' to contain API key '{}'",
                    credentials.namespace,
                    API_KEYS_SECRET_NAME,
                    tenant_namespace,
                    provider_name
                )
            });
        }
    }

    configured
        .context(missing_config_message)?
        .resolve()
        .await
        .with_context(|| format!("Failed to resolve API key for '{}'", provider_name))
}

pub const API_KEYS_SECRET_NAME: &str = "api-keys";

async fn resolve_tenant_provider_api_key(
    cp: &ControlPlane,
    tenant_namespace: &str,
    provider_name: &str,
) -> Result<Option<String>> {
    let store = crate::control::resources::ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    let Some(secret) = store
        .get_secret(tenant_namespace, API_KEYS_SECRET_NAME)
        .await?
    else {
        return Ok(None);
    };
    let Some(spec) = secret.spec else {
        return Ok(None);
    };
    let Some(encoded) = provider_api_key_value(&spec.data, provider_name) else {
        return Ok(None);
    };
    let raw = BASE64_STANDARD
        .decode(encoded.as_bytes())
        .with_context(|| {
            format!(
                "Secret '{}' provider API key must be base64-encoded",
                API_KEYS_SECRET_NAME
            )
        })?;
    let api_key = String::from_utf8(raw).with_context(|| {
        format!(
            "Secret '{}' provider API key must be UTF-8",
            API_KEYS_SECRET_NAME
        )
    })?;
    if api_key.trim().is_empty() {
        return Err(anyhow!(
            "Secret '{}' in namespace '{}' has empty API key for provider '{}'",
            API_KEYS_SECRET_NAME,
            tenant_namespace,
            provider_name
        ));
    }
    tracing::debug!(
        provider = provider_name,
        secret_namespace = tenant_namespace,
        secret_name = %API_KEYS_SECRET_NAME,
        "Resolved tenant LLM provider secret"
    );
    Ok(Some(api_key))
}

pub fn tenant_root_namespace(namespace: &str) -> Option<String> {
    let mut segments = namespace.split(':');
    match (segments.next(), segments.next()) {
        (Some("Tenant"), Some(tenant_id)) if !tenant_id.trim().is_empty() => {
            Some(format!("Tenant:{tenant_id}"))
        }
        _ => None,
    }
}

fn provider_api_key_value<'a>(
    data: &'a std::collections::HashMap<String, String>,
    provider_name: &str,
) -> Option<&'a String> {
    data.get(provider_name)
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_llm, resolve_llm_for_namespace, resolve_model_profile, API_KEYS_SECRET_NAME,
    };
    use crate::control::config::{proto, Config, ProviderConfig, Secret};
    use crate::control::ControlPlane;
    use crate::gateway::rpc::manifests;
    use crate::gateway::rpc::resources_proto;
    use axum::{http::HeaderMap, routing::post, Json, Router};
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
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
            post_history_prompt: String::new(),
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

    async fn put_llm_provider_secret(
        cp: &ControlPlane,
        namespace: &str,
        provider: &str,
        api_key: &str,
    ) {
        let store = crate::control::resources::ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
        store
            .upsert(
                namespace,
                resources_proto::Resource {
                    api_version: "talon.impalasys.com/v1".to_string(),
                    kind: "Secret".to_string(),
                    metadata: Some(resources_proto::ResourceMeta {
                        namespace: namespace.to_string(),
                        name: API_KEYS_SECRET_NAME.to_string(),
                        ..Default::default()
                    }),
                    spec: Some(resources_proto::ResourceSpec {
                        kind: Some(resources_proto::resource_spec::Kind::Secret(
                            resources_proto::SecretSpec {
                                r#type: String::new(),
                                data: HashMap::from([(
                                    provider.to_string(),
                                    BASE64_STANDARD.encode(api_key.as_bytes()),
                                )]),
                                string_data: HashMap::new(),
                            },
                        )),
                    }),
                    status: None,
                },
            )
            .await
            .unwrap();
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
    async fn resolve_llm_for_namespace_uses_tenant_provider_secret() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
        let app = Router::new().route(
            "/chat/completions",
            post(|headers: HeaderMap| async move {
                assert_eq!(
                    headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok()),
                    Some("Bearer tenant-key")
                );
                Json(json!({
                    "choices": [{
                        "message": {
                            "content": "resolved via tenant secret",
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
            std::env::set_var("OPENAI_BASE_URL", format!("http://{addr}"));
        }

        let kv = Arc::new(crate::test_support::MockKvStore::default());
        let cp = ControlPlane::builder(
            kv,
            Arc::new(crate::test_support::RecordingPubSub::default()),
        )
        .build();
        put_llm_provider_secret(&cp, "Tenant:acme", "openai", "tenant-key").await;
        let config = config_with_provider(
            "openai",
            ProviderConfig {
                config: Some(proto::llm_provider_config::Config::Openai(
                    proto::OpenAiConfig {
                        model: "config-model".to_string(),
                        api_key: None,
                        org_id: String::new(),
                    },
                )),
            },
        );
        let llm = resolve_llm_for_namespace(
            &manifests::AgentSpec::default(),
            &config,
            &cp,
            "Tenant:acme:Workspace:main",
        )
        .await
        .unwrap();
        assert_eq!(llm.provider_key, "openai");
        let response = llm.provider.completion("ping").await.unwrap();
        assert_eq!(response, "resolved via tenant secret");

        unsafe {
            std::env::remove_var("OPENAI_BASE_URL");
        }
        server.abort();
    }

    #[tokio::test]
    async fn resolve_llm_for_namespace_uses_literal_provider_secret_key() {
        let app = Router::new().route(
            "/chat/completions",
            post(|headers: HeaderMap| async move {
                assert_eq!(
                    headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok()),
                    Some("Bearer tenant-azure-key")
                );
                Json(json!({
                    "choices": [{
                        "message": {
                            "content": "resolved via literal tenant key",
                            "tool_calls": []
                        }
                    }]
                }))
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let kv = Arc::new(crate::test_support::MockKvStore::default());
        let cp = ControlPlane::builder(
            kv,
            Arc::new(crate::test_support::RecordingPubSub::default()),
        )
        .build();
        put_llm_provider_secret(&cp, "Tenant:acme", "azure_openai", "tenant-azure-key").await;
        let config = config_with_provider(
            "azure_openai",
            openai_compatible_provider(format!("http://{addr}"), None),
        );
        let llm = resolve_llm_for_namespace(
            &manifests::AgentSpec::default(),
            &config,
            &cp,
            "Tenant:acme:Workspace:main",
        )
        .await
        .unwrap();
        assert_eq!(llm.provider_key, "azure_openai");
        let response = llm.provider.completion("ping").await.unwrap();
        assert_eq!(response, "resolved via literal tenant key");

        server.abort();
    }

    #[tokio::test]
    async fn resolve_llm_for_namespace_ignores_workspace_provider_secret() {
        let kv = Arc::new(crate::test_support::MockKvStore::default());
        let cp = ControlPlane::builder(
            kv,
            Arc::new(crate::test_support::RecordingPubSub::default()),
        )
        .build();
        put_llm_provider_secret(&cp, "Tenant:acme:Workspace:main", "openai", "workspace-key").await;
        let config = config_with_provider(
            "openai",
            ProviderConfig {
                config: Some(proto::llm_provider_config::Config::Openai(
                    proto::OpenAiConfig {
                        model: "config-model".to_string(),
                        api_key: None,
                        org_id: String::new(),
                    },
                )),
            },
        );
        let err = match resolve_llm_for_namespace(
            &manifests::AgentSpec::default(),
            &config,
            &cp,
            "Tenant:acme:Workspace:main",
        )
        .await
        {
            Ok(_) => panic!("workspace-scoped provider Secret should be ignored"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("requires Secret 'api-keys' in namespace 'Tenant:acme'"));
    }

    #[tokio::test]
    async fn resolve_llm_for_tenant_namespace_does_not_fallback_to_provider_config() {
        let kv = Arc::new(crate::test_support::MockKvStore::default());
        let cp = ControlPlane::builder(
            kv,
            Arc::new(crate::test_support::RecordingPubSub::default()),
        )
        .build();
        let config = config_with_provider(
            "openai",
            ProviderConfig {
                config: Some(proto::llm_provider_config::Config::Openai(
                    proto::OpenAiConfig {
                        model: "config-model".to_string(),
                        api_key: Some(Secret {
                            source: Some(proto::secret::Source::Plain(
                                "global-key-must-not-be-used".to_string(),
                            )),
                        }),
                        org_id: String::new(),
                    },
                )),
            },
        );
        let err = match resolve_llm_for_namespace(
            &manifests::AgentSpec::default(),
            &config,
            &cp,
            "Tenant:acme:Workspace:main",
        )
        .await
        {
            Ok(_) => panic!("tenant namespace should not use provider-level fallback key"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("requires Secret 'api-keys' in namespace 'Tenant:acme'"));
    }

    #[tokio::test]
    async fn resolve_llm_uses_configured_base_url() {
        let app = Router::new().route(
            "/chat/completions",
            post(|| async {
                Json(json!({
                    "choices": [{
                        "message": {
                            "content": "resolved via configured base URL",
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
            "primary",
            openai_compatible_provider(
                format!("http://{addr}"),
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
        assert_eq!(response, "resolved via configured base URL");

        server.abort();
    }
}
