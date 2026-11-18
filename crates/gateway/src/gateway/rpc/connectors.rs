// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{data_proto, external_proto, resources_proto, GrpcGatewayHandler};
use ::connectors::controller as connectors;
use ::control::control::keys;
use ::control::control::resource_model::ChannelResourceExt;
use ::control::control::resources::ResourceStore;
use ::control::control::scheduling;
use ::control::control::ControlPlane;
use ::control::control::ProtoKeyValueStoreExt;
use anyhow::Context;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub use ::connectors::rpc::*;
pub use ::control::connector_runtime::*;

impl GrpcGatewayHandler {
    pub async fn handle_ingest_connector_message_event(
        &self,
        req: tonic::Request<external_proto::ConnectorMessageEvent>,
    ) -> Result<tonic::Response<external_proto::ConnectorMessageEventResponse>, tonic::Status> {
        let started = Instant::now();
        let event = req.into_inner();
        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            external_conversation_id = %event.external_conversation_id,
            external_thread_id = %event.external_thread_id.as_deref().unwrap_or_default(),
            external_message_id = %event.external_message_id,
            "connector message event received"
        );
        if event.registration_id.trim().is_empty() {
            return Err(tonic::Status::invalid_argument(
                "registration_id is required",
            ));
        }
        if event.event_id.trim().is_empty() {
            return Err(tonic::Status::invalid_argument("event_id is required"));
        }
        if event.event_kind != external_proto::ConnectorMessageEventKind::Created as i32 {
            return Err(tonic::Status::invalid_argument(format!(
                "unsupported connector event_kind {}",
                event.event_kind
            )));
        }

        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            elapsed_ms = started.elapsed().as_millis(),
            "connector message event resolving ConnectorClass"
        );
        let class = self
            .class_for_registration(&event.registration_id, &event.connector_class)
            .await?;
        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            class_namespace = %class.namespace,
            class_name = %class.name,
            elapsed_ms = started.elapsed().as_millis(),
            "connector message event resolved ConnectorClass"
        );

        let event_key = keys::connector_event(&class.namespace, &class.name, &event.event_id);
        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            class_namespace = %class.namespace,
            class_name = %class.name,
            event_key = %event_key,
            elapsed_ms = started.elapsed().as_millis(),
            "connector message event reserving"
        );
        if !self
            .gateway
            .kv
            .compare_and_swap(&event_key, None, b"reserved")
            .await
            .map_err(internal_error)?
        {
            tracing::info!(
                registration_id = %event.registration_id,
                connector_class = %event.connector_class,
                event_id = %event.event_id,
                class_namespace = %class.namespace,
                class_name = %class.name,
                elapsed_ms = started.elapsed().as_millis(),
                "connector message event duplicate ignored"
            );
            return Ok(tonic::Response::new(
                external_proto::ConnectorMessageEventResponse {
                    status: external_proto::ConnectorMessageEventStatus::Duplicate as i32,
                    reason: String::new(),
                    namespace: String::new(),
                    connector_name: String::new(),
                    consumer: None,
                },
            ));
        }
        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            class_namespace = %class.namespace,
            class_name = %class.name,
            event_key = %event_key,
            elapsed_ms = started.elapsed().as_millis(),
            "connector message event reserved"
        );

        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            class_namespace = %class.namespace,
            class_name = %class.name,
            elapsed_ms = started.elapsed().as_millis(),
            "connector message event resolving route"
        );
        let Some(route) = connectors::resolve_route(
            self.gateway.kv.as_ref(),
            &class.namespace,
            &class.name,
            &class.spec,
            &event.match_fields,
        )
        .await
        .map_err(internal_error)?
        else {
            tracing::warn!(
                registration_id = %event.registration_id,
                connector_class = %event.connector_class,
                event_id = %event.event_id,
                class_namespace = %class.namespace,
                class_name = %class.name,
                elapsed_ms = started.elapsed().as_millis(),
                "connector message event did not match any Connector"
            );
            self.gateway
                .kv
                .set(&event_key, b"unmatched")
                .await
                .map_err(internal_error)?;
            return Ok(tonic::Response::new(
                external_proto::ConnectorMessageEventResponse {
                    status: external_proto::ConnectorMessageEventStatus::Unmatched as i32,
                    reason: "unmatched".to_string(),
                    namespace: String::new(),
                    connector_name: String::new(),
                    consumer: None,
                },
            ));
        };
        let connector_ref = route_connector_ref(&route)?;
        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            class_namespace = %class.namespace,
            class_name = %class.name,
            connector_namespace = %connector_ref.namespace,
            connector_name = %connector_ref.name,
            route_uid = %route.connector_uid,
            consumer_kind = %message_consumer_kind(&route.consumer),
            elapsed_ms = started.elapsed().as_millis(),
            "connector message event matched route"
        );

        let consumer = route
            .consumer
            .clone()
            .ok_or_else(|| tonic::Status::failed_precondition("Route consumer is missing"))?;
        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            class_namespace = %class.namespace,
            class_name = %class.name,
            connector_namespace = %connector_ref.namespace,
            connector_name = %connector_ref.name,
            route_uid = %route.connector_uid,
            consumer_kind = %message_consumer_kind(&Some(consumer.clone())),
            elapsed_ms = started.elapsed().as_millis(),
            "connector message event dispatch starting"
        );
        let dispatch = match dispatch_connector_message(
            &self.gateway.control_plane(),
            &route,
            &consumer,
            &event,
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(err) => {
                if let Err(delete_err) = self.gateway.kv.delete(&event_key).await {
                    tracing::warn!(
                        error = %delete_err,
                        registration_id = %event.registration_id,
                        event_id = %event.event_id,
                        "failed to release connector event reservation after dispatch error"
                    );
                }
                tracing::warn!(
                    error = %err,
                    registration_id = %event.registration_id,
                    connector_class = %event.connector_class,
                    event_id = %event.event_id,
                    class_namespace = %class.namespace,
                    class_name = %class.name,
                    connector_namespace = %connector_ref.namespace,
                    connector_name = %connector_ref.name,
                    route_uid = %route.connector_uid,
                    elapsed_ms = started.elapsed().as_millis(),
                    "connector message event dispatch failed"
                );
                return Err(err);
            }
        };

        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            class_namespace = %class.namespace,
            class_name = %class.name,
            connector_namespace = %connector_ref.namespace,
            connector_name = %connector_ref.name,
            route_uid = %route.connector_uid,
            elapsed_ms = started.elapsed().as_millis(),
            "connector message event marking dispatched"
        );
        self.gateway
            .kv
            .set(&event_key, b"dispatched")
            .await
            .map_err(internal_error)?;
        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            class_namespace = %class.namespace,
            class_name = %class.name,
            connector_namespace = %connector_ref.namespace,
            connector_name = %connector_ref.name,
            route_uid = %route.connector_uid,
            elapsed_ms = started.elapsed().as_millis(),
            "connector message event dispatched"
        );

        let connector = route_connector_ref(&route)?;
        Ok(tonic::Response::new(
            external_proto::ConnectorMessageEventResponse {
                status: external_proto::ConnectorMessageEventStatus::Accepted as i32,
                reason: String::new(),
                namespace: connector.namespace.clone(),
                connector_name: connector.name.clone(),
                consumer: Some(dispatch.consumer),
            },
        ))
    }

    pub async fn handle_report_connector_status(
        &self,
        req: tonic::Request<external_proto::ConnectorStatusEvent>,
    ) -> Result<tonic::Response<external_proto::ConnectorAckResponse>, tonic::Status> {
        let status = req.into_inner();
        if status.registration_id.trim().is_empty() {
            return Err(tonic::Status::invalid_argument(
                "registration_id is required",
            ));
        }
        tracing::info!(
            registration_id = %status.registration_id,
            status = %status.status,
            reason = %status.reason,
            "connector status event received"
        );
        Ok(tonic::Response::new(external_proto::ConnectorAckResponse {
            accepted: true,
            disposition: "accepted".to_string(),
        }))
    }

    async fn class_for_registration(
        &self,
        registration_id: &str,
        requested_class: &str,
    ) -> Result<ConnectorClassRegistration, tonic::Status> {
        let (namespace, name) = keys::parse_connector_registration_id(registration_id)
            .map_err(|err| tonic::Status::invalid_argument(err.to_string()))?;
        if !requested_class.trim().is_empty() && requested_class != name {
            return Err(tonic::Status::failed_precondition(
                "connector_class conflicts with ConnectorClass registration",
            ));
        }
        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        let class = store
            .get(&namespace, "ConnectorClass", &name)
            .await
            .map_err(internal_error)?
            .ok_or_else(|| tonic::Status::not_found("ConnectorClass registration not found"))?;
        match class
            .status
            .as_ref()
            .and_then(|status| status.kind.as_ref())
        {
            Some(resources_proto::resource_status::Kind::ConnectorClass(status))
                if status.phase == "Ready" => {}
            _ => {
                return Err(tonic::Status::failed_precondition(
                    "ConnectorClass registration is not Ready",
                ));
            }
        }
        let spec = match class.spec.as_ref().and_then(|spec| spec.kind.as_ref()) {
            Some(resources_proto::resource_spec::Kind::ConnectorClass(spec)) => spec.clone(),
            _ => {
                return Err(tonic::Status::failed_precondition(
                    "ConnectorClass registration missing spec",
                ));
            }
        };
        Ok(ConnectorClassRegistration {
            namespace,
            name,
            spec,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockKvStore;
    use crate::test_support::RecordingPubSub;
    use ::control::control::keys;
    use ::control::control::ControlPlane;
    use ::control::control::KeyValueStore;
    use ::control::control::ProtoKeyValueStoreExt;
    use chrono::Utc;
    use prost::Message;
    use std::sync::Arc;

    const TEST_NAMESPACE: &str = "acme:test";
    const TEST_AGENT: &str = "connector-agent";
    const TEST_SESSION: &str = "session-1";

    async fn put_session(kv: &MockKvStore, status: &str, last_active: i64) {
        kv.set_msg(
            &keys::session(TEST_NAMESPACE, TEST_AGENT, TEST_SESSION),
            &data_proto::Session {
                id: TEST_SESSION.to_string(),
                agent: TEST_AGENT.to_string(),
                ns: TEST_NAMESPACE.to_string(),
                status: status.to_string(),
                created_at: 1,
                last_active,
                metadata: Default::default(),
                labels: Default::default(),
                skill_state: None,
                context_tokens: None,
            },
        )
        .await
        .unwrap();
    }

    fn test_route() -> data_proto::Route {
        data_proto::Route {
            connector_uid: "connector-uid".to_string(),
            connector: Some(data_proto::ResourceRef {
                namespace: TEST_NAMESPACE.to_string(),
                name: "room-one".to_string(),
            }),
            consumer: None,
        }
    }

    fn test_consumer() -> data_proto::SessionMessageConsumer {
        data_proto::SessionMessageConsumer {
            agent: Some(data_proto::ResourceRef {
                namespace: TEST_NAMESPACE.to_string(),
                name: TEST_AGENT.to_string(),
            }),
            session_id: TEST_SESSION.to_string(),
            continuity: "pinned".to_string(),
            reply_mode: String::new(),
        }
    }

    fn test_event(
        event_id: &str,
        text: &str,
        event_time_ms: i64,
    ) -> external_proto::ConnectorMessageEvent {
        external_proto::ConnectorMessageEvent {
            event_id: event_id.to_string(),
            event_kind: external_proto::ConnectorMessageEventKind::Created as i32,
            registration_id: format!("Namespace/{TEST_NAMESPACE}/ConnectorClass/mock-chat"),
            connector_class: "mock-chat".to_string(),
            external_conversation_id: "room-1".to_string(),
            external_message_id: format!("provider-{event_id}"),
            conversation_type: "room".to_string(),
            text: text.to_string(),
            event_time_ms,
            ..Default::default()
        }
    }

    fn control_plane(kv: Arc<MockKvStore>, pubsub: Arc<RecordingPubSub>) -> ControlPlane {
        ControlPlane::builder(kv, pubsub).build()
    }

    async fn queued_messages(kv: &MockKvStore) -> Vec<data_proto::SessionMessage> {
        let entries = kv
            .list_entries(
                &keys::session_queue_prefix(
                    TEST_NAMESPACE,
                    TEST_AGENT,
                    TEST_SESSION,
                    ::control::control::session_queue::STEER_QUEUE,
                ),
                None,
            )
            .await
            .unwrap();
        entries
            .into_iter()
            .map(|(_, bytes)| data_proto::SessionMessage::decode(bytes.as_slice()).unwrap())
            .collect()
    }

    #[test]
    fn connector_session_message_uses_chronological_uuid_message_id() {
        let message = connector_session_message(
            &external_proto::ConnectorMessageEvent {
                event_id: "provider-event-1".to_string(),
                event_kind: external_proto::ConnectorMessageEventKind::Created as i32,
                registration_id: "Namespace/acme/ConnectorClass/slack".to_string(),
                connector_class: "slack".to_string(),
                external_message_id: "provider-message-1".to_string(),
                text: "hello from connector".to_string(),
                event_time_ms: 1_700_000_000_000,
                ..Default::default()
            },
            HashMap::new(),
        )
        .expect("connector message should be valid");

        assert!(!message.id.starts_with("connector-"));
        assert!(!message.id.ends_with("-assistant"));
        assert_eq!(
            uuid::Uuid::parse_str(&message.id)
                .expect("message id should be UUID")
                .get_version_num(),
            7
        );
    }

    #[test]
    fn resolved_session_consumer_preserves_direct_route_policy() {
        for (continuity, configured_session_id) in
            [("", ""), ("reuse", ""), ("pinned", "configured-session")]
        {
            let consumer = data_proto::SessionMessageConsumer {
                agent: Some(data_proto::ResourceRef {
                    namespace: String::new(),
                    name: "support-agent".to_string(),
                }),
                session_id: configured_session_id.to_string(),
                continuity: continuity.to_string(),
                reply_mode: "thread".to_string(),
            };
            let resolved = resolved_session_consumer(
                &consumer,
                "Tenant/acme".to_string(),
                "support-agent".to_string(),
                "selected-session".to_string(),
            );
            let session = resolved.session.expect("resolved session consumer");
            assert_eq!(session.session_id, "selected-session");
            assert_eq!(session.continuity, continuity);
            assert_eq!(session.reply_mode, "thread");
            assert_eq!(
                session.agent.expect("resolved agent").namespace,
                "Tenant/acme"
            );
        }
    }

    #[tokio::test]
    async fn connector_session_dispatch_queues_when_the_pinned_session_is_busy() {
        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        put_session(&kv, "PROCESSING", Utc::now().timestamp_micros()).await;
        let cp = control_plane(kv.clone(), pubsub.clone());

        dispatch_to_session(
            &cp,
            &test_route(),
            &test_consumer(),
            &test_event("event-1", "queued connector message", 1_700_000_000_000),
        )
        .await
        .unwrap();

        let queued = queued_messages(&kv).await;
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].id, "");
        assert_eq!(queued[0].parts[0].content, "queued connector message");
        assert_eq!(
            queued[0].labels.get(LABEL_CONNECTOR_EVENT),
            Some(&"event-1".to_string())
        );
        assert!(pubsub
            .published
            .lock()
            .await
            .iter()
            .all(|(topic, _)| { topic != ::control::control::topics::SESSION_DISPATCH_TOPIC }));
    }

    #[tokio::test]
    async fn connector_session_dispatch_immediately_dispatches_when_the_session_is_idle() {
        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        put_session(&kv, "IDLE", 1).await;
        let cp = control_plane(kv.clone(), pubsub.clone());

        dispatch_to_session(
            &cp,
            &test_route(),
            &test_consumer(),
            &test_event("event-1", "immediate connector message", 1_700_000_000_000),
        )
        .await
        .unwrap();

        assert!(queued_messages(&kv).await.is_empty());
        let published = pubsub.published.lock().await;
        let dispatches = published
            .iter()
            .filter(|(topic, _)| topic == ::control::control::topics::SESSION_DISPATCH_TOPIC)
            .map(|(_, bytes)| {
                ::control::control::events::SessionDispatchEvent::decode(bytes.as_slice()).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(dispatches.len(), 1);
        let message = kv
            .get_msg::<data_proto::SessionMessage>(&keys::session_message(
                TEST_NAMESPACE,
                TEST_AGENT,
                TEST_SESSION,
                &dispatches[0].message_id,
            ))
            .await
            .unwrap()
            .expect("canonical session message should exist");
        assert_eq!(message.parts[0].content, "immediate connector message");
        assert_eq!(message.created_at, 1_700_000_000_000_000);
        assert_eq!(message.parts[0].created_at, 1_700_000_000_000_000);
        assert_eq!(
            message.labels.get(LABEL_CONNECTOR_EVENT),
            Some(&"event-1".to_string())
        );
    }

    #[tokio::test]
    async fn busy_connector_session_messages_are_queued_in_event_order() {
        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        put_session(&kv, "PROCESSING", Utc::now().timestamp_micros()).await;
        let cp = control_plane(kv.clone(), pubsub);

        for (event_id, text, event_time_ms) in [
            (
                "event-1",
                "first queued connector message",
                1_700_000_001_000,
            ),
            (
                "event-2",
                "second queued connector message",
                1_700_000_000_000,
            ),
        ] {
            dispatch_to_session(
                &cp,
                &test_route(),
                &test_consumer(),
                &test_event(event_id, text, event_time_ms),
            )
            .await
            .unwrap();
        }

        let queued = queued_messages(&kv).await;
        assert_eq!(
            queued
                .iter()
                .map(|message| message.parts[0].content.as_str())
                .collect::<Vec<_>>(),
            vec![
                "first queued connector message",
                "second queued connector message"
            ]
        );
    }
}

pub use ::control::connector_runtime::*;
