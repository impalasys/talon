// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use prost::Message;
use serde::Deserialize;
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
    DatabaseConfig, LlmProviderConfig as ProviderConfig, SchedulerConfig, SecretRef, ServerConfig,
    TalonConfig as Config,
};
pub use secrets::{Secret, SecretExt};

// Interop with Serde for file-based config
#[derive(Debug, Deserialize, Clone)]
pub struct SerdeConfig {
    #[serde(default)]
    pub providers: HashMap<String, SerdeProviderConfig>,
    pub database: Option<DatabaseConfigWrapper>,
    pub server: Option<ServerConfigWrapper>,
    pub default_provider: Option<String>,
    pub workspace_dir: Option<String>,
    pub control_plane: Option<ControlPlaneConfigWrapper>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SerdeProviderConfig {
    Openai {
        model: String,
        api_key: SerdeSecret,
    },
    Anthropic {
        model: String,
        api_key: SerdeSecret,
    },
    Google {
        model: String,
        api_key: SerdeSecret,
    },
    OpenaiCompatible {
        base_url: String,
        model: String,
        api_key: SerdeSecret,
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

impl From<SerdeConfig> for Config {
    fn from(s: SerdeConfig) -> Self {
        let providers = s
            .providers
            .into_iter()
            .map(|(name, p)| {
                let p_proto = match p {
                    SerdeProviderConfig::Openai { model, api_key } => ProviderConfig {
                        config: Some(proto::llm_provider_config::Config::Openai(
                            proto::OpenAiConfig {
                                model,
                                api_key: Some(api_key.into()),
                                org_id: "".to_string(),
                            },
                        )),
                    },
                    SerdeProviderConfig::Anthropic { model, api_key } => ProviderConfig {
                        config: Some(proto::llm_provider_config::Config::Anthropic(
                            proto::AnthropicConfig {
                                model,
                                api_key: Some(api_key.into()),
                            },
                        )),
                    },
                    SerdeProviderConfig::Google { model, api_key } => ProviderConfig {
                        config: Some(proto::llm_provider_config::Config::Google(
                            proto::GoogleConfig {
                                model,
                                api_key: Some(api_key.into()),
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
                                api_key: Some(api_key.into()),
                            },
                        )),
                    },
                };
                (name, p_proto)
            })
            .collect();

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
            control_plane: s.control_plane.map(|cp| proto::ControlPlaneConfig {
                database: Some(proto::DatabaseConfig {
                    data_dir: cp.database.data_dir.unwrap_or_default(),
                    driver: cp.database.driver.unwrap_or_default(),
                    url: cp.database.url.map(Into::into),
                }),
                message_broker: Some(proto::MessageBrokerConfig {
                    driver: cp.message_broker.driver,
                }),
                scheduler: cp.scheduler.map(Into::into),
            }),
        }
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn resolve_config_relative_data_dir(path: &Path, data_dir: &mut Option<String>) {
    let Some(raw) = data_dir.as_ref() else {
        return;
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }

    let dir = Path::new(trimmed);
    if dir.is_absolute() {
        return;
    }

    let base_dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    *data_dir = Some(normalize_path(base_dir.join(dir)).display().to_string());
}

fn resolve_config_relative_string_path(path: &Path, value: &mut Option<String>) {
    let Some(raw) = value.as_ref() else {
        return;
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }

    let resolved = if Path::new(trimmed).is_absolute() {
        PathBuf::from(trimmed)
    } else {
        let base_dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        normalize_path(base_dir.join(trimmed))
    };
    *value = Some(resolved.display().to_string());
}

fn resolve_config_relative_paths(path: &Path, config: &mut SerdeConfig) {
    if let Some(database) = config.database.as_mut() {
        resolve_config_relative_data_dir(path, &mut database.data_dir);
    }

    if let Some(control_plane) = config.control_plane.as_mut() {
        resolve_config_relative_data_dir(path, &mut control_plane.database.data_dir);
    }

    resolve_config_relative_string_path(path, &mut config.workspace_dir);
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

impl ConfigExt for Config {
    fn from_file<P: AsRef<Path>>(path: P) -> Result<Config> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)?;
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("toml");

        let mut serde_config: SerdeConfig = match extension {
            "toml" => toml::from_str(&content)?,
            "yaml" | "yml" => serde_yaml::from_str(&content)?,
            "json" => serde_json::from_str(&content)?,
            _ => return Err(anyhow!("Unsupported config format: {}", extension)),
        };
        resolve_config_relative_paths(path, &mut serde_config);
        Ok(serde_config.into())
    }

    fn load_default() -> Result<Config> {
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

#[cfg(test)]
mod tests;
