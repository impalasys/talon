// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use prost::Message;
use std::time::Duration;

use crate::control::{
    events, keys, scheduling, topics, KeyValueStore, ListOptions, MessagePublisher,
    ProtoKeyValueStoreExt,
};
use crate::gateway::rpc::data_proto;

pub const A2A_QUEUE: &str = "a2a";
pub const NEXT_QUEUE: &str = "next";
pub const STEER_QUEUE: &str = "steer";
const MAX_QUEUE_DISPATCH_CAS_RETRIES: usize = 8;

fn validate_queue_name(queue: &str) -> Result<()> {
    match queue {
        A2A_QUEUE | NEXT_QUEUE | STEER_QUEUE => Ok(()),
        _ => Err(anyhow!("session queue name is invalid")),
    }
}

async fn sleep_after_cas_retry(attempt: usize) {
    let base_ms = 1_u64 << attempt.min(4);
    let jitter_ms = Utc::now().timestamp_subsec_micros() as u64 % 3;
    tokio::time::sleep(Duration::from_millis(base_ms + jitter_ms)).await;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedSessionMessage {
    pub queue: String,
    pub entry_id: String,
    pub message_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchedQueuedSessionMessage {
    pub queue: String,
    pub entry_id: String,
    pub message_id: String,
    pub submission_id: String,
}

pub async fn queue_text_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    queue: &str,
    message: &str,
    labels: std::collections::HashMap<String, String>,
    now: DateTime<Utc>,
) -> Result<QueuedSessionMessage> {
    if message.trim().is_empty() {
        return Err(scheduling::EmptyMessageError.into());
    }
    let now_micros = now.timestamp_micros();
    let user_msg = data_proto::SessionMessage {
        id: String::new(),
        role: data_proto::MessageRole::RoleUser as i32,
        created_at: now_micros,
        labels,
        parts: vec![data_proto::SessionMessagePart {
            id: "000000".to_string(),
            part_type: data_proto::SessionMessagePartType::Text as i32,
            content: message.to_string(),
            name: String::new(),
            payload_json: String::new(),
            created_at: now_micros,
            object: None,
        }],
    };
    queue_session_message(kv, ns, agent, session_id, queue, user_msg, now).await
}

pub async fn queue_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    queue: &str,
    mut message: data_proto::SessionMessage,
    now: DateTime<Utc>,
) -> Result<QueuedSessionMessage> {
    validate_queue_name(queue)?;
    if message.parts.is_empty() {
        return Err(scheduling::EmptyMessageError.into());
    }
    let now_micros = now.timestamp_micros();
    let queue_entry_suffix = if message.id.is_empty() {
        crate::control::uuid::session_message_id()
    } else {
        std::mem::take(&mut message.id)
    };
    if message.role == data_proto::MessageRole::RoleUnspecified as i32 {
        message.role = data_proto::MessageRole::RoleUser as i32;
    }
    if message.created_at == 0 {
        message.created_at = now_micros;
    }
    for (index, part) in message.parts.iter_mut().enumerate() {
        if part.id.is_empty() {
            part.id = format!("{index:06}");
        }
        if part.created_at == 0 {
            part.created_at = message.created_at;
        }
    }

    let entry_id = format!("{:020}-{queue_entry_suffix}", message.created_at);
    kv.set_msg(
        &keys::session_queue_entry(ns, agent, session_id, queue, &entry_id),
        &message,
    )
    .await?;

    Ok(QueuedSessionMessage {
        queue: queue.to_string(),
        entry_id,
        message_id: String::new(),
    })
}

pub async fn dispatch_next_queued_message(
    kv: &dyn KeyValueStore,
    pubsub: &dyn MessagePublisher,
    ns: &str,
    agent: &str,
    session_id: &str,
    queue: &str,
    now: DateTime<Utc>,
) -> Result<Option<DispatchedQueuedSessionMessage>> {
    validate_queue_name(queue)?;
    let prefix = keys::session_queue_prefix(ns, agent, session_id, queue);
    let Some(entry_key) = kv
        .list_keys(&prefix, Some(ListOptions::default().limit(1)))
        .await?
        .into_iter()
        .next()
    else {
        return Ok(None);
    };
    let Some(entry_id) = keys::direct_child_name(&prefix, &entry_key) else {
        return Ok(None);
    };
    let Some(message_bytes) = kv.get(&entry_key).await? else {
        return Ok(None);
    };
    let message = data_proto::SessionMessage::decode(message_bytes.as_slice())?;
    let session_key = keys::session(ns, agent, session_id);
    let now_micros = now.timestamp_micros();
    let timeout_micros = scheduling::session_processing_timeout_micros();

    let mut acquired = false;
    for attempt in 0..MAX_QUEUE_DISPATCH_CAS_RETRIES {
        let current = kv.get(&session_key).await?;
        let Some(current_bytes) = current.as_ref() else {
            return Err(scheduling::SessionNotFoundError.into());
        };
        let mut session = data_proto::Session::decode(current_bytes.as_slice())?;
        if session.status == "PROCESSING"
            && now_micros.saturating_sub(session.last_active) <= timeout_micros
        {
            return Ok(None);
        }
        if session.status != "IDLE" && session.status != "PROCESSING" {
            return Ok(None);
        }
        session.status = "PROCESSING".to_string();
        session.last_active = now_micros;
        if kv
            .compare_and_swap(
                &session_key,
                Some(current_bytes.as_slice()),
                &session.encode_to_vec(),
            )
            .await?
        {
            acquired = true;
            break;
        }
        if attempt + 1 < MAX_QUEUE_DISPATCH_CAS_RETRIES {
            sleep_after_cas_retry(attempt).await;
        }
    }
    if !acquired {
        return Err(anyhow!("failed to atomically acquire session lock"));
    }

    match publish_queued_message(
        kv, pubsub, ns, agent, session_id, queue, &entry_id, message, now,
    )
    .await
    {
        Ok(dispatched) => Ok(Some(dispatched)),
        Err(err) => {
            if let Err(release_err) =
                release_session_lock_after_queue_dispatch_failure(kv, &session_key, now_micros)
                    .await
            {
                tracing::warn!(
                    namespace = %ns,
                    key = %session_key,
                    error = %release_err,
                    "failed to release session lock after queued dispatch error"
                );
            }
            Err(err)
        }
    }
}

async fn publish_queued_message(
    kv: &dyn KeyValueStore,
    pubsub: &dyn MessagePublisher,
    ns: &str,
    agent: &str,
    session_id: &str,
    queue: &str,
    entry_id: &str,
    mut message: data_proto::SessionMessage,
    now: DateTime<Utc>,
) -> Result<DispatchedQueuedSessionMessage> {
    let now_micros = now.timestamp_micros();
    message.id = crate::control::uuid::session_message_id();
    message.created_at = now_micros;
    for part in &mut message.parts {
        part.created_at = now_micros;
    }

    let message_key = keys::session_message(ns, agent, session_id, &message.id);
    kv.set_msg(&message_key, &message).await?;
    if let Err(error) = crate::control::search::publish_index_event(
        pubsub,
        events::IndexEvent {
            operation: events::IndexOperation::Upsert as i32,
            key: message_key.canonical(),
            ..Default::default()
        },
    )
    .await
    {
        tracing::warn!(
            error = %error,
            namespace = %ns,
            agent = %agent,
            session_id = %session_id,
            message_id = %message.id,
            "failed to publish search index event for queued session message"
        );
    }

    let submission_id = crate::control::uuid::session_submission_id();
    let submission = crate::harness::sessions::pending_submission(
        submission_id.clone(),
        session_id.to_string(),
        message.id.clone(),
        now_micros,
    );
    crate::harness::sessions::create_submission_if_absent(kv, ns, agent, session_id, &submission)
        .await?;

    let event = events::SessionMessageEvent {
        session_id: session_id.to_string(),
        message_id: message.id.clone(),
        direction: events::MessageDirection::Inbound as i32,
        timestamp: now_micros,
        agent: agent.to_string(),
        message: scheduling::session_message_text_projection(&message),
        ns: ns.to_string(),
        submission_id: submission_id.clone(),
    };
    kv.delete(&keys::session_queue_entry(
        ns, agent, session_id, queue, entry_id,
    ))
    .await?;
    pubsub
        .publish(topics::SESSION_DISPATCH_TOPIC, &event.encode_to_vec())
        .await?;

    Ok(DispatchedQueuedSessionMessage {
        queue: queue.to_string(),
        entry_id: entry_id.to_string(),
        message_id: message.id,
        submission_id,
    })
}

async fn release_session_lock_after_queue_dispatch_failure(
    kv: &dyn KeyValueStore,
    key: &keys::ResourceKey,
    expected_last_active: i64,
) -> Result<()> {
    for attempt in 0..MAX_QUEUE_DISPATCH_CAS_RETRIES {
        let Some(current_bytes) = kv.get(key).await? else {
            return Ok(());
        };
        let mut session = data_proto::Session::decode(current_bytes.as_slice())?;
        if session.status != "PROCESSING" || session.last_active != expected_last_active {
            return Ok(());
        }
        session.status = "IDLE".to_string();
        if kv
            .compare_and_swap(
                key,
                Some(current_bytes.as_slice()),
                &session.encode_to_vec(),
            )
            .await?
        {
            return Ok(());
        }
        if attempt + 1 < MAX_QUEUE_DISPATCH_CAS_RETRIES {
            sleep_after_cas_retry(attempt).await;
        }
    }
    Err(anyhow!(
        "failed to release session lock after queued dispatch error"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::topics;
    use crate::gateway::rpc::data_proto::SessionSubmission;
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use std::sync::Arc;

    async fn put_session(kv: &MockKvStore, status: &str) {
        kv.set_msg(
            &keys::session("Tenant:acme:Ops", "agent", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "agent".to_string(),
                ns: "Tenant:acme:Ops".to_string(),
                status: status.to_string(),
                created_at: 1,
                last_active: 1,
                metadata: Default::default(),
                labels: Default::default(),
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn queue_names_are_restricted_to_known_channels() {
        let kv = MockKvStore::new();
        put_session(&kv, "PROCESSING").await;

        for queue in [A2A_QUEUE, NEXT_QUEUE, STEER_QUEUE] {
            let queued = queue_text_message(
                &kv,
                "Tenant:acme:Ops",
                "agent",
                "session-1",
                queue,
                "hello",
                Default::default(),
                Utc::now(),
            )
            .await
            .unwrap();
            assert_eq!(queued.queue, queue);
        }

        let err = queue_text_message(
            &kv,
            "Tenant:acme:Ops",
            "agent",
            "session-1",
            "arbitrary",
            "hello",
            Default::default(),
            Utc::now(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("session queue name is invalid"));
    }

    #[tokio::test]
    async fn queue_message_does_not_create_submission() {
        let kv = MockKvStore::new();
        put_session(&kv, "PROCESSING").await;

        let queued = queue_text_message(
            &kv,
            "Tenant:acme:Ops",
            "agent",
            "session-1",
            NEXT_QUEUE,
            "hello",
            Default::default(),
            Utc::now(),
        )
        .await
        .unwrap();

        let queue_keys = kv
            .list_keys(
                &keys::session_queue_prefix("Tenant:acme:Ops", "agent", "session-1", NEXT_QUEUE),
                None,
            )
            .await
            .unwrap();
        assert_eq!(queue_keys.len(), 1);
        let submission_keys = kv
            .list_keys(
                &keys::session_submission_prefix("Tenant:acme:Ops", "agent", "session-1"),
                None,
            )
            .await
            .unwrap();
        assert!(submission_keys.is_empty());
        assert!(queued.message_id.is_empty());
        let stored = kv
            .get_msg::<data_proto::SessionMessage>(&keys::session_queue_entry(
                "Tenant:acme:Ops",
                "agent",
                "session-1",
                NEXT_QUEUE,
                &queued.entry_id,
            ))
            .await
            .unwrap()
            .expect("queued message should be stored");
        assert!(stored.id.is_empty());
    }

    #[tokio::test]
    async fn dispatch_creates_canonical_message_and_submission() {
        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        put_session(&kv, "IDLE").await;
        let queued_at = DateTime::<Utc>::from_timestamp_micros(1_000_000).unwrap();
        let dispatched_at = DateTime::<Utc>::from_timestamp_micros(2_000_000).unwrap();
        let queued = queue_text_message(
            kv.as_ref(),
            "Tenant:acme:Ops",
            "agent",
            "session-1",
            NEXT_QUEUE,
            "hello",
            Default::default(),
            queued_at,
        )
        .await
        .unwrap();

        let dispatched = dispatch_next_queued_message(
            kv.as_ref(),
            pubsub.as_ref(),
            "Tenant:acme:Ops",
            "agent",
            "session-1",
            NEXT_QUEUE,
            dispatched_at,
        )
        .await
        .unwrap()
        .expect("queued message should dispatch");

        assert!(queued.message_id.is_empty());
        assert!(!dispatched.message_id.is_empty());
        let stored_message = kv
            .get_msg::<data_proto::SessionMessage>(&keys::session_message(
                "Tenant:acme:Ops",
                "agent",
                "session-1",
                &dispatched.message_id,
            ))
            .await
            .unwrap()
            .expect("dispatched message should be stored");
        assert_eq!(stored_message.id, dispatched.message_id);
        assert_eq!(stored_message.created_at, dispatched_at.timestamp_micros());
        assert!(stored_message
            .parts
            .iter()
            .all(|part| part.created_at == dispatched_at.timestamp_micros()));
        assert!(kv
            .get_msg::<SessionSubmission>(&keys::session_submission(
                "Tenant:acme:Ops",
                "agent",
                "session-1",
                &dispatched.submission_id,
            ))
            .await
            .unwrap()
            .is_some());
        assert!(kv
            .list_keys(
                &keys::session_queue_prefix("Tenant:acme:Ops", "agent", "session-1", NEXT_QUEUE),
                None,
            )
            .await
            .unwrap()
            .is_empty());
        assert!(pubsub
            .published
            .lock()
            .await
            .iter()
            .any(|(topic, _)| topic == topics::SESSION_DISPATCH_TOPIC));
    }

    #[tokio::test]
    async fn dispatch_does_not_process_error_session() {
        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        put_session(&kv, "ERROR").await;
        queue_text_message(
            kv.as_ref(),
            "Tenant:acme:Ops",
            "agent",
            "session-1",
            NEXT_QUEUE,
            "hello",
            Default::default(),
            Utc::now(),
        )
        .await
        .unwrap();

        let dispatched = dispatch_next_queued_message(
            kv.as_ref(),
            pubsub.as_ref(),
            "Tenant:acme:Ops",
            "agent",
            "session-1",
            NEXT_QUEUE,
            Utc::now(),
        )
        .await
        .unwrap();

        assert!(dispatched.is_none());
        assert_eq!(
            kv.list_keys(
                &keys::session_queue_prefix("Tenant:acme:Ops", "agent", "session-1", NEXT_QUEUE),
                None,
            )
            .await
            .unwrap()
            .len(),
            1
        );
    }
}
