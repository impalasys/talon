// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::events::{SessionMessagePartEvent, SessionMessagePartEventKind};
use crate::gateway::rpc::worker_proto::{
    fanout_service_server::FanoutService, StreamSessionPartsBatchRequest,
    StreamSessionPartsRequest, StreamSessionPartsResponse,
};
use futures::stream::SelectAll;
use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tonic::{Request, Response, Status};

const DEFAULT_REPLAY_CAPACITY: usize = 256;
const DEFAULT_BROADCAST_CAPACITY: usize = 512;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SessionFanoutKey {
    pub ns: String,
    pub agent: String,
    pub session_id: String,
    pub submission_id: String,
    pub attempt_id: String,
}

impl SessionFanoutKey {
    pub fn new(
        ns: impl Into<String>,
        agent: impl Into<String>,
        session_id: impl Into<String>,
        submission_id: impl Into<String>,
        attempt_id: impl Into<String>,
    ) -> Self {
        Self {
            ns: ns.into(),
            agent: agent.into(),
            session_id: session_id.into(),
            submission_id: submission_id.into(),
            attempt_id: attempt_id.into(),
        }
    }
}

#[derive(Debug)]
pub enum FanoutSubscribeError {
    NotFound,
    ReplayGap,
}

pub struct FanoutSubscription {
    replay: Vec<StreamSessionPartsResponse>,
    receiver: broadcast::Receiver<StreamSessionPartsResponse>,
}

impl FanoutSubscription {
    pub fn into_stream(
        mut self,
    ) -> Pin<
        Box<
            dyn futures::Stream<Item = std::result::Result<StreamSessionPartsResponse, Status>>
                + Send,
        >,
    > {
        Box::pin(async_stream::stream! {
            for event in self.replay {
                let terminal = event
                    .event
                    .as_ref()
                    .is_some_and(session_part_event_is_terminal);
                yield Ok(event);
                if terminal {
                    return;
                }
            }

            loop {
                match self.receiver.recv().await {
                    Ok(event) => {
                        let terminal = event
                            .event
                            .as_ref()
                            .is_some_and(session_part_event_is_terminal);
                        yield Ok(event);
                        if terminal {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        yield Err(Status::out_of_range("session fanout replay gap"));
                        break;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }
}

struct SessionAttemptFanout {
    sender: broadcast::Sender<StreamSessionPartsResponse>,
    replay: VecDeque<StreamSessionPartsResponse>,
    next_sequence: u64,
}

impl SessionAttemptFanout {
    fn new() -> Self {
        let (sender, _) = broadcast::channel(DEFAULT_BROADCAST_CAPACITY);
        Self {
            sender,
            replay: VecDeque::with_capacity(DEFAULT_REPLAY_CAPACITY),
            next_sequence: 1,
        }
    }
}

#[derive(Default)]
pub struct FanoutHub {
    sessions: Mutex<HashMap<SessionFanoutKey, SessionAttemptFanout>>,
}

impl FanoutHub {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create_session_attempt(&self, key: SessionFanoutKey) {
        let mut sessions = self.sessions.lock().await;
        sessions
            .entry(key)
            .or_insert_with(SessionAttemptFanout::new);
    }

    pub async fn publish_session_part(
        &self,
        key: &SessionFanoutKey,
        event: SessionMessagePartEvent,
    ) {
        let mut sessions = self.sessions.lock().await;
        let fanout = sessions
            .entry(key.clone())
            .or_insert_with(SessionAttemptFanout::new);
        let response = StreamSessionPartsResponse {
            sequence: fanout.next_sequence,
            event: Some(event),
        };
        fanout.next_sequence = fanout.next_sequence.saturating_add(1);
        fanout.replay.push_back(response.clone());
        while fanout.replay.len() > DEFAULT_REPLAY_CAPACITY {
            fanout.replay.pop_front();
        }
        let _ = fanout.sender.send(response);
    }

    pub async fn subscribe_session_parts(
        &self,
        key: &SessionFanoutKey,
        after_sequence: u64,
    ) -> std::result::Result<FanoutSubscription, FanoutSubscribeError> {
        let sessions = self.sessions.lock().await;
        let Some(fanout) = sessions.get(key) else {
            return Err(FanoutSubscribeError::NotFound);
        };

        if after_sequence > 0 {
            if let Some(first) = fanout.replay.front() {
                if first.sequence > after_sequence.saturating_add(1) {
                    return Err(FanoutSubscribeError::ReplayGap);
                }
            }
        }

        let replay = fanout
            .replay
            .iter()
            .filter(|event| event.sequence > after_sequence)
            .cloned()
            .collect();
        Ok(FanoutSubscription {
            replay,
            receiver: fanout.sender.subscribe(),
        })
    }

    #[cfg(test)]
    pub async fn replay_session_part_events(
        &self,
        key: &SessionFanoutKey,
    ) -> Vec<SessionMessagePartEvent> {
        self.sessions
            .lock()
            .await
            .get(key)
            .map(|fanout| {
                fanout
                    .replay
                    .iter()
                    .filter_map(|response| response.event.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Clone)]
pub struct FanoutServiceImpl {
    hub: Arc<FanoutHub>,
}

impl FanoutServiceImpl {
    pub fn new(hub: Arc<FanoutHub>) -> Self {
        Self { hub }
    }
}

#[tonic::async_trait]
impl FanoutService for FanoutServiceImpl {
    type StreamSessionPartsStream = Pin<
        Box<
            dyn futures::Stream<Item = std::result::Result<StreamSessionPartsResponse, Status>>
                + Send,
        >,
    >;
    type StreamSessionPartsBatchStream = Pin<
        Box<
            dyn futures::Stream<Item = std::result::Result<StreamSessionPartsResponse, Status>>
                + Send,
        >,
    >;

    async fn stream_session_parts(
        &self,
        request: Request<StreamSessionPartsRequest>,
    ) -> std::result::Result<Response<Self::StreamSessionPartsStream>, Status> {
        let request = request.into_inner();
        let key = SessionFanoutKey::new(
            request.ns,
            request.agent,
            request.session_id,
            request.submission_id,
            request.attempt_id,
        );
        let subscription = self
            .hub
            .subscribe_session_parts(&key, request.after_sequence)
            .await
            .map_err(|err| match err {
                FanoutSubscribeError::NotFound => Status::not_found("session attempt not found"),
                FanoutSubscribeError::ReplayGap => {
                    Status::out_of_range("session fanout replay gap")
                }
            })?;
        Ok(Response::new(subscription.into_stream()))
    }

    async fn stream_session_parts_batch(
        &self,
        request: Request<StreamSessionPartsBatchRequest>,
    ) -> std::result::Result<Response<Self::StreamSessionPartsBatchStream>, Status> {
        let request = request.into_inner();
        if request.streams.is_empty() {
            return Err(Status::invalid_argument(
                "streams must contain at least one session",
            ));
        }

        let mut streams: SelectAll<Self::StreamSessionPartsBatchStream> = SelectAll::new();
        for request in request.streams {
            let key = SessionFanoutKey::new(
                request.ns,
                request.agent,
                request.session_id,
                request.submission_id,
                request.attempt_id,
            );
            let subscription = self
                .hub
                .subscribe_session_parts(&key, request.after_sequence)
                .await
                .map_err(|err| match err {
                    FanoutSubscribeError::NotFound => {
                        Status::not_found("session attempt not found")
                    }
                    FanoutSubscribeError::ReplayGap => {
                        Status::out_of_range("session fanout replay gap")
                    }
                })?;
            streams.push(subscription.into_stream());
        }

        Ok(Response::new(Box::pin(streams)))
    }
}

fn session_part_event_is_terminal(event: &SessionMessagePartEvent) -> bool {
    event.kind == SessionMessagePartEventKind::Done as i32
        || event.kind == SessionMessagePartEventKind::Error as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::events::SessionMessagePartEventKind;
    use futures::StreamExt;

    fn event(kind: SessionMessagePartEventKind, content: &str) -> SessionMessagePartEvent {
        event_for_session("session-1", kind, content)
    }

    fn event_for_session(
        session_id: &str,
        kind: SessionMessagePartEventKind,
        content: &str,
    ) -> SessionMessagePartEvent {
        SessionMessagePartEvent {
            session_id: session_id.to_string(),
            kind: kind as i32,
            part: Some(crate::gateway::rpc::data_proto::SessionMessagePart {
                content: content.to_string(),
                ..Default::default()
            }),
            timestamp: 1,
            agent: "agent".to_string(),
            ns: "ns".to_string(),
            message_id: "message-1".to_string(),
        }
    }

    fn key() -> SessionFanoutKey {
        SessionFanoutKey::new("ns", "agent", "session-1", "submission-1", "attempt-1")
    }

    fn key_for_session(session_id: &str) -> SessionFanoutKey {
        SessionFanoutKey::new("ns", "agent", session_id, "submission-1", "attempt-1")
    }

    #[tokio::test]
    async fn fanout_replays_events_after_sequence() {
        let hub = FanoutHub::new();
        let key = key();
        hub.create_session_attempt(key.clone()).await;
        hub.publish_session_part(&key, event(SessionMessagePartEventKind::Delta, "one"))
            .await;
        hub.publish_session_part(&key, event(SessionMessagePartEventKind::Delta, "two"))
            .await;

        let subscription = hub.subscribe_session_parts(&key, 1).await.unwrap();
        assert_eq!(subscription.replay.len(), 1);
        assert_eq!(subscription.replay[0].sequence, 2);
    }

    #[tokio::test]
    async fn fanout_returns_not_found_for_unknown_attempt() {
        let hub = FanoutHub::new();
        assert!(matches!(
            hub.subscribe_session_parts(&key(), 0).await,
            Err(FanoutSubscribeError::NotFound)
        ));
    }

    #[tokio::test]
    async fn fanout_batch_streams_multiple_attempts() {
        let hub = Arc::new(FanoutHub::new());
        let service = FanoutServiceImpl::new(hub.clone());
        let key_one = key_for_session("session-1");
        let key_two = key_for_session("session-2");
        hub.create_session_attempt(key_one.clone()).await;
        hub.create_session_attempt(key_two.clone()).await;

        let mut stream = service
            .stream_session_parts_batch(Request::new(StreamSessionPartsBatchRequest {
                streams: vec![
                    StreamSessionPartsRequest {
                        ns: "ns".to_string(),
                        agent: "agent".to_string(),
                        session_id: "session-1".to_string(),
                        submission_id: "submission-1".to_string(),
                        attempt_id: "attempt-1".to_string(),
                        after_sequence: 0,
                    },
                    StreamSessionPartsRequest {
                        ns: "ns".to_string(),
                        agent: "agent".to_string(),
                        session_id: "session-2".to_string(),
                        submission_id: "submission-1".to_string(),
                        attempt_id: "attempt-1".to_string(),
                        after_sequence: 0,
                    },
                ],
            }))
            .await
            .unwrap()
            .into_inner();

        hub.publish_session_part(
            &key_one,
            event_for_session("session-1", SessionMessagePartEventKind::Delta, "one"),
        )
        .await;
        hub.publish_session_part(
            &key_two,
            event_for_session("session-2", SessionMessagePartEventKind::Delta, "two"),
        )
        .await;

        let mut contents = Vec::new();
        for _ in 0..2 {
            let response = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            contents.push(response.event.unwrap().part.unwrap().content);
        }
        contents.sort();
        assert_eq!(contents, ["one", "two"]);
    }
}
