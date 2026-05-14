use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, LocalResult, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use cron::Schedule as CronSchedule;
use prost::Message;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

use crate::control::events;
use crate::control::{
    keys,
    scheduler::{ScheduleWakeupRequest, SchedulerBackend},
    ControlPlane, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
};
use crate::gateway::rpc::models;

pub const MIN_RECURRING_INTERVAL_SECONDS: u32 = 300;
const DEFAULT_SESSION_PROCESSING_TIMEOUT_SECONDS: i64 = 30 * 60;
const DEFAULT_SCHEDULE_CLAIM_TIMEOUT_SECONDS: i64 = 60;
const MAX_CAS_RETRIES: usize = 8;
const MAX_RECENT_SCHEDULE_EVENTS: usize = 20;

#[derive(Debug)]
pub struct SessionCurrentlyProcessingError;

impl std::fmt::Display for SessionCurrentlyProcessingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session is currently processing")
    }
}

impl std::error::Error for SessionCurrentlyProcessingError {}

#[derive(Debug)]
pub struct SessionNotFoundError;

impl std::fmt::Display for SessionNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session not found")
    }
}

impl std::error::Error for SessionNotFoundError {}

#[derive(Debug)]
pub struct EmptyMessageError;

impl std::fmt::Display for EmptyMessageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "message content is required")
    }
}

impl std::error::Error for EmptyMessageError {}

#[derive(Debug)]
pub struct ScheduleWakeupInProgressError;

impl std::fmt::Display for ScheduleWakeupInProgressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "schedule wakeup is already being processed")
    }
}

impl std::error::Error for ScheduleWakeupInProgressError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleWakeupPayload {
    pub namespace: String,
    pub schedule_id: String,
    pub revision: u64,
    pub intended_run_at: i64,
}

pub fn validate_schedule(schedule: &models::Schedule) -> Result<()> {
    if schedule.name.is_empty() {
        return Err(anyhow!("schedule name is required"));
    }
    if schedule.name.contains('/') {
        return Err(anyhow!("schedule name cannot contain '/'"));
    }
    if schedule.ns.is_empty() {
        return Err(anyhow!("schedule namespace is required"));
    }

    let spec = schedule
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("schedule spec is required"))?;
    let target = spec
        .target
        .as_ref()
        .ok_or_else(|| anyhow!("schedule target is required"))?;

    if target.agent.is_empty() {
        return Err(anyhow!("schedule target agent is required"));
    }
    let session_mode = normalize_session_mode(&target.session_mode)?;
    if session_mode != "new" && session_mode != "reuse" {
        return Err(anyhow!(
            "schedule target session_mode must be 'new' or 'reuse'"
        ));
    }
    if session_mode == "reuse" && target.session_id.is_empty() {
        return Err(anyhow!(
            "schedule target session_id is required for reuse sessions"
        ));
    }
    if spec.input_message.trim().is_empty() {
        return Err(anyhow!("schedule input message is required"));
    }

    match spec.kind.as_str() {
        "at" => {
            parse_run_at(&spec.run_at)?;
        }
        "every" => {
            if spec.interval_seconds < MIN_RECURRING_INTERVAL_SECONDS {
                return Err(anyhow!(
                    "schedule interval_seconds must be at least {}",
                    MIN_RECURRING_INTERVAL_SECONDS
                ));
            }
        }
        "cron" => {
            if spec.cron.trim().is_empty() {
                return Err(anyhow!("schedule cron expression is required"));
            }
            let schedule = CronSchedule::from_str(&normalize_cron_expression(&spec.cron))
                .map_err(|e| anyhow!("invalid cron expression: {}", e))?;
            if !spec.timezone.is_empty() {
                let _ = parse_timezone(&spec.timezone)?;
            }
            validate_cron_min_interval(spec, &schedule)?;
        }
        other => {
            return Err(anyhow!(
                "schedule kind must be one of at, every, cron; got {}",
                other
            ));
        }
    }

    Ok(())
}

pub fn initialize_schedule(
    schedule: &mut models::Schedule,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>> {
    validate_schedule(schedule)?;
    let next = compute_next_run(schedule, now, None)?;
    let status = schedule
        .status
        .get_or_insert_with(models::ScheduleStatus::default);
    status.revision = status.revision.saturating_add(1).max(1);
    status.claimed_run_at = None;
    status.claim_expires_at = None;
    status.next_run_at = next.map(|dt| dt.timestamp_micros());
    append_schedule_event(
        schedule,
        now,
        "initialize",
        if next.is_some() { "ready" } else { "disabled" },
        next.map(|dt| format!("next run at {}", dt.to_rfc3339()))
            .unwrap_or_else(|| "schedule has no upcoming run".to_string()),
    );
    Ok(next)
}

pub fn compute_successor_run(
    schedule: &models::Schedule,
    fired_at: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>> {
    compute_next_run(schedule, fired_at, Some(fired_at))
}

pub fn compute_aligned_every_successor(
    schedule: &models::Schedule,
    fired_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>> {
    let spec = schedule
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("schedule spec is required"))?;
    if !spec.enabled {
        return Ok(None);
    }
    if spec.kind != "every" {
        return compute_successor_run(schedule, std::cmp::max(fired_at, now));
    }

    let interval_micros = i64::from(spec.interval_seconds) * 1_000_000;
    let elapsed_micros = (now.timestamp_micros() - fired_at.timestamp_micros()).max(0);
    let intervals_to_advance = (elapsed_micros / interval_micros) + 1;
    Ok(Some(
        fired_at + Duration::microseconds(interval_micros * intervals_to_advance),
    ))
}

fn compute_next_run(
    schedule: &models::Schedule,
    now: DateTime<Utc>,
    previous_fire: Option<DateTime<Utc>>,
) -> Result<Option<DateTime<Utc>>> {
    let spec = schedule
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("schedule spec is required"))?;
    if !spec.enabled {
        return Ok(None);
    }
    match spec.kind.as_str() {
        "at" => {
            if previous_fire.is_some() {
                return Ok(None);
            }
            let run_at = parse_run_at(&spec.run_at)?;
            if run_at < now {
                Err(anyhow!("schedule run_at must be in the future"))
            } else {
                Ok(Some(run_at))
            }
        }
        "every" => {
            let base = previous_fire.unwrap_or(now);
            Ok(Some(base + Duration::seconds(spec.interval_seconds as i64)))
        }
        "cron" => compute_next_cron_run(spec, now),
        other => Err(anyhow!("unsupported schedule kind {}", other)),
    }
}

fn compute_next_cron_run(
    spec: &models::ScheduleSpec,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>> {
    let schedule = CronSchedule::from_str(&normalize_cron_expression(&spec.cron))
        .map_err(|e| anyhow!("invalid cron expression: {}", e))?;
    if spec.timezone.is_empty() {
        return Ok(schedule.after(&now).next());
    }
    let tz = parse_timezone(&spec.timezone)?;
    let local_now = now.with_timezone(&tz);
    Ok(schedule
        .after(&local_now)
        .next()
        .map(|dt| dt.with_timezone(&Utc)))
}

fn validate_cron_min_interval(spec: &models::ScheduleSpec, schedule: &CronSchedule) -> Result<()> {
    let minimum_interval = Duration::seconds(MIN_RECURRING_INTERVAL_SECONDS as i64);
    if spec.timezone.is_empty() {
        let now = Utc::now();
        let mut upcoming = schedule.after(&now);
        if let (Some(first), Some(second)) = (upcoming.next(), upcoming.next()) {
            if second - first < minimum_interval {
                return Err(anyhow!(
                    "schedule cron interval must be at least {} seconds",
                    MIN_RECURRING_INTERVAL_SECONDS
                ));
            }
        }
        return Ok(());
    }

    let tz = parse_timezone(&spec.timezone)?;
    let now = Utc::now().with_timezone(&tz);
    let mut upcoming = schedule.after(&now);
    if let (Some(first), Some(second)) = (upcoming.next(), upcoming.next()) {
        if second.with_timezone(&Utc) - first.with_timezone(&Utc) < minimum_interval {
            return Err(anyhow!(
                "schedule cron interval must be at least {} seconds",
                MIN_RECURRING_INTERVAL_SECONDS
            ));
        }
    }
    Ok(())
}

fn parse_timezone(tz: &str) -> Result<Tz> {
    tz.parse::<Tz>()
        .map_err(|_| anyhow!("invalid IANA timezone {}", tz))
}

fn normalize_cron_expression(expr: &str) -> String {
    match expr.split_whitespace().count() {
        5 => format!("0 {} *", expr),
        6 => format!("{} *", expr),
        _ => expr.to_string(),
    }
}

fn parse_run_at(value: &str) -> Result<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Ok(dt.with_timezone(&Utc));
    }
    let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .context("run_at must be RFC3339 or YYYY-MM-DDTHH:MM:SS")?;
    match Utc.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Ok(dt),
        _ => Err(anyhow!("ambiguous or invalid run_at timestamp")),
    }
}

pub async fn arm_schedule(
    scheduler: &dyn SchedulerBackend,
    schedule: &mut models::Schedule,
    next_run_at: Option<DateTime<Utc>>,
) -> Result<()> {
    let status = schedule
        .status
        .get_or_insert_with(models::ScheduleStatus::default);

    if let Some(handle) = status.backend_handle.clone() {
        if let Err(err) = scheduler.cancel(&handle).await {
            tracing::warn!(handle = %handle, error = %err, "Failed to cancel previous schedule wakeup");
        }
    }
    status.backend_handle = None;
    status.backend_armed = false;
    status.next_run_at = next_run_at.map(|dt| dt.timestamp_micros());

    let Some(fire_at) = next_run_at else {
        append_schedule_event(
            schedule,
            Utc::now(),
            "arm",
            "disarmed",
            "schedule has no upcoming run".to_string(),
        );
        return Ok(());
    };

    let payload = ScheduleWakeupPayload {
        namespace: schedule.ns.clone(),
        schedule_id: schedule.name.clone(),
        revision: status.revision,
        intended_run_at: fire_at.timestamp_micros(),
    };
    let wakeup = scheduler
        .schedule(ScheduleWakeupRequest {
            namespace: schedule.ns.clone(),
            schedule_id: schedule.name.clone(),
            revision: status.revision,
            fire_at,
            payload: serde_json::to_vec(&payload)?,
        })
        .await?;
    let backend_armed = wakeup.armed;
    status.backend_handle = wakeup.handle;
    status.backend_armed = backend_armed;
    let detail = format!("next run at {}", fire_at.to_rfc3339());
    let outcome = if backend_armed { "armed" } else { "pending" }.to_string();
    let _ = status;
    append_schedule_event(
        schedule,
        Utc::now(),
        "arm",
        outcome,
        detail,
    );
    Ok(())
}

pub fn session_processing_timeout_micros() -> i64 {
    duration_from_env(
        "TALON_SESSION_PROCESSING_TIMEOUT_SECONDS",
        DEFAULT_SESSION_PROCESSING_TIMEOUT_SECONDS,
    ) * 1_000_000
}

pub fn schedule_claim_timeout_micros() -> i64 {
    duration_from_env(
        "TALON_SCHEDULE_CLAIM_TIMEOUT_SECONDS",
        DEFAULT_SCHEDULE_CLAIM_TIMEOUT_SECONDS,
    ) * 1_000_000
}

fn duration_from_env(name: &str, default_seconds: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_seconds)
}

pub async fn persist_schedule(kv: &dyn KeyValueStore, schedule: &models::Schedule) -> Result<()> {
    kv.set_msg(&schedule.ns, &keys::schedule(&schedule.name), schedule)
        .await
}

pub async fn load_schedule(
    kv: &dyn KeyValueStore,
    ns: &str,
    name: &str,
) -> Result<Option<models::Schedule>> {
    kv.get_msg(ns, &keys::schedule(name)).await
}

pub async fn dispatch_schedule(
    cp: &ControlPlane,
    schedule: &models::Schedule,
    now: DateTime<Utc>,
) -> Result<String> {
    let spec = schedule
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("schedule spec missing"))?;
    let target = spec
        .target
        .as_ref()
        .ok_or_else(|| anyhow!("schedule target missing"))?;

    let session_mode = normalize_session_mode(&target.session_mode)?;
    let session_id = if session_mode == "new" {
        create_session(cp, &schedule.ns, &target.agent).await?
    } else {
        target.session_id.clone()
    };

    let scheduled_prompt = format_scheduled_message(&schedule.name, &spec.input_message);
    let mut labels = HashMap::new();
    labels.insert("talon.impalasys.com/message-source".to_string(), "schedule".to_string());
    labels.insert(
        "talon.impalasys.com/schedule-name".to_string(),
        schedule.name.clone(),
    );

    send_message(
        cp.kv.as_ref(),
        cp.pubsub.as_ref(),
        &schedule.ns,
        &target.agent,
        &session_id,
        &scheduled_prompt,
        labels,
        now,
    )
    .await?;
    Ok(session_id)
}

fn format_scheduled_message(schedule_name: &str, input_message: &str) -> String {
    format!(
        "[Scheduled run: {}]\nThis is an automated scheduled execution. Execute the task below. Do not create, update, or delete schedules unless the task explicitly asks for that.\n\nTask:\n{}",
        schedule_name,
        input_message.trim()
    )
}

pub fn normalize_schedule_kind(kind: &str) -> String {
    match kind.trim().to_ascii_lowercase().as_str() {
        "interval" | "recurring" => "every".to_string(),
        other => other.to_string(),
    }
}

pub fn normalize_session_mode(session_mode: &str) -> Result<String> {
    match session_mode.trim().to_ascii_lowercase().as_str() {
        "fresh" | "new" => Ok("new".to_string()),
        "named" | "reuse" => Ok("reuse".to_string()),
        other => Err(anyhow!(
            "schedule target session_mode must be 'new' or 'reuse'; got {}",
            other
        )),
    }
}

pub async fn create_session(cp: &ControlPlane, ns: &str, agent: &str) -> Result<String> {
    cp.kv
        .get_msg::<models::Agent>(ns, &keys::agent(agent))
        .await?
        .ok_or_else(|| anyhow!("agent '{}' not found", agent))?;
    let session_id = uuid::Uuid::now_v7().to_string();
    let session = models::Session {
        id: session_id.clone(),
        agent: agent.to_string(),
        ns: ns.to_string(),
        status: "IDLE".to_string(),
        created_at: chrono::Utc::now().timestamp_micros(),
        last_active: chrono::Utc::now().timestamp_micros(),
        metadata: std::collections::HashMap::new(),
        labels: std::collections::HashMap::new(),
    };
    cp.kv
        .set_msg(ns, &keys::session(agent, &session_id), &session)
        .await?;
    tracing::info!(
        namespace = %ns,
        agent = %agent,
        session_id = %session_id,
        "Created session for scheduled dispatch"
    );
    let event = events::LifecycleEvent {
        resource_type: "Session".to_string(),
        name: session_id.clone(),
        ns: ns.to_string(),
        action: events::SystemAction::Create as i32,
        timestamp: chrono::Utc::now().timestamp_micros(),
    };
    cp.pubsub
        .publish(
            crate::control::topics::RESOURCE_LIFECYCLE_TOPIC,
            &event.encode_to_vec(),
        )
        .await?;
    Ok(session_id)
}

pub async fn send_message(
    kv: &dyn KeyValueStore,
    pubsub: &dyn MessagePublisher,
    ns: &str,
    agent: &str,
    session_id: &str,
    message: &str,
    labels: HashMap<String, String>,
    now: DateTime<Utc>,
) -> Result<()> {
    if message.trim().is_empty() {
        return Err(EmptyMessageError.into());
    }

    let key = keys::session(agent, session_id);
    let now_micros = now.timestamp_micros();
    let timeout_micros = session_processing_timeout_micros();

    let mut acquired = false;
    for _ in 0..MAX_CAS_RETRIES {
        let current = kv.get(ns, &key).await?;
        let Some(current_bytes) = current.as_ref() else {
            return Err(SessionNotFoundError.into());
        };
        let mut session = models::Session::decode(current_bytes.as_slice())?;

        if session.status == "PROCESSING"
            && now_micros.saturating_sub(session.last_active) <= timeout_micros
        {
            return Err(SessionCurrentlyProcessingError.into());
        }

        session.status = "PROCESSING".to_string();
        session.last_active = now_micros;
        let updated = session.encode_to_vec();
        if kv
            .compare_and_swap(ns, &key, Some(current_bytes.as_slice()), &updated)
            .await?
        {
            acquired = true;
            break;
        }
    }

    if !acquired {
        return Err(anyhow!("failed to atomically acquire session lock"));
    }

    let user_msg = models::SessionMessage {
        id: uuid::Uuid::now_v7().to_string(),
        role: models::MessageRole::RoleUser as i32,
        content: message.to_string(),
        created_at: now.timestamp_micros(),
        labels,
    };
    if let Err(err) = kv
        .set_msg(
            ns,
            &keys::session_message(agent, session_id, &user_msg.id),
            &user_msg,
        )
        .await
    {
        log_session_release_failure(
            try_release_session_lock_after_send_failure(kv, ns, &key, now_micros).await,
            ns,
            &key,
        );
        return Err(err);
    }

    let message_id = user_msg.id.clone();
    let message_event = events::SessionMessageEvent {
        session_id: session_id.to_string(),
        message_id: message_id.clone(),
        direction: events::MessageDirection::Inbound as i32,
        timestamp: now_micros,
        agent: agent.to_string(),
        message: message.to_string(),
        ns: ns.to_string(),
    };
    if let Err(err) = pubsub
        .publish(
            crate::control::topics::SESSION_DISPATCH_TOPIC,
            &message_event.encode_to_vec(),
        )
        .await
    {
        log_session_release_failure(
            try_release_session_lock_after_send_failure(kv, ns, &key, now_micros).await,
            ns,
            &key,
        );
        return Err(err);
    }
    tracing::info!(
        namespace = %ns,
        agent = %agent,
        session_id = %session_id,
        message_id = %message_id,
        "Queued scheduled message for session dispatch"
    );
    Ok(())
}

fn log_session_release_failure(result: Result<()>, namespace: &str, key: &str) {
    if let Err(err) = result {
        tracing::warn!(namespace = %namespace, key = %key, error = %err, "failed to release session lock after send_message error");
    }
}

async fn try_release_session_lock_after_send_failure(
    kv: &dyn KeyValueStore,
    namespace: &str,
    key: &str,
    expected_last_active: i64,
) -> Result<()> {
    for _ in 0..MAX_CAS_RETRIES {
        let Some(current_bytes) = kv.get(namespace, key).await? else {
            return Ok(());
        };
        let mut session = models::Session::decode(current_bytes.as_slice())?;
        if session.status != "PROCESSING" || session.last_active != expected_last_active {
            return Ok(());
        }
        session.status = "IDLE".to_string();
        let updated = session.encode_to_vec();
        if kv
            .compare_and_swap(namespace, key, Some(current_bytes.as_slice()), &updated)
            .await?
        {
            return Ok(());
        }
    }

    Err(anyhow!(
        "failed to release session lock after send_message error"
    ))
}

pub async fn claim_schedule_wakeup(
    kv: &dyn KeyValueStore,
    namespace: &str,
    schedule_id: &str,
    revision: u64,
    intended_run_at: i64,
    now: DateTime<Utc>,
) -> Result<Option<models::Schedule>> {
    let key = keys::schedule(schedule_id);
    let claim_expires_at = now
        .timestamp_micros()
        .saturating_add(schedule_claim_timeout_micros());

    for _ in 0..MAX_CAS_RETRIES {
        let current = kv.get(namespace, &key).await?;
        let Some(current_bytes) = current.as_ref() else {
            tracing::warn!(
                namespace = %namespace,
                schedule_id = %schedule_id,
                revision = revision,
                intended_run_at = intended_run_at,
                "Schedule wakeup claim found no schedule resource"
            );
            return Ok(None);
        };
        let mut schedule = models::Schedule::decode(current_bytes.as_slice())?;
        let Some(spec) = schedule.spec.as_ref() else {
            tracing::warn!(
                namespace = %namespace,
                schedule_id = %schedule_id,
                "Schedule wakeup claim found schedule without spec"
            );
            return Ok(None);
        };
        let Some(status) = schedule.status.as_mut() else {
            tracing::warn!(
                namespace = %namespace,
                schedule_id = %schedule_id,
                "Schedule wakeup claim found schedule without status"
            );
            return Ok(None);
        };

        if !spec.enabled
            || status.revision != revision
            || status.next_run_at != Some(intended_run_at)
        {
            tracing::warn!(
                namespace = %namespace,
                schedule_id = %schedule_id,
                requested_revision = revision,
                current_revision = status.revision,
                requested_next_run_at = intended_run_at,
                current_next_run_at = ?status.next_run_at,
                enabled = spec.enabled,
                "Schedule wakeup claim skipped because schedule state no longer matches wakeup"
            );
            return Ok(None);
        }

        if status.claimed_run_at == Some(intended_run_at)
            && status.claim_expires_at.unwrap_or_default() > now.timestamp_micros()
        {
            return Err(ScheduleWakeupInProgressError.into());
        }

        status.claimed_run_at = Some(intended_run_at);
        status.claim_expires_at = Some(claim_expires_at);
        append_schedule_event(
            &mut schedule,
            now,
            "claim",
            "acquired",
            format!("claimed wakeup for {}", intended_run_at),
        );
        let updated = schedule.encode_to_vec();
        if kv
            .compare_and_swap(namespace, &key, Some(current_bytes.as_slice()), &updated)
            .await?
        {
            return Ok(Some(schedule));
        }
    }

    Err(anyhow!("failed to atomically claim schedule wakeup"))
}

pub fn release_schedule_claim(schedule: &mut models::Schedule) {
    if let Some(status) = schedule.status.as_mut() {
        status.claimed_run_at = None;
        status.claim_expires_at = None;
    }
}

pub fn append_schedule_event(
    schedule: &mut models::Schedule,
    timestamp: DateTime<Utc>,
    phase: impl Into<String>,
    outcome: impl Into<String>,
    detail: impl Into<String>,
) {
    let status = schedule
        .status
        .get_or_insert_with(models::ScheduleStatus::default);
    status.recent_events.push(models::ScheduleEvent {
        timestamp: timestamp.timestamp_micros(),
        phase: phase.into(),
        outcome: outcome.into(),
        detail: detail.into(),
    });
    if status.recent_events.len() > MAX_RECENT_SCHEDULE_EVENTS {
        let extra = status.recent_events.len() - MAX_RECENT_SCHEDULE_EVENTS;
        status.recent_events.drain(0..extra);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{scheduler::NoopSchedulerBackend, KeyValueStore, MessagePublisher};
    use futures::stream;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockKvStore {
        store: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MockKvStore {
        fn make_key(namespace: &str, key: &str) -> String {
            format!("{namespace}/{key}")
        }
    }

    #[async_trait::async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
            let map = self.store.lock().await;
            Ok(map.get(&Self::make_key(namespace, key)).cloned())
        }

        async fn set(&self, namespace: &str, key: &str, value: &[u8]) -> anyhow::Result<()> {
            let mut map = self.store.lock().await;
            map.insert(Self::make_key(namespace, key), value.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            namespace: &str,
            key: &str,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut map = self.store.lock().await;
            let full_key = Self::make_key(namespace, key);
            let current = map.get(&full_key).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if !matches {
                return Ok(false);
            }
            map.insert(full_key, value.to_vec());
            Ok(true)
        }

        async fn delete(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
            let mut map = self.store.lock().await;
            map.remove(&Self::make_key(namespace, key));
            Ok(())
        }

        async fn list_keys(&self, namespace: &str, prefix: &str) -> anyhow::Result<Vec<String>> {
            let map = self.store.lock().await;
            let ns_prefix = format!("{namespace}/{prefix}");
            let ns_root = format!("{namespace}/");
            Ok(map
                .keys()
                .filter(|key| key.starts_with(&ns_prefix))
                .map(|key| key.strip_prefix(&ns_root).unwrap().to_string())
                .collect())
        }
    }

    #[derive(Default)]
    struct MockPubSub {
        messages: Mutex<Vec<Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl MessagePublisher for MockPubSub {
        async fn publish(&self, _topic: &str, message: &[u8]) -> anyhow::Result<()> {
            self.messages.lock().await.push(message.to_vec());
            Ok(())
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            Ok(Box::pin(stream::empty()))
        }
    }

    fn schedule(kind: &str) -> models::Schedule {
        models::Schedule {
            name: "daily-digest".to_string(),
            ns: "conic:test".to_string(),
            labels: Default::default(),
            spec: Some(models::ScheduleSpec {
                kind: kind.to_string(),
                cron: "0 9 * * *".to_string(),
                interval_seconds: 600,
                run_at: "2026-05-03T09:00:00Z".to_string(),
                timezone: "America/Los_Angeles".to_string(),
                target: Some(models::ScheduleTarget {
                    agent: "assistant".to_string(),
                    session_mode: "new".to_string(),
                    session_id: "".to_string(),
                }),
                input_message: "check in".to_string(),
                enabled: true,
            }),
            status: None,
        }
    }

    #[tokio::test]
    async fn initialize_every_schedule_sets_next_run_and_revision() {
        let mut schedule = schedule("every");
        let now = DateTime::parse_from_rfc3339("2026-05-02T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let next = initialize_schedule(&mut schedule, now).unwrap().unwrap();

        assert_eq!(next, now + Duration::seconds(600));
        let status = schedule.status.unwrap();
        assert_eq!(status.revision, 1);
        assert_eq!(status.next_run_at, Some(next.timestamp_micros()));
    }

    #[tokio::test]
    async fn initialize_schedule_preserves_last_successful_run_context() {
        let mut schedule = schedule("cron");
        schedule.status = Some(models::ScheduleStatus {
            revision: 4,
            next_run_at: Some(123),
            backend_handle: Some("old-handle".to_string()),
            backend_armed: true,
            last_run_at: Some(456),
            last_session_id: Some("session-123".to_string()),
            last_error: Some("previous dispatch failed".to_string()),
            claimed_run_at: Some(789),
            claim_expires_at: Some(999),
            recent_events: vec![models::ScheduleEvent {
                timestamp: 111,
                phase: "dispatch".to_string(),
                outcome: "success".to_string(),
                detail: "started session session-123".to_string(),
            }],
        });
        let now = DateTime::parse_from_rfc3339("2026-05-02T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let next = initialize_schedule(&mut schedule, now).unwrap().unwrap();

        let status = schedule.status.unwrap();
        assert_eq!(status.revision, 5);
        assert_eq!(status.next_run_at, Some(next.timestamp_micros()));
        assert_eq!(status.last_run_at, Some(456));
        assert_eq!(status.last_session_id.as_deref(), Some("session-123"));
        assert_eq!(
            status.last_error.as_deref(),
            Some("previous dispatch failed")
        );
        assert_eq!(status.claimed_run_at, None);
        assert_eq!(status.claim_expires_at, None);
        assert_eq!(status.recent_events.len(), 2);
    }

    #[test]
    fn session_processing_timeout_defaults_to_30_minutes() {
        unsafe {
            std::env::remove_var("TALON_SESSION_PROCESSING_TIMEOUT_SECONDS");
        }
        assert_eq!(
            session_processing_timeout_micros(),
            DEFAULT_SESSION_PROCESSING_TIMEOUT_SECONDS * 1_000_000
        );
    }

    #[test]
    fn validate_reuse_schedule_requires_session_id() {
        let mut schedule = schedule("cron");
        let spec = schedule.spec.as_mut().unwrap();
        let target = spec.target.as_mut().unwrap();
        target.session_mode = "reuse".to_string();

        let err = validate_schedule(&schedule).unwrap_err().to_string();
        assert!(err.contains("session_id is required"));
    }

    #[test]
    fn validate_standard_five_field_cron_expression() {
        let schedule = schedule("cron");
        validate_schedule(&schedule).unwrap();
    }

    #[test]
    fn validate_six_field_cron_expression() {
        let mut schedule = schedule("cron");
        schedule.spec.as_mut().unwrap().cron = "0 */15 * * * *".to_string();
        validate_schedule(&schedule).unwrap();
    }

    #[test]
    fn reject_high_frequency_cron_expression() {
        let mut schedule = schedule("cron");
        schedule.spec.as_mut().unwrap().cron = "*/30 * * * * *".to_string();

        let err = validate_schedule(&schedule).unwrap_err().to_string();

        assert!(err.contains("cron interval must be at least"));
    }

    #[tokio::test]
    async fn arm_schedule_with_noop_backend_leaves_schedule_unarmed() {
        let mut schedule = schedule("at");
        let now = DateTime::parse_from_rfc3339("2026-05-02T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let next = initialize_schedule(&mut schedule, now).unwrap();

        arm_schedule(&NoopSchedulerBackend, &mut schedule, next)
            .await
            .unwrap();

        let status = schedule.status.unwrap();
        assert_eq!(status.backend_handle, None);
        assert!(!status.backend_armed);
    }

    #[test]
    fn at_schedule_has_no_successor_after_fire() {
        let schedule = schedule("at");
        let fired_at = DateTime::parse_from_rfc3339("2026-05-03T09:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let next = compute_successor_run(&schedule, fired_at).unwrap();

        assert_eq!(next, None);
    }

    #[test]
    fn reject_past_at_schedule_initialization() {
        let mut schedule = schedule("at");
        let now = DateTime::parse_from_rfc3339("2026-05-04T09:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let err = initialize_schedule(&mut schedule, now)
            .unwrap_err()
            .to_string();

        assert!(err.contains("run_at must be in the future"));
    }

    #[test]
    fn every_schedule_skips_missed_runs_without_drifting() {
        let schedule = schedule("every");
        let fired_at = DateTime::parse_from_rfc3339("2026-05-03T09:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let now = DateTime::parse_from_rfc3339("2026-05-03T09:23:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let next = compute_aligned_every_successor(&schedule, fired_at, now)
            .unwrap()
            .unwrap();

        assert_eq!(
            next,
            DateTime::parse_from_rfc3339("2026-05-03T09:30:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    #[tokio::test]
    async fn claim_schedule_wakeup_rejects_concurrent_claims() {
        let kv = MockKvStore::default();
        let now = DateTime::parse_from_rfc3339("2026-05-02T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut schedule = schedule("every");
        let next = initialize_schedule(&mut schedule, now).unwrap().unwrap();
        persist_schedule(&kv, &schedule).await.unwrap();

        let first = claim_schedule_wakeup(
            &kv,
            &schedule.ns,
            &schedule.name,
            schedule.status.as_ref().unwrap().revision,
            next.timestamp_micros(),
            now,
        )
        .await
        .unwrap();
        assert!(first.is_some());

        let second = claim_schedule_wakeup(
            &kv,
            &schedule.ns,
            &schedule.name,
            schedule.status.as_ref().unwrap().revision,
            next.timestamp_micros(),
            now,
        )
        .await;
        assert!(second
            .unwrap_err()
            .downcast_ref::<ScheduleWakeupInProgressError>()
            .is_some());
    }

    #[tokio::test]
    async fn send_message_uses_atomic_session_lock() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(MockPubSub::default());
        let cp = ControlPlane {
            kv: kv.clone(),
            pubsub,
            scheduler: Arc::new(NoopSchedulerBackend),
        };

        let session = models::Session {
            id: "session-1".to_string(),
            agent: "assistant".to_string(),
            ns: "conic:test".to_string(),
            status: "IDLE".to_string(),
            created_at: 0,
            last_active: 0,
            metadata: HashMap::new(),
            labels: HashMap::new(),
        };
        kv.set_msg(
            "conic:test",
            &keys::session("assistant", "session-1"),
            &session,
        )
        .await
        .unwrap();

        let now = DateTime::parse_from_rfc3339("2026-05-02T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        send_message(
            cp.kv.as_ref(),
            cp.pubsub.as_ref(),
            "conic:test",
            "assistant",
            "session-1",
            "hello",
            HashMap::new(),
            now,
        )
        .await
        .unwrap();

        let err = send_message(
            cp.kv.as_ref(),
            cp.pubsub.as_ref(),
            "conic:test",
            "assistant",
            "session-1",
            "again",
            HashMap::new(),
            now,
        )
        .await
        .unwrap_err();
        assert!(err
            .downcast_ref::<SessionCurrentlyProcessingError>()
            .is_some());
    }

    #[tokio::test]
    async fn send_message_rejects_empty_content() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(MockPubSub::default());
        let session = models::Session {
            id: "session-1".to_string(),
            agent: "assistant".to_string(),
            ns: "conic:test".to_string(),
            status: "IDLE".to_string(),
            created_at: 0,
            last_active: 0,
            metadata: HashMap::new(),
            labels: HashMap::new(),
        };
        kv.set_msg(
            "conic:test",
            &keys::session("assistant", "session-1"),
            &session,
        )
        .await
        .unwrap();

        let now = DateTime::parse_from_rfc3339("2026-05-02T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let err = send_message(
            kv.as_ref(),
            pubsub.as_ref(),
            "conic:test",
            "assistant",
            "session-1",
            "   ",
            HashMap::new(),
            now,
        )
        .await
        .unwrap_err();
        assert!(err.downcast_ref::<EmptyMessageError>().is_some());
    }

    #[test]
    fn format_scheduled_message_includes_schedule_provenance() {
        assert_eq!(
            format_scheduled_message("hello-world-ping", "  Hello world!  "),
            "[Scheduled run: hello-world-ping]\nThis is an automated scheduled execution. Execute the task below. Do not create, update, or delete schedules unless the task explicitly asks for that.\n\nTask:\nHello world!"
        );
    }
}
