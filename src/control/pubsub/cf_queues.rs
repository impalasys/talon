// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::MessagePublisher;
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::{pin::Pin, time::Duration};

const CLOUDFLARE_HTTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct CfQueuesPublisher {
    client: reqwest::Client,
    endpoint: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishRequest<'a> {
    topic: &'a str,
    payload_base64: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubscribeMessage {
    payload_base64: String,
}

impl CfQueuesPublisher {
    pub fn from_env() -> Self {
        let endpoint = std::env::var("TALON_CLOUDFLARE_QUEUES_URL")
            .unwrap_or_else(|_| "http://talon-queues.internal".to_string());
        Self::new(endpoint)
    }

    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .build()
                .expect("Cloudflare Queues HTTP client should build"),
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
        }
    }
}

#[async_trait::async_trait]
impl MessagePublisher for CfQueuesPublisher {
    async fn publish(&self, topic: &str, message: &[u8]) -> Result<()> {
        let response = self
            .client
            .post(format!("{}/publish", self.endpoint))
            .timeout(CLOUDFLARE_HTTP_TIMEOUT)
            .json(&PublishRequest {
                topic,
                payload_base64: general_purpose::STANDARD.encode(message),
            })
            .send()
            .await?;
        if response.status().is_success() {
            return Ok(());
        }
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(anyhow!(
            "Cloudflare Queues publish failed for topic {topic} with HTTP {status}: {body}"
        ))
    }

    async fn subscribe(
        &self,
        topic: &str,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
        let response = self
            .client
            .get(format!(
                "{}/subscribe?topic={}",
                self.endpoint,
                urlencoding::encode(topic)
            ))
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Cloudflare Queues subscribe failed for topic {topic} with HTTP {status}: {body}"
            ));
        }

        let mut chunks = response.bytes_stream();
        let topic = topic.to_string();
        let stream = async_stream::stream! {
            let mut buffer = Vec::new();
            while let Some(chunk) = chunks.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(err) => {
                        tracing::warn!(topic = %topic, error = %err, "Cloudflare stream subscription ended with error");
                        break;
                    }
                };
                buffer.extend_from_slice(&chunk);

                while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
                    let line = buffer.drain(..=newline).collect::<Vec<_>>();
                    let line = &line[..line.len().saturating_sub(1)];
                    if line.iter().all(|byte| byte.is_ascii_whitespace()) {
                        continue;
                    }
                    let message = match serde_json::from_slice::<SubscribeMessage>(line) {
                        Ok(message) => message,
                        Err(err) => {
                            tracing::warn!(topic = %topic, error = %err, "Cloudflare stream subscription yielded invalid JSON");
                            continue;
                        }
                    };
                    match general_purpose::STANDARD.decode(message.payload_base64) {
                        Ok(payload) => yield payload,
                        Err(err) => {
                            tracing::warn!(topic = %topic, error = %err, "Cloudflare stream subscription yielded invalid base64 payload");
                        }
                    }
                }
            }
        };
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::CfQueuesPublisher;
    use crate::control::MessagePublisher;
    use axum::{routing::get, Router};
    use futures::StreamExt;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn subscribe_decodes_streamed_base64_payloads() {
        let app = Router::new().route(
            "/subscribe",
            get(|| async {
                concat!(
                    "{\"payloadBase64\":\"aGVsbG8=\"}\n",
                    "{\"payloadBase64\":\"d29ybGQ=\"}\n"
                )
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let publisher = CfQueuesPublisher::new(format!("http://{addr}"));
        let mut stream = publisher.subscribe("talon.session.parts.7").await.unwrap();

        assert_eq!(stream.next().await.unwrap(), b"hello");
        assert_eq!(stream.next().await.unwrap(), b"world");
        assert!(stream.next().await.is_none());

        server.abort();
    }
}
