// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use std::sync::Arc;

use super::mcp_registry::McpRegistry;
use crate::config::Config;
use crate::control::events::{SessionStepEvent, StepType};
use crate::control::ControlPlane;
use crate::control::ProtoKeyValueStoreExt;
use crate::core::context_budget::tool_result_preview;
use crate::core::executor::{
    AgentExecutor, ContextAssembler, ExecutionContext, LoopMessage, RegisteredMcpTool,
};
use crate::gateway::rpc::models;
use crate::gateway::rpc::{manifests, protobuf_value::value::Kind as ProtoValueKind};
use crate::knowledge::KvKnowledgeBook;
use crate::llm::ToolCall;
use crate::skills::registry::ToolRegistry;

/// Fully-assembled, ready-to-run environment for one agent session.
/// Build it from identity coordinates; it resolves everything else
/// (agent spec, history, LLM, tools, knowledge) from the control plane.
pub struct AgentRuntime {
    pub executor: AgentExecutor,
    pub context: ExecutionContext,
}

impl AgentRuntime {
    /// Resolve and assemble the runtime for `(ns, agent_id, session_id)`.
    pub async fn build(
        ns: &str,
        agent_id: &str,
        session_id: &str,
        cp: &ControlPlane,
        config: &Config,
        mcp_registry: &McpRegistry,
    ) -> Result<Self> {
        // 1. Fetch AgentSpec from KV
        let agent_key = crate::control::keys::agent(ns, agent_id);
        let agent = cp
            .kv
            .get_msg::<models::Agent>(&agent_key)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found in ns '{}'", agent_id, ns))?;
        let spec = agent
            .effective_spec
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' has no effective spec", agent_id))?;

        // 2. Load session history from KV
        let msg_prefix = crate::control::keys::session_message_prefix(ns, agent_id, session_id);
        let mut msg_keys = cp.kv.list_keys(&msg_prefix).await?;
        msg_keys.sort();

        let mut history = Vec::new();
        for key in msg_keys {
            if let Some(msg) = cp
                .kv
                .get_msg::<models::SessionMessage>(&key)
                .await
                .unwrap_or(None)
            {
                let role = match msg.role {
                    1 => "user",
                    2 => "assistant",
                    3 => "system",
                    _ => "user",
                };

                let mut tool_calls = None;
                let mut tool_results: Vec<LoopMessage> = Vec::new();

                if msg.role == 2 {
                    let step_prefix = crate::control::keys::session_message_step_prefix(
                        ns, agent_id, session_id, &msg.id,
                    );
                    let mut step_keys = cp.kv.list_keys(&step_prefix).await?;
                    step_keys.sort();

                    let mut collected_tool_calls = Vec::new();
                    for step_key in step_keys {
                        if let Some(step) = cp
                            .kv
                            .get_msg::<SessionStepEvent>(&step_key)
                            .await
                            .unwrap_or(None)
                        {
                            if step.step_type == StepType::Action as i32 {
                                let payload: serde_json::Value =
                                    serde_json::from_str(&step.payload_json)
                                        .unwrap_or(serde_json::Value::Null);
                                let tool_call_id = payload
                                    .get("tool_call_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                let input = payload
                                    .get("input")
                                    .cloned()
                                    .unwrap_or(serde_json::Value::Null);
                                collected_tool_calls.push(ToolCall {
                                    id: tool_call_id,
                                    name: step.name.clone(),
                                    arguments: serde_json::to_string(&input)
                                        .unwrap_or_else(|_| "null".to_string()),
                                });
                            } else if step.step_type == StepType::Observation as i32 {
                                if let Some(message) = tool_result_message_from_step(&step) {
                                    tool_results.push(message);
                                }
                            }
                        }
                    }

                    if !collected_tool_calls.is_empty() {
                        tool_calls = Some(collected_tool_calls);
                    }
                }

                history.push(LoopMessage {
                    role: role.to_string(),
                    content: msg.content,
                    tool_calls,
                    tool_call_id: None,
                });
                history.extend(tool_results);
            }
        }

        // 3. Resolve LLM from AgentSpec + Config
        let llm = crate::llm::resolver::resolve_llm(&spec, config).await?;

        // 4. Build tool registry (builtins + future MCP servers)
        let mut mcp_tools = std::collections::HashMap::new();
        let mut reg = ToolRegistry::new();
        crate::knowledge::register_tools(&mut reg);
        crate::native_tools::register_tools(&mut reg, &spec);
        let builtin_tool_names = builtin_tool_names();
        for mcp_ref in &spec.mcp_server_refs {
            let server = match mcp_registry.resolve_server(cp, mcp_ref, ns).await {
                Ok(server) => server,
                Err(err) => {
                    tracing::warn!(
                        namespace = %ns,
                        mcp_server = %mcp_ref,
                        error = %err,
                        "Failed to resolve MCP server; continuing without it"
                    );
                    continue;
                }
            };

            let mut server_config = server.config.clone();
            server_config.agent_name = Some(agent_id.to_string());

            let mut accepted_tools = Vec::new();
            for tool in visible_tools_for_agent(&server_config, &server.tools, &spec) {
                let qualified_tool_name = qualify_mcp_tool_name(&server_config, &tool.name);
                if qualified_tool_name.len() > 64 {
                    return Err(anyhow!(
                        "Qualified MCP tool name '{}' exceeds the 64-character limit for LLM tools",
                        qualified_tool_name
                    ));
                }
                if builtin_tool_names.contains(&qualified_tool_name.as_str()) {
                    tracing::warn!(
                        tool = %qualified_tool_name,
                        server = %server.config.server_name,
                        namespace = %ns,
                        "Skipping MCP tool because it would shadow a builtin tool"
                    );
                    continue;
                }

                if mcp_tools
                    .insert(
                        qualified_tool_name.clone(),
                        RegisteredMcpTool {
                            config: server_config.clone(),
                            remote_name: tool.name.clone(),
                        },
                    )
                    .is_some()
                {
                    return Err(anyhow!(
                        "Duplicate MCP tool '{}' registered in namespace '{}'",
                        qualified_tool_name,
                        ns
                    ));
                }
                let mut qualified_tool = tool.clone();
                qualified_tool.name = qualified_tool_name;
                accepted_tools.push(qualified_tool);
            }

            if accepted_tools.is_empty() && !server.tools.is_empty() {
                tracing::warn!(
                    namespace = %ns,
                    mcp_server = %server.config.server_name,
                    "Resolved MCP server but skipped all of its tools"
                );
            }
            reg.register_mcp_tools(&server_config.server_name, accepted_tools);
        }
        let registry = Arc::new(tokio::sync::RwLock::new(reg));

        // 5. Build executor
        let executor = AgentExecutor::new(
            llm,
            ContextAssembler::new("."),
            registry,
            Arc::new(config.clone()),
            Arc::new(KvKnowledgeBook::new(cp.kv.clone())),
            ns.to_string(),
            agent_id.to_string(),
            cp.clone(),
            spec.clone(),
            mcp_tools,
        );

        Ok(Self {
            executor,
            context: ExecutionContext::with_history(agent_id.to_string(), history),
        })
    }
}

fn visible_tools_for_agent(
    config: &crate::connectors::mcp::McpConnectionConfig,
    tools: &[crate::connectors::mcp::McpTool],
    spec: &manifests::AgentSpec,
) -> Vec<crate::connectors::mcp::McpTool> {
    let binding_name = config
        .binding_name
        .as_deref()
        .unwrap_or(&config.server_name);
    if binding_name != "talon-ops" {
        return tools.to_vec();
    }

    tools
        .iter()
        .filter(|tool| {
            if is_schedule_tool_name(&tool.name) {
                return match tool.name.as_str() {
                    "list_schedules" | "get_schedule" => {
                        has_capability_action(spec, "schedules", "inspect")
                    }
                    "create_schedule" => has_capability_action(spec, "schedules", "create"),
                    "update_schedule" => has_capability_action(spec, "schedules", "update"),
                    "delete_schedule" => has_capability_action(spec, "schedules", "delete"),
                    _ => true,
                };
            }

            if is_session_tool_name(&tool.name) {
                return match tool.name.as_str() {
                    "list_sessions" | "get_session" | "list_recent_steps" => {
                        has_capability_action(spec, "sessions", "inspect")
                    }
                    _ => true,
                };
            }

            true
        })
        .cloned()
        .collect()
}

fn is_schedule_tool_name(name: &str) -> bool {
    matches!(
        name,
        "list_schedules"
            | "get_schedule"
            | "create_schedule"
            | "update_schedule"
            | "delete_schedule"
    )
}

fn is_session_tool_name(name: &str) -> bool {
    matches!(name, "list_sessions" | "get_session" | "list_recent_steps")
}

fn has_capability_action(spec: &manifests::AgentSpec, capability: &str, action: &str) -> bool {
    spec.capabilities
        .get(capability)
        .map(|actions| {
            actions.values.iter().any(|value| {
                matches!(
                    value.kind.as_ref(),
                    Some(ProtoValueKind::StringValue(current)) if current == action
                )
            })
        })
        .unwrap_or(false)
}

fn builtin_tool_names() -> &'static [&'static str] {
    &[
        crate::knowledge::KNOWLEDGE_WRITE_TOOL,
        crate::knowledge::KNOWLEDGE_SEARCH_TOOL,
        crate::knowledge::KNOWLEDGE_GET_TOOL,
        crate::knowledge::KNOWLEDGE_LIST_TOOL,
        crate::native_tools::CREATE_SCHEDULE_TOOL,
        crate::native_tools::GET_SCHEDULE_TOOL,
        crate::native_tools::LIST_SCHEDULES_TOOL,
        crate::native_tools::UPDATE_SCHEDULE_TOOL,
        crate::native_tools::DELETE_SCHEDULE_TOOL,
    ]
}

fn qualify_mcp_tool_name(
    config: &crate::connectors::mcp::McpConnectionConfig,
    tool_name: &str,
) -> String {
    let prefix = config
        .binding_name
        .as_deref()
        .unwrap_or(&config.server_name);
    format!(
        "mcp_{}_{}",
        sanitize_tool_name_component(prefix),
        sanitize_tool_name_component(tool_name)
    )
}

fn sanitize_tool_name_component(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    let mut last_was_underscore = false;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch);
            last_was_underscore = false;
        } else if !last_was_underscore {
            sanitized.push('_');
            last_was_underscore = true;
        }
    }

    sanitized.trim_matches('_').to_string()
}

fn tool_result_message_from_step(step: &SessionStepEvent) -> Option<LoopMessage> {
    let payload: serde_json::Value =
        serde_json::from_str(&step.payload_json).unwrap_or(serde_json::Value::Null);
    let tool_call_id = payload.get("tool_call_id").and_then(|v| v.as_str())?;
    let output = payload
        .get("output_preview")
        .and_then(|v| v.as_str())
        .or_else(|| payload.get("output").and_then(|v| v.as_str()))
        .map(tool_result_preview)
        .unwrap_or_else(|| tool_result_preview(&step.content));
    Some(LoopMessage {
        role: "tool".to_string(),
        content: output,
        tool_calls: None,
        tool_call_id: Some(tool_call_id.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        builtin_tool_names, has_capability_action, qualify_mcp_tool_name,
        tool_result_message_from_step, visible_tools_for_agent, AgentRuntime,
    };
    use crate::config::{proto, Config, ProviderConfig, Secret};
    use crate::connectors::mcp::McpConnectionConfig;
    use crate::control::{
        events::{SessionStepEvent, StepType},
        scheduler::NoopSchedulerBackend,
        ControlPlane, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::{manifests, models, protobuf_value};
    use futures::stream;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockKvStore {
        data: Mutex<HashMap<String, Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.data.lock().await.get(key).cloned())
        }

        async fn set(&self, key: &str, value: &[u8]) -> anyhow::Result<()> {
            self.data
                .lock()
                .await
                .insert(key.to_string(), value.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            key: &str,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let full_key = key.to_string();
            let current = data.get(&full_key).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(full_key, value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, key: &str) -> anyhow::Result<()> {
            self.data.lock().await.remove(key);
            Ok(())
        }

        async fn list_keys(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|key| key.starts_with(prefix).then(|| key.clone()))
                .collect::<Vec<_>>();
            keys.sort();
            Ok(keys)
        }
    }

    #[derive(Default)]
    struct MockPubSub;

    #[async_trait::async_trait]
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

    fn config(server_name: &str, binding_name: Option<&str>) -> McpConnectionConfig {
        McpConnectionConfig {
            server_name: server_name.to_string(),
            server_ref: server_name.to_string(),
            transport: "stdio".to_string(),
            target: "test".to_string(),
            args: Vec::new(),
            headers: HashMap::new(),
            disabled: false,
            namespace: None,
            binding_name: binding_name.map(str::to_string),
            agent_name: None,
            auth_broker: None,
        }
    }

    fn runtime_config() -> Config {
        Config {
            providers: HashMap::from([(
                "novita".to_string(),
                ProviderConfig {
                    config: Some(proto::llm_provider_config::Config::OpenaiCompatible(
                        proto::GenericConfig {
                            name: "novita".to_string(),
                            base_url: "http://127.0.0.1:1".to_string(),
                            model: "test-model".to_string(),
                            api_key: Some(Secret {
                                source: Some(proto::secret::Source::Plain("test-key".to_string())),
                            }),
                        },
                    )),
                },
            )]),
            default_provider: "novita".to_string(),
            ..Config::default()
        }
    }

    fn control_plane(kv: Arc<MockKvStore>) -> ControlPlane {
        ControlPlane {
            kv,
            pubsub: Arc::new(MockPubSub),
            scheduler: Arc::new(NoopSchedulerBackend),
        }
    }

    #[test]
    fn qualify_mcp_tool_name_uses_binding_name_when_present() {
        let qualified = qualify_mcp_tool_name(
            &config("github", Some("workspace-gh")),
            "search_repositories",
        );

        assert_eq!(qualified, "mcp_workspace_gh_search_repositories");
    }

    #[test]
    fn qualify_mcp_tool_name_falls_back_to_server_name() {
        let qualified = qualify_mcp_tool_name(&config("github-enterprise", None), "get-issue");

        assert_eq!(qualified, "mcp_github_enterprise_get_issue");
    }

    #[test]
    fn sanitize_tool_name_component_collapses_repeated_separators() {
        let qualified = qualify_mcp_tool_name(&config("workspace--gh", None), "get---issue");

        assert_eq!(qualified, "mcp_workspace_gh_get_issue");
    }

    #[test]
    fn tool_result_message_prefers_output_preview_when_present() {
        let step = SessionStepEvent {
            session_id: "session-1".to_string(),
            step_type: StepType::Observation as i32,
            content: "preview".to_string(),
            timestamp: 0,
            agent: "cmo".to_string(),
            ns: "conic:wks:13".to_string(),
            message_id: "message-1".to_string(),
            name: "mcp_github_get_file_contents".to_string(),
            payload_json: serde_json::json!({
                "tool_call_id": "tool-1",
                "output_preview": "small preview",
                "output": format!("{{\"payload\":\"{}\"}}", "x".repeat(10_000)),
            })
            .to_string(),
        };

        let message = tool_result_message_from_step(&step).unwrap();

        assert_eq!(message.tool_call_id.as_deref(), Some("tool-1"));
        assert_eq!(message.content, "small preview");
    }

    #[test]
    fn tool_result_message_compacts_legacy_raw_output() {
        let raw_output = format!(
            "{{\"payload\":\"{}\",\"items\":[\"{}\",\"{}\"]}}",
            "x".repeat(20_000),
            "y".repeat(8_000),
            "z".repeat(8_000)
        );
        let step = SessionStepEvent {
            session_id: "session-1".to_string(),
            step_type: StepType::Observation as i32,
            content: raw_output.clone(),
            timestamp: 0,
            agent: "cmo".to_string(),
            ns: "conic:wks:13".to_string(),
            message_id: "message-1".to_string(),
            name: "mcp_github_search_code".to_string(),
            payload_json: serde_json::json!({
                "tool_call_id": "tool-1",
                "output": raw_output,
            })
            .to_string(),
        };

        let message = tool_result_message_from_step(&step).unwrap();

        assert!(message.content.len() < 10_000);
        assert!(
            message.content.contains("chars omitted") || message.content.contains("_truncated")
        );
    }

    fn spec_with_capabilities(capabilities: &[(&str, &[&str])]) -> manifests::AgentSpec {
        manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: String::new(),
            mcp_server_refs: Vec::new(),
            capabilities: capabilities
                .iter()
                .map(|(name, actions)| {
                    (
                        (*name).to_string(),
                        protobuf_value::ListValue {
                            values: actions
                                .iter()
                                .map(|action| protobuf_value::Value {
                                    kind: Some(protobuf_value::value::Kind::StringValue(
                                        (*action).to_string(),
                                    )),
                                })
                                .collect(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn tool(name: &str) -> crate::connectors::mcp::McpTool {
        crate::connectors::mcp::McpTool {
            name: name.to_string(),
            description: String::new(),
            input_schema: serde_json::json!({"type":"object"}),
        }
    }

    #[test]
    fn visible_tools_for_non_talon_ops_binding_returns_all_tools() {
        let tools = vec![tool("list_schedules"), tool("custom_tool")];
        let visible = visible_tools_for_agent(
            &config("github", Some("github")),
            &tools,
            &spec_with_capabilities(&[]),
        );

        assert_eq!(visible.len(), 2);
    }

    #[test]
    fn visible_tools_for_talon_ops_binding_filters_by_capabilities() {
        let tools = vec![
            tool("list_schedules"),
            tool("create_schedule"),
            tool("delete_schedule"),
            tool("list_sessions"),
            tool("custom_tool"),
        ];
        let spec = spec_with_capabilities(&[
            ("schedules", &["inspect", "create"]),
            ("sessions", &["inspect"]),
        ]);

        let visible =
            visible_tools_for_agent(&config("talon-ops", Some("talon-ops")), &tools, &spec);
        let names = visible
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();

        assert!(names.contains(&"list_schedules".to_string()));
        assert!(names.contains(&"create_schedule".to_string()));
        assert!(names.contains(&"list_sessions".to_string()));
        assert!(names.contains(&"custom_tool".to_string()));
        assert!(!names.contains(&"delete_schedule".to_string()));
    }

    #[test]
    fn has_capability_action_matches_only_present_actions() {
        let spec = spec_with_capabilities(&[("schedules", &["inspect", "create"])]);

        assert!(has_capability_action(&spec, "schedules", "inspect"));
        assert!(!has_capability_action(&spec, "schedules", "delete"));
        assert!(!has_capability_action(&spec, "sessions", "inspect"));
    }

    #[test]
    fn tool_result_message_requires_tool_call_id() {
        let step = SessionStepEvent {
            session_id: "session-1".to_string(),
            step_type: StepType::Observation as i32,
            content: "preview".to_string(),
            timestamp: 0,
            agent: "cmo".to_string(),
            ns: "conic:wks:13".to_string(),
            message_id: "message-1".to_string(),
            name: "mcp_github_get_file_contents".to_string(),
            payload_json: serde_json::json!({
                "output_preview": "small preview",
            })
            .to_string(),
        };

        assert!(tool_result_message_from_step(&step).is_none());
    }

    #[test]
    fn tool_result_message_falls_back_to_step_content_when_payload_has_no_output() {
        let step = SessionStepEvent {
            session_id: "session-1".to_string(),
            step_type: StepType::Observation as i32,
            content: "fallback output".to_string(),
            timestamp: 0,
            agent: "cmo".to_string(),
            ns: "conic:wks:13".to_string(),
            message_id: "message-1".to_string(),
            name: "mcp_demo_tool".to_string(),
            payload_json: serde_json::json!({
                "tool_call_id": "tool-1"
            })
            .to_string(),
        };

        let message = tool_result_message_from_step(&step).unwrap();
        assert_eq!(message.content, "fallback output");
    }

    #[test]
    fn builtin_tool_names_contains_expected_schedule_and_knowledge_tools() {
        let names = builtin_tool_names();
        assert!(names.contains(&crate::knowledge::KNOWLEDGE_GET_TOOL));
        assert!(names.contains(&crate::knowledge::KNOWLEDGE_LIST_TOOL));
        assert!(names.contains(&crate::native_tools::CREATE_SCHEDULE_TOOL));
        assert!(names.contains(&crate::native_tools::DELETE_SCHEDULE_TOOL));
    }

    #[tokio::test]
    async fn agent_runtime_build_errors_for_missing_agent_or_effective_spec() {
        let kv = Arc::new(MockKvStore::default());
        let cp = control_plane(kv.clone());
        let config = runtime_config();
        let registry = crate::worker::mcp_registry::McpRegistry::new();

        let missing =
            match AgentRuntime::build("conic", "missing", "session-1", &cp, &config, &registry)
                .await
            {
                Ok(_) => panic!("expected missing agent error"),
                Err(err) => err,
            };
        assert!(missing.to_string().contains("Agent 'missing' not found"));

        kv.set_msg(
            &crate::control::keys::agent("conic", "writer"),
            &models::Agent {
                name: "writer".to_string(),
                ns: "conic".to_string(),
                definition: None,
                effective_spec: None,
                template_deps: Vec::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();

        let no_spec = match AgentRuntime::build(
            "conic",
            "writer",
            "session-1",
            &cp,
            &config,
            &registry,
        )
        .await
        {
            Ok(_) => panic!("expected missing effective spec error"),
            Err(err) => err,
        };
        assert!(no_spec.to_string().contains("has no effective spec"));
    }

    #[tokio::test]
    async fn agent_runtime_build_assembles_history_and_skips_bad_entries() {
        let kv = Arc::new(MockKvStore::default());
        let cp = control_plane(kv.clone());
        let config = runtime_config();
        let registry = crate::worker::mcp_registry::McpRegistry::new();
        let spec = manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: "assist".to_string(),
            mcp_server_refs: vec!["missing-server".to_string()],
            capabilities: HashMap::new(),
        };

        kv.set_msg(
            &crate::control::keys::agent("conic", "writer"),
            &models::Agent {
                name: "writer".to_string(),
                ns: "conic".to_string(),
                definition: None,
                effective_spec: Some(spec),
                template_deps: Vec::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session_message("conic", "writer", "session-1", "msg-1"),
            &models::SessionMessage {
                id: "msg-1".to_string(),
                role: 1,
                content: "hello".to_string(),
                created_at: 10,
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session_message("conic", "writer", "session-1", "msg-2"),
            &models::SessionMessage {
                id: "msg-2".to_string(),
                role: 2,
                content: "assistant reply".to_string(),
                created_at: 20,
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set(
            &crate::control::keys::session_message("conic", "writer", "session-1", "msg-2/sub"),
            b"nested",
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session_message_step(
                "conic",
                "writer",
                "session-1",
                "msg-2",
                "step-1",
            ),
            &SessionStepEvent {
                session_id: "session-1".to_string(),
                step_type: StepType::Action as i32,
                content: String::new(),
                timestamp: 21,
                agent: "writer".to_string(),
                ns: "conic".to_string(),
                message_id: "msg-2".to_string(),
                name: "search".to_string(),
                payload_json: serde_json::json!({
                    "tool_call_id": "call-1",
                    "input": { "q": "talon" }
                })
                .to_string(),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session_message_step(
                "conic",
                "writer",
                "session-1",
                "msg-2",
                "step-2",
            ),
            &SessionStepEvent {
                session_id: "session-1".to_string(),
                step_type: StepType::Observation as i32,
                content: "tool output".to_string(),
                timestamp: 22,
                agent: "writer".to_string(),
                ns: "conic".to_string(),
                message_id: "msg-2".to_string(),
                name: "search".to_string(),
                payload_json: serde_json::json!({
                    "tool_call_id": "call-1",
                    "output_preview": "tool preview"
                })
                .to_string(),
            },
        )
        .await
        .unwrap();

        let runtime = AgentRuntime::build("conic", "writer", "session-1", &cp, &config, &registry)
            .await
            .unwrap();

        assert_eq!(runtime.context.agent_id, "writer");
        assert_eq!(runtime.context.history.len(), 3);
        assert_eq!(runtime.context.history[0].role, "user");
        assert_eq!(runtime.context.history[0].content, "hello");
        assert_eq!(runtime.context.history[1].role, "assistant");
        assert_eq!(
            runtime.context.history[1].tool_calls.as_ref().unwrap()[0].name,
            "search"
        );
        assert_eq!(runtime.context.history[2].role, "tool");
        assert_eq!(runtime.context.history[2].content, "tool preview");
    }
}
