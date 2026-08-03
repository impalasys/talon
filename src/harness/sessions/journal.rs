// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use prost::Message;

use super::submission::{ensure_submission_attempt_current, update_submission_from_entry};
use super::SessionJournalEntry;
use crate::control::cas::CasStore;
use crate::control::tool_output::{self, ToolOutputStorageContext};
use crate::control::{keys, KeyValueStore, ListOptions};
use crate::gateway::rpc::data_proto::{
    session_journal_entry_payload, SessionExecutionPhase, SessionJournalEntryPayload,
    SessionJournalEntryPayloadCommit, SessionJournalEntryPayloadCompaction,
    SessionJournalEntryPayloadLlmResponse, SessionJournalEntryPayloadLlmUsage,
    SessionJournalEntryPayloadToolResult,
};
use crate::harness::llm::ToolOutput;
use crate::harness::llm::{ChatResponse, TokenCounter};

pub async fn append_compaction(
    kv: &dyn KeyValueStore,
    cas: &CasStore,
    ns: &str,
    agent_id: &str,
    session_id: &str,
    submission_id: &str,
    attempt_id: &str,
    summary: &str,
    now_micros: i64,
) -> Result<(
    SessionJournalEntry,
    crate::gateway::rpc::data_proto::ObjectRef,
)> {
    // Allocate the entry id before writing CAS so the immutable object key is
    // shared by the journal and the canonical internal message marker. A
    // contested append may leave an unreachable object, which session GC can
    // safely reclaim; it never exposes a partially written context boundary.
    let mut entry = None;
    for _ in 0..16 {
        ensure_submission_attempt_current(kv, ns, agent_id, session_id, submission_id, attempt_id)
            .await?;
        let journal_entry_id =
            next_journal_entry_id(kv, ns, agent_id, session_id, submission_id).await?;
        let summary_object = cas
            .put_compaction_summary(
                ns,
                agent_id,
                session_id,
                submission_id,
                &journal_entry_id,
                summary,
            )
            .await?;
        let candidate = SessionJournalEntry {
            submission_id: submission_id.to_string(),
            journal_entry_id: journal_entry_id.clone(),
            attempt_id: attempt_id.to_string(),
            phase: SessionExecutionPhase::Compaction as i32,
            payload: Some(SessionJournalEntryPayload {
                payload: Some(session_journal_entry_payload::Payload::Compaction(
                    compaction_payload(summary_object.clone()),
                )),
            }),
            created_at: now_micros,
            updated_at: now_micros,
            committed_at: None,
            committed_message_id: None,
        };
        let key =
            keys::session_journal_entry(ns, agent_id, session_id, submission_id, &journal_entry_id);
        if kv
            .compare_and_swap(&key, None, &candidate.encode_to_vec())
            .await?
        {
            update_submission_from_entry(
                kv,
                ns,
                agent_id,
                session_id,
                submission_id,
                &candidate,
                None,
                None,
                now_micros,
            )
            .await?;
            entry = Some((candidate, summary_object));
            break;
        }
    }
    let (entry, summary_object) =
        entry.ok_or_else(|| anyhow!("failed to append session compaction journal entry"))?;

    tracing::info!(
        submission = %submission_id,
        journal_entry_id = %entry.journal_entry_id,
        "Compaction journal entry written",
    );

    Ok((entry, summary_object))
}

fn compaction_payload(
    summary_object: crate::gateway::rpc::data_proto::ObjectRef,
) -> SessionJournalEntryPayloadCompaction {
    SessionJournalEntryPayloadCompaction {
        summary: Some(summary_object),
    }
}

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
    ensure_submission_attempt_current(kv, ns, agent, session_id, submission_id, attempt_id).await?;
    if let Some(counter) = response.usage.as_ref() {
        if let Some(existing) =
            existing_context_entry_for_request(kv, ns, agent, session_id, counter).await?
        {
            log_duplicate_context_entry(&existing.0, &existing.1, counter);
            return Ok(existing.0);
        }
    }
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

pub async fn append_llm_usage(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    attempt_id: &str,
    counter: &TokenCounter,
    now_micros: i64,
) -> Result<SessionJournalEntry> {
    ensure_submission_attempt_current(kv, ns, agent, session_id, submission_id, attempt_id).await?;
    if let Some(existing) =
        existing_context_entry_for_request(kv, ns, agent, session_id, counter).await?
    {
        log_duplicate_context_entry(&existing.0, &existing.1, counter);
        return Ok(existing.0);
    }
    append_journal_entry(
        kv,
        ns,
        agent,
        session_id,
        submission_id,
        attempt_id,
        SessionExecutionPhase::LlmUsage as i32,
        Some(SessionJournalEntryPayload {
            payload: Some(session_journal_entry_payload::Payload::LlmUsage(
                SessionJournalEntryPayloadLlmUsage {
                    context_tokens: Some(counter.clone()),
                },
            )),
        }),
        None,
        now_micros,
    )
    .await
}

pub fn latest_context_tokens(entries: &[SessionJournalEntry]) -> Option<TokenCounter> {
    latest_context_token_entry(entries).map(|(_, counter)| counter)
}

pub fn latest_context_token_entry(
    entries: &[SessionJournalEntry],
) -> Option<(SessionJournalEntry, TokenCounter)> {
    entries
        .iter()
        .rev()
        .find_map(|entry| context_tokens(entry).map(|counter| (entry.clone(), counter)))
}

pub fn context_tokens(entry: &SessionJournalEntry) -> Option<TokenCounter> {
    let payload = entry.payload.as_ref()?.payload.as_ref()?;
    match payload {
        session_journal_entry_payload::Payload::LlmResponse(response) => {
            response.response.as_ref()?.usage.clone()
        }
        session_journal_entry_payload::Payload::LlmUsage(usage) => usage.context_tokens.clone(),
        _ => None,
    }
}

async fn existing_context_entry_for_request(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    counter: &TokenCounter,
) -> Result<Option<(SessionJournalEntry, TokenCounter)>> {
    let Some(request_id) = counter
        .provider_request_id
        .as_deref()
        .filter(|request_id| !request_id.is_empty())
    else {
        return Ok(None);
    };

    let mut matches = Vec::new();
    for submission_key in kv
        .list_keys(
            &keys::session_submission_prefix(ns, agent, session_id),
            None,
        )
        .await?
    {
        for (_, bytes) in kv
            .list_entries(
                &keys::session_journal_entry_prefix(ns, agent, session_id, &submission_key.name),
                None,
            )
            .await?
        {
            let entry = SessionJournalEntry::decode(bytes.as_slice())?;
            let Some(entry_counter) = context_tokens(&entry) else {
                continue;
            };
            if entry_counter.provider_request_id.as_deref() == Some(request_id) {
                matches.push((entry, entry_counter));
            }
        }
    }

    matches.sort_by(|(left, _), (right, _)| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.submission_id.cmp(&right.submission_id))
            .then_with(|| {
                journal_entry_order(&left.journal_entry_id)
                    .cmp(&journal_entry_order(&right.journal_entry_id))
            })
    });
    Ok(matches.into_iter().next())
}

fn log_duplicate_context_entry(
    existing_entry: &SessionJournalEntry,
    existing_counter: &TokenCounter,
    incoming_counter: &TokenCounter,
) {
    if existing_counter == incoming_counter {
        tracing::warn!(
            provider_request_id = ?incoming_counter.provider_request_id,
            submission = %existing_entry.submission_id,
            journal_entry_id = %existing_entry.journal_entry_id,
            "ignored duplicate provider token snapshot"
        );
    } else {
        tracing::warn!(
            provider_request_id = ?incoming_counter.provider_request_id,
            submission = %existing_entry.submission_id,
            journal_entry_id = %existing_entry.journal_entry_id,
            "ignored conflicting duplicate provider token snapshot; preserving first journaled snapshot"
        );
    }
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
    result: &ToolOutput,
    now_micros: i64,
) -> Result<SessionJournalEntry> {
    ensure_submission_attempt_current(kv, ns, agent, session_id, submission_id, attempt_id).await?;
    let result = tool_output::normalize_for_session_storage(
        cas,
        ToolOutputStorageContext {
            ns,
            agent,
            session_id,
            message_id,
            part_id,
            tool_call_id,
            tool_name: name,
        },
        result,
    )
    .await?;
    ensure_submission_attempt_current(kv, ns, agent, session_id, submission_id, attempt_id).await?;
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
                    output: String::new(),
                    object: None,
                    tool_output: Some(result),
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
    let entries = kv
        .list_entries(&prefix, None)
        .await?
        .into_iter()
        .map(|(_, bytes)| SessionJournalEntry::decode(bytes.as_slice()).map_err(Into::into))
        .collect::<Result<Vec<_>>>()?;
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
        .list_keys(&prefix, Some(ListOptions::desc().limit(1)))
        .await?
        .into_iter()
        .next()
        .and_then(|key| key.name.parse::<u64>().ok())
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
        .list_keys(&prefix, Some(ListOptions::desc().limit(1)))
        .await?
        .into_iter()
        .next()
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
    for (_, bytes) in kv.list_entries(&prefix, None).await? {
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
    use crate::control::tool_output::ToolOutputExt;
    use crate::control::ProtoKeyValueStoreExt;
    use crate::gateway::rpc::data_proto::{SessionExecutionPhase, SessionSubmissionStatus};
    use crate::harness::llm::{ChatResponse, TokenCounter, ToolCall};
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
            &ToolOutput::text("answer"),
            3,
        )
        .await
        .unwrap();

        assert_eq!(first.journal_entry_id, "000001");
        assert_eq!(second.journal_entry_id, "000002");
        assert_eq!(first.phase, SessionExecutionPhase::LlmResponse as i32);
        assert_eq!(second.phase, SessionExecutionPhase::ToolResult as i32);
        let Some(session_journal_entry_payload::Payload::ToolResult(tool_result)) = second
            .payload
            .as_ref()
            .and_then(|payload| payload.payload.as_ref())
        else {
            panic!("expected tool result payload");
        };
        assert_eq!(tool_result.output, "");
        assert!(tool_result.object.is_none());
        assert_eq!(
            tool_result
                .tool_output
                .as_ref()
                .map(|output| output.summary.as_str()),
            Some("answer")
        );
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
    async fn usage_boundary_is_recoverable_as_latest_context_tokens() {
        let kv = crate::test_support::MockKvStore::default();
        seed_claimed_submission(&kv).await;
        let counter = TokenCounter {
            input_tokens: 12,
            cached_input_tokens: 3,
            output_tokens: 4,
            reasoning_output_tokens: 1,
            total_tokens: 17,
            usage_available: true,
            provider_request_id: Some("provider-request".to_string()),
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
        };

        let entry = append_llm_usage(
            &kv,
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            &counter,
            2,
        )
        .await
        .unwrap();
        assert_eq!(entry.phase, SessionExecutionPhase::LlmUsage as i32);

        let entries = list_journal_entries(&kv, "ns", "agent", "session-1", "submission-1")
            .await
            .unwrap();
        assert_eq!(latest_context_tokens(&entries), Some(counter));
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

    #[tokio::test]
    async fn compaction_entry_appends_and_updates_submission() {
        let kv = crate::test_support::MockKvStore::default();
        seed_claimed_submission(&kv).await;
        let cas = CasStore::new(std::sync::Arc::new(
            crate::control::object_store::InMemoryObjectStore::default(),
        ));

        let (compact_entry, _) = append_compaction(
            &kv,
            &cas,
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            "# Compaction\nCompleted Step 1.",
            10,
        )
        .await
        .unwrap();

        assert_eq!(
            compact_entry.phase,
            SessionExecutionPhase::Compaction as i32
        );
        let submission = kv
            .get_msg::<SessionSubmission>(&keys::session_submission(
                "ns",
                "agent",
                "session-1",
                "submission-1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            submission.current_journal_entry_id.as_deref(),
            Some(compact_entry.journal_entry_id.clone().as_str()),
        );

        let entries = list_journal_entries(&kv, "ns", "agent", "session-1", "submission-1")
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        let payload = entries[0]
            .payload
            .as_ref()
            .unwrap()
            .payload
            .as_ref()
            .unwrap();
        let crate::gateway::rpc::data_proto::session_journal_entry_payload::Payload::Compaction(
            payload,
        ) = payload
        else {
            panic!("expected compaction payload")
        };
        assert!(payload
            .summary
            .as_ref()
            .unwrap()
            .key
            .ends_with("/000001.txt"));
    }
}
