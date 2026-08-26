// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use prost::Message;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

use super::super::runtime::AgentRuntime;
use super::super::sink::PubSubSessionSink;
use crate::control::cas::{decode_stored_object_bytes, CasStore};
use crate::control::tool_output::ToolOutputExt;
use crate::control::ControlPlane;
use crate::gateway::rpc::data_proto::{self, session_journal_entry_payload, SessionExecutionPhase};
use crate::harness::executor::{tool_output_loop_message, LoopMessage};
use crate::harness::llm::ToolOutput;
use crate::harness::sessions::{
    self, latest_submission_projection_message_id, plan_journal_recovery, JournalRecoveryPlan,
    SessionJournalEntryExt,
};

#[derive(Debug, Clone, PartialEq)]
pub(super) enum PreparedSubmissionState {
    ContinueExecution,
    StopAfterToolResult,
    FinalResponseReady {
        content: String,
        encrypted_reasoning: Option<data_proto::ObjectRef>,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum RecoveredProjectionPart {
    Text {
        part_id: String,
        content: String,
    },
    ToolCall {
        part_id: String,
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        part_id: String,
        id: String,
        name: String,
        result: String,
    },
    EncryptedReasoning {
        part_id: String,
        object: data_proto::ObjectRef,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct PreparedSubmission {
    state: PreparedSubmissionState,
    projection_parts: Vec<RecoveredProjectionPart>,
    latest_appended_journal_entry_id: Option<String>,
}

/// Resolve the assistant projection that this claimed submission should reuse.
///
/// Journaled assistant/continuation IDs are authoritative. The projection
/// fallback covers a crash after an uncommitted projection was written but
/// before its first journal entry became durable.
pub(super) async fn resolve_active_assistant_message_id(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    journal_entries: &[sessions::SessionJournalEntry],
) -> Result<Option<String>> {
    let journaled_message_id = journal_entries.iter().fold(None, |current, entry| {
        if let Some(response) = entry.as_llm_response() {
            if !response.assistant_message_id.is_empty() {
                return Some(response.assistant_message_id.clone());
            }
        }
        if let Some(steer) = entry.as_steer_input() {
            if !steer.next_assistant_message_id.is_empty() {
                return Some(steer.next_assistant_message_id.clone());
            }
        }
        current
    });
    if journaled_message_id.is_some() {
        return Ok(journaled_message_id);
    }

    latest_submission_projection_message_id(cp, ns, agent, session_id, submission_id).await
}

/// Build the worker recovery plan and reconcile steer queue copies across the
/// journal/queue crash windows. The harness planner remains responsible for
/// choosing the canonical transcript boundary and journal replay suffix.
pub(super) async fn prepare_recovery_plan(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    journal_entries: &[sessions::SessionJournalEntry],
) -> Result<JournalRecoveryPlan> {
    let mut recovery_plan =
        plan_journal_recovery(cp, ns, agent, session_id, submission_id, journal_entries).await?;
    let journaled_steer_message_ids = journal_entries
        .iter()
        .filter_map(SessionJournalEntryExt::as_steer_input)
        .flat_map(|payload| payload.message_ids.iter().cloned())
        .collect::<HashSet<_>>();
    crate::control::session_queue::commit_journaled_steer_queue_entries(
        cp.kv.as_ref(),
        ns,
        agent,
        session_id,
        &journaled_steer_message_ids,
    )
    .await?;
    recovery_plan.excluded_history_message_ids.extend(
        crate::control::session_queue::list_prepared_steer_message_ids(
            cp.kv.as_ref(),
            ns,
            agent,
            session_id,
        )
        .await?,
    );
    Ok(recovery_plan)
}

/// Replay the selected journal suffix and restore the sink's durable
/// projection state before the worker enters the normal LLM loop.
pub(super) async fn recover_claimed_submission(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    message_id: &str,
    submission_id: &str,
    attempt_id: &str,
    journal_entries: &[sessions::SessionJournalEntry],
    runtime: &mut AgentRuntime,
    sink: &PubSubSessionSink,
) -> Result<PreparedSubmissionState> {
    let prepared = replay_claimed_submission_journal(
        cp,
        ns,
        agent,
        session_id,
        message_id,
        submission_id,
        attempt_id,
        journal_entries,
        runtime,
    )
    .await?;

    if let Some(entry_id) = prepared.latest_appended_journal_entry_id.as_deref() {
        sink.seed_latest_journal_entry_id(Some(entry_id));
    }
    for part in &prepared.projection_parts {
        match part {
            RecoveredProjectionPart::Text { part_id, content } => {
                sink.seed_recovered_text_part(part_id, content);
                sink.advance_next_part_id_past(part_id);
            }
            RecoveredProjectionPart::ToolCall {
                part_id,
                id,
                name,
                input,
            } => {
                sink.seed_recovered_tool_call_part(part_id, id, name, input);
                sink.advance_next_part_id_past(part_id);
            }
            RecoveredProjectionPart::ToolResult {
                part_id,
                id,
                name,
                result,
            } => {
                sink.seed_recovered_tool_result_part(part_id, id, name, result)
                    .await?;
                sink.advance_next_part_id_past(part_id);
            }
            RecoveredProjectionPart::EncryptedReasoning { part_id, object } => {
                sink.seed_recovered_encrypted_reasoning_part(part_id, object.clone());
                sink.advance_next_part_id_past(part_id);
            }
        }
    }
    if let PreparedSubmissionState::FinalResponseReady {
        content,
        encrypted_reasoning,
    } = &prepared.state
    {
        sink.seed_recovered_final_text_part(content);
        if let Some(object) = encrypted_reasoning {
            sink.seed_recovered_final_encrypted_reasoning_part(object.clone());
        }
    }

    Ok(prepared.state)
}

async fn tool_output_from_recorded_object(
    cp: &ControlPlane,
    object: &data_proto::ObjectRef,
) -> Result<ToolOutput> {
    let stored = cp
        .objects
        .get(&object.key)
        .await?
        .ok_or_else(|| anyhow!("tool result object '{}' is missing", object.key))?;
    let bytes = decode_stored_object_bytes(&stored, &object.key)?;
    let mut media_type = if object.media_type.trim().is_empty() {
        stored.metadata.media_type.trim().to_string()
    } else {
        object.media_type.trim().to_string()
    };
    let filename = if object.filename.trim().is_empty() {
        stored.metadata.filename.clone()
    } else {
        object.filename.clone()
    };
    if media_type.is_empty() {
        media_type = [&filename, &object.key]
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .find_map(|value| mime_guess::from_path(value).first_raw())
            .unwrap_or_default()
            .to_string();
    }
    Ok(ToolOutput::from_source_object(
        bytes,
        media_type,
        filename,
        object.clone(),
    ))
}

async fn replay_claimed_submission_journal(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    message_id: &str,
    submission_id: &str,
    attempt_id: &str,
    journal_entries: &[sessions::SessionJournalEntry],
    runtime: &mut AgentRuntime,
) -> Result<PreparedSubmission> {
    async fn hydrate_steering_input(
        cp: &ControlPlane,
        ns: &str,
        agent: &str,
        session_id: &str,
        entry: &sessions::SessionJournalEntry,
        hydrated_message_ids: &mut HashSet<String>,
        runtime: &mut AgentRuntime,
    ) -> Result<()> {
        let Some(session_journal_entry_payload::Payload::SteerInput(payload)) = entry
            .payload
            .as_ref()
            .and_then(|payload| payload.payload.as_ref())
        else {
            return Err(anyhow!("STEER_INPUT entry is missing payload"));
        };
        for message_id in &payload.message_ids {
            if !hydrated_message_ids.insert(message_id.clone()) {
                continue;
            }
            let key = crate::control::keys::session_message(ns, agent, session_id, message_id);
            let bytes =
                cp.kv.get(&key).await?.ok_or_else(|| {
                    anyhow!("STEER_INPUT references missing message '{message_id}'")
                })?;
            let message = data_proto::SessionMessage::decode(bytes.as_slice())?;
            runtime.context.push(LoopMessage::text(
                "user",
                crate::control::scheduling::session_message_text_projection(&message),
            ));
        }
        Ok(())
    }

    let mut latest_final_response = None;
    let mut projection_parts = Vec::new();
    let mut next_projection_part_index = 0usize;
    let mut hydrated_steering_message_ids = HashSet::new();
    let mut latest_appended_journal_entry_id = None;
    let mut index = 0;
    while index < journal_entries.len() {
        let entry = &journal_entries[index];

        // Hydrate the immutable summary. The journal payload intentionally has
        // no tail anchor yet, so this in-flight recovery path restores only the
        // summary. Exact tail reconstruction for this path requires the
        // follow-up journal-anchor work.
        if entry.phase == SessionExecutionPhase::Compaction as i32 {
            if let Some(session_journal_entry_payload::Payload::Compaction(payl)) =
                entry.payload.as_ref().and_then(|p| p.payload.as_ref())
            {
                let summary = payl
                    .summary
                    .as_ref()
                    .ok_or_else(|| anyhow!("COMPACTION entry is missing summary object"))?;
                let summary = CasStore::new(cp.objects.clone())
                    .get_session_object_by_key_decoded(&summary.key)
                    .await?
                    .ok_or_else(|| anyhow!("COMPACTION summary object is missing"))?;
                let summary = String::from_utf8(summary.1.bytes)
                    .map_err(|_| anyhow!("COMPACTION summary is not UTF-8"))?;

                tracing::info!(
                    submission = %submission_id,
                    "Recovery: compaction boundary hydrated",
                );

                runtime.context.history.clear();
                projection_parts.clear();
                next_projection_part_index = 0;
                runtime
                    .context
                    .push(LoopMessage::text("assistant", summary));
                latest_final_response = None;
            } else {
                return Err(anyhow!("COMPACTION entry is missing payload"));
            }

            index += 1;
            continue;
        }

        if entry.phase == SessionExecutionPhase::SteerInput as i32 {
            hydrate_steering_input(
                cp,
                ns,
                agent,
                session_id,
                entry,
                &mut hydrated_steering_message_ids,
                runtime,
            )
            .await?;
            index += 1;
            continue;
        }

        let response = match (
            entry.phase,
            entry
                .payload
                .as_ref()
                .and_then(|payload| payload.payload.as_ref()),
        ) {
            (phase, Some(session_journal_entry_payload::Payload::LlmResponse(payload)))
                if phase == SessionExecutionPhase::LlmResponse as i32 =>
            {
                payload
                    .response
                    .clone()
                    .ok_or_else(|| anyhow!("LLM_RESPONSE entry is missing response"))?
            }
            (phase, Some(_)) if phase == SessionExecutionPhase::LlmResponse as i32 => {
                return Err(anyhow!("LLM_RESPONSE entry has non-LLM payload"));
            }
            (phase, None) if phase == SessionExecutionPhase::LlmResponse as i32 => {
                return Err(anyhow!("LLM_RESPONSE entry is missing payload"));
            }
            (phase, Some(session_journal_entry_payload::Payload::ToolResult(result)))
                if phase == SessionExecutionPhase::ToolResult as i32 =>
            {
                return Err(anyhow!(
                    "TOOL_RESULT references unknown tool call '{}'",
                    result.tool_call_id
                ));
            }
            (phase, Some(_)) if phase == SessionExecutionPhase::ToolResult as i32 => {
                return Err(anyhow!("TOOL_RESULT entry has non-tool-result payload"));
            }
            (phase, None) if phase == SessionExecutionPhase::ToolResult as i32 => {
                return Err(anyhow!("TOOL_RESULT entry is missing payload"));
            }
            _ => {
                tracing::warn!(
                    journal_entry_id = %entry.journal_entry_id,
                    "Unreachable: ignored unexpected journal phase during hydration",
                );
                index += 1;
                continue;
            }
        };
        if response.tool_calls.is_empty() {
            latest_final_response = Some(response);
            index += 1;
            continue;
        }

        latest_final_response = None;
        let tool_calls = response.tool_calls.clone();
        let mut assistant_message = LoopMessage::text("assistant", response.content.clone());
        assistant_message.tool_calls = Some(tool_calls.clone());
        assistant_message.encrypted_reasoning = response.encrypted_reasoning.clone();
        runtime.context.push(assistant_message);
        if !response.content.is_empty() {
            let part_id = next_recovered_part_id(&mut next_projection_part_index);
            projection_parts.push(RecoveredProjectionPart::Text {
                part_id,
                content: response.content.clone(),
            });
        }
        if let Some(object) = response.encrypted_reasoning.clone() {
            projection_parts.push(RecoveredProjectionPart::EncryptedReasoning {
                part_id: next_recovered_part_id(&mut next_projection_part_index),
                object,
            });
        }

        index += 1;
        let mut stop_after_tool_results = false;
        let mut results_by_call_id = BTreeMap::new();
        let mut steering_entries = Vec::new();
        while index < journal_entries.len() {
            let entry = &journal_entries[index];
            if entry.phase == SessionExecutionPhase::LlmResponse as i32
                || entry.phase == SessionExecutionPhase::Committed as i32
                || entry.phase == SessionExecutionPhase::Compaction as i32
            {
                break;
            }
            match (
                entry.phase,
                entry
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.payload.as_ref()),
            ) {
                (phase, Some(session_journal_entry_payload::Payload::ToolResult(result)))
                    if phase == SessionExecutionPhase::ToolResult as i32 =>
                {
                    if !tool_calls.iter().any(|tool| tool.id == result.tool_call_id) {
                        return Err(anyhow!(
                            "TOOL_RESULT references unknown tool call '{}'",
                            result.tool_call_id
                        ));
                    }
                    results_by_call_id
                        .entry(result.tool_call_id.clone())
                        .or_insert_with(|| result.clone());
                }
                (phase, Some(_)) if phase == SessionExecutionPhase::ToolResult as i32 => {
                    return Err(anyhow!("TOOL_RESULT entry has non-tool-result payload"));
                }
                (phase, None) if phase == SessionExecutionPhase::ToolResult as i32 => {
                    return Err(anyhow!("TOOL_RESULT entry is missing payload"));
                }
                (phase, Some(session_journal_entry_payload::Payload::SteerInput(_)))
                    if phase == SessionExecutionPhase::SteerInput as i32 =>
                {
                    steering_entries.push(entry.clone());
                }
                _ => {}
            }
            index += 1;
        }

        for tool in &tool_calls {
            let input_json: Value = serde_json::from_str(&tool.arguments).unwrap_or(Value::Null);
            let tool_call_part_id = next_recovered_part_id(&mut next_projection_part_index);
            projection_parts.push(RecoveredProjectionPart::ToolCall {
                part_id: tool_call_part_id,
                id: tool.id.clone(),
                name: tool.name.clone(),
                input: input_json,
            });
            let tool_result_part_id = next_recovered_part_id(&mut next_projection_part_index);

            let result_output = if let Some(recorded) = results_by_call_id.get(&tool.id) {
                if let Some(output) = recorded.tool_output.clone() {
                    output
                } else if let Some(object) = recorded.object.as_ref() {
                    tool_output_from_recorded_object(cp, object).await?
                } else {
                    ToolOutput::text(recorded.output.clone())
                }
            } else {
                let executed = runtime.executor.execute_tool_call_result(tool).await;
                let result_output = executed.result;
                let cas = CasStore::new(cp.objects.clone());
                let entry = sessions::append_tool_result(
                    cp.kv.as_ref(),
                    &cas,
                    ns,
                    agent,
                    session_id,
                    message_id,
                    &tool_result_part_id,
                    submission_id,
                    attempt_id,
                    &tool.id,
                    &tool.name,
                    &result_output,
                    chrono::Utc::now().timestamp_micros(),
                )
                .await?;
                latest_appended_journal_entry_id = Some(entry.journal_entry_id.clone());
                entry
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.payload.as_ref())
                    .and_then(|payload| match payload {
                        session_journal_entry_payload::Payload::ToolResult(result) => {
                            result.tool_output.clone()
                        }
                        _ => None,
                    })
                    .unwrap_or(result_output)
            };
            let result = result_output.summary();

            projection_parts.push(RecoveredProjectionPart::ToolResult {
                part_id: tool_result_part_id,
                id: tool.id.clone(),
                name: tool.name.clone(),
                result: result.clone(),
            });
            runtime
                .context
                .push(recovered_tool_output_loop_message(cp, &tool.id, &result_output).await);
            stop_after_tool_results |=
                crate::harness::native_tools::tool_requests_worker_stop(&tool.name);
        }
        for entry in steering_entries {
            hydrate_steering_input(
                cp,
                ns,
                agent,
                session_id,
                &entry,
                &mut hydrated_steering_message_ids,
                runtime,
            )
            .await?;
        }
        if stop_after_tool_results {
            return Ok(PreparedSubmission {
                state: PreparedSubmissionState::StopAfterToolResult,
                projection_parts,
                latest_appended_journal_entry_id,
            });
        }
    }

    if let Some(response) = latest_final_response {
        return Ok(PreparedSubmission {
            state: PreparedSubmissionState::FinalResponseReady {
                content: response.content,
                encrypted_reasoning: response.encrypted_reasoning,
            },
            projection_parts,
            latest_appended_journal_entry_id,
        });
    }

    Ok(PreparedSubmission {
        state: PreparedSubmissionState::ContinueExecution,
        projection_parts,
        latest_appended_journal_entry_id,
    })
}

async fn recovered_tool_output_loop_message(
    cp: &ControlPlane,
    tool_call_id: &str,
    output: &ToolOutput,
) -> LoopMessage {
    let Some(descriptor) = output.content_descriptor.as_ref() else {
        return tool_output_loop_message(tool_call_id, output);
    };
    let selection = descriptor.selection.as_ref();
    let byte_range = descriptor.byte_range.as_ref();
    if selection.is_none() && byte_range.is_none() {
        return tool_output_loop_message(tool_call_id, output);
    }
    let Some(object_ref) = output.object_ref() else {
        return tool_output_loop_message(tool_call_id, output);
    };
    let cas = CasStore::new(cp.objects.clone());
    let content = match (byte_range, selection) {
        (Some(range), _) => match cas.get_text_range_decoded(&object_ref.key, range.start, range.end).await {
            Ok(Some(bytes)) => String::from_utf8(bytes).unwrap_or_else(|error| {
                tracing::warn!(%error, object_key = %object_ref.key, tool_call_id, "recovered tool result range was not valid UTF-8; replaying summary only");
                output.summary()
            }),
            Ok(None) => { tracing::warn!(object_key = %object_ref.key, tool_call_id, "recovered tool result object is missing; replaying summary only"); output.summary() }
            Err(error) => { tracing::warn!(%error, object_key = %object_ref.key, tool_call_id, "failed to read recovered tool result object; replaying summary only"); output.summary() }
        },
        (_, Some(selection)) => match cas.get_object_decoded(&object_ref.key).await {
            Ok(Some(object)) => bounded_recovered_line_selection(&String::from_utf8_lossy(&object.bytes), selection.start_line, selection.end_line),
            Ok(None) => { tracing::warn!(object_key = %object_ref.key, tool_call_id, "recovered tool result object is missing; replaying summary only"); output.summary() }
            Err(error) => { tracing::warn!(%error, object_key = %object_ref.key, tool_call_id, "failed to read recovered tool result object; replaying summary only"); output.summary() }
        },
        _ => output.summary(),
    };
    let mut message = LoopMessage::text("tool", content);
    message.tool_call_id = Some(tool_call_id.to_string());
    message
}

fn bounded_recovered_line_selection(text: &str, start_line: u64, end_line: u64) -> String {
    const MAX_BYTES: usize = crate::control::tool_output::TOOL_RESULT_INLINE_CONTEXT_BYTES;
    let mut output = text
        .lines()
        .enumerate()
        .filter(|(index, _)| {
            let line = *index as u64 + 1;
            line >= start_line && line <= end_line
        })
        .map(|(_, line)| line)
        .collect::<Vec<_>>()
        .join("\n");
    if output.len() > MAX_BYTES {
        let mut end = MAX_BYTES;
        while end > 0 && !output.is_char_boundary(end) {
            end -= 1;
        }
        output.truncate(end);
        output.push_str("\n...[SECTION TRUNCATED]");
    }
    output
}

fn next_recovered_part_id(next_projection_part_index: &mut usize) -> String {
    *next_projection_part_index += 1;
    format!("{:06}", *next_projection_part_index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::session_queue::{self, STEER_QUEUE};
    use crate::control::{KeyValueStore, ProtoKeyValueStoreExt};
    use crate::gateway::rpc::data_proto::SessionSubmissionStatus;
    use crate::harness::sessions;
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use chrono::Utc;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn llm_entry(assistant_message_id: &str) -> sessions::SessionJournalEntry {
        sessions::SessionJournalEntry {
            phase: SessionExecutionPhase::LlmResponse as i32,
            payload: Some(data_proto::SessionJournalEntryPayload {
                payload: Some(session_journal_entry_payload::Payload::LlmResponse(
                    data_proto::SessionJournalEntryPayloadLlmResponse {
                        response: Some(crate::harness::llm::ChatResponse::default()),
                        assistant_message_id: assistant_message_id.to_string(),
                    },
                )),
            }),
            ..Default::default()
        }
    }

    fn steer_entry(next_assistant_message_id: &str) -> sessions::SessionJournalEntry {
        sessions::SessionJournalEntry {
            phase: SessionExecutionPhase::SteerInput as i32,
            payload: Some(data_proto::SessionJournalEntryPayload {
                payload: Some(session_journal_entry_payload::Payload::SteerInput(
                    data_proto::SessionJournalEntryPayloadSteerInput {
                        next_assistant_message_id: next_assistant_message_id.to_string(),
                        ..Default::default()
                    },
                )),
            }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn latest_journaled_assistant_or_continuation_id_wins() {
        let cp = crate::control::ControlPlane::builder(
            Arc::new(MockKvStore::default()),
            Arc::new(RecordingPubSub::default()),
        )
        .build();
        let entries = vec![llm_entry("assistant-1"), steer_entry("assistant-2")];

        assert_eq!(
            resolve_active_assistant_message_id(
                &cp,
                "ns",
                "agent",
                "session-1",
                "submission-1",
                &entries,
            )
            .await
            .unwrap(),
            Some("assistant-2".to_string())
        );
    }

    #[tokio::test]
    async fn uncommitted_submission_projection_is_the_id_fallback() {
        let kv = Arc::new(MockKvStore::default());
        let cp =
            crate::control::ControlPlane::builder(kv.clone(), Arc::new(RecordingPubSub::default()))
                .build();
        kv.set_msg(
            &crate::control::keys::session_message("ns", "agent", "session-1", "projection-1"),
            &data_proto::SessionMessage {
                id: "projection-1".to_string(),
                role: data_proto::MessageRole::RoleAssistant as i32,
                labels: HashMap::from([
                    (
                        sessions::SESSION_LABEL_SUBMISSION_ID.to_string(),
                        "submission-1".to_string(),
                    ),
                    (
                        sessions::SESSION_LABEL_PROJECTION_STATE.to_string(),
                        sessions::SESSION_PROJECTION_STATE_IN_PROGRESS.to_string(),
                    ),
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(
            resolve_active_assistant_message_id(
                &cp,
                "ns",
                "agent",
                "session-1",
                "submission-1",
                &[],
            )
            .await
            .unwrap(),
            Some("projection-1".to_string())
        );
    }

    #[tokio::test]
    async fn recovery_reconciles_journaled_and_prepared_steer_queue_entries() {
        let kv = Arc::new(MockKvStore::default());
        let cp =
            crate::control::ControlPlane::builder(kv.clone(), Arc::new(RecordingPubSub::default()))
                .build();
        let mut submission = sessions::pending_submission("submission-1", "session-1", "user-1", 1);
        submission.status = SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        sessions::create_submission_if_absent(&*kv, "ns", "agent", "session-1", &submission)
            .await
            .unwrap();

        let first = session_queue::queue_text_message(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            STEER_QUEUE,
            "journaled steer",
            HashMap::new(),
            Utc::now(),
        )
        .await
        .unwrap();
        let prepared = session_queue::prepare_steer_batch(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            4,
            10_000,
            Utc::now(),
        )
        .await
        .unwrap();
        let first_message_id = prepared[0].message_id.clone();
        sessions::append_steer_input(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            &[first_message_id],
            "assistant-1",
            "assistant-2",
            2,
        )
        .await
        .unwrap();

        let second = session_queue::queue_text_message(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            STEER_QUEUE,
            "prepared but not journaled",
            HashMap::new(),
            Utc::now(),
        )
        .await
        .unwrap();
        let prepared = session_queue::prepare_steer_batch(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            4,
            10_000,
            Utc::now(),
        )
        .await
        .unwrap();
        let second_message_id = prepared
            .iter()
            .find(|message| message.entry_id == second.entry_id)
            .unwrap()
            .message_id
            .clone();
        let entries =
            sessions::list_journal_entries(&*kv, "ns", "agent", "session-1", "submission-1")
                .await
                .unwrap();

        let plan = prepare_recovery_plan(&cp, "ns", "agent", "session-1", "submission-1", &entries)
            .await
            .unwrap();

        assert!(kv
            .get(&crate::control::keys::session_queue_entry(
                "ns",
                "agent",
                "session-1",
                STEER_QUEUE,
                &first.entry_id,
            ))
            .await
            .unwrap()
            .is_none());
        assert!(kv
            .get(&crate::control::keys::session_queue_entry(
                "ns",
                "agent",
                "session-1",
                STEER_QUEUE,
                &second.entry_id,
            ))
            .await
            .unwrap()
            .is_some());
        assert!(plan
            .excluded_history_message_ids
            .contains(&second_message_id));
    }

    #[tokio::test]
    async fn recorded_image_object_recovery_preserves_object_ref() {
        use crate::control::object_store::ObjectMetadata;

        let cp = crate::control::ControlPlane::builder(
            Arc::new(MockKvStore::default()),
            Arc::new(RecordingPubSub::default()),
        )
        .build();
        let object = cp
            .objects
            .put(
                "cas/conic%3Atest/sessions/session-1/messages/message-1/screenshot.png",
                b"png-bytes",
                ObjectMetadata::default(),
            )
            .await
            .unwrap();

        let output = tool_output_from_recorded_object(&cp, &object)
            .await
            .unwrap();

        assert_eq!(output.object_ref().unwrap().key, object.key);
        let parts = output.content_parts();
        assert_eq!(parts.len(), 1);
        assert_eq!(
            crate::harness::llm::content_part_object_ref(&parts[0])
                .unwrap()
                .media_type,
            "image/png"
        );
    }

    #[tokio::test]
    async fn prepare_context_hydrates_compaction_summary_without_tail_replay() {
        use crate::control::cas::CasStore;
        use crate::gateway::rpc::data_proto::{
            SessionExecutionPhase as DataPhase, SessionSubmissionStatus,
        };
        use crate::harness::executor::{AgentExecutor, ContextAssembler, ExecutionContext};
        use crate::harness::llm::{ChatResponse, MockLlmProvider, ToolCall};
        use crate::harness::skills::registry::ToolRegistry;
        use crate::worker::runtime::AgentRuntime;

        let kv = Arc::new(MockKvStore::default());
        let cp =
            crate::control::ControlPlane::builder(kv.clone(), Arc::new(RecordingPubSub::default()))
                .build();

        let mut submission = sessions::pending_submission("submission-1", "session-1", "user-1", 1);
        submission.status = SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        sessions::create_submission_if_absent(&*kv, "ns", "agent", "session-1", &submission)
            .await
            .unwrap();

        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let mut runtime = AgentRuntime {
            executor: AgentExecutor::new_with_session(
                Arc::new(MockLlmProvider),
                "test-provider".to_string(),
                "test-model".to_string(),
                ContextAssembler::new("."),
                registry,
                Arc::new(crate::control::config::Config::default()),
                "ns".to_string(),
                "agent".to_string(),
                "session-1".to_string(),
                None,
                cp.clone(),
                crate::gateway::rpc::manifests::AgentSpec::default(),
                HashMap::new(),
            ),
            context: ExecutionContext::new("agent"),
        };

        let call_response = ChatResponse {
            content: "I will look that up.".to_string(),
            tool_calls: vec![ToolCall {
                id: "tool-1".to_string(),
                name: "lookup".to_string(),
                arguments: "{\"query\":\"value\"}".to_string(),
            }],
            usage: None,
            encrypted_reasoning: None,
        };
        let reasoning = data_proto::ObjectRef {
            key: "cas/ns/sessions/session-1/encrypted-reasoning.bin".to_string(),
            media_type: "application/octet-stream".to_string(),
            size_bytes: 42,
            ..Default::default()
        };
        sessions::append_llm_response(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            "reply-1",
            &call_response,
            20,
        )
        .await
        .unwrap();
        sessions::append_tool_result(
            kv.as_ref(),
            &CasStore::new(cp.objects.clone()),
            "ns",
            "agent",
            "session-1",
            "reply-1",
            "part-1",
            "submission-1",
            "attempt-1",
            "tool-1",
            "lookup",
            &crate::harness::llm::ToolOutput::text("lookup result"),
            30,
        )
        .await
        .unwrap();
        let _ = sessions::append_compaction(
            kv.as_ref(),
            &CasStore::new(cp.objects.clone()),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            "# Compacted context\n\nThe user asked for a lookup.",
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
            "reply-1",
            &ChatResponse {
                content: "continued after recovery".to_string(),
                tool_calls: Vec::new(),
                usage: None,
                encrypted_reasoning: Some(reasoning.clone()),
            },
            50,
        )
        .await
        .unwrap();

        let entries =
            sessions::list_journal_entries(&*kv, "ns", "agent", "session-1", "submission-1")
                .await
                .unwrap();
        let journal_shape = entries
            .iter()
            .map(|entry| (entry.journal_entry_id.as_str(), entry.phase))
            .collect::<Vec<_>>();
        assert_eq!(
            journal_shape,
            vec![
                ("000001", DataPhase::LlmResponse as i32),
                ("000002", DataPhase::ToolResult as i32),
                ("000003", DataPhase::Compaction as i32),
                ("000004", DataPhase::LlmResponse as i32)
            ]
        );

        let prepared = replay_claimed_submission_journal(
            &cp,
            "ns",
            "agent",
            "session-1",
            "reply-1",
            "submission-1",
            "attempt-1",
            &entries,
            &mut runtime,
        )
        .await
        .unwrap();

        assert_eq!(
            prepared.state,
            PreparedSubmissionState::FinalResponseReady {
                content: "continued after recovery".to_string(),
                encrypted_reasoning: Some(reasoning),
            }
        );
        assert_eq!(runtime.context.history.len(), 1);
        assert_eq!(runtime.context.history[0].role, "assistant");
        assert_eq!(
            runtime.context.history[0].text_content(),
            "# Compacted context\n\nThe user asked for a lookup."
        );
    }

    #[tokio::test]
    async fn prepare_context_preserves_projection_parts_for_legacy_steer_input() {
        use crate::control::cas::CasStore;
        use crate::gateway::rpc::data_proto::{
            SessionExecutionPhase as DataPhase, SessionSubmissionStatus,
        };
        use crate::harness::executor::{AgentExecutor, ContextAssembler, ExecutionContext};
        use crate::harness::llm::{ChatResponse, MockLlmProvider, ToolCall, ToolOutput};
        use crate::harness::skills::registry::ToolRegistry;
        use crate::worker::runtime::AgentRuntime;

        let kv = Arc::new(MockKvStore::default());
        let cp =
            crate::control::ControlPlane::builder(kv.clone(), Arc::new(RecordingPubSub::default()))
                .build();
        let mut submission = sessions::pending_submission("submission-1", "session-1", "user-1", 1);
        submission.status = SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        sessions::create_submission_if_absent(&*kv, "ns", "agent", "session-1", &submission)
            .await
            .unwrap();

        kv.set_msg(
            &crate::control::keys::session_message("ns", "agent", "session-1", "user-steer"),
            &data_proto::SessionMessage {
                id: "user-steer".to_string(),
                role: data_proto::MessageRole::RoleUser as i32,
                created_at: 15,
                labels: HashMap::new(),
                parts: vec![data_proto::SessionMessagePart {
                    id: "000000".to_string(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: "continue with this extra request".to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                    created_at: 15,
                    object: None,
                }],
            },
        )
        .await
        .unwrap();

        let mut runtime = AgentRuntime {
            executor: AgentExecutor::new_with_session(
                Arc::new(MockLlmProvider),
                "test-provider".to_string(),
                "test-model".to_string(),
                ContextAssembler::new("."),
                Arc::new(tokio::sync::RwLock::new(ToolRegistry::new())),
                Arc::new(crate::control::config::Config::default()),
                "ns".to_string(),
                "agent".to_string(),
                "session-1".to_string(),
                None,
                cp.clone(),
                crate::gateway::rpc::manifests::AgentSpec::default(),
                HashMap::new(),
            ),
            context: ExecutionContext::new("agent"),
        };
        let response = ChatResponse {
            content: "I will look that up.".to_string(),
            tool_calls: vec![ToolCall {
                id: "tool-1".to_string(),
                name: "lookup".to_string(),
                arguments: "{\"query\":\"value\"}".to_string(),
            }],
            usage: None,
            encrypted_reasoning: None,
        };
        sessions::append_llm_response(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            "reply-1",
            &response,
            20,
        )
        .await
        .unwrap();
        sessions::append_tool_result(
            kv.as_ref(),
            &CasStore::new(cp.objects.clone()),
            "ns",
            "agent",
            "session-1",
            "reply-1",
            "part-1",
            "submission-1",
            "attempt-1",
            "tool-1",
            "lookup",
            &ToolOutput::text("lookup result"),
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
            &["user-steer".to_string()],
            "",
            "",
            40,
        )
        .await
        .unwrap();

        let entries =
            sessions::list_journal_entries(&*kv, "ns", "agent", "session-1", "submission-1")
                .await
                .unwrap();
        assert_eq!(entries[2].phase, DataPhase::SteerInput as i32);
        let prepared = replay_claimed_submission_journal(
            &cp,
            "ns",
            "agent",
            "session-1",
            "reply-1",
            "submission-1",
            "attempt-1",
            &entries,
            &mut runtime,
        )
        .await
        .unwrap();

        assert_eq!(prepared.projection_parts.len(), 3);
        assert!(matches!(
            prepared.projection_parts[0],
            RecoveredProjectionPart::Text { .. }
        ));
        assert!(matches!(
            prepared.projection_parts[1],
            RecoveredProjectionPart::ToolCall { .. }
        ));
        assert!(matches!(
            prepared.projection_parts[2],
            RecoveredProjectionPart::ToolResult { .. }
        ));
        assert_eq!(
            runtime.context.history.last().unwrap().text_content(),
            "continue with this extra request"
        );
    }

    #[tokio::test]
    async fn recovery_reports_the_watermark_of_a_recreated_tool_result() {
        use crate::gateway::rpc::data_proto::SessionSubmissionStatus;
        use crate::harness::executor::{AgentExecutor, ContextAssembler, ExecutionContext};
        use crate::harness::llm::{ChatResponse, MockLlmProvider, ToolCall};
        use crate::harness::skills::registry::ToolRegistry;
        use crate::worker::runtime::AgentRuntime;

        let kv = Arc::new(MockKvStore::default());
        let cp =
            crate::control::ControlPlane::builder(kv.clone(), Arc::new(RecordingPubSub::default()))
                .build();
        let mut submission = sessions::pending_submission("submission-1", "session-1", "user-1", 1);
        submission.status = SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        sessions::create_submission_if_absent(&*kv, "ns", "agent", "session-1", &submission)
            .await
            .unwrap();
        sessions::append_llm_response(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
            "reply-1",
            &ChatResponse {
                content: "calling a tool".to_string(),
                tool_calls: vec![ToolCall {
                    id: "tool-1".to_string(),
                    name: "missing-tool".to_string(),
                    arguments: "{}".to_string(),
                }],
                usage: None,
                encrypted_reasoning: None,
            },
            10,
        )
        .await
        .unwrap();
        let entries =
            sessions::list_journal_entries(&*kv, "ns", "agent", "session-1", "submission-1")
                .await
                .unwrap();
        let mut runtime = AgentRuntime {
            executor: AgentExecutor::new_with_session(
                Arc::new(MockLlmProvider),
                "test-provider".to_string(),
                "test-model".to_string(),
                ContextAssembler::new("."),
                Arc::new(tokio::sync::RwLock::new(ToolRegistry::new())),
                Arc::new(crate::control::config::Config::default()),
                "ns".to_string(),
                "agent".to_string(),
                "session-1".to_string(),
                None,
                cp.clone(),
                crate::gateway::rpc::manifests::AgentSpec::default(),
                HashMap::new(),
            ),
            context: ExecutionContext::new("agent"),
        };

        let prepared = replay_claimed_submission_journal(
            &cp,
            "ns",
            "agent",
            "session-1",
            "reply-1",
            "submission-1",
            "attempt-1",
            &entries,
            &mut runtime,
        )
        .await
        .unwrap();
        let recovered_entries =
            sessions::list_journal_entries(&*kv, "ns", "agent", "session-1", "submission-1")
                .await
                .unwrap();

        assert_eq!(recovered_entries.len(), 2);
        assert_eq!(prepared.state, PreparedSubmissionState::ContinueExecution);
        assert_eq!(
            prepared.latest_appended_journal_entry_id.as_deref(),
            Some(recovered_entries[1].journal_entry_id.as_str())
        );
        assert_eq!(
            recovered_entries[1].phase,
            data_proto::SessionExecutionPhase::ToolResult as i32
        );
    }
}
