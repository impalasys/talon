use super::*;

pub async fn execute_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    spec: &manifests::AgentSpec,
    name: &str,
    args: &Value,
    config: &Config,
) -> Result<Option<String>> {
    execute_tool_for_session(
        cp,
        current_namespace,
        current_agent,
        "",
        spec,
        name,
        args,
        config,
    )
    .await
}

pub async fn execute_tool_for_session(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    name: &str,
    args: &Value,
    config: &Config,
) -> Result<Option<String>> {
    Ok(execute_tool_for_session_output(
        cp,
        current_namespace,
        current_agent,
        current_session,
        spec,
        name,
        args,
        config,
    )
    .await?
    .map(|output| output.summary()))
}

pub async fn execute_tool_for_session_output(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    name: &str,
    args: &Value,
    config: &Config,
) -> Result<Option<ToolOutput>> {
    if let Some((capability, action)) = global_capability_for_tool(name) {
        require_global_capability(config, capability, action)?;
    }

    if let Some(result) = artifact_tools::execute_output(
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
        return Ok(Some(ToolOutput::text(result)));
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
        return Ok(Some(ToolOutput::text(result)));
    }
    // Keep the Monty execution future off the Tokio worker stack.  Its
    // subprocess/lifecycle state is substantially larger than the generic
    // native-tool dispatch future, and this branch is also evaluated for
    // unrelated tools.
    if let Some(result) = Box::pin(code_tools::execute(
        config,
        cp,
        current_namespace,
        current_agent,
        current_session,
        spec,
        name,
        args,
    ))
    .await?
    {
        return Ok(Some(result));
    }

    match name {
        READ_SESSION_MESSAGES_TOOL => {
            require_capability(spec, "sessions", "read:messages")?;
            crate::harness::native_tools::sessions::read_session_messages(
                cp,
                current_namespace,
                current_agent,
                args,
            )
            .await
            .map(ToolOutput::text)
            .map(Some)
        }
        LIST_FILES_TOOL => {
            require_file_read(spec)?;
            crate::harness::native_tools::files::list_files_tool(cp, current_namespace, args)
                .await
                .map(ToolOutput::text)
                .map(Some)
        }
        READ_FILE_TOOL => {
            require_file_read(spec)?;
            crate::harness::native_tools::files::read_file_tool(cp, current_namespace, args)
                .await
                .map(Some)
        }
        GET_FILE_METADATA_TOOL => {
            require_file_read(spec)?;
            crate::harness::native_tools::files::get_file_metadata_tool(cp, current_namespace, args)
                .await
                .map(ToolOutput::text)
                .map(Some)
        }
        CREATE_FILE_TOOL => {
            require_capability(spec, "files", "create")?;
            crate::harness::native_tools::files::create_file_tool(cp, current_namespace, args)
                .await
                .map(ToolOutput::text)
                .map(Some)
        }
        UPDATE_FILE_TOOL => {
            require_capability(spec, "files", "update")?;
            crate::harness::native_tools::files::update_file_tool(cp, current_namespace, args)
                .await
                .map(ToolOutput::text)
                .map(Some)
        }
        DELETE_FILE_TOOL => {
            require_capability(spec, "files", "delete")?;
            crate::harness::native_tools::files::delete_file_tool(cp, current_namespace, args)
                .await
                .map(ToolOutput::text)
                .map(Some)
        }
        FETCH_URL_TOOL => {
            require_capability(spec, "research", "fetch_url")?;
            crate::harness::native_tools::research::fetch_url(args)
                .await
                .map(ToolOutput::text)
                .map(Some)
        }
        WEB_SEARCH_TOOL => {
            require_capability(spec, "research", "web_search")?;
            crate::harness::native_tools::research::web_search(args)
                .await
                .map(ToolOutput::text)
                .map(Some)
        }
        ACTIVATE_SKILL_TOOL => {
            let skill_name = req_str(args, "name")?;
            let skills = namespace::load_available_skills(cp, current_namespace).await?;
            let skill = namespace::find_effective_skill(&skills, skill_name)
                .ok_or_else(|| anyhow!("skill '{}' is not available", skill_name))?;
            let instructions = namespace::load_skill_instructions(cp, skill).await?;
            if current_session.is_empty() {
                return Ok(Some(ToolOutput::text(format_active_skill_context(&[(
                    skill.clone(),
                    instructions,
                )]))));
            }
            crate::harness::sessions::activate_skill(
                cp.kv.as_ref(),
                current_namespace,
                current_agent,
                current_session,
                skill_name,
            )
            .await?;
            Ok(Some(ToolOutput::text(format!(
                "Activated skill '{}'. Its workflow guidance is now active for this session.",
                skill_name
            ))))
        }
        DEACTIVATE_SKILL_TOOL => {
            let skill_name = req_str(args, "name")?;
            if current_session.is_empty() {
                return Err(anyhow!("deactivate_skill requires a session"));
            }
            let removed = crate::harness::sessions::deactivate_skill(
                cp.kv.as_ref(),
                current_namespace,
                current_agent,
                current_session,
                skill_name,
            )
            .await?;
            Ok(Some(ToolOutput::text(if removed {
                format!("Deactivated skill '{}'.", skill_name)
            } else {
                format!("Skill '{}' was not active.", skill_name)
            })))
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
            Ok(Some(ToolOutput::text(serde_json::to_string_pretty(
                &json!({
                    "published": true,
                    "messageId": message.id,
                    "channel": message.channel
                }),
            )?)))
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
            Ok(Some(ToolOutput::text(serde_json::to_string_pretty(
                &json!({
                    "published": false,
                    "skipped": true
                }),
            )?)))
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
                    schedules.push(crate::harness::native_tools::schedules::schedule_json(
                        &schedule,
                    ));
                }
                if schedules.len() >= limit {
                    break;
                }
            }
            Ok(Some(ToolOutput::text(serde_json::to_string_pretty(
                &json!({ "schedules": schedules }),
            )?)))
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
            Ok(Some(ToolOutput::text(serde_json::to_string_pretty(
                &json!({
                    "schedule": crate::harness::native_tools::schedules::schedule_json(&schedule)
                }),
            )?)))
        }
        CREATE_SCHEDULE_TOOL => {
            require_capability(spec, "schedules", "create")?;
            let schedule = crate::harness::native_tools::schedules::upsert_schedule(
                cp,
                current_namespace,
                current_agent,
                args,
                None,
            )
            .await?;
            Ok(Some(ToolOutput::text(serde_json::to_string_pretty(
                &json!({
                    "schedule": crate::harness::native_tools::schedules::schedule_json(&schedule),
                    "backendArmed": schedule.status.as_ref().map(|status| status.backend_armed).unwrap_or(false)
                }),
            )?)))
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
            let schedule = crate::harness::native_tools::schedules::upsert_schedule(
                cp,
                current_namespace,
                current_agent,
                args,
                Some(existing),
            )
            .await?;
            Ok(Some(ToolOutput::text(serde_json::to_string_pretty(
                &json!({
                    "schedule": crate::harness::native_tools::schedules::schedule_json(&schedule),
                    "backendArmed": schedule.status.as_ref().map(|status| status.backend_armed).unwrap_or(false)
                }),
            )?)))
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
            Ok(Some(ToolOutput::text(serde_json::to_string_pretty(
                &json!({ "success": true }),
            )?)))
        }
        LIST_GOALS_TOOL => {
            require_capability(spec, "goals", "inspect")?;
            let namespace = opt_str(args, "namespace").unwrap_or(current_namespace);
            let agent = opt_str(args, "agent").unwrap_or(current_agent);
            let session_id = opt_str(args, "session_id").unwrap_or(current_session);
            let status_group = opt_str(args, "status_group");
            let phase = opt_str(args, "phase");
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(100) as usize;
            let goals = crate::harness::native_tools::goals::list_goals(
                cp,
                namespace,
                agent,
                session_id,
                status_group,
                phase,
                limit,
            )
            .await?
            .into_iter()
            .map(|goal| crate::harness::native_tools::goals::goal_json(&goal))
            .collect::<Vec<_>>();
            Ok(Some(ToolOutput::text(serde_json::to_string_pretty(
                &json!({
                    "goals": goals
                }),
            )?)))
        }
        GET_GOAL_TOOL => {
            require_capability(spec, "goals", "inspect")?;
            let goal = crate::harness::native_tools::goals::get_goal_from_args(
                cp,
                current_namespace,
                current_agent,
                current_session,
                args,
            )
            .await?;
            Ok(Some(ToolOutput::text(serde_json::to_string_pretty(
                &json!({
                    "goal": crate::harness::native_tools::goals::goal_json(&goal)
                }),
            )?)))
        }
        CREATE_GOAL_TOOL => {
            require_capability(spec, "goals", "create")?;
            let goal = crate::harness::native_tools::goals::create_goal(
                cp,
                current_namespace,
                current_agent,
                current_session,
                args,
            )
            .await?;
            Ok(Some(ToolOutput::text(serde_json::to_string_pretty(
                &json!({
                    "goal": crate::harness::native_tools::goals::goal_json(&goal)
                }),
            )?)))
        }
        UPDATE_GOAL_TOOL => {
            require_capability(spec, "goals", "update")?;
            let mut goal = crate::harness::native_tools::goals::get_goal_from_args(
                cp,
                current_namespace,
                current_agent,
                current_session,
                args,
            )
            .await?;
            crate::harness::native_tools::goals::update_goal_from_args(&mut goal, args)?;
            crate::harness::native_tools::goals::upsert_goal(cp, goal.clone()).await?;
            Ok(Some(ToolOutput::text(serde_json::to_string_pretty(
                &json!({
                    "goal": crate::harness::native_tools::goals::goal_json(&goal)
                }),
            )?)))
        }
        COMPLETE_GOAL_TOOL => {
            require_capability(spec, "goals", "update")?;
            let mut goal = crate::harness::native_tools::goals::get_goal_from_args(
                cp,
                current_namespace,
                current_agent,
                current_session,
                args,
            )
            .await?;
            let now = chrono::Utc::now().timestamp_micros();
            goal.phase = crate::gateway::rpc::data_proto::GoalPhase::Succeeded as i32;
            goal.updated_at = now;
            goal.completed_at = now;
            if let Some(summary) = opt_str(args, "progress_summary") {
                goal.progress_summary = summary.to_string();
            }
            crate::harness::native_tools::goals::upsert_goal(cp, goal.clone()).await?;
            Ok(Some(ToolOutput::text(serde_json::to_string_pretty(
                &json!({
                    "goal": crate::harness::native_tools::goals::goal_json(&goal)
                }),
            )?)))
        }
        BLOCK_GOAL_TOOL => {
            require_capability(spec, "goals", "update")?;
            let mut goal = crate::harness::native_tools::goals::get_goal_from_args(
                cp,
                current_namespace,
                current_agent,
                current_session,
                args,
            )
            .await?;
            let now = chrono::Utc::now().timestamp_micros();
            goal.phase = crate::gateway::rpc::data_proto::GoalPhase::Blocked as i32;
            goal.updated_at = now;
            goal.blocked_reason = req_str(args, "blocked_reason")?.to_string();
            if let Some(summary) = opt_str(args, "progress_summary") {
                goal.progress_summary = summary.to_string();
            }
            crate::harness::native_tools::goals::upsert_goal(cp, goal.clone()).await?;
            Ok(Some(ToolOutput::text(serde_json::to_string_pretty(
                &json!({
                    "goal": crate::harness::native_tools::goals::goal_json(&goal)
                }),
            )?)))
        }
        _ => Ok(None),
    }
}

fn require_global_capability(config: &Config, capability: &str, action: &str) -> Result<()> {
    if global_capability_allowed(config, capability, action) {
        Ok(())
    } else {
        Err(anyhow!(
            "capability '{}:{}' is disabled by deployment configuration",
            capability,
            action
        ))
    }
}

fn global_capability_for_tool(name: &str) -> Option<(&'static str, &'static str)> {
    match name {
        READ_SESSION_MESSAGES_TOOL => Some(("sessions", "read:messages")),
        LIST_FILES_TOOL | READ_FILE_TOOL | GET_FILE_METADATA_TOOL => Some(("files", "read")),
        CREATE_FILE_TOOL => Some(("files", "create")),
        UPDATE_FILE_TOOL => Some(("files", "update")),
        DELETE_FILE_TOOL => Some(("files", "delete")),
        FETCH_URL_TOOL => Some(("research", "fetch_url")),
        WEB_SEARCH_TOOL => Some(("research", "web_search")),
        LIST_SCHEDULES_TOOL | GET_SCHEDULE_TOOL => Some(("schedules", "inspect")),
        CREATE_SCHEDULE_TOOL => Some(("schedules", "create")),
        UPDATE_SCHEDULE_TOOL => Some(("schedules", "update")),
        DELETE_SCHEDULE_TOOL => Some(("schedules", "delete")),
        GET_GOAL_TOOL | LIST_GOALS_TOOL => Some(("goals", "inspect")),
        CREATE_GOAL_TOOL => Some(("goals", "create")),
        UPDATE_GOAL_TOOL | COMPLETE_GOAL_TOOL | BLOCK_GOAL_TOOL => Some(("goals", "update")),
        _ => None,
    }
}
