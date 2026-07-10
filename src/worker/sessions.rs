// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use futures::FutureExt;
use prost::Message;
use serde_json::Value;
use std::collections::BTreeMap;
use std::panic::AssertUnwindSafe;

use super::runtime::AgentRuntime;
use super::sink::PubSubSessionSink;
use super::WorkerEventHandler;
use crate::control::{events::SessionMessageEvent, ControlPlane, ProtoKeyValueStoreExt};
use crate::gateway::rpc::connectors as connector_rpc;
use crate::gateway::rpc::data_proto::{
    self, session_journal_entry_payload, SessionExecutionPhase, SessionSubmissionStatus,
};
use crate::harness::executor::{tool_result_loop_message, ExecutionSink, LoopMessage};
use crate::harness::sessions::{self, ClaimOutcome};
use crate::harness::tool_results::hydrate_tool_result;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

const MAX_SESSION_RELEASE_CAS_RETRIES: usize = 8;
const SESSION_RELEASE_CAS_BACKOFF_MS: u64 = 10;
const DEFAULT_FANOUT_SUBSCRIBER_GRACE_MS: u64 = 100;
const LABEL_MESSAGE_SOURCE: &str = "talon.impalasys.com/message-source";
const LABEL_CONNECTOR_REGISTRATION: &str = "talon.impalasys.com/connector-registration";
const LABEL_CHANNEL_TRIGGER: &str = "talon.impalasys.com/channel-trigger";

fn fanout_subscriber_grace() -> std::time::Duration {
    let millis = match std::env::var("TALON_WORKER_FANOUT_SUBSCRIBER_GRACE_MS") {
        Ok(raw) => match raw.trim().parse::<u64>() {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(
                    value = %raw,
                    error = %error,
                    default_ms = DEFAULT_FANOUT_SUBSCRIBER_GRACE_MS,
                    "Ignoring invalid TALON_WORKER_FANOUT_SUBSCRIBER_GRACE_MS"
                );
                DEFAULT_FANOUT_SUBSCRIBER_GRACE_MS
            }
        },
        Err(_) => DEFAULT_FANOUT_SUBSCRIBER_GRACE_MS,
    };
    std::time::Duration::from_millis(millis)
}

async fn execute_with_panic_boundary<F>(
    future: F,
    sink: &dyn ExecutionSink,
    agent: &str,
    session_id: &str,
) -> Result<SessionCompletionStatus>
where
    F: std::future::Future<Output = Result<String>>,
{
    match AssertUnwindSafe(future).catch_unwind().await {
        Ok(Ok(_)) => Ok(SessionCompletionStatus::Completed),
        Ok(Err(e)) => {
            tracing::error!(agent = %agent, error = %format!("{:#}", e), "Execution failed");
            sink.on_error(&format!("Error: {:#}", e)).await;
            Ok(SessionCompletionStatus::Errored)
        }
        Err(panic) => {
            let panic_msg = if let Some(msg) = panic.downcast_ref::<&str>() {
                (*msg).to_string()
            } else if let Some(msg) = panic.downcast_ref::<String>() {
                msg.clone()
            } else {
                "unknown panic".to_string()
            };
            tracing::error!(
                agent = %agent,
                session = %session_id,
                "Execution panicked: {}",
                panic_msg
            );
            sink.on_error(&format!("Error: execution panicked: {}", panic_msg))
                .await;
            Ok(SessionCompletionStatus::Panicked)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionCompletionStatus {
    Completed,
    Errored,
    Panicked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreparedSubmissionState {
    ContinueExecution,
    FinalResponseReady { content: String },
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
}

#[derive(Debug, Clone, PartialEq)]
struct PreparedSubmission {
    state: PreparedSubmissionState,
    projection_parts: Vec<RecoveredProjectionPart>,
}

async fn prepare_context_for_claimed_submission(
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
    let mut latest_final_response = None;
    let mut projection_parts = Vec::new();
    let mut next_projection_part_index = 0usize;
    let mut index = 0;
    while index < journal_entries.len() {
        let entry = &journal_entries[index];
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
                index += 1;
                continue;
            }
        };

        index += 1;
        if response.tool_calls.is_empty() {
            latest_final_response = Some(response);
            continue;
        }

        latest_final_response = None;
        let tool_calls = response.tool_calls.clone();
        let mut assistant_message = LoopMessage::text("assistant", response.content.clone());
        assistant_message.tool_calls = Some(tool_calls.clone());
        runtime.context.push(assistant_message);
        if !response.content.is_empty() {
            let part_id = next_recovered_part_id(&mut next_projection_part_index);
            projection_parts.push(RecoveredProjectionPart::Text {
                part_id,
                content: response.content.clone(),
            });
        }

        let mut results_by_call_id = BTreeMap::new();
        while index < journal_entries.len() {
            let entry = &journal_entries[index];
            if entry.phase == SessionExecutionPhase::LlmResponse as i32
                || entry.phase == SessionExecutionPhase::Committed as i32
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

            let result = if let Some(recorded) = results_by_call_id.get(&tool.id) {
                hydrate_tool_result(
                    cp.objects.as_ref(),
                    recorded.object.as_ref(),
                    &recorded.output,
                )
                .await?
            } else {
                let (_input, result) = runtime.executor.execute_tool_call(tool).await;
                sessions::append_tool_result(
                    cp.kv.as_ref(),
                    cp.objects.as_ref(),
                    ns,
                    agent,
                    session_id,
                    message_id,
                    &tool_result_part_id,
                    submission_id,
                    attempt_id,
                    &tool.id,
                    &tool.name,
                    &result,
                    chrono::Utc::now().timestamp_micros(),
                )
                .await?;
                result
            };

            projection_parts.push(RecoveredProjectionPart::ToolResult {
                part_id: tool_result_part_id,
                id: tool.id.clone(),
                name: tool.name.clone(),
                result: result.clone(),
            });
            runtime
                .context
                .push(tool_result_loop_message(&tool.id, &result));
        }
    }

    if let Some(response) = latest_final_response {
        return Ok(PreparedSubmission {
            state: PreparedSubmissionState::FinalResponseReady {
                content: response.content,
            },
            projection_parts,
        });
    }

    Ok(PreparedSubmission {
        state: PreparedSubmissionState::ContinueExecution,
        projection_parts,
    })
}

impl WorkerEventHandler {
    #[tracing::instrument(
        name = "WorkerEventHandler.handle_session_message",
        skip_all,
        fields(
            namespace = %event.ns,
            agent = %event.agent,
            session = %event.session_id,
            message_chars = event.message.len(),
        )
    )]
    pub async fn handle_session_message(&self, event: SessionMessageEvent) -> Result<()> {
        tracing::info!(
            agent = %event.agent,
            session = %event.session_id,
            "Handling session message"
        );

        let ns = &event.ns;
        let now_micros = chrono::Utc::now().timestamp_micros();

        // Claim the durable submission. This is the idempotency boundary for
        // one accepted user message, and it fences later journal/projection
        // writes with a fresh attempt id.
        let submission_id = if event.submission_id.is_empty() {
            event.message_id.as_str()
        } else {
            event.submission_id.as_str()
        };
        let claim = sessions::claim_submission(
            self.cp.kv.as_ref(),
            ns,
            &event.agent,
            &event.session_id,
            submission_id,
            &event.message_id,
            &self.worker_id,
            now_micros,
            crate::control::scheduling::session_processing_timeout_micros(),
        )
        .instrument(tracing::info_span!(
            "WorkerEventHandler.claim_session_submission"
        ))
        .await?;
        let submission = match claim {
            ClaimOutcome::Claimed(submission) => submission,
            ClaimOutcome::AlreadyTerminal(submission) => {
                tracing::info!(
                    agent = %event.agent,
                    session = %event.session_id,
                    submission = %submission.submission_id,
                    status = %submission.status,
                    committed_message_id = ?submission.committed_message_id,
                    "Session submission already terminal; skipping duplicate delivery"
                );
                if submission.status == SessionSubmissionStatus::Committed as i32 {
                    if let Some(committed_message_id) = submission.committed_message_id.as_deref() {
                        if let Err(err) = self
                            .maybe_deliver_connector_session_reply(
                                ns,
                                &event.agent,
                                &event.session_id,
                                committed_message_id,
                            )
                            .await
                        {
                            tracing::warn!(
                                error = %err,
                                agent = %event.agent,
                                session = %event.session_id,
                                message_id = %committed_message_id,
                                "failed to deliver already-committed connector session reply"
                            );
                        }
                    }
                }
                self.release_session_lock(
                    ns,
                    &event.agent,
                    &event.session_id,
                    event.timestamp,
                    SessionCompletionStatus::Completed,
                )
                .await;
                return Ok(());
            }
            ClaimOutcome::Busy(submission) => {
                tracing::info!(
                    agent = %event.agent,
                    session = %event.session_id,
                    submission = %submission.submission_id,
                    claim_expires_at = ?submission.claim_expires_at,
                    "Session submission already claimed; skipping concurrent duplicate delivery"
                );
                return Ok(());
            }
        };

        // Keep this claimed submission and its user-visible session lock alive
        // while the attempt is executing.
        let lease_renewal = sessions::SubmissionLeaseRenewer::start(
            self.cp.kv.clone(),
            sessions::SubmissionLease {
                ns: ns.to_string(),
                agent: event.agent.clone(),
                session_id: event.session_id.clone(),
                submission_id: submission.submission_id.clone(),
                attempt_id: submission.attempt_id.clone(),
                ttl_micros: crate::control::scheduling::session_processing_timeout_micros(),
            },
            event.timestamp,
        );
        let cancellation_token = CancellationToken::new();
        self.session_cancellations
            .lock()
            .await
            .insert(event.session_id.clone(), cancellation_token.clone());
        let reply_msg_id = crate::control::uuid::session_message_id();
        let reply_msg_key = crate::control::keys::session_message(
            ns,
            &event.agent,
            &event.session_id,
            &reply_msg_id,
        );
        let fanout_key = crate::worker::fanout::SessionFanoutKey::new(
            event.ns.clone(),
            event.agent.clone(),
            event.session_id.clone(),
            submission.submission_id.clone(),
            submission.attempt_id.clone(),
        );
        self.fanout_hub
            .create_session_attempt(fanout_key.clone())
            .await;
        self.fanout_hub
            .wait_for_subscriber(&fanout_key, fanout_subscriber_grace())
            .await;

        // Build the deterministic assistant reply sink. The sink owns live UI
        // fanout plus mutable SessionMessage projection writes for this attempt.
        let sink = PubSubSessionSink::new_with_fanout(
            self.cp.kv.clone(),
            self.cp.pubsub.clone(),
            self.cp.objects.clone(),
            self.fanout_hub.clone(),
            fanout_key,
            event.ns.clone(),
            event.session_id.clone(),
            event.agent.clone(),
            reply_msg_id.clone(),
            reply_msg_key,
            submission.submission_id.clone(),
            submission.attempt_id.clone(),
        );

        // Load the ordered recovery journal once. If the last durable boundary
        // is COMMITTED, repair the mutable submission tombstone and stop here.
        let journal_entries = sessions::list_journal_entries(
            self.cp.kv.as_ref(),
            ns,
            &event.agent,
            &event.session_id,
            &submission.submission_id,
        )
        .await?;
        if let Some(entry) = journal_entries
            .last()
            .filter(|entry| entry.phase == SessionExecutionPhase::Committed as i32)
        {
            let committed_message_id = entry.committed_message_id.clone().or_else(|| {
                match entry
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.payload.as_ref())
                {
                    Some(session_journal_entry_payload::Payload::Commit(commit)) => {
                        Some(commit.committed_message_id.clone())
                    }
                    _ => None,
                }
            });
            let committed_message_id = committed_message_id
                .filter(|id| !id.is_empty())
                .ok_or_else(|| anyhow!("COMMITTED journal entry is missing message id"))?;
            let committed_message_key = crate::control::keys::session_message(
                ns,
                &event.agent,
                &event.session_id,
                &committed_message_id,
            );
            if let Some(mut message) = self
                .cp
                .kv
                .get_msg::<data_proto::SessionMessage>(&committed_message_key)
                .await?
            {
                if message
                    .labels
                    .get(sessions::SESSION_LABEL_PROJECTION_STATE)
                    .map(String::as_str)
                    != Some(sessions::SESSION_PROJECTION_STATE_COMMITTED)
                {
                    message.labels.insert(
                        sessions::SESSION_LABEL_PROJECTION_STATE.to_string(),
                        sessions::SESSION_PROJECTION_STATE_COMMITTED.to_string(),
                    );
                    self.cp.kv.set_msg(&committed_message_key, &message).await?;
                }
            }
            sessions::mark_terminal(
                self.cp.kv.as_ref(),
                ns,
                &event.agent,
                &event.session_id,
                &submission.submission_id,
                &submission.attempt_id,
                SessionSubmissionStatus::Committed as i32,
                &committed_message_id,
                chrono::Utc::now().timestamp_micros(),
            )
            .await?;
            self.session_cancellations
                .lock()
                .await
                .remove(&event.session_id);
            self.release_session_lock(
                ns,
                &event.agent,
                &event.session_id,
                lease_renewal.last_renewed_at(),
                SessionCompletionStatus::Completed,
            )
            .await;
            return Ok(());
        }

        if let Err(err) = self
            .maybe_send_connector_session_activity(
                ns,
                &event.agent,
                &event.session_id,
                &submission.submission_id,
                "start",
                "is thinking...",
            )
            .await
        {
            tracing::warn!(
                error = %err,
                agent = %event.agent,
                session = %event.session_id,
                submission = %submission.submission_id,
                "failed to send connector typing start activity"
            );
        }

        let outcome = async {
            // Load the agent resource before deciding which runtime owns the
            // rest of the session execution.
            let store = crate::control::resources::ResourceStore::new(
                self.cp.kv.clone(),
                self.cp.pubsub.clone(),
            );
            let agent = match store.get_agent(ns, &event.agent).await {
                Ok(Some(agent)) => agent,
                Ok(None) => {
                    let err = format!("Agent '{}' not found in ns '{}'", event.agent, ns);
                    tracing::error!(
                        agent = %event.agent,
                        session = %event.session_id,
                        "{err}"
                    );
                    sink.on_error(&format!("Error: {err}")).await;
                    return Ok((SessionCompletionStatus::Errored, sink.summary()));
                }
                Err(err) => {
                    tracing::error!(
                        agent = %event.agent,
                        session = %event.session_id,
                        "Failed to fetch agent: {}",
                        err
                    );
                    sink.on_error(&format!("Error: failed to fetch agent: {err}"))
                        .await;
                    return Ok((SessionCompletionStatus::Errored, sink.summary()));
                }
            };
            let is_acp = agent
                .spec
                .as_ref()
                .and_then(|spec| spec.runtime.as_ref())
                .map(|runtime| runtime.kind == "acp")
                .unwrap_or(false);

            if is_acp {
                // ACP runtimes are not journal-hydrated by this durable LLM
                // loop; they keep their existing execution path.
                let runtime = match crate::harness::acp::AcpAgentRuntime::build_from_agent(
                    ns,
                    &event.agent,
                    &event.session_id,
                    agent,
                    &self.cp,
                    &self.config,
                )
                .instrument(tracing::info_span!("AcpAgentRuntime.build"))
                .await
                {
                    Ok(runtime) => runtime,
                    Err(err) => {
                        tracing::error!(
                            agent = %event.agent,
                            session = %event.session_id,
                            "Failed to build ACP agent runtime: {}",
                            err
                        );
                        sink.on_error(&format!("Error: {}", err)).await;
                        return Ok((SessionCompletionStatus::Errored, sink.summary()));
                    }
                };

                return execute_with_panic_boundary(
                    runtime.execute(&event.message, &sink, Some(&cancellation_token)),
                    &sink,
                    &event.agent,
                    &event.session_id,
                )
                .instrument(tracing::info_span!("AcpAgentRuntime.execute_session"))
                .await
                .map(|status| (status, sink.summary()));
            }

            // Build the LLM-loop runtime from canonical SessionMessage history.
            // Active in-progress projections are ignored by AgentRuntime.
            let mut runtime = match AgentRuntime::build_from_agent(
                ns,
                &event.agent,
                &event.session_id,
                agent,
                &self.cp,
                &self.config,
                &self.mcp_registry,
            )
            .instrument(tracing::info_span!("AgentRuntime.build"))
            .await
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    tracing::error!(
                        agent = %event.agent,
                        session = %event.session_id,
                        "Failed to build agent runtime: {}",
                        err
                    );
                    sink.on_error(&format!("Error: {}", err)).await;
                    return Ok((SessionCompletionStatus::Errored, sink.summary()));
                }
            };

            // Hydrate the runtime context from the stable journal and execute
            // any missing tool results before returning to the LLM loop.
            let prepared_submission = prepare_context_for_claimed_submission(
                &self.cp,
                ns,
                &event.agent,
                &event.session_id,
                &reply_msg_id,
                &submission.submission_id,
                &submission.attempt_id,
                &journal_entries,
                &mut runtime,
            )
            .await?;
            for part in &prepared_submission.projection_parts {
                match part {
                    RecoveredProjectionPart::Text { part_id, content } => {
                        sink.seed_recovered_text_part(part_id, content);
                    }
                    RecoveredProjectionPart::ToolCall {
                        part_id,
                        id,
                        name,
                        input,
                    } => {
                        sink.seed_recovered_tool_call_part(part_id, id, name, input);
                    }
                    RecoveredProjectionPart::ToolResult {
                        part_id,
                        id,
                        name,
                        result,
                    } => {
                        sink.seed_recovered_tool_result_part(part_id, id, name, result)
                            .await?;
                    }
                }
            }
            if let PreparedSubmissionState::FinalResponseReady { content } =
                prepared_submission.state
            {
                sink.seed_recovered_final_text_part(&content);
                sink.on_done().await;
                return Ok((SessionCompletionStatus::Completed, sink.summary()));
            }

            // Continue execution from the prepared context. The executor only
            // appends new durable journal boundaries after this point.
            execute_with_panic_boundary(
                runtime
                    .executor
                    .execute(&mut runtime.context, &sink, Some(&cancellation_token)),
                &sink,
                &event.agent,
                &event.session_id,
            )
            .instrument(tracing::info_span!("WorkerEventHandler.execute_session"))
            .await
            .map(|status| (status, sink.summary()))
        }
        .await;

        self.session_cancellations
            .lock()
            .await
            .remove(&event.session_id);
        let completion_status = outcome
            .as_ref()
            .map(|(status, _)| *status)
            .unwrap_or(SessionCompletionStatus::Errored);
        if let Err(err) = &outcome {
            sink.on_error(&format!("Error: {:#}", err)).await;
        }

        // Release the user-visible session lock after the worker has either
        // completed, failed, or panicked.
        self.release_session_lock(
            ns,
            &event.agent,
            &event.session_id,
            lease_renewal.last_renewed_at(),
            completion_status,
        )
        .instrument(tracing::info_span!(
            "WorkerEventHandler.release_session_lock"
        ))
        .await;

        if let Err(err) = self
            .maybe_send_connector_session_activity(
                ns,
                &event.agent,
                &event.session_id,
                &submission.submission_id,
                "stop",
                "",
            )
            .await
        {
            tracing::warn!(
                error = %err,
                agent = %event.agent,
                session = %event.session_id,
                submission = %submission.submission_id,
                "failed to send connector typing stop activity"
            );
        }

        if completion_status == SessionCompletionStatus::Completed {
            if let Err(err) = self
                .maybe_deliver_connector_session_reply(
                    ns,
                    &event.agent,
                    &event.session_id,
                    &sink.reply_msg_id,
                )
                .await
            {
                tracing::warn!(
                    error = %err,
                    agent = %event.agent,
                    session = %event.session_id,
                    message_id = %sink.reply_msg_id,
                    "failed to deliver connector session reply"
                );
            }
        }

        // If execution failed after writing a reply projection, terminalize the
        // submission as failed so redelivery does not treat it as still claimed.
        if outcome.is_err() || completion_status != SessionCompletionStatus::Completed {
            match crate::control::ProtoKeyValueStoreExt::get_msg::<data_proto::SessionMessage>(
                self.cp.kv.as_ref(),
                &sink.reply_msg_key,
            )
            .await
            {
                Ok(Some(_)) => {
                    if let Err(err) = sessions::mark_terminal(
                        self.cp.kv.as_ref(),
                        ns,
                        &event.agent,
                        &event.session_id,
                        &submission.submission_id,
                        &submission.attempt_id,
                        SessionSubmissionStatus::Failed as i32,
                        &sink.reply_msg_id,
                        chrono::Utc::now().timestamp_micros(),
                    )
                    .await
                    {
                        tracing::error!(
                            error = %err,
                            agent = %event.agent,
                            session = %event.session_id,
                            submission = %submission.submission_id,
                            "Failed to mark session submission terminal after execution failure"
                        );
                    }
                }
                Ok(None) => {
                    tracing::warn!(
                        agent = %event.agent,
                        session = %event.session_id,
                        submission = %submission.submission_id,
                        "Skipping terminal session submission update because reply message was not persisted"
                    );
                }
                Err(err) => {
                    tracing::error!(error = %err, "Failed to inspect reply message before terminal update");
                }
            }
        }

        if completion_status != SessionCompletionStatus::Completed {
            if let Err(err) = self
                .maybe_deliver_connector_session_reply(
                    ns,
                    &event.agent,
                    &event.session_id,
                    &sink.reply_msg_id,
                )
                .await
            {
                tracing::warn!(
                    error = %err,
                    agent = %event.agent,
                    session = %event.session_id,
                    message_id = %sink.reply_msg_id,
                    "failed to deliver failed connector session reply"
                );
            }
        }

        if let Ok((status, summary)) = &outcome {
            tracing::info!(
                agent = %event.agent,
                session = %event.session_id,
                status = ?status,
                duration_ms = summary.duration_ms,
                input_token_chunks = summary.input_token_chunks,
                input_token_chars = summary.input_token_chars,
                published_token_batches = summary.published_token_batches,
                published_token_chars = summary.published_token_chars,
                tool_calls = summary.tool_calls,
                tool_results = summary.tool_results,
                "Session message completed"
            );
        }

        outcome.map(|_| ())
    }

    async fn maybe_send_connector_session_activity(
        &self,
        ns: &str,
        agent: &str,
        session_id: &str,
        submission_id: &str,
        phase: &str,
        status_text: &str,
    ) -> Result<()> {
        let session = self
            .cp
            .kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(ns, agent, session_id))
            .await?
            .ok_or_else(|| anyhow!("session not found"))?;
        if !session.labels.contains_key(LABEL_CONNECTOR_REGISTRATION) {
            return Ok(());
        }
        if session
            .labels
            .get(LABEL_MESSAGE_SOURCE)
            .is_some_and(|source| source != "connector")
        {
            return Ok(());
        }
        if session.labels.contains_key(LABEL_CHANNEL_TRIGGER) {
            return Ok(());
        }
        connector_rpc::send_connector_session_activity(
            &self.cp,
            &session,
            &format!("{submission_id}:typing:{phase}"),
            phase,
            status_text,
        )
        .await
    }

    async fn maybe_deliver_connector_session_reply(
        &self,
        ns: &str,
        agent: &str,
        session_id: &str,
        message_id: &str,
    ) -> Result<()> {
        connector_rpc::maybe_deliver_connector_session_message(
            &self.cp, ns, agent, session_id, message_id,
        )
        .await
    }

    pub async fn handle_session_control(
        &self,
        event: crate::control::events::SessionControlEvent,
    ) -> Result<()> {
        if event.action != "stop_generation" {
            tracing::warn!(
                session = %event.session_id,
                action = %event.action,
                "Ignoring unknown session control action"
            );
            return Ok(());
        }

        let cancellations = self.session_cancellations.lock().await;
        if let Some(token) = cancellations.get(&event.session_id) {
            tracing::info!(
                namespace = %event.ns,
                agent = %event.agent,
                session = %event.session_id,
                "Cancelling in-flight session generation"
            );
            token.cancel();
        } else {
            tracing::info!(
                namespace = %event.ns,
                agent = %event.agent,
                session = %event.session_id,
                "Session stop requested, but no in-flight generation was registered"
            );
        }
        Ok(())
    }

    async fn release_session_lock(
        &self,
        ns: &str,
        agent_id: &str,
        session_id: &str,
        expected_last_active: i64,
        completion_status: SessionCompletionStatus,
    ) {
        let key = crate::control::keys::session(ns, agent_id, session_id);
        let mut released_session = None;
        let mut last_error = None;
        for _ in 0..MAX_SESSION_RELEASE_CAS_RETRIES {
            let current = match self.cp.kv.get(&key).await {
                Ok(Some(current)) => current,
                Ok(None) => return,
                Err(err) => {
                    last_error = Some(err.to_string());
                    break;
                }
            };
            let mut session = match data_proto::Session::decode(current.as_slice()) {
                Ok(session) => session,
                Err(err) => {
                    last_error = Some(err.to_string());
                    break;
                }
            };
            if session.status != "PROCESSING" || session.last_active != expected_last_active {
                return;
            }
            session.status = match completion_status {
                SessionCompletionStatus::Completed => "IDLE",
                SessionCompletionStatus::Errored | SessionCompletionStatus::Panicked => "ERROR",
            }
            .to_string();
            let updated = session.encode_to_vec();
            match self
                .cp
                .kv
                .compare_and_swap(&key, Some(current.as_slice()), &updated)
                .await
            {
                Ok(true) => {
                    released_session = Some(session);
                    break;
                }
                Ok(false) => {
                    let jitter = rand::random::<u64>() % (SESSION_RELEASE_CAS_BACKOFF_MS / 2 + 1);
                    tokio::time::sleep(std::time::Duration::from_millis(
                        SESSION_RELEASE_CAS_BACKOFF_MS + jitter,
                    ))
                    .await;
                    continue;
                }
                Err(err) => {
                    last_error = Some(err.to_string());
                    break;
                }
            }
        }
        let Some(session) = released_session else {
            tracing::error!(
                namespace = %ns,
                agent = %agent_id,
                session = %session_id,
                error = last_error.as_deref().unwrap_or("compare-and-swap conflict"),
                "failed to release session lock atomically"
            );
            return;
        };
        if let Err(err) =
            crate::worker::workflows::dispatch_workflow_from_session_labels(&self.cp, &session)
                .await
        {
            tracing::warn!(
                namespace = %ns,
                agent = %agent_id,
                session = %session_id,
                error = %err,
                "failed to dispatch workflow from completed child session"
            );
        }
    }
}

fn next_recovered_part_id(next_projection_part_index: &mut usize) -> String {
    *next_projection_part_index += 1;
    format!("{:06}", *next_projection_part_index)
}

#[cfg(test)]
mod tests {
    use super::{execute_with_panic_boundary, SessionCompletionStatus};
    use crate::control::config::{proto, Config, ProviderConfig, Secret};
    use crate::control::{
        events::{MessageDirection, SessionMessageEvent},
        keys::{ResourceKey, ResourceList},
        ControlPlane, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::connectors::session_message_final_response;
    use crate::gateway::rpc::{data_proto, manifests, resources_proto};
    use crate::harness::executor::ExecutionSink;
    use crate::harness::sessions;
    use crate::worker::{
        mcp_registry::McpRegistry, scheduler_auth::SchedulerRequestAuthenticator,
        WorkerEventHandler,
    };
    use async_trait::async_trait;
    use axum::{extract::State, routing::post, Json, Router};
    use futures::stream;
    use prost::Message;
    use serde_json::json;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::sync::Mutex;
    use tokio::net::TcpListener;
    use tokio::sync::Mutex as AsyncMutex;
    use tokio_util::sync::CancellationToken;

    struct CaptureErrorSink {
        errors: Mutex<Vec<String>>,
    }

    impl CaptureErrorSink {
        fn new() -> Self {
            Self {
                errors: Mutex::new(Vec::new()),
            }
        }

        fn errors(&self) -> Vec<String> {
            self.errors.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ExecutionSink for CaptureErrorSink {
        async fn on_token(&self, _: &str) {}
        async fn on_reasoning(&self, _: &str) {}
        async fn on_tool_call(&self, _: &str, _: &str, _: &Value) {}
        async fn on_tool_result(&self, _: &str, _: &str, _: &str) {}
        async fn on_usage(&self, _: &crate::harness::llm::ChatUsage) {}
        async fn on_done(&self) {}
        async fn on_error(&self, err: &str) {
            self.errors.lock().unwrap().push(err.to_string());
        }
    }

    fn message_part(
        part_type: data_proto::SessionMessagePartType,
        content: &str,
    ) -> data_proto::SessionMessagePart {
        data_proto::SessionMessagePart {
            id: String::new(),
            part_type: part_type as i32,
            content: content.to_string(),
            name: String::new(),
            payload_json: String::new(),
            created_at: 0,
            object: None,
        }
    }

    fn assistant_message(parts: Vec<data_proto::SessionMessagePart>) -> data_proto::SessionMessage {
        data_proto::SessionMessage {
            id: "assistant-1".to_string(),
            role: data_proto::MessageRole::RoleAssistant as i32,
            created_at: 1,
            labels: HashMap::new(),
            parts,
        }
    }

    #[test]
    fn session_message_final_response_uses_only_terminal_non_thinking_text_after_tools() {
        let message = assistant_message(vec![
            message_part(data_proto::SessionMessagePartType::Text, "private setup"),
            message_part(
                data_proto::SessionMessagePartType::Reasoning,
                "hidden thinking",
            ),
            message_part(data_proto::SessionMessagePartType::ToolCall, "Tool call"),
            message_part(
                data_proto::SessionMessagePartType::ToolResult,
                "Tool result",
            ),
            message_part(
                data_proto::SessionMessagePartType::Reasoning,
                "more thinking",
            ),
            message_part(data_proto::SessionMessagePartType::Text, "final line 1"),
            message_part(data_proto::SessionMessagePartType::Usage, ""),
            message_part(data_proto::SessionMessagePartType::Text, " final line 2 "),
        ]);

        assert_eq!(
            session_message_final_response(&message),
            "final line 1\nfinal line 2"
        );
    }

    #[test]
    fn session_message_final_response_starts_after_last_reasoning_boundary() {
        let message = assistant_message(vec![
            message_part(
                data_proto::SessionMessagePartType::Text,
                "draft before thinking",
            ),
            message_part(
                data_proto::SessionMessagePartType::Reasoning,
                "private reconsideration",
            ),
            message_part(data_proto::SessionMessagePartType::Text, "final answer"),
            message_part(data_proto::SessionMessagePartType::Image, "ignored media"),
        ]);

        assert_eq!(session_message_final_response(&message), "final answer");
    }

    #[test]
    fn session_message_final_response_keeps_error_when_it_is_terminal_response() {
        let message = assistant_message(vec![
            message_part(data_proto::SessionMessagePartType::Text, "drafting"),
            message_part(data_proto::SessionMessagePartType::ToolCall, "Tool call"),
            message_part(
                data_proto::SessionMessagePartType::ToolResult,
                "Tool result",
            ),
            message_part(data_proto::SessionMessagePartType::Error, " Error: failed "),
        ]);

        assert_eq!(session_message_final_response(&message), "Error: failed");
    }

    async fn put_agent_resource(
        kv: Arc<MockKvStore>,
        namespace: &str,
        name: &str,
        spec: resources_proto::AgentSpec,
    ) {
        let store = crate::control::resources::ResourceStore::new(
            kv,
            Arc::new(crate::test_support::RecordingPubSub::default()),
        );
        store
            .upsert(
                namespace,
                resources_proto::Resource {
                    api_version: "talon.impalasys.com/v1".to_string(),
                    kind: "Agent".to_string(),
                    metadata: Some(resources_proto::ResourceMeta {
                        name: name.to_string(),
                        namespace: namespace.to_string(),
                        labels: HashMap::new(),
                        annotations: HashMap::new(),
                        owner_references: Vec::new(),
                        finalizers: Vec::new(),
                        generation: 0,
                        resource_version: String::new(),
                        uid: String::new(),
                        deletion_timestamp: None,
                    }),
                    spec: Some(resources_proto::ResourceSpec {
                        kind: Some(resources_proto::resource_spec::Kind::Agent(spec)),
                    }),
                    status: Some(resources_proto::ResourceStatus {
                        kind: Some(resources_proto::resource_status::Kind::Agent(
                            resources_proto::AgentStatus {
                                observed_generation: 0,
                                phase: String::new(),
                                conditions: Vec::new(),
                                last_session_id: None,
                            },
                        )),
                    }),
                },
            )
            .await
            .unwrap();
    }

    async fn put_connector_class_resource(kv: Arc<MockKvStore>, endpoint: String) {
        let store = crate::control::resources::ResourceStore::new(kv, Arc::new(MockPubSub));
        store
            .upsert(
                "conic:test",
                resources_proto::Resource {
                    api_version: "talon.impalasys.com/v1".to_string(),
                    kind: "ConnectorClass".to_string(),
                    metadata: Some(resources_proto::ResourceMeta {
                        name: "slack".to_string(),
                        namespace: "conic:test".to_string(),
                        ..Default::default()
                    }),
                    spec: Some(resources_proto::ResourceSpec {
                        kind: Some(resources_proto::resource_spec::Kind::ConnectorClass(
                            resources_proto::ConnectorClassSpec {
                                platform: "slack".to_string(),
                                runtime: Some(resources_proto::ConnectorClassRuntimeSpec {
                                    kind: "externalService".to_string(),
                                    endpoint,
                                }),
                                auth: Some(resources_proto::ConnectorClassAuthSpec {
                                    kind: "apiKey".to_string(),
                                    api_key: Some(resources_proto::ConnectorSecretRef {
                                        plain: Some("connector-runtime-key".to_string()),
                                        env: None,
                                    }),
                                }),
                                match_indexes: Vec::new(),
                            },
                        )),
                    }),
                    status: Some(resources_proto::ResourceStatus {
                        kind: Some(resources_proto::resource_status::Kind::ConnectorClass(
                            resources_proto::ConnectorClassStatus {
                                observed_generation: 1,
                                phase: "Ready".to_string(),
                                conditions: Vec::new(),
                            },
                        )),
                    }),
                },
            )
            .await
            .unwrap();
    }

    fn connector_session_labels(
        extra: impl IntoIterator<Item = (&'static str, &'static str)>,
    ) -> HashMap<String, String> {
        let mut labels = HashMap::from([
            (
                "talon.impalasys.com/message-source".to_string(),
                "connector".to_string(),
            ),
            (
                "talon.impalasys.com/connector-registration".to_string(),
                "Namespace/conic%3Atest/ConnectorClass/slack".to_string(),
            ),
            (
                "talon.impalasys.com/connector".to_string(),
                "slack-main".to_string(),
            ),
            (
                "talon.impalasys.com/connector-class".to_string(),
                "slack".to_string(),
            ),
            (
                "talon.impalasys.com/external-conversation".to_string(),
                "C123".to_string(),
            ),
            (
                "talon.impalasys.com/external-message".to_string(),
                "1710000000.000100".to_string(),
            ),
        ]);
        for (key, value) in extra {
            labels.insert(key.to_string(), value.to_string());
        }
        labels
    }

    async fn put_connector_session_and_assistant_message(
        kv: Arc<MockKvStore>,
        session_labels: HashMap<String, String>,
        message_labels: HashMap<String, String>,
        text: &str,
    ) {
        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                status: "READY".to_string(),
                created_at: 0,
                last_active: 123,
                metadata: HashMap::new(),
                labels: session_labels,
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session_message(
                "conic:test",
                "assistant",
                "session-1",
                "assistant-1",
            ),
            &data_proto::SessionMessage {
                id: "assistant-1".to_string(),
                role: data_proto::MessageRole::RoleAssistant as i32,
                created_at: 1,
                labels: message_labels,
                parts: vec![data_proto::SessionMessagePart {
                    id: "000000".to_string(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: text.to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                    created_at: 1,
                    object: None,
                }],
            },
        )
        .await
        .unwrap();
    }

    async fn put_usage_policy(
        kv: Arc<MockKvStore>,
        namespace: &str,
        name: &str,
        hard: Vec<resources_proto::UsageLimit>,
    ) {
        let store = crate::control::resources::ResourceStore::new(kv, Arc::new(MockPubSub));
        store
            .upsert(
                namespace,
                resources_proto::Resource {
                    api_version: "talon.impalasys.com/v1".to_string(),
                    kind: "UsagePolicy".to_string(),
                    metadata: Some(resources_proto::ResourceMeta {
                        name: name.to_string(),
                        namespace: namespace.to_string(),
                        ..Default::default()
                    }),
                    spec: Some(resources_proto::ResourceSpec {
                        kind: Some(resources_proto::resource_spec::Kind::UsagePolicy(
                            resources_proto::UsagePolicySpec {
                                namespace_scope: "self".to_string(),
                                hard,
                            },
                        )),
                    }),
                    status: None,
                },
            )
            .await
            .unwrap();
    }

    async fn usage_policy_status(
        kv: Arc<MockKvStore>,
        namespace: &str,
        name: &str,
    ) -> resources_proto::UsagePolicyStatus {
        let store = crate::control::resources::ResourceStore::new(kv, Arc::new(MockPubSub));
        let resource = store
            .get(namespace, "UsagePolicy", name)
            .await
            .unwrap()
            .expect("UsagePolicy should exist");
        match resource.status.unwrap().kind.unwrap() {
            resources_proto::resource_status::Kind::UsagePolicy(status) => status,
            _ => panic!("expected UsagePolicy status"),
        }
    }

    fn usage_limit(metric: &str, max: u64) -> resources_proto::UsageLimit {
        resources_proto::UsageLimit {
            selector: Some(resources_proto::UsageSelector {
                agent: "assistant".to_string(),
                provider: "novita".to_string(),
                model: "test-model".to_string(),
            }),
            metric: metric.to_string(),
            max,
            window: "1h".to_string(),
            subject_scope: String::new(),
        }
    }

    #[derive(Default)]
    struct MockKvStore {
        data: AsyncMutex<HashMap<ResourceKey, Vec<u8>>>,
    }

    #[async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, key: &ResourceKey) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.data.lock().await.get(key).cloned())
        }

        async fn set(&self, key: &ResourceKey, value: &[u8]) -> anyhow::Result<()> {
            self.data.lock().await.insert(key.clone(), value.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            key: &ResourceKey,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let current = data.get(key).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(key.clone(), value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, key: &ResourceKey) -> anyhow::Result<()> {
            self.data.lock().await.remove(key);
            Ok(())
        }

        async fn list_keys(&self, list: &ResourceList) -> anyhow::Result<Vec<ResourceKey>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|key| list.matches(key).then(|| key.clone()))
                .collect::<Vec<_>>();
            keys.sort();
            Ok(keys)
        }
    }

    #[derive(Default)]
    struct MockPubSub;

    #[async_trait]
    impl MessagePublisher for MockPubSub {
        async fn publish(&self, _topic: &str, _message: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            Ok(Box::pin(stream::empty()))
        }
    }

    fn handler_with_config(kv: Arc<MockKvStore>, config: Config) -> WorkerEventHandler {
        WorkerEventHandler {
            cp: Arc::new(ControlPlane::builder(kv, Arc::new(MockPubSub)).build()),
            config: Arc::new(config),
            mcp_registry: Arc::new(McpRegistry::new()),
            scheduler_authenticator: Arc::new(SchedulerRequestAuthenticator::deny_all()),
            worker_id: "test-worker".to_string(),
            fanout_hub: Arc::new(crate::worker::fanout::FanoutHub::new()),
            session_cancellations: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }

    fn handler_with_kv(kv: Arc<MockKvStore>) -> WorkerEventHandler {
        handler_with_config(
            kv,
            Config {
                providers: HashMap::from([(
                    "novita".to_string(),
                    ProviderConfig {
                        config: Some(proto::llm_provider_config::Config::OpenaiCompatible(
                            proto::GenericConfig {
                                name: "novita".to_string(),
                                base_url: "https://unused.example.com".to_string(),
                                model: "test-model".to_string(),
                                api_key: Some(Secret {
                                    source: Some(proto::secret::Source::Plain(
                                        "test-key".to_string(),
                                    )),
                                }),
                            },
                        )),
                    },
                )]),
                default_provider: "novita".to_string(),
                ..Config::default()
            },
        )
    }

    #[tokio::test]
    async fn execute_with_panic_boundary_reports_panic_to_sink() {
        let sink = CaptureErrorSink::new();

        let result = execute_with_panic_boundary(
            async { panic!("unicode excerpt panic") },
            &sink,
            "infra",
            "session-1",
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(
            sink.errors(),
            vec!["Error: execution panicked: unicode excerpt panic".to_string()]
        );
        assert_eq!(result.unwrap(), SessionCompletionStatus::Panicked);
    }

    #[tokio::test]
    async fn execute_with_panic_boundary_reports_regular_error_to_sink() {
        let sink = CaptureErrorSink::new();

        let result = execute_with_panic_boundary(
            async { Err(anyhow::anyhow!("tool failed")) },
            &sink,
            "infra",
            "session-1",
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(sink.errors(), vec!["Error: tool failed".to_string()]);
        assert_eq!(result.unwrap(), SessionCompletionStatus::Errored);
    }

    #[tokio::test]
    async fn execute_with_panic_boundary_reports_success_and_string_panic() {
        let sink = CaptureErrorSink::new();
        let ok =
            execute_with_panic_boundary(async { Ok("done".to_string()) }, &sink, "infra", "s1")
                .await
                .unwrap();
        assert_eq!(ok, SessionCompletionStatus::Completed);
        assert!(sink.errors().is_empty());

        let string_panic = execute_with_panic_boundary(
            async { std::panic::panic_any("owned panic".to_string()) },
            &sink,
            "infra",
            "s2",
        )
        .await
        .unwrap();
        assert_eq!(string_panic, SessionCompletionStatus::Panicked);
        assert!(sink.errors().iter().any(|err| err.contains("owned panic")));
    }

    #[tokio::test]
    async fn handle_session_control_cancels_registered_session() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv);
        let token = CancellationToken::new();
        handler
            .session_cancellations
            .lock()
            .await
            .insert("session-1".to_string(), token.clone());

        handler
            .handle_session_control(crate::control::events::SessionControlEvent {
                session_id: "session-1".to_string(),
                action: "stop_generation".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                timestamp: 0,
            })
            .await
            .expect("stop generation should succeed");

        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn handle_session_control_ignores_unknown_actions() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv);

        handler
            .handle_session_control(crate::control::events::SessionControlEvent {
                session_id: "session-1".to_string(),
                action: "noop".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                timestamp: 0,
            })
            .await
            .expect("unknown action should be ignored");
    }

    #[tokio::test]
    async fn handle_session_control_allows_missing_inflight_session() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv);

        handler
            .handle_session_control(crate::control::events::SessionControlEvent {
                session_id: "missing".to_string(),
                action: "stop_generation".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                timestamp: 0,
            })
            .await
            .expect("missing inflight session should be ignored");
    }

    #[tokio::test]
    async fn release_session_lock_sets_session_back_to_idle() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv.clone());
        let session = data_proto::Session {
            id: "session-1".to_string(),
            agent: "assistant".to_string(),
            ns: "conic:test".to_string(),
            status: "PROCESSING".to_string(),
            created_at: 0,
            last_active: 123,
            metadata: HashMap::new(),
            labels: HashMap::new(),
        };
        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &session,
        )
        .await
        .expect("session should persist");

        handler
            .release_session_lock(
                "conic:test",
                "assistant",
                "session-1",
                123,
                SessionCompletionStatus::Completed,
            )
            .await;

        let updated = kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .expect("session should load")
            .expect("session should exist");
        assert_eq!(updated.status, "IDLE");
    }

    #[tokio::test]
    async fn release_session_lock_does_not_release_stolen_lock() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv.clone());
        let session = data_proto::Session {
            id: "session-1".to_string(),
            agent: "assistant".to_string(),
            ns: "conic:test".to_string(),
            status: "PROCESSING".to_string(),
            created_at: 0,
            last_active: 456,
            metadata: HashMap::new(),
            labels: HashMap::new(),
        };
        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &session,
        )
        .await
        .expect("session should persist");

        handler
            .release_session_lock(
                "conic:test",
                "assistant",
                "session-1",
                123,
                SessionCompletionStatus::Completed,
            )
            .await;

        let updated = kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .expect("session should load")
            .expect("session should exist");
        assert_eq!(updated.status, "PROCESSING");
        assert_eq!(updated.last_active, 456);
    }

    #[tokio::test]
    async fn handle_session_message_persists_runtime_build_error_and_keeps_user_message() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_config(
            kv.clone(),
            Config {
                providers: HashMap::from([(
                    "openai".to_string(),
                    ProviderConfig {
                        config: Some(proto::llm_provider_config::Config::Openai(
                            proto::OpenAiConfig {
                                model: "gpt-test".to_string(),
                                api_key: None,
                                org_id: String::new(),
                            },
                        )),
                    },
                )]),
                default_provider: "openai".to_string(),
                ..Config::default()
            },
        );
        let spec = manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: "assist".to_string(),
            post_history_prompt: String::new(),
            mcp_server_refs: Vec::new(),
            capabilities: HashMap::new(),
            a2a: None,
            runtime: None,
        };

        put_agent_resource(kv.clone(), "conic:test", "assistant", spec).await;
        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 0,
                last_active: 123,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session_message(
                "conic:test",
                "assistant",
                "session-1",
                "user-1",
            ),
            &data_proto::SessionMessage {
                id: "user-1".to_string(),
                role: data_proto::MessageRole::RoleUser as i32,
                created_at: 1,
                labels: HashMap::new(),
                parts: vec![data_proto::SessionMessagePart {
                    id: "000000".to_string(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: "operator prompt".to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                    created_at: 1,
                    object: None,
                }],
            },
        )
        .await
        .unwrap();
        handler
            .handle_session_message(SessionMessageEvent {
                ns: "conic:test".to_string(),
                agent: "assistant".to_string(),
                session_id: "session-1".to_string(),
                message_id: "user-1".to_string(),
                submission_id: "user-1".to_string(),
                direction: MessageDirection::Inbound as i32,
                message: "operator prompt".to_string(),
                timestamp: 123,
            })
            .await
            .expect("runtime build errors should be persisted and acked");

        let session = kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.status, "ERROR");

        let message_keys = kv
            .list_keys(&crate::control::keys::session_message_prefix(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap();
        let mut user_message = None;
        let mut error_message = None;
        for key in message_keys {
            if let Some(message) = kv
                .get_msg::<data_proto::SessionMessage>(&key)
                .await
                .unwrap()
            {
                if message.role == data_proto::MessageRole::RoleUser as i32 {
                    user_message = Some(message);
                } else if message.role == data_proto::MessageRole::RoleAssistant as i32
                    && message.parts.iter().any(|part| {
                        part.part_type == data_proto::SessionMessagePartType::Error as i32
                    })
                {
                    error_message = Some(message);
                }
            }
        }

        let user_message = user_message.expect("operator message should remain persisted");
        assert_eq!(user_message.parts[0].content, "operator prompt");
        let error_message = error_message.expect("assistant error should be persisted");
        let error_part = error_message
            .parts
            .iter()
            .find(|part| part.part_type == data_proto::SessionMessagePartType::Error as i32)
            .expect("error part should exist");
        assert!(error_part
            .content
            .contains("OpenAI provider config is missing api_key"));
    }

    #[tokio::test]
    async fn handle_session_message_delivers_connector_error_reply() {
        let kv = Arc::new(MockKvStore::default());
        let deliveries: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/v1/deliveries",
                post(
                    |State(deliveries): State<Arc<Mutex<Vec<Value>>>>,
                     Json(payload): Json<Value>| async move {
                        deliveries.lock().unwrap().push(payload);
                        Json(json!({
                            "accepted": true,
                            "disposition": "accepted",
                            "error": ""
                        }))
                    },
                ),
            )
            .route(
                "/v1/activities",
                post(|| async {
                    Json(json!({
                        "accepted": true,
                        "disposition": "accepted",
                        "error": ""
                    }))
                }),
            )
            .with_state(deliveries.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let handler = handler_with_config(
            kv.clone(),
            Config {
                providers: HashMap::from([(
                    "openai".to_string(),
                    ProviderConfig {
                        config: Some(proto::llm_provider_config::Config::Openai(
                            proto::OpenAiConfig {
                                model: "gpt-test".to_string(),
                                api_key: None,
                                org_id: String::new(),
                            },
                        )),
                    },
                )]),
                default_provider: "openai".to_string(),
                ..Config::default()
            },
        );
        put_agent_resource(
            kv.clone(),
            "conic:test",
            "assistant",
            manifests::AgentSpec {
                features: Vec::new(),
                model_policy: None,
                system_prompt: "assist".to_string(),
                post_history_prompt: String::new(),
                mcp_server_refs: Vec::new(),
                capabilities: HashMap::new(),
                a2a: None,
                runtime: None,
            },
        )
        .await;
        put_connector_class_resource(kv.clone(), endpoint).await;
        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 0,
                last_active: 123,
                metadata: HashMap::new(),
                labels: HashMap::from([
                    (
                        "talon.impalasys.com/message-source".to_string(),
                        "connector".to_string(),
                    ),
                    (
                        "talon.impalasys.com/connector-registration".to_string(),
                        "Namespace/conic%3Atest/ConnectorClass/slack".to_string(),
                    ),
                    (
                        "talon.impalasys.com/connector".to_string(),
                        "slack-main".to_string(),
                    ),
                    (
                        "talon.impalasys.com/connector-class".to_string(),
                        "slack".to_string(),
                    ),
                    (
                        "talon.impalasys.com/connector-event".to_string(),
                        "Ev123".to_string(),
                    ),
                    (
                        "talon.impalasys.com/external-conversation".to_string(),
                        "C123".to_string(),
                    ),
                    (
                        "talon.impalasys.com/external-thread".to_string(),
                        "1710000000.000100".to_string(),
                    ),
                    (
                        "talon.impalasys.com/external-message".to_string(),
                        "1710000000.000100".to_string(),
                    ),
                    (
                        "talon.impalasys.com/connector-match/teamId".to_string(),
                        "T123".to_string(),
                    ),
                ]),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session_message(
                "conic:test",
                "assistant",
                "session-1",
                "user-1",
            ),
            &data_proto::SessionMessage {
                id: "user-1".to_string(),
                role: data_proto::MessageRole::RoleUser as i32,
                created_at: 1,
                labels: HashMap::new(),
                parts: vec![data_proto::SessionMessagePart {
                    id: "000000".to_string(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: "operator prompt".to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                    created_at: 1,
                    object: None,
                }],
            },
        )
        .await
        .unwrap();

        handler
            .handle_session_message(SessionMessageEvent {
                ns: "conic:test".to_string(),
                agent: "assistant".to_string(),
                session_id: "session-1".to_string(),
                message_id: "user-1".to_string(),
                submission_id: "user-1".to_string(),
                direction: MessageDirection::Inbound as i32,
                message: "operator prompt".to_string(),
                timestamp: 123,
            })
            .await
            .expect("runtime build errors should be persisted, delivered, and acked");

        let deliveries = deliveries.lock().unwrap().clone();
        assert_eq!(deliveries.len(), 1);
        let delivery = &deliveries[0];
        assert_eq!(
            uuid::Uuid::parse_str(delivery["deliveryId"].as_str().unwrap_or_default())
                .expect("delivery id should be UUIDv7")
                .get_version_num(),
            7
        );
        assert_eq!(delivery["connectorClass"], "slack");
        assert_eq!(delivery["connectorName"], "slack-main");
        assert_eq!(delivery["externalConversationId"], "C123");
        assert!(delivery["text"]
            .as_str()
            .unwrap_or_default()
            .contains("OpenAI provider config is missing api_key"));

        server.abort();
    }

    #[tokio::test]
    async fn maybe_deliver_connector_session_message_delivers_appended_assistant_message() {
        let kv = Arc::new(MockKvStore::default());
        let deliveries: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/v1/deliveries",
                post(
                    |State(deliveries): State<Arc<Mutex<Vec<Value>>>>,
                     Json(payload): Json<Value>| async move {
                        deliveries.lock().unwrap().push(payload);
                        Json(json!({
                            "accepted": true,
                            "disposition": "accepted",
                            "error": ""
                        }))
                    },
                ),
            )
            .with_state(deliveries.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        put_connector_class_resource(kv.clone(), endpoint).await;

        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                status: "READY".to_string(),
                created_at: 0,
                last_active: 123,
                metadata: HashMap::new(),
                labels: HashMap::from([
                    (
                        "talon.impalasys.com/message-source".to_string(),
                        "connector".to_string(),
                    ),
                    (
                        "talon.impalasys.com/connector-registration".to_string(),
                        "Namespace/conic%3Atest/ConnectorClass/slack".to_string(),
                    ),
                    (
                        "talon.impalasys.com/connector".to_string(),
                        "slack-main".to_string(),
                    ),
                    (
                        "talon.impalasys.com/connector-class".to_string(),
                        "slack".to_string(),
                    ),
                    (
                        "talon.impalasys.com/external-conversation".to_string(),
                        "C123".to_string(),
                    ),
                    (
                        "talon.impalasys.com/external-message".to_string(),
                        "1710000000.000100".to_string(),
                    ),
                ]),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session_message(
                "conic:test",
                "assistant",
                "session-1",
                "assistant-1",
            ),
            &data_proto::SessionMessage {
                id: "assistant-1".to_string(),
                role: data_proto::MessageRole::RoleAssistant as i32,
                created_at: 1,
                labels: HashMap::from([(
                    "talon.impalasys.com/message-source".to_string(),
                    "sightline".to_string(),
                )]),
                parts: vec![data_proto::SessionMessagePart {
                    id: "000000".to_string(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: "human-authored reply".to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                    created_at: 1,
                    object: None,
                }],
            },
        )
        .await
        .unwrap();

        let cp = ControlPlane::builder(kv, Arc::new(MockPubSub)).build();
        crate::gateway::rpc::connectors::maybe_deliver_connector_session_message(
            &cp,
            "conic:test",
            "assistant",
            "session-1",
            "assistant-1",
        )
        .await
        .expect("assistant append should deliver");

        let deliveries = deliveries.lock().unwrap().clone();
        assert_eq!(deliveries.len(), 1);
        let delivery = &deliveries[0];
        assert_eq!(delivery["deliveryId"], "assistant-1");
        assert_eq!(delivery["text"], "human-authored reply");
        assert_eq!(delivery["externalConversationId"], "C123");

        server.abort();
    }

    #[tokio::test]
    async fn maybe_deliver_connector_session_message_marks_hold_for_review_pending() {
        let kv = Arc::new(MockKvStore::default());
        put_connector_session_and_assistant_message(
            kv.clone(),
            connector_session_labels([("talon.impalasys.com/connector-reply-mode", "review")]),
            HashMap::new(),
            "draft reply",
        )
        .await;

        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        crate::gateway::rpc::connectors::maybe_deliver_connector_session_message(
            &cp,
            "conic:test",
            "assistant",
            "session-1",
            "assistant-1",
        )
        .await
        .expect("hold_for_review should only mark pending");

        let message = kv
            .get_msg::<data_proto::SessionMessage>(&crate::control::keys::session_message(
                "conic:test",
                "assistant",
                "session-1",
                "assistant-1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            message
                .labels
                .get("talon.impalasys.com/connector-delivery-status")
                .map(String::as_str),
            Some("pending_review")
        );
        assert_eq!(
            message
                .labels
                .get("talon.impalasys.com/connector")
                .map(String::as_str),
            Some("slack-main")
        );
    }

    #[tokio::test]
    async fn maybe_deliver_connector_session_message_delivers_requested_review_text() {
        let kv = Arc::new(MockKvStore::default());
        let deliveries: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/v1/deliveries",
                post(
                    |State((deliveries, delivery_kv)): State<(
                        Arc<Mutex<Vec<Value>>>,
                        Arc<MockKvStore>,
                    )>,
                     Json(payload): Json<Value>| async move {
                        deliveries.lock().unwrap().push(payload);
                        let message_key = crate::control::keys::session_message(
                            "conic:test",
                            "assistant",
                            "session-1",
                            "assistant-1",
                        );
                        let mut message = delivery_kv
                            .get_msg::<data_proto::SessionMessage>(&message_key)
                            .await
                            .unwrap()
                            .unwrap();
                        message
                            .labels
                            .insert("operator-note".to_string(), "keep me".to_string());
                        delivery_kv.set_msg(&message_key, &message).await.unwrap();
                        Json(json!({
                            "accepted": true,
                            "disposition": "accepted",
                            "error": ""
                        }))
                    },
                ),
            )
            .with_state((deliveries.clone(), kv.clone()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        put_connector_class_resource(kv.clone(), endpoint).await;
        put_connector_session_and_assistant_message(
            kv.clone(),
            connector_session_labels([(
                "talon.impalasys.com/connector-reply-mode",
                "hold_for_review",
            )]),
            HashMap::from([(
                "talon.impalasys.com/connector-delivery-status".to_string(),
                "delivery_requested".to_string(),
            )]),
            "edited reply",
        )
        .await;

        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        crate::gateway::rpc::connectors::maybe_deliver_connector_session_message(
            &cp,
            "conic:test",
            "assistant",
            "session-1",
            "assistant-1",
        )
        .await
        .expect("delivery_requested should deliver");

        let deliveries = deliveries.lock().unwrap().clone();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0]["text"], "edited reply");
        let message = kv
            .get_msg::<data_proto::SessionMessage>(&crate::control::keys::session_message(
                "conic:test",
                "assistant",
                "session-1",
                "assistant-1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            message
                .labels
                .get("talon.impalasys.com/connector-delivery-status")
                .map(String::as_str),
            Some("delivered")
        );
        assert_eq!(
            message.labels.get("operator-note").map(String::as_str),
            Some("keep me")
        );

        server.abort();
    }

    #[tokio::test]
    async fn maybe_deliver_connector_session_message_skips_review_delivery() {
        let kv = Arc::new(MockKvStore::default());
        put_connector_session_and_assistant_message(
            kv.clone(),
            connector_session_labels([(
                "talon.impalasys.com/connector-reply-mode",
                "hold_for_review",
            )]),
            HashMap::from([(
                "talon.impalasys.com/connector-delivery-status".to_string(),
                "skipped".to_string(),
            )]),
            "do not send",
        )
        .await;

        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        crate::gateway::rpc::connectors::maybe_deliver_connector_session_message(
            &cp,
            "conic:test",
            "assistant",
            "session-1",
            "assistant-1",
        )
        .await
        .expect("skipped review delivery should be a no-op");
        let message = kv
            .get_msg::<data_proto::SessionMessage>(&crate::control::keys::session_message(
                "conic:test",
                "assistant",
                "session-1",
                "assistant-1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            message
                .labels
                .get("talon.impalasys.com/connector-delivery-status")
                .map(String::as_str),
            Some("skipped")
        );
    }

    #[tokio::test]
    async fn maybe_deliver_connector_session_message_marks_requested_delivery_failed() {
        let kv = Arc::new(MockKvStore::default());
        let app = Router::new().route(
            "/v1/deliveries",
            post(|| async {
                (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(json!({
                        "accepted": false,
                        "disposition": "rejected",
                        "error": "provider rejected message"
                    })),
                )
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        put_connector_class_resource(kv.clone(), endpoint).await;
        put_connector_session_and_assistant_message(
            kv.clone(),
            connector_session_labels([(
                "talon.impalasys.com/connector-reply-mode",
                "hold_for_review",
            )]),
            HashMap::from([(
                "talon.impalasys.com/connector-delivery-status".to_string(),
                "delivery_requested".to_string(),
            )]),
            "edited reply",
        )
        .await;

        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        crate::gateway::rpc::connectors::maybe_deliver_connector_session_message(
            &cp,
            "conic:test",
            "assistant",
            "session-1",
            "assistant-1",
        )
        .await
        .expect("requested delivery failure should be recorded on the message");

        let message = kv
            .get_msg::<data_proto::SessionMessage>(&crate::control::keys::session_message(
                "conic:test",
                "assistant",
                "session-1",
                "assistant-1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            message
                .labels
                .get("talon.impalasys.com/connector-delivery-status")
                .map(String::as_str),
            Some("failed")
        );
        assert!(message
            .labels
            .get("talon.impalasys.com/connector-delivery-error")
            .is_some_and(|error| error.contains("provider rejected message")));

        server.abort();
    }

    #[tokio::test]
    async fn handle_session_message_persists_setup_error_from_bad_journal() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv.clone());
        put_agent_resource(
            kv.clone(),
            "conic:test",
            "assistant",
            manifests::AgentSpec {
                features: Vec::new(),
                model_policy: None,
                system_prompt: "assist".to_string(),
                post_history_prompt: String::new(),
                mcp_server_refs: Vec::new(),
                capabilities: HashMap::new(),
                a2a: None,
                runtime: None,
            },
        )
        .await;
        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 0,
                last_active: 123,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session_message(
                "conic:test",
                "assistant",
                "session-1",
                "user-1",
            ),
            &data_proto::SessionMessage {
                id: "user-1".to_string(),
                role: data_proto::MessageRole::RoleUser as i32,
                created_at: 1,
                labels: HashMap::new(),
                parts: vec![data_proto::SessionMessagePart {
                    id: "000000".to_string(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: "operator prompt".to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                    created_at: 1,
                    object: None,
                }],
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session_journal_entry(
                "conic:test",
                "assistant",
                "session-1",
                "user-1",
                "000001",
            ),
            &data_proto::SessionJournalEntry {
                submission_id: "user-1".to_string(),
                journal_entry_id: "000001".to_string(),
                attempt_id: "prior-attempt".to_string(),
                phase: data_proto::SessionExecutionPhase::LlmResponse as i32,
                payload: None,
                created_at: 1,
                updated_at: 1,
                committed_at: None,
                committed_message_id: None,
            },
        )
        .await
        .unwrap();

        let result = handler
            .handle_session_message(SessionMessageEvent {
                ns: "conic:test".to_string(),
                agent: "assistant".to_string(),
                session_id: "session-1".to_string(),
                message_id: "user-1".to_string(),
                submission_id: "user-1".to_string(),
                direction: MessageDirection::Inbound as i32,
                message: "operator prompt".to_string(),
                timestamp: 123,
            })
            .await;
        assert!(result.is_err());

        let session = kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.status, "ERROR");
        let message_keys = kv
            .list_keys(&crate::control::keys::session_message_prefix(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap();
        let error_message_id = message_keys
            .iter()
            .map(|key| key.name.as_str())
            .find(|id| *id != "user-1")
            .expect("assistant error message should be persisted");
        assert_eq!(
            uuid::Uuid::parse_str(error_message_id)
                .expect("assistant error message id should be UUIDv7")
                .get_version_num(),
            7
        );
        let error_message = kv
            .get_msg::<data_proto::SessionMessage>(&crate::control::keys::session_message(
                "conic:test",
                "assistant",
                "session-1",
                error_message_id,
            ))
            .await
            .unwrap()
            .expect("assistant error should be persisted");
        let error_part = error_message
            .parts
            .iter()
            .find(|part| part.part_type == data_proto::SessionMessagePartType::Error as i32)
            .expect("error part should exist");
        assert!(error_part
            .content
            .contains("LLM_RESPONSE entry is missing payload"));
    }

    #[tokio::test]
    async fn handle_session_message_runs_end_to_end_and_releases_lock() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
        let app = Router::new().route(
            "/chat/completions",
            post(|| async {
                Json(json!({
                    "choices": [{
                        "message": {
                            "content": "assistant reply"
                        }
                    }]
                }))
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        unsafe {
            std::env::set_var("NOVITA_BASE_URL", format!("http://{addr}"));
        }

        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv.clone());
        let spec = manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: "assist".to_string(),
            post_history_prompt: String::new(),
            mcp_server_refs: Vec::new(),
            capabilities: HashMap::new(),
            a2a: None,
            runtime: None,
        };

        put_agent_resource(kv.clone(), "conic:test", "assistant", spec).await;
        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 0,
                last_active: 123,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();

        handler
            .handle_session_message(SessionMessageEvent {
                ns: "conic:test".to_string(),
                agent: "assistant".to_string(),
                session_id: "session-1".to_string(),
                message_id: "user-1".to_string(),
                submission_id: "submission-1".to_string(),
                direction: MessageDirection::Inbound as i32,
                message: "hello".to_string(),
                timestamp: 123,
            })
            .await
            .unwrap();

        let session = kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.status, "IDLE");
        assert!(handler
            .session_cancellations
            .lock()
            .await
            .get("session-1")
            .is_none());

        let message_keys = kv
            .list_keys(&crate::control::keys::session_message_prefix(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap();
        let prefix =
            crate::control::keys::session_message_prefix("conic:test", "assistant", "session-1");
        let mut reply = None;
        for key in message_keys {
            if !prefix.matches(&key) {
                continue;
            }
            if let Some(message) = kv
                .get_msg::<data_proto::SessionMessage>(&key)
                .await
                .unwrap()
            {
                reply = Some(message);
                break;
            }
        }
        let reply = reply.expect("assistant reply should be stored");
        assert_eq!(reply.role, 2);

        let submission = kv
            .get_msg::<crate::harness::sessions::SessionSubmission>(
                &crate::control::keys::session_submission(
                    "conic:test",
                    "assistant",
                    "session-1",
                    "submission-1",
                ),
            )
            .await
            .unwrap()
            .expect("submission tombstone should exist");
        assert_eq!(submission.submission_id, "submission-1");
        assert_eq!(submission.user_message_id, "user-1");
        assert_eq!(
            submission.status,
            crate::gateway::rpc::data_proto::SessionSubmissionStatus::Committed as i32
        );
        assert_eq!(submission.completed_at.is_some(), true);
        assert_eq!(
            submission.committed_message_id.as_deref(),
            Some(reply.id.as_str())
        );

        assert_eq!(
            submission.current_phase,
            crate::gateway::rpc::data_proto::SessionExecutionPhase::Committed as i32
        );
        let journal_entry_id = submission
            .current_journal_entry_id
            .as_deref()
            .expect("submission should point at committed journal entry");
        let journal_entry_key = crate::control::keys::session_journal_entry(
            "conic:test",
            "assistant",
            "session-1",
            "submission-1",
            journal_entry_id,
        );
        let journal_entry = kv
            .get(&journal_entry_key)
            .await
            .unwrap()
            .map(|bytes| {
                crate::harness::sessions::SessionJournalEntry::decode(bytes.as_slice())
                    .map_err(anyhow::Error::from)
            })
            .transpose()
            .unwrap()
            .expect("committed journal entry should exist");
        assert_eq!(
            journal_entry.phase,
            crate::gateway::rpc::data_proto::SessionExecutionPhase::Committed as i32
        );
        assert_eq!(journal_entry.committed_at.is_some(), true);
        assert_eq!(
            journal_entry.committed_message_id.as_deref(),
            Some(reply.id.as_str())
        );

        let before_duplicate_keys = kv
            .list_keys(&crate::control::keys::session_message_prefix(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap();
        handler
            .handle_session_message(SessionMessageEvent {
                ns: "conic:test".to_string(),
                agent: "assistant".to_string(),
                session_id: "session-1".to_string(),
                message_id: "user-1".to_string(),
                submission_id: "submission-1".to_string(),
                direction: MessageDirection::Inbound as i32,
                message: "hello".to_string(),
                timestamp: 123,
            })
            .await
            .unwrap();
        let after_duplicate_keys = kv
            .list_keys(&crate::control::keys::session_message_prefix(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap();
        assert_eq!(after_duplicate_keys.len(), before_duplicate_keys.len());

        unsafe {
            std::env::remove_var("NOVITA_BASE_URL");
        }
        server.abort();
    }

    #[tokio::test]
    async fn handle_session_message_charges_llm_usage_policy_and_redelivery_is_idempotent() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
        let call_count = Arc::new(AtomicUsize::new(0));
        let route_call_count = call_count.clone();
        let app = Router::new().route(
            "/chat/completions",
            post(move || {
                let route_call_count = route_call_count.clone();
                async move {
                    route_call_count.fetch_add(1, Ordering::SeqCst);
                    concat!(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"assistant reply\"}}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{}}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":5,\"completion_tokens_details\":{\"reasoning_tokens\":2},\"total_tokens\":12}}\n\n",
                        "data: [DONE]\n\n"
                    )
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        unsafe {
            std::env::set_var("NOVITA_BASE_URL", format!("http://{addr}"));
        }

        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv.clone());
        let spec = manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: "assist".to_string(),
            post_history_prompt: String::new(),
            mcp_server_refs: Vec::new(),
            capabilities: HashMap::new(),
            a2a: None,
            runtime: None,
        };

        put_agent_resource(kv.clone(), "conic:test", "assistant", spec).await;
        put_usage_policy(
            kv.clone(),
            "conic:test",
            "llm-token-limit",
            vec![
                usage_limit(crate::control::usage::METRIC_LLM_INPUT_TOKENS, 100),
                usage_limit(crate::control::usage::METRIC_LLM_OUTPUT_TOKENS, 100),
                usage_limit(crate::control::usage::METRIC_LLM_REASONING_TOKENS, 100),
                usage_limit(crate::control::usage::METRIC_LLM_TOTAL_TOKENS, 10),
            ],
        )
        .await;
        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 0,
                last_active: 123,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();

        let event = SessionMessageEvent {
            ns: "conic:test".to_string(),
            agent: "assistant".to_string(),
            session_id: "session-1".to_string(),
            message_id: "user-1".to_string(),
            submission_id: "submission-1".to_string(),
            direction: MessageDirection::Inbound as i32,
            message: "hello".to_string(),
            timestamp: 123,
        };
        handler.handle_session_message(event.clone()).await.unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        let status = usage_policy_status(kv.clone(), "conic:test", "llm-token-limit").await;
        let used_for = |metric: &str| {
            status
                .hard
                .iter()
                .find(|limit| limit.metric == metric)
                .map(|limit| (limit.used, limit.remaining, limit.exceeded))
                .expect("metric should be present")
        };
        assert_eq!(
            used_for(crate::control::usage::METRIC_LLM_INPUT_TOKENS),
            (7, 93, false)
        );
        assert_eq!(
            used_for(crate::control::usage::METRIC_LLM_OUTPUT_TOKENS),
            (5, 95, false)
        );
        assert_eq!(
            used_for(crate::control::usage::METRIC_LLM_REASONING_TOKENS),
            (2, 98, false)
        );
        assert_eq!(
            used_for(crate::control::usage::METRIC_LLM_TOTAL_TOKENS),
            (12, 0, true)
        );

        handler.handle_session_message(event).await.unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
        let status = usage_policy_status(kv.clone(), "conic:test", "llm-token-limit").await;
        assert_eq!(
            status
                .hard
                .iter()
                .find(|limit| limit.metric == crate::control::usage::METRIC_LLM_TOTAL_TOKENS)
                .map(|limit| limit.used),
            Some(12)
        );

        unsafe {
            std::env::remove_var("NOVITA_BASE_URL");
        }
        server.abort();
    }

    #[tokio::test]
    async fn redelivery_with_committed_journal_repairs_submission_without_duplicate_execution() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv.clone());
        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 1,
                last_active: 123,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session_message(
                "conic:test",
                "assistant",
                "session-1",
                "user-1-assistant",
            ),
            &data_proto::SessionMessage {
                id: "user-1-assistant".to_string(),
                role: data_proto::MessageRole::RoleAssistant as i32,
                created_at: 124,
                labels: HashMap::from([(
                    sessions::SESSION_LABEL_PROJECTION_STATE.to_string(),
                    sessions::SESSION_PROJECTION_STATE_COMPLETE_UNCOMMITTED.to_string(),
                )]),
                parts: vec![data_proto::SessionMessagePart {
                    id: "000000".to_string(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: "already committed".to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                    created_at: 124,
                    object: None,
                }],
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session_journal_entry(
                "conic:test",
                "assistant",
                "session-1",
                "user-1",
                "000001",
            ),
            &data_proto::SessionJournalEntry {
                submission_id: "user-1".to_string(),
                journal_entry_id: "000001".to_string(),
                attempt_id: "prior-attempt".to_string(),
                phase: data_proto::SessionExecutionPhase::Committed as i32,
                payload: Some(data_proto::SessionJournalEntryPayload {
                    payload: Some(data_proto::session_journal_entry_payload::Payload::Commit(
                        data_proto::SessionJournalEntryPayloadCommit {
                            committed_message_id: "user-1-assistant".to_string(),
                        },
                    )),
                }),
                created_at: 124,
                updated_at: 124,
                committed_at: Some(124),
                committed_message_id: Some("user-1-assistant".to_string()),
            },
        )
        .await
        .unwrap();

        handler
            .handle_session_message(SessionMessageEvent {
                ns: "conic:test".to_string(),
                agent: "assistant".to_string(),
                session_id: "session-1".to_string(),
                message_id: "user-1".to_string(),
                submission_id: "user-1".to_string(),
                direction: MessageDirection::Inbound as i32,
                message: "hello".to_string(),
                timestamp: 123,
            })
            .await
            .unwrap();

        let messages = kv
            .list_keys(&crate::control::keys::session_message_prefix(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap();
        assert_eq!(messages.len(), 1);
        let assistant_message = kv
            .get_msg::<data_proto::SessionMessage>(&crate::control::keys::session_message(
                "conic:test",
                "assistant",
                "session-1",
                "user-1-assistant",
            ))
            .await
            .unwrap()
            .expect("committed assistant message should remain readable");
        assert_eq!(
            assistant_message
                .labels
                .get(sessions::SESSION_LABEL_PROJECTION_STATE)
                .map(String::as_str),
            Some(sessions::SESSION_PROJECTION_STATE_COMMITTED)
        );
        let submission = kv
            .get_msg::<crate::harness::sessions::SessionSubmission>(
                &crate::control::keys::session_submission(
                    "conic:test",
                    "assistant",
                    "session-1",
                    "user-1",
                ),
            )
            .await
            .unwrap()
            .expect("submission should be tombstoned");
        assert_eq!(
            submission.status,
            crate::gateway::rpc::data_proto::SessionSubmissionStatus::Committed as i32
        );
        assert_eq!(
            submission.committed_message_id.as_deref(),
            Some("user-1-assistant")
        );
        let session = kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.status, "IDLE");
    }
}
