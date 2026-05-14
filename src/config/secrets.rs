use crate::config::proto;
use anyhow::{anyhow, Result};
use std::process::Command;
use std::sync::Arc;

pub use proto::Secret;

#[async_trait::async_trait]
pub trait SecretExt {
    async fn resolve(&self) -> Result<String>;
}

#[async_trait::async_trait]
impl SecretExt for Secret {
    async fn resolve(&self) -> Result<String> {
        match &self.source {
            Some(proto::secret::Source::Plain(s)) => Ok(s.clone()),
            Some(proto::secret::Source::Ref(r)) => {
                let source = proto::secret_ref::Source::try_from(r.source)
                    .map_err(|_| anyhow!("Invalid secret source"))?;
                match source {
                    proto::secret_ref::Source::Env => {
                        std::env::var(&r.key).map_err(|_| anyhow!("Env var {} not set", r.key))
                    }
                    proto::secret_ref::Source::Gcp => {
                        use google_cloud_secretmanager_v1::client::SecretManagerService;

                        let client = SecretManagerService::builder().build().await?;

                        // We need to get the project ID from the client if possible, but
                        // the client might not expose it easily.
                        // For now, let's assume the key is either full name or we can't easily guess.
                        if !r.key.contains("/") {
                            return Err(anyhow!("GCP secret key must be in 'projects/PROJECT/secrets/NAME/versions/VERSION' format for now"));
                        }

                        let response = client
                            .access_secret_version()
                            .set_name(&r.key)
                            .send()
                            .await?;

                        let payload = response
                            .payload
                            .ok_or_else(|| anyhow!("Empty GCP secret payload"))?;
                        let data = String::from_utf8(payload.data.to_vec())?;
                        Ok(data)
                    }
                    proto::secret_ref::Source::Aws => {
                        use aws_config::BehaviorVersion;
                        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
                        let client = aws_sdk_secretsmanager::Client::new(&config);

                        let response = client
                            .get_secret_value()
                            .secret_id(&r.key)
                            .send()
                            .await
                            .map_err(|e| {
                                anyhow!("AWS Secrets Manager error for {}: {}", r.key, e)
                            })?;

                        response
                            .secret_string()
                            .map(|s| s.to_string())
                            .ok_or_else(|| anyhow!("AWS secret {} is not a string", r.key))
                    }
                    proto::secret_ref::Source::Azure => {
                        use azure_identity::AzureCliCredential;
                        use azure_security_keyvault_secrets::SecretClient;

                        // Expect r.key to be "vault-name/secret-name"
                        let parts: Vec<&str> = r.key.split('/').collect();
                        if parts.len() != 2 {
                            return Err(anyhow!(
                                "Azure secret key must be in 'vault-name/secret-name' format"
                            ));
                        }
                        let vault_name = parts[0];
                        let secret_name = parts[1];
                        let vault_url = format!("https://{}.vault.azure.net/", vault_name);

                        let credential = AzureCliCredential::new(None)?;
                        let client = SecretClient::new(&vault_url, credential, None)?;

                        let response = client.get_secret(secret_name, None).await?;
                        let secret = response.into_model()?;
                        secret
                            .value
                            .ok_or_else(|| anyhow!("Azure secret {} value is missing", r.key))
                    }
                    proto::secret_ref::Source::Keychain => {
                        // We use 'talon-engine' as the account name for all Talon secrets
                        let output = Command::new("security")
                            .args([
                                "find-generic-password",
                                "-a",
                                "talon-engine",
                                "-s",
                                &r.key,
                                "-w",
                            ])
                            .output()?;

                        if output.status.success() {
                            let password = String::from_utf8(output.stdout)?.trim().to_string();
                            Ok(password)
                        } else {
                            let err = String::from_utf8_lossy(&output.stderr);
                            Err(anyhow!("Keychain error for {}: {}", r.key, err))
                        }
                    }
                }
            }
            None => Err(anyhow!("Secret source missing")),
        }
    }
}
