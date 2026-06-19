// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, Notify, OnceCell};

use crate::control::{events::SessionMessagePartEvent, keys, topics, MessagePublisher};

type SessionStreamItem = Result<SessionMessagePartEvent, tonic::Status>;
type SessionStreamSender = mpsc::UnboundedSender<SessionStreamItem>;
type SessionStreamRawReceiver = mpsc::UnboundedReceiver<SessionStreamItem>;

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
    listeners: HashMap<String, Vec<SessionStreamSender>>,
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

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SessionStreamTarget {
    pub ns: String,
    pub agent: String,
    pub session_id: String,
}

impl SessionStreamTarget {
    pub fn new(
        ns: impl Into<String>,
        agent: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            ns: ns.into(),
            agent: agent.into(),
            session_id: session_id.into(),
        }
    }

    fn listener_key(&self) -> String {
        keys::session(&self.ns, &self.agent, &self.session_id).canonical()
    }

    fn shard(&self) -> usize {
        topics::session_part_shard(&self.session_id) as usize
    }
}

pub struct SessionStreamHub {
    pubsub: Arc<dyn MessagePublisher + Send + Sync>,
    shards: Vec<Arc<Shard>>,
    initialized: OnceCell<()>,
}

pub struct SessionStreamReceiver {
    receiver: SessionStreamRawReceiver,
    cleanup: Option<SessionStreamCleanup>,
}

struct SessionStreamCleanup {
    tx: mpsc::WeakUnboundedSender<SessionStreamItem>,
    listeners: Vec<(Arc<Shard>, Vec<String>)>,
}

impl SessionStreamReceiver {
    pub async fn recv(&mut self) -> Option<SessionStreamItem> {
        self.receiver.recv().await
    }
}

impl Drop for SessionStreamReceiver {
    fn drop(&mut self) {
        let Some(cleanup) = self.cleanup.take() else {
            return;
        };
        let Some(tx) = cleanup.tx.upgrade() else {
            return;
        };
        for (state, listener_keys) in cleanup.listeners {
            remove_listeners(&state, &listener_keys, &tx);
        }
    }
}

impl SessionStreamHub {
    pub fn new(pubsub: Arc<dyn MessagePublisher + Send + Sync>) -> Self {
        let shard_count = topics::session_part_shard_count() as usize;
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
        ns: &str,
        agent: &str,
        session_id: &str,
    ) -> anyhow::Result<SessionStreamReceiver> {
        self.subscribe_many(vec![SessionStreamTarget::new(ns, agent, session_id)])
            .await
    }

    pub async fn subscribe_many(
        &self,
        targets: Vec<SessionStreamTarget>,
    ) -> anyhow::Result<SessionStreamReceiver> {
        if targets.is_empty() {
            anyhow::bail!("session stream batch must contain at least one target");
        }

        let _ = self.initialized.get_or_init(|| async {}).await;

        let (tx, rx) = mpsc::unbounded_channel();

        let mut inserted = Vec::new();
        let mut shards_to_ensure = HashMap::new();
        let mut targets_by_shard: HashMap<usize, Vec<String>> = HashMap::new();
        let mut shard_states = HashMap::new();
        let mut seen = HashSet::new();

        for target in targets {
            let listener_key = target.listener_key();
            if !seen.insert(listener_key.clone()) {
                continue;
            }

            let shard = target.shard();
            let state = self
                .shards
                .get(shard)
                .ok_or_else(|| anyhow::anyhow!("Invalid session shard {}", shard))?
                .clone();

            targets_by_shard
                .entry(shard)
                .or_default()
                .push(listener_key);
            shard_states.entry(shard).or_insert(state);
        }

        for (shard, listener_keys) in targets_by_shard {
            let state = shard_states
                .get(&shard)
                .expect("state should exist for grouped shard")
                .clone();
            let mut guard = state.state.lock().unwrap();
            for listener_key in &listener_keys {
                guard
                    .listeners
                    .entry(listener_key.clone())
                    .or_default()
                    .push(tx.clone());
            }
            drop(guard);

            inserted.push((state.clone(), listener_keys));
            shards_to_ensure.entry(shard).or_insert(state);
        }

        for (shard, state) in shards_to_ensure {
            if let Err(err) = self.ensure_shard_task(shard, state.clone()).await {
                for (state, listener_keys) in inserted {
                    remove_listeners(&state, &listener_keys, &tx);
                }
                return Err(err);
            }
        }

        Ok(SessionStreamReceiver {
            receiver: rx,
            cleanup: Some(SessionStreamCleanup {
                tx: tx.downgrade(),
                listeners: inserted,
            }),
        })
    }

    async fn ensure_shard_task(&self, shard: usize, state: Arc<Shard>) -> anyhow::Result<()> {
        let topic = topics::session_part_topic_for_shard(shard as u32);
        loop {
            let should_subscribe = {
                let mut guard = state.state.lock().unwrap();
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
                    let mut guard = state.state.lock().unwrap();
                    guard.lifecycle = ShardLifecycle::Idle;
                    state.ready.notify_waiters();
                    return Err(err);
                }
            };

            {
                let mut guard = state.state.lock().unwrap();
                guard.lifecycle = ShardLifecycle::Started;
            }
            state.ready.notify_waiters();

            tokio::spawn(async move {
                use futures::StreamExt;
                use prost::Message;

                while let Some(bytes) = stream.next().await {
                    let is_idle = {
                        let mut guard = state.state.lock().unwrap();
                        if guard.listeners.is_empty() {
                            guard.lifecycle = ShardLifecycle::Idle;
                            true
                        } else {
                            false
                        }
                    };
                    if is_idle {
                        state.ready.notify_waiters();
                        break;
                    }

                    let event = match SessionMessagePartEvent::decode(bytes.as_slice()) {
                        Ok(event) => event,
                        Err(err) => {
                            tracing::error!(
                                "Failed to decode session part event from shard {}: {}",
                                shard,
                                err
                            );
                            continue;
                        }
                    };

                    let listener_key =
                        keys::session(&event.ns, &event.agent, &event.session_id).canonical();
                    let listeners = {
                        let mut guard = state.state.lock().unwrap();
                        let Some(entries) = guard.listeners.get_mut(&listener_key) else {
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
                let mut guard = state.state.lock().unwrap();
                guard.listeners.clear();
                guard.lifecycle = ShardLifecycle::Idle;
                state.ready.notify_waiters();
            });

            return Ok(());
        }
    }
}

fn remove_listeners(state: &Arc<Shard>, listener_keys: &[String], tx: &SessionStreamSender) {
    let mut guard = state.state.lock().unwrap();
    for listener_key in listener_keys {
        if let Some(entries) = guard.listeners.get_mut(listener_key) {
            entries.retain(|sender| !sender.same_channel(tx));
            if entries.is_empty() {
                guard.listeners.remove(listener_key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionStreamHub, SessionStreamTarget};
    use crate::control::{
        events::{SessionMessagePartEvent, SessionMessagePartEventKind},
        keys, topics, MessagePublisher,
    };
    use crate::gateway::rpc::data_proto::{SessionMessagePart, SessionMessagePartType};
    use prost::Message;
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct FakePubSub {
        batches: Mutex<HashMap<String, VecDeque<Vec<Vec<u8>>>>>,
        subscribe_calls: Mutex<Vec<String>>,
        fail_once_topics: Mutex<HashMap<String, usize>>,
        pending_topics: Mutex<HashSet<String>>,
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

            if self.pending_topics.lock().await.contains(topic) {
                return Ok(Box::pin(futures::stream::pending()));
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

    fn part_event_for(
        ns: &str,
        agent: &str,
        session_id: &str,
        content: &str,
    ) -> SessionMessagePartEvent {
        SessionMessagePartEvent {
            session_id: session_id.to_string(),
            kind: SessionMessagePartEventKind::Delta as i32,
            part: Some(SessionMessagePart {
                id: String::new(),
                part_type: SessionMessagePartType::Text as i32,
                content: content.to_string(),
                name: String::new(),
                payload_json: String::new(),
                created_at: 1,
                object: None,
            }),
            timestamp: 1,
            agent: agent.to_string(),
            ns: ns.to_string(),
            message_id: "msg-1".to_string(),
        }
    }

    fn part_event(session_id: &str, content: &str) -> SessionMessagePartEvent {
        part_event_for("default", "agent", session_id, content)
    }

    fn event_content(event: &SessionMessagePartEvent) -> &str {
        event.part.as_ref().expect("event part").content.as_str()
    }

    #[tokio::test]
    async fn subscribe_retries_after_initial_pubsub_failure() {
        let session_id = "session-retry";
        let topic = topics::session_part_topic_for_shard(topics::session_part_shard(session_id));
        let pubsub = Arc::new(FakePubSub::default());
        pubsub
            .fail_once_topics
            .lock()
            .await
            .insert(topic.clone(), 1);
        pubsub.batches.lock().await.insert(
            topic.clone(),
            VecDeque::from(vec![vec![part_event(session_id, "hello").encode_to_vec()]]),
        );
        let hub = SessionStreamHub::new(pubsub.clone());

        let first = hub.subscribe("default", "agent", session_id).await;
        assert!(first.is_err());

        let mut receiver = hub
            .subscribe("default", "agent", session_id)
            .await
            .expect("retry should succeed");
        let event = receiver
            .recv()
            .await
            .expect("event should be delivered")
            .expect("event should decode");
        assert_eq!(event_content(&event), "hello");

        let calls = pubsub.subscribe_calls.lock().await.clone();
        assert_eq!(calls, vec![topic.clone(), topic]);
    }

    #[tokio::test]
    async fn concurrent_subscribe_does_not_leave_shard_started_after_failed_subscribe() {
        let session_id = "session-concurrent-retry";
        let topic = topics::session_part_topic_for_shard(topics::session_part_shard(session_id));
        let pubsub = Arc::new(FakePubSub::default());
        pubsub
            .fail_once_topics
            .lock()
            .await
            .insert(topic.clone(), 1);
        pubsub.batches.lock().await.insert(
            topic.clone(),
            VecDeque::from(vec![vec![part_event(session_id, "hello").encode_to_vec()]]),
        );
        let hub = Arc::new(SessionStreamHub::new(pubsub.clone()));

        let first = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.subscribe("default", "agent", session_id).await })
        };
        let second = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.subscribe("default", "agent", session_id).await })
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
        assert_eq!(event_content(&event), "hello");
        assert_eq!(pubsub.subscribe_calls.lock().await.len(), 2);
    }

    #[tokio::test]
    async fn subscribe_skips_invalid_events_and_reinitializes_after_stream_end() {
        let session_id = "session-end";
        let topic = topics::session_part_topic_for_shard(topics::session_part_shard(session_id));
        let pubsub = Arc::new(FakePubSub::default());
        pubsub.batches.lock().await.insert(
            topic.clone(),
            VecDeque::from(vec![
                vec![
                    b"not-protobuf".to_vec(),
                    part_event(session_id, "first").encode_to_vec(),
                ],
                vec![part_event(session_id, "second").encode_to_vec()],
            ]),
        );
        let hub = SessionStreamHub::new(pubsub.clone());

        let mut first = hub
            .subscribe("default", "agent", session_id)
            .await
            .expect("subscribe should succeed");
        let first_event = first
            .recv()
            .await
            .expect("first stream should yield")
            .expect("first event should decode");
        assert_eq!(event_content(&first_event), "first");
        assert!(first.recv().await.is_none());

        let mut second = hub
            .subscribe("default", "agent", session_id)
            .await
            .expect("resubscribe should succeed");
        let second_event = second
            .recv()
            .await
            .expect("second stream should yield")
            .expect("second event should decode");
        assert_eq!(event_content(&second_event), "second");

        assert_eq!(pubsub.subscribe_calls.lock().await.len(), 2);
    }

    #[tokio::test]
    async fn subscribe_uses_canonical_session_identity() {
        let session_id = "shared-session-id";
        let topic = topics::session_part_topic_for_shard(topics::session_part_shard(session_id));
        let pubsub = Arc::new(FakePubSub::default());
        pubsub.batches.lock().await.insert(
            topic,
            VecDeque::from(vec![vec![
                part_event_for("default", "other-agent", session_id, "wrong").encode_to_vec(),
                part_event_for("default", "agent", session_id, "right").encode_to_vec(),
            ]]),
        );
        let hub = SessionStreamHub::new(pubsub);

        let mut receiver = hub
            .subscribe("default", "agent", session_id)
            .await
            .expect("subscribe should succeed");
        let event = receiver
            .recv()
            .await
            .expect("event should be delivered")
            .expect("event should decode");
        assert_eq!(event_content(&event), "right");
        assert!(receiver.recv().await.is_none());
    }

    #[tokio::test]
    async fn subscribe_many_delivers_multiple_sessions_on_one_receiver() {
        let first_id = "session-one";
        let second_id = "session-two";
        let first_topic =
            topics::session_part_topic_for_shard(topics::session_part_shard(first_id));
        let second_topic =
            topics::session_part_topic_for_shard(topics::session_part_shard(second_id));
        let pubsub = Arc::new(FakePubSub::default());
        pubsub.batches.lock().await.insert(
            first_topic,
            VecDeque::from(vec![vec![part_event(first_id, "first").encode_to_vec()]]),
        );
        pubsub.batches.lock().await.insert(
            second_topic,
            VecDeque::from(vec![vec![part_event(second_id, "second").encode_to_vec()]]),
        );
        let hub = SessionStreamHub::new(pubsub);

        let mut receiver = hub
            .subscribe_many(vec![
                SessionStreamTarget::new("default", "agent", first_id),
                SessionStreamTarget::new("default", "agent", second_id),
            ])
            .await
            .expect("batch subscribe should succeed");

        let mut contents = Vec::new();
        while let Some(event) = receiver.recv().await {
            let event = event.expect("event should decode");
            contents.push(event_content(&event).to_string());
            if contents.len() == 2 {
                break;
            }
        }
        contents.sort();
        assert_eq!(contents, vec!["first", "second"]);
    }

    #[tokio::test]
    async fn subscribe_many_cleans_up_listeners_when_receiver_drops() {
        let first_id = "session-cleanup-one";
        let second_id = "session-cleanup-two";
        let first_topic =
            topics::session_part_topic_for_shard(topics::session_part_shard(first_id));
        let second_topic =
            topics::session_part_topic_for_shard(topics::session_part_shard(second_id));
        let pubsub = Arc::new(FakePubSub::default());
        {
            let mut pending = pubsub.pending_topics.lock().await;
            pending.insert(first_topic);
            pending.insert(second_topic);
        }
        let hub = SessionStreamHub::new(pubsub);

        let receiver = hub
            .subscribe_many(vec![
                SessionStreamTarget::new("default", "agent", first_id),
                SessionStreamTarget::new("default", "agent", second_id),
            ])
            .await
            .expect("batch subscribe should succeed");

        assert_listeners(&hub, &[first_id, second_id], 1).await;
        drop(receiver);
        wait_for_listeners(&hub, &[first_id, second_id], 0).await;
    }

    async fn assert_listeners(hub: &SessionStreamHub, session_ids: &[&str], expected: usize) {
        for session_id in session_ids {
            let shard = topics::session_part_shard(session_id) as usize;
            let state = hub.shards.get(shard).expect("valid shard");
            let listener_key = keys::session("default", "agent", session_id).canonical();
            let guard = state.state.lock().unwrap();
            let actual = guard
                .listeners
                .get(&listener_key)
                .map_or(0, std::vec::Vec::len);
            assert_eq!(actual, expected, "listener count for {session_id}");
        }
    }

    async fn wait_for_listeners(hub: &SessionStreamHub, session_ids: &[&str], expected: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let mut all_match = true;
                for session_id in session_ids {
                    let shard = topics::session_part_shard(session_id) as usize;
                    let state = hub.shards.get(shard).expect("valid shard");
                    let listener_key = keys::session("default", "agent", session_id).canonical();
                    let guard = state.state.lock().unwrap();
                    let actual = guard
                        .listeners
                        .get(&listener_key)
                        .map_or(0, std::vec::Vec::len);
                    if actual != expected {
                        all_match = false;
                        break;
                    }
                }
                if all_match {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("listeners should reach expected count");
    }
}
