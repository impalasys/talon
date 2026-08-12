// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use futures::FutureExt;
use prost::Message;
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::panic::{resume_unwind, AssertUnwindSafe};
use std::time::Duration;

use super::runtime::AgentRuntime;
use super::sink::PubSubSessionSink;
use super::WorkerEventHandler;
use crate::control::cas::{decode_stored_object_bytes, CasStore};
use crate::control::tool_output::{self, ToolOutputExt};
use crate::control::{events::SessionDispatchEvent, ControlPlane, ProtoKeyValueStoreExt};
use crate::gateway::rpc::connectors as connector_rpc;
use crate::gateway::rpc::data_proto::{
    self, session_journal_entry_payload, SessionExecutionPhase, SessionSubmissionKind,
    SessionSubmissionStatus,
};
use crate::harness::executor::{tool_output_loop_message, ExecutionSink, LoopMessage};
use crate::harness::llm::ToolOutput;
use crate::harness::sessions::{self, ClaimOutcome};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

const MAX_SESSION_RELEASE_CAS_RETRIES: usize = 8;
const SESSION_RELEASE_CAS_BACKOFF_MS: u64 = 10;
const DEFAULT_FANOUT_SUBSCRIBER_GRACE_MS: u64 = 100;
const CONNECTOR_TYPING_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(20);
const LABEL_MESSAGE_SOURCE: &str = "talon.impalasys.com/message-source";
const LABEL_CONNECTOR_REGISTRATION: &str = "talon.impalasys.com/connector-registration";
const LABEL_CHANNEL_TRIGGER: &str = "talon.impalasys.com/channel-trigger";

#[derive(Clone, Debug, PartialEq, Eq)]
struct ConnectorTypingActivity {
    activity_id: String,
    phase: &'static str,
    status_text: &'static str,
}

impl ConnectorTypingActivity {
    fn start(submission_id: &str) -> Self {
        Self {
            activity_id: format!("{submission_id}:typing:start"),
            phase: "start",
            status_text: "is thinking...",
        }
    }

    fn active() -> Self {
        Self {
            activity_id: uuid::Uuid::now_v7().to_string(),
            phase: "active",
            status_text: "is thinking...",
        }
    }

    fn stop(submission_id: &str) -> Self {
        Self {
            activity_id: format!("{submission_id}:typing:stop"),
            phase: "stop",
            status_text: "",
        }
    }
}

enum ConnectorTypingDelivery {
    Ineligible,
    Eligible(Result<()>),
}

#[derive(Clone)]
struct ConnectorTypingLogContext {
    agent: String,
    session_id: String,
    submission_id: String,
}

struct ConnectorTypingKeepalive {
    context: ConnectorTypingLogContext,
    heartbeat_cancellation: CancellationToken,
    heartbeat_task: Option<JoinHandle<()>>,
    stop_signal: Option<oneshot::Sender<()>>,
    stop_task: Option<JoinHandle<()>>,
}

async fn send_connector_typing_activity_best_effort<S, SFut>(
    sender: &S,
    context: &ConnectorTypingLogContext,
    activity: ConnectorTypingActivity,
) -> bool
where
    S: Fn(ConnectorTypingActivity) -> SFut,
    SFut: Future<Output = Result<ConnectorTypingDelivery>>,
{
    let activity_id = activity.activity_id.clone();
    let phase = activity.phase;
    match sender(activity).await {
        Ok(ConnectorTypingDelivery::Ineligible) => false,
        Ok(ConnectorTypingDelivery::Eligible(Ok(()))) => true,
        Ok(ConnectorTypingDelivery::Eligible(Err(error))) => {
            tracing::warn!(
                error = %error,
                agent = %context.agent,
                session = %context.session_id,
                submission = %context.submission_id,
                activity_id = %activity_id,
                phase,
                "failed to send connector typing activity"
            );
            // Once a session reaches the sender, an endpoint failure must not
            // prevent later heartbeats or the final stop attempt.
            true
        }
        Err(error) => {
            tracing::warn!(
                error = %error,
                agent = %context.agent,
                session = %context.session_id,
                submission = %context.submission_id,
                activity_id = %activity_id,
                phase,
                "failed to resolve connector typing activity routing"
            );
            false
        }
    }
}

async fn run_connector_typing_keepalive<S, SFut>(
    cancellation: CancellationToken,
    interval: Duration,
    context: ConnectorTypingLogContext,
    sender: S,
) where
    S: Fn(ConnectorTypingActivity) -> SFut + Send + Sync + 'static,
    SFut: Future<Output = Result<ConnectorTypingDelivery>> + Send + 'static,
{
    let first_tick = tokio::time::Instant::now() + interval;
    let mut ticks = tokio::time::interval_at(first_tick, interval);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => break,
            _ = ticks.tick() => {
                let activity = ConnectorTypingActivity::active();
                let send = send_connector_typing_activity_best_effort(
                    &sender,
                    &context,
                    activity,
                );
                tokio::pin!(send);
                let sent = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => break,
                    sent = &mut send => sent
                };
                if !sent {
                    break;
                }
            }
        }
    }
}

async fn run_connector_typing_stop<S, SFut>(
    context: ConnectorTypingLogContext,
    sender: S,
    stop_signal: oneshot::Receiver<()>,
) where
    S: Fn(ConnectorTypingActivity) -> SFut + Send + Sync + 'static,
    SFut: Future<Output = Result<ConnectorTypingDelivery>> + Send + 'static,
{
    if stop_signal.await.is_ok() {
        let _ = send_connector_typing_activity_best_effort(
            &sender,
            &context,
            ConnectorTypingActivity::stop(&context.submission_id),
        )
        .await;
    }
}

async fn start_connector_typing_keepalive<S, SFut>(
    context: ConnectorTypingLogContext,
    interval: Duration,
    sender: S,
) -> Option<ConnectorTypingKeepalive>
where
    S: Fn(ConnectorTypingActivity) -> SFut + Clone + Send + Sync + 'static,
    SFut: Future<Output = Result<ConnectorTypingDelivery>> + Send + 'static,
{
    let eligible = send_connector_typing_activity_best_effort(
        &sender,
        &context,
        ConnectorTypingActivity::start(&context.submission_id),
    )
    .await;
    if !eligible {
        return None;
    }

    let heartbeat_cancellation = CancellationToken::new();
    let heartbeat_task = tokio::spawn(run_connector_typing_keepalive(
        heartbeat_cancellation.clone(),
        interval,
        context.clone(),
        sender.clone(),
    ));
    let (stop_signal, stop_receiver) = oneshot::channel();
    let stop_task = tokio::spawn(run_connector_typing_stop(
        context.clone(),
        sender,
        stop_receiver,
    ));

    Some(ConnectorTypingKeepalive {
        context,
        heartbeat_cancellation,
        heartbeat_task: Some(heartbeat_task),
        stop_signal: Some(stop_signal),
        stop_task: Some(stop_task),
    })
}

impl ConnectorTypingKeepalive {
    async fn cancel_and_join_heartbeat(&mut self) {
        self.heartbeat_cancellation.cancel();
        if let Some(task) = self.heartbeat_task.take() {
            if let Err(error) = task.await {
                tracing::warn!(
                    error = %error,
                    agent = %self.context.agent,
                    session = %self.context.session_id,
                    submission = %self.context.submission_id,
                    "connector typing keepalive task failed"
                );
            }
        }
    }

    async fn stop_after_release(mut self) {
        self.cancel_and_join_heartbeat().await;

        let Some(stop_signal) = self.stop_signal.take() else {
            return;
        };
        if stop_signal.send(()).is_err() {
            return;
        }

        if let Some(task) = self.stop_task.take() {
            if let Err(error) = task.await {
                tracing::warn!(
                    error = %error,
                    agent = %self.context.agent,
                    session = %self.context.session_id,
                    submission = %self.context.submission_id,
                    "connector typing stop task failed"
                );
            }
        }
    }
}

impl Drop for ConnectorTypingKeepalive {
    fn drop(&mut self) {
        self.heartbeat_cancellation.cancel();
        if let Some(stop_signal) = self.stop_signal.take() {
            let _ = stop_signal.send(());
        }
    }
}

async fn with_connector_typing_keepalive<S, SFut, F, T>(
    context: ConnectorTypingLogContext,
    interval: Duration,
    sender: S,
    work: F,
) -> (T, Option<ConnectorTypingKeepalive>)
where
    S: Fn(ConnectorTypingActivity) -> SFut + Clone + Send + Sync + 'static,
    SFut: Future<Output = Result<ConnectorTypingDelivery>> + Send + 'static,
    F: Future<Output = T>,
{
    let Some(mut keepalive) = start_connector_typing_keepalive(context, interval, sender).await
    else {
        return (work.await, None);
    };

    let outcome = AssertUnwindSafe(work).catch_unwind().await;
    keepalive.cancel_and_join_heartbeat().await;

    match outcome {
        Ok(value) => (value, Some(keepalive)),
        Err(panic) => {
            keepalive.stop_after_release().await;
            resume_unwind(panic);
        }
    }
}

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
        Ok(Ok(reply)) => {
            if is_wait_for_message_reply(&reply) {
                Ok(SessionCompletionStatus::Waiting)
            } else {
                Ok(SessionCompletionStatus::Completed)
            }
        }
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

fn is_wait_for_message_reply(reply: &str) -> bool {
    reply.contains("\"status\":\"WAITING\"")
        && reply.contains("Waiting for a message")
        && reply.contains("do not poll agent_status")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionCompletionStatus {
    Completed,
    Waiting,
    Errored,
    Panicked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreparedSubmissionState {
    ContinueExecution,
    StopAfterToolResult,
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
                // Drop derived recovery state since the compaction boundary replaces history.
                latest_final_response = None;
            } else {
                return Err(anyhow!("COMPACTION entry is missing payload"));
            }

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
            // Already handled above (commit markers are terminal; we never reach here in recovery).
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
        runtime.context.push(assistant_message);
        if !response.content.is_empty() {
            let part_id = next_recovered_part_id(&mut next_projection_part_index);
            projection_parts.push(RecoveredProjectionPart::Text {
                part_id,
                content: response.content.clone(),
            });
        }

        index += 1;
        let mut stop_after_tool_results = false;
        let mut results_by_call_id = BTreeMap::new();
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
                let (_input, result) = runtime.executor.execute_tool_call(tool).await;
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
                    &ToolOutput::text(result.clone()),
                    chrono::Utc::now().timestamp_micros(),
                )
                .await?;
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
                    .unwrap_or_else(|| ToolOutput::text(result.clone()))
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
                .push(tool_output_loop_message(&tool.id, &result_output));
            stop_after_tool_results |=
                crate::harness::native_tools::tool_requests_worker_stop(&tool.name);
        }
        if stop_after_tool_results {
            return Ok(PreparedSubmission {
                state: PreparedSubmissionState::StopAfterToolResult,
                projection_parts,
            });
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
    pub async fn handle_session_message(&self, event: SessionDispatchEvent) -> Result<()> {
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
        let cancellation_key = crate::worker::session_control::SessionCancellationKey::new(
            ns,
            &event.agent,
            &event.session_id,
            &submission.submission_id,
            &submission.attempt_id,
        );
        self.session_cancellations
            .insert(cancellation_key.clone(), cancellation_token.clone())
            .await;
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
        sink.seed_latest_journal_entry_id(
            journal_entries
                .last()
                .map(|entry| entry.journal_entry_id.as_str()),
        );
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
            self.session_cancellations.remove(&cancellation_key).await;
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

        let typing_context = ConnectorTypingLogContext {
            agent: event.agent.clone(),
            session_id: event.session_id.clone(),
            submission_id: submission.submission_id.clone(),
        };
        let activity_handler = self.clone();
        let activity_ns = ns.to_string();
        let activity_agent = event.agent.clone();
        let activity_session_id = event.session_id.clone();
        let activity_sender = move |activity: ConnectorTypingActivity| {
            let handler = activity_handler.clone();
            let ns = activity_ns.clone();
            let agent = activity_agent.clone();
            let session_id = activity_session_id.clone();
            async move {
                handler
                    .maybe_send_connector_session_activity(
                        &ns,
                        &agent,
                        &session_id,
                        &activity.activity_id,
                        activity.phase,
                        activity.status_text,
                    )
                    .await
            }
        };

        let execution = Box::pin(async {
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

            if submission.kind == SessionSubmissionKind::Compact as i32 {
                runtime
                    .executor
                    .force_compact_context(&mut runtime.context, &sink)
                    .await?;
                // A successful compaction summary is a new canonical history,
                // so the old provider-side continuation is no longer valid.
                // A no-op compaction is also the explicit escape hatch for a
                // stale continuation in an otherwise minimal transcript.
                sink.clear_provider_continuation().await?;
                sink.on_done().await;
                return Ok((SessionCompletionStatus::Completed, sink.summary()));
            }

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
                }
            }
            if let PreparedSubmissionState::FinalResponseReady { content } =
                prepared_submission.state
            {
                sink.seed_recovered_final_text_part(&content);
                sink.on_done().await;
                return Ok((SessionCompletionStatus::Completed, sink.summary()));
            }
            if prepared_submission.state == PreparedSubmissionState::StopAfterToolResult {
                sink.on_done().await;
                return Ok((SessionCompletionStatus::Waiting, sink.summary()));
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
        });
        let (outcome, typing_keepalive) = with_connector_typing_keepalive(
            typing_context,
            CONNECTOR_TYPING_KEEPALIVE_INTERVAL,
            activity_sender,
            execution,
        )
        .await;

        self.session_cancellations.remove(&cancellation_key).await;
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

        if let Some(typing_keepalive) = typing_keepalive {
            typing_keepalive.stop_after_release().await;
        }

        if completion_status == SessionCompletionStatus::Completed {
            let is_delegated_task = self
                .cp
                .kv
                .get_msg::<data_proto::Session>(&crate::control::keys::session(
                    ns,
                    &event.agent,
                    &event.session_id,
                ))
                .await?
                .is_some_and(|session| {
                    session
                        .labels
                        .get(crate::control::delegation::LABEL_TASK_ROLE)
                        .map(String::as_str)
                        == Some("delegate")
                });
            if !is_delegated_task {
                if let Err(err) = self
                    .maybe_auto_forward_a2a_final_message(
                        ns,
                        &event.agent,
                        &event.session_id,
                        &sink.reply_msg_key,
                    )
                    .await
                {
                    tracing::warn!(
                        error = %err,
                        agent = %event.agent,
                        session = %event.session_id,
                        message_id = %sink.reply_msg_id,
                        "failed to auto-forward completed A2A session reply to owner"
                    );
                }
            } else {
                tracing::debug!(
                    agent = %event.agent,
                    session = %event.session_id,
                    "skipping final-response A2A auto-forward for delegated Task session"
                );
            }

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
        if outcome.is_err()
            || matches!(
                completion_status,
                SessionCompletionStatus::Errored | SessionCompletionStatus::Panicked
            )
        {
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

        if matches!(
            completion_status,
            SessionCompletionStatus::Errored | SessionCompletionStatus::Panicked
        ) {
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
        activity_id: &str,
        phase: &str,
        status_text: &str,
    ) -> Result<ConnectorTypingDelivery> {
        let session = self
            .cp
            .kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(ns, agent, session_id))
            .await?
            .ok_or_else(|| anyhow!("session not found"))?;
        if !session.labels.contains_key(LABEL_CONNECTOR_REGISTRATION) {
            return Ok(ConnectorTypingDelivery::Ineligible);
        }
        if session
            .labels
            .get(LABEL_MESSAGE_SOURCE)
            .is_some_and(|source| source != "connector")
        {
            return Ok(ConnectorTypingDelivery::Ineligible);
        }
        if session.labels.contains_key(LABEL_CHANNEL_TRIGGER) {
            return Ok(ConnectorTypingDelivery::Ineligible);
        }
        Ok(ConnectorTypingDelivery::Eligible(
            connector_rpc::send_connector_session_activity(
                &self.cp,
                &session,
                activity_id,
                phase,
                status_text,
            )
            .await,
        ))
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

    async fn maybe_auto_forward_a2a_final_message(
        &self,
        ns: &str,
        agent: &str,
        session_id: &str,
        reply_msg_key: &crate::control::keys::ResourceKey,
    ) -> Result<()> {
        let Some(message) = self
            .cp
            .kv
            .get_msg::<data_proto::SessionMessage>(reply_msg_key)
            .await?
        else {
            return Ok(());
        };
        if message.role != data_proto::MessageRole::RoleAssistant as i32 {
            return Ok(());
        }
        if session_message_sent_to_owner(&message) {
            return Ok(());
        }

        let final_text = connector_rpc::session_message_final_response(&message);
        let artifact_uris = session_message_artifact_uris(&message, &final_text);
        if final_text.trim().is_empty() && artifact_uris.is_empty() {
            return Ok(());
        }
        let forwarded_text = if final_text.trim().is_empty() {
            "Completed with attached artifacts.".to_string()
        } else {
            final_text.trim().to_string()
        };
        let forwarded = crate::harness::native_tools::auto_forward_a2a_final_message(
            &self.cp,
            ns,
            agent,
            session_id,
            &forwarded_text,
            &artifact_uris,
        )
        .await?;
        if forwarded {
            tracing::info!(
                namespace = %ns,
                agent = %agent,
                session = %session_id,
                artifacts = artifact_uris.len(),
                "auto-forwarded completed A2A session reply to owner"
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
                SessionCompletionStatus::Completed | SessionCompletionStatus::Waiting => "IDLE",
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
        if completion_status != SessionCompletionStatus::Waiting {
            let delegated_completion = match completion_status {
                SessionCompletionStatus::Completed => {
                    crate::control::delegation::DelegatedSessionCompletion::Completed
                }
                SessionCompletionStatus::Waiting => unreachable!(),
                SessionCompletionStatus::Errored | SessionCompletionStatus::Panicked => {
                    crate::control::delegation::DelegatedSessionCompletion::Failed
                }
            };
            if let Err(err) = crate::control::delegation::complete_delegated_task_from_session(
                &self.cp,
                &session,
                delegated_completion,
            )
            .await
            {
                tracing::warn!(
                    namespace = %ns,
                    agent = %agent_id,
                    session = %session_id,
                    error = %err,
                    "failed to update delegated Task from completed child session"
                );
            }
        }
        if let Err(err) = crate::control::session_queue::dispatch_next_queued_message(
            self.cp.kv.as_ref(),
            self.cp.pubsub.as_ref(),
            ns,
            agent_id,
            session_id,
            crate::control::session_queue::NEXT_QUEUE,
            chrono::Utc::now(),
        )
        .await
        {
            tracing::warn!(
                namespace = %ns,
                agent = %agent_id,
                session = %session_id,
                error = %err,
                "failed to dispatch next queued session message after session release"
            );
        }
    }
}

fn session_message_sent_to_owner(message: &data_proto::SessionMessage) -> bool {
    message.parts.iter().any(|part| {
        part.part_type == data_proto::SessionMessagePartType::ToolCall as i32
            && part.name == crate::harness::native_tools::AGENT_SEND_TOOL
            && tool_call_target(&part.payload_json).is_some_and(|target| target == "owner")
    })
}

fn tool_call_target(payload_json: &str) -> Option<String> {
    let payload = serde_json::from_str::<Value>(payload_json).ok()?;
    payload
        .get("input")?
        .get("target")?
        .as_str()
        .map(str::to_string)
}

fn session_message_artifact_uris(
    message: &data_proto::SessionMessage,
    final_text: &str,
) -> Vec<String> {
    let mut uris = crate::harness::native_tools::artifact_uris_from_message_text(final_text);
    for part in &message.parts {
        if part.part_type != data_proto::SessionMessagePartType::ToolResult as i32 {
            continue;
        }
        if part.name != crate::harness::native_tools::CREATE_ARTIFACT_TOOL {
            continue;
        }
        if let Some(output) = tool_result_output(&part.payload_json) {
            uris.extend(crate::harness::native_tools::artifact_uris_from_message_text(&output));
            if let Ok(value) = serde_json::from_str::<Value>(&output) {
                if let Some(uri) = value.get("artifactUri").and_then(Value::as_str) {
                    uris.push(uri.to_string());
                }
            }
        }
    }
    uris.sort();
    uris.dedup();
    uris
}

fn tool_result_output(payload_json: &str) -> Option<String> {
    tool_output::text_from_payload_json(payload_json)
}

fn next_recovered_part_id(next_projection_part_index: &mut usize) -> String {
    *next_projection_part_index += 1;
    format!("{:06}", *next_projection_part_index)
}

#[cfg(test)]
mod tests {
    use super::{
        execute_with_panic_boundary, session_message_artifact_uris,
        with_connector_typing_keepalive, ConnectorTypingActivity, ConnectorTypingDelivery,
        ConnectorTypingLogContext, SessionCompletionStatus,
    };
    use crate::control::config::{proto, Config, ProviderConfig, Secret};
    use crate::control::object_store::ObjectMetadata;
    use crate::control::tool_output::ToolOutputExt;
    use crate::control::{
        events::{MessageDirection, SessionDispatchEvent},
        ControlPlane, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::connectors::session_message_final_response;
    use crate::gateway::rpc::{data_proto, manifests, resources_proto};
    use crate::harness::executor::ExecutionSink;
    use crate::harness::sessions;
    use crate::test_support::MockKvStore;
    use crate::worker::{
        mcp_registry::McpRegistry, scheduler_auth::SchedulerRequestAuthenticator,
        WorkerEventHandler,
    };
    use async_trait::async_trait;
    use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
    use futures::{stream, FutureExt};
    use prost::Message;
    use serde_json::json;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::panic::AssertUnwindSafe;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use tokio_util::sync::CancellationToken;

    fn typing_test_context() -> ConnectorTypingLogContext {
        ConnectorTypingLogContext {
            agent: "assistant".to_string(),
            session_id: "session-1".to_string(),
            submission_id: "submission-1".to_string(),
        }
    }

    fn recording_typing_sender(
        activities: Arc<Mutex<Vec<ConnectorTypingActivity>>>,
    ) -> impl Fn(
        ConnectorTypingActivity,
    ) -> std::future::Ready<anyhow::Result<ConnectorTypingDelivery>>
           + Clone {
        move |activity| {
            activities.lock().unwrap().push(activity);
            std::future::ready(Ok(ConnectorTypingDelivery::Eligible(Ok(()))))
        }
    }

    fn typing_phases(activities: &Arc<Mutex<Vec<ConnectorTypingActivity>>>) -> Vec<&'static str> {
        activities
            .lock()
            .unwrap()
            .iter()
            .map(|activity| activity.phase)
            .collect()
    }

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
        async fn on_tool_result(&self, _: &str, _: &str, _: &crate::harness::llm::ToolOutput) {}
        async fn on_usage(&self, _: &crate::harness::llm::TokenCounter) {}
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

    fn message_part_with_payload(
        part_type: data_proto::SessionMessagePartType,
        name: &str,
        content: &str,
        payload_json: &str,
    ) -> data_proto::SessionMessagePart {
        data_proto::SessionMessagePart {
            id: String::new(),
            part_type: part_type as i32,
            content: content.to_string(),
            name: name.to_string(),
            payload_json: payload_json.to_string(),
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

    async fn put_session_with_metadata(
        kv: &MockKvStore,
        namespace: &str,
        agent: &str,
        session_id: &str,
        metadata: HashMap<String, String>,
    ) {
        kv.set_msg(
            &crate::control::keys::session(namespace, agent, session_id),
            &data_proto::Session {
                id: session_id.to_string(),
                agent: agent.to_string(),
                ns: namespace.to_string(),
                status: "IDLE".to_string(),
                created_at: 1,
                last_active: 1,
                metadata,
                labels: HashMap::new(),
                context_tokens: None,
            },
        )
        .await
        .unwrap();
    }

    async fn session_text_messages(
        kv: &MockKvStore,
        namespace: &str,
        agent: &str,
        session_id: &str,
    ) -> Vec<String> {
        let entries = kv
            .list_entries(
                &crate::control::keys::session_message_prefix(namespace, agent, session_id),
                None,
            )
            .await
            .unwrap();
        entries
            .into_iter()
            .map(|(_, bytes)| data_proto::SessionMessage::decode(bytes.as_slice()).unwrap())
            .flat_map(|message| {
                message
                    .parts
                    .into_iter()
                    .filter(|part| {
                        part.part_type == data_proto::SessionMessagePartType::Text as i32
                    })
                    .map(|part| part.content)
                    .collect::<Vec<_>>()
            })
            .collect()
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

    async fn put_connector_resource(
        kv: Arc<MockKvStore>,
        reply_mode: &str,
        match_fields: impl IntoIterator<Item = (&'static str, &'static str)>,
    ) {
        let store = crate::control::resources::ResourceStore::new(kv, Arc::new(MockPubSub));
        store
            .upsert(
                "conic:test",
                resources_proto::Resource {
                    api_version: "talon.impalasys.com/v1".to_string(),
                    kind: "Connector".to_string(),
                    metadata: Some(resources_proto::ResourceMeta {
                        name: "slack-main".to_string(),
                        namespace: "conic:test".to_string(),
                        uid: "connector-uid-1".to_string(),
                        ..Default::default()
                    }),
                    spec: Some(resources_proto::ResourceSpec {
                        kind: Some(resources_proto::resource_spec::Kind::Connector(
                            resources_proto::ConnectorSpec {
                                class_ref: Some(resources_proto::ResourceRef {
                                    name: "slack".to_string(),
                                    namespace: String::new(),
                                }),
                                enabled: true,
                                match_fields: match_fields
                                    .into_iter()
                                    .map(|(key, value)| (key.to_string(), value.to_string()))
                                    .collect(),
                                consumer: Some(data_proto::MessageConsumer {
                                    session: Some(data_proto::SessionMessageConsumer {
                                        agent: Some(data_proto::ResourceRef {
                                            name: "assistant".to_string(),
                                            namespace: String::new(),
                                        }),
                                        continuity: "reuse".to_string(),
                                        session_id: String::new(),
                                        reply_mode: reply_mode.to_string(),
                                    }),
                                    channel: None,
                                    workflow: None,
                                }),
                            },
                        )),
                    }),
                    status: Some(resources_proto::ResourceStatus {
                        kind: Some(resources_proto::resource_status::Kind::Connector(
                            resources_proto::ConnectorStatus {
                                observed_generation: 1,
                                phase: "Ready".to_string(),
                                conditions: Vec::new(),
                                compiled_route_ids: Vec::new(),
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
                context_tokens: None,
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
            session_cancellations: Arc::new(
                crate::worker::session_control::SessionCancellationRegistry::default(),
            ),
        }
    }

    fn handler_with_kv(kv: Arc<MockKvStore>) -> WorkerEventHandler {
        handler_with_kv_and_base_url(kv, "https://unused.example.com".to_string())
    }

    fn handler_with_kv_and_base_url(kv: Arc<MockKvStore>, base_url: String) -> WorkerEventHandler {
        handler_with_config(
            kv,
            Config {
                providers: HashMap::from([(
                    "novita".to_string(),
                    ProviderConfig {
                        config: Some(proto::llm_provider_config::Config::OpenaiCompatible(
                            proto::GenericConfig {
                                name: "novita".to_string(),
                                base_url,
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

    #[tokio::test(start_paused = true)]
    async fn connector_typing_short_run_sends_start_then_stop() {
        let activities = Arc::new(Mutex::new(Vec::new()));

        let (result, typing_keepalive) = with_connector_typing_keepalive(
            typing_test_context(),
            Duration::from_secs(20),
            recording_typing_sender(activities.clone()),
            async { "completed" },
        )
        .await;
        typing_keepalive
            .expect("eligible session should own keepalive")
            .stop_after_release()
            .await;

        assert_eq!(result, "completed");
        assert_eq!(typing_phases(&activities), vec!["start", "stop"]);
    }

    #[tokio::test(start_paused = true)]
    async fn connector_typing_ineligible_session_does_not_start_keepalive() {
        let attempted_activities = Arc::new(Mutex::new(Vec::new()));
        let sender = {
            let attempted_activities = attempted_activities.clone();
            move |activity: ConnectorTypingActivity| {
                attempted_activities.lock().unwrap().push(activity);
                std::future::ready(Ok(ConnectorTypingDelivery::Ineligible))
            }
        };
        let (complete_tx, complete_rx) = oneshot::channel();
        let task = tokio::spawn(with_connector_typing_keepalive(
            typing_test_context(),
            Duration::from_secs(20),
            sender,
            async move {
                complete_rx.await.unwrap();
                "completed"
            },
        ));

        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(60)).await;
        tokio::task::yield_now().await;
        complete_tx.send(()).unwrap();
        let (result, typing_keepalive) = task.await.unwrap();
        assert_eq!(result, "completed");
        assert!(typing_keepalive.is_none());
        assert_eq!(typing_phases(&attempted_activities), vec!["start"]);
    }

    #[tokio::test(start_paused = true)]
    async fn connector_typing_long_run_sends_unique_actives_until_stop() {
        let activities = Arc::new(Mutex::new(Vec::new()));
        let (complete_tx, complete_rx) = oneshot::channel();
        let task = tokio::spawn(with_connector_typing_keepalive(
            typing_test_context(),
            Duration::from_secs(20),
            recording_typing_sender(activities.clone()),
            async move {
                complete_rx.await.unwrap();
                "completed"
            },
        ));

        tokio::task::yield_now().await;
        assert_eq!(typing_phases(&activities), vec!["start"]);

        tokio::time::advance(Duration::from_secs(19)).await;
        tokio::task::yield_now().await;
        assert_eq!(typing_phases(&activities), vec!["start"]);

        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(typing_phases(&activities), vec!["start", "active"]);

        tokio::time::advance(Duration::from_secs(20)).await;
        tokio::task::yield_now().await;
        assert_eq!(
            typing_phases(&activities),
            vec!["start", "active", "active"]
        );

        let active_ids = activities
            .lock()
            .unwrap()
            .iter()
            .filter(|activity| activity.phase == "active")
            .map(|activity| activity.activity_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(active_ids.len(), 2);
        assert_ne!(active_ids[0], active_ids[1]);
        for activity_id in &active_ids {
            uuid::Uuid::parse_str(activity_id).expect("active activity id should be a UUID");
        }

        complete_tx.send(()).unwrap();
        let (result, typing_keepalive) = task.await.unwrap();
        typing_keepalive
            .expect("eligible session should own keepalive")
            .stop_after_release()
            .await;
        assert_eq!(result, "completed");
        assert_eq!(
            typing_phases(&activities),
            vec!["start", "active", "active", "stop"]
        );

        let activity_count_after_stop = activities.lock().unwrap().len();
        tokio::time::advance(Duration::from_secs(60)).await;
        tokio::task::yield_now().await;
        assert_eq!(activities.lock().unwrap().len(), activity_count_after_stop);
    }

    #[tokio::test(start_paused = true)]
    async fn connector_typing_abort_cancels_heartbeat_and_attempts_stop() {
        let activities = Arc::new(Mutex::new(Vec::new()));
        let (_complete_tx, complete_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(with_connector_typing_keepalive(
            typing_test_context(),
            Duration::from_secs(20),
            recording_typing_sender(activities.clone()),
            async move {
                complete_rx.await.unwrap();
                "unreachable"
            },
        ));

        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(20)).await;
        tokio::task::yield_now().await;
        assert_eq!(typing_phases(&activities), vec!["start", "active"]);

        task.abort();
        let abort_error = match task.await {
            Ok(_) => panic!("wrapper task should be aborted"),
            Err(error) => error,
        };
        assert!(abort_error.is_cancelled());
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(60)).await;
        tokio::task::yield_now().await;

        let phases = typing_phases(&activities);
        assert_eq!(phases.iter().filter(|phase| **phase == "active").count(), 1);
        assert_eq!(phases.iter().filter(|phase| **phase == "stop").count(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn connector_typing_stop_is_deferred_until_after_release() {
        let released = Arc::new(AtomicBool::new(false));
        let stop_observations = Arc::new(Mutex::new(Vec::new()));
        let activities = Arc::new(Mutex::new(Vec::new()));
        let sender = {
            let released = released.clone();
            let stop_observations = stop_observations.clone();
            let activities = activities.clone();
            move |activity: ConnectorTypingActivity| {
                if activity.phase == "stop" {
                    stop_observations
                        .lock()
                        .unwrap()
                        .push(released.load(Ordering::SeqCst));
                }
                activities.lock().unwrap().push(activity);
                std::future::ready(Ok(ConnectorTypingDelivery::Eligible(Ok(()))))
            }
        };

        let (result, typing_keepalive) = with_connector_typing_keepalive(
            typing_test_context(),
            Duration::from_secs(20),
            sender,
            async { "completed" },
        )
        .await;

        assert_eq!(result, "completed");
        assert!(stop_observations.lock().unwrap().is_empty());
        released.store(true, Ordering::SeqCst);
        typing_keepalive
            .expect("eligible session should own keepalive")
            .stop_after_release()
            .await;
        assert_eq!(*stop_observations.lock().unwrap(), vec![true]);
    }

    #[tokio::test(start_paused = true)]
    async fn connector_typing_cleans_up_after_cancellation_error_and_panic() {
        let cancelled_activities = Arc::new(Mutex::new(Vec::new()));
        let work_cancellation = CancellationToken::new();
        let cancel_work = work_cancellation.clone();
        let cancelled_task = tokio::spawn(with_connector_typing_keepalive(
            typing_test_context(),
            Duration::from_secs(20),
            recording_typing_sender(cancelled_activities.clone()),
            async move {
                work_cancellation.cancelled().await;
                "cancelled"
            },
        ));
        tokio::task::yield_now().await;
        cancel_work.cancel();
        let (result, typing_keepalive) = cancelled_task.await.unwrap();
        typing_keepalive
            .expect("eligible session should own keepalive")
            .stop_after_release()
            .await;
        assert_eq!(result, "cancelled");
        assert_eq!(typing_phases(&cancelled_activities), vec!["start", "stop"]);

        let error_activities = Arc::new(Mutex::new(Vec::new()));
        let (error_result, typing_keepalive): (anyhow::Result<()>, _) =
            with_connector_typing_keepalive(
                typing_test_context(),
                Duration::from_secs(20),
                recording_typing_sender(error_activities.clone()),
                async { Err(anyhow::anyhow!("work failed")) },
            )
            .await;
        typing_keepalive
            .expect("eligible session should own keepalive")
            .stop_after_release()
            .await;
        assert!(error_result.is_err());
        assert_eq!(typing_phases(&error_activities), vec!["start", "stop"]);

        let waiting_activities = Arc::new(Mutex::new(Vec::new()));
        let (waiting_status, typing_keepalive) = with_connector_typing_keepalive(
            typing_test_context(),
            Duration::from_secs(20),
            recording_typing_sender(waiting_activities.clone()),
            async { SessionCompletionStatus::Waiting },
        )
        .await;
        typing_keepalive
            .expect("eligible session should own keepalive")
            .stop_after_release()
            .await;
        assert_eq!(waiting_status, SessionCompletionStatus::Waiting);
        assert_eq!(typing_phases(&waiting_activities), vec!["start", "stop"]);

        let panic_activities = Arc::new(Mutex::new(Vec::new()));
        let panic_result = AssertUnwindSafe(with_connector_typing_keepalive(
            typing_test_context(),
            Duration::from_secs(20),
            recording_typing_sender(panic_activities.clone()),
            async {
                panic!("work panicked");
            },
        ))
        .catch_unwind()
        .await;
        assert!(panic_result.is_err());
        assert_eq!(typing_phases(&panic_activities), vec!["start", "stop"]);
    }

    #[tokio::test(start_paused = true)]
    async fn connector_typing_endpoint_failures_do_not_fail_session_work() {
        let attempted_activities = Arc::new(Mutex::new(Vec::new()));
        let sender = {
            let attempted_activities = attempted_activities.clone();
            move |activity: ConnectorTypingActivity| {
                attempted_activities.lock().unwrap().push(activity);
                std::future::ready(Ok(ConnectorTypingDelivery::Eligible(Err(anyhow::anyhow!(
                    "activity endpoint unavailable"
                )))))
            }
        };
        let (complete_tx, complete_rx) = oneshot::channel();
        let task = tokio::spawn(with_connector_typing_keepalive(
            typing_test_context(),
            Duration::from_secs(20),
            sender,
            async move {
                complete_rx.await.unwrap();
                "session completed"
            },
        ));

        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(20)).await;
        tokio::task::yield_now().await;
        complete_tx.send(()).unwrap();

        let (result, typing_keepalive) = task.await.unwrap();
        typing_keepalive
            .expect("eligible session should own keepalive")
            .stop_after_release()
            .await;
        assert_eq!(result, "session completed");
        assert_eq!(
            typing_phases(&attempted_activities),
            vec!["start", "active", "stop"]
        );
    }

    #[tokio::test]
    async fn recorded_image_object_recovery_preserves_object_ref() {
        let cp =
            ControlPlane::builder(Arc::new(MockKvStore::default()), Arc::new(MockPubSub)).build();
        let object = cp
            .objects
            .put(
                "cas/conic%3Atest/sessions/session-1/messages/message-1/screenshot.png",
                b"png-bytes",
                ObjectMetadata::default(),
            )
            .await
            .unwrap();

        let output = super::tool_output_from_recorded_object(&cp, &object)
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
    async fn execute_with_panic_boundary_reports_waiting_for_wait_tool_reply() {
        let sink = CaptureErrorSink::new();

        let status = execute_with_panic_boundary(
            async {
                Ok(
                    "{\"status\":\"WAITING\",\"target\":\"critic-1\"}\nWaiting for a message from critic-1. The worker will stop this turn and resume when an inbound message is dispatched; do not poll agent_status."
                        .to_string(),
                )
            },
            &sink,
            "writer",
            "session-1",
        )
        .await
        .expect("wait reply should not fail");

        assert_eq!(status, SessionCompletionStatus::Waiting);
        assert!(sink.errors().is_empty());
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
            context_tokens: None,
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
    async fn release_session_lock_sets_waiting_session_back_to_idle() {
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
            context_tokens: None,
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
                SessionCompletionStatus::Waiting,
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
    async fn release_waiting_session_dispatches_queued_message_after_unlock() {
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
            context_tokens: None,
        };
        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &session,
        )
        .await
        .expect("session should persist");
        crate::control::session_queue::queue_text_message(
            kv.as_ref(),
            "conic:test",
            "assistant",
            "session-1",
            crate::control::session_queue::NEXT_QUEUE,
            "queued follow-up",
            HashMap::new(),
            chrono::Utc::now(),
        )
        .await
        .expect("queued message should persist");

        handler
            .release_session_lock(
                "conic:test",
                "assistant",
                "session-1",
                123,
                SessionCompletionStatus::Waiting,
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
        let submissions = kv
            .list_entries(
                &crate::control::keys::session_submission_prefix(
                    "conic:test",
                    "assistant",
                    "session-1",
                ),
                None,
            )
            .await
            .expect("submissions should list");
        assert_eq!(submissions.len(), 1);
    }

    #[tokio::test]
    async fn completed_a2a_child_reply_auto_forwards_to_owner() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv.clone());
        put_session_with_metadata(
            kv.as_ref(),
            "Tenant:acme:Main",
            "cmo",
            "owner-session",
            HashMap::from([(
                "wire.a2a.talon.impalasys.com/writer-1".to_string(),
                "Tenant:acme:Copywriter/writer/child-session".to_string(),
            )]),
        )
        .await;
        put_session_with_metadata(
            kv.as_ref(),
            "Tenant:acme:Copywriter",
            "writer",
            "child-session",
            HashMap::from([(
                "wire.a2a.talon.impalasys.com/owner".to_string(),
                "Tenant:acme:Main/cmo/owner-session".to_string(),
            )]),
        )
        .await;
        let reply_key = crate::control::keys::session_message(
            "Tenant:acme:Copywriter",
            "writer",
            "child-session",
            "assistant-1",
        );
        kv.set_msg(
            &reply_key,
            &assistant_message(vec![message_part(
                data_proto::SessionMessagePartType::Text,
                "Draft is ready.",
            )]),
        )
        .await
        .unwrap();

        handler
            .maybe_auto_forward_a2a_final_message(
                "Tenant:acme:Copywriter",
                "writer",
                "child-session",
                &reply_key,
            )
            .await
            .unwrap();

        let owner_messages =
            session_text_messages(kv.as_ref(), "Tenant:acme:Main", "cmo", "owner-session").await;
        assert_eq!(owner_messages, vec!["From @writer-1:\n\nDraft is ready."]);
    }

    #[tokio::test]
    async fn completed_a2a_child_reply_does_not_duplicate_explicit_owner_send() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv.clone());
        put_session_with_metadata(
            kv.as_ref(),
            "Tenant:acme:Main",
            "cmo",
            "owner-session",
            HashMap::from([(
                "wire.a2a.talon.impalasys.com/writer-1".to_string(),
                "Tenant:acme:Copywriter/writer/child-session".to_string(),
            )]),
        )
        .await;
        put_session_with_metadata(
            kv.as_ref(),
            "Tenant:acme:Copywriter",
            "writer",
            "child-session",
            HashMap::from([(
                "wire.a2a.talon.impalasys.com/owner".to_string(),
                "Tenant:acme:Main/cmo/owner-session".to_string(),
            )]),
        )
        .await;
        let reply_key = crate::control::keys::session_message(
            "Tenant:acme:Copywriter",
            "writer",
            "child-session",
            "assistant-1",
        );
        kv.set_msg(
            &reply_key,
            &assistant_message(vec![
                message_part_with_payload(
                    data_proto::SessionMessagePartType::ToolCall,
                    crate::harness::native_tools::AGENT_SEND_TOOL,
                    "Tool call",
                    r#"{"tool_call_id":"call-1","input":{"target":"owner","message":"Done."}}"#,
                ),
                message_part(data_proto::SessionMessagePartType::Text, "Done."),
            ]),
        )
        .await
        .unwrap();

        handler
            .maybe_auto_forward_a2a_final_message(
                "Tenant:acme:Copywriter",
                "writer",
                "child-session",
                &reply_key,
            )
            .await
            .unwrap();

        let owner_messages =
            session_text_messages(kv.as_ref(), "Tenant:acme:Main", "cmo", "owner-session").await;
        assert!(owner_messages.is_empty());
    }

    #[test]
    fn session_message_artifact_uris_extracts_visible_and_created_artifacts() {
        let message = assistant_message(vec![
            message_part_with_payload(
                data_proto::SessionMessagePartType::ToolResult,
                crate::harness::native_tools::CREATE_ARTIFACT_TOOL,
                "",
                r#"{"tool_call_id":"call-1","output":"{\"artifactUri\":\"artifact://Tenant:acme:Copywriter/writer/child-session/draft\"}"}"#,
            ),
            message_part(
                data_proto::SessionMessagePartType::Text,
                "Done: artifact://Tenant:acme:Copywriter/writer/child-session/draft",
            ),
        ]);

        assert_eq!(
            session_message_artifact_uris(
                &message,
                "Done: artifact://Tenant:acme:Copywriter/writer/child-session/draft",
            ),
            vec!["artifact://Tenant:acme:Copywriter/writer/child-session/draft"]
        );
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
            context_tokens: None,
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
                                api: "chat_completions".to_string(),
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
                context_tokens: None,
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
            .handle_session_message(SessionDispatchEvent {
                ns: "conic:test".to_string(),
                agent: "assistant".to_string(),
                session_id: "session-1".to_string(),
                message_id: "user-1".to_string(),
                submission_id: "user-1".to_string(),
                direction: MessageDirection::Inbound as i32,
                message: "operator prompt".to_string(),
                timestamp: 123,
                kind: Default::default(),
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
            .list_keys(
                &crate::control::keys::session_message_prefix(
                    "conic:test",
                    "assistant",
                    "session-1",
                ),
                None,
            )
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
    async fn handle_session_message_ignores_activity_failures_and_delivers_error_reply() {
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
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({
                            "accepted": false,
                            "disposition": "unavailable",
                            "error": "activity endpoint unavailable"
                        })),
                    )
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
                                api: "chat_completions".to_string(),
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
                context_tokens: None,
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
            .handle_session_message(SessionDispatchEvent {
                ns: "conic:test".to_string(),
                agent: "assistant".to_string(),
                session_id: "session-1".to_string(),
                message_id: "user-1".to_string(),
                submission_id: "user-1".to_string(),
                direction: MessageDirection::Inbound as i32,
                message: "operator prompt".to_string(),
                timestamp: 123,
                kind: Default::default(),
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
                context_tokens: None,
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
    async fn maybe_deliver_connector_session_message_derives_delivery_request_from_connector() {
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
        put_connector_resource(
            kv.clone(),
            "",
            [("teamId", "fresh-team"), ("channelId", "fresh-channel")],
        )
        .await;
        put_connector_session_and_assistant_message(
            kv.clone(),
            connector_session_labels([
                (
                    "talon.impalasys.com/connector-registration",
                    "Namespace/conic%3Atest/ConnectorClass/stale-class",
                ),
                ("talon.impalasys.com/connector-class", "stale-class"),
                ("talon.impalasys.com/connector-match/teamId", "stale-team"),
            ]),
            HashMap::new(),
            "freshly routed reply",
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
        .expect("delivery should use current Connector context");

        let deliveries = deliveries.lock().unwrap().clone();
        assert_eq!(deliveries.len(), 1);
        let delivery = &deliveries[0];
        assert_eq!(
            delivery["registrationId"],
            "Namespace/conic%3Atest/ConnectorClass/slack"
        );
        assert_eq!(delivery["connectorClass"], "slack");
        assert_eq!(delivery["connectorName"], "slack-main");
        assert_eq!(delivery["matchFields"]["teamId"], "fresh-team");
        assert_eq!(delivery["matchFields"]["channelId"], "fresh-channel");

        server.abort();
    }

    #[tokio::test]
    async fn connector_typing_activity_skips_non_connector_and_channel_triggered_sessions() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv(kv.clone());

        put_connector_session_and_assistant_message(
            kv.clone(),
            HashMap::new(),
            HashMap::new(),
            "non-connector reply",
        )
        .await;
        let sent = handler
            .maybe_send_connector_session_activity(
                "conic:test",
                "assistant",
                "session-1",
                "non-connector-activity",
                "active",
                "is thinking...",
            )
            .await
            .expect("non-connector activity should be skipped");
        assert!(matches!(sent, ConnectorTypingDelivery::Ineligible));

        put_connector_session_and_assistant_message(
            kv,
            connector_session_labels([("talon.impalasys.com/channel-trigger", "true")]),
            HashMap::new(),
            "channel-triggered reply",
        )
        .await;
        let sent = handler
            .maybe_send_connector_session_activity(
                "conic:test",
                "assistant",
                "session-1",
                "channel-triggered-activity",
                "active",
                "is thinking...",
            )
            .await
            .expect("channel-triggered activity should be skipped");
        assert!(matches!(sent, ConnectorTypingDelivery::Ineligible));
    }

    #[tokio::test]
    async fn send_connector_session_activity_derives_request_from_connector() {
        let kv = Arc::new(MockKvStore::default());
        let activities: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/v1/activities",
                post(
                    |State(activities): State<Arc<Mutex<Vec<Value>>>>,
                     Json(payload): Json<Value>| async move {
                        activities.lock().unwrap().push(payload);
                        Json(json!({
                            "accepted": true,
                            "disposition": "accepted",
                            "error": ""
                        }))
                    },
                ),
            )
            .with_state(activities.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        put_connector_class_resource(kv.clone(), endpoint).await;
        put_connector_resource(kv.clone(), "", [("teamId", "fresh-team")]).await;
        let session = data_proto::Session {
            id: "session-1".to_string(),
            agent: "assistant".to_string(),
            ns: "conic:test".to_string(),
            status: "PROCESSING".to_string(),
            created_at: 0,
            last_active: 123,
            metadata: HashMap::new(),
            labels: connector_session_labels([
                (
                    "talon.impalasys.com/connector-registration",
                    "Namespace/conic%3Atest/ConnectorClass/stale-class",
                ),
                ("talon.impalasys.com/connector-class", "stale-class"),
                ("talon.impalasys.com/connector-match/teamId", "stale-team"),
            ]),
            context_tokens: None,
        };

        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        crate::gateway::rpc::connectors::send_connector_session_activity(
            &cp,
            &session,
            "activity-1",
            "started",
            "typing",
        )
        .await
        .expect("activity should use current Connector context");

        let activities = activities.lock().unwrap().clone();
        assert_eq!(activities.len(), 1);
        let activity = &activities[0];
        assert_eq!(activity["activityId"], "activity-1");
        assert_eq!(
            activity["registrationId"],
            "Namespace/conic%3Atest/ConnectorClass/slack"
        );
        assert_eq!(activity["connectorClass"], "slack");
        assert_eq!(activity["connectorName"], "slack-main");
        assert_eq!(activity["matchFields"]["teamId"], "fresh-team");

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
    async fn maybe_deliver_connector_session_message_uses_connector_reply_mode_over_stale_label() {
        let kv = Arc::new(MockKvStore::default());
        put_connector_resource(
            kv.clone(),
            "hold_for_review",
            [("teamId", "fresh-team"), ("channelId", "fresh-channel")],
        )
        .await;
        put_connector_session_and_assistant_message(
            kv.clone(),
            connector_session_labels([("talon.impalasys.com/connector-reply-mode", "")]),
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
        .expect("current Connector replyMode should require review");

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
    }

    #[tokio::test]
    async fn maybe_deliver_connector_session_message_uses_connector_to_disable_stale_review_label()
    {
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
        put_connector_resource(kv.clone(), "", [("teamId", "fresh-team")]).await;
        put_connector_session_and_assistant_message(
            kv.clone(),
            connector_session_labels([(
                "talon.impalasys.com/connector-reply-mode",
                "hold_for_review",
            )]),
            HashMap::new(),
            "send now",
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
        .expect("current Connector should disable stale review label");

        let deliveries = deliveries.lock().unwrap().clone();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0]["text"], "send now");
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
            None
        );

        server.abort();
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
                context_tokens: None,
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
            .handle_session_message(SessionDispatchEvent {
                ns: "conic:test".to_string(),
                agent: "assistant".to_string(),
                session_id: "session-1".to_string(),
                message_id: "user-1".to_string(),
                submission_id: "user-1".to_string(),
                direction: MessageDirection::Inbound as i32,
                message: "operator prompt".to_string(),
                timestamp: 123,
                kind: Default::default(),
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
            .list_keys(
                &crate::control::keys::session_message_prefix(
                    "conic:test",
                    "assistant",
                    "session-1",
                ),
                None,
            )
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
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv_and_base_url(kv.clone(), format!("http://{addr}"));
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
                context_tokens: None,
            },
        )
        .await
        .unwrap();

        handler
            .handle_session_message(SessionDispatchEvent {
                ns: "conic:test".to_string(),
                agent: "assistant".to_string(),
                session_id: "session-1".to_string(),
                message_id: "user-1".to_string(),
                submission_id: "submission-1".to_string(),
                direction: MessageDirection::Inbound as i32,
                message: "hello".to_string(),
                timestamp: 123,
                kind: Default::default(),
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
        assert!(handler.session_cancellations.is_empty().await);

        let message_keys = kv
            .list_keys(
                &crate::control::keys::session_message_prefix(
                    "conic:test",
                    "assistant",
                    "session-1",
                ),
                None,
            )
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
            .list_keys(
                &crate::control::keys::session_message_prefix(
                    "conic:test",
                    "assistant",
                    "session-1",
                ),
                None,
            )
            .await
            .unwrap();
        handler
            .handle_session_message(SessionDispatchEvent {
                ns: "conic:test".to_string(),
                agent: "assistant".to_string(),
                session_id: "session-1".to_string(),
                message_id: "user-1".to_string(),
                submission_id: "submission-1".to_string(),
                direction: MessageDirection::Inbound as i32,
                message: "hello".to_string(),
                timestamp: 123,
                kind: Default::default(),
            })
            .await
            .unwrap();
        let after_duplicate_keys = kv
            .list_keys(
                &crate::control::keys::session_message_prefix(
                    "conic:test",
                    "assistant",
                    "session-1",
                ),
                None,
            )
            .await
            .unwrap();
        assert_eq!(after_duplicate_keys.len(), before_duplicate_keys.len());

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
        let kv = Arc::new(MockKvStore::default());
        let handler = handler_with_kv_and_base_url(kv.clone(), format!("http://{addr}"));
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
                context_tokens: None,
            },
        )
        .await
        .unwrap();

        let event = SessionDispatchEvent {
            ns: "conic:test".to_string(),
            agent: "assistant".to_string(),
            session_id: "session-1".to_string(),
            message_id: "user-1".to_string(),
            submission_id: "submission-1".to_string(),
            direction: MessageDirection::Inbound as i32,
            message: "hello".to_string(),
            timestamp: 123,
            kind: Default::default(),
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
            (3, 97, false)
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
                context_tokens: None,
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
            .handle_session_message(SessionDispatchEvent {
                ns: "conic:test".to_string(),
                agent: "assistant".to_string(),
                session_id: "session-1".to_string(),
                message_id: "user-1".to_string(),
                submission_id: "user-1".to_string(),
                direction: MessageDirection::Inbound as i32,
                message: "hello".to_string(),
                timestamp: 123,
                kind: Default::default(),
            })
            .await
            .unwrap();

        let messages = kv
            .list_keys(
                &crate::control::keys::session_message_prefix(
                    "conic:test",
                    "assistant",
                    "session-1",
                ),
                None,
            )
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

    #[tokio::test]
    async fn prepare_context_hydrates_compaction_summary_without_tail_replay() {
        use crate::control::cas::CasStore;
        use crate::gateway::rpc::data_proto::{
            SessionExecutionPhase as DataPhase, SessionSubmissionStatus,
        };
        use crate::harness::executor::{AgentExecutor, ContextAssembler, ExecutionContext};
        use crate::harness::llm::{ChatResponse, MockLlmProvider, ToolCall};
        use crate::harness::sessions::list_journal_entries;
        use crate::harness::skills::registry::ToolRegistry;

        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(
            kv.clone(),
            Arc::new(crate::test_support::RecordingPubSub::default()),
        )
        .build();

        // Seed a claimed submission.
        let mut submission = sessions::pending_submission("submission-1", "session-1", "user-1", 1);
        submission.status = SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        sessions::create_submission_if_absent(&*kv, "ns", "agent", "session-1", &submission)
            .await
            .unwrap();

        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let mut runtime = super::AgentRuntime {
            executor: AgentExecutor::new_with_session(
                Arc::new(MockLlmProvider),
                "test-provider".to_string(),
                "test-model".to_string(),
                ContextAssembler::new("."),
                registry,
                Arc::new(Config::default()),
                "ns".to_string(),
                "agent".to_string(),
                "session-1".to_string(),
                None,
                cp.clone(),
                manifests::AgentSpec::default(),
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
        };
        sessions::append_llm_response(
            kv.as_ref(),
            "ns",
            "agent",
            "session-1",
            "submission-1",
            "attempt-1",
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
            &ChatResponse {
                content: "continued after recovery".to_string(),
                tool_calls: Vec::new(),
                usage: None,
            },
            50,
        )
        .await
        .unwrap();

        let entries = list_journal_entries(&*kv, "ns", "agent", "session-1", "submission-1")
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

        assert!(!entries.is_empty());
        let has_compaction = entries
            .iter()
            .any(|e| e.phase == DataPhase::Compaction as i32);
        assert!(has_compaction, "journal should contain compaction entry");

        let prepared = super::prepare_context_for_claimed_submission(
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
            super::PreparedSubmissionState::FinalResponseReady {
                content: "continued after recovery".to_string(),
            }
        );
        let history_shape = runtime
            .context
            .history
            .iter()
            .map(|message| (message.role.as_str(), message.text_content()))
            .collect::<Vec<_>>();
        assert_eq!(runtime.context.history.len(), 1, "{history_shape:?}");
        assert_eq!(runtime.context.history[0].role, "assistant");
        assert_eq!(
            runtime.context.history[0].text_content(),
            "# Compacted context\n\nThe user asked for a lookup."
        );
    }
}
