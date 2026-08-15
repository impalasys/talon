// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use prost::Message;
use std::collections::{HashMap, HashSet};

use super::{
    SessionJournalEntry, SessionJournalEntryExt, SESSION_LABEL_LATEST_JOURNAL_ENTRY_ID,
    SESSION_LABEL_PROJECTION_STATE, SESSION_LABEL_SUBMISSION_ID,
    SESSION_PROJECTION_STATE_COMMITTED, SESSION_PROJECTION_STATE_COMPLETE_UNCOMMITTED,
    SESSION_PROJECTION_STATE_IN_PROGRESS,
};
use crate::control::{ControlPlane, ListOptions, ProtoKeyValueStoreExt};
use crate::gateway::rpc::data_proto::{self, SessionExecutionPhase};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JournalRecoveryPlan {
    pub(crate) replay_start_index: usize,
    pub(crate) excluded_history_message_ids: HashSet<String>,
}

/// Returns the last journal entry already represented by a committed assistant
/// projection.
///
/// This closes the crash window where the assistant was finalized but the
/// following `STEER_INPUT` checkpoint was not appended.
async fn committed_assistant_journal_watermark(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    message_id: &str,
    submission_id: &str,
) -> Result<Option<String>> {
    let Some(message) = cp
        .kv
        .get_msg::<data_proto::SessionMessage>(&crate::control::keys::session_message(
            ns, agent, session_id, message_id,
        ))
        .await?
    else {
        return Ok(None);
    };
    if message.role != data_proto::MessageRole::RoleAssistant as i32
        || message
            .labels
            .get(SESSION_LABEL_PROJECTION_STATE)
            .map(String::as_str)
            != Some(SESSION_PROJECTION_STATE_COMMITTED)
        || message
            .labels
            .get(SESSION_LABEL_SUBMISSION_ID)
            .map(String::as_str)
            != Some(submission_id)
    {
        return Ok(None);
    }
    Ok(message
        .labels
        .get(SESSION_LABEL_LATEST_JOURNAL_ENTRY_ID)
        .filter(|entry_id| !entry_id.is_empty())
        .cloned())
}

/// Finds the newest uncommitted assistant projection owned by this submission.
///
/// A worker can crash after writing its projection but before its first journal
/// entry. Reusing that message ID prevents recovery from creating a duplicate
/// assistant projection.
pub(crate) async fn latest_submission_projection_message_id(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
) -> Result<Option<String>> {
    let prefix = crate::control::keys::session_message_prefix(ns, agent, session_id);
    for (_, bytes) in cp
        .kv
        .list_entries(&prefix, Some(ListOptions::desc()))
        .await?
    {
        let Ok(message) = data_proto::SessionMessage::decode(bytes.as_slice()) else {
            continue;
        };
        let projection_state = message
            .labels
            .get(SESSION_LABEL_PROJECTION_STATE)
            .map(String::as_str);
        if message.role == data_proto::MessageRole::RoleAssistant as i32
            && message
                .labels
                .get(SESSION_LABEL_SUBMISSION_ID)
                .map(String::as_str)
                == Some(submission_id)
            && matches!(
                projection_state,
                Some(SESSION_PROJECTION_STATE_IN_PROGRESS)
                    | Some(SESSION_PROJECTION_STATE_COMPLETE_UNCOMMITTED)
            )
        {
            return Ok(Some(message.id));
        }
    }
    Ok(None)
}

/// Chooses the canonical transcript boundary and journal suffix for recovery.
///
/// A `STEER_INPUT` with a continuation ID is a transcript checkpoint, whether
/// or not an assistant segment preceded it. Legacy steer entries without a
/// continuation ID remain on the full-replay path, while committed assistant
/// watermarks cover the finalization-before-checkpoint crash window.
pub(crate) async fn plan_journal_recovery(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    journal_entries: &[SessionJournalEntry],
) -> Result<JournalRecoveryPlan> {
    let latest_checkpoint = journal_entries
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, entry)| {
            let payload = entry.as_steer_input()?;
            (!payload.next_assistant_message_id.is_empty())
                .then_some((index, payload.next_assistant_message_id.clone()))
        });
    let mut replay_start_index = latest_checkpoint.as_ref().map_or(0, |(index, _)| index + 1);
    let mut active_assistant_message_id =
        latest_checkpoint.map(|(_, next_message_id)| next_message_id);
    let journal_entry_ids = journal_entries
        .iter()
        .map(|entry| entry.journal_entry_id.as_str())
        .collect::<HashSet<_>>();
    let mut committed_watermark_cache: HashMap<String, Option<String>> = HashMap::new();

    // A committed assistant after the latest checkpoint identifies the narrow
    // crash window between finalizing that segment and appending its steer
    // boundary. Never use this shortcut across a steer entry: legacy steer
    // records must continue through the full replay path.
    let pending_slice_contains_steer = journal_entries[replay_start_index..]
        .iter()
        .any(|entry| entry.as_steer_input().is_some());
    if !pending_slice_contains_steer {
        while replay_start_index < journal_entries.len() {
            let entry = &journal_entries[replay_start_index];
            if entry.phase != SessionExecutionPhase::LlmResponse as i32
                && entry.phase != SessionExecutionPhase::ToolResult as i32
            {
                break;
            }
            if let Some(payload) = entry.as_llm_response() {
                if !payload.assistant_message_id.is_empty() {
                    active_assistant_message_id = Some(payload.assistant_message_id.clone());
                }
            }
            let Some(message_id) = active_assistant_message_id.as_deref() else {
                break;
            };
            let watermark = if let Some(watermark) = committed_watermark_cache.get(message_id) {
                watermark.clone()
            } else {
                let watermark = committed_assistant_journal_watermark(
                    cp,
                    ns,
                    agent,
                    session_id,
                    message_id,
                    submission_id,
                )
                .await?;
                committed_watermark_cache.insert(message_id.to_string(), watermark.clone());
                watermark
            };
            let Some(watermark) = watermark else {
                break;
            };
            if !journal_entry_ids.contains(watermark.as_str()) || entry.journal_entry_id > watermark
            {
                break;
            }
            replay_start_index += 1;
        }
    }

    let mut excluded_history_message_ids = journal_entries[replay_start_index..]
        .iter()
        .filter_map(|entry| entry.as_steer_input())
        .flat_map(|payload| payload.message_ids.iter().cloned())
        .collect::<HashSet<_>>();
    if pending_slice_contains_steer {
        // Legacy steer records stay on the full replay path. Exclude canonical
        // assistant projections represented by that same journal so the
        // assistant/tool exchange is reconstructed exactly once.
        excluded_history_message_ids.extend(
            journal_entries[replay_start_index..]
                .iter()
                .filter_map(|entry| entry.as_llm_response())
                .map(|payload| payload.assistant_message_id.clone())
                .filter(|message_id| !message_id.is_empty()),
        );
    }

    Ok(JournalRecoveryPlan {
        replay_start_index,
        excluded_history_message_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cas::CasStore;
    use crate::control::tool_output::ToolOutputExt;
    use crate::gateway::rpc::data_proto::SessionSubmissionStatus;
    use crate::harness::llm::{ChatResponse, ToolCall, ToolOutput};
    use crate::harness::sessions;
    use crate::test_support::MockKvStore;
    use std::sync::Arc;

    fn control_plane(kv: Arc<MockKvStore>) -> ControlPlane {
        ControlPlane::builder(
            kv,
            Arc::new(crate::test_support::RecordingPubSub::default()),
        )
        .build()
    }

    async fn create_claimed_submission(kv: &MockKvStore) {
        let mut submission = sessions::pending_submission("submission-1", "session-1", "user-1", 1);
        submission.status = SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        sessions::create_submission_if_absent(kv, "ns", "agent", "session-1", &submission)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn reuses_latest_uncommitted_projection_when_journal_is_empty() {
        let kv = Arc::new(MockKvStore::default());
        let cp = control_plane(kv.clone());
        for (id, submission_id, projection_state) in [
            (
                "000001",
                "submission-1",
                sessions::SESSION_PROJECTION_STATE_IN_PROGRESS,
            ),
            (
                "000002",
                "other-submission",
                sessions::SESSION_PROJECTION_STATE_IN_PROGRESS,
            ),
            (
                "000003",
                "submission-1",
                sessions::SESSION_PROJECTION_STATE_COMPLETE_UNCOMMITTED,
            ),
            (
                "000004",
                "submission-1",
                sessions::SESSION_PROJECTION_STATE_COMMITTED,
            ),
        ] {
            kv.set_msg(
                &crate::control::keys::session_message("ns", "agent", "session-1", id),
                &data_proto::SessionMessage {
                    id: id.to_string(),
                    role: data_proto::MessageRole::RoleAssistant as i32,
                    labels: HashMap::from([
                        (
                            sessions::SESSION_LABEL_SUBMISSION_ID.to_string(),
                            submission_id.to_string(),
                        ),
                        (
                            sessions::SESSION_LABEL_PROJECTION_STATE.to_string(),
                            projection_state.to_string(),
                        ),
                    ]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        assert_eq!(
            latest_submission_projection_message_id(
                &cp,
                "ns",
                "agent",
                "session-1",
                "submission-1",
            )
            .await
            .unwrap()
            .as_deref(),
            Some("000003")
        );
    }

    #[tokio::test]
    async fn starts_after_latest_new_format_steer_checkpoint() {
        let kv = Arc::new(MockKvStore::default());
        let cp = control_plane(kv.clone());
        create_claimed_submission(kv.as_ref()).await;

        sessions::append_llm_response(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            "assistant-1",
            &ChatResponse {
                content: "working".to_string(),
                tool_calls: vec![ToolCall {
                    id: "tool-1".to_string(),
                    name: "lookup".to_string(),
                    arguments: "{}".to_string(),
                }],
                usage: None,
            },
            10,
        )
        .await
        .unwrap();
        sessions::append_steer_input(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            &["steer-1".to_string()],
            "assistant-1",
            "assistant-2",
            20,
        )
        .await
        .unwrap();
        sessions::append_llm_response(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            "assistant-2",
            &ChatResponse {
                content: "continuing".to_string(),
                tool_calls: vec![ToolCall {
                    id: "tool-2".to_string(),
                    name: "lookup".to_string(),
                    arguments: "{}".to_string(),
                }],
                usage: None,
            },
            30,
        )
        .await
        .unwrap();
        sessions::append_steer_input(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            &["steer-2".to_string(), "steer-3".to_string()],
            "assistant-2",
            "assistant-3",
            40,
        )
        .await
        .unwrap();
        sessions::append_llm_response(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            "assistant-3",
            &ChatResponse {
                content: "active continuation".to_string(),
                tool_calls: Vec::new(),
                usage: None,
            },
            50,
        )
        .await
        .unwrap();

        let entries =
            sessions::list_journal_entries(kv.as_ref(), "ns", "agent", "session-1", "submission-1")
                .await
                .unwrap();
        let plan = plan_journal_recovery(&cp, "ns", "agent", "session-1", "submission-1", &entries)
            .await
            .unwrap();

        assert_eq!(plan.replay_start_index, 4);
        assert!(plan.excluded_history_message_ids.is_empty());
        assert_eq!(
            entries[plan.replay_start_index]
                .as_llm_response()
                .unwrap()
                .assistant_message_id,
            "assistant-3"
        );
    }

    #[tokio::test]
    async fn uses_committed_tool_segment_before_unjournaled_steer() {
        let kv = Arc::new(MockKvStore::default());
        let cp = control_plane(kv.clone());
        create_claimed_submission(kv.as_ref()).await;
        sessions::append_llm_response(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            "assistant-1",
            &ChatResponse {
                content: "working".to_string(),
                tool_calls: vec![ToolCall {
                    id: "tool-1".to_string(),
                    name: "lookup".to_string(),
                    arguments: "{}".to_string(),
                }],
                usage: None,
            },
            10,
        )
        .await
        .unwrap();
        let tool_result_entry = sessions::append_tool_result(
            kv.as_ref(),
            &CasStore::new(cp.objects.clone()),
            "ns",
            "agent",
            "session-1",
            "assistant-1",
            "part-1",
            "submission-1",
            "attempt-1",
            "tool-1",
            "lookup",
            &ToolOutput::text("result"),
            20,
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session_message("ns", "agent", "session-1", "assistant-1"),
            &data_proto::SessionMessage {
                id: "assistant-1".to_string(),
                role: data_proto::MessageRole::RoleAssistant as i32,
                created_at: 20,
                labels: HashMap::from([
                    (
                        sessions::SESSION_LABEL_PROJECTION_STATE.to_string(),
                        sessions::SESSION_PROJECTION_STATE_COMMITTED.to_string(),
                    ),
                    (
                        sessions::SESSION_LABEL_SUBMISSION_ID.to_string(),
                        "submission-1".to_string(),
                    ),
                    (
                        sessions::SESSION_LABEL_LATEST_JOURNAL_ENTRY_ID.to_string(),
                        tool_result_entry.journal_entry_id.clone(),
                    ),
                ]),
                parts: Vec::new(),
            },
        )
        .await
        .unwrap();
        sessions::append_llm_response(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            "assistant-1",
            &ChatResponse {
                content: "not covered by the committed projection".to_string(),
                tool_calls: Vec::new(),
                usage: None,
            },
            30,
        )
        .await
        .unwrap();

        let entries =
            sessions::list_journal_entries(kv.as_ref(), "ns", "agent", "session-1", "submission-1")
                .await
                .unwrap();
        let plan = plan_journal_recovery(&cp, "ns", "agent", "session-1", "submission-1", &entries)
            .await
            .unwrap();

        assert_eq!(plan.replay_start_index, 2);
    }

    #[tokio::test]
    async fn starts_after_pre_assistant_steer_checkpoint() {
        let kv = Arc::new(MockKvStore::default());
        let cp = control_plane(kv.clone());
        create_claimed_submission(kv.as_ref()).await;

        sessions::append_steer_input(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            &["steer-1".to_string(), "steer-2".to_string()],
            "",
            "assistant-1",
            10,
        )
        .await
        .unwrap();
        sessions::append_llm_response(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            "assistant-1",
            &ChatResponse {
                content: "working".to_string(),
                tool_calls: vec![ToolCall {
                    id: "tool-1".to_string(),
                    name: "lookup".to_string(),
                    arguments: "{}".to_string(),
                }],
                usage: None,
            },
            15,
        )
        .await
        .unwrap();
        sessions::append_steer_input(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            &["steer-3".to_string()],
            "assistant-1",
            "assistant-2",
            20,
        )
        .await
        .unwrap();
        sessions::append_steer_input(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            &["steer-4".to_string()],
            "",
            "assistant-3",
            25,
        )
        .await
        .unwrap();
        sessions::append_llm_response(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            "assistant-3",
            &ChatResponse {
                content: "continuing".to_string(),
                tool_calls: vec![ToolCall {
                    id: "tool-2".to_string(),
                    name: "lookup".to_string(),
                    arguments: "{}".to_string(),
                }],
                usage: None,
            },
            30,
        )
        .await
        .unwrap();

        let entries =
            sessions::list_journal_entries(kv.as_ref(), "ns", "agent", "session-1", "submission-1")
                .await
                .unwrap();
        let plan = plan_journal_recovery(&cp, "ns", "agent", "session-1", "submission-1", &entries)
            .await
            .unwrap();

        assert_eq!(plan.replay_start_index, 4);
        assert!(plan.excluded_history_message_ids.is_empty());
        assert_eq!(
            entries[plan.replay_start_index]
                .as_llm_response()
                .unwrap()
                .assistant_message_id,
            "assistant-3"
        );
    }
}
