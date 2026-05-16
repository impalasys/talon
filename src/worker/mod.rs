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
    pub session_cancellations: Arc<
        tokio::sync::Mutex<std::collections::HashMap<String, tokio_util::sync::CancellationToken>>,
    >,
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

#[cfg(test)]
mod tests {
    use super::WorkerEventHandler;
    use crate::config::Config;
    use crate::control::{
        events::{LifecycleEvent, MessageDirection, SessionControlEvent, SessionMessageEvent},
        scheduler::NoopSchedulerBackend,
        topics, ControlPlane, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::{manifests, models};
    use crate::worker::{mcp_registry::McpRegistry, scheduler_auth::SchedulerRequestAuthenticator};
    use async_trait::async_trait;
    use chrono::{Duration, Utc};
    use futures::stream;
    use prost::Message;
    use std::{collections::HashMap, pin::Pin, sync::Arc};
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockKvStore {
        data: Mutex<HashMap<(String, String), Vec<u8>>>,
    }

    #[async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, ns: &str, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self
                .data
                .lock()
                .await
                .get(&(ns.to_string(), key.to_string()))
                .cloned())
        }

        async fn set(&self, ns: &str, key: &str, value: &[u8]) -> anyhow::Result<()> {
            self.data
                .lock()
                .await
                .insert((ns.to_string(), key.to_string()), value.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            ns: &str,
            key: &str,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let full_key = (ns.to_string(), key.to_string());
            let current = data.get(&full_key).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(full_key, value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, ns: &str, key: &str) -> anyhow::Result<()> {
            self.data
                .lock()
                .await
                .remove(&(ns.to_string(), key.to_string()));
            Ok(())
        }

        async fn list_keys(&self, ns: &str, prefix: &str) -> anyhow::Result<Vec<String>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|(stored_ns, key)| {
                    (stored_ns == ns && key.starts_with(prefix)).then(|| key.clone())
                })
                .collect::<Vec<_>>();
            keys.sort();
            Ok(keys)
        }
    }

    #[derive(Default)]
    struct MockPubSub {
        published: Mutex<Vec<(String, Vec<u8>)>>,
    }

    #[async_trait]
    impl MessagePublisher for MockPubSub {
        async fn publish(&self, topic: &str, message: &[u8]) -> anyhow::Result<()> {
            self.published
                .lock()
                .await
                .push((topic.to_string(), message.to_vec()));
            Ok(())
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            Ok(Box::pin(stream::empty()))
        }
    }

    fn handler(kv: Arc<MockKvStore>, pubsub: Arc<MockPubSub>) -> WorkerEventHandler {
        WorkerEventHandler {
            cp: Arc::new(ControlPlane {
                kv,
                pubsub,
                scheduler: Arc::new(NoopSchedulerBackend),
            }),
            config: Arc::new(Config::default()),
            mcp_registry: Arc::new(McpRegistry::new()),
            scheduler_authenticator: Arc::new(SchedulerRequestAuthenticator::deny_all()),
            session_cancellations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn schedule(revision: u64, next_run_at: i64, session_mode: &str) -> models::Schedule {
        models::Schedule {
            name: "nightly".to_string(),
            ns: "conic:test".to_string(),
            labels: HashMap::new(),
            spec: Some(models::ScheduleSpec {
                kind: "every".to_string(),
                cron: String::new(),
                interval_seconds: 600,
                run_at: String::new(),
                timezone: String::new(),
                target: Some(models::ScheduleTarget {
                    agent: "assistant".to_string(),
                    session_mode: session_mode.to_string(),
                    session_id: "session-1".to_string(),
                }),
                input_message: "Run the report".to_string(),
                enabled: true,
            }),
            status: Some(models::ScheduleStatus {
                revision,
                next_run_at: Some(next_run_at),
                backend_handle: None,
                backend_armed: false,
                last_run_at: None,
                last_session_id: None,
                last_error: None,
                claimed_run_at: None,
                claim_expires_at: None,
                recent_events: Vec::new(),
            }),
        }
    }

    async fn seed_agent_and_session(kv: &MockKvStore) {
        kv.set_msg(
            "conic:test",
            &crate::control::keys::agent("assistant"),
            &models::Agent {
                name: "assistant".to_string(),
                ns: "conic:test".to_string(),
                definition: Some(manifests::AgentDefinition::default()),
                effective_spec: Some(manifests::AgentSpec::default()),
                template_deps: Vec::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            "conic:test",
            &crate::control::keys::session("assistant", "session-1"),
            &models::Session {
                id: "session-1".to_string(),
                agent: "assistant".to_string(),
                ns: "conic:test".to_string(),
                status: "IDLE".to_string(),
                created_at: 0,
                last_active: 0,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
    }

    #[test]
    fn event_type_for_subscription_maps_known_topics() {
        assert_eq!(
            WorkerEventHandler::event_type_for_subscription(&format!(
                "projects/test/subscriptions/{}",
                topics::SESSION_DISPATCH_TOPIC
            )),
            Some("session_dispatch")
        );
        assert_eq!(
            WorkerEventHandler::event_type_for_subscription(&format!(
                "projects/test/subscriptions/{}",
                topics::SESSION_CONTROL_TOPIC
            )),
            Some("session_control")
        );
        assert_eq!(
            WorkerEventHandler::event_type_for_subscription(&format!(
                "projects/test/subscriptions/{}",
                topics::RESOURCE_LIFECYCLE_TOPIC
            )),
            Some("resource_lifecycle")
        );
        assert_eq!(
            WorkerEventHandler::event_type_for_subscription("unknown"),
            None
        );
    }

    #[tokio::test]
    async fn dispatch_rejects_unknown_event_types_and_payloads() {
        let handler = handler(
            Arc::new(MockKvStore::default()),
            Arc::new(MockPubSub::default()),
        );

        let unknown_type = handler
            .dispatch(Some("wat"), &[])
            .await
            .expect_err("unknown event type should fail");
        assert!(unknown_type
            .to_string()
            .contains("Unknown worker event type"));

        let unknown_payload = handler
            .dispatch(None, b"not-protobuf")
            .await
            .expect_err("unknown payload should fail");
        assert!(unknown_payload
            .to_string()
            .contains("Received unknown event payload"));
    }

    #[tokio::test]
    async fn dispatch_accepts_lifecycle_and_stop_generation_events() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(MockPubSub::default());
        let handler = handler(kv, pubsub);

        handler.session_cancellations.lock().await.insert(
            "session-1".to_string(),
            tokio_util::sync::CancellationToken::new(),
        );

        let lifecycle = LifecycleEvent {
            resource_type: "McpServerBinding".to_string(),
            name: "binding-1".to_string(),
            ns: "conic:test".to_string(),
            action: 1,
            timestamp: 123,
        };
        handler
            .dispatch(Some("resource_lifecycle"), &lifecycle.encode_to_vec())
            .await
            .expect("lifecycle event should dispatch");

        let stop = SessionControlEvent {
            session_id: "session-1".to_string(),
            action: "stop_generation".to_string(),
            agent: "assistant".to_string(),
            ns: "conic:test".to_string(),
            timestamp: 0,
        };
        handler
            .dispatch(Some("session_control"), &stop.encode_to_vec())
            .await
            .expect("session control should dispatch");
    }

    #[tokio::test]
    async fn handle_schedule_wakeup_returns_ok_when_no_schedule_matches() {
        let handler = handler(
            Arc::new(MockKvStore::default()),
            Arc::new(MockPubSub::default()),
        );

        handler
            .handle_schedule_wakeup(crate::scheduling::ScheduleWakeupPayload {
                namespace: "conic:test".to_string(),
                schedule_id: "missing".to_string(),
                revision: 1,
                intended_run_at: Utc::now().timestamp_micros(),
            })
            .await
            .expect("missing schedule should be ignored");
    }

    #[tokio::test]
    async fn handle_schedule_wakeup_defers_early_delivery() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(MockPubSub::default());
        let handler = handler(kv.clone(), pubsub);
        let intended = (Utc::now() + Duration::seconds(30)).timestamp_micros();
        kv.set_msg(
            "conic:test",
            &crate::control::keys::schedule("nightly"),
            &schedule(3, intended, "reuse"),
        )
        .await
        .unwrap();

        handler
            .handle_schedule_wakeup(crate::scheduling::ScheduleWakeupPayload {
                namespace: "conic:test".to_string(),
                schedule_id: "nightly".to_string(),
                revision: 3,
                intended_run_at: intended,
            })
            .await
            .expect("early wakeup should defer");

        let updated = kv
            .get_msg::<models::Schedule>("conic:test", &crate::control::keys::schedule("nightly"))
            .await
            .unwrap()
            .unwrap();
        let status = updated.status.unwrap();
        assert_eq!(status.claimed_run_at, None);
        assert_eq!(status.next_run_at, Some(intended));
        assert!(status
            .recent_events
            .iter()
            .any(|event| event.outcome == "deferred"));
    }

    #[tokio::test]
    async fn handle_schedule_wakeup_dispatches_message_and_updates_schedule() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(MockPubSub::default());
        let handler = handler(kv.clone(), pubsub.clone());
        seed_agent_and_session(kv.as_ref()).await;
        let intended = (Utc::now() - Duration::seconds(2)).timestamp_micros();
        kv.set_msg(
            "conic:test",
            &crate::control::keys::schedule("nightly"),
            &schedule(7, intended, "reuse"),
        )
        .await
        .unwrap();

        handler
            .handle_schedule_wakeup(crate::scheduling::ScheduleWakeupPayload {
                namespace: "conic:test".to_string(),
                schedule_id: "nightly".to_string(),
                revision: 7,
                intended_run_at: intended,
            })
            .await
            .expect("schedule dispatch should succeed");

        let updated = kv
            .get_msg::<models::Schedule>("conic:test", &crate::control::keys::schedule("nightly"))
            .await
            .unwrap()
            .unwrap();
        let status = updated.status.unwrap();
        assert_eq!(status.last_session_id.as_deref(), Some("session-1"));
        assert!(status.last_run_at.is_some());
        assert!(status.last_error.is_none());
        assert!(status.next_run_at.is_some());

        let session = kv
            .get_msg::<models::Session>(
                "conic:test",
                &crate::control::keys::session("assistant", "session-1"),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.status, "PROCESSING");

        let message_keys = kv
            .list_keys(
                "conic:test",
                &crate::control::keys::session_message_prefix("assistant", "session-1"),
            )
            .await
            .unwrap();
        assert_eq!(message_keys.len(), 1);

        let published = pubsub.published.lock().await;
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, topics::SESSION_DISPATCH_TOPIC);
        let event = SessionMessageEvent::decode(published[0].1.as_slice()).unwrap();
        assert_eq!(event.direction, MessageDirection::Inbound as i32);
        assert_eq!(event.agent, "assistant");
        assert_eq!(event.session_id, "session-1");
        assert!(event.message.contains("Scheduled run: nightly"));
    }
}
