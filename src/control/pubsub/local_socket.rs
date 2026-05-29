// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::MessagePublisher;
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, Mutex, OnceCell};

type SubscriberSender = mpsc::Sender<Vec<u8>>;
const MAX_FRAME_SIZE: usize = 32 * 1024 * 1024;
const DEFAULT_SUBSCRIBER_BUFFER_SIZE: usize = 1024;

#[derive(Default)]
struct SubscriptionState {
    next_index: usize,
    subscribers: Vec<SubscriberSender>,
}

#[derive(Default)]
struct BrokerState {
    topics: HashMap<String, HashMap<String, SubscriptionState>>,
}

#[derive(Clone)]
pub struct LocalSocketMessagePublisher {
    socket_path: Arc<PathBuf>,
    server_started: Arc<OnceCell<()>>,
    connection: Arc<Mutex<Option<UnixStream>>>,
}

#[derive(Clone)]
pub struct LocalSocketSubscriber {
    socket_path: Arc<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientFrame {
    Publish {
        request_id: u64,
        topic: String,
        payload_b64: String,
    },
    Subscribe {
        request_id: u64,
        topic: String,
        subscription: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerFrame {
    Ack { request_id: u64 },
    Error { request_id: u64, message: String },
    Delivery { topic: String, payload_b64: String },
}

impl LocalSocketMessagePublisher {
    pub async fn new(socket_path: PathBuf) -> Result<Self> {
        let publisher = Self {
            socket_path: Arc::new(socket_path),
            server_started: Arc::new(OnceCell::new()),
            connection: Arc::new(Mutex::new(None)),
        };
        publisher.ensure_server().await?;
        Ok(publisher)
    }

    pub fn subscriber(&self) -> LocalSocketSubscriber {
        LocalSocketSubscriber {
            socket_path: self.socket_path.clone(),
        }
    }

    async fn ensure_server(&self) -> Result<()> {
        if self.server_started.get().is_some() {
            return Ok(());
        }
        start_or_connect_server(&self.socket_path).await?;
        let _ = self.server_started.set(());
        Ok(())
    }
}

impl LocalSocketSubscriber {
    pub async fn subscribe_named(
        &self,
        topic: &str,
        subscription: &str,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
        let mut stream = connect_stream(&self.socket_path).await?;
        let request_id = 1;
        write_frame(
            &mut stream,
            &ClientFrame::Subscribe {
                request_id,
                topic: topic.to_string(),
                subscription: subscription.to_string(),
            },
        )
        .await?;
        match read_frame::<_, ServerFrame>(&mut stream).await? {
            Some(ServerFrame::Ack { request_id: ack_id }) if ack_id == request_id => {}
            Some(ServerFrame::Error { message, .. }) => {
                return Err(anyhow!("local socket subscribe failed: {}", message));
            }
            Some(other) => {
                return Err(anyhow!(
                    "unexpected local socket subscribe response: {:?}",
                    other
                ));
            }
            None => {
                return Err(anyhow!("local socket broker closed before subscribe ack"));
            }
        }

        let output = async_stream::stream! {
            loop {
                match read_frame::<_, ServerFrame>(&mut stream).await {
                    Ok(Some(ServerFrame::Delivery { payload_b64, .. })) => {
                        match general_purpose::STANDARD.decode(payload_b64) {
                            Ok(payload) => yield payload,
                            Err(err) => {
                                tracing::error!(error = %err, "failed to decode local socket delivery");
                                break;
                            }
                        }
                    }
                    Ok(Some(ServerFrame::Ack { .. })) => {}
                    Ok(Some(ServerFrame::Error { message, .. })) => {
                        tracing::error!(message = %message, "local socket broker returned stream error");
                        break;
                    }
                    Ok(None) => break,
                    Err(err) => {
                        tracing::error!(error = %err, "local socket subscription stream failed");
                        break;
                    }
                }
            }
        };
        Ok(Box::pin(output))
    }
}

#[async_trait::async_trait]
impl MessagePublisher for LocalSocketMessagePublisher {
    async fn publish(&self, topic: &str, message: &[u8]) -> Result<()> {
        self.ensure_server().await?;
        let mut connection = self.connection.lock().await;
        if connection.is_none() {
            *connection = Some(connect_stream(&self.socket_path).await?);
        }

        let payload_b64 = general_purpose::STANDARD.encode(message);
        if let Err(err) = publish_with_stream(
            connection
                .as_mut()
                .expect("connection should be initialized"),
            topic,
            &payload_b64,
        )
        .await
        {
            tracing::warn!(error = %err, topic = %topic, "local socket publish failed; reconnecting");
            *connection = Some(connect_stream(&self.socket_path).await?);
            let stream = connection
                .as_mut()
                .expect("connection should be reinitialized");
            if let Err(retry_err) = publish_with_stream(stream, topic, &payload_b64).await {
                *connection = None;
                return Err(retry_err);
            }
        }

        Ok(())
    }

    async fn subscribe(
        &self,
        topic: &str,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
        self.ensure_server().await?;
        let subscription = format!("local-sub-{}", uuid::Uuid::now_v7());
        self.subscriber()
            .subscribe_named(topic, &subscription)
            .await
    }
}

async fn start_or_connect_server(path: &Path) -> Result<()> {
    if connect_stream(path).await.is_ok() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    remove_stale_socket(path).await?;
    match UnixListener::bind(path) {
        Ok(listener) => {
            let state = Arc::new(Mutex::new(BrokerState::default()));
            tokio::spawn(run_server(listener, state));
            Ok(())
        }
        Err(err) => {
            if connect_stream(path).await.is_ok() {
                return Ok(());
            }
            Err(err).with_context(|| {
                format!("failed to bind local socket broker at {}", path.display())
            })
        }
    }
}

async fn connect_stream(path: &Path) -> Result<UnixStream> {
    UnixStream::connect(path).await.with_context(|| {
        format!(
            "failed to connect to local socket broker at {}",
            path.display()
        )
    })
}

async fn publish_with_stream(
    stream: &mut UnixStream,
    topic: &str,
    payload_b64: &str,
) -> Result<()> {
    let request_id = 1;
    write_frame(
        stream,
        &ClientFrame::Publish {
            request_id,
            topic: topic.to_string(),
            payload_b64: payload_b64.to_string(),
        },
    )
    .await?;
    match read_frame::<_, ServerFrame>(stream).await? {
        Some(ServerFrame::Ack { request_id: ack_id }) if ack_id == request_id => Ok(()),
        Some(ServerFrame::Error { message, .. }) => {
            Err(anyhow!("local socket publish failed: {}", message))
        }
        Some(other) => Err(anyhow!(
            "unexpected local socket publish response: {:?}",
            other
        )),
        None => Err(anyhow!("local socket broker closed before publish ack")),
    }
}

async fn remove_stale_socket(path: &Path) -> Result<()> {
    let Ok(metadata) = tokio::fs::metadata(path).await else {
        return Ok(());
    };
    if !metadata.file_type().is_socket() {
        return Err(anyhow!(
            "refusing to remove non-socket path at {}",
            path.display()
        ));
    }
    tokio::fs::remove_file(path).await.with_context(|| {
        format!(
            "failed to remove stale local socket broker at {}",
            path.display()
        )
    })
}

async fn run_server(listener: UnixListener, state: Arc<Mutex<BrokerState>>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_connection(stream, state).await {
                        tracing::error!(error = %err, "local socket broker connection failed");
                    }
                });
            }
            Err(err) => {
                tracing::error!(error = %err, "local socket broker accept failed");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

async fn handle_connection(stream: UnixStream, state: Arc<Mutex<BrokerState>>) -> Result<()> {
    let (mut reader, mut writer) = stream.into_split();
    while let Some(frame) = read_frame::<_, ClientFrame>(&mut reader).await? {
        match frame {
            ClientFrame::Publish {
                request_id,
                topic,
                payload_b64,
            } => {
                let payload = general_purpose::STANDARD
                    .decode(payload_b64)
                    .map_err(|err| anyhow!("invalid publish payload encoding: {}", err))?;
                distribute_message(&state, &topic, payload).await;
                write_frame(&mut writer, &ServerFrame::Ack { request_id }).await?;
            }
            ClientFrame::Subscribe {
                request_id,
                topic,
                subscription,
            } => {
                let (tx, mut rx) = mpsc::channel::<Vec<u8>>(subscriber_buffer_size());
                register_subscriber(&state, &topic, &subscription, tx).await;
                write_frame(&mut writer, &ServerFrame::Ack { request_id }).await?;
                while let Some(payload) = rx.recv().await {
                    write_frame(
                        &mut writer,
                        &ServerFrame::Delivery {
                            topic: topic.clone(),
                            payload_b64: general_purpose::STANDARD.encode(payload),
                        },
                    )
                    .await?;
                }
                return Ok(());
            }
        }
    }
    Ok(())
}

async fn register_subscriber(
    state: &Arc<Mutex<BrokerState>>,
    topic: &str,
    subscription: &str,
    sender: SubscriberSender,
) {
    let mut state = state.lock().await;
    let subscription_state = state
        .topics
        .entry(topic.to_string())
        .or_default()
        .entry(subscription.to_string())
        .or_default();
    subscription_state.subscribers.push(sender);
}

async fn distribute_message(state: &Arc<Mutex<BrokerState>>, topic: &str, payload: Vec<u8>) {
    let targets = {
        let mut state = state.lock().await;
        let Some(subscriptions) = state.topics.get_mut(topic) else {
            return;
        };
        let mut targets = Vec::new();
        let mut empty_subscriptions = Vec::new();
        for (subscription_name, subscription_state) in subscriptions.iter_mut() {
            subscription_state
                .subscribers
                .retain(|sender| !sender.is_closed());
            if subscription_state.subscribers.is_empty() {
                empty_subscriptions.push(subscription_name.clone());
                continue;
            }
            let index = subscription_state.next_index % subscription_state.subscribers.len();
            let sender = subscription_state.subscribers[index].clone();
            subscription_state.next_index =
                (subscription_state.next_index + 1) % subscription_state.subscribers.len();
            targets.push(sender);
        }
        for subscription_name in empty_subscriptions {
            subscriptions.remove(&subscription_name);
        }
        targets
    };

    let target_count = targets.len();
    let mut payload = Some(payload);
    for (index, sender) in targets.into_iter().enumerate() {
        let message = if index + 1 == target_count {
            payload
                .take()
                .expect("payload should be present for final subscriber")
        } else {
            payload
                .as_ref()
                .expect("payload should be present before final subscriber")
                .clone()
        };
        if let Err(err) = sender.try_send(message) {
            tracing::warn!(error = %err, topic = %topic, "local socket broker failed to deliver message");
        }
    }
}

fn subscriber_buffer_size() -> usize {
    std::env::var("TALON_LOCAL_SOCKET_SUBSCRIBER_BUFFER_SIZE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_SUBSCRIBER_BUFFER_SIZE)
}

async fn write_frame<W: AsyncWriteExt + Unpin, T: Serialize>(
    writer: &mut W,
    frame: &T,
) -> Result<()> {
    let bytes = serde_json::to_vec(frame)?;
    if bytes.len() > MAX_FRAME_SIZE {
        return Err(anyhow!("frame too large: {}", bytes.len()));
    }
    let len = u32::try_from(bytes.len()).context("frame too large")?;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

async fn read_frame<R: AsyncReadExt + Unpin, T: for<'de> Deserialize<'de>>(
    reader: &mut R,
) -> Result<Option<T>> {
    let mut len_buf = [0_u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err.into()),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_SIZE {
        return Err(anyhow!("frame too large: {}", len));
    }
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(len)
        .map_err(|err| anyhow!("failed to reserve frame buffer: {}", err))?;
    let bytes_read = reader.take(len as u64).read_to_end(&mut payload).await?;
    if bytes_read != len {
        return Err(anyhow!(
            "unexpected frame length: expected {} bytes, read {}",
            len,
            bytes_read
        ));
    }
    Ok(Some(serde_json::from_slice(&payload)?))
}

#[cfg(test)]
mod tests {
    use super::{
        read_frame, remove_stale_socket, write_frame, ClientFrame, LocalSocketMessagePublisher,
        LocalSocketSubscriber, MAX_FRAME_SIZE,
    };
    use crate::control::MessagePublisher;
    use base64::{engine::general_purpose, Engine as _};
    use futures::StreamExt;
    use tempfile::tempdir;
    use tokio::io::{duplex, AsyncWriteExt};

    async fn broker() -> (LocalSocketMessagePublisher, LocalSocketSubscriber) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("talon-broker.sock");
        let publisher = LocalSocketMessagePublisher::new(path).await.unwrap();
        let subscriber = publisher.subscriber();
        std::mem::forget(dir);
        (publisher, subscriber)
    }

    #[tokio::test]
    async fn publish_reaches_unique_subscription() {
        let (publisher, subscriber) = broker().await;
        let mut stream = subscriber.subscribe_named("events", "sub-1").await.unwrap();

        publisher.publish("events", b"hello").await.unwrap();
        assert_eq!(stream.next().await.unwrap(), b"hello".to_vec());
    }

    #[tokio::test]
    async fn multiple_group_subscribers_share_delivery() {
        let (publisher, subscriber) = broker().await;
        let mut first = subscriber
            .subscribe_named("events", "workers")
            .await
            .unwrap();
        let mut second = subscriber
            .subscribe_named("events", "workers")
            .await
            .unwrap();

        publisher.publish("events", b"one").await.unwrap();
        publisher.publish("events", b"two").await.unwrap();

        let a = tokio::time::timeout(std::time::Duration::from_secs(1), first.next())
            .await
            .unwrap()
            .unwrap();
        let b = tokio::time::timeout(std::time::Duration::from_secs(1), second.next())
            .await
            .unwrap()
            .unwrap();
        let mut got = vec![a, b];
        got.sort();
        assert_eq!(got, vec![b"one".to_vec(), b"two".to_vec()]);
    }

    #[tokio::test]
    async fn distinct_subscriptions_each_receive_copy() {
        let (publisher, subscriber) = broker().await;
        let mut first = subscriber.subscribe_named("events", "sub-a").await.unwrap();
        let mut second = subscriber.subscribe_named("events", "sub-b").await.unwrap();

        publisher.publish("events", b"hello").await.unwrap();
        assert_eq!(first.next().await.unwrap(), b"hello".to_vec());
        assert_eq!(second.next().await.unwrap(), b"hello".to_vec());
    }

    #[tokio::test]
    async fn publish_connection_can_send_multiple_frames() {
        let (publisher, subscriber) = broker().await;
        let mut first = subscriber.subscribe_named("events", "sub-a").await.unwrap();
        let mut stream = super::connect_stream(&publisher.socket_path).await.unwrap();

        write_frame(
            &mut stream,
            &ClientFrame::Publish {
                request_id: 1,
                topic: "events".to_string(),
                payload_b64: general_purpose::STANDARD.encode(b"one"),
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            read_frame::<_, super::ServerFrame>(&mut stream)
                .await
                .unwrap(),
            Some(super::ServerFrame::Ack { request_id: 1 })
        ));

        write_frame(
            &mut stream,
            &ClientFrame::Publish {
                request_id: 2,
                topic: "events".to_string(),
                payload_b64: general_purpose::STANDARD.encode(b"two"),
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            read_frame::<_, super::ServerFrame>(&mut stream)
                .await
                .unwrap(),
            Some(super::ServerFrame::Ack { request_id: 2 })
        ));

        assert_eq!(first.next().await.unwrap(), b"one".to_vec());
        assert_eq!(first.next().await.unwrap(), b"two".to_vec());
    }

    #[tokio::test]
    async fn read_frame_rejects_oversized_payloads() {
        let (mut writer, mut reader) = duplex(64);
        let writer_task = tokio::spawn(async move {
            writer
                .write_all(&((MAX_FRAME_SIZE + 1) as u32).to_be_bytes())
                .await
                .unwrap();
        });

        let err = read_frame::<_, ClientFrame>(&mut reader).await.unwrap_err();
        assert!(err.to_string().contains("frame too large"));
        writer_task.await.unwrap();
    }

    #[tokio::test]
    async fn remove_stale_socket_rejects_non_socket_files() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("not-a-socket");
        tokio::fs::write(&path, b"hello").await.unwrap();

        let err = remove_stale_socket(&path).await.unwrap_err();
        assert!(err
            .to_string()
            .contains("refusing to remove non-socket path"));
    }

    #[tokio::test]
    async fn write_frame_rejects_oversized_payloads() {
        let (mut writer, _) = duplex(64);
        let err = write_frame(
            &mut writer,
            &ClientFrame::Publish {
                request_id: 1,
                topic: "events".to_string(),
                payload_b64: "a".repeat(MAX_FRAME_SIZE),
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("frame too large"));
    }
}
