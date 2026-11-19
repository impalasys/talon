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

#[derive(Debug)]
pub struct SessionMessageAlreadyExistsError;
impl std::fmt::Display for SessionMessageAlreadyExistsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("session message ID already exists; submit a new turn with a fresh ID")
    }
}
impl std::error::Error for SessionMessageAlreadyExistsError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Receipt {
    order: u64,
    nonce: String,
}
#[derive(Serialize, Deserialize)]
struct QueueReceipt {
    #[serde(flatten)]
    receipt: Receipt,
    #[serde(default)]
    canonical_id: Option<String>,
    #[serde(default)]
    write_nonce: String,
    #[serde(default)]
    payload_digest: String,
    #[serde(default)]
    reply_digest: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct AdmissionState {
    order: u64,
    #[serde(default)]
    consumed_through_order: u64,
    revision: String,
    latest: Option<Receipt>,
    pending_write: Option<String>,
    #[serde(default)]
    pending_kind: String,
    #[serde(default)]
    pending_digest: String,
    #[serde(default)]
    pending_payload_digest: String,
    #[serde(default)]
    completed_by: String,
    text_digest: String,
    canonical_id: Option<String>,
}
struct AdmissionWrite {
    receipt: Receipt,
    nonce: String,
    completion: String,
    payload_committed: bool,
    retry: bool,
}

#[derive(Debug)]
pub struct UserConfirmationSnapshot {
    key: keys::ResourceKey,
    bytes: Vec<u8>,
    requested_after: u64,
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
fn payload_digest(message: &data_proto::SessionMessage) -> String {
    fn hash_map(hasher: &mut Sha256, map: &std::collections::HashMap<String, String>) {
        let mut entries = map.iter().collect::<Vec<_>>();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        hasher.update((entries.len() as u64).to_le_bytes());
        for (key, value) in entries {
            hasher.update((key.len() as u64).to_le_bytes());
            hasher.update(key.as_bytes());
            hasher.update((value.len() as u64).to_le_bytes());
            hasher.update(value.as_bytes());
        }
    }
    let mut payload = message.clone();
    payload.created_at = 0;
    let labels = std::mem::take(&mut payload.labels);
    let mut metadata = Vec::with_capacity(payload.parts.len());
    for part in &mut payload.parts {
        part.created_at = 0;
        metadata.push(
            part.object
                .as_mut()
                .map(|object| std::mem::take(&mut object.metadata)),
        );
    }
    let bytes = payload.encode_to_vec();
    let mut hasher = Sha256::new();
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
    hash_map(&mut hasher, &labels);
    for map in metadata {
        hasher.update([u8::from(map.is_some())]);
        if let Some(map) = map {
            hash_map(&mut hasher, &map);
        }
    }
    format!("{:x}", hasher.finalize())
}
fn text_digest(message: &data_proto::SessionMessage) -> String {
    if message
        .parts
        .iter()
        .any(|part| part.part_type != data_proto::SessionMessagePartType::Text as i32)
    {
        return String::new();
    }
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
    let bytes = serde_json::to_vec(state)?;
    match kv.compare_and_swap(key, expected, &bytes).await {
        Ok(changed) => Ok(changed),
        Err(error) => match kv.get(key).await {
            Ok(Some(current)) if current == bytes => Ok(true),
            Ok(_) => Err(error),
            Err(read_error) => Err(error.context(format!(
                "admission CAS outcome unknown; verification failed: {read_error}"
            ))),
        },
    }
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
        if canonical {
            // Read after the admission snapshot, so a competing append either
            // has materialized already or changes the state CAS below. A
            // duplicate must never readmit an old reply after newer input.
            if kv
                .get(&keys::session_message(ns, agent, session, &message.id))
                .await?
                .is_some()
            {
                return Ok(None);
            }
            if state.canonical_id.as_deref() == Some(message.id.as_str())
                && state.pending_write.is_some()
            {
                if state.pending_kind == "append"
                    && state.pending_payload_digest == payload_digest(message)
                {
                    return Ok(Some(AdmissionWrite {
                        receipt: state
                            .latest
                            .clone()
                            .ok_or_else(|| anyhow!("pending admission receipt missing"))?,
                        nonce: state.pending_write.clone().unwrap(),
                        completion: nonce(),
                        payload_committed: false,
                        retry: true,
                    }));
                }
                return Err(anyhow!(
                    "message ID has a different pending append; use a fresh ID"
                ));
            }
        }
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
        state.pending_kind = if canonical { "append" } else { "queue" }.into();
        state.pending_digest = text_digest(message);
        state.pending_payload_digest = payload_digest(message);
        state.completed_by.clear();
        state.text_digest.clear();
        state.canonical_id = canonical.then(|| message.id.clone());
        if save(kv, &key, raw.as_deref(), &mut state).await? {
            return Ok(Some(AdmissionWrite {
                receipt,
                nonce: write_nonce,
                completion: nonce(),
                payload_committed: false,
                retry: false,
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
        if let Some(pending) = state.pending_write.as_ref() {
            if state.pending_kind == "edit"
                && state.pending_payload_digest == payload_digest(message)
            {
                return Ok(Some(AdmissionWrite {
                    receipt,
                    nonce: pending.clone(),
                    completion: nonce(),
                    payload_committed: false,
                    retry: true,
                }));
            }
            return Err(anyhow!("latest user message update is pending; retry the same update or send a fresh reply"));
        }
        let write_nonce = nonce();
        state.pending_write = Some(write_nonce.clone());
        state.pending_kind = "edit".into();
        state.pending_digest = text_digest(message);
        state.pending_payload_digest = payload_digest(message);
        state.completed_by.clear();
        if save(kv, &key, raw.as_deref(), &mut state).await? {
            return Ok(Some(AdmissionWrite {
                receipt,
                nonce: write_nonce,
                completion: nonce(),
                payload_committed: false,
                retry: false,
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
) -> Result<bool> {
    let Some(write) = write else {
        return Ok(true);
    };
    let key = state_key(ns, agent, session);
    for _ in 0..32 {
        let (raw, mut state) = load(kv, &key).await?;
        if state.latest.as_ref() != Some(&write.receipt) {
            return Ok(write.payload_committed);
        }
        if state.pending_write.as_deref() != Some(write.nonce.as_str()) {
            return Ok(state.completed_by == write.completion);
        }
        state.pending_write = None;
        state.pending_kind.clear();
        state.pending_digest.clear();
        state.pending_payload_digest.clear();
        state.completed_by = write.completion.clone();
        state.text_digest = text_digest(message);
        if save(kv, &key, raw.as_deref(), &mut state).await? {
            return Ok(true);
        }
    }
    Err(anyhow!("failed to complete user message admission"))
}

// A known failed insert owns no durable input. Clear only its own reservation,
// retaining the monotonic order but no confirmable text. Never restore an older
// snapshot: another writer may already have completed while it was superseded.
async fn abandon_insert(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    write: Option<&AdmissionWrite>,
) -> Result<()> {
    let Some(write) = write else { return Ok(()) };
    let key = state_key(ns, agent, session);
    for _ in 0..32 {
        let (raw, mut state) = load(kv, &key).await?;
        if state.latest.as_ref() != Some(&write.receipt)
            || state.pending_write.as_deref() != Some(write.nonce.as_str())
        {
            return Ok(());
        }
        state.pending_write = None;
        state.pending_kind.clear();
        state.pending_digest.clear();
        state.pending_payload_digest.clear();
        state.completed_by.clear();
        state.latest = None;
        state.canonical_id = None;
        state.text_digest.clear();
        if save(kv, &key, raw.as_deref(), &mut state).await? {
            return Ok(());
        }
    }
    Err(anyhow!("failed to clear unsuccessful message append"))
}

// Complete only the reservation owned by this write. If a storage call failed
// after committing, its durable payload is the authority; otherwise retain the
// durable winner's text or abandon an absent/non-User insert, never a newer fence.
async fn reconcile_write(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    write: Option<&AdmissionWrite>,
    desired: &data_proto::SessionMessage,
) -> Result<bool> {
    let key = keys::session_message(ns, agent, session, &desired.id);
    if let Some(raw) = kv.get(&key).await? {
        let current = data_proto::SessionMessage::decode(raw.as_slice())?;
        if is_user(&current) {
            let completed = finish(kv, ns, agent, session, write, &current).await?;
            return Ok(completed && payload_digest(&current) == payload_digest(desired));
        }
    }
    abandon_insert(kv, ns, agent, session, write).await?;
    Ok(false)
}

/// Transcript append retries can recover an input whose payload committed before
/// admission completion failed. Completed IDs always remain append-only.
pub async fn append_admitted_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    message: &data_proto::SessionMessage,
) -> Result<bool> {
    append_session_message(kv, ns, agent, session, message, true).await
}

async fn append_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    message: &data_proto::SessionMessage,
    recover_existing: bool,
) -> Result<bool> {
    let key = keys::session_message(ns, agent, session, &message.id);
    if let Some(raw) = kv.get(&key).await? {
        let current = data_proto::SessionMessage::decode(raw.as_slice())?;
        if recover_existing
            && is_user(message)
            && payload_digest(&current) == payload_digest(message)
        {
            let (_, state) = load(kv, &state_key(ns, agent, session)).await?;
            if state.canonical_id.as_deref() == Some(message.id.as_str())
                && state.pending_kind == "append"
                && state.pending_payload_digest == payload_digest(message)
            {
                if let (Some(receipt), Some(nonce)) = (state.latest, state.pending_write) {
                    return finish(
                        kv,
                        ns,
                        agent,
                        session,
                        Some(&AdmissionWrite {
                            receipt,
                            nonce,
                            completion: self::nonce(),
                            payload_committed: false,
                            retry: true,
                        }),
                        &current,
                    )
                    .await;
                }
            }
        }
        return Ok(false);
    }
    let mut write = begin_input(kv, ns, agent, session, message, true).await?;
    match kv
        .compare_and_swap(&key, None, &message.encode_to_vec())
        .await
    {
        Ok(true) => {
            if let Some(write) = write.as_mut() {
                write.payload_committed = true;
            }
        }
        Ok(false) => {
            if write.is_none() {
                return Ok(false);
            }
            return reconcile_write(kv, ns, agent, session, write.as_ref(), message).await;
        }
        Err(error) => {
            if write.is_none() {
                return Err(error);
            }
            if reconcile_write(kv, ns, agent, session, write.as_ref(), message).await? {
                return Ok(true);
            }
            return Err(error);
        }
    }
    match finish(kv, ns, agent, session, write.as_ref(), message).await {
        Ok(completed) => Ok(completed),
        Err(error) => {
            if reconcile_write(kv, ns, agent, session, write.as_ref(), message).await? {
                return Ok(true);
            }
            Err(error)
        }
    }
}

pub async fn edit_admitted_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    expected: &[u8],
    message: &data_proto::SessionMessage,
) -> Result<bool> {
    let mut write = begin_edit(kv, ns, agent, session, message).await?;
    let key = keys::session_message(ns, agent, session, &message.id);
    match kv
        .compare_and_swap(&key, Some(expected), &message.encode_to_vec())
        .await
    {
        Ok(true) => {
            if let Some(write) = write.as_mut() {
                write.payload_committed = true;
            }
        }
        Ok(false) => {
            let recovered =
                reconcile_write(kv, ns, agent, session, write.as_ref(), message).await?;
            return Ok(recovered && write.as_ref().is_some_and(|write| write.retry));
        }
        Err(error) => {
            if reconcile_write(kv, ns, agent, session, write.as_ref(), message).await? {
                return Ok(true);
            }
            return Err(error);
        }
    }
    match finish(kv, ns, agent, session, write.as_ref(), message).await {
        Ok(completed) => Ok(completed),
        Err(error) => {
            if reconcile_write(kv, ns, agent, session, write.as_ref(), message).await? {
                return Ok(true);
            }
            Err(error)
        }
    }
}

pub async fn persist_admitted_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    message: &data_proto::SessionMessage,
) -> Result<()> {
    // Completed IDs cannot become a new turn. An exact unfinished append may
    // recover its original reservation, without editing or readmitting it.
    if !append_session_message(kv, ns, agent, session, message, true).await? {
        return Err(SessionMessageAlreadyExistsError.into());
    }
    Ok(())
}

async fn finish_queued_admission(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    queued: &QueueReceipt,
    message: &data_proto::SessionMessage,
) -> Result<()> {
    if !queued.write_nonce.is_empty() {
        let key = state_key(ns, agent, session);
        // A known pre-commit failure can clear the owned fence while receipt
        // cleanup fails. An exact retry may restore only that same last order;
        // it must never move an older retry after a newer input.
        for attempt in 0..32 {
            let (raw, mut state) = load(kv, &key).await?;
            if state.latest.is_some() || state.order != queued.receipt.order {
                break;
            }
            if queued.reply_digest != text_digest(message) {
                return Err(anyhow!("queued retry does not match its original reply"));
            }
            state.latest = Some(queued.receipt.clone());
            state.pending_write = Some(queued.write_nonce.clone());
            state.pending_kind = "queue".into();
            state.pending_digest = queued.reply_digest.clone();
            state.pending_payload_digest = payload_digest(message);
            state.completed_by.clear();
            if save(kv, &key, raw.as_deref(), &mut state).await? {
                break;
            }
            if attempt == 31 {
                return Err(anyhow!("failed to recover queued admission"));
            }
        }
        let (_, state) = load(kv, &key).await?;
        if state.latest.as_ref() == Some(&queued.receipt)
            && state.pending_write.as_deref() == Some(queued.write_nonce.as_str())
            && state.pending_digest != text_digest(message)
        {
            return Err(anyhow!(
                "queued input does not match its admission reservation"
            ));
        }
        finish(
            kv,
            ns,
            agent,
            session,
            Some(&AdmissionWrite {
                receipt: queued.receipt.clone(),
                nonce: queued.write_nonce.clone(),
                completion: nonce(),
                payload_committed: true,
                retry: true,
            }),
            message,
        )
        .await?;
    }
    Ok(())
}

/// The atomic primary queue write is acceptance. All fallible preparation is
/// before it; separate confirmation-state finalization is recoverable from the
/// receipt and cannot turn a committed enqueue into a reported rejection.
pub async fn persist_queued_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    queue: &str,
    entry: &str,
    message: &data_proto::SessionMessage,
) -> Result<()> {
    let receipt_key = queue_receipt_key(ns, agent, session, entry);
    let payload_key = keys::session_queue_entry(ns, agent, session, queue, entry);
    let payload = message.encode_to_vec();
    let payload_digest = self::payload_digest(message);
    let (queued, receipt_bytes) = if let Some(raw) = kv.get(&receipt_key).await? {
        let queued: QueueReceipt = serde_json::from_slice(&raw)?;
        if queued.payload_digest != payload_digest {
            return Err(anyhow!(
                "queued message entry already exists with different content"
            ));
        }
        if queued.canonical_id.is_some() {
            // A prepared/dispatching entry already committed. Never recreate its
            // primary queue copy after terminal removal or overwrite an edit.
            if let Err(error) =
                finish_queued_admission(kv, ns, agent, session, &queued, message).await
            {
                tracing::warn!(%error, entry_id = %entry, "committed queue admission finalization will recover during dispatch");
            }
            return Ok(());
        }
        (queued, raw)
    } else {
        let write = begin_input(kv, ns, agent, session, message, false).await?;
        let Some(write) = write else {
            return Err(anyhow!("queued inputs must be User messages"));
        };
        let queued = QueueReceipt {
            receipt: write.receipt.clone(),
            canonical_id: None,
            write_nonce: write.nonce.clone(),
            payload_digest,
            reply_digest: text_digest(message),
        };
        let raw = serde_json::to_vec(&queued)?;
        match kv.compare_and_swap(&receipt_key, None, &raw).await {
            Ok(true) => {}
            Ok(false) => {
                abandon_insert(kv, ns, agent, session, Some(&write)).await?;
                return Err(anyhow!("queued message entry already exists"));
            }
            Err(error) => {
                if kv.get(&receipt_key).await?.as_deref() != Some(raw.as_slice()) {
                    let _ = abandon_insert(kv, ns, agent, session, Some(&write)).await;
                    return Err(error);
                }
            }
        }
        (queued, raw)
    };
    let inserted = kv.compare_and_swap(&payload_key, None, &payload).await;
    let committed = match inserted {
        Ok(true) => true,
        // An exact retry can arrive after a previous payload commit. If it was
        // already canonicalized, the original digest in the receipt is stable.
        Ok(false) | Err(_) => match kv.get(&payload_key).await? {
            Some(current)
                if self::payload_digest(&data_proto::SessionMessage::decode(
                    current.as_slice(),
                )?) == queued.payload_digest =>
            {
                true
            }
            Some(_) => kv
                .get(&receipt_key)
                .await?
                .map(|raw| serde_json::from_slice::<QueueReceipt>(&raw))
                .transpose()?
                .is_some_and(|current| {
                    current.payload_digest == queued.payload_digest
                        && current.canonical_id.is_some()
                }),
            _ => false,
        },
    };
    if !committed {
        // There is no primary commit by this request, so removing the staging
        // receipt cannot expose a previously rejected payload to consumers.
        let _ = kv.compare_and_delete(&receipt_key, &receipt_bytes).await;
        let write = AdmissionWrite {
            receipt: queued.receipt.clone(),
            nonce: queued.write_nonce.clone(),
            completion: nonce(),
            payload_committed: false,
            retry: false,
        };
        let _ = abandon_insert(kv, ns, agent, session, Some(&write)).await;
        return Err(inserted.err().unwrap_or_else(|| {
            anyhow!("queued message entry already exists with different content")
        }));
    }
    if let Err(error) = finish_queued_admission(kv, ns, agent, session, &queued, message).await {
        tracing::warn!(%error, entry_id = %entry, "queue write committed; dispatch will recover admission finalization");
    }
    Ok(())
}

/// Resolve a retry's durable payload without creating or preparing a new input.
pub(crate) async fn load_promoted_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    entry: &str,
    message: &mut data_proto::SessionMessage,
) -> Result<()> {
    if let Some(raw) = kv
        .get(&queue_receipt_key(ns, agent, session, entry))
        .await?
    {
        if let Some(id) = serde_json::from_slice::<QueueReceipt>(&raw)?.canonical_id {
            message.id = id;
        }
    }
    if !message.id.is_empty() {
        if let Some(raw) = kv
            .get(&keys::session_message(ns, agent, session, &message.id))
            .await?
        {
            *message = data_proto::SessionMessage::decode(raw.as_slice())?;
        }
    }
    Ok(())
}

pub async fn persist_promoted_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    entry: &str,
    message: &mut data_proto::SessionMessage,
) -> Result<()> {
    // The per-entry mapping outlives the latest-input fence. Bind it first so
    // retries after newer admissions still identify the same canonical input.
    let receipt_key = queue_receipt_key(ns, agent, session, entry);
    let mut receipt = None;
    for attempt in 0..32 {
        let Some(raw) = kv.get(&receipt_key).await? else {
            break;
        };
        let mut queued: QueueReceipt = serde_json::from_slice(&raw)?;
        finish_queued_admission(kv, ns, agent, session, &queued, message).await?;
        if let Some(id) = queued.canonical_id.as_ref() {
            message.id = id.clone();
            receipt = Some(queued.receipt);
            break;
        }
        queued.canonical_id = Some(message.id.clone());
        if kv
            .compare_and_swap(&receipt_key, Some(&raw), &serde_json::to_vec(&queued)?)
            .await?
        {
            receipt = Some(queued.receipt);
            break;
        }
        if attempt == 31 {
            return Err(anyhow!("failed to bind queued message ID"));
        }
    }
    if let Some(receipt) = receipt {
        let key = state_key(ns, agent, session);
        for attempt in 0..32 {
            let (raw, mut state) = load(kv, &key).await?;
            if state.latest.as_ref() != Some(&receipt)
                || state.canonical_id.as_deref() == Some(&message.id)
            {
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
    let key = keys::session_message(ns, agent, session, &message.id);
    // The caller dispatches this value, so reusing only the ID is insufficient:
    // a retry must execute the durable edited payload as well.
    if !kv
        .compare_and_swap(&key, None, &message.encode_to_vec())
        .await?
    {
        *message = data_proto::SessionMessage::decode(
            kv.get(&key)
                .await?
                .ok_or_else(|| anyhow!("canonical queued message disappeared"))?
                .as_slice(),
        )?;
    }
    Ok(())
}

/// Call only on terminal queue removal, never when moving or retrying an entry.
pub(crate) async fn remove_queue_receipt(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    entry: &str,
) -> Result<()> {
    kv.delete(&queue_receipt_key(ns, agent, session, entry))
        .await
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
        || state.order <= state.consumed_through_order
        || state.consumed_through_order > after
        || state.pending_write.is_some()
        || state.text_digest != digest(required_reply)
    {
        return Err(anyhow!("delete requires a separate user message exactly: {}; latest input may be pending or different", required_reply));
    }
    Ok(UserConfirmationSnapshot {
        key,
        requested_after: after,
        bytes: raw.ok_or_else(|| anyhow!("user confirmation state missing"))?,
    })
}

/// Atomic token removal and session-state CAS jointly fence consumption. If a newer
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
    let mut state: AdmissionState = serde_json::from_slice(&snapshot.bytes)?;
    if state.order <= state.consumed_through_order
        || state.consumed_through_order > snapshot.requested_after
    {
        return Err(anyhow!("user approval has already been consumed"));
    }
    let removed = match kv.compare_and_delete(confirmation_key, pending).await {
        Ok(removed) => removed,
        Err(error) => match kv.get(confirmation_key).await {
            Ok(None) => true,
            Ok(Some(_)) => return Err(error),
            Err(read_error) => {
                return Err(error.context(format!(
                    "confirmation consumption outcome unknown; verification failed: {read_error}"
                )))
            }
        },
    };
    if !removed {
        return Err(anyhow!("delete confirmation is missing or already used"));
    }
    check_expiry()?;
    state.consumed_through_order = state.order;
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

    #[derive(Default)]
    struct FaultKv {
        inner: MockKvStore,
        payload_error: std::sync::atomic::AtomicUsize,
        queue_payload_error: std::sync::atomic::AtomicUsize,
        finish_error: std::sync::atomic::AtomicBool,
        finish_after_commit: std::sync::atomic::AtomicBool,
        fail_reconciliation_read: std::sync::atomic::AtomicBool,
        read_error: std::sync::atomic::AtomicBool,
        receipt_cleanup_error: std::sync::atomic::AtomicBool,
        confirmation_delete_error: std::sync::atomic::AtomicBool,
        pause_queue: std::sync::atomic::AtomicBool,
        queue_entered: tokio::sync::Notify,
        queue_release: tokio::sync::Notify,
    }
    #[async_trait::async_trait]
    impl KeyValueStore for FaultKv {
        async fn get(&self, key: &keys::ResourceKey) -> Result<Option<Vec<u8>>> {
            if self
                .read_error
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                return Err(anyhow!("injected reconciliation read outage"));
            }
            self.inner.get(key).await
        }
        async fn set(&self, key: &keys::ResourceKey, value: &[u8]) -> Result<()> {
            self.inner.set(key, value).await
        }
        async fn compare_and_swap(
            &self,
            key: &keys::ResourceKey,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> Result<bool> {
            use std::sync::atomic::Ordering::SeqCst;
            if key.kind == "SessionMessage"
                && key.parent_path.contains("/Queue/")
                && self.pause_queue.swap(false, SeqCst)
            {
                self.queue_entered.notify_one();
                self.queue_release.notified().await;
            }
            if key.kind == "SessionMessage" && key.parent_path.contains("/Queue/") {
                let mode = self.queue_payload_error.swap(0, SeqCst);
                if mode == 1 {
                    return Err(anyhow!("injected queue write failure before commit"));
                }
                let result = self.inner.compare_and_swap(key, expected, value).await?;
                if mode == 2 {
                    return Err(anyhow!("injected queue write failure after commit"));
                }
                return Ok(result);
            }
            if key.kind == "SessionMessage" && !key.parent_path.contains("/Queue/") {
                let mode = self.payload_error.swap(0, SeqCst);
                if mode == 1 {
                    if self.fail_reconciliation_read.swap(false, SeqCst) {
                        self.read_error.store(true, SeqCst);
                    }
                    return Err(anyhow!("injected payload failure before commit"));
                }
                let result = self.inner.compare_and_swap(key, expected, value).await?;
                if mode == 2 {
                    if self.fail_reconciliation_read.swap(false, SeqCst) {
                        self.read_error.store(true, SeqCst);
                    }
                    return Err(anyhow!("injected payload failure after commit"));
                }
                return Ok(result);
            }
            if key.kind == "MessageAdmissionState"
                && expected.is_some()
                && serde_json::from_slice::<AdmissionState>(value)?
                    .pending_write
                    .is_none()
                && self.finish_error.swap(false, SeqCst)
            {
                return Err(anyhow!("injected finish failure"));
            }
            if key.kind == "MessageAdmissionState"
                && expected.is_some()
                && serde_json::from_slice::<AdmissionState>(value)?
                    .pending_write
                    .is_none()
                && self.finish_after_commit.swap(false, SeqCst)
            {
                self.inner.compare_and_swap(key, expected, value).await?;
                return Err(anyhow!("injected finish response failure after commit"));
            }
            self.inner.compare_and_swap(key, expected, value).await
        }
        async fn compare_and_delete(
            &self,
            key: &keys::ResourceKey,
            expected: &[u8],
        ) -> Result<bool> {
            if key.kind == "QueuedMessageAdmission"
                && self
                    .receipt_cleanup_error
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                return Err(anyhow!("injected failed-queue receipt cleanup error"));
            }
            if key.kind == "DeleteConfirmation"
                && self
                    .confirmation_delete_error
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                self.inner.compare_and_delete(key, expected).await?;
                return Err(anyhow!("injected confirmation delete response failure"));
            }
            self.inner.compare_and_delete(key, expected).await
        }
        async fn delete(&self, key: &keys::ResourceKey) -> Result<()> {
            self.inner.delete(key).await
        }
        async fn list_keys(
            &self,
            list: &keys::ResourceList,
            options: Option<crate::control::ListOptions<'_>>,
        ) -> Result<Vec<keys::ResourceKey>> {
            self.inner.list_keys(list, options).await
        }
    }

    #[tokio::test]
    async fn payload_hash_canonicalizes_all_maps_and_ignores_only_timestamps() {
        let mut original = message("map-retry", "exact reply");
        original.labels = [
            ("z".into(), "last".into()),
            ("a".into(), "first".into()),
            ("m".into(), "middle".into()),
        ]
        .into_iter()
        .collect();
        original.parts[0].object = Some(data_proto::ObjectRef {
            key: "object-key".into(),
            metadata: [("z".into(), "last".into()), ("a".into(), "first".into())]
                .into_iter()
                .collect(),
            ..Default::default()
        });
        let expected = payload_digest(&original);
        for _ in 0..32 {
            let mut decoded =
                data_proto::SessionMessage::decode(original.encode_to_vec().as_slice()).unwrap();
            decoded.created_at += 100;
            decoded.parts[0].created_at += 100;
            assert_eq!(payload_digest(&decoded), expected);
            decoded.labels.insert("a".into(), "changed".into());
            assert_ne!(payload_digest(&decoded), expected);
            decoded = original.clone();
            decoded.parts[0]
                .object
                .as_mut()
                .unwrap()
                .metadata
                .insert("a".into(), "changed".into());
            assert_ne!(payload_digest(&decoded), expected);
        }
    }

    #[tokio::test]
    async fn pending_new_turn_retry_recovers_lost_payload_response_and_readback() {
        use std::sync::atomic::Ordering::SeqCst;
        let kv = FaultKv::default();
        let pubsub = crate::test_support::RecordingPubSub::default();
        let session_key = keys::session("ns", "agent", "session");
        kv.set_msg(
            &session_key,
            &data_proto::Session {
                id: "session".into(),
                status: "IDLE".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        kv.payload_error.store(2, SeqCst);
        kv.fail_reconciliation_read.store(true, SeqCst);
        let mut input = message("turn-retry", "cancel");
        input.created_at = 0;
        input.labels = [("z".into(), "last".into()), ("a".into(), "first".into())]
            .into_iter()
            .collect();
        assert!(crate::control::scheduling::send_session_message(
            &kv,
            &pubsub,
            "ns",
            "agent",
            "session",
            input.clone(),
            chrono::Utc::now()
        )
        .await
        .is_err());
        let before = current_message_admission_order(&kv, "ns", "agent", "session")
            .await
            .unwrap();
        crate::control::scheduling::send_session_message(
            &kv,
            &pubsub,
            "ns",
            "agent",
            "session",
            input.clone(),
            chrono::Utc::now(),
        )
        .await
        .unwrap();
        assert_eq!(
            current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap(),
            before
        );
        assert!(load(&kv, &state_key("ns", "agent", "session"))
            .await
            .unwrap()
            .1
            .pending_write
            .is_none());
        assert_eq!(
            kv.list_keys(
                &keys::session_submission_prefix("ns", "agent", "session"),
                None
            )
            .await
            .unwrap()
            .len(),
            1
        );
        let mut session: data_proto::Session = kv.get_msg(&session_key).await.unwrap().unwrap();
        session.status = "IDLE".into();
        kv.set_msg(&session_key, &session).await.unwrap();
        assert!(crate::control::scheduling::send_session_message(
            &kv,
            &pubsub,
            "ns",
            "agent",
            "session",
            input,
            chrono::Utc::now()
        )
        .await
        .unwrap_err()
        .downcast_ref::<SessionMessageAlreadyExistsError>()
        .is_some());
        let messages = pubsub.published.lock().await;
        assert_eq!(
            messages
                .iter()
                .filter(|(topic, _)| topic == crate::control::topics::SESSION_DISPATCH_TOPIC)
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn exact_edit_retries_recover_retained_reservations_before_or_after_commit() {
        use std::sync::atomic::Ordering::SeqCst;
        for mode in [1, 2] {
            let kv = FaultKv::default();
            let original = message("edit-retry", "exact reply");
            append_admitted_session_message(&kv, "ns", "agent", "session", &original)
                .await
                .unwrap();
            let desired = message("edit-retry", "cancel");
            kv.payload_error.store(mode, SeqCst);
            kv.fail_reconciliation_read.store(true, SeqCst);
            assert!(edit_admitted_session_message(
                &kv,
                "ns",
                "agent",
                "session",
                &original.encode_to_vec(),
                &desired
            )
            .await
            .is_err());
            let before = current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap();
            assert!(edit_admitted_session_message(
                &kv,
                "ns",
                "agent",
                "session",
                &original.encode_to_vec(),
                &desired
            )
            .await
            .unwrap());
            assert_eq!(
                current_message_admission_order(&kv, "ns", "agent", "session")
                    .await
                    .unwrap(),
                before
            );
            assert!(load(&kv, &state_key("ns", "agent", "session"))
                .await
                .unwrap()
                .1
                .pending_write
                .is_none());
            assert!(
                validate_user_confirmation(&kv, "ns", "agent", "session", 0, "exact reply")
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn confirmation_consumption_reconciles_lost_conditional_responses_once() {
        use std::sync::atomic::Ordering::SeqCst;
        for mode in [1, 2, 3] {
            let kv = FaultKv::default();
            append_admitted_session_message(
                &kv,
                "ns",
                "agent",
                "session",
                &message("approval", "exact reply"),
            )
            .await
            .unwrap();
            let token = pending(&kv.inner, "lost-consumption").await;
            let approved =
                validate_user_confirmation(&kv, "ns", "agent", "session", 0, "exact reply")
                    .await
                    .unwrap();
            kv.confirmation_delete_error.store(mode != 2, SeqCst);
            kv.finish_after_commit.store(mode != 1, SeqCst);
            consume_user_confirmation(&kv, approved, &token, b"pending", deadline())
                .await
                .unwrap();
            assert!(kv.get(&token).await.unwrap().is_none());
            assert!(
                validate_user_confirmation(&kv, "ns", "agent", "session", 0, "exact reply")
                    .await
                    .is_err()
            );
            let key = state_key("ns", "agent", "session");
            let bytes = kv.get(&key).await.unwrap().unwrap();
            kv.confirmation_delete_error.store(true, SeqCst);
            assert!(consume_user_confirmation(
                &kv,
                UserConfirmationSnapshot {
                    key: key.clone(),
                    bytes: bytes.clone(),
                    requested_after: 0,
                },
                &token,
                b"pending",
                deadline()
            )
            .await
            .is_err());
            assert_eq!(kv.get(&key).await.unwrap().unwrap(), bytes);
            append_admitted_session_message(
                &kv,
                "ns",
                "agent",
                "session",
                &message("replayed-text", "exact reply"),
            )
            .await
            .unwrap();
            assert!(
                validate_user_confirmation(&kv, "ns", "agent", "session", 0, "exact reply")
                    .await
                    .is_err()
            );
            let after = current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap();
            let fresh = pending(&kv.inner, "fresh-consumption").await;
            append_admitted_session_message(
                &kv,
                "ns",
                "agent",
                "session",
                &message("fresh-reply", "exact reply"),
            )
            .await
            .unwrap();
            consume_user_confirmation(
                &kv,
                validate_user_confirmation(&kv, "ns", "agent", "session", after, "exact reply")
                    .await
                    .unwrap(),
                &fresh,
                b"pending",
                deadline(),
            )
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn append_storage_errors_reconcile_before_and_after_payload_commit() {
        use std::sync::atomic::Ordering::SeqCst;
        for mode in [1, 2, 3, 4] {
            let kv = FaultKv::default();
            kv.payload_error
                .store(if mode < 3 { mode } else { 0 }, SeqCst);
            kv.finish_error.store(mode == 3, SeqCst);
            kv.finish_after_commit.store(mode == 4, SeqCst);
            let reply = message("reply", "exact reply");
            let result =
                append_admitted_session_message(&kv, "ns", "agent", "session", &reply).await;
            if mode == 1 {
                assert!(result.is_err());
                assert!(
                    append_admitted_session_message(&kv, "ns", "agent", "session", &reply)
                        .await
                        .unwrap()
                );
            } else {
                assert!(result.unwrap());
            }
            validate_user_confirmation(&kv, "ns", "agent", "session", 0, "exact reply")
                .await
                .unwrap();
            assert!(
                !append_admitted_session_message(&kv, "ns", "agent", "session", &reply)
                    .await
                    .unwrap()
            );
        }
    }

    #[tokio::test]
    async fn exact_retry_reuses_retained_reservation_after_payload_and_read_outage() {
        use std::sync::atomic::Ordering::SeqCst;
        let kv = FaultKv::default();
        kv.payload_error.store(1, SeqCst);
        kv.fail_reconciliation_read.store(true, SeqCst);
        let original = message("retry", "exact reply");
        assert!(
            append_admitted_session_message(&kv, "ns", "agent", "session", &original)
                .await
                .is_err()
        );
        let before = current_message_admission_order(&kv, "ns", "agent", "session")
            .await
            .unwrap();
        let mut retry = original.clone();
        retry.created_at = 100;
        retry.parts[0].created_at = 100;
        assert!(
            append_admitted_session_message(&kv, "ns", "agent", "session", &retry)
                .await
                .unwrap()
        );
        assert_eq!(
            current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap(),
            before
        );
        validate_user_confirmation(&kv, "ns", "agent", "session", 0, "exact reply")
            .await
            .unwrap();
        assert!(
            !append_admitted_session_message(&kv, "ns", "agent", "session", &retry)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn edit_storage_errors_reconcile_without_retaining_a_pending_fence() {
        use std::sync::atomic::Ordering::SeqCst;
        for mode in [1, 2, 3, 4] {
            let kv = FaultKv::default();
            let original = message("reply", "exact reply");
            append_admitted_session_message(&kv, "ns", "agent", "session", &original)
                .await
                .unwrap();
            kv.payload_error
                .store(if mode < 3 { mode } else { 0 }, SeqCst);
            kv.finish_error.store(mode == 3, SeqCst);
            kv.finish_after_commit.store(mode == 4, SeqCst);
            let cancel = message("reply", "cancel");
            let result = edit_admitted_session_message(
                &kv,
                "ns",
                "agent",
                "session",
                &original.encode_to_vec(),
                &cancel,
            )
            .await;
            if mode == 1 {
                assert!(result.is_err());
                assert!(edit_admitted_session_message(
                    &kv,
                    "ns",
                    "agent",
                    "session",
                    &original.encode_to_vec(),
                    &cancel
                )
                .await
                .unwrap());
            } else {
                assert!(result.unwrap());
            }
            assert!(load(&kv, &state_key("ns", "agent", "session"))
                .await
                .unwrap()
                .1
                .pending_write
                .is_none());
            assert!(
                validate_user_confirmation(&kv, "ns", "agent", "session", 0, "exact reply")
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn retry_recovers_committed_append_without_readmitting_a_completed_message() {
        let kv = MockKvStore::default();
        let reply = message("reply", "exact reply");
        let write = begin_input(&kv, "ns", "agent", "session", &reply, true)
            .await
            .unwrap()
            .unwrap();
        kv.set(
            &keys::session_message("ns", "agent", "session", "reply"),
            &reply.encode_to_vec(),
        )
        .await
        .unwrap();
        assert!(
            append_admitted_session_message(&kv, "ns", "agent", "session", &reply)
                .await
                .unwrap()
        );
        assert!(!finish(&kv, "ns", "agent", "session", Some(&write), &reply)
            .await
            .unwrap());
        assert!(
            !append_admitted_session_message(&kv, "ns", "agent", "session", &reply)
                .await
                .unwrap()
        );
        assert_eq!(
            current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap(),
            1
        );
        validate_user_confirmation(&kv, "ns", "agent", "session", 0, "exact reply")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn failed_queue_write_is_not_dispatched_or_duplicated_on_retry() {
        use std::sync::atomic::Ordering::SeqCst;
        let kv = FaultKv::default();
        kv.queue_payload_error.store(1, SeqCst);
        let input = message("", "cancel");
        assert!(persist_queued_session_message(
            &kv, "ns", "agent", "session", "steer", "entry", &input
        )
        .await
        .is_err());
        assert!(kv
            .get(&keys::session_queue_entry(
                "ns", "agent", "session", "steer", "entry"
            ))
            .await
            .unwrap()
            .is_none());
        assert!(crate::control::session_queue::prepare_steer_batch(
            &kv,
            "ns",
            "agent",
            "session",
            8,
            100,
            chrono::Utc::now()
        )
        .await
        .unwrap()
        .is_empty());
        assert!(
            validate_user_confirmation(&kv, "ns", "agent", "session", 0, "cancel")
                .await
                .is_err()
        );
        persist_queued_session_message(&kv, "ns", "agent", "session", "steer", "entry", &input)
            .await
            .unwrap();
        let batch = crate::control::session_queue::prepare_steer_batch(
            &kv,
            "ns",
            "agent",
            "session",
            8,
            100,
            chrono::Utc::now(),
        )
        .await
        .unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(
            kv.list_keys(
                &keys::session_message_prefix("ns", "agent", "session"),
                None
            )
            .await
            .unwrap()
            .len(),
            1
        );
    }

    #[tokio::test]
    async fn retained_failed_queue_receipt_retries_without_overtaking_newer_input() {
        use std::sync::atomic::Ordering::SeqCst;
        for newer_input in [false, true] {
            let kv = FaultKv::default();
            kv.queue_payload_error.store(1, SeqCst);
            kv.receipt_cleanup_error.store(true, SeqCst);
            let approval = message("", "exact reply");
            assert!(persist_queued_session_message(
                &kv, "ns", "agent", "session", "steer", "entry", &approval
            )
            .await
            .is_err());
            if newer_input {
                append_admitted_session_message(
                    &kv,
                    "ns",
                    "agent",
                    "session",
                    &message("newer", "cancel"),
                )
                .await
                .unwrap();
            }
            let before = current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap();
            persist_queued_session_message(
                &kv, "ns", "agent", "session", "steer", "entry", &approval,
            )
            .await
            .unwrap();
            assert_eq!(
                current_message_admission_order(&kv, "ns", "agent", "session")
                    .await
                    .unwrap(),
                before
            );
            assert_eq!(
                validate_user_confirmation(&kv, "ns", "agent", "session", 0, "exact reply")
                    .await
                    .is_err(),
                newer_input
            );
        }
    }

    #[tokio::test]
    async fn committed_queue_ack_and_promotion_recover_failed_state_finalization() {
        use std::sync::atomic::Ordering::SeqCst;
        let kv = FaultKv::default();
        kv.finish_error.store(true, SeqCst);
        let input = message("", "exact reply");
        // Acceptance committed: the call reports success even when its separate
        // confirmation-state finalization needs recovery by dispatch.
        persist_queued_session_message(&kv, "ns", "agent", "session", "steer", "entry", &input)
            .await
            .unwrap();
        assert!(kv
            .get(&keys::session_queue_entry(
                "ns", "agent", "session", "steer", "entry"
            ))
            .await
            .unwrap()
            .is_some());
        assert!(
            validate_user_confirmation(&kv, "ns", "agent", "session", 0, "exact reply")
                .await
                .is_err()
        );
        let batch = crate::control::session_queue::prepare_steer_batch(
            &kv,
            "ns",
            "agent",
            "session",
            8,
            100,
            chrono::Utc::now(),
        )
        .await
        .unwrap();
        assert_eq!(batch.len(), 1);
        validate_user_confirmation(&kv, "ns", "agent", "session", 0, "exact reply")
            .await
            .unwrap();
        // Retrying the same active enqueue also returns success without a new
        // admission order or second queue entry.
        let before = current_message_admission_order(&kv, "ns", "agent", "session")
            .await
            .unwrap();
        persist_queued_session_message(&kv, "ns", "agent", "session", "steer", "entry", &input)
            .await
            .unwrap();
        assert_eq!(
            current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap(),
            before
        );
        assert_eq!(
            kv.list_keys(
                &keys::session_message_prefix("ns", "agent", "session"),
                None
            )
            .await
            .unwrap()
            .len(),
            1
        );
    }

    #[tokio::test]
    async fn receipt_without_committed_payload_is_invisible_to_dispatch() {
        let kv = MockKvStore::default();
        let input = message("", "cancel");
        let receipt = QueueReceipt {
            receipt: Receipt {
                order: 1,
                nonce: "pending".into(),
            },
            canonical_id: None,
            write_nonce: "write".into(),
            payload_digest: payload_digest(&input),
            reply_digest: text_digest(&input),
        };
        kv.set(
            &queue_receipt_key("ns", "agent", "session", "entry"),
            &serde_json::to_vec(&receipt).unwrap(),
        )
        .await
        .unwrap();
        assert!(crate::control::session_queue::prepare_steer_batch(
            &kv,
            "ns",
            "agent",
            "session",
            8,
            100,
            chrono::Utc::now()
        )
        .await
        .unwrap()
        .is_empty());
        assert!(crate::control::session_queue::dispatch_next_queued_message(
            &kv,
            &crate::test_support::RecordingPubSub::default(),
            "ns",
            "agent",
            "session",
            "steer",
            chrono::Utc::now()
        )
        .await
        .unwrap()
        .is_none());
        crate::control::session_queue::reclassify_steer_queue_to_next(
            &kv, "ns", "agent", "session",
        )
        .await
        .unwrap();
        assert!(kv
            .list_keys(
                &keys::session_message_prefix("ns", "agent", "session"),
                None
            )
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn queue_write_error_after_commit_recovers_success_without_duplicate_admission() {
        use std::sync::atomic::Ordering::SeqCst;
        let kv = FaultKv::default();
        kv.queue_payload_error.store(2, SeqCst);
        let input = message("", "exact reply");
        persist_queued_session_message(&kv, "ns", "agent", "session", "steer", "entry", &input)
            .await
            .unwrap();
        let before = current_message_admission_order(&kv, "ns", "agent", "session")
            .await
            .unwrap();
        persist_queued_session_message(&kv, "ns", "agent", "session", "steer", "entry", &input)
            .await
            .unwrap();
        assert_eq!(
            current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap(),
            before
        );
        let batch = crate::control::session_queue::prepare_steer_batch(
            &kv,
            "ns",
            "agent",
            "session",
            8,
            100,
            chrono::Utc::now(),
        )
        .await
        .unwrap();
        assert_eq!(batch.len(), 1);
    }

    #[tokio::test]
    async fn overlapping_inputs_keep_server_order_when_older_queue_write_finishes_last() {
        use std::sync::atomic::Ordering::SeqCst;
        let kv = FaultKv::default();
        kv.pause_queue.store(true, SeqCst);
        let cancel = message("", "cancel");
        let (queued, ()) = tokio::join!(
            persist_queued_session_message(
                &kv, "ns", "agent", "session", "steer", "older", &cancel
            ),
            async {
                kv.queue_entered.notified().await;
                append_admitted_session_message(
                    &kv,
                    "ns",
                    "agent",
                    "session",
                    &message("later", "exact reply"),
                )
                .await
                .unwrap();
                kv.queue_release.notify_one();
            }
        );
        queued.unwrap();
        assert!(kv
            .get(&keys::session_queue_entry(
                "ns", "agent", "session", "steer", "older"
            ))
            .await
            .unwrap()
            .is_some());
        validate_user_confirmation(&kv, "ns", "agent", "session", 0, "exact reply")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn confirmation_requires_only_text_parts() {
        let kv = MockKvStore::default();
        let mut mixed = message("mixed", "exact reply");
        mixed.parts.push(data_proto::SessionMessagePart {
            part_type: data_proto::SessionMessagePartType::File as i32,
            content: "attachment".into(),
            ..Default::default()
        });
        append_admitted_session_message(&kv, "ns", "agent", "session", &mixed)
            .await
            .unwrap();
        assert!(snapshot(&kv, 0).await.is_err());
    }

    struct RacingAppendKv {
        inner: MockKvStore,
        reads: std::sync::atomic::AtomicUsize,
        barrier: tokio::sync::Barrier,
    }
    #[async_trait::async_trait]
    impl KeyValueStore for RacingAppendKv {
        async fn get(&self, key: &keys::ResourceKey) -> Result<Option<Vec<u8>>> {
            let result = self.inner.get(key).await?;
            if key.kind == "SessionMessage"
                && key.name == "same-id"
                && self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 2
            {
                self.barrier.wait().await;
            }
            Ok(result)
        }
        async fn set(&self, key: &keys::ResourceKey, value: &[u8]) -> Result<()> {
            self.inner.set(key, value).await
        }
        async fn compare_and_swap(
            &self,
            key: &keys::ResourceKey,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> Result<bool> {
            self.inner.compare_and_swap(key, expected, value).await
        }
        async fn compare_and_delete(
            &self,
            key: &keys::ResourceKey,
            expected: &[u8],
        ) -> Result<bool> {
            self.inner.compare_and_delete(key, expected).await
        }
        async fn delete(&self, key: &keys::ResourceKey) -> Result<()> {
            self.inner.delete(key).await
        }
        async fn list_keys(
            &self,
            list: &keys::ResourceList,
            options: Option<crate::control::ListOptions<'_>>,
        ) -> Result<Vec<keys::ResourceKey>> {
            self.inner.list_keys(list, options).await
        }
    }

    #[tokio::test]
    async fn concurrent_duplicate_appends_do_not_leave_a_pending_reservation() {
        let kv = RacingAppendKv {
            inner: MockKvStore::default(),
            reads: Default::default(),
            barrier: tokio::sync::Barrier::new(2),
        };
        let reply = message("same-id", "exact reply");
        let (a, b) = tokio::join!(
            append_admitted_session_message(&kv, "ns", "agent", "session", &reply),
            append_admitted_session_message(&kv, "ns", "agent", "session", &reply)
        );
        assert_eq!(
            [a, b].into_iter().filter(|r| matches!(r, Ok(true))).count(),
            1
        );
        let (_, state) = load(&kv, &state_key("ns", "agent", "session"))
            .await
            .unwrap();
        assert!(state.pending_write.is_none());
        assert_eq!(state.order, 1);
        validate_user_confirmation(&kv, "ns", "agent", "session", 0, "exact reply")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn duplicate_append_cannot_readmit_an_old_reply_after_newer_input() {
        let kv = MockKvStore::default();
        input(&kv, "same-id", "exact reply").await;
        input(&kv, "newer", "cancel").await;
        let before = kv.get(&state_key("ns", "agent", "session")).await.unwrap();
        // Exercise the reservation path after a competing client passed its
        // initial missing-key read, then paused while the winner/cancel arrived.
        assert!(begin_input(
            &kv,
            "ns",
            "agent",
            "session",
            &message("same-id", "exact reply"),
            true
        )
        .await
        .unwrap()
        .is_none());
        assert_eq!(
            kv.get(&state_key("ns", "agent", "session")).await.unwrap(),
            before
        );
        assert!(snapshot(&kv, 0).await.is_err());
    }

    #[tokio::test]
    async fn failed_edit_reconciles_durable_text_without_sticking_the_fence() {
        let kv = MockKvStore::default();
        input(&kv, "reply", "exact reply").await;
        let key = keys::session_message("ns", "agent", "session", "reply");
        let stale = kv.get(&key).await.unwrap().unwrap();
        assert!(edit_admitted_session_message(
            &kv,
            "ns",
            "agent",
            "session",
            &stale,
            &message("reply", "cancel")
        )
        .await
        .unwrap());
        assert!(!edit_admitted_session_message(
            &kv,
            "ns",
            "agent",
            "session",
            &stale,
            &message("reply", "exact reply")
        )
        .await
        .unwrap());
        assert!(load(&kv, &state_key("ns", "agent", "session"))
            .await
            .unwrap()
            .1
            .pending_write
            .is_none());
        assert!(snapshot(&kv, 0).await.is_err());
        let current = kv.get(&key).await.unwrap().unwrap();
        assert!(edit_admitted_session_message(
            &kv,
            "ns",
            "agent",
            "session",
            &current,
            &message("reply", "exact reply")
        )
        .await
        .unwrap());
        snapshot(&kv, 0).await.unwrap();
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
        assert_eq!(retry, edited);
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
    async fn promotion_retry_after_newer_input_reuses_edited_canonical_payload() {
        let kv = MockKvStore::default();
        let mut queued = message("canonical", "original");
        persist_queued_session_message(&kv, "ns", "agent", "session", "next", "entry", &queued)
            .await
            .unwrap();
        persist_promoted_session_message(&kv, "ns", "agent", "session", "entry", &mut queued)
            .await
            .unwrap();
        let key = keys::session_message("ns", "agent", "session", "canonical");
        let edited = message("canonical", "edited");
        let raw = kv.get(&key).await.unwrap().unwrap();
        assert!(
            edit_admitted_session_message(&kv, "ns", "agent", "session", &raw, &edited)
                .await
                .unwrap()
        );
        input(&kv, "newer", "cancel").await;
        let before = kv.get(&state_key("ns", "agent", "session")).await.unwrap();
        let mut retry = message("duplicate", "original");
        persist_promoted_session_message(&kv, "ns", "agent", "session", "entry", &mut retry)
            .await
            .unwrap();
        assert_eq!(retry, edited);
        assert!(kv
            .get(&keys::session_message(
                "ns",
                "agent",
                "session",
                "duplicate"
            ))
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            kv.get(&state_key("ns", "agent", "session")).await.unwrap(),
            before
        );
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
        let replaced = pending(&kv, "replaced").await;
        kv.set(&replaced, b"new request").await.unwrap();
        assert!(consume_user_confirmation(
            &kv,
            snapshot(&kv, 0).await.unwrap(),
            &replaced,
            b"pending",
            deadline()
        )
        .await
        .is_err());
        assert_eq!(kv.get(&replaced).await.unwrap().unwrap(), b"new request");
        let key = pending(&kv, "token").await;
        let first = snapshot(&kv, 0).await.unwrap();
        let second = snapshot(&kv, 0).await.unwrap();
        let (a, b) = tokio::join!(
            consume_user_confirmation(&kv, first, &key, b"pending", deadline()),
            consume_user_confirmation(&kv, second, &key, b"pending", deadline())
        );
        assert_ne!(a.is_ok(), b.is_ok());
        assert!(kv.get(&key).await.unwrap().is_none());
    }
}
