// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex, Notify, OnceCell};

use crate::control::{events::SessionStepEvent, topics, MessagePublisher};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShardLifecycle {
    Idle,
    Initializing,
    Started,
}

impl Default for ShardLifecycle {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Default)]
struct ShardState {
    lifecycle: ShardLifecycle,
    listeners: HashMap<String, Vec<mpsc::UnboundedSender<Result<SessionStepEvent, tonic::Status>>>>,
}

struct Shard {
    state: Mutex<ShardState>,
    ready: Notify,
}

impl Default for Shard {
    fn default() -> Self {
        Self {
            state: Mutex::new(ShardState::default()),
            ready: Notify::new(),
        }
    }
}

pub struct SessionStreamHub {
    pubsub: Arc<dyn MessagePublisher + Send + Sync>,
    shards: Vec<Arc<Shard>>,
    initialized: OnceCell<()>,
}

impl SessionStreamHub {
    pub fn new(pubsub: Arc<dyn MessagePublisher + Send + Sync>) -> Self {
        let shard_count = topics::session_step_shard_count() as usize;
        let shards = (0..shard_count)
            .map(|_| Arc::new(Shard::default()))
            .collect();

        Self {
            pubsub,
            shards,
            initialized: OnceCell::new(),
        }
    }

    pub async fn subscribe(
        &self,
        session_id: &str,
    ) -> anyhow::Result<mpsc::UnboundedReceiver<Result<SessionStepEvent, tonic::Status>>> {
        let _ = self.initialized.get_or_init(|| async {}).await;

        let shard = topics::session_step_shard(session_id) as usize;
        let state = self
            .shards
            .get(shard)
            .ok_or_else(|| anyhow::anyhow!("Invalid session shard {}", shard))?
            .clone();

        let (tx, rx) = mpsc::unbounded_channel();
        {
            let mut guard = state.state.lock().await;
            guard
                .listeners
                .entry(session_id.to_string())
                .or_default()
                .push(tx);
        }

        if let Err(err) = self.ensure_shard_task(shard, state.clone()).await {
            let mut guard = state.state.lock().await;
            if let Some(entries) = guard.listeners.get_mut(session_id) {
                entries.retain(|sender| !sender.is_closed());
                if entries.is_empty() {
                    guard.listeners.remove(session_id);
                }
            }
            return Err(err);
        }
        Ok(rx)
    }

    async fn ensure_shard_task(&self, shard: usize, state: Arc<Shard>) -> anyhow::Result<()> {
        let topic = topics::session_step_topic_for_shard(shard as u32);
        loop {
            let should_subscribe = {
                let mut guard = state.state.lock().await;
                match guard.lifecycle {
                    ShardLifecycle::Started => return Ok(()),
                    ShardLifecycle::Initializing => false,
                    ShardLifecycle::Idle => {
                        guard.lifecycle = ShardLifecycle::Initializing;
                        true
                    }
                }
            };

            if !should_subscribe {
                state.ready.notified().await;
                continue;
            }

            let mut stream = match self.pubsub.subscribe(&topic).await {
                Ok(stream) => stream,
                Err(err) => {
                    let mut guard = state.state.lock().await;
                    guard.lifecycle = ShardLifecycle::Idle;
                    state.ready.notify_waiters();
                    return Err(err);
                }
            };

            {
                let mut guard = state.state.lock().await;
                guard.lifecycle = ShardLifecycle::Started;
            }
            state.ready.notify_waiters();

            tokio::spawn(async move {
                use futures::StreamExt;
                use prost::Message;

                while let Some(bytes) = stream.next().await {
                    let event = match SessionStepEvent::decode(bytes.as_slice()) {
                        Ok(event) => event,
                        Err(err) => {
                            tracing::error!(
                                "Failed to decode session step event from shard {}: {}",
                                shard,
                                err
                            );
                            continue;
                        }
                    };

                    let session_id = event.session_id.clone();
                    let listeners = {
                        let mut guard = state.state.lock().await;
                        let Some(entries) = guard.listeners.get_mut(&session_id) else {
                            continue;
                        };

                        entries.retain(|sender| !sender.is_closed());
                        entries.clone()
                    };

                    for sender in listeners {
                        let _ = sender.send(Ok(event.clone()));
                    }
                }

                // Drop all outstanding senders for this shard so subscribers see EOF
                // if the backing pubsub stream terminates.
                let mut guard = state.state.lock().await;
                guard.listeners.clear();
                guard.lifecycle = ShardLifecycle::Idle;
                state.ready.notify_waiters();
            });

            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SessionStreamHub;
    use crate::control::{
        events::{SessionStepEvent, StepType},
        topics, MessagePublisher,
    };
    use prost::Message;
    use std::collections::{HashMap, VecDeque};
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct FakePubSub {
        batches: Mutex<HashMap<String, VecDeque<Vec<Vec<u8>>>>>,
        subscribe_calls: Mutex<Vec<String>>,
        fail_once_topics: Mutex<HashMap<String, usize>>,
    }

    #[async_trait::async_trait]
    impl MessagePublisher for FakePubSub {
        async fn publish(&self, _topic: &str, _message: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe(
            &self,
            topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            self.subscribe_calls.lock().await.push(topic.to_string());

            if let Some(remaining) = self.fail_once_topics.lock().await.get_mut(topic) {
                if *remaining > 0 {
                    *remaining -= 1;
                    anyhow::bail!("subscribe failed for {}", topic);
                }
            }

            let batch = self
                .batches
                .lock()
                .await
                .get_mut(topic)
                .and_then(|entries| entries.pop_front())
                .unwrap_or_default();
            Ok(Box::pin(futures::stream::iter(batch)))
        }
    }

    fn step_event(session_id: &str, content: &str) -> SessionStepEvent {
        SessionStepEvent {
            session_id: session_id.to_string(),
            step_type: StepType::Token as i32,
            content: content.to_string(),
            timestamp: 1,
            agent: "agent".to_string(),
            ns: "default".to_string(),
            message_id: "msg-1".to_string(),
            name: String::new(),
            payload_json: String::new(),
        }
    }

    #[tokio::test]
    async fn subscribe_retries_after_initial_pubsub_failure() {
        let session_id = "session-retry";
        let topic = topics::session_step_topic_for_shard(topics::session_step_shard(session_id));
        let pubsub = Arc::new(FakePubSub::default());
        pubsub
            .fail_once_topics
            .lock()
            .await
            .insert(topic.clone(), 1);
        pubsub.batches.lock().await.insert(
            topic.clone(),
            VecDeque::from(vec![vec![step_event(session_id, "hello").encode_to_vec()]]),
        );
        let hub = SessionStreamHub::new(pubsub.clone());

        let first = hub.subscribe(session_id).await;
        assert!(first.is_err());

        let mut receiver = hub
            .subscribe(session_id)
            .await
            .expect("retry should succeed");
        let event = receiver
            .recv()
            .await
            .expect("event should be delivered")
            .expect("event should decode");
        assert_eq!(event.content, "hello");

        let calls = pubsub.subscribe_calls.lock().await.clone();
        assert_eq!(calls, vec![topic.clone(), topic]);
    }

    #[tokio::test]
    async fn concurrent_subscribe_does_not_leave_shard_started_after_failed_subscribe() {
        let session_id = "session-concurrent-retry";
        let topic = topics::session_step_topic_for_shard(topics::session_step_shard(session_id));
        let pubsub = Arc::new(FakePubSub::default());
        pubsub
            .fail_once_topics
            .lock()
            .await
            .insert(topic.clone(), 1);
        pubsub.batches.lock().await.insert(
            topic.clone(),
            VecDeque::from(vec![vec![step_event(session_id, "hello").encode_to_vec()]]),
        );
        let hub = Arc::new(SessionStreamHub::new(pubsub.clone()));

        let first = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.subscribe(session_id).await })
        };
        let second = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.subscribe(session_id).await })
        };

        let first = first.await.expect("first task panicked");
        let second = second.await.expect("second task panicked");
        assert!(
            first.is_err() || second.is_err(),
            "one subscribe should observe the transient failure"
        );

        let mut receiver = if let Ok(receiver) = first {
            receiver
        } else {
            second.expect("one subscribe should recover after retry")
        };
        let event = receiver
            .recv()
            .await
            .expect("event should be delivered")
            .expect("event should decode");
        assert_eq!(event.content, "hello");
        assert_eq!(pubsub.subscribe_calls.lock().await.len(), 2);
    }

    #[tokio::test]
    async fn subscribe_skips_invalid_events_and_reinitializes_after_stream_end() {
        let session_id = "session-end";
        let topic = topics::session_step_topic_for_shard(topics::session_step_shard(session_id));
        let pubsub = Arc::new(FakePubSub::default());
        pubsub.batches.lock().await.insert(
            topic.clone(),
            VecDeque::from(vec![
                vec![
                    b"not-protobuf".to_vec(),
                    step_event(session_id, "first").encode_to_vec(),
                ],
                vec![step_event(session_id, "second").encode_to_vec()],
            ]),
        );
        let hub = SessionStreamHub::new(pubsub.clone());

        let mut first = hub
            .subscribe(session_id)
            .await
            .expect("subscribe should succeed");
        let first_event = first
            .recv()
            .await
            .expect("first stream should yield")
            .expect("first event should decode");
        assert_eq!(first_event.content, "first");
        assert!(first.recv().await.is_none());

        let mut second = hub
            .subscribe(session_id)
            .await
            .expect("resubscribe should succeed");
        let second_event = second
            .recv()
            .await
            .expect("second stream should yield")
            .expect("second event should decode");
        assert_eq!(second_event.content, "second");

        assert_eq!(pubsub.subscribe_calls.lock().await.len(), 2);
    }
}
