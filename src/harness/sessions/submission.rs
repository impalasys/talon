// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use prost::Message;

use super::{SessionJournalEntry, SessionSubmission};
use crate::control::{keys, KeyValueStore};
use crate::gateway::rpc::data_proto::{SessionExecutionPhase, SessionSubmissionStatus};

#[derive(Debug, Clone, PartialEq)]
pub enum ClaimOutcome {
    Claimed(SessionSubmission),
    AlreadyTerminal(SessionSubmission),
    Busy(SessionSubmission),
}

pub fn pending_submission(
    submission_id: impl Into<String>,
    session_id: impl Into<String>,
    user_message_id: impl Into<String>,
    now_micros: i64,
) -> SessionSubmission {
    SessionSubmission {
        submission_id: submission_id.into(),
        session_id: session_id.into(),
        user_message_id: user_message_id.into(),
        status: SessionSubmissionStatus::Pending as i32,
        attempt_id: String::new(),
        attempt_count: 0,
        claim_expires_at: None,
        created_at: now_micros,
        updated_at: now_micros,
        completed_at: None,
        committed_message_id: None,
        current_phase: SessionExecutionPhase::Unspecified as i32,
        current_journal_entry_id: None,
    }
}

pub fn submission_is_terminal(submission: &SessionSubmission) -> bool {
    submission.status == SessionSubmissionStatus::Committed as i32
        || submission.status == SessionSubmissionStatus::Failed as i32
        || submission.status == SessionSubmissionStatus::Interrupted as i32
}

pub async fn create_submission_if_absent(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission: &SessionSubmission,
) -> Result<()> {
    let key = keys::session_submission(ns, agent, session_id, &submission.submission_id);
    let bytes = submission.encode_to_vec();
    if !kv.compare_and_swap(&key, None, &bytes).await? {
        tracing::debug!(
            namespace = %ns,
            agent = %agent,
            session = %session_id,
            submission = %submission.submission_id,
            "Session submission already exists"
        );
    }
    Ok(())
}

pub async fn claim_submission(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    user_message_id: &str,
    now_micros: i64,
    claim_ttl_micros: i64,
) -> Result<ClaimOutcome> {
    let key = keys::session_submission(ns, agent, session_id, submission_id);
    for _ in 0..8 {
        let current = kv.get(&key).await?;
        let mut submission = match current.as_ref() {
            Some(bytes) => SessionSubmission::decode(bytes.as_slice())?,
            None => pending_submission(submission_id, session_id, user_message_id, now_micros),
        };

        if submission_is_terminal(&submission) {
            return Ok(ClaimOutcome::AlreadyTerminal(submission));
        }

        if submission.status == SessionSubmissionStatus::Claimed as i32
            && submission
                .claim_expires_at
                .is_some_and(|expires_at| expires_at > now_micros)
        {
            return Ok(ClaimOutcome::Busy(submission));
        }

        submission.status = SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = uuid::Uuid::now_v7().to_string();
        submission.attempt_count = submission.attempt_count.saturating_add(1);
        submission.claim_expires_at = Some(now_micros.saturating_add(claim_ttl_micros));
        submission.updated_at = now_micros;
        let updated = submission.encode_to_vec();
        if kv
            .compare_and_swap(&key, current.as_deref(), &updated)
            .await?
        {
            return Ok(ClaimOutcome::Claimed(submission));
        }
    }
    Err(anyhow!("failed to atomically claim session submission"))
}

pub(super) async fn ensure_submission_attempt_current(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    attempt_id: &str,
) -> Result<()> {
    // Journal appends are fenced by the active claim attempt. This prevents a
    // worker whose lease expired from appending new recovery state after another
    // worker has reclaimed the same submission.
    let submission_key = keys::session_submission(ns, agent, session_id, submission_id);
    let Some(current) = kv.get(&submission_key).await? else {
        return Err(anyhow!("session submission not found"));
    };
    let submission = SessionSubmission::decode(current.as_slice())?;
    if submission_is_terminal(&submission) {
        return Err(anyhow!("session submission already terminal"));
    }
    if submission.attempt_id != attempt_id {
        return Err(anyhow!(
            "stale session submission attempt: current={}, entry={}",
            submission.attempt_id,
            attempt_id
        ));
    }
    Ok(())
}

pub(super) async fn update_submission_from_entry(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    entry: &SessionJournalEntry,
    terminal_status: Option<i32>,
    committed_message_id: Option<&str>,
    now_micros: i64,
) -> Result<()> {
    // Journal entries are the authoritative recovery log; this CAS-updates the
    // submission's fast pointer after an entry is durable. The guards below keep
    // stale attempts from moving the pointer and keep older entries from moving
    // it backward if overlapping workers race.
    let submission_key = keys::session_submission(ns, agent, session_id, submission_id);
    for _ in 0..8 {
        let Some(current) = kv.get(&submission_key).await? else {
            return Err(anyhow!("session submission not found"));
        };
        let mut submission = SessionSubmission::decode(current.as_slice())?;
        let terminal_update = terminal_status.is_some();
        if !terminal_update && submission.attempt_id != entry.attempt_id {
            return Err(anyhow!(
                "stale session submission attempt: current={}, entry={}",
                submission.attempt_id,
                entry.attempt_id
            ));
        }
        if submission_is_terminal(&submission) {
            if terminal_update {
                return Ok(());
            }
            return Err(anyhow!("session submission already terminal"));
        }
        if let Some(current_journal_entry_id) = submission.current_journal_entry_id.as_deref() {
            let current_order = journal_entry_order(current_journal_entry_id);
            let entry_order = journal_entry_order(&entry.journal_entry_id);
            if current_order > entry_order
                && !(terminal_update && entry.phase == SessionExecutionPhase::Committed as i32)
            {
                return Ok(());
            }
        }
        submission.current_phase = entry.phase;
        submission.current_journal_entry_id = Some(entry.journal_entry_id.clone());
        submission.updated_at = now_micros;
        if let Some(status) = terminal_status {
            submission.status = status;
            submission.claim_expires_at = None;
            submission.completed_at = Some(now_micros);
            submission.committed_message_id = committed_message_id.map(str::to_string);
        }
        let updated = submission.encode_to_vec();
        if kv
            .compare_and_swap(&submission_key, Some(current.as_slice()), &updated)
            .await?
        {
            return Ok(());
        }
    }
    Err(anyhow!(
        "failed to atomically update session submission pointer"
    ))
}

fn journal_entry_order(journal_entry_id: &str) -> u64 {
    journal_entry_id.parse::<u64>().unwrap_or(0)
}
