// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Workload identity acquisition, independent of the destination LLM service.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::OnceCell;

use crate::control::config::proto;

const GOOGLE_IDENTITY_ENDPOINT: &str =
    "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/identity";

/// Supplies a fresh OIDC subject token whenever a destination needs to exchange it.
#[async_trait]
pub trait WorkloadIdentitySource: Send + Sync {
    async fn subject_token(&self) -> Result<String>;
}

pub(super) fn identity_source(
    config: &proto::WorkloadIdentitySource,
) -> Result<Arc<dyn WorkloadIdentitySource>> {
    use proto::workload_identity_source::Config;
    match config.config.as_ref() {
        Some(Config::GoogleMetadata(source)) => Ok(Arc::new(GoogleMetadata::new(
            source.audience.clone(),
            GOOGLE_IDENTITY_ENDPOINT.into(),
        )?)),
        Some(Config::AwsSts(source)) => Ok(Arc::new(AwsSts {
            audience: source.audience.clone(),
            region: source.region.clone(),
            client: OnceCell::new(),
        })),
        Some(Config::TokenFile(source)) => Ok(Arc::new(TokenFile {
            path: source.path.clone().into(),
        })),
        None => Err(anyhow!("workload_identity requires an identity source")),
    }
}

pub(super) struct GoogleMetadata {
    audience: String,
    endpoint: String,
    http: reqwest::Client,
}

impl GoogleMetadata {
    pub(super) fn new(audience: String, endpoint: String) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("Failed to initialize Google identity HTTP client")?;
        Ok(Self {
            audience,
            endpoint,
            http,
        })
    }
}

#[async_trait]
impl WorkloadIdentitySource for GoogleMetadata {
    async fn subject_token(&self) -> Result<String> {
        let mut url = reqwest::Url::parse(&self.endpoint)
            .context("Invalid Google metadata token endpoint")?;
        url.query_pairs_mut()
            .append_pair("audience", &self.audience)
            .append_pair("format", "full");
        let token = self
            .http
            .get(url)
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .context("Failed to request Google metadata identity token")?
            .error_for_status()
            .context("Google metadata identity token request was rejected")?
            .text()
            .await
            .context("Failed to read Google metadata identity token")?;
        nonempty_token(token, "Google metadata server")
    }
}

struct AwsSts {
    audience: String,
    region: String,
    client: OnceCell<aws_sdk_sts::Client>,
}

#[async_trait]
impl WorkloadIdentitySource for AwsSts {
    async fn subject_token(&self) -> Result<String> {
        let client = self
            .client
            .get_or_init(|| async {
                let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
                    .region(aws_sdk_sts::config::Region::new(self.region.clone()))
                    .load()
                    .await;
                aws_sdk_sts::Client::new(&config)
            })
            .await;
        // Outbound identity federation must be enabled on the AWS account.
        // AWS access keys/SigV4 are used by STS, never as the LLM subject token.
        let response = client
            .get_web_identity_token()
            .audience(&self.audience)
            .duration_seconds(300)
            .signing_algorithm("ES384")
            .send()
            .await
            .context("Failed to request AWS STS workload identity token")?;
        nonempty_token(
            response.web_identity_token().unwrap_or_default().into(),
            "AWS STS",
        )
    }
}

struct TokenFile {
    path: PathBuf,
}

#[async_trait]
impl WorkloadIdentitySource for TokenFile {
    async fn subject_token(&self) -> Result<String> {
        // Do not retain an open handle: Kubernetes rotates projected files via symlinks.
        let token = tokio::fs::read_to_string(&self.path)
            .await
            .context("Failed to read workload identity token file")?;
        nonempty_token(token, "Workload identity token file")
    }
}

fn nonempty_token(token: String, source: &str) -> Result<String> {
    let token = token.trim();
    if token.is_empty() {
        return Err(anyhow!("{source} returned an empty identity token"));
    }
    Ok(token.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn token_file_rereads_rotated_token_and_rejects_missing_or_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        let source = TokenFile { path: path.clone() };
        assert!(source
            .subject_token()
            .await
            .unwrap_err()
            .to_string()
            .contains("Failed to read"));
        tokio::fs::write(&path, "first-token\n").await.unwrap();
        assert_eq!(source.subject_token().await.unwrap(), "first-token");
        let replacement = dir.path().join("replacement");
        tokio::fs::write(&replacement, "second-token\n")
            .await
            .unwrap();
        tokio::fs::rename(&replacement, &path).await.unwrap();
        assert_eq!(source.subject_token().await.unwrap(), "second-token");
        tokio::fs::write(&path, " \n").await.unwrap();
        assert!(source
            .subject_token()
            .await
            .unwrap_err()
            .to_string()
            .contains("empty identity token"));
    }

    #[tokio::test]
    async fn aws_sts_requests_oidc_token_with_configured_audience() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0; 8192];
            loop {
                let n = stream.read(&mut buf).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&bytes);
                if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|n| n.trim().parse().unwrap())
                        })
                        .unwrap();
                    if body.len() >= length {
                        break;
                    }
                }
            }
            let request = String::from_utf8(bytes).unwrap();
            assert!(request.contains("Action=GetWebIdentityToken"));
            assert!(request.contains("Audience.member.1=https%3A%2F%2Fapi.openai.com%2Fv1"));
            assert!(request.contains("SigningAlgorithm=ES384"));
            assert!(request.contains("DurationSeconds=300"));
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: aws4-hmac-sha256"));
            let body = "<GetWebIdentityTokenResponse xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\"><GetWebIdentityTokenResult><WebIdentityToken>aws-subject-token</WebIdentityToken></GetWebIdentityTokenResult><ResponseMetadata><RequestId>test</RequestId></ResponseMetadata></GetWebIdentityTokenResponse>";
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        });
        let config = aws_sdk_sts::config::Builder::new()
            .behavior_version(aws_config::BehaviorVersion::latest())
            .region(aws_sdk_sts::config::Region::new("us-west-2"))
            .credentials_provider(aws_sdk_sts::config::Credentials::new(
                "test-key",
                "test-secret",
                None,
                None,
                "test",
            ))
            .endpoint_url(format!("http://{address}"))
            .build();
        let source = AwsSts {
            audience: "https://api.openai.com/v1".into(),
            region: "us-west-2".into(),
            client: OnceCell::new_with(Some(aws_sdk_sts::Client::from_conf(config))),
        };
        assert_eq!(source.subject_token().await.unwrap(), "aws-subject-token");
        server.await.unwrap();
    }
}
