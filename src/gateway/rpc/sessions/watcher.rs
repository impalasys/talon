// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::scheduling;
use crate::control::topics;
use crate::control::ProtoKeyValueStoreExt;
use crate::control::{events, keys, ns, KeyValueStore, MessagePublisher};
use crate::gateway::rpc::{data_proto, resources_proto, worker_proto};
use crate::gateway::session_streams::SessionStreamTarget;
use crate::gateway::worker_conn::WorkerConnectionPool;
use futures::{stream::SelectAll, Stream, StreamExt};
use prost::Message;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const SESSION_STREAM_LEASE_CHECK_MAX_MICROS: u64 = 5_000_000;
const SESSION_STREAM_LEASE_CHECK_MIN_MICROS: u64 = 100_000;
const WORKER_FANOUT_NOT_FOUND_RETRY_DELAY: Duration = Duration::from_millis(50);
const SESSION_PART_PREFETCH_BUFFER: usize = 64;

pub(crate) type SessionPartEventStream = Pin<
    Box<
        dyn Stream<Item = std::result::Result<events::SessionMessagePartEvent, tonic::Status>>
            + Send,
    >,
>;

fn stream_session_lease_check_interval() -> Duration {
    let ttl_micros = scheduling::session_processing_timeout_micros().max(1) as u64;
    Duration::from_micros((ttl_micros / 100).clamp(
        SESSION_STREAM_LEASE_CHECK_MIN_MICROS,
        SESSION_STREAM_LEASE_CHECK_MAX_MICROS,
    ))
}

fn stream_session_redispatch_throttle_micros() -> i64 {
    scheduling::session_processing_timeout_micros().max(1)
}

pub(crate) fn session_parts_event_stream(
    targets: Vec<SessionStreamTarget>,
    kv: Arc<dyn KeyValueStore + Send + Sync>,
    pubsub: Arc<dyn MessagePublisher + Send + Sync>,
    worker_connections: Arc<WorkerConnectionPool>,
) -> SessionPartEventStream {
    if targets.is_empty() {
        return Box::pin(futures::stream::empty());
    }

    let stream = if targets.len() > 1 {
        batch_session_parts_event_stream(targets, kv, pubsub, worker_connections)
    } else {
        single_session_parts_event_stream(
            targets.into_iter().next().expect("target"),
            None,
            kv,
            pubsub,
            worker_connections,
        )
    };
    eager_buffer_session_part_stream(stream)
}

pub(crate) fn session_submission_event_stream(
    target: SessionStreamTarget,
    submission_id: String,
    kv: Arc<dyn KeyValueStore + Send + Sync>,
    pubsub: Arc<dyn MessagePublisher + Send + Sync>,
    worker_connections: Arc<WorkerConnectionPool>,
) -> SessionPartEventStream {
    eager_buffer_session_part_stream(single_session_parts_event_stream(
        target,
        Some(submission_id),
        kv,
        pubsub,
        worker_connections,
    ))
}

// Start each returned gateway stream's upstream watcher immediately and isolate it
// behind a private bounded queue. That lets the gateway attach to worker fanout
// before the client polls the response stream, while keeping concurrent watchers
// independent: no shared receiver or fanout cursor can let one stream consume,
// block, or advance another stream's events.
fn eager_buffer_session_part_stream(mut source: SessionPartEventStream) -> SessionPartEventStream {
    let (sender, receiver) = mpsc::channel(SESSION_PART_PREFETCH_BUFFER);
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    tokio::spawn(async move {
        loop {
            let item = tokio::select! {
                _ = task_cancel.cancelled() => break,
                item = source.next() => item,
            };
            let Some(item) = item else {
                break;
            };
            tokio::select! {
                _ = task_cancel.cancelled() => break,
                result = sender.send(item) => {
                    if result.is_err() {
                        break;
                    }
                }
            }
        }
    });
    let cancel_on_drop = cancel.drop_guard();
    Box::pin(futures::stream::unfold(
        (receiver, cancel_on_drop),
        |(mut receiver, cancel_on_drop)| async move {
            receiver
                .recv()
                .await
                .map(|item| (item, (receiver, cancel_on_drop)))
        },
    ))
}

fn single_session_parts_event_stream(
    target: SessionStreamTarget,
    submission_id: Option<String>,
    kv: Arc<dyn KeyValueStore + Send + Sync>,
    pubsub: Arc<dyn MessagePublisher + Send + Sync>,
    worker_connections: Arc<WorkerConnectionPool>,
) -> SessionPartEventStream {
    Box::pin(async_stream::stream! {
        let mut after_sequence = 0u64;
        let mut active_attempt_id = String::new();
        let mut tick = tokio::time::interval(stream_session_lease_check_interval());
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        'outer: loop {
            tick.tick().await;
            let Some(submission) = (match submission_id.as_deref() {
                Some(submission_id) => load_session_submission(kv.as_ref(), &target, submission_id).await?,
                None => latest_submission(kv.as_ref(), &target).await?,
            }) else {
                continue;
            };

            if submission.attempt_id != active_attempt_id {
                active_attempt_id = submission.attempt_id.clone();
                after_sequence = 0;
            }

            let now_micros = chrono::Utc::now().timestamp_micros();
            let terminal = crate::harness::sessions::submission_is_terminal(&submission);
            if terminal {
                yield Ok(terminal_event_from_submission(&target, &submission));
                break 'outer;
            }

            if submission.status != data_proto::SessionSubmissionStatus::Claimed as i32
                || submission
                    .claim_expires_at
                    .is_some_and(|expires_at| expires_at <= now_micros)
            {
                redispatch_expired_session_lease(
                    kv.as_ref(),
                    pubsub.as_ref(),
                    &target,
                    now_micros,
                )
                .await?;
                continue;
            }

            if submission.claim_worker_id.is_empty() {
                continue;
            }

            match connect_worker_stream(
                kv.as_ref(),
                pubsub.as_ref(),
                worker_connections.as_ref(),
                &target,
                &submission,
                after_sequence,
            ).await {
                Ok(mut stream) => {
                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(response) => {
                                after_sequence = response.sequence;
                                let Some(event) = response.event else {
                                    continue;
                                };
                                let terminal = event.kind
                                    == events::SessionMessagePartEventKind::Done as i32
                                    || event.kind
                                        == events::SessionMessagePartEventKind::Error as i32;
                                yield Ok(event);
                                if terminal {
                                    break 'outer;
                                }
                            }
                            Err(status) => {
                                if status.code() == tonic::Code::NotFound
                                    || status.code() == tonic::Code::Unavailable
                                {
                                    break;
                                }
                                yield Err(status);
                                break;
                            }
                        }
                    }
                }
                Err(status)
                    if status.code() == tonic::Code::NotFound
                        || status.code() == tonic::Code::Unavailable =>
                {
                    tokio::time::sleep(WORKER_FANOUT_NOT_FOUND_RETRY_DELAY).await;
                }
                Err(status) => {
                    yield Err(status);
                    break;
                }
            }
        }
    })
}

#[derive(Clone)]
struct BatchSessionState {
    target: SessionStreamTarget,
    after_sequence: u64,
    active_attempt_id: String,
    done: bool,
}

#[derive(Clone)]
struct ClaimedBatchStream {
    target: SessionStreamTarget,
    submission: data_proto::SessionSubmission,
    after_sequence: u64,
}

type WorkerFanoutStream = Pin<
    Box<
        dyn Stream<
                Item = std::result::Result<worker_proto::StreamSessionPartsResponse, tonic::Status>,
            > + Send,
    >,
>;

fn batch_session_parts_event_stream(
    targets: Vec<SessionStreamTarget>,
    kv: Arc<dyn KeyValueStore + Send + Sync>,
    pubsub: Arc<dyn MessagePublisher + Send + Sync>,
    worker_connections: Arc<WorkerConnectionPool>,
) -> SessionPartEventStream {
    Box::pin(async_stream::stream! {
        let mut states: Vec<BatchSessionState> = targets
            .into_iter()
            .map(|target| BatchSessionState {
                target,
                after_sequence: 0,
                active_attempt_id: String::new(),
                done: false,
            })
            .collect();
        let mut tick = tokio::time::interval(stream_session_lease_check_interval());
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        'outer: loop {
            if states.iter().all(|state| state.done) {
                break;
            }

            tick.tick().await;
            let now_micros = chrono::Utc::now().timestamp_micros();
            let mut by_worker: HashMap<String, Vec<ClaimedBatchStream>> = HashMap::new();
            let target_indexes: HashMap<(String, String, String), usize> = states
                .iter()
                .enumerate()
                .map(|(index, state)| {
                    (
                        (
                            state.target.ns.clone(),
                            state.target.agent.clone(),
                            state.target.session_id.clone(),
                        ),
                        index,
                    )
                })
                .collect();

            for state in &mut states {
                if state.done {
                    continue;
                }

                let Some(submission) = latest_submission(kv.as_ref(), &state.target).await? else {
                    continue;
                };

                if submission.attempt_id != state.active_attempt_id {
                    state.active_attempt_id = submission.attempt_id.clone();
                    state.after_sequence = 0;
                }

                let terminal = crate::harness::sessions::submission_is_terminal(&submission);
                if terminal {
                    state.done = true;
                    yield Ok(terminal_event_from_submission(&state.target, &submission));
                    continue;
                }

                if submission.status != data_proto::SessionSubmissionStatus::Claimed as i32
                    || submission
                        .claim_expires_at
                        .is_some_and(|expires_at| expires_at <= now_micros)
                {
                    redispatch_expired_session_lease(
                        kv.as_ref(),
                        pubsub.as_ref(),
                        &state.target,
                        now_micros,
                    )
                    .await?;
                    continue;
                }

                if submission.claim_worker_id.is_empty() {
                    continue;
                }

                by_worker
                    .entry(submission.claim_worker_id.clone())
                    .or_default()
                    .push(ClaimedBatchStream {
                        target: state.target.clone(),
                        submission,
                        after_sequence: state.after_sequence,
                    });
            }

            if by_worker.is_empty() {
                continue;
            }

            let mut worker_streams: SelectAll<WorkerFanoutStream> = SelectAll::new();
            for (worker_id, streams) in by_worker {
                let terminal_batch = streams
                    .iter()
                    .all(|stream| crate::harness::sessions::submission_is_terminal(&stream.submission));
                match connect_worker_batch_stream(
                    kv.as_ref(),
                    pubsub.as_ref(),
                    worker_connections.as_ref(),
                    &worker_id,
                    &streams,
                )
                .await
                {
                    Ok(stream) => worker_streams.push(Box::pin(stream)),
                    Err(status)
                        if status.code() == tonic::Code::NotFound
                            || status.code() == tonic::Code::Unavailable =>
                    {
                        if terminal_batch {
                            yield Err(status);
                            break 'outer;
                        }
                        continue;
                    }
                    Err(status) => {
                        yield Err(status);
                        break 'outer;
                    }
                }
            }

            loop {
                let item = tokio::select! {
                    item = worker_streams.next() => item,
                    _ = tick.tick() => break,
                };
                let Some(item) = item else {
                    break;
                };
                match item {
                    Ok(response) => {
                        let Some(event) = response.event else {
                            continue;
                        };
                        let target_key = (
                            event.ns.clone(),
                            event.agent.clone(),
                            event.session_id.clone(),
                        );
                        if let Some(index) = target_indexes.get(&target_key).copied() {
                            states[index].after_sequence = response.sequence;
                            let terminal = event.kind
                                == events::SessionMessagePartEventKind::Done as i32
                                || event.kind
                                    == events::SessionMessagePartEventKind::Error as i32;
                            if terminal {
                                states[index].done = true;
                            }
                        }
                        yield Ok(event);
                        if states.iter().all(|state| state.done) {
                            break 'outer;
                        }
                    }
                    Err(status) => {
                        if status.code() == tonic::Code::NotFound
                            || status.code() == tonic::Code::Unavailable
                        {
                            break;
                        }
                        yield Err(status);
                        break 'outer;
                    }
                }
            }
        }
    })
}

async fn connect_worker_stream(
    kv: &dyn KeyValueStore,
    pubsub: &dyn MessagePublisher,
    worker_connections: &WorkerConnectionPool,
    target: &SessionStreamTarget,
    submission: &data_proto::SessionSubmission,
    after_sequence: u64,
) -> std::result::Result<tonic::Streaming<worker_proto::StreamSessionPartsResponse>, tonic::Status>
{
    let endpoints = worker_endpoints(kv, pubsub, &submission.claim_worker_id).await?;
    let mut last_status = None;
    for endpoint in endpoints {
        match stream_from_endpoint(
            worker_connections,
            &endpoint,
            target,
            submission,
            after_sequence,
        )
        .await
        {
            Ok(stream) => return Ok(stream),
            Err(status) => last_status = Some(status),
        }
    }
    Err(last_status.unwrap_or_else(|| tonic::Status::unavailable("worker has no endpoints")))
}

async fn connect_worker_batch_stream(
    kv: &dyn KeyValueStore,
    pubsub: &dyn MessagePublisher,
    worker_connections: &WorkerConnectionPool,
    worker_id: &str,
    streams: &[ClaimedBatchStream],
) -> std::result::Result<tonic::Streaming<worker_proto::StreamSessionPartsResponse>, tonic::Status>
{
    let endpoints = worker_endpoints(kv, pubsub, worker_id).await?;
    let mut last_status = None;
    for endpoint in endpoints {
        match stream_batch_from_endpoint(worker_connections, &endpoint, streams).await {
            Ok(stream) => return Ok(stream),
            Err(status) => last_status = Some(status),
        }
    }
    Err(last_status.unwrap_or_else(|| tonic::Status::unavailable("worker has no endpoints")))
}

async fn stream_from_endpoint(
    worker_connections: &WorkerConnectionPool,
    endpoint: &resources_proto::WorkerEndpoint,
    target: &SessionStreamTarget,
    submission: &data_proto::SessionSubmission,
    after_sequence: u64,
) -> std::result::Result<tonic::Streaming<worker_proto::StreamSessionPartsResponse>, tonic::Status>
{
    let mut client = worker_connections.fanout_client(endpoint).await?;
    let response = client
        .stream_session_parts(worker_proto::StreamSessionPartsRequest {
            ns: target.ns.clone(),
            agent: target.agent.clone(),
            session_id: target.session_id.clone(),
            submission_id: submission.submission_id.clone(),
            attempt_id: submission.attempt_id.clone(),
            after_sequence,
        })
        .await?;
    Ok(response.into_inner())
}

async fn stream_batch_from_endpoint(
    worker_connections: &WorkerConnectionPool,
    endpoint: &resources_proto::WorkerEndpoint,
    streams: &[ClaimedBatchStream],
) -> std::result::Result<tonic::Streaming<worker_proto::StreamSessionPartsResponse>, tonic::Status>
{
    let mut client = worker_connections.fanout_client(endpoint).await?;
    let response = client
        .stream_session_parts_batch(worker_proto::StreamSessionPartsBatchRequest {
            streams: streams
                .iter()
                .map(|stream| worker_proto::StreamSessionPartsRequest {
                    ns: stream.target.ns.clone(),
                    agent: stream.target.agent.clone(),
                    session_id: stream.target.session_id.clone(),
                    submission_id: stream.submission.submission_id.clone(),
                    attempt_id: stream.submission.attempt_id.clone(),
                    after_sequence: stream.after_sequence,
                })
                .collect(),
        })
        .await?;
    Ok(response.into_inner())
}

async fn worker_endpoints(
    kv: &dyn KeyValueStore,
    _pubsub: &dyn MessagePublisher,
    worker_id: &str,
) -> std::result::Result<Vec<resources_proto::WorkerEndpoint>, tonic::Status> {
    let key = keys::ResourceKey::new(ns::TALON_SYSTEM, &[], "Worker", worker_id);
    let Some(bytes) = kv
        .get(&key)
        .await
        .map_err(|err| tonic::Status::internal(format!("Failed to fetch worker: {err}")))?
    else {
        return Err(tonic::Status::not_found("claim worker not found"));
    };
    let worker = resources_proto::Worker::decode(bytes.as_slice())
        .map_err(|err| tonic::Status::internal(format!("Failed to decode worker: {err}")))?;
    let Some(status) = worker.status else {
        return Err(tonic::Status::unavailable("worker status is missing"));
    };
    if status.phase != "ready" {
        return Err(tonic::Status::unavailable("worker is not ready"));
    }
    let endpoints: Vec<_> = status
        .endpoints
        .into_iter()
        .filter(|endpoint| !endpoint.url.trim().is_empty())
        .collect();
    if endpoints.is_empty() {
        return Err(tonic::Status::unavailable("worker has no endpoints"));
    }
    Ok(endpoints)
}

async fn redispatch_expired_session_lease(
    kv: &dyn KeyValueStore,
    pubsub: &dyn MessagePublisher,
    target: &SessionStreamTarget,
    now_micros: i64,
) -> std::result::Result<bool, tonic::Status> {
    let session_key = keys::session(&target.ns, &target.agent, &target.session_id);
    let Some(session) = kv
        .get_msg::<data_proto::Session>(&session_key)
        .await
        .map_err(|err| tonic::Status::internal(format!("Failed to fetch session: {err}")))?
    else {
        return Ok(false);
    };
    if session.status != "PROCESSING" {
        return Ok(false);
    }

    let Some(submission) = latest_nonterminal_submission(kv, target).await? else {
        return Ok(false);
    };
    if submission.status == data_proto::SessionSubmissionStatus::Claimed as i32
        && submission
            .claim_expires_at
            .is_some_and(|expires_at| expires_at > now_micros)
    {
        return Ok(false);
    }
    if now_micros.saturating_sub(submission.updated_at)
        < stream_session_redispatch_throttle_micros()
    {
        return Ok(false);
    }

    let submission_key = keys::session_submission(
        &target.ns,
        &target.agent,
        &target.session_id,
        &submission.submission_id,
    );
    let Some(current_bytes) = kv
        .get(&submission_key)
        .await
        .map_err(|err| tonic::Status::internal(format!("Failed to fetch submission: {err}")))?
    else {
        return Ok(false);
    };
    let mut current = data_proto::SessionSubmission::decode(current_bytes.as_slice())
        .map_err(|err| tonic::Status::internal(format!("Failed to decode submission: {err}")))?;
    if crate::harness::sessions::submission_is_terminal(&current)
        || current.status == data_proto::SessionSubmissionStatus::Claimed as i32
            && current
                .claim_expires_at
                .is_some_and(|expires_at| expires_at > now_micros)
        || now_micros.saturating_sub(current.updated_at)
            < stream_session_redispatch_throttle_micros()
    {
        return Ok(false);
    }

    current.updated_at = now_micros;
    let updated = current.encode_to_vec();
    if !kv
        .compare_and_swap(&submission_key, Some(current_bytes.as_slice()), &updated)
        .await
        .map_err(|err| tonic::Status::internal(format!("Failed to throttle redispatch: {err}")))?
    {
        return Ok(false);
    }

    let message_key = keys::session_message(
        &target.ns,
        &target.agent,
        &target.session_id,
        &current.user_message_id,
    );
    let Some(user_message) = kv
        .get_msg::<data_proto::SessionMessage>(&message_key)
        .await
        .map_err(|err| tonic::Status::internal(format!("Failed to fetch user message: {err}")))?
    else {
        tracing::warn!(
            namespace = %target.ns,
            agent = %target.agent,
            session = %target.session_id,
            submission = %current.submission_id,
            message = %current.user_message_id,
            "Cannot redispatch expired session lease because the user message is missing"
        );
        return Ok(false);
    };

    let event = events::SessionMessageEvent {
        session_id: target.session_id.clone(),
        message_id: current.user_message_id.clone(),
        direction: events::MessageDirection::Inbound as i32,
        timestamp: session.last_active,
        agent: target.agent.clone(),
        message: scheduling::session_message_text_projection(&user_message),
        ns: target.ns.clone(),
        submission_id: current.submission_id.clone(),
    };
    pubsub
        .publish(topics::SESSION_DISPATCH_TOPIC, &event.encode_to_vec())
        .await
        .map_err(|err| tonic::Status::internal(format!("Failed to publish redispatch: {err}")))?;
    tracing::info!(
        namespace = %target.ns,
        agent = %target.agent,
        session = %target.session_id,
        submission = %current.submission_id,
        "Redispatched session submission after expired lease"
    );
    Ok(true)
}

async fn load_session_submission(
    kv: &dyn KeyValueStore,
    target: &SessionStreamTarget,
    submission_id: &str,
) -> std::result::Result<Option<data_proto::SessionSubmission>, tonic::Status> {
    let key =
        keys::session_submission(&target.ns, &target.agent, &target.session_id, submission_id);
    let Some(bytes) = kv
        .get(&key)
        .await
        .map_err(|err| tonic::Status::internal(format!("Failed to fetch submission: {err}")))?
    else {
        return Ok(None);
    };
    data_proto::SessionSubmission::decode(bytes.as_slice())
        .map(Some)
        .map_err(|err| tonic::Status::internal(format!("Failed to decode submission: {err}")))
}

async fn latest_nonterminal_submission(
    kv: &dyn KeyValueStore,
    target: &SessionStreamTarget,
) -> std::result::Result<Option<data_proto::SessionSubmission>, tonic::Status> {
    let prefix = keys::session_submission_prefix(&target.ns, &target.agent, &target.session_id);
    let entries = kv
        .list_entries(&prefix)
        .await
        .map_err(|err| tonic::Status::internal(format!("Failed to list submissions: {err}")))?;
    let mut submissions = Vec::new();
    for (_key, bytes) in entries {
        let submission =
            data_proto::SessionSubmission::decode(bytes.as_slice()).map_err(|err| {
                tonic::Status::internal(format!("Failed to decode submission: {err}"))
            })?;
        if !crate::harness::sessions::submission_is_terminal(&submission) {
            submissions.push(submission);
        }
    }
    Ok(submissions
        .into_iter()
        .max_by_key(|submission| (submission.created_at, submission.updated_at)))
}

async fn latest_submission(
    kv: &dyn KeyValueStore,
    target: &SessionStreamTarget,
) -> std::result::Result<Option<data_proto::SessionSubmission>, tonic::Status> {
    let prefix = keys::session_submission_prefix(&target.ns, &target.agent, &target.session_id);
    let entries = kv
        .list_entries(&prefix)
        .await
        .map_err(|err| tonic::Status::internal(format!("Failed to list submissions: {err}")))?;
    let mut submissions = Vec::new();
    for (_key, bytes) in entries {
        let submission =
            data_proto::SessionSubmission::decode(bytes.as_slice()).map_err(|err| {
                tonic::Status::internal(format!("Failed to decode submission: {err}"))
            })?;
        submissions.push(submission);
    }
    Ok(submissions
        .into_iter()
        .max_by_key(|submission| (submission.created_at, submission.updated_at)))
}

fn terminal_event_from_submission(
    target: &SessionStreamTarget,
    submission: &data_proto::SessionSubmission,
) -> events::SessionMessagePartEvent {
    let failed = submission.status == data_proto::SessionSubmissionStatus::Failed as i32
        || submission.status == data_proto::SessionSubmissionStatus::Interrupted as i32;
    events::SessionMessagePartEvent {
        session_id: target.session_id.clone(),
        kind: if failed {
            events::SessionMessagePartEventKind::Error as i32
        } else {
            events::SessionMessagePartEventKind::Done as i32
        },
        part: Some(data_proto::SessionMessagePart {
            part_type: if failed {
                data_proto::SessionMessagePartType::Error as i32
            } else {
                data_proto::SessionMessagePartType::Text as i32
            },
            ..Default::default()
        }),
        timestamp: submission.updated_at,
        agent: target.agent.clone(),
        ns: target.ns.clone(),
        message_id: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::events::SessionMessagePartEventKind;
    use crate::worker::fanout::{FanoutHub, SessionFanoutKey};
    use futures::StreamExt;
    use tempfile::tempdir;
    use tokio::net::UnixListener;
    use tokio::sync::oneshot;
    use tokio_stream::wrappers::UnixListenerStream;
    use tokio_util::sync::CancellationToken;
    use tonic::transport::Server;

    fn part_event(
        kind: SessionMessagePartEventKind,
        content: &str,
    ) -> events::SessionMessagePartEvent {
        events::SessionMessagePartEvent {
            session_id: "session-1".to_string(),
            kind: kind as i32,
            part: Some(data_proto::SessionMessagePart {
                content: content.to_string(),
                ..Default::default()
            }),
            timestamp: 1,
            agent: "agent".to_string(),
            ns: "ns".to_string(),
            message_id: "message-1".to_string(),
        }
    }

    #[tokio::test]
    async fn eager_buffer_polls_and_buffers_before_consumer_poll() {
        let (started_tx, started_rx) = oneshot::channel();
        let (event_tx, event_rx) = oneshot::channel();
        let source: SessionPartEventStream = Box::pin(async_stream::stream! {
            let _ = started_tx.send(());
            if let Ok(event) = event_rx.await {
                yield Ok(event);
            }
        });

        let mut stream = eager_buffer_session_part_stream(source);
        tokio::time::timeout(Duration::from_secs(1), started_rx)
            .await
            .unwrap()
            .unwrap();

        assert!(event_tx
            .send(part_event(SessionMessagePartEventKind::Delta, "prefetched"))
            .is_ok());
        tokio::time::sleep(Duration::from_millis(10)).await;

        let event = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(event.part.unwrap().content, "prefetched");
    }

    #[tokio::test]
    async fn stream_from_endpoint_connects_to_unix_fanout_service() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("fanout.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let shutdown = CancellationToken::new();
        let hub = Arc::new(FanoutHub::new());
        let service = worker_proto::fanout_service_server::FanoutServiceServer::new(
            crate::worker::fanout::FanoutServiceImpl::new(hub.clone()),
        );
        let server = tokio::spawn(
            Server::builder()
                .add_service(service)
                .serve_with_incoming_shutdown(
                    UnixListenerStream::new(listener),
                    shutdown.clone().cancelled_owned(),
                ),
        );

        let key = SessionFanoutKey::new("ns", "agent", "session-1", "submission-1", "attempt-1");
        hub.create_session_attempt(key.clone()).await;

        let endpoint = resources_proto::WorkerEndpoint {
            url: format!("unix://{}", socket_path.display()),
            protocol: "grpc".to_string(),
            audience: String::new(),
        };
        let target = SessionStreamTarget::new("ns", "agent", "session-1");
        let submission = data_proto::SessionSubmission {
            submission_id: "submission-1".to_string(),
            attempt_id: "attempt-1".to_string(),
            ..Default::default()
        };

        let pool = WorkerConnectionPool::new();
        let mut stream = tokio::time::timeout(
            Duration::from_secs(5),
            stream_from_endpoint(&pool, &endpoint, &target, &submission, 0),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(pool.cached_channel_count().await, 1);
        hub.publish_session_part(
            &key,
            part_event(SessionMessagePartEventKind::Delta, "hello"),
        )
        .await;
        let response = tokio::time::timeout(Duration::from_secs(5), stream.message())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response.sequence, 1);
        assert_eq!(response.event.unwrap().part.unwrap().content, "hello");

        let mut second_stream = tokio::time::timeout(
            Duration::from_secs(5),
            stream_from_endpoint(&pool, &endpoint, &target, &submission, 1),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(pool.cached_channel_count().await, 1);
        hub.publish_session_part(
            &key,
            part_event(SessionMessagePartEventKind::Delta, "again"),
        )
        .await;
        let response = tokio::time::timeout(Duration::from_secs(5), second_stream.message())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response.sequence, 2);
        assert_eq!(response.event.unwrap().part.unwrap().content, "again");

        drop(stream);
        drop(second_stream);
        shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
