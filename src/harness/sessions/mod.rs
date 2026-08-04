// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

mod journal;
mod lease;
mod submission;

use crate::control::{keys, KeyValueStore};
use crate::gateway::rpc::data_proto::{self, TokenCounter};
use anyhow::{anyhow, Result};
use prost::Message;

pub const SESSION_LABEL_SUBMISSION_ID: &str = "talon.session.submission_id";
pub const SESSION_LABEL_ATTEMPT_ID: &str = "talon.session.attempt_id";
pub const SESSION_LABEL_PROJECTION_STATE: &str = "talon.session.projection_state";
pub const SESSION_LABEL_LATEST_JOURNAL_ENTRY_ID: &str = "talon.session.latest_journal_entry_id";

pub const SESSION_PROJECTION_STATE_IN_PROGRESS: &str = "in_progress";
pub const SESSION_PROJECTION_STATE_COMPLETE_UNCOMMITTED: &str = "complete_uncommitted";
pub const SESSION_PROJECTION_STATE_COMMITTED: &str = "committed";
pub const SESSION_PROJECTION_STATE_FAILED: &str = "failed";

pub use crate::gateway::rpc::data_proto::{SessionJournalEntry, SessionSubmission};
pub use journal::{
    append_compaction, list_journal_entries, mark_terminal, repair_submission_pointer_to_latest,
};
pub use journal::{
    append_llm_response, append_tool_result, context_tokens, latest_context_token_entry,
};
pub use lease::{SubmissionLease, SubmissionLeaseRenewer};
pub use submission::{
    claim_submission, create_submission_if_absent, pending_submission, renew_submission_claim,
    submission_is_terminal, ClaimOutcome, RenewOutcome,
};

pub async fn persist_context_tokens(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    counter: &TokenCounter,
) -> Result<()> {
    let key = keys::session(ns, agent, session_id);
    for _ in 0..8 {
        let Some(current) = kv.get(&key).await? else {
            return Err(anyhow!("session not found while persisting context tokens"));
        };
        let mut session = data_proto::Session::decode(current.as_slice())?;
        session.context_tokens = Some(counter.clone());
        if kv
            .compare_and_swap(&key, Some(current.as_slice()), &session.encode_to_vec())
            .await?
        {
            return Ok(());
        }
    }
    Err(anyhow!(
        "failed to persist session context tokens after CAS retries"
    ))
}

/// Persist a token snapshot only when its durable journal entry is still the
/// latest usage-bearing entry for the session. The journal remains the source
/// of ordering truth; the Session field is only the latest materialized value.
pub async fn persist_context_tokens_for_journal_entry(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    entry: &SessionJournalEntry,
    counter: &TokenCounter,
) -> Result<()> {
    let latest = latest_session_context_entry(kv, ns, agent, session_id).await?;
    let Some(latest_entry) = latest else {
        return Ok(());
    };
    if context_entry_order(&latest_entry) > context_entry_order(entry) {
        tracing::warn!(
            session = %session_id,
            submission = %entry.submission_id,
            journal_entry_id = %entry.journal_entry_id,
            latest_submission = %latest_entry.submission_id,
            latest_journal_entry_id = %latest_entry.journal_entry_id,
            "ignored stale session context token snapshot"
        );
        return Ok(());
    }

    let key = keys::session(ns, agent, session_id);
    for _ in 0..8 {
        let Some(current) = kv.get(&key).await? else {
            return Err(anyhow!("session not found while persisting context tokens"));
        };
        let session = data_proto::Session::decode(current.as_slice())?;
        if let Some(current_request_id) = session
            .context_tokens
            .as_ref()
            .and_then(|current| current.provider_request_id.as_deref())
            .filter(|request_id| !request_id.is_empty())
        {
            if let Some(current_entry) = find_session_context_entry_by_request_id(
                kv,
                ns,
                agent,
                session_id,
                current_request_id,
            )
            .await?
            {
                if context_entry_order(&current_entry) > context_entry_order(entry) {
                    tracing::warn!(
                        session = %session_id,
                        submission = %entry.submission_id,
                        journal_entry_id = %entry.journal_entry_id,
                        current_submission = %current_entry.submission_id,
                        current_journal_entry_id = %current_entry.journal_entry_id,
                        "ignored stale session context token snapshot after CAS retry"
                    );
                    return Ok(());
                }
            }
        }

        let mut updated = session;
        updated.context_tokens = Some(counter.clone());
        if kv
            .compare_and_swap(&key, Some(current.as_slice()), &updated.encode_to_vec())
            .await?
        {
            return Ok(());
        }
    }
    Err(anyhow!(
        "failed to persist session context tokens after CAS retries"
    ))
}

async fn latest_session_context_entry(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
) -> Result<Option<SessionJournalEntry>> {
    let mut latest = None;
    for submission_key in kv
        .list_keys(
            &keys::session_submission_prefix(ns, agent, session_id),
            None,
        )
        .await?
    {
        let entries =
            journal::list_journal_entries(kv, ns, agent, session_id, &submission_key.name).await?;
        for entry in entries {
            if context_tokens(&entry).is_some()
                && latest.as_ref().is_none_or(|current| {
                    context_entry_order(&entry) > context_entry_order(current)
                })
            {
                latest = Some(entry);
            }
        }
    }
    Ok(latest)
}

async fn find_session_context_entry_by_request_id(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    request_id: &str,
) -> Result<Option<SessionJournalEntry>> {
    let mut found = None;
    for submission_key in kv
        .list_keys(
            &keys::session_submission_prefix(ns, agent, session_id),
            None,
        )
        .await?
    {
        for entry in
            journal::list_journal_entries(kv, ns, agent, session_id, &submission_key.name).await?
        {
            if context_tokens(&entry)
                .and_then(|counter| counter.provider_request_id)
                .as_deref()
                == Some(request_id)
                && found.as_ref().is_none_or(|current| {
                    context_entry_order(&entry) > context_entry_order(current)
                })
            {
                found = Some(entry);
            }
        }
    }
    Ok(found)
}

fn context_entry_order(entry: &SessionJournalEntry) -> (i64, &str, u64) {
    (
        entry.created_at,
        &entry.submission_id,
        entry.journal_entry_id.parse::<u64>().unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ProtoKeyValueStoreExt;
    use crate::test_support::MockKvStore;
    use std::sync::Arc;

    #[tokio::test]
    async fn persists_latest_context_tokens_without_replacing_session_fields() {
        let kv = Arc::new(MockKvStore::default());
        let key = keys::session("ns", "agent", "session");
        kv.set_msg(
            &key,
            &data_proto::Session {
                id: "session".to_string(),
                agent: "agent".to_string(),
                ns: "ns".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 1,
                last_active: 2,
                metadata: [("source".to_string(), "test".to_string())]
                    .into_iter()
                    .collect(),
                labels: [("label".to_string(), "value".to_string())]
                    .into_iter()
                    .collect(),
                context_tokens: None,
            },
        )
        .await
        .unwrap();

        let counter = TokenCounter {
            input_tokens: 10,
            cached_input_tokens: 2,
            output_tokens: 3,
            reasoning_output_tokens: 1,
            total_tokens: 14,
            usage_available: true,
            provider_request_id: Some("request-1".to_string()),
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
        };
        persist_context_tokens(kv.as_ref(), "ns", "agent", "session", &counter)
            .await
            .unwrap();

        let stored = kv
            .get_msg::<data_proto::Session>(&key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.context_tokens, Some(counter));
        assert_eq!(stored.status, "PROCESSING");
        assert_eq!(stored.metadata.get("source"), Some(&"test".to_string()));
        assert_eq!(stored.labels.get("label"), Some(&"value".to_string()));
    }
}
