// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::config::Config;
use crate::connectors::mcp::{invalidate_all_broker_auth_cache, invalidate_broker_auth_cache};
use crate::control::topics;
use crate::control::ControlPlane;
use anyhow::{anyhow, Result};
use prost::Message;
use std::sync::Arc;

pub mod mcp_registry;
pub mod runtime;
pub mod scheduler_auth;
pub mod sessions;
pub mod sink;
pub mod talon_ops;

const SCHEDULE_WAKEUP_SKEW_TOLERANCE_SECONDS: i64 = 1;

#[derive(Clone)]
pub struct WorkerEventHandler {
    pub cp: Arc<ControlPlane>,
    pub config: Arc<Config>,
    pub mcp_registry: Arc<mcp_registry::McpRegistry>,
    pub scheduler_authenticator: Arc<scheduler_auth::SchedulerRequestAuthenticator>,
    pub session_cancellations:
        Arc<tokio::sync::Mutex<std::collections::HashMap<String, tokio_util::sync::CancellationToken>>>,
}

impl WorkerEventHandler {
    pub async fn dispatch(&self, event_type: Option<&str>, payload: &[u8]) -> Result<()> {
        match event_type {
            Some("session_dispatch") => {
                let event = crate::control::events::SessionMessageEvent::decode(payload)?;
                self.handle_session_message(event).await
            }
            Some("session_control") => {
                let event = crate::control::events::SessionControlEvent::decode(payload)?;
                self.handle_session_control(event).await
            }
            Some("resource_lifecycle") => {
                let event = crate::control::events::LifecycleEvent::decode(payload)?;
                self.handle_lifecycle_event(event).await
            }
            Some(other) => Err(anyhow!("Unknown worker event type '{}'", other)),
            None => {
                if let Ok(event) = crate::control::events::SessionMessageEvent::decode(payload) {
                    return self.handle_session_message(event).await;
                }

                if let Ok(event) = crate::control::events::SessionControlEvent::decode(payload) {
                    return self.handle_session_control(event).await;
                }

                if let Ok(event) = crate::control::events::LifecycleEvent::decode(payload) {
                    return self.handle_lifecycle_event(event).await;
                }

                Err(anyhow!(
                    "Received unknown event payload of size {} bytes",
                    payload.len()
                ))
            }
        }
    }

    async fn handle_lifecycle_event(
        &self,
        event: crate::control::events::LifecycleEvent,
    ) -> Result<()> {
        if event.resource_type == "McpServer" {
            self.mcp_registry.invalidate_all().await;
            invalidate_all_broker_auth_cache().await;
        } else if event.resource_type == "McpServerBinding" {
            self.mcp_registry
                .invalidate(&event.ns, Some(&event.name))
                .await;
            invalidate_broker_auth_cache(&event.ns, Some(&event.name)).await;
        }
        Ok(())
    }

    pub async fn handle_schedule_wakeup(
        &self,
        payload: crate::scheduling::ScheduleWakeupPayload,
    ) -> Result<()> {
        let now = chrono::Utc::now();
        tracing::info!(
            namespace = %payload.namespace,
            schedule_id = %payload.schedule_id,
            revision = payload.revision,
            intended_run_at = payload.intended_run_at,
            "Received schedule wakeup"
        );
        let Some(mut schedule) = crate::scheduling::claim_schedule_wakeup(
            self.cp.kv.as_ref(),
            &payload.namespace,
            &payload.schedule_id,
            payload.revision,
            payload.intended_run_at,
            now,
        )
        .await?
        else {
            tracing::warn!(
                namespace = %payload.namespace,
                schedule_id = %payload.schedule_id,
                revision = payload.revision,
                intended_run_at = payload.intended_run_at,
                "Schedule wakeup was acknowledged but no matching runnable schedule was found"
            );
            return Ok(());
        };

        let is_one_shot = match schedule.spec.as_ref() {
            Some(spec) => spec.kind == "at",
            None => return Ok(()),
        };
        if schedule.status.is_none() {
            return Ok(());
        }
        crate::scheduling::append_schedule_event(
            &mut schedule,
            now,
            "wakeup",
            "received",
            format!("processing revision {}", payload.revision),
        );

        let intended_fire = chrono::DateTime::from_timestamp_micros(payload.intended_run_at)
            .ok_or_else(|| anyhow!("invalid intended_run_at {}", payload.intended_run_at))?;
        if intended_fire > now + chrono::Duration::seconds(SCHEDULE_WAKEUP_SKEW_TOLERANCE_SECONDS) {
            tracing::info!(
                namespace = %payload.namespace,
                schedule_id = %payload.schedule_id,
                intended_fire = %intended_fire,
                now = %now,
                "Deferring early schedule wakeup"
            );
            crate::scheduling::append_schedule_event(
                &mut schedule,
                now,
                "wakeup",
                "deferred",
                format!("wakeup arrived early for {}", intended_fire.to_rfc3339()),
            );
            crate::scheduling::release_schedule_claim(&mut schedule);
            crate::scheduling::arm_schedule(
                self.cp.scheduler.as_ref(),
                &mut schedule,
                Some(intended_fire),
            )
            .await?;
            crate::scheduling::persist_schedule(self.cp.kv.as_ref(), &schedule).await?;
            return Ok(());
        }
        let dispatch_result = crate::scheduling::dispatch_schedule(&self.cp, &schedule, now).await;
        let dispatch_timestamp = now.timestamp_micros();

        let status = schedule
            .status
            .as_mut()
            .ok_or_else(|| anyhow!("schedule missing status after dispatch"))?;
        match &dispatch_result {
            Ok(session_id) => {
                tracing::info!(
                    namespace = %schedule.ns,
                    schedule_id = %schedule.name,
                    session_id = %session_id,
                    "Schedule dispatch created session successfully"
                );
                status.last_run_at = Some(dispatch_timestamp);
                status.last_session_id = Some(session_id.clone());
                status.last_error = None;
                crate::scheduling::append_schedule_event(
                    &mut schedule,
                    now,
                    "dispatch",
                    "success",
                    format!("started session {}", session_id),
                );
                if is_one_shot {
                    let spec = schedule
                        .spec
                        .as_mut()
                        .ok_or_else(|| anyhow!("schedule missing spec after dispatch"))?;
                    spec.enabled = false;
                    crate::scheduling::append_schedule_event(
                        &mut schedule,
                        now,
                        "dispatch",
                        "disabled",
                        "one-shot schedule completed and was disabled".to_string(),
                    );
                }
            }
            Err(err)
                if err
                    .downcast_ref::<crate::scheduling::SessionCurrentlyProcessingError>()
                    .is_some() =>
            {
                tracing::warn!(
                    namespace = %schedule.ns,
                    schedule_id = %schedule.name,
                    error = %err,
                    "Schedule dispatch skipped because target session is processing"
                );
                status.last_error =
                    Some("skipped: target session is currently processing".to_string());
                crate::scheduling::append_schedule_event(
                    &mut schedule,
                    now,
                    "dispatch",
                    "skipped",
                    "target session is currently processing".to_string(),
                );
            }
            Err(err) => {
                tracing::error!(
                    namespace = %schedule.ns,
                    schedule_id = %schedule.name,
                    error = %err,
                    "Schedule dispatch failed"
                );
                status.last_error = Some(err.to_string());
                crate::scheduling::append_schedule_event(
                    &mut schedule,
                    now,
                    "dispatch",
                    "error",
                    err.to_string(),
                );
            }
        }

        crate::scheduling::release_schedule_claim(&mut schedule);
        let now = chrono::Utc::now();
        let next =
            crate::scheduling::compute_aligned_every_successor(&schedule, intended_fire, now)?;
        crate::scheduling::arm_schedule(self.cp.scheduler.as_ref(), &mut schedule, next).await?;
        crate::scheduling::persist_schedule(self.cp.kv.as_ref(), &schedule).await?;

        match dispatch_result {
            Ok(_) => Ok(()),
            Err(err)
                if err
                    .downcast_ref::<crate::scheduling::SessionCurrentlyProcessingError>()
                    .is_some() =>
            {
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub fn event_type_for_subscription(subscription: &str) -> Option<&'static str> {
        if subscription.contains(topics::SESSION_DISPATCH_TOPIC) {
            Some("session_dispatch")
        } else if subscription.contains(topics::SESSION_CONTROL_TOPIC) {
            Some("session_control")
        } else if subscription.contains(topics::RESOURCE_LIFECYCLE_TOPIC) {
            Some("resource_lifecycle")
        } else {
            None
        }
    }
}
