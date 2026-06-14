// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::MessagePublisher;
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use serde::Serialize;
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

impl CfQueuesPublisher {
    pub fn from_env() -> Self {
        let endpoint = std::env::var("TALON_CLOUDFLARE_QUEUES_URL")
            .unwrap_or_else(|_| "http://talon-queues.internal".to_string());
        Self::new(endpoint)
    }

    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(CLOUDFLARE_HTTP_TIMEOUT)
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
        let _ = topic;
        Err(anyhow!(
            "cf_queues does not support pull subscribe; use Worker queue delivery"
        ))
    }
}
