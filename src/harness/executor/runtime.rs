// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::config::Config;
use crate::control::ControlPlane;
use crate::harness::executor::compaction::compact_history_for_llm;
use crate::harness::knowledge::KnowledgeBook;
use crate::harness::llm::resolver::resolve_model_profile;
use crate::harness::llm::{
    chat_content_part, chat_stream_event, text_part, ChatContentPart, ChatMessage, ChatRequest,
    ChatResponse, ChatStreamEvent, ChatUsage, LlmProvider, ToolCall,
};
use crate::harness::mcp::{call_tool_for_config, McpConnectionConfig};
use crate::harness::skills::registry::ToolRegistry;
use crate::harness::telemetry;
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    Usage(ChatUsage),
    Done(String),
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
    /// The full completed LLM response reached a durable recovery boundary.
    async fn on_llm_response(&self, _: &crate::harness::llm::ChatResponse) -> Result<()> {
        Ok(())
    }
    /// The tool returned a result.
    async fn on_tool_result(&self, id: &str, name: &str, result: &str);
    /// A tool result has been durably recorded.
    async fn on_tool_result_recorded(&self, _: &str, _: &str, _: &str) -> Result<()> {
        Ok(())
    }
    /// The agent requested permission from the user/client.
    async fn on_request_permission(&self, _: &str, _: &str, _: &Value) {}
    /// The permission request was answered or cancelled.
    async fn on_permission_result(&self, _: &str, _: &Value) {}
    /// Usage metadata for the completed model turn.
    async fn on_usage(&self, usage: &ChatUsage);
    /// The execution completed successfully with a final reply.
    async fn on_done(&self, reply: &str);
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
    async fn on_tool_result(&self, _: &str, _: &str, _: &str) {}
    async fn on_tool_result_recorded(&self, _: &str, _: &str, _: &str) -> Result<()> {
        Ok(())
    }
    async fn on_request_permission(&self, _: &str, _: &str, _: &Value) {}
    async fn on_permission_result(&self, _: &str, _: &Value) {}
    async fn on_usage(&self, _: &ChatUsage) {}
    async fn on_done(&self, _: &str) {}
    async fn on_error(&self, _: &str) {}
}

/// Test sink that captures all events for assertion.
pub struct CaptureSink {
    pub events: std::sync::Mutex<Vec<AgentEvent>>,
}

impl CaptureSink {
    pub fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn events(&self) -> Vec<AgentEvent> {
        self.events.lock().unwrap().clone()
    }
}

#[async_trait]
impl ExecutionSink for CaptureSink {
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
    async fn on_tool_result(&self, id: &str, name: &str, result: &str) {
        self.events.lock().unwrap().push(AgentEvent::Observation {
            id: id.to_string(),
            name: name.to_string(),
            output: result.to_string(),
        });
    }
    async fn on_tool_result_recorded(&self, _: &str, _: &str, _: &str) -> Result<()> {
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
    async fn on_usage(&self, usage: &ChatUsage) {
        self.events
            .lock()
            .unwrap()
            .push(AgentEvent::Usage(usage.clone()));
    }
    async fn on_done(&self, reply: &str) {
        self.events
            .lock()
            .unwrap()
            .push(AgentEvent::Done(reply.to_string()));
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
    pub knowledge: Arc<dyn KnowledgeBook>,
    pub namespace: String,
    pub agent_id: String,
    pub session_id: String,
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
        knowledge: Arc<dyn KnowledgeBook>,
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
            registry,
            config,
            knowledge,
            namespace,
            agent_id,
            String::new(),
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
        knowledge: Arc<dyn KnowledgeBook>,
        namespace: String,
        agent_id: String,
        session_id: String,
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
            knowledge,
            namespace,
            agent_id,
            session_id,
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

    fn render_execution_prompts(&self, context: &ExecutionContext) -> Result<ExecutionPrompts> {
        let system_prompt = self.agent_spec.system_prompt.trim();
        let system_prompt = if !system_prompt.is_empty() && !context.has_system_message() {
            Some(
                crate::control::manifest::templating::render_runtime_system_prompt_template(
                    system_prompt,
                )?,
            )
        } else {
            None
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

    fn messages_for_llm(
        &self,
        context: &ExecutionContext,
        prompts: &ExecutionPrompts,
    ) -> Vec<ChatMessage> {
        let mut history = context.history.clone();
        if let Some(system_prompt) = prompts.system_prompt.as_deref() {
            history.insert(0, LoopMessage::text("system", system_prompt.to_string()));
        }
        if let Some(post_history_prompt) = prompts.post_history_prompt.as_deref() {
            prefix_latest_user_message(&mut history, post_history_prompt);
        }

        compact_history_for_llm(&history)
            .iter()
            .map(|m| ChatMessage {
                role: m.role.clone(),
                content_parts: m.content_parts.clone(),
                tool_calls: m.tool_calls.clone().unwrap_or_default(),
                tool_call_id: m.tool_call_id.clone(),
            })
            .collect()
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
        loop {
            if turn_limit == 0 {
                let msg = "Turn limit reached".to_string();
                return Err(anyhow::anyhow!(msg));
            }
            turn_limit -= 1;

            let messages = self.messages_for_llm(context, &prompts);

            let tools = {
                let reg = self.registry.read().await;
                reg.to_provider_tools()
            };

            let mut final_reply = String::new();
            let mut tool_calls_by_index: BTreeMap<usize, ToolCall> = BTreeMap::new();
            let mut final_usage: Option<ChatUsage> = None;
            let thinking = resolve_model_profile(self.agent_spec.model_policy.as_ref())
                .and_then(|model| model.thinking.clone());
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
            };
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
                &request,
                reasoning_level,
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
                            sink.on_done(&final_reply).await;
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
                        final_usage = Some(usage.clone());
                        sink.on_usage(&usage).await;
                    }
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
                usage: final_usage,
            };
            telemetry::record_chat_output(
                &llm_span,
                &llm_response.content,
                &llm_response.tool_calls,
            );
            if let Some(usage) = llm_response.usage.as_ref() {
                telemetry::record_usage(&llm_span, usage);
            }
            sink.on_llm_response(&llm_response).await?;
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
                    let result = self
                        .execute_tool_call_result(tool)
                        .instrument(tool_span.clone())
                        .await;
                    telemetry::record_tool_result(&tool_span, &result);
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
                    context.push(tool_result_loop_message(&tool.id, &result));
                }
                continue;
            }

            sink.on_done(&final_reply).await;
            return Ok(final_reply);
        }
    }

    pub async fn execute_tool_call(&self, tool: &ToolCall) -> (Value, String) {
        let input = Self::tool_call_input(tool);
        let result = self.execute_tool_call_result(tool).await;
        (input, result)
    }

    pub fn tool_call_input(tool: &ToolCall) -> Value {
        serde_json::from_str(&tool.arguments).unwrap_or(Value::Null)
    }

    async fn tool_type(&self, name: &str) -> &'static str {
        if self.mcp_tools.contains_key(name) {
            "mcp"
        } else if matches!(
            name,
            crate::harness::knowledge::KNOWLEDGE_WRITE_TOOL
                | crate::harness::knowledge::KNOWLEDGE_SEARCH_TOOL
                | crate::harness::knowledge::KNOWLEDGE_GET_TOOL
                | crate::harness::knowledge::KNOWLEDGE_LIST_TOOL
        ) {
            "retrieval"
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

    async fn execute_tool_call_result(&self, tool: &ToolCall) -> String {
        let result = match self.execute_tool(&tool.name, &tool.arguments).await {
            Ok(result) => result,
            Err(error) => tool_error_result(&tool.name, &error),
        };
        result
    }

    async fn execute_tool(&self, name: &str, input: &str) -> Result<String> {
        let args: Value = serde_json::from_str(input).unwrap_or(Value::Null);
        if let Some(tool) = self.mcp_tools.get(name) {
            return call_tool_for_config(&tool.config, &tool.remote_name, args).await;
        }
        if let Some(result) = crate::harness::native_tools::execute_tool_for_session(
            &self.control_plane,
            &self.namespace,
            &self.agent_id,
            &self.session_id,
            &self.agent_spec,
            name,
            &args,
        )
        .await?
        {
            return Ok(result);
        }
        if let Some(result) = crate::harness::knowledge::execute_tool(
            self.knowledge.as_ref(),
            &self.namespace,
            name,
            &args,
        )
        .await?
        {
            Ok(result)
        } else {
            Ok(format!("Tool '{}' not found.", name))
        }
    }
}

pub fn tool_result_loop_message(tool_call_id: &str, result: &str) -> LoopMessage {
    let mut tool_message = LoopMessage::text("tool", result.to_string());
    tool_message.tool_call_id = Some(tool_call_id.to_string());
    tool_message
}

#[cfg(test)]
mod tests {
    use super::{
        AgentEvent, AgentExecutor, CaptureSink, ContextAssembler, ExecutionContext, ExecutionSink,
        LoopMessage,
    };
    use crate::control::config::Config;
    use crate::control::ControlPlane;
    use crate::gateway::rpc::{
        manifests,
        protobuf_value::{value::Kind as ProtoValueKind, ListValue, Value as ProtoValue},
    };
    use crate::harness::knowledge::{
        KnowledgeBook, KnowledgeEntry, KnowledgeListEntry, KnowledgeResult,
    };
    use crate::harness::llm::provider::{
        image_data_part, text_delta_event, tool_call_delta_event, ChatMessage, ChatMessageExt,
        ChatRequest, ChatResponse, ChatStream, LlmProvider,
    };
    use crate::harness::memory::Embedding;
    use crate::harness::skills::registry::ToolRegistry;
    use anyhow::Result;
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    #[derive(Default)]
    struct EmptyKnowledgeBook;

    #[async_trait]
    impl KnowledgeBook for EmptyKnowledgeBook {
        async fn get(&self, _ns: &str, _path: &str) -> Result<Option<KnowledgeEntry>> {
            Ok(None)
        }

        async fn write(&self, _ns: &str, _path: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        async fn search(
            &self,
            _ns: &str,
            _query: &str,
            _limit: usize,
        ) -> Result<Vec<KnowledgeResult>> {
            Ok(Vec::new())
        }

        async fn list(
            &self,
            _ns: &str,
            _path_prefix: &str,
            _local_only: bool,
            _recursive: bool,
            _limit: usize,
        ) -> Result<Vec<KnowledgeListEntry>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct RecordingKnowledgeBook {
        writes: Mutex<Vec<(String, String, String)>>,
    }

    #[async_trait]
    impl KnowledgeBook for RecordingKnowledgeBook {
        async fn get(&self, ns: &str, path: &str) -> Result<Option<KnowledgeEntry>> {
            Ok((path == "notes/plan.md").then(|| KnowledgeEntry {
                namespace: ns.to_string(),
                name: "plan".to_string(),
                path: path.to_string(),
                content: "remember the plan".to_string(),
                updated_at: 42,
            }))
        }

        async fn write(&self, ns: &str, path: &str, content: &str) -> Result<()> {
            self.writes.lock().unwrap().push((
                ns.to_string(),
                path.to_string(),
                content.to_string(),
            ));
            Ok(())
        }

        async fn search(
            &self,
            ns: &str,
            query: &str,
            _limit: usize,
        ) -> Result<Vec<KnowledgeResult>> {
            if query == "plan" {
                Ok(vec![KnowledgeResult {
                    namespace: ns.to_string(),
                    path: "notes/plan.md".to_string(),
                    excerpt: "remember the plan".to_string(),
                    updated_at: 42,
                }])
            } else {
                Ok(Vec::new())
            }
        }

        async fn list(
            &self,
            ns: &str,
            path_prefix: &str,
            _local_only: bool,
            _recursive: bool,
            _limit: usize,
        ) -> Result<Vec<KnowledgeListEntry>> {
            Ok(vec![KnowledgeListEntry {
                namespace: ns.to_string(),
                path: format!("{}/plan.md", path_prefix.trim_matches('/')),
                updated_at: 42,
                inherited: false,
            }])
        }
    }

    #[derive(Default)]
    struct RecordingLlmProvider {
        seen_messages: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
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
            Ok(ChatResponse {
                content: "resolved".to_string(),
                tool_calls: Vec::new(),
                usage: None,
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
            registry,
            Arc::new(Config::default()),
            Arc::new(EmptyKnowledgeBook),
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
            registry,
            Arc::new(Config::default()),
            Arc::new(EmptyKnowledgeBook),
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
            registry,
            Arc::new(Config::default()),
            Arc::new(EmptyKnowledgeBook),
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
            registry,
            Arc::new(Config::default()),
            Arc::new(EmptyKnowledgeBook),
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
            registry,
            Arc::new(Config::default()),
            Arc::new(EmptyKnowledgeBook),
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
        let mut spec = manifests::AgentSpec::default();
        spec.post_history_prompt = "Use this context.".to_string();
        let executor = AgentExecutor::new(
            llm.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            registry,
            Arc::new(Config::default()),
            Arc::new(EmptyKnowledgeBook),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            spec,
            HashMap::new(),
        );

        let mut context = ExecutionContext::new("cmo");
        context.push(LoopMessage {
            role: "user".to_string(),
            content_parts: vec![image_data_part("image/png", "cG5n", Some("low"))],
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
            registry,
            Arc::new(Config::default()),
            Arc::new(EmptyKnowledgeBook),
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
            registry,
            Arc::new(Config::default()),
            Arc::new(EmptyKnowledgeBook),
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
            registry,
            Arc::new(Config::default()),
            Arc::new(EmptyKnowledgeBook),
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
            Arc::new(EmptyKnowledgeBook),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            spec.clone(),
            HashMap::new(),
        );
        {
            let mut reg = registry.write().await;
            crate::harness::native_tools::register_tools(&mut reg, &spec);
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
            registry,
            Arc::new(Config::default()),
            Arc::new(EmptyKnowledgeBook),
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
        assert!(matches!(
            sink.events().last(),
            Some(AgentEvent::Done(content)) if content == "Hello"
        ));
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
        sink.on_tool_result("id-1", "tool", "result").await;
        sink.on_done("done").await;
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
                AgentEvent::Done("done".to_string()),
                AgentEvent::Error("boom".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn executor_execute_tool_covers_knowledge_and_unknown_paths() {
        let knowledge = Arc::new(RecordingKnowledgeBook::default());
        let executor = AgentExecutor::new(
            Arc::new(RecordingLlmProvider::default()),
            "test-provider".to_string(),
            "test-model".to_string(),
            ContextAssembler::new("."),
            Arc::new(tokio::sync::RwLock::new(ToolRegistry::new())),
            Arc::new(Config::default()),
            knowledge.clone(),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane::noop(),
            manifests::AgentSpec::default(),
            HashMap::new(),
        );

        let write = executor
            .execute_tool(
                crate::harness::knowledge::KNOWLEDGE_WRITE_TOOL,
                r#"{"path":"notes/plan.md","content":"remember the plan"}"#,
            )
            .await
            .expect("knowledge write");
        assert!(write.contains("wrote artifact"));

        let get = executor
            .execute_tool(
                crate::harness::knowledge::KNOWLEDGE_GET_TOOL,
                r#"{"path":"notes/plan.md"}"#,
            )
            .await
            .expect("knowledge get");
        assert!(get.contains("[conic:wks:13:notes/plan.md]"));

        let search = executor
            .execute_tool(
                crate::harness::knowledge::KNOWLEDGE_SEARCH_TOOL,
                r#"{"query":"plan"}"#,
            )
            .await
            .expect("knowledge search");
        assert!(search.contains("remember the plan"));

        let list = executor
            .execute_tool(
                crate::harness::knowledge::KNOWLEDGE_LIST_TOOL,
                r#"{"path":"notes"}"#,
            )
            .await
            .expect("knowledge list");
        assert!(list.contains("\"path\": \"notes/plan.md\""));

        let unknown = executor
            .execute_tool("missing_tool", "not-json")
            .await
            .expect("unknown tool should not error");
        assert_eq!(unknown, "Tool 'missing_tool' not found.");

        assert_eq!(
            knowledge.writes.lock().unwrap().as_slice(),
            &[(
                "conic:wks:13".to_string(),
                "notes/plan.md".to_string(),
                "remember the plan".to_string(),
            )]
        );
    }
}
