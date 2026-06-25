// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::config::Config;
use crate::control::resource_model::TypedResource;
use crate::control::topics;
use crate::control::ControlPlane;
use crate::harness::mcp::{invalidate_all_broker_auth_cache, invalidate_broker_auth_cache};
use anyhow::{anyhow, Result};
use prost::Message;
use std::sync::Arc;

pub mod controllers;
pub mod fanout;
pub mod mcp_registry;
pub mod registration;
pub mod runtime;
pub mod scheduler_auth;
pub mod sessions;
pub mod sink;
pub mod talon_ops;
pub mod workflows;

const SCHEDULE_WAKEUP_SKEW_TOLERANCE_SECONDS: i64 = 1;

#[derive(Clone)]
pub struct WorkerEventHandler {
    pub cp: Arc<ControlPlane>,
    pub config: Arc<Config>,
    pub mcp_registry: Arc<mcp_registry::McpRegistry>,
    pub scheduler_authenticator: Arc<scheduler_auth::SchedulerRequestAuthenticator>,
    pub worker_id: String,
    pub fanout_hub: Arc<fanout::FanoutHub>,
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
            Some("workflow_dispatch") => {
                let event = crate::control::events::WorkflowDispatchEvent::decode(payload)?;
                self.handle_workflow_dispatch(event).await
            }
            Some("index") => {
                let event = crate::control::events::IndexEvent::decode(payload)?;
                let controller =
                    crate::worker::controllers::index::IndexController::new(self.cp.clone());
                controller.handle_event(event).await
            }
            Some("resource_lifecycle") => {
                if let Ok(event) = crate::control::events::ResourceChangedEvent::decode(payload) {
                    return self.handle_resource_changed_event(event).await;
                }
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

                if let Ok(event) = crate::control::events::WorkflowDispatchEvent::decode(payload) {
                    return self.handle_workflow_dispatch(event).await;
                }

                if let Ok(event) = crate::control::events::IndexEvent::decode(payload) {
                    let controller =
                        crate::worker::controllers::index::IndexController::new(self.cp.clone());
                    return controller.handle_event(event).await;
                }

                if let Ok(event) = crate::control::events::ResourceChangedEvent::decode(payload) {
                    return self.handle_resource_changed_event(event).await;
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

    async fn handle_resource_changed_event(
        &self,
        event: crate::control::events::ResourceChangedEvent,
    ) -> Result<()> {
        if event.resource_kind == "McpServer" {
            self.mcp_registry.invalidate_all().await;
            invalidate_all_broker_auth_cache().await;
        } else if event.resource_kind == "McpServerBinding" {
            self.mcp_registry
                .invalidate(&event.namespace, Some(&event.name))
                .await;
            invalidate_broker_auth_cache(&event.namespace, Some(&event.name)).await;
        }

        crate::worker::controllers::controller::ControllerHost::new(
            self.cp.clone(),
            self.config.clone(),
        )
        .handle_resource_changed(event)
        .await
    }

    pub async fn handle_schedule_wakeup(
        &self,
        payload: crate::control::scheduling::ScheduleWakeupPayload,
    ) -> Result<()> {
        let now = chrono::Utc::now();
        tracing::info!(
            namespace = %payload.namespace,
            schedule_id = %payload.schedule_id,
            revision = payload.revision,
            intended_run_at = payload.intended_run_at,
            "Received schedule wakeup"
        );
        let Some(mut schedule) = crate::control::scheduling::claim_schedule_wakeup(
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
        crate::control::scheduling::append_schedule_event(
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
            crate::control::scheduling::append_schedule_event(
                &mut schedule,
                now,
                "wakeup",
                "deferred",
                format!("wakeup arrived early for {}", intended_fire.to_rfc3339()),
            );
            crate::control::scheduling::release_schedule_claim(&mut schedule);
            crate::control::scheduling::arm_schedule(
                self.cp.scheduler.as_ref(),
                &mut schedule,
                Some(intended_fire),
            )
            .await?;
            crate::control::scheduling::persist_schedule(self.cp.kv.as_ref(), &schedule).await?;
            return Ok(());
        }
        let dispatch_result =
            crate::control::scheduling::dispatch_schedule(&self.cp, &schedule, now).await;
        let dispatch_timestamp = now.timestamp_micros();
        let schedule_namespace = schedule.namespace().to_string();
        let schedule_name = schedule.name().to_string();

        let status = schedule
            .status
            .as_mut()
            .ok_or_else(|| anyhow!("schedule missing status after dispatch"))?;
        match &dispatch_result {
            Ok(session_id) => {
                tracing::info!(
                    namespace = %schedule_namespace,
                    schedule_id = %schedule_name,
                    session_id = %session_id,
                    "Schedule dispatch created session successfully"
                );
                status.last_run_at = Some(dispatch_timestamp);
                status.last_session_id = Some(session_id.clone());
                status.last_error = None;
                crate::control::scheduling::append_schedule_event(
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
                    crate::control::scheduling::append_schedule_event(
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
                    .downcast_ref::<crate::control::scheduling::SessionCurrentlyProcessingError>()
                    .is_some() =>
            {
                tracing::warn!(
                    namespace = %schedule_namespace,
                    schedule_id = %schedule_name,
                    error = %err,
                    "Schedule dispatch skipped because target session is processing"
                );
                status.last_error =
                    Some("skipped: target session is currently processing".to_string());
                crate::control::scheduling::append_schedule_event(
                    &mut schedule,
                    now,
                    "dispatch",
                    "skipped",
                    "target session is currently processing".to_string(),
                );
            }
            Err(err) => {
                tracing::error!(
                    namespace = %schedule_namespace,
                    schedule_id = %schedule_name,
                    error = %err,
                    "Schedule dispatch failed"
                );
                status.last_error = Some(err.to_string());
                crate::control::scheduling::append_schedule_event(
                    &mut schedule,
                    now,
                    "dispatch",
                    "error",
                    err.to_string(),
                );
            }
        }

        crate::control::scheduling::release_schedule_claim(&mut schedule);
        let now = chrono::Utc::now();
        let next = crate::control::scheduling::compute_aligned_every_successor(
            &schedule,
            intended_fire,
            now,
        )?;
        crate::control::scheduling::arm_schedule(self.cp.scheduler.as_ref(), &mut schedule, next)
            .await?;
        crate::control::scheduling::persist_schedule(self.cp.kv.as_ref(), &schedule).await?;

        match dispatch_result {
            Ok(_) => Ok(()),
            Err(err)
                if err
                    .downcast_ref::<crate::control::scheduling::SessionCurrentlyProcessingError>()
                    .is_some() =>
            {
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub async fn handle_workflow_wakeup(
        &self,
        payload: crate::worker::workflows::WorkflowWakeupPayload,
    ) -> Result<()> {
        crate::worker::workflows::handle_workflow_wakeup(&self.cp, payload).await
    }

    pub fn event_type_for_subscription(subscription: &str) -> Option<&'static str> {
        if subscription.contains(topics::SESSION_DISPATCH_TOPIC) {
            Some("session_dispatch")
        } else if subscription.contains(topics::SESSION_CONTROL_TOPIC) {
            Some("session_control")
        } else if subscription.contains(topics::WORKFLOW_DISPATCH_TOPIC) {
            Some("workflow_dispatch")
        } else if subscription.contains(topics::INDEX_EVENTS_TOPIC) {
            Some("index")
        } else if subscription.contains(topics::RESOURCE_LIFECYCLE_TOPIC) {
            Some("resource_lifecycle")
        } else {
            None
        }
    }
}

impl WorkerEventHandler {
    pub async fn handle_workflow_dispatch(
        &self,
        event: crate::control::events::WorkflowDispatchEvent,
    ) -> Result<()> {
        tracing::info!(
            namespace = %event.ns,
            workflow = %event.workflow,
            run_id = %event.run_id,
            reason = %event.reason,
            "Handling workflow dispatch"
        );
        let Some(run) = crate::worker::workflows::claim_run(
            self.cp.kv.as_ref(),
            &event.ns,
            &event.workflow,
            &event.run_id,
            chrono::Utc::now(),
            &event.reason,
        )
        .await?
        else {
            return Ok(());
        };
        crate::worker::workflows::advance_run(&self.cp, run).await
    }
}

#[cfg(test)]
mod tests {
    use super::WorkerEventHandler;
    use crate::control::config::Config;
    use crate::control::{
        events::{LifecycleEvent, MessageDirection, SessionControlEvent, SessionMessageEvent},
        keys::{ResourceKey, ResourceList},
        topics, ControlPlane, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::{data_proto, manifests, resources_proto};
    use crate::worker::{mcp_registry::McpRegistry, scheduler_auth::SchedulerRequestAuthenticator};
    use async_trait::async_trait;
    use chrono::{Duration, Utc};
    use futures::stream;
    use prost::Message;
    use std::{collections::HashMap, pin::Pin, sync::Arc};
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockKvStore {
        data: Mutex<HashMap<ResourceKey, Vec<u8>>>,
    }

    #[async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, key: &ResourceKey) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.data.lock().await.get(key).cloned())
        }

        async fn set(&self, key: &ResourceKey, value: &[u8]) -> anyhow::Result<()> {
            self.data.lock().await.insert(key.clone(), value.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            key: &ResourceKey,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let current = data.get(key).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(key.clone(), value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, key: &ResourceKey) -> anyhow::Result<()> {
            self.data.lock().await.remove(key);
            Ok(())
        }

        async fn list_keys(&self, list: &ResourceList) -> anyhow::Result<Vec<ResourceKey>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|key| list.matches(key).then(|| key.clone()))
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
            cp: Arc::new(ControlPlane::builder(kv, pubsub).build()),
            config: Arc::new(Config::default()),
            mcp_registry: Arc::new(McpRegistry::new()),
            scheduler_authenticator: Arc::new(SchedulerRequestAuthenticator::deny_all()),
            worker_id: "test-worker".to_string(),
            fanout_hub: Arc::new(crate::worker::fanout::FanoutHub::new()),
            session_cancellations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn schedule(revision: u64, next_run_at: i64, session_mode: &str) -> resources_proto::Schedule {
        crate::control::resource_model::schedule(
            "conic:test",
            "nightly",
            resources_proto::ScheduleSpec {
                kind: "every".to_string(),
                cron: String::new(),
                interval_seconds: 600,
                run_at: String::new(),
                timezone: String::new(),
                target: Some(resources_proto::ScheduleTarget {
                    agent: "assistant".to_string(),
                    workflow: String::new(),
                    session_mode: session_mode.to_string(),
                    session_id: "session-1".to_string(),
                }),
                input_message: "Run the report".to_string(),
                input_json: String::new(),
                enabled: true,
            },
            resources_proto::ScheduleStatus {
                observed_generation: 0,
                phase: String::new(),
                conditions: Vec::new(),
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
            },
            HashMap::new(),
        )
    }

    async fn seed_agent_and_session(kv: &MockKvStore) {
        kv.set_msg(
            &crate::control::keys::agent("conic:test", "assistant"),
            &crate::control::resource_model::agent(
                "conic:test",
                "assistant",
                manifests::AgentSpec::default(),
                HashMap::new(),
            ),
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::session("conic:test", "assistant", "session-1"),
            &data_proto::Session {
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
                topics::WORKFLOW_DISPATCH_TOPIC
            )),
            Some("workflow_dispatch")
        );
        assert_eq!(
            WorkerEventHandler::event_type_for_subscription(&format!(
                "projects/test/subscriptions/{}",
                topics::INDEX_EVENTS_TOPIC
            )),
            Some("index")
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
    async fn dispatch_accepts_typed_index_events() {
        let handler = handler(
            Arc::new(MockKvStore::default()),
            Arc::new(MockPubSub::default()),
        );
        let event = crate::control::events::IndexEvent {
            id: "event-1".to_string(),
            key: crate::control::keys::session_message("acme", "support", "s1", "m1").canonical(),
            ..Default::default()
        };

        handler
            .dispatch(Some("index"), &event.encode_to_vec())
            .await
            .unwrap();
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
            .handle_schedule_wakeup(crate::control::scheduling::ScheduleWakeupPayload {
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
            &crate::control::keys::schedule("conic:test", "nightly"),
            &schedule(3, intended, "reuse"),
        )
        .await
        .unwrap();

        handler
            .handle_schedule_wakeup(crate::control::scheduling::ScheduleWakeupPayload {
                namespace: "conic:test".to_string(),
                schedule_id: "nightly".to_string(),
                revision: 3,
                intended_run_at: intended,
            })
            .await
            .expect("early wakeup should defer");

        let updated = kv
            .get_msg::<resources_proto::Schedule>(&crate::control::keys::schedule(
                "conic:test",
                "nightly",
            ))
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
            &crate::control::keys::schedule("conic:test", "nightly"),
            &schedule(7, intended, "reuse"),
        )
        .await
        .unwrap();

        handler
            .handle_schedule_wakeup(crate::control::scheduling::ScheduleWakeupPayload {
                namespace: "conic:test".to_string(),
                schedule_id: "nightly".to_string(),
                revision: 7,
                intended_run_at: intended,
            })
            .await
            .expect("schedule dispatch should succeed");

        let updated = kv
            .get_msg::<resources_proto::Schedule>(&crate::control::keys::schedule(
                "conic:test",
                "nightly",
            ))
            .await
            .unwrap()
            .unwrap();
        let status = updated.status.unwrap();
        assert_eq!(status.last_session_id.as_deref(), Some("session-1"));
        assert!(status.last_run_at.is_some());
        assert!(status.last_error.is_none());
        assert!(status.next_run_at.is_some());

        let session = kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.status, "PROCESSING");

        let message_keys = kv
            .list_keys(&crate::control::keys::session_message_prefix(
                "conic:test",
                "assistant",
                "session-1",
            ))
            .await
            .unwrap();
        assert_eq!(message_keys.len(), 1);

        let published = pubsub.published.lock().await;
        let index_event = published
            .iter()
            .find_map(|(topic, payload)| {
                (topic == topics::INDEX_EVENTS_TOPIC)
                    .then(|| crate::control::events::IndexEvent::decode(payload.as_slice()).ok())
                    .flatten()
            })
            .expect("scheduled message should publish a search index event");
        assert_eq!(
            index_event.operation,
            crate::control::events::IndexOperation::Upsert as i32
        );
        assert_eq!(index_event.key, message_keys[0].canonical());

        let event = published
            .iter()
            .find_map(|(topic, payload)| {
                (topic == topics::SESSION_DISPATCH_TOPIC)
                    .then(|| SessionMessageEvent::decode(payload.as_slice()).ok())
                    .flatten()
            })
            .expect("scheduled message should publish a dispatch event");
        assert_eq!(event.direction, MessageDirection::Inbound as i32);
        assert_eq!(event.agent, "assistant");
        assert_eq!(event.session_id, "session-1");
        assert!(event.message.contains("Scheduled run: nightly"));
    }
}
