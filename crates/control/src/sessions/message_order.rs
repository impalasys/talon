// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! A bounded, server-owned confirmation fence. Admission starts before the
//! payload write and completes only after durable acceptance. Queue promotion
//! keeps the original receipt. Edits of the latest canonical input use the same
//! fence, so confirmation consumption can linearize against both inputs/edits.
use crate::control::{keys, KeyValueStore};
use crate::talon::protocol::data_proto;
use anyhow::{anyhow, Result};
use prost::Message;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Receipt {
    order: u64,
    nonce: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct AdmissionState {
    order: u64,
    revision: String,
    latest: Option<Receipt>,
    pending_write: Option<String>,
    text_digest: String,
    canonical_id: Option<String>,
}
struct AdmissionWrite {
    receipt: Receipt,
    nonce: String,
}

#[derive(Debug)]
pub struct UserConfirmationSnapshot {
    key: keys::ResourceKey,
    bytes: Vec<u8>,
}

fn state_key(ns: &str, agent: &str, session: &str) -> keys::ResourceKey {
    keys::ResourceKey::new(
        ns,
        &[("Agent", agent), ("Session", session)],
        "MessageAdmissionState",
        "state",
    )
}
fn queue_receipt_key(ns: &str, agent: &str, session: &str, entry: &str) -> keys::ResourceKey {
    keys::ResourceKey::new(
        ns,
        &[("Agent", agent), ("Session", session)],
        "QueuedMessageAdmission",
        entry,
    )
}
fn nonce() -> String {
    crate::control::uuid::session_message_id()
}
fn is_user(message: &data_proto::SessionMessage) -> bool {
    message.role == data_proto::MessageRole::RoleUser as i32
}
fn text_digest(message: &data_proto::SessionMessage) -> String {
    let text = message
        .parts
        .iter()
        .filter(|p| p.part_type == data_proto::SessionMessagePartType::Text as i32)
        .map(|p| p.content.as_str())
        .collect::<String>();
    digest(&text)
}
fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.trim().as_bytes()))
}
async fn load(
    kv: &dyn KeyValueStore,
    key: &keys::ResourceKey,
) -> Result<(Option<Vec<u8>>, AdmissionState)> {
    let raw = kv.get(key).await?;
    let state = raw
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()?
        .unwrap_or_default();
    Ok((raw, state))
}
async fn save(
    kv: &dyn KeyValueStore,
    key: &keys::ResourceKey,
    expected: Option<&[u8]>,
    state: &mut AdmissionState,
) -> Result<bool> {
    state.revision = nonce();
    kv.compare_and_swap(key, expected, &serde_json::to_vec(state)?)
        .await
}

pub async fn current_message_admission_order(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
) -> Result<u64> {
    Ok(load(kv, &state_key(ns, agent, session)).await?.1.order)
}

async fn begin_input(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    message: &data_proto::SessionMessage,
    canonical: bool,
) -> Result<Option<AdmissionWrite>> {
    if !is_user(message) {
        return Ok(None);
    }
    let key = state_key(ns, agent, session);
    for _ in 0..32 {
        let (raw, mut state) = load(kv, &key).await?;
        state.order = state
            .order
            .checked_add(1)
            .ok_or_else(|| anyhow!("session admission order exhausted"))?;
        let receipt = Receipt {
            order: state.order,
            nonce: nonce(),
        };
        let write_nonce = nonce();
        state.latest = Some(receipt.clone());
        state.pending_write = Some(write_nonce.clone());
        state.text_digest.clear();
        state.canonical_id = canonical.then(|| message.id.clone());
        if save(kv, &key, raw.as_deref(), &mut state).await? {
            return Ok(Some(AdmissionWrite {
                receipt,
                nonce: write_nonce,
            }));
        }
    }
    Err(anyhow!("failed to reserve session message admission"))
}

async fn begin_edit(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    message: &data_proto::SessionMessage,
) -> Result<Option<AdmissionWrite>> {
    if !is_user(message) {
        return Ok(None);
    }
    let key = state_key(ns, agent, session);
    for _ in 0..32 {
        let (raw, mut state) = load(kv, &key).await?;
        if state.canonical_id.as_deref() != Some(message.id.as_str()) {
            return Ok(None);
        }
        let Some(receipt) = state.latest.clone() else {
            return Ok(None);
        };
        if state.pending_write.is_some() {
            return Err(anyhow!(
                "latest user message update is pending; retry or send a fresh reply"
            ));
        }
        let write_nonce = nonce();
        state.pending_write = Some(write_nonce.clone());
        if save(kv, &key, raw.as_deref(), &mut state).await? {
            return Ok(Some(AdmissionWrite {
                receipt,
                nonce: write_nonce,
            }));
        }
    }
    Err(anyhow!("failed to reserve user message edit"))
}

async fn finish(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    write: Option<&AdmissionWrite>,
    message: &data_proto::SessionMessage,
) -> Result<()> {
    let Some(write) = write else {
        return Ok(());
    };
    let key = state_key(ns, agent, session);
    for _ in 0..32 {
        let (raw, mut state) = load(kv, &key).await?;
        if state.latest.as_ref() != Some(&write.receipt) {
            return Ok(());
        }
        if state.pending_write.as_deref() != Some(write.nonce.as_str()) {
            return Err(anyhow!("user message admission changed before completion"));
        }
        state.pending_write = None;
        state.text_digest = text_digest(message);
        if save(kv, &key, raw.as_deref(), &mut state).await? {
            return Ok(());
        }
    }
    Err(anyhow!("failed to complete user message admission"))
}

/// Appends cannot give an existing/edited message a fresh admission order.
pub async fn append_admitted_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    message: &data_proto::SessionMessage,
) -> Result<bool> {
    let key = keys::session_message(ns, agent, session, &message.id);
    if kv.get(&key).await?.is_some() {
        return Ok(false);
    }
    let write = begin_input(kv, ns, agent, session, message, true).await?;
    if !kv
        .compare_and_swap(&key, None, &message.encode_to_vec())
        .await?
    {
        return Ok(false);
    }
    finish(kv, ns, agent, session, write.as_ref(), message).await?;
    Ok(true)
}

pub async fn edit_admitted_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    expected: &[u8],
    message: &data_proto::SessionMessage,
) -> Result<bool> {
    let write = begin_edit(kv, ns, agent, session, message).await?;
    if !kv
        .compare_and_swap(
            &keys::session_message(ns, agent, session, &message.id),
            Some(expected),
            &message.encode_to_vec(),
        )
        .await?
    {
        return Ok(false);
    }
    finish(kv, ns, agent, session, write.as_ref(), message).await?;
    Ok(true)
}

pub async fn persist_admitted_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    message: &data_proto::SessionMessage,
) -> Result<()> {
    let key = keys::session_message(ns, agent, session, &message.id);
    let changed = if let Some(current) = kv.get(&key).await? {
        edit_admitted_session_message(kv, ns, agent, session, &current, message).await?
    } else {
        append_admitted_session_message(kv, ns, agent, session, message).await?
    };
    if !changed {
        return Err(anyhow!("session message changed during persistence"));
    }
    Ok(())
}

/// The queue acknowledgement, rather than later promotion, is input acceptance.
pub async fn persist_queued_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    queue: &str,
    entry: &str,
    message: &data_proto::SessionMessage,
) -> Result<()> {
    let write = begin_input(kv, ns, agent, session, message, false).await?;
    if let Some(write) = write.as_ref() {
        kv.set(
            &queue_receipt_key(ns, agent, session, entry),
            &serde_json::to_vec(&write.receipt)?,
        )
        .await?;
    }
    kv.set(
        &keys::session_queue_entry(ns, agent, session, queue, entry),
        &message.encode_to_vec(),
    )
    .await?;
    finish(kv, ns, agent, session, write.as_ref(), message).await
}

pub async fn persist_promoted_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    entry: &str,
    message: &mut data_proto::SessionMessage,
) -> Result<()> {
    let receipt = kv
        .get(&queue_receipt_key(ns, agent, session, entry))
        .await?
        .map(|raw| serde_json::from_slice::<Receipt>(&raw))
        .transpose()?;
    if let Some(receipt) = receipt {
        let key = state_key(ns, agent, session);
        for attempt in 0..32 {
            let (raw, mut state) = load(kv, &key).await?;
            if state.latest.as_ref() != Some(&receipt) {
                break;
            }
            if let Some(id) = state.canonical_id {
                message.id = id;
                break;
            }
            state.canonical_id = Some(message.id.clone());
            if save(kv, &key, raw.as_deref(), &mut state).await? {
                break;
            }
            if attempt == 31 {
                return Err(anyhow!("failed to bind promoted user message"));
            }
        }
    }
    // Retries must not overwrite a user edit of an already materialized input.
    kv.compare_and_swap(
        &keys::session_message(ns, agent, session, &message.id),
        None,
        &message.encode_to_vec(),
    )
    .await?;
    Ok(())
}

/// One state read; no historical message/index scans or client timestamps.
pub async fn validate_user_confirmation(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    after: u64,
    required_reply: &str,
) -> Result<UserConfirmationSnapshot> {
    let key = state_key(ns, agent, session);
    let (raw, state) = load(kv, &key).await?;
    if state.order <= after
        || state.pending_write.is_some()
        || state.text_digest != digest(required_reply)
    {
        return Err(anyhow!("delete requires a separate user message exactly: {}; latest input may be pending or different", required_reply));
    }
    Ok(UserConfirmationSnapshot {
        key,
        bytes: raw.ok_or_else(|| anyhow!("user confirmation state missing"))?,
    })
}

/// Token single-use and session-state CAS jointly fence consumption. If a newer
/// input/edit wins, consume fails closed and the caller must request a new token.
pub async fn consume_user_confirmation(
    kv: &dyn KeyValueStore,
    snapshot: UserConfirmationSnapshot,
    confirmation_key: &keys::ResourceKey,
    pending: &[u8],
    expires_at: i64,
) -> Result<()> {
    let check_expiry = || -> Result<()> {
        if chrono::Utc::now().timestamp_micros() > expires_at {
            return Err(anyhow!(
                "delete confirmation expired; start a new delete request"
            ));
        }
        Ok(())
    };
    check_expiry()?;
    if !kv
        .compare_and_swap(confirmation_key, Some(pending), b"consumed")
        .await?
    {
        return Err(anyhow!("delete confirmation is missing or already used"));
    }
    check_expiry()?;
    let mut state: AdmissionState = serde_json::from_slice(&snapshot.bytes)?;
    if !save(kv, &snapshot.key, Some(&snapshot.bytes), &mut state).await? {
        return Err(anyhow!(
            "latest user reply changed while confirming deletion; start a new delete request"
        ));
    }
    check_expiry()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ProtoKeyValueStoreExt;
    use crate::test_support::MockKvStore;
    fn message(id: &str, text: &str) -> data_proto::SessionMessage {
        data_proto::SessionMessage {
            id: id.into(),
            role: data_proto::MessageRole::RoleUser as i32,
            created_at: if id == "future" { i64::MAX } else { 1 },
            parts: vec![data_proto::SessionMessagePart {
                part_type: data_proto::SessionMessagePartType::Text as i32,
                content: text.into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }
    async fn input(kv: &MockKvStore, id: &str, text: &str) {
        assert!(
            append_admitted_session_message(kv, "ns", "agent", "session", &message(id, text))
                .await
                .unwrap()
        );
    }
    async fn pending(kv: &MockKvStore, id: &str) -> keys::ResourceKey {
        let key = keys::ResourceKey::new(
            "ns",
            &[("Agent", "agent"), ("Session", "session")],
            "DeleteConfirmation",
            id,
        );
        kv.set(&key, b"pending").await.unwrap();
        key
    }
    async fn snapshot(kv: &MockKvStore, after: u64) -> Result<UserConfirmationSnapshot> {
        validate_user_confirmation(kv, "ns", "agent", "session", after, "exact reply").await
    }
    fn deadline() -> i64 {
        chrono::Utc::now().timestamp_micros() + 60_000_000
    }

    #[tokio::test]
    async fn queue_acceptance_order_survives_promotion_and_client_clock_skew() {
        let kv = MockKvStore::default();
        input(&kv, "future", "before request").await;
        let after = current_message_admission_order(&kv, "ns", "agent", "session")
            .await
            .unwrap();
        let mut queued = message("promoted", "exact reply");
        persist_queued_session_message(&kv, "ns", "agent", "session", "steer", "entry", &queued)
            .await
            .unwrap();
        assert!(
            snapshot(&kv, after).await.is_ok(),
            "durably queued reply qualifies before promotion"
        );
        input(&kv, "a-client-id", "cancel deletion").await;
        let order = current_message_admission_order(&kv, "ns", "agent", "session")
            .await
            .unwrap();
        persist_promoted_session_message(&kv, "ns", "agent", "session", "entry", &mut queued)
            .await
            .unwrap();
        assert_eq!(
            current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap(),
            order
        );
        assert!(
            snapshot(&kv, after).await.is_err(),
            "older queued approval cannot supersede newer cancellation"
        );
    }

    #[tokio::test]
    async fn pending_admission_blocks_confirmation_and_invalidates_existing_snapshot() {
        let kv = MockKvStore::default();
        input(&kv, "approval", "exact reply").await;
        let old = snapshot(&kv, 0).await.unwrap();
        let newer = message("cancellation", "cancel");
        let write = begin_input(&kv, "ns", "agent", "session", &newer, true)
            .await
            .unwrap();
        assert!(snapshot(&kv, 0)
            .await
            .unwrap_err()
            .to_string()
            .contains("pending"));
        let key = pending(&kv, "token").await;
        assert!(
            consume_user_confirmation(&kv, old, &key, b"pending", deadline())
                .await
                .unwrap_err()
                .to_string()
                .contains("changed")
        );
        kv.set_msg(
            &keys::session_message("ns", "agent", "session", &newer.id),
            &newer,
        )
        .await
        .unwrap();
        finish(&kv, "ns", "agent", "session", write.as_ref(), &newer)
            .await
            .unwrap();
        assert!(snapshot(&kv, 0).await.is_err());
        input(&kv, "fresh-approval", "exact reply").await;
        assert!(
            snapshot(&kv, 0).await.is_ok(),
            "fresh input recovers from an interrupted older admission"
        );
    }

    #[tokio::test]
    async fn edits_fence_consumption_without_readmitting_old_messages() {
        let kv = MockKvStore::default();
        input(&kv, "old", "before request").await;
        let after = current_message_admission_order(&kv, "ns", "agent", "session")
            .await
            .unwrap();
        let old_key = keys::session_message("ns", "agent", "session", "old");
        let raw = kv.get(&old_key).await.unwrap().unwrap();
        assert!(edit_admitted_session_message(
            &kv,
            "ns",
            "agent",
            "session",
            &raw,
            &message("old", "exact reply")
        )
        .await
        .unwrap());
        assert!(snapshot(&kv, after).await.is_err());
        assert!(!append_admitted_session_message(
            &kv,
            "ns",
            "agent",
            "session",
            &message("old", "exact reply")
        )
        .await
        .unwrap());
        input(&kv, "new", "exact reply").await;
        let old = snapshot(&kv, after).await.unwrap();
        let key = keys::session_message("ns", "agent", "session", "new");
        let raw = kv.get(&key).await.unwrap().unwrap();
        assert!(edit_admitted_session_message(
            &kv,
            "ns",
            "agent",
            "session",
            &raw,
            &message("new", "cancel")
        )
        .await
        .unwrap());
        let token = pending(&kv, "edit-token").await;
        assert!(
            consume_user_confirmation(&kv, old, &token, b"pending", deadline())
                .await
                .is_err()
        );
        assert!(snapshot(&kv, after).await.is_err());
    }

    #[tokio::test]
    async fn promotion_retry_preserves_edits_and_original_admission() {
        let kv = MockKvStore::default();
        let mut queued = message("canonical", "exact reply");
        persist_queued_session_message(&kv, "ns", "agent", "session", "steer", "entry", &queued)
            .await
            .unwrap();
        persist_promoted_session_message(&kv, "ns", "agent", "session", "entry", &mut queued)
            .await
            .unwrap();
        let old = snapshot(&kv, 0).await.unwrap();
        let key = keys::session_message("ns", "agent", "session", "canonical");
        let raw = kv.get(&key).await.unwrap().unwrap();
        let edited = message("canonical", "cancel");
        assert!(
            edit_admitted_session_message(&kv, "ns", "agent", "session", &raw, &edited)
                .await
                .unwrap()
        );
        let mut retry = message("different-retry-id", "exact reply");
        persist_promoted_session_message(&kv, "ns", "agent", "session", "entry", &mut retry)
            .await
            .unwrap();
        assert_eq!(retry.id, "canonical");
        assert_eq!(
            kv.get_msg::<data_proto::SessionMessage>(&key)
                .await
                .unwrap()
                .unwrap(),
            edited
        );
        let token = pending(&kv, "promote-token").await;
        assert!(
            consume_user_confirmation(&kv, old, &token, b"pending", deadline())
                .await
                .is_err()
        );
        assert!(snapshot(&kv, 0).await.is_err());
    }

    #[tokio::test]
    async fn consumption_rechecks_expiry_and_is_single_use_under_concurrency() {
        let kv = MockKvStore::default();
        input(&kv, "approval", "exact reply").await;
        let expired = pending(&kv, "expired").await;
        assert!(consume_user_confirmation(
            &kv,
            snapshot(&kv, 0).await.unwrap(),
            &expired,
            b"pending",
            0
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("expired"));
        assert_eq!(kv.get(&expired).await.unwrap().unwrap(), b"pending");
        let key = pending(&kv, "token").await;
        let first = snapshot(&kv, 0).await.unwrap();
        let second = snapshot(&kv, 0).await.unwrap();
        let (a, b) = tokio::join!(
            consume_user_confirmation(&kv, first, &key, b"pending", deadline()),
            consume_user_confirmation(&kv, second, &key, b"pending", deadline())
        );
        assert_ne!(a.is_ok(), b.is_ok());
    }
}
