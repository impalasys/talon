// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info_span;

use crate::config::Config;
use crate::connectors::mcp::{call_tool_for_config, McpConnectionConfig};
use crate::control::ControlPlane;
use crate::core::context_budget::{compact_history_for_llm, tool_result_preview};
use crate::gateway::rpc::manifests;
use crate::knowledge::KnowledgeBook;
use crate::llm::{ChatMessage, ChatRequest, ChatStreamEvent, ChatUsage, LlmProvider, ToolCall};
use crate::skills::registry::ToolRegistry;

const DEFAULT_EXECUTION_TURN_LIMIT: usize = 25;

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
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
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
    /// The tool returned a result.
    async fn on_tool_result(&self, id: &str, name: &str, result: &str);
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
    async fn on_tool_result(&self, _: &str, _: &str, _: &str) {}
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
}

impl ContextAssembler {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
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
        Ok(format!(
            "# IDENTITY & PERSONALITY (SOUL.md)\n{}\n\n# USER CONTEXT (USER.md)\n{}\n\n# OPERATIONAL RULES (AGENTS.md)\n{}\n",
            soul, user, agents
        ))
    }
}

// ─── AgentExecutor ────────────────────────────────────────────────────────────

pub struct AgentExecutor {
    pub llm: Arc<dyn LlmProvider>,
    pub assembler: ContextAssembler,
    pub registry: Arc<tokio::sync::RwLock<ToolRegistry>>,
    pub config: Arc<Config>,
    pub knowledge: Arc<dyn KnowledgeBook>,
    pub namespace: String,
    pub agent_id: String,
    pub control_plane: ControlPlane,
    pub agent_spec: crate::gateway::rpc::manifests::AgentSpec,
    pub mcp_tools: HashMap<String, RegisteredMcpTool>,
}

#[derive(Debug, Clone)]
pub struct RegisteredMcpTool {
    pub config: McpConnectionConfig,
    pub remote_name: String,
}

impl AgentExecutor {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
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
        Self {
            llm,
            assembler,
            registry,
            config,
            knowledge,
            namespace,
            agent_id,
            control_plane,
            agent_spec,
            mcp_tools,
        }
    }

    /// Run a task to completion, emitting events to `sink` along the way.
    /// Returns the final reply text.
    pub async fn execute(
        &self,
        context: &mut ExecutionContext,
        task: &str,
        sink: &dyn ExecutionSink,
        cancellation_token: Option<&CancellationToken>,
    ) -> Result<String> {
        let span = info_span!("agent_execute", agent_id = %context.agent_id, task = %task);
        let _enter = span.enter();

        // Inject system prompt on first turn
        if context.history.is_empty() {
            let soul = self.assembler.assemble().await?;
            context.push(LoopMessage {
                role: "system".to_string(),
                content: soul,
                tool_calls: None,
                tool_call_id: None,
            });
        }

        context.push(LoopMessage {
            role: "user".to_string(),
            content: task.to_string(),
            tool_calls: None,
            tool_call_id: None,
        });

        let mut turn_limit = DEFAULT_EXECUTION_TURN_LIMIT;
        loop {
            if turn_limit == 0 {
                let msg = "Turn limit reached".to_string();
                sink.on_error(&msg).await;
                return Err(anyhow::anyhow!(msg));
            }
            turn_limit -= 1;

            let messages: Vec<ChatMessage> = compact_history_for_llm(&context.history)
                .iter()
                .map(|m| ChatMessage {
                    role: m.role.clone(),
                    content: m.content.clone(),
                    tool_calls: m.tool_calls.clone(),
                    tool_call_id: m.tool_call_id.clone(),
                })
                .collect();

            let tools = {
                let reg = self.registry.read().await;
                reg.to_provider_tools()
            };

            let mut final_reply = String::new();
            let mut tool_calls_by_index: BTreeMap<usize, ToolCall> = BTreeMap::new();
            let mut stream = self
                .llm
                .stream_chat_completion(ChatRequest {
                    messages,
                    tools,
                    thinking: self
                        .agent_spec
                        .model_policy
                        .as_ref()
                        .and_then(|policy| {
                            policy
                                .profiles
                                .iter()
                                .find(|profile| profile.name == "default")
                        })
                        .and_then(|profile| profile.model.as_ref())
                        .and_then(|model| model.thinking.clone()),
                })
                .await?;

            loop {
                let next_chunk = if let Some(token) = cancellation_token {
                    tokio::select! {
                        _ = token.cancelled() => {
                            tracing::info!(agent_id = %context.agent_id, "Generation interrupted by user");
                            context.push(LoopMessage {
                                role: "assistant".to_string(),
                                content: final_reply.clone(),
                                tool_calls: None,
                                tool_call_id: None,
                            });
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

                match chunk? {
                    ChatStreamEvent::TextDelta(token) => {
                        final_reply.push_str(&token);
                        sink.on_token(&token).await;
                    }
                    ChatStreamEvent::ReasoningDelta(reasoning) => {
                        sink.on_reasoning(&reasoning).await;
                    }
                    ChatStreamEvent::ToolCallDelta(delta) => {
                        let entry =
                            tool_calls_by_index
                                .entry(delta.index)
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
                    ChatStreamEvent::Usage(usage) => {
                        sink.on_usage(&usage).await;
                    }
                }
            }

            let tool_calls: Vec<ToolCall> = tool_calls_by_index
                .into_values()
                .filter(|tool| !tool.name.is_empty())
                .collect();

            // Record assistant turn
            context.push(LoopMessage {
                role: "assistant".to_string(),
                content: final_reply.clone(),
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls.clone())
                },
                tool_call_id: None,
            });

            if !tool_calls.is_empty() {
                for tool in &tool_calls {
                    let input: Value = serde_json::from_str(&tool.arguments).unwrap_or(Value::Null);
                    sink.on_tool_call(&tool.id, &tool.name, &input).await;
                    let result = match self.execute_tool(&tool.name, &tool.arguments).await {
                        Ok(result) => result,
                        Err(error) => tool_error_result(&tool.name, &error),
                    };
                    sink.on_tool_result(&tool.id, &tool.name, &result).await;
                    let preview = tool_result_preview(&result);

                    context.push(LoopMessage {
                        role: "tool".to_string(),
                        content: preview,
                        tool_calls: None,
                        tool_call_id: Some(tool.id.clone()),
                    });
                }
                continue;
            }

            sink.on_done(&final_reply).await;
            return Ok(final_reply);
        }
    }

    async fn execute_tool(&self, name: &str, input: &str) -> Result<String> {
        let args: Value = serde_json::from_str(input).unwrap_or(Value::Null);
        if let Some(tool) = self.mcp_tools.get(name) {
            return call_tool_for_config(&tool.config, &tool.remote_name, args).await;
        }
        if let Some(result) = crate::native_tools::execute_tool(
            &self.control_plane,
            &self.namespace,
            &self.agent_id,
            &self.agent_spec,
            name,
            &args,
        )
        .await?
        {
            return Ok(result);
        }
        if let Some(result) =
            crate::knowledge::execute_tool(self.knowledge.as_ref(), &self.namespace, name, &args)
                .await?
        {
            Ok(result)
        } else {
            Ok(format!("Tool '{}' not found.", name))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AgentEvent, AgentExecutor, CaptureSink, ContextAssembler, ExecutionContext, ExecutionSink,
        LoopMessage,
    };
    use crate::config::Config;
    use crate::control::scheduler::{ScheduleWakeupRequest, ScheduledWakeup, SchedulerBackend};
    use crate::control::{ControlPlane, KeyValueStore, MessagePublisher};
    use crate::core::ContextBudget;
    use crate::gateway::rpc::{
        manifests,
        protobuf_value::{value::Kind as ProtoValueKind, ListValue, Value as ProtoValue},
    };
    use crate::knowledge::{KnowledgeBook, KnowledgeEntry, KnowledgeResult};
    use crate::llm::provider::{
        ChatMessage, ChatRequest, ChatResponse, ChatStream, ChatStreamEvent, LlmProvider,
    };
    use crate::memory::Embedding;
    use crate::skills::registry::ToolRegistry;
    use anyhow::Result;
    use async_trait::async_trait;
    use futures::Stream;
    use serde_json::json;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    #[derive(Default)]
    struct EmptyKnowledgeBook;

    struct NoopKv;

    #[async_trait]
    impl KeyValueStore for NoopKv {
        async fn get(&self, _namespace: &str, _key: &str) -> Result<Option<Vec<u8>>> {
            Ok(None)
        }
        async fn set(&self, _namespace: &str, _key: &str, _value: &[u8]) -> Result<()> {
            Ok(())
        }
        async fn compare_and_swap(
            &self,
            _namespace: &str,
            _key: &str,
            _expected: Option<&[u8]>,
            _value: &[u8],
        ) -> Result<bool> {
            Ok(true)
        }
        async fn delete(&self, _namespace: &str, _key: &str) -> Result<()> {
            Ok(())
        }
        async fn list_keys(&self, _namespace: &str, _prefix: &str) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    struct NoopPubSub;

    #[async_trait]
    impl MessagePublisher for NoopPubSub {
        async fn publish(&self, _topic: &str, _message: &[u8]) -> Result<()> {
            Ok(())
        }
        async fn subscribe(
            &self,
            _topic: &str,
        ) -> Result<Pin<Box<dyn Stream<Item = Vec<u8>> + Send>>> {
            Ok(Box::pin(futures::stream::empty()))
        }
    }

    struct NoopScheduler;

    #[async_trait]
    impl SchedulerBackend for NoopScheduler {
        async fn schedule(&self, _req: ScheduleWakeupRequest) -> Result<ScheduledWakeup> {
            Ok(ScheduledWakeup::default())
        }
        async fn cancel(&self, _handle: &str) -> Result<()> {
            Ok(())
        }
    }

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

        async fn chat_completion(
            &self,
            request: ChatRequest,
        ) -> Result<ChatResponse> {
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
                Ok(ChatStreamEvent::TextDelta(response.content))
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

        async fn chat_completion(
            &self,
            _request: ChatRequest,
        ) -> Result<ChatResponse> {
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
                    Ok(ChatStreamEvent::ToolCallDelta(crate::llm::provider::ToolCallDelta {
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
                    Ok(ChatStreamEvent::TextDelta(
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

    #[tokio::test]
    async fn executor_compacts_noisy_history_before_next_turn() {
        let llm = Arc::new(RecordingLlmProvider::default());
        let registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let executor = AgentExecutor::new(
            llm.clone(),
            ContextAssembler::new("."),
            registry,
            Arc::new(Config::default()),
            Arc::new(EmptyKnowledgeBook),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane {
                kv: Arc::new(NoopKv),
                pubsub: Arc::new(NoopPubSub),
                scheduler: Arc::new(NoopScheduler),
            },
            manifests::AgentSpec::default(),
            HashMap::new(),
        );

        let huge_tool_result = format!(
            "{{\"items\":[{{\"path\":\"footer.tsx\",\"content\":\"{}\"}}],\"query\":\"repo:pablonyx/proliferate blog\"}}",
            "x".repeat(150_000)
        );
        let mut history = vec![LoopMessage {
            role: "system".to_string(),
            content: "You are Conic.".to_string(),
            tool_calls: None,
            tool_call_id: None,
        }];
        for index in 0..10 {
            history.push(LoopMessage {
                role: "user".to_string(),
                content: format!("Earlier question #{index}: {}", "q".repeat(8_000)),
                tool_calls: None,
                tool_call_id: None,
            });
            history.push(LoopMessage {
                role: "assistant".to_string(),
                content: format!("Earlier answer #{index}: {}", "a".repeat(8_000)),
                tool_calls: None,
                tool_call_id: None,
            });
        }
        history.push(LoopMessage {
            role: "assistant".to_string(),
            content: "Investigating repo.".to_string(),
            tool_calls: Some(vec![crate::llm::ToolCall {
                id: "tool-1".to_string(),
                name: "mcp_github_search_code".to_string(),
                arguments: "{\"query\":\"repo:pablonyx/proliferate blog\"}".to_string(),
            }]),
            tool_call_id: None,
        });
        history.push(LoopMessage {
            role: "tool".to_string(),
            content: huge_tool_result,
            tool_calls: None,
            tool_call_id: Some("tool-1".to_string()),
        });

        let mut context = ExecutionContext::with_history("cmo", history);
        let reply = executor
            .execute(
                &mut context,
                "I'm talking about the blogs link in the footer and the blogs pages",
                &CaptureSink::new(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(reply, "resolved");
        let seen = llm.seen_messages.lock().unwrap();
        let messages = seen.last().unwrap();
        assert!(messages.iter().any(|message| {
            message.role == "user"
                && message.content
                    == "I'm talking about the blogs link in the footer and the blogs pages"
        }));
        let tool_message = messages
            .iter()
            .find(|message| message.role == "tool")
            .unwrap();
        assert!(tool_message.content.len() <= ContextBudget::default().max_tool_result_chars);
        assert!(
            tool_message.content.contains("chars omitted")
                || tool_message.content.contains("_truncated")
        );
        let assistant_tool_call = messages
            .iter()
            .find(|message| {
                message
                    .tool_calls
                    .as_ref()
                    .is_some_and(|calls| calls.iter().any(|call| call.id == "tool-1"))
            })
            .unwrap();
        assert_eq!(assistant_tool_call.role, "assistant");
        assert!(messages.iter().any(|message| {
            message.role == "system" && message.content.contains("earlier messages omitted")
        }));
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
            ContextAssembler::new("."),
            registry.clone(),
            Arc::new(Config::default()),
            Arc::new(EmptyKnowledgeBook),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane {
                kv: Arc::new(NoopKv),
                pubsub: Arc::new(NoopPubSub),
                scheduler: Arc::new(NoopScheduler),
            },
            spec.clone(),
            HashMap::new(),
        );
        {
            let mut reg = registry.write().await;
            crate::native_tools::register_tools(&mut reg, &spec);
        }

        let mut context = ExecutionContext::new("cmo");
        let sink = CaptureSink::new();
        let reply = executor
            .execute(&mut context, "Create a 1-minute schedule", &sink, None)
            .await
            .unwrap();

        assert_eq!(
            reply,
            "That failed because the minimum interval is 300 seconds."
        );
        let events = sink.events();
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

        async fn chat_completion(
            &self,
            _request: ChatRequest,
        ) -> Result<ChatResponse> {
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
                    Some((Ok(ChatStreamEvent::TextDelta(token.to_string())), state + 1))
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
            ContextAssembler::new("."),
            registry,
            Arc::new(Config::default()),
            Arc::new(EmptyKnowledgeBook),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane {
                kv: Arc::new(NoopKv),
                pubsub: Arc::new(NoopPubSub),
                scheduler: Arc::new(NoopScheduler),
            },
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
        let sink = CaptureSink::new();
        let reply = executor
            .execute(&mut context, "Say hello", &sink, Some(&cancellation))
            .await
            .unwrap();

        assert_eq!(reply, "Hello");
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
            ContextAssembler::new("."),
            Arc::new(tokio::sync::RwLock::new(ToolRegistry::new())),
            Arc::new(Config::default()),
            knowledge.clone(),
            "conic:wks:13".to_string(),
            "cmo".to_string(),
            ControlPlane {
                kv: Arc::new(NoopKv),
                pubsub: Arc::new(NoopPubSub),
                scheduler: Arc::new(NoopScheduler),
            },
            manifests::AgentSpec::default(),
            HashMap::new(),
        );

        let write = executor
            .execute_tool(
                crate::knowledge::KNOWLEDGE_WRITE_TOOL,
                r#"{"path":"notes/plan.md","content":"remember the plan"}"#,
            )
            .await
            .expect("knowledge write");
        assert!(write.contains("wrote artifact"));

        let get = executor
            .execute_tool(
                crate::knowledge::KNOWLEDGE_GET_TOOL,
                r#"{"path":"notes/plan.md"}"#,
            )
            .await
            .expect("knowledge get");
        assert!(get.contains("[conic:wks:13:notes/plan.md]"));

        let search = executor
            .execute_tool(
                crate::knowledge::KNOWLEDGE_SEARCH_TOOL,
                r#"{"query":"plan"}"#,
            )
            .await
            .expect("knowledge search");
        assert!(search.contains("remember the plan"));

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
