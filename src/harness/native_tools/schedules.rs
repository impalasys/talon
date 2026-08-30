use super::*;

pub(crate) async fn upsert_schedule(
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

pub(crate) fn schedule_json(schedule: &resources_proto::Schedule) -> Value {
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
