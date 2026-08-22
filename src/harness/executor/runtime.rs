// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::cas::CasStore;
use crate::control::config::Config;
use crate::control::tool_output::{self, is_text_object_media_type, ToolOutputExt};
use crate::control::ControlPlane;
use crate::harness::executor::compaction::{
    compact, compact_history_for_llm_with_model_limits_and_tool_schema, context_metrics,
    serialized_message_weight, ContextBudget, ContextMetrics,
};
use crate::harness::llm::resolver::{model_context_limits, resolve_model_profile};
use crate::harness::llm::ToolOutput;
use crate::harness::llm::{
    chat_content_part, chat_stream_event, object_ref_part, provider_error_token_counter, text_part,
    ChatContentPart, ChatMessage, ChatRequest, ChatResponse, ChatStreamEvent, LlmProvider,
    TokenCounter, ToolCall,
};
use crate::harness::mcp::{call_tool_for_config, McpConnectionConfig};
use crate::harness::skills::{
    namespace::{find_effective_skill, load_available_skills, load_skill_instructions},
    registry::ToolRegistry,
    render::format_active_skill_context,
};
use crate::harness::telemetry;
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

const DEFAULT_EXECUTION_TURN_LIMIT: usize = 25;
const LLM_PREFLIGHT_METRICS: &[&str] = &[
    crate::control::usage::METRIC_LLM_REQUESTS,
    crate::control::usage::METRIC_LLM_INPUT_TOKENS,
    crate::control::usage::METRIC_LLM_OUTPUT_TOKENS,
    crate::control::usage::METRIC_LLM_REASONING_TOKENS,
    crate::control::usage::METRIC_LLM_TOTAL_TOKENS,
];

fn tool_error_result(name: &str, error: &anyhow::Error) -> String {
    serde_json::json!({
        "ok": false,
        "tool": name,
        "error": error.to_string(),
    })
    .to_string()
}

// ─── Message types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoopMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_parts: Vec<ChatContentPart>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct ExecutedToolCall {
    result: ToolOutput,
    stop_after_result: bool,
}

impl LoopMessage {
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        let content = content.into();
        Self {
            role: role.into(),
            content_parts: if content.is_empty() {
                Vec::new()
            } else {
                vec![text_part(content)]
            },
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn text_content(&self) -> String {
        crate::harness::llm::content_parts_text(&self.content_parts)
    }

    pub fn text_len_chars(&self) -> usize {
        self.content_parts
            .iter()
            .filter_map(|part| match part.content.as_ref() {
                Some(chat_content_part::Content::Text(text)) => Some(text.chars().count()),
                _ => None,
            })
            .sum()
    }

    pub fn is_empty_content(&self) -> bool {
        self.content_parts
            .iter()
            .all(|part| match part.content.as_ref() {
                Some(chat_content_part::Content::Text(text)) => text.is_empty(),
                None => true,
                _ => false,
            })
    }
}

/// Events emitted by the executor during a run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum AgentEvent {
    Reasoning(String),
    Action {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    Observation {
        id: String,
        name: String,
        output: String,
    },
    RequestPermission {
        id: String,
        action: String,
        payload: serde_json::Value,
    },
    PermissionResult {
        id: String,
        outcome: serde_json::Value,
    },
    Token(String),
    Usage(TokenCounter),
    Done,
    Error(String),
}

// ─── ExecutionContext ─────────────────────────────────────────────────────────

/// In-memory conversation context for a single agent execution.
/// Contains only what the LLM loop actually needs: identity for logging
/// and the message history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContext {
    pub agent_id: String,
    pub history: Vec<LoopMessage>,
}

impl ExecutionContext {
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            history: Vec::new(),
        }
    }

    pub fn with_history(agent_id: impl Into<String>, history: Vec<LoopMessage>) -> Self {
        Self {
            agent_id: agent_id.into(),
            history,
        }
    }

    pub fn push(&mut self, msg: LoopMessage) {
        self.history.push(msg);
    }

    pub fn push_many(&mut self, msgs: Vec<LoopMessage>) {
        for msg in msgs {
            self.history.push(msg);
        }
    }

    pub fn take_history(&mut self) -> Vec<LoopMessage> {
        std::mem::take(&mut self.history)
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub fn has_system_message(&self) -> bool {
        self.history.iter().any(|msg| msg.role == "system")
    }

    pub fn prepend_system_message(&mut self, msg: LoopMessage) {
        self.history.insert(0, msg);
    }

    pub fn push_user_text_if_missing(&mut self, text: &str) {
        let already_present = self
            .history
            .iter()
            .any(|msg| msg.role == "user" && msg.text_content() == text);
        if !already_present {
            self.history.push(LoopMessage::text("user", text));
        }
    }
}

fn prefix_latest_user_message(history: &mut [LoopMessage], prefix: &str) {
    let Some(message) = history.iter_mut().rev().find(|msg| msg.role == "user") else {
        return;
    };
    let prefix = format!("{prefix}\n\n");

    if let Some(first_part) = message.content_parts.first_mut() {
        if let Some(chat_content_part::Content::Text(text)) = first_part.content.as_mut() {
            text.insert_str(0, &prefix);
            return;
        }
    }

    message.content_parts.insert(0, text_part(prefix));
}

fn infer_media_type_for_object_ref(
    object_ref: &crate::gateway::rpc::data_proto::ObjectRef,
) -> Option<String> {
    [&object_ref.filename, &object_ref.key]
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .find_map(|value| mime_guess::from_path(value).first_raw().map(str::to_string))
}

fn is_image_object_media_type(media_type: &str) -> bool {
    media_type.trim().to_ascii_lowercase().starts_with("image/")
}

// ─── ExecutionSink ────────────────────────────────────────────────────────────

/// Receives structured events from the executor. Implement this to fan out to
/// PubSub, accumulate for tests, log to stdout, etc.
#[async_trait]
pub trait ExecutionSink: Send + Sync {
    /// A streaming text chunk from the model.
    async fn on_token(&self, token: &str);
    /// A reasoning chunk from the model.
    async fn on_reasoning(&self, reasoning: &str);
    /// The agent chose to call a tool.
    async fn on_tool_call(&self, id: &str, name: &str, input: &Value);
    /// The model has started emitting tool-call deltas. Implementations can
    /// flush any buffered live output before the tool call is fully assembled.
    async fn on_tool_call_stream_started(&self) {}
    /// The full completed LLM response reached a durable recovery boundary.
    async fn on_llm_response(&self, _: &crate::harness::llm::ChatResponse) -> Result<()> {
        Ok(())
    }
    /// An immutable LLM-written summary has been durably recorded.
    async fn on_compaction(&self, _: &str) -> Result<()> {
        Ok(())
    }
    /// The tool returned a result.
    async fn on_tool_result(&self, id: &str, name: &str, result: &ToolOutput);
    /// A tool result has been durably recorded.
    async fn on_tool_result_recorded(&self, _: &str, _: &str, _: &ToolOutput) -> Result<()> {
        Ok(())
    }
    /// Claim and return interactive inputs that should be incorporated before
    /// the next LLM request in the active execution.
    async fn take_steering_messages(&self) -> Result<Vec<LoopMessage>> {
        Ok(Vec::new())
    }
    /// The agent requested permission from the user/client.
    async fn on_request_permission(&self, _: &str, _: &str, _: &Value) {}
    /// The permission request was answered or cancelled.
    async fn on_permission_result(&self, _: &str, _: &Value) {}
    /// Usage metadata for the completed model turn.
    async fn on_usage(&self, usage: &TokenCounter);
    /// The execution completed successfully.
    async fn on_done(&self);
    /// The execution failed.
    async fn on_error(&self, err: &str);
}

/// No-op sink. Use when you only care about the return value.
pub struct NullSink;

#[async_trait]
impl ExecutionSink for NullSink {
    async fn on_token(&self, _: &str) {}
    async fn on_reasoning(&self, _: &str) {}
    async fn on_tool_call(&self, _: &str, _: &str, _: &Value) {}
    async fn on_llm_response(&self, _: &crate::harness::llm::ChatResponse) -> Result<()> {
        Ok(())
    }
    async fn on_tool_result(&self, _: &str, _: &str, _: &ToolOutput) {}
    async fn on_tool_result_recorded(&self, _: &str, _: &str, _: &ToolOutput) -> Result<()> {
        Ok(())
    }
    async fn on_request_permission(&self, _: &str, _: &str, _: &Value) {}
    async fn on_permission_result(&self, _: &str, _: &Value) {}
    async fn on_usage(&self, _: &TokenCounter) {}
    async fn on_done(&self) {}
    async fn on_error(&self, _: &str) {}
}

/// Test sink that captures all events for assertion.
pub struct CaptureSink {
    pub events: std::sync::Mutex<Vec<AgentEvent>>,
    compactions: std::sync::Mutex<Vec<String>>,
    fail_compaction: bool,
}

impl CaptureSink {
    pub fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
            compactions: std::sync::Mutex::new(Vec::new()),
            fail_compaction: false,
        }
    }

    pub fn events(&self) -> Vec<AgentEvent> {
        self.events.lock().unwrap().clone()
    }

    pub fn compactions(&self) -> Vec<String> {
        self.compactions.lock().unwrap().clone()
    }

    pub fn failing_compaction() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
            compactions: std::sync::Mutex::new(Vec::new()),
            fail_compaction: true,
        }
    }
}

#[async_trait]
impl ExecutionSink for CaptureSink {
    async fn on_compaction(&self, summary: &str) -> Result<()> {
        self.compactions.lock().unwrap().push(summary.to_string());
        if self.fail_compaction {
            anyhow::bail!("injected compaction persistence failure");
        }
        Ok(())
    }
    async fn on_token(&self, token: &str) {
        self.events
            .lock()
            .unwrap()
            .push(AgentEvent::Token(token.to_string()));
    }
    async fn on_reasoning(&self, reasoning: &str) {
        self.events
            .lock()
            .unwrap()
            .push(AgentEvent::Reasoning(reasoning.to_string()));
    }
    async fn on_tool_call(&self, id: &str, name: &str, input: &Value) {
        self.events.lock().unwrap().push(AgentEvent::Action {
            id: id.to_string(),
            name: name.to_string(),
            input: input.clone(),
        });
    }
    async fn on_tool_result(&self, id: &str, name: &str, result: &ToolOutput) {
        self.events.lock().unwrap().push(AgentEvent::Observation {
            id: id.to_string(),
            name: name.to_string(),
            output: tool_output::display_text(result),
        });
    }
    async fn on_tool_result_recorded(&self, _: &str, _: &str, _: &ToolOutput) -> Result<()> {
        Ok(())
    }
    async fn on_request_permission(&self, id: &str, action: &str, payload: &Value) {
        self.events
            .lock()
            .unwrap()
            .push(AgentEvent::RequestPermission {
                id: id.to_string(),
                action: action.to_string(),
                payload: payload.clone(),
            });
    }
    async fn on_permission_result(&self, id: &str, outcome: &Value) {
        self.events
            .lock()
            .unwrap()
            .push(AgentEvent::PermissionResult {
                id: id.to_string(),
                outcome: outcome.clone(),
            });
    }
    async fn on_usage(&self, usage: &TokenCounter) {
        self.events
            .lock()
            .unwrap()
            .push(AgentEvent::Usage(usage.clone()));
    }
    async fn on_done(&self) {
        self.events.lock().unwrap().push(AgentEvent::Done);
    }
    async fn on_error(&self, err: &str) {
        self.events
            .lock()
            .unwrap()
            .push(AgentEvent::Error(err.to_string()));
    }
}

// ─── ContextAssembler ─────────────────────────────────────────────────────────

/// Builds the system prompt from SOUL.md, USER.md, AGENTS.md.
#[derive(Clone)]
pub struct ContextAssembler {
    pub base_dir: PathBuf,
    pub skill_context: String,
}

impl ContextAssembler {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
            skill_context: String::new(),
        }
    }

    pub fn new_with_skill_context(
        base_dir: impl Into<PathBuf>,
        skill_context: impl Into<String>,
    ) -> Self {
        Self {
            base_dir: base_dir.into(),
            skill_context: skill_context.into(),
        }
    }

    async fn read_file_or_default(&self, name: &str) -> String {
        let path = self.base_dir.join(name);
        tokio::fs::read_to_string(&path)
            .await
            .unwrap_or_else(|_| format!("(No {} provided)", name))
    }

    pub async fn assemble(&self) -> Result<String> {
        let soul = self.read_file_or_default("SOUL.md").await;
        let user = self.read_file_or_default("USER.md").await;
        let agents = self.read_file_or_default("AGENTS.md").await;
        let mut context = format!(
            "# IDENTITY & PERSONALITY (SOUL.md)\n{}\n\n# USER CONTEXT (USER.md)\n{}\n\n# OPERATIONAL RULES (AGENTS.md)\n{}\n",
            soul, user, agents
        );
        if !self.skill_context.trim().is_empty() {
            context.push('\n');
            context.push_str(self.skill_context.trim());
            context.push('\n');
        }
        Ok(context)
    }
}

// ─── AgentExecutor ────────────────────────────────────────────────────────────

pub struct AgentExecutor {
    pub llm: Arc<dyn LlmProvider>,
    pub llm_provider_key: String,
    pub llm_model: String,
    pub assembler: ContextAssembler,
    pub registry: Arc<tokio::sync::RwLock<ToolRegistry>>,
    pub config: Arc<Config>,
    pub namespace: String,
    pub agent_id: String,
    pub session_id: String,
    pub context_tokens: Option<TokenCounter>,
    pub control_plane: ControlPlane,
    pub agent_spec: crate::gateway::rpc::manifests::AgentSpec,
    pub mcp_tools: HashMap<String, RegisteredMcpTool>,
}

#[derive(Debug, Clone)]
pub struct RegisteredMcpTool {
    pub config: McpConnectionConfig,
    pub remote_name: String,
}

#[derive(Debug, Default)]
struct ExecutionPrompts {
    system_prompt: Option<String>,
    post_history_prompt: Option<String>,
}

impl AgentExecutor {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        llm_provider_key: String,
        llm_model: String,
        assembler: ContextAssembler,
        registry: Arc<tokio::sync::RwLock<ToolRegistry>>,
        config: Arc<Config>,
        namespace: String,
        agent_id: String,
        control_plane: ControlPlane,
        agent_spec: crate::gateway::rpc::manifests::AgentSpec,
        mcp_tools: HashMap<String, RegisteredMcpTool>,
    ) -> Self {
        Self::new_with_session(
            llm,
            llm_provider_key,
            llm_model,
            assembler,
            registry.clone(),
            config,
            namespace,
            agent_id,
            String::new(),
            None,
            control_plane,
            agent_spec,
            mcp_tools,
        )
    }

    pub fn new_with_session(
        llm: Arc<dyn LlmProvider>,
        llm_provider_key: String,
        llm_model: String,
        assembler: ContextAssembler,
        registry: Arc<tokio::sync::RwLock<ToolRegistry>>,
        config: Arc<Config>,
        namespace: String,
        agent_id: String,
        session_id: String,
        context_tokens: Option<TokenCounter>,
        control_plane: ControlPlane,
        agent_spec: crate::gateway::rpc::manifests::AgentSpec,
        mcp_tools: HashMap<String, RegisteredMcpTool>,
    ) -> Self {
        Self {
            llm,
            llm_provider_key,
            llm_model,
            assembler,
            registry,
            config,
            namespace,
            agent_id,
            session_id,
            context_tokens,
            control_plane,
            agent_spec,
            mcp_tools,
        }
    }

    pub async fn system_loop_message(&self) -> Result<LoopMessage> {
        Ok(LoopMessage::text(
            "system",
            self.assembler.assemble().await?,
        ))
    }

    /// Run the durable compaction path as a session-maintenance operation.
    /// The compaction request intentionally has no provider continuation ID.
    pub async fn force_compact_context(
        &self,
        context: &mut ExecutionContext,
        sink: &dyn ExecutionSink,
    ) -> Result<bool> {
        compact(self.llm.as_ref(), context, sink).await
    }

    fn normalize_token_counter(&self, mut counter: TokenCounter) -> TokenCounter {
        counter.provider = self.llm_provider_key.clone();
        counter.model = self.llm_model.clone();
        counter
    }

    fn previous_response_id(&self, context_tokens: Option<&TokenCounter>) -> Option<String> {
        let counter = context_tokens.filter(|counter| {
            counter.provider == self.llm_provider_key
                && counter.model == self.llm_model
                && counter.provider_request_id.is_some()
        })?;
        counter.provider_request_id.clone()
    }

    fn render_execution_prompts(&self, context: &ExecutionContext) -> Result<ExecutionPrompts> {
        let configured_system_prompt = self.agent_spec.system_prompt.trim();
        let configured_system_prompt =
            if !configured_system_prompt.is_empty() && !context.has_system_message() {
                Some(
                    crate::control::manifest::templating::render_runtime_system_prompt_template(
                        configured_system_prompt,
                    )?,
                )
            } else {
                None
            };
        let skill_catalog = self.assembler.skill_context.trim();
        let system_prompt = match (configured_system_prompt, skill_catalog) {
            (Some(prompt), catalog) if !catalog.is_empty() => {
                Some(format!("{prompt}\n\n{catalog}"))
            }
            (Some(prompt), _) => Some(prompt),
            (None, catalog) if !catalog.is_empty() => Some(catalog.to_string()),
            (None, _) => None,
        };

        let post_history_prompt = self.agent_spec.post_history_prompt.trim();
        let post_history_prompt = if post_history_prompt.is_empty() {
            None
        } else {
            Some(
                crate::control::manifest::templating::render_runtime_post_history_prompt_template(
                    post_history_prompt,
                )?,
            )
        };

        Ok(ExecutionPrompts {
            system_prompt,
            post_history_prompt,
        })
    }

    async fn messages_for_llm(
        &self,
        context: &ExecutionContext,
        prompts: &ExecutionPrompts,
        tool_schema_chars: usize,
        active_skill_context: &str,
    ) -> Result<(Vec<ChatMessage>, ContextMetrics)> {
        let mut history = context.history.clone();
        if let Some(system_prompt) = prompts.system_prompt.as_deref() {
            history.insert(0, LoopMessage::text("system", system_prompt.to_string()));
        }
        if let Some(post_history_prompt) = prompts.post_history_prompt.as_deref() {
            prefix_latest_user_message(&mut history, post_history_prompt);
        }

        let mut history_with_active_skill = history.clone();
        if !active_skill_context.is_empty() {
            let insert_at = history_with_active_skill
                .iter()
                .take_while(|message| message.role == "system")
                .count();
            history_with_active_skill.insert(
                insert_at,
                LoopMessage::text("user", active_skill_context.to_string()),
            );
        }

        let model_limits = model_context_limits(
            self.config.as_ref(),
            &self.llm_provider_key,
            &self.llm_model,
        );
        let mut compacted_history = compact_history_for_llm_with_model_limits_and_tool_schema(
            &history,
            model_limits,
            tool_schema_chars + active_skill_context.len(),
        );
        if !active_skill_context.is_empty() {
            let insert_at = compacted_history
                .iter()
                .take_while(|message| message.role == "system")
                .count();
            compacted_history.insert(
                insert_at,
                LoopMessage::text("user", active_skill_context.to_string()),
            );
        }
        let metrics = context_metrics(
            &history_with_active_skill,
            &compacted_history,
            tool_schema_chars,
        );
        let mut messages = compacted_history
            .iter()
            .map(|m| ChatMessage {
                role: m.role.clone(),
                content_parts: m.content_parts.clone(),
                tool_calls: m.tool_calls.clone().unwrap_or_default(),
                tool_call_id: m.tool_call_id.clone(),
            })
            .collect::<Vec<_>>();
        for message in &mut messages {
            message.content_parts = self
                .hydrate_object_ref_parts_for_llm(std::mem::take(&mut message.content_parts))
                .await?;
        }
        Ok((messages, metrics))
    }

    async fn active_skill_context(
        &self,
        context_tokens: &mut Option<TokenCounter>,
    ) -> Result<String> {
        let names = crate::harness::sessions::active_skill_names(
            self.control_plane.kv.as_ref(),
            &self.namespace,
            &self.agent_id,
            &self.session_id,
        )
        .await?;
        let available = load_available_skills(&self.control_plane, &self.namespace).await?;
        let mut resolved = Vec::new();
        let mut unavailable = Vec::new();
        for name in names {
            let Some(skill) = find_effective_skill(&available, &name) else {
                unavailable.push(name);
                continue;
            };
            match load_skill_instructions(&self.control_plane, skill).await {
                Ok(instructions) => resolved.push((skill.clone(), instructions)),
                Err(error) => {
                    tracing::warn!(skill = %skill.name, namespace = %skill.namespace, %error, "removing unavailable active Skill");
                    unavailable.push(name);
                }
            }
        }
        for name in &unavailable {
            if let Err(error) = crate::harness::sessions::deactivate_skill(
                self.control_plane.kv.as_ref(),
                &self.namespace,
                &self.agent_id,
                &self.session_id,
                name,
            )
            .await
            {
                tracing::warn!(
                    namespace = %self.namespace,
                    agent = %self.agent_id,
                    session = %self.session_id,
                    skill = %name,
                    %error,
                    "failed to deactivate unavailable Skill"
                );
            }
        }
        let mut text = format_active_skill_context(&resolved);
        if !unavailable.is_empty() {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&format!(
                "# SKILL NOTICE\nThe following previously active Skills are no longer available and were deactivated: {}.",
                unavailable.join(", ")
            ));
        }
        let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
        let digest_changed = crate::harness::sessions::persist_active_skill_context_digest(
            self.control_plane.kv.as_ref(),
            &self.namespace,
            &self.agent_id,
            &self.session_id,
            &digest,
        )
        .await?;
        if digest_changed {
            if let Some(counter) = context_tokens.as_mut() {
                counter.provider_request_id = None;
            }
        }
        Ok(text)
    }

    async fn hydrate_object_ref_parts_for_llm(
        &self,
        parts: Vec<ChatContentPart>,
    ) -> Result<Vec<ChatContentPart>> {
        let cas = CasStore::new(self.control_plane.objects.clone());
        let mut hydrated = Vec::with_capacity(parts.len());
        for part in parts {
            let Some(object_ref) = (match part.content.as_ref() {
                Some(chat_content_part::Content::ObjectRef(object_ref)) => Some(object_ref),
                _ => None,
            }) else {
                hydrated.push(part);
                continue;
            };
            let object_ref_media_type = object_ref.media_type.trim();
            if !object_ref_media_type.is_empty()
                && !is_text_object_media_type(object_ref_media_type)
                && !is_image_object_media_type(object_ref_media_type)
            {
                hydrated.push(part);
                continue;
            }
            let Some(stored) = cas.get_object_decoded(&object_ref.key).await? else {
                hydrated.push(text_part(format!(
                    "[Object '{}' is missing.]",
                    object_ref.key
                )));
                continue;
            };
            let media_type = if !object_ref.media_type.trim().is_empty() {
                object_ref.media_type.trim().to_string()
            } else if !stored.metadata.media_type.trim().is_empty() {
                stored.metadata.media_type.trim().to_string()
            } else {
                infer_media_type_for_object_ref(object_ref).unwrap_or_default()
            };
            if is_text_object_media_type(&media_type) {
                hydrated.push(text_part(
                    String::from_utf8_lossy(&stored.bytes).to_string(),
                ));
                continue;
            }
            if !is_image_object_media_type(&media_type) {
                hydrated.push(object_ref_part(object_ref.clone()));
                continue;
            }
            let mut hydrated_ref = object_ref.clone();
            if hydrated_ref.media_type.trim().is_empty() {
                hydrated_ref.media_type = media_type;
            }
            if hydrated_ref.filename.trim().is_empty() {
                hydrated_ref.filename = stored.metadata.filename;
            }
            if hydrated_ref.size_bytes == 0 {
                hydrated_ref.size_bytes = stored.metadata.size_bytes;
            }
            hydrated.push(object_ref_part(hydrated_ref));
        }
        Ok(hydrated)
    }

    fn estimate_context_budget(
        &self,
        context: &ExecutionContext,
        prompts: &ExecutionPrompts,
        tools: &[crate::harness::llm::Tool],
        active_skill_context_chars: usize,
    ) -> usize {
        let history_weight = context
            .history
            .iter()
            .map(serialized_message_weight)
            .sum::<usize>();
        let prompt_weight = prompts
            .system_prompt
            .as_ref()
            .map(|prompt| prompt.len())
            .unwrap_or(0)
            + prompts
                .post_history_prompt
                .as_ref()
                .map(|prompt| prompt.len() + 2)
                .unwrap_or(0);
        let tool_weight = tools
            .iter()
            .map(|tool| tool.name.len() + tool.description.len() + tool.input_schema_json.len())
            .sum::<usize>();
        history_weight + prompt_weight + tool_weight + active_skill_context_chars
    }

    fn estimate_context_input_tokens(
        &self,
        context: &ExecutionContext,
        context_tokens: Option<&TokenCounter>,
        prior_request_history_len: Option<usize>,
        active_skill_context_chars: usize,
    ) -> Option<u64> {
        let counter = context_tokens.filter(|counter| {
            counter.usage_available
                && counter.input_tokens > 0
                && counter.provider == self.llm_provider_key
                && counter.model == self.llm_model
        })?;

        let delta_start = prior_request_history_len.unwrap_or_else(|| {
            context
                .history
                .iter()
                .rposition(|message| message.role == "user")
                .unwrap_or(context.history.len())
        });
        let delta_chars = context
            .history
            .get(delta_start..)
            .unwrap_or_default()
            .iter()
            .map(serialized_message_weight)
            .sum::<usize>();
        let delta_tokens = (delta_chars as u64).saturating_add(3) / 4;
        let active_tokens = (active_skill_context_chars as u64).saturating_add(3) / 4;
        Some(
            counter
                .input_tokens
                .saturating_add(delta_tokens)
                .saturating_add(active_tokens),
        )
    }

    /// Run the prepared execution context to completion, emitting events to
    /// `sink` along the way.
    /// Returns the final reply text.
    pub async fn execute(
        &self,
        context: &mut ExecutionContext,
        sink: &dyn ExecutionSink,
        cancellation_token: Option<&CancellationToken>,
    ) -> Result<String> {
        let span = telemetry::agent_span(&self.namespace, &self.agent_id, &self.session_id);
        let instrument_span = span.clone();
        let result = self
            .execute_inner(context, sink, cancellation_token)
            .instrument(instrument_span)
            .await;
        if let Err(err) = &result {
            telemetry::record_error(&span, err);
        }
        result
    }

    async fn execute_inner(
        &self,
        context: &mut ExecutionContext,
        sink: &dyn ExecutionSink,
        cancellation_token: Option<&CancellationToken>,
    ) -> Result<String> {
        let prompts = self.render_execution_prompts(context)?;
        let mut turn_limit = DEFAULT_EXECUTION_TURN_LIMIT;
        let mut compaction_disabled = false;
        let mut context_tokens = self.context_tokens.clone();
        let mut prior_request_history_len = None;
        loop {
            if turn_limit == 0 {
                let msg = "Turn limit reached".to_string();
                return Err(anyhow::anyhow!(msg));
            }
            turn_limit -= 1;

            let tools = {
                let reg = self.registry.read().await;
                reg.to_provider_tools()
            };
            let tool_schema_chars = telemetry::serialize_tool_definitions_json(&tools).len();
            let active_skill_context = self.active_skill_context(&mut context_tokens).await?;

            // Trigger durable compaction before preparing messages if the complete
            // request estimate exceeds the model's effective window. Compaction
            // is journaled before the in-memory history is replaced.
            let model_limits = model_context_limits(
                self.config.as_ref(),
                &self.llm_provider_key,
                &self.llm_model,
            );
            let estimated_context_budget =
                self.estimate_context_budget(context, &prompts, &tools, active_skill_context.len());
            let durable_budget = ContextBudget::default()
                .with_model_limits(model_limits)
                .with_tool_schema_chars(tool_schema_chars)
                .total_chars;
            let estimated_context_tokens = self.estimate_context_input_tokens(
                context,
                context_tokens.as_ref(),
                prior_request_history_len,
                active_skill_context.len(),
            );
            let should_compact = !compaction_disabled
                && match (
                    estimated_context_tokens,
                    model_limits.effective_input_tokens(),
                ) {
                    (Some(estimated_tokens), Some(input_budget)) => {
                        estimated_tokens > input_budget || estimated_context_budget > durable_budget
                    }
                    _ => estimated_context_budget > durable_budget,
                };

            if should_compact {
                let prev_history_len = context.history.len();
                match compact(self.llm.as_ref(), context, sink).await? {
                    true => {
                        if let Some(counter) = context_tokens.as_mut() {
                            counter.provider_request_id = None;
                        }
                        let new_context_budget = self.estimate_context_budget(
                            context,
                            &prompts,
                            &tools,
                            active_skill_context.len(),
                        );
                        if new_context_budget >= durable_budget {
                            compaction_disabled = true;
                        }
                        tracing::info!(
                            prev_history_len,
                            new_history_len = context.history.len(),
                            new_context_budget,
                            "Durable model-context compaction completed"
                        );
                    }
                    false => {
                        compaction_disabled = true;
                        tracing::warn!(
                            context_len = estimated_context_budget,
                            "Durable compaction made no progress; disabling retries for this execution"
                        );
                    }
                };
            }

            let (messages, context_metrics) = self
                .messages_for_llm(context, &prompts, tool_schema_chars, &active_skill_context)
                .await?;

            let mut final_reply = String::new();
            let mut tool_calls_by_index: BTreeMap<usize, ToolCall> = BTreeMap::new();
            let mut final_usage: Option<TokenCounter> = None;
            let mut saw_tool_call_delta = false;
            let model = resolve_model_profile(self.agent_spec.model_policy.as_ref());
            let thinking = model.and_then(|model| model.thinking.clone());
            let zero_data_retention = model.is_some_and(|model| model.zero_data_retention);
            let usage_subject = self.usage_subject();
            crate::control::usage::check_namespace_usage(
                self.control_plane.kv.as_ref(),
                &usage_subject,
                LLM_PREFLIGHT_METRICS,
                chrono::Utc::now().timestamp(),
            )
            .await?;
            let request = ChatRequest {
                messages,
                tools,
                thinking,
                previous_response_id: (!zero_data_retention)
                    .then(|| self.previous_response_id(context_tokens.as_ref()))
                    .flatten(),
                zero_data_retention,
            };
            prior_request_history_len = Some(context.history.len());
            let reasoning_level = request
                .thinking
                .as_ref()
                .map(|thinking| thinking.effort.as_str());
            let llm_span = telemetry::chat_span(
                &self.namespace,
                &self.agent_id,
                &self.session_id,
                &self.llm_provider_key,
                &self.llm_model,
                reasoning_level,
            );
            telemetry::record_chat_operation_details(
                &llm_span,
                &request,
                model_context_limits(
                    self.config.as_ref(),
                    &self.llm_provider_key,
                    &self.llm_model,
                ),
                context_metrics,
            );
            let llm_started_at = Instant::now();
            let mut saw_first_chunk = false;
            let mut stream = match self
                .llm
                .stream_chat_completion(request)
                .instrument(llm_span.clone())
                .await
            {
                Ok(stream) => stream,
                Err(err) => {
                    if let Some(counter) = provider_error_token_counter(&err).cloned() {
                        let counter = self.normalize_token_counter(counter);
                        telemetry::record_usage(&llm_span, &counter);
                        sink.on_usage(&counter).await;
                    }
                    telemetry::record_error(&llm_span, &err);
                    return Err(err);
                }
            };

            loop {
                let next_chunk = if let Some(token) = cancellation_token {
                    tokio::select! {
                        _ = token.cancelled() => {
                            tracing::info!(agent_id = %context.agent_id, "Generation interrupted by user");
                            telemetry::record_chat_output(&llm_span, &final_reply, &[]);
                            context.push(LoopMessage::text("assistant", final_reply.clone()));
                            let usage = final_usage.take();
                            let usage_for_event = usage.clone().unwrap_or_else(|| {
                                self.normalize_token_counter(TokenCounter::default())
                            });
                            if let Some(usage) = usage.as_ref().filter(|usage| usage.usage_available) {
                                telemetry::record_usage(&llm_span, usage);
                            } else {
                                tracing::warn!(
                                    provider = %self.llm_provider_key,
                                    model = %self.llm_model,
                                    "LLM request cancelled without provider token usage"
                                );
                            }
                            sink.on_usage(&usage_for_event).await;
                            crate::control::usage::charge_namespace_usage(
                                self.control_plane.kv.as_ref(),
                                &usage_subject,
                                &crate::control::usage::llm_usage_charges(usage.as_ref()),
                                chrono::Utc::now().timestamp(),
                            )
                            .await?;
                            sink.on_done().await;
                            return Ok(final_reply);
                        }
                        chunk = stream.next() => chunk,
                    }
                } else {
                    stream.next().await
                };

                let Some(chunk) = next_chunk else {
                    break;
                };

                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(err) => {
                        if let Some(usage) = final_usage.take() {
                            if usage.usage_available {
                                telemetry::record_usage(&llm_span, &usage);
                            }
                            sink.on_usage(&usage).await;
                        }
                        telemetry::record_error(&llm_span, &err);
                        return Err(err);
                    }
                };

                if !saw_first_chunk {
                    saw_first_chunk = true;
                    telemetry::record_time_to_first_chunk(
                        &llm_span,
                        llm_started_at.elapsed().as_secs_f64(),
                    );
                }

                match chunk {
                    ChatStreamEvent {
                        event: Some(chat_stream_event::Event::TextDelta(token)),
                    } => {
                        final_reply.push_str(&token);
                        sink.on_token(&token).await;
                    }
                    ChatStreamEvent {
                        event: Some(chat_stream_event::Event::ReasoningDelta(reasoning)),
                    } => {
                        sink.on_reasoning(&reasoning).await;
                    }
                    ChatStreamEvent {
                        event: Some(chat_stream_event::Event::ToolCallDelta(delta)),
                    } => {
                        if !saw_tool_call_delta {
                            saw_tool_call_delta = true;
                            sink.on_tool_call_stream_started().await;
                        }
                        let entry = tool_calls_by_index
                            .entry(delta.index as usize)
                            .or_insert_with(|| ToolCall {
                                id: format!("tool_call_{}", delta.index),
                                name: String::new(),
                                arguments: String::new(),
                            });

                        if let Some(id) = delta.id {
                            entry.id = id;
                        }
                        if let Some(name) = delta.name {
                            entry.name = name;
                        }
                        if let Some(arguments) = delta.arguments {
                            entry.arguments.push_str(&arguments);
                        }
                    }
                    ChatStreamEvent {
                        event: Some(chat_stream_event::Event::Usage(usage)),
                    } => {
                        final_usage = Some(self.normalize_token_counter(usage));
                    }
                    ChatStreamEvent {
                        event: Some(chat_stream_event::Event::EncryptedReasoning(_)),
                    } => {}
                    ChatStreamEvent { event: None } => {}
                }
            }

            let tool_calls: Vec<ToolCall> = tool_calls_by_index
                .into_values()
                .filter(|tool| !tool.name.is_empty())
                .collect();

            let llm_response = ChatResponse {
                content: final_reply.clone(),
                tool_calls: tool_calls.clone(),
                usage: Some(
                    final_usage
                        .unwrap_or_else(|| self.normalize_token_counter(TokenCounter::default())),
                ),
                encrypted_reasoning: None,
            };
            context_tokens = llm_response.usage.clone();
            telemetry::record_chat_output(
                &llm_span,
                &llm_response.content,
                &llm_response.tool_calls,
            );
            if let Some(usage) = llm_response
                .usage
                .as_ref()
                .filter(|usage| usage.usage_available)
            {
                telemetry::record_usage(&llm_span, usage);
            } else {
                tracing::warn!(
                    provider = %self.llm_provider_key,
                    model = %self.llm_model,
                    "LLM provider completed without token usage"
                );
            }
            sink.on_llm_response(&llm_response).await?;
            if let Some(usage) = llm_response.usage.as_ref() {
                sink.on_usage(usage).await;
            }
            crate::control::usage::charge_namespace_usage(
                self.control_plane.kv.as_ref(),
                &usage_subject,
                &crate::control::usage::llm_usage_charges(llm_response.usage.as_ref()),
                chrono::Utc::now().timestamp(),
            )
            .await?;

            // Record assistant turn
            let mut assistant_message = LoopMessage::text("assistant", final_reply.clone());
            assistant_message.tool_calls = if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls.clone())
            };
            context.push(assistant_message);

            if !tool_calls.is_empty() {
                let mut stop_after_tool_result = None;
                for tool in &tool_calls {
                    let input = Self::tool_call_input(tool);
                    let tool_type = self.tool_type(&tool.name).await;
                    let tool_span = telemetry::tool_span(
                        &self.namespace,
                        &self.agent_id,
                        &self.session_id,
                        tool,
                        tool_type,
                    );
                    crate::control::usage::check_namespace_usage(
                        self.control_plane.kv.as_ref(),
                        &self.usage_subject(),
                        &[crate::control::usage::METRIC_TOOL_CALLS],
                        chrono::Utc::now().timestamp(),
                    )
                    .await?;
                    sink.on_tool_call(&tool.id, &tool.name, &input).await;
                    let executed = if let Some(token) = cancellation_token {
                        tokio::select! {
                            biased;
                            _ = token.cancelled() => {
                                tracing::info!(
                                    agent_id = %context.agent_id,
                                    tool = %tool.name,
                                    tool_call_id = %tool.id,
                                    "Tool call interrupted by user"
                                );
                                sink.on_done().await;
                                return Ok(final_reply);
                            }
                            executed = self
                                .execute_tool_call_result(tool)
                                .instrument(tool_span.clone()) => executed,
                        }
                    } else {
                        self.execute_tool_call_result(tool)
                            .instrument(tool_span.clone())
                            .await
                    };
                    let stop_after_result = executed.stop_after_result;
                    let result = executed.result;
                    let result_text = result.summary();
                    telemetry::record_tool_result(&tool_span, &result_text);
                    sink.on_tool_result_recorded(&tool.id, &tool.name, &result)
                        .await?;
                    crate::control::usage::charge_namespace_usage(
                        self.control_plane.kv.as_ref(),
                        &self.usage_subject(),
                        &[crate::control::usage::UsageCharge {
                            metric: crate::control::usage::METRIC_TOOL_CALLS,
                            delta: 1,
                        }],
                        chrono::Utc::now().timestamp(),
                    )
                    .await?;
                    sink.on_tool_result(&tool.id, &tool.name, &result).await;
                    context.push(tool_output_loop_message(&tool.id, &result));
                    if stop_after_result {
                        stop_after_tool_result = Some(result_text);
                        break;
                    }
                }
                if let Some(result) = stop_after_tool_result {
                    sink.on_done().await;
                    return Ok(result);
                }
                let steering = sink.take_steering_messages().await?;
                context.push_many(steering);
                continue;
            }

            sink.on_done().await;
            return Ok(final_reply);
        }
    }

    pub async fn execute_tool_call(&self, tool: &ToolCall) -> (Value, String) {
        let input = Self::tool_call_input(tool);
        let executed = self.execute_tool_call_result(tool).await;
        (input, executed.result.summary())
    }

    pub fn tool_call_input(tool: &ToolCall) -> Value {
        serde_json::from_str(&tool.arguments).unwrap_or(Value::Null)
    }

    async fn tool_type(&self, name: &str) -> &'static str {
        if self.mcp_tools.contains_key(name) {
            "mcp"
        } else if self.registry.read().await.get_tool(name).is_some() {
            "native"
        } else {
            "unknown"
        }
    }

    fn usage_subject(&self) -> crate::control::usage::UsageSubject {
        crate::control::usage::UsageSubject {
            namespace: self.namespace.clone(),
            agent: self.agent_id.clone(),
            provider: self.llm_provider_key.clone(),
            model: self.llm_model.clone(),
            rate_limit_key: None,
        }
    }

    async fn execute_tool_call_result(&self, tool: &ToolCall) -> ExecutedToolCall {
        match self.execute_tool(&tool.name, &tool.arguments).await {
            Ok(result) => ExecutedToolCall {
                result,
                stop_after_result: crate::harness::native_tools::tool_requests_worker_stop(
                    &tool.name,
                ),
            },
            Err(error) => ExecutedToolCall {
                result: ToolOutput::text(tool_error_result(&tool.name, &error)),
                stop_after_result: false,
            },
        }
    }

    async fn execute_tool(&self, name: &str, input: &str) -> Result<ToolOutput> {
        let args: Value = serde_json::from_str(input).unwrap_or(Value::Null);
        if let Some(tool) = self.mcp_tools.get(name) {
            return call_tool_for_config(&tool.config, &tool.remote_name, args)
                .await
                .map(ToolOutput::text);
        }
        if let Some(result) = crate::harness::native_tools::execute_tool_for_session_output(
            &self.control_plane,
            &self.namespace,
            &self.agent_id,
            &self.session_id,
            &self.agent_spec,
            name,
            &args,
            &self.config,
        )
        .await?
        {
            return Ok(result);
        }
        Ok(ToolOutput::text(format!("Tool '{}' not found.", name)))
    }
}

pub fn tool_result_loop_message(tool_call_id: &str, result: &str) -> LoopMessage {
    let mut tool_message = LoopMessage::text("tool", result.to_string());
    tool_message.tool_call_id = Some(tool_call_id.to_string());
    tool_message
}

pub fn tool_output_loop_message(tool_call_id: &str, result: &ToolOutput) -> LoopMessage {
    LoopMessage {
        role: "tool".to_string(),
        content_parts: result.content_parts(),
        tool_calls: None,
        tool_call_id: Some(tool_call_id.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        serialized_message_weight, AgentEvent, AgentExecutor, CaptureSink, ContextAssembler,
        ExecutionContext, ExecutionSink, LoopMessage,
    };
    use crate::control::config::Config;
    use crate::control::object_store::ObjectMetadata;
    use crate::control::tool_output::ToolOutputExt;
    use crate::control::{keys, ControlPlane, ProtoKeyValueStoreExt};
    use crate::gateway::rpc::{
        data_proto, manifests,
        protobuf_value::{value::Kind as ProtoValueKind, ListValue, Value as ProtoValue},
    };
    use crate::harness::executor::compaction::compact;
    use crate::harness::llm::provider::{
        content_part_object_ref, object_ref_part, text_delta_event, tool_call_delta_event,
        usage_event, ChatMessage, ChatMessageExt, ChatRequest, ChatResponse, ChatStream,
        LlmProvider, TokenCounter,
    };
    use crate::harness::llm::ToolOutput;
    use crate::harness::memory::Embedding;
    use crate::harness::skills::registry::ToolRegistry;
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use anyhow::Result;
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    #[derive(Default)]
    struct RecordingLlmProvider {
        seen_messages: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
        compaction_content: Option<String>,
    }

    impl RecordingLlmProvider {
        fn with_compaction_content(compaction_content: impl Into<String>) -> Self {
            Self {
                seen_messages: Arc::new(Mutex::new(Vec::new())),
                compaction_content: Some(compaction_content.into()),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for RecordingLlmProvider {
        async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
            Ok(vec![0.0; 8])
        }

        async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse> {
            self.seen_messages
                .lock()
                .unwrap()
                .push(request.messages.clone());
            let is_compaction = request.messages.iter().any(|message| {
                message.role == "system"
                    && message
                        .text_content()
                        .contains("You are the Context Compactor")
            });
            Ok(ChatResponse {
                content: if is_compaction {
                    self.compaction_content.clone().unwrap_or_else(|| "<summary>\n## User goal\nTest compaction.\n## Requirements and constraints\nNone recorded.\n## Facts to preserve\nNone recorded.\n## Decisions and rationale\nNone recorded.\n## Completed work\nCompaction test completed.\n## Files and artifacts\nNone recorded.\n## Tool results and external facts\nNone recorded.\n## Current state\nThe test continues.\n## Open issues\nNone recorded.\n## Next action\nNone recorded.\n</summary>".to_string())
                } else {
                    "resolved".to_string()
                },
                tool_calls: Vec::new(),
                usage: None,
                encrypted_reasoning: None,
            })
        }

        async fn stream_chat_completion(&self, request: ChatRequest) -> Result<ChatStream> {
            let response = self.chat_completion(request).await?;
            Ok(Box::pin(futures::stream::once(async move {
                Ok(text_delta_event(response.content))
            })))
        }

        async fn completion(&self, prompt: &str) -> Result<String> {
            Ok(prompt.to_string())
        }
    }

    struct ToolFailureThenReplyLlm {
        seen_messages: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
        call_count: Arc<Mutex<usize>>,
    }

    impl Default for ToolFailureThenReplyLlm {
        fn default() -> Self {
            Self {
                seen_messages: Arc::new(Mutex::new(Vec::new())),
                call_count: Arc::new(Mutex::new(0)),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for ToolFailureThenReplyLlm {
        async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
            Ok(vec![0.0; 8])
        }

        async fn chat_completion(&self, _request: ChatRequest) -> Result<ChatResponse> {
            unreachable!("stream_chat_completion is used in this test");
        }

        async fn stream_chat_completion(&self, request: ChatRequest) -> Result<ChatStream> {
            self.seen_messages
                .lock()
                .unwrap()
                .push(request.messages.clone());
            let mut call_count = self.call_count.lock().unwrap();
            let stream = if *call_count == 0 {
                *call_count += 1;
                Box::pin(futures::stream::iter(vec![
                    Ok(tool_call_delta_event(crate::harness::llm::provider::ToolCallDelta {
                        index: 0,
                        id: Some("tool-1".to_string()),
                        name: Some("create_schedule".to_string()),
                        arguments: Some(
                            "{\"name\":\"hello-world-ping\",\"kind\":\"every\",\"interval_seconds\":60,\"input_message\":\"Say Hello world!\"}"
                                .to_string(),
                        ),
                    })),
                ])) as ChatStream
            } else {
                Box::pin(futures::stream::once(async {
                    Ok(text_delta_event(
                        "That failed because the minimum interval is 300 seconds.".to_string(),
                    ))
                })) as ChatStream
            };
            Ok(stream)
        }

        async fn completion(&self, prompt: &str) -> Result<String> {
            Ok(prompt.to_string())
        }
    }

    struct WaitToolThenReplyLlm {
        seen_messages: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
        call_count: Arc<Mutex<usize>>,
    }

    impl Default for WaitToolThenReplyLlm {
        fn default() -> Self {
            Self {
                seen_messages: Arc::new(Mutex::new(Vec::new())),
                call_count: Arc::new(Mutex::new(0)),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for WaitToolThenReplyLlm {
        async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
            Ok(vec![0.0; 8])
        }

        async fn chat_completion(&self, _request: ChatRequest) -> Result<ChatResponse> {
            unreachable!("stream_chat_completion is used in this test");
        }

        async fn stream_chat_completion(&self, request: ChatRequest) -> Result<ChatStream> {
            self.seen_messages
                .lock()
                .unwrap()
                .push(request.messages.clone());
            let mut call_count = self.call_count.lock().unwrap();
            let stream = if *call_count == 0 {
                *call_count += 1;
                Box::pin(futures::stream::iter(vec![Ok(tool_call_delta_event(
                    crate::harness::llm::provider::ToolCallDelta {
                        index: 0,
                        id: Some("tool-1".to_string()),
                        name: Some(
                            crate::harness::native_tools::AGENT_WAIT_FOR_MESSAGE_TOOL.to_string(),
                        ),
                        arguments: Some("{\"target\":\"critic-1\"}".to_string()),
                    },
                ))])) as ChatStream
            } else {
                Box::pin(futures::stream::once(async {
                    Ok(text_delta_event("recovered after wait error".to_string()))
                })) as ChatStream
            };
            Ok(stream)
        }

        async fn completion(&self, prompt: &str) -> Result<String> {
            Ok(prompt.to_string())
        }
    }

    struct WaitThenScheduleLlm {
        seen_messages: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
    }

    impl Default for WaitThenScheduleLlm {
        fn default() -> Self {
            Self {
                seen_messages: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for WaitThenScheduleLlm {
        async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
            Ok(vec![0.0; 8])
        }

        async fn chat_completion(&self, _request: ChatRequest) -> Result<ChatResponse> {
            unreachable!("stream_chat_completion is used in this test");
        }

        async fn stream_chat_completion(&self, request: ChatRequest) -> Result<ChatStream> {
            self.seen_messages
                .lock()
                .unwrap()
                .push(request.messages.clone());
            Ok(Box::pin(futures::stream::iter(vec![
                Ok(tool_call_delta_event(crate::harness::llm::provider::ToolCallDelta {
                    index: 0,
                    id: Some("wait-tool".to_string()),
                    name: Some(
                        crate::harness::native_tools::AGENT_WAIT_FOR_MESSAGE_TOOL.to_string(),
                    ),
                    arguments: Some("{\"target\":\"critic-1\"}".to_string()),
                })),
                Ok(tool_call_delta_event(crate::harness::llm::provider::ToolCallDelta {
                    index: 1,
                    id: Some("schedule-tool".to_string()),
                    name: Some("create_schedule".to_string()),
                    arguments: Some(
                        "{\"name\":\"should-not-run\",\"kind\":\"every\",\"interval_seconds\":60,\"input_message\":\"ping\"}"
                            .to_string(),
                    ),
                })),
            ])))
        }

        async fn completion(&self, prompt: &str) -> Result<String> {
            Ok(prompt.to_string())
        }
    }

    async fn control_plane_with_wire() -> ControlPlane {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(RecordingPubSub::default())).build();
        kv.set_msg(
            &keys::session("Tenant:acme:Workspace:main", "writer", "writer-session"),
            &data_proto::Session {
                id: "writer-session".to_string(),
                agent: "writer".to_string(),
                ns: "Tenant:acme:Workspace:main".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 1,
                last_active: 1,
                metadata: [(
                    "wire.a2a.talon.impalasys.com/critic-1".to_string(),
                    "Tenant:acme:Workspace:main/critic/critic-session".to_string(),
                )]
                .into_iter()
                .collect(),
                labels: Default::default(),
                skill_state: None,
                context_tokens: None,
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::session("Tenant:acme:Workspace:main", "critic", "critic-session"),
            &data_proto::Session {
                id: "critic-session".to_string(),
                agent: "critic".to_string(),
                ns: "Tenant:acme:Workspace:main".to_string(),
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
        cp
    }

    struct DelayedToolThenReplyLlm {
        seen_messages: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
        call_count: Arc<Mutex<usize>>,
    }

    impl Default for DelayedToolThenReplyLlm {
        fn default() -> Self {
            Self {
                seen_messages: Arc::new(Mutex::new(Vec::new())),
                call_count: Arc::new(Mutex::new(0)),
            }
        }
    }

    struct InterleavedUsageLlm;

    #[async_trait]
    impl LlmProvider for InterleavedUsageLlm {
        async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
            Ok(vec![0.0; 8])
        }

        async fn chat_completion(&self, _request: ChatRequest) -> Result<ChatResponse> {
            unreachable!("stream_chat_completion is used in this test");
        }

        async fn stream_chat_completion(&self, _request: ChatRequest) -> Result<ChatStream> {
            Ok(Box::pin(futures::stream::iter(vec![
                Ok(text_delta_event("hello ".to_string())),
                Ok(usage_event(TokenCounter {
                    input_tokens: 10,
                    output_tokens: 1,
                    reasoning_output_tokens: 0,
                    total_tokens: 11,
                    usage_available: true,
                    ..Default::default()
                })),
                Ok(text_delta_event("world".to_string())),
                Ok(usage_event(TokenCounter {
                    input_tokens: 10,
                    output_tokens: 2,
                    reasoning_output_tokens: 0,
                    total_tokens: 12,
                    usage_available: true,
                    ..Default::default()
                })),
            ])))
        }

        async fn completion(&self, prompt: &str) -> Result<String> {
            Ok(prompt.to_string())
        }
    }

    #[async_trait]
    impl LlmProvider for DelayedToolThenReplyLlm {
        async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
            Ok(vec![0.0; 8])
        }

        async fn chat_completion(&self, _request: ChatRequest) -> Result<ChatResponse> {
            unreachable!("stream_chat_completion is used in this test");
        }

        async fn stream_chat_completion(&self, request: ChatRequest) -> Result<ChatStream> {
            self.seen_messages
                .lock()
                .unwrap()
                .push(request.messages.clone());
            let mut call_count = self.call_count.lock().unwrap();
            let stream = if *call_count == 0 {
                *call_count += 1;
                Box::pin(futures::stream::once(async {
                    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
                    Ok(tool_call_delta_event(
                        crate::harness::llm::provider::ToolCallDelta {
                            index: 0,
                            id: Some("tool-1".to_string()),
                            name: Some("unknown_tool".to_string()),
                            arguments: Some("{}".to_string()),
                        },
                    ))
                })) as ChatStream
            } else {
                Box::pin(futures::stream::once(async {
                    Ok(text_delta_event("done".to_string()))
                })) as ChatStream
            };
            Ok(stream)
        }

        async fn completion(&self, prompt: &str) -> Result<String> {
            Ok(prompt.to_string())
        }
    }

    #[tokio::test]
    async fn executor_compacts_noisy_history_before_next_turn() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            manifests::AgentSpec::default(),
            HashMap::new(),
        );

        let huge_tool_result = format!(
            "{{\"items\":[{{\"path\":\"footer.tsx\",\"content\":\"{}\"}}],\"query\":\"repo:pablonyx/proliferate blog\"}}",
            "x".repeat(150_000)
        );
        let mut history = vec![LoopMessage::text("system", "You are Conic.")];
        for index in 0..10 {
            history.push(LoopMessage::text(
                "user",
                format!("Earlier question #{index}: {}", "q".repeat(8_000)),
            ));
            history.push(LoopMessage::text(
                "assistant",
                format!("Earlier answer #{index}: {}", "a".repeat(8_000)),
            ));
        }
        let mut assistant_message = LoopMessage::text("assistant", "Investigating repo.");
        assistant_message.tool_calls = Some(vec![crate::harness::llm::ToolCall {
            id: "tool-1".to_string(),
            name: "mcp_github_search_code".to_string(),
            arguments: "{\"query\":\"repo:pablonyx/proliferate blog\"}".to_string(),
        }]);
        history.push(assistant_message);
        let mut tool_message = LoopMessage::text("tool", huge_tool_result);
        tool_message.tool_call_id = Some("tool-1".to_string());
        history.push(tool_message);

        let mut context = ExecutionContext::with_history("cmo", history);
        context.push(LoopMessage::text(
            "user",
            "I'm talking about the blogs link in the footer and the blogs pages",
        ));
        let reply = executor
            .execute(&mut context, &CaptureSink::new(), None)
            .await
            .unwrap();

        assert_eq!(reply, "resolved");
        let seen = llm.seen_messages.lock().unwrap();
        let messages = seen.last().unwrap();
        assert!(messages.iter().any(|message| {
            message.role == "user"
                && message.text_content()
                    == "I'm talking about the blogs link in the footer and the blogs pages"
        }));
        assert!(!messages.iter().any(|message| message.role == "tool"));
        assert!(!messages
            .iter()
            .any(|message| message.tool_calls.iter().any(|call| call.id == "tool-1")));
        assert!(messages.iter().any(|message| {
            message.role == "assistant"
                && message
                    .text_content()
                    .contains("Prior tool interaction omitted")
        }));
        assert!(messages.iter().any(|message| {
            message.role == "assistant"
                && message.text_content().contains("earlier messages omitted")
        }));
    }

    #[test]
    fn tool_call_input_preserves_numeric_arguments() {
        let tool = crate::harness::llm::ToolCall {
            id: "call_1".to_string(),
            name: "mcp_conic_list_links".to_string(),
            arguments: "{\"limit\":50,\"offset\":0}".to_string(),
        };

        let input = AgentExecutor::tool_call_input(&tool);

        assert_eq!(input["limit"], 50);
        assert_eq!(input["offset"], 0);
        assert!(input["limit"].is_number());
        assert!(!input["limit"].is_string());
    }

    #[tokio::test]
    async fn executor_injects_agent_system_prompt_into_llm_request() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let mut spec = manifests::AgentSpec::default();
        spec.system_prompt = "Answer like the configured agent.".to_string();
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            spec,
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage::text("user", "Hello"));

        let reply = executor
            .execute(&mut context, &CaptureSink::new(), None)
            .await
            .unwrap();

        assert_eq!(reply, "resolved");
        let seen = llm.seen_messages.lock().unwrap();
        let messages = seen.last().unwrap();
        assert_eq!(messages[0].role, "system");
        assert_eq!(
            messages[0].text_content(),
            "Answer like the configured agent."
        );
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].text_content(), "Hello");
    }

    #[tokio::test]
    async fn executor_renders_agent_system_prompt_template_into_llm_request() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let mut spec = manifests::AgentSpec::default();
        spec.system_prompt = "Now: {{ talon.now }}".to_string();
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            spec,
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage::text("user", "Hello"));

        executor
            .execute(&mut context, &CaptureSink::new(), None)
            .await
            .unwrap();

        let seen = llm.seen_messages.lock().unwrap();
        let messages = seen.last().unwrap();
        let timestamp = messages[0]
            .text_content()
            .strip_prefix("Now: ")
            .unwrap()
            .to_string();
        assert!(timestamp.ends_with('Z'));
        chrono::DateTime::parse_from_rfc3339(&timestamp).unwrap();
    }

    #[tokio::test]
    async fn executor_errors_on_unknown_system_prompt_template_variable() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let mut spec = manifests::AgentSpec::default();
        spec.system_prompt = "{{ talon.nope }}".to_string();
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            spec,
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage::text("user", "Hello"));

        let err = executor
            .execute(&mut context, &CaptureSink::new(), None)
            .await
            .expect_err("unknown system prompt variables should fail");

        assert!(err
            .to_string()
            .contains("Failed to render system prompt template"));
        assert!(llm.seen_messages.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn executor_prefixes_latest_user_message_with_post_history_prompt() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let mut spec = manifests::AgentSpec::default();
        spec.post_history_prompt = "Current time: fixed".to_string();
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            spec,
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage::text("user", "Hello"));
        let original_user_message = context.history[0].clone();

        executor
            .execute(&mut context, &CaptureSink::new(), None)
            .await
            .unwrap();

        assert_eq!(context.history[0], original_user_message);
        let seen = llm.seen_messages.lock().unwrap();
        let messages = seen.last().unwrap();
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].text_content(), "Current time: fixed\n\nHello");
    }

    #[tokio::test]
    async fn executor_prefixes_multimodal_latest_user_message_without_dropping_parts() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let cp = ControlPlane::noop();
        let object = cp
            .objects
            .put(
                "cas/acme/files/file-1/screenshot.png",
                b"png-bytes",
                ObjectMetadata {
                    media_type: "image/png".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let mut spec = manifests::AgentSpec::default();
        spec.post_history_prompt = "Use this context.".to_string();
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            cp,
            spec,
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage {
            role: "user".to_string(),
            content_parts: vec![object_ref_part(object)],
            tool_calls: None,
            tool_call_id: None,
        });
        let original_user_message = context.history[0].clone();

        executor
            .execute(&mut context, &CaptureSink::new(), None)
            .await
            .unwrap();

        assert_eq!(context.history[0], original_user_message);
        let seen = llm.seen_messages.lock().unwrap();
        let messages = seen.last().unwrap();
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content_parts.len(), 2);
        assert_eq!(messages[0].text_content(), "Use this context.\n\n");
        assert_eq!(
            messages[0].content_parts[1],
            original_user_message.content_parts[0]
        );
    }

    #[tokio::test]
    async fn executor_hydrates_object_ref_image_parts_for_llm() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let cp = ControlPlane::noop();
        let object = cp
            .objects
            .put(
                "cas/acme/files/file-1/sha",
                b"png-bytes",
                ObjectMetadata {
                    media_type: "image/png".to_string(),
                    size_bytes: 9,
                    sha256: "sha".to_string(),
                    filename: "image.png".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry,
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            cp,
            manifests::AgentSpec::default(),
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage {
            role: "user".to_string(),
            content_parts: vec![object_ref_part(object.clone())],
            tool_calls: None,
            tool_call_id: None,
        });

        executor
            .execute(&mut context, &CaptureSink::new(), None)
            .await
            .unwrap();

        let seen = llm.seen_messages.lock().unwrap();
        let part = &seen.last().unwrap()[0].content_parts[0];
        assert_eq!(content_part_object_ref(part).unwrap().key, object.key);
        assert_eq!(
            content_part_object_ref(part).unwrap().media_type,
            "image/png"
        );
    }

    #[tokio::test]
    async fn executor_infers_object_ref_image_media_type_for_llm() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let cp = ControlPlane::noop();
        let object = cp
            .objects
            .put(
                "cas/acme/files/file-1/screenshot.png",
                b"png-bytes",
                ObjectMetadata {
                    size_bytes: 9,
                    sha256: "sha".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry,
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            cp,
            manifests::AgentSpec::default(),
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage {
            role: "user".to_string(),
            content_parts: vec![object_ref_part(object)],
            tool_calls: None,
            tool_call_id: None,
        });

        executor
            .execute(&mut context, &CaptureSink::new(), None)
            .await
            .unwrap();

        let seen = llm.seen_messages.lock().unwrap();
        assert_eq!(
            content_part_object_ref(&seen.last().unwrap()[0].content_parts[0])
                .unwrap()
                .media_type,
            "image/png"
        );
    }

    #[tokio::test]
    async fn executor_hydrates_text_object_ref_parts_for_llm() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let cp = ControlPlane::noop();
        let object = cp
            .objects
            .put(
                "cas/acme/files/file-1/notes.txt",
                b"source text",
                ObjectMetadata {
                    media_type: "text/plain; charset=utf-8".to_string(),
                    size_bytes: 11,
                    filename: "notes.txt".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry,
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            cp,
            manifests::AgentSpec::default(),
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage {
            role: "user".to_string(),
            content_parts: vec![object_ref_part(object)],
            tool_calls: None,
            tool_call_id: None,
        });

        executor
            .execute(&mut context, &CaptureSink::new(), None)
            .await
            .unwrap();

        let seen = llm.seen_messages.lock().unwrap();
        assert_eq!(seen.last().unwrap()[0].text_content(), "source text");
    }

    #[tokio::test]
    async fn executor_replaces_missing_object_ref_image_with_placeholder() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry,
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            manifests::AgentSpec::default(),
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage {
            role: "user".to_string(),
            content_parts: vec![object_ref_part(data_proto::ObjectRef {
                key: "cas/acme/files/file-1/missing.png".to_string(),
                media_type: "image/png".to_string(),
                ..Default::default()
            })],
            tool_calls: None,
            tool_call_id: None,
        });

        executor
            .execute(&mut context, &CaptureSink::new(), None)
            .await
            .unwrap();

        let seen = llm.seen_messages.lock().unwrap();
        assert!(seen.last().unwrap()[0]
            .text_content()
            .contains("missing.png"));
    }

    #[tokio::test]
    async fn executor_describes_non_image_object_ref_parts_for_llm() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry,
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            manifests::AgentSpec::default(),
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage {
            role: "user".to_string(),
            content_parts: vec![object_ref_part(data_proto::ObjectRef {
                key: "cas/acme/files/file-1/clip.mp4".to_string(),
                media_type: "video/mp4".to_string(),
                size_bytes: 42,
                filename: "clip.mp4".to_string(),
                ..Default::default()
            })],
            tool_calls: None,
            tool_call_id: None,
        });

        executor
            .execute(&mut context, &CaptureSink::new(), None)
            .await
            .unwrap();

        let seen = llm.seen_messages.lock().unwrap();
        assert_eq!(
            content_part_object_ref(&seen.last().unwrap()[0].content_parts[0])
                .unwrap()
                .media_type,
            "video/mp4"
        );
    }

    #[tokio::test]
    async fn executor_errors_on_unknown_post_history_prompt_template_variable() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let mut spec = manifests::AgentSpec::default();
        spec.post_history_prompt = "{{ talon.nope }}".to_string();
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            spec,
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage::text("user", "Hello"));

        let err = executor
            .execute(&mut context, &CaptureSink::new(), None)
            .await
            .expect_err("unknown post-history prompt variables should fail");

        assert!(err
            .to_string()
            .contains("Failed to render post-history prompt template"));
        assert!(llm.seen_messages.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn executor_reuses_rendered_runtime_prompts_across_tool_loop_calls() {
        let llm = Arc::new(DelayedToolThenReplyLlm::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let mut spec = manifests::AgentSpec::default();
        spec.system_prompt = "System now: {{ talon.now }}".to_string();
        spec.post_history_prompt = "Post now: {{ talon.now }}".to_string();
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            spec,
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage::text("user", "Hello"));

        executor
            .execute(&mut context, &CaptureSink::new(), None)
            .await
            .unwrap();

        let seen = llm.seen_messages.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0][0].role, "system");
        assert_eq!(seen[1][0].role, "system");
        assert_eq!(seen[0][0].text_content(), seen[1][0].text_content());

        let first_user = seen[0]
            .iter()
            .find(|message| message.role == "user")
            .unwrap();
        let second_user = seen[1]
            .iter()
            .find(|message| message.role == "user")
            .unwrap();
        assert_eq!(first_user.text_content(), second_user.text_content());
        assert!(first_user.text_content().starts_with("Post now: "));
        assert!(first_user.text_content().ends_with("\n\nHello"));
    }

    #[tokio::test]
    async fn executor_does_not_duplicate_existing_system_message() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let mut spec = manifests::AgentSpec::default();
        spec.system_prompt = "Configured prompt".to_string();
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            spec,
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage::text("system", "Existing prompt"));
        context.push(LoopMessage::text("user", "Hello"));

        executor
            .execute(&mut context, &CaptureSink::new(), None)
            .await
            .unwrap();

        let seen = llm.seen_messages.lock().unwrap();
        let messages = seen.last().unwrap();
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.role == "system")
                .count(),
            1
        );
        assert_eq!(messages[0].text_content(), "Existing prompt");
    }

    #[tokio::test]
    async fn executor_surfaces_native_tool_errors_as_tool_results() {
        let llm = Arc::new(ToolFailureThenReplyLlm::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let mut spec = manifests::AgentSpec::default();
        spec.capabilities.insert(
            "schedules".to_string(),
            ListValue {
                values: vec![
                    ProtoValue {
                        kind: Some(ProtoValueKind::StringValue("create".to_string())),
                    },
                    ProtoValue {
                        kind: Some(ProtoValueKind::StringValue("create:new".to_string())),
                    },
                ],
            },
        );
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            spec.clone(),
            HashMap::new(),
        );
        {
            let mut reg = registry.write().await;
            crate::harness::native_tools::register_tools(&mut reg, &spec, &Config::default());
        }

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage::text("user", "Create a 1-minute schedule"));
        let sink = CaptureSink::new();
        let reply = executor.execute(&mut context, &sink, None).await.unwrap();

        assert_eq!(
            reply,
            "That failed because the minimum interval is 300 seconds."
        );
        let events = sink.events();
        let action_index = events
            .iter()
            .position(|event| matches!(event, AgentEvent::Action { name, .. } if name == "create_schedule"))
            .expect("expected a tool action");
        let observation_index = events
            .iter()
            .position(|event| matches!(event, AgentEvent::Observation { name, .. } if name == "create_schedule"))
            .expect("expected a tool observation");
        assert!(
            action_index < observation_index,
            "tool call should be published before its result"
        );
        let observation = events
            .iter()
            .find_map(|event| match event {
                AgentEvent::Observation { name, output, .. } if name == "create_schedule" => {
                    Some(output.clone())
                }
                _ => None,
            })
            .expect("expected a tool observation");
        assert!(observation.contains("\"ok\":false"));
        assert!(observation.contains("interval_seconds must be at least 300"));
    }

    #[tokio::test]
    async fn executor_stops_after_wait_for_message_tool_result() {
        let llm = Arc::new(WaitToolThenReplyLlm::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let spec = manifests::AgentSpec::default();
        let executor = AgentExecutor::new_with_session(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "Tenant:acme:Workspace:main".to_string(),
            "writer".to_string(),
            "writer-session".to_string(),
            None,
            control_plane_with_wire().await,
            spec.clone(),
            HashMap::new(),
        );
        {
            let mut reg = registry.write().await;
            crate::harness::native_tools::register_tools(&mut reg, &spec, &Config::default());
        }

        let mut context = ExecutionContext::new("writer");
        context.push(LoopMessage::text("user", "Ask the critic and wait."));
        let sink = CaptureSink::new();
        let reply = executor.execute(&mut context, &sink, None).await.unwrap();

        assert!(reply.contains("\"status\":\"WAITING\""), "reply={reply}");
        assert!(reply.contains("Waiting for a message from critic-1."));
        assert_eq!(*llm.call_count.lock().unwrap(), 1);
        let events = sink.events();
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::Action { name, .. }
                if name == crate::harness::native_tools::AGENT_WAIT_FOR_MESSAGE_TOOL
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::Observation { name, .. }
                if name == crate::harness::native_tools::AGENT_WAIT_FOR_MESSAGE_TOOL
        )));
        assert!(events.iter().any(|event| matches!(event, AgentEvent::Done)));
    }

    #[tokio::test]
    async fn executor_continues_after_invalid_wait_for_message_tool_result() {
        let llm = Arc::new(WaitToolThenReplyLlm::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let spec = manifests::AgentSpec::default();
        let executor = AgentExecutor::new_with_session(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "Tenant:acme:Workspace:main".to_string(),
            "writer".to_string(),
            "writer-session".to_string(),
            None,
            ControlPlane::noop(),
            spec.clone(),
            HashMap::new(),
        );
        {
            let mut reg = registry.write().await;
            crate::harness::native_tools::register_tools(&mut reg, &spec, &Config::default());
        }

        let mut context = ExecutionContext::new("writer");
        context.push(LoopMessage::text("user", "Ask the critic and wait."));
        let sink = CaptureSink::new();
        let reply = executor.execute(&mut context, &sink, None).await.unwrap();

        assert_eq!(*llm.call_count.lock().unwrap(), 1);
        assert_eq!(llm.seen_messages.lock().unwrap().len(), 2);
        assert!(reply.contains("recovered after wait error"));
    }

    #[tokio::test]
    async fn executor_does_not_run_later_tool_calls_after_wait_for_message() {
        let llm = Arc::new(WaitThenScheduleLlm::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let spec = manifests::AgentSpec::default();
        let executor = AgentExecutor::new_with_session(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "Tenant:acme:Workspace:main".to_string(),
            "writer".to_string(),
            "writer-session".to_string(),
            None,
            control_plane_with_wire().await,
            spec.clone(),
            HashMap::new(),
        );
        {
            let mut reg = registry.write().await;
            crate::harness::native_tools::register_tools(&mut reg, &spec, &Config::default());
        }

        let mut context = ExecutionContext::new("writer");
        context.push(LoopMessage::text("user", "Ask the critic and wait."));
        let sink = CaptureSink::new();
        let reply = executor.execute(&mut context, &sink, None).await.unwrap();

        assert!(reply.contains("\"status\":\"WAITING\""), "reply={reply}");
        assert_eq!(llm.seen_messages.lock().unwrap().len(), 1);
        let events = sink.events();
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::Action { name, .. }
                if name == crate::harness::native_tools::AGENT_WAIT_FOR_MESSAGE_TOOL
        )));
        assert!(!events.iter().any(|event| matches!(
            event,
            AgentEvent::Action { name, .. } if name == "create_schedule"
        )));
    }

    struct SlowStreamingLlm;

    #[async_trait]
    impl LlmProvider for SlowStreamingLlm {
        async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
            Ok(vec![0.0; 8])
        }

        async fn chat_completion(&self, _request: ChatRequest) -> Result<ChatResponse> {
            unreachable!("stream_chat_completion is used in this test");
        }

        async fn stream_chat_completion(&self, _request: ChatRequest) -> Result<ChatStream> {
            Ok(Box::pin(futures::stream::unfold(
                0usize,
                |state| async move {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    let token = match state {
                        0 => "Hello",
                        1 => " world",
                        _ => " trailing",
                    };
                    Some((Ok(text_delta_event(token.to_string())), state + 1))
                },
            )))
        }

        async fn completion(&self, prompt: &str) -> Result<String> {
            Ok(prompt.to_string())
        }
    }

    #[tokio::test]
    async fn executor_returns_partial_reply_when_cancelled() {
        let llm = Arc::new(SlowStreamingLlm);
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let executor = AgentExecutor::new(
            llm,
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            manifests::AgentSpec::default(),
            HashMap::new(),
        );

        let cancellation = CancellationToken::new();
        let cancel_clone = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(35)).await;
            cancel_clone.cancel();
        });

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage::text("user", "Say hello"));
        let sink = CaptureSink::new();
        let reply = executor
            .execute(&mut context, &sink, Some(&cancellation))
            .await
            .unwrap();

        assert_eq!(reply, "Hello");
        assert_eq!(
            context
                .history
                .iter()
                .filter(|message| message.role == "user")
                .count(),
            1
        );
        assert!(matches!(sink.events().last(), Some(AgentEvent::Done)));
    }

    #[tokio::test]
    async fn executor_buffers_stream_usage_until_turn_boundary() {
        let llm = Arc::new(InterleavedUsageLlm);
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let executor = AgentExecutor::new(
            llm,
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            manifests::AgentSpec::default(),
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage::text("user", "Say hello"));
        let sink = CaptureSink::new();
        let reply = executor.execute(&mut context, &sink, None).await.unwrap();

        assert_eq!(reply, "hello world");
        assert_eq!(
            sink.events(),
            vec![
                AgentEvent::Token("hello ".to_string()),
                AgentEvent::Token("world".to_string()),
                AgentEvent::Usage(TokenCounter {
                    input_tokens: 10,
                    output_tokens: 2,
                    reasoning_output_tokens: 0,
                    total_tokens: 12,
                    usage_available: true,
                    provider: "test-provider".to_string(),
                    model: "test-model".to_string(),
                    ..Default::default()
                }),
                AgentEvent::Done,
            ]
        );
    }

    #[tokio::test]
    async fn context_assembler_reads_existing_files_and_defaults_missing_ones() {
        let dir = tempdir().expect("tempdir");
        tokio::fs::write(dir.path().join("SOUL.md"), "soul body")
            .await
            .expect("write soul");
        tokio::fs::write(dir.path().join("USER.md"), "user body")
            .await
            .expect("write user");

        let assembled = ContextAssembler::new(dir.path())
            .assemble()
            .await
            .expect("assemble");
        assert!(assembled.contains("soul body"));
        assert!(assembled.contains("user body"));
        assert!(assembled.contains("(No AGENTS.md provided)"));
    }

    #[tokio::test]
    async fn capture_sink_records_all_event_types() {
        let sink = CaptureSink::new();
        sink.on_token("tok").await;
        sink.on_tool_call("id-1", "tool", &json!({"x": 1})).await;
        sink.on_tool_result("id-1", "tool", &ToolOutput::text("result"))
            .await;
        sink.on_done().await;
        sink.on_error("boom").await;

        assert_eq!(
            sink.events(),
            vec![
                AgentEvent::Token("tok".to_string()),
                AgentEvent::Action {
                    id: "id-1".to_string(),
                    name: "tool".to_string(),
                    input: json!({"x": 1}),
                },
                AgentEvent::Observation {
                    id: "id-1".to_string(),
                    name: "tool".to_string(),
                    output: "result".to_string(),
                },
                AgentEvent::Done,
                AgentEvent::Error("boom".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn executor_execute_tool_does_not_fallback_to_legacy_knowledge() {
        let executor = AgentExecutor::new(
            Arc::new(RecordingLlmProvider::default()),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            Arc::new(tokio::sync::RwLock::new(ToolRegistry::new())),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            manifests::AgentSpec::default(),
            HashMap::new(),
        );

        let legacy = executor
            .execute_tool(
                crate::harness::knowledge::KNOWLEDGE_SEARCH_TOOL,
                r#"{"query":"plan"}"#,
            )
            .await
            .expect("legacy knowledge tool should not error");
        assert_eq!(legacy.summary(), "Tool 'knowledge_search' not found.");

        let unknown = executor
            .execute_tool("missing_tool", "not-json")
            .await
            .expect("unknown tool should not error");
        assert_eq!(unknown.summary(), "Tool 'missing_tool' not found.");
    }

    #[tokio::test]
    async fn executor_triggers_compaction_when_context_exceeds_threshold() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let spec = manifests::AgentSpec::default();
        let executor = AgentExecutor::new_with_session(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            "session-1".to_string(),
            None,
            ControlPlane::noop(),
            spec.clone(),
            HashMap::new(),
        );
        {
            let mut reg = registry.write().await;
            crate::harness::native_tools::register_tools(&mut reg, &spec, &Config::default());
        }

        // Build history that exceeds the model-aware default context budget.
        let mut context = ExecutionContext::new("cmo");
        for i in 0..5 {
            context.push(LoopMessage::text(
                "assistant",
                format!("Assistant response #{}: {}", i, "x".repeat(40_000)),
            ));
            context.push(LoopMessage::text(
                "user",
                format!("User input #{}: {}", i, "y".repeat(500)),
            ));
        }

        let sink = CaptureSink::new();
        let reply = executor.execute(&mut context, &sink, None).await.unwrap();
        assert_eq!(reply, "resolved");
        assert_eq!(sink.compactions().len(), 1);
        assert!(!sink.compactions()[0].trim().is_empty());
    }

    #[test]
    fn provider_snapshot_estimate_adds_the_new_message() {
        let executor = AgentExecutor::new(
            Arc::new(RecordingLlmProvider::default()),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            Arc::new(tokio::sync::RwLock::new(ToolRegistry::new())),
            Arc::new(Config::default()),
            "ns".to_string(),
            "agent".to_string(),
            ControlPlane::noop(),
            manifests::AgentSpec::default(),
            HashMap::new(),
        );
        let context = ExecutionContext::with_history(
            "agent",
            vec![
                LoopMessage::text("assistant", "prior response"),
                LoopMessage::text("user", "new message"),
            ],
        );
        let counter = TokenCounter {
            input_tokens: 100,
            usage_available: true,
            provider: "test-provider".to_string(),
            model: "test-model".to_string(),
            ..Default::default()
        };
        let delta_tokens = (serialized_message_weight(&context.history[1]) as u64 + 3) / 4;

        assert_eq!(
            executor.estimate_context_input_tokens(&context, Some(&counter), None, 0),
            Some(100 + delta_tokens)
        );
    }

    #[test]
    fn provider_snapshot_estimate_requires_reported_input_tokens() {
        let executor = AgentExecutor::new(
            Arc::new(RecordingLlmProvider::default()),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            Arc::new(tokio::sync::RwLock::new(ToolRegistry::new())),
            Arc::new(Config::default()),
            "ns".to_string(),
            "agent".to_string(),
            ControlPlane::noop(),
            manifests::AgentSpec::default(),
            HashMap::new(),
        );
        let context =
            ExecutionContext::with_history("agent", vec![LoopMessage::text("user", "new message")]);
        let counter = TokenCounter {
            usage_available: true,
            provider: "test-provider".to_string(),
            model: "test-model".to_string(),
            ..Default::default()
        };

        assert_eq!(
            executor.estimate_context_input_tokens(&context, Some(&counter), None, 0),
            None
        );
    }

    #[tokio::test]
    async fn provider_snapshot_can_trigger_compaction_before_character_budget() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let mut config = Config::default();
        config.models.insert(
            "test-model".to_string(),
            crate::control::config::proto::ModelConfig {
                context_window_tokens: Some(1_000),
                ..Default::default()
            },
        );
        let executor = AgentExecutor::new_with_session(
            llm,
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry,
            Arc::new(config),
            "ns".to_string(),
            "agent".to_string(),
            String::new(),
            Some(TokenCounter {
                input_tokens: 900,
                usage_available: true,
                provider: "test-provider".to_string(),
                model: "test-model".to_string(),
                ..Default::default()
            }),
            ControlPlane::noop(),
            manifests::AgentSpec::default(),
            HashMap::new(),
        );
        let mut context = ExecutionContext::with_history(
            "agent",
            vec![
                LoopMessage::text("system", "system"),
                LoopMessage::text("user", "initial ".to_string() + &"x".repeat(2_000)),
                LoopMessage::text("assistant", "prior response"),
                LoopMessage::text("user", "new message"),
            ],
        );
        let sink = CaptureSink::new();

        executor.execute(&mut context, &sink, None).await.unwrap();

        assert_eq!(sink.compactions().len(), 1);
    }

    #[tokio::test]
    async fn failed_compaction_persistence_keeps_live_history_unchanged() {
        let executor = AgentExecutor::new(
            Arc::new(RecordingLlmProvider::default()),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            Arc::new(tokio::sync::RwLock::new(ToolRegistry::new())),
            Arc::new(Config::default()),
            "ns".to_string(),
            "agent".to_string(),
            ControlPlane::noop(),
            manifests::AgentSpec::default(),
            HashMap::new(),
        );
        let mut context = ExecutionContext::new("agent");
        for index in 0..4 {
            context.push(LoopMessage::text(
                "assistant",
                format!("old {index}: {}", "x".repeat(20_000)),
            ));
            context.push(LoopMessage::text(
                "user",
                format!("new {index}: {}", "y".repeat(20_000)),
            ));
        }
        let before = context.history.clone();
        let sink = CaptureSink::failing_compaction();

        let error = compact(executor.llm.as_ref(), &mut context, &sink)
            .await
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("injected compaction persistence failure"));
        assert_eq!(context.history, before);
        assert_eq!(sink.compactions().len(), 1);
    }

    #[tokio::test]
    async fn executor_skips_compaction_when_history_is_short() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let spec = manifests::AgentSpec::default();
        let executor = AgentExecutor::new_with_session(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            "session-1".to_string(),
            None,
            ControlPlane::noop(),
            spec.clone(),
            HashMap::new(),
        );
        {
            let mut reg = registry.write().await;
            crate::harness::native_tools::register_tools(&mut reg, &spec, &Config::default());
        }

        // Only 2 messages -- well under threshold.
        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage::text("assistant", "Small reply"));
        context.push(LoopMessage::text("user", "A user asked"));

        let sink = CaptureSink::new();
        let reply = executor.execute(&mut context, &sink, None).await.unwrap();
        assert_eq!(reply, "resolved");
    }

    #[tokio::test]
    async fn compaction_continues_execution_after_malformed_summary() {
        let llm = Arc::new(RecordingLlmProvider::with_compaction_content(
            "Facts acknowledged.",
        ));
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let spec = manifests::AgentSpec::default();
        let executor = AgentExecutor::new_with_session(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            "session-1".to_string(),
            None,
            ControlPlane::noop(),
            spec.clone(),
            HashMap::new(),
        );
        {
            let mut reg = registry.write().await;
            crate::harness::native_tools::register_tools(&mut reg, &spec, &Config::default());
        }

        let mut context = ExecutionContext::new("cmo");
        for _ in 0..5 {
            context.push(LoopMessage::text(
                "assistant",
                format!("Long rep: {}", "x".repeat(20_000)),
            ));
            context.push(LoopMessage::text("user", "Continue"));
        }

        let sink = CaptureSink::new();
        let reply = executor.execute(&mut context, &sink, None).await.unwrap();
        assert_eq!(reply, "resolved");
        assert_eq!(sink.compactions(), vec!["Facts acknowledged.".to_string()]);
    }

    #[tokio::test]
    async fn empty_compaction_summary_skips_durable_compaction_and_continues_execution() {
        let llm = Arc::new(RecordingLlmProvider::with_compaction_content(""));
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let spec = manifests::AgentSpec::default();
        let executor = AgentExecutor::new_with_session(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            "session-1".to_string(),
            None,
            ControlPlane::noop(),
            spec.clone(),
            HashMap::new(),
        );
        {
            let mut reg = registry.write().await;
            crate::harness::native_tools::register_tools(&mut reg, &spec, &Config::default());
        }

        let mut context = ExecutionContext::new("cmo");
        for _ in 0..5 {
            context.push(LoopMessage::text(
                "assistant",
                format!("Long rep: {}", "x".repeat(20_000)),
            ));
            context.push(LoopMessage::text("user", "Continue"));
        }
        let before = context.history.clone();

        let sink = CaptureSink::new();
        let reply = executor.execute(&mut context, &sink, None).await.unwrap();

        assert_eq!(reply, "resolved");
        assert!(sink.compactions().is_empty());
        assert_eq!(&context.history[..before.len()], before);
    }
}
