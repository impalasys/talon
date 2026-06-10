// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use prost::Message;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::control::{keys, ControlPlane, ProtoKeyValueStoreExt};
use crate::gateway::rpc::{manifests, models, protobuf_value::value::Kind as ProtoValueKind};
use crate::scheduling;
use crate::skills::registry::ToolRegistry;

pub const CREATE_SCHEDULE_TOOL: &str = "create_schedule";
pub const GET_SCHEDULE_TOOL: &str = "get_schedule";
pub const LIST_SCHEDULES_TOOL: &str = "list_schedules";
pub const UPDATE_SCHEDULE_TOOL: &str = "update_schedule";
pub const DELETE_SCHEDULE_TOOL: &str = "delete_schedule";
pub const CHANNEL_PUBLISH_TOOL: &str = "channel_publish";
pub const CHANNEL_SKIP_REPLY_TOOL: &str = "channel_skip_reply";

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
    if !has_capability_action(spec, "schedules", "inspect")
        && !has_capability_action(spec, "schedules", "create")
        && !has_capability_action(spec, "schedules", "update")
        && !has_capability_action(spec, "schedules", "delete")
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
    match name {
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
            let mut entries = cp
                .kv
                .list_entries(&keys::schedule_prefix(namespace))
                .await?;
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut schedules = Vec::new();
            for (_key, value) in entries {
                let schedule = models::Schedule::decode(value.as_slice())?;
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
                .get_msg::<models::Schedule>(&keys::schedule(namespace, schedule_name))
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
                .get_msg::<models::Schedule>(&keys::schedule(namespace, schedule_name))
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
            if let Some(schedule) = cp.kv.get_msg::<models::Schedule>(&key).await? {
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
        _ => Ok(None),
    }
}

async fn upsert_schedule(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    args: &Value,
    existing: Option<models::Schedule>,
) -> Result<models::Schedule> {
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
        .or_else(|| existing.as_ref().map(|schedule| schedule.labels.clone()))
        .unwrap_or_default();

    let mut schedule = models::Schedule {
        name: name.clone(),
        ns: namespace.clone(),
        labels,
        spec: Some(models::ScheduleSpec {
            kind,
            cron,
            interval_seconds,
            run_at,
            timezone,
            target: Some(models::ScheduleTarget {
                agent,
                session_mode,
                session_id,
            }),
            input_message,
            enabled,
        }),
        status: existing.and_then(|schedule| schedule.status),
    };

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

fn schedule_json(schedule: &models::Schedule) -> Value {
    let spec = schedule.spec.as_ref();
    let status = schedule.status.as_ref();
    let target = spec.and_then(|spec| spec.target.as_ref());
    json!({
        "name": schedule.name,
        "ns": schedule.ns,
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
        "labels": schedule.labels,
    })
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
        }
    }

    fn control_plane(kv: Arc<MockKvStore>, scheduler: Arc<MockScheduler>) -> ControlPlane {
        ControlPlane {
            kv,
            pubsub: Arc::new(EmptyPubSub),
            scheduler,
            objects: crate::control::object_store::default_object_store(),
        }
    }

    async fn seed_agent(kv: &MockKvStore, ns: &str, name: &str) {
        kv.set_msg(
            &keys::agent(ns, name),
            &models::Agent {
                name: name.to_string(),
                ns: ns.to_string(),
                definition: Some(manifests::AgentDefinition::default()),
                effective_spec: Some(manifests::AgentSpec::default()),
                template_deps: Vec::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
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
            .get_msg::<models::Schedule>(&keys::schedule("conic:test", "nightly"))
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
}
