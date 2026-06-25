// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::scheduling;
use crate::control::topics;
use crate::control::ProtoKeyValueStoreExt;
use crate::control::{events, keys, ns, KeyValueStore, MessagePublisher};
use crate::gateway::rpc::{data_proto, resources_proto, worker_proto};
use crate::gateway::session_streams::SessionStreamTarget;
use futures::{stream::SelectAll, Stream, StreamExt};
use prost::Message;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tonic::transport::Channel;

const SESSION_STREAM_LEASE_CHECK_MAX_MICROS: u64 = 5_000_000;
const SESSION_STREAM_LEASE_CHECK_MIN_MICROS: u64 = 100_000;
const WORKER_FANOUT_NOT_FOUND_RETRY_DELAY: Duration = Duration::from_millis(50);

pub(crate) type SessionPartEventStream = Pin<
    Box<
        dyn Stream<Item = std::result::Result<events::SessionMessagePartEvent, tonic::Status>>
            + Send,
    >,
>;

fn stream_session_lease_check_interval() -> Duration {
    let ttl_micros = scheduling::session_processing_timeout_micros().max(1) as u64;
    Duration::from_micros((ttl_micros / 10).clamp(
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
) -> SessionPartEventStream {
    let mut streams: SelectAll<SessionPartEventStream> = SelectAll::new();
    for target in targets {
        streams.push(single_session_parts_event_stream(
            target,
            None,
            kv.clone(),
            pubsub.clone(),
        ));
    }
    Box::pin(streams)
}

pub(crate) fn session_submission_event_stream(
    target: SessionStreamTarget,
    submission_id: String,
    kv: Arc<dyn KeyValueStore + Send + Sync>,
    pubsub: Arc<dyn MessagePublisher + Send + Sync>,
) -> SessionPartEventStream {
    single_session_parts_event_stream(target, Some(submission_id), kv, pubsub)
}

fn single_session_parts_event_stream(
    target: SessionStreamTarget,
    submission_id: Option<String>,
    kv: Arc<dyn KeyValueStore + Send + Sync>,
    pubsub: Arc<dyn MessagePublisher + Send + Sync>,
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
                None => latest_nonterminal_submission(kv.as_ref(), &target).await?,
            }) else {
                continue;
            };

            if submission.attempt_id != active_attempt_id {
                active_attempt_id = submission.attempt_id.clone();
                after_sequence = 0;
            }

            let now_micros = chrono::Utc::now().timestamp_micros();
            let terminal = crate::harness::sessions::submission_is_terminal(&submission);
            if !terminal {
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
            }

            if submission.claim_worker_id.is_empty() {
                continue;
            }

            match connect_worker_stream(kv.as_ref(), pubsub.as_ref(), &target, &submission, after_sequence).await {
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
                    if terminal {
                        yield Err(status);
                        break;
                    }
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

async fn connect_worker_stream(
    kv: &dyn KeyValueStore,
    pubsub: &dyn MessagePublisher,
    target: &SessionStreamTarget,
    submission: &data_proto::SessionSubmission,
    after_sequence: u64,
) -> std::result::Result<tonic::Streaming<worker_proto::StreamSessionPartsResponse>, tonic::Status>
{
    let endpoints = worker_endpoints(kv, pubsub, &submission.claim_worker_id).await?;
    let mut last_status = None;
    for endpoint in endpoints {
        match stream_from_endpoint(&endpoint, target, submission, after_sequence).await {
            Ok(stream) => return Ok(stream),
            Err(status) => last_status = Some(status),
        }
    }
    Err(last_status.unwrap_or_else(|| tonic::Status::unavailable("worker has no endpoints")))
}

async fn stream_from_endpoint(
    endpoint: &resources_proto::WorkerEndpoint,
    target: &SessionStreamTarget,
    submission: &data_proto::SessionSubmission,
    after_sequence: u64,
) -> std::result::Result<tonic::Streaming<worker_proto::StreamSessionPartsResponse>, tonic::Status>
{
    let channel = Channel::from_shared(endpoint.url.clone())
        .map_err(|err| tonic::Status::unavailable(format!("invalid worker endpoint: {err}")))?
        .connect()
        .await
        .map_err(|err| tonic::Status::unavailable(format!("failed to connect to worker: {err}")))?;
    let mut client = worker_proto::fanout_service_client::FanoutServiceClient::new(channel);
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
