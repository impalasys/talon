// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

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
        bearer_token, bounded_limit, load_talon_ops_binding, load_talon_ops_policy,
        mint_talon_ops_access_token, parse_bool_query_param, parse_mcp_auth_broker_claims,
        parse_non_negative_i32_query_param, parse_talon_ops_access_claims,
        parse_talon_ops_policy_from_target, require_namespace_access, schedule_json,
        talon_jwt_secret, talon_ops_access_from_parts, talon_ops_access_from_request,
        talon_ops_auth_broker, to_json_string, DeleteScheduleArgs, GetAgentArgs,
        GetScheduleArgs, ListMcpBindingsArgs, ListMcpServersArgs, ListNamespacesArgs,
        ListRecentStepsArgs, ListSchedulesArgs, ListSessionsArgs, McpAuthBrokerClaims,
        McpAuthBrokerRequest, PutScheduleArgs, TalonOpsAccess, TalonOpsAccessClaims,
        TalonOpsPolicy, TalonOpsServer, DEFAULT_MAX_HISTORY_LOOKBACK_SECONDS,
        DEFAULT_MAX_LIST_LIMIT, META_NS,
    };
    use crate::config::Config;
    use crate::control::{
        keys, scheduler::NoopSchedulerBackend, ControlPlane, KeyValueStore, MessagePublisher,
        ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::{manifests, models};
    use crate::worker::{
        mcp_registry::McpRegistry, scheduler_auth::SchedulerRequestAuthenticator,
        WorkerEventHandler,
    };
    use async_trait::async_trait;
    use axum::{
        extract::State,
        http::{header, HeaderMap, HeaderValue, Request, StatusCode},
        response::IntoResponse,
        Json,
    };
    use futures::stream;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use prost::Message;
    use rmcp::handler::server::wrapper::Parameters;
    use serde_json::json;
    use std::{
        collections::HashMap,
        pin::Pin,
        sync::Arc,
    };
    use tokio::sync::Mutex as AsyncMutex;

    #[derive(Default)]
    struct MockKvStore {
        entries: Arc<tokio::sync::RwLock<HashMap<(String, String), Vec<u8>>>>,
    }

    #[async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self
                .entries
                .read()
                .await
                .get(&(namespace.to_string(), key.to_string()))
                .cloned())
        }

        async fn set(&self, namespace: &str, key: &str, value: &[u8]) -> anyhow::Result<()> {
            self.entries
                .write()
                .await
                .insert((namespace.to_string(), key.to_string()), value.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            namespace: &str,
            key: &str,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut entries = self.entries.write().await;
            let current = entries.get(&(namespace.to_string(), key.to_string())).cloned();
            if current.as_deref() == expected {
                entries.insert((namespace.to_string(), key.to_string()), value.to_vec());
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn delete(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
            self.entries
                .write()
                .await
                .remove(&(namespace.to_string(), key.to_string()));
            Ok(())
        }

        async fn list_keys(&self, namespace: &str, prefix: &str) -> anyhow::Result<Vec<String>> {
            Ok(self
                .entries
                .read()
                .await
                .keys()
                .filter(|(ns, key)| ns == namespace && key.starts_with(prefix))
                .map(|(_, key)| key.clone())
                .collect())
        }
    }

    #[derive(Default)]
    struct MockPubSub;

    #[async_trait]
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

    fn env_mutex() -> &'static AsyncMutex<()> {
        crate::test_support::async_env_mutex()
    }

    fn handler_with_kv(kv: Arc<MockKvStore>) -> WorkerEventHandler {
        WorkerEventHandler {
            cp: Arc::new(ControlPlane {
                kv,
                pubsub: Arc::new(MockPubSub),
                scheduler: Arc::new(NoopSchedulerBackend),
            }),
            config: Arc::new(Config::default()),
            mcp_registry: Arc::new(McpRegistry::new()),
            scheduler_authenticator: Arc::new(SchedulerRequestAuthenticator::deny_all()),
            session_cancellations: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }

    async fn seed_talon_ops_binding(kv: &MockKvStore, namespace: &str, binding_name: &str) {
        kv.set_msg(
            crate::control::ns::TALON_SYSTEM,
            &keys::mcp_server("talon-ops"),
            &manifests::McpServer {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "McpServer".to_string(),
                metadata: Some(manifests::ObjectMeta {
                    name: "talon-ops".to_string(),
                    namespace: String::new(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                }),
                spec: Some(manifests::McpServerSpec {
                    transport: "streamable_http".to_string(),
                    target: format!(
                        "https://worker.example.com/mcp/talon-ops?allowed_prefix={namespace}"
                    ),
                    args: Vec::new(),
                    headers: HashMap::new(),
                    disabled: false,
                }),
            },
        )
        .await
        .expect("talon-ops server should persist");
        kv.set_msg(
            namespace,
            &keys::mcp_server_binding(binding_name),
            &manifests::McpServerBinding {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "McpServerBinding".to_string(),
                metadata: Some(manifests::ObjectMeta {
                    name: binding_name.to_string(),
                    namespace: namespace.to_string(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                }),
                spec: Some(manifests::McpServerBindingSpec {
                    server_ref: "talon-ops".to_string(),
                    args: Vec::new(),
                    headers: HashMap::new(),
                    disabled: false,
                    auth_broker: None,
                    allowed_tool_names: Vec::new(),
                }),
            },
        )
        .await
        .expect("binding should persist");
    }

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

    fn parts_with_access(access: TalonOpsAccess) -> axum::http::request::Parts {
        let mut request = Request::builder().uri("/").body(()).unwrap();
        request.extensions_mut().insert(access);
        let (parts, _) = request.into_parts();
        parts
    }

    async fn seed_namespace(kv: &MockKvStore, name: &str, parent: &str) {
        kv.set_msg(
            META_NS,
            &format!("Namespace/{name}"),
            &models::Namespace {
                name: name.to_string(),
                parent: parent.to_string(),
                is_deleted: false,
                deleted_at: 0,
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
    }

    async fn seed_agent(kv: &MockKvStore, namespace: &str, name: &str) {
        kv.set_msg(
            namespace,
            &keys::agent(name),
            &models::Agent {
                name: name.to_string(),
                ns: namespace.to_string(),
                definition: Some(manifests::AgentDefinition {
                    source: Some(manifests::agent_definition::Source::CustomSpec(
                        manifests::AgentSpec {
                            features: Vec::new(),
                            model_policy: Some(manifests::ModelPolicy {
                                profiles: vec![manifests::ModelProfile {
                                    name: "default".to_string(),
                                    model: Some(manifests::Model {
                                        provider: "mock".to_string(),
                                        name: "gpt-5".to_string(),
                                        temperature: 0.0,
                                        thinking: None,
                                    }),
                                }],
                            }),
                            system_prompt: "You are helpful.".to_string(),
                            mcp_server_refs: Vec::new(),
                            capabilities: HashMap::new(),
                        },
                    )),
                }),
                effective_spec: Some(manifests::AgentSpec {
                    features: Vec::new(),
                    model_policy: Some(manifests::ModelPolicy {
                        profiles: vec![manifests::ModelProfile {
                            name: "default".to_string(),
                            model: Some(manifests::Model {
                                provider: "mock".to_string(),
                                name: "gpt-5".to_string(),
                                temperature: 0.0,
                                thinking: None,
                            }),
                        }],
                    }),
                    system_prompt: "You are helpful.".to_string(),
                    mcp_server_refs: Vec::new(),
                    capabilities: HashMap::new(),
                }),
                template_deps: Vec::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
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
            "https://worker.example.com/mcp/talon-ops?allowed_prefix=conic&wat=1",
        )
        .expect_err("unknown params should fail");

        assert!(error
            .to_string()
            .contains("unsupported talon-ops target query parameter 'wat'"));
    }

    #[test]
    fn parse_talon_ops_policy_from_target_reads_known_params() {
        let policy = parse_talon_ops_policy_from_target(
            "https://worker.example.com/mcp/talon-ops?allowed_prefix=conic&allowed_prefix=conic%3Awks%3A&session_messages=1&step_payloads=0&max_limit=25&max_lookback_s=60",
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
    fn parse_talon_ops_policy_from_target_rejects_invalid_values_and_duplicates() {
        let duplicate = parse_talon_ops_policy_from_target(
            "https://worker.example.com/mcp/talon-ops?allowed_prefix=conic&session_messages=1&session_messages=0",
        )
        .expect_err("duplicate singleton params should fail");
        assert!(duplicate.to_string().contains("may only be specified once"));

        let invalid_bool = parse_talon_ops_policy_from_target(
            "https://worker.example.com/mcp/talon-ops?allowed_prefix=conic&session_messages=yes",
        )
        .expect_err("invalid boolean should fail");
        assert!(invalid_bool.to_string().contains("must be 0 or 1"));

        let invalid_int = parse_talon_ops_policy_from_target(
            "https://worker.example.com/mcp/talon-ops?allowed_prefix=conic&max_limit=-1",
        )
        .expect_err("negative integers should fail");
        assert!(invalid_int.to_string().contains("must be non-negative"));

        let missing_prefix = parse_talon_ops_policy_from_target(
            "https://worker.example.com/mcp/talon-ops?session_messages=1",
        )
        .expect_err("missing allowed_prefix should fail");
        assert!(missing_prefix
            .to_string()
            .contains("must define at least one allowed_prefix"));
    }

    #[test]
    fn parse_talon_ops_policy_from_target_uses_defaults_when_optionals_absent() {
        let policy = parse_talon_ops_policy_from_target(
            "https://worker.example.com/mcp/talon-ops?allowed_prefix=conic",
        )
        .expect("policy should parse");

        assert_eq!(policy.allowed_namespace_prefixes, vec!["conic".to_string()]);
        assert!(!policy.allow_session_messages);
        assert!(!policy.allow_step_payloads);
        assert_eq!(policy.max_list_limit, DEFAULT_MAX_LIST_LIMIT);
        assert_eq!(
            policy.max_history_lookback_seconds,
            DEFAULT_MAX_HISTORY_LOOKBACK_SECONDS
        );
    }

    #[test]
    fn normalize_schedule_kind_maps_interval_to_every() {
        assert_eq!(crate::scheduling::normalize_schedule_kind("interval"), "every");
        assert_eq!(crate::scheduling::normalize_schedule_kind(" every "), "every");
        assert_eq!(crate::scheduling::normalize_schedule_kind("cron"), "cron");
    }

    #[test]
    fn talon_ops_access_uses_configured_limits_and_bounded_limit() {
        let access = TalonOpsAccess {
            namespace: "conic".to_string(),
            binding_name: "talon-ops".to_string(),
            agent_name: None,
            policy: TalonOpsPolicy {
                allowed_namespace_prefixes: vec!["conic".to_string()],
                allow_session_messages: false,
                allow_step_payloads: false,
                max_list_limit: 12,
                max_history_lookback_seconds: 42,
            },
        };

        assert_eq!(access.max_list_limit(), 12);
        assert_eq!(access.max_history_lookback_seconds(), 42);
        assert_eq!(bounded_limit(&access, None), 12);
        assert_eq!(bounded_limit(&access, Some(5)), 5);
        assert_eq!(bounded_limit(&access, Some(99)), 12);
    }

    #[test]
    fn bearer_token_requires_bearer_prefix() {
        assert_eq!(
            bearer_token("Bearer abc123").expect("token should parse"),
            "abc123"
        );
        assert_eq!(
            bearer_token("Basic abc123").expect_err("non-bearer should fail"),
            "missing bearer token"
        );
    }

    #[test]
    fn query_param_parsers_accept_valid_values_and_reject_invalid_ones() {
        assert!(parse_bool_query_param("enabled", "1").unwrap());
        assert!(!parse_bool_query_param("enabled", "0").unwrap());
        assert!(parse_bool_query_param("enabled", "true").is_err());

        assert_eq!(parse_non_negative_i32_query_param("limit", "0").unwrap(), 0);
        assert_eq!(parse_non_negative_i32_query_param("limit", "42").unwrap(), 42);
        assert!(parse_non_negative_i32_query_param("limit", "-1").is_err());
        assert!(parse_non_negative_i32_query_param("limit", "abc").is_err());
    }

    #[test]
    fn talon_jwt_secret_prefers_talon_and_falls_back_to_gateway() {
        let _guard = env_mutex().blocking_lock();
        unsafe {
            std::env::remove_var("TALON_JWT_SECRET");
            std::env::remove_var("GATEWAY_JWT_SECRET");
        }
        assert!(talon_jwt_secret().is_none());

        unsafe {
            std::env::set_var("GATEWAY_JWT_SECRET", "gateway-secret");
        }
        assert_eq!(talon_jwt_secret().as_deref(), Some("gateway-secret"));

        unsafe {
            std::env::set_var("TALON_JWT_SECRET", "talon-secret");
        }
        assert_eq!(talon_jwt_secret().as_deref(), Some("talon-secret"));

        unsafe {
            std::env::set_var("TALON_JWT_SECRET", "   ");
        }
        assert_eq!(talon_jwt_secret().as_deref(), Some("gateway-secret"));

        unsafe {
            std::env::remove_var("TALON_JWT_SECRET");
            std::env::remove_var("GATEWAY_JWT_SECRET");
        }
    }

    #[test]
    fn access_and_auth_broker_claims_round_trip_from_minted_tokens() {
        let _guard = env_mutex().blocking_lock();
        unsafe {
            std::env::set_var("TALON_JWT_SECRET", "secret-for-tests");
        }

        let access_token = mint_talon_ops_access_token("conic", "talon-ops", Some("cmo"), 4_102_444_800)
            .expect("access token should mint");
        let access_claims =
            parse_talon_ops_access_claims(&format!("Bearer {access_token}")).expect("claims should parse");
        assert_eq!(access_claims.namespace, "conic");
        assert_eq!(access_claims.binding_name, "talon-ops");
        assert_eq!(access_claims.agent_name.as_deref(), Some("cmo"));

        let broker_claims_token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &super::McpAuthBrokerClaims {
                sub: "talon-ops".to_string(),
                aud: "conic-mcp-auth-broker".to_string(),
                exp: 4_102_444_800usize,
                namespace: "conic".to_string(),
                binding_name: "talon-ops".to_string(),
                agent_name: None,
            },
            &jsonwebtoken::EncodingKey::from_secret("secret-for-tests".as_bytes()),
        )
        .expect("broker token should mint");
        let broker_claims = parse_mcp_auth_broker_claims(&format!("Bearer {broker_claims_token}"))
            .expect("broker claims should parse");
        assert_eq!(broker_claims.namespace, "conic");
        assert_eq!(broker_claims.binding_name, "talon-ops");
        assert!(broker_claims.agent_name.is_none());

        let invalid = parse_talon_ops_access_claims("Bearer definitely-not-a-jwt")
            .expect_err("invalid token should fail");
        assert!(invalid.contains("invalid token"));

        unsafe {
            std::env::remove_var("TALON_JWT_SECRET");
        }
    }

    #[test]
    fn claim_parsers_reject_blank_namespace_or_binding() {
        let _guard = env_mutex().blocking_lock();
        unsafe {
            std::env::set_var("TALON_JWT_SECRET", "secret-for-tests");
        }

        let access_token = encode(
            &Header::default(),
            &TalonOpsAccessClaims {
                sub: "talon-ops".to_string(),
                aud: "talon-ops".to_string(),
                exp: 4_102_444_800usize,
                namespace: " ".to_string(),
                binding_name: "talon-ops".to_string(),
                agent_name: None,
            },
            &EncodingKey::from_secret("secret-for-tests".as_bytes()),
        )
        .expect("access token should mint");
        assert!(
            parse_talon_ops_access_claims(&format!("Bearer {access_token}"))
                .expect_err("blank namespace should fail")
                .contains("missing namespace or binding claim")
        );

        let broker_token = encode(
            &Header::default(),
            &McpAuthBrokerClaims {
                sub: "talon-ops".to_string(),
                aud: "conic-mcp-auth-broker".to_string(),
                exp: 4_102_444_800usize,
                namespace: "conic".to_string(),
                binding_name: " ".to_string(),
                agent_name: None,
            },
            &EncodingKey::from_secret("secret-for-tests".as_bytes()),
        )
        .expect("broker token should mint");
        assert!(
            parse_mcp_auth_broker_claims(&format!("Bearer {broker_token}"))
                .expect_err("blank binding should fail")
                .contains("missing namespace or binding claim")
        );

        unsafe {
            std::env::remove_var("TALON_JWT_SECRET");
        }
    }

    #[test]
    fn talon_ops_access_from_parts_requires_extension() {
        let request = Request::builder().uri("/").body(()).unwrap();
        let (parts, _) = request.into_parts();
        let error = talon_ops_access_from_parts(&parts).expect_err("missing extension should fail");
        assert!(format!("{error:?}").contains("missing extension"));

        let mut request = Request::builder().uri("/").body(()).unwrap();
        request.extensions_mut().insert(access(&["conic"]));
        let (parts, _) = request.into_parts();
        let extracted = talon_ops_access_from_parts(&parts).expect("extension should load");
        assert_eq!(extracted.namespace, "conic");
    }

    #[test]
    fn to_json_string_serializes_objects() {
        assert_eq!(
            to_json_string(&json!({"ok": true, "count": 2})).unwrap(),
            r#"{"count":2,"ok":true}"#
        );
    }

    #[test]
    fn require_namespace_access_rejects_out_of_scope_namespace() {
        let access = access(&["conic", "conic:wks:"]);
        require_namespace_access(&access, "conic:wks:1").expect("namespace should be allowed");
        let error = require_namespace_access(&access, "default")
            .expect_err("out of scope namespace should fail");
        assert!(format!("{error:?}").contains("outside binding scope"));
    }

    #[test]
    fn schedule_json_includes_target_and_status_details() {
        let schedule = models::Schedule {
            name: "nightly".to_string(),
            ns: "conic".to_string(),
            labels: HashMap::from([("tier".to_string(), "prod".to_string())]),
            spec: Some(models::ScheduleSpec {
                kind: "cron".to_string(),
                cron: "0 0 * * *".to_string(),
                interval_seconds: 0,
                run_at: String::new(),
                timezone: "UTC".to_string(),
                target: Some(models::ScheduleTarget {
                    agent: "ctl".to_string(),
                    session_mode: "new".to_string(),
                    session_id: String::new(),
                }),
                input_message: "ping".to_string(),
                enabled: true,
            }),
            status: Some(models::ScheduleStatus {
                revision: 7,
                next_run_at: Some(111),
                backend_handle: Some("handle-1".to_string()),
                backend_armed: true,
                last_run_at: Some(101),
                last_session_id: Some("session-1".to_string()),
                last_error: Some("none".to_string()),
                claimed_run_at: Some(0),
                claim_expires_at: Some(0),
                recent_events: vec![models::ScheduleEvent {
                    timestamp: 99,
                    phase: "armed".to_string(),
                    outcome: "ok".to_string(),
                    detail: "scheduled".to_string(),
                }],
            }),
        };

        let json = schedule_json(&schedule);
        assert_eq!(json["name"], "nightly");
        assert_eq!(json["spec"]["target"]["agent"], "ctl");
        assert_eq!(json["status"]["backendHandle"], "handle-1");
        assert_eq!(json["status"]["recentEvents"][0]["phase"], "armed");
    }

    #[tokio::test]
    async fn load_talon_ops_policy_and_binding_validate_kv_records() {
        let kv = MockKvStore::default();
        kv.set_msg(
            crate::control::ns::TALON_SYSTEM,
            &keys::mcp_server("talon-ops"),
            &manifests::McpServer {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "McpServer".to_string(),
                metadata: Some(manifests::ObjectMeta {
                    name: "talon-ops".to_string(),
                    namespace: String::new(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                }),
                spec: Some(manifests::McpServerSpec {
                    transport: "streamable_http".to_string(),
                    target: "https://worker.example.com/mcp/talon-ops?allowed_prefix=conic&session_messages=1".to_string(),
                    args: Vec::new(),
                    headers: HashMap::new(),
                    disabled: false,
                }),
            },
        )
        .await
        .expect("talon-ops server should persist");
        kv.set_msg(
            "conic",
            &keys::mcp_server_binding("talon-ops"),
            &manifests::McpServerBinding {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "McpServerBinding".to_string(),
                metadata: Some(manifests::ObjectMeta {
                    name: "talon-ops".to_string(),
                    namespace: "conic".to_string(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                }),
                spec: Some(manifests::McpServerBindingSpec {
                    server_ref: "talon-ops".to_string(),
                    args: Vec::new(),
                    headers: HashMap::new(),
                    disabled: false,
                    auth_broker: None,
                    allowed_tool_names: Vec::new(),
                }),
            },
        )
        .await
        .expect("binding should persist");

        let policy = load_talon_ops_policy(&kv).await.expect("policy should load");
        assert_eq!(policy.allowed_namespace_prefixes, vec!["conic".to_string()]);
        assert!(policy.allow_session_messages);

        let access = load_talon_ops_binding(&kv, "conic", "talon-ops", Some("ctl"))
            .await
            .expect("binding should load");
        assert_eq!(access.namespace, "conic");
        assert_eq!(access.binding_name, "talon-ops");
        assert_eq!(access.agent_name.as_deref(), Some("ctl"));
    }

    #[tokio::test]
    async fn load_talon_ops_binding_rejects_missing_or_wrong_server_binding() {
        let kv = MockKvStore::default();
        kv.set_msg(
            crate::control::ns::TALON_SYSTEM,
            &keys::mcp_server("talon-ops"),
            &manifests::McpServer {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "McpServer".to_string(),
                metadata: Some(manifests::ObjectMeta {
                    name: "talon-ops".to_string(),
                    namespace: String::new(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                }),
                spec: Some(manifests::McpServerSpec {
                    transport: "streamable_http".to_string(),
                    target: "https://worker.example.com/mcp/talon-ops?allowed_prefix=conic".to_string(),
                    args: Vec::new(),
                    headers: HashMap::new(),
                    disabled: false,
                }),
            },
        )
        .await
        .expect("talon-ops server should persist");

        let missing = load_talon_ops_binding(&kv, "conic", "talon-ops", None)
            .await
            .expect_err("missing binding should fail");
        assert!(missing.to_string().contains("not found"));

        kv.set_msg(
            "conic",
            &keys::mcp_server_binding("wrong"),
            &manifests::McpServerBinding {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "McpServerBinding".to_string(),
                metadata: Some(manifests::ObjectMeta {
                    name: "wrong".to_string(),
                    namespace: "conic".to_string(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                }),
                spec: Some(manifests::McpServerBindingSpec {
                    server_ref: "github".to_string(),
                    args: Vec::new(),
                    headers: HashMap::new(),
                    disabled: false,
                    auth_broker: None,
                    allowed_tool_names: Vec::new(),
                }),
            },
        )
        .await
        .expect("wrong binding should persist");

        let wrong = load_talon_ops_binding(&kv, "conic", "wrong", None)
            .await
            .expect_err("wrong server ref should fail");
        assert!(wrong.to_string().contains("does not reference talon-ops"));
    }

    #[tokio::test]
    async fn talon_ops_access_from_request_checks_header_and_binding() {
        let kv = Arc::new(MockKvStore::default());
        seed_talon_ops_binding(kv.as_ref(), "conic", "talon-ops").await;
        let handler = handler_with_kv(kv);

        let missing = talon_ops_access_from_request(&handler, None)
            .await
            .expect_err("missing header should fail");
        assert_eq!(missing.0, StatusCode::UNAUTHORIZED);

        let _guard = env_mutex().lock().await;
        unsafe {
            std::env::set_var("TALON_JWT_SECRET", "secret-for-tests");
        }
        let token = mint_talon_ops_access_token("conic", "talon-ops", Some("ctl"), 4_102_444_800)
            .expect("access token should mint");
        let header = HeaderValue::from_str(&format!("Bearer {token}")).unwrap();
        let access = talon_ops_access_from_request(&handler, Some(&header))
            .await
            .expect("binding should load");
        assert_eq!(access.namespace, "conic");
        assert_eq!(access.binding_name, "talon-ops");
        assert_eq!(access.agent_name.as_deref(), Some("ctl"));

        let invalid = HeaderValue::from_static("Bearer bad-token");
        let invalid_error = talon_ops_access_from_request(&handler, Some(&invalid))
            .await
            .expect_err("invalid token should fail");
        assert_eq!(invalid_error.0, StatusCode::UNAUTHORIZED);

        unsafe {
            std::env::remove_var("TALON_JWT_SECRET");
        }
    }

    #[tokio::test]
    async fn talon_ops_auth_broker_validates_request_and_mints_token() {
        let kv = Arc::new(MockKvStore::default());
        seed_talon_ops_binding(kv.as_ref(), "conic", "talon-ops").await;
        let handler = handler_with_kv(kv);
        let _guard = env_mutex().lock().await;
        unsafe {
            std::env::set_var("TALON_JWT_SECRET", "secret-for-tests");
        }

        let broker_claims_token = encode(
            &Header::default(),
            &McpAuthBrokerClaims {
                sub: "talon-ops".to_string(),
                aud: "conic-mcp-auth-broker".to_string(),
                exp: 4_102_444_800usize,
                namespace: "conic".to_string(),
                binding_name: "talon-ops".to_string(),
                agent_name: Some("ctl".to_string()),
            },
            &EncodingKey::from_secret("secret-for-tests".as_bytes()),
        )
        .expect("broker token should mint");
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {broker_claims_token}")).unwrap(),
        );

        let mismatched = talon_ops_auth_broker(
            State(handler.clone()),
            headers.clone(),
            Json(McpAuthBrokerRequest {
                namespace: "other".to_string(),
                binding_name: "talon-ops".to_string(),
                server_ref: None,
                agent_name: Some("ctl".to_string()),
                audience: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(mismatched.status(), StatusCode::FORBIDDEN);

        let unsupported_server = talon_ops_auth_broker(
            State(handler.clone()),
            headers.clone(),
            Json(McpAuthBrokerRequest {
                namespace: "conic".to_string(),
                binding_name: "talon-ops".to_string(),
                server_ref: Some("github".to_string()),
                agent_name: Some("ctl".to_string()),
                audience: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(unsupported_server.status(), StatusCode::BAD_REQUEST);

        let response = talon_ops_auth_broker(
            State(handler),
            headers,
            Json(McpAuthBrokerRequest {
                namespace: "conic".to_string(),
                binding_name: "talon-ops".to_string(),
                server_ref: Some("talon-ops".to_string()),
                agent_name: Some("ctl".to_string()),
                audience: Some("talon-ops".to_string()),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        unsafe {
            std::env::remove_var("TALON_JWT_SECRET");
        }
    }

    #[tokio::test]
    async fn talon_ops_server_lists_visible_resources_and_filters_sessions() {
        let kv = Arc::new(MockKvStore::default());
        seed_talon_ops_binding(kv.as_ref(), "conic", "talon-ops").await;
        seed_namespace(kv.as_ref(), "conic", "").await;
        seed_namespace(kv.as_ref(), "conic:child", "conic").await;
        seed_namespace(kv.as_ref(), "default", "").await;
        seed_agent(kv.as_ref(), "conic", "alpha").await;
        seed_agent(kv.as_ref(), "conic", "beta").await;
        seed_agent(kv.as_ref(), "default", "hidden").await;

        kv.set_msg(
            "conic",
            &keys::session("alpha", "session-old"),
            &models::Session {
                id: "session-old".to_string(),
                agent: "alpha".to_string(),
                ns: "conic".to_string(),
                status: "IDLE".to_string(),
                created_at: 10,
                last_active: 100,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            "conic",
            &keys::session("beta", "session-new"),
            &models::Session {
                id: "session-new".to_string(),
                agent: "beta".to_string(),
                ns: "conic".to_string(),
                status: "PROCESSING".to_string(),
                created_at: 20,
                last_active: 200,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            "conic",
            &keys::session_message("beta", "session-new", "msg-1"),
            &models::SessionMessage {
                id: "msg-1".to_string(),
                role: 1,
                content: "hello".to_string(),
                created_at: 150,
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set(
            "conic",
            &keys::session_message_step("beta", "session-new", "msg-1", "step-1"),
            &crate::control::events::SessionStepEvent {
                session_id: "session-new".to_string(),
                step_type: crate::control::events::StepType::Action as i32,
                content: "tool".to_string(),
                timestamp: 175,
                agent: "beta".to_string(),
                ns: "conic".to_string(),
                message_id: "msg-1".to_string(),
                name: "search".to_string(),
                payload_json: "{\"q\":\"talon\"}".to_string(),
            }
            .encode_to_vec(),
        )
        .await
        .unwrap();

        kv.set_msg(
            "conic",
            &keys::mcp_server_binding("talon-ops"),
            &manifests::McpServerBinding {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "McpServerBinding".to_string(),
                metadata: Some(manifests::ObjectMeta {
                    name: "talon-ops".to_string(),
                    namespace: "conic".to_string(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                }),
                spec: Some(manifests::McpServerBindingSpec {
                    server_ref: "talon-ops".to_string(),
                    args: Vec::new(),
                    headers: HashMap::new(),
                    disabled: false,
                    auth_broker: None,
                    allowed_tool_names: Vec::new(),
                }),
            },
        )
        .await
        .unwrap();

        let server = TalonOpsServer::new(handler_with_kv(kv));
        let parts = parts_with_access(access(&["conic"]));

        let namespaces: String = server
            .list_namespaces(
                rmcp::handler::server::common::Extension(parts.clone()),
                Parameters(ListNamespacesArgs {
                    parent: None,
                    prefix: Some("conic".to_string()),
                    limit: Some(10),
                }),
            )
            .await
            .unwrap();
        let namespaces_json: serde_json::Value = serde_json::from_str(&namespaces).unwrap();
        assert_eq!(namespaces_json["namespaces"].as_array().unwrap().len(), 2);

        let agent: String = server
            .get_agent(
                rmcp::handler::server::common::Extension(parts.clone()),
                Parameters(GetAgentArgs {
                    namespace: "conic".to_string(),
                    name: "alpha".to_string(),
                }),
            )
            .await
            .unwrap();
        assert!(agent.contains("alpha"));

        let sessions: String = server
            .list_sessions(
                rmcp::handler::server::common::Extension(parts.clone()),
                Parameters(ListSessionsArgs {
                    namespace: "conic".to_string(),
                    agent: None,
                    state: Some("PROCESSING".to_string()),
                    limit: Some(10),
                    updated_since: Some(150),
                }),
            )
            .await
            .unwrap();
        let sessions_json: serde_json::Value = serde_json::from_str(&sessions).unwrap();
        assert_eq!(sessions_json["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(sessions_json["sessions"][0]["id"], "session-new");

        let recent_steps: String = server
            .list_recent_steps(
                rmcp::handler::server::common::Extension(parts.clone()),
                Parameters(ListRecentStepsArgs {
                    namespace: "conic".to_string(),
                    agent: Some("beta".to_string()),
                    session_id: Some("session-new".to_string()),
                    limit: Some(10),
                    since: Some(150),
                }),
            )
            .await
            .unwrap();
        let steps_json: serde_json::Value = serde_json::from_str(&recent_steps).unwrap();
        assert_eq!(steps_json["steps"].as_array().unwrap().len(), 1);
        assert_eq!(steps_json["steps"][0]["name"], "search");

        let bindings: String = server
            .list_mcp_bindings(
                rmcp::handler::server::common::Extension(parts.clone()),
                Parameters(ListMcpBindingsArgs {
                    namespace: "conic".to_string(),
                    limit: Some(10),
                }),
            )
            .await
            .unwrap();
        let bindings_json: serde_json::Value = serde_json::from_str(&bindings).unwrap();
        assert_eq!(bindings_json["bindings"].as_array().unwrap().len(), 1);

        let servers: String = server
            .list_mcp_servers(Parameters(ListMcpServersArgs { limit: Some(10) }))
            .await
            .unwrap();
        let servers_json: serde_json::Value = serde_json::from_str(&servers).unwrap();
        assert_eq!(servers_json["servers"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn talon_ops_server_manages_schedule_lifecycle() {
        let kv = Arc::new(MockKvStore::default());
        let server = TalonOpsServer::new(handler_with_kv(kv.clone()));
        let parts = parts_with_access(access(&["conic"]));

        let created: String = server
            .create_schedule(
                rmcp::handler::server::common::Extension(parts.clone()),
                Parameters(PutScheduleArgs {
                    namespace: "conic".to_string(),
                    name: "nightly".to_string(),
                    labels: Some(HashMap::from([("tier".to_string(), "prod".to_string())])),
                    kind: "every".to_string(),
                    cron: None,
                    interval_seconds: Some(3600),
                    run_at: None,
                    timezone: Some("UTC".to_string()),
                    agent: "alpha".to_string(),
                    session_mode: Some("new".to_string()),
                    session_id: None,
                    input_message: "ping".to_string(),
                    enabled: Some(true),
                }),
            )
            .await
            .unwrap();
        let created_json: serde_json::Value = serde_json::from_str(&created).unwrap();
        assert_eq!(created_json["schedule"]["name"], "nightly");
        assert_eq!(created_json["schedule"]["spec"]["target"]["agent"], "alpha");

        let listed: String = server
            .list_schedules(
                rmcp::handler::server::common::Extension(parts.clone()),
                Parameters(ListSchedulesArgs {
                    namespace: "conic".to_string(),
                    agent: Some("alpha".to_string()),
                    enabled: Some(true),
                    limit: Some(10),
                }),
            )
            .await
            .unwrap();
        let listed_json: serde_json::Value = serde_json::from_str(&listed).unwrap();
        assert_eq!(listed_json["schedules"].as_array().unwrap().len(), 1);

        let updated: String = server
            .update_schedule(
                rmcp::handler::server::common::Extension(parts.clone()),
                Parameters(PutScheduleArgs {
                    namespace: "conic".to_string(),
                    name: "nightly".to_string(),
                    labels: None,
                    kind: "".to_string(),
                    cron: None,
                    interval_seconds: Some(7200),
                    run_at: None,
                    timezone: None,
                    agent: "".to_string(),
                    session_mode: Some("reuse".to_string()),
                    session_id: Some("session-1".to_string()),
                    input_message: "".to_string(),
                    enabled: Some(false),
                }),
            )
            .await
            .unwrap();
        let updated_json: serde_json::Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(updated_json["schedule"]["spec"]["intervalSeconds"], 7200);
        assert_eq!(updated_json["schedule"]["spec"]["enabled"], false);

        let fetched: String = server
            .get_schedule(
                rmcp::handler::server::common::Extension(parts.clone()),
                Parameters(GetScheduleArgs {
                    namespace: "conic".to_string(),
                    name: "nightly".to_string(),
                }),
            )
            .await
            .unwrap();
        let fetched_json: serde_json::Value = serde_json::from_str(&fetched).unwrap();
        assert_eq!(fetched_json["schedule"]["name"], "nightly");

        let deleted: String = server
            .delete_schedule(
                rmcp::handler::server::common::Extension(parts),
                Parameters(DeleteScheduleArgs {
                    namespace: "conic".to_string(),
                    name: "nightly".to_string(),
                }),
            )
            .await
            .unwrap();
        let deleted_json: serde_json::Value = serde_json::from_str(&deleted).unwrap();
        assert_eq!(deleted_json["deleted"], true);
    }
}
