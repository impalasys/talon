// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;

use crate::control::cas::CasStore;
use crate::control::delegation;
use crate::control::events::{SessionMessagePartEvent, SessionMessagePartEventKind};
use crate::control::object_store::{default_object_store, ObjectStore};
use crate::control::resources::ResourceStore;
use crate::control::session_queue;
use crate::control::tool_output::{self, ToolOutputExt, ToolOutputStorageContext};
use crate::control::{
    keys::{self, ResourceKey},
    KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
};
use crate::gateway::rpc::data_proto::{self, SessionSubmissionStatus};
use crate::gateway::rpc::resources_proto;
use crate::harness::executor::{AgentEvent, ExecutionSink};
use crate::harness::llm::{ChatResponse, TokenCounter, ToolOutput};
use crate::harness::sessions::{self, SessionSubmission};
use crate::worker::fanout::{FanoutHub, SessionFanoutKey};
use tracing::Instrument;

fn chat_usage_payload_json(usage: &TokenCounter) -> String {
    serde_json::to_string(&serde_json::json!({
        "input_tokens": usage.input_tokens,
        "cached_input_tokens": usage.cached_input_tokens,
        "cache_write_tokens": usage.cache_write_tokens,
        "output_tokens": usage.output_tokens,
        "reasoning_output_tokens": usage.reasoning_output_tokens,
        "total_tokens": usage.total_tokens,
        "usage_available": usage.usage_available,
        "provider_request_id": usage.provider_request_id,
        "provider": usage.provider,
        "model": usage.model,
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

#[derive(Debug, Clone, PartialEq)]
struct RecordedToolResult {
    part_id: String,
    output: ToolOutput,
}

#[derive(Debug, Clone, PartialEq)]
struct InlineArtifactTag {
    start: usize,
    end: usize,
    title: String,
    media_type: String,
    content: String,
}

fn extract_inline_artifact_tags(text: &str) -> Vec<InlineArtifactTag> {
    const OPEN_TAG: &str = "<artifact";
    const CLOSE_TAG: &str = "</artifact>";

    let mut tags = Vec::new();
    let mut cursor = 0;
    while let Some(open) = find_ascii_case_insensitive(text, OPEN_TAG, cursor) {
        let after_name = open + OPEN_TAG.len();
        let valid_name_boundary = text[after_name..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_whitespace() || ch == '>');
        if !valid_name_boundary {
            cursor = after_name;
            continue;
        }

        let Some(open_end_rel) = text[open..].find('>') else {
            break;
        };
        let open_end = open + open_end_rel + 1;
        if text[open + 1..open_end - 1].contains('<') {
            cursor = after_name;
            continue;
        }
        let Some(close) = find_ascii_case_insensitive(text, CLOSE_TAG, open_end) else {
            break;
        };
        if let Some(next_open) = find_ascii_case_insensitive(text, OPEN_TAG, open_end) {
            if next_open < close {
                cursor = open_end;
                continue;
            }
        }
        let end = close + CLOSE_TAG.len();
        let attrs = artifact_tag_attrs(&text[after_name..open_end - 1]);
        let content = text[open_end..close].trim().to_string();
        if !content.is_empty() {
            tags.push(InlineArtifactTag {
                start: open,
                end,
                title: attrs
                    .get("name")
                    .or_else(|| attrs.get("title"))
                    .cloned()
                    .unwrap_or_else(|| "Artifact".to_string()),
                media_type: attrs
                    .get("type")
                    .or_else(|| attrs.get("media_type"))
                    .cloned()
                    .unwrap_or_else(|| "text/markdown".to_string()),
                content,
            });
        }
        cursor = end;
    }
    tags
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str, start: usize) -> Option<usize> {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() || start > haystack.len() || needle.len() > haystack.len() {
        return None;
    }
    haystack[start..]
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
        .map(|index| start + index)
}

fn artifact_tag_attrs(input: &str) -> HashMap<String, String> {
    let chars = input.chars().collect::<Vec<_>>();
    let mut attrs = HashMap::new();
    let mut cursor = 0;
    while cursor < chars.len() {
        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }
        let key_start = cursor;
        while cursor < chars.len()
            && (chars[cursor].is_ascii_alphanumeric()
                || chars[cursor] == '_'
                || chars[cursor] == '-')
        {
            cursor += 1;
        }
        if cursor == key_start {
            cursor += 1;
            continue;
        }
        let key = chars[key_start..cursor]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase();
        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }
        if cursor >= chars.len() || chars[cursor] != '=' {
            continue;
        }
        cursor += 1;
        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }
        if cursor >= chars.len() || (chars[cursor] != '"' && chars[cursor] != '\'') {
            continue;
        }
        let quote = chars[cursor];
        cursor += 1;
        let value_start = cursor;
        while cursor < chars.len() && chars[cursor] != quote {
            cursor += 1;
        }
        if cursor >= chars.len() {
            break;
        }
        let value = chars[value_start..cursor].iter().collect::<String>();
        cursor += 1;
        attrs.insert(key, value);
    }
    attrs
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
    pub reply_msg_id: Mutex<String>,
    pub reply_msg_key: Mutex<ResourceKey>,

    // Durable work identity. Journal writes are fenced by `attempt_id` so a
    // worker whose lease expired cannot keep appending state after reclaim.
    pub(crate) claim: sessions::SubmissionLease,
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
    recorded_tool_results: Mutex<std::collections::HashMap<String, RecordedToolResult>>,
    steer_drains: Mutex<u8>,
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
        let claim = sessions::SubmissionLease {
            ns: ns.clone(),
            agent: agent_id.clone(),
            session_id: session_id.clone(),
            submission_id: submission_id.clone(),
            attempt_id: attempt_id.clone(),
            ttl_micros: 0,
        };
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
            reply_msg_id: Mutex::new(reply_msg_id.into()),
            reply_msg_key: Mutex::new(reply_msg_key),
            claim,
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
            steer_drains: Mutex::new(0),
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

    pub fn current_reply_msg_id(&self) -> String {
        self.reply_msg_id.lock().unwrap().clone()
    }

    pub fn current_reply_msg_key(&self) -> ResourceKey {
        self.reply_msg_key.lock().unwrap().clone()
    }

    pub(crate) fn rotate_reply_message(&self, message_id: String) {
        let key = crate::control::keys::session_message(
            &self.ns,
            &self.agent_id,
            &self.session_id,
            &message_id,
        );
        *self.reply_msg_id.lock().unwrap() = message_id;
        *self.reply_msg_key.lock().unwrap() = key;
        self.durable_parts.lock().unwrap().clear();
        self.active_stream_buffer.lock().unwrap().take();
        self.recorded_tool_results.lock().unwrap().clear();
        *self.next_part_index.lock().unwrap() = 0;
    }

    pub(crate) fn advance_next_part_id_past(&self, part_id: &str) {
        if let Ok(index) = part_id.parse::<u64>() {
            let mut next = self.next_part_index.lock().unwrap();
            *next = (*next).max(index);
        }
    }

    pub(crate) fn seed_latest_journal_entry_id(&self, entry_id: Option<&str>) {
        *self.latest_journal_entry_id.lock().unwrap() = entry_id.map(str::to_string);
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

    pub(crate) fn seed_recovered_encrypted_reasoning_part(
        &self,
        part_id: &str,
        object: data_proto::ObjectRef,
    ) {
        self.record_part_with_id_and_object(
            part_id.to_string(),
            data_proto::SessionMessagePartType::EncryptedReasoning,
            String::new(),
            String::new(),
            String::new(),
            Some(object),
        );
    }

    pub(crate) fn seed_recovered_final_encrypted_reasoning_part(
        &self,
        object: data_proto::ObjectRef,
    ) {
        self.seed_recovered_encrypted_reasoning_part(&self.next_part_id(), object);
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
        let output = tool_output::normalize_for_session_storage(
            &CasStore::new(self.objects.clone()),
            ToolOutputStorageContext {
                ns: &self.ns,
                agent: &self.agent_id,
                session_id: &self.session_id,
                message_id: &self.current_reply_msg_id(),
                part_id,
                tool_call_id: id,
                tool_name: name,
            },
            &ToolOutput::text(result),
        )
        .await?;
        let object = tool_output::first_object_ref(&output).cloned();
        let payload_json =
            tool_output::tool_result_payload_json(id, &output).unwrap_or_else(|_| "{}".to_string());
        self.record_part_with_id_and_object(
            part_id.to_string(),
            data_proto::SessionMessagePartType::ToolResult,
            name.to_string(),
            String::new(),
            payload_json,
            object,
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

    async fn materialize_inline_artifact_tags(
        &self,
        parts: &mut [data_proto::SessionMessagePart],
    ) -> anyhow::Result<Vec<String>> {
        let mut uris = Vec::new();
        for part in parts.iter_mut() {
            if part.part_type != data_proto::SessionMessagePartType::Text as i32 {
                continue;
            }
            let tags = extract_inline_artifact_tags(&part.content);
            if tags.is_empty() {
                continue;
            }

            let mut rewritten = String::with_capacity(part.content.len());
            let mut cursor = 0;
            for tag in tags {
                rewritten.push_str(&part.content[cursor..tag.start]);
                let artifact_uri = self.create_inline_artifact(&tag, uris.len()).await?;
                rewritten.push_str(&artifact_uri);
                cursor = tag.end;
                uris.push(artifact_uri);
            }
            rewritten.push_str(&part.content[cursor..]);
            part.content = rewritten;
        }
        Ok(uris)
    }

    async fn create_inline_artifact(
        &self,
        tag: &InlineArtifactTag,
        index: usize,
    ) -> anyhow::Result<String> {
        let artifact_id = format!("artifact-{}-inline-{index:03}", self.current_reply_msg_id());
        let mut metadata = HashMap::new();
        metadata.insert(
            "source".to_string(),
            "assistant-inline-artifact".to_string(),
        );
        let object_ref = CasStore::new(self.objects.clone())
            .put_artifact(
                &self.ns,
                &self.agent_id,
                &self.session_id,
                &artifact_id,
                tag.content.as_bytes(),
                &tag.media_type,
                metadata.clone(),
            )
            .await?;
        let artifact = data_proto::Artifact {
            id: artifact_id.clone(),
            session_id: self.session_id.clone(),
            title: tag.title.clone(),
            media_type: tag.media_type.clone(),
            object_ref: Some(object_ref),
            created_by_agent: self.agent_id.clone(),
            created_at: chrono::Utc::now().timestamp_micros(),
            labels: HashMap::new(),
            metadata,
        };
        crate::harness::native_tools::artifacts::record_artifact_revision(
            self.kv.as_ref(),
            &self.ns,
            &self.agent_id,
            &self.session_id,
            &artifact_id,
            artifact
                .object_ref
                .as_ref()
                .expect("inline artifact object ref"),
        )
        .await?;
        self.kv
            .set_msg(
                &keys::artifact(&self.ns, &self.agent_id, &self.session_id, &artifact_id),
                &artifact,
            )
            .await?;
        Ok(format!(
            "artifact://{}/{}/{}/{}",
            self.ns, self.agent_id, self.session_id, artifact_id
        ))
    }

    async fn attach_inline_artifacts_to_delegated_task(
        &self,
        artifact_uris: &[String],
    ) -> anyhow::Result<()> {
        if artifact_uris.is_empty() {
            return Ok(());
        }
        let Some(session) = self
            .kv
            .get_msg::<data_proto::Session>(&keys::session(
                &self.ns,
                &self.agent_id,
                &self.session_id,
            ))
            .await?
        else {
            return Ok(());
        };
        let is_delegate = session
            .labels
            .get(delegation::LABEL_TASK_ROLE)
            .map(String::as_str)
            == Some("delegate");
        if !is_delegate {
            return Ok(());
        }
        let Some(task_namespace) = session.labels.get(delegation::LABEL_TASK_NAMESPACE) else {
            return Ok(());
        };
        let Some(task_name) = session.labels.get(delegation::LABEL_TASK_NAME) else {
            return Ok(());
        };

        let now = chrono::Utc::now().timestamp_micros();
        let store = ResourceStore::new(self.kv.clone(), self.pubsub.clone());
        store
            .patch_status_with(task_namespace, "Task", task_name, None, |_, status| {
                let mut task_status = match status.kind.take() {
                    Some(resources_proto::resource_status::Kind::Task(task_status)) => task_status,
                    _ => resources_proto::TaskStatus::default(),
                };
                for uri in artifact_uris {
                    if !task_status.output_artifact_uris.contains(uri) {
                        task_status.output_artifact_uris.push(uri.clone());
                    }
                }
                task_status.updated_at = now;
                status.kind = Some(resources_proto::resource_status::Kind::Task(task_status));
                Ok(())
            })
            .await
            .with_context(|| {
                format!(
                    "failed to attach inline artifact outputs to Task {task_namespace}/{task_name}"
                )
            })?;
        Ok(())
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
            message_id: self.current_reply_msg_id(),
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
            id: self.current_reply_msg_id(),
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
                    &self.current_reply_msg_key(),
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
            id: self.current_reply_msg_id(),
            role: data_proto::MessageRole::RoleAssistant as i32,
            created_at: chrono::Utc::now().timestamp_micros(),
            labels: self.projection_labels(sessions::SESSION_PROJECTION_STATE_COMPLETE_UNCOMMITTED),
            parts: self.durable_parts.lock().unwrap().clone(),
        };
        let result = async {
            let _guard = self.persist_lock.lock().await;
            crate::control::ProtoKeyValueStoreExt::set_msg(
                self.kv.as_ref(),
                &self.current_reply_msg_key(),
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

    async fn finalize_assistant_segment(&self) -> Result<String> {
        self.flush_active_stream_event_buffer().await;
        let parts = self.final_message_parts()?;
        let message_id = self.current_reply_msg_id();
        let message = data_proto::SessionMessage {
            id: message_id.clone(),
            role: data_proto::MessageRole::RoleAssistant as i32,
            created_at: chrono::Utc::now().timestamp_micros(),
            labels: self.projection_labels(sessions::SESSION_PROJECTION_STATE_COMMITTED),
            parts,
        };
        let key = self.current_reply_msg_key();
        let _guard = self.persist_lock.lock().await;
        crate::control::ProtoKeyValueStoreExt::set_msg(self.kv.as_ref(), &key, &message).await?;
        drop(_guard);
        self.publish_reply_index_event().await;
        Ok(message_id)
    }

    async fn publish_reply_index_event(&self) {
        if let Err(error) = crate::control::search::publish_index_event(
            self.pubsub.as_ref(),
            crate::control::events::IndexEvent {
                operation: crate::control::events::IndexOperation::Upsert as i32,
                key: self.current_reply_msg_key().canonical(),
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
                message_id = %self.current_reply_msg_id(),
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
            &self.current_reply_msg_id(),
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

    pub async fn clear_provider_continuation(&self) -> Result<()> {
        sessions::clear_provider_request_id(self.kv.as_ref(), &self.claim).await
    }
}

#[async_trait]
impl ExecutionSink for PubSubSessionSink {
    async fn on_llm_response(&self, response: &ChatResponse) -> Result<()> {
        // A Responses API response that asks for tools is waiting for matching
        // function_call_output values on the provider. It remains usable only
        // in this live executor turn; durable recovery must reconstruct from
        // local history instead of reviving that incomplete continuation.
        let mut durable_response = response.clone();
        if !durable_response.tool_calls.is_empty() {
            if let Some(counter) = durable_response.usage.as_mut() {
                counter.provider_request_id = None;
            }
        }
        let entry = sessions::append_llm_response(
            self.kv.as_ref(),
            &self.ns,
            &self.agent_id,
            &self.session_id,
            &self.submission_id,
            &self.attempt_id,
            &self.current_reply_msg_id(),
            &durable_response,
            chrono::Utc::now().timestamp_micros(),
        )
        .await?;
        *self.latest_journal_entry_id.lock().unwrap() = Some(entry.journal_entry_id);
        if let Some(object) = response.encrypted_reasoning.clone() {
            self.record_part_with_id_and_object(
                self.next_part_id(),
                data_proto::SessionMessagePartType::EncryptedReasoning,
                String::new(),
                String::new(),
                String::new(),
                Some(object),
            );
        }
        if let Some(counter) = response.usage.as_ref() {
            // A Responses API response containing function calls is not a
            // durable continuation point until every call has been answered.
            // Keep the ID in the executor's in-memory counter for this turn,
            // but never let cancellation/crash recovery resume this server-side
            // response without its required function_call_output items.
            let mut counter = counter.clone();
            if !response.tool_calls.is_empty() {
                counter.provider_request_id = None;
            }
            if let Err(error) =
                sessions::persist_context_tokens(self.kv.as_ref(), &self.claim, &counter).await
            {
                tracing::error!(
                    error = %error,
                    namespace = %self.ns,
                    agent = %self.agent_id,
                    session = %self.session_id,
                    "failed to persist session context token snapshot"
                );
            }
        }
        Ok(())
    }

    async fn on_compaction(&self, summary: &str) -> Result<()> {
        let (entry, summary_object) = sessions::append_compaction(
            self.kv.as_ref(),
            &CasStore::new(self.objects.clone()),
            &self.ns,
            &self.agent_id,
            &self.session_id,
            &self.submission_id,
            &self.attempt_id,
            summary,
            chrono::Utc::now().timestamp_micros(),
        )
        .await?;
        // This canonical marker carries the summary ObjectRef used by history
        // reconstruction and is exposed to session clients unchanged.
        self.record_part_with_id_and_object(
            self.next_part_id(),
            data_proto::SessionMessagePartType::Compaction,
            String::new(),
            String::new(),
            String::new(),
            Some(summary_object),
        );
        *self.latest_journal_entry_id.lock().unwrap() = Some(entry.journal_entry_id);
        sessions::clear_provider_request_id(self.kv.as_ref(), &self.claim).await?;
        // Make the marker visible to subsequent-session reconstruction before
        // replacing the executor's live context. This projection is canonical;
        // public RPC paths expose the same compaction metadata.
        self.persist_durable_message("compaction").await;
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
        self.persist_durable_message("tool_call").await;
        self.publish_event(AgentEvent::Action {
            id: id.to_string(),
            name: name.to_string(),
            input: input.clone(),
        })
        .await;
    }

    async fn on_tool_call_stream_started(&self) {
        self.flush_active_stream_event_buffer().await;
    }

    async fn on_tool_result_recorded(
        &self,
        id: &str,
        name: &str,
        result: &ToolOutput,
    ) -> Result<()> {
        let part_id = self.next_part_id();
        let cas = CasStore::new(self.objects.clone());
        let entry = sessions::append_tool_result(
            self.kv.as_ref(),
            &cas,
            &self.ns,
            &self.agent_id,
            &self.session_id,
            &self.current_reply_msg_id(),
            &part_id,
            &self.submission_id,
            &self.attempt_id,
            id,
            name,
            result,
            chrono::Utc::now().timestamp_micros(),
        )
        .await?;
        let output = entry
            .payload
            .as_ref()
            .and_then(|payload| payload.payload.as_ref())
            .and_then(|payload| match payload {
                data_proto::session_journal_entry_payload::Payload::ToolResult(result) => {
                    result.tool_output.clone()
                }
                _ => None,
            })
            .unwrap_or_else(|| result.clone());
        self.recorded_tool_results.lock().unwrap().insert(
            id.to_string(),
            RecordedToolResult {
                part_id: part_id.clone(),
                output,
            },
        );
        *self.latest_journal_entry_id.lock().unwrap() = Some(entry.journal_entry_id);
        Ok(())
    }

    async fn take_steering_messages(&self) -> Result<Vec<crate::harness::executor::LoopMessage>> {
        const MAX_STEER_DRAINS: u8 = 4;
        let drain_attempt = {
            let mut drains = self.steer_drains.lock().unwrap();
            if *drains >= MAX_STEER_DRAINS {
                return Ok(Vec::new());
            }
            *drains += 1;
            *drains
        };

        let mut batch = session_queue::prepare_steer_batch(
            self.kv.as_ref(),
            &self.ns,
            &self.agent_id,
            &self.session_id,
            session_queue::DEFAULT_STEER_BATCH_MAX_MESSAGES,
            session_queue::DEFAULT_STEER_BATCH_MAX_CHARS,
            chrono::Utc::now(),
        )
        .await?;
        if batch.is_empty() && drain_attempt == 1 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            batch = session_queue::prepare_steer_batch(
                self.kv.as_ref(),
                &self.ns,
                &self.agent_id,
                &self.session_id,
                session_queue::DEFAULT_STEER_BATCH_MAX_MESSAGES,
                session_queue::DEFAULT_STEER_BATCH_MAX_CHARS,
                chrono::Utc::now(),
            )
            .await?;
        }
        if batch.is_empty() {
            return Ok(Vec::new());
        }

        let message_ids = batch
            .iter()
            .map(|message| message.message_id.clone())
            .collect::<Vec<_>>();
        // Choose the assistant side of the checkpoint without creating a
        // transcript gap. Real output is finalized, an already committed
        // segment is reused after recovery, and an owned partial projection is
        // discarded so pre-output steering can leave the previous ID empty.
        let has_active_assistant_output = !self.durable_parts.lock().unwrap().is_empty()
            || self
                .active_stream_buffer
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|buffer| !buffer.unclosed_content().is_empty());
        let previous_assistant_message_id = if has_active_assistant_output {
            self.finalize_assistant_segment().await?
        } else {
            let key = self.current_reply_msg_key();
            match self.kv.get_msg::<data_proto::SessionMessage>(&key).await? {
                Some(message) => {
                    let projection_state = message
                        .labels
                        .get(sessions::SESSION_LABEL_PROJECTION_STATE)
                        .map(String::as_str);
                    let projection_submission_id = message
                        .labels
                        .get(sessions::SESSION_LABEL_SUBMISSION_ID)
                        .map(String::as_str);
                    let belongs_to_submission = projection_submission_id
                        .is_none_or(|submission_id| submission_id == self.submission_id);

                    if message.role == data_proto::MessageRole::RoleAssistant as i32
                        && projection_state == Some(sessions::SESSION_PROJECTION_STATE_COMMITTED)
                        && belongs_to_submission
                    {
                        message.id
                    } else {
                        if projection_submission_id == Some(self.submission_id.as_str())
                            && matches!(
                                projection_state,
                                Some(sessions::SESSION_PROJECTION_STATE_IN_PROGRESS)
                                    | Some(sessions::SESSION_PROJECTION_STATE_COMPLETE_UNCOMMITTED)
                            )
                        {
                            self.kv.delete(&key).await?;
                        }
                        String::new()
                    }
                }
                None => String::new(),
            }
        };

        // The steer messages were assigned their UUID7 ids while preparing the
        // batch. Allocate the continuation only after them so session-key
        // ordering matches conversational ordering.
        let next_assistant_message_id = crate::control::uuid::session_message_id();
        sessions::append_steer_input(
            self.kv.as_ref(),
            &self.ns,
            &self.agent_id,
            &self.session_id,
            &self.submission_id,
            &self.attempt_id,
            &message_ids,
            &previous_assistant_message_id,
            &next_assistant_message_id,
            chrono::Utc::now().timestamp_micros(),
        )
        .await?;
        session_queue::commit_steer_batch(
            self.kv.as_ref(),
            &self.ns,
            &self.agent_id,
            &self.session_id,
            &batch,
        )
        .await?;
        self.rotate_reply_message(next_assistant_message_id);
        Ok(batch
            .into_iter()
            .map(|message| {
                crate::harness::executor::LoopMessage::text(
                    "user",
                    crate::control::scheduling::session_message_text_projection(&message.message),
                )
            })
            .collect())
    }

    async fn on_tool_result(&self, id: &str, name: &str, result: &ToolOutput) {
        *self.tool_results.lock().unwrap() += 1;
        self.flush_active_stream_event_buffer().await;
        self.close_active_stream_part();
        let result_output = tool_output::display_text(result);
        let recorded = { self.recorded_tool_results.lock().unwrap().remove(id) };
        let stored = match recorded {
            Some(stored) => stored,
            None => {
                let part_id = self.next_part_id();
                let cas = CasStore::new(self.objects.clone());
                let output = match tool_output::normalize_for_session_storage(
                    &cas,
                    ToolOutputStorageContext {
                        ns: &self.ns,
                        agent: &self.agent_id,
                        session_id: &self.session_id,
                        message_id: &self.current_reply_msg_id(),
                        part_id: &part_id,
                        tool_call_id: id,
                        tool_name: name,
                    },
                    result,
                )
                .await
                {
                    Ok(output) => output,
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
                };
                RecordedToolResult { part_id, output }
            }
        };
        let object = tool_output::first_object_ref(&stored.output).cloned();
        let payload_json = tool_output::tool_result_payload_json(id, &stored.output)
            .unwrap_or_else(|_| "{}".to_string());
        self.record_part_with_id_and_object(
            stored.part_id,
            data_proto::SessionMessagePartType::ToolResult,
            name.to_string(),
            String::new(),
            payload_json,
            object,
        );
        self.publish_event(AgentEvent::Observation {
            id: id.to_string(),
            name: name.to_string(),
            output: result_output,
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

    async fn on_usage(&self, usage: &TokenCounter) {
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
        let mut parts = match self.final_message_parts() {
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
        let inline_artifact_uris = match self.materialize_inline_artifact_tags(&mut parts).await {
            Ok(uris) => uris,
            Err(err) => {
                tracing::error!(error = %err, "Failed to materialize inline artifact tags");
                self.publish_event(AgentEvent::Error(
                    "Error: failed to create inline artifact".to_string(),
                ))
                .await;
                return;
            }
        };
        if let Err(err) = self
            .attach_inline_artifacts_to_delegated_task(&inline_artifact_uris)
            .await
        {
            tracing::error!(error = %err, "Failed to attach inline artifacts to delegated Task");
            self.publish_event(AgentEvent::Error(
                "Error: failed to attach inline artifact to task".to_string(),
            ))
            .await;
            return;
        }
        let msg = data_proto::SessionMessage {
            id: self.current_reply_msg_id(),
            role: data_proto::MessageRole::RoleAssistant as i32,
            created_at: chrono::Utc::now().timestamp_micros(),
            labels: self.projection_labels(sessions::SESSION_PROJECTION_STATE_COMPLETE_UNCOMMITTED),
            parts,
        };
        let result = async {
            let _guard = self.persist_lock.lock().await;
            crate::control::ProtoKeyValueStoreExt::set_msg(
                self.kv.as_ref(),
                &self.current_reply_msg_key(),
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
                            &self.current_reply_msg_key(),
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
            id: self.current_reply_msg_id(),
            role: data_proto::MessageRole::RoleAssistant as i32,
            created_at: chrono::Utc::now().timestamp_micros(),
            labels: self.projection_labels(sessions::SESSION_PROJECTION_STATE_FAILED),
            parts: self.durable_parts.lock().unwrap().clone(),
        };
        let result = async {
            let _guard = self.persist_lock.lock().await;
            crate::control::ProtoKeyValueStoreExt::set_msg(
                self.kv.as_ref(),
                &self.current_reply_msg_key(),
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
    use super::{
        extract_inline_artifact_tags, token_publish_interval, PubSubSessionSink,
        StreamingPartBuffer,
    };
    use crate::control::events::{
        IndexEvent, SessionMessagePartEvent, SessionMessagePartEventKind,
    };
    use crate::control::keys::{self, ResourceKey, ResourceList};
    use crate::control::object_store::{InMemoryObjectStore, ObjectStore};
    use crate::control::tool_output::ToolOutputExt;
    use crate::control::{KeyValueStore, MessagePublisher};
    use crate::gateway::rpc::data_proto;
    use crate::harness::executor::ExecutionSink;
    use crate::harness::llm::TokenCounter;
    use crate::harness::llm::ToolOutput;
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
        async fn list_keys(
            &self,
            _list: &ResourceList,
            _options: Option<crate::control::ListOptions<'_>>,
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

    async fn latest_artifact(kv: &MockKvStore) -> data_proto::Artifact {
        kv.entries
            .lock()
            .await
            .iter()
            .filter_map(|(_, value)| data_proto::Artifact::decode(value.as_slice()).ok())
            .rev()
            .next()
            .expect("artifact should be persisted")
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

    #[test]
    fn inline_artifact_parser_extracts_multiple_tags_and_large_content() {
        let large_body = "a".repeat(10_500);
        let text = format!(
            "before <artifact name=\"Redline\" type=\"text/markdown\"># Draft\n{large_body}</artifact> middle <artifact title='Summary' media_type='text/plain'>done</artifact> after"
        );

        let tags = extract_inline_artifact_tags(&text);

        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].title, "Redline");
        assert_eq!(tags[0].media_type, "text/markdown");
        assert!(tags[0].content.ends_with(&large_body));
        assert_eq!(tags[1].title, "Summary");
        assert_eq!(tags[1].media_type, "text/plain");
        assert_eq!(tags[1].content, "done");
    }

    #[test]
    fn inline_artifact_parser_skips_malformed_tags_without_crossing_boundaries() {
        let text = concat!(
            "bad <artifact name=\"Broken\" ",
            "middle <artifact name=\"Valid\" type=\"text/plain\">done</artifact> after"
        );

        let tags = extract_inline_artifact_tags(text);

        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].title, "Valid");
        assert_eq!(tags[0].media_type, "text/plain");
        assert_eq!(tags[0].content, "done");
    }

    #[tokio::test]
    async fn final_message_materializes_inline_artifact_tags() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let objects = Arc::new(InMemoryObjectStore::default());
        let sink = PubSubSessionSink::new_inner(
            kv.clone(),
            Arc::new(MockPubSub { events }),
            objects.clone(),
            None,
            None,
            "conic",
            "session-1",
            "infra",
            "reply-1",
            reply_key(),
            "submission-1",
            "attempt-1",
            Duration::from_secs(10),
        );

        sink.on_token(
            "Review complete: <artifact name=\"Redline\" type=\"text/markdown\"># Redline\n\n- edit</artifact>",
        )
        .await;
        sink.on_done().await;

        let final_message = latest_reply_message(kv.as_ref()).await;
        let text = final_message
            .parts
            .iter()
            .filter(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
            .map(|part| part.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            text,
            "Review complete: artifact://conic/infra/session-1/artifact-reply-1-inline-000"
        );

        let artifact = latest_artifact(kv.as_ref()).await;
        assert_eq!(artifact.id, "artifact-reply-1-inline-000");
        assert_eq!(artifact.title, "Redline");
        assert_eq!(artifact.media_type, "text/markdown");
        let object_ref = artifact.object_ref.expect("artifact object ref");
        let object = objects
            .get(&object_ref.key)
            .await
            .unwrap()
            .expect("artifact object");
        assert_eq!(object.bytes, b"# Redline\n\n- edit");
        let revision_key = keys::artifact_revision(
            "conic",
            "infra",
            "session-1",
            &format!("{}-{}", artifact.id, object_ref.sha256),
        )
        .to_string();
        assert!(kv
            .entries
            .lock()
            .await
            .iter()
            .any(|(key, _)| key == &revision_key));
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

        let projection = latest_reply_message(kv.as_ref()).await;
        assert_eq!(
            projection
                .labels
                .get(sessions::SESSION_LABEL_PROJECTION_STATE)
                .map(String::as_str),
            Some(sessions::SESSION_PROJECTION_STATE_COMPLETE_UNCOMMITTED)
        );
        let projection_part_contents = projection
            .parts
            .iter()
            .map(|part| part.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            projection_part_contents,
            vec!["drafting request", "Tool call"]
        );

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
    async fn token_buffer_flushes_when_tool_call_stream_starts() {
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
        sink.on_tool_call_stream_started().await;

        let event = next_fanout_event(&mut fanout).await;
        assert_eq!(
            event_part(&event).part_type,
            data_proto::SessionMessagePartType::Text as i32
        );
        assert_eq!(event_part(&event).content, "drafting request");

        sink.on_tool_call("tool-1", "create_prompt", &json!({"content": "x"}))
            .await;
        let tool_event = next_fanout_event(&mut fanout).await;
        assert_eq!(
            event_part(&tool_event).part_type,
            data_proto::SessionMessagePartType::ToolCall as i32
        );

        let projection = latest_reply_message(kv.as_ref()).await;
        let projection_part_contents = projection
            .parts
            .iter()
            .map(|part| part.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            projection_part_contents,
            vec!["drafting request", "Tool call"]
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
        sink.on_tool_result("tool-1", "create_prompt", &ToolOutput::text("created"))
            .await;
        sink.on_reasoning("checking ").await;
        sink.on_token("final").await;
        sink.on_usage(&TokenCounter {
            input_tokens: 10,
            cached_input_tokens: 4,
            cache_write_tokens: 6,
            output_tokens: 5,
            reasoning_output_tokens: 2,
            total_tokens: 17,
            ..Default::default()
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
                (data_proto::SessionMessagePartType::ToolResult, ""),
                (data_proto::SessionMessagePartType::Reasoning, "checking "),
                (data_proto::SessionMessagePartType::Text, "final"),
                (data_proto::SessionMessagePartType::Usage, ""),
            ]
        );
        let tool_result_payload = reply
            .parts
            .iter()
            .find(|part| part.part_type == data_proto::SessionMessagePartType::ToolResult as i32)
            .and_then(|part| serde_json::from_str::<serde_json::Value>(&part.payload_json).ok())
            .expect("tool result payload should parse");
        assert_eq!(
            tool_result_payload["tool_output"]["content_parts"][0]["text"],
            "created"
        );
        assert!(tool_result_payload.get("output_preview").is_none());
        let usage_payload = reply
            .parts
            .iter()
            .find(|part| part.part_type == data_proto::SessionMessagePartType::Usage as i32)
            .and_then(|part| serde_json::from_str::<serde_json::Value>(&part.payload_json).ok())
            .expect("usage payload should parse");
        assert_eq!(usage_payload["cached_input_tokens"], 4);
        assert_eq!(usage_payload["cache_write_tokens"], 6);
    }

    #[tokio::test]
    async fn large_tool_results_store_empty_content_and_object_in_payload() {
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

        sink.on_tool_result(
            "tool-1",
            "mcp_github_get_file_contents",
            &ToolOutput::text(raw_output.clone()),
        )
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

        assert_eq!(persisted.content, "");
        assert!(payload.get("output").is_none());
        assert!(payload.get("output_preview").is_none());
        assert_eq!(
            payload["tool_output"]["content_parts"][0]["object_ref"]["key"],
            persisted.object.as_ref().unwrap().key
        );
        let object = persisted.object.as_ref().unwrap();
        let stored = sink.objects.get(&object.key).await.unwrap().unwrap();
        let hydrated = String::from_utf8(
            crate::control::cas::decode_stored_object_bytes(&stored, &object.key).unwrap(),
        )
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
    async fn live_parts_advance_past_recovered_part_ids() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv,
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

        sink.seed_recovered_text_part("000003", "recovered");
        sink.advance_next_part_id_past("000003");
        sink.on_token(" live").await;

        let parts = sink.final_message_parts().unwrap();
        assert_eq!(parts[0].id, "000003");
        assert_eq!(parts[1].id, "000004");
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
            encrypted_reasoning: None,
        })
        .await
        .unwrap();
        sink.on_tool_result_recorded("call-a", "search", &ToolOutput::text("result-a"))
            .await
            .unwrap();
        sink.on_llm_response(&ChatResponse {
            content: "final".to_string(),
            tool_calls: Vec::new(),
            usage: None,
            encrypted_reasoning: None,
        })
        .await
        .unwrap();
        sink.on_done().await;

        let entry_keys = kv
            .list_keys(
                &keys::session_journal_entry_prefix("conic", "infra", "session-1", "submission-1"),
                None,
            )
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
        assert_eq!(result.output, "");
        assert_eq!(
            result
                .tool_output
                .as_ref()
                .and_then(crate::control::tool_output::plain_text)
                .as_deref(),
            Some("result-a")
        );

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
    async fn steering_before_first_response_does_not_persist_empty_assistant() {
        use crate::control::session_queue::{self, STEER_QUEUE};
        use crate::control::ProtoKeyValueStoreExt;

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
        let initial_reply_id = crate::control::uuid::session_message_id();
        let initial_reply_key =
            keys::session_message("conic", "infra", "session-1", &initial_reply_id);
        for message in ["first follow-up", "second follow-up"] {
            session_queue::queue_text_message(
                kv.as_ref(),
                "conic",
                "infra",
                "session-1",
                STEER_QUEUE,
                message,
                Default::default(),
                chrono::Utc::now(),
            )
            .await
            .unwrap();
        }
        let sink = PubSubSessionSink::new(
            kv.clone(),
            Arc::new(MockPubSub { events }),
            "conic",
            "session-1",
            "infra",
            initial_reply_id.clone(),
            initial_reply_key.clone(),
            "submission-1",
            "attempt-1",
        );

        let steering = sink.take_steering_messages().await.unwrap();

        assert_eq!(steering.len(), 2);
        assert!(kv
            .get_msg::<data_proto::SessionMessage>(&initial_reply_key)
            .await
            .unwrap()
            .is_none());
        let entries = sessions::list_journal_entries(
            kv.as_ref(),
            "conic",
            "infra",
            "session-1",
            "submission-1",
        )
        .await
        .unwrap();
        assert_eq!(entries.len(), 1);
        let Some(data_proto::session_journal_entry_payload::Payload::SteerInput(steer)) = entries
            [0]
        .payload
        .as_ref()
        .and_then(|payload| payload.payload.as_ref()) else {
            panic!("expected steer-input journal payload");
        };
        assert!(steer.previous_assistant_message_id.is_empty());
        assert_eq!(steer.next_assistant_message_id, sink.current_reply_msg_id());
        assert!(steer
            .message_ids
            .iter()
            .all(|message_id| message_id < &steer.next_assistant_message_id));
        assert_ne!(sink.current_reply_msg_id(), initial_reply_id);
    }

    #[tokio::test]
    async fn empty_steering_queue_does_not_persist_projection_or_journal() {
        use crate::control::ProtoKeyValueStoreExt;

        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(crate::test_support::MockKvStore::default());
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

        let steering = sink.take_steering_messages().await.unwrap();

        assert!(steering.is_empty());
        assert!(kv
            .get_msg::<data_proto::SessionMessage>(&reply_key())
            .await
            .unwrap()
            .is_none());
        assert!(sessions::list_journal_entries(
            kv.as_ref(),
            "conic",
            "infra",
            "session-1",
            "submission-1",
        )
        .await
        .unwrap()
        .is_empty());
    }

    #[tokio::test]
    async fn recovered_tool_only_segment_is_finalized_before_steering() {
        use crate::control::session_queue::{self, STEER_QUEUE};
        use crate::control::ProtoKeyValueStoreExt;

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
        session_queue::queue_text_message(
            kv.as_ref(),
            "conic",
            "infra",
            "session-1",
            STEER_QUEUE,
            "change direction",
            Default::default(),
            chrono::Utc::now(),
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
        sink.seed_recovered_tool_call_part("000001", "tool-1", "lookup", &json!({}));

        let steering = sink.take_steering_messages().await.unwrap();

        assert_eq!(steering.len(), 1);
        let assistant = kv
            .get_msg::<data_proto::SessionMessage>(&reply_key())
            .await
            .unwrap()
            .expect("recovered assistant should be finalized");
        assert_eq!(assistant.parts.len(), 1);
        assert_eq!(
            assistant.parts[0].part_type,
            data_proto::SessionMessagePartType::ToolCall as i32
        );
        assert_eq!(
            assistant
                .labels
                .get(sessions::SESSION_LABEL_PROJECTION_STATE)
                .map(String::as_str),
            Some(sessions::SESSION_PROJECTION_STATE_COMMITTED)
        );
        let entries = sessions::list_journal_entries(
            kv.as_ref(),
            "conic",
            "infra",
            "session-1",
            "submission-1",
        )
        .await
        .unwrap();
        let steer = entries[0]
            .payload
            .as_ref()
            .and_then(|payload| payload.payload.as_ref())
            .and_then(|payload| match payload {
                data_proto::session_journal_entry_payload::Payload::SteerInput(steer) => {
                    Some(steer)
                }
                _ => None,
            })
            .expect("steer-input payload");
        assert_eq!(steer.previous_assistant_message_id, "reply-1");
    }

    #[tokio::test]
    async fn recovery_steer_preserves_an_already_finalized_assistant_segment() {
        use crate::control::session_queue::{self, STEER_QUEUE};
        use crate::control::ProtoKeyValueStoreExt;

        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(crate::test_support::MockKvStore::default());
        kv.set_msg(
            &keys::session("conic", "infra", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "infra".to_string(),
                ns: "conic".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 1,
                last_active: 1,
                metadata: Default::default(),
                labels: Default::default(),
                skill_state: None,
                context_tokens: None,
            },
        )
        .await
        .unwrap();
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
        let committed_part = data_proto::SessionMessagePart {
            part_type: data_proto::SessionMessagePartType::Text as i32,
            content: "completed tool turn".to_string(),
            ..Default::default()
        };
        kv.set_msg(
            &reply_key(),
            &data_proto::SessionMessage {
                id: "reply-1".to_string(),
                role: data_proto::MessageRole::RoleAssistant as i32,
                created_at: 2,
                labels: std::collections::HashMap::from([(
                    sessions::SESSION_LABEL_PROJECTION_STATE.to_string(),
                    sessions::SESSION_PROJECTION_STATE_COMMITTED.to_string(),
                )]),
                parts: vec![committed_part.clone()],
            },
        )
        .await
        .unwrap();
        session_queue::queue_text_message(
            kv.as_ref(),
            "conic",
            "infra",
            "session-1",
            STEER_QUEUE,
            "change direction",
            Default::default(),
            chrono::Utc::now(),
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

        let steering = sink.take_steering_messages().await.unwrap();

        assert_eq!(steering.len(), 1);
        let unused_continuation_message_id = sink.current_reply_msg_id();
        kv.set_msg(
            &keys::session_message(
                "conic",
                "infra",
                "session-1",
                &unused_continuation_message_id,
            ),
            &data_proto::SessionMessage {
                id: unused_continuation_message_id.clone(),
                role: data_proto::MessageRole::RoleAssistant as i32,
                labels: std::collections::HashMap::from([
                    (
                        sessions::SESSION_LABEL_SUBMISSION_ID.to_string(),
                        "submission-1".to_string(),
                    ),
                    (
                        sessions::SESSION_LABEL_PROJECTION_STATE.to_string(),
                        sessions::SESSION_PROJECTION_STATE_IN_PROGRESS.to_string(),
                    ),
                ]),
                parts: vec![data_proto::SessionMessagePart {
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: "partial abandoned continuation".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        session_queue::queue_text_message(
            kv.as_ref(),
            "conic",
            "infra",
            "session-1",
            STEER_QUEUE,
            "arrived during restart",
            Default::default(),
            chrono::Utc::now(),
        )
        .await
        .unwrap();
        let late_steering = sink.take_steering_messages().await.unwrap();
        assert_eq!(late_steering.len(), 1);
        assert_ne!(sink.current_reply_msg_id(), unused_continuation_message_id);
        assert!(kv
            .get_msg::<data_proto::SessionMessage>(&keys::session_message(
                "conic",
                "infra",
                "session-1",
                &unused_continuation_message_id,
            ))
            .await
            .unwrap()
            .is_none());
        let committed = kv
            .get_msg::<data_proto::SessionMessage>(&reply_key())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(committed.parts, vec![committed_part]);
        assert_eq!(
            committed
                .labels
                .get(sessions::SESSION_LABEL_PROJECTION_STATE)
                .map(String::as_str),
            Some(sessions::SESSION_PROJECTION_STATE_COMMITTED)
        );
        let entries = sessions::list_journal_entries(
            kv.as_ref(),
            "conic",
            "infra",
            "session-1",
            "submission-1",
        )
        .await
        .unwrap();
        let steers = entries
            .iter()
            .filter_map(|entry| {
                entry
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.payload.as_ref())
                    .and_then(|payload| match payload {
                        data_proto::session_journal_entry_payload::Payload::SteerInput(steer) => {
                            Some(steer)
                        }
                        _ => None,
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(steers.len(), 2);
        assert_eq!(steers[0].previous_assistant_message_id, "reply-1");
        assert!(steers[1].previous_assistant_message_id.is_empty());
        assert_eq!(
            steers[0].next_assistant_message_id,
            unused_continuation_message_id
        );
        assert_eq!(
            steers[1].next_assistant_message_id,
            sink.current_reply_msg_id()
        );
        assert!(kv
            .list_keys(
                &keys::session_queue_prefix("conic", "infra", "session-1", STEER_QUEUE),
                None,
            )
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn latest_llm_response_replaces_session_context_tokens() {
        use crate::control::ProtoKeyValueStoreExt;
        use crate::harness::llm::ChatResponse;

        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(crate::test_support::MockKvStore::default());
        let session_key = keys::session("conic", "infra", "session-1");
        kv.set_msg(
            &session_key,
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "infra".to_string(),
                ns: "conic".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 1,
                last_active: 1,
                metadata: Default::default(),
                labels: Default::default(),
                skill_state: None,
                context_tokens: None,
            },
        )
        .await
        .unwrap();
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
        let first = TokenCounter {
            input_tokens: 10,
            output_tokens: 2,
            total_tokens: 12,
            usage_available: true,
            provider_request_id: Some("request-first".to_string()),
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
            ..Default::default()
        };
        let second = TokenCounter {
            input_tokens: 21,
            cached_input_tokens: 5,
            output_tokens: 3,
            reasoning_output_tokens: 1,
            total_tokens: 25,
            usage_available: true,
            provider_request_id: Some("request-second".to_string()),
            provider: "anthropic".to_string(),
            model: "claude-test".to_string(),
            ..Default::default()
        };
        for counter in [&first, &second] {
            sink.on_llm_response(&ChatResponse {
                content: String::new(),
                tool_calls: Vec::new(),
                usage: Some(counter.clone()),
                encrypted_reasoning: None,
            })
            .await
            .unwrap();
        }
        let session = kv
            .get_msg::<data_proto::Session>(&session_key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.context_tokens, Some(second));
        let journal_entries = sessions::list_journal_entries(
            kv.as_ref(),
            "conic",
            "infra",
            "session-1",
            "submission-1",
        )
        .await
        .unwrap();
        assert_eq!(journal_entries.len(), 2);
    }

    #[tokio::test]
    async fn tool_call_response_does_not_persist_provider_continuation_id() {
        use crate::control::ProtoKeyValueStoreExt;
        use crate::harness::llm::{ChatResponse, ToolCall};

        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(crate::test_support::MockKvStore::default());
        let session_key = keys::session("conic", "infra", "session-1");
        kv.set_msg(
            &session_key,
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "infra".to_string(),
                ns: "conic".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 1,
                last_active: 1,
                metadata: Default::default(),
                labels: Default::default(),
                skill_state: None,
                context_tokens: None,
            },
        )
        .await
        .unwrap();
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
        sink.on_llm_response(&ChatResponse {
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "lookup".to_string(),
                arguments: "{}".to_string(),
            }],
            usage: Some(TokenCounter {
                provider_request_id: Some("resp-pending-tool".to_string()),
                provider: "openai".to_string(),
                model: "gpt-test".to_string(),
                ..Default::default()
            }),
            encrypted_reasoning: None,
        })
        .await
        .unwrap();
        let session = kv
            .get_msg::<data_proto::Session>(&session_key)
            .await
            .unwrap()
            .unwrap();
        assert!(session
            .context_tokens
            .unwrap()
            .provider_request_id
            .is_none());
        let entries = sessions::list_journal_entries(
            kv.as_ref(),
            "conic",
            "infra",
            "session-1",
            "submission-1",
        )
        .await
        .unwrap();
        let persisted_response = entries[0]
            .payload
            .as_ref()
            .and_then(|payload| payload.payload.as_ref())
            .and_then(|payload| match payload {
                data_proto::session_journal_entry_payload::Payload::LlmResponse(response) => {
                    response.response.as_ref()
                }
                _ => None,
            })
            .expect("LLM response should be journaled");
        assert!(persisted_response
            .usage
            .as_ref()
            .and_then(|usage| usage.provider_request_id.as_ref())
            .is_none());
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
        sink.on_tool_result("tool-1", "search", &ToolOutput::text("result body"))
            .await;
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

    #[tokio::test]
    async fn llm_response_projects_encrypted_reasoning_as_object_only_part() {
        use crate::harness::llm::ChatResponse;

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
        let reasoning = data_proto::ObjectRef {
            key: "cas/conic/infra/session-1/encrypted-reasoning.bin".to_string(),
            media_type: "application/octet-stream".to_string(),
            size_bytes: 42,
            ..Default::default()
        };

        sink.on_llm_response(&ChatResponse {
            content: "answer".to_string(),
            tool_calls: Vec::new(),
            usage: None,
            encrypted_reasoning: Some(reasoning.clone()),
        })
        .await
        .unwrap();
        sink.on_done().await;

        let message = latest_reply_message(kv.as_ref()).await;
        let part = message
            .parts
            .iter()
            .find(|part| {
                part.part_type == data_proto::SessionMessagePartType::EncryptedReasoning as i32
            })
            .expect("encrypted reasoning part");
        assert_eq!(part.content, "");
        assert_eq!(part.payload_json, "");
        assert_eq!(part.object.as_ref(), Some(&reasoning));
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
