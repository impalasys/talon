// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only
use super::common::has_capability_action;
use super::{
    ACTIVATE_SKILL_TOOL, BLOCK_GOAL_TOOL, CHANNEL_PUBLISH_TOOL, CHANNEL_SKIP_REPLY_TOOL,
    COMPLETE_GOAL_TOOL, CREATE_FILE_TOOL, CREATE_GOAL_TOOL, CREATE_SCHEDULE_TOOL,
    DEACTIVATE_SKILL_TOOL, DELETE_FILE_TOOL, DELETE_SCHEDULE_TOOL, FETCH_URL_TOOL,
    GET_FILE_METADATA_TOOL, GET_GOAL_TOOL, GET_SCHEDULE_TOOL, LIST_FILES_TOOL, LIST_GOALS_TOOL,
    LIST_SCHEDULES_TOOL, READ_FILE_TOOL, READ_SESSION_MESSAGES_TOOL, READ_TOOL, UPDATE_FILE_TOOL,
    UPDATE_GOAL_TOOL, UPDATE_SCHEDULE_TOOL, WEB_SEARCH_TOOL, WRITE_TOOL,
};
use crate::control::config::Config;
use crate::gateway::rpc::manifests;
use crate::harness::skills::namespace::{self, NamespaceSkill};
use crate::harness::skills::registry::ToolRegistry;
use serde_json::{json, Value};

pub fn register_skill_tools(registry: &mut ToolRegistry, skills: &[NamespaceSkill]) {
    let names = namespace::effective_skill_names(skills);
    if !names.is_empty() {
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
    registry.register_builtin(
        DEACTIVATE_SKILL_TOOL,
        "Stop applying an active namespace skill for the rest of this session.",
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Active skill name to deactivate." }
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

pub fn register_tools(registry: &mut ToolRegistry, spec: &manifests::AgentSpec, config: &Config) {
    super::artifact_tools::register(registry);
    registry.register_builtin(
        READ_TOOL,
        "Read a File or session Artifact. Use a file:// or artifact:// URI, or namespace/path for a File.",
        super::resource_read_schema(),
    );
    registry.register_builtin(
        WRITE_TOOL,
        "Create or update a namespace File or session Artifact. Use ref to update; omit ref and choose kind=file or kind=artifact to create.",
        super::resource_write_schema(),
    );
    super::a2a_tools::register(registry, spec);
    register_research_tools(registry, spec);

    if !has_capability_action(spec, "schedules", "inspect")
        && !has_capability_action(spec, "schedules", "create")
        && !has_capability_action(spec, "schedules", "update")
        && !has_capability_action(spec, "schedules", "delete")
        && !has_capability_action(spec, "tasks", "inspect")
        && !has_capability_action(spec, "tasks", "create")
        && !has_capability_action(spec, "tasks", "update")
        && !has_capability_action(spec, "sessions", "read:messages")
        && !has_capability_action(spec, "files", "inspect")
        && !has_capability_action(spec, "files", "read")
        && !has_capability_action(spec, "files", "create")
        && !has_capability_action(spec, "files", "update")
        && !has_capability_action(spec, "files", "delete")
        && !has_capability_action(spec, "code", "run")
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

    if has_capability_action(spec, "files", "inspect")
        || has_capability_action(spec, "files", "read")
    {
        registry.register_builtin(
            LIST_FILES_TOOL,
            "List namespace File resources by optional path prefix, purpose, and index policy.",
            json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "File namespace. Defaults to current namespace." },
                    "prefix": { "type": "string", "description": "Optional logical path prefix such as /content/pages." },
                    "purpose": { "type": "string", "description": "Optional purpose: ARTIFACT, MEMORY, or SKILL." },
                    "index_policy": { "type": "string", "description": "Optional index policy: NONE, SEARCH, or RETRIEVAL." },
                    "limit": { "type": "integer", "description": "Maximum results. Defaults to 50." }
                }
            }),
        );
        registry.register_builtin(
            READ_FILE_TOOL,
            "Read a namespace File by file:// URI or logical path.",
            json!({
                "type": "object",
                "properties": {
                    "uri": { "type": "string", "description": "Optional file://<namespace>/<path> URI returned by Talon or Conic." },
                    "namespace": { "type": "string", "description": "File namespace. Defaults to current namespace." },
                    "path": { "type": "string", "description": "Logical File path, for example /content/pages/<id>/content.md." }
                }
            }),
        );
        registry.register_builtin(
            GET_FILE_METADATA_TOOL,
            "Get File metadata by file:// URI or logical path without reading content.",
            json!({
                "type": "object",
                "properties": {
                    "uri": { "type": "string", "description": "Optional file://<namespace>/<path> URI returned by Talon or Conic." },
                    "namespace": { "type": "string", "description": "File namespace. Defaults to current namespace." },
                    "path": { "type": "string", "description": "Logical File path." }
                }
            }),
        );
    }

    if has_capability_action(spec, "files", "create") {
        registry.register_builtin(
            CREATE_FILE_TOOL,
            "Create a namespace File. Defaults to purpose=ARTIFACT, index_policy=SEARCH, and retention=RETAINED.",
            file_write_schema(),
        );
    }
    if has_capability_action(spec, "files", "update") {
        registry.register_builtin(
            UPDATE_FILE_TOOL,
            "Update an existing namespace File by file:// URI or logical path.",
            file_write_schema(),
        );
    }
    if has_capability_action(spec, "files", "delete") {
        registry.register_builtin(
            DELETE_FILE_TOOL,
            "Delete a namespace File resource by file:// URI or logical path.",
            json!({
                "type": "object",
                "properties": {
                    "uri": { "type": "string", "description": "Optional file://<namespace>/<path> URI returned by Talon or Conic." },
                    "namespace": { "type": "string", "description": "File namespace. Defaults to current namespace." },
                    "path": { "type": "string", "description": "Logical File path." }
                }
            }),
        );
    }

    super::code_tools::register(registry, spec, config);

    super::task_tools::register(registry, spec);

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

fn file_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "uri": { "type": "string", "description": "Optional file://<namespace>/<path> URI for updates." },
            "namespace": { "type": "string", "description": "File namespace. Defaults to current namespace." },
            "path": { "type": "string", "description": "Logical File path, for example /content/pages/<id>/content.md." },
            "content": { "type": "string", "description": "Markdown, HTML, or text content to store." },
            "media_type": { "type": "string", "description": "Media type. Defaults to text/markdown." },
            "purpose": { "type": "string", "description": "File purpose: ARTIFACT, MEMORY, or SKILL. Defaults to ARTIFACT." },
            "index_policy": { "type": "string", "description": "Index policy: NONE, SEARCH, or RETRIEVAL. Defaults to SEARCH." },
            "retention": { "type": "string", "description": "Retention policy: RETAINED. Defaults to RETAINED." }
        },
        "required": ["content"]
    })
}
