// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[cfg(test)]
mod tests {
    use crate::config::{proto, Config, ConfigExt, Secret, SecretExt};
    use prost::Message;
    use std::io::Write;
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
        std::env::set_var("MY_TEST_SECRET", "resolved-value");
        let secret = Secret {
            source: Some(proto::secret::Source::Ref(proto::SecretRef {
                source: proto::secret_ref::Source::Env as i32,
                key: "MY_TEST_SECRET".to_string(),
            })),
        };

        let value = secret.resolve().await.unwrap();
        assert_eq!(value, "resolved-value");
    }
}
