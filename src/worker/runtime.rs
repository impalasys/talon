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
use crate::knowledge::KvKnowledgeBook;
use crate::llm::ToolCall;
use crate::skills::registry::ToolRegistry;
use crate::gateway::rpc::{manifests, protobuf_value::value::Kind as ProtoValueKind};

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
        let agent_key = crate::control::keys::agent(agent_id);
        let agent = cp
            .kv
            .get_msg::<models::Agent>(ns, &agent_key)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found in ns '{}'", agent_id, ns))?;
        let spec = agent
            .effective_spec
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' has no effective spec", agent_id))?;

        // 2. Load session history from KV
        let msg_prefix = crate::control::keys::session_message_prefix(agent_id, session_id);
        let mut msg_keys = cp.kv.list_keys(ns, &msg_prefix).await?;
        msg_keys.sort();

        let mut history = Vec::new();
        for key in msg_keys {
            if key.strip_prefix(&msg_prefix).unwrap_or(&key).contains('/') {
                continue;
            }
            if let Some(msg) = cp
                .kv
                .get_msg::<models::SessionMessage>(ns, &key)
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
                        agent_id, session_id, &msg.id,
                    );
                    let mut step_keys = cp.kv.list_keys(ns, &step_prefix).await?;
                    step_keys.sort();

                    let mut collected_tool_calls = Vec::new();
                    for step_key in step_keys {
                        if let Some(step) = cp
                            .kv
                            .get_msg::<SessionStepEvent>(ns, &step_key)
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

    tools.iter()
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
        "list_schedules" | "get_schedule" | "create_schedule" | "update_schedule"
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
    use super::{qualify_mcp_tool_name, tool_result_message_from_step};
    use crate::connectors::mcp::McpConnectionConfig;
    use crate::control::events::{SessionStepEvent, StepType};
    use std::collections::HashMap;

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
}
