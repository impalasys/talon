// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

mod journal;
mod submission;

pub use crate::gateway::rpc::data_proto::{SessionJournalEntry, SessionSubmission};
pub use journal::{
    append_llm_response, append_tool_result, list_journal_entries, mark_terminal,
    repair_submission_pointer_to_latest,
};
pub use submission::{
    claim_submission, create_submission_if_absent, pending_submission, submission_is_terminal,
    ClaimOutcome,
};
