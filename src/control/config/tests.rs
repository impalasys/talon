// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[cfg(test)]
mod tests {
    use crate::control::config::{
        expand_env_placeholders, normalize_path, proto, Config, ConfigExt, Secret, SecretExt,
    };
    use prost::Message;
    use std::env;
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir;
    use tempfile::NamedTempFile;

    fn copy_checked_in_model_catalog(destination: &Path) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("models");
        let destination = destination.join("models");
        std::fs::create_dir_all(&destination).unwrap();

        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                std::fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
            }
        }
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = env::var_os(key);
            // Tests serialize environment access through env_lock/async_env_mutex.
            unsafe {
                env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = env::var_os(key);
            // Tests serialize environment access through env_lock/async_env_mutex.
            unsafe {
                env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            // Tests serialize environment access through env_lock/async_env_mutex.
            unsafe {
                if let Some(previous) = &self.previous {
                    env::set_var(self.key, previous);
                } else {
                    env::remove_var(self.key);
                }
            }
        }
    }

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

models:
  minimax-m2.7:
    contextWindow: 32768
    maxOutputTokens: 4096
    inputCostPerMillionTokens: 0.30
    outputCostPerMillionTokens: 1.20
    longContextTokens: 272000
    longContextInputCostPerMillionTokens: 0.60
    longContextOutputCostPerMillionTokens: 1.80

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
        assert_eq!(
            config.database.as_ref().unwrap().data_dir,
            path.parent()
                .unwrap()
                .join("test-data")
                .display()
                .to_string()
        );

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

        let model = config.models.get("minimax-m2.7").unwrap();
        assert_eq!(model.context_window_tokens, Some(32768));
        assert_eq!(model.max_output_tokens, Some(4096));
        assert_eq!(model.input_cost_per_million_tokens, Some(0.30));
        assert_eq!(model.output_cost_per_million_tokens, Some(1.20));
        assert_eq!(model.long_context_tokens, Some(272000));
        assert_eq!(model.long_context_input_cost_per_million_tokens, Some(0.60));
        assert_eq!(
            model.long_context_output_cost_per_million_tokens,
            Some(1.80)
        );
    }

    #[test]
    fn test_config_from_yaml_parses_oidc_trust_grants() {
        let file = NamedTempFile::new().expect("Failed to create temp file");
        let path = file.path().with_extension("yaml");
        let mut file = std::fs::File::create(&path).expect("Failed to create yaml file");

        writeln!(
            file,
            r#"
trust:
  oidc:
    - name: google-admins
      issuer: https://accounts.google.com
      audiences:
        - web-client-id.apps.googleusercontent.com
        - cli-client-id.apps.googleusercontent.com
      allowedDomains:
        - impala.systems
      allowedEmails:
        - alice@impala.systems
      jwksUrl: https://www.googleapis.com/oauth2/v3/certs
      clockSkewSeconds: 60
      grants:
        - kind: readwrite
        - kind: read
          namespace: Support
        - kind: readwrite
          namespace: Support
          agent: retention-reviewer
        - kind: read
          namespace: Support
          agent: retention-reviewer
          session: session-123
        - kind: readwrite
          namespace: Support
          channel: incident-room
"#
        )
        .unwrap();

        let config = Config::from_file(&path).unwrap();
        let trust = config.trust.as_ref().unwrap();
        assert_eq!(trust.oidc.len(), 1);
        let entry = &trust.oidc[0];
        assert_eq!(entry.name, "google-admins");
        assert_eq!(entry.issuer, "https://accounts.google.com");
        assert_eq!(
            entry.audiences,
            vec![
                "web-client-id.apps.googleusercontent.com".to_string(),
                "cli-client-id.apps.googleusercontent.com".to_string()
            ]
        );
        assert_eq!(entry.allowed_domains, vec!["impala.systems".to_string()]);
        assert_eq!(
            entry.allowed_emails,
            vec!["alice@impala.systems".to_string()]
        );
        assert_eq!(entry.clock_skew_seconds, 60);
        assert_eq!(entry.grants.len(), 5);
        assert_eq!(
            entry.grants[0].kind,
            proto::oidc_trust_grant::Kind::Readwrite as i32
        );
        assert_eq!(entry.grants[0].namespace, "");
        assert_eq!(
            entry.grants[3].kind,
            proto::oidc_trust_grant::Kind::Read as i32
        );
        assert_eq!(entry.grants[3].namespace, "Support");
        assert_eq!(entry.grants[3].agent, "retention-reviewer");
        assert_eq!(entry.grants[3].session, "session-123");
        assert_eq!(entry.grants[4].channel, "incident-room");
    }

    #[test]
    fn test_checked_in_compose_config_parses_current_shape() {
        let _guard = crate::test_support::env_lock();
        let _desktop_client = EnvVarGuard::set(
            "TALON_GOOGLE_CLIENT_ID",
            "test-desktop-client.apps.googleusercontent.com",
        );
        let _web_client = EnvVarGuard::set(
            "TALON_GOOGLE_WEB_CLIENT_ID",
            "test-web-client.apps.googleusercontent.com",
        );
        let dir = tempdir().unwrap();
        let path = dir.path().join("talon.docker-compose.yaml");
        std::fs::write(&path, include_str!("../../../talon.docker-compose.yaml")).unwrap();
        std::fs::write(
            dir.path().join("models.yaml"),
            include_str!("../../../models.yaml"),
        )
        .unwrap();
        copy_checked_in_model_catalog(dir.path());
        let config = Config::from_file(&path).unwrap();

        assert!(config.providers.contains_key("openai"));
        let openai = config.providers.get("openai").unwrap();
        assert!(matches!(
            &openai.config,
            Some(proto::llm_provider_config::Config::Openai(_))
        ));

        let control_plane = config.control_plane.as_ref().unwrap();
        assert_eq!(control_plane.database.as_ref().unwrap().driver, "postgres");
        let url = control_plane
            .database
            .as_ref()
            .unwrap()
            .url
            .as_ref()
            .unwrap();
        let Some(proto::secret::Source::Ref(url_ref)) = &url.source else {
            panic!("expected control database URL to be an env ref");
        };
        assert_eq!(url_ref.key, "TALON_CONTROL_DATABASE_URL");
        let document_url = control_plane
            .documents
            .as_ref()
            .unwrap()
            .url
            .as_ref()
            .unwrap();
        let Some(proto::secret::Source::Ref(document_url_ref)) = &document_url.source else {
            panic!("expected document database URL to be an env ref");
        };
        assert_eq!(document_url_ref.key, "TALON_DOCUMENT_DATABASE_URL");
        assert_eq!(
            control_plane.message_broker.as_ref().unwrap().driver,
            "gcp_pubsub"
        );
        assert!(matches!(
            &control_plane.object_store.as_ref().unwrap().backend,
            Some(proto::object_store_config::Backend::Local(_))
        ));

        let trust = config.trust.as_ref().unwrap();
        assert_eq!(trust.oidc[0].name, "google-admins");
        assert_eq!(
            trust.oidc[0].audiences,
            vec![
                "test-desktop-client.apps.googleusercontent.com".to_string(),
                "test-web-client.apps.googleusercontent.com".to_string()
            ]
        );
        assert_eq!(
            trust.oidc[0].grants[0].kind,
            proto::oidc_trust_grant::Kind::Readwrite as i32
        );
        assert_eq!(
            config
                .models
                .get("openai/gpt-5.4-nano")
                .unwrap()
                .context_window_tokens,
            Some(400000)
        );
        assert_eq!(
            config
                .models
                .get("novita/google/gemma-4-31b-it")
                .unwrap()
                .context_window_tokens,
            Some(262144)
        );
        assert_eq!(
            config
                .models
                .get("meta/muse-spark-1.1")
                .unwrap()
                .context_window_tokens,
            Some(1048576)
        );
        assert_eq!(
            config
                .models
                .get("meta/muse-spark-1.1")
                .unwrap()
                .max_output_tokens,
            Some(131072)
        );
    }

    #[test]
    fn test_checked_in_talon_config_loads_shared_model_catalog() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("talon.yaml");
        std::fs::write(&path, include_str!("../../../talon.yaml")).unwrap();
        std::fs::write(
            dir.path().join("models.yaml"),
            include_str!("../../../models.yaml"),
        )
        .unwrap();
        copy_checked_in_model_catalog(dir.path());

        let config = Config::from_file(&path).unwrap();
        assert!(config.providers.contains_key("openai"));
        assert!(config.models.len() >= 17);
        assert_eq!(
            config.models.get("openai/gpt-5").unwrap().provider,
            "openai"
        );
    }

    #[test]
    fn test_env_placeholder_expansion() {
        let _guard = crate::test_support::env_lock();
        let _placeholder = EnvVarGuard::set("TALON_TEST_PLACEHOLDER", "expanded");
        let _missing = EnvVarGuard::remove("TALON_MISSING_PLACEHOLDER");

        assert_eq!(
            expand_env_placeholders(
                "before ${TALON_TEST_PLACEHOLDER} ${TALON_MISSING_PLACEHOLDER} ${bad-name"
            ),
            "before expanded ${TALON_MISSING_PLACEHOLDER} ${bad-name"
        );
    }

    #[test]
    fn test_config_rejects_invalid_oidc_trust_grants() {
        let dir = tempdir().unwrap();
        let cases = [
            (
                "agent_without_namespace.yaml",
                r#"
trust:
  oidc:
    - name: bad
      issuer: https://accounts.google.com
      audiences: [client]
      grants:
        - kind: read
          agent: retention-reviewer
"#,
                "agent selector must include namespace",
            ),
            (
                "session_without_agent.yaml",
                r#"
trust:
  oidc:
    - name: bad
      issuer: https://accounts.google.com
      audiences: [client]
      grants:
        - kind: readwrite
          namespace: Support
          session: session-123
"#,
                "session selector must include namespace and agent",
            ),
            (
                "channel_with_agent.yaml",
                r#"
trust:
  oidc:
    - name: bad
      issuer: https://accounts.google.com
      audiences: [client]
      grants:
        - kind: read
          namespace: Support
          agent: retention-reviewer
          channel: incident-room
"#,
                "cannot combine channel with agent or session selectors",
            ),
            (
                "unknown_kind.yaml",
                r#"
trust:
  oidc:
    - name: bad
      issuer: https://accounts.google.com
      audiences: [client]
      grants:
        - kind: admin
"#,
                "unknown variant",
            ),
        ];

        for (filename, yaml, expected) in cases {
            let path = dir.path().join(filename);
            std::fs::write(&path, yaml).unwrap();
            let err = Config::from_file(&path).unwrap_err().to_string();
            assert!(
                err.contains(expected),
                "expected '{expected}' in error for {filename}, got: {err}"
            );
        }
    }

    #[test]
    fn test_native_openai_api_is_parsed_and_validated() {
        let dir = tempdir().unwrap();
        let valid = dir.path().join("valid.yaml");
        std::fs::write(
            &valid,
            "providers:\n  openai:\n    type: openai\n    model: gpt-5.6-luna\n    api: responses\n",
        )
        .unwrap();
        let config = Config::from_file(&valid).unwrap();
        let Some(proto::llm_provider_config::Config::Openai(openai)) =
            &config.providers["openai"].config
        else {
            panic!("expected native OpenAI provider");
        };
        assert_eq!(openai.api, "responses");

        let invalid = dir.path().join("invalid.yaml");
        std::fs::write(
            &invalid,
            "providers:\n  openai:\n    type: openai\n    model: gpt-5.6-luna\n    api: unknown\n",
        )
        .unwrap();
        let error = Config::from_file(&invalid).unwrap_err().to_string();
        assert!(error.contains("expected 'responses' or 'chat_completions'"));
    }

    #[test]
    fn test_binary_decode() {
        let mut config = Config::default();
        config.providers.insert(
            "test".to_string(),
            crate::control::config::ProviderConfig {
                config: Some(proto::llm_provider_config::Config::Openai(
                    proto::OpenAiConfig {
                        model: "m1".to_string(),
                        api_key: Some(Secret {
                            source: Some(proto::secret::Source::Plain("key".to_string())),
                        }),
                        org_id: "".to_string(),
                        api: "".to_string(),
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
    fn test_extends_supports_strings_lists_recursive_merges_and_list_replacement() {
        let dir = tempdir().unwrap();
        let base = dir.path().join("base.yml");
        std::fs::write(
            &base,
            r#"
providers:
  shared:
    type: openai
    model: base-model
    api_key: base-key
llmModels:
  shared:
    provider: openai
    contextWindowTokens: 100
    maxOutputTokens: 10
  base-only:
    contextWindowTokens: 50
trust:
  oidc:
    - name: base
      issuer: https://accounts.example.com
      audiences: [base-client]
      grants:
        - kind: read
controllers:
  deployment:
    enabled: true
    workers: 1
"#,
        )
        .unwrap();

        let overlay = dir.path().join("overlay.toml");
        std::fs::write(
            &overlay,
            r#"
extends = "./base.yml "

[llmProviders.shared]
type = "openai"
model = "overlay-model"
api_key = "overlay-key"

[models.shared]
maxOutputTokens = 20

[controllers.deployment]
workers = 2
"#,
        )
        .unwrap();

        let json_layer = dir.path().join("json-layer.json");
        std::fs::write(
            &json_layer,
            r#"{"models":{"json-only":{"contextWindowTokens":77}}}"#,
        )
        .unwrap();

        let root = dir.path().join("root.yaml");
        std::fs::write(
            &root,
            r#"
extends:
  - "./base.yml "
  - ./overlay.toml
  - ./json-layer.json

models:
  shared:
    contextWindowTokens: 200
trust:
  oidc:
    - name: child
      issuer: https://accounts.example.com
      audiences: [child-client]
      grants:
        - kind: readwrite
"#,
        )
        .unwrap();

        let config = Config::from_file(&root).unwrap();
        let provider = config.providers.get("shared").unwrap();
        let Some(proto::llm_provider_config::Config::Openai(provider)) = &provider.config else {
            panic!("expected openai provider");
        };
        assert_eq!(provider.model, "overlay-model");
        assert_eq!(config.models["shared"].context_window_tokens, Some(200));
        assert_eq!(config.models["shared"].max_output_tokens, Some(20));
        assert!(config.models.contains_key("base-only"));
        assert_eq!(config.models["json-only"].context_window_tokens, Some(77));
        assert_eq!(config.controllers["deployment"].workers, 2);
        assert_eq!(config.trust.as_ref().unwrap().oidc.len(), 1);
        assert_eq!(config.trust.as_ref().unwrap().oidc[0].name, "child");
    }

    #[test]
    fn test_extends_resolves_parent_paths_and_expands_each_file_independently() {
        let _guard = crate::test_support::env_lock();
        let _parent_data = EnvVarGuard::set("TALON_PARENT_DATA_DIR", "./parent-data");
        let _child_workspace = EnvVarGuard::set("TALON_CHILD_WORKSPACE", "./child-workspace");

        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested");
        let shared = dir.path().join("shared");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(
            shared.join("catalog.yaml"),
            "models:\n  shared-model:\n    contextWindowTokens: 123\n",
        )
        .unwrap();
        let parent = nested.join("parent.yaml");
        std::fs::write(
            &parent,
            r#"
extends: ../shared/catalog.yaml
workspace_dir: ${TALON_PARENT_DATA_DIR}
control_plane:
  database:
    driver: sqlite
    data_dir: ${TALON_PARENT_DATA_DIR}
  message_broker:
    driver: local_socket
  object_store:
    driver: local
    path: ./parent-objects
"#,
        )
        .unwrap();

        let child = dir.path().join("child.yaml");
        std::fs::write(
            &child,
            r#"
extends: ./nested/parent.yaml
workspace_dir: ${TALON_CHILD_WORKSPACE}
"#,
        )
        .unwrap();

        let config = Config::from_file(&child).unwrap();
        assert_eq!(
            config.workspace_dir,
            dir.path().join("child-workspace").display().to_string()
        );
        assert_eq!(
            config.models["shared-model"].context_window_tokens,
            Some(123)
        );
        let control_plane = config.control_plane.unwrap();
        assert_eq!(
            control_plane.database.unwrap().data_dir,
            nested.join("parent-data").display().to_string()
        );
        let Some(proto::object_store_config::Backend::Local(object_store)) =
            control_plane.object_store.unwrap().backend
        else {
            panic!("expected local object store");
        };
        assert_eq!(
            object_store.path,
            nested.join("parent-objects").display().to_string()
        );
    }

    #[test]
    fn test_extends_rejects_invalid_files_paths_roots_cycles_and_depth() {
        let dir = tempdir().unwrap();

        let missing = dir.path().join("missing.yaml");
        std::fs::write(&missing, "extends: ./does-not-exist.yaml\n").unwrap();
        let error = Config::from_file(&missing).unwrap_err().to_string();
        assert!(error.contains("Failed to resolve config file"));

        let malformed_parent = dir.path().join("malformed.yaml");
        std::fs::write(&malformed_parent, "providers: [\n").unwrap();
        let malformed = dir.path().join("malformed-root.yaml");
        std::fs::write(&malformed, "extends: ./malformed.yaml\n").unwrap();
        let error = Config::from_file(&malformed).unwrap_err().to_string();
        assert!(error.contains("Failed to parse YAML config"));

        let unsupported_parent = dir.path().join("parent.txt");
        std::fs::write(&unsupported_parent, "{}").unwrap();
        let unsupported = dir.path().join("unsupported.yaml");
        std::fs::write(&unsupported, "extends: ./parent.txt\n").unwrap();
        let error = Config::from_file(&unsupported).unwrap_err().to_string();
        assert!(error.contains("Unsupported config format"));

        let scalar_parent = dir.path().join("scalar.yaml");
        std::fs::write(&scalar_parent, "- one\n- two\n").unwrap();
        let scalar = dir.path().join("scalar-root.yaml");
        std::fs::write(&scalar, "extends: ./scalar.yaml\n").unwrap();
        let error = Config::from_file(&scalar).unwrap_err().to_string();
        assert!(error.contains("must contain a top-level object"));

        let remote = dir.path().join("remote.yaml");
        std::fs::write(&remote, "extends: https://example.com/models.yaml\n").unwrap();
        let error = Config::from_file(&remote).unwrap_err().to_string();
        assert!(error.contains("remote URLs are not supported"));

        let cycle_a = dir.path().join("cycle-a.yaml");
        let cycle_b = dir.path().join("cycle-b.yaml");
        std::fs::write(&cycle_a, "extends: ./cycle-b.yaml\n").unwrap();
        std::fs::write(&cycle_b, "extends: ./cycle-a.yaml\n").unwrap();
        let error = Config::from_file(&cycle_a).unwrap_err().to_string();
        assert!(error.contains("configuration extends cycle detected"));
        assert!(error.contains("cycle-a.yaml") && error.contains("cycle-b.yaml"));

        let max_test_depth = 33;
        for index in 0..=max_test_depth {
            let current = dir.path().join(format!("depth-{index}.yaml"));
            if index == max_test_depth {
                std::fs::write(&current, "{}\n").unwrap();
            } else {
                let next = dir.path().join(format!("depth-{}.yaml", index + 1));
                std::fs::write(
                    &current,
                    format!(
                        "extends: ./{}\n",
                        next.file_name().unwrap().to_string_lossy()
                    ),
                )
                .unwrap();
            }
        }
        let error = Config::from_file(dir.path().join("depth-0.yaml"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("maximum depth"));
    }

    #[test]
    fn test_control_plane_relative_data_dir_resolves_from_config_file_directory() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("nested").join("talon.yaml");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(
            &config_path,
            r#"
control_plane:
  database:
    driver: sqlite
    data_dir: ./data
  message_broker:
    driver: local_socket
  object_store:
    driver: local
    path: ./objects
"#,
        )
        .unwrap();

        let config = Config::from_file(&config_path).unwrap();
        assert_eq!(
            config
                .control_plane
                .as_ref()
                .unwrap()
                .database
                .as_ref()
                .unwrap()
                .data_dir,
            config_path
                .parent()
                .unwrap()
                .join("data")
                .display()
                .to_string()
        );
        let Some(proto::object_store_config::Backend::Local(local)) = config
            .control_plane
            .as_ref()
            .unwrap()
            .object_store
            .as_ref()
            .unwrap()
            .backend
            .as_ref()
        else {
            panic!("expected local object store");
        };
        assert_eq!(
            local.path,
            config_path
                .parent()
                .unwrap()
                .join("objects")
                .display()
                .to_string()
        );
    }

    #[test]
    fn test_relative_workspace_dir_resolves_from_config_file_directory() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("nested").join("talon.yaml");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(
            &config_path,
            r#"
workspace_dir: ./workspace
"#,
        )
        .unwrap();

        let config = Config::from_file(&config_path).unwrap();
        assert_eq!(
            config.workspace_dir,
            config_path
                .parent()
                .unwrap()
                .join("workspace")
                .display()
                .to_string()
        );
    }

    #[test]
    fn test_relative_paths_preserve_parent_dir_components() {
        assert_eq!(
            normalize_path(std::path::PathBuf::from("../workspace")),
            std::path::PathBuf::from("../workspace")
        );
        assert_eq!(
            normalize_path(std::path::PathBuf::from("../../data")),
            std::path::PathBuf::from("../../data")
        );

        let dir = tempdir().unwrap();
        let config_path = dir.path().join("configs").join("nested").join("talon.yaml");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(
            &config_path,
            r#"
workspace_dir: ../../workspace
control_plane:
  database:
    driver: sqlite
    data_dir: ../../data
  message_broker:
    driver: local_socket
"#,
        )
        .unwrap();

        let config = Config::from_file(&config_path).unwrap();
        assert_eq!(
            config.workspace_dir,
            dir.path().join("workspace").display().to_string()
        );
        assert_eq!(
            config
                .control_plane
                .as_ref()
                .unwrap()
                .database
                .as_ref()
                .unwrap()
                .data_dir,
            dir.path().join("data").display().to_string()
        );
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
    fn test_load_default_uses_inline_yaml_before_path() {
        let _guard = crate::test_support::env_lock();
        unsafe {
            env::set_var(
                "TALON_CONFIG_INLINE_YAML",
                "providers:\n  inline:\n    type: openai_compatible\n    base_url: https://example.com\n    model: demo\n    api_key: x\n",
            );
            env::set_var("TALON_CONFIG_PATH", "/does/not/exist.yaml");
        }

        let config = Config::load_default().unwrap();
        assert!(config.providers.contains_key("inline"));

        unsafe {
            env::remove_var("TALON_CONFIG_INLINE_YAML");
            env::remove_var("TALON_CONFIG_PATH");
        }
    }

    #[test]
    fn test_inline_yaml_rejects_extends() {
        let _guard = crate::test_support::env_lock();
        let _inline = EnvVarGuard::set("TALON_CONFIG_INLINE_YAML", "extends: ./models.yaml\n");
        let _path = EnvVarGuard::set("TALON_CONFIG_PATH", "/does/not/exist.yaml");

        let error = Config::load_default().unwrap_err().to_string();
        assert!(error.contains("TALON_CONFIG_INLINE_YAML does not support 'extends'"));
    }

    #[test]
    fn test_provider_api_key_is_optional_in_yaml_config() {
        let file = NamedTempFile::new().expect("Failed to create temp file");
        let path = file.path().with_extension("yaml");
        std::fs::write(
            &path,
            r#"
providers:
  meta:
    type: openai_compatible
    base_url: https://api.llama.com/compat/v1
    model: Llama-4-Maverick-17B-128E-Instruct-FP8
"#,
        )
        .unwrap();

        let config = Config::from_file(&path).unwrap();
        let provider = config.providers.get("meta").unwrap();
        let Some(proto::llm_provider_config::Config::OpenaiCompatible(generic)) = &provider.config
        else {
            panic!("expected openai compatible provider");
        };
        assert_eq!(generic.base_url, "https://api.llama.com/compat/v1");
        assert_eq!(generic.model, "Llama-4-Maverick-17B-128E-Instruct-FP8");
        assert!(generic.api_key.is_none());
    }

    #[test]
    fn test_s3_object_store_config_from_yaml() {
        let file = NamedTempFile::new().expect("Failed to create temp file");
        let path = file.path().with_extension("yaml");
        std::fs::write(
            &path,
            r#"
control_plane:
  database:
    driver: sqlite
  message_broker:
    driver: local_socket
  object_store:
    driver: s3
    bucket: talon-objects
    endpoint_url: http://localhost:9000
    force_path_style: true
"#,
        )
        .unwrap();

        let config = Config::from_file(&path).unwrap();
        let backend = config
            .control_plane
            .unwrap()
            .object_store
            .unwrap()
            .backend
            .unwrap();
        match backend {
            proto::object_store_config::Backend::S3(cfg) => {
                assert_eq!(cfg.bucket, "talon-objects");
                assert_eq!(cfg.endpoint_url, "http://localhost:9000");
                assert!(cfg.force_path_style);
            }
            other => panic!("unexpected object store backend: {other:?}"),
        }
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
        let serde = crate::control::config::SerdeConfig {
            providers: std::collections::HashMap::from([
                (
                    "primary".to_string(),
                    crate::control::config::SerdeProviderConfig::OpenaiCompatible {
                        base_url: "https://llm.example.com".to_string(),
                        model: "model-x".to_string(),
                        api_key: Some(crate::control::config::SerdeSecret::Ref(
                            crate::control::config::SerdeSecretRef {
                                source: "azure".to_string(),
                                key: "azure-key".to_string(),
                            },
                        )),
                    },
                ),
                (
                    "backup".to_string(),
                    crate::control::config::SerdeProviderConfig::Google {
                        model: "gemini".to_string(),
                        api_key: Some(crate::control::config::SerdeSecret::Plain(
                            "plain".to_string(),
                        )),
                    },
                ),
            ]),
            llm_providers: std::collections::HashMap::new(),
            database: Some(crate::control::config::DatabaseConfigWrapper {
                data_dir: Some("./data".to_string()),
                driver: Some("sqlite".to_string()),
                url: Some(crate::control::config::SerdeSecret::Plain(
                    "postgres://db".to_string(),
                )),
            }),
            server: Some(crate::control::config::ServerConfigWrapper {
                host: "0.0.0.0".to_string(),
                port: 7000,
            }),
            default_provider: Some("primary".to_string()),
            workspace_dir: None,
            control_plane: Some(crate::control::config::ControlPlaneConfigWrapper {
                database: crate::control::config::DatabaseConfigWrapper {
                    data_dir: None,
                    driver: Some("postgres".to_string()),
                    url: Some(crate::control::config::SerdeSecret::Ref(
                        crate::control::config::SerdeSecretRef {
                            source: "keychain".to_string(),
                            key: "db-url".to_string(),
                        },
                    )),
                },
                message_broker: crate::control::config::MessageBrokerConfigWrapper {
                    driver: "pubsub".to_string(),
                    extra: Default::default(),
                },
                scheduler: Some(crate::control::config::SchedulerConfigWrapper::CloudTasks {
                    project_id: Some("project".to_string()),
                    location: Some("us-central1".to_string()),
                    queue: Some("talon".to_string()),
                    target_url: Some("https://worker.example.com".to_string()),
                    callback_auth: Some(
                        crate::control::config::SchedulerCallbackAuthConfigWrapper::GoogleOidc {
                            audience: "https://worker.example.com".to_string(),
                            service_account_email: Some("svc@example.com".to_string()),
                        },
                    ),
                }),
                object_store: Some(crate::control::config::ObjectStoreConfigWrapper::S3 {
                    bucket: "talon-objects".to_string(),
                    prefix: Some("dev".to_string()),
                    region: Some("us-west-2".to_string()),
                    endpoint_url: Some("https://s3.example.com".to_string()),
                    force_path_style: Some(true),
                }),
                documents: None,
            }),
            storage: None,
            pubsub: None,
            controllers: std::collections::HashMap::new(),
            trust: None,
            models: std::collections::HashMap::new(),
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

        let control_plane = config.control_plane.as_ref().unwrap();
        let scheduler = control_plane.scheduler.as_ref().unwrap();
        let Some(proto::scheduler_config::Backend::CloudTasks(cloud)) = scheduler.backend.as_ref()
        else {
            panic!("expected cloud tasks scheduler");
        };
        assert_eq!(cloud.project_id, "project");
        let Some(proto::scheduler_callback_auth_config::Auth::GoogleOidc(oidc)) =
            cloud.callback_auth.as_ref().unwrap().auth.as_ref()
        else {
            panic!("expected oidc auth");
        };
        assert_eq!(oidc.service_account_email, "svc@example.com");

        let Some(proto::object_store_config::Backend::S3(s3)) = control_plane
            .object_store
            .as_ref()
            .unwrap()
            .backend
            .as_ref()
        else {
            panic!("expected s3 object store");
        };
        assert_eq!(s3.bucket, "talon-objects");
        assert_eq!(s3.prefix, "dev");
        assert_eq!(s3.region, "us-west-2");
        assert_eq!(s3.endpoint_url, "https://s3.example.com");
        assert!(s3.force_path_style);
    }

    #[test]
    fn test_secret_from_serde_secret_maps_known_and_unknown_sources() {
        let plain: Secret = crate::control::config::SerdeSecret::Plain("value".to_string()).into();
        let Some(proto::secret::Source::Plain(value)) = plain.source else {
            panic!("expected plain secret");
        };
        assert_eq!(value, "value");

        let env_ref: Secret =
            crate::control::config::SerdeSecret::Ref(crate::control::config::SerdeSecretRef {
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

    #[test]
    fn test_serde_config_into_proto_preserves_aws_eventbridge_scheduler() {
        let scheduler = crate::control::config::SchedulerConfigWrapper::AwsEventBridgeScheduler {
            group_name: Some("talon-group".to_string()),
            queue_url: Some("https://sqs.us-east-1.amazonaws.com/123/talon".to_string()),
            execution_role_arn: Some("arn:aws:iam::123:role/talon-scheduler".to_string()),
            schedule_name_prefix: Some("talon-dev".to_string()),
            dlq_arn: Some("arn:aws:sqs:us-east-1:123:talon-dlq".to_string()),
            maximum_event_age_seconds: Some(600),
            maximum_retry_attempts: Some(2),
            endpoint_url: Some("http://localhost:4566".to_string()),
        };

        let proto: proto::SchedulerConfig = scheduler.into();
        let Some(proto::scheduler_config::Backend::AwsEventbridgeScheduler(cfg)) = proto.backend
        else {
            panic!("expected EventBridge Scheduler backend");
        };
        assert_eq!(cfg.group_name, "talon-group");
        assert_eq!(
            cfg.queue_url,
            "https://sqs.us-east-1.amazonaws.com/123/talon"
        );
        assert_eq!(
            cfg.execution_role_arn,
            "arn:aws:iam::123:role/talon-scheduler"
        );
        assert_eq!(cfg.schedule_name_prefix, "talon-dev");
        assert_eq!(cfg.dlq_arn, "arn:aws:sqs:us-east-1:123:talon-dlq");
        assert_eq!(cfg.maximum_event_age_seconds, 600);
        assert_eq!(cfg.maximum_retry_attempts, Some(2));
        assert_eq!(cfg.endpoint_url, "http://localhost:4566");
    }

    #[test]
    fn test_serde_config_into_proto_preserves_zero_aws_retry_attempts() {
        let scheduler = crate::control::config::SchedulerConfigWrapper::AwsEventBridgeScheduler {
            group_name: None,
            queue_url: None,
            execution_role_arn: None,
            schedule_name_prefix: None,
            dlq_arn: None,
            maximum_event_age_seconds: None,
            maximum_retry_attempts: Some(0),
            endpoint_url: None,
        };

        let proto: proto::SchedulerConfig = scheduler.into();
        let Some(proto::scheduler_config::Backend::AwsEventbridgeScheduler(cfg)) = proto.backend
        else {
            panic!("expected EventBridge Scheduler backend");
        };
        assert_eq!(cfg.maximum_retry_attempts, Some(0));
    }
}
