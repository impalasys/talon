// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

mod local_socket;

use crate::control::MessagePublisher;
use anyhow::Result;
use google_cloud_googleapis::pubsub::v1::ExpirationPolicy;
use google_cloud_googleapis::pubsub::v1::PubsubMessage;
use google_cloud_pubsub::client::{Client, ClientConfig};
use google_cloud_pubsub::publisher::Publisher;
use google_cloud_pubsub::subscriber::ReceivedMessage;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

pub use local_socket::{LocalSocketMessagePublisher, LocalSocketSubscriber};

pub struct GcpPubSubPublisher {
    backend: Arc<dyn PubSubBackend>,
    initialized_topics: Arc<RwLock<HashSet<String>>>,
}

#[async_trait::async_trait]
trait PubSubBackend: Send + Sync {
    async fn ensure_topic(&self, fq_topic: &str) -> Result<()>;
    async fn publish(&self, fq_topic: &str, payload: Vec<u8>) -> Result<()>;
    async fn ensure_subscription(&self, fq_topic: &str, fq_sub: &str) -> Result<()>;
    async fn receive(
        &self,
        fq_sub: &str,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>>;
    async fn delete_subscription(&self, fq_sub: &str) -> Result<()>;
}

struct GcpPubSubBackend {
    client: Client,
    publishers: RwLock<HashMap<String, Publisher>>,
}

fn configured_project_id() -> String {
    std::env::var("GCP_PROJECT_ID").unwrap_or_else(|_| "talon-local".to_string())
}

pub fn configured_local_socket_path(default_root: Option<&std::path::Path>) -> PathBuf {
    if let Ok(path) = std::env::var("TALON_LOCAL_SOCKET_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Some(root) = default_root {
        return root.join("talon-broker.sock");
    }
    PathBuf::from("/tmp/talon-broker.sock")
}

pub fn fully_qualified_topic_name(project: &str, topic_name: &str) -> String {
    if topic_name.starts_with("projects/") {
        topic_name.to_string()
    } else {
        format!("projects/{}/topics/{}", project, topic_name)
    }
}

pub fn fully_qualified_subscription_name(project: &str, subscription_name: &str) -> String {
    if subscription_name.starts_with("projects/") {
        subscription_name.to_string()
    } else {
        format!("projects/{}/subscriptions/{}", project, subscription_name)
    }
}

impl GcpPubSubPublisher {
    pub async fn new() -> Result<Self> {
        let mut retries = 0;
        let client = loop {
            let mut config = ClientConfig::default().with_auth().await?;
            config.project_id = Some(configured_project_id());

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
                    tracing::error!(
                        attempt = retries,
                        error = %e,
                        "PubSub connection failed, retrying in 2 seconds"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        };

        Ok(Self {
            backend: Arc::new(GcpPubSubBackend {
                client,
                publishers: RwLock::new(HashMap::new()),
            }),
            initialized_topics: Arc::new(RwLock::new(HashSet::new())),
        })
    }

    #[cfg(test)]
    fn with_backend(backend: Arc<dyn PubSubBackend>) -> Self {
        Self {
            backend,
            initialized_topics: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    async fn ensure_topic_cached(&self, topic_name: &str) -> Result<String> {
        let project = configured_project_id();
        let fq_topic = fully_qualified_topic_name(&project, topic_name);
        {
            let lock = self.initialized_topics.read().await;
            if lock.contains(&fq_topic) {
                return Ok(fq_topic);
            }
        }

        let lock = self.initialized_topics.write().await;
        if lock.contains(&fq_topic) {
            return Ok(fq_topic);
        }
        drop(lock);

        self.backend.ensure_topic(&fq_topic).await?;

        let mut lock = self.initialized_topics.write().await;
        lock.insert(fq_topic.clone());
        Ok(fq_topic)
    }
}

use futures::StreamExt;

struct SubscriptionGuard {
    sub_id: String,
    backend: Arc<dyn PubSubBackend>,
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        let backend = self.backend.clone();
        let sub_id = self.sub_id.clone();
        tokio::spawn(async move {
            if let Err(e) = backend.delete_subscription(&sub_id).await {
                tracing::error!(subscription = %sub_id, error = %e, "Failed to cleanup PubSub subscription");
            }
        });
    }
}

#[async_trait::async_trait]
impl PubSubBackend for GcpPubSubBackend {
    async fn ensure_topic(&self, fq_topic: &str) -> Result<()> {
        let topic = self.client.topic(fq_topic);
        if !topic.exists(None).await? {
            if let Err(err) = topic.create(None, None).await {
                if !topic.exists(None).await? {
                    return Err(err.into());
                }
            }
        }
        Ok(())
    }

    async fn publish(&self, fq_topic: &str, payload: Vec<u8>) -> Result<()> {
        let publisher = {
            let cached = self.publishers.read().await;
            cached.get(fq_topic).cloned()
        };
        let publisher = match publisher {
            Some(publisher) => publisher,
            None => {
                let topic = self.client.topic(fq_topic);
                let publisher = topic.new_publisher(None);
                let mut cached = self.publishers.write().await;
                cached
                    .entry(fq_topic.to_string())
                    .or_insert_with(|| publisher.clone())
                    .clone()
            }
        };
        let mut msg = PubsubMessage::default();
        msg.data = payload.into();
        let awaiter = publisher.publish(msg).await;
        awaiter.get().await?;
        Ok(())
    }

    async fn ensure_subscription(&self, fq_topic: &str, fq_sub: &str) -> Result<()> {
        let sub_config = google_cloud_pubsub::subscription::SubscriptionConfig {
            ack_deadline_seconds: 60,
            expiration_policy: Some(ExpirationPolicy {
                ttl: Some(
                    std::time::Duration::from_secs(24 * 60 * 60)
                        .try_into()
                        .expect("24 hour subscription ttl should convert"),
                ),
            }),
            ..Default::default()
        };
        let subscription = self.client.subscription(fq_sub);
        if !subscription.exists(None).await? {
            if let Err(err) = subscription.create(fq_topic, sub_config, None).await {
                if !subscription.exists(None).await? {
                    return Err(err.into());
                }
            }
        }
        Ok(())
    }

    async fn receive(
        &self,
        fq_sub: &str,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
        let mut receiver = self.client.subscription(fq_sub).subscribe(None).await?;
        let stream = async_stream::stream! {
            let mut pending_ack: Option<ReceivedMessage> = None;
            while let Some(msg) = receiver.next().await {
                if let Some(previous) = pending_ack.take() {
                    let _ = previous.ack().await;
                }
                let data = msg.message.data.to_vec();
                pending_ack = Some(msg);
                yield data;
            }
            if let Some(previous) = pending_ack.take() {
                let _ = previous.ack().await;
            }
        };
        Ok(Box::pin(stream))
    }

    async fn delete_subscription(&self, fq_sub: &str) -> Result<()> {
        let sub = self.client.subscription(fq_sub);
        sub.delete(None).await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl MessagePublisher for GcpPubSubPublisher {
    async fn publish(&self, topic: &str, message: &[u8]) -> Result<()> {
        let fq_topic = self.ensure_topic_cached(topic).await?;
        self.backend.publish(&fq_topic, message.to_vec()).await
    }

    async fn subscribe(
        &self,
        topic_name: &str,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
        let fq_topic = self.ensure_topic_cached(topic_name).await?;

        // Create a temporary subscription for this stream
        let project = configured_project_id();
        let base_name = topic_name.rsplit('/').next().unwrap_or(topic_name);
        let sub_id = format!("{}-sub-{}", base_name, uuid::Uuid::now_v7());
        let fq_sub = fully_qualified_subscription_name(&project, &sub_id);
        self.backend.ensure_subscription(&fq_topic, &fq_sub).await?;
        let _guard = SubscriptionGuard {
            sub_id: fq_sub.clone(),
            backend: self.backend.clone(),
        };
        let receive_stream = self.backend.receive(&fq_sub).await?;

        let stream = async_stream::stream! {
            let _lifetime_guard = _guard;
            tokio::pin!(receive_stream);
            while let Some(msg) = receive_stream.next().await {
                yield msg;
            }
        };
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        configured_project_id, fully_qualified_subscription_name, fully_qualified_topic_name,
        GcpPubSubPublisher, PubSubBackend,
    };
    use crate::control::MessagePublisher;
    use std::collections::{HashMap, VecDeque};
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct FakeBackend {
        ensured_topics: Mutex<Vec<String>>,
        published: Mutex<Vec<(String, Vec<u8>)>>,
        ensured_subscriptions: Mutex<Vec<(String, String)>>,
        deleted_subscriptions: Mutex<Vec<String>>,
        receive_batches: Mutex<HashMap<String, VecDeque<Vec<Vec<u8>>>>>,
        default_receive_batches: Mutex<VecDeque<Vec<Vec<u8>>>>,
        fail_topic: Mutex<Option<String>>,
        fail_publish: Mutex<Option<String>>,
        fail_ensure_subscription: Mutex<Option<String>>,
        fail_subscribe_contains: Mutex<Option<String>>,
        fail_delete_contains: Mutex<Option<String>>,
    }

    #[async_trait::async_trait]
    impl PubSubBackend for FakeBackend {
        async fn ensure_topic(&self, fq_topic: &str) -> anyhow::Result<()> {
            if self.fail_topic.lock().await.as_deref() == Some(fq_topic) {
                anyhow::bail!("topic failure for {}", fq_topic);
            }
            self.ensured_topics.lock().await.push(fq_topic.to_string());
            Ok(())
        }

        async fn publish(&self, fq_topic: &str, payload: Vec<u8>) -> anyhow::Result<()> {
            if self.fail_publish.lock().await.as_deref() == Some(fq_topic) {
                anyhow::bail!("publish failure for {}", fq_topic);
            }
            self.published
                .lock()
                .await
                .push((fq_topic.to_string(), payload));
            Ok(())
        }

        async fn ensure_subscription(&self, fq_topic: &str, fq_sub: &str) -> anyhow::Result<()> {
            if self
                .fail_ensure_subscription
                .lock()
                .await
                .as_deref()
                .is_some_and(|needle| fq_sub.contains(needle))
            {
                anyhow::bail!("ensure subscription failure for {}", fq_sub);
            }
            self.ensured_subscriptions
                .lock()
                .await
                .push((fq_topic.to_string(), fq_sub.to_string()));
            Ok(())
        }

        async fn receive(
            &self,
            fq_sub: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            if self
                .fail_subscribe_contains
                .lock()
                .await
                .as_deref()
                .is_some_and(|needle| fq_sub.contains(needle))
            {
                anyhow::bail!("subscribe failure for {}", fq_sub);
            }
            let named_batches = {
                let mut named = self.receive_batches.lock().await;
                named
                    .get_mut(fq_sub)
                    .and_then(|entries| entries.pop_front())
            };
            let batches = if let Some(batches) = named_batches {
                batches
            } else {
                self.default_receive_batches
                    .lock()
                    .await
                    .pop_front()
                    .unwrap_or_default()
            };
            Ok(Box::pin(futures::stream::iter(batches)))
        }

        async fn delete_subscription(&self, fq_sub: &str) -> anyhow::Result<()> {
            if self
                .fail_delete_contains
                .lock()
                .await
                .as_deref()
                .is_some_and(|needle| fq_sub.contains(needle))
            {
                anyhow::bail!("delete failure for {}", fq_sub);
            }
            self.deleted_subscriptions
                .lock()
                .await
                .push(fq_sub.to_string());
            Ok(())
        }
    }

    #[test]
    fn topic_and_subscription_names_preserve_fully_qualified_inputs() {
        assert_eq!(
            fully_qualified_topic_name("acme", "projects/demo/topics/existing"),
            "projects/demo/topics/existing"
        );
        assert_eq!(
            fully_qualified_subscription_name("acme", "projects/demo/subscriptions/existing"),
            "projects/demo/subscriptions/existing"
        );
    }

    #[test]
    fn topic_and_subscription_names_expand_short_inputs() {
        assert_eq!(
            fully_qualified_topic_name("acme", "events"),
            "projects/acme/topics/events"
        );
        assert_eq!(
            fully_qualified_subscription_name("acme", "events-sub"),
            "projects/acme/subscriptions/events-sub"
        );
    }

    #[test]
    fn configured_project_id_defaults_when_env_missing() {
        let _lock = crate::test_support::env_lock();
        std::env::remove_var("GCP_PROJECT_ID");
        assert_eq!(configured_project_id(), "talon-local");
        std::env::set_var("GCP_PROJECT_ID", "project-123");
        assert_eq!(configured_project_id(), "project-123");
        std::env::remove_var("GCP_PROJECT_ID");
    }

    #[tokio::test]
    async fn publish_caches_topic_initialization_and_records_payloads() {
        let _lock = crate::test_support::async_env_mutex().lock().await;
        std::env::set_var("GCP_PROJECT_ID", "project-123");
        let backend = Arc::new(FakeBackend::default());
        let publisher = GcpPubSubPublisher::with_backend(backend.clone());

        publisher.publish("events", b"one").await.unwrap();
        publisher.publish("events", b"two").await.unwrap();

        assert_eq!(
            *backend.ensured_topics.lock().await,
            vec!["projects/project-123/topics/events".to_string()]
        );
        assert_eq!(
            *backend.published.lock().await,
            vec![
                (
                    "projects/project-123/topics/events".to_string(),
                    b"one".to_vec()
                ),
                (
                    "projects/project-123/topics/events".to_string(),
                    b"two".to_vec()
                ),
            ]
        );
        std::env::remove_var("GCP_PROJECT_ID");
    }

    #[tokio::test]
    async fn publish_concurrently_initializes_topic_once() {
        let _lock = crate::test_support::async_env_mutex().lock().await;
        std::env::set_var("GCP_PROJECT_ID", "project-123");
        let backend = Arc::new(FakeBackend::default());
        let publisher = Arc::new(GcpPubSubPublisher::with_backend(backend.clone()));

        let first = {
            let publisher = publisher.clone();
            tokio::spawn(async move { publisher.publish("events", b"one").await })
        };
        let second = {
            let publisher = publisher.clone();
            tokio::spawn(async move { publisher.publish("events", b"two").await })
        };

        first.await.expect("first task panicked").unwrap();
        second.await.expect("second task panicked").unwrap();

        assert_eq!(
            *backend.ensured_topics.lock().await,
            vec!["projects/project-123/topics/events".to_string()]
        );
        std::env::remove_var("GCP_PROJECT_ID");
    }

    #[tokio::test]
    async fn subscribe_creates_and_cleans_up_temporary_subscription() {
        let _lock = crate::test_support::async_env_mutex().lock().await;
        std::env::set_var("GCP_PROJECT_ID", "project-123");
        let backend = Arc::new(FakeBackend::default());
        let publisher = GcpPubSubPublisher::with_backend(backend.clone());

        let stream = publisher.subscribe("events").await.unwrap();
        drop(stream);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let ensured = backend.ensured_subscriptions.lock().await.clone();
        assert_eq!(ensured.len(), 1);
        assert_eq!(ensured[0].0, "projects/project-123/topics/events");
        assert!(ensured[0]
            .1
            .starts_with("projects/project-123/subscriptions/events-sub-"));
        assert_eq!(backend.deleted_subscriptions.lock().await.len(), 1);
        std::env::remove_var("GCP_PROJECT_ID");
    }

    #[tokio::test]
    async fn subscribe_uses_topic_basename_for_temporary_subscription_id() {
        let _lock = crate::test_support::async_env_mutex().lock().await;
        std::env::set_var("GCP_PROJECT_ID", "project-123");
        let backend = Arc::new(FakeBackend::default());
        let publisher = GcpPubSubPublisher::with_backend(backend.clone());

        let stream = publisher
            .subscribe("projects/demo/topics/session-control")
            .await
            .unwrap();
        drop(stream);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let ensured = backend.ensured_subscriptions.lock().await.clone();
        assert_eq!(ensured.len(), 1);
        assert_eq!(ensured[0].0, "projects/demo/topics/session-control");
        assert!(ensured[0]
            .1
            .starts_with("projects/project-123/subscriptions/session-control-sub-"));
        std::env::remove_var("GCP_PROJECT_ID");
    }

    #[tokio::test]
    async fn publish_and_subscribe_surface_backend_failures() {
        let _lock = crate::test_support::async_env_mutex().lock().await;
        std::env::set_var("GCP_PROJECT_ID", "project-123");
        let backend = Arc::new(FakeBackend::default());
        *backend.fail_topic.lock().await =
            Some("projects/project-123/topics/bad-topic".to_string());
        let publisher = GcpPubSubPublisher::with_backend(backend.clone());

        let err = publisher
            .publish("bad-topic", b"payload")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("topic failure"));

        *backend.fail_topic.lock().await = None;
        *backend.fail_subscribe_contains.lock().await = Some("events-sub-".to_string());
        let subscribe_err = match publisher.subscribe("events").await {
            Ok(_) => panic!("expected subscribe failure"),
            Err(err) => err,
        };
        assert!(subscribe_err.to_string().contains("subscribe failure"));
        std::env::remove_var("GCP_PROJECT_ID");
    }

    #[tokio::test]
    async fn subscribe_stream_yields_received_messages_in_order() {
        let _lock = crate::test_support::async_env_mutex().lock().await;
        std::env::set_var("GCP_PROJECT_ID", "project-123");
        let backend = Arc::new(FakeBackend::default());
        let publisher = GcpPubSubPublisher::with_backend(backend.clone());
        *backend.default_receive_batches.lock().await =
            VecDeque::from(vec![vec![b"first".to_vec(), b"second".to_vec()]]);

        let mut stream = publisher.subscribe("events").await.unwrap();
        let first = futures::StreamExt::next(&mut stream)
            .await
            .expect("first item");
        let second = futures::StreamExt::next(&mut stream)
            .await
            .expect("second item");
        assert_eq!(first, b"first".to_vec());
        assert_eq!(second, b"second".to_vec());
        drop(stream);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let dynamic_subscription = backend
            .ensured_subscriptions
            .lock()
            .await
            .last()
            .expect("expected ensured subscription")
            .1
            .clone();
        assert!(backend
            .deleted_subscriptions
            .lock()
            .await
            .contains(&dynamic_subscription));
        std::env::remove_var("GCP_PROJECT_ID");
    }

    #[tokio::test]
    async fn publish_surfaces_backend_publish_failure_after_topic_init() {
        let _lock = crate::test_support::async_env_mutex().lock().await;
        std::env::set_var("GCP_PROJECT_ID", "project-123");
        let backend = Arc::new(FakeBackend::default());
        *backend.fail_publish.lock().await = Some("projects/project-123/topics/events".to_string());
        let publisher = GcpPubSubPublisher::with_backend(backend.clone());

        let err = publisher.publish("events", b"payload").await.unwrap_err();
        assert!(err.to_string().contains("publish failure"));
        assert_eq!(
            *backend.ensured_topics.lock().await,
            vec!["projects/project-123/topics/events".to_string()]
        );
        std::env::remove_var("GCP_PROJECT_ID");
    }

    #[tokio::test]
    async fn subscribe_surfaces_subscription_creation_failure() {
        let _lock = crate::test_support::async_env_mutex().lock().await;
        std::env::set_var("GCP_PROJECT_ID", "project-123");
        let backend = Arc::new(FakeBackend::default());
        *backend.fail_ensure_subscription.lock().await = Some("events-sub-".to_string());
        let publisher = GcpPubSubPublisher::with_backend(backend);

        let err = match publisher.subscribe("events").await {
            Ok(_) => panic!("subscription creation failure should surface"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("ensure subscription failure"));
        std::env::remove_var("GCP_PROJECT_ID");
    }

    #[tokio::test]
    async fn backend_named_receive_batches_and_cleanup_failure_path_are_exercised() {
        let _lock = crate::test_support::async_env_mutex().lock().await;
        std::env::set_var("GCP_PROJECT_ID", "project-123");
        let backend = Arc::new(FakeBackend::default());
        *backend.fail_delete_contains.lock().await = Some("events-sub-".to_string());
        let publisher = GcpPubSubPublisher::with_backend(backend.clone());

        let stream = publisher.subscribe("events").await.unwrap();
        let dynamic_subscription = backend
            .ensured_subscriptions
            .lock()
            .await
            .last()
            .expect("expected ensured subscription")
            .1
            .clone();
        backend.receive_batches.lock().await.insert(
            dynamic_subscription.clone(),
            VecDeque::from(vec![vec![b"named".to_vec()]]),
        );
        drop(stream);

        let mut stream = backend.receive(&dynamic_subscription).await.unwrap();
        let value = futures::StreamExt::next(&mut stream)
            .await
            .expect("named batch should yield");
        assert_eq!(value, b"named".to_vec());
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        assert!(!backend
            .deleted_subscriptions
            .lock()
            .await
            .contains(&dynamic_subscription));
        std::env::remove_var("GCP_PROJECT_ID");
    }
}
