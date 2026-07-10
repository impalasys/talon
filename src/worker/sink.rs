// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;

use crate::control::events::{SessionMessagePartEvent, SessionMessagePartEventKind};
use crate::control::object_store::{default_object_store, ObjectStore};
use crate::control::{keys::ResourceKey, KeyValueStore, MessagePublisher};
use crate::gateway::rpc::data_proto::{self, SessionSubmissionStatus};
use crate::harness::executor::compaction::tool_result_preview;
use crate::harness::executor::{AgentEvent, ExecutionSink};
use crate::harness::llm::{ChatResponse, ChatUsage};
use crate::harness::sessions::{self, SessionSubmission};
use crate::harness::tool_results::{store_tool_result, StoredToolResult};
use crate::worker::fanout::{FanoutHub, SessionFanoutKey};
use tracing::Instrument;

fn chat_usage_payload_json(usage: &ChatUsage) -> String {
    serde_json::to_string(&serde_json::json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "reasoning_tokens": usage.reasoning_tokens,
        "total_tokens": usage.total_tokens,
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

/// Shared buffering state for append-only streamed message parts.
///
/// This intentionally keeps the live fanout lifecycle separate from durable
/// `SessionMessage.parts` assembly: `live_buffer` is drained often for small UI
/// deltas, while `accumulated` is closed only at semantic transcript boundaries.
struct StreamingPartBuffer {
    /// The durable/live part kind represented by this buffer, currently text or reasoning.
    part_type: data_proto::SessionMessagePartType,
    /// Pending content for the next live delta event; drained without changing durable state.
    live_buffer: String,
    /// Full streamed content for the current semantic part segment.
    accumulated: String,
    /// Byte offset in `accumulated` that has already been committed durably.
    durable_bytes: usize,
    /// Stable ID for the in-progress projection part so repeated writes update the same logical part.
    active_part_id: Option<String>,
    /// Last live publish timestamp used to throttle fanout batching.
    last_publish: Instant,
    /// Whether the terminal durable close has already consumed this buffer.
    final_closed: bool,
}

impl StreamingPartBuffer {
    fn new(part_type: data_proto::SessionMessagePartType) -> Self {
        Self {
            part_type,
            live_buffer: String::new(),
            accumulated: String::new(),
            durable_bytes: 0,
            active_part_id: None,
            last_publish: Instant::now(),
            final_closed: false,
        }
    }

    fn push(&mut self, chunk: &str) {
        debug_assert!(
            !self.final_closed,
            "cannot append to a stream buffer after final close"
        );
        self.accumulated.push_str(chunk);
        self.live_buffer.push_str(chunk);
    }

    fn should_publish(&self, now: Instant, interval: Duration) -> bool {
        !self.live_buffer.is_empty() && now.saturating_duration_since(self.last_publish) >= interval
    }

    fn take_live_batch(&mut self, now: Instant) -> Option<String> {
        if self.live_buffer.is_empty() {
            return None;
        }
        self.last_publish = now;
        Some(std::mem::take(&mut self.live_buffer))
    }

    fn projection_part<F>(&mut self, mut id_factory: F) -> Option<data_proto::SessionMessagePart>
    where
        F: FnMut() -> String,
    {
        let content = self.unclosed_content().to_string();
        if content.is_empty() {
            return None;
        }
        let id = self
            .active_part_id
            .get_or_insert_with(&mut id_factory)
            .clone();
        Some(self.part(id, content))
    }

    fn close_durable_part<F>(&mut self, mut id_factory: F) -> Option<data_proto::SessionMessagePart>
    where
        F: FnMut() -> String,
    {
        let content = self.unclosed_content().to_string();
        if content.is_empty() {
            return None;
        }
        self.durable_bytes = self.accumulated.len();
        let id = self.active_part_id.take().unwrap_or_else(&mut id_factory);
        Some(self.part(id, content))
    }

    fn final_part<F>(
        &mut self,
        mut id_factory: F,
    ) -> anyhow::Result<Option<data_proto::SessionMessagePart>>
    where
        F: FnMut() -> String,
    {
        anyhow::ensure!(
            !self.final_closed,
            "streaming part buffer was finalized more than once"
        );
        let content = self.unclosed_content().to_string();
        if content.is_empty() {
            self.final_closed = true;
            return Ok(None);
        }
        self.durable_bytes = self.accumulated.len();
        self.final_closed = true;
        let id = self.active_part_id.take().unwrap_or_else(&mut id_factory);
        Ok(Some(self.part(id, content)))
    }

    fn unclosed_content(&self) -> &str {
        let start = self.durable_bytes.min(self.accumulated.len());
        &self.accumulated[start..]
    }

    fn part(&self, id: String, content: String) -> data_proto::SessionMessagePart {
        data_proto::SessionMessagePart {
            id,
            part_type: self.part_type as i32,
            content,
            name: String::new(),
            payload_json: String::new(),
            created_at: chrono::Utc::now().timestamp_micros(),
            object: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionRunSummary {
    pub duration_ms: u128,
    pub input_token_chunks: u64,
    pub input_token_chars: usize,
    pub published_token_batches: u64,
    pub published_token_chars: usize,
    pub reasoning_chunks: u64,
    pub reasoning_chars: usize,
    pub tool_calls: u64,
    pub tool_results: u64,
    pub usage_events: u64,
}

/// Production execution sink for one claimed session submission.
///
/// This sink keeps backend recovery and UI projection separate. The recovery
/// journal only records completed, hydratable boundaries: full LLM responses,
/// tool results, and commit. During streaming, the deterministic assistant
/// `SessionMessage` is periodically overwritten as an in-progress UI
/// projection; recovery may roll that projection back to the latest journaled
/// boundary after a worker crash.
pub struct PubSubSessionSink {
    // Shared IO handles and session identity.
    pub kv: Arc<dyn KeyValueStore>,
    pub pubsub: Arc<dyn MessagePublisher>,
    pub objects: Arc<dyn ObjectStore + Send + Sync>,
    pub fanout_hub: Arc<FanoutHub>,
    pub fanout_key: SessionFanoutKey,
    pub ns: String,
    pub session_id: String,
    pub agent_id: String,

    // The assistant message that will be committed once generation reaches a
    // terminal boundary.
    pub reply_msg_id: String,
    pub reply_msg_key: ResourceKey,

    // Durable work identity. Journal writes are fenced by `attempt_id` so a
    // worker whose lease expired cannot keep appending state after reclaim.
    pub submission_id: String,
    pub attempt_id: String,

    // Live UI event batching.
    token_publish_interval: Duration,
    started_at: Instant,
    // At most one streamed semantic part can be open. Switching between text
    // and reasoning closes the previous buffer before opening the next.
    active_stream_buffer: Mutex<Option<StreamingPartBuffer>>,

    // Canonical assistant message assembly. `durable_parts` holds non-streaming
    // parts and streaming segments already closed by a semantic boundary.
    durable_parts: Mutex<Vec<data_proto::SessionMessagePart>>,
    next_part_index: Mutex<u64>,

    // Mutable projection state. Projection writes are UI-only and fenced by the
    // current submission attempt; journal entries remain the backend authority.
    last_flush: Mutex<Instant>, // Last time the UI projection was considered for persistence.
    latest_journal_entry_id: Mutex<Option<String>>, // Latest durable boundary reflected in projection labels.
    recorded_tool_results: Mutex<std::collections::HashMap<String, StoredToolResult>>,
    persist_lock: Arc<AsyncMutex<()>>, // Serializes projection writes with final message commit.

    // Run summary counters for logs/telemetry.
    input_token_chunks: Mutex<u64>,
    input_token_chars: Mutex<usize>,
    published_token_batches: Mutex<u64>,
    published_token_chars: Mutex<usize>,
    reasoning_chunks: Mutex<u64>,
    reasoning_chars: Mutex<usize>,
    tool_calls: Mutex<u64>,
    tool_results: Mutex<u64>,
    usage_events: Mutex<u64>,
}

impl PubSubSessionSink {
    pub fn new(
        kv: Arc<dyn KeyValueStore>,
        pubsub: Arc<dyn MessagePublisher>,
        ns: impl Into<String>,
        session_id: impl Into<String>,
        agent_id: impl Into<String>,
        reply_msg_id: impl Into<String>,
        reply_msg_key: ResourceKey,
        submission_id: impl Into<String>,
        attempt_id: impl Into<String>,
    ) -> Self {
        Self::new_inner(
            kv,
            pubsub,
            default_object_store(),
            None,
            None,
            ns,
            session_id,
            agent_id,
            reply_msg_id,
            reply_msg_key,
            submission_id,
            attempt_id,
            token_publish_interval(),
        )
    }

    pub fn new_with_fanout(
        kv: Arc<dyn KeyValueStore>,
        pubsub: Arc<dyn MessagePublisher>,
        objects: Arc<dyn ObjectStore + Send + Sync>,
        fanout_hub: Arc<FanoutHub>,
        fanout_key: SessionFanoutKey,
        ns: impl Into<String>,
        session_id: impl Into<String>,
        agent_id: impl Into<String>,
        reply_msg_id: impl Into<String>,
        reply_msg_key: ResourceKey,
        submission_id: impl Into<String>,
        attempt_id: impl Into<String>,
    ) -> Self {
        Self::new_inner(
            kv,
            pubsub,
            objects,
            Some(fanout_hub),
            Some(fanout_key),
            ns,
            session_id,
            agent_id,
            reply_msg_id,
            reply_msg_key,
            submission_id,
            attempt_id,
            token_publish_interval(),
        )
    }

    #[cfg(test)]
    fn new_with_token_publish_interval(
        kv: Arc<dyn KeyValueStore>,
        pubsub: Arc<dyn MessagePublisher>,
        ns: impl Into<String>,
        session_id: impl Into<String>,
        agent_id: impl Into<String>,
        reply_msg_id: impl Into<String>,
        reply_msg_key: ResourceKey,
        submission_id: impl Into<String>,
        attempt_id: impl Into<String>,
        token_publish_interval: Duration,
    ) -> Self {
        Self::new_inner(
            kv,
            pubsub,
            default_object_store(),
            None,
            None,
            ns,
            session_id,
            agent_id,
            reply_msg_id,
            reply_msg_key,
            submission_id,
            attempt_id,
            token_publish_interval,
        )
    }

    fn new_inner(
        kv: Arc<dyn KeyValueStore>,
        pubsub: Arc<dyn MessagePublisher>,
        objects: Arc<dyn ObjectStore + Send + Sync>,
        fanout_hub: Option<Arc<FanoutHub>>,
        fanout_key: Option<SessionFanoutKey>,
        ns: impl Into<String>,
        session_id: impl Into<String>,
        agent_id: impl Into<String>,
        reply_msg_id: impl Into<String>,
        reply_msg_key: ResourceKey,
        submission_id: impl Into<String>,
        attempt_id: impl Into<String>,
        token_publish_interval: Duration,
    ) -> Self {
        let ns = ns.into();
        let session_id = session_id.into();
        let agent_id = agent_id.into();
        let submission_id = submission_id.into();
        let attempt_id = attempt_id.into();
        let fanout_key = fanout_key.unwrap_or_else(|| {
            SessionFanoutKey::new(
                ns.clone(),
                agent_id.clone(),
                session_id.clone(),
                submission_id.clone(),
                attempt_id.clone(),
            )
        });
        Self {
            kv,
            pubsub,
            objects,
            fanout_hub: fanout_hub.unwrap_or_else(|| Arc::new(FanoutHub::new())),
            fanout_key,
            ns,
            session_id,
            agent_id,
            reply_msg_id: reply_msg_id.into(),
            reply_msg_key,
            submission_id,
            attempt_id,
            token_publish_interval,
            started_at: Instant::now(),
            active_stream_buffer: Mutex::new(None),
            durable_parts: Mutex::new(Vec::new()),
            next_part_index: Mutex::new(0),
            last_flush: Mutex::new(Instant::now()),
            latest_journal_entry_id: Mutex::new(None),
            recorded_tool_results: Mutex::new(std::collections::HashMap::new()),
            persist_lock: Arc::new(AsyncMutex::new(())),
            input_token_chunks: Mutex::new(0),
            input_token_chars: Mutex::new(0),
            published_token_batches: Mutex::new(0),
            published_token_chars: Mutex::new(0),
            reasoning_chunks: Mutex::new(0),
            reasoning_chars: Mutex::new(0),
            tool_calls: Mutex::new(0),
            tool_results: Mutex::new(0),
            usage_events: Mutex::new(0),
        }
    }

    fn next_part_id(&self) -> String {
        let mut next = self.next_part_index.lock().unwrap();
        *next += 1;
        format!("{:06}", *next)
    }

    // Record a canonical part for the final assistant SessionMessage.
    fn record_part(
        &self,
        part_type: data_proto::SessionMessagePartType,
        name: String,
        content: String,
        payload_json: String,
    ) {
        self.record_part_with_id_and_object(
            self.next_part_id(),
            part_type,
            name,
            content,
            payload_json,
            None,
        );
    }

    // Used when provisional stream chunks have already reserved the logical
    // final SessionMessagePart id for a text segment.
    fn record_part_with_id(
        &self,
        id: String,
        part_type: data_proto::SessionMessagePartType,
        name: String,
        content: String,
        payload_json: String,
    ) {
        self.record_part_with_id_and_object(id, part_type, name, content, payload_json, None);
    }

    fn record_part_with_id_and_object(
        &self,
        id: String,
        part_type: data_proto::SessionMessagePartType,
        name: String,
        content: String,
        payload_json: String,
        object: Option<data_proto::ObjectRef>,
    ) {
        self.durable_parts
            .lock()
            .unwrap()
            .push(data_proto::SessionMessagePart {
                id,
                part_type: part_type as i32,
                content,
                name,
                payload_json,
                created_at: chrono::Utc::now().timestamp_micros(),
                object,
            });
    }

    fn record_durable_stream_part(&self, part: data_proto::SessionMessagePart) {
        self.durable_parts.lock().unwrap().push(part);
    }

    pub(crate) fn seed_recovered_text_part(&self, part_id: &str, content: &str) {
        if content.is_empty() {
            return;
        }
        self.record_part_with_id(
            part_id.to_string(),
            data_proto::SessionMessagePartType::Text,
            String::new(),
            content.to_string(),
            String::new(),
        );
    }

    pub(crate) fn seed_recovered_final_text_part(&self, content: &str) {
        if content.is_empty() {
            return;
        }
        self.record_part(
            data_proto::SessionMessagePartType::Text,
            String::new(),
            content.to_string(),
            String::new(),
        );
    }

    pub(crate) fn seed_recovered_tool_call_part(
        &self,
        part_id: &str,
        id: &str,
        name: &str,
        input: &Value,
    ) {
        self.record_part_with_id(
            part_id.to_string(),
            data_proto::SessionMessagePartType::ToolCall,
            name.to_string(),
            "Tool call".to_string(),
            serde_json::to_string(&serde_json::json!({
                "tool_call_id": id,
                "input": input,
            }))
            .unwrap_or_else(|_| "{}".to_string()),
        );
    }

    pub(crate) async fn seed_recovered_tool_result_part(
        &self,
        part_id: &str,
        id: &str,
        name: &str,
        result: &str,
    ) -> Result<()> {
        let stored = store_tool_result(
            self.objects.as_ref(),
            &self.ns,
            &self.agent_id,
            &self.session_id,
            &self.reply_msg_id,
            part_id,
            id,
            name,
            result,
        )
        .await?;
        let payload_json = stored.payload_json(id);
        self.record_part_with_id_and_object(
            stored.part_id,
            data_proto::SessionMessagePartType::ToolResult,
            name.to_string(),
            stored.preview.clone(),
            payload_json,
            stored.object,
        );
        Ok(())
    }

    fn final_message_parts(&self) -> anyhow::Result<Vec<data_proto::SessionMessagePart>> {
        let mut parts = self.durable_parts.lock().unwrap().clone();
        let active = self.active_stream_buffer.lock().unwrap().take();
        if let Some(mut buffer) = active {
            if let Some(part) = buffer.final_part(|| self.next_part_id())? {
                parts.push(part);
            }
        }
        Ok(parts)
    }

    fn close_active_stream_part(&self) {
        let part = self
            .active_stream_buffer
            .lock()
            .unwrap()
            .take()
            .and_then(|mut buffer| buffer.close_durable_part(|| self.next_part_id()));
        if let Some(part) = part {
            self.record_durable_stream_part(part);
        }
    }

    fn push_active_stream_part(&self, part_type: data_proto::SessionMessagePartType, chunk: &str) {
        let closed_part = {
            let mut active = self.active_stream_buffer.lock().unwrap();
            if active
                .as_ref()
                .is_some_and(|buffer| buffer.part_type != part_type)
            {
                active
                    .take()
                    .and_then(|mut buffer| buffer.close_durable_part(|| self.next_part_id()))
            } else {
                None
            }
        };
        if let Some(part) = closed_part {
            self.record_durable_stream_part(part);
        }

        let mut active = self.active_stream_buffer.lock().unwrap();
        let buffer = active.get_or_insert_with(|| StreamingPartBuffer::new(part_type));
        buffer.push(chunk);
    }

    fn should_flush_active_stream_event(&self) -> bool {
        self.active_stream_buffer
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|buffer| {
                buffer.should_publish(Instant::now(), self.token_publish_interval)
            })
    }

    fn active_stream_type(&self) -> Option<data_proto::SessionMessagePartType> {
        self.active_stream_buffer
            .lock()
            .unwrap()
            .as_ref()
            .map(|buffer| buffer.part_type)
    }

    async fn flush_active_stream_event_buffer(&self) {
        let event = self
            .active_stream_buffer
            .lock()
            .unwrap()
            .as_mut()
            .and_then(|buffer| {
                let part_type = buffer.part_type;
                buffer
                    .take_live_batch(Instant::now())
                    .map(|content| (part_type, content))
            });
        if let Some((part_type, content)) = event {
            match part_type {
                data_proto::SessionMessagePartType::Text => {
                    *self.published_token_batches.lock().unwrap() += 1;
                    *self.published_token_chars.lock().unwrap() += content.len();
                    self.publish_event(AgentEvent::Token(content)).await;
                }
                data_proto::SessionMessagePartType::Reasoning => {
                    self.publish_event(AgentEvent::Reasoning(content)).await;
                }
                _ => {}
            }
        }
    }

    async fn publish_event(&self, event: AgentEvent) {
        let (kind, part_type, name, content, payload_json) = match event {
            AgentEvent::Reasoning(content) => (
                SessionMessagePartEventKind::Delta,
                data_proto::SessionMessagePartType::Reasoning,
                String::new(),
                content,
                String::new(),
            ),
            AgentEvent::Action { id, name, input } => (
                SessionMessagePartEventKind::Delta,
                data_proto::SessionMessagePartType::ToolCall,
                name,
                "Tool call".to_string(),
                serde_json::to_string(&serde_json::json!({
                    "tool_call_id": id,
                    "input": input,
                }))
                .unwrap_or_else(|_| "{}".to_string()),
            ),
            AgentEvent::Observation { id, name, output } => (
                SessionMessagePartEventKind::Delta,
                data_proto::SessionMessagePartType::ToolResult,
                name,
                output.clone(),
                serde_json::to_string(&serde_json::json!({
                    "tool_call_id": id,
                    "output": output,
                }))
                .unwrap_or_else(|_| "{}".to_string()),
            ),
            AgentEvent::RequestPermission {
                id,
                action,
                payload,
            } => (
                SessionMessagePartEventKind::Delta,
                data_proto::SessionMessagePartType::RequestPermission,
                action,
                "Permission requested".to_string(),
                serde_json::to_string(&serde_json::json!({
                    "requestId": id,
                    "status": "pending",
                    "request": payload,
                }))
                .unwrap_or_else(|_| "{}".to_string()),
            ),
            AgentEvent::PermissionResult { id, outcome } => (
                SessionMessagePartEventKind::Delta,
                data_proto::SessionMessagePartType::PermissionResult,
                String::new(),
                "Permission answered".to_string(),
                serde_json::to_string(&serde_json::json!({
                    "requestId": id,
                    "status": outcome
                        .get("outcome")
                        .and_then(|value| value.as_str())
                        .unwrap_or("selected"),
                    "outcome": outcome,
                }))
                .unwrap_or_else(|_| "{}".to_string()),
            ),
            AgentEvent::Token(content) => (
                SessionMessagePartEventKind::Delta,
                data_proto::SessionMessagePartType::Text,
                String::new(),
                content,
                String::new(),
            ),
            AgentEvent::Usage(usage) => (
                SessionMessagePartEventKind::Delta,
                data_proto::SessionMessagePartType::Usage,
                String::new(),
                String::new(),
                chat_usage_payload_json(&usage),
            ),
            AgentEvent::Done => (
                SessionMessagePartEventKind::Done,
                data_proto::SessionMessagePartType::Text,
                String::new(),
                String::new(),
                String::new(),
            ),
            AgentEvent::Error(err) => (
                SessionMessagePartEventKind::Error,
                data_proto::SessionMessagePartType::Error,
                String::new(),
                err,
                String::new(),
            ),
        };

        let event = SessionMessagePartEvent {
            session_id: self.session_id.clone(),
            kind: kind as i32,
            part: Some(data_proto::SessionMessagePart {
                id: String::new(),
                part_type: part_type as i32,
                content,
                name,
                payload_json,
                created_at: chrono::Utc::now().timestamp_micros(),
                object: None,
            }),
            timestamp: chrono::Utc::now().timestamp_micros(),
            agent: self.agent_id.clone(),
            ns: self.ns.clone(),
            message_id: self.reply_msg_id.clone(),
        };
        async {
            self.fanout_hub
                .publish_session_part(&self.fanout_key, event)
                .await
        }
        .instrument(tracing::info_span!(
            "PubSubSessionSink.publish_event",
            namespace = %self.ns,
            agent = %self.agent_id,
            session = %self.session_id,
            kind = ?kind,
            part_type = ?part_type,
        ))
        .await;
    }

    fn projection_labels(&self, state: &str) -> std::collections::HashMap<String, String> {
        let mut labels = std::collections::HashMap::new();
        labels.insert(
            sessions::SESSION_LABEL_SUBMISSION_ID.to_string(),
            self.submission_id.clone(),
        );
        labels.insert(
            sessions::SESSION_LABEL_ATTEMPT_ID.to_string(),
            self.attempt_id.clone(),
        );
        labels.insert(
            sessions::SESSION_LABEL_PROJECTION_STATE.to_string(),
            state.to_string(),
        );
        if let Some(entry_id) = self.latest_journal_entry_id.lock().unwrap().clone() {
            labels.insert(
                sessions::SESSION_LABEL_LATEST_JOURNAL_ENTRY_ID.to_string(),
                entry_id,
            );
        }
        labels
    }

    fn projection_message_parts(&self) -> Vec<data_proto::SessionMessagePart> {
        let mut parts = self.durable_parts.lock().unwrap().clone();
        let active_part = {
            let mut active = self.active_stream_buffer.lock().unwrap();
            active
                .as_mut()
                .and_then(|buffer| buffer.projection_part(|| self.next_part_id()))
        };
        if let Some(part) = active_part {
            parts.push(part);
        }
        parts
    }

    fn projection_message(&self, state: &str) -> data_proto::SessionMessage {
        data_proto::SessionMessage {
            id: self.reply_msg_id.clone(),
            role: data_proto::MessageRole::RoleAssistant as i32,
            created_at: chrono::Utc::now().timestamp_micros(),
            labels: self.projection_labels(state),
            parts: self.projection_message_parts(),
        }
    }

    async fn submission_attempt_is_current(
        kv: &dyn KeyValueStore,
        ns: &str,
        agent: &str,
        session_id: &str,
        submission_id: &str,
        attempt_id: &str,
    ) -> bool {
        let key = crate::control::keys::session_submission(ns, agent, session_id, submission_id);
        match crate::control::ProtoKeyValueStoreExt::get_msg::<SessionSubmission>(kv, &key).await {
            Ok(Some(submission)) => {
                submission.attempt_id == attempt_id
                    && !sessions::submission_is_terminal(&submission)
            }
            Ok(None) => false,
            Err(err) => {
                tracing::debug!(error = %err, "Failed to verify session projection attempt");
                false
            }
        }
    }

    async fn maybe_flush_kv(&self) {
        let should_flush = {
            let mut last = self.last_flush.lock().unwrap();
            if last.elapsed().as_millis() > 1000 {
                *last = Instant::now();
                true
            } else {
                false
            }
        };
        if should_flush {
            let msg = self.projection_message(sessions::SESSION_PROJECTION_STATE_IN_PROGRESS);
            let span = tracing::info_span!(
                "PubSubSessionSink.persist_projection_message",
                namespace = %self.ns,
                agent = %self.agent_id,
                session = %self.session_id,
            );
            async {
                let _guard = self.persist_lock.lock().await;
                if !Self::submission_attempt_is_current(
                    self.kv.as_ref(),
                    &self.ns,
                    &self.agent_id,
                    &self.session_id,
                    &self.submission_id,
                    &self.attempt_id,
                )
                .await
                {
                    return;
                }
                if let Err(e) = crate::control::ProtoKeyValueStoreExt::set_msg(
                    self.kv.as_ref(),
                    &self.reply_msg_key,
                    &msg,
                )
                .await
                {
                    tracing::error!("Failed to persist session projection: {}", e);
                }
            }
            .instrument(span)
            .await;
        }
    }

    async fn persist_durable_message(&self, span_name: &'static str) {
        let msg = data_proto::SessionMessage {
            id: self.reply_msg_id.clone(),
            role: data_proto::MessageRole::RoleAssistant as i32,
            created_at: chrono::Utc::now().timestamp_micros(),
            labels: self.projection_labels(sessions::SESSION_PROJECTION_STATE_COMPLETE_UNCOMMITTED),
            parts: self.durable_parts.lock().unwrap().clone(),
        };
        let result = async {
            let _guard = self.persist_lock.lock().await;
            crate::control::ProtoKeyValueStoreExt::set_msg(
                self.kv.as_ref(),
                &self.reply_msg_key,
                &msg,
            )
            .await
        }
        .instrument(tracing::info_span!(
            "PubSubSessionSink.persist_durable_message",
            operation = span_name,
            namespace = %self.ns,
            agent = %self.agent_id,
            session = %self.session_id,
        ))
        .await;
        if let Err(e) = result {
            tracing::error!(
                operation = span_name,
                "Failed to persist durable message: {}",
                e
            );
            return;
        }
        self.publish_reply_index_event().await;
    }

    async fn publish_reply_index_event(&self) {
        if let Err(error) = crate::control::search::publish_index_event(
            self.pubsub.as_ref(),
            crate::control::events::IndexEvent {
                operation: crate::control::events::IndexOperation::Upsert as i32,
                key: self.reply_msg_key.canonical(),
                ..Default::default()
            },
        )
        .await
        {
            tracing::warn!(
                error = %error,
                namespace = %self.ns,
                agent = %self.agent_id,
                session_id = %self.session_id,
                message_id = %self.reply_msg_id,
                "failed to publish search index event for durable assistant message"
            );
        }
    }

    async fn mark_terminal(&self, status: i32) -> bool {
        match sessions::mark_terminal(
            self.kv.as_ref(),
            &self.ns,
            &self.agent_id,
            &self.session_id,
            &self.submission_id,
            &self.attempt_id,
            status,
            &self.reply_msg_id,
            chrono::Utc::now().timestamp_micros(),
        )
        .await
        {
            Ok(entry) => {
                *self.latest_journal_entry_id.lock().unwrap() = Some(entry.journal_entry_id);
                true
            }
            Err(err) => {
                tracing::error!(error = %err, status, "Failed to mark session submission terminal");
                false
            }
        }
    }

    pub fn summary(&self) -> SessionRunSummary {
        SessionRunSummary {
            duration_ms: self.started_at.elapsed().as_millis(),
            input_token_chunks: *self.input_token_chunks.lock().unwrap(),
            input_token_chars: *self.input_token_chars.lock().unwrap(),
            published_token_batches: *self.published_token_batches.lock().unwrap(),
            published_token_chars: *self.published_token_chars.lock().unwrap(),
            reasoning_chunks: *self.reasoning_chunks.lock().unwrap(),
            reasoning_chars: *self.reasoning_chars.lock().unwrap(),
            tool_calls: *self.tool_calls.lock().unwrap(),
            tool_results: *self.tool_results.lock().unwrap(),
            usage_events: *self.usage_events.lock().unwrap(),
        }
    }
}

#[async_trait]
impl ExecutionSink for PubSubSessionSink {
    async fn on_llm_response(&self, response: &ChatResponse) -> Result<()> {
        let entry = sessions::append_llm_response(
            self.kv.as_ref(),
            &self.ns,
            &self.agent_id,
            &self.session_id,
            &self.submission_id,
            &self.attempt_id,
            response,
            chrono::Utc::now().timestamp_micros(),
        )
        .await?;
        *self.latest_journal_entry_id.lock().unwrap() = Some(entry.journal_entry_id);
        Ok(())
    }

    async fn on_token(&self, token: &str) {
        *self.input_token_chunks.lock().unwrap() += 1;
        *self.input_token_chars.lock().unwrap() += token.len();
        if self
            .active_stream_type()
            .is_some_and(|part_type| part_type != data_proto::SessionMessagePartType::Text)
        {
            self.flush_active_stream_event_buffer().await;
        }
        self.push_active_stream_part(data_proto::SessionMessagePartType::Text, token);
        self.maybe_flush_kv().await;
        if self.should_flush_active_stream_event() {
            self.flush_active_stream_event_buffer().await;
        }
    }

    async fn on_reasoning(&self, reasoning: &str) {
        *self.reasoning_chunks.lock().unwrap() += 1;
        *self.reasoning_chars.lock().unwrap() += reasoning.len();
        if self
            .active_stream_type()
            .is_some_and(|part_type| part_type != data_proto::SessionMessagePartType::Reasoning)
        {
            self.flush_active_stream_event_buffer().await;
        }
        self.push_active_stream_part(data_proto::SessionMessagePartType::Reasoning, reasoning);
        self.maybe_flush_kv().await;
        if self.should_flush_active_stream_event() {
            self.flush_active_stream_event_buffer().await;
        }
    }

    async fn on_tool_call(&self, id: &str, name: &str, input: &Value) {
        *self.tool_calls.lock().unwrap() += 1;
        self.flush_active_stream_event_buffer().await;
        self.close_active_stream_part();
        self.record_part(
            data_proto::SessionMessagePartType::ToolCall,
            name.to_string(),
            "Tool call".to_string(),
            serde_json::to_string(&serde_json::json!({
                "tool_call_id": id,
                "input": input,
            }))
            .unwrap_or_else(|_| "{}".to_string()),
        );
        self.publish_event(AgentEvent::Action {
            id: id.to_string(),
            name: name.to_string(),
            input: input.clone(),
        })
        .await;
    }

    async fn on_tool_result_recorded(&self, id: &str, name: &str, result: &str) -> Result<()> {
        let part_id = self.next_part_id();
        let entry = sessions::append_tool_result(
            self.kv.as_ref(),
            self.objects.as_ref(),
            &self.ns,
            &self.agent_id,
            &self.session_id,
            &self.reply_msg_id,
            &part_id,
            &self.submission_id,
            &self.attempt_id,
            id,
            name,
            result,
            chrono::Utc::now().timestamp_micros(),
        )
        .await?;
        if let Some(payload) = entry
            .payload
            .as_ref()
            .and_then(|payload| payload.payload.as_ref())
            .and_then(|payload| match payload {
                data_proto::session_journal_entry_payload::Payload::ToolResult(result) => {
                    Some(result)
                }
                _ => None,
            })
        {
            self.recorded_tool_results.lock().unwrap().insert(
                id.to_string(),
                StoredToolResult {
                    part_id: part_id.clone(),
                    output: payload.output.clone(),
                    preview: tool_result_preview(result),
                    object: payload.object.clone(),
                },
            );
        }
        *self.latest_journal_entry_id.lock().unwrap() = Some(entry.journal_entry_id);
        Ok(())
    }

    async fn on_tool_result(&self, id: &str, name: &str, result: &str) {
        *self.tool_results.lock().unwrap() += 1;
        self.flush_active_stream_event_buffer().await;
        self.close_active_stream_part();
        let recorded = { self.recorded_tool_results.lock().unwrap().remove(id) };
        let stored = match recorded {
            Some(stored) => stored,
            None => {
                let part_id = self.next_part_id();
                match store_tool_result(
                    self.objects.as_ref(),
                    &self.ns,
                    &self.agent_id,
                    &self.session_id,
                    &self.reply_msg_id,
                    &part_id,
                    id,
                    name,
                    result,
                )
                .await
                {
                    Ok(stored) => stored,
                    Err(err) => {
                        tracing::error!(
                            error = %err,
                            namespace = %self.ns,
                            agent = %self.agent_id,
                            session = %self.session_id,
                            tool_call_id = %id,
                            "Failed to store tool result object"
                        );
                        self.publish_event(AgentEvent::Error(
                            "Error: failed to persist tool result".to_string(),
                        ))
                        .await;
                        return;
                    }
                }
            }
        };
        let payload_json = stored.payload_json(id);
        self.record_part_with_id_and_object(
            stored.part_id,
            data_proto::SessionMessagePartType::ToolResult,
            name.to_string(),
            stored.preview.clone(),
            payload_json,
            stored.object,
        );
        self.publish_event(AgentEvent::Observation {
            id: id.to_string(),
            name: name.to_string(),
            output: result.to_string(),
        })
        .await;
    }

    async fn on_request_permission(&self, id: &str, action: &str, payload: &Value) {
        self.flush_active_stream_event_buffer().await;
        self.close_active_stream_part();
        self.record_part(
            data_proto::SessionMessagePartType::RequestPermission,
            action.to_string(),
            "Permission requested".to_string(),
            serde_json::to_string(&serde_json::json!({
                "requestId": id,
                "status": "pending",
                "request": payload,
            }))
            .unwrap_or_else(|_| "{}".to_string()),
        );
        self.persist_durable_message("request_permission").await;
        self.publish_event(AgentEvent::RequestPermission {
            id: id.to_string(),
            action: action.to_string(),
            payload: payload.clone(),
        })
        .await;
    }

    async fn on_permission_result(&self, id: &str, outcome: &Value) {
        self.flush_active_stream_event_buffer().await;
        self.close_active_stream_part();
        self.record_part(
            data_proto::SessionMessagePartType::PermissionResult,
            String::new(),
            "Permission answered".to_string(),
            serde_json::to_string(&serde_json::json!({
                "requestId": id,
                "status": outcome
                    .get("outcome")
                    .and_then(|value| value.as_str())
                    .unwrap_or("selected"),
                "outcome": outcome,
            }))
            .unwrap_or_else(|_| "{}".to_string()),
        );
        self.persist_durable_message("permission_result").await;
        self.publish_event(AgentEvent::PermissionResult {
            id: id.to_string(),
            outcome: outcome.clone(),
        })
        .await;
    }

    async fn on_usage(&self, usage: &ChatUsage) {
        *self.usage_events.lock().unwrap() += 1;
        self.flush_active_stream_event_buffer().await;
        self.close_active_stream_part();
        self.record_part(
            data_proto::SessionMessagePartType::Usage,
            String::new(),
            String::new(),
            chat_usage_payload_json(usage),
        );
        self.publish_event(AgentEvent::Usage(usage.clone())).await;
    }

    async fn on_done(&self) {
        self.flush_active_stream_event_buffer().await;
        // Final KV write (complete message)
        let parts = match self.final_message_parts() {
            Ok(parts) => parts,
            Err(err) => {
                tracing::error!(error = %err, "Failed to assemble final assistant message parts");
                self.publish_event(AgentEvent::Error(
                    "Error: failed to assemble final assistant message".to_string(),
                ))
                .await;
                return;
            }
        };
        let msg = data_proto::SessionMessage {
            id: self.reply_msg_id.clone(),
            role: data_proto::MessageRole::RoleAssistant as i32,
            created_at: chrono::Utc::now().timestamp_micros(),
            labels: self.projection_labels(sessions::SESSION_PROJECTION_STATE_COMPLETE_UNCOMMITTED),
            parts,
        };
        let result = async {
            let _guard = self.persist_lock.lock().await;
            crate::control::ProtoKeyValueStoreExt::set_msg(
                self.kv.as_ref(),
                &self.reply_msg_key,
                &msg,
            )
            .await
        }
        .instrument(tracing::info_span!(
            "PubSubSessionSink.persist_final_message",
            namespace = %self.ns,
            agent = %self.agent_id,
            session = %self.session_id,
        ))
        .await;
        match result {
            Ok(()) => {
                if self
                    .mark_terminal(SessionSubmissionStatus::Committed as i32)
                    .await
                {
                    let committed_msg = data_proto::SessionMessage {
                        labels: self
                            .projection_labels(sessions::SESSION_PROJECTION_STATE_COMMITTED),
                        ..msg
                    };
                    let commit_result = async {
                        let _guard = self.persist_lock.lock().await;
                        crate::control::ProtoKeyValueStoreExt::set_msg(
                            self.kv.as_ref(),
                            &self.reply_msg_key,
                            &committed_msg,
                        )
                        .await
                    }
                    .await;
                    if let Err(err) = commit_result {
                        tracing::error!(error = %err, "Failed to persist committed projection");
                        self.publish_event(AgentEvent::Error(
                            "Error: failed to persist committed assistant message".to_string(),
                        ))
                        .await;
                        return;
                    }
                    self.publish_reply_index_event().await;
                    self.publish_event(AgentEvent::Done).await;
                } else {
                    self.publish_event(AgentEvent::Error(
                        "Error: failed to mark session submission terminal".to_string(),
                    ))
                    .await;
                }
            }
            Err(e) => {
                tracing::error!("Failed to persist final message: {}", e);
                self.publish_event(AgentEvent::Error(
                    "Error: failed to persist final assistant message".to_string(),
                ))
                .await;
            }
        }
    }

    async fn on_error(&self, err: &str) {
        self.flush_active_stream_event_buffer().await;
        self.close_active_stream_part();

        self.record_part(
            data_proto::SessionMessagePartType::Error,
            String::new(),
            err.to_string(),
            String::new(),
        );
        let msg = data_proto::SessionMessage {
            id: self.reply_msg_id.clone(),
            role: data_proto::MessageRole::RoleAssistant as i32,
            created_at: chrono::Utc::now().timestamp_micros(),
            labels: self.projection_labels(sessions::SESSION_PROJECTION_STATE_FAILED),
            parts: self.durable_parts.lock().unwrap().clone(),
        };
        let result = async {
            let _guard = self.persist_lock.lock().await;
            crate::control::ProtoKeyValueStoreExt::set_msg(
                self.kv.as_ref(),
                &self.reply_msg_key,
                &msg,
            )
            .await
        }
        .instrument(tracing::info_span!(
            "PubSubSessionSink.persist_error_message",
            namespace = %self.ns,
            agent = %self.agent_id,
            session = %self.session_id,
        ))
        .await;
        match result {
            Ok(()) => {
                self.mark_terminal(SessionSubmissionStatus::Failed as i32)
                    .await;
                self.publish_reply_index_event().await;
                self.publish_event(AgentEvent::Error(err.to_string())).await;
            }
            Err(e) => {
                tracing::error!("Failed to persist error message: {}", e);
                self.publish_event(AgentEvent::Error(
                    "Error: failed to persist session error message".to_string(),
                ))
                .await;
            }
        }
    }
}

fn token_publish_interval() -> Duration {
    std::env::var("TALON_TOKEN_BATCH_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(250))
}

#[cfg(test)]
mod tests {
    use super::{token_publish_interval, PubSubSessionSink, StreamingPartBuffer};
    use crate::control::events::{
        IndexEvent, SessionMessagePartEvent, SessionMessagePartEventKind,
    };
    use crate::control::keys::{self, ResourceKey, ResourceList};
    use crate::control::{KeyValueStore, MessagePublisher};
    use crate::gateway::rpc::data_proto;
    use crate::harness::executor::ExecutionSink;
    use crate::harness::llm::ChatUsage;
    use crate::harness::sessions;
    use async_trait::async_trait;
    use futures::StreamExt;
    use prost::Message;
    use serde_json::json;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockKvStore {
        entries: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
        fail_reply_sets_after: Option<usize>,
        reply_set_count: Arc<Mutex<usize>>,
    }

    fn reply_key() -> ResourceKey {
        keys::session_message("conic", "infra", "session-1", "reply-1")
    }

    #[async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, key: &ResourceKey) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self
                .entries
                .lock()
                .await
                .iter()
                .rev()
                .find(|(entry_key, _)| entry_key == &key.to_string())
                .map(|(_, value)| value.clone()))
        }
        async fn set(&self, key: &ResourceKey, value: &[u8]) -> anyhow::Result<()> {
            if key.to_string() == reply_key().to_string() {
                let mut count = self.reply_set_count.lock().await;
                *count += 1;
                if self
                    .fail_reply_sets_after
                    .is_some_and(|limit| *count > limit)
                {
                    anyhow::bail!("injected reply write failure");
                }
            }
            self.entries
                .lock()
                .await
                .push((key.to_string(), value.to_vec()));
            Ok(())
        }
        async fn compare_and_swap(
            &self,
            _k: &ResourceKey,
            _expected: Option<&[u8]>,
            _value: &[u8],
        ) -> anyhow::Result<bool> {
            Ok(true)
        }
        async fn delete(&self, _k: &ResourceKey) -> anyhow::Result<()> {
            Ok(())
        }
        async fn list_keys(&self, _list: &ResourceList) -> anyhow::Result<Vec<ResourceKey>> {
            Ok(vec![])
        }
        async fn list_keys_page(
            &self,
            _list: &ResourceList,
            _before_key: Option<&str>,
            _limit: usize,
        ) -> anyhow::Result<Vec<ResourceKey>> {
            Ok(vec![])
        }
    }

    struct MockPubSub {
        events: Arc<Mutex<Vec<SessionMessagePartEvent>>>,
    }

    #[derive(Default)]
    struct RecordingPubSub {
        published: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    }

    fn event_part(event: &SessionMessagePartEvent) -> &data_proto::SessionMessagePart {
        event.part.as_ref().expect("event part")
    }

    async fn latest_reply_message(kv: &MockKvStore) -> data_proto::SessionMessage {
        kv.entries
            .lock()
            .await
            .iter()
            .filter_map(|(_, value)| data_proto::SessionMessage::decode(value.as_slice()).ok())
            .rev()
            .find(|message| message.id == "reply-1")
            .expect("reply message should be persisted")
    }

    type TestFanoutStream = std::pin::Pin<
        Box<
            dyn futures::Stream<
                    Item = std::result::Result<
                        crate::gateway::rpc::worker_proto::StreamSessionPartsResponse,
                        tonic::Status,
                    >,
                > + Send,
        >,
    >;

    async fn fanout_stream(sink: &PubSubSessionSink) -> TestFanoutStream {
        sink.fanout_hub
            .create_session_attempt(sink.fanout_key.clone())
            .await;
        sink.fanout_hub
            .subscribe_session_parts(&sink.fanout_key, 0)
            .await
            .expect("fanout subscription")
            .into_stream()
    }

    async fn next_fanout_event(stream: &mut TestFanoutStream) -> SessionMessagePartEvent {
        tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("fanout event timed out")
            .expect("fanout stream ended")
            .expect("fanout stream error")
            .event
            .expect("fanout event")
    }

    async fn fanout_events_until_terminal(
        stream: &mut TestFanoutStream,
    ) -> Vec<SessionMessagePartEvent> {
        let mut events = Vec::new();
        loop {
            let event = next_fanout_event(stream).await;
            let terminal = event.kind == SessionMessagePartEventKind::Done as i32
                || event.kind == SessionMessagePartEventKind::Error as i32;
            events.push(event);
            if terminal {
                break;
            }
        }
        events
    }

    #[async_trait]
    impl MessagePublisher for MockPubSub {
        async fn publish(&self, topic: &str, message: &[u8]) -> anyhow::Result<()> {
            if topic == crate::control::topics::INDEX_EVENTS_TOPIC {
                return Ok(());
            }
            let event = SessionMessagePartEvent::decode(message)?;
            self.events.lock().await.push(event);
            Ok(())
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<std::pin::Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>>
        {
            Ok(Box::pin(futures::stream::empty()))
        }
    }

    #[async_trait]
    impl MessagePublisher for RecordingPubSub {
        async fn publish(&self, topic: &str, message: &[u8]) -> anyhow::Result<()> {
            self.published
                .lock()
                .await
                .push((topic.to_string(), message.to_vec()));
            Ok(())
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<std::pin::Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>>
        {
            Ok(Box::pin(futures::stream::empty()))
        }
    }

    #[tokio::test]
    async fn token_events_are_batched_by_time_window() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let mut submission =
            sessions::pending_submission("submission-1", "session-1", "user-1", 100);
        submission.status = data_proto::SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        crate::control::ProtoKeyValueStoreExt::set_msg(
            kv.as_ref(),
            &keys::session_submission("conic", "infra", "session-1", "submission-1"),
            &submission,
        )
        .await
        .unwrap();
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv.clone(),
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_millis(5),
        );

        let mut fanout = fanout_stream(&sink).await;
        sink.on_token("hello").await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        sink.on_token(" world").await;
        sink.on_done().await;

        let events = fanout_events_until_terminal(&mut fanout).await;
        let token_events = events
            .iter()
            .filter(|event| event.kind == SessionMessagePartEventKind::Delta as i32)
            .filter(|event| {
                event_part(event).part_type == data_proto::SessionMessagePartType::Text as i32
            })
            .map(|event| event_part(event).content.clone())
            .collect::<Vec<_>>();

        assert_eq!(token_events, vec!["hello world".to_string()]);
        let done_event = events
            .iter()
            .find(|event| event.kind == SessionMessagePartEventKind::Done as i32)
            .expect("done event should be published");
        assert_eq!(event_part(done_event).content, "");

        let final_message = latest_reply_message(kv.as_ref()).await;
        let persisted_text = final_message
            .parts
            .iter()
            .filter(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
            .map(|part| part.content.clone())
            .collect::<Vec<_>>();
        assert_eq!(persisted_text, vec!["hello world".to_string()]);
    }

    #[tokio::test]
    async fn final_message_persists_accumulated_streamed_text_when_done_reply_is_empty() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv.clone(),
            Arc::new(MockPubSub { events }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_secs(10),
        );

        sink.on_token("The answer is ").await;
        sink.on_token("12.").await;
        sink.on_done().await;

        let entries = kv.entries.lock().await.clone();
        let reply = entries
            .iter()
            .rev()
            .filter_map(|(_, value)| data_proto::SessionMessage::decode(value.as_slice()).ok())
            .find(|message| message.id == "reply-1")
            .expect("reply message should be persisted");
        let reply_text = reply
            .parts
            .iter()
            .filter(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
            .map(|part| part.content.as_str())
            .collect::<String>();
        assert_eq!(reply_text, "The answer is 12.");
    }

    #[tokio::test]
    async fn final_message_uses_streamed_text_before_done() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv.clone(),
            Arc::new(MockPubSub { events }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_secs(10),
        );

        sink.on_token("streamed ").await;
        sink.on_token("answer").await;
        sink.on_done().await;

        let entries = kv.entries.lock().await.clone();
        let reply = entries
            .iter()
            .rev()
            .filter_map(|(_, value)| data_proto::SessionMessage::decode(value.as_slice()).ok())
            .find(|message| message.id == "reply-1")
            .expect("reply message should be persisted");
        let reply_text = reply
            .parts
            .iter()
            .filter(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
            .map(|part| part.content.as_str())
            .collect::<String>();
        assert_eq!(reply_text, "streamed answer");
    }

    #[tokio::test]
    async fn final_assistant_message_publishes_search_index_event() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let mut submission =
            sessions::pending_submission("submission-1", "session-1", "user-1", 100);
        submission.status = data_proto::SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        crate::control::ProtoKeyValueStoreExt::set_msg(
            kv.as_ref(),
            &keys::session_submission("conic", "infra", "session-1", "submission-1"),
            &submission,
        )
        .await
        .unwrap();
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv,
            pubsub.clone(),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_secs(10),
        );

        sink.on_token("final").await;
        sink.on_done().await;

        let published = pubsub.published.lock().await.clone();
        let index_event = published
            .iter()
            .find_map(|(topic, payload)| {
                (topic == crate::control::topics::INDEX_EVENTS_TOPIC)
                    .then(|| IndexEvent::decode(payload.as_slice()).ok())
                    .flatten()
            })
            .expect("assistant reply should publish a search index event");
        assert_eq!(index_event.key, reply_key().canonical());
    }

    #[tokio::test]
    async fn token_buffer_flushes_before_tool_call_boundary() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv.clone(),
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_secs(10),
        );

        let mut fanout = fanout_stream(&sink).await;
        sink.on_token("drafting ").await;
        sink.on_token("request").await;
        sink.on_tool_call("tool-1", "create_prompt", &json!({"content": "x"}))
            .await;

        let events = vec![
            next_fanout_event(&mut fanout).await,
            next_fanout_event(&mut fanout).await,
        ];
        assert_eq!(
            event_part(&events[0]).part_type,
            data_proto::SessionMessagePartType::Text as i32
        );
        assert_eq!(event_part(&events[0]).content, "drafting request");
        assert_eq!(
            event_part(&events[1]).part_type,
            data_proto::SessionMessagePartType::ToolCall as i32
        );
        assert_eq!(event_part(&events[1]).name, "create_prompt");

        sink.on_token("final").await;
        sink.on_done().await;
        let entries = kv.entries.lock().await.clone();
        let reply = entries
            .iter()
            .filter_map(|(_, value)| data_proto::SessionMessage::decode(value.as_slice()).ok())
            .rev()
            .find(|message| message.id == "reply-1")
            .expect("reply message should be persisted");
        let reply_part_contents = reply
            .parts
            .iter()
            .map(|part| part.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            reply_part_contents,
            vec!["drafting request", "Tool call", "final"]
        );
    }

    #[tokio::test]
    async fn reasoning_events_are_batched_by_time_window() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv.clone(),
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_millis(5),
        );

        let mut fanout = fanout_stream(&sink).await;
        sink.on_reasoning("first").await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        sink.on_reasoning(" second").await;
        sink.on_done().await;

        let events = fanout_events_until_terminal(&mut fanout).await;
        let reasoning_events = events
            .iter()
            .filter(|event| {
                event_part(event).part_type == data_proto::SessionMessagePartType::Reasoning as i32
            })
            .map(|event| event_part(event).content.clone())
            .collect::<Vec<_>>();
        assert_eq!(reasoning_events, vec!["first second".to_string()]);

        let entries = kv.entries.lock().await.clone();
        let persisted_reasoning = entries
            .iter()
            .filter_map(|(_, value)| data_proto::SessionMessage::decode(value.as_slice()).ok())
            .flat_map(|message| message.parts)
            .filter(|part| part.part_type == data_proto::SessionMessagePartType::Reasoning as i32)
            .map(|part| part.content)
            .collect::<Vec<_>>();
        assert_eq!(persisted_reasoning, vec!["first second".to_string()]);
    }

    #[tokio::test]
    async fn reasoning_live_batches_do_not_become_durable_parts() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv.clone(),
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_millis(5),
        );

        let mut fanout = fanout_stream(&sink).await;
        sink.on_reasoning("first").await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        sink.on_reasoning(" second").await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        sink.on_reasoning(" third").await;
        sink.on_done().await;

        let events = fanout_events_until_terminal(&mut fanout).await;
        let reasoning_events = events
            .iter()
            .filter(|event| {
                event_part(event).part_type == data_proto::SessionMessagePartType::Reasoning as i32
            })
            .map(|event| event_part(event).content.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            reasoning_events,
            vec!["first second".to_string(), " third".to_string()]
        );

        let entries = kv.entries.lock().await.clone();
        let persisted_reasoning = entries
            .iter()
            .filter_map(|(_, value)| data_proto::SessionMessage::decode(value.as_slice()).ok())
            .flat_map(|message| message.parts)
            .filter(|part| part.part_type == data_proto::SessionMessagePartType::Reasoning as i32)
            .map(|part| part.content)
            .collect::<Vec<_>>();
        assert_eq!(persisted_reasoning, vec!["first second third".to_string()]);
    }

    #[tokio::test]
    async fn streaming_part_boundaries_preserve_mixed_order() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv.clone(),
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_secs(10),
        );

        sink.on_reasoning("planning ").await;
        sink.on_token("drafting ").await;
        sink.on_tool_call("tool-1", "create_prompt", &json!({"content": "x"}))
            .await;
        sink.on_tool_result("tool-1", "create_prompt", "created")
            .await;
        sink.on_reasoning("checking ").await;
        sink.on_token("final").await;
        sink.on_usage(&ChatUsage {
            input_tokens: 10,
            output_tokens: 5,
            reasoning_tokens: 2,
            total_tokens: 17,
        })
        .await;
        sink.on_done().await;

        let entries = kv.entries.lock().await.clone();
        let reply = entries
            .iter()
            .filter_map(|(_, value)| data_proto::SessionMessage::decode(value.as_slice()).ok())
            .rev()
            .find(|message| message.id == "reply-1")
            .expect("reply message should be persisted");
        let reply_parts = reply
            .parts
            .iter()
            .map(|part| {
                (
                    data_proto::SessionMessagePartType::try_from(part.part_type).unwrap(),
                    part.content.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            reply_parts,
            vec![
                (data_proto::SessionMessagePartType::Reasoning, "planning "),
                (data_proto::SessionMessagePartType::Text, "drafting "),
                (data_proto::SessionMessagePartType::ToolCall, "Tool call"),
                (data_proto::SessionMessagePartType::ToolResult, "created"),
                (data_proto::SessionMessagePartType::Reasoning, "checking "),
                (data_proto::SessionMessagePartType::Text, "final"),
                (data_proto::SessionMessagePartType::Usage, ""),
            ]
        );
    }

    #[tokio::test]
    async fn large_tool_results_store_preview_in_content_and_object_in_payload() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv.clone(),
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_secs(10),
        );
        let raw_output = format!(
            "{{\"items\":[{{\"path\":\"footer.tsx\",\"content\":\"{}\"}}]}}",
            "x".repeat(40_000)
        );

        sink.on_tool_result("tool-1", "mcp_github_get_file_contents", &raw_output)
            .await;
        sink.on_done().await;

        let entries = kv.entries.lock().await.clone();
        let persisted = entries
            .iter()
            .filter_map(|(_, value)| data_proto::SessionMessage::decode(value.as_slice()).ok())
            .flat_map(|message| message.parts)
            .find(|part| part.part_type == data_proto::SessionMessagePartType::ToolResult as i32)
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&persisted.payload_json).unwrap();

        assert!(persisted.content.len() < raw_output.len());
        assert!(payload.get("output").is_none());
        assert_eq!(payload["output_preview"], persisted.content);
        assert_eq!(
            payload["output_object_key"],
            persisted.object.as_ref().unwrap().key
        );
        let hydrated = crate::harness::tool_results::hydrate_tool_result(
            sink.objects.as_ref(),
            persisted.object.as_ref(),
            "",
        )
        .await
        .unwrap();
        assert_eq!(hydrated, raw_output);
    }

    #[tokio::test]
    async fn partial_flush_writes_in_progress_session_message_projection() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let mut submission =
            sessions::pending_submission("submission-1", "session-1", "user-1", 100);
        submission.status = data_proto::SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        crate::control::ProtoKeyValueStoreExt::set_msg(
            kv.as_ref(),
            &keys::session_submission("conic", "infra", "session-1", "submission-1"),
            &submission,
        )
        .await
        .unwrap();
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv.clone(),
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_secs(10),
        );

        *sink.last_flush.lock().unwrap() = Instant::now() - Duration::from_secs(2);
        sink.on_token("partial").await;
        *sink.last_flush.lock().unwrap() = Instant::now() - Duration::from_secs(2);
        sink.on_token(" response").await;

        let entries = kv.entries.lock().await.clone();
        let persisted_messages = entries
            .iter()
            .filter_map(|(_, value)| data_proto::SessionMessage::decode(value.as_slice()).ok())
            .collect::<Vec<_>>();
        let projection = persisted_messages
            .iter()
            .rev()
            .find(|message| message.id == "reply-1")
            .expect("projection message should be persisted");
        assert_eq!(
            projection
                .labels
                .get(sessions::SESSION_LABEL_PROJECTION_STATE)
                .map(String::as_str),
            Some(sessions::SESSION_PROJECTION_STATE_IN_PROGRESS)
        );
        let projection_text = projection
            .parts
            .iter()
            .filter(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
            .map(|part| part.content.as_str())
            .collect::<String>();
        assert_eq!(projection_text, "partial response");

        sink.on_done().await;
        let entries = kv.entries.lock().await.clone();
        let reply = entries
            .iter()
            .filter_map(|(_, value)| data_proto::SessionMessage::decode(value.as_slice()).ok())
            .rev()
            .find(|message| message.id == "reply-1")
            .expect("reply message should be persisted");
        let final_text_part = reply
            .parts
            .iter()
            .find(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
            .expect("final text part should exist");
        assert_eq!(final_text_part.content, "partial response");
    }

    #[tokio::test]
    async fn projection_uses_stable_streaming_part_ids() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let mut submission =
            sessions::pending_submission("submission-1", "session-1", "user-1", 100);
        submission.status = data_proto::SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        crate::control::ProtoKeyValueStoreExt::set_msg(
            kv.as_ref(),
            &keys::session_submission("conic", "infra", "session-1", "submission-1"),
            &submission,
        )
        .await
        .unwrap();
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv.clone(),
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_secs(10),
        );

        *sink.last_flush.lock().unwrap() = Instant::now() - Duration::from_secs(2);
        sink.on_reasoning("thinking").await;
        let first_reasoning = latest_reply_message(kv.as_ref())
            .await
            .parts
            .into_iter()
            .find(|part| part.part_type == data_proto::SessionMessagePartType::Reasoning as i32)
            .expect("projection should include reasoning");

        *sink.last_flush.lock().unwrap() = Instant::now() - Duration::from_secs(2);
        sink.on_reasoning(" more").await;
        let second_reasoning = latest_reply_message(kv.as_ref())
            .await
            .parts
            .into_iter()
            .find(|part| part.part_type == data_proto::SessionMessagePartType::Reasoning as i32)
            .expect("projection should include updated reasoning");
        assert_eq!(second_reasoning.id, first_reasoning.id);
        assert_eq!(second_reasoning.content, "thinking more");

        *sink.last_flush.lock().unwrap() = Instant::now() - Duration::from_secs(2);
        sink.on_token("answer").await;
        let projection = latest_reply_message(kv.as_ref()).await;
        let closed_reasoning = projection
            .parts
            .iter()
            .find(|part| part.part_type == data_proto::SessionMessagePartType::Reasoning as i32)
            .expect("projection should keep closed reasoning");
        let first_text = projection
            .parts
            .iter()
            .find(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
            .expect("projection should include text");
        assert_eq!(closed_reasoning.id, first_reasoning.id);

        *sink.last_flush.lock().unwrap() = Instant::now() - Duration::from_secs(2);
        sink.on_token(" now").await;
        let second_text = latest_reply_message(kv.as_ref())
            .await
            .parts
            .into_iter()
            .find(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
            .expect("projection should include updated text");
        assert_eq!(second_text.id, first_text.id);
        assert_eq!(second_text.content, "answer now");
    }

    #[tokio::test]
    async fn final_reply_projection_does_not_commit_streaming_text() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv.clone(),
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_secs(10),
        );

        sink.on_token("hello").await;
        let projection = sink.projection_message(sessions::SESSION_PROJECTION_STATE_IN_PROGRESS);
        let projection_text = projection
            .parts
            .iter()
            .find(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
            .expect("projection should include streamed text");
        assert_eq!(projection_text.content, "hello");

        sink.on_token(" world").await;
        sink.on_done().await;

        let final_message = latest_reply_message(kv.as_ref()).await;
        let final_text = final_message
            .parts
            .iter()
            .find(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
            .expect("final message should include text");
        assert_eq!(final_text.content, "hello world");
    }

    #[tokio::test]
    async fn journal_boundaries_record_stable_llm_responses_and_tool_results() {
        use crate::control::ProtoKeyValueStoreExt;
        use crate::harness::llm::{ChatResponse, ToolCall};

        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(crate::test_support::MockKvStore::default());
        let mut submission =
            sessions::pending_submission("submission-1", "session-1", "user-1", 100);
        submission.status = data_proto::SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        sessions::create_submission_if_absent(
            kv.as_ref(),
            "conic",
            "infra",
            "session-1",
            &submission,
        )
        .await
        .unwrap();
        let sink = PubSubSessionSink::new(
            kv.clone(),
            Arc::new(MockPubSub { events }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
        );

        let tool_calls = vec![
            ToolCall {
                id: "call-a".to_string(),
                name: "search".to_string(),
                arguments: "{\"q\":\"a\"}".to_string(),
            },
            ToolCall {
                id: "call-b".to_string(),
                name: "search".to_string(),
                arguments: "{\"q\":\"b\"}".to_string(),
            },
        ];
        sink.on_llm_response(&ChatResponse {
            content: "first".to_string(),
            tool_calls: tool_calls.clone(),
            usage: None,
        })
        .await
        .unwrap();
        sink.on_tool_result_recorded("call-a", "search", "result-a")
            .await
            .unwrap();
        sink.on_llm_response(&ChatResponse {
            content: "final".to_string(),
            tool_calls: Vec::new(),
            usage: None,
        })
        .await
        .unwrap();
        sink.on_done().await;

        let entry_keys = kv
            .list_keys(&keys::session_journal_entry_prefix(
                "conic",
                "infra",
                "session-1",
                "submission-1",
            ))
            .await
            .unwrap();
        let mut entries = Vec::new();
        for key in entry_keys {
            entries.push(
                kv.get_msg::<sessions::SessionJournalEntry>(&key)
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        let phases = entries.iter().map(|entry| entry.phase).collect::<Vec<_>>();
        assert_eq!(
            phases,
            vec![
                data_proto::SessionExecutionPhase::LlmResponse as i32,
                data_proto::SessionExecutionPhase::ToolResult as i32,
                data_proto::SessionExecutionPhase::LlmResponse as i32,
                data_proto::SessionExecutionPhase::Committed as i32,
            ]
        );
        let Some(data_proto::session_journal_entry_payload::Payload::LlmResponse(response)) =
            entries[0]
                .payload
                .as_ref()
                .and_then(|payload| payload.payload.as_ref())
        else {
            panic!("expected LLM response journal payload");
        };
        assert!(response
            .response
            .as_ref()
            .expect("response")
            .tool_calls
            .iter()
            .any(|tool| tool.id == "call-b"));
        let Some(data_proto::session_journal_entry_payload::Payload::ToolResult(result)) = entries
            [1]
        .payload
        .as_ref()
        .and_then(|payload| payload.payload.as_ref()) else {
            panic!("expected tool-result journal payload");
        };
        assert_eq!(result.output, "result-a");

        let stored_submission = kv
            .get_msg::<sessions::SessionSubmission>(&keys::session_submission(
                "conic",
                "infra",
                "session-1",
                "submission-1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            stored_submission.current_journal_entry_id.as_deref(),
            Some(entries[3].journal_entry_id.as_str())
        );
        assert_eq!(
            stored_submission.current_phase,
            data_proto::SessionExecutionPhase::Committed as i32
        );
    }

    #[tokio::test]
    async fn done_and_error_persist_and_publish_expected_events() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let mut submission =
            sessions::pending_submission("submission-1", "session-1", "user-1", 100);
        submission.status = data_proto::SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        crate::control::ProtoKeyValueStoreExt::set_msg(
            kv.as_ref(),
            &keys::session_submission("conic", "infra", "session-1", "submission-1"),
            &submission,
        )
        .await
        .unwrap();
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv.clone(),
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_secs(10),
        );

        let mut fanout = fanout_stream(&sink).await;
        sink.on_token("partial ").await;
        sink.on_error("tool failed").await;
        sink.on_token("final reply").await;
        sink.on_done().await;
        tokio::time::sleep(Duration::from_millis(25)).await;

        let events = fanout_events_until_terminal(&mut fanout).await;
        assert!(events.iter().any(
            |event| event.kind == SessionMessagePartEventKind::Error as i32
                && event_part(event).content == "tool failed"
        ));
        assert!(!events
            .iter()
            .any(|event| event.kind == SessionMessagePartEventKind::Done as i32));

        let entries = kv.entries.lock().await.clone();
        let persisted_messages = entries
            .iter()
            .filter_map(|(_, value)| {
                crate::gateway::rpc::data_proto::SessionMessage::decode(value.as_slice()).ok()
            })
            .collect::<Vec<_>>();
        assert!(persisted_messages.iter().any(|msg| msg.id == "reply-1"));

        let persisted_parts = entries
            .iter()
            .filter_map(|(_, value)| data_proto::SessionMessage::decode(value.as_slice()).ok())
            .flat_map(|message| message.parts)
            .collect::<Vec<_>>();
        assert!(persisted_parts.iter().any(|part| part.part_type
            == data_proto::SessionMessagePartType::Text as i32
            && part.content == "final reply"));
        assert!(persisted_parts.iter().any(|part| part.part_type
            == data_proto::SessionMessagePartType::Error as i32
            && part.content == "tool failed"));

        let reply_message = persisted_messages
            .iter()
            .rev()
            .find(|msg| msg.id == "reply-1")
            .expect("reply message should be persisted");
        let reply_part_contents = reply_message
            .parts
            .iter()
            .map(|part| part.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            reply_part_contents,
            vec!["partial ", "tool failed", "final reply"]
        );
    }

    #[tokio::test]
    async fn done_publishes_error_when_terminal_mark_fails() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv,
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "missing-submission",
            "attempt-1",
            Duration::from_secs(10),
        );

        let mut fanout = fanout_stream(&sink).await;
        sink.on_done().await;

        let events = fanout_events_until_terminal(&mut fanout).await;
        assert!(events.iter().any(
            |event| event.kind == SessionMessagePartEventKind::Error as i32
                && event_part(event).content == "Error: failed to mark session submission terminal"
        ));
        assert!(!events
            .iter()
            .any(|event| event.kind == SessionMessagePartEventKind::Done as i32));
    }

    #[tokio::test]
    async fn done_publishes_error_when_committed_projection_write_fails() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore {
            fail_reply_sets_after: Some(1),
            ..MockKvStore::default()
        });
        let mut submission =
            sessions::pending_submission("submission-1", "session-1", "user-1", 100);
        submission.status = data_proto::SessionSubmissionStatus::Claimed as i32;
        submission.attempt_id = "attempt-1".to_string();
        crate::control::ProtoKeyValueStoreExt::set_msg(
            kv.as_ref(),
            &keys::session_submission("conic", "infra", "session-1", "submission-1"),
            &submission,
        )
        .await
        .unwrap();
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv,
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_secs(10),
        );

        let mut fanout = fanout_stream(&sink).await;
        sink.on_done().await;

        let events = fanout_events_until_terminal(&mut fanout).await;
        assert!(events.iter().any(
            |event| event.kind == SessionMessagePartEventKind::Error as i32
                && event_part(event).content
                    == "Error: failed to persist committed assistant message"
        ));
        assert!(!events
            .iter()
            .any(|event| event.kind == SessionMessagePartEventKind::Done as i32));
    }

    #[tokio::test]
    async fn summary_tracks_tokens_tool_calls_and_results() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv,
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_millis(1),
        );

        sink.on_token("hi").await;
        tokio::time::sleep(Duration::from_millis(2)).await;
        sink.on_token(" there").await;
        sink.on_tool_call("tool-1", "search", &json!({"q": "talon"}))
            .await;
        sink.on_tool_result("tool-1", "search", "result body").await;
        sink.on_done().await;

        let summary = sink.summary();
        assert_eq!(summary.input_token_chunks, 2);
        assert_eq!(summary.input_token_chars, "hi there".len());
        assert!(summary.published_token_batches >= 1);
        assert!(summary.published_token_chars >= "hi there".len());
        assert_eq!(summary.tool_calls, 1);
        assert_eq!(summary.tool_results, 1);
        assert!(summary.duration_ms <= 10_000);
    }

    #[test]
    fn token_publish_interval_uses_env_override_and_defaults() {
        let _guard = crate::test_support::env_lock();
        std::env::remove_var("TALON_TOKEN_BATCH_MS");
        assert_eq!(token_publish_interval(), Duration::from_millis(250));

        std::env::set_var("TALON_TOKEN_BATCH_MS", "5");
        assert_eq!(token_publish_interval(), Duration::from_millis(5));

        std::env::set_var("TALON_TOKEN_BATCH_MS", "0");
        assert_eq!(token_publish_interval(), Duration::from_millis(250));

        std::env::set_var("TALON_TOKEN_BATCH_MS", "not-a-number");
        assert_eq!(token_publish_interval(), Duration::from_millis(250));

        std::env::remove_var("TALON_TOKEN_BATCH_MS");
    }

    #[test]
    fn streaming_part_buffer_rejects_repeated_final_close() {
        let mut buffer = StreamingPartBuffer::new(data_proto::SessionMessagePartType::Text);
        buffer.push("hello");

        let first = buffer
            .final_part(|| "part-1".to_string())
            .expect("first final close should succeed");
        assert!(first.is_some());

        let second = buffer.final_part(|| "part-2".to_string());
        assert!(second.is_err());
    }
}
