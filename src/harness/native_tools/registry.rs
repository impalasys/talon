// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only
use serde_json::{json, Value};
use crate::control::config::Config;
use crate::gateway::rpc::manifests;
use crate::harness::skills::namespace::{self, NamespaceSkill};
use crate::harness::skills::registry::ToolRegistry;
use super::common::has_capability_action;
use super::{
    ACTIVATE_SKILL_TOOL, BLOCK_GOAL_TOOL, CHANNEL_PUBLISH_TOOL, CHANNEL_SKIP_REPLY_TOOL,
    COMPLETE_GOAL_TOOL, CREATE_GOAL_TOOL, CREATE_SCHEDULE_TOOL, CREATE_TASK_TOOL, DEACTIVATE_SKILL_TOOL,
    DELETE_FILE_TOOL, DELETE_SCHEDULE_TOOL, GET_FILE_METADATA_TOOL, GET_GOAL_TOOL, GET_SCHEDULE_TOOL,
    LIST_FILES_TOOL, LIST_GOALS_TOOL, LIST_SCHEDULES_TOOL, READ_FILE_TOOL, UPDATE_FILE_TOOL,
    UPDATE_GOAL_TOOL, UPDATE_SCHEDULE_TOOL, CREATE_FILE_TOOL, READ_SESSION_MESSAGES_TOOL,
    FETCH_URL_TOOL, WEB_SEARCH_TOOL,
};

pub fn register_skill_tools(registry: &mut ToolRegistry, skills: &[NamespaceSkill]) {
    let names = namespace::effective_skill_names(skills);
    if !names.is_empty() {
        registry.register_builtin(ACTIVATE_SKILL_TOOL, "Load the full instructions for an available namespace skill before applying it.", json!({"type":"object","properties":{"name":{"type":"string","description":"Available skill name to activate.","enum":names}},"required":["name"]}));
    }
    registry.register_builtin(DEACTIVATE_SKILL_TOOL, "Stop applying an active namespace skill for the rest of this session.", json!({"type":"object","properties":{"name":{"type":"string","description":"Active skill name to deactivate."}},"required":["name"]}));
}
pub fn register_channel_tools(registry: &mut ToolRegistry) {
    registry.register_builtin(CHANNEL_PUBLISH_TOOL, "Publish a public response to the channel that triggered this session. Normal assistant text remains private; use this tool for channel-visible replies.", json!({"type":"object","properties":{"content":{"type":"string","description":"Public channel response content."}},"required":["content"]}));
    registry.register_builtin(CHANNEL_SKIP_REPLY_TOOL, "Mark this channel-triggered session as not needing a public channel reply.", json!({"type":"object","properties":{"reason":{"type":"string","description":"Optional private reason for skipping a channel reply."}}}));
}
pub fn register_tools(registry: &mut ToolRegistry, spec: &manifests::AgentSpec, config: &Config) {
    super::a2a_tools::register(registry, spec);
    crate::harness::native_tools::artifact_tools::register(registry);
    register_research_tools(registry, spec);
    if !has_capability_action(spec, "schedules", "inspect") && !has_capability_action(spec, "schedules", "create") && !has_capability_action(spec, "schedules", "update") && !has_capability_action(spec, "schedules", "delete") && !has_capability_action(spec, "tasks", "inspect") && !has_capability_action(spec, "tasks", "create") && !has_capability_action(spec, "tasks", "update") && !has_capability_action(spec, "sessions", "read:messages") && !has_capability_action(spec, "files", "inspect") && !has_capability_action(spec, "files", "read") && !has_capability_action(spec, "files", "create") && !has_capability_action(spec, "files", "update") && !has_capability_action(spec, "files", "delete") && !has_capability_action(spec, "code", "run") && !has_capability_action(spec, "goals", "inspect") && !has_capability_action(spec, "goals", "create") && !has_capability_action(spec, "goals", "update") { return; }
    if has_capability_action(spec, "schedules", "inspect") {
        registry.register_builtin(LIST_SCHEDULES_TOOL, "List schedules in a namespace.", json!({"type":"object","properties":{"namespace":{"type":"string"},"agent":{"type":"string"},"enabled":{"type":"boolean"},"limit":{"type":"integer"}}}));
        registry.register_builtin(GET_SCHEDULE_TOOL, "Get a single schedule.", json!({"type":"object","properties":{"namespace":{"type":"string"},"name":{"type":"string"}},"required":["name"]}));
    }
    if has_capability_action(spec, "schedules", "create") { registry.register_builtin(CREATE_SCHEDULE_TOOL, "Create a schedule.", put_schedule_schema()); }
    if has_capability_action(spec, "schedules", "update") { registry.register_builtin(UPDATE_SCHEDULE_TOOL, "Update a schedule.", put_schedule_schema()); }
    if has_capability_action(spec, "schedules", "delete") { registry.register_builtin(DELETE_SCHEDULE_TOOL, "Delete a schedule.", json!({"type":"object","properties":{"namespace":{"type":"string"},"name":{"type":"string"}},"required":["name"]})); }
    if has_capability_action(spec, "sessions", "read:messages") { registry.register_builtin(READ_SESSION_MESSAGES_TOOL, "Read text messages from a Talon session.", json!({"type":"object","properties":{"namespace":{"type":"string"},"agent":{"type":"string"},"session_id":{"type":"string"},"limit":{"type":"integer"}},"required":["session_id"]})); }
    if has_capability_action(spec, "files", "inspect") || has_capability_action(spec, "files", "read") {
        registry.register_builtin(LIST_FILES_TOOL, "List namespace File resources.", json!({"type":"object","properties":{"namespace":{"type":"string"},"prefix":{"type":"string"},"purpose":{"type":"string"},"index_policy":{"type":"string"},"limit":{"type":"integer"}}}));
        registry.register_builtin(READ_FILE_TOOL, "Read a namespace File.", json!({"type":"object","properties":{"uri":{"type":"string"},"namespace":{"type":"string"},"path":{"type":"string"}}}));
        registry.register_builtin(GET_FILE_METADATA_TOOL, "Get File metadata.", json!({"type":"object","properties":{"uri":{"type":"string"},"namespace":{"type":"string"},"path":{"type":"string"}}}));
    }
    if has_capability_action(spec, "files", "create") { registry.register_builtin(CREATE_FILE_TOOL, "Create a namespace File.", file_write_schema()); }
    if has_capability_action(spec, "files", "update") { registry.register_builtin(UPDATE_FILE_TOOL, "Update a namespace File.", file_write_schema()); }
    if has_capability_action(spec, "files", "delete") { registry.register_builtin(DELETE_FILE_TOOL, "Delete a namespace File.", json!({"type":"object","properties":{"uri":{"type":"string"},"namespace":{"type":"string"},"path":{"type":"string"}}})); }
    super::code_tools::register(registry, spec, config);
    super::task_tools::register(registry, spec);
    if has_capability_action(spec, "goals", "inspect") {
        registry.register_builtin(LIST_GOALS_TOOL, "List Talon Goals.", json!({"type":"object","properties":{"namespace":{"type":"string"},"agent":{"type":"string"},"session_id":{"type":"string"},"status_group":{"type":"string"},"phase":{"type":"string"},"limit":{"type":"integer"}}}));
        registry.register_builtin(GET_GOAL_TOOL, "Get one Talon Goal.", json!({"type":"object","properties":{"namespace":{"type":"string"},"agent":{"type":"string"},"session_id":{"type":"string"},"goal_id":{"type":"string"}},"required":["goal_id"]}));
    }
    if has_capability_action(spec, "goals", "create") { registry.register_builtin(CREATE_GOAL_TOOL, "Create a Goal.", json!({"type":"object","properties":{"objective":{"type":"string"},"success_criteria":{"type":"array","items":{"type":"string"}},"max_iterations":{"type":"integer"},"progress_summary":{"type":"string"},"labels":{"type":"object","additionalProperties":{"type":"string"}},"metadata":{"type":"object","additionalProperties":{"type":"string"}}},"required":["objective"]})); }
    if has_capability_action(spec, "goals", "update") {
        registry.register_builtin(UPDATE_GOAL_TOOL, "Update Goal.", json!({"type":"object","properties":{"namespace":{"type":"string"},"agent":{"type":"string"},"session_id":{"type":"string"},"goal_id":{"type":"string"},"phase":{"type":"string"},"progress_summary":{"type":"string"},"iteration":{"type":"integer"},"blocked_reason":{"type":"string"}},"required":["goal_id"]}));
        registry.register_builtin(COMPLETE_GOAL_TOOL, "Mark Goal SUCCEEDED.", json!({"type":"object","properties":{"namespace":{"type":"string"},"agent":{"type":"string"},"session_id":{"type":"string"},"goal_id":{"type":"string"},"progress_summary":{"type":"string"}},"required":["goal_id"]}));
        registry.register_builtin(BLOCK_GOAL_TOOL, "Mark Goal BLOCKED.", json!({"type":"object","properties":{"namespace":{"type":"string"},"agent":{"type":"string"},"session_id":{"type":"string"},"goal_id":{"type":"string"},"blocked_reason":{"type":"string"},"progress_summary":{"type":"string"}},"required":["goal_id","blocked_reason"]}));
    }
}
fn register_research_tools(registry: &mut ToolRegistry, spec: &manifests::AgentSpec) {
    if has_capability_action(spec, "research", "fetch_url") { registry.register_builtin(FETCH_URL_TOOL, "Fetch a URL.", json!({"type":"object","properties":{"url":{"type":"string"},"max_chars":{"type":"integer"}},"required":["url"]})); }
    if has_capability_action(spec, "research", "web_search") { registry.register_builtin(WEB_SEARCH_TOOL, "Search web.", json!({"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer"}},"required":["query"]})); }
}
fn put_schedule_schema() -> Value { json!({"type":"object","properties":{"namespace":{"type":"string"},"name":{"type":"string"},"labels":{"type":"object","additionalProperties":{"type":"string"}},"kind":{"type":"string"},"cron":{"type":"string"},"interval_seconds":{"type":"integer"},"run_at":{"type":"string"},"timezone":{"type":"string"},"agent":{"type":"string"},"session_mode":{"type":"string"},"session_id":{"type":"string"},"input_message":{"type":"string"},"enabled":{"type":"boolean"}},"required":["name","kind","input_message"]}) }
fn file_write_schema() -> Value { json!({"type":"object","properties":{"uri":{"type":"string"},"namespace":{"type":"string"},"path":{"type":"string"},"content":{"type":"string"},"media_type":{"type":"string"},"purpose":{"type":"string"},"index_policy":{"type":"string"},"retention":{"type":"string"}},"required":["content"]}) }
