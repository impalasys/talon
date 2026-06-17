// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use std::path::Path;
use std::sync::Arc;

use super::mcp_registry::McpRegistry;
use crate::control::config::Config;
use crate::control::ControlPlane;
use crate::control::ProtoKeyValueStoreExt;
use crate::gateway::rpc::data_proto;
use crate::gateway::rpc::{manifests, protobuf_value::value::Kind as ProtoValueKind};
use crate::harness::executor::context_budget::tool_result_preview;
use crate::harness::executor::{
    AgentExecutor, ContextAssembler, ExecutionContext, LoopMessage, RegisteredMcpTool,
};
use crate::harness::knowledge::KvKnowledgeBook;
use crate::harness::llm::{ChatContentPart, ToolCall};
use crate::harness::skills::registry::ToolRegistry;
use base64::{engine::general_purpose, Engine as _};
use prost::Message;

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
        let store = crate::control::resources::ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
        let agent = store
            .get_agent(ns, agent_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found in ns '{}'", agent_id, ns))?;
        let spec = agent
            .spec
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' has no spec", agent_id))?;
        let session = cp
            .kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(
                ns, agent_id, session_id,
            ))
            .await?;
        let allow_channel_reply_tools = session
            .as_ref()
            .map(|session| {
                session
                    .labels
                    .contains_key(crate::gateway::rpc::channels::LABEL_CHANNEL)
                    && session
                        .labels
                        .get(crate::gateway::rpc::channels::LABEL_CHANNEL_REPLY_MODE)
                        .map(|mode| mode != "none")
                        .unwrap_or(true)
            })
            .unwrap_or(false);

        // 2. Load session history from KV
        let msg_prefix = crate::control::keys::session_message_prefix(ns, agent_id, session_id);
        let mut msg_entries = cp.kv.list_entries(&msg_prefix).await?;
        msg_entries.sort_by(|(left, _), (right, _)| left.cmp(right));

        let mut history = Vec::new();
        for (_, value) in msg_entries {
            if let Ok(msg) = data_proto::SessionMessage::decode(value.as_slice()) {
                let role = match data_proto::MessageRole::try_from(msg.role) {
                    Ok(data_proto::MessageRole::RoleUser) => "user",
                    Ok(data_proto::MessageRole::RoleAssistant) => "assistant",
                    Ok(data_proto::MessageRole::RoleSystem) => "system",
                    _ => "user",
                };

                let mut tool_calls = None;
                let mut tool_results: Vec<LoopMessage> = Vec::new();

                if msg.role == data_proto::MessageRole::RoleAssistant as i32 {
                    let mut collected_tool_calls = Vec::new();
                    for part in &msg.parts {
                        if part.part_type == data_proto::SessionMessagePartType::ToolCall as i32 {
                            let payload: serde_json::Value =
                                serde_json::from_str(&part.payload_json)
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
                                name: part.name.clone(),
                                arguments: serde_json::to_string(&input)
                                    .unwrap_or_else(|_| "null".to_string()),
                            });
                        } else if part.part_type
                            == data_proto::SessionMessagePartType::ToolResult as i32
                        {
                            if let Some(message) = tool_result_message_from_part(part) {
                                tool_results.push(message);
                            }
                        }
                    }

                    if !collected_tool_calls.is_empty() {
                        tool_calls = Some(collected_tool_calls);
                    }
                }

                history.push(LoopMessage {
                    role: role.to_string(),
                    content_parts: message_content_parts(&msg, cp.objects.as_ref()).await?,
                    tool_calls,
                    tool_call_id: None,
                });
                if !tool_results.is_empty() {
                    history.extend(tool_results);
                }
            }
        }

        // 3. Resolve LLM from AgentSpec + Config
        let llm = crate::harness::llm::resolver::resolve_llm(&spec, config).await?;

        // 4. Build tool registry (builtins + future MCP servers)
        let mut mcp_tools = std::collections::HashMap::new();
        let mut reg = ToolRegistry::new();
        crate::harness::knowledge::register_tools(&mut reg);
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
        let executor = AgentExecutor::new_with_session(
            llm,
            ContextAssembler::new("."),
            registry,
            Arc::new(config.clone()),
            Arc::new(KvKnowledgeBook::new(cp.kv.clone())),
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
                    "list_sessions" | "get_session" => {
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
    matches!(name, "list_sessions" | "get_session")
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
        crate::harness::knowledge::KNOWLEDGE_WRITE_TOOL,
        crate::harness::knowledge::KNOWLEDGE_SEARCH_TOOL,
        crate::harness::knowledge::KNOWLEDGE_GET_TOOL,
        crate::harness::knowledge::KNOWLEDGE_LIST_TOOL,
        crate::harness::native_tools::CREATE_SCHEDULE_TOOL,
        crate::harness::native_tools::GET_SCHEDULE_TOOL,
        crate::harness::native_tools::LIST_SCHEDULES_TOOL,
        crate::harness::native_tools::UPDATE_SCHEDULE_TOOL,
        crate::harness::native_tools::DELETE_SCHEDULE_TOOL,
        crate::harness::native_tools::CHANNEL_PUBLISH_TOOL,
        crate::harness::native_tools::CHANNEL_SKIP_REPLY_TOOL,
    ]
}

fn qualify_mcp_tool_name(
    config: &crate::harness::mcp::McpConnectionConfig,
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

fn inferred_image_media_type(key: &str) -> Option<&'static str> {
    match Path::new(key)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

async fn message_content_parts(
    message: &data_proto::SessionMessage,
    objects: &(dyn crate::control::object_store::ObjectStore + Send + Sync),
) -> Result<Vec<ChatContentPart>> {
    let mut content_parts = Vec::new();
    for part in &message.parts {
        if part.part_type == data_proto::SessionMessagePartType::Text as i32 {
            if !part.content.is_empty() {
                content_parts.push(ChatContentPart::Text {
                    text: part.content.clone(),
                });
            }
            continue;
        }

        if part.part_type != data_proto::SessionMessagePartType::Image as i32 {
            continue;
        }

        if !part.content.is_empty() {
            content_parts.push(ChatContentPart::Text {
                text: part.content.clone(),
            });
        }

        let payload = serde_json::from_str::<serde_json::Value>(&part.payload_json)
            .unwrap_or(serde_json::Value::Null);
        let detail = payload
            .get("detail")
            .and_then(|value| value.as_str())
            .map(ToString::to_string);
        if let Some(url) = payload.get("url").and_then(|value| value.as_str()) {
            content_parts.push(ChatContentPart::ImageUrl {
                url: url.to_string(),
                detail,
            });
            continue;
        }

        let Some(object) = part.object.as_ref() else {
            continue;
        };
        let stored = objects.get(&object.key).await?.ok_or_else(|| {
            anyhow!(
                "object '{}' referenced by message part is missing",
                object.key
            )
        })?;
        let mut media_type = if object.media_type.trim().is_empty() {
            stored.metadata.media_type.trim().to_string()
        } else {
            object.media_type.trim().to_string()
        };
        if media_type.is_empty() {
            media_type = inferred_image_media_type(&object.key)
                .ok_or_else(|| anyhow!("missing media type for image object '{}'", object.key))?
                .to_string();
        }
        if !media_type.to_ascii_lowercase().starts_with("image/") {
            return Err(anyhow!(
                "unsupported media type '{}' for image object '{}'",
                media_type,
                object.key
            ));
        }
        content_parts.push(ChatContentPart::ImageData {
            media_type,
            data_base64: general_purpose::STANDARD.encode(stored.bytes),
            detail,
        });
    }
    Ok(content_parts)
}

fn tool_result_message_from_part(part: &data_proto::SessionMessagePart) -> Option<LoopMessage> {
    let payload: serde_json::Value =
        serde_json::from_str(&part.payload_json).unwrap_or(serde_json::Value::Null);
    let tool_call_id = payload.get("tool_call_id").and_then(|v| v.as_str())?;
    let output = payload
        .get("output_preview")
        .and_then(|v| v.as_str())
        .or_else(|| payload.get("output").and_then(|v| v.as_str()))
        .map(tool_result_preview)
        .unwrap_or_else(|| tool_result_preview(&part.content));
    let mut message = LoopMessage::text("tool", output);
    message.tool_call_id = Some(tool_call_id.to_string());
    Some(message)
}

#[cfg(test)]
mod tests {
    use super::{
        builtin_tool_names, has_capability_action, message_content_parts, qualify_mcp_tool_name,
        tool_result_message_from_part, visible_tools_for_agent, AgentRuntime,
    };
    use crate::control::config::{proto, Config, ProviderConfig, Secret};
    use crate::control::{
        keys::{ResourceKey, ResourceList},
        object_store::{InMemoryObjectStore, ObjectMetadata, ObjectStore},
        scheduler::NoopSchedulerBackend,
        ControlPlane, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::{data_proto, manifests, protobuf_value, resources_proto};
    use crate::harness::llm::ChatContentPart;
    use crate::harness::mcp::McpConnectionConfig;
    use futures::stream;
    use prost::Message;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn tool_result_part(content: String, payload_json: String) -> data_proto::SessionMessagePart {
        data_proto::SessionMessagePart {
            id: "part-1".to_string(),
            part_type: data_proto::SessionMessagePartType::ToolResult as i32,
            content,
            name: "tool".to_string(),
            payload_json,
            created_at: 0,
            object: None,
        }
    }

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
            objects: crate::control::object_store::default_object_store(),
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
        let part = tool_result_part(
            "preview".to_string(),
            serde_json::json!({
                "tool_call_id": "tool-1",
                "output_preview": "small preview",
                "output": format!("{{\"payload\":\"{}\"}}", "x".repeat(10_000)),
            })
            .to_string(),
        );

        let message = tool_result_message_from_part(&part).unwrap();

        assert_eq!(message.tool_call_id.as_deref(), Some("tool-1"));
        assert_eq!(message.text_content(), "small preview");
    }

    #[test]
    fn tool_result_message_compacts_legacy_raw_output() {
        let raw_output = format!(
            "{{\"payload\":\"{}\",\"items\":[\"{}\",\"{}\"]}}",
            "x".repeat(20_000),
            "y".repeat(8_000),
            "z".repeat(8_000)
        );
        let part = tool_result_part(
            raw_output.clone(),
            serde_json::json!({
                "tool_call_id": "tool-1",
                "output": raw_output,
            })
            .to_string(),
        );

        let message = tool_result_message_from_part(&part).unwrap();

        assert!(message.text_content().len() < 10_000);
        assert!(
            message.text_content().contains("chars omitted")
                || message.text_content().contains("_truncated")
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
        let part = tool_result_part(
            "preview".to_string(),
            serde_json::json!({
                "output_preview": "small preview",
            })
            .to_string(),
        );

        assert!(tool_result_message_from_part(&part).is_none());
    }

    #[test]
    fn tool_result_message_falls_back_to_step_content_when_payload_has_no_output() {
        let part = tool_result_part(
            "fallback output".to_string(),
            serde_json::json!({
                "tool_call_id": "tool-1"
            })
            .to_string(),
        );

        let message = tool_result_message_from_part(&part).unwrap();
        assert_eq!(message.text_content(), "fallback output");
    }

    #[test]
    fn builtin_tool_names_contains_expected_schedule_and_knowledge_tools() {
        let names = builtin_tool_names();
        assert!(names.contains(&crate::harness::knowledge::KNOWLEDGE_GET_TOOL));
        assert!(names.contains(&crate::harness::knowledge::KNOWLEDGE_LIST_TOOL));
        assert!(names.contains(&crate::harness::native_tools::CREATE_SCHEDULE_TOOL));
        assert!(names.contains(&crate::harness::native_tools::DELETE_SCHEDULE_TOOL));
    }

    #[tokio::test]
    async fn agent_runtime_build_errors_for_missing_agent_or_spec() {
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
        let cp = control_plane(kv.clone());
        let config = runtime_config();
        let registry = crate::worker::mcp_registry::McpRegistry::new();
        let spec = manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: "assist".to_string(),
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
    async fn agent_runtime_build_rehydrates_image_parts_from_object_store() {
        let kv = Arc::new(MockKvStore::default());
        let cp = control_plane(kv.clone());
        let config = runtime_config();
        let registry = crate::worker::mcp_registry::McpRegistry::new();
        let spec = manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: "assist".to_string(),
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
                ChatContentPart::Text {
                    text: "describe this".to_string(),
                },
                ChatContentPart::ImageData {
                    media_type: "image/png".to_string(),
                    data_base64: "cG5nLWJ5dGVz".to_string(),
                    detail: None,
                },
            ]
        );
    }

    #[tokio::test]
    async fn message_content_parts_infers_missing_image_media_type_from_extension() {
        let store = InMemoryObjectStore::default();
        let object = store
            .put(
                "sessions/session-1/screenshot.jpeg",
                b"jpeg-bytes",
                ObjectMetadata::default(),
            )
            .await
            .unwrap();
        let message = data_proto::SessionMessage {
            id: "msg-1".to_string(),
            role: data_proto::MessageRole::RoleUser as i32,
            created_at: 2,
            labels: HashMap::new(),
            parts: vec![data_proto::SessionMessagePart {
                id: "000001".to_string(),
                part_type: data_proto::SessionMessagePartType::Image as i32,
                content: String::new(),
                name: String::new(),
                payload_json: String::new(),
                created_at: 2,
                object: Some(object),
            }],
        };

        let parts = message_content_parts(&message, &store).await.unwrap();

        assert_eq!(
            parts,
            vec![ChatContentPart::ImageData {
                media_type: "image/jpeg".to_string(),
                data_base64: "anBlZy1ieXRlcw==".to_string(),
                detail: None,
            }]
        );
    }

    #[tokio::test]
    async fn message_content_parts_rejects_non_image_object_media_type() {
        let store = InMemoryObjectStore::default();
        let object = store
            .put(
                "sessions/session-1/file.txt",
                b"text",
                ObjectMetadata {
                    media_type: "text/plain".to_string(),
                    ..ObjectMetadata::default()
                },
            )
            .await
            .unwrap();
        let message = data_proto::SessionMessage {
            id: "msg-1".to_string(),
            role: data_proto::MessageRole::RoleUser as i32,
            created_at: 2,
            labels: HashMap::new(),
            parts: vec![data_proto::SessionMessagePart {
                id: "000001".to_string(),
                part_type: data_proto::SessionMessagePartType::Image as i32,
                content: String::new(),
                name: String::new(),
                payload_json: String::new(),
                created_at: 2,
                object: Some(object),
            }],
        };

        let err = message_content_parts(&message, &store).await.unwrap_err();

        assert!(err.to_string().contains(
            "unsupported media type 'text/plain' for image object 'sessions/session-1/file.txt'"
        ));
    }

    #[tokio::test]
    async fn message_content_parts_rejects_unknown_image_media_type() {
        let store = InMemoryObjectStore::default();
        let object = store
            .put(
                "sessions/session-1/upload",
                b"unknown-bytes",
                ObjectMetadata::default(),
            )
            .await
            .unwrap();
        let message = data_proto::SessionMessage {
            id: "msg-1".to_string(),
            role: data_proto::MessageRole::RoleUser as i32,
            created_at: 2,
            labels: HashMap::new(),
            parts: vec![data_proto::SessionMessagePart {
                id: "000001".to_string(),
                part_type: data_proto::SessionMessagePartType::Image as i32,
                content: String::new(),
                name: String::new(),
                payload_json: String::new(),
                created_at: 2,
                object: Some(object),
            }],
        };

        let err = message_content_parts(&message, &store).await.unwrap_err();

        assert!(err
            .to_string()
            .contains("missing media type for image object 'sessions/session-1/upload'"));
    }
}
