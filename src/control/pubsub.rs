use crate::control::MessagePublisher;
use anyhow::Result;
use google_cloud_googleapis::pubsub::v1::PubsubMessage;
use google_cloud_pubsub::client::{Client, ClientConfig};
use google_cloud_pubsub::publisher::Publisher;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct GcpPubSubPublisher {
    client: Client,
    publishers: Arc<RwLock<HashMap<String, Publisher>>>,
}

impl GcpPubSubPublisher {
    pub async fn new() -> Result<Self> {
        let mut retries = 0;
        let client = loop {
            let mut config = ClientConfig::default().with_auth().await?;
            config.project_id =
                Some(std::env::var("GCP_PROJECT_ID").unwrap_or_else(|_| "talon-local".to_string()));

            match Client::new(config).await {
                Ok(c) => break c,
                Err(e) => {
                    retries += 1;
                    if retries > 10 {
                        return Err(anyhow::anyhow!(
                            "Failed to connect to PubSub after {} retries: {}",
                            retries,
                            e
                        ));
                    }
                    eprintln!(
                        "PubSub connection failed, retrying in 2 seconds... (Attempt {}) Error: {}",
                        retries, e
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        };

        Ok(Self {
            client,
            publishers: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    async fn get_publisher(&self, topic_name: &str) -> Result<Publisher> {
        {
            let lock = self.publishers.read().await;
            if let Some(pub_ref) = lock.get(topic_name) {
                return Ok(pub_ref.clone());
            }
        }

        let project = std::env::var("GCP_PROJECT_ID").unwrap_or_else(|_| "talon-local".to_string());
        let fq_topic = if topic_name.starts_with("projects/") {
            topic_name.to_string()
        } else {
            format!("projects/{}/topics/{}", project, topic_name)
        };

        let mut topic = self.client.topic(&fq_topic);
        if !topic.exists(None).await? {
            topic.create(None, None).await?;
        }

        let publisher = topic.new_publisher(None);
        let mut lock = self.publishers.write().await;
        lock.insert(topic_name.to_string(), publisher.clone());
        Ok(publisher)
    }
}

use futures::StreamExt;
use std::pin::Pin;

struct SubscriptionGuard {
    sub_id: String,
    client: Client,
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        let client = self.client.clone();
        let sub_id = self.sub_id.clone();
        tokio::spawn(async move {
            let mut sub = client.subscription(&sub_id);
            if let Err(e) = sub.delete(None).await {
                eprintln!("Failed to cleanup PubSub subscription {}: {}", sub_id, e);
            }
        });
    }
}

#[async_trait::async_trait]
impl MessagePublisher for GcpPubSubPublisher {
    async fn publish(&self, topic: &str, message: &[u8]) -> Result<()> {
        let payload = message.to_vec();
        let publisher = self.get_publisher(topic).await?;

        let mut msg = PubsubMessage::default();
        msg.data = payload.into();

        let awaiter = publisher.publish(msg).await;
        awaiter.get().await?;

        Ok(())
    }

    async fn subscribe(
        &self,
        topic_name: &str,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
        let project = std::env::var("GCP_PROJECT_ID").unwrap_or_else(|_| "talon-local".to_string());
        let fq_topic = if topic_name.starts_with("projects/") {
            topic_name.to_string()
        } else {
            format!("projects/{}/topics/{}", project, topic_name)
        };

        let mut topic = self.client.topic(&fq_topic);
        if !topic.exists(None).await? {
            topic.create(None, None).await?;
        }

        // Create a temporary subscription for this stream
        let sub_id = format!("{}-sub-{}", topic_name, uuid::Uuid::now_v7());
        let fq_sub = format!("projects/{}/subscriptions/{}", project, sub_id);
        let sub_config = google_cloud_pubsub::subscription::SubscriptionConfig {
            ack_deadline_seconds: 60,
            ..Default::default()
        };

        let mut subscription = self.client.subscription(&fq_sub);
        if !subscription.exists(None).await? {
            subscription.create(&fq_topic, sub_config, None).await?;
        }

        let mut receiver = subscription.subscribe(None).await?;
        let _guard = SubscriptionGuard {
            sub_id: fq_sub.clone(),
            client: self.client.clone(),
        };

        let stream = async_stream::stream! {
            // Moving the guard into the stream closure so its lifetime is tied to the stream
            let _lifetime_guard = _guard;
            while let Some(msg) = receiver.next().await {
                let _ = msg.ack().await;
                yield msg.message.data.to_vec();
            }
        };

        Ok(Box::pin(stream))
    }
}
