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
pub use journal::{append_llm_response, append_tool_result};
pub use lease::{SubmissionLease, SubmissionLeaseRenewer};
pub use submission::{
    claim_submission, create_submission_if_absent, pending_submission, renew_submission_claim,
    submission_is_terminal, ClaimOutcome, RenewOutcome,
};

pub async fn persist_context_tokens(
    kv: &dyn KeyValueStore,
    claim: &SubmissionLease,
    counter: &TokenCounter,
) -> Result<()> {
    let key = keys::session(&claim.ns, &claim.agent, &claim.session_id);
    for _ in 0..8 {
        let Some(current) = kv.get(&key).await? else {
            return Err(anyhow!("session not found while persisting context tokens"));
        };
        let mut session = data_proto::Session::decode(current.as_slice())?;
        session.context_tokens = Some(counter.clone());
        submission::ensure_submission_attempt_current(
            kv,
            &claim.ns,
            &claim.agent,
            &claim.session_id,
            &claim.submission_id,
            &claim.attempt_id,
        )
        .await?;
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
        let claim = SubmissionLease {
            ns: "ns".to_string(),
            agent: "agent".to_string(),
            session_id: "session".to_string(),
            submission_id: "submission".to_string(),
            attempt_id: "attempt".to_string(),
            ttl_micros: 1_000_000,
        };
        kv.set_msg(
            &keys::session_submission("ns", "agent", "session", "submission"),
            &SessionSubmission {
                submission_id: "submission".to_string(),
                session_id: "session".to_string(),
                status: data_proto::SessionSubmissionStatus::Claimed as i32,
                attempt_id: "attempt".to_string(),
                claim_expires_at: Some(chrono::Utc::now().timestamp_micros() + 1_000_000),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        persist_context_tokens(kv.as_ref(), &claim, &counter)
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
