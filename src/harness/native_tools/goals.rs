// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;

pub(crate) async fn create_goal(
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

pub(crate) async fn get_goal_from_args(
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

pub(crate) async fn load_goal(
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

pub(crate) async fn upsert_goal(cp: &ControlPlane, goal: data_proto::Goal) -> Result<()> {
    cp.kv
        .set_msg(
            &keys::goal(&goal.namespace, &goal.agent, &goal.session_id, &goal.id),
            &goal,
        )
        .await
}

pub(crate) async fn list_goals(
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

pub(crate) async fn list_session_goals(
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

pub(crate) fn goal_matches(
    goal: &data_proto::Goal,
    status_group: Option<&str>,
    phase: Option<&str>,
) -> bool {
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

pub(crate) fn update_goal_from_args(goal: &mut data_proto::Goal, args: &Value) -> Result<()> {
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

pub(crate) fn goal_json(goal: &data_proto::Goal) -> Value {
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

pub(crate) fn parse_goal_phase(value: &str) -> Result<i32> {
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

pub(crate) fn goal_phase_name(value: i32) -> &'static str {
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

pub(crate) fn goal_status_group(value: i32) -> &'static str {
    if is_active_goal_phase(value) {
        "ACTIVE"
    } else if is_terminal_goal_phase(value) {
        "TERMINAL"
    } else {
        "UNKNOWN"
    }
}

pub(crate) fn is_active_goal_phase(value: i32) -> bool {
    matches!(
        data_proto::GoalPhase::try_from(value).ok(),
        Some(data_proto::GoalPhase::Running)
            | Some(data_proto::GoalPhase::Paused)
            | Some(data_proto::GoalPhase::NeedsReview)
            | Some(data_proto::GoalPhase::Blocked)
    )
}

pub(crate) fn is_terminal_goal_phase(value: i32) -> bool {
    matches!(
        data_proto::GoalPhase::try_from(value).ok(),
        Some(data_proto::GoalPhase::Succeeded)
            | Some(data_proto::GoalPhase::Failed)
            | Some(data_proto::GoalPhase::Canceled)
            | Some(data_proto::GoalPhase::Expired)
    )
}
