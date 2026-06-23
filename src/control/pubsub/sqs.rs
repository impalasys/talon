// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{topics, MessagePublisher};
use anyhow::{anyhow, Result};
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_sqs::{config::Credentials, types::MessageAttributeValue, Client};
use base64::{engine::general_purpose, Engine as _};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

pub const TALON_TOPIC_ATTRIBUTE: &str = "talon_topic";

// Default SQS queue name for all durable Talon worker-delivered messages.
const DEFAULT_QUEUE_NAME: &str = "talon";
// Default long-poll duration for ReceiveMessage; AWS allows 0-20 seconds.
const DEFAULT_WAIT_TIME_SECONDS: i32 = 10;
// Default time a received message stays hidden while a worker handles it.
const DEFAULT_VISIBILITY_TIMEOUT_SECONDS: i32 = 30;
// AWS SQS maximum ReceiveMessage long-poll duration.
const MAX_WAIT_TIME_SECONDS: i32 = 20;
// AWS SQS maximum message visibility timeout, in seconds.
const MAX_VISIBILITY_TIMEOUT_SECONDS: i32 = 43_200;
// AWS SQS queue names are capped at 80 characters.
const QUEUE_NAME_MAX_LEN: usize = 80;

#[derive(Clone)]
pub struct SqsMessagePublisher {
    client: Client,
    queue_name: String,
    queue_url: Arc<RwLock<Option<String>>>,
    wait_time_seconds: i32,
    visibility_timeout_seconds: i32,
}

impl SqsMessagePublisher {
    pub async fn from_env() -> Result<Self> {
        let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(RegionProviderChain::default_provider().or_else("us-east-1"));
        if let Ok(endpoint_url) = std::env::var("TALON_SQS_ENDPOINT_URL") {
            if !endpoint_url.trim().is_empty() {
                loader = loader
                    .credentials_provider(Credentials::new("fake", "fake", None, None, "local"))
                    .endpoint_url(endpoint_url);
            }
        }
        let config = loader.load().await;
        let queue_name = std::env::var("TALON_SQS_QUEUE_NAME")
            .or_else(|_| std::env::var("TALON_SQS_QUEUE_PREFIX"))
            .unwrap_or_else(|_| DEFAULT_QUEUE_NAME.into());
        Ok(Self::new(Client::new(&config), queue_name))
    }

    pub fn new(client: Client, queue_name: impl Into<String>) -> Self {
        Self {
            client,
            queue_name: queue_name_for_config(queue_name.into()),
            queue_url: Arc::new(RwLock::new(None)),
            wait_time_seconds: configured_i32(
                "TALON_SQS_WAIT_TIME_SECONDS",
                DEFAULT_WAIT_TIME_SECONDS,
                0,
                MAX_WAIT_TIME_SECONDS,
            ),
            visibility_timeout_seconds: configured_i32(
                "TALON_SQS_VISIBILITY_TIMEOUT_SECONDS",
                DEFAULT_VISIBILITY_TIMEOUT_SECONDS,
                0,
                MAX_VISIBILITY_TIMEOUT_SECONDS,
            ),
        }
    }

    pub async fn queue_url(&self) -> Result<String> {
        {
            let cached = self.queue_url.read().await;
            if let Some(queue_url) = cached.as_ref() {
                return Ok(queue_url.clone());
            }
        }

        let queue_url = match self
            .client
            .get_queue_url()
            .queue_name(&self.queue_name)
            .send()
            .await
        {
            Ok(output) => output
                .queue_url()
                .ok_or_else(|| anyhow!("SQS get_queue_url returned no queue URL"))?
                .to_string(),
            Err(err)
                if err
                    .as_service_error()
                    .is_some_and(|err| err.is_queue_does_not_exist()) =>
            {
                self.create_queue().await?
            }
            Err(err) => return Err(err.into()),
        };

        let mut cached = self.queue_url.write().await;
        Ok(cached.get_or_insert(queue_url).clone())
    }

    async fn create_queue(&self) -> Result<String> {
        let output = self
            .client
            .create_queue()
            .queue_name(&self.queue_name)
            .send()
            .await?;
        output
            .queue_url()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("SQS create_queue returned no queue URL"))
    }
}

impl SqsMessagePublisher {
    pub fn client(&self) -> Client {
        self.client.clone()
    }

    pub fn wait_time_seconds(&self) -> i32 {
        self.wait_time_seconds
    }

    pub fn visibility_timeout_seconds(&self) -> i32 {
        self.visibility_timeout_seconds
    }
}

#[async_trait::async_trait]
impl MessagePublisher for SqsMessagePublisher {
    async fn publish(&self, topic: &str, message: &[u8]) -> Result<()> {
        if !is_worker_delivered_topic(topic) {
            return Err(anyhow!(
                "SQS cannot publish Talon topic {topic}; SQS is a durable worker queue and only supports worker-delivered topics"
            ));
        }

        let queue_url = self.queue_url().await?;
        let body = general_purpose::STANDARD.encode(message);
        self.client
            .send_message()
            .queue_url(queue_url)
            .message_body(body)
            .message_attributes(TALON_TOPIC_ATTRIBUTE, string_attribute(topic)?)
            .send()
            .await?;
        Ok(())
    }

    async fn subscribe(
        &self,
        topic: &str,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
        if is_live_session_stream_topic(topic) {
            return Err(anyhow!(
                "SQS cannot subscribe to live Talon session stream topic {topic}; SQS is a durable work queue and does not provide fanout for token deltas"
            ));
        }
        Err(anyhow!(
            "SQS generic subscriptions are not supported because the MessagePublisher stream API cannot acknowledge messages after handler completion; use Talon worker pull mode for SQS topics"
        ))
    }
}

fn string_attribute(value: &str) -> Result<MessageAttributeValue> {
    Ok(MessageAttributeValue::builder()
        .data_type("String")
        .string_value(value)
        .build()?)
}

fn configured_i32(name: &str, default: i32, min: i32, max: i32) -> i32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .map(|value| value.clamp(min, max))
        .unwrap_or(default)
}

fn queue_name_for_config(value: String) -> String {
    let name = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .take(QUEUE_NAME_MAX_LEN)
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if name.is_empty() {
        DEFAULT_QUEUE_NAME.to_string()
    } else {
        name
    }
}

fn is_live_session_stream_topic(topic: &str) -> bool {
    topic
        .strip_prefix(topics::SESSION_PARTS_TOPIC_PREFIX)
        .is_some_and(|suffix| suffix.starts_with('.'))
}

fn is_worker_delivered_topic(topic: &str) -> bool {
    matches!(
        topic,
        topics::SESSION_DISPATCH_TOPIC
            | topics::RESOURCE_LIFECYCLE_TOPIC
            | topics::SESSION_CONTROL_TOPIC
            | topics::WORKFLOW_DISPATCH_TOPIC
            | topics::INDEX_EVENTS_TOPIC
    )
}

#[cfg(test)]
mod tests {
    use super::{
        configured_i32, is_live_session_stream_topic, is_worker_delivered_topic,
        queue_name_for_config,
    };

    #[test]
    fn queue_names_are_sqs_safe_and_stable() {
        let name = queue_name_for_config("talon-dev".to_string());

        assert_eq!(name, queue_name_for_config("talon-dev".to_string()));
        assert!(name.len() <= 80);
        assert!(name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
    }

    #[test]
    fn queue_names_truncate_to_sqs_limit() {
        let name = queue_name_for_config(
            "tenant-queue-name-that-is-far-too-long-for-an-sqs-queue-name-and-must-be-truncated"
                .to_string(),
        );

        assert!(name.len() <= 80, "{name}");
        assert!(name.starts_with("tenant-queue"));
    }

    #[test]
    fn configured_i32_clamps_to_aws_bounds() {
        let name = "TALON_TEST_SQS_CONFIGURED_I32";
        std::env::set_var(name, "999");
        assert_eq!(configured_i32(name, 10, 0, 20), 20);

        std::env::set_var(name, "-10");
        assert_eq!(configured_i32(name, 10, 0, 20), 0);

        std::env::set_var(name, "not-an-int");
        assert_eq!(configured_i32(name, 10, 0, 20), 10);

        std::env::remove_var(name);
    }

    #[test]
    fn live_session_stream_topics_are_not_sqs_subscribable() {
        assert!(is_live_session_stream_topic("talon.session.parts.7"));
        assert!(!is_live_session_stream_topic("talon.session.dispatch"));
        assert!(!is_live_session_stream_topic("talon.session.parts-extra"));
    }

    #[test]
    fn only_worker_delivered_topics_are_sqs_publishable() {
        assert!(is_worker_delivered_topic("talon.session.dispatch"));
        assert!(is_worker_delivered_topic("talon.workflow.dispatch"));
        assert!(!is_worker_delivered_topic("talon.session.parts.7"));
        assert!(!is_worker_delivered_topic("talon.channel.events.acme.main"));
    }
}
