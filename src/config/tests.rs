// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[cfg(test)]
mod tests {
    use crate::config::{proto, Config, ConfigExt, Secret, SecretExt};
    use prost::Message;
    use std::env;
    use std::io::Write;
    use tempfile::tempdir;
    use tempfile::NamedTempFile;

    #[test]
    fn test_config_from_yaml() {
        let file = NamedTempFile::new().expect("Failed to create temp file");
        let path = file.path().with_extension("yaml");
        let mut file = std::fs::File::create(&path).expect("Failed to create yaml file");

        writeln!(
            file,
            r#"
providers:
  my-novita:
    type: openai_compatible
    base_url: "https://api.novita.ai/v3"
    model: minimax-m2.7
    api_key: "direct-key"

database:
  data_dir: "./test-data"

server:
  host: "0.0.0.0"
  port: 9000
"#
        )
        .unwrap();

        let config = Config::from_file(&path).unwrap();
        assert_eq!(config.providers.len(), 1);

        let novita = config.providers.get("my-novita").unwrap();
        if let Some(proto::llm_provider_config::Config::OpenaiCompatible(c)) = &novita.config {
            assert_eq!(c.model, "minimax-m2.7");
            if let Some(api_key) = &c.api_key {
                match &api_key.source {
                    Some(proto::secret::Source::Plain(s)) => assert_eq!(s, "direct-key"),
                    _ => panic!("Expected Plain secret"),
                }
            }
        }
    }

    #[test]
    fn test_binary_decode() {
        let mut config = Config::default();
        config.providers.insert(
            "test".to_string(),
            crate::config::ProviderConfig {
                config: Some(proto::llm_provider_config::Config::Openai(
                    proto::OpenAiConfig {
                        model: "m1".to_string(),
                        api_key: Some(Secret {
                            source: Some(proto::secret::Source::Plain("key".to_string())),
                        }),
                        org_id: "".to_string(),
                    },
                )),
            },
        );
        config.database = Some(proto::DatabaseConfig {
            data_dir: "dir".to_string(),
            driver: "".to_string(),
            url: None,
        });
        config.server = Some(proto::ServerConfig {
            host: "h".to_string(),
            port: 123,
        });

        let mut buf = Vec::new();
        config.encode(&mut buf).unwrap();

        let decoded = Config::decode_binary(&buf).unwrap();
        assert_eq!(decoded.providers.len(), 1);
    }

    #[tokio::test]
    async fn test_secret_resolution() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
        unsafe {
            std::env::set_var("MY_TEST_SECRET", "resolved-value");
        }
        let secret = Secret {
            source: Some(proto::secret::Source::Ref(proto::SecretRef {
                source: proto::secret_ref::Source::Env as i32,
                key: "MY_TEST_SECRET".to_string(),
            })),
        };

        let value = secret.resolve().await.unwrap();
        assert_eq!(value, "resolved-value");
        unsafe {
            std::env::remove_var("MY_TEST_SECRET");
        }
    }

    #[tokio::test]
    async fn test_secret_resolution_reports_local_error_paths() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
        unsafe {
            std::env::remove_var("MISSING_TEST_SECRET");
        }

        let plain = Secret {
            source: Some(proto::secret::Source::Plain("plain-secret".to_string())),
        };
        assert_eq!(plain.resolve().await.unwrap(), "plain-secret");

        let missing_source = Secret { source: None };
        assert!(missing_source
            .resolve()
            .await
            .unwrap_err()
            .to_string()
            .contains("Secret source missing"));

        let invalid_source = Secret {
            source: Some(proto::secret::Source::Ref(proto::SecretRef {
                source: 999,
                key: "ignored".to_string(),
            })),
        };
        assert!(invalid_source
            .resolve()
            .await
            .unwrap_err()
            .to_string()
            .contains("Invalid secret source"));

        let missing_env = Secret {
            source: Some(proto::secret::Source::Ref(proto::SecretRef {
                source: proto::secret_ref::Source::Env as i32,
                key: "MISSING_TEST_SECRET".to_string(),
            })),
        };
        assert!(missing_env
            .resolve()
            .await
            .unwrap_err()
            .to_string()
            .contains("Env var MISSING_TEST_SECRET not set"));

        let malformed_gcp = Secret {
            source: Some(proto::secret::Source::Ref(proto::SecretRef {
                source: proto::secret_ref::Source::Gcp as i32,
                key: "secret-name-only".to_string(),
            })),
        };
        assert!(malformed_gcp
            .resolve()
            .await
            .unwrap_err()
            .to_string()
            .contains("projects/PROJECT/secrets/NAME/versions/VERSION"));

        let malformed_azure = Secret {
            source: Some(proto::secret::Source::Ref(proto::SecretRef {
                source: proto::secret_ref::Source::Azure as i32,
                key: "vault-only".to_string(),
            })),
        };
        assert!(malformed_azure
            .resolve()
            .await
            .unwrap_err()
            .to_string()
            .contains("vault-name/secret-name"));

        let keychain = Secret {
            source: Some(proto::secret::Source::Ref(proto::SecretRef {
                source: proto::secret_ref::Source::Keychain as i32,
                key: "definitely-missing-test-key".to_string(),
            })),
        };
        let keychain_err = keychain.resolve().await.unwrap_err().to_string();
        assert!(
            keychain_err.contains("Keychain error")
                || keychain_err.contains("No such file or directory")
                || keychain_err.contains("not found"),
            "unexpected keychain error: {keychain_err}"
        );
    }

    #[test]
    fn test_config_from_toml_and_json_and_unsupported_extension() {
        let dir = tempdir().unwrap();

        let toml_path = dir.path().join("config.toml");
        std::fs::write(
            &toml_path,
            r#"
default_provider = "primary"
workspace_dir = "/workspace"

[providers.primary]
type = "openai"
model = "gpt-4.1"
api_key = "secret"
"#,
        )
        .unwrap();
        let toml = Config::from_file(&toml_path).unwrap();
        assert_eq!(toml.default_provider, "primary");
        assert_eq!(toml.workspace_dir, "/workspace");
        assert!(toml.providers.contains_key("primary"));

        let json_path = dir.path().join("config.json");
        std::fs::write(
            &json_path,
            r#"{
  "providers": {
    "anthropic": {
      "type": "anthropic",
      "model": "claude",
      "api_key": {"source": "env", "key": "ANTHROPIC_API_KEY"}
    }
  },
  "server": {"host": "127.0.0.1", "port": 8080}
}"#,
        )
        .unwrap();
        let json = Config::from_file(&json_path).unwrap();
        assert!(json.providers.contains_key("anthropic"));
        assert_eq!(json.server.unwrap().port, 8080);

        let txt_path = dir.path().join("config.txt");
        std::fs::write(&txt_path, "invalid").unwrap();
        let err = Config::from_file(&txt_path).unwrap_err();
        assert!(err.to_string().contains("Unsupported config format"));
    }

    #[test]
    fn test_load_default_uses_env_override_and_fallback_search() {
        let _guard = crate::test_support::env_lock();
        let dir = tempdir().unwrap();
        let original_dir = env::current_dir().unwrap();

        let explicit = dir.path().join("explicit.yaml");
        std::fs::write(
            &explicit,
            "providers:\n  explicit:\n    type: google\n    model: gemini\n    api_key: direct\n",
        )
        .unwrap();

        unsafe {
            env::set_var("TALON_CONFIG_PATH", explicit.as_os_str());
        }
        let explicit_cfg = Config::load_default().unwrap();
        assert!(explicit_cfg.providers.contains_key("explicit"));

        unsafe {
            env::remove_var("TALON_CONFIG_PATH");
        }
        env::set_current_dir(dir.path()).unwrap();
        std::fs::write(
            dir.path().join("talon.json"),
            r#"{"providers":{"fallback":{"type":"openai_compatible","base_url":"https://example.com","model":"demo","api_key":"x"}}}"#,
        )
        .unwrap();

        let fallback_cfg = Config::load_default().unwrap();
        assert!(fallback_cfg.providers.contains_key("fallback"));

        env::set_current_dir(&original_dir).unwrap();
    }

    #[test]
    fn test_load_default_errors_when_nothing_is_available() {
        let _guard = crate::test_support::env_lock();
        let dir = tempdir().unwrap();
        let original_dir = env::current_dir().unwrap();
        unsafe {
            env::remove_var("TALON_CONFIG_PATH");
        }
        env::set_current_dir(dir.path()).unwrap();

        let err = Config::load_default().unwrap_err();
        assert!(err.to_string().contains("No configuration file found"));

        env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_serde_config_into_proto_preserves_scheduler_and_secret_variants() {
        let serde = crate::config::SerdeConfig {
            providers: std::collections::HashMap::from([
                (
                    "primary".to_string(),
                    crate::config::SerdeProviderConfig::OpenaiCompatible {
                        base_url: "https://llm.example.com".to_string(),
                        model: "model-x".to_string(),
                        api_key: crate::config::SerdeSecret::Ref(crate::config::SerdeSecretRef {
                            source: "azure".to_string(),
                            key: "azure-key".to_string(),
                        }),
                    },
                ),
                (
                    "backup".to_string(),
                    crate::config::SerdeProviderConfig::Google {
                        model: "gemini".to_string(),
                        api_key: crate::config::SerdeSecret::Plain("plain".to_string()),
                    },
                ),
            ]),
            database: Some(crate::config::DatabaseConfigWrapper {
                data_dir: Some("./data".to_string()),
                driver: Some("sqlite".to_string()),
                url: Some(crate::config::SerdeSecret::Plain(
                    "postgres://db".to_string(),
                )),
            }),
            server: Some(crate::config::ServerConfigWrapper {
                host: "0.0.0.0".to_string(),
                port: 7000,
            }),
            default_provider: Some("primary".to_string()),
            workspace_dir: None,
            control_plane: Some(crate::config::ControlPlaneConfigWrapper {
                database: crate::config::DatabaseConfigWrapper {
                    data_dir: None,
                    driver: Some("postgres".to_string()),
                    url: Some(crate::config::SerdeSecret::Ref(
                        crate::config::SerdeSecretRef {
                            source: "keychain".to_string(),
                            key: "db-url".to_string(),
                        },
                    )),
                },
                message_broker: crate::config::MessageBrokerConfigWrapper {
                    driver: "pubsub".to_string(),
                    extra: Default::default(),
                },
                scheduler: Some(crate::config::SchedulerConfigWrapper::CloudTasks {
                    project_id: Some("project".to_string()),
                    location: Some("us-central1".to_string()),
                    queue: Some("talon".to_string()),
                    target_url: Some("https://worker.example.com".to_string()),
                    callback_auth: Some(
                        crate::config::SchedulerCallbackAuthConfigWrapper::GoogleOidc {
                            audience: "https://worker.example.com".to_string(),
                            service_account_email: Some("svc@example.com".to_string()),
                        },
                    ),
                }),
            }),
        };

        let config: Config = serde.into();
        assert_eq!(config.workspace_dir, ".");
        assert_eq!(config.default_provider, "primary");
        assert_eq!(config.server.as_ref().unwrap().port, 7000);
        assert_eq!(
            config
                .control_plane
                .as_ref()
                .unwrap()
                .message_broker
                .as_ref()
                .unwrap()
                .driver,
            "pubsub"
        );

        let provider = config.providers.get("primary").unwrap();
        let Some(proto::llm_provider_config::Config::OpenaiCompatible(generic)) = &provider.config
        else {
            panic!("expected openai compatible provider");
        };
        assert_eq!(generic.base_url, "https://llm.example.com");
        let Some(proto::secret::Source::Ref(secret_ref)) =
            &generic.api_key.as_ref().unwrap().source
        else {
            panic!("expected secret ref");
        };
        assert_eq!(secret_ref.source, proto::secret_ref::Source::Azure as i32);

        let scheduler = config.control_plane.unwrap().scheduler.unwrap();
        let Some(proto::scheduler_config::Backend::CloudTasks(cloud)) = scheduler.backend else {
            panic!("expected cloud tasks scheduler");
        };
        assert_eq!(cloud.project_id, "project");
        let Some(proto::scheduler_callback_auth_config::Auth::GoogleOidc(oidc)) =
            cloud.callback_auth.unwrap().auth
        else {
            panic!("expected oidc auth");
        };
        assert_eq!(oidc.service_account_email, "svc@example.com");
    }

    #[test]
    fn test_secret_from_serde_secret_maps_known_and_unknown_sources() {
        let plain: Secret = crate::config::SerdeSecret::Plain("value".to_string()).into();
        let Some(proto::secret::Source::Plain(value)) = plain.source else {
            panic!("expected plain secret");
        };
        assert_eq!(value, "value");

        let env_ref: Secret = crate::config::SerdeSecret::Ref(crate::config::SerdeSecretRef {
            source: "unknown".to_string(),
            key: "NAME".to_string(),
        })
        .into();
        let Some(proto::secret::Source::Ref(secret_ref)) = env_ref.source else {
            panic!("expected secret ref");
        };
        assert_eq!(secret_ref.source, proto::secret_ref::Source::Env as i32);
        assert_eq!(secret_ref.key, "NAME");
    }
}
