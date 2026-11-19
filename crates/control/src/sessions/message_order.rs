// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! User input, its admission order and queue/submission records commit together.
//! There is no visible payload with an unfinished admission fence.
use crate::control::{keys, KeyValueBatchOperation as Op, KeyValueStore};
use crate::talon::protocol::data_proto;
use anyhow::{anyhow, Result};
use prost::Message;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct SessionMessageAlreadyExistsError;
impl std::fmt::Display for SessionMessageAlreadyExistsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("session message ID already exists; use a fresh ID for a new turn")
    }
}
impl std::error::Error for SessionMessageAlreadyExistsError {}

#[derive(Clone, Serialize, Deserialize)]
struct AdmissionState {
    version: u32,
    order: u64,
    revision: String,
    latest_id: Option<String>,
    latest_nonce: Option<String>,
    text_digest: String,
    consumed_through_order: u64,
}
impl Default for AdmissionState {
    fn default() -> Self {
        Self {
            version: 2,
            order: 0,
            revision: String::new(),
            latest_id: None,
            latest_nonce: None,
            text_digest: String::new(),
            consumed_through_order: 0,
        }
    }
}

/// A compact live idempotency outcome, retained with the session's transcript.
/// Terminal receipts prevent a paused retry from recreating a dispatched input.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct QueueReceipt {
    pub order: u64,
    pub nonce: String,
    #[serde(default)]
    pub revision: String,
    pub canonical_id: String,
    pub submission_id: String,
    pub payload_digest: String,
    pub prepared: bool,
    pub terminal: bool,
    pub rejected: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct TurnReceipt {
    submission_id: String,
    canonical_id: String,
    payload_digest: String,
}

pub struct AdmittedTurn {
    pub submission_id: String,
    pub message: data_proto::SessionMessage,
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
fn turn_key(ns: &str, agent: &str, session: &str, message: &str) -> keys::ResourceKey {
    keys::ResourceKey::new(
        ns,
        &[("Agent", agent), ("Session", session)],
        "TurnAdmission",
        message,
    )
}
pub(crate) fn queued_input_identity_key(
    ns: &str,
    agent: &str,
    session: &str,
    client_id: &str,
) -> keys::ResourceKey {
    keys::ResourceKey::new(
        ns,
        &[("Agent", agent), ("Session", session)],
        "QueuedInputIdentity",
        client_id,
    )
}
pub(crate) async fn queued_input_entry(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    client_id: &str,
) -> Result<Option<String>> {
    kv.get(&queued_input_identity_key(ns, agent, session, client_id))
        .await?
        .map(String::from_utf8)
        .transpose()
        .map_err(Into::into)
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
fn turn_payload_digest(message: &data_proto::SessionMessage) -> String {
    let mut payload = message.clone();
    payload.id.clear(); // The client identity and server transcript key have different roles.
    payload_digest(&payload)
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
        .map(serde_json::from_slice::<AdmissionState>)
        .transpose()?
        .unwrap_or_default();
    anyhow::ensure!(
        state.version == 2,
        "unsupported admission state; clear the session before continuing"
    );
    Ok((raw, state))
}
fn encode_state(state: &mut AdmissionState) -> Result<Vec<u8>> {
    state.revision = nonce();
    Ok(serde_json::to_vec(state)?)
}
fn admit(
    state: &mut AdmissionState,
    message: &data_proto::SessionMessage,
    id: &str,
    input_nonce: &str,
) -> Result<()> {
    state.order = state
        .order
        .checked_add(1)
        .ok_or_else(|| anyhow!("admission order exhausted"))?;
    state.latest_id = Some(id.to_string());
    state.latest_nonce = Some(input_nonce.to_string());
    state.text_digest = if is_user(message) {
        text_digest(message)
    } else {
        String::new()
    };
    Ok(())
}
async fn commit(
    kv: &dyn KeyValueStore,
    operations: &[Op],
    proof_key: &keys::ResourceKey,
    proof: &[u8],
) -> Result<bool> {
    match kv.compare_and_swap_batch(operations).await {
        Ok(committed) => Ok(committed),
        Err(error) => match kv.get(proof_key).await {
            Ok(Some(actual)) if actual == proof => Ok(true),
            Ok(_) => Err(error),
            Err(read_error) => Err(error.context(format!(
                "atomic commit outcome unknown; verification failed: {read_error}"
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

pub async fn append_admitted_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    message: &data_proto::SessionMessage,
) -> Result<bool> {
    let message_key = keys::session_message(ns, agent, session, &message.id);
    let payload = message.encode_to_vec();
    if !is_user(message) {
        return kv.compare_and_swap(&message_key, None, &payload).await;
    }
    let key = state_key(ns, agent, session);
    let input_nonce = nonce();
    for _ in 0..32 {
        if kv.get(&message_key).await?.is_some() {
            return Ok(false);
        }
        let (raw, mut state) = load(kv, &key).await?;
        admit(&mut state, message, &message.id, &input_nonce)?;
        let next = encode_state(&mut state)?;
        let ops = [
            Op::new(key.clone(), raw.as_deref(), Some(&next)),
            Op::new(message_key.clone(), None, Some(&payload)),
        ];
        if commit(kv, &ops, &key, &next).await? {
            return Ok(true);
        }
    }
    Err(anyhow!("failed to atomically append session input"))
}

pub async fn edit_admitted_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    expected: &[u8],
    message: &data_proto::SessionMessage,
) -> Result<bool> {
    let message_key = keys::session_message(ns, agent, session, &message.id);
    let key = state_key(ns, agent, session);
    for _ in 0..32 {
        let Some(current) = kv.get(&message_key).await? else {
            return Ok(false);
        };
        if current != expected {
            let current = data_proto::SessionMessage::decode(current.as_slice())?;
            return Ok(payload_digest(&current) == payload_digest(message));
        }
        let (raw, mut state) = load(kv, &key).await?;
        if state.latest_id.as_deref() == Some(message.id.as_str()) {
            state.text_digest = if is_user(message) {
                text_digest(message)
            } else {
                String::new()
            };
        }
        let next = encode_state(&mut state)?;
        let payload = message.encode_to_vec();
        let ops = [
            Op::new(key.clone(), raw.as_deref(), Some(&next)),
            Op::new(message_key.clone(), Some(expected), Some(&payload)),
        ];
        if commit(kv, &ops, &key, &next).await? {
            return Ok(true);
        }
    }
    Err(anyhow!("failed to atomically edit session input"))
}
pub async fn persist_admitted_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    message: &data_proto::SessionMessage,
) -> Result<()> {
    if !append_admitted_session_message(kv, ns, agent, session, message).await? {
        return Err(SessionMessageAlreadyExistsError.into());
    }
    Ok(())
}

pub async fn lookup_admitted_turn(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    desired: &data_proto::SessionMessage,
) -> Result<Option<AdmittedTurn>> {
    let Some(raw) = kv.get(&turn_key(ns, agent, session, &desired.id)).await? else {
        return Ok(None);
    };
    let receipt: TurnReceipt = serde_json::from_slice(&raw)?;
    let Some(message) = kv
        .get(&keys::session_message(
            ns,
            agent,
            session,
            &receipt.canonical_id,
        ))
        .await?
    else {
        return Err(anyhow!("admitted turn payload is missing"));
    };
    let message = data_proto::SessionMessage::decode(message.as_slice())?;
    if receipt.payload_digest != turn_payload_digest(desired)
        && turn_payload_digest(&message) != turn_payload_digest(desired)
    {
        return Err(SessionMessageAlreadyExistsError.into());
    }
    anyhow::ensure!(
        is_user(&message),
        "admitted turn no longer has a User payload"
    );
    anyhow::ensure!(
        kv.get(&keys::session_submission(
            ns,
            agent,
            session,
            &receipt.submission_id
        ))
        .await?
        .is_some(),
        "admitted turn submission is missing"
    );
    Ok(Some(AdmittedTurn {
        submission_id: receipt.submission_id,
        message,
    }))
}

pub async fn admit_session_turn(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    message: &data_proto::SessionMessage,
    now_micros: i64,
) -> Result<AdmittedTurn> {
    anyhow::ensure!(is_user(message), "new turns require User messages");
    let client_message_key = keys::session_message(ns, agent, session, &message.id);
    let receipt_key = turn_key(ns, agent, session, &message.id);
    let key = state_key(ns, agent, session);
    let submission_id = crate::control::uuid::session_submission_id();
    let submission_key = keys::session_submission(ns, agent, session, &submission_id);
    let input_nonce = nonce();
    for _ in 0..32 {
        if let Some(existing) = lookup_admitted_turn(kv, ns, agent, session, message).await? {
            return Ok(existing);
        }
        if kv.get(&client_message_key).await?.is_some()
            || queued_input_entry(kv, ns, agent, session, &message.id)
                .await?
                .is_some()
        {
            return Err(SessionMessageAlreadyExistsError.into());
        }
        let (raw, mut state) = load(kv, &key).await?;
        // Allocate after reading the shared state; a lost CAS discards this ID.
        // The committed transcript key therefore follows server acceptance order,
        // independently of client IDs and clocks.
        let mut canonical = message.clone();
        canonical.id = nonce();
        let message_key = keys::session_message(ns, agent, session, &canonical.id);
        admit(&mut state, &canonical, &canonical.id, &input_nonce)?;
        let next = encode_state(&mut state)?;
        let receipt = serde_json::to_vec(&TurnReceipt {
            submission_id: submission_id.clone(),
            canonical_id: canonical.id.clone(),
            payload_digest: turn_payload_digest(message),
        })?;
        let submission = super::pending_submission(
            submission_id.clone(),
            session.to_string(),
            canonical.id.clone(),
            now_micros,
        )
        .encode_to_vec();
        let payload = canonical.encode_to_vec();
        let mut ops = vec![
            Op::new(key.clone(), raw.as_deref(), Some(&next)),
            Op::new(message_key.clone(), None, Some(&payload)),
            Op::new(receipt_key.clone(), None, Some(&receipt)),
            Op::new(submission_key.clone(), None, Some(&submission)),
            Op::new(
                queued_input_identity_key(ns, agent, session, &message.id),
                None,
                None,
            ),
        ];
        if client_message_key != message_key {
            ops.push(Op::new(client_message_key.clone(), None, None));
        }
        match commit(kv, &ops, &receipt_key, &receipt).await {
            Ok(true) => {
                return Ok(AdmittedTurn {
                    submission_id,
                    message: canonical,
                })
            }
            Ok(false) => continue,
            Err(error) => match lookup_admitted_turn(kv, ns, agent, session, message).await? {
                Some(existing) => return Ok(existing),
                None => return Err(error),
            },
        }
    }
    Err(anyhow!("failed to atomically admit session turn"))
}

pub(crate) async fn queue_receipt(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    entry: &str,
) -> Result<Option<QueueReceipt>> {
    kv.get(&queue_receipt_key(ns, agent, session, entry))
        .await?
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()
        .map_err(Into::into)
}
pub async fn persist_queued_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    queue: &str,
    entry: &str,
    message: &data_proto::SessionMessage,
) -> Result<()> {
    persist_queued_session_message_with_identity(
        kv, ns, agent, session, queue, entry, message, None,
    )
    .await
    .map(|_| ())
}

pub(crate) async fn persist_queued_session_message_with_identity(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    queue: &str,
    entry: &str,
    message: &data_proto::SessionMessage,
    client_id: Option<&str>,
) -> Result<String> {
    anyhow::ensure!(is_user(message), "queued inputs must be User messages");
    let candidate_entry = entry.to_string();
    let identity_key = client_id.map(|id| queued_input_identity_key(ns, agent, session, id));
    let key = state_key(ns, agent, session);
    let payload = message.encode_to_vec();
    let payload_hash = payload_digest(message);
    let mut receipt = QueueReceipt {
        order: 0,
        nonce: nonce(),
        revision: nonce(),
        canonical_id: String::new(),
        submission_id: crate::control::uuid::session_submission_id(),
        payload_digest: payload_hash.clone(),
        prepared: false,
        terminal: false,
        rejected: false,
    };
    for _ in 0..32 {
        let identity = match &identity_key {
            Some(key) => kv.get(key).await?,
            None => None,
        };
        let entry = match &identity {
            Some(bytes) => std::str::from_utf8(bytes)?,
            None => candidate_entry.as_str(),
        };
        let receipt_key = queue_receipt_key(ns, agent, session, entry);
        if let Some(existing) = queue_receipt(kv, ns, agent, session, entry).await? {
            anyhow::ensure!(
                !existing.rejected && existing.payload_digest == payload_hash,
                "queued message entry already exists with different or rejected content"
            );
            return Ok(entry.to_string());
        }
        anyhow::ensure!(identity.is_none(), "queue idempotency outcome is missing");
        if let Some(id) = client_id {
            if kv.get(&turn_key(ns, agent, session, id)).await?.is_some()
                || kv
                    .get(&keys::session_message(ns, agent, session, id))
                    .await?
                    .is_some()
            {
                return Err(SessionMessageAlreadyExistsError.into());
            }
        }
        let (raw, mut state) = load(kv, &key).await?;
        admit(
            &mut state,
            message,
            &format!("queued:{}", receipt.nonce),
            &receipt.nonce,
        )?;
        receipt.order = state.order;
        // Keep existing timestamp-based backlog ahead of new v2 entries, then
        // sort new inputs by the transaction's authoritative admission order.
        let entry = match client_id {
            Some(id) => format!("v2-{:020}-{id}", state.order),
            None => entry.to_string(),
        };
        let receipt_key = queue_receipt_key(ns, agent, session, &entry);
        let message_key = keys::session_queue_entry(ns, agent, session, queue, &entry);
        let next = encode_state(&mut state)?;
        let encoded = serde_json::to_vec(&receipt)?;
        let mut ops = vec![
            Op::new(key.clone(), raw.as_deref(), Some(&next)),
            Op::new(receipt_key.clone(), None, Some(&encoded)),
            Op::new(message_key.clone(), None, Some(&payload)),
        ];
        if let Some(key) = &identity_key {
            ops.push(Op::new(key.clone(), None, Some(entry.as_bytes())));
            let id = client_id.expect("identity key requires a client ID");
            ops.push(Op::new(turn_key(ns, agent, session, id), None, None));
            ops.push(Op::new(
                keys::session_message(ns, agent, session, id),
                None,
                None,
            ));
        }
        if commit(kv, &ops, &receipt_key, &encoded).await? {
            return Ok(entry.to_string());
        }
    }
    Err(anyhow!("failed to atomically enqueue session input"))
}

pub(crate) async fn load_promoted_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    entry: &str,
    message: &mut data_proto::SessionMessage,
) -> Result<()> {
    let id = queue_receipt(kv, ns, agent, session, entry)
        .await?
        .map(|receipt| receipt.canonical_id)
        .unwrap_or_else(|| message.id.clone());
    if !id.is_empty() {
        if let Some(raw) = kv
            .get(&keys::session_message(ns, agent, session, &id))
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
    queue: &str,
    entry: &str,
    output: &mut data_proto::SessionMessage,
) -> Result<bool> {
    let source_key = keys::session_queue_entry(ns, agent, session, queue, entry);
    let receipt_key = queue_receipt_key(ns, agent, session, entry);
    for _ in 0..32 {
        let Some(source) = kv.get(&source_key).await? else {
            return Ok(false);
        };
        let mut message = data_proto::SessionMessage::decode(source.as_slice())?;
        let raw = kv.get(&receipt_key).await?;
        let mut receipt = raw
            .as_deref()
            .map(serde_json::from_slice::<QueueReceipt>)
            .transpose()?
            .unwrap_or_else(|| QueueReceipt {
                order: 0,
                nonce: nonce(),
                revision: nonce(),
                canonical_id: if message.id.is_empty() {
                    nonce()
                } else {
                    message.id.clone()
                },
                submission_id: crate::control::uuid::session_submission_id(),
                payload_digest: payload_digest(&message),
                prepared: !message.id.is_empty(),
                terminal: false,
                rejected: false,
            });
        if receipt.terminal {
            return Ok(false);
        }
        let mut binding = None;
        if receipt.canonical_id.is_empty() {
            let admission_key = state_key(ns, agent, session);
            let (state_raw, mut state) = load(kv, &admission_key).await?;
            receipt.canonical_id = nonce();
            if state.latest_nonce.as_deref() == Some(receipt.nonce.as_str()) {
                state.latest_id = Some(receipt.canonical_id.clone());
            }
            let next = encode_state(&mut state)?;
            binding = Some(Op::new(admission_key, state_raw.as_deref(), Some(&next)));
        }
        let canonical_key = keys::session_message(ns, agent, session, &receipt.canonical_id);
        let current = kv.get(&canonical_key).await?;
        if let Some(value) = &current {
            message = data_proto::SessionMessage::decode(value.as_slice())?;
        } else {
            anyhow::ensure!(
                !receipt.prepared,
                "prepared queue input lost its canonical payload"
            );
            message.id = receipt.canonical_id.clone();
            if message.created_at == 0 {
                message.created_at = chrono::Utc::now().timestamp_micros();
            }
            for (index, part) in message.parts.iter_mut().enumerate() {
                if part.id.is_empty() {
                    part.id = format!("{index:06}");
                }
                if part.created_at == 0 {
                    part.created_at = message.created_at;
                }
            }
        }
        receipt.prepared = true;
        receipt.revision = nonce();
        let encoded = serde_json::to_vec(&receipt)?;
        let payload = message.encode_to_vec();
        let canonical_value = current.as_deref().unwrap_or(&payload);
        let mut ops = vec![
            Op::new(receipt_key.clone(), raw.as_deref(), Some(&encoded)),
            Op::new(
                canonical_key.clone(),
                current.as_deref(),
                Some(canonical_value),
            ),
            Op::new(source_key.clone(), Some(&source), Some(&payload)),
        ];
        if let Some(binding) = binding {
            ops.push(binding);
        }
        match commit(kv, &ops, &receipt_key, &encoded).await {
            Ok(true) => {
                *output = message;
                return Ok(true);
            }
            Ok(false) => continue,
            Err(error) => {
                if let Some(actual) = queue_receipt(kv, ns, agent, session, entry).await? {
                    if actual.terminal {
                        return Ok(false);
                    }
                    if actual.prepared && actual.canonical_id == receipt.canonical_id {
                        if let Some(value) = kv.get(&canonical_key).await? {
                            *output = data_proto::SessionMessage::decode(value.as_slice())?;
                            return Ok(true);
                        }
                    }
                }
                return Err(error);
            }
        }
    }
    Err(anyhow!("failed to atomically promote queued input"))
}

pub(crate) async fn terminal_queue_entry(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    entry: &str,
    rejected: bool,
) -> Result<()> {
    let key = queue_receipt_key(ns, agent, session, entry);
    for _ in 0..32 {
        let raw = kv.get(&key).await?;
        let mut sources = Vec::new();
        for queue in ["steer", "next", "a2a"] {
            let source = keys::session_queue_entry(ns, agent, session, queue, entry);
            let bytes = kv.get(&source).await?;
            sources.push((source, bytes));
        }
        if raw.is_none() && sources.iter().all(|(_, raw)| raw.is_none()) {
            return Ok(());
        }
        let legacy_message = sources
            .iter()
            .filter_map(|(_, raw)| raw.as_deref())
            .find_map(|raw| data_proto::SessionMessage::decode(raw).ok());
        let mut receipt = raw
            .as_deref()
            .map(serde_json::from_slice::<QueueReceipt>)
            .transpose()?
            .unwrap_or_else(|| QueueReceipt {
                order: 0,
                nonce: nonce(),
                revision: nonce(),
                canonical_id: legacy_message
                    .as_ref()
                    .map(|message| message.id.clone())
                    .unwrap_or_default(),
                submission_id: crate::control::uuid::session_submission_id(),
                payload_digest: String::new(),
                prepared: true,
                terminal: false,
                rejected,
            });
        receipt.terminal = true;
        receipt.revision = nonce();
        receipt.rejected |= rejected;
        let encoded = serde_json::to_vec(&receipt)?;
        let mut ops = vec![Op::new(key.clone(), raw.as_deref(), Some(&encoded))];
        for (source, bytes) in sources {
            ops.push(Op::new(source, bytes.as_deref(), None));
        }
        if commit(kv, &ops, &key, &encoded).await? {
            return Ok(());
        }
    }
    Err(anyhow!("failed to atomically finish queued input"))
}

pub(crate) async fn move_steer_entry_to_next(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    entry: &str,
) -> Result<()> {
    let source = keys::session_queue_entry(ns, agent, session, "steer", entry);
    let target = keys::session_queue_entry(ns, agent, session, "next", entry);
    let receipt_key = queue_receipt_key(ns, agent, session, entry);
    for _ in 0..32 {
        let Some(payload) = kv.get(&source).await? else {
            return Ok(());
        };
        let receipt = kv.get(&receipt_key).await?;
        if receipt
            .as_deref()
            .map(serde_json::from_slice::<QueueReceipt>)
            .transpose()?
            .is_some_and(|receipt| receipt.terminal)
        {
            return terminal_queue_entry(kv, ns, agent, session, entry, false).await;
        }
        let existing = kv.get(&target).await?;
        let next = existing.as_deref().unwrap_or(&payload);
        let ops = [
            Op::new(source.clone(), Some(&payload), None),
            Op::new(target.clone(), existing.as_deref(), Some(next)),
            Op::new(receipt_key.clone(), receipt.as_deref(), receipt.as_deref()),
        ];
        match kv.compare_and_swap_batch(&ops).await {
            Ok(true) => return Ok(()),
            Ok(false) => continue,
            Err(error) => {
                if kv.get(&source).await?.is_none() {
                    return Ok(());
                }
                return Err(error);
            }
        }
    }
    Err(anyhow!("failed to atomically reclassify queued input"))
}

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
        || state.text_digest != digest(required_reply)
    {
        return Err(anyhow!("delete requires a separate user message exactly: {}; latest input may be different or already consumed", required_reply));
    }
    Ok(UserConfirmationSnapshot {
        key,
        bytes: raw.ok_or_else(|| anyhow!("user confirmation state missing"))?,
        requested_after: after,
    })
}
pub async fn consume_user_confirmation(
    kv: &dyn KeyValueStore,
    snapshot: UserConfirmationSnapshot,
    confirmation_key: &keys::ResourceKey,
    pending: &[u8],
    expires_at: i64,
) -> Result<()> {
    let expiry = || -> Result<()> {
        anyhow::ensure!(
            chrono::Utc::now().timestamp_micros() <= expires_at,
            "delete confirmation expired; start a new delete request"
        );
        Ok(())
    };
    expiry()?;
    let approved: AdmissionState = serde_json::from_slice(&snapshot.bytes)?;
    anyhow::ensure!(
        approved.order > approved.consumed_through_order
            && approved.consumed_through_order <= snapshot.requested_after,
        "user approval has already been consumed"
    );
    let mut proof_key = confirmation_key.clone();
    proof_key.kind = "FileConfirmationConsumption".into();
    proof_key.name = format!("{:x}", Sha256::digest(pending));
    let proof = nonce().into_bytes();
    let mut consumed = false;
    for _ in 0..32 {
        expiry()?;
        let (raw, mut state) = load(kv, &snapshot.key).await?;
        // Promotion may bind a canonical ID, and unrelated transcript edits may
        // change the revision. Only the same unspent User input can authorize.
        anyhow::ensure!(
            state.order == approved.order
                && state.latest_nonce == approved.latest_nonce
                && state.text_digest == approved.text_digest
                && state.consumed_through_order == approved.consumed_through_order,
            "confirmation missing, used, or latest reply changed; start a new delete request"
        );
        state.consumed_through_order = state.order;
        let next = encode_state(&mut state)?;
        let ops = [
            Op::new(snapshot.key.clone(), raw.as_deref(), Some(&next)),
            Op::new(confirmation_key.clone(), Some(pending), None),
            Op::new(proof_key.clone(), None, Some(&proof)),
        ];
        expiry()?; // Check again immediately before the mutation, after storage reads.
        if commit(kv, &ops, &proof_key, &proof).await? {
            consumed = true;
            break;
        }
    }
    anyhow::ensure!(
        consumed,
        "confirmation changed while consuming; start a new delete request"
    );
    match tokio::time::timeout(
        std::time::Duration::from_secs(1),
        kv.compare_and_delete(&proof_key, &proof),
    )
    .await
    {
        Ok(Ok(_)) => {}
        result => {
            tracing::warn!(?result, key = %proof_key, "confirmation consumed; temporary proof cleanup deferred to session clear")
        }
    }
    // The deadline gates starting a consumption attempt. A committed approval
    // stays authoritative even if the response or cleanup crosses that deadline.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ListOptions;
    use crate::test_support::MockKvStore;
    use std::sync::atomic::{AtomicI32, Ordering};

    #[derive(Default)]
    struct FaultKv {
        inner: MockKvStore,
        fault: AtomicI32,
    }
    #[async_trait::async_trait]
    impl KeyValueStore for FaultKv {
        async fn get(&self, key: &keys::ResourceKey) -> Result<Option<Vec<u8>>> {
            tokio::task::yield_now().await;
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
            self.inner.compare_and_swap(key, expected, value).await
        }
        async fn compare_and_delete(
            &self,
            key: &keys::ResourceKey,
            expected: &[u8],
        ) -> Result<bool> {
            if key.kind == "FileConfirmationConsumption" && self.fault.load(Ordering::SeqCst) == 4 {
                return futures::future::pending().await;
            }
            self.inner.compare_and_delete(key, expected).await
        }
        async fn compare_and_swap_batch(&self, operations: &[Op]) -> Result<bool> {
            let fault = self.fault.swap(0, Ordering::SeqCst);
            if fault == 1 {
                return Err(anyhow!("batch unavailable before commit"));
            }
            let committed = self.inner.compare_and_swap_batch(operations).await?;
            if fault == 3 && committed {
                append_admitted_session_message(
                    &self.inner,
                    "ns",
                    "agent",
                    "session",
                    &message("later", "later input"),
                )
                .await?;
            }
            if fault == 2 || fault == 3 {
                return Err(anyhow!("response lost after commit"));
            }
            if fault == 4 {
                self.fault.store(4, Ordering::SeqCst);
            }
            Ok(committed)
        }
        async fn delete(&self, key: &keys::ResourceKey) -> Result<()> {
            self.inner.delete(key).await
        }
        async fn list_keys(
            &self,
            list: &keys::ResourceList,
            options: Option<ListOptions<'_>>,
        ) -> Result<Vec<keys::ResourceKey>> {
            self.inner.list_keys(list, options).await
        }
    }
    fn message(id: &str, text: &str) -> data_proto::SessionMessage {
        data_proto::SessionMessage {
            id: id.into(),
            role: data_proto::MessageRole::RoleUser as i32,
            parts: vec![data_proto::SessionMessagePart {
                part_type: data_proto::SessionMessagePartType::Text as i32,
                content: text.into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }
    #[tokio::test]
    async fn failed_admission_is_invisible_and_exact_retry_has_one_submission() {
        let kv = FaultKv::default();
        let input = message("client-id", "hello");
        kv.fault.store(1, Ordering::SeqCst);
        assert!(admit_session_turn(&kv, "ns", "agent", "session", &input, 1)
            .await
            .is_err());
        assert_eq!(
            current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap(),
            0
        );
        assert!(kv
            .get(&keys::session_message(
                "ns",
                "agent",
                "session",
                "client-id"
            ))
            .await
            .unwrap()
            .is_none());
        kv.fault.store(2, Ordering::SeqCst);
        let first = admit_session_turn(&kv, "ns", "agent", "session", &input, 2)
            .await
            .unwrap();
        let second = admit_session_turn(&kv, "ns", "agent", "session", &input, 3)
            .await
            .unwrap();
        assert_eq!(first.submission_id, second.submission_id);
        assert_eq!(
            current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap(),
            1
        );
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
        assert!(admit_session_turn(
            &kv,
            "ns",
            "agent",
            "session",
            &message("client-id", "different"),
            4
        )
        .await
        .is_err());
    }
    #[tokio::test]
    async fn client_ids_and_timestamps_do_not_order_the_canonical_transcript() {
        let kv = FaultKv::default();
        let mut previous = String::new();
        for (id, text, timestamp) in [
            ("ffffffff-ffff-4fff-8fff-ffffffffffff", "first", i64::MAX),
            ("turn-10", "second", 1),
            ("00000000-0000-7000-8000-000000000000", "third", 2),
        ] {
            let mut input = message(id, text);
            input.created_at = timestamp;
            let admitted = admit_session_turn(&kv, "ns", "agent", "session", &input, 1)
                .await
                .unwrap();
            assert_ne!(admitted.message.id, id);
            assert!(admitted.message.id > previous);
            previous = admitted.message.id;
            let retried = admit_session_turn(&kv, "ns", "agent", "session", &input, 2)
                .await
                .unwrap();
            assert_eq!(retried.submission_id, admitted.submission_id);
            assert_eq!(retried.message.id, previous);
        }
        let entries = kv
            .list_entries(
                &keys::session_message_prefix("ns", "agent", "session"),
                None,
            )
            .await
            .unwrap();
        let texts = entries
            .iter()
            .map(|(_, raw)| {
                data_proto::SessionMessage::decode(raw.as_slice())
                    .unwrap()
                    .parts[0]
                    .content
                    .clone()
            })
            .collect::<Vec<_>>();
        assert_eq!(texts, ["first", "second", "third"]);
    }
    #[tokio::test]
    async fn concurrent_queue_admissions_bind_transcript_ids_during_ordered_promotion() {
        let kv = FaultKv::default();
        let input = message("", "queued");
        let (left, right) = tokio::join!(
            persist_queued_session_message_with_identity(
                &kv,
                "ns",
                "agent",
                "session",
                "steer",
                "z-start",
                &input,
                Some("z-client")
            ),
            persist_queued_session_message_with_identity(
                &kv,
                "ns",
                "agent",
                "session",
                "steer",
                "a-start",
                &input,
                Some("a-client")
            )
        );
        let left = left.unwrap();
        let right = right.unwrap();
        for entry in [&left, &right] {
            assert!(queue_receipt(&kv, "ns", "agent", "session", entry)
                .await
                .unwrap()
                .unwrap()
                .canonical_id
                .is_empty());
        }
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
        assert_eq!(batch.len(), 2);
        assert!(batch[0].message_id < batch[1].message_id);
        let first = queue_receipt(&kv, "ns", "agent", "session", &batch[0].entry_id)
            .await
            .unwrap()
            .unwrap();
        let second = queue_receipt(&kv, "ns", "agent", "session", &batch[1].entry_id)
            .await
            .unwrap()
            .unwrap();
        assert!(first.order < second.order);
        assert_eq!(
            current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap(),
            2
        );
    }
    #[tokio::test]
    async fn consumed_confirmation_reconciles_lost_response_after_later_admission() {
        let kv = FaultKv::default();
        append_admitted_session_message(
            &kv,
            "ns",
            "agent",
            "session",
            &message("reply", "approve"),
        )
        .await
        .unwrap();
        let snapshot = validate_user_confirmation(&kv, "ns", "agent", "session", 0, "approve")
            .await
            .unwrap();
        let token = keys::ResourceKey::new(
            "ns",
            &[("Agent", "agent"), ("Session", "session")],
            "Confirmation",
            "pending",
        );
        kv.set(&token, b"pending").await.unwrap();
        kv.fault.store(3, Ordering::SeqCst);
        consume_user_confirmation(&kv, snapshot, &token, b"pending", i64::MAX)
            .await
            .unwrap();
        assert!(kv.get(&token).await.unwrap().is_none());
        assert_eq!(
            current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap(),
            2
        );
        assert!(
            validate_user_confirmation(&kv, "ns", "agent", "session", 0, "approve")
                .await
                .is_err()
        );
        let mut proof = token.clone();
        proof.kind = "FileConfirmationConsumption".into();
        proof.name = format!("{:x}", Sha256::digest(b"pending"));
        assert!(
            kv.get(&proof).await.unwrap().is_none(),
            "confirmed operation cleans its temporary proof"
        );
    }
    #[tokio::test]
    async fn canonical_binding_preserves_the_same_unspent_queued_approval() {
        let kv = FaultKv::default();
        let input = message("", "approve");
        let entry = persist_queued_session_message_with_identity(
            &kv,
            "ns",
            "agent",
            "session",
            "steer",
            "entry",
            &input,
            Some("client"),
        )
        .await
        .unwrap();
        let snapshot = validate_user_confirmation(&kv, "ns", "agent", "session", 0, "approve")
            .await
            .unwrap();
        let mut promoted = input;
        assert!(persist_promoted_session_message(
            &kv,
            "ns",
            "agent",
            "session",
            "steer",
            &entry,
            &mut promoted
        )
        .await
        .unwrap());
        let token = keys::ResourceKey::new("ns", &[], "Confirmation", "pending");
        kv.set(&token, b"pending").await.unwrap();
        consume_user_confirmation(&kv, snapshot, &token, b"pending", i64::MAX)
            .await
            .unwrap();
        assert!(kv.get(&token).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn expiry_during_cleanup_cannot_revoke_committed_approval() {
        let kv = FaultKv::default();
        append_admitted_session_message(
            &kv,
            "ns",
            "agent",
            "session",
            &message("reply", "approve"),
        )
        .await
        .unwrap();
        let snapshot = validate_user_confirmation(&kv, "ns", "agent", "session", 0, "approve")
            .await
            .unwrap();
        let token = keys::ResourceKey::new("ns", &[], "Confirmation", "pending");
        kv.set(&token, b"pending").await.unwrap();
        assert!(consume_user_confirmation(
            &kv,
            snapshot,
            &token,
            b"pending",
            chrono::Utc::now().timestamp_micros() - 1
        )
        .await
        .is_err());
        assert!(
            kv.get(&token).await.unwrap().is_some(),
            "expired approval cannot be consumed"
        );
        let snapshot = validate_user_confirmation(&kv, "ns", "agent", "session", 0, "approve")
            .await
            .unwrap();
        let deadline = chrono::Utc::now().timestamp_micros() + 500_000;
        kv.fault.store(4, Ordering::SeqCst); // Cleanup stalls for its one-second deadline.
        consume_user_confirmation(&kv, snapshot, &token, b"pending", deadline)
            .await
            .unwrap();
        assert!(chrono::Utc::now().timestamp_micros() > deadline);
        assert!(kv.get(&token).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn proof_cleanup_cannot_stall_authoritative_consumption() {
        let kv = FaultKv::default();
        append_admitted_session_message(
            &kv,
            "ns",
            "agent",
            "session",
            &message("reply", "approve"),
        )
        .await
        .unwrap();
        let snapshot = validate_user_confirmation(&kv, "ns", "agent", "session", 0, "approve")
            .await
            .unwrap();
        let token = keys::ResourceKey::new("ns", &[], "Confirmation", "pending");
        kv.set(&token, b"pending").await.unwrap();
        kv.fault.store(4, Ordering::SeqCst);
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            consume_user_confirmation(&kv, snapshot, &token, b"pending", i64::MAX),
        )
        .await
        .expect("cleanup is bounded")
        .unwrap();
        assert!(kv.get(&token).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn concurrent_identical_edits_cannot_restore_old_approval_on_failure() {
        let kv = FaultKv::default();
        let initial = message("reply", "approve");
        append_admitted_session_message(&kv, "ns", "agent", "session", &initial)
            .await
            .unwrap();
        let old_snapshot = validate_user_confirmation(&kv, "ns", "agent", "session", 0, "approve")
            .await
            .unwrap();
        let replacement = message("reply", "cancel");
        let raw = initial.encode_to_vec();
        kv.fault.store(1, Ordering::SeqCst);
        let (left, right) = tokio::join!(
            edit_admitted_session_message(&kv, "ns", "agent", "session", &raw, &replacement),
            edit_admitted_session_message(&kv, "ns", "agent", "session", &raw, &replacement)
        );
        assert!(left.is_err() || right.is_err());
        assert!(left.unwrap_or(false) || right.unwrap_or(false));
        assert!(
            validate_user_confirmation(&kv, "ns", "agent", "session", 0, "approve")
                .await
                .is_err()
        );
        let token = keys::ResourceKey::new("ns", &[], "Confirmation", "token");
        kv.set(&token, b"pending").await.unwrap();
        assert!(
            consume_user_confirmation(&kv, old_snapshot, &token, b"pending", i64::MAX)
                .await
                .is_err()
        );
        assert_eq!(
            kv.get(&token).await.unwrap().as_deref(),
            Some(&b"pending"[..])
        );
    }
    #[tokio::test]
    async fn direct_and_queued_admission_cannot_accept_the_same_client_id_twice() {
        let kv = FaultKv::default();
        let direct = message("client-id", "hello");
        let mut queued = direct.clone();
        queued.id.clear();
        let (direct, queued) = tokio::join!(
            admit_session_turn(&kv, "ns", "agent", "session", &direct, 1),
            persist_queued_session_message_with_identity(
                &kv,
                "ns",
                "agent",
                "session",
                "next",
                "entry",
                &queued,
                Some("client-id")
            )
        );
        assert_ne!(direct.is_ok(), queued.is_ok());
        assert_eq!(
            current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn concurrent_duplicate_enqueue_admits_exactly_once() {
        let kv = FaultKv::default();
        let input = message("", "approve");
        let (left, right) = tokio::join!(
            persist_queued_session_message(&kv, "ns", "agent", "session", "steer", "entry", &input),
            persist_queued_session_message(&kv, "ns", "agent", "session", "steer", "entry", &input)
        );
        left.unwrap();
        right.unwrap();
        assert_eq!(
            current_message_admission_order(&kv, "ns", "agent", "session")
                .await
                .unwrap(),
            1
        );
        validate_user_confirmation(&kv, "ns", "agent", "session", 0, "approve")
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn moving_and_finishing_queue_input_cannot_resurrect_it() {
        for _ in 0..16 {
            let kv = FaultKv::default();
            let input = message("", "hello");
            persist_queued_session_message(&kv, "ns", "agent", "session", "steer", "entry", &input)
                .await
                .unwrap();
            let (moved, finished) = tokio::join!(
                move_steer_entry_to_next(&kv, "ns", "agent", "session", "entry"),
                terminal_queue_entry(&kv, "ns", "agent", "session", "entry", false)
            );
            moved.unwrap();
            finished.unwrap();
            // A paused producer retry sees the terminal outcome and cannot create a fresh canonical turn.
            persist_queued_session_message(&kv, "ns", "agent", "session", "steer", "entry", &input)
                .await
                .unwrap();
            for queue in ["steer", "next", "a2a"] {
                assert!(kv
                    .get(&keys::session_queue_entry(
                        "ns", "agent", "session", queue, "entry"
                    ))
                    .await
                    .unwrap()
                    .is_none());
            }
            assert!(
                queue_receipt(&kv, "ns", "agent", "session", "entry")
                    .await
                    .unwrap()
                    .unwrap()
                    .terminal
            );
            assert_eq!(
                current_message_admission_order(&kv, "ns", "agent", "session")
                    .await
                    .unwrap(),
                1
            );
        }
    }
    #[tokio::test]
    async fn confirmation_and_consumption_commit_together_and_cannot_be_replayed() {
        let kv = FaultKv::default();
        append_admitted_session_message(
            &kv,
            "ns",
            "agent",
            "session",
            &message("reply", "approve"),
        )
        .await
        .unwrap();
        let token = keys::ResourceKey::new("ns", &[], "Confirmation", "token");
        kv.set(&token, b"pending").await.unwrap();
        let snapshot = validate_user_confirmation(&kv, "ns", "agent", "session", 0, "approve")
            .await
            .unwrap();
        kv.fault.store(1, Ordering::SeqCst);
        assert!(
            consume_user_confirmation(&kv, snapshot, &token, b"pending", i64::MAX)
                .await
                .is_err()
        );
        let snapshot = validate_user_confirmation(&kv, "ns", "agent", "session", 0, "approve")
            .await
            .unwrap();
        kv.fault.store(2, Ordering::SeqCst);
        consume_user_confirmation(&kv, snapshot, &token, b"pending", i64::MAX)
            .await
            .unwrap();
        assert!(kv.get(&token).await.unwrap().is_none());
        assert!(
            validate_user_confirmation(&kv, "ns", "agent", "session", 0, "approve")
                .await
                .is_err()
        );
    }
    #[test]
    fn idempotency_digest_ignores_map_iteration_order_and_timestamps() {
        let mut left = message("m", "hello");
        left.labels.insert("a".into(), "1".into());
        left.labels.insert("b".into(), "2".into());
        let mut right = left.clone();
        right.labels.clear();
        right.labels.insert("b".into(), "2".into());
        right.labels.insert("a".into(), "1".into());
        right.created_at = 123;
        right.parts[0].created_at = 123;
        assert_eq!(payload_digest(&left), payload_digest(&right));
        right.parts[0].content = "changed".into();
        assert_ne!(payload_digest(&left), payload_digest(&right));
    }
}
