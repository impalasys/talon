// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use prost::Message;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use crate::control::resource_model::{self, TypedResource};
use crate::control::resources::ResourceStore;
use crate::control::scheduling;
use crate::control::{delegation, keys, ControlPlane, ListOptions, ProtoKeyValueStoreExt};
use crate::gateway::rpc::{
    data_proto, manifests, protobuf_value::value::Kind as ProtoValueKind, resources_proto,
};
use crate::harness::skills::namespace::{self, NamespaceSkill};
use crate::harness::skills::registry::ToolRegistry;

#[path = "tools/a2a.rs"]
mod a2a_tools;
#[path = "tools/artifacts.rs"]
mod artifact_tools;
#[path = "tools/tasks.rs"]
mod task_tools;

pub const CREATE_SCHEDULE_TOOL: &str = "create_schedule";
pub const GET_SCHEDULE_TOOL: &str = "get_schedule";
pub const LIST_SCHEDULES_TOOL: &str = "list_schedules";
pub const UPDATE_SCHEDULE_TOOL: &str = "update_schedule";
pub const DELETE_SCHEDULE_TOOL: &str = "delete_schedule";
pub const CREATE_TASK_TOOL: &str = "create_task";
pub const DELEGATE_TASK_TOOL: &str = "delegate_task";
pub const AGENT_OPEN_TOOL: &str = "agent_open";
pub const AGENT_SEND_TOOL: &str = "agent_send";
pub const AGENT_STATUS_TOOL: &str = "agent_status";
pub const GET_TASK_TOOL: &str = "get_task";
pub const LIST_TASKS_TOOL: &str = "list_tasks";
pub const UPDATE_TASK_TOOL: &str = "update_task";
pub const READ_SESSION_MESSAGES_TOOL: &str = "read_session_messages";
pub const CREATE_GOAL_TOOL: &str = "create_goal";
pub const GET_GOAL_TOOL: &str = "get_goal";
pub const LIST_GOALS_TOOL: &str = "list_goals";
pub const UPDATE_GOAL_TOOL: &str = "update_goal";
pub const COMPLETE_GOAL_TOOL: &str = "complete_goal";
pub const BLOCK_GOAL_TOOL: &str = "block_goal";
pub const CHANNEL_PUBLISH_TOOL: &str = "channel_publish";
pub const CHANNEL_SKIP_REPLY_TOOL: &str = "channel_skip_reply";
pub const ACTIVATE_SKILL_TOOL: &str = "activate_skill";
pub const CREATE_ARTIFACT_TOOL: &str = "create_artifact";
pub const UPDATE_ARTIFACT_TOOL: &str = "update_artifact";
pub const READ_ARTIFACT_TOOL: &str = "read_artifact";
pub const GET_ARTIFACT_METADATA_TOOL: &str = "get_artifact_metadata";
pub const GRANT_ARTIFACT_TOOL: &str = "grant_artifact";
pub const FETCH_URL_TOOL: &str = "fetch_url";
pub const WEB_SEARCH_TOOL: &str = "web_search";
pub const SEARCH_MEMORY_TOOL: &str = "search_memory";
pub const READ_MEMORY_TOOL: &str = "read_memory";
pub const LIST_MEMORY_TOOL: &str = "list_memory";
pub const CREATE_MEMORY_TOOL: &str = "create_memory";
pub const UPDATE_MEMORY_TOOL: &str = "update_memory";

pub(super) const OP_READ: &str = "read";
pub(super) const OP_METADATA: &str = "metadata";
pub(super) const OP_PROMOTE: &str = "promote";
const MAX_ACCESS_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;

pub fn register_skill_tools(registry: &mut ToolRegistry, skills: &[NamespaceSkill]) {
    let names = namespace::effective_skill_names(skills);
    if names.is_empty() {
        return;
    }

    registry.register_builtin(
        ACTIVATE_SKILL_TOOL,
        "Load the full instructions for an available namespace skill before applying it.",
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Available skill name to activate.",
                    "enum": names
                }
            },
            "required": ["name"]
        }),
    );
}

pub fn register_channel_tools(registry: &mut ToolRegistry) {
    registry.register_builtin(
        CHANNEL_PUBLISH_TOOL,
        "Publish a public response to the channel that triggered this session. Normal assistant text remains private; use this tool for channel-visible replies.",
        json!({
            "type": "object",
            "properties": {
                "content": { "type": "string", "description": "Public channel response content." }
            },
            "required": ["content"]
        }),
    );
    registry.register_builtin(
        CHANNEL_SKIP_REPLY_TOOL,
        "Mark this channel-triggered session as not needing a public channel reply.",
        json!({
            "type": "object",
            "properties": {
                "reason": { "type": "string", "description": "Optional private reason for skipping a channel reply." }
            }
        }),
    );
}

pub fn register_tools(registry: &mut ToolRegistry, spec: &manifests::AgentSpec) {
    artifact_tools::register(registry);
    a2a_tools::register(registry, spec);
    register_research_tools(registry, spec);

    if !has_capability_action(spec, "schedules", "inspect")
        && !has_capability_action(spec, "schedules", "create")
        && !has_capability_action(spec, "schedules", "update")
        && !has_capability_action(spec, "schedules", "delete")
        && !has_capability_action(spec, "tasks", "inspect")
        && !has_capability_action(spec, "tasks", "create")
        && !has_capability_action(spec, "tasks", "update")
        && !has_capability_action(spec, "sessions", "read:messages")
        && !has_capability_action(spec, "memory", "inspect")
        && !has_capability_action(spec, "memory", "read")
        && !has_capability_action(spec, "memory", "create")
        && !has_capability_action(spec, "memory", "update")
        && !has_capability_action(spec, "goals", "inspect")
        && !has_capability_action(spec, "goals", "create")
        && !has_capability_action(spec, "goals", "update")
    {
        return;
    }

    if has_capability_action(spec, "schedules", "inspect") {
        registry.register_builtin(
            LIST_SCHEDULES_TOOL,
            "List schedules in a namespace. Use this to inspect existing schedule configuration and status.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Namespace to inspect. Defaults to the current agent namespace if omitted." },
                    "agent": { "type": "string", "description": "Optional target agent filter." },
                    "enabled": { "type": "boolean", "description": "Optional enabled-state filter." },
                    "limit": { "type": "integer", "description": "Optional maximum number of results to return." }
                }
            }),
        );
        registry.register_builtin(
            GET_SCHEDULE_TOOL,
            "Get a single schedule and its runtime status.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Namespace containing the schedule. Defaults to the current agent namespace if omitted." },
                    "name": { "type": "string", "description": "Schedule name." }
                },
                "required": ["name"]
            }),
        );
    }

    if has_capability_action(spec, "schedules", "create") {
        registry.register_builtin(
            CREATE_SCHEDULE_TOOL,
            "Create a schedule directly in Talon without using talon-ops MCP.",
            put_schedule_schema(),
        );
    }
    if has_capability_action(spec, "schedules", "update") {
        registry.register_builtin(
            UPDATE_SCHEDULE_TOOL,
            "Update an existing schedule directly in Talon without using talon-ops MCP.",
            put_schedule_schema(),
        );
    }
    if has_capability_action(spec, "schedules", "delete") {
        registry.register_builtin(
            DELETE_SCHEDULE_TOOL,
            "Delete a schedule directly in Talon without using talon-ops MCP.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Namespace containing the schedule. Defaults to the current agent namespace if omitted." },
                    "name": { "type": "string", "description": "Schedule name." }
                },
                "required": ["name"]
            }),
        );
    }

    if has_capability_action(spec, "sessions", "read:messages") {
        registry.register_builtin(
            READ_SESSION_MESSAGES_TOOL,
            "Read text messages from a Talon session. Use this to inspect delegated child agent output by namespace, agent, and session id.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Session namespace. Defaults to current namespace." },
                    "agent": { "type": "string", "description": "Session agent. Defaults to current agent." },
                    "session_id": { "type": "string", "description": "Session id to inspect." },
                    "limit": { "type": "integer", "description": "Maximum messages to return. Defaults to 20." }
                },
                "required": ["session_id"]
            }),
        );
    }

    if has_capability_action(spec, "memory", "inspect")
        || has_capability_action(spec, "memory", "read")
    {
        registry.register_builtin(
            SEARCH_MEMORY_TOOL,
            "Search durable workspace memory Files. Memory is stored as namespace File resources with purpose=MEMORY and indexPolicy=RETRIEVAL.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Memory namespace. Defaults to current namespace." },
                    "query": { "type": "string", "description": "Keyword query to match against memory path and text content." },
                    "prefix": { "type": "string", "description": "Optional logical path prefix such as /memory/playbooks." },
                    "limit": { "type": "integer", "description": "Maximum results. Defaults to 10." }
                },
                "required": ["query"]
            }),
        );
        registry.register_builtin(
            READ_MEMORY_TOOL,
            "Read one durable workspace memory File by logical path.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Memory namespace. Defaults to current namespace." },
                    "path": { "type": "string", "description": "Logical memory path, for example /memory/playbooks/aeo-prompt-strategy.md." }
                },
                "required": ["path"]
            }),
        );
        registry.register_builtin(
            LIST_MEMORY_TOOL,
            "List durable workspace memory Files by optional logical path prefix.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Memory namespace. Defaults to current namespace." },
                    "prefix": { "type": "string", "description": "Optional logical path prefix. Defaults to /memory." },
                    "limit": { "type": "integer", "description": "Maximum results. Defaults to 50." }
                }
            }),
        );
    }

    if has_capability_action(spec, "memory", "create") {
        registry.register_builtin(
            CREATE_MEMORY_TOOL,
            "Create a durable workspace memory File with purpose=MEMORY and indexPolicy=RETRIEVAL.",
            memory_write_schema(),
        );
    }
    if has_capability_action(spec, "memory", "update") {
        registry.register_builtin(
            UPDATE_MEMORY_TOOL,
            "Update a durable workspace memory File. Updates write a new immutable CAS object and advance File.status.objectRef.",
            memory_write_schema(),
        );
    }

    task_tools::register(registry, spec);

    if has_capability_action(spec, "goals", "inspect") {
        registry.register_builtin(
            LIST_GOALS_TOOL,
            "List Talon Goals for one session.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Namespace to inspect. Defaults to the current namespace." },
                    "agent": { "type": "string", "description": "Owning agent. Defaults to the current agent." },
                    "session_id": { "type": "string", "description": "Owning session id. Defaults to the current session." },
                    "status_group": { "type": "string", "description": "Optional group: active or terminal." },
                    "phase": { "type": "string", "description": "Optional phase such as RUNNING, NEEDS_REVIEW, SUCCEEDED, FAILED, BLOCKED, or CANCELED." },
                    "limit": { "type": "integer", "description": "Optional maximum number of goals to return." }
                }
            }),
        );
        registry.register_builtin(
            GET_GOAL_TOOL,
            "Get one Talon Goal by id.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Goal namespace. Defaults to the current namespace." },
                    "agent": { "type": "string", "description": "Owning agent. Defaults to the current agent." },
                    "session_id": { "type": "string", "description": "Owning session id. Defaults to the current session." },
                    "goal_id": { "type": "string", "description": "Goal id." }
                },
                "required": ["goal_id"]
            }),
        );
    }

    if has_capability_action(spec, "goals", "create") {
        registry.register_builtin(
            CREATE_GOAL_TOOL,
            "Create a session-scoped Talon Goal that tracks a durable objective and success criteria.",
            json!({
                "type": "object",
                "properties": {
                    "objective": { "type": "string", "description": "Durable objective the agent should keep in view." },
                    "success_criteria": { "type": "array", "items": { "type": "string" }, "description": "Concrete completion criteria." },
                    "max_iterations": { "type": "integer", "description": "Optional maximum iteration count." },
                    "progress_summary": { "type": "string", "description": "Optional initial progress summary." },
                    "labels": { "type": "object", "additionalProperties": { "type": "string" } },
                    "metadata": { "type": "object", "additionalProperties": { "type": "string" } }
                },
                "required": ["objective"]
            }),
        );
    }

    if has_capability_action(spec, "goals", "update") {
        registry.register_builtin(
            UPDATE_GOAL_TOOL,
            "Update Goal phase, progress, iteration, or blocked reason.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Goal namespace. Defaults to current namespace." },
                    "agent": { "type": "string", "description": "Owning agent. Defaults to current agent." },
                    "session_id": { "type": "string", "description": "Owning session id. Defaults to current session." },
                    "goal_id": { "type": "string", "description": "Goal id." },
                    "phase": { "type": "string", "description": "RUNNING, PAUSED, NEEDS_REVIEW, SUCCEEDED, FAILED, BLOCKED, CANCELED, or EXPIRED." },
                    "progress_summary": { "type": "string", "description": "Concise current state." },
                    "iteration": { "type": "integer", "description": "Current iteration number." },
                    "blocked_reason": { "type": "string", "description": "Reason the Goal is blocked." }
                },
                "required": ["goal_id"]
            }),
        );
        registry.register_builtin(
            COMPLETE_GOAL_TOOL,
            "Mark a Goal as SUCCEEDED with an optional final progress summary.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Goal namespace. Defaults to current namespace." },
                    "agent": { "type": "string", "description": "Owning agent. Defaults to current agent." },
                    "session_id": { "type": "string", "description": "Owning session id. Defaults to current session." },
                    "goal_id": { "type": "string", "description": "Goal id." },
                    "progress_summary": { "type": "string", "description": "Final result summary." }
                },
                "required": ["goal_id"]
            }),
        );
        registry.register_builtin(
            BLOCK_GOAL_TOOL,
            "Mark a Goal as BLOCKED with the reason no meaningful progress can continue.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Goal namespace. Defaults to current namespace." },
                    "agent": { "type": "string", "description": "Owning agent. Defaults to current agent." },
                    "session_id": { "type": "string", "description": "Owning session id. Defaults to current session." },
                    "goal_id": { "type": "string", "description": "Goal id." },
                    "blocked_reason": { "type": "string", "description": "Concrete blocker." },
                    "progress_summary": { "type": "string", "description": "Optional progress summary." }
                },
                "required": ["goal_id", "blocked_reason"]
            }),
        );
    }
}

fn register_research_tools(registry: &mut ToolRegistry, spec: &manifests::AgentSpec) {
    if has_capability_action(spec, "research", "fetch_url") {
        registry.register_builtin(
            FETCH_URL_TOOL,
            "Fetch a supplied HTTP(S) URL and return title, final URL, status, and compact visible text for source-grounded research.",
            json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "HTTP(S) URL to fetch." },
                    "max_chars": {
                        "type": "integer",
                        "description": "Maximum visible text characters to return. Defaults to 12000."
                    }
                },
                "required": ["url"]
            }),
        );
    }

    if has_capability_action(spec, "research", "web_search") {
        registry.register_builtin(
            WEB_SEARCH_TOOL,
            "Search the public web for source candidates. Use returned URLs with fetch_url before citing claims.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query." },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of search results. Defaults to 5."
                    }
                },
                "required": ["query"]
            }),
        );
    }
}

fn put_schedule_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "namespace": { "type": "string", "description": "Target namespace. Defaults to the current agent namespace if omitted." },
            "name": { "type": "string", "description": "Schedule name." },
            "labels": {
                "type": "object",
                "description": "Optional schedule labels.",
                "additionalProperties": { "type": "string" }
            },
            "kind": { "type": "string", "description": "Schedule kind: at, every, or cron." },
            "cron": { "type": "string", "description": "Cron expression for cron schedules." },
            "interval_seconds": { "type": "integer", "description": "Interval in seconds for every schedules." },
            "run_at": { "type": "string", "description": "RFC3339 timestamp for at schedules." },
            "timezone": { "type": "string", "description": "Optional timezone." },
            "agent": { "type": "string", "description": "Target agent. Defaults to the current agent if omitted." },
            "session_mode": { "type": "string", "description": "Session mode: new or reuse." },
            "session_id": { "type": "string", "description": "Session id to reuse when session_mode is reuse." },
            "input_message": { "type": "string", "description": "Message the schedule should send when it runs." },
            "enabled": { "type": "boolean", "description": "Whether the schedule is enabled." }
        },
        "required": ["name", "kind", "input_message"]
    })
}

fn memory_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "namespace": { "type": "string", "description": "Memory namespace. Defaults to current namespace." },
            "path": { "type": "string", "description": "Logical memory path, for example /memory/research/context.md." },
            "content": { "type": "string", "description": "Markdown or text content to store." },
            "media_type": { "type": "string", "description": "Media type. Defaults to text/markdown." }
        },
        "required": ["path", "content"]
    })
}

pub async fn execute_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    spec: &manifests::AgentSpec,
    name: &str,
    args: &Value,
) -> Result<Option<String>> {
    execute_tool_for_session(cp, current_namespace, current_agent, "", spec, name, args).await
}

pub async fn execute_tool_for_session(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    name: &str,
    args: &Value,
) -> Result<Option<String>> {
    if let Some(result) = artifact_tools::execute(
        cp,
        current_namespace,
        current_agent,
        current_session,
        name,
        args,
    )
    .await?
    {
        return Ok(Some(result));
    }
    if let Some(result) = a2a_tools::execute(
        cp,
        current_namespace,
        current_agent,
        current_session,
        spec,
        name,
        args,
    )
    .await?
    {
        return Ok(Some(result));
    }
    if let Some(result) = task_tools::execute(
        cp,
        current_namespace,
        current_agent,
        current_session,
        spec,
        name,
        args,
    )
    .await?
    {
        return Ok(Some(result));
    }

    match name {
        READ_SESSION_MESSAGES_TOOL => {
            require_capability(spec, "sessions", "read:messages")?;
            read_session_messages(cp, current_namespace, current_agent, args)
                .await
                .map(Some)
        }
        SEARCH_MEMORY_TOOL => {
            require_memory_read(spec)?;
            search_memory(cp, current_namespace, args).await.map(Some)
        }
        READ_MEMORY_TOOL => {
            require_memory_read(spec)?;
            read_memory(cp, current_namespace, args).await.map(Some)
        }
        LIST_MEMORY_TOOL => {
            require_memory_read(spec)?;
            list_memory(cp, current_namespace, args).await.map(Some)
        }
        CREATE_MEMORY_TOOL => {
            require_capability(spec, "memory", "create")?;
            put_memory(cp, current_namespace, args).await.map(Some)
        }
        UPDATE_MEMORY_TOOL => {
            require_capability(spec, "memory", "update")?;
            put_memory(cp, current_namespace, args).await.map(Some)
        }
        FETCH_URL_TOOL => {
            require_capability(spec, "research", "fetch_url")?;
            fetch_url(args).await.map(Some)
        }
        WEB_SEARCH_TOOL => {
            require_capability(spec, "research", "web_search")?;
            web_search(args).await.map(Some)
        }
        ACTIVATE_SKILL_TOOL => {
            let skill_name = req_str(args, "name")?;
            let skills = namespace::load_effective_skills(cp.kv.clone(), current_namespace).await?;
            let skill = namespace::find_effective_skill(&skills, skill_name)
                .ok_or_else(|| anyhow!("skill '{}' is not available", skill_name))?;
            let activated = namespace::format_activated_skill(skill)
                .ok_or_else(|| anyhow!("skill '{}' has no instructions", skill_name))?;
            Ok(Some(activated))
        }
        CHANNEL_PUBLISH_TOOL => {
            let content = req_str(args, "content")?;
            let message = crate::gateway::rpc::channels::publish_channel_message_from_session(
                cp,
                current_namespace,
                current_agent,
                current_session,
                content,
            )
            .await?;
            Ok(Some(serde_json::to_string_pretty(&json!({
                "published": true,
                "messageId": message.id,
                "channel": message.channel
            }))?))
        }
        CHANNEL_SKIP_REPLY_TOOL => {
            let reason = opt_str(args, "reason").unwrap_or("");
            crate::gateway::rpc::channels::skip_channel_reply_from_session(
                cp,
                current_namespace,
                current_agent,
                current_session,
                reason,
            )
            .await?;
            Ok(Some(serde_json::to_string_pretty(&json!({
                "published": false,
                "skipped": true
            }))?))
        }
        LIST_SCHEDULES_TOOL => {
            require_capability(spec, "schedules", "inspect")?;
            let namespace = opt_str(args, "namespace").unwrap_or(current_namespace);
            let agent = opt_str(args, "agent");
            let enabled = args.get("enabled").and_then(Value::as_bool);
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(100) as usize;
            let entries = cp
                .kv
                .list_entries(&keys::schedule_prefix(namespace), None)
                .await?;
            let mut schedules = Vec::new();
            for (_key, value) in entries {
                let schedule = resources_proto::Schedule::decode(value.as_slice())?;
                let spec_model = schedule.spec.as_ref();
                let matches_agent = agent
                    .map(|target| {
                        spec_model
                            .and_then(|current| current.target.as_ref())
                            .map(|target_model| target_model.agent == target)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true);
                let matches_enabled = enabled
                    .map(|value| {
                        spec_model
                            .map(|current| current.enabled == value)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true);
                if matches_agent && matches_enabled {
                    schedules.push(schedule_json(&schedule));
                }
                if schedules.len() >= limit {
                    break;
                }
            }
            Ok(Some(serde_json::to_string_pretty(
                &json!({ "schedules": schedules }),
            )?))
        }
        GET_SCHEDULE_TOOL => {
            require_capability(spec, "schedules", "inspect")?;
            let namespace = opt_str(args, "namespace").unwrap_or(current_namespace);
            let schedule_name = req_str(args, "name")?;
            let schedule = cp
                .kv
                .get_msg::<resources_proto::Schedule>(&keys::schedule(namespace, schedule_name))
                .await?
                .ok_or_else(|| anyhow!("schedule '{}' not found", schedule_name))?;
            Ok(Some(serde_json::to_string_pretty(&json!({
                "schedule": schedule_json(&schedule)
            }))?))
        }
        CREATE_SCHEDULE_TOOL => {
            require_capability(spec, "schedules", "create")?;
            let schedule =
                upsert_schedule(cp, current_namespace, current_agent, args, None).await?;
            Ok(Some(serde_json::to_string_pretty(&json!({
                "schedule": schedule_json(&schedule),
                "backendArmed": schedule.status.as_ref().map(|status| status.backend_armed).unwrap_or(false)
            }))?))
        }
        UPDATE_SCHEDULE_TOOL => {
            require_capability(spec, "schedules", "update")?;
            let namespace = opt_str(args, "namespace").unwrap_or(current_namespace);
            let schedule_name = req_str(args, "name")?;
            let existing = cp
                .kv
                .get_msg::<resources_proto::Schedule>(&keys::schedule(namespace, schedule_name))
                .await?
                .ok_or_else(|| anyhow!("schedule '{}' not found", schedule_name))?;
            let schedule =
                upsert_schedule(cp, current_namespace, current_agent, args, Some(existing)).await?;
            Ok(Some(serde_json::to_string_pretty(&json!({
                "schedule": schedule_json(&schedule),
                "backendArmed": schedule.status.as_ref().map(|status| status.backend_armed).unwrap_or(false)
            }))?))
        }
        DELETE_SCHEDULE_TOOL => {
            require_capability(spec, "schedules", "delete")?;
            let namespace = opt_str(args, "namespace").unwrap_or(current_namespace);
            let schedule_name = req_str(args, "name")?;
            let key = keys::schedule(namespace, schedule_name);
            if let Some(schedule) = cp.kv.get_msg::<resources_proto::Schedule>(&key).await? {
                if let Some(handle) = schedule.status.and_then(|status| status.backend_handle) {
                    if let Err(error) = cp.scheduler.cancel(&handle).await {
                        tracing::warn!(handle = %handle, error = %error, "failed to cancel schedule handle");
                    }
                }
            }
            cp.kv.delete(&key).await?;
            Ok(Some(serde_json::to_string_pretty(
                &json!({ "success": true }),
            )?))
        }
        LIST_GOALS_TOOL => {
            require_capability(spec, "goals", "inspect")?;
            let namespace = opt_str(args, "namespace").unwrap_or(current_namespace);
            let agent = opt_str(args, "agent").unwrap_or(current_agent);
            let session_id = opt_str(args, "session_id").unwrap_or(current_session);
            let status_group = opt_str(args, "status_group");
            let phase = opt_str(args, "phase");
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(100) as usize;
            let goals = list_goals(cp, namespace, agent, session_id, status_group, phase, limit)
                .await?
                .into_iter()
                .map(|goal| goal_json(&goal))
                .collect::<Vec<_>>();
            Ok(Some(serde_json::to_string_pretty(&json!({
                "goals": goals
            }))?))
        }
        GET_GOAL_TOOL => {
            require_capability(spec, "goals", "inspect")?;
            let goal =
                get_goal_from_args(cp, current_namespace, current_agent, current_session, args)
                    .await?;
            Ok(Some(serde_json::to_string_pretty(&json!({
                "goal": goal_json(&goal)
            }))?))
        }
        CREATE_GOAL_TOOL => {
            require_capability(spec, "goals", "create")?;
            let goal =
                create_goal(cp, current_namespace, current_agent, current_session, args).await?;
            Ok(Some(serde_json::to_string_pretty(&json!({
                "goal": goal_json(&goal)
            }))?))
        }
        UPDATE_GOAL_TOOL => {
            require_capability(spec, "goals", "update")?;
            let mut goal =
                get_goal_from_args(cp, current_namespace, current_agent, current_session, args)
                    .await?;
            update_goal_from_args(&mut goal, args)?;
            upsert_goal(cp, goal.clone()).await?;
            Ok(Some(serde_json::to_string_pretty(&json!({
                "goal": goal_json(&goal)
            }))?))
        }
        COMPLETE_GOAL_TOOL => {
            require_capability(spec, "goals", "update")?;
            let mut goal =
                get_goal_from_args(cp, current_namespace, current_agent, current_session, args)
                    .await?;
            let now = chrono::Utc::now().timestamp_micros();
            goal.phase = crate::gateway::rpc::data_proto::GoalPhase::Succeeded as i32;
            goal.updated_at = now;
            goal.completed_at = now;
            if let Some(summary) = opt_str(args, "progress_summary") {
                goal.progress_summary = summary.to_string();
            }
            upsert_goal(cp, goal.clone()).await?;
            Ok(Some(serde_json::to_string_pretty(&json!({
                "goal": goal_json(&goal)
            }))?))
        }
        BLOCK_GOAL_TOOL => {
            require_capability(spec, "goals", "update")?;
            let mut goal =
                get_goal_from_args(cp, current_namespace, current_agent, current_session, args)
                    .await?;
            let now = chrono::Utc::now().timestamp_micros();
            goal.phase = crate::gateway::rpc::data_proto::GoalPhase::Blocked as i32;
            goal.updated_at = now;
            goal.blocked_reason = req_str(args, "blocked_reason")?.to_string();
            if let Some(summary) = opt_str(args, "progress_summary") {
                goal.progress_summary = summary.to_string();
            }
            upsert_goal(cp, goal.clone()).await?;
            Ok(Some(serde_json::to_string_pretty(&json!({
                "goal": goal_json(&goal)
            }))?))
        }
        _ => Ok(None),
    }
}

async fn read_session_messages(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    args: &Value,
) -> Result<String> {
    let namespace = opt_str(args, "namespace").unwrap_or(current_namespace);
    let agent = opt_str(args, "agent").unwrap_or(current_agent);
    let session_id = req_str(args, "session_id")?;
    let limit = opt_usize(args, "limit").unwrap_or(20).clamp(1, 100);
    let mut entries = cp
        .kv
        .list_entries(
            &keys::session_message_prefix(namespace, agent, session_id),
            Some(ListOptions::desc().limit(limit)),
        )
        .await?;
    let mut messages = Vec::new();
    entries.reverse();
    for (_, bytes) in entries {
        let message = data_proto::SessionMessage::decode(bytes.as_slice())?;
        let role = data_proto::MessageRole::try_from(message.role)
            .map(|role| format!("{role:?}"))
            .unwrap_or_else(|_| message.role.to_string());
        let text = message
            .parts
            .iter()
            .filter(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
            .map(|part| part.content.as_str())
            .collect::<String>();
        messages.push(json!({
            "id": message.id,
            "role": role,
            "text": text,
            "createdAt": message.created_at,
            "labels": message.labels,
        }));
    }
    Ok(serde_json::to_string_pretty(&json!({
        "namespace": namespace,
        "agent": agent,
        "sessionId": session_id,
        "messages": messages,
    }))?)
}

async fn search_memory(cp: &ControlPlane, current_namespace: &str, args: &Value) -> Result<String> {
    let namespace = opt_str(args, "namespace").unwrap_or(current_namespace);
    let query = req_str(args, "query")?.to_ascii_lowercase();
    let prefix = opt_str(args, "prefix")
        .map(normalize_logical_path)
        .transpose()?
        .unwrap_or_else(|| "/memory".to_string());
    let limit = opt_usize(args, "limit").unwrap_or(10).clamp(1, 50);
    let mut results = Vec::new();
    for file in list_memory_files(cp, namespace, &prefix).await? {
        let Some(spec) = file.spec.as_ref() else {
            continue;
        };
        let content = read_file_content(cp, &file).await.unwrap_or_default();
        let haystack = format!("{}\n{}", spec.path, content).to_ascii_lowercase();
        if haystack.contains(&query) {
            results.push(json!({
                "namespace": namespace,
                "name": file_name_from_file(&file),
                "path": spec.path,
                "mediaType": spec.media_type,
                "excerpt": memory_excerpt(&content, &query),
            }));
            if results.len() >= limit {
                break;
            }
        }
    }
    Ok(serde_json::to_string_pretty(
        &json!({ "results": results }),
    )?)
}

async fn read_memory(cp: &ControlPlane, current_namespace: &str, args: &Value) -> Result<String> {
    let namespace = opt_str(args, "namespace").unwrap_or(current_namespace);
    let path = normalize_logical_path(req_str(args, "path")?)?;
    let file = find_memory_file_by_path(cp, namespace, &path)
        .await?
        .ok_or_else(|| anyhow!("memory file '{}' not found", path))?;
    let content = read_file_content(cp, &file).await?;
    Ok(serde_json::to_string_pretty(&json!({
        "namespace": namespace,
        "name": file_name_from_file(&file),
        "path": path,
        "content": content,
    }))?)
}

async fn list_memory(cp: &ControlPlane, current_namespace: &str, args: &Value) -> Result<String> {
    let namespace = opt_str(args, "namespace").unwrap_or(current_namespace);
    let prefix = opt_str(args, "prefix")
        .map(normalize_logical_path)
        .transpose()?
        .unwrap_or_else(|| "/memory".to_string());
    let limit = opt_usize(args, "limit").unwrap_or(50).clamp(1, 100);
    let files = list_memory_files(cp, namespace, &prefix).await?;
    let entries = files
        .into_iter()
        .take(limit)
        .filter_map(|file| {
            let spec = file.spec.as_ref()?;
            let object = file
                .status
                .as_ref()
                .and_then(|status| status.object_ref.as_ref());
            Some(json!({
                "namespace": namespace,
                "name": file_name_from_file(&file),
                "path": spec.path,
                "mediaType": spec.media_type,
                "sizeBytes": object.map(|object| object.size_bytes).unwrap_or_default(),
                "sha256": object.map(|object| object.sha256.as_str()).unwrap_or_default(),
            }))
        })
        .collect::<Vec<_>>();
    Ok(serde_json::to_string_pretty(
        &json!({ "entries": entries }),
    )?)
}

async fn put_memory(cp: &ControlPlane, current_namespace: &str, args: &Value) -> Result<String> {
    let namespace = opt_str(args, "namespace").unwrap_or(current_namespace);
    let path = normalize_memory_path(req_str(args, "path")?)?;
    let content = req_str(args, "content")?;
    let media_type = opt_str(args, "media_type").unwrap_or("text/markdown");
    let file = upsert_memory_file(cp, namespace, &path, media_type, content.as_bytes()).await?;
    Ok(serde_json::to_string_pretty(&json!({
        "file": memory_file_json(&file),
    }))?)
}

async fn list_memory_files(
    cp: &ControlPlane,
    namespace: &str,
    prefix: &str,
) -> Result<Vec<resources_proto::File>> {
    let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    let mut files = Vec::new();
    for resource in store.list(namespace, Some("File")).await? {
        let Some(file) = file_from_resource(resource) else {
            continue;
        };
        let Some(spec) = file.spec.as_ref() else {
            continue;
        };
        if spec.purpose != resources_proto::FilePurpose::Memory as i32 {
            continue;
        }
        if spec.index_policy != resources_proto::FileIndexPolicy::Retrieval as i32 {
            continue;
        }
        if !prefix.is_empty() && !spec.path.starts_with(prefix) {
            continue;
        }
        files.push(file);
    }
    files.sort_by(|left, right| {
        left.spec
            .as_ref()
            .map(|spec| spec.path.as_str())
            .cmp(&right.spec.as_ref().map(|spec| spec.path.as_str()))
    });
    Ok(files)
}

async fn find_memory_file_by_path(
    cp: &ControlPlane,
    namespace: &str,
    path: &str,
) -> Result<Option<resources_proto::File>> {
    Ok(list_memory_files(cp, namespace, path)
        .await?
        .into_iter()
        .find(|file| file.spec.as_ref().map(|spec| spec.path.as_str()) == Some(path)))
}

async fn read_file_content(cp: &ControlPlane, file: &resources_proto::File) -> Result<String> {
    let object_ref = file
        .status
        .as_ref()
        .and_then(|status| status.object_ref.as_ref())
        .ok_or_else(|| anyhow!("File has no objectRef"))?;
    let object = crate::control::cas::CasStore::new(cp.objects.clone())
        .get_object_decoded(&object_ref.key)
        .await?
        .ok_or_else(|| anyhow!("File object '{}' not found", object_ref.key))?;
    Ok(String::from_utf8_lossy(&object.bytes).to_string())
}

async fn upsert_memory_file(
    cp: &ControlPlane,
    namespace: &str,
    path: &str,
    media_type: &str,
    content: &[u8],
) -> Result<resources_proto::File> {
    let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    let existing = find_memory_file_by_path(cp, namespace, path).await?;
    let name = existing
        .as_ref()
        .map(file_name_from_file)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| safe_file_resource_name(path));
    let status = existing
        .as_ref()
        .and_then(|file| file.status.clone())
        .unwrap_or_default();
    let spec = resources_proto::FileSpec {
        path: path.to_string(),
        media_type: media_type.to_string(),
        purpose: resources_proto::FilePurpose::Memory as i32,
        index_policy: resources_proto::FileIndexPolicy::Retrieval as i32,
        retention: resources_proto::FileRetention::Retained as i32,
    };
    let mut resource = store
        .upsert(
            namespace,
            resource_model::file_resource(
                namespace.to_string(),
                name,
                spec,
                status,
                file_resource_labels(
                    resources_proto::FilePurpose::Memory as i32,
                    resources_proto::FileIndexPolicy::Retrieval as i32,
                    resources_proto::FileRetention::Retained as i32,
                ),
            ),
        )
        .await?;
    let uid = resource
        .metadata
        .as_ref()
        .map(|metadata| metadata.uid.as_str())
        .filter(|uid| !uid.is_empty())
        .ok_or_else(|| anyhow!("File resource uid missing after upsert"))?;
    let object_ref = write_file_objects(cp, namespace, uid, path, media_type, content).await?;
    let status = resources_proto::FileStatus {
        observed_generation: resource
            .metadata
            .as_ref()
            .map(|metadata| metadata.generation)
            .unwrap_or_default(),
        phase: "Ready".to_string(),
        conditions: Vec::new(),
        object_ref: Some(object_ref),
        updated_at: chrono::Utc::now().timestamp_micros(),
        pending_upload: None,
    };
    resource.status = Some(resources_proto::ResourceStatus {
        kind: Some(resources_proto::resource_status::Kind::File(status)),
    });
    let name = resource
        .metadata
        .as_ref()
        .map(|metadata| metadata.name.clone())
        .ok_or_else(|| anyhow!("File resource name missing"))?;
    let resource = store
        .patch_status(namespace, "File", &name, None, resource.status.unwrap())
        .await?;
    file_from_resource(resource).ok_or_else(|| anyhow!("invalid File resource"))
}

async fn write_file_objects(
    cp: &ControlPlane,
    namespace: &str,
    file_uid: &str,
    path: &str,
    media_type: &str,
    content: &[u8],
) -> Result<resources_proto::FileObjectRef> {
    let cas = crate::control::cas::CasStore::new(cp.objects.clone());
    let object_ref = cas
        .put_file(namespace, file_uid, path, content, media_type)
        .await?;
    cas.put_latest_file(namespace, path, content, media_type)
        .await?;
    Ok(resources_proto::FileObjectRef {
        key: object_ref.key,
        media_type: object_ref.media_type,
        size_bytes: object_ref.size_bytes,
        sha256: object_ref.sha256,
        filename: object_ref.filename,
        metadata: object_ref.metadata,
    })
}

fn file_from_resource(resource: resources_proto::Resource) -> Option<resources_proto::File> {
    let spec = resource.spec.and_then(|spec| match spec.kind {
        Some(resources_proto::resource_spec::Kind::File(spec)) => Some(spec),
        _ => None,
    })?;
    let status = resource.status.and_then(|status| match status.kind {
        Some(resources_proto::resource_status::Kind::File(status)) => Some(status),
        _ => None,
    });
    Some(resources_proto::File {
        metadata: resource.metadata,
        spec: Some(spec),
        status,
    })
}

fn file_name_from_file(file: &resources_proto::File) -> String {
    file.metadata
        .as_ref()
        .map(|metadata| metadata.name.clone())
        .unwrap_or_default()
}

fn memory_file_json(file: &resources_proto::File) -> Value {
    let spec = file.spec.as_ref();
    let object = file
        .status
        .as_ref()
        .and_then(|status| status.object_ref.as_ref());
    json!({
        "namespace": file.metadata.as_ref().map(|metadata| metadata.namespace.as_str()).unwrap_or_default(),
        "name": file_name_from_file(file),
        "path": spec.map(|spec| spec.path.as_str()).unwrap_or_default(),
        "mediaType": spec.map(|spec| spec.media_type.as_str()).unwrap_or_default(),
        "purpose": "MEMORY",
        "indexPolicy": "RETRIEVAL",
        "sizeBytes": object.map(|object| object.size_bytes).unwrap_or_default(),
        "sha256": object.map(|object| object.sha256.as_str()).unwrap_or_default(),
    })
}

fn normalize_memory_path(path: &str) -> Result<String> {
    let path = normalize_logical_path(path)?;
    if !path.starts_with("/memory/") && path != "/memory" {
        return Err(anyhow!("memory path must be under /memory"));
    }
    Ok(path)
}

fn safe_file_resource_name(path: &str) -> String {
    let slug = path
        .trim_matches('/')
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(48)
        .collect::<String>();
    crate::control::uuid::unique_name(if slug.is_empty() { "file" } else { &slug })
}

fn file_resource_labels(
    purpose: i32,
    index_policy: i32,
    retention: i32,
) -> HashMap<String, String> {
    HashMap::from([
        (
            "talon.impalasys.com/purpose".to_string(),
            file_purpose_label(purpose).to_string(),
        ),
        (
            "talon.impalasys.com/index-policy".to_string(),
            file_index_policy_label(index_policy).to_string(),
        ),
        (
            "talon.impalasys.com/retention".to_string(),
            file_retention_label(retention).to_string(),
        ),
    ])
}

fn file_purpose_label(value: i32) -> &'static str {
    match resources_proto::FilePurpose::try_from(value).ok() {
        Some(resources_proto::FilePurpose::Memory) => "memory",
        Some(resources_proto::FilePurpose::Artifact) => "artifact",
        _ => "unspecified",
    }
}

fn file_index_policy_label(value: i32) -> &'static str {
    match resources_proto::FileIndexPolicy::try_from(value).ok() {
        Some(resources_proto::FileIndexPolicy::None) => "none",
        Some(resources_proto::FileIndexPolicy::Search) => "search",
        Some(resources_proto::FileIndexPolicy::Retrieval) => "retrieval",
        _ => "unspecified",
    }
}

fn file_retention_label(value: i32) -> &'static str {
    match resources_proto::FileRetention::try_from(value).ok() {
        Some(resources_proto::FileRetention::Retained) => "retained",
        _ => "unspecified",
    }
}

fn memory_excerpt(content: &str, query: &str) -> String {
    let lower = content.to_ascii_lowercase();
    let query_index = lower.find(query).unwrap_or(0);
    let mut byte_count: usize = 0;
    let mut char_index: usize = 0;
    for ch in content.chars() {
        if byte_count >= query_index {
            break;
        }
        byte_count += ch.len_utf8();
        char_index += 1;
    }
    let start = char_index.saturating_sub(120);
    content
        .chars()
        .skip(start)
        .take(340)
        .collect::<String>()
        .trim()
        .to_string()
}

async fn create_artifact(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<String> {
    if current_session.trim().is_empty() {
        return Err(anyhow!("create_artifact requires an active session"));
    }
    let title = req_str(args, "title")?;
    let media_type = opt_str(args, "media_type").unwrap_or("text/markdown");
    if args.get("content").and_then(Value::as_str).is_none()
        && opt_str(args, "content_base64").is_none()
    {
        return Err(anyhow!(
            "create_artifact requires content or content_base64"
        ));
    }
    let content = artifact_content_bytes(args)?;
    let labels = string_map(args.get("labels"));
    let metadata = string_map(args.get("metadata"));
    let artifact_id = crate::control::uuid::unique_name("artifact");
    let object_ref = crate::control::cas::CasStore::new(cp.objects.clone())
        .put_artifact(
            current_namespace,
            current_agent,
            current_session,
            &artifact_id,
            &content,
            media_type,
            metadata.clone(),
        )
        .await?;
    let artifact = crate::gateway::rpc::data_proto::Artifact {
        id: artifact_id.clone(),
        session_id: current_session.to_string(),
        title: title.to_string(),
        media_type: media_type.to_string(),
        object_ref: Some(object_ref),
        created_by_agent: current_agent.to_string(),
        created_at: chrono::Utc::now().timestamp_micros(),
        labels,
        metadata,
    };
    cp.kv
        .set_msg(
            &keys::artifact(
                current_namespace,
                current_agent,
                current_session,
                &artifact_id,
            ),
            &artifact,
        )
        .await?;
    let artifact_uri = ArtifactUri {
        namespace: current_namespace.to_string(),
        agent: current_agent.to_string(),
        session_id: current_session.to_string(),
        artifact_id: artifact_id.clone(),
    }
    .encode();
    Ok(serde_json::to_string_pretty(&json!({
        "artifact": artifact_json(&artifact),
        "artifactUri": artifact_uri
    }))?)
}

async fn read_artifact(
    cp: &ControlPlane,
    _current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<String> {
    let artifact_uri = req_str(args, "artifact_uri")?;
    let (_, artifact) =
        resolve_artifact_uri(cp, current_agent, current_session, artifact_uri, OP_READ).await?;
    let object_ref = artifact
        .object_ref
        .as_ref()
        .ok_or_else(|| anyhow!("Artifact has no objectRef"))?;
    let object = cp
        .objects
        .get(&object_ref.key)
        .await?
        .ok_or_else(|| anyhow!("Artifact object not found"))?;
    let content_text = String::from_utf8(object.bytes.clone()).ok();
    Ok(serde_json::to_string_pretty(&json!({
        "artifact": artifact_json(&artifact),
        "content": content_text,
        "contentBase64": if content_text.is_none() {
            Some(general_purpose::STANDARD.encode(&object.bytes))
        } else {
            None
        }
    }))?)
}

async fn update_artifact(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<String> {
    let artifact_uri = req_str(args, "artifact_uri")?;
    let uri = parse_artifact_uri(artifact_uri)?;
    if current_namespace != uri.namespace
        || current_agent != uri.agent
        || current_session != uri.session_id
    {
        return Err(anyhow!(
            "only the owning artifact namespace/agent/session may update '{artifact_uri}'"
        ));
    }
    let mut artifact = cp
        .kv
        .get_msg::<crate::gateway::rpc::data_proto::Artifact>(&keys::artifact(
            &uri.namespace,
            &uri.agent,
            &uri.session_id,
            &uri.artifact_id,
        ))
        .await?
        .ok_or_else(|| anyhow!("Artifact '{}' not found", uri.artifact_id))?;
    let previous_object_key = artifact
        .object_ref
        .as_ref()
        .map(|object_ref| object_ref.key.clone());
    let media_type = opt_str(args, "media_type").unwrap_or(&artifact.media_type);
    if args.get("content").and_then(Value::as_str).is_none()
        && opt_str(args, "content_base64").is_none()
    {
        return Err(anyhow!(
            "update_artifact requires content or content_base64"
        ));
    }
    let content = artifact_content_bytes(args)?;
    let cas = crate::control::cas::CasStore::new(cp.objects.clone());
    let object_ref = cas
        .put_artifact(
            &uri.namespace,
            &uri.agent,
            &uri.session_id,
            &uri.artifact_id,
            &content,
            media_type,
            artifact.metadata.clone(),
        )
        .await?;
    artifact.media_type = media_type.to_string();
    artifact.object_ref = Some(object_ref);
    let artifact_key = keys::artifact(
        &uri.namespace,
        &uri.agent,
        &uri.session_id,
        &uri.artifact_id,
    );
    if let Err(error) = cp.kv.set_msg(&artifact_key, &artifact).await {
        if let Some(new_object_key) = artifact
            .object_ref
            .as_ref()
            .map(|object_ref| &object_ref.key)
        {
            if let Err(cleanup_error) = cas.delete_object(new_object_key).await {
                tracing::warn!(
                    error = %cleanup_error,
                    object_key = %new_object_key,
                    "failed to delete uncommitted artifact CAS object after update failure"
                );
            }
        }
        return Err(error);
    }
    if let Some(previous_object_key) = previous_object_key {
        if artifact
            .object_ref
            .as_ref()
            .is_none_or(|object_ref| object_ref.key != previous_object_key)
        {
            if let Err(error) = cas.delete_object(&previous_object_key).await {
                tracing::warn!(
                    error = %error,
                    object_key = %previous_object_key,
                    "failed to delete superseded artifact CAS object"
                );
            }
        }
    }
    Ok(serde_json::to_string_pretty(&json!({
        "artifact": artifact_json(&artifact),
        "artifactUri": uri.encode()
    }))?)
}

async fn get_artifact_metadata(
    cp: &ControlPlane,
    _current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<String> {
    let artifact_uri = req_str(args, "artifact_uri")?;
    let (_, artifact) = resolve_artifact_uri(
        cp,
        current_agent,
        current_session,
        artifact_uri,
        OP_METADATA,
    )
    .await?;
    Ok(serde_json::to_string_pretty(&json!({
        "artifact": artifact_json(&artifact)
    }))?)
}

async fn grant_artifact(
    cp: &ControlPlane,
    _current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<String> {
    let artifact_uri = req_str(args, "artifact_uri")?;
    let (uri, _) =
        resolve_artifact_uri(cp, current_agent, current_session, artifact_uri, OP_READ).await?;
    let operations = args
        .get("operations")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| vec![OP_READ.to_string(), OP_METADATA.to_string()]);
    for operation in &operations {
        if !matches!(operation.as_str(), OP_READ | OP_METADATA | OP_PROMOTE) {
            return Err(anyhow!("unsupported artifact operation '{}'", operation));
        }
    }
    let ttl = args
        .get("ttl_seconds")
        .and_then(Value::as_i64)
        .map(access_expiry_from_ttl_seconds)
        .unwrap_or_else(default_access_expiry);
    let target_agent = opt_str(args, "target_agent").unwrap_or("");
    let target_session_id = opt_str(args, "target_session_id").unwrap_or("");
    let access = crate::gateway::rpc::data_proto::ArtifactAccess {
        target_agent: target_agent.to_string(),
        target_session_id: target_session_id.to_string(),
        operations,
        expires_at: ttl,
        granted_by_agent: current_agent.to_string(),
        granted_by_session_id: current_session.to_string(),
        created_at: chrono::Utc::now().timestamp_micros(),
    };
    cp.kv
        .set_msg(
            &keys::artifact_access(
                &uri.namespace,
                &uri.agent,
                &uri.session_id,
                &uri.artifact_id,
                target_agent,
                target_session_id,
            ),
            &access,
        )
        .await?;
    Ok(serde_json::to_string_pretty(&json!({
        "artifactUri": uri.encode()
    }))?)
}

async fn fetch_url(args: &Value) -> Result<String> {
    let url = req_str(args, "url")?;
    let mut current_url = validate_public_http_url(url).await?;
    let max_chars = args
        .get("max_chars")
        .and_then(Value::as_u64)
        .unwrap_or(12_000)
        .clamp(1_000, 40_000) as usize;
    let mut redirects = 0usize;
    let response = loop {
        let client = http_client(&current_url)?;
        let response = client.get(current_url.url.clone()).send().await?;
        if !response.status().is_redirection() {
            break response;
        }
        if redirects >= 5 {
            return Err(anyhow!("too many redirects while fetching URL"));
        }
        let Some(location) = response.headers().get(reqwest::header::LOCATION) else {
            break response;
        };
        let location = location
            .to_str()
            .map_err(|err| anyhow!("redirect Location is not valid UTF-8: {err}"))?;
        current_url = validate_public_http_url(current_url.url.join(location)?.as_str()).await?;
        redirects += 1;
    };
    let status = response.status().as_u16();
    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = response.text().await?;
    let title = extract_title(&body);
    let visible_text = compact_visible_text(&body, max_chars);

    Ok(serde_json::to_string_pretty(&json!({
        "url": url,
        "finalUrl": final_url,
        "status": status,
        "contentType": content_type,
        "title": title,
        "text": visible_text,
        "truncated": body.len() > max_chars
    }))?)
}

async fn web_search(args: &Value) -> Result<String> {
    let query = req_str(args, "query")?;
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, 10) as usize;
    let url = format!(
        "https://duckduckgo.com/html/?q={}",
        urlencoding::encode(query)
    );
    let url = validate_public_http_url(&url).await?;
    let response = http_client(&url)?.get(url.url.clone()).send().await?;
    let status = response.status().as_u16();
    let body = response.text().await?;
    let results = extract_duckduckgo_results(&body, limit);

    Ok(serde_json::to_string_pretty(&json!({
        "query": query,
        "provider": "duckduckgo-html",
        "status": status,
        "results": results,
        "citationPolicy": "Fetch a result URL with fetch_url before using it as evidence."
    }))?)
}

async fn upsert_schedule(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    args: &Value,
    existing: Option<resources_proto::Schedule>,
) -> Result<resources_proto::Schedule> {
    let namespace = opt_str(args, "namespace")
        .unwrap_or(current_namespace)
        .to_string();
    let name = req_str(args, "name")?.to_string();
    let existing_spec = existing
        .as_ref()
        .and_then(|schedule| schedule.spec.as_ref());
    let existing_target = existing_spec.and_then(|spec| spec.target.as_ref());
    let kind = scheduling::normalize_schedule_kind(
        opt_str(args, "kind")
            .or_else(|| existing_spec.map(|spec| spec.kind.as_str()))
            .unwrap_or(""),
    );
    let cron = opt_str(args, "cron")
        .map(str::to_string)
        .or_else(|| existing_spec.map(|spec| spec.cron.clone()))
        .unwrap_or_default();
    let interval_seconds = args
        .get("interval_seconds")
        .and_then(Value::as_u64)
        .map(|value| value as u32)
        .or_else(|| existing_spec.map(|spec| spec.interval_seconds))
        .unwrap_or_default();
    let run_at = opt_str(args, "run_at")
        .map(str::to_string)
        .or_else(|| existing_spec.map(|spec| spec.run_at.clone()))
        .unwrap_or_default();
    let timezone = opt_str(args, "timezone")
        .map(str::to_string)
        .or_else(|| existing_spec.map(|spec| spec.timezone.clone()))
        .unwrap_or_default();
    let agent = opt_str(args, "agent")
        .map(str::to_string)
        .or_else(|| existing_target.map(|target| target.agent.clone()))
        .unwrap_or_else(|| current_agent.to_string());
    let session_mode = opt_str(args, "session_mode")
        .map(str::to_string)
        .or_else(|| existing_target.map(|target| target.session_mode.clone()))
        .unwrap_or_else(|| "new".to_string());
    let session_mode = scheduling::normalize_session_mode(&session_mode)?;
    let session_id = opt_str(args, "session_id")
        .map(str::to_string)
        .or_else(|| existing_target.map(|target| target.session_id.clone()))
        .unwrap_or_default();
    let input_message = opt_str(args, "input_message")
        .map(str::to_string)
        .or_else(|| existing_spec.map(|spec| spec.input_message.clone()))
        .unwrap_or_default();
    let enabled = args
        .get("enabled")
        .and_then(Value::as_bool)
        .or_else(|| existing_spec.map(|spec| spec.enabled))
        .unwrap_or(true);
    let labels = args
        .get("labels")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    value
                        .as_str()
                        .map(|current| (key.clone(), current.to_string()))
                })
                .collect::<HashMap<_, _>>()
        })
        .or_else(|| existing.as_ref().map(|schedule| schedule.labels().clone()))
        .unwrap_or_default();

    let mut schedule = resource_model::schedule(
        namespace.clone(),
        name.clone(),
        resources_proto::ScheduleSpec {
            kind,
            cron,
            interval_seconds,
            run_at,
            timezone,
            target: Some(resources_proto::ScheduleTarget {
                agent,
                workflow: String::new(),
                session_mode,
                session_id,
            }),
            input_message,
            input_json: String::new(),
            enabled,
        },
        existing
            .and_then(|schedule| schedule.status)
            .unwrap_or_default(),
        labels,
    );

    scheduling::initialize_schedule(&mut schedule, chrono::Utc::now())?;
    let next_run = schedule
        .status
        .as_ref()
        .and_then(|status| status.next_run_at)
        .and_then(chrono::DateTime::from_timestamp_micros);
    scheduling::persist_schedule(cp.kv.as_ref(), &schedule).await?;
    scheduling::arm_schedule(cp.scheduler.as_ref(), &mut schedule, next_run).await?;
    scheduling::persist_schedule(cp.kv.as_ref(), &schedule).await?;
    Ok(schedule)
}

fn schedule_json(schedule: &resources_proto::Schedule) -> Value {
    let spec = schedule.spec.as_ref();
    let status = schedule.status.as_ref();
    let target = spec.and_then(|spec| spec.target.as_ref());
    json!({
        "name": schedule.name(),
        "ns": schedule.namespace(),
        "spec": {
            "kind": spec.map(|spec| spec.kind.clone()).unwrap_or_default(),
            "cron": spec.map(|spec| spec.cron.clone()).unwrap_or_default(),
            "intervalSeconds": spec.map(|spec| spec.interval_seconds).unwrap_or_default(),
            "runAt": spec.map(|spec| spec.run_at.clone()).unwrap_or_default(),
            "timezone": spec.map(|spec| spec.timezone.clone()).unwrap_or_default(),
            "target": {
                "agent": target.map(|target| target.agent.clone()).unwrap_or_default(),
                "sessionMode": target.map(|target| target.session_mode.clone()).unwrap_or_default(),
                "sessionId": target.map(|target| target.session_id.clone()).unwrap_or_default(),
            },
            "inputMessage": spec.map(|spec| spec.input_message.clone()).unwrap_or_default(),
            "enabled": spec.map(|spec| spec.enabled).unwrap_or(false),
        },
        "status": status.map(|status| json!({
            "revision": status.revision,
            "backendArmed": status.backend_armed,
            "backendHandle": status.backend_handle,
            "nextRunAt": status.next_run_at,
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
            })).collect::<Vec<_>>()
        })).unwrap_or_else(|| json!({})),
        "labels": schedule.labels(),
    })
}

fn create_task(
    current_namespace: &str,
    current_agent: &str,
    args: &Value,
) -> Result<resources_proto::Task> {
    let namespace = opt_str(args, "namespace")
        .unwrap_or(current_namespace)
        .to_string();
    let title = req_str(args, "title")?.to_string();
    let description = req_str(args, "description")?.to_string();
    let delegate_name = req_str(args, "delegate_name")?.to_string();
    let delegate_namespace = opt_str(args, "delegate_namespace")
        .unwrap_or(current_namespace)
        .to_string();
    let task_type = opt_str(args, "type")
        .unwrap_or("agent_delegation")
        .trim()
        .to_string();
    let now = chrono::Utc::now().timestamp_micros();
    let name = unique_task_name(&title);
    let labels = HashMap::from([
        (
            delegation::LABEL_OWNER_NAME.to_string(),
            current_agent.to_string(),
        ),
        (
            delegation::LABEL_DELEGATE_NAME.to_string(),
            delegate_name.clone(),
        ),
    ]);
    let resource = resource_model::task_resource(
        namespace,
        name,
        resources_proto::TaskSpec {
            title,
            description,
            r#type: task_type,
            owner: Some(resources_proto::ResourceRef {
                namespace: current_namespace.to_string(),
                name: current_agent.to_string(),
            }),
            delegate: Some(resources_proto::ResourceRef {
                namespace: delegate_namespace.clone(),
                name: delegate_name.clone(),
            }),
        },
        resources_proto::TaskStatus {
            observed_generation: 0,
            phase: resources_proto::TaskPhase::Queued as i32,
            conditions: Vec::new(),
            progress_summary: "Task created; waiting for delegated execution.".to_string(),
            result_artifacts: Vec::new(),
            output_artifact_uris: Vec::new(),
            created_at: now,
            updated_at: now,
            completed_at: 0,
            expires_at: 0,
            execution_ref: Some(resources_proto::TaskExecutionRef {
                kind: "AGENT_SESSION".to_string(),
                namespace: delegate_namespace,
                name: delegate_name,
                session_id: String::new(),
                run_id: String::new(),
            }),
        },
        labels,
    );
    task_from_resource(resource).ok_or_else(|| anyhow!("invalid Task after create"))
}

async fn delegate_task(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
    spec: &manifests::AgentSpec,
) -> Result<resources_proto::Task> {
    let namespace = opt_str(args, "namespace")
        .unwrap_or(current_namespace)
        .to_string();
    let title = req_str(args, "title")?.to_string();
    let description = req_str(args, "description")?.to_string();
    if args.get("delegate_name").is_some() || args.get("delegate_namespace").is_some() {
        return Err(anyhow!(
            "delegate_task requires a declared A2A connection; delegate_name and delegate_namespace are not accepted"
        ));
    }
    let connection_name = req_str(args, "connection")?;
    let target = crate::harness::a2a::resolve_internal_connection(spec, connection_name)?;
    let task_type = opt_str(args, "type")
        .unwrap_or("agent_delegation")
        .trim()
        .to_string();
    let name = unique_task_name(&title);
    let req = delegation::TaskDelegationRequest {
        namespace,
        name,
        title,
        description,
        task_type,
        owner_namespace: current_namespace.to_string(),
        owner_name: current_agent.to_string(),
        owner_session_id: current_session.to_string(),
        connection_name: target.connection_name.clone(),
        delegate_namespace: target.target_namespace,
        delegate_name: target.target_agent,
    };
    let task = delegation::create_delegated_task(cp, req.clone()).await?;
    let labels = delegation::task_execution_labels(&req);
    let alias = format!("{}-1", req.connection_name);
    let opened = a2a_tools::open_or_reuse_wire(
        cp,
        current_namespace,
        current_agent,
        current_session,
        spec,
        &req.connection_name,
        &alias,
        labels.clone(),
    )
    .await
    .inspect_err(|err| {
        tracing::warn!(
            task_namespace = %req.namespace,
            task_name = %req.name,
            error = %err,
            "failed to open delegated Task A2A wire"
        );
    });
    let opened = match opened {
        Ok(opened) => opened,
        Err(err) => {
            let _ = delegation::mark_task_dispatch_failed(cp, &req, &err.to_string()).await;
            return Err(err);
        }
    };
    let sent = a2a_tools::send_wire_message(
        cp,
        current_namespace,
        current_agent,
        current_session,
        &opened.alias,
        &delegation::delegated_task_message(&req),
        &[],
        labels,
    )
    .await
    .inspect_err(|err| {
        tracing::warn!(
            task_namespace = %req.namespace,
            task_name = %req.name,
            error = %err,
            "failed to send delegated Task over A2A wire"
        );
    });
    let sent = match sent {
        Ok(sent) => sent,
        Err(err) => {
            let _ = delegation::mark_task_dispatch_failed(cp, &req, &err.to_string()).await;
            return Err(err);
        }
    };
    match delegation::mark_task_execution_started(
        cp,
        &req,
        &sent.reference.session_id,
        sent.submission_id.as_deref(),
    )
    .await
    {
        Ok(task) => Ok(task),
        Err(err) => {
            tracing::warn!(
                task_namespace = %req.namespace,
                task_name = %req.name,
                error = %err,
                "failed to update delegated Task execution status after A2A wire send"
            );
            Ok(task)
        }
    }
}

fn task_resource_from_task(task: resources_proto::Task) -> resources_proto::Resource {
    let namespace = task.namespace().to_string();
    let name = task.name().to_string();
    let labels = task.labels().clone();
    resource_model::task_resource(
        namespace,
        name,
        task.spec.unwrap_or_default(),
        task.status.unwrap_or_default(),
        labels,
    )
}

async fn task_output_artifact_uris_from_args(
    cp: &ControlPlane,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<Vec<String>> {
    let mut output_artifact_uris = Vec::new();
    if let Some(uri) = opt_str(args, "output_artifact_uri") {
        output_artifact_uris.push(uri.to_string());
    }
    if let Some(values) = args.get("output_artifact_uris") {
        let Some(values) = values.as_array() else {
            return Err(anyhow!("output_artifact_uris must be an array"));
        };
        for value in values {
            let Some(uri) = value.as_str() else {
                return Err(anyhow!("output_artifact_uris must contain strings"));
            };
            output_artifact_uris.push(uri.to_string());
        }
    }
    output_artifact_uris.sort();
    output_artifact_uris.dedup();
    for uri in &output_artifact_uris {
        resolve_artifact_uri(cp, current_agent, current_session, uri, OP_READ).await?;
    }
    Ok(output_artifact_uris)
}

fn update_task_status(
    status: &mut resources_proto::TaskStatus,
    args: &Value,
    output_artifact_uris: &[String],
) -> Result<()> {
    let now = chrono::Utc::now().timestamp_micros();
    if let Some(namespace) = opt_str(args, "execution_namespace") {
        status
            .execution_ref
            .get_or_insert_with(Default::default)
            .namespace = namespace.to_string();
    }
    if let Some(name) = opt_str(args, "execution_name") {
        status
            .execution_ref
            .get_or_insert_with(Default::default)
            .name = name.to_string();
    }
    if let Some(session_id) = opt_str(args, "execution_session_id") {
        let execution = status.execution_ref.get_or_insert_with(Default::default);
        execution.kind = "AGENT_SESSION".to_string();
        execution.session_id = session_id.to_string();
    }
    if let Some(run_id) = opt_str(args, "run_id") {
        status
            .execution_ref
            .get_or_insert_with(Default::default)
            .run_id = run_id.to_string();
    }
    if let Some(phase) = opt_str(args, "phase") {
        status.phase = parse_task_phase(phase)?;
    }
    if let Some(summary) = opt_str(args, "progress_summary") {
        status.progress_summary = summary.to_string();
    }
    status.updated_at = now;
    if is_terminal_phase(status.phase) && status.completed_at == 0 {
        status.completed_at = now;
        status.expires_at = now + 90 * 24 * 60 * 60 * 1_000_000;
    }
    for uri in output_artifact_uris {
        if !status.output_artifact_uris.contains(uri) {
            status.output_artifact_uris.push(uri.clone());
        }
    }
    Ok(())
}

fn task_from_resource(resource: resources_proto::Resource) -> Option<resources_proto::Task> {
    let spec = match resource.spec?.kind? {
        resources_proto::resource_spec::Kind::Task(spec) => spec,
        _ => return None,
    };
    let status = match resource.status.and_then(|status| status.kind) {
        Some(resources_proto::resource_status::Kind::Task(status)) => Some(status),
        _ => None,
    };
    Some(resources_proto::Task {
        metadata: resource.metadata,
        spec: Some(spec),
        status,
    })
}

fn task_matches(
    task: &resources_proto::Task,
    status_group: Option<&str>,
    phase: Option<&str>,
    owner_name: Option<&str>,
    delegate_name: Option<&str>,
) -> bool {
    let spec = task.spec.as_ref();
    let current_phase = task
        .status
        .as_ref()
        .map(|status| status.phase)
        .unwrap_or_default();
    if let Some(group) = status_group {
        let matches = match group.to_ascii_lowercase().as_str() {
            "active" => is_active_phase(current_phase),
            "terminal" => is_terminal_phase(current_phase),
            _ => false,
        };
        if !matches {
            return false;
        }
    }
    if let Some(phase) = phase {
        if parse_task_phase(phase).ok() != Some(current_phase) {
            return false;
        }
    }
    if owner_name.is_some_and(|name| {
        spec.and_then(|spec| spec.owner.as_ref())
            .map(|owner| owner.name.as_str())
            != Some(name)
    }) {
        return false;
    }
    if delegate_name.is_some_and(|name| {
        spec.and_then(|spec| spec.delegate.as_ref())
            .map(|delegate| delegate.name.as_str())
            != Some(name)
    }) {
        return false;
    }
    true
}

fn task_updated_at(resource: &resources_proto::Resource) -> i64 {
    match resource
        .status
        .as_ref()
        .and_then(|status| status.kind.as_ref())
    {
        Some(resources_proto::resource_status::Kind::Task(status)) => status.updated_at,
        _ => 0,
    }
}

fn task_json(task: &resources_proto::Task) -> Value {
    let spec = task.spec.as_ref();
    let status = task.status.as_ref();
    let owner = spec.and_then(|spec| spec.owner.as_ref());
    let delegate = spec.and_then(|spec| spec.delegate.as_ref());
    let execution = status.and_then(|status| status.execution_ref.as_ref());
    json!({
        "name": task.name(),
        "namespace": task.namespace(),
        "title": spec.map(|spec| spec.title.clone()).unwrap_or_default(),
        "description": spec.map(|spec| spec.description.clone()).unwrap_or_default(),
        "type": spec.map(|spec| spec.r#type.clone()).unwrap_or_default(),
        "owner": resource_ref_json(owner),
        "delegate": resource_ref_json(delegate),
        "executionRef": execution.map(|execution| json!({
            "kind": execution.kind,
            "namespace": execution.namespace,
            "name": execution.name,
            "sessionId": execution.session_id,
            "runId": execution.run_id,
        })).unwrap_or_else(|| json!({})),
        "phase": status.map(|status| task_phase_name(status.phase)).unwrap_or("UNSPECIFIED"),
        "statusGroup": status.map(|status| {
            if is_active_phase(status.phase) {
                "ACTIVE"
            } else if is_terminal_phase(status.phase) {
                "TERMINAL"
            } else {
                "UNKNOWN"
            }
        }).unwrap_or("UNKNOWN"),
        "progressSummary": status.map(|status| status.progress_summary.clone()).unwrap_or_default(),
        "resultArtifacts": status.map(|status| {
            status.result_artifacts.iter().map(file_object_ref_json).collect::<Vec<_>>()
        }).unwrap_or_default(),
        "outputArtifactUris": status.map(|status| {
            status.output_artifact_uris.clone()
        }).unwrap_or_default(),
        "createdAt": status.map(|status| status.created_at).unwrap_or_default(),
        "updatedAt": status.map(|status| status.updated_at).unwrap_or_default(),
        "completedAt": status.map(|status| status.completed_at).unwrap_or_default(),
        "expiresAt": status.map(|status| status.expires_at).unwrap_or_default(),
        "labels": task.labels(),
    })
}

fn file_object_ref_json(reference: &resources_proto::FileObjectRef) -> Value {
    json!({
        "key": reference.key,
        "mediaType": reference.media_type,
        "sizeBytes": reference.size_bytes,
        "sha256": reference.sha256,
        "filename": reference.filename,
        "metadata": reference.metadata,
    })
}

fn resource_ref_json(reference: Option<&resources_proto::ResourceRef>) -> Value {
    reference
        .map(|reference| {
            json!({
                "namespace": reference.namespace,
                "name": reference.name,
            })
        })
        .unwrap_or_else(|| json!({}))
}

fn unique_task_name(title: &str) -> String {
    let slug = task_name_slug(title, 48);
    format!("{slug}-{}", crate::control::uuid::unique_name("tsk"))
}

fn task_name_slug(title: &str, max_chars: usize) -> String {
    let mut slug = title
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "task" } else { slug };
    let trimmed = slug.chars().take(max_chars).collect::<String>();
    trimmed.trim_matches('-').to_string()
}

fn parse_task_phase(value: &str) -> Result<i32> {
    let phase = match value.trim().to_ascii_uppercase().as_str() {
        "" | "UNSPECIFIED" => resources_proto::TaskPhase::Unspecified,
        "QUEUED" => resources_proto::TaskPhase::Queued,
        "RUNNING" => resources_proto::TaskPhase::Running,
        "BLOCKED" => resources_proto::TaskPhase::Blocked,
        "NEEDS_REVIEW" | "NEEDS-REVIEW" => resources_proto::TaskPhase::NeedsReview,
        "SUCCEEDED" | "SUCCESS" | "COMPLETED" => resources_proto::TaskPhase::Succeeded,
        "FAILED" => resources_proto::TaskPhase::Failed,
        "CANCELED" | "CANCELLED" => resources_proto::TaskPhase::Canceled,
        "EXPIRED" => resources_proto::TaskPhase::Expired,
        other => return Err(anyhow!("unsupported task phase '{}'", other)),
    };
    Ok(phase as i32)
}

fn task_phase_name(value: i32) -> &'static str {
    match resources_proto::TaskPhase::try_from(value).ok() {
        Some(resources_proto::TaskPhase::Queued) => "QUEUED",
        Some(resources_proto::TaskPhase::Running) => "RUNNING",
        Some(resources_proto::TaskPhase::Blocked) => "BLOCKED",
        Some(resources_proto::TaskPhase::NeedsReview) => "NEEDS_REVIEW",
        Some(resources_proto::TaskPhase::Succeeded) => "SUCCEEDED",
        Some(resources_proto::TaskPhase::Failed) => "FAILED",
        Some(resources_proto::TaskPhase::Canceled) => "CANCELED",
        Some(resources_proto::TaskPhase::Expired) => "EXPIRED",
        _ => "UNSPECIFIED",
    }
}

fn is_active_phase(value: i32) -> bool {
    matches!(
        resources_proto::TaskPhase::try_from(value).ok(),
        Some(resources_proto::TaskPhase::Queued)
            | Some(resources_proto::TaskPhase::Running)
            | Some(resources_proto::TaskPhase::Blocked)
            | Some(resources_proto::TaskPhase::NeedsReview)
    )
}

fn is_terminal_phase(value: i32) -> bool {
    matches!(
        resources_proto::TaskPhase::try_from(value).ok(),
        Some(resources_proto::TaskPhase::Succeeded)
            | Some(resources_proto::TaskPhase::Failed)
            | Some(resources_proto::TaskPhase::Canceled)
            | Some(resources_proto::TaskPhase::Expired)
    )
}

async fn create_goal(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<data_proto::Goal> {
    let objective = req_str(args, "objective")?.to_string();
    let now = chrono::Utc::now().timestamp_micros();
    let goal = data_proto::Goal {
        id: crate::control::uuid::unique_name("goal"),
        namespace: current_namespace.to_string(),
        agent: current_agent.to_string(),
        session_id: current_session.to_string(),
        objective,
        success_criteria: string_vec(args.get("success_criteria")),
        phase: data_proto::GoalPhase::Running as i32,
        progress_summary: opt_str(args, "progress_summary")
            .unwrap_or("Goal created.")
            .to_string(),
        iteration: 0,
        max_iterations: args
            .get("max_iterations")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .try_into()
            .unwrap_or_default(),
        created_at: now,
        updated_at: now,
        completed_at: 0,
        blocked_reason: String::new(),
        labels: string_map(args.get("labels")),
        metadata: string_map(args.get("metadata")),
    };
    upsert_goal(cp, goal.clone()).await?;
    Ok(goal)
}

async fn get_goal_from_args(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<data_proto::Goal> {
    let namespace = opt_str(args, "namespace").unwrap_or(current_namespace);
    let agent = opt_str(args, "agent").unwrap_or(current_agent);
    let session_id = opt_str(args, "session_id").unwrap_or(current_session);
    let goal_id = req_str(args, "goal_id")?;
    load_goal(cp, namespace, agent, session_id, goal_id)
        .await?
        .ok_or_else(|| anyhow!("goal '{}' not found", goal_id))
}

async fn load_goal(
    cp: &ControlPlane,
    namespace: &str,
    agent: &str,
    session_id: &str,
    goal_id: &str,
) -> Result<Option<data_proto::Goal>> {
    cp.kv
        .get_msg::<data_proto::Goal>(&keys::goal(namespace, agent, session_id, goal_id))
        .await
}

async fn upsert_goal(cp: &ControlPlane, goal: data_proto::Goal) -> Result<()> {
    cp.kv
        .set_msg(
            &keys::goal(&goal.namespace, &goal.agent, &goal.session_id, &goal.id),
            &goal,
        )
        .await
}

async fn list_goals(
    cp: &ControlPlane,
    namespace: &str,
    agent: &str,
    session_id: &str,
    status_group: Option<&str>,
    phase: Option<&str>,
    limit: usize,
) -> Result<Vec<data_proto::Goal>> {
    list_session_goals(cp, namespace, agent, session_id, status_group, phase, limit).await
}

async fn list_session_goals(
    cp: &ControlPlane,
    namespace: &str,
    agent: &str,
    session_id: &str,
    status_group: Option<&str>,
    phase: Option<&str>,
    limit: usize,
) -> Result<Vec<data_proto::Goal>> {
    let mut goals = cp
        .kv
        .list_entries(&keys::goal_prefix(namespace, agent, session_id), None)
        .await?
        .into_iter()
        .filter_map(|(_, value)| data_proto::Goal::decode(value.as_slice()).ok())
        .filter(|goal| goal_matches(goal, status_group, phase))
        .collect::<Vec<_>>();
    goals.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    goals.truncate(limit);
    Ok(goals)
}

fn goal_matches(goal: &data_proto::Goal, status_group: Option<&str>, phase: Option<&str>) -> bool {
    if status_group
        .is_some_and(|current| !goal_status_group(goal.phase).eq_ignore_ascii_case(current))
    {
        return false;
    }
    if phase.is_some_and(|current| parse_goal_phase(current).ok() != Some(goal.phase)) {
        return false;
    }
    true
}

fn update_goal_from_args(goal: &mut data_proto::Goal, args: &Value) -> Result<()> {
    let now = chrono::Utc::now().timestamp_micros();
    if let Some(phase) = opt_str(args, "phase") {
        goal.phase = parse_goal_phase(phase)?;
    }
    if let Some(summary) = opt_str(args, "progress_summary") {
        goal.progress_summary = summary.to_string();
    }
    if let Some(iteration) = args.get("iteration").and_then(Value::as_i64) {
        goal.iteration = iteration.try_into().unwrap_or_default();
    }
    if let Some(blocked_reason) = opt_str(args, "blocked_reason") {
        goal.blocked_reason = blocked_reason.to_string();
    }
    goal.updated_at = now;
    if is_terminal_goal_phase(goal.phase) && goal.completed_at == 0 {
        goal.completed_at = now;
    }
    Ok(())
}

fn goal_json(goal: &data_proto::Goal) -> Value {
    json!({
        "id": goal.id,
        "namespace": goal.namespace,
        "agent": goal.agent,
        "sessionId": goal.session_id,
        "objective": goal.objective,
        "successCriteria": goal.success_criteria,
        "phase": goal_phase_name(goal.phase),
        "statusGroup": goal_status_group(goal.phase),
        "progressSummary": goal.progress_summary,
        "iteration": goal.iteration,
        "maxIterations": goal.max_iterations,
        "createdAt": goal.created_at,
        "updatedAt": goal.updated_at,
        "completedAt": goal.completed_at,
        "blockedReason": goal.blocked_reason,
        "labels": goal.labels,
        "metadata": goal.metadata,
    })
}

pub async fn active_goals_context(
    cp: &ControlPlane,
    namespace: &str,
    agent: &str,
    session_id: &str,
) -> Result<Option<String>> {
    let goals = list_goals(cp, namespace, agent, session_id, Some("active"), None, 20).await?;
    if goals.is_empty() {
        return Ok(None);
    }

    let mut lines = vec![
        "# Active Talon Goals".to_string(),
        "Keep these session-scoped objectives in view while deciding next steps.".to_string(),
    ];
    for goal in goals {
        lines.push(format!(
            "- {} [{}] {}",
            goal.id,
            goal_phase_name(goal.phase),
            goal.objective
        ));
        if !goal.success_criteria.is_empty() {
            lines.push(format!(
                "  Success criteria: {}",
                goal.success_criteria.join("; ")
            ));
        }
        if !goal.progress_summary.is_empty() {
            lines.push(format!("  Progress: {}", goal.progress_summary));
        }
        if !goal.blocked_reason.is_empty() {
            lines.push(format!("  Blocked reason: {}", goal.blocked_reason));
        }
    }
    Ok(Some(lines.join("\n")))
}

fn parse_goal_phase(value: &str) -> Result<i32> {
    let phase = match value.trim().to_ascii_uppercase().as_str() {
        "" | "UNSPECIFIED" => data_proto::GoalPhase::Unspecified,
        "RUNNING" => data_proto::GoalPhase::Running,
        "PAUSED" => data_proto::GoalPhase::Paused,
        "NEEDS_REVIEW" | "NEEDS-REVIEW" => data_proto::GoalPhase::NeedsReview,
        "SUCCEEDED" | "SUCCESS" | "COMPLETED" => data_proto::GoalPhase::Succeeded,
        "FAILED" => data_proto::GoalPhase::Failed,
        "BLOCKED" => data_proto::GoalPhase::Blocked,
        "CANCELED" | "CANCELLED" => data_proto::GoalPhase::Canceled,
        "EXPIRED" => data_proto::GoalPhase::Expired,
        other => return Err(anyhow!("unsupported goal phase '{}'", other)),
    };
    Ok(phase as i32)
}

fn goal_phase_name(value: i32) -> &'static str {
    match data_proto::GoalPhase::try_from(value).ok() {
        Some(data_proto::GoalPhase::Running) => "RUNNING",
        Some(data_proto::GoalPhase::Paused) => "PAUSED",
        Some(data_proto::GoalPhase::NeedsReview) => "NEEDS_REVIEW",
        Some(data_proto::GoalPhase::Succeeded) => "SUCCEEDED",
        Some(data_proto::GoalPhase::Failed) => "FAILED",
        Some(data_proto::GoalPhase::Blocked) => "BLOCKED",
        Some(data_proto::GoalPhase::Canceled) => "CANCELED",
        Some(data_proto::GoalPhase::Expired) => "EXPIRED",
        _ => "UNSPECIFIED",
    }
}

fn goal_status_group(value: i32) -> &'static str {
    if is_active_goal_phase(value) {
        "ACTIVE"
    } else if is_terminal_goal_phase(value) {
        "TERMINAL"
    } else {
        "UNKNOWN"
    }
}

fn is_active_goal_phase(value: i32) -> bool {
    matches!(
        data_proto::GoalPhase::try_from(value).ok(),
        Some(data_proto::GoalPhase::Running)
            | Some(data_proto::GoalPhase::Paused)
            | Some(data_proto::GoalPhase::NeedsReview)
            | Some(data_proto::GoalPhase::Blocked)
    )
}

fn is_terminal_goal_phase(value: i32) -> bool {
    matches!(
        data_proto::GoalPhase::try_from(value).ok(),
        Some(data_proto::GoalPhase::Succeeded)
            | Some(data_proto::GoalPhase::Failed)
            | Some(data_proto::GoalPhase::Canceled)
            | Some(data_proto::GoalPhase::Expired)
    )
}

fn req_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("'{}' is required", key))
}

fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn opt_usize(args: &Value, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
}

fn string_map(value: Option<&Value>) -> HashMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    value
                        .as_str()
                        .map(|current| (key.clone(), current.to_string()))
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default()
}

fn string_vec(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::trim))
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn artifact_content_bytes(args: &Value) -> Result<Vec<u8>> {
    if let Some(encoded) = opt_str(args, "content_base64") {
        return general_purpose::STANDARD
            .decode(encoded)
            .map_err(|err| anyhow!("content_base64 is invalid: {err}"));
    }
    Ok(args
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .as_bytes()
        .to_vec())
}

#[derive(Debug, Clone)]
struct ArtifactUri {
    namespace: String,
    agent: String,
    session_id: String,
    artifact_id: String,
}

impl ArtifactUri {
    fn encode(&self) -> String {
        format!(
            "artifact://{}/{}/{}/{}",
            self.namespace, self.agent, self.session_id, self.artifact_id
        )
    }
}

async fn resolve_artifact_uri(
    cp: &ControlPlane,
    current_agent: &str,
    current_session: &str,
    artifact_uri: &str,
    operation: &str,
) -> Result<(ArtifactUri, crate::gateway::rpc::data_proto::Artifact)> {
    let uri = parse_artifact_uri(artifact_uri)?;
    let artifact = cp
        .kv
        .get_msg::<crate::gateway::rpc::data_proto::Artifact>(&keys::artifact(
            &uri.namespace,
            &uri.agent,
            &uri.session_id,
            &uri.artifact_id,
        ))
        .await?
        .ok_or_else(|| anyhow!("Artifact '{}' not found", uri.artifact_id))?;
    authorize_artifact_access(
        cp,
        &uri,
        current_agent,
        current_session,
        operation,
        artifact_uri,
    )
    .await?;
    Ok((uri, artifact))
}

fn artifact_json(artifact: &crate::gateway::rpc::data_proto::Artifact) -> Value {
    let object_ref = artifact.object_ref.as_ref();
    json!({
        "id": artifact.id,
        "sessionId": artifact.session_id,
        "title": artifact.title,
        "mediaType": artifact.media_type,
        "createdByAgent": artifact.created_by_agent,
        "createdAt": artifact.created_at,
        "labels": artifact.labels,
        "metadata": artifact.metadata,
        "objectRef": object_ref.map(|object| json!({
            "key": object.key,
            "mediaType": object.media_type,
            "sizeBytes": object.size_bytes,
            "sha256": object.sha256,
            "filename": object.filename,
            "metadata": object.metadata,
        })).unwrap_or_else(|| json!(null))
    })
}

#[derive(Clone, Debug)]
struct ValidatedHttpUrl {
    url: url::Url,
    host: Option<String>,
    addrs: Vec<SocketAddr>,
}

fn http_client(target: &ValidatedHttpUrl) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("TalonResearchBot/0.1 (+https://impalasys.com)")
        .redirect(reqwest::redirect::Policy::none());
    if let Some(host) = target.host.as_deref() {
        builder = builder.resolve_to_addrs(host, &target.addrs);
    }
    builder
        .build()
        .map_err(|err| anyhow!("research HTTP client failed to build: {err}"))
}

fn validate_http_url(value: &str) -> Result<()> {
    let url = url::Url::parse(value).map_err(|err| anyhow!("invalid URL: {err}"))?;
    match url.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(anyhow!("unsupported URL scheme '{}'", scheme)),
    }
}

async fn validate_public_http_url(value: &str) -> Result<ValidatedHttpUrl> {
    validate_http_url(value)?;
    let url = url::Url::parse(value).map_err(|err| anyhow!("invalid URL: {err}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("URL host is required"))?
        .to_string();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("URL port is required"))?;

    if let Ok(ip) = host.parse::<IpAddr>() {
        ensure_public_ip(ip)?;
        return Ok(ValidatedHttpUrl {
            url,
            host: None,
            addrs: vec![SocketAddr::new(ip, port)],
        });
    }

    let mut addrs = tokio::net::lookup_host((host.clone(), port))
        .await
        .map_err(|err| anyhow!("failed to resolve URL host '{host}': {err}"))?;
    let mut public_addrs = Vec::new();
    for addr in addrs.by_ref() {
        ensure_public_ip(addr.ip())?;
        public_addrs.push(addr);
    }
    if public_addrs.is_empty() {
        return Err(anyhow!("URL host '{host}' resolved no addresses"));
    }
    Ok(ValidatedHttpUrl {
        url,
        host: Some(host),
        addrs: public_addrs,
    })
}

fn ensure_public_ip(ip: IpAddr) -> Result<()> {
    let blocked = match ip {
        IpAddr::V4(ip) => is_blocked_ipv4(ip),
        IpAddr::V6(ip) => is_blocked_ipv6(ip),
    };
    if blocked {
        Err(anyhow!("URL resolves to a non-public address"))
    } else {
        Ok(())
    }
}

fn is_blocked_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || octets[0] == 0
        || octets[0] >= 224
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 169 && octets[1] == 254)
}

fn is_blocked_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (ip.segments()[0] & 0xfe00) == 0xfc00
        || (ip.segments()[0] & 0xffc0) == 0xfe80
}

fn extract_title(html: &str) -> String {
    let lower = html.to_lowercase();
    let Some(start) = lower.find("<title") else {
        return String::new();
    };
    let Some(open_end) = lower[start..].find('>') else {
        return String::new();
    };
    let content_start = start + open_end + 1;
    let Some(close) = lower[content_start..].find("</title>") else {
        return String::new();
    };
    decode_html_entities(&html[content_start..content_start + close])
        .trim()
        .to_string()
}

fn compact_visible_text(input: &str, max_chars: usize) -> String {
    let without_scripts = remove_tag_blocks(input, "script");
    let without_styles = remove_tag_blocks(&without_scripts, "style");
    let mut text = String::with_capacity(without_styles.len().min(max_chars));
    let mut in_tag = false;
    let mut last_was_space = true;
    for ch in without_styles.chars() {
        match ch {
            '<' => {
                in_tag = true;
                if !last_was_space {
                    text.push(' ');
                    last_was_space = true;
                }
            }
            '>' => in_tag = false,
            _ if in_tag => {}
            _ if ch.is_whitespace() => {
                if !last_was_space {
                    text.push(' ');
                    last_was_space = true;
                }
            }
            _ => {
                text.push(ch);
                last_was_space = false;
            }
        }
        if text.len() >= max_chars {
            break;
        }
    }
    decode_html_entities(text.trim())
}

fn remove_tag_blocks(input: &str, tag: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let lower = input.to_lowercase();
    let open_prefix = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let mut pos = 0;
    while let Some(start_rel) = lower[pos..].find(&open_prefix) {
        let start = pos + start_rel;
        output.push_str(&input[pos..start]);
        if let Some(end_rel) = lower[start..].find(&close) {
            pos = start + end_rel + close.len();
        } else {
            pos = input.len();
            break;
        }
    }
    output.push_str(&input[pos..]);
    output
}

fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

fn extract_duckduckgo_results(html: &str, limit: usize) -> Vec<Value> {
    let mut results: Vec<Value> = Vec::new();
    let mut pos = 0;
    while results.len() < limit {
        let Some(class_rel) = html[pos..].find("result__a") else {
            break;
        };
        let class_pos = pos + class_rel;
        let anchor_start = html[..class_pos].rfind("<a").unwrap_or(class_pos);
        let Some(anchor_end_rel) = html[class_pos..].find("</a>") else {
            break;
        };
        let anchor_end = class_pos + anchor_end_rel + "</a>".len();
        let anchor = &html[anchor_start..anchor_end];
        let Some(href) = extract_attr(anchor, "href") else {
            pos = anchor_end;
            continue;
        };
        let Some(url) = normalize_search_result_url(&href) else {
            pos = anchor_end;
            continue;
        };
        let title = compact_visible_text(anchor, 500);
        if !title.is_empty() && !results.iter().any(|item| item["url"] == url) {
            results.push(json!({
                "title": title,
                "url": url
            }));
        }
        pos = anchor_end;
    }
    results
}

fn extract_attr(input: &str, attr: &str) -> Option<String> {
    let needle = format!("{}=\"", attr);
    let start = input.find(&needle)? + needle.len();
    let end = input[start..].find('"')?;
    Some(decode_html_entities(&input[start..start + end]))
}

fn normalize_search_result_url(href: &str) -> Option<String> {
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href.to_string());
    }
    let query_start = href.find("uddg=")? + "uddg=".len();
    let query = &href[query_start..];
    let value = query.split('&').next().unwrap_or(query);
    urlencoding::decode(value)
        .ok()
        .map(|value| value.into_owned())
}

fn default_access_expiry() -> i64 {
    access_expiry_from_ttl_seconds(24 * 60 * 60)
}

fn access_expiry_from_ttl_seconds(ttl_seconds: i64) -> i64 {
    if ttl_seconds <= 0 {
        return default_access_expiry();
    }
    let ttl_micros = ttl_seconds.min(MAX_ACCESS_TTL_SECONDS) * 1_000_000;
    chrono::Utc::now()
        .timestamp_micros()
        .saturating_add(ttl_micros)
}

fn normalize_logical_path(path: &str) -> Result<String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(anyhow!("path is required"));
    }
    if !path.starts_with('/') {
        return Err(anyhow!("path must be absolute"));
    }
    if path.contains("//") || path.contains('\0') || path.contains("..") {
        return Err(anyhow!("path is not normalized"));
    }
    Ok(path.trim_end_matches('/').to_string())
}

fn parse_artifact_uri(uri: &str) -> Result<ArtifactUri> {
    let rest = uri
        .trim()
        .strip_prefix("artifact://")
        .ok_or_else(|| anyhow!("artifact uri must start with 'artifact://'"))?;
    let parts = rest.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        [namespace, agent, session_id, artifact_id] => Ok(ArtifactUri {
            namespace: validate_uri_segment(namespace, "artifact namespace")?,
            agent: validate_uri_segment(agent, "artifact agent")?,
            session_id: validate_uri_segment(session_id, "artifact session")?,
            artifact_id: validate_uri_segment(artifact_id, "artifact id")?,
        }),
        _ => Err(anyhow!(
            "artifact uri must be artifact://<namespace>/<agent>/<session>/<artifact>"
        )),
    }
}

fn validate_uri_segment(segment: &str, name: &str) -> Result<String> {
    if segment.trim().is_empty()
        || segment.contains('/')
        || segment.contains('\0')
        || segment.chars().any(char::is_control)
    {
        return Err(anyhow!("{name} segment is invalid"));
    }
    Ok(segment.to_string())
}

async fn authorize_artifact_access(
    cp: &ControlPlane,
    uri: &ArtifactUri,
    current_agent: &str,
    current_session: &str,
    operation: &str,
    artifact_uri: &str,
) -> Result<()> {
    if current_agent == uri.agent && current_session == uri.session_id {
        return Ok(());
    }
    if current_agent.trim().is_empty() || current_session.trim().is_empty() {
        return Err(anyhow!(
            "artifact uri requires caller agent and session identity"
        ));
    }
    let access = cp
        .kv
        .get_msg::<crate::gateway::rpc::data_proto::ArtifactAccess>(&keys::artifact_access(
            &uri.namespace,
            &uri.agent,
            &uri.session_id,
            &uri.artifact_id,
            current_agent,
            current_session,
        ))
        .await?
        .ok_or_else(|| anyhow!("artifact access denied for '{artifact_uri}'"))?;
    if access.expires_at > 0 && access.expires_at < chrono::Utc::now().timestamp_micros() {
        return Err(anyhow!("artifact access for '{artifact_uri}' is expired"));
    }
    if !access.operations.iter().any(|op| op == operation) {
        return Err(anyhow!(
            "artifact access for '{artifact_uri}' does not allow '{operation}'"
        ));
    }
    Ok(())
}

fn require_capability(spec: &manifests::AgentSpec, capability: &str, action: &str) -> Result<()> {
    if has_capability_action(spec, capability, action) {
        return Ok(());
    }
    Err(anyhow!(
        "agent does not have capability '{}:{}'",
        capability,
        action
    ))
}

fn require_memory_read(spec: &manifests::AgentSpec) -> Result<()> {
    if has_capability_action(spec, "memory", "read")
        || has_capability_action(spec, "memory", "inspect")
    {
        return Ok(());
    }
    Err(anyhow!("agent does not have capability 'memory:read'"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::scheduler::{ScheduleWakeupRequest, ScheduledWakeup, SchedulerBackend};
    use crate::control::KeyValueStore;
    use crate::test_support::{EmptyPubSub, MockKvStore};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockScheduler {
        scheduled: Mutex<Vec<ScheduleWakeupRequest>>,
        cancelled: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl SchedulerBackend for MockScheduler {
        async fn schedule(&self, req: ScheduleWakeupRequest) -> anyhow::Result<ScheduledWakeup> {
            self.scheduled.lock().await.push(req);
            Ok(ScheduledWakeup {
                handle: Some("handle-1".to_string()),
                armed: true,
            })
        }

        async fn cancel(&self, handle: &str) -> anyhow::Result<()> {
            self.cancelled.lock().await.push(handle.to_string());
            Ok(())
        }
    }

    fn spec(capabilities: &[&str]) -> manifests::AgentSpec {
        manifests::AgentSpec {
            features: Vec::new(),
            model_policy: None,
            system_prompt: String::new(),
            post_history_prompt: String::new(),
            mcp_server_refs: Vec::new(),
            capabilities: HashMap::from([(
                "schedules".to_string(),
                crate::gateway::rpc::protobuf_value::ListValue {
                    values: capabilities
                        .iter()
                        .map(|action| crate::gateway::rpc::protobuf_value::Value {
                            kind: Some(ProtoValueKind::StringValue((*action).to_string())),
                        })
                        .collect(),
                },
            )]),
            a2a: None,
            runtime: None,
        }
    }

    fn research_spec(capabilities: &[&str]) -> manifests::AgentSpec {
        manifests::AgentSpec {
            capabilities: HashMap::from([(
                "research".to_string(),
                crate::gateway::rpc::protobuf_value::ListValue {
                    values: capabilities
                        .iter()
                        .map(|action| crate::gateway::rpc::protobuf_value::Value {
                            kind: Some(ProtoValueKind::StringValue((*action).to_string())),
                        })
                        .collect(),
                },
            )]),
            ..manifests::AgentSpec::default()
        }
    }

    fn task_spec(capabilities: &[&str]) -> manifests::AgentSpec {
        manifests::AgentSpec {
            capabilities: HashMap::from([(
                "tasks".to_string(),
                crate::gateway::rpc::protobuf_value::ListValue {
                    values: capabilities
                        .iter()
                        .map(|action| crate::gateway::rpc::protobuf_value::Value {
                            kind: Some(ProtoValueKind::StringValue((*action).to_string())),
                        })
                        .collect(),
                },
            )]),
            ..manifests::AgentSpec::default()
        }
    }

    fn task_spec_with_internal_connection(
        capabilities: &[&str],
        connection: &str,
        namespace: &str,
        agent: &str,
    ) -> manifests::AgentSpec {
        let mut spec = task_spec(capabilities);
        spec.a2a = Some(manifests::A2a {
            connections: vec![manifests::Connection {
                name: connection.to_string(),
                target: Some(manifests::ConnectionRef {
                    internal: Some(manifests::InternalConnectionRef {
                        namespace: namespace.to_string(),
                        agent: agent.to_string(),
                    }),
                    external: None,
                }),
                ..Default::default()
            }],
            agent_card: None,
        });
        spec
    }

    fn task_spec_with_external_connection(
        capabilities: &[&str],
        connection: &str,
    ) -> manifests::AgentSpec {
        let mut spec = task_spec(capabilities);
        spec.a2a = Some(manifests::A2a {
            connections: vec![manifests::Connection {
                name: connection.to_string(),
                target: Some(manifests::ConnectionRef {
                    internal: None,
                    external: Some(manifests::ExternalConnectionRef {
                        agent_card_url: "https://example.com/agent-card.json".to_string(),
                    }),
                }),
                ..Default::default()
            }],
            agent_card: None,
        });
        spec
    }

    fn goal_spec(capabilities: &[&str]) -> manifests::AgentSpec {
        manifests::AgentSpec {
            capabilities: HashMap::from([(
                "goals".to_string(),
                crate::gateway::rpc::protobuf_value::ListValue {
                    values: capabilities
                        .iter()
                        .map(|action| crate::gateway::rpc::protobuf_value::Value {
                            kind: Some(ProtoValueKind::StringValue((*action).to_string())),
                        })
                        .collect(),
                },
            )]),
            ..manifests::AgentSpec::default()
        }
    }

    fn control_plane(kv: Arc<MockKvStore>, scheduler: Arc<MockScheduler>) -> ControlPlane {
        ControlPlane::builder(kv, Arc::new(EmptyPubSub))
            .scheduler(scheduler)
            .build()
    }

    async fn seed_agent(kv: &MockKvStore, ns: &str, name: &str) {
        kv.set_msg(
            &keys::agent(ns, name),
            &resource_model::agent(ns, name, manifests::AgentSpec::default(), HashMap::new()),
        )
        .await
        .unwrap();
    }

    async fn seed_session(kv: &MockKvStore, ns: &str, agent: &str, session_id: &str) {
        let now = chrono::Utc::now().timestamp_micros();
        kv.set_msg(
            &keys::session(ns, agent, session_id),
            &data_proto::Session {
                id: session_id.to_string(),
                agent: agent.to_string(),
                ns: ns.to_string(),
                status: "IDLE".to_string(),
                created_at: now,
                last_active: now,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
    }

    async fn set_session_status(
        kv: &MockKvStore,
        ns: &str,
        agent: &str,
        session_id: &str,
        status: &str,
    ) {
        let key = keys::session(ns, agent, session_id);
        let mut session = kv
            .get_msg::<data_proto::Session>(&key)
            .await
            .unwrap()
            .unwrap();
        session.status = status.to_string();
        kv.set_msg(&key, &session).await.unwrap();
    }

    async fn session_text_messages(
        kv: &MockKvStore,
        ns: &str,
        agent: &str,
        session_id: &str,
    ) -> Vec<String> {
        let entries = kv
            .list_entries(&keys::session_message_prefix(ns, agent, session_id), None)
            .await
            .unwrap();
        entries
            .into_iter()
            .filter_map(|(_, bytes)| data_proto::SessionMessage::decode(bytes.as_slice()).ok())
            .flat_map(|message| message.parts.into_iter())
            .filter(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
            .map(|part| part.content)
            .collect()
    }

    fn skill(ns: &str, name: &str, description: &str, instructions: &str) -> NamespaceSkill {
        NamespaceSkill {
            name: name.to_string(),
            namespace: ns.to_string(),
            description: description.to_string(),
            instructions: instructions.to_string(),
        }
    }

    fn skill_resource(
        ns: &str,
        name: &str,
        description: &str,
        instructions: &str,
    ) -> resources_proto::Skill {
        namespace::skill_resource(ns, name, description, instructions)
    }

    #[test]
    fn register_tools_respects_capabilities() {
        let mut registry = ToolRegistry::new();
        register_tools(&mut registry, &spec(&["inspect", "create"]));

        assert!(registry.get_tool(LIST_SCHEDULES_TOOL).is_some());
        assert!(registry.get_tool(GET_SCHEDULE_TOOL).is_some());
        assert!(registry.get_tool(CREATE_SCHEDULE_TOOL).is_some());
        assert!(registry.get_tool(UPDATE_SCHEDULE_TOOL).is_none());
        assert!(registry.get_tool(DELETE_SCHEDULE_TOOL).is_none());
    }

    #[test]
    fn register_research_tools_respects_capabilities() {
        let mut registry = ToolRegistry::new();
        register_tools(&mut registry, &research_spec(&["fetch_url"]));

        assert!(registry.get_tool(FETCH_URL_TOOL).is_some());
        assert!(registry.get_tool(WEB_SEARCH_TOOL).is_none());
    }

    #[test]
    fn delegate_task_schema_uses_internal_a2a_connection_enum() {
        let mut registry = ToolRegistry::new();
        register_tools(
            &mut registry,
            &task_spec_with_internal_connection(
                &["create"],
                "worker",
                "Tenant:acme:Operations",
                "support-agent",
            ),
        );

        let tool = registry
            .get_tool(DELEGATE_TASK_TOOL)
            .expect("delegate_task should be registered");
        assert_eq!(
            tool.input_schema["properties"]["connection"]["enum"],
            json!(["worker"])
        );
        assert!(tool.input_schema["properties"]
            .as_object()
            .unwrap()
            .get("delegate_name")
            .is_none());
        assert!(tool.input_schema["properties"]
            .as_object()
            .unwrap()
            .get("delegate_namespace")
            .is_none());
    }

    #[test]
    fn delegate_task_not_registered_without_internal_a2a_connection() {
        let mut no_connection_registry = ToolRegistry::new();
        register_tools(&mut no_connection_registry, &task_spec(&["create"]));
        assert!(no_connection_registry
            .get_tool(DELEGATE_TASK_TOOL)
            .is_none());

        let mut external_registry = ToolRegistry::new();
        register_tools(
            &mut external_registry,
            &task_spec_with_external_connection(&["create"], "remote"),
        );
        assert!(external_registry.get_tool(DELEGATE_TASK_TOOL).is_none());
    }

    #[test]
    fn agent_wire_schemas_use_internal_a2a_connection_enum_without_task_capability() {
        let mut registry = ToolRegistry::new();
        let mut spec = manifests::AgentSpec::default();
        spec.a2a = Some(manifests::A2a {
            connections: vec![manifests::Connection {
                name: "critic".to_string(),
                target: Some(manifests::ConnectionRef {
                    internal: Some(manifests::InternalConnectionRef {
                        namespace: "Tenant:acme:Copywriter".to_string(),
                        agent: "critic-agent".to_string(),
                    }),
                    external: None,
                }),
                ..Default::default()
            }],
            agent_card: None,
        });

        register_tools(&mut registry, &spec);

        let tool = registry
            .get_tool(AGENT_OPEN_TOOL)
            .expect("agent_open should be registered");
        assert_eq!(
            tool.input_schema["properties"]["connection"]["enum"],
            json!(["critic"])
        );
        assert!(registry.get_tool(AGENT_SEND_TOOL).is_some());
        assert!(registry.get_tool(DELEGATE_TASK_TOOL).is_none());
    }

    #[test]
    fn agent_open_not_registered_without_internal_a2a_connection() {
        let mut no_connection_registry = ToolRegistry::new();
        register_tools(
            &mut no_connection_registry,
            &manifests::AgentSpec::default(),
        );
        assert!(no_connection_registry.get_tool(AGENT_OPEN_TOOL).is_none());
        assert!(no_connection_registry.get_tool(AGENT_SEND_TOOL).is_some());

        let mut external_registry = ToolRegistry::new();
        register_tools(
            &mut external_registry,
            &task_spec_with_external_connection(&["create"], "remote"),
        );
        assert!(external_registry.get_tool(AGENT_OPEN_TOOL).is_none());
        assert!(external_registry.get_tool(AGENT_SEND_TOOL).is_some());
    }

    #[test]
    fn validate_http_url_rejects_non_http_schemes() {
        assert!(validate_http_url("https://example.com/path").is_ok());
        assert!(validate_http_url("http://example.com/path").is_ok());
        assert!(validate_http_url("file:///etc/passwd").is_err());
        assert!(validate_http_url("not a url").is_err());
    }

    #[test]
    fn public_ip_validation_rejects_private_and_metadata_ranges() {
        assert!(ensure_public_ip("8.8.8.8".parse().unwrap()).is_ok());
        assert!(ensure_public_ip("10.0.0.1".parse().unwrap()).is_err());
        assert!(ensure_public_ip("127.0.0.1".parse().unwrap()).is_err());
        assert!(ensure_public_ip("169.254.169.254".parse().unwrap()).is_err());
        assert!(ensure_public_ip("::1".parse().unwrap()).is_err());
        assert!(ensure_public_ip("fc00::1".parse().unwrap()).is_err());
    }

    #[test]
    fn parse_artifact_uri_accepts_literal_namespace_segments() {
        let parsed = parse_artifact_uri(
            "artifact://Tenant:acme:Workspace:main/copywriter/session-1/artifact-1",
        )
        .unwrap();

        assert_eq!(parsed.namespace, "Tenant:acme:Workspace:main");
        assert_eq!(parsed.agent, "copywriter");
        assert_eq!(parsed.session_id, "session-1");
        assert_eq!(parsed.artifact_id, "artifact-1");
    }

    #[test]
    fn access_expiry_clamps_requested_ttl() {
        let now = chrono::Utc::now().timestamp_micros();
        let expires_at = access_expiry_from_ttl_seconds(i64::MAX);
        let max_delta = (MAX_ACCESS_TTL_SECONDS * 1_000_000) + 1_000_000;

        assert!(expires_at >= now);
        assert!(expires_at - now <= max_delta);
    }

    #[test]
    fn compact_visible_text_removes_scripts_styles_and_tags() {
        let html = r#"
            <html>
              <head>
                <title>Research &amp; Notes</title>
                <style>.hidden { display: none; }</style>
              </head>
              <body>
                <script>alert("nope")</script>
                <h1>Useful&nbsp;Heading</h1>
                <p>Visible <strong>claim</strong>.</p>
              </body>
            </html>
        "#;

        assert_eq!(extract_title(html), "Research & Notes");
        let text = compact_visible_text(html, 1_000);
        assert!(text.contains("Useful Heading"));
        assert!(text.contains("Visible"));
        assert!(text.contains("claim"));
        assert!(!text.contains("alert"));
        assert!(!text.contains("display"));
    }

    #[test]
    fn extract_duckduckgo_results_decodes_redirect_urls() {
        let html = r#"
            <a rel="nofollow" class="result__a"
               href="/l/?kh=-1&uddg=https%3A%2F%2Fexample.com%2Fpost%3Fx%3D1">
               Example &amp; Result
            </a>
            <a class="result__a" href="https://direct.example/page">
               Direct Result
            </a>
        "#;

        let results = extract_duckduckgo_results(html, 5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["title"], "Example & Result");
        assert_eq!(results[0]["url"], "https://example.com/post?x=1");
        assert_eq!(results[1]["url"], "https://direct.example/page");
    }

    #[test]
    fn register_skill_tools_uses_effective_skill_name_enum() {
        let mut registry = ToolRegistry::new();
        register_skill_tools(
            &mut registry,
            &[
                skill("acme", "review", "Review code", "parent"),
                skill("acme", "release", "Release notes", "release"),
            ],
        );

        let tool = registry
            .get_tool(ACTIVATE_SKILL_TOOL)
            .expect("activation tool should be registered");
        assert_eq!(
            tool.input_schema["properties"]["name"]["enum"],
            json!(["review", "release"])
        );
    }

    #[test]
    fn register_skill_tools_skips_empty_catalog() {
        let mut registry = ToolRegistry::new();

        register_skill_tools(&mut registry, &[]);

        assert!(registry.get_tool(ACTIVATE_SKILL_TOOL).is_none());
    }

    #[tokio::test]
    async fn execute_tool_requires_capabilities() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let err = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &manifests::AgentSpec::default(),
            LIST_SCHEDULES_TOOL,
            &json!({}),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("agent does not have capability"));
    }

    #[tokio::test]
    async fn create_artifact_stores_canonical_session_owned_cas_object() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv.clone(), scheduler);
        let output = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Final draft",
                "content": "draft body",
                "media_type": "text/markdown",
                "metadata": {"source": "tool"}
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        let artifact_id = value["artifact"]["id"].as_str().unwrap();
        let artifact_uri = value["artifactUri"].as_str().unwrap();
        let object_key = value["artifact"]["objectRef"]["key"].as_str().unwrap();

        assert!(object_key.starts_with("cas/Tenant%3Aacme%3AWorkspace%3Amain/artifacts/"));
        assert!(object_key.contains(artifact_id));

        let stored = crate::control::cas::CasStore::new(cp.objects.clone())
            .get_object_decoded(object_key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.bytes, b"draft body");
        assert_eq!(stored.metadata.filename, "");
        assert_eq!(
            stored.metadata.metadata[crate::control::cas::METADATA_KIND],
            crate::control::cas::METADATA_KIND_ARTIFACT
        );
        assert_eq!(
            stored.metadata.metadata[crate::control::cas::METADATA_AGENT],
            "writer"
        );
        assert_eq!(stored.metadata.metadata["session_id"], "session-1");
        assert_eq!(stored.metadata.metadata["source"], "tool");

        let parsed_uri = parse_artifact_uri(artifact_uri).unwrap();
        assert_eq!(parsed_uri.namespace, "Tenant:acme:Workspace:main");
        assert_eq!(parsed_uri.agent, "writer");
        assert_eq!(parsed_uri.session_id, "session-1");
        assert_eq!(parsed_uri.artifact_id, artifact_id);

        let empty_create = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Missing content",
            }),
        )
        .await
        .unwrap_err();
        assert!(empty_create
            .to_string()
            .contains("requires content or content_base64"));

        let read_output = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            READ_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let read_value: Value = serde_json::from_str(&read_output).unwrap();
        assert_eq!(read_value["content"], "draft body");

        let update_output = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            UPDATE_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
                "content": "revised body",
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let update_value: Value = serde_json::from_str(&update_output).unwrap();
        assert_eq!(update_value["artifactUri"], artifact_uri);
        let updated_object_key = update_value["artifact"]["objectRef"]["key"]
            .as_str()
            .unwrap();
        assert_ne!(updated_object_key, object_key);
        assert!(crate::control::cas::CasStore::new(cp.objects.clone())
            .get_object_decoded(object_key)
            .await
            .unwrap()
            .is_none());

        let read_updated_output = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            READ_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let read_updated_value: Value = serde_json::from_str(&read_updated_output).unwrap();
        assert_eq!(read_updated_value["content"], "revised body");

        let empty_update = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            UPDATE_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
            }),
        )
        .await
        .unwrap_err();
        assert!(empty_update
            .to_string()
            .contains("requires content or content_base64"));

        execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            GRANT_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
                "target_agent": "critic",
                "target_session_id": "session-2",
                "operations": ["read", "metadata"],
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let stored_access = kv
            .get_msg::<crate::gateway::rpc::data_proto::ArtifactAccess>(&keys::artifact_access(
                "Tenant:acme:Workspace:main",
                "writer",
                "session-1",
                artifact_id,
                "critic",
                "session-2",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored_access.target_agent, "critic");
        assert_eq!(stored_access.target_session_id, "session-2");
        assert_eq!(stored_access.operations, vec!["read", "metadata"]);

        let update_denied = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "critic",
            "session-2",
            &manifests::AgentSpec::default(),
            UPDATE_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
                "content": "critic overwrite",
            }),
        )
        .await
        .unwrap_err();
        assert!(update_denied.to_string().contains("only the owning"));

        let cross_namespace_update_denied = execute_tool_for_session(
            &cp,
            "Tenant:other:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            UPDATE_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
                "content": "cross-tenant overwrite",
            }),
        )
        .await
        .unwrap_err();
        assert!(cross_namespace_update_denied
            .to_string()
            .contains("only the owning artifact namespace/agent/session"));
    }

    #[tokio::test]
    async fn agent_send_with_artifact_uri_grants_receiver_artifact_access() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        seed_agent(kv.as_ref(), "Tenant:acme:Workspace:main", "writer").await;
        seed_agent(kv.as_ref(), "Tenant:acme:Workspace:main", "critic").await;
        seed_session(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "writer",
            "writer-session",
        )
        .await;
        let cp = control_plane(kv.clone(), scheduler);
        let writer_spec = task_spec_with_internal_connection(
            &[],
            "critic",
            "Tenant:acme:Workspace:main",
            "critic",
        );

        let opened = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "writer-session",
            &writer_spec,
            AGENT_OPEN_TOOL,
            &json!({"connection": "critic"}),
        )
        .await
        .unwrap()
        .unwrap();
        let opened: Value = serde_json::from_str(&opened).unwrap();
        assert_eq!(opened["name"], "critic-1");
        let critic_ref = a2a_tools::load_wire_ref(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "writer-session",
            "critic-1",
        )
        .await
        .unwrap()
        .expect("critic wire should exist");

        let artifact = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "writer-session",
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Draft",
                "content": "# Draft\n\nPlease review.",
                "media_type": "text/markdown"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let artifact: Value = serde_json::from_str(&artifact).unwrap();
        let artifact_uri = artifact["artifactUri"].as_str().unwrap();
        let artifact_id = artifact["artifact"]["id"].as_str().unwrap();

        let denied = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "critic",
            &critic_ref.session_id,
            &manifests::AgentSpec::default(),
            READ_ARTIFACT_TOOL,
            &json!({ "artifact_uri": artifact_uri }),
        )
        .await
        .unwrap_err();
        assert!(denied.to_string().contains("artifact access denied"));

        let sent = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "writer-session",
            &writer_spec,
            AGENT_SEND_TOOL,
            &json!({
                "target": "critic-1",
                "message": "Please review the draft.",
                "artifact_uri": artifact_uri
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(!sent.contains("Please review the draft."));
        let sent: Value = serde_json::from_str(&sent).unwrap();
        if sent.get("status").is_some() {
            assert_eq!(sent["status"], "DISPATCHED");
            assert_eq!(sent["artifactCount"], 1);
            assert!(!sent.to_string().contains(artifact_uri));
        } else {
            assert_eq!(sent["dispatched"], true);
            assert_eq!(sent["artifactUris"], json!([artifact_uri]));
        }

        let access = kv
            .get_msg::<data_proto::ArtifactAccess>(&keys::artifact_access(
                "Tenant:acme:Workspace:main",
                "writer",
                "writer-session",
                artifact_id,
                "critic",
                &critic_ref.session_id,
            ))
            .await
            .unwrap()
            .expect("agent_send should grant artifact access to target session");
        assert_eq!(access.operations, vec!["read", "metadata"]);
        assert_eq!(access.granted_by_agent, "writer");
        assert_eq!(access.granted_by_session_id, "writer-session");

        let read = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "critic",
            &critic_ref.session_id,
            &manifests::AgentSpec::default(),
            READ_ARTIFACT_TOOL,
            &json!({ "artifact_uri": artifact_uri }),
        )
        .await
        .unwrap()
        .unwrap();
        let read: Value = serde_json::from_str(&read).unwrap();
        assert_eq!(read["content"], "# Draft\n\nPlease review.");

        let messages = session_text_messages(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "critic",
            &critic_ref.session_id,
        )
        .await;
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("Please review the draft."));
        assert!(messages[0].contains("Attached artifacts:"));
        assert!(messages[0].contains(artifact_uri));
    }

    #[tokio::test]
    async fn artifact_tools_accept_large_string_content_arguments() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let large_content = format!("# Large Artifact\n\n{}", "0123456789abcdef ".repeat(700));
        assert!(
            large_content.len() > 10_000,
            "test must exercise a 10k+ string argument"
        );

        let output = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Large draft",
                "content": large_content,
                "media_type": "text/markdown"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        let artifact_uri = value["artifactUri"].as_str().unwrap();

        let read_output = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            READ_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let read_value: Value = serde_json::from_str(&read_output).unwrap();
        let actual_content = read_value["content"].as_str().unwrap();
        assert_eq!(
            actual_content.len(),
            large_content.len(),
            "large create content length mismatch; actual_suffix={:?} expected_suffix={:?}",
            &actual_content[actual_content.len().saturating_sub(64)..],
            &large_content[large_content.len().saturating_sub(64)..]
        );
        assert!(
            actual_content == large_content,
            "large create content mismatch; actual_suffix={:?} expected_suffix={:?}",
            &actual_content[actual_content.len().saturating_sub(64)..],
            &large_content[large_content.len().saturating_sub(64)..]
        );

        let large_revision = format!("# Large Revision\n\n{}", "fedcba9876543210 ".repeat(700));
        assert!(large_revision.len() > 10_000);
        execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            UPDATE_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
                "content": large_revision,
            }),
        )
        .await
        .unwrap()
        .unwrap();

        let read_revision = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "writer",
            "session-1",
            &manifests::AgentSpec::default(),
            READ_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let read_revision: Value = serde_json::from_str(&read_revision).unwrap();
        let actual_revision = read_revision["content"].as_str().unwrap();
        assert_eq!(
            actual_revision.len(),
            large_revision.len(),
            "large update content length mismatch; actual_suffix={:?} expected_suffix={:?}",
            &actual_revision[actual_revision.len().saturating_sub(64)..],
            &large_revision[large_revision.len().saturating_sub(64)..]
        );
        assert!(
            actual_revision == large_revision,
            "large update content mismatch; actual_suffix={:?} expected_suffix={:?}",
            &actual_revision[actual_revision.len().saturating_sub(64)..],
            &large_revision[large_revision.len().saturating_sub(64)..]
        );
    }

    #[tokio::test]
    async fn activate_skill_returns_shadowed_effective_instructions() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv.clone(), scheduler);
        kv.set_msg(
            &keys::skill("acme", "review"),
            &skill_resource("acme", "review", "Review code", "parent instructions"),
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::skill("acme:team", "review"),
            &skill_resource(
                "acme:team",
                "review",
                "Review code locally",
                "child instructions",
            ),
        )
        .await
        .unwrap();

        let activated = execute_tool(
            &cp,
            "acme:team",
            "assistant",
            &manifests::AgentSpec::default(),
            ACTIVATE_SKILL_TOOL,
            &json!({"name":"review"}),
        )
        .await
        .unwrap()
        .unwrap();

        assert!(activated.contains("ACTIVATED SKILL: review"));
        assert!(activated.contains("Source namespace: acme:team"));
        assert!(activated.contains("child instructions"));
        assert!(!activated.contains("parent instructions"));
    }

    #[tokio::test]
    async fn activate_skill_skips_unreadable_records() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv.clone(), scheduler);
        kv.set(&keys::skill("acme", "broken"), b"not-protobuf")
            .await
            .unwrap();
        kv.set_msg(
            &keys::skill("acme", "review"),
            &skill_resource("acme", "review", "Review code", "instructions"),
        )
        .await
        .unwrap();

        let activated = execute_tool(
            &cp,
            "acme",
            "assistant",
            &manifests::AgentSpec::default(),
            ACTIVATE_SKILL_TOOL,
            &json!({"name":"review"}),
        )
        .await
        .unwrap()
        .unwrap();

        assert!(activated.contains("instructions"));
    }

    #[tokio::test]
    async fn activate_skill_reports_missing_or_invalid_name() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv.clone(), scheduler);

        let invalid = execute_tool(
            &cp,
            "acme",
            "assistant",
            &manifests::AgentSpec::default(),
            ACTIVATE_SKILL_TOOL,
            &json!({}),
        )
        .await
        .unwrap_err();
        assert!(invalid.to_string().contains("'name' is required"));

        let missing = execute_tool(
            &cp,
            "acme",
            "assistant",
            &manifests::AgentSpec::default(),
            ACTIVATE_SKILL_TOOL,
            &json!({"name":"review"}),
        )
        .await
        .unwrap_err();
        assert!(missing
            .to_string()
            .contains("skill 'review' is not available"));
    }

    #[tokio::test]
    async fn create_get_list_update_and_delete_schedule_round_trip() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        seed_agent(kv.as_ref(), "conic:test", "assistant").await;
        let cp = control_plane(kv.clone(), scheduler.clone());
        let schedule_spec = spec(&["inspect", "create", "update", "delete"]);

        let created = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &schedule_spec,
            CREATE_SCHEDULE_TOOL,
            &json!({
                "name": "nightly",
                "kind": "every",
                "interval_seconds": 600,
                "input_message": "run report",
                "labels": {"tier":"prod"},
                "enabled": true
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(created.contains("\"name\": \"nightly\""));
        assert!(created.contains("\"backendArmed\": true"));
        assert_eq!(scheduler.scheduled.lock().await.len(), 1);

        let fetched = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &schedule_spec,
            GET_SCHEDULE_TOOL,
            &json!({"name":"nightly"}),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(fetched.contains("\"name\": \"nightly\""));
        assert!(fetched.contains("\"tier\": \"prod\""));

        let listed = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &schedule_spec,
            LIST_SCHEDULES_TOOL,
            &json!({"agent":"assistant","enabled":true}),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(listed.contains("\"schedules\""));
        assert!(listed.contains("\"nightly\""));

        let updated = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &schedule_spec,
            UPDATE_SCHEDULE_TOOL,
            &json!({
                "name": "nightly",
                "input_message": "run report v2",
                "session_mode": "reuse",
                "session_id": "session-1"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(updated.contains("run report v2"));
        assert_eq!(
            scheduler.cancelled.lock().await.clone(),
            vec!["handle-1".to_string()]
        );

        let deleted = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &schedule_spec,
            DELETE_SCHEDULE_TOOL,
            &json!({"name":"nightly"}),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(deleted.contains("\"success\": true"));
        assert!(kv
            .get_msg::<resources_proto::Schedule>(&keys::schedule("conic:test", "nightly"))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn list_schedules_honors_limit_and_namespace_override() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        seed_agent(kv.as_ref(), "conic:other", "assistant").await;
        let cp = control_plane(kv.clone(), scheduler);
        let create_spec = spec(&["inspect", "create"]);

        for name in ["a", "b"] {
            execute_tool(
                &cp,
                "conic:other",
                "assistant",
                &create_spec,
                CREATE_SCHEDULE_TOOL,
                &json!({
                    "namespace": "conic:other",
                    "name": name,
                    "kind": "every",
                    "interval_seconds": 600,
                    "input_message": "run report"
                }),
            )
            .await
            .unwrap();
        }

        let listed = execute_tool(
            &cp,
            "conic:test",
            "assistant",
            &spec(&["inspect"]),
            LIST_SCHEDULES_TOOL,
            &json!({"namespace":"conic:other","limit":1}),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(listed.matches("\"name\":").count(), 1);
    }

    #[tokio::test]
    async fn task_tools_create_update_and_list_active_work() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let spec = task_spec(&["inspect", "create", "update"]);

        let created = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            CREATE_TASK_TOOL,
            &json!({
                "title": "Prepare customer onboarding checklist",
                "description": "Create a reviewed onboarding checklist.",
                "type": "OPERATIONS",
                "delegate_namespace": "Tenant:acme:Operations",
                "delegate_name": "support-agent"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let name = created["task"]["name"].as_str().unwrap();
        assert_eq!(created["task"]["phase"], "QUEUED");
        assert_eq!(created["task"]["statusGroup"], "ACTIVE");
        assert_eq!(created["task"]["owner"]["name"], "ops-lead");
        assert_eq!(created["task"]["delegate"]["name"], "support-agent");

        let updated = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            UPDATE_TASK_TOOL,
            &json!({
                "name": name,
                "phase": "RUNNING",
                "progress_summary": "Support agent is preparing the checklist.",
                "execution_namespace": "Tenant:acme:Operations",
                "execution_name": "support-agent",
                "execution_session_id": "support-session-1"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let updated: Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(updated["task"]["phase"], "RUNNING");
        assert_eq!(updated["task"]["executionRef"]["name"], "support-agent");
        assert_eq!(
            updated["task"]["executionRef"]["sessionId"],
            "support-session-1"
        );

        let listed = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-3",
            &spec,
            LIST_TASKS_TOOL,
            &json!({
                "status_group": "active",
                "owner_name": "ops-lead"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let listed: Value = serde_json::from_str(&listed).unwrap();
        assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(listed["tasks"][0]["name"], name);
    }

    #[tokio::test]
    async fn task_tools_reject_cross_namespace_overrides() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let spec = task_spec_with_internal_connection(
            &["inspect", "create"],
            "support",
            "Tenant:acme:Operations",
            "support-agent",
        );

        let create_err = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            CREATE_TASK_TOOL,
            &json!({
                "namespace": "Tenant:other:Workspace:main",
                "title": "Prepare checklist",
                "description": "Create a reviewed checklist."
            }),
        )
        .await
        .unwrap_err();
        assert!(create_err.to_string().contains("cannot target namespace"));

        let delegate_err = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "namespace": "Tenant:other:Workspace:main",
                "title": "Prepare checklist",
                "description": "Create a reviewed checklist.",
                "connection": "support"
            }),
        )
        .await
        .unwrap_err();
        assert!(delegate_err.to_string().contains("cannot target namespace"));
    }

    #[tokio::test]
    async fn delegate_task_starts_delegate_session() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        seed_agent(kv.as_ref(), "Tenant:acme:Operations", "support-agent").await;
        seed_session(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
        )
        .await;
        let cp = control_plane(kv.clone(), scheduler);
        let spec = task_spec_with_internal_connection(
            &["inspect", "create"],
            "support",
            "Tenant:acme:Operations",
            "support-agent",
        );

        let created = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Prepare customer onboarding checklist",
                "description": "Create a reviewed onboarding checklist.",
                "type": "OPERATIONS",
                "connection": "support"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let task = &created["task"];
        let task_name = task["name"].as_str().unwrap();
        let child_session_id = task["executionRef"]["sessionId"].as_str().unwrap();
        assert_eq!(task["phase"], "RUNNING");
        assert_eq!(task["executionRef"]["namespace"], "Tenant:acme:Operations");
        assert_eq!(task["executionRef"]["name"], "support-agent");
        assert!(!child_session_id.is_empty());
        assert!(!task["executionRef"]["runId"].as_str().unwrap().is_empty());

        let child_session = kv
            .get_msg::<data_proto::Session>(&keys::session(
                "Tenant:acme:Operations",
                "support-agent",
                child_session_id,
            ))
            .await
            .unwrap()
            .expect("child session should exist");
        assert_eq!(
            child_session.labels.get(delegation::LABEL_TASK_NAME),
            Some(&task_name.to_string())
        );
        assert_eq!(
            child_session.labels.get(delegation::LABEL_OWNER_NAMESPACE),
            Some(&"Tenant:acme:Workspace:main".to_string())
        );
        assert_eq!(
            child_session.labels.get(delegation::LABEL_OWNER_SESSION_ID),
            Some(&"session-1".to_string())
        );
        assert_eq!(
            child_session.labels.get(delegation::LABEL_A2A_CONNECTION),
            Some(&"support".to_string())
        );
        let owner_session = kv
            .get_msg::<data_proto::Session>(&keys::session(
                "Tenant:acme:Workspace:main",
                "ops-lead",
                "session-1",
            ))
            .await
            .unwrap()
            .expect("owner session should exist");
        let expected_wire_ref = format!("Tenant:acme:Operations/support-agent/{child_session_id}");
        assert_eq!(
            owner_session
                .metadata
                .get("wire.a2a.talon.impalasys.com/support-1")
                .map(String::as_str),
            Some(expected_wire_ref.as_str())
        );

        let store = ResourceStore::new(kv.clone(), Arc::new(EmptyPubSub));
        let task_resource = store
            .get("Tenant:acme:Workspace:main", "Task", task_name)
            .await
            .unwrap()
            .expect("delegated task resource should exist");
        assert_eq!(
            task_resource
                .metadata
                .as_ref()
                .unwrap()
                .labels
                .get(delegation::LABEL_A2A_CONNECTION),
            Some(&"support".to_string())
        );
        assert_eq!(
            task_resource.metadata.as_ref().unwrap().generation,
            1,
            "delegated status updates must not bump resource generation"
        );

        let entries = kv
            .list_entries(
                &keys::session_message_prefix(
                    "Tenant:acme:Operations",
                    "support-agent",
                    child_session_id,
                ),
                None,
            )
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        let message = data_proto::SessionMessage::decode(entries[0].1.as_slice()).unwrap();
        assert_eq!(
            message.labels.get(delegation::LABEL_TASK_NAME),
            Some(&task_name.to_string())
        );
        assert_eq!(
            message.labels.get(delegation::LABEL_A2A_CONNECTION),
            Some(&"support".to_string())
        );
        assert!(message
            .parts
            .first()
            .unwrap()
            .content
            .contains("Create a reviewed onboarding checklist."));

        let listed = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            LIST_TASKS_TOOL,
            &json!({
                "status_group": "active",
                "owner_name": "ops-lead"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let listed: Value = serde_json::from_str(&listed).unwrap();
        assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn delegate_task_completion_ignores_stale_child_sessions() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        seed_agent(kv.as_ref(), "Tenant:acme:Operations", "support-agent").await;
        seed_session(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
        )
        .await;
        let cp = control_plane(kv.clone(), scheduler);
        let spec = task_spec_with_internal_connection(
            &["create"],
            "support",
            "Tenant:acme:Operations",
            "support-agent",
        );

        let created = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Prepare checklist",
                "description": "Create a reviewed checklist.",
                "connection": "support"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let task_name = created["task"]["name"].as_str().unwrap();
        let child_session_id = created["task"]["executionRef"]["sessionId"]
            .as_str()
            .unwrap();
        let mut stale_session = kv
            .get_msg::<data_proto::Session>(&keys::session(
                "Tenant:acme:Operations",
                "support-agent",
                child_session_id,
            ))
            .await
            .unwrap()
            .unwrap();
        stale_session.id = "stale-session".to_string();

        delegation::complete_delegated_task_from_session(
            &cp,
            &stale_session,
            delegation::DelegatedSessionCompletion::Completed,
        )
        .await
        .unwrap();

        let store = ResourceStore::new(kv.clone(), Arc::new(EmptyPubSub));
        let task_resource = store
            .get("Tenant:acme:Workspace:main", "Task", task_name)
            .await
            .unwrap()
            .unwrap();
        let phase = match task_resource.status.unwrap().kind.unwrap() {
            resources_proto::resource_status::Kind::Task(status) => status.phase,
            _ => panic!("expected Task status"),
        };
        assert_eq!(phase, resources_proto::TaskPhase::Running as i32);
        assert!(session_text_messages(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
        )
        .await
        .is_empty());
    }

    #[tokio::test]
    async fn delegate_task_failure_does_not_auto_notify_owner_session() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        seed_agent(kv.as_ref(), "Tenant:acme:Operations", "support-agent").await;
        seed_session(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
        )
        .await;
        let cp = control_plane(kv.clone(), scheduler);
        let spec = task_spec_with_internal_connection(
            &["create"],
            "support",
            "Tenant:acme:Operations",
            "support-agent",
        );

        let created = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Prepare checklist",
                "description": "Create a reviewed checklist.",
                "connection": "support"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let task_name = created["task"]["name"].as_str().unwrap();
        let child_session_id = created["task"]["executionRef"]["sessionId"]
            .as_str()
            .unwrap();
        let child_session = kv
            .get_msg::<data_proto::Session>(&keys::session(
                "Tenant:acme:Operations",
                "support-agent",
                child_session_id,
            ))
            .await
            .unwrap()
            .unwrap();

        set_session_status(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            "PROCESSING",
        )
        .await;
        let task = delegation::complete_delegated_task_from_session(
            &cp,
            &child_session,
            delegation::DelegatedSessionCompletion::Failed,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            task.status.as_ref().unwrap().phase,
            resources_proto::TaskPhase::Failed as i32
        );
        assert_eq!(task.name(), task_name);
        assert!(session_text_messages(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
        )
        .await
        .is_empty());

        let owner_messages = session_text_messages(
            kv.as_ref(),
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
        )
        .await;
        assert!(
            owner_messages.is_empty(),
            "Task completion must not auto-send owner wake messages; delegates should use agent_send owner"
        );
    }

    #[tokio::test]
    async fn delegate_task_rejects_unknown_external_and_raw_delegate_targets() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let spec = task_spec_with_internal_connection(
            &["create"],
            "support",
            "Tenant:acme:Operations",
            "support-agent",
        );

        let unknown = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "connection": "missing",
                "title": "Prepare checklist",
                "description": "Create a reviewed checklist."
            }),
        )
        .await
        .unwrap_err();
        assert!(unknown.to_string().contains("valid connections: support"));

        let raw = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "connection": "support",
                "title": "Prepare checklist",
                "description": "Create a reviewed checklist.",
                "delegate_name": "support-agent"
            }),
        )
        .await
        .unwrap_err();
        assert!(raw.to_string().contains("delegate_name"));

        let external_spec = task_spec_with_external_connection(&["create"], "remote");
        let external = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &external_spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "connection": "remote",
                "title": "Prepare checklist",
                "description": "Create a reviewed checklist."
            }),
        )
        .await
        .unwrap_err();
        assert!(external.to_string().contains("external A2A connection"));
    }

    #[tokio::test]
    async fn nested_delegation_grants_task_output_artifacts_and_notifies_through_agent_send() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let namespace = "Tenant:acme:Workspace:main";
        seed_agent(kv.as_ref(), namespace, "router").await;
        seed_agent(kv.as_ref(), namespace, "writer").await;
        seed_session(kv.as_ref(), namespace, "owner", "owner-session").await;
        let cp = control_plane(kv.clone(), scheduler);

        let owner_spec =
            task_spec_with_internal_connection(&["create"], "router", namespace, "router");
        let parent = execute_tool_for_session(
            &cp,
            namespace,
            "owner",
            "owner-session",
            &owner_spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Review legal memo",
                "description": "Route the memo to a writing delegate and return the final artifact.",
                "connection": "router"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let parent: Value = serde_json::from_str(&parent).unwrap();
        let parent_task_name = parent["task"]["name"].as_str().unwrap();
        let router_session_id = parent["task"]["executionRef"]["sessionId"]
            .as_str()
            .unwrap();

        let router_spec = task_spec_with_internal_connection(
            &["create", "update"],
            "writer",
            namespace,
            "writer",
        );
        let child = execute_tool_for_session(
            &cp,
            namespace,
            "router",
            router_session_id,
            &router_spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Draft legal memo",
                "description": "Prepare the final legal memo artifact.",
                "connection": "writer"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let child: Value = serde_json::from_str(&child).unwrap();
        let child_task_name = child["task"]["name"].as_str().unwrap();
        let writer_session_id = child["task"]["executionRef"]["sessionId"].as_str().unwrap();

        let writer_artifact = execute_tool_for_session(
            &cp,
            namespace,
            "writer",
            writer_session_id,
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Final memo",
                "content": "# Final Memo\n\nThe agreement should be revised.",
                "media_type": "text/markdown"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let writer_artifact: Value = serde_json::from_str(&writer_artifact).unwrap();
        let artifact_uri = writer_artifact["artifactUri"].as_str().unwrap();
        let artifact_id = writer_artifact["artifact"]["id"].as_str().unwrap();

        let writer_update = execute_tool_for_session(
            &cp,
            namespace,
            "writer",
            writer_session_id,
            &task_spec(&["update"]),
            UPDATE_TASK_TOOL,
            &json!({
                "name": child_task_name,
                "phase": "NEEDS_REVIEW",
                "progress_summary": "Final memo is ready.",
                "output_artifact_uri": artifact_uri
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let writer_update: Value = serde_json::from_str(&writer_update).unwrap();
        assert_eq!(writer_update["task"]["outputArtifactUris"][0], artifact_uri);

        let wrong_task_update = execute_tool_for_session(
            &cp,
            namespace,
            "writer",
            writer_session_id,
            &task_spec(&["update"]),
            UPDATE_TASK_TOOL,
            &json!({
                "name": parent_task_name,
                "phase": "NEEDS_REVIEW",
                "progress_summary": "Writer should not be able to update the parent task."
            }),
        )
        .await
        .unwrap_err();
        assert!(wrong_task_update.to_string().contains("cannot target task"));

        let writer_session = kv
            .get_msg::<data_proto::Session>(&keys::session(namespace, "writer", writer_session_id))
            .await
            .unwrap()
            .unwrap();
        let mut stale_writer_session = writer_session.clone();
        stale_writer_session.id = "stale-writer-session".to_string();
        kv.set_msg(
            &keys::session(namespace, "writer", &stale_writer_session.id),
            &stale_writer_session,
        )
        .await
        .unwrap();
        let stale_update = execute_tool_for_session(
            &cp,
            namespace,
            "writer",
            &stale_writer_session.id,
            &task_spec(&["update"]),
            UPDATE_TASK_TOOL,
            &json!({
                "name": child_task_name,
                "phase": "NEEDS_REVIEW",
                "progress_summary": "Stale writer session should not be active."
            }),
        )
        .await
        .unwrap_err();
        assert!(stale_update
            .to_string()
            .contains("not the active execution session"));

        let writer_session = kv
            .get_msg::<data_proto::Session>(&keys::session(namespace, "writer", writer_session_id))
            .await
            .unwrap()
            .unwrap();
        let child_task = delegation::complete_delegated_task_from_session(
            &cp,
            &writer_session,
            delegation::DelegatedSessionCompletion::Completed,
        )
        .await
        .unwrap()
        .unwrap();
        let child_status = child_task.status.as_ref().unwrap();
        assert!(child_status.result_artifacts.is_empty());
        assert_eq!(child_status.output_artifact_uris, vec![artifact_uri]);

        let router_access = kv
            .get_msg::<crate::gateway::rpc::data_proto::ArtifactAccess>(&keys::artifact_access(
                namespace,
                "writer",
                writer_session_id,
                artifact_id,
                "router",
                router_session_id,
            ))
            .await
            .unwrap()
            .expect("update_task output artifact should grant access to the Task owner");
        assert_eq!(
            router_access.operations,
            vec!["read", "metadata", "promote"]
        );

        execute_tool_for_session(
            &cp,
            namespace,
            "writer",
            writer_session_id,
            &manifests::AgentSpec::default(),
            AGENT_SEND_TOOL,
            &json!({
                "target": "owner",
                "message": "Final memo is ready for router review."
            }),
        )
        .await
        .unwrap()
        .unwrap();

        let router_artifact = execute_tool_for_session(
            &cp,
            namespace,
            "router",
            router_session_id,
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Router notes",
                "content": "This should not be propagated.",
                "media_type": "text/plain"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let router_artifact: Value = serde_json::from_str(&router_artifact).unwrap();
        let router_artifact_id = router_artifact["artifact"]["id"].as_str().unwrap();

        let parent_update = execute_tool_for_session(
            &cp,
            namespace,
            "router",
            router_session_id,
            &task_spec(&["update"]),
            UPDATE_TASK_TOOL,
            &json!({
                "name": parent_task_name,
                "phase": "NEEDS_REVIEW",
                "progress_summary": "Final memo is ready for owner review.",
                "output_artifact_uris": [artifact_uri]
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let parent_update: Value = serde_json::from_str(&parent_update).unwrap();
        assert_eq!(parent_update["task"]["outputArtifactUris"][0], artifact_uri);

        let router_session = kv
            .get_msg::<data_proto::Session>(&keys::session(namespace, "router", router_session_id))
            .await
            .unwrap()
            .unwrap();
        let parent_task = delegation::complete_delegated_task_from_session(
            &cp,
            &router_session,
            delegation::DelegatedSessionCompletion::Completed,
        )
        .await
        .unwrap()
        .unwrap();
        let parent_status = parent_task.status.as_ref().unwrap();
        assert!(parent_status.result_artifacts.is_empty());
        assert_eq!(parent_status.output_artifact_uris, vec![artifact_uri]);

        let owner_access = kv
            .get_msg::<crate::gateway::rpc::data_proto::ArtifactAccess>(&keys::artifact_access(
                namespace,
                "writer",
                writer_session_id,
                artifact_id,
                "owner",
                "owner-session",
            ))
            .await
            .unwrap()
            .expect("update_task output artifact should grant access to the parent Task owner");
        assert_eq!(owner_access.operations, vec!["read", "metadata", "promote"]);

        execute_tool_for_session(
            &cp,
            namespace,
            "router",
            router_session_id,
            &manifests::AgentSpec::default(),
            AGENT_SEND_TOOL,
            &json!({
                "target": "owner",
                "message": "Final memo is ready for owner review."
            }),
        )
        .await
        .unwrap()
        .unwrap();

        let unrelated_owner_access = kv
            .get_msg::<crate::gateway::rpc::data_proto::ArtifactAccess>(&keys::artifact_access(
                namespace,
                "router",
                router_session_id,
                router_artifact_id,
                "owner",
                "owner-session",
            ))
            .await
            .unwrap();
        assert!(
            unrelated_owner_access.is_none(),
            "completion must not scan and propagate unrelated session artifacts"
        );

        let owner_messages =
            session_text_messages(kv.as_ref(), namespace, "owner", "owner-session").await;
        assert!(owner_messages
            .iter()
            .any(|message| message.contains("Final memo is ready for owner review.")));
    }

    #[tokio::test]
    async fn delegated_session_can_update_its_owner_namespace_task_only() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let owner_namespace = "Tenant:acme:Workspace:main";
        let delegate_namespace = "Tenant:acme:Nexus:copywriter";
        seed_agent(kv.as_ref(), owner_namespace, "cmo").await;
        seed_agent(kv.as_ref(), delegate_namespace, "copywriter").await;
        seed_session(kv.as_ref(), owner_namespace, "cmo", "owner-session").await;
        let cp = control_plane(kv.clone(), scheduler);

        let owner_spec = task_spec_with_internal_connection(
            &["create"],
            "copywriter",
            delegate_namespace,
            "copywriter",
        );
        let task = execute_tool_for_session(
            &cp,
            owner_namespace,
            "cmo",
            "owner-session",
            &owner_spec,
            DELEGATE_TASK_TOOL,
            &json!({
                "title": "Draft announcement",
                "description": "Create an announcement artifact and attach it to this task.",
                "connection": "copywriter"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let task: Value = serde_json::from_str(&task).unwrap();
        let task_name = task["task"]["name"].as_str().unwrap();
        let delegate_session_id = task["task"]["executionRef"]["sessionId"].as_str().unwrap();

        let artifact = execute_tool_for_session(
            &cp,
            delegate_namespace,
            "copywriter",
            delegate_session_id,
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "Announcement",
                "content": "# Announcement\n\nThe draft is ready.",
                "media_type": "text/markdown"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let artifact: Value = serde_json::from_str(&artifact).unwrap();
        let artifact_uri = artifact["artifactUri"].as_str().unwrap();

        let updated = execute_tool_for_session(
            &cp,
            delegate_namespace,
            "copywriter",
            delegate_session_id,
            &task_spec(&["update"]),
            UPDATE_TASK_TOOL,
            &json!({
                "namespace": owner_namespace,
                "name": task_name,
                "phase": "NEEDS_REVIEW",
                "progress_summary": "Draft announcement is ready.",
                "output_artifact_uri": artifact_uri
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let updated: Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(updated["task"]["outputArtifactUris"][0], artifact_uri);

        let updated_with_task_id = execute_tool_for_session(
            &cp,
            delegate_namespace,
            "copywriter",
            delegate_session_id,
            &task_spec(&["update"]),
            UPDATE_TASK_TOOL,
            &json!({
                "name": format!("{owner_namespace}/{task_name}"),
                "phase": "NEEDS_REVIEW",
                "progress_summary": "Draft announcement is still ready.",
                "output_artifact_uri": artifact_uri
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let updated_with_task_id: Value = serde_json::from_str(&updated_with_task_id).unwrap();
        assert_eq!(
            updated_with_task_id["task"]["outputArtifactUris"][0],
            artifact_uri
        );

        let rejected = execute_tool_for_session(
            &cp,
            delegate_namespace,
            "copywriter",
            delegate_session_id,
            &task_spec(&["update"]),
            UPDATE_TASK_TOOL,
            &json!({
                "namespace": owner_namespace,
                "name": "different-task",
                "phase": "NEEDS_REVIEW"
            }),
        )
        .await
        .unwrap_err();
        assert!(rejected.to_string().contains("cannot target task"));
    }

    #[tokio::test]
    async fn goal_tools_create_update_list_and_complete() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let spec = goal_spec(&["inspect", "create", "update"]);

        let created = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            CREATE_GOAL_TOOL,
            &json!({
                "objective": "Complete the onboarding checklist to review-ready quality.",
                "success_criteria": [
                    "Uses sourced product facts",
                    "Passes critic review"
                ],
                "max_iterations": 4
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let goal_id = created["goal"]["id"].as_str().unwrap();
        assert_eq!(created["goal"]["phase"], "RUNNING");
        assert_eq!(created["goal"]["statusGroup"], "ACTIVE");

        let context =
            active_goals_context(&cp, "Tenant:acme:Workspace:main", "ops-lead", "session-1")
                .await
                .unwrap()
                .unwrap();
        assert!(context.contains("Complete the onboarding checklist"));

        let listed = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-2",
            &spec,
            LIST_GOALS_TOOL,
            &json!({
                "status_group": "active",
                "agent": "ops-lead",
                "session_id": "session-1"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let listed: Value = serde_json::from_str(&listed).unwrap();
        assert_eq!(listed["goals"].as_array().unwrap().len(), 1);
        assert_eq!(listed["goals"][0]["id"], goal_id);

        let listed_from_session = list_goals(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            Some("active"),
            None,
            10,
        )
        .await
        .unwrap();
        assert_eq!(listed_from_session.len(), 1);
        assert_eq!(listed_from_session[0].id, goal_id);

        let updated = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            UPDATE_GOAL_TOOL,
            &json!({
                "goal_id": goal_id,
                "phase": "NEEDS_REVIEW",
                "iteration": 2,
                "progress_summary": "Support task produced the revised checklist; draft is ready for critic review."
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let updated: Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(updated["goal"]["phase"], "NEEDS_REVIEW");
        assert_eq!(updated["goal"]["iteration"], 2);
        assert!(updated["goal"]["progressSummary"]
            .as_str()
            .unwrap()
            .contains("revised checklist"));

        let completed = execute_tool_for_session(
            &cp,
            "Tenant:acme:Workspace:main",
            "ops-lead",
            "session-1",
            &spec,
            COMPLETE_GOAL_TOOL,
            &json!({
                "goal_id": goal_id,
                "progress_summary": "Reviewer approved the final checklist."
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let completed: Value = serde_json::from_str(&completed).unwrap();
        assert_eq!(completed["goal"]["phase"], "SUCCEEDED");
        assert_eq!(completed["goal"]["statusGroup"], "TERMINAL");

        let active_context =
            active_goals_context(&cp, "Tenant:acme:Workspace:main", "ops-lead", "session-1")
                .await
                .unwrap();
        assert!(active_context.is_none());
    }
}
