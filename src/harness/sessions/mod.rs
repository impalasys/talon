// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

mod journal;
mod lease;
mod session;
mod submission;

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
pub use session::{
    clear_provider_request_id, persist_context_tokens, reset_provider_request_id_if_idle,
};
pub use submission::{
    claim_submission, create_submission_if_absent, pending_compaction_submission,
    pending_submission, renew_submission_claim, submission_is_terminal, ClaimOutcome, RenewOutcome,
};
