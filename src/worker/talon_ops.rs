use anyhow::{anyhow, Result};
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use chrono::Utc;
use futures::{stream, StreamExt};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    },
    ErrorData as McpError, ServerHandler,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use crate::{
    control::{events, keys, ProtoKeyValueStoreExt},
    gateway::rpc::{manifests, models},
    scheduling,
};

use super::WorkerEventHandler;

const TALON_OPS_SERVER_NAME: &str = "talon-ops";
const TALON_OPS_AUDIENCE: &str = "talon-ops";
const MCP_AUTH_BROKER_AUDIENCE: &str = "conic-mcp-auth-broker";
const META_NS: &str = "talon-system:ns";
const DEFAULT_MAX_LIST_LIMIT: i32 = 100;
const DEFAULT_MAX_HISTORY_LOOKBACK_SECONDS: i32 = 7 * 24 * 60 * 60;
const DEFAULT_ACCESS_TOKEN_TTL_SECONDS: i64 = 3600;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TalonOpsAccessClaims {
    sub: String,
    aud: String,
    exp: usize,
    #[serde(rename = "talon:ns")]
    namespace: String,
    #[serde(rename = "talon:binding")]
    binding_name: String,
    #[serde(rename = "talon:agent")]
    agent_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct McpAuthBrokerClaims {
    sub: String,
    aud: String,
    exp: usize,
    #[serde(rename = "talon:ns")]
    namespace: String,
    #[serde(rename = "talon:binding")]
    binding_name: String,
    #[serde(rename = "talon:agent")]
    agent_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct McpAuthBrokerRequest {
    namespace: String,
    binding_name: String,
    server_ref: Option<String>,
    agent_name: Option<String>,
    audience: Option<String>,
}

#[derive(Debug, Serialize)]
struct McpAuthBrokerResponse {
    authorization_bearer_token: String,
    expires_at_unix: Option<i64>,
    issued_at_unix: i64,
}

#[derive(Debug, Clone)]
pub struct TalonOpsAccess {
    namespace: String,
    binding_name: String,
    agent_name: Option<String>,
    policy: TalonOpsPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TalonOpsPolicy {
    allowed_namespace_prefixes: Vec<String>,
    allow_session_messages: bool,
    allow_step_payloads: bool,
    max_list_limit: i32,
    max_history_lookback_seconds: i32,
}

impl TalonOpsAccess {
    fn max_list_limit(&self) -> usize {
        let configured = self.policy.max_list_limit;
        if configured > 0 {
            configured as usize
        } else {
            DEFAULT_MAX_LIST_LIMIT as usize
        }
    }

    fn max_history_lookback_seconds(&self) -> i64 {
        let configured = self.policy.max_history_lookback_seconds;
        if configured > 0 {
            configured as i64
        } else {
            DEFAULT_MAX_HISTORY_LOOKBACK_SECONDS as i64
        }
    }

    fn allows_namespace(&self, namespace: &str) -> bool {
        self.policy
            .allowed_namespace_prefixes
            .iter()
            .any(|prefix| namespace.starts_with(prefix))
    }
}

#[derive(Clone)]
pub struct TalonOpsServer {
    handler: WorkerEventHandler,
    tool_router: ToolRouter<Self>,
}

impl TalonOpsServer {
    pub fn new(handler: WorkerEventHandler) -> Self {
        Self {
            handler,
            tool_router: Self::tool_router(),
        }
    }

    fn kv(&self) -> &Arc<dyn crate::control::KeyValueStore + Send + Sync> {
        &self.handler.cp.kv
    }

    async fn load_messages<M>(
        &self,
        namespace: &str,
        keys: Vec<String>,
        concurrency: usize,
    ) -> Result<Vec<M>>
    where
        M: prost::Message + Default + Send + 'static,
    {
        let kv = self.handler.cp.kv.clone();
        let namespace = namespace.to_string();
        let mut items = stream::iter(keys.into_iter().map(move |key| {
            let kv = kv.clone();
            let namespace = namespace.clone();
            async move { kv.get_msg::<M>(&namespace, &key).await }
        }))
        .buffer_unordered(concurrency.max(1))
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        items.shrink_to_fit();
        Ok(items)
    }

    async fn list_all_namespaces(&self) -> Result<Vec<models::Namespace>> {
        let mut keys = self.kv().list_keys(META_NS, "Namespace/").await?;
        keys.sort();
        let namespaces = self
            .load_messages::<models::Namespace>(META_NS, keys, 32)
            .await?;
        Ok(namespaces
            .into_iter()
            .filter(|namespace| !namespace.is_deleted)
            .collect())
    }

    async fn get_namespace_model(&self, name: &str) -> Result<models::Namespace> {
        let key = format!("Namespace/{name}");
        self.kv()
            .get_msg::<models::Namespace>(META_NS, &key)
            .await?
            .filter(|ns| !ns.is_deleted)
            .ok_or_else(|| anyhow!("namespace '{name}' not found"))
    }

    async fn list_agent_names(&self, namespace: &str) -> Result<Vec<String>> {
        let mut keys = self.kv().list_keys(namespace, "Agent/").await?;
        keys.sort();
        Ok(keys
            .into_iter()
            .filter_map(|key| {
                let stripped = key.strip_prefix("Agent/").unwrap_or(&key);
                if stripped.contains('/') {
                    None
                } else {
                    Some(stripped.to_string())
                }
            })
            .collect())
    }

    async fn get_agent_model(&self, namespace: &str, name: &str) -> Result<models::Agent> {
        self.kv()
            .get_msg::<models::Agent>(namespace, &keys::agent(name))
            .await?
            .ok_or_else(|| anyhow!("agent '{name}' not found in namespace '{namespace}'"))
    }

    async fn list_sessions_for_agent(
        &self,
        namespace: &str,
        agent: &str,
    ) -> Result<Vec<models::Session>> {
        let prefix = keys::session_prefix(agent);
        let mut keys = self.kv().list_keys(namespace, &prefix).await?;
        keys.sort();
        let session_keys = keys
            .into_iter()
            .filter(|key| {
                let stripped = key.strip_prefix(&prefix).unwrap_or(key);
                !stripped.contains('/')
            })
            .collect::<Vec<_>>();
        self.load_messages::<models::Session>(namespace, session_keys, 32)
            .await
    }

    async fn get_session_messages(
        &self,
        namespace: &str,
        agent: &str,
        session_id: &str,
    ) -> Result<Vec<models::SessionMessage>> {
        let prefix = keys::session_message_prefix(agent, session_id);
        let mut keys = self.kv().list_keys(namespace, &prefix).await?;
        keys.sort();
        let message_keys = keys
            .into_iter()
            .filter(|key| {
                let stripped = key.strip_prefix(&prefix).unwrap_or(key);
                !stripped.contains('/')
            })
            .collect::<Vec<_>>();
        self.load_messages::<models::SessionMessage>(namespace, message_keys, 32)
            .await
    }

    async fn get_session_steps(
        &self,
        namespace: &str,
        agent: &str,
        session_id: &str,
    ) -> Result<Vec<events::SessionStepEvent>> {
        let prefix = keys::session_message_prefix(agent, session_id);
        let mut keys = self.kv().list_keys(namespace, &prefix).await?;
        keys.sort();
        let step_keys = keys
            .into_iter()
            .filter(|key| {
                let stripped = key.strip_prefix(&prefix).unwrap_or(key);
                stripped.contains("/Steps/")
            })
            .collect::<Vec<_>>();
        self.load_messages::<events::SessionStepEvent>(namespace, step_keys, 64)
            .await
    }

    async fn list_schedule_models(&self, namespace: &str) -> Result<Vec<models::Schedule>> {
        let mut keys = self
            .kv()
            .list_keys(namespace, keys::schedule_prefix())
            .await?;
        keys.sort();
        let schedule_keys = keys
            .into_iter()
            .filter(|key| {
                let stripped = key.strip_prefix(keys::schedule_prefix()).unwrap_or(key);
                !stripped.contains('/')
            })
            .collect::<Vec<_>>();
        self.load_messages::<models::Schedule>(namespace, schedule_keys, 32)
            .await
    }

    async fn upsert_schedule(
        &self,
        args: &PutScheduleArgs,
        existing: Option<models::Schedule>,
    ) -> Result<models::Schedule> {
        let existing_spec = existing
            .as_ref()
            .and_then(|schedule| schedule.spec.as_ref());
        let existing_target = existing_spec.and_then(|spec| spec.target.as_ref());
        let mut schedule = models::Schedule {
            name: args.name.clone(),
            ns: args.namespace.clone(),
            labels: args
                .labels
                .clone()
                .or_else(|| existing.as_ref().map(|schedule| schedule.labels.clone()))
                .unwrap_or_default(),
            spec: Some(models::ScheduleSpec {
                kind: if args.kind.is_empty() {
                    existing_spec
                        .map(|spec| spec.kind.clone())
                        .unwrap_or_default()
                } else {
                    crate::scheduling::normalize_schedule_kind(&args.kind)
                },
                cron: args
                    .cron
                    .clone()
                    .or_else(|| existing_spec.map(|spec| spec.cron.clone()))
                    .unwrap_or_default(),
                interval_seconds: args
                    .interval_seconds
                    .or_else(|| existing_spec.map(|spec| spec.interval_seconds))
                    .unwrap_or_default(),
                run_at: args
                    .run_at
                    .clone()
                    .or_else(|| existing_spec.map(|spec| spec.run_at.clone()))
                    .unwrap_or_default(),
                timezone: args
                    .timezone
                    .clone()
                    .or_else(|| existing_spec.map(|spec| spec.timezone.clone()))
                    .unwrap_or_default(),
                target: Some(models::ScheduleTarget {
                    agent: if args.agent.is_empty() {
                        existing_target
                            .map(|target| target.agent.clone())
                            .unwrap_or_default()
                    } else {
                        args.agent.clone()
                    },
                    session_mode: crate::scheduling::normalize_session_mode(&args
                        .session_mode
                        .clone()
                        .or_else(|| existing_target.map(|target| target.session_mode.clone()))
                        .unwrap_or_else(|| "new".to_string()))?,
                    session_id: args
                        .session_id
                        .clone()
                        .or_else(|| existing_target.map(|target| target.session_id.clone()))
                        .unwrap_or_default(),
                }),
                input_message: if args.input_message.is_empty() {
                    existing_spec
                        .map(|spec| spec.input_message.clone())
                        .unwrap_or_default()
                } else {
                    args.input_message.clone()
                },
                enabled: args
                    .enabled
                    .or_else(|| existing_spec.map(|spec| spec.enabled))
                    .unwrap_or(true),
            }),
            status: existing.and_then(|schedule| schedule.status),
        };

        let next_run = scheduling::initialize_schedule(&mut schedule, Utc::now())?;
        scheduling::persist_schedule(self.handler.cp.kv.as_ref(), &schedule).await?;
        scheduling::arm_schedule(self.handler.cp.scheduler.as_ref(), &mut schedule, next_run)
            .await?;
        scheduling::persist_schedule(self.handler.cp.kv.as_ref(), &schedule).await?;
        Ok(schedule)
    }
}

fn talon_ops_access_from_parts(
    parts: &axum::http::request::Parts,
) -> Result<TalonOpsAccess, McpError> {
    parts
        .extensions
        .get::<TalonOpsAccess>()
        .cloned()
        .ok_or_else(|| {
            McpError::invalid_params(
                format!(
                    "missing extension {}",
                    std::any::type_name::<TalonOpsAccess>()
                ),
                None,
            )
        })
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(default)]
struct ListNamespacesArgs {
    parent: Option<String>,
    prefix: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetNamespaceArgs {
    name: String,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(default)]
struct ListAgentsArgs {
    namespace: String,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetAgentArgs {
    namespace: String,
    name: String,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(default)]
struct ListSessionsArgs {
    namespace: String,
    agent: Option<String>,
    state: Option<String>,
    limit: Option<usize>,
    updated_since: Option<i64>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(default)]
struct GetSessionArgs {
    namespace: String,
    agent: String,
    session_id: String,
    include_messages: Option<bool>,
    include_steps: Option<bool>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(default)]
struct ListRecentStepsArgs {
    namespace: String,
    agent: Option<String>,
    session_id: Option<String>,
    limit: Option<usize>,
    since: Option<i64>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(default)]
struct ListMcpBindingsArgs {
    namespace: String,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetMcpBindingArgs {
    namespace: String,
    name: String,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(default)]
struct ListMcpServersArgs {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetMcpServerArgs {
    name: String,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(default)]
struct ListSchedulesArgs {
    namespace: String,
    agent: Option<String>,
    enabled: Option<bool>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetScheduleArgs {
    namespace: String,
    name: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PutScheduleArgs {
    namespace: String,
    name: String,
    labels: Option<HashMap<String, String>>,
    kind: String,
    cron: Option<String>,
    interval_seconds: Option<u32>,
    run_at: Option<String>,
    timezone: Option<String>,
    agent: String,
    session_mode: Option<String>,
    session_id: Option<String>,
    input_message: String,
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DeleteScheduleArgs {
    namespace: String,
    name: String,
}

#[tool_router]
impl TalonOpsServer {
    #[tool(description = "List namespaces visible to the caller's talon-ops binding policy.")]
    async fn list_namespaces(
        &self,
        rmcp::handler::server::common::Extension(parts): rmcp::handler::server::common::Extension<
            axum::http::request::Parts,
        >,
        Parameters(args): Parameters<ListNamespacesArgs>,
    ) -> Result<String, McpError> {
        let access = talon_ops_access_from_parts(&parts)?;
        let limit = bounded_limit(&access, args.limit);
        let namespaces = self
            .list_all_namespaces()
            .await
            .map_err(internal_mcp_error)?;
        let mut filtered = namespaces
            .into_iter()
            .filter(|namespace| access.allows_namespace(&namespace.name))
            .filter(|namespace| {
                args.parent
                    .as_ref()
                    .map(|parent| namespace.parent == *parent)
                    .unwrap_or(true)
            })
            .filter(|namespace| {
                args.prefix
                    .as_ref()
                    .map(|prefix| namespace.name.starts_with(prefix))
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();
        filtered.sort_by(|left, right| left.name.cmp(&right.name));
        filtered.truncate(limit);
        to_json_string(&json!({ "namespaces": filtered }))
    }

    #[tool(description = "Get a namespace visible to the caller's talon-ops binding policy.")]
    async fn get_namespace(
        &self,
        rmcp::handler::server::common::Extension(parts): rmcp::handler::server::common::Extension<
            axum::http::request::Parts,
        >,
        Parameters(args): Parameters<GetNamespaceArgs>,
    ) -> Result<String, McpError> {
        let access = talon_ops_access_from_parts(&parts)?;
        require_namespace_access(&access, &args.name)?;
        let namespace = self
            .get_namespace_model(&args.name)
            .await
            .map_err(internal_mcp_error)?;
        to_json_string(&json!({ "namespace": namespace }))
    }

    #[tool(
        description = "List agents in a namespace visible to the caller's talon-ops binding policy."
    )]
    async fn list_agents(
        &self,
        rmcp::handler::server::common::Extension(parts): rmcp::handler::server::common::Extension<
            axum::http::request::Parts,
        >,
        Parameters(args): Parameters<ListAgentsArgs>,
    ) -> Result<String, McpError> {
        let access = talon_ops_access_from_parts(&parts)?;
        require_namespace_access(&access, &args.namespace)?;
        let limit = bounded_limit(&access, args.limit);
        let names = self
            .list_agent_names(&args.namespace)
            .await
            .map_err(internal_mcp_error)?;
        let mut agents = Vec::new();
        for name in names.into_iter().take(limit) {
            let agent = self
                .get_agent_model(&args.namespace, &name)
                .await
                .map_err(internal_mcp_error)?;
            agents.push(crate::manifest::render_agent_json(&agent).map_err(internal_mcp_error)?);
        }
        to_json_string(&json!({ "agents": agents }))
    }

    #[tool(description = "Get a single agent and its effective spec.")]
    async fn get_agent(
        &self,
        rmcp::handler::server::common::Extension(parts): rmcp::handler::server::common::Extension<
            axum::http::request::Parts,
        >,
        Parameters(args): Parameters<GetAgentArgs>,
    ) -> Result<String, McpError> {
        let access = talon_ops_access_from_parts(&parts)?;
        require_namespace_access(&access, &args.namespace)?;
        let agent = self
            .get_agent_model(&args.namespace, &args.name)
            .await
            .map_err(internal_mcp_error)?;
        to_json_string(&json!({ "agent": crate::manifest::render_agent_json(&agent).map_err(internal_mcp_error)? }))
    }

    #[tool(description = "List sessions in one or more visible namespaces and agents.")]
    async fn list_sessions(
        &self,
        rmcp::handler::server::common::Extension(parts): rmcp::handler::server::common::Extension<
            axum::http::request::Parts,
        >,
        Parameters(args): Parameters<ListSessionsArgs>,
    ) -> Result<String, McpError> {
        let access = talon_ops_access_from_parts(&parts)?;
        require_namespace_access(&access, &args.namespace)?;
        let limit = bounded_limit(&access, args.limit);
        let mut sessions = Vec::new();
        let agents = if let Some(agent) = args.agent.clone() {
            vec![agent]
        } else {
            self.list_agent_names(&args.namespace)
                .await
                .map_err(internal_mcp_error)?
        };
        for agent in agents {
            let mut agent_sessions = self
                .list_sessions_for_agent(&args.namespace, &agent)
                .await
                .map_err(internal_mcp_error)?;
            sessions.append(&mut agent_sessions);
        }
        sessions.retain(|session| {
            args.state
                .as_ref()
                .map(|state| session.status == *state)
                .unwrap_or(true)
                && args
                    .updated_since
                    .map(|updated_since| session.last_active >= updated_since)
                    .unwrap_or(true)
        });
        sessions.sort_by(|left, right| right.last_active.cmp(&left.last_active));
        sessions.truncate(limit);
        to_json_string(&json!({ "sessions": sessions }))
    }

    #[tool(
        description = "Get a session, optionally including raw messages and step payloads if the binding policy allows it."
    )]
    async fn get_session(
        &self,
        rmcp::handler::server::common::Extension(parts): rmcp::handler::server::common::Extension<
            axum::http::request::Parts,
        >,
        Parameters(args): Parameters<GetSessionArgs>,
    ) -> Result<String, McpError> {
        let access = talon_ops_access_from_parts(&parts)?;
        require_namespace_access(&access, &args.namespace)?;
        let session = self
            .kv()
            .get_msg::<models::Session>(
                &args.namespace,
                &keys::session(&args.agent, &args.session_id),
            )
            .await
            .map_err(internal_mcp_error)?
            .ok_or_else(|| McpError::invalid_params("session not found".to_string(), None))?;
        let include_messages = args.include_messages.unwrap_or(false);
        let include_steps = args.include_steps.unwrap_or(false);
        if include_messages && !access.policy.allow_session_messages {
            return Err(McpError::invalid_params(
                "binding policy does not allow session messages".to_string(),
                None,
            ));
        }
        if include_steps && !access.policy.allow_step_payloads {
            return Err(McpError::invalid_params(
                "binding policy does not allow step payloads".to_string(),
                None,
            ));
        }

        let mut payload = json!({ "session": session });
        if include_messages {
            let messages = self
                .get_session_messages(&args.namespace, &args.agent, &args.session_id)
                .await
                .map_err(internal_mcp_error)?;
            payload["messages"] = serde_json::to_value(&messages).map_err(internal_mcp_error)?;
        }
        if include_steps {
            let steps = self
                .get_session_steps(&args.namespace, &args.agent, &args.session_id)
                .await
                .map_err(internal_mcp_error)?;
            payload["steps"] = serde_json::to_value(&steps).map_err(internal_mcp_error)?;
        }
        to_json_string(&payload)
    }

    #[tool(
        description = "List recent session steps across visible namespaces, optionally filtered by agent or session."
    )]
    async fn list_recent_steps(
        &self,
        rmcp::handler::server::common::Extension(parts): rmcp::handler::server::common::Extension<
            axum::http::request::Parts,
        >,
        Parameters(args): Parameters<ListRecentStepsArgs>,
    ) -> Result<String, McpError> {
        let access = talon_ops_access_from_parts(&parts)?;
        require_namespace_access(&access, &args.namespace)?;
        let limit = bounded_limit(&access, args.limit);
        let now = Utc::now().timestamp();
        let min_timestamp = args
            .since
            .unwrap_or(now - access.max_history_lookback_seconds());
        let agents = if let Some(agent) = args.agent.clone() {
            vec![agent]
        } else {
            self.list_agent_names(&args.namespace)
                .await
                .map_err(internal_mcp_error)?
        };
        let mut steps = Vec::new();
        for agent in agents {
            let sessions = self
                .list_sessions_for_agent(&args.namespace, &agent)
                .await
                .map_err(internal_mcp_error)?;
            for session in sessions {
                if args
                    .session_id
                    .as_ref()
                    .map(|session_id| &session.id == session_id)
                    .unwrap_or(true)
                {
                    let mut session_steps = self
                        .get_session_steps(&args.namespace, &agent, &session.id)
                        .await
                        .map_err(internal_mcp_error)?;
                    steps.append(&mut session_steps);
                }
            }
        }
        steps.retain(|step| step.timestamp >= min_timestamp);
        steps.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
        steps.truncate(limit);
        if !access.policy.allow_step_payloads {
            for step in &mut steps {
                step.payload_json.clear();
            }
        }
        to_json_string(&json!({ "steps": steps }))
    }

    #[tool(description = "List MCP bindings in a visible namespace.")]
    async fn list_mcp_bindings(
        &self,
        rmcp::handler::server::common::Extension(parts): rmcp::handler::server::common::Extension<
            axum::http::request::Parts,
        >,
        Parameters(args): Parameters<ListMcpBindingsArgs>,
    ) -> Result<String, McpError> {
        let access = talon_ops_access_from_parts(&parts)?;
        require_namespace_access(&access, &args.namespace)?;
        let limit = bounded_limit(&access, args.limit);
        let mut keys = self
            .kv()
            .list_keys(&args.namespace, keys::mcp_server_binding_prefix())
            .await
            .map_err(internal_mcp_error)?;
        keys.sort();
        let mut bindings = Vec::new();
        for key in keys.into_iter().take(limit) {
            if let Some(binding) = self
                .kv()
                .get_msg::<manifests::McpServerBinding>(&args.namespace, &key)
                .await
                .map_err(internal_mcp_error)?
            {
                bindings.push(binding);
            }
        }
        to_json_string(&json!({ "bindings": bindings }))
    }

    #[tool(description = "Get a single MCP binding from a visible namespace.")]
    async fn get_mcp_binding(
        &self,
        rmcp::handler::server::common::Extension(parts): rmcp::handler::server::common::Extension<
            axum::http::request::Parts,
        >,
        Parameters(args): Parameters<GetMcpBindingArgs>,
    ) -> Result<String, McpError> {
        let access = talon_ops_access_from_parts(&parts)?;
        require_namespace_access(&access, &args.namespace)?;
        let binding = self
            .kv()
            .get_msg::<manifests::McpServerBinding>(
                &args.namespace,
                &keys::mcp_server_binding(&args.name),
            )
            .await
            .map_err(internal_mcp_error)?
            .ok_or_else(|| McpError::invalid_params("MCP binding not found".to_string(), None))?;
        to_json_string(&json!({ "binding": binding }))
    }

    #[tool(description = "List system MCP servers available in Talon.")]
    async fn list_mcp_servers(
        &self,
        Parameters(args): Parameters<ListMcpServersArgs>,
    ) -> Result<String, McpError> {
        let limit = args.limit.unwrap_or(DEFAULT_MAX_LIST_LIMIT as usize);
        let mut keys = self
            .kv()
            .list_keys(crate::control::ns::TALON_SYSTEM, keys::mcp_server_prefix())
            .await
            .map_err(internal_mcp_error)?;
        keys.sort();
        let mut servers = Vec::new();
        for key in keys.into_iter().take(limit) {
            if let Some(server) = self
                .kv()
                .get_msg::<manifests::McpServer>(crate::control::ns::TALON_SYSTEM, &key)
                .await
                .map_err(internal_mcp_error)?
            {
                servers.push(server);
            }
        }
        to_json_string(&json!({ "servers": servers }))
    }

    #[tool(description = "Get a single system MCP server by name.")]
    async fn get_mcp_server(
        &self,
        Parameters(args): Parameters<GetMcpServerArgs>,
    ) -> Result<String, McpError> {
        let server = self
            .kv()
            .get_msg::<manifests::McpServer>(
                crate::control::ns::TALON_SYSTEM,
                &keys::mcp_server(&args.name),
            )
            .await
            .map_err(internal_mcp_error)?
            .ok_or_else(|| McpError::invalid_params("MCP server not found".to_string(), None))?;
        to_json_string(&json!({ "server": server }))
    }

    #[tool(
        description = "List schedules in a visible namespace, optionally filtered by target agent or enabled state."
    )]
    async fn list_schedules(
        &self,
        rmcp::handler::server::common::Extension(parts): rmcp::handler::server::common::Extension<
            axum::http::request::Parts,
        >,
        Parameters(args): Parameters<ListSchedulesArgs>,
    ) -> Result<String, McpError> {
        let access = talon_ops_access_from_parts(&parts)?;
        require_namespace_access(&access, &args.namespace)?;
        let limit = bounded_limit(&access, args.limit);
        let mut schedules = self
            .list_schedule_models(&args.namespace)
            .await
            .map_err(internal_mcp_error)?;
        schedules.retain(|schedule| {
            let spec = schedule.spec.as_ref();
            args.agent
                .as_ref()
                .map(|agent| {
                    spec.and_then(|spec| spec.target.as_ref())
                        .map(|target| &target.agent == agent)
                        .unwrap_or(false)
                })
                .unwrap_or(true)
                && args
                    .enabled
                    .map(|enabled| spec.map(|spec| spec.enabled == enabled).unwrap_or(false))
                    .unwrap_or(true)
        });
        schedules.sort_by(|left, right| left.name.cmp(&right.name));
        schedules.truncate(limit);
        to_json_string(&json!({
            "schedules": schedules.iter().map(schedule_json).collect::<Vec<_>>()
        }))
    }

    #[tool(description = "Get a single schedule from a visible namespace.")]
    async fn get_schedule(
        &self,
        rmcp::handler::server::common::Extension(parts): rmcp::handler::server::common::Extension<
            axum::http::request::Parts,
        >,
        Parameters(args): Parameters<GetScheduleArgs>,
    ) -> Result<String, McpError> {
        let access = talon_ops_access_from_parts(&parts)?;
        require_namespace_access(&access, &args.namespace)?;
        let schedule =
            scheduling::load_schedule(self.handler.cp.kv.as_ref(), &args.namespace, &args.name)
                .await
                .map_err(internal_mcp_error)?
                .ok_or_else(|| McpError::invalid_params("schedule not found".to_string(), None))?;
        to_json_string(&json!({ "schedule": schedule_json(&schedule) }))
    }

    #[tool(description = "Create a schedule for an agent in a visible namespace.")]
    async fn create_schedule(
        &self,
        rmcp::handler::server::common::Extension(parts): rmcp::handler::server::common::Extension<
            axum::http::request::Parts,
        >,
        Parameters(args): Parameters<PutScheduleArgs>,
    ) -> Result<String, McpError> {
        let access = talon_ops_access_from_parts(&parts)?;
        require_namespace_access(&access, &args.namespace)?;
        let key = keys::schedule(&args.name);
        if self
            .kv()
            .get_msg::<models::Schedule>(&args.namespace, &key)
            .await
            .map_err(internal_mcp_error)?
            .is_some()
        {
            return Err(McpError::invalid_params(
                format!("schedule '{}' already exists", args.name),
                None,
            ));
        }
        let schedule = self
            .upsert_schedule(&args, None)
            .await
            .map_err(internal_mcp_error)?;
        to_json_string(&json!({
            "schedule": schedule_json(&schedule),
            "backendArmed": schedule.status.as_ref().map(|status| status.backend_armed).unwrap_or(false),
        }))
    }

    #[tool(description = "Update an existing schedule in a visible namespace.")]
    async fn update_schedule(
        &self,
        rmcp::handler::server::common::Extension(parts): rmcp::handler::server::common::Extension<
            axum::http::request::Parts,
        >,
        Parameters(args): Parameters<PutScheduleArgs>,
    ) -> Result<String, McpError> {
        let access = talon_ops_access_from_parts(&parts)?;
        require_namespace_access(&access, &args.namespace)?;
        let existing =
            scheduling::load_schedule(self.handler.cp.kv.as_ref(), &args.namespace, &args.name)
                .await
                .map_err(internal_mcp_error)?
                .ok_or_else(|| McpError::invalid_params("schedule not found".to_string(), None))?;
        let schedule = self
            .upsert_schedule(&args, Some(existing))
            .await
            .map_err(internal_mcp_error)?;
        to_json_string(&json!({
            "schedule": schedule_json(&schedule),
            "backendArmed": schedule.status.as_ref().map(|status| status.backend_armed).unwrap_or(false),
        }))
    }

    #[tool(description = "Delete a schedule from a visible namespace.")]
    async fn delete_schedule(
        &self,
        rmcp::handler::server::common::Extension(parts): rmcp::handler::server::common::Extension<
            axum::http::request::Parts,
        >,
        Parameters(args): Parameters<DeleteScheduleArgs>,
    ) -> Result<String, McpError> {
        let access = talon_ops_access_from_parts(&parts)?;
        require_namespace_access(&access, &args.namespace)?;
        let key = keys::schedule(&args.name);
        if let Some(schedule) = self
            .kv()
            .get_msg::<models::Schedule>(&args.namespace, &key)
            .await
            .map_err(internal_mcp_error)?
        {
            if let Some(handle) = schedule.status.and_then(|status| status.backend_handle) {
                self.handler
                    .cp
                    .scheduler
                    .cancel(&handle)
                    .await
                    .map_err(internal_mcp_error)?;
            }
        }
        self.kv()
            .delete(&args.namespace, &key)
            .await
            .map_err(internal_mcp_error)?;
        to_json_string(&json!({ "deleted": true }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for TalonOpsServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(
            "Talon operations MCP for control-plane inspection and schedule management.".into(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}

pub fn talon_ops_router(handler: WorkerEventHandler) -> Router<WorkerEventHandler> {
    let service: StreamableHttpService<TalonOpsServer, LocalSessionManager> =
        StreamableHttpService::new(
            {
                let handler = handler.clone();
                move || Ok(TalonOpsServer::new(handler.clone()))
            },
            Default::default(),
            StreamableHttpServerConfig::default()
                .with_stateful_mode(true)
                .with_sse_keep_alive(Some(Duration::from_secs(15)))
                .disable_allowed_hosts(),
        );

    let protected_service =
        Router::new()
            .nest_service("/", service)
            .route_layer(axum::middleware::from_fn_with_state(
                handler.clone(),
                talon_ops_auth_middleware,
            ));

    Router::new()
        .route("/auth", post(talon_ops_auth_broker))
        .nest("/", protected_service)
}

async fn talon_ops_auth_middleware(
    State(handler): State<WorkerEventHandler>,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    match talon_ops_access_from_request(&handler, request.headers().get(header::AUTHORIZATION))
        .await
    {
        Ok(access) => {
            request.extensions_mut().insert(access);
            next.run(request).await
        }
        Err(error) => error.into_response(),
    }
}

async fn talon_ops_auth_broker(
    State(handler): State<WorkerEventHandler>,
    headers: HeaderMap,
    Json(payload): Json<McpAuthBrokerRequest>,
) -> impl IntoResponse {
    let Some(auth_header) = headers.get(header::AUTHORIZATION) else {
        return (StatusCode::UNAUTHORIZED, "missing authorization header").into_response();
    };
    let claims = match parse_mcp_auth_broker_claims(auth_header.to_str().unwrap_or_default()) {
        Ok(claims) => claims,
        Err(error) => return (StatusCode::UNAUTHORIZED, error).into_response(),
    };
    if claims.namespace != payload.namespace || claims.binding_name != payload.binding_name {
        return (StatusCode::FORBIDDEN, "namespace or binding mismatch").into_response();
    }
    if claims.agent_name != payload.agent_name {
        return (StatusCode::FORBIDDEN, "agent mismatch").into_response();
    }
    if payload
        .server_ref
        .as_deref()
        .is_some_and(|server_ref| server_ref != TALON_OPS_SERVER_NAME)
    {
        return (StatusCode::BAD_REQUEST, "unsupported talon-ops binding").into_response();
    }
    if payload
        .audience
        .as_deref()
        .is_some_and(|audience| audience != TALON_OPS_SERVER_NAME)
    {
        return (StatusCode::BAD_REQUEST, "unsupported talon-ops audience").into_response();
    }

    let binding = match load_talon_ops_binding(
        handler.cp.kv.as_ref(),
        &claims.namespace,
        &claims.binding_name,
        claims.agent_name.as_deref(),
    )
    .await
    {
        Ok(binding) => binding,
        Err(error) => return (StatusCode::FORBIDDEN, error.to_string()).into_response(),
    };

    let issued_at_unix = Utc::now().timestamp();
    let expires_at_unix = issued_at_unix + DEFAULT_ACCESS_TOKEN_TTL_SECONDS;
    let token = match mint_talon_ops_access_token(
        &binding.namespace,
        &binding.binding_name,
        binding.agent_name.as_deref(),
        expires_at_unix,
    ) {
        Ok(token) => token,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to mint talon-ops token: {error}"),
            )
                .into_response()
        }
    };

    Json(McpAuthBrokerResponse {
        authorization_bearer_token: token,
        expires_at_unix: Some(expires_at_unix),
        issued_at_unix,
    })
    .into_response()
}

async fn talon_ops_access_from_request(
    handler: &WorkerEventHandler,
    auth_header: Option<&header::HeaderValue>,
) -> std::result::Result<TalonOpsAccess, (StatusCode, String)> {
    let auth_header = auth_header
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "missing authorization header".to_string(),
            )
        })?;
    let claims = parse_talon_ops_access_claims(auth_header)
        .map_err(|error| (StatusCode::UNAUTHORIZED, error))?;
    load_talon_ops_binding(
        handler.cp.kv.as_ref(),
        &claims.namespace,
        &claims.binding_name,
        claims.agent_name.as_deref(),
    )
    .await
    .map_err(|error| (StatusCode::FORBIDDEN, error.to_string()))
}

async fn load_talon_ops_binding(
    kv: &dyn crate::control::KeyValueStore,
    namespace: &str,
    binding_name: &str,
    agent_name: Option<&str>,
) -> Result<TalonOpsAccess> {
    let binding = kv
        .get_msg::<manifests::McpServerBinding>(namespace, &keys::mcp_server_binding(binding_name))
        .await?
        .ok_or_else(|| anyhow!("binding '{binding_name}' not found in namespace '{namespace}'"))?;
    let spec = binding
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("binding '{binding_name}' missing spec"))?;
    if spec.server_ref != TALON_OPS_SERVER_NAME {
        return Err(anyhow!(
            "binding '{binding_name}' does not reference {}",
            TALON_OPS_SERVER_NAME
        ));
    }
    let policy = load_talon_ops_policy(kv).await?;
    Ok(TalonOpsAccess {
        namespace: namespace.to_string(),
        binding_name: binding_name.to_string(),
        agent_name: agent_name.map(str::to_string),
        policy,
    })
}

async fn load_talon_ops_policy(kv: &dyn crate::control::KeyValueStore) -> Result<TalonOpsPolicy> {
    let server = kv
        .get_msg::<manifests::McpServer>(
            crate::control::ns::TALON_SYSTEM,
            &keys::mcp_server(TALON_OPS_SERVER_NAME),
        )
        .await?
        .ok_or_else(|| anyhow!("MCPServer '{}' not found", TALON_OPS_SERVER_NAME))?;
    let spec = server
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("MCPServer '{}' missing spec", TALON_OPS_SERVER_NAME))?;
    parse_talon_ops_policy_from_target(spec.target.trim())
}

fn parse_talon_ops_policy_from_target(target: &str) -> Result<TalonOpsPolicy> {
    let url = reqwest::Url::parse(target)
        .map_err(|error| anyhow!("invalid talon-ops target URL '{}': {}", target, error))?;
    let mut allowed_namespace_prefixes = Vec::new();
    let mut allow_session_messages = None;
    let mut allow_step_payloads = None;
    let mut max_list_limit = None;
    let mut max_history_lookback_seconds = None;
    let mut seen_singletons = HashSet::new();

    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "allowed_prefix" => {
                if value.trim().is_empty() || value.as_ref() != value.trim() {
                    return Err(anyhow!(
                        "talon-ops target allowed_prefix values must be non-empty and trimmed"
                    ));
                }
                allowed_namespace_prefixes.push(value.into_owned());
            }
            "session_messages" => {
                ensure_singleton(&mut seen_singletons, "session_messages")?;
                allow_session_messages = Some(parse_bool_query_param("session_messages", &value)?);
            }
            "step_payloads" => {
                ensure_singleton(&mut seen_singletons, "step_payloads")?;
                allow_step_payloads = Some(parse_bool_query_param("step_payloads", &value)?);
            }
            "max_limit" => {
                ensure_singleton(&mut seen_singletons, "max_limit")?;
                max_list_limit = Some(parse_non_negative_i32_query_param("max_limit", &value)?);
            }
            "max_lookback_s" => {
                ensure_singleton(&mut seen_singletons, "max_lookback_s")?;
                max_history_lookback_seconds = Some(parse_non_negative_i32_query_param(
                    "max_lookback_s",
                    &value,
                )?);
            }
            other => {
                return Err(anyhow!(
                    "unsupported talon-ops target query parameter '{}'",
                    other
                ));
            }
        }
    }

    if allowed_namespace_prefixes.is_empty() {
        return Err(anyhow!(
            "talon-ops target must define at least one allowed_prefix query parameter"
        ));
    }

    Ok(TalonOpsPolicy {
        allowed_namespace_prefixes,
        allow_session_messages: allow_session_messages.unwrap_or(false),
        allow_step_payloads: allow_step_payloads.unwrap_or(false),
        max_list_limit: max_list_limit.unwrap_or(DEFAULT_MAX_LIST_LIMIT),
        max_history_lookback_seconds: max_history_lookback_seconds
            .unwrap_or(DEFAULT_MAX_HISTORY_LOOKBACK_SECONDS),
    })
}

fn ensure_singleton(seen: &mut HashSet<&'static str>, key: &'static str) -> Result<()> {
    if !seen.insert(key) {
        return Err(anyhow!(
            "talon-ops target query parameter '{}' may only be specified once",
            key
        ));
    }
    Ok(())
}

fn parse_bool_query_param(key: &str, value: &str) -> Result<bool> {
    match value {
        "1" => Ok(true),
        "0" => Ok(false),
        _ => Err(anyhow!(
            "talon-ops target query parameter '{}' must be 0 or 1",
            key
        )),
    }
}

fn parse_non_negative_i32_query_param(key: &str, value: &str) -> Result<i32> {
    let parsed = value.parse::<i32>().map_err(|_| {
        anyhow!(
            "talon-ops target query parameter '{}' must be an integer",
            key
        )
    })?;
    if parsed < 0 {
        return Err(anyhow!(
            "talon-ops target query parameter '{}' must be non-negative",
            key
        ));
    }
    Ok(parsed)
}

fn parse_mcp_auth_broker_claims(
    raw_auth_header: &str,
) -> std::result::Result<McpAuthBrokerClaims, String> {
    let token = bearer_token(raw_auth_header)?;
    decode_claims(token, MCP_AUTH_BROKER_AUDIENCE).and_then(|claims: McpAuthBrokerClaims| {
        if claims.namespace.trim().is_empty() || claims.binding_name.trim().is_empty() {
            Err("missing namespace or binding claim".to_string())
        } else {
            Ok(claims)
        }
    })
}

fn parse_talon_ops_access_claims(
    raw_auth_header: &str,
) -> std::result::Result<TalonOpsAccessClaims, String> {
    let token = bearer_token(raw_auth_header)?;
    decode_claims(token, TALON_OPS_AUDIENCE).and_then(|claims: TalonOpsAccessClaims| {
        if claims.namespace.trim().is_empty() || claims.binding_name.trim().is_empty() {
            Err("missing namespace or binding claim".to_string())
        } else {
            Ok(claims)
        }
    })
}

fn bearer_token(raw_auth_header: &str) -> std::result::Result<&str, String> {
    raw_auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| "missing bearer token".to_string())
}

fn decode_claims<T>(token: &str, audience: &str) -> std::result::Result<T, String>
where
    T: for<'de> Deserialize<'de> + Clone,
{
    crate::security::install_jwt_crypto_provider();
    let secret = talon_jwt_secret().ok_or_else(|| "missing talon jwt secret".to_string())?;
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&[audience]);
    decode::<T>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|error| format!("invalid token: {error}"))
}

fn mint_talon_ops_access_token(
    namespace: &str,
    binding_name: &str,
    agent_name: Option<&str>,
    expires_at_unix: i64,
) -> Result<String> {
    crate::security::install_jwt_crypto_provider();
    let secret = talon_jwt_secret().ok_or_else(|| anyhow!("missing talon jwt secret"))?;
    let claims = TalonOpsAccessClaims {
        sub: TALON_OPS_SERVER_NAME.to_string(),
        aud: TALON_OPS_AUDIENCE.to_string(),
        exp: expires_at_unix as usize,
        namespace: namespace.to_string(),
        binding_name: binding_name.to_string(),
        agent_name: agent_name.map(str::to_string),
    };
    Ok(encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?)
}

fn talon_jwt_secret() -> Option<String> {
    std::env::var("TALON_JWT_SECRET")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("GATEWAY_JWT_SECRET")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

fn bounded_limit(access: &TalonOpsAccess, requested: Option<usize>) -> usize {
    requested
        .unwrap_or(access.max_list_limit())
        .min(access.max_list_limit())
}

fn require_namespace_access(access: &TalonOpsAccess, namespace: &str) -> Result<(), McpError> {
    if access.allows_namespace(namespace) {
        Ok(())
    } else {
        Err(McpError::invalid_params(
            format!(
                "namespace '{namespace}' is outside binding scope '{}:{}'",
                access.namespace, access.binding_name
            ),
            None,
        ))
    }
}

fn to_json_string(value: &Value) -> Result<String, McpError> {
    serde_json::to_string(value).map_err(internal_mcp_error)
}

fn schedule_json(schedule: &models::Schedule) -> Value {
    let spec = schedule.spec.as_ref();
    let target = spec.and_then(|spec| spec.target.as_ref());
    let status = schedule.status.as_ref();

    json!({
        "name": schedule.name,
        "ns": schedule.ns,
        "labels": schedule.labels,
        "spec": spec.map(|spec| json!({
            "kind": spec.kind,
            "cron": spec.cron,
            "intervalSeconds": spec.interval_seconds,
            "runAt": spec.run_at,
            "timezone": spec.timezone,
            "target": target.map(|target| json!({
                "agent": target.agent,
                "sessionMode": target.session_mode,
                "sessionId": target.session_id,
            })),
            "inputMessage": spec.input_message,
            "enabled": spec.enabled,
        })),
        "status": status.map(|status| json!({
            "revision": status.revision,
            "nextRunAt": status.next_run_at,
            "backendHandle": status.backend_handle,
            "backendArmed": status.backend_armed,
            "lastRunAt": status.last_run_at,
            "lastSessionId": status.last_session_id,
            "lastError": status.last_error,
            "claimedRunAt": status.claimed_run_at,
            "claimExpiresAt": status.claim_expires_at,
            "recentEvents": status.recent_events.iter().map(|event| json!({
                "timestamp": event.timestamp,
                "phase": event.phase,
                "outcome": event.outcome,
                "detail": event.detail,
            })).collect::<Vec<_>>(),
        })),
    })
}

fn internal_mcp_error(error: impl std::fmt::Display) -> McpError {
    McpError::internal_error(error.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_limit, parse_talon_ops_policy_from_target, TalonOpsAccess, TalonOpsPolicy,
        DEFAULT_MAX_HISTORY_LOOKBACK_SECONDS, DEFAULT_MAX_LIST_LIMIT,
    };
    fn access(prefixes: &[&str]) -> TalonOpsAccess {
        TalonOpsAccess {
            namespace: "conic".to_string(),
            binding_name: "talon-ops".to_string(),
            agent_name: Some("cmo".to_string()),
            policy: TalonOpsPolicy {
                allowed_namespace_prefixes: prefixes
                    .iter()
                    .map(|prefix| prefix.to_string())
                    .collect(),
                allow_session_messages: true,
                allow_step_payloads: true,
                max_list_limit: DEFAULT_MAX_LIST_LIMIT,
                max_history_lookback_seconds: DEFAULT_MAX_HISTORY_LOOKBACK_SECONDS,
            },
        }
    }

    #[test]
    fn talon_ops_access_checks_prefix_scope() {
        let access = access(&["conic", "conic:wks:"]);
        assert!(access.allows_namespace("conic"));
        assert!(access.allows_namespace("conic:wks:13"));
        assert!(!access.allows_namespace("default"));
    }

    #[test]
    fn talon_ops_access_uses_default_limits() {
        let access = access(&["conic"]);
        assert_eq!(access.max_list_limit(), DEFAULT_MAX_LIST_LIMIT as usize);
        assert_eq!(
            access.max_history_lookback_seconds(),
            DEFAULT_MAX_HISTORY_LOOKBACK_SECONDS as i64
        );
        assert_eq!(
            bounded_limit(&access, Some(9999)),
            DEFAULT_MAX_LIST_LIMIT as usize
        );
    }

    #[test]
    fn parse_talon_ops_policy_from_target_rejects_unknown_params() {
        let error = parse_talon_ops_policy_from_target(
            "https://worker.useconic.com/mcp/talon-ops?allowed_prefix=conic&wat=1",
        )
        .expect_err("unknown params should fail");

        assert!(error
            .to_string()
            .contains("unsupported talon-ops target query parameter 'wat'"));
    }

    #[test]
    fn parse_talon_ops_policy_from_target_reads_known_params() {
        let policy = parse_talon_ops_policy_from_target(
            "https://worker.useconic.com/mcp/talon-ops?allowed_prefix=conic&allowed_prefix=conic%3Awks%3A&session_messages=1&step_payloads=0&max_limit=25&max_lookback_s=60",
        )
        .expect("policy should parse");

        assert_eq!(
            policy.allowed_namespace_prefixes,
            vec!["conic".to_string(), "conic:wks:".to_string()]
        );
        assert!(policy.allow_session_messages);
        assert!(!policy.allow_step_payloads);
        assert_eq!(policy.max_list_limit, 25);
        assert_eq!(policy.max_history_lookback_seconds, 60);
    }

    #[test]
    fn normalize_schedule_kind_maps_interval_to_every() {
        assert_eq!(crate::scheduling::normalize_schedule_kind("interval"), "every");
        assert_eq!(crate::scheduling::normalize_schedule_kind(" every "), "every");
        assert_eq!(crate::scheduling::normalize_schedule_kind("cron"), "cron");
    }
}
