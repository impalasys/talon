use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex, OnceCell};

use crate::control::{events::SessionStepEvent, topics, MessagePublisher};

#[derive(Default)]
struct ShardState {
    started: bool,
    listeners: HashMap<String, Vec<mpsc::UnboundedSender<Result<SessionStepEvent, tonic::Status>>>>,
}

pub struct SessionStreamHub {
    pubsub: Arc<dyn MessagePublisher + Send + Sync>,
    shards: Vec<Arc<Mutex<ShardState>>>,
    initialized: OnceCell<()>,
}

impl SessionStreamHub {
    pub fn new(pubsub: Arc<dyn MessagePublisher + Send + Sync>) -> Self {
        let shard_count = topics::session_step_shard_count() as usize;
        let shards = (0..shard_count)
            .map(|_| Arc::new(Mutex::new(ShardState::default())))
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
            let mut guard = state.lock().await;
            guard
                .listeners
                .entry(session_id.to_string())
                .or_default()
                .push(tx);
        }

        if let Err(err) = self.ensure_shard_task(shard, state.clone()).await {
            let mut guard = state.lock().await;
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

    async fn ensure_shard_task(
        &self,
        shard: usize,
        state: Arc<Mutex<ShardState>>,
    ) -> anyhow::Result<()> {
        {
            let mut guard = state.lock().await;
            if guard.started {
                return Ok(());
            }
            guard.started = true;
        }

        let topic = topics::session_step_topic_for_shard(shard as u32);
        let mut stream = self.pubsub.subscribe(&topic).await?;
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
                    let mut guard = state.lock().await;
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
            let mut guard = state.lock().await;
            guard.listeners.clear();
            guard.started = false;
        });

        Ok(())
    }
}
