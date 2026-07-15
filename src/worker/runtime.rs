// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use std::sync::Arc;

use super::mcp_registry::McpRegistry;
use crate::control::config::Config;
use crate::control::ProtoKeyValueStoreExt;
use crate::control::{ControlPlane, Order};
use crate::gateway::rpc::data_proto;
use crate::gateway::rpc::resources_proto;
use crate::gateway::rpc::{manifests, protobuf_value::value::Kind as ProtoValueKind};
use crate::harness::executor::{
    session_message_to_loop_messages, AgentExecutor, ContextAssembler, ExecutionContext,
    LoopMessage, RegisteredMcpTool,
};
use crate::harness::sessions::{
    SESSION_LABEL_PROJECTION_STATE, SESSION_PROJECTION_STATE_COMMITTED,
};
use crate::harness::skills::registry::ToolRegistry;
use prost::Message;

/// Fully-assembled, ready-to-run environment for one agent session.
/// Build it from identity coordinates; it resolves everything else
/// (agent spec, history, LLM, and tools) from the control plane.
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
        let store = crate::control::resources::ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
        let agent = store
            .get_agent(ns, agent_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found in ns '{}'", agent_id, ns))?;
        Self::build_from_agent(ns, agent_id, session_id, agent, cp, config, mcp_registry).await
    }

    pub async fn build_from_agent(
        ns: &str,
        agent_id: &str,
        session_id: &str,
        agent: resources_proto::Agent,
        cp: &ControlPlane,
        config: &Config,
        mcp_registry: &McpRegistry,
    ) -> Result<Self> {
        let mut spec = agent
            .spec
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' has no spec", agent_id))?;
        let session = cp
            .kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(
                ns, agent_id, session_id,
            ))
            .await?;
        let mut is_delegated_task_session = session.as_ref().is_some_and(|session| {
            session
                .labels
                .get(crate::control::delegation::LABEL_TASK_ROLE)
                .map(String::as_str)
                == Some("delegate")
        });
        let allow_channel_reply_tools = session
            .as_ref()
            .map(|session| {
                session
                    .labels
                    .contains_key(crate::gateway::rpc::channels::LABEL_CHANNEL)
                    && session
                        .labels
                        .get(crate::gateway::rpc::channels::LABEL_CHANNEL_REPLY_MODE)
                        .map(|mode| mode != "none" && mode != "hold_for_review" && mode != "review")
                        .unwrap_or(true)
            })
            .unwrap_or(false);

        // 2. Load session history from KV
        let msg_prefix = crate::control::keys::session_message_prefix(ns, agent_id, session_id);
        let msg_entries = cp.kv.list_entries(&msg_prefix, Order::Asc).await?;

        let mut history = Vec::new();
        for (_, value) in msg_entries {
            if let Ok(msg) = data_proto::SessionMessage::decode(value.as_slice()) {
                if msg
                    .labels
                    .get(crate::control::delegation::LABEL_TASK_ROLE)
                    .map(String::as_str)
                    == Some("delegate")
                {
                    is_delegated_task_session = true;
                }
                if msg.role == data_proto::MessageRole::RoleAssistant as i32
                    && !assistant_projection_is_replayable(&msg)
                {
                    continue;
                }
                history.extend(session_message_to_loop_messages(&msg, cp.objects.as_ref()).await?);
            }
        }
        if is_delegated_task_session {
            add_capability_action(&mut spec, "tasks", "update");
        }
        if let Some(goal_context) =
            crate::harness::native_tools::active_goals_context(cp, ns, agent_id, session_id).await?
        {
            history.insert(0, LoopMessage::text("system", goal_context));
        }

        // 3. Resolve LLM from AgentSpec + Config
        let llm = crate::harness::llm::resolver::resolve_llm(&spec, config).await?;

        // 4. Build tool registry (builtins + future MCP servers)
        let mut mcp_tools = std::collections::HashMap::new();
        let mut reg = ToolRegistry::new();
        crate::harness::native_tools::register_tools(&mut reg, &spec);
        if allow_channel_reply_tools {
            crate::harness::native_tools::register_channel_tools(&mut reg);
        }
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

            let mut server_config = config_for_agent_namespace(&server.config, ns);
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
        let executor = AgentExecutor::new_with_session(
            llm.provider,
            llm.provider_key,
            llm.model,
            ContextAssembler::new("."),
            registry,
            Arc::new(config.clone()),
            ns.to_string(),
            agent_id.to_string(),
            session_id.to_string(),
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
    config: &crate::harness::mcp::McpConnectionConfig,
    tools: &[crate::harness::mcp::McpTool],
    spec: &manifests::AgentSpec,
) -> Vec<crate::harness::mcp::McpTool> {
    let mcp_server_name = config
        .mcp_server_name
        .as_deref()
        .unwrap_or(&config.server_name);
    if mcp_server_name != "talon-ops" {
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
                    "list_sessions" | "get_session" => {
                        has_capability_action(spec, "sessions", "inspect")
                    }
                    _ => true,
                };
            }

            if is_goal_tool_name(&tool.name) {
                return match tool.name.as_str() {
                    "list_goals" | "get_goal" => has_capability_action(spec, "goals", "inspect"),
                    "create_goal" => has_capability_action(spec, "goals", "create"),
                    "update_goal" | "complete_goal" | "block_goal" => {
                        has_capability_action(spec, "goals", "update")
                    }
                    _ => true,
                };
            }

            true
        })
        .cloned()
        .collect()
}

fn config_for_agent_namespace(
    config: &crate::harness::mcp::McpConnectionConfig,
    namespace: &str,
) -> crate::harness::mcp::McpConnectionConfig {
    let mut config = config.clone();
    config.namespace = Some(namespace.to_string());
    config
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
    matches!(name, "list_sessions" | "get_session")
}

fn is_goal_tool_name(name: &str) -> bool {
    matches!(
        name,
        "list_goals" | "get_goal" | "create_goal" | "update_goal" | "complete_goal" | "block_goal"
    )
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

fn add_capability_action(spec: &mut manifests::AgentSpec, capability: &str, action: &str) {
    let actions = spec.capabilities.entry(capability.to_string()).or_default();
    if actions.values.iter().any(|value| {
        matches!(
            value.kind.as_ref(),
            Some(ProtoValueKind::StringValue(current)) if current == action
        )
    }) {
        return;
    }
    actions
        .values
        .push(crate::gateway::rpc::protobuf_value::Value {
            kind: Some(ProtoValueKind::StringValue(action.to_string())),
        });
}

fn builtin_tool_names() -> &'static [&'static str] {
    &[
        crate::harness::native_tools::CREATE_SCHEDULE_TOOL,
        crate::harness::native_tools::GET_SCHEDULE_TOOL,
        crate::harness::native_tools::LIST_SCHEDULES_TOOL,
        crate::harness::native_tools::UPDATE_SCHEDULE_TOOL,
        crate::harness::native_tools::DELETE_SCHEDULE_TOOL,
        crate::harness::native_tools::CREATE_GOAL_TOOL,
        crate::harness::native_tools::DELEGATE_TASK_TOOL,
        crate::harness::native_tools::ASK_AGENT_TOOL,
        crate::harness::native_tools::UPDATE_TASK_TOOL,
        crate::harness::native_tools::GET_GOAL_TOOL,
        crate::harness::native_tools::LIST_GOALS_TOOL,
        crate::harness::native_tools::UPDATE_GOAL_TOOL,
        crate::harness::native_tools::COMPLETE_GOAL_TOOL,
        crate::harness::native_tools::BLOCK_GOAL_TOOL,
        crate::harness::native_tools::CHANNEL_PUBLISH_TOOL,
        crate::harness::native_tools::CHANNEL_SKIP_REPLY_TOOL,
        crate::harness::native_tools::READ_SESSION_MESSAGES_TOOL,
        crate::harness::native_tools::CREATE_ARTIFACT_TOOL,
        crate::harness::native_tools::UPDATE_ARTIFACT_TOOL,
        crate::harness::native_tools::READ_ARTIFACT_TOOL,
        crate::harness::native_tools::GET_ARTIFACT_METADATA_TOOL,
        crate::harness::native_tools::GRANT_ARTIFACT_TOOL,
        crate::harness::native_tools::FETCH_URL_TOOL,
        crate::harness::native_tools::WEB_SEARCH_TOOL,
        crate::harness::native_tools::SEARCH_MEMORY_TOOL,
        crate::harness::native_tools::READ_MEMORY_TOOL,
        crate::harness::native_tools::LIST_MEMORY_TOOL,
        crate::harness::native_tools::CREATE_MEMORY_TOOL,
        crate::harness::native_tools::UPDATE_MEMORY_TOOL,
    ]
}

fn assistant_projection_is_replayable(message: &data_proto::SessionMessage) -> bool {
    match message
        .labels
        .get(SESSION_LABEL_PROJECTION_STATE)
        .map(String::as_str)
    {
        None | Some(SESSION_PROJECTION_STATE_COMMITTED) => true,
        Some(crate::harness::sessions::SESSION_PROJECTION_STATE_FAILED) => true,
        Some(_) => false,
    }
}

fn qualify_mcp_tool_name(
    config: &crate::harness::mcp::McpConnectionConfig,
    tool_name: &str,
) -> String {
    let prefix = config
        .mcp_server_name
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

#[cfg(test)]
mod tests {
    use super::{
        builtin_tool_names, config_for_agent_namespace, has_capability_action,
        qualify_mcp_tool_name, visible_tools_for_agent, AgentRuntime,
    };
    use crate::control::config::{proto, Config, ProviderConfig, Secret};
    use crate::control::{
        keys::{ResourceKey, ResourceList},
        ControlPlane, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::{data_proto, manifests, protobuf_value, resources_proto};
    use crate::harness::llm::{image_data_part, text_part};
    use crate::harness::mcp::McpConnectionConfig;
    use futures::stream;
    use prost::Message;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockKvStore {
        data: Mutex<HashMap<ResourceKey, Vec<u8>>>,
    }

    #[async_trait::async_trait]
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

        async fn list_keys(
            &self,
            list: &ResourceList,
            _order: crate::control::Order,
        ) -> anyhow::Result<Vec<ResourceKey>> {
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

    fn config(server_name: &str, mcp_server_name: Option<&str>) -> McpConnectionConfig {
        McpConnectionConfig {
            server_name: server_name.to_string(),
            server_ref: server_name.to_string(),
            transport: "stdio".to_string(),
            target: "test".to_string(),
            args: Vec::new(),
            headers: HashMap::new(),
            disabled: false,
            namespace: None,
            mcp_server_name: mcp_server_name.map(str::to_string),
            agent_name: None,
            jwt_issuer: None,
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

    async fn put_agent_resource(
        kv: Arc<MockKvStore>,
        namespace: &str,
        name: &str,
        spec: Option<resources_proto::AgentSpec>,
    ) {
        let agent = resources_proto::Agent {
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
            spec,
            status: Some(resources_proto::AgentStatus {
                observed_generation: 0,
                phase: String::new(),
                conditions: Vec::new(),
                last_session_id: None,
            }),
        };
        if agent.spec.is_none() {
            kv.set(
                &crate::control::keys::agent(namespace, name),
                &agent.encode_to_vec(),
            )
            .await
            .unwrap();
            return;
        }

        let store = crate::control::resources::ResourceStore::new(kv, Arc::new(MockPubSub));
        store
            .upsert(
                namespace,
                resources_proto::Resource {
                    api_version: "talon.impalasys.com/v1".to_string(),
                    kind: "Agent".to_string(),
                    metadata: agent.metadata,
                    spec: agent.spec.map(|spec| resources_proto::ResourceSpec {
                        kind: Some(resources_proto::resource_spec::Kind::Agent(spec)),
                    }),
                    status: Some(resources_proto::ResourceStatus {
                        kind: Some(resources_proto::resource_status::Kind::Agent(
                            agent.status.unwrap_or_default(),
                        )),
                    }),
                },
            )
            .await
            .unwrap();
    }

    #[test]
    fn qualify_mcp_tool_name_uses_mcp_server_name_when_present() {
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
    fn config_for_agent_namespace_uses_calling_namespace_for_inherited_mcp_server() {
        let inherited = McpConnectionConfig {
            namespace: Some("Tenant:conic:Customers".to_string()),
            mcp_server_name: Some("conic".to_string()),
            ..config("conic", Some("conic"))
        };

        let scoped = config_for_agent_namespace(&inherited, "Tenant:conic:Customers:42");

        assert_eq!(
            scoped.namespace.as_deref(),
            Some("Tenant:conic:Customers:42")
        );
        assert_eq!(scoped.mcp_server_name.as_deref(), Some("conic"));
    }

    fn spec_with_capabilities(capabilities: &[(&str, &[&str])]) -> manifests::AgentSpec {
        manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: String::new(),
            post_history_prompt: String::new(),
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
            a2a: None,
            runtime: None,
        }
    }

    fn tool(name: &str) -> crate::harness::mcp::McpTool {
        crate::harness::mcp::McpTool {
            name: name.to_string(),
            description: String::new(),
            input_schema: serde_json::json!({"type":"object"}),
        }
    }

    #[test]
    fn visible_tools_for_non_talon_ops_server_returns_all_tools() {
        let tools = vec![tool("list_schedules"), tool("custom_tool")];
        let visible = visible_tools_for_agent(
            &config("github", Some("github")),
            &tools,
            &spec_with_capabilities(&[]),
        );

        assert_eq!(visible.len(), 2);
    }

    #[test]
    fn visible_tools_for_talon_ops_server_filters_by_capabilities() {
        let tools = vec![
            tool("list_schedules"),
            tool("create_schedule"),
            tool("delete_schedule"),
            tool("list_sessions"),
            tool("list_goals"),
            tool("create_goal"),
            tool("custom_tool"),
        ];
        let spec = spec_with_capabilities(&[
            ("schedules", &["inspect", "create"]),
            ("sessions", &["inspect"]),
            ("goals", &["inspect"]),
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
        assert!(names.contains(&"list_goals".to_string()));
        assert!(names.contains(&"custom_tool".to_string()));
        assert!(!names.contains(&"delete_schedule".to_string()));
        assert!(!names.contains(&"create_goal".to_string()));
    }

    #[test]
    fn has_capability_action_matches_only_present_actions() {
        let spec = spec_with_capabilities(&[("schedules", &["inspect", "create"])]);

        assert!(has_capability_action(&spec, "schedules", "inspect"));
        assert!(!has_capability_action(&spec, "schedules", "delete"));
        assert!(!has_capability_action(&spec, "sessions", "inspect"));
    }

    #[test]
    fn builtin_tool_names_contains_expected_native_tools() {
        let names = builtin_tool_names();
        assert!(names.contains(&crate::harness::native_tools::CREATE_SCHEDULE_TOOL));
        assert!(names.contains(&crate::harness::native_tools::DELETE_SCHEDULE_TOOL));
        assert!(names.contains(&crate::harness::native_tools::CREATE_GOAL_TOOL));
        assert!(names.contains(&crate::harness::native_tools::LIST_GOALS_TOOL));
        assert!(names.contains(&crate::harness::native_tools::CREATE_ARTIFACT_TOOL));
        assert!(names.contains(&crate::harness::native_tools::UPDATE_ARTIFACT_TOOL));
        assert!(names.contains(&crate::harness::native_tools::DELEGATE_TASK_TOOL));
        assert!(names.contains(&crate::harness::native_tools::READ_SESSION_MESSAGES_TOOL));
        assert!(names.contains(&crate::harness::native_tools::SEARCH_MEMORY_TOOL));
        assert!(names.contains(&crate::harness::native_tools::READ_MEMORY_TOOL));
    }

    #[tokio::test]
    async fn agent_runtime_build_errors_for_missing_agent_or_spec() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
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

        put_agent_resource(kv.clone(), "conic", "writer", None).await;

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
        assert!(no_spec
            .to_string()
            .contains("Agent resource is missing typed Agent spec"));
    }

    #[tokio::test]
    async fn channel_reply_mode_none_withholds_channel_reply_tools() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        let config = runtime_config();
        let registry = crate::worker::mcp_registry::McpRegistry::new();
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

        put_agent_resource(kv.clone(), "conic", "writer", Some(spec)).await;

        let mut reply_labels = HashMap::new();
        reply_labels.insert(
            crate::gateway::rpc::channels::LABEL_CHANNEL.to_string(),
            "incident-room".to_string(),
        );
        kv.set_msg(
            &crate::control::keys::session("conic", "writer", "reply-session"),
            &data_proto::Session {
                id: "reply-session".to_string(),
                agent: "writer".to_string(),
                ns: "conic".to_string(),
                status: "active".to_string(),
                labels: reply_labels,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let mut no_reply_labels = HashMap::new();
        no_reply_labels.insert(
            crate::gateway::rpc::channels::LABEL_CHANNEL.to_string(),
            "incident-room".to_string(),
        );
        no_reply_labels.insert(
            crate::gateway::rpc::channels::LABEL_CHANNEL_REPLY_MODE.to_string(),
            "none".to_string(),
        );
        kv.set_msg(
            &crate::control::keys::session("conic", "writer", "no-reply-session"),
            &data_proto::Session {
                id: "no-reply-session".to_string(),
                agent: "writer".to_string(),
                ns: "conic".to_string(),
                status: "active".to_string(),
                labels: no_reply_labels,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let reply_runtime =
            AgentRuntime::build("conic", "writer", "reply-session", &cp, &config, &registry)
                .await
                .unwrap();
        let reply_registry = reply_runtime.executor.registry.read().await;
        assert!(reply_registry
            .tools
            .contains_key(crate::harness::native_tools::CHANNEL_PUBLISH_TOOL));
        assert!(reply_registry
            .tools
            .contains_key(crate::harness::native_tools::CHANNEL_SKIP_REPLY_TOOL));
        drop(reply_registry);

        let no_reply_runtime = AgentRuntime::build(
            "conic",
            "writer",
            "no-reply-session",
            &cp,
            &config,
            &registry,
        )
        .await
        .unwrap();
        let no_reply_registry = no_reply_runtime.executor.registry.read().await;
        assert!(!no_reply_registry
            .tools
            .contains_key(crate::harness::native_tools::CHANNEL_PUBLISH_TOOL));
        assert!(!no_reply_registry
            .tools
            .contains_key(crate::harness::native_tools::CHANNEL_SKIP_REPLY_TOOL));
    }

    #[tokio::test]
    async fn delegated_task_session_gets_update_task_tool() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        let config = runtime_config();
        let registry = crate::worker::mcp_registry::McpRegistry::new();
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

        put_agent_resource(kv.clone(), "delegate-ns", "writer", Some(spec)).await;
        kv.set_msg(
            &crate::control::keys::session("delegate-ns", "writer", "session-1"),
            &data_proto::Session {
                id: "session-1".to_string(),
                agent: "writer".to_string(),
                ns: "delegate-ns".to_string(),
                status: "active".to_string(),
                labels: HashMap::from([(
                    crate::control::delegation::LABEL_TASK_ROLE.to_string(),
                    "delegate".to_string(),
                )]),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let runtime = AgentRuntime::build(
            "delegate-ns",
            "writer",
            "session-1",
            &cp,
            &config,
            &registry,
        )
        .await
        .unwrap();
        let runtime_registry = runtime.executor.registry.read().await;
        assert!(runtime_registry
            .tools
            .contains_key(crate::harness::native_tools::UPDATE_TASK_TOOL));
    }

    #[tokio::test]
    async fn agent_runtime_build_assembles_history_and_skips_bad_entries() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        let config = runtime_config();
        let registry = crate::worker::mcp_registry::McpRegistry::new();
        let spec = manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: "assist".to_string(),
            post_history_prompt: String::new(),
            mcp_server_refs: vec!["missing-server".to_string()],
            capabilities: HashMap::new(),
            a2a: None,
            runtime: None,
        };

        put_agent_resource(kv.clone(), "conic", "writer", Some(spec)).await;
        kv.set_msg(
            &crate::control::keys::session_message("conic", "writer", "session-1", "msg-1"),
            &data_proto::SessionMessage {
                id: "msg-1".to_string(),
                role: 1,
                created_at: 10,
                labels: HashMap::new(),
                parts: vec![data_proto::SessionMessagePart {
                    id: "000000".to_string(),
                    part_type: data_proto::SessionMessagePartType::Text as i32,
                    content: "hello".to_string(),
                    name: String::new(),
                    payload_json: String::new(),
                    created_at: 10,
                    object: None,
                }],
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session_message("conic", "writer", "session-1", "msg-2"),
            &data_proto::SessionMessage {
                id: "msg-2".to_string(),
                role: 2,
                created_at: 20,
                labels: HashMap::new(),
                parts: vec![
                    data_proto::SessionMessagePart {
                        id: "000000".to_string(),
                        part_type: data_proto::SessionMessagePartType::Text as i32,
                        content: "assistant reply".to_string(),
                        name: String::new(),
                        payload_json: String::new(),
                        created_at: 20,
                        object: None,
                    },
                    data_proto::SessionMessagePart {
                        id: "000001".to_string(),
                        part_type: data_proto::SessionMessagePartType::ToolCall as i32,
                        content: String::new(),
                        name: "search".to_string(),
                        payload_json: serde_json::json!({
                            "tool_call_id": "call-1",
                            "input": { "q": "talon" }
                        })
                        .to_string(),
                        created_at: 21,
                        object: None,
                    },
                    data_proto::SessionMessagePart {
                        id: "000002".to_string(),
                        part_type: data_proto::SessionMessagePartType::ToolResult as i32,
                        content: "tool output".to_string(),
                        name: "search".to_string(),
                        payload_json: serde_json::json!({
                            "tool_call_id": "call-1",
                            "output_preview": "tool preview"
                        })
                        .to_string(),
                        created_at: 22,
                        object: None,
                    },
                ],
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
        let runtime = AgentRuntime::build("conic", "writer", "session-1", &cp, &config, &registry)
            .await
            .unwrap();

        assert_eq!(runtime.context.agent_id, "writer");
        assert_eq!(runtime.context.history.len(), 3);
        assert_eq!(runtime.context.history[0].role, "user");
        assert_eq!(runtime.context.history[0].text_content(), "hello");
        assert_eq!(runtime.context.history[1].role, "assistant");
        assert_eq!(
            runtime.context.history[1].tool_calls.as_ref().unwrap()[0].name,
            "search"
        );
        assert_eq!(runtime.context.history[2].role, "tool");
        assert_eq!(runtime.context.history[2].text_content(), "tool preview");
    }

    #[tokio::test]
    async fn agent_runtime_build_replays_failed_terminal_assistant_projection() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        let config = runtime_config();
        let registry = crate::worker::mcp_registry::McpRegistry::new();
        let spec = manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: "assist".to_string(),
            post_history_prompt: String::new(),
            mcp_server_refs: vec!["missing-server".to_string()],
            capabilities: HashMap::new(),
            a2a: None,
            runtime: None,
        };

        put_agent_resource(kv.clone(), "conic", "writer", Some(spec)).await;
        let mut failed_labels = HashMap::new();
        failed_labels.insert(
            crate::harness::sessions::SESSION_LABEL_PROJECTION_STATE.to_string(),
            crate::harness::sessions::SESSION_PROJECTION_STATE_FAILED.to_string(),
        );
        kv.set_msg(
            &crate::control::keys::session_message("conic", "writer", "session-1", "msg-1"),
            &data_proto::SessionMessage {
                id: "msg-1".to_string(),
                role: data_proto::MessageRole::RoleAssistant as i32,
                created_at: 20,
                labels: failed_labels,
                parts: vec![
                    data_proto::SessionMessagePart {
                        id: "000001".to_string(),
                        part_type: data_proto::SessionMessagePartType::Text as i32,
                        content: "I found Gmail is connected. ".to_string(),
                        name: String::new(),
                        payload_json: String::new(),
                        created_at: 20,
                        object: None,
                    },
                    data_proto::SessionMessagePart {
                        id: "000002".to_string(),
                        part_type: data_proto::SessionMessagePartType::ToolCall as i32,
                        content: "Tool call".to_string(),
                        name: "search_tools".to_string(),
                        payload_json: serde_json::json!({
                            "tool_call_id": "call-1",
                            "input": { "query": "gmail" }
                        })
                        .to_string(),
                        created_at: 21,
                        object: None,
                    },
                    data_proto::SessionMessagePart {
                        id: "000003".to_string(),
                        part_type: data_proto::SessionMessagePartType::ToolResult as i32,
                        content: "gmail_search available".to_string(),
                        name: "search_tools".to_string(),
                        payload_json: serde_json::json!({
                            "tool_call_id": "call-1",
                            "output_preview": "gmail_search available"
                        })
                        .to_string(),
                        created_at: 22,
                        object: None,
                    },
                    data_proto::SessionMessagePart {
                        id: "000004".to_string(),
                        part_type: data_proto::SessionMessagePartType::Error as i32,
                        content: "Turn limit reached".to_string(),
                        name: String::new(),
                        payload_json: String::new(),
                        created_at: 23,
                        object: None,
                    },
                ],
            },
        )
        .await
        .unwrap();

        let runtime = AgentRuntime::build("conic", "writer", "session-1", &cp, &config, &registry)
            .await
            .unwrap();

        assert_eq!(runtime.context.history.len(), 2);
        assert_eq!(runtime.context.history[0].role, "assistant");
        assert_eq!(
            runtime.context.history[0].text_content(),
            "I found Gmail is connected. "
        );
        assert_eq!(
            runtime.context.history[0].tool_calls.as_ref().unwrap()[0].name,
            "search_tools"
        );
        assert_eq!(runtime.context.history[1].role, "tool");
        assert_eq!(
            runtime.context.history[1].text_content(),
            "gmail_search available"
        );
    }

    #[tokio::test]
    async fn agent_runtime_build_rehydrates_image_parts_from_object_store() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        let config = runtime_config();
        let registry = crate::worker::mcp_registry::McpRegistry::new();
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

        put_agent_resource(kv.clone(), "conic", "writer", Some(spec)).await;

        let object = cp
            .objects
            .put(
                "sessions/session-1/screenshot.png",
                b"png-bytes",
                crate::control::object_store::ObjectMetadata {
                    media_type: "image/png".to_string(),
                    filename: "screenshot.png".to_string(),
                    ..crate::control::object_store::ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        kv.set_msg(
            &crate::control::keys::session_message("conic", "writer", "session-1", "msg-1"),
            &data_proto::SessionMessage {
                id: "msg-1".to_string(),
                role: data_proto::MessageRole::RoleUser as i32,
                created_at: 2,
                labels: HashMap::new(),
                parts: vec![
                    data_proto::SessionMessagePart {
                        id: "000000".to_string(),
                        part_type: data_proto::SessionMessagePartType::Text as i32,
                        content: "describe this".to_string(),
                        name: String::new(),
                        payload_json: String::new(),
                        created_at: 2,
                        object: None,
                    },
                    data_proto::SessionMessagePart {
                        id: "000001".to_string(),
                        part_type: data_proto::SessionMessagePartType::Image as i32,
                        content: String::new(),
                        name: String::new(),
                        payload_json: String::new(),
                        created_at: 2,
                        object: Some(object),
                    },
                ],
            },
        )
        .await
        .unwrap();

        let runtime = AgentRuntime::build("conic", "writer", "session-1", &cp, &config, &registry)
            .await
            .unwrap();

        assert_eq!(runtime.context.history.len(), 1);
        assert_eq!(runtime.context.history[0].text_content(), "describe this");
        assert_eq!(
            runtime.context.history[0].content_parts,
            vec![
                text_part("describe this"),
                image_data_part("image/png", "cG5nLWJ5dGVz", None::<String>),
            ]
        );
    }
}
