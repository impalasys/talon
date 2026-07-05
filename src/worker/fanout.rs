// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::events::{SessionMessagePartEvent, SessionMessagePartEventKind};
use crate::gateway::rpc::data_proto::WorkflowRunEvent;
use crate::gateway::rpc::worker_proto::{
    fanout_service_server::FanoutService, StreamSessionPartsBatchRequest,
    StreamSessionPartsRequest, StreamSessionPartsResponse, StreamWorkflowEventsRequest,
    StreamWorkflowEventsResponse,
};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{
    mpsc::{self, error::TrySendError},
    Mutex, Notify,
};
use tonic::{Request, Response, Status};

const MAX_SUBSCRIBER_QUEUE_CAPACITY: usize = 1_048_576;
const SUBSCRIBER_QUEUE_EVENTS_PER_STREAM: usize = 96;

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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct WorkflowFanoutKey {
    pub ns: String,
    pub workflow: String,
    pub run_id: String,
}

impl WorkflowFanoutKey {
    pub fn new(
        ns: impl Into<String>,
        workflow: impl Into<String>,
        run_id: impl Into<String>,
    ) -> Self {
        Self {
            ns: ns.into(),
            workflow: workflow.into(),
            run_id: run_id.into(),
        }
    }
}

#[derive(Debug)]
pub enum FanoutSubscribeError {
    NotFound,
}

struct FanoutSubscriber {
    keys: Vec<SessionFanoutKey>,
    sender: mpsc::Sender<StreamSessionPartsResponse>,
}

struct WorkflowFanoutSubscriber {
    keys: Vec<WorkflowFanoutKey>,
    sender: mpsc::Sender<StreamWorkflowEventsResponse>,
}

struct SessionAttemptFanout {
    subscribers: Vec<u64>,
    next_sequence: u64,
    subscriber_notify: Arc<Notify>,
}

impl SessionAttemptFanout {
    fn new() -> Self {
        Self {
            subscribers: Vec::new(),
            next_sequence: 1,
            subscriber_notify: Arc::new(Notify::new()),
        }
    }
}

struct WorkflowRunFanout {
    subscribers: Vec<u64>,
    next_sequence: u64,
    subscriber_notify: Arc<Notify>,
}

impl WorkflowRunFanout {
    fn new() -> Self {
        Self {
            subscribers: Vec::new(),
            next_sequence: 1,
            subscriber_notify: Arc::new(Notify::new()),
        }
    }
}

#[derive(Default)]
struct FanoutState {
    // Active session attempts keyed by the exact worker-local stream identity.
    // Each attempt owns ordering state and the subscriber ids currently attached
    // to that attempt.
    attempts: HashMap<SessionFanoutKey, SessionAttemptFanout>,
    // Subscriber id -> outbound session stream. This reverse index lets one
    // batch subscriber attach to multiple attempts and be removed from all of
    // them when the client disconnects.
    subscribers: HashMap<u64, FanoutSubscriber>,
    // Active workflow runs keyed by namespace/workflow/run id. Each run owns
    // ordering state and the subscriber ids currently attached to that run.
    workflow_runs: HashMap<WorkflowFanoutKey, WorkflowRunFanout>,
    // Subscriber id -> outbound workflow stream. Workflow streams currently
    // attach to one run, but this mirrors session fanout so a future batch
    // workflow stream can clean up one subscriber across multiple run keys.
    workflow_subscribers: HashMap<u64, WorkflowFanoutSubscriber>,
    // Shared monotonic id source for session and workflow subscribers.
    next_subscriber_id: u64,
}

#[derive(Clone, Default)]
pub struct FanoutHub {
    state: Arc<Mutex<FanoutState>>,
}

impl FanoutHub {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create_session_attempt(&self, key: SessionFanoutKey) {
        let mut state = self.state.lock().await;
        state
            .attempts
            .entry(key)
            .or_insert_with(SessionAttemptFanout::new);
    }

    pub async fn create_workflow_run(&self, key: WorkflowFanoutKey) {
        let mut state = self.state.lock().await;
        state
            .workflow_runs
            .entry(key)
            .or_insert_with(WorkflowRunFanout::new);
    }

    pub async fn wait_for_subscriber(
        &self,
        key: &SessionFanoutKey,
        timeout: std::time::Duration,
    ) -> bool {
        loop {
            let notify = {
                let state = self.state.lock().await;
                let Some(fanout) = state.attempts.get(key) else {
                    return false;
                };
                if !fanout.subscribers.is_empty() {
                    return true;
                }
                fanout.subscriber_notify.clone()
            };

            if tokio::time::timeout(timeout, notify.notified())
                .await
                .is_err()
            {
                return false;
            }
        }
    }

    pub async fn publish_session_part(
        &self,
        key: &SessionFanoutKey,
        event: SessionMessagePartEvent,
    ) {
        let terminal = session_part_event_is_terminal(&event);
        let (response, subscribers) = {
            let mut state = self.state.lock().await;
            let fanout = state
                .attempts
                .entry(key.clone())
                .or_insert_with(SessionAttemptFanout::new);
            let response = StreamSessionPartsResponse {
                sequence: fanout.next_sequence,
                event: Some(event),
            };
            fanout.next_sequence = fanout.next_sequence.saturating_add(1);

            let subscriber_ids = fanout.subscribers.clone();
            let subscribers = subscriber_ids
                .into_iter()
                .filter_map(|subscriber_id| {
                    state
                        .subscribers
                        .get(&subscriber_id)
                        .map(|subscriber| (subscriber_id, subscriber.sender.clone()))
                })
                .collect::<Vec<_>>();
            (response, subscribers)
        };

        let mut stale_subscribers = Vec::new();
        for (subscriber_id, sender) in subscribers {
            match sender.try_send(response.clone()) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => stale_subscribers.push(subscriber_id),
                Err(TrySendError::Closed(_)) => stale_subscribers.push(subscriber_id),
            }
        }

        let mut state = self.state.lock().await;
        for subscriber_id in stale_subscribers {
            remove_subscriber(&mut state, subscriber_id);
        }
        if terminal {
            for subscriber_id in state
                .attempts
                .get(key)
                .map(|fanout| fanout.subscribers.clone())
                .unwrap_or_default()
            {
                remove_subscriber_from_key(&mut state, subscriber_id, key);
            }
            state.attempts.remove(key);
        }
    }

    pub async fn publish_workflow_event(&self, key: &WorkflowFanoutKey, event: WorkflowRunEvent) {
        let terminal = workflow_event_is_terminal(&event);
        let (response, subscribers) = {
            let mut state = self.state.lock().await;
            let fanout = state
                .workflow_runs
                .entry(key.clone())
                .or_insert_with(WorkflowRunFanout::new);
            let response = StreamWorkflowEventsResponse {
                sequence: fanout.next_sequence,
                event: Some(event),
            };
            fanout.next_sequence = fanout.next_sequence.saturating_add(1);

            let subscriber_ids = fanout.subscribers.clone();
            let subscribers = subscriber_ids
                .into_iter()
                .filter_map(|subscriber_id| {
                    state
                        .workflow_subscribers
                        .get(&subscriber_id)
                        .map(|subscriber| (subscriber_id, subscriber.sender.clone()))
                })
                .collect::<Vec<_>>();
            (response, subscribers)
        };

        let mut stale_subscribers = Vec::new();
        for (subscriber_id, sender) in subscribers {
            match sender.try_send(response.clone()) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => stale_subscribers.push(subscriber_id),
                Err(TrySendError::Closed(_)) => stale_subscribers.push(subscriber_id),
            }
        }

        let mut state = self.state.lock().await;
        for subscriber_id in stale_subscribers {
            remove_workflow_subscriber(&mut state, subscriber_id);
        }
        if terminal {
            for subscriber_id in state
                .workflow_runs
                .get(key)
                .map(|fanout| fanout.subscribers.clone())
                .unwrap_or_default()
            {
                remove_workflow_subscriber_from_key(&mut state, subscriber_id, key);
            }
            state.workflow_runs.remove(key);
        }
    }

    pub async fn subscribe_session_parts(
        &self,
        key: &SessionFanoutKey,
        after_sequence: u64,
    ) -> std::result::Result<FanoutSubscription, FanoutSubscribeError> {
        self.subscribe_session_parts_batch(vec![(key.clone(), after_sequence)])
            .await
    }

    pub async fn subscribe_session_parts_batch(
        &self,
        streams: Vec<(SessionFanoutKey, u64)>,
    ) -> std::result::Result<FanoutSubscription, FanoutSubscribeError> {
        let mut state = self.state.lock().await;
        for (key, _) in &streams {
            if !state.attempts.contains_key(key) {
                return Err(FanoutSubscribeError::NotFound);
            }
        }

        let subscriber_id = state.next_subscriber_id;
        state.next_subscriber_id = state.next_subscriber_id.saturating_add(1);
        let keys = streams
            .into_iter()
            .map(|(key, _after_sequence)| key)
            .collect::<Vec<_>>();
        let (sender, receiver) = mpsc::channel(subscriber_queue_capacity(keys.len()));
        let mut subscriber_notifies = Vec::new();
        for key in &keys {
            if let Some(fanout) = state.attempts.get_mut(key) {
                fanout.subscribers.push(subscriber_id);
                subscriber_notifies.push(fanout.subscriber_notify.clone());
            }
        }
        state
            .subscribers
            .insert(subscriber_id, FanoutSubscriber { keys, sender });
        for notify in subscriber_notifies {
            notify.notify_one();
        }

        Ok(FanoutSubscription {
            receiver,
            cleanup: FanoutSubscriptionCleanup {
                state: Some(self.state.clone()),
                subscriber_id,
            },
        })
    }

    pub async fn subscribe_workflow_events(
        &self,
        key: &WorkflowFanoutKey,
        _after_sequence: u64,
    ) -> std::result::Result<WorkflowFanoutSubscription, FanoutSubscribeError> {
        let mut state = self.state.lock().await;
        if !state.workflow_runs.contains_key(key) {
            return Err(FanoutSubscribeError::NotFound);
        }

        let subscriber_id = state.next_subscriber_id;
        state.next_subscriber_id = state.next_subscriber_id.saturating_add(1);
        let (sender, receiver) = mpsc::channel(subscriber_queue_capacity(1));
        let notify = state.workflow_runs.get_mut(key).map(|fanout| {
            fanout.subscribers.push(subscriber_id);
            fanout.subscriber_notify.clone()
        });
        state.workflow_subscribers.insert(
            subscriber_id,
            WorkflowFanoutSubscriber {
                keys: vec![key.clone()],
                sender,
            },
        );
        if let Some(notify) = notify {
            notify.notify_one();
        }

        Ok(WorkflowFanoutSubscription {
            receiver,
            cleanup: WorkflowFanoutSubscriptionCleanup {
                state: Some(self.state.clone()),
                subscriber_id,
            },
        })
    }

    #[cfg(test)]
    pub async fn attempt_count(&self) -> usize {
        self.state.lock().await.attempts.len()
    }

    #[cfg(test)]
    pub async fn subscriber_count(&self) -> usize {
        self.state.lock().await.subscribers.len()
    }

    #[cfg(test)]
    pub async fn workflow_run_count(&self) -> usize {
        self.state.lock().await.workflow_runs.len()
    }
}

fn subscriber_queue_capacity(stream_count: usize) -> usize {
    stream_count
        .saturating_mul(SUBSCRIBER_QUEUE_EVENTS_PER_STREAM)
        .max(2)
        .min(MAX_SUBSCRIBER_QUEUE_CAPACITY)
}

fn remove_subscriber(state: &mut FanoutState, subscriber_id: u64) {
    let Some(subscriber) = state.subscribers.remove(&subscriber_id) else {
        return;
    };
    for key in subscriber.keys {
        if let Some(fanout) = state.attempts.get_mut(&key) {
            fanout
                .subscribers
                .retain(|existing_id| *existing_id != subscriber_id);
        }
    }
}

fn remove_subscriber_from_key(state: &mut FanoutState, subscriber_id: u64, key: &SessionFanoutKey) {
    if let Some(fanout) = state.attempts.get_mut(key) {
        fanout
            .subscribers
            .retain(|existing_id| *existing_id != subscriber_id);
    }

    let Some(subscriber) = state.subscribers.get_mut(&subscriber_id) else {
        return;
    };
    subscriber.keys.retain(|existing_key| existing_key != key);
    if subscriber.keys.is_empty() {
        state.subscribers.remove(&subscriber_id);
    }
}

fn remove_workflow_subscriber(state: &mut FanoutState, subscriber_id: u64) {
    let Some(subscriber) = state.workflow_subscribers.remove(&subscriber_id) else {
        return;
    };
    for key in subscriber.keys {
        if let Some(fanout) = state.workflow_runs.get_mut(&key) {
            fanout
                .subscribers
                .retain(|existing_id| *existing_id != subscriber_id);
        }
    }
}

fn remove_workflow_subscriber_from_key(
    state: &mut FanoutState,
    subscriber_id: u64,
    key: &WorkflowFanoutKey,
) {
    if let Some(fanout) = state.workflow_runs.get_mut(key) {
        fanout
            .subscribers
            .retain(|existing_id| *existing_id != subscriber_id);
    }

    let Some(subscriber) = state.workflow_subscribers.get_mut(&subscriber_id) else {
        return;
    };
    subscriber.keys.retain(|existing_key| existing_key != key);
    if subscriber.keys.is_empty() {
        state.workflow_subscribers.remove(&subscriber_id);
    }
}

pub struct FanoutSubscription {
    receiver: mpsc::Receiver<StreamSessionPartsResponse>,
    cleanup: FanoutSubscriptionCleanup,
}

pub struct WorkflowFanoutSubscription {
    receiver: mpsc::Receiver<StreamWorkflowEventsResponse>,
    cleanup: WorkflowFanoutSubscriptionCleanup,
}

impl WorkflowFanoutSubscription {
    pub fn into_stream(
        self,
    ) -> Pin<
        Box<
            dyn futures::Stream<Item = std::result::Result<StreamWorkflowEventsResponse, Status>>
                + Send,
        >,
    > {
        let mut receiver = self.receiver;
        let cleanup = self.cleanup;
        Box::pin(async_stream::stream! {
            let _cleanup = cleanup;
            while let Some(event) = receiver.recv().await {
                yield Ok(event);
            }
        })
    }
}

impl FanoutSubscription {
    pub fn into_stream(
        self,
    ) -> Pin<
        Box<
            dyn futures::Stream<Item = std::result::Result<StreamSessionPartsResponse, Status>>
                + Send,
        >,
    > {
        let mut receiver = self.receiver;
        let cleanup = self.cleanup;
        Box::pin(async_stream::stream! {
            let _cleanup = cleanup;
            while let Some(event) = receiver.recv().await {
                yield Ok(event);
            }
        })
    }
}

struct FanoutSubscriptionCleanup {
    state: Option<Arc<Mutex<FanoutState>>>,
    subscriber_id: u64,
}

impl Drop for FanoutSubscriptionCleanup {
    fn drop(&mut self) {
        let Some(state) = self.state.take() else {
            return;
        };
        let subscriber_id = self.subscriber_id;
        tokio::spawn(async move {
            let mut state = state.lock().await;
            remove_subscriber(&mut state, subscriber_id);
        });
    }
}

struct WorkflowFanoutSubscriptionCleanup {
    state: Option<Arc<Mutex<FanoutState>>>,
    subscriber_id: u64,
}

impl Drop for WorkflowFanoutSubscriptionCleanup {
    fn drop(&mut self) {
        let Some(state) = self.state.take() else {
            return;
        };
        let subscriber_id = self.subscriber_id;
        tokio::spawn(async move {
            let mut state = state.lock().await;
            remove_workflow_subscriber(&mut state, subscriber_id);
        });
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
    type StreamWorkflowEventsStream = Pin<
        Box<
            dyn futures::Stream<Item = std::result::Result<StreamWorkflowEventsResponse, Status>>
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

        let streams = request
            .streams
            .into_iter()
            .map(|request| {
                (
                    SessionFanoutKey::new(
                        request.ns,
                        request.agent,
                        request.session_id,
                        request.submission_id,
                        request.attempt_id,
                    ),
                    request.after_sequence,
                )
            })
            .collect();
        let subscription = self
            .hub
            .subscribe_session_parts_batch(streams)
            .await
            .map_err(|err| match err {
                FanoutSubscribeError::NotFound => Status::not_found("session attempt not found"),
            })?;
        Ok(Response::new(subscription.into_stream()))
    }

    async fn stream_workflow_events(
        &self,
        request: Request<StreamWorkflowEventsRequest>,
    ) -> std::result::Result<Response<Self::StreamWorkflowEventsStream>, Status> {
        let request = request.into_inner();
        let key = WorkflowFanoutKey::new(request.ns, request.workflow, request.run_id);
        let subscription = self
            .hub
            .subscribe_workflow_events(&key, request.after_sequence)
            .await
            .map_err(|err| match err {
                FanoutSubscribeError::NotFound => Status::not_found("workflow run not found"),
            })?;
        Ok(Response::new(subscription.into_stream()))
    }
}

fn session_part_event_is_terminal(event: &SessionMessagePartEvent) -> bool {
    event.kind == SessionMessagePartEventKind::Done as i32
        || event.kind == SessionMessagePartEventKind::Error as i32
}

fn workflow_event_is_terminal(event: &WorkflowRunEvent) -> bool {
    matches!(
        event.r#type.as_str(),
        "run_completed" | "run_failed" | "run_cancelled"
    )
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

    fn workflow_key() -> WorkflowFanoutKey {
        WorkflowFanoutKey::new("ns", "workflow", "run-1")
    }

    fn workflow_event(id: &str, event_type: &str) -> WorkflowRunEvent {
        WorkflowRunEvent {
            id: id.to_string(),
            r#type: event_type.to_string(),
            timestamp: 1,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn fanout_streams_live_events_without_replay() {
        let hub = FanoutHub::new();
        let key = key();
        hub.create_session_attempt(key.clone()).await;
        hub.publish_session_part(&key, event(SessionMessagePartEventKind::Delta, "missed"))
            .await;

        let mut subscription = hub
            .subscribe_session_parts(&key, 0)
            .await
            .unwrap()
            .into_stream();
        hub.publish_session_part(&key, event(SessionMessagePartEventKind::Delta, "live"))
            .await;

        let response = tokio::time::timeout(std::time::Duration::from_secs(5), subscription.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response.sequence, 2);
        assert_eq!(response.event.unwrap().part.unwrap().content, "live");
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
    async fn workflow_fanout_streams_live_events_without_replay() {
        let hub = FanoutHub::new();
        let key = workflow_key();
        hub.create_workflow_run(key.clone()).await;
        hub.publish_workflow_event(&key, workflow_event("event-1", "run_started"))
            .await;

        let mut subscription = hub
            .subscribe_workflow_events(&key, 0)
            .await
            .unwrap()
            .into_stream();
        hub.publish_workflow_event(&key, workflow_event("event-2", "step_started"))
            .await;

        let response = tokio::time::timeout(std::time::Duration::from_secs(5), subscription.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response.sequence, 2);
        assert_eq!(response.event.unwrap().id, "event-2");
    }

    #[tokio::test]
    async fn workflow_fanout_returns_not_found_for_unknown_run() {
        let hub = FanoutHub::new();
        assert!(matches!(
            hub.subscribe_workflow_events(&workflow_key(), 0).await,
            Err(FanoutSubscribeError::NotFound)
        ));
    }

    #[tokio::test]
    async fn terminal_workflow_event_removes_run_and_subscriber() {
        let hub = FanoutHub::new();
        let key = workflow_key();
        hub.create_workflow_run(key.clone()).await;
        let mut stream = hub
            .subscribe_workflow_events(&key, 0)
            .await
            .unwrap()
            .into_stream();

        hub.publish_workflow_event(&key, workflow_event("event-1", "run_completed"))
            .await;
        let response = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response.event.unwrap().id, "event-1");
        assert!(stream.next().await.is_none());
        assert_eq!(hub.workflow_run_count().await, 0);
        assert_eq!(hub.subscriber_count().await, 0);
    }

    #[tokio::test]
    async fn wait_for_subscriber_returns_when_stream_attaches() {
        let hub = Arc::new(FanoutHub::new());
        let key = key();
        hub.create_session_attempt(key.clone()).await;

        let waiter = {
            let hub = hub.clone();
            let key = key.clone();
            tokio::spawn(async move {
                hub.wait_for_subscriber(&key, std::time::Duration::from_secs(5))
                    .await
            })
        };

        assert!(
            !hub.wait_for_subscriber(&key, std::time::Duration::from_millis(1))
                .await
        );
        let _stream = hub
            .subscribe_session_parts(&key, 0)
            .await
            .unwrap()
            .into_stream();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
                .await
                .unwrap()
                .unwrap()
        );
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

    #[tokio::test]
    async fn batch_subscription_survives_one_attempt_terminal_event() {
        let hub = FanoutHub::new();
        let key_one = key_for_session("session-1");
        let key_two = key_for_session("session-2");
        hub.create_session_attempt(key_one.clone()).await;
        hub.create_session_attempt(key_two.clone()).await;

        let mut stream = hub
            .subscribe_session_parts_batch(vec![(key_one.clone(), 0), (key_two.clone(), 0)])
            .await
            .unwrap()
            .into_stream();

        hub.publish_session_part(
            &key_one,
            event_for_session("session-1", SessionMessagePartEventKind::Done, "one"),
        )
        .await;
        hub.publish_session_part(
            &key_two,
            event_for_session("session-2", SessionMessagePartEventKind::Delta, "two-delta"),
        )
        .await;
        hub.publish_session_part(
            &key_two,
            event_for_session("session-2", SessionMessagePartEventKind::Done, "two-done"),
        )
        .await;

        let mut contents = Vec::new();
        for _ in 0..3 {
            let response = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            contents.push(response.event.unwrap().part.unwrap().content);
        }
        assert_eq!(contents, ["one", "two-delta", "two-done"]);
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn terminal_event_removes_attempt_and_subscriber() {
        let hub = FanoutHub::new();
        let key = key();
        hub.create_session_attempt(key.clone()).await;
        let mut stream = hub
            .subscribe_session_parts(&key, 0)
            .await
            .unwrap()
            .into_stream();

        hub.publish_session_part(&key, event(SessionMessagePartEventKind::Done, "done"))
            .await;
        let response = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response.event.unwrap().part.unwrap().content, "done");
        assert!(stream.next().await.is_none());
        assert_eq!(hub.attempt_count().await, 0);
        assert_eq!(hub.subscriber_count().await, 0);
    }
}
