// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use prost::Message;

use super::submission::{ensure_submission_attempt_current, update_submission_from_entry};
use super::SessionJournalEntry;
use crate::control::cas::CasStore;
use crate::control::{keys, KeyValueStore};
use crate::gateway::rpc::data_proto::{
    session_journal_entry_payload, SessionExecutionPhase, SessionJournalEntryPayload,
    SessionJournalEntryPayloadCommit, SessionJournalEntryPayloadLlmResponse,
    SessionJournalEntryPayloadToolResult,
};
use crate::harness::llm::ChatResponse;

const TOOL_RESULT_OBJECT_THRESHOLD_BYTES: usize = 2 * 1024;

pub async fn append_llm_response(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    attempt_id: &str,
    response: &ChatResponse,
    now_micros: i64,
) -> Result<SessionJournalEntry> {
    append_journal_entry(
        kv,
        ns,
        agent,
        session_id,
        submission_id,
        attempt_id,
        SessionExecutionPhase::LlmResponse as i32,
        Some(SessionJournalEntryPayload {
            payload: Some(session_journal_entry_payload::Payload::LlmResponse(
                SessionJournalEntryPayloadLlmResponse {
                    response: Some(response.clone()),
                },
            )),
        }),
        None,
        now_micros,
    )
    .await
}

pub async fn append_tool_result(
    kv: &dyn KeyValueStore,
    cas: &CasStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    message_id: &str,
    part_id: &str,
    submission_id: &str,
    attempt_id: &str,
    tool_call_id: &str,
    name: &str,
    result: &str,
    now_micros: i64,
) -> Result<SessionJournalEntry> {
    ensure_submission_attempt_current(kv, ns, agent, session_id, submission_id, attempt_id).await?;
    let object = cas
        .put_tool_result_if_raw_at_least(
            ns,
            agent,
            session_id,
            message_id,
            part_id,
            tool_call_id,
            name,
            result.as_bytes(),
            TOOL_RESULT_OBJECT_THRESHOLD_BYTES,
        )
        .await?;
    ensure_submission_attempt_current(kv, ns, agent, session_id, submission_id, attempt_id).await?;
    let output = if object.is_some() {
        String::new()
    } else {
        result.to_string()
    };
    append_journal_entry(
        kv,
        ns,
        agent,
        session_id,
        submission_id,
        attempt_id,
        SessionExecutionPhase::ToolResult as i32,
        Some(SessionJournalEntryPayload {
            payload: Some(session_journal_entry_payload::Payload::ToolResult(
                SessionJournalEntryPayloadToolResult {
                    tool_call_id: tool_call_id.to_string(),
                    name: name.to_string(),
                    output,
                    object,
                },
            )),
        }),
        None,
        now_micros,
    )
    .await
}

pub async fn mark_terminal(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    attempt_id: &str,
    status: i32,
    committed_message_id: &str,
    now_micros: i64,
) -> Result<SessionJournalEntry> {
    if let Some(existing) = committed_journal_entry(
        kv,
        ns,
        agent,
        session_id,
        submission_id,
        committed_message_id,
    )
    .await?
    {
        if existing.attempt_id == attempt_id {
            update_submission_from_entry(
                kv,
                ns,
                agent,
                session_id,
                submission_id,
                &existing,
                Some(status),
                Some(committed_message_id),
                now_micros,
            )
            .await?;
            return Ok(existing);
        }
    }

    let entry = append_journal_entry_raw(
        kv,
        ns,
        agent,
        session_id,
        submission_id,
        attempt_id,
        SessionExecutionPhase::Committed as i32,
        Some(SessionJournalEntryPayload {
            payload: Some(session_journal_entry_payload::Payload::Commit(
                SessionJournalEntryPayloadCommit {
                    committed_message_id: committed_message_id.to_string(),
                },
            )),
        }),
        Some(committed_message_id),
        now_micros,
    )
    .await?;
    update_submission_from_entry(
        kv,
        ns,
        agent,
        session_id,
        submission_id,
        &entry,
        Some(status),
        Some(committed_message_id),
        now_micros,
    )
    .await?;
    Ok(entry)
}

pub async fn repair_submission_pointer_to_latest(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    now_micros: i64,
) -> Result<Option<SessionJournalEntry>> {
    let Some(entry) = latest_journal_entry(kv, ns, agent, session_id, submission_id).await? else {
        return Ok(None);
    };
    update_submission_from_entry(
        kv,
        ns,
        agent,
        session_id,
        submission_id,
        &entry,
        None,
        None,
        now_micros,
    )
    .await?;
    Ok(Some(entry))
}

pub async fn list_journal_entries(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
) -> Result<Vec<SessionJournalEntry>> {
    let prefix = keys::session_journal_entry_prefix(ns, agent, session_id, submission_id);
    let mut entries = kv
        .list_entries(&prefix)
        .await?
        .into_iter()
        .map(|(_, bytes)| SessionJournalEntry::decode(bytes.as_slice()).map_err(Into::into))
        .collect::<Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.journal_entry_id.parse::<u64>().unwrap_or(0));
    Ok(entries)
}

async fn append_journal_entry(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    attempt_id: &str,
    phase: i32,
    payload: Option<SessionJournalEntryPayload>,
    committed_message_id: Option<&str>,
    now_micros: i64,
) -> Result<SessionJournalEntry> {
    let entry = append_journal_entry_raw(
        kv,
        ns,
        agent,
        session_id,
        submission_id,
        attempt_id,
        phase,
        payload,
        committed_message_id,
        now_micros,
    )
    .await?;
    update_submission_from_entry(
        kv,
        ns,
        agent,
        session_id,
        submission_id,
        &entry,
        None,
        None,
        now_micros,
    )
    .await?;
    Ok(entry)
}

async fn append_journal_entry_raw(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    attempt_id: &str,
    phase: i32,
    payload: Option<SessionJournalEntryPayload>,
    committed_message_id: Option<&str>,
    now_micros: i64,
) -> Result<SessionJournalEntry> {
    // Append the ordered journal entry before the submission pointer is updated.
    // If the process crashes after this write, recovery can repair the pointer
    // by scanning the journal; if another worker wins the same sequence number,
    // the CAS fails and we retry with the next observed id.
    for _ in 0..16 {
        ensure_submission_attempt_current(kv, ns, agent, session_id, submission_id, attempt_id)
            .await?;
        let journal_entry_id =
            next_journal_entry_id(kv, ns, agent, session_id, submission_id).await?;
        let entry = SessionJournalEntry {
            submission_id: submission_id.to_string(),
            journal_entry_id: journal_entry_id.clone(),
            attempt_id: attempt_id.to_string(),
            phase,
            payload: payload.clone(),
            created_at: now_micros,
            updated_at: now_micros,
            committed_at: (phase == SessionExecutionPhase::Committed as i32).then_some(now_micros),
            committed_message_id: committed_message_id.map(str::to_string),
        };
        let key =
            keys::session_journal_entry(ns, agent, session_id, submission_id, &journal_entry_id);
        if kv
            .compare_and_swap(&key, None, &entry.encode_to_vec())
            .await?
        {
            return Ok(entry);
        }
    }
    Err(anyhow!("failed to append session journal entry"))
}

async fn next_journal_entry_id(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
) -> Result<String> {
    let prefix = keys::session_journal_entry_prefix(ns, agent, session_id, submission_id);
    let max_id = kv
        .list_keys(&prefix)
        .await?
        .into_iter()
        .filter_map(|key| key.name.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    Ok(format!("{:06}", max_id.saturating_add(1)))
}

async fn latest_journal_entry(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
) -> Result<Option<SessionJournalEntry>> {
    let prefix = keys::session_journal_entry_prefix(ns, agent, session_id, submission_id);
    let Some(key) = kv
        .list_keys(&prefix)
        .await?
        .into_iter()
        .max_by_key(|key| key.name.parse::<u64>().unwrap_or(0))
    else {
        return Ok(None);
    };
    kv.get(&key)
        .await?
        .map(|bytes| SessionJournalEntry::decode(bytes.as_slice()).map_err(Into::into))
        .transpose()
}

async fn committed_journal_entry(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    committed_message_id: &str,
) -> Result<Option<SessionJournalEntry>> {
    let prefix = keys::session_journal_entry_prefix(ns, agent, session_id, submission_id);
    let mut found: Option<SessionJournalEntry> = None;
    for (_, bytes) in kv.list_entries(&prefix).await? {
        let entry = SessionJournalEntry::decode(bytes.as_slice())?;
        if entry.phase == SessionExecutionPhase::Committed as i32
            && entry.committed_message_id.as_deref() == Some(committed_message_id)
        {
            match &found {
                Some(existing)
                    if journal_entry_order(&existing.journal_entry_id)
                        >= journal_entry_order(&entry.journal_entry_id) => {}
                _ => found = Some(entry),
            }
        }
    }
    Ok(found)
}

fn journal_entry_order(journal_entry_id: &str) -> u64 {
    journal_entry_id.parse::<u64>().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ProtoKeyValueStoreExt;
    use crate::gateway::rpc::data_proto::{SessionExecutionPhase, SessionSubmissionStatus};
    use crate::harness::llm::{ChatResponse, ToolCall};
    use crate::harness::sessions::{
        create_submission_if_absent, pending_submission, SessionSubmission,
    };

    async fn load_submission(kv: &crate::test_support::MockKvStore) -> Option<SessionSubmission> {
        kv.get_msg::<SessionSubmission>(&keys::session_submission(
            "ns",
            "agent",
            "session-1",
            "submission-1",
        ))
        .await
        .unwrap()
    }

    async fn seed_claimed_submission(kv: &crate::test_support::MockKvStore) {
        let mut submission = pending_submission("submission-1", "session-1", "user-1", 1);
        submission.status = SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        create_submission_if_absent(kv, "ns", "agent", "session-1", &submission)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn journal_entries_append_in_order_and_update_submission_pointer() {
        let kv = crate::test_support::MockKvStore::default();
        let objects =
            std::sync::Arc::new(crate::control::object_store::InMemoryObjectStore::default());
        let cas = crate::control::cas::CasStore::new(objects);
        seed_claimed_submission(&kv).await;

        let response = ChatResponse {
            content: "hello".to_string(),
            tool_calls: Vec::new(),
            usage: None,
        };
        let first = append_llm_response(
            &kv,
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            &response,
            2,
        )
        .await
        .unwrap();
        let second = append_tool_result(
            &kv,
            &cas,
            "ns",
            "agent",
            "session-1",
            "message-1",
            "000002",
            "submission-1",
            "attempt-1",
            "call-1",
            "search",
            "answer",
            3,
        )
        .await
        .unwrap();

        assert_eq!(first.journal_entry_id, "000001");
        assert_eq!(second.journal_entry_id, "000002");
        assert_eq!(first.phase, SessionExecutionPhase::LlmResponse as i32);
        assert_eq!(second.phase, SessionExecutionPhase::ToolResult as i32);
        let submission = load_submission(&kv).await.unwrap();
        assert_eq!(
            submission.current_journal_entry_id.as_deref(),
            Some("000002")
        );
        assert_eq!(
            submission.current_phase,
            SessionExecutionPhase::ToolResult as i32
        );
    }

    #[tokio::test]
    async fn terminal_mark_appends_committed_once() {
        let kv = crate::test_support::MockKvStore::default();
        seed_claimed_submission(&kv).await;

        let first = mark_terminal(
            &kv,
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            SessionSubmissionStatus::Committed as i32,
            "reply-1",
            4,
        )
        .await
        .unwrap();
        let second = mark_terminal(
            &kv,
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            SessionSubmissionStatus::Committed as i32,
            "reply-1",
            5,
        )
        .await
        .unwrap();

        assert_eq!(first.journal_entry_id, second.journal_entry_id);
        let entries = list_journal_entries(&kv, "ns", "agent", "session-1", "submission-1")
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn terminal_mark_is_attempt_fenced() {
        let kv = crate::test_support::MockKvStore::default();
        let mut submission = pending_submission("submission-1", "session-1", "user-1", 1);
        submission.status = SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-2".to_string();
        create_submission_if_absent(&kv, "ns", "agent", "session-1", &submission)
            .await
            .unwrap();
        kv.set_msg(
            &keys::session_journal_entry("ns", "agent", "session-1", "submission-1", "000001"),
            &SessionJournalEntry {
                submission_id: "submission-1".to_string(),
                journal_entry_id: "000001".to_string(),
                attempt_id: "attempt-1".to_string(),
                phase: SessionExecutionPhase::Committed as i32,
                payload: Some(SessionJournalEntryPayload {
                    payload: Some(session_journal_entry_payload::Payload::Commit(
                        SessionJournalEntryPayloadCommit {
                            committed_message_id: "reply-1".to_string(),
                        },
                    )),
                }),
                created_at: 2,
                updated_at: 2,
                committed_at: Some(2),
                committed_message_id: Some("reply-1".to_string()),
            },
        )
        .await
        .unwrap();

        let stale = mark_terminal(
            &kv,
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            SessionSubmissionStatus::Committed as i32,
            "reply-1",
            3,
        )
        .await
        .unwrap_err();
        assert!(stale
            .to_string()
            .contains("stale session submission attempt"));

        let repaired = mark_terminal(
            &kv,
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-2",
            SessionSubmissionStatus::Committed as i32,
            "reply-1",
            4,
        )
        .await
        .unwrap();
        assert_eq!(repaired.journal_entry_id, "000002");
        let entries = list_journal_entries(&kv, "ns", "agent", "session-1", "submission-1")
            .await
            .unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn stale_attempt_cannot_append() {
        let kv = crate::test_support::MockKvStore::default();
        seed_claimed_submission(&kv).await;
        let response = ChatResponse {
            content: "hello".to_string(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "search".to_string(),
                arguments: "{}".to_string(),
            }],
            usage: None,
        };

        let err = append_llm_response(
            &kv,
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "stale-attempt",
            &response,
            2,
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("stale session submission attempt"));
    }
}
