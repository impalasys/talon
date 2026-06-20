// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::scheduling;
use crate::control::topics;
use crate::control::ProtoKeyValueStoreExt;
use crate::control::{events, keys, KeyValueStore, MessagePublisher};
use crate::gateway::rpc::data_proto;
use crate::gateway::session_streams::{SessionStreamReceiver, SessionStreamTarget};
use futures::{Stream, StreamExt};
use prost::Message;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

const SESSION_STREAM_LEASE_CHECK_MAX_MICROS: u64 = 5_000_000;
const SESSION_STREAM_LEASE_CHECK_MIN_MICROS: u64 = 1_000_000;
const SESSION_STREAM_LEASE_CHECK_CONCURRENCY: usize = 16;

pub(super) type SessionPartEventStream = Pin<
    Box<
        dyn Stream<Item = std::result::Result<events::SessionMessagePartEvent, tonic::Status>>
            + Send,
    >,
>;

fn stream_session_lease_check_interval() -> Duration {
    let ttl_micros = scheduling::session_processing_timeout_micros().max(1) as u64;
    Duration::from_micros((ttl_micros / 6).clamp(
        SESSION_STREAM_LEASE_CHECK_MIN_MICROS,
        SESSION_STREAM_LEASE_CHECK_MAX_MICROS,
    ))
}

fn stream_session_redispatch_throttle_micros() -> i64 {
    scheduling::session_processing_timeout_micros().max(1)
}

pub(super) fn session_parts_event_stream(
    mut receiver: SessionStreamReceiver,
    targets: Vec<SessionStreamTarget>,
    kv: Arc<dyn KeyValueStore + Send + Sync>,
    pubsub: Arc<dyn MessagePublisher + Send + Sync>,
) -> SessionPartEventStream {
    Box::pin(async_stream::stream! {
        let mut lease_check = tokio::time::interval(stream_session_lease_check_interval());
        lease_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        lease_check.tick().await;
        let mut lease_check_task: Option<tokio::task::JoinHandle<()>> = None;

        loop {
            tokio::select! {
                event = receiver.recv() => {
                    let Some(event) = event else {
                        break;
                    };
                    yield event;
                }
                _ = lease_check.tick() => {
                    if lease_check_task.as_ref().is_some_and(|task| task.is_finished()) {
                        lease_check_task = None;
                    }
                    if lease_check_task.is_none() {
                        lease_check_task = Some(tokio::spawn(check_session_leases(
                            targets.clone(),
                            kv.clone(),
                            pubsub.clone(),
                        )));
                    }
                }
            }
        }
        if let Some(task) = lease_check_task {
            task.abort();
        }
    })
}

async fn check_session_leases(
    targets: Vec<SessionStreamTarget>,
    kv: Arc<dyn KeyValueStore + Send + Sync>,
    pubsub: Arc<dyn MessagePublisher + Send + Sync>,
) {
    let now_micros = chrono::Utc::now().timestamp_micros();
    futures::stream::iter(targets)
        .for_each_concurrent(SESSION_STREAM_LEASE_CHECK_CONCURRENCY, |target| {
            let kv = kv.clone();
            let pubsub = pubsub.clone();
            async move {
                if let Err(err) = redispatch_expired_session_lease(
                    kv.as_ref(),
                    pubsub.as_ref(),
                    &target,
                    now_micros,
                )
                .await
                {
                    tracing::warn!(
                        namespace = %target.ns,
                        agent = %target.agent,
                        session = %target.session_id,
                        error = %err,
                        "Failed to check session submission lease while streaming"
                    );
                }
            }
        })
        .await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ProtoKeyValueStoreExt;
    use crate::test_support::{MockKvStore, RecordingPubSub};

    #[tokio::test]
    async fn stream_lease_check_skips_active_submission_lease() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let target = SessionStreamTarget::new("conic", "infra", "session-1");
        seed_processing_session_with_submission(
            kv.as_ref(),
            &target,
            data_proto::SessionSubmission {
                submission_id: "submission-1".to_string(),
                session_id: "session-1".to_string(),
                user_message_id: "user-1".to_string(),
                status: data_proto::SessionSubmissionStatus::Claimed as i32,
                attempt_id: "attempt-1".to_string(),
                attempt_count: 1,
                claim_expires_at: Some(30_000_000),
                created_at: 1,
                updated_at: 1,
                completed_at: None,
                committed_message_id: None,
                current_phase: data_proto::SessionExecutionPhase::Unspecified as i32,
                current_journal_entry_id: None,
            },
        )
        .await;

        let redispatched =
            redispatch_expired_session_lease(kv.as_ref(), pubsub.as_ref(), &target, 20_000_000)
                .await
                .unwrap();

        assert!(!redispatched);
        assert!(pubsub.published.lock().await.is_empty());
    }

    #[tokio::test]
    async fn stream_lease_check_redispatches_expired_submission_lease() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let target = SessionStreamTarget::new("conic", "infra", "session-1");
        seed_processing_session_with_submission(
            kv.as_ref(),
            &target,
            data_proto::SessionSubmission {
                submission_id: "submission-1".to_string(),
                session_id: "session-1".to_string(),
                user_message_id: "user-1".to_string(),
                status: data_proto::SessionSubmissionStatus::Claimed as i32,
                attempt_id: "attempt-1".to_string(),
                attempt_count: 1,
                claim_expires_at: Some(10_000_000),
                created_at: 1,
                updated_at: 1,
                completed_at: None,
                committed_message_id: None,
                current_phase: data_proto::SessionExecutionPhase::Unspecified as i32,
                current_journal_entry_id: None,
            },
        )
        .await;

        let redispatched =
            redispatch_expired_session_lease(kv.as_ref(), pubsub.as_ref(), &target, 20_000_000)
                .await
                .unwrap();

        assert!(redispatched);
        let published = pubsub.published.lock().await;
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, topics::SESSION_DISPATCH_TOPIC);
        let event = events::SessionMessageEvent::decode(published[0].1.as_slice()).unwrap();
        assert_eq!(event.ns, "conic");
        assert_eq!(event.agent, "infra");
        assert_eq!(event.session_id, "session-1");
        assert_eq!(event.message_id, "user-1");
        assert_eq!(event.submission_id, "submission-1");
        assert_eq!(event.timestamp, 123);
        assert_eq!(event.message, "please continue");
        drop(published);

        let submission = kv
            .get_msg::<data_proto::SessionSubmission>(&keys::session_submission(
                "conic",
                "infra",
                "session-1",
                "submission-1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(submission.updated_at, 20_000_000);
    }

    async fn seed_processing_session_with_submission(
        kv: &MockKvStore,
        target: &SessionStreamTarget,
        submission: data_proto::SessionSubmission,
    ) {
        kv.set_msg(
            &keys::session(&target.ns, &target.agent, &target.session_id),
            &data_proto::Session {
                id: target.session_id.clone(),
                agent: target.agent.clone(),
                ns: target.ns.clone(),
                status: "PROCESSING".to_string(),
                created_at: 1,
                last_active: 123,
                metadata: std::collections::HashMap::new(),
                labels: std::collections::HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::session_message(&target.ns, &target.agent, &target.session_id, "user-1"),
            &data_proto::SessionMessage {
                id: "user-1".to_string(),
                role: data_proto::MessageRole::RoleUser as i32,
                created_at: 1,
                labels: std::collections::HashMap::new(),
                parts: vec![data_proto::SessionMessagePart {
                    id: "000000".to_string(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: "please continue".to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                    created_at: 1,
                    object: None,
                }],
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::session_submission(
                &target.ns,
                &target.agent,
                &target.session_id,
                &submission.submission_id,
            ),
            &submission,
        )
        .await
        .unwrap();
    }
}
