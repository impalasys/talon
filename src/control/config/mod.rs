// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Context, Result};
use prost::Message;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::env;
use std::path::{Component, Path, PathBuf};

pub mod secrets;

// The generated code's location depends on the build system (Cargo vs. Bazel)
#[cfg(not(feature = "bazel"))]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/talon.config.rs"));
}

#[cfg(feature = "bazel")]
pub mod proto {
    pub use talon_config_proto::talon::config::*;
}

pub use proto::{
    DatabaseConfig, LlmProviderConfig as ProviderConfig, ObjectStoreConfig, SchedulerConfig,
    SecretRef, ServerConfig, TalonConfig as Config,
};
pub use secrets::{Secret, SecretExt};

// Interop with Serde for file-based config
#[derive(Debug, Deserialize, Clone)]
pub struct SerdeConfig {
    #[serde(default)]
    pub providers: HashMap<String, SerdeProviderConfig>,
    #[serde(default, rename = "llmProviders")]
    pub llm_providers: HashMap<String, SerdeProviderConfig>,
    pub database: Option<DatabaseConfigWrapper>,
    pub server: Option<ServerConfigWrapper>,
    pub default_provider: Option<String>,
    pub workspace_dir: Option<String>,
    pub control_plane: Option<ControlPlaneConfigWrapper>,
    pub storage: Option<StorageConfigWrapper>,
    pub pubsub: Option<MessageBrokerConfigWrapper>,
    #[serde(default)]
    pub controllers: HashMap<String, ControllerConfigWrapper>,
    pub trust: Option<TrustConfigWrapper>,
    #[serde(default, alias = "llmModels", alias = "modelLimits")]
    pub models: HashMap<String, ModelConfigWrapper>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SerdeProviderConfig {
    Openai {
        model: String,
        #[serde(alias = "apiKey")]
        api_key: Option<SerdeSecret>,
        #[serde(default)]
        api: Option<String>,
    },
    Anthropic {
        model: String,
        #[serde(alias = "apiKey")]
        api_key: Option<SerdeSecret>,
    },
    Google {
        model: String,
        #[serde(alias = "apiKey")]
        api_key: Option<SerdeSecret>,
    },
    OpenaiCompatible {
        #[serde(alias = "baseUrl")]
        base_url: String,
        model: String,
        #[serde(alias = "apiKey")]
        api_key: Option<SerdeSecret>,
    },
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum SerdeSecret {
    Plain(String),
    Ref(SerdeSecretRef),
}

#[derive(Debug, Deserialize, Clone)]
pub struct SerdeSecretRef {
    pub source: String,
    pub key: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfigWrapper {
    pub data_dir: Option<String>,
    pub driver: Option<String>,
    pub url: Option<SerdeSecret>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MessageBrokerConfigWrapper {
    pub driver: String,
    // absorb any extra fields (e.g. project_id) without failing
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ControlPlaneConfigWrapper {
    pub database: DatabaseConfigWrapper,
    pub message_broker: MessageBrokerConfigWrapper,
    pub scheduler: Option<SchedulerConfigWrapper>,
    pub object_store: Option<ObjectStoreConfigWrapper>,
    pub documents: Option<DatabaseConfigWrapper>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfigWrapper {
    pub control: DatabaseConfigWrapper,
    pub data: Option<DatabaseConfigWrapper>,
    pub documents: Option<DatabaseConfigWrapper>,
    pub objects: Option<ObjectStoreConfigWrapper>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ControllerConfigWrapper {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub workers: u32,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ModelConfigWrapper {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(
        default,
        alias = "contextWindowTokens",
        alias = "contextWindow",
        alias = "contextLimit"
    )]
    pub context_window_tokens: Option<u64>,
    #[serde(default, alias = "maxOutputTokens", alias = "maxOutput")]
    pub max_output_tokens: Option<u64>,
    #[serde(default, alias = "inputCostPerMillionTokens", alias = "inputCost")]
    pub input_cost_per_million_tokens: Option<f64>,
    #[serde(default, alias = "outputCostPerMillionTokens", alias = "outputCost")]
    pub output_cost_per_million_tokens: Option<f64>,
    #[serde(default, alias = "cacheReadCostPerMillionTokens")]
    pub cache_read_cost_per_million_tokens: Option<f64>,
    #[serde(default, alias = "cacheWriteCostPerMillionTokens")]
    pub cache_write_cost_per_million_tokens: Option<f64>,
    #[serde(default, alias = "longContextTokens")]
    pub long_context_tokens: Option<u64>,
    #[serde(default, alias = "longContextInputCostPerMillionTokens")]
    pub long_context_input_cost_per_million_tokens: Option<f64>,
    #[serde(default, alias = "longContextOutputCostPerMillionTokens")]
    pub long_context_output_cost_per_million_tokens: Option<f64>,
    #[serde(default, alias = "longContextCacheReadCostPerMillionTokens")]
    pub long_context_cache_read_cost_per_million_tokens: Option<f64>,
    #[serde(default, alias = "longContextCacheWriteCostPerMillionTokens")]
    pub long_context_cache_write_cost_per_million_tokens: Option<f64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfigWrapper {
    pub host: String,
    pub port: u32,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "driver", rename_all = "snake_case")]
pub enum SchedulerConfigWrapper {
    CloudTasks {
        project_id: Option<String>,
        location: Option<String>,
        queue: Option<String>,
        target_url: Option<String>,
        callback_auth: Option<SchedulerCallbackAuthConfigWrapper>,
    },
    AwsEventBridgeScheduler {
        group_name: Option<String>,
        queue_url: Option<String>,
        execution_role_arn: Option<String>,
        schedule_name_prefix: Option<String>,
        dlq_arn: Option<String>,
        maximum_event_age_seconds: Option<u32>,
        maximum_retry_attempts: Option<u32>,
        endpoint_url: Option<String>,
    },
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "driver", rename_all = "snake_case")]
pub enum ObjectStoreConfigWrapper {
    Local {
        path: Option<String>,
    },
    Gcs {
        bucket: String,
        prefix: Option<String>,
        api_base_url: Option<String>,
    },
    S3 {
        bucket: String,
        prefix: Option<String>,
        region: Option<String>,
        endpoint_url: Option<String>,
        force_path_style: Option<bool>,
    },
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SchedulerCallbackAuthConfigWrapper {
    SharedSecret {
        token: SerdeSecret,
    },
    GoogleOidc {
        audience: String,
        service_account_email: Option<String>,
    },
}

#[derive(Debug, Deserialize, Clone)]
pub struct TrustConfigWrapper {
    #[serde(default)]
    pub oidc: Vec<OidcTrustEntryConfigWrapper>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OidcTrustEntryConfigWrapper {
    pub name: String,
    pub issuer: String,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default)]
    pub audiences: Vec<String>,
    #[serde(default, rename = "allowedDomains")]
    pub allowed_domains: Vec<String>,
    #[serde(default, rename = "allowedEmails")]
    pub allowed_emails: Vec<String>,
    #[serde(default, rename = "jwksUrl")]
    pub jwks_url: Option<String>,
    #[serde(default, rename = "clockSkewSeconds")]
    pub clock_skew_seconds: Option<u32>,
    #[serde(default)]
    pub grants: Vec<OidcTrustGrantConfigWrapper>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OidcTrustGrantConfigWrapper {
    Read {
        #[serde(default)]
        namespace: String,
        #[serde(default)]
        agent: String,
        #[serde(default)]
        session: String,
        #[serde(default)]
        channel: String,
    },
    Readwrite {
        #[serde(default)]
        namespace: String,
        #[serde(default)]
        agent: String,
        #[serde(default)]
        session: String,
        #[serde(default)]
        channel: String,
    },
}

impl From<SerdeConfig> for Config {
    fn from(s: SerdeConfig) -> Self {
        let mut provider_inputs = s.providers;
        provider_inputs.extend(s.llm_providers);
        let providers = provider_inputs
            .into_iter()
            .map(|(name, p)| {
                let p_proto = match p {
                    SerdeProviderConfig::Openai {
                        model,
                        api_key,
                        api,
                    } => ProviderConfig {
                        config: Some(proto::llm_provider_config::Config::Openai(
                            proto::OpenAiConfig {
                                model,
                                api_key: api_key.map(Into::into),
                                org_id: "".to_string(),
                                api: api.unwrap_or_default(),
                            },
                        )),
                    },
                    SerdeProviderConfig::Anthropic { model, api_key } => ProviderConfig {
                        config: Some(proto::llm_provider_config::Config::Anthropic(
                            proto::AnthropicConfig {
                                model,
                                api_key: api_key.map(Into::into),
                            },
                        )),
                    },
                    SerdeProviderConfig::Google { model, api_key } => ProviderConfig {
                        config: Some(proto::llm_provider_config::Config::Google(
                            proto::GoogleConfig {
                                model,
                                api_key: api_key.map(Into::into),
                            },
                        )),
                    },
                    SerdeProviderConfig::OpenaiCompatible {
                        base_url,
                        model,
                        api_key,
                    } => ProviderConfig {
                        config: Some(proto::llm_provider_config::Config::OpenaiCompatible(
                            proto::GenericConfig {
                                name: "".to_string(),
                                base_url,
                                model,
                                api_key: api_key.map(Into::into),
                            },
                        )),
                    },
                };
                (name, p_proto)
            })
            .collect();

        let control_plane = match (s.control_plane, s.storage, s.pubsub) {
            (Some(cp), _, _) => Some(cp),
            (None, Some(storage), Some(pubsub)) => Some(ControlPlaneConfigWrapper {
                database: storage.control,
                message_broker: pubsub,
                scheduler: None,
                object_store: storage.objects,
                documents: storage.documents,
            }),
            (None, Some(storage), None) => Some(ControlPlaneConfigWrapper {
                database: storage.control,
                message_broker: MessageBrokerConfigWrapper {
                    driver: "local_socket".to_string(),
                    extra: HashMap::new(),
                },
                scheduler: None,
                object_store: storage.objects,
                documents: storage.documents,
            }),
            (None, None, _) => None,
        };

        Config {
            providers,
            database: s.database.map(|db| proto::DatabaseConfig {
                data_dir: db.data_dir.unwrap_or_default(),
                driver: db.driver.unwrap_or_default(),
                url: db.url.map(Into::into),
            }),
            server: s.server.map(|srv| proto::ServerConfig {
                host: srv.host,
                port: srv.port,
            }),
            default_provider: s.default_provider.unwrap_or_default(),
            workspace_dir: s.workspace_dir.unwrap_or_else(|| ".".to_string()),
            control_plane: control_plane.map(|cp| proto::ControlPlaneConfig {
                database: Some(proto::DatabaseConfig {
                    data_dir: cp.database.data_dir.unwrap_or_default(),
                    driver: cp.database.driver.unwrap_or_default(),
                    url: cp.database.url.map(Into::into),
                }),
                message_broker: Some(proto::MessageBrokerConfig {
                    driver: cp.message_broker.driver,
                }),
                scheduler: cp.scheduler.map(Into::into),
                object_store: cp.object_store.map(Into::into),
                documents: cp.documents.map(|db| proto::DatabaseConfig {
                    data_dir: db.data_dir.unwrap_or_default(),
                    driver: db.driver.unwrap_or_default(),
                    url: db.url.map(Into::into),
                }),
            }),
            controllers: s
                .controllers
                .into_iter()
                .map(|(name, controller)| {
                    (
                        name,
                        proto::ControllerConfig {
                            enabled: controller.enabled,
                            workers: controller.workers,
                        },
                    )
                })
                .collect(),
            trust: s.trust.map(Into::into),
            models: s
                .models
                .into_iter()
                .map(|(name, model)| {
                    (
                        name,
                        proto::ModelConfig {
                            provider: model.provider.unwrap_or_default(),
                            context_window_tokens: model.context_window_tokens,
                            max_output_tokens: model.max_output_tokens,
                            input_cost_per_million_tokens: model.input_cost_per_million_tokens,
                            output_cost_per_million_tokens: model.output_cost_per_million_tokens,
                            cache_read_cost_per_million_tokens: model
                                .cache_read_cost_per_million_tokens,
                            cache_write_cost_per_million_tokens: model
                                .cache_write_cost_per_million_tokens,
                            long_context_tokens: model.long_context_tokens,
                            long_context_input_cost_per_million_tokens: model
                                .long_context_input_cost_per_million_tokens,
                            long_context_output_cost_per_million_tokens: model
                                .long_context_output_cost_per_million_tokens,
                            long_context_cache_read_cost_per_million_tokens: model
                                .long_context_cache_read_cost_per_million_tokens,
                            long_context_cache_write_cost_per_million_tokens: model
                                .long_context_cache_write_cost_per_million_tokens,
                        },
                    )
                })
                .collect(),
        }
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Prefix(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => match normalized.components().next_back() {
                Some(Component::Normal(_)) => {
                    normalized.pop();
                }
                Some(Component::ParentDir) | None if !path.is_absolute() => {
                    normalized.push("..");
                }
                _ => {}
            },
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

pub(crate) fn expand_env_placeholders(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find('}') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let name = &after_start[..end];
        if !name.is_empty()
            && name
                .chars()
                .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        {
            match env::var(name) {
                Ok(value) => output.push_str(&value),
                Err(_) => {
                    output.push_str("${");
                    output.push_str(name);
                    output.push('}');
                }
            }
        } else {
            output.push_str("${");
            output.push_str(name);
            output.push('}');
        }
        rest = &after_start[end + 1..];
    }

    output.push_str(rest);
    output
}

fn validate_trust_config(config: &SerdeConfig) -> Result<()> {
    let Some(trust) = &config.trust else {
        return Ok(());
    };

    for entry in &trust.oidc {
        if entry.name.trim().is_empty() {
            return Err(anyhow!("trust.oidc entry name cannot be empty"));
        }
        if entry.issuer.trim().is_empty() {
            return Err(anyhow!(
                "trust.oidc entry '{}' issuer cannot be empty",
                entry.name
            ));
        }
        if entry
            .audience
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
            && entry
                .audiences
                .iter()
                .all(|audience| audience.trim().is_empty())
        {
            return Err(anyhow!(
                "trust.oidc entry '{}' must declare at least one audience",
                entry.name
            ));
        }
        if entry.grants.is_empty() {
            return Err(anyhow!(
                "trust.oidc entry '{}' must declare at least one grant",
                entry.name
            ));
        }

        for grant in &entry.grants {
            validate_oidc_trust_grant(&entry.name, grant)?;
        }
    }

    Ok(())
}

fn validate_provider_config(config: &SerdeConfig) -> Result<()> {
    for (name, provider) in config.providers.iter().chain(config.llm_providers.iter()) {
        let SerdeProviderConfig::Openai { api, .. } = provider else {
            continue;
        };
        let value = api.as_deref().unwrap_or_default().trim();
        if !value.is_empty() && !matches!(value, "responses" | "chat_completions") {
            return Err(anyhow!(
                "provider '{}' has invalid api '{}'; expected 'responses' or 'chat_completions'",
                name,
                value
            ));
        }
    }
    Ok(())
}

fn validate_oidc_trust_grant(entry_name: &str, grant: &OidcTrustGrantConfigWrapper) -> Result<()> {
    let (namespace, agent, session, channel) = match grant {
        OidcTrustGrantConfigWrapper::Read {
            namespace,
            agent,
            session,
            channel,
        }
        | OidcTrustGrantConfigWrapper::Readwrite {
            namespace,
            agent,
            session,
            channel,
        } => (
            namespace.trim(),
            agent.trim(),
            session.trim(),
            channel.trim(),
        ),
    };

    if !agent.is_empty() && namespace.is_empty() {
        return Err(anyhow!(
            "trust.oidc entry '{}' grant with agent selector must include namespace",
            entry_name
        ));
    }
    if !session.is_empty() && (namespace.is_empty() || agent.is_empty()) {
        return Err(anyhow!(
            "trust.oidc entry '{}' grant with session selector must include namespace and agent",
            entry_name
        ));
    }
    if !channel.is_empty() && namespace.is_empty() {
        return Err(anyhow!(
            "trust.oidc entry '{}' grant with channel selector must include namespace",
            entry_name
        ));
    }
    if !channel.is_empty() && (!agent.is_empty() || !session.is_empty()) {
        return Err(anyhow!(
            "trust.oidc entry '{}' grant cannot combine channel with agent or session selectors",
            entry_name
        ));
    }

    Ok(())
}

impl From<SchedulerConfigWrapper> for proto::SchedulerConfig {
    fn from(s: SchedulerConfigWrapper) -> Self {
        match s {
            SchedulerConfigWrapper::CloudTasks {
                project_id,
                location,
                queue,
                target_url,
                callback_auth,
            } => proto::SchedulerConfig {
                backend: Some(proto::scheduler_config::Backend::CloudTasks(
                    proto::CloudTasksSchedulerConfig {
                        project_id: project_id.unwrap_or_default(),
                        location: location.unwrap_or_default(),
                        queue: queue.unwrap_or_default(),
                        target_url: target_url.unwrap_or_default(),
                        callback_auth: callback_auth.map(Into::into),
                    },
                )),
            },
            SchedulerConfigWrapper::AwsEventBridgeScheduler {
                group_name,
                queue_url,
                execution_role_arn,
                schedule_name_prefix,
                dlq_arn,
                maximum_event_age_seconds,
                maximum_retry_attempts,
                endpoint_url,
            } => proto::SchedulerConfig {
                backend: Some(proto::scheduler_config::Backend::AwsEventbridgeScheduler(
                    proto::AwsEventBridgeSchedulerConfig {
                        group_name: group_name.unwrap_or_default(),
                        queue_url: queue_url.unwrap_or_default(),
                        execution_role_arn: execution_role_arn.unwrap_or_default(),
                        schedule_name_prefix: schedule_name_prefix.unwrap_or_default(),
                        dlq_arn: dlq_arn.unwrap_or_default(),
                        maximum_event_age_seconds: maximum_event_age_seconds.unwrap_or_default(),
                        maximum_retry_attempts,
                        endpoint_url: endpoint_url.unwrap_or_default(),
                    },
                )),
            },
        }
    }
}

impl From<ObjectStoreConfigWrapper> for proto::ObjectStoreConfig {
    fn from(s: ObjectStoreConfigWrapper) -> Self {
        match s {
            ObjectStoreConfigWrapper::Local { path } => Self {
                backend: Some(proto::object_store_config::Backend::Local(
                    proto::LocalObjectStoreConfig {
                        path: path.unwrap_or_default(),
                    },
                )),
            },
            ObjectStoreConfigWrapper::Gcs {
                bucket,
                prefix,
                api_base_url,
            } => Self {
                backend: Some(proto::object_store_config::Backend::Gcs(
                    proto::GcsObjectStoreConfig {
                        bucket,
                        prefix: prefix.unwrap_or_default(),
                        api_base_url: api_base_url.unwrap_or_default(),
                    },
                )),
            },
            ObjectStoreConfigWrapper::S3 {
                bucket,
                prefix,
                region,
                endpoint_url,
                force_path_style,
            } => Self {
                backend: Some(proto::object_store_config::Backend::S3(
                    proto::S3ObjectStoreConfig {
                        bucket,
                        prefix: prefix.unwrap_or_default(),
                        region: region.unwrap_or_default(),
                        endpoint_url: endpoint_url.unwrap_or_default(),
                        force_path_style: force_path_style.unwrap_or(false),
                    },
                )),
            },
        }
    }
}

impl From<SchedulerCallbackAuthConfigWrapper> for proto::SchedulerCallbackAuthConfig {
    fn from(s: SchedulerCallbackAuthConfigWrapper) -> Self {
        match s {
            SchedulerCallbackAuthConfigWrapper::SharedSecret { token } => Self {
                auth: Some(proto::scheduler_callback_auth_config::Auth::SharedSecret(
                    token.into(),
                )),
            },
            SchedulerCallbackAuthConfigWrapper::GoogleOidc {
                audience,
                service_account_email,
            } => Self {
                auth: Some(proto::scheduler_callback_auth_config::Auth::GoogleOidc(
                    proto::GoogleOidcAuthConfig {
                        audience,
                        service_account_email: service_account_email.unwrap_or_default(),
                    },
                )),
            },
        }
    }
}

impl From<TrustConfigWrapper> for proto::TrustConfig {
    fn from(s: TrustConfigWrapper) -> Self {
        Self {
            oidc: s.oidc.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<OidcTrustEntryConfigWrapper> for proto::OidcTrustEntry {
    fn from(mut s: OidcTrustEntryConfigWrapper) -> Self {
        if let Some(audience) = s.audience.take() {
            if !audience.trim().is_empty() {
                s.audiences.insert(0, audience);
            }
        }

        Self {
            name: s.name,
            issuer: s.issuer,
            audiences: s.audiences,
            allowed_domains: s.allowed_domains,
            allowed_emails: s.allowed_emails,
            jwks_url: s.jwks_url.unwrap_or_default(),
            clock_skew_seconds: s.clock_skew_seconds.unwrap_or_default(),
            grants: s.grants.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<OidcTrustGrantConfigWrapper> for proto::OidcTrustGrant {
    fn from(s: OidcTrustGrantConfigWrapper) -> Self {
        match s {
            OidcTrustGrantConfigWrapper::Read {
                namespace,
                agent,
                session,
                channel,
            } => Self {
                kind: proto::oidc_trust_grant::Kind::Read as i32,
                namespace,
                agent,
                session,
                channel,
            },
            OidcTrustGrantConfigWrapper::Readwrite {
                namespace,
                agent,
                session,
                channel,
            } => Self {
                kind: proto::oidc_trust_grant::Kind::Readwrite as i32,
                namespace,
                agent,
                session,
                channel,
            },
        }
    }
}

impl From<SerdeSecret> for Secret {
    fn from(s: SerdeSecret) -> Self {
        match s {
            SerdeSecret::Plain(p) => Secret {
                source: Some(proto::secret::Source::Plain(p)),
            },
            SerdeSecret::Ref(r) => {
                let source = match r.source.to_lowercase().as_str() {
                    "env" => proto::secret_ref::Source::Env,
                    "gcp" => proto::secret_ref::Source::Gcp,
                    "keychain" => proto::secret_ref::Source::Keychain,
                    "aws" => proto::secret_ref::Source::Aws,
                    "azure" => proto::secret_ref::Source::Azure,
                    _ => proto::secret_ref::Source::Env,
                };
                Secret {
                    source: Some(proto::secret::Source::Ref(proto::SecretRef {
                        source: source as i32,
                        key: r.key,
                    })),
                }
            }
        }
    }
}

pub trait ConfigExt {
    fn from_file<P: AsRef<Path>>(path: P) -> Result<Config>;
    fn load_default() -> Result<Config>;
    fn decode_binary(data: &[u8]) -> Result<Config>;
}

const MAX_CONFIG_EXTENDS_DEPTH: usize = 32;

impl ConfigExt for Config {
    fn from_file<P: AsRef<Path>>(path: P) -> Result<Config> {
        let path = path.as_ref();
        let mut stack = Vec::new();
        let value = load_config_value(path, &mut stack, 0)?;
        let serde_config: SerdeConfig = serde_json::from_value(value).map_err(|error| {
            anyhow!("Failed to deserialize merged Talon configuration: {error}")
        })?;
        validate_trust_config(&serde_config)?;
        validate_provider_config(&serde_config)?;
        Ok(serde_config.into())
    }

    fn load_default() -> Result<Config> {
        if let Ok(inline_yaml) = env::var("TALON_CONFIG_INLINE_YAML") {
            if !inline_yaml.trim().is_empty() {
                let inline_yaml = expand_env_placeholders(&inline_yaml);
                let mut value: Value = serde_yaml::from_str(&inline_yaml)
                    .context("Failed to parse TALON_CONFIG_INLINE_YAML")?;
                normalize_config_document(&mut value)?;

                // Treat the process working directory as the base for relative
                // inline extensions. Absolute paths are useful in container
                // images where a large shared catalog can be baked into a
                // read-only layer while deployment-specific overrides remain
                // in the environment.
                let inline_path = Path::new("TALON_CONFIG_INLINE_YAML.yaml");
                let extends = take_extends(&mut value, inline_path)?;
                let mut merged = Value::Object(Map::new());
                let mut stack = Vec::new();
                for parent in extends {
                    let parent_path = resolve_extend_path(inline_path, &parent)?;
                    let parent_value =
                        load_config_value(&parent_path, &mut stack, 1).map_err(|error| {
                            anyhow!(
                                "While loading inline extension '{}': {error}",
                                parent_path.display()
                            )
                        })?;
                    merge_config_values(&mut merged, parent_value);
                }
                merge_config_values(&mut merged, value);

                let serde_config: SerdeConfig =
                    serde_json::from_value(merged).map_err(|error| {
                        anyhow!("Failed to deserialize TALON_CONFIG_INLINE_YAML: {error}")
                    })?;
                validate_trust_config(&serde_config)?;
                validate_provider_config(&serde_config)?;
                return Ok(serde_config.into());
            }
        }

        if let Ok(explicit_path) = env::var("TALON_CONFIG_PATH") {
            if !explicit_path.trim().is_empty() {
                return Self::from_file(explicit_path);
            }
        }

        let paths = [
            "config.yaml",
            "config.yml",
            "config.json",
            "config.toml",
            "talon.yaml",
            "talon.yml",
            "talon.json",
            "talon.toml",
        ];
        for path in paths {
            if Path::new(path).exists() {
                return Self::from_file(path);
            }
        }

        Err(anyhow!("No configuration file found"))
    }

    fn decode_binary(data: &[u8]) -> Result<Config> {
        Config::decode(data).map_err(|e| anyhow!("Failed to decode binary config: {}", e))
    }
}

fn load_config_value(path: &Path, stack: &mut Vec<PathBuf>, depth: usize) -> Result<Value> {
    if depth > MAX_CONFIG_EXTENDS_DEPTH {
        return Err(anyhow!(
            "configuration extends nesting exceeds maximum depth of {} (chain: {})",
            MAX_CONFIG_EXTENDS_DEPTH,
            format_config_chain(stack, path)
        ));
    }

    let canonical_path = std::fs::canonicalize(path)
        .with_context(|| format!("Failed to resolve config file '{}'", path.display()))?;
    if let Some(cycle_start) = stack.iter().position(|entry| entry == &canonical_path) {
        let mut cycle = stack[cycle_start..].to_vec();
        cycle.push(canonical_path);
        return Err(anyhow!(
            "configuration extends cycle detected: {}",
            cycle
                .iter()
                .map(|entry| entry.display().to_string())
                .collect::<Vec<_>>()
                .join(" -> ")
        ));
    }

    stack.push(canonical_path);
    let result = (|| {
        let content = expand_env_placeholders(
            &std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read config file '{}'", path.display()))?,
        );
        let mut value = parse_config_value(path, &content)?;
        normalize_config_document(&mut value)?;

        let extends = take_extends(&mut value, path)?;
        let mut merged = Value::Object(Map::new());
        for parent in extends {
            let parent_path = resolve_extend_path(path, &parent)?;
            let parent_value =
                load_config_value(&parent_path, stack, depth + 1).map_err(|error| {
                    anyhow!(
                        "While loading extension '{}': {error}",
                        parent_path.display()
                    )
                })?;
            merge_config_values(&mut merged, parent_value);
        }

        resolve_config_relative_paths_value(path, &mut value);
        merge_config_values(&mut merged, value);
        Ok(merged)
    })();
    stack.pop();
    result
}

fn parse_config_value(path: &Path, content: &str) -> Result<Value> {
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("toml");
    match extension {
        "toml" => {
            let value: toml::Value = toml::from_str(content)
                .with_context(|| format!("Failed to parse TOML config '{}'", path.display()))?;
            serde_json::to_value(value).context("Failed to convert TOML config to JSON")
        }
        "yaml" | "yml" => serde_yaml::from_str(content)
            .with_context(|| format!("Failed to parse YAML config '{}'", path.display())),
        "json" => serde_json::from_str(content)
            .with_context(|| format!("Failed to parse JSON config '{}'", path.display())),
        _ => Err(anyhow!("Unsupported config format: {}", extension)),
    }
}

fn take_extends(value: &mut Value, path: &Path) -> Result<Vec<String>> {
    let Some(object) = value.as_object_mut() else {
        return Err(anyhow!(
            "Config file '{}' must contain a top-level object",
            path.display()
        ));
    };
    let Some(extends) = object.remove("extends") else {
        return Ok(Vec::new());
    };

    match extends {
        Value::String(path) if !path.trim().is_empty() => Ok(vec![path.trim().to_string()]),
        Value::Array(paths) => paths
            .into_iter()
            .map(|path_value| match path_value {
                Value::String(path) if !path.trim().is_empty() => Ok(path.trim().to_string()),
                _ => Err(anyhow!(
                    "Config file '{}' has an 'extends' list that must contain only non-empty path strings",
                    path.display()
                )),
            })
            .collect(),
        Value::Null => Ok(Vec::new()),
        _ => Err(anyhow!(
            "Config file '{}' must define 'extends' as a path string or list of path strings",
            path.display()
        )),
    }
}

fn resolve_extend_path(path: &Path, extend: &str) -> Result<PathBuf> {
    if extend.contains("://") {
        return Err(anyhow!(
            "Config extension '{}' is not a local file path; remote URLs are not supported",
            extend
        ));
    }

    let extend_path = Path::new(extend);
    Ok(if extend_path.is_absolute() {
        extend_path.to_path_buf()
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .join(extend_path)
    })
}

fn merge_config_values(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                if let Some(existing) = base.get_mut(&key) {
                    merge_config_values(existing, value);
                } else {
                    base.insert(key, value);
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

fn normalize_config_document(value: &mut Value) -> Result<()> {
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };

    merge_top_level_alias(object, "providers", "llmProviders");
    merge_top_level_alias(object, "models", "llmModels");
    merge_top_level_alias(object, "models", "modelLimits");
    Ok(())
}

fn merge_top_level_alias(object: &mut Map<String, Value>, canonical: &str, alias: &str) {
    let Some(alias_value) = object.remove(alias) else {
        return;
    };
    if let Some(canonical_value) = object.get_mut(canonical) {
        merge_config_values(canonical_value, alias_value);
    } else {
        object.insert(canonical.to_string(), alias_value);
    }
}

fn resolve_config_relative_paths_value(path: &Path, value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    resolve_raw_path_at(path, object, &["workspace_dir"]);
    resolve_raw_path_at(path, object, &["database", "data_dir"]);
    resolve_raw_path_at(path, object, &["control_plane", "database", "data_dir"]);
    resolve_raw_path_at(path, object, &["control_plane", "documents", "data_dir"]);
    resolve_raw_path_at(path, object, &["control_plane", "object_store", "path"]);
    resolve_raw_path_at(path, object, &["storage", "control", "data_dir"]);
    resolve_raw_path_at(path, object, &["storage", "data", "data_dir"]);
    resolve_raw_path_at(path, object, &["storage", "documents", "data_dir"]);
    resolve_raw_path_at(path, object, &["storage", "objects", "path"]);
}

fn resolve_raw_path_at(path: &Path, object: &mut Map<String, Value>, segments: &[&str]) {
    let Some(value) = value_at_mut(object, segments) else {
        return;
    };
    let Some(raw) = value.as_str() else {
        return;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() || Path::new(trimmed).is_absolute() {
        return;
    }
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let resolved = normalize_path(base_dir.join(trimmed));
    *value = Value::String(resolved.display().to_string());
}

fn value_at_mut<'a>(
    object: &'a mut Map<String, Value>,
    segments: &[&str],
) -> Option<&'a mut Value> {
    let (first, rest) = segments.split_first()?;
    let mut current = object.get_mut(*first)?;
    for segment in rest {
        current = current.as_object_mut()?.get_mut(*segment)?;
    }
    Some(current)
}

fn format_config_chain(stack: &[PathBuf], next: &Path) -> String {
    stack
        .iter()
        .map(|entry| entry.display().to_string())
        .chain(std::iter::once(next.display().to_string()))
        .collect::<Vec<_>>()
        .join(" -> ")
}

#[cfg(test)]
mod tests;
