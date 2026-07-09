// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{data_proto, external_proto, resources_proto, GrpcGatewayHandler};
use crate::control::resource_model::ChannelResourceExt;
use crate::control::resources::ResourceStore;
use crate::control::scheduling;
use crate::control::{keys, ControlPlane, ProtoKeyValueStoreExt};
use crate::worker::controllers::connectors;
use anyhow::Context;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const LABEL_MESSAGE_SOURCE: &str = "talon.impalasys.com/message-source";
const LABEL_CONNECTOR: &str = "talon.impalasys.com/connector";
const LABEL_CONNECTOR_CLASS: &str = "talon.impalasys.com/connector-class";
const LABEL_CONNECTOR_REGISTRATION: &str = "talon.impalasys.com/connector-registration";
const LABEL_CONNECTOR_EVENT: &str = "talon.impalasys.com/connector-event";
const LABEL_EXTERNAL_CONVERSATION: &str = "talon.impalasys.com/external-conversation";
const LABEL_EXTERNAL_THREAD: &str = "talon.impalasys.com/external-thread";
const LABEL_EXTERNAL_MESSAGE: &str = "talon.impalasys.com/external-message";
const LABEL_EXTERNAL_SENDER: &str = "talon.impalasys.com/external-sender";
const LABEL_CONVERSATION_TYPE: &str = "talon.impalasys.com/conversation-type";
const LABEL_CONNECTOR_MATCH_PREFIX: &str = "talon.impalasys.com/connector-match/";
const LABEL_CONNECTOR_REPLY_MODE: &str = "talon.impalasys.com/connector-reply-mode";
const LABEL_CONNECTOR_DELIVERY_STATUS: &str = "talon.impalasys.com/connector-delivery-status";
const LABEL_CONNECTOR_DELIVERY_ERROR: &str = "talon.impalasys.com/connector-delivery-error";
const LABEL_CHANNEL_TRIGGER: &str = "talon.impalasys.com/channel-trigger";
const LABEL_CHANNEL_REPLY_MODE: &str = "talon.impalasys.com/channel-reply-mode";
const LABEL_CHANNEL: &str = "talon.impalasys.com/channel";
const LABEL_CHANNEL_MESSAGE: &str = "talon.impalasys.com/channel-message";
const LABEL_CHANNEL_SUBSCRIPTION: &str = "talon.impalasys.com/channel-subscription";
const CONNECTOR_HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECTOR_SESSION_RESERVATION_PREFIX: &str = "reserved:";
const CONNECTOR_REPLY_MODE_HOLD_FOR_REVIEW: &str = "hold_for_review";
const CONNECTOR_DELIVERY_PENDING_REVIEW: &str = "pending_review";
const CONNECTOR_DELIVERY_REQUESTED: &str = "delivery_requested";
const CONNECTOR_DELIVERY_DELIVERED: &str = "delivered";
const CONNECTOR_DELIVERY_FAILED: &str = "failed";
const CONNECTOR_DELIVERY_SKIPPED: &str = "skipped";

struct ConnectorClassRegistration {
    namespace: String,
    name: String,
    spec: resources_proto::ConnectorClassSpec,
}

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
        if let Err(err) =
            dispatch_connector_message(&self.gateway.control_plane(), &route, &consumer, &event)
                .await
        {
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
                consumer: Some(consumer),
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

fn internal_error(err: impl std::fmt::Display) -> tonic::Status {
    tonic::Status::internal(err.to_string())
}

pub fn is_connector_silence_response(text: &str) -> bool {
    matches!(
        text.trim().to_ascii_uppercase().as_str(),
        "[SILENT]" | "SILENT" | "NO_REPLY" | "NO REPLY"
    )
}

pub(crate) fn session_message_final_response(message: &data_proto::SessionMessage) -> String {
    let final_parts_start = message
        .parts
        .iter()
        .rposition(|part| is_final_response_boundary(part.part_type));
    let final_parts = match final_parts_start {
        Some(index) => &message.parts[index + 1..],
        None => message.parts.as_slice(),
    };

    final_parts
        .iter()
        .filter_map(final_response_text)
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn final_response_text(part: &data_proto::SessionMessagePart) -> Option<&str> {
    match data_proto::SessionMessagePartType::try_from(part.part_type) {
        Ok(data_proto::SessionMessagePartType::Text)
        | Ok(data_proto::SessionMessagePartType::Error) => Some(part.content.trim()),
        _ => None,
    }
}

fn is_final_response_boundary(part_type: i32) -> bool {
    matches!(
        data_proto::SessionMessagePartType::try_from(part_type),
        Ok(data_proto::SessionMessagePartType::Reasoning)
            | Ok(data_proto::SessionMessagePartType::ToolCall)
            | Ok(data_proto::SessionMessagePartType::ToolResult)
            | Ok(data_proto::SessionMessagePartType::RequestPermission)
            | Ok(data_proto::SessionMessagePartType::PermissionResult)
    )
}

pub async fn maybe_deliver_connector_session_message(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    message_id: &str,
) -> anyhow::Result<()> {
    let session = cp
        .kv
        .get_msg::<data_proto::Session>(&keys::session(ns, agent, session_id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("session not found"))?;
    if !session.labels.contains_key(LABEL_CONNECTOR_REGISTRATION) {
        return Ok(());
    }
    if session
        .labels
        .get(LABEL_MESSAGE_SOURCE)
        .is_some_and(|source| source != "connector")
    {
        return Ok(());
    }
    let hold_for_review = connector_reply_mode_from_labels(&session.labels).as_deref()
        == Some(CONNECTOR_REPLY_MODE_HOLD_FOR_REVIEW)
        || connector_channel_reply_mode_from_labels(&session.labels).as_deref()
            == Some(CONNECTOR_REPLY_MODE_HOLD_FOR_REVIEW);
    if session.labels.contains_key(LABEL_CHANNEL_TRIGGER) && !hold_for_review {
        return Ok(());
    }

    let message = cp
        .kv
        .get_msg::<data_proto::SessionMessage>(&keys::session_message(
            ns, agent, session_id, message_id,
        ))
        .await?
        .ok_or_else(|| anyhow::anyhow!("assistant message not found"))?;
    if message.role != data_proto::MessageRole::RoleAssistant as i32 {
        return Ok(());
    }

    let status = message
        .labels
        .get(LABEL_CONNECTOR_DELIVERY_STATUS)
        .map(|value| value.as_str());
    match status {
        Some(CONNECTOR_DELIVERY_PENDING_REVIEW)
        | Some(CONNECTOR_DELIVERY_DELIVERED)
        | Some(CONNECTOR_DELIVERY_FAILED)
        | Some(CONNECTOR_DELIVERY_SKIPPED) => return Ok(()),
        Some(CONNECTOR_DELIVERY_REQUESTED) => {}
        Some(_) => return Ok(()),
        None if hold_for_review => {
            mutate_connector_session_message_labels(
                cp,
                ns,
                agent,
                session_id,
                message_id,
                |labels| {
                    copy_connector_delivery_context_labels(&session.labels, labels);
                    labels.insert(
                        LABEL_CONNECTOR_DELIVERY_STATUS.to_string(),
                        CONNECTOR_DELIVERY_PENDING_REVIEW.to_string(),
                    );
                    labels.remove(LABEL_CONNECTOR_DELIVERY_ERROR);
                },
            )
            .await?;
            return Ok(());
        }
        None => {}
    }

    let text = session_message_final_response(&message);
    if text.trim().is_empty() {
        if status == Some(CONNECTOR_DELIVERY_REQUESTED) {
            set_connector_delivery_status(
                cp,
                ns,
                agent,
                session_id,
                message_id,
                CONNECTOR_DELIVERY_FAILED,
                Some("connector reply text is empty"),
            )
            .await?;
        }
        tracing::info!(
            namespace = %ns,
            agent = %agent,
            session = %session_id,
            message_id = %message_id,
            "connector session reply has no text; skipping outbound delivery"
        );
        return Ok(());
    }
    if is_connector_silence_response(&text) {
        if status == Some(CONNECTOR_DELIVERY_REQUESTED) {
            set_connector_delivery_status(
                cp,
                ns,
                agent,
                session_id,
                message_id,
                CONNECTOR_DELIVERY_SKIPPED,
                None,
            )
            .await?;
        }
        tracing::info!(
            namespace = %ns,
            agent = %agent,
            session = %session_id,
            message_id = %message_id,
            "connector session reply suppressed by no-reply token"
        );
        return Ok(());
    }

    let mut delivery_context_labels = session.labels.clone();
    delivery_context_labels.extend(message.labels.clone());
    let delivery_result = deliver_connector_session_message_with_labels(
        cp,
        &session,
        &message,
        &text,
        &delivery_context_labels,
    )
    .await;
    if status == Some(CONNECTOR_DELIVERY_REQUESTED) {
        match delivery_result {
            Ok(()) => {
                set_connector_delivery_status(
                    cp,
                    ns,
                    agent,
                    session_id,
                    message_id,
                    CONNECTOR_DELIVERY_DELIVERED,
                    None,
                )
                .await?;
                Ok(())
            }
            Err(error) => {
                set_connector_delivery_status(
                    cp,
                    ns,
                    agent,
                    session_id,
                    message_id,
                    CONNECTOR_DELIVERY_FAILED,
                    Some(&error.to_string()),
                )
                .await?;
                Ok(())
            }
        }
    } else {
        delivery_result
    }
}

pub async fn deliver_connector_session_message(
    cp: &ControlPlane,
    session: &data_proto::Session,
    message: &data_proto::SessionMessage,
    text: &str,
) -> anyhow::Result<()> {
    deliver_connector_session_message_with_labels(cp, session, message, text, &session.labels).await
}

async fn deliver_connector_session_message_with_labels(
    cp: &ControlPlane,
    session: &data_proto::Session,
    message: &data_proto::SessionMessage,
    text: &str,
    labels: &HashMap<String, String>,
) -> anyhow::Result<()> {
    let registration_id = required_label(labels, LABEL_CONNECTOR_REGISTRATION)?;
    let connector_name = required_label(labels, LABEL_CONNECTOR)?;
    let connector_class = required_label(labels, LABEL_CONNECTOR_CLASS)?;
    let external_conversation_id = required_label(labels, LABEL_EXTERNAL_CONVERSATION)?;
    let (runtime_endpoint, api_key) =
        connector_runtime_endpoint_and_api_key(cp, registration_id, connector_class).await?;

    let mut match_fields = HashMap::new();
    for (key, value) in labels {
        if let Some(field) = key.strip_prefix(LABEL_CONNECTOR_MATCH_PREFIX) {
            match_fields.insert(field.to_string(), value.clone());
        }
    }

    let mut delivery_labels = HashMap::new();
    delivery_labels.insert("talon.session".to_string(), session.id.clone());
    delivery_labels.insert("talon.agent".to_string(), session.agent.clone());
    delivery_labels.insert("talon.sessionMessage".to_string(), message.id.clone());
    if let Some(source_event) = labels.get(LABEL_CONNECTOR_EVENT).cloned() {
        delivery_labels.insert("talon.connectorEvent".to_string(), source_event);
    }

    let url = format!("{}/v1/deliveries", runtime_endpoint);
    let response = connector_http_client()?
        .post(url)
        .bearer_auth(api_key)
        .json(&external_proto::ConnectorDeliveryRequest {
            delivery_id: message.id.clone(),
            registration_id: registration_id.to_string(),
            connector_class: connector_class.to_string(),
            namespace: session.ns.clone(),
            connector_name: connector_name.to_string(),
            match_fields,
            external_conversation_id: external_conversation_id.to_string(),
            external_thread_id: labels
                .get(LABEL_EXTERNAL_THREAD)
                .cloned()
                .or_else(|| labels.get(LABEL_EXTERNAL_MESSAGE).cloned()),
            reply_to_external_message_id: labels.get(LABEL_EXTERNAL_MESSAGE).cloned(),
            text: text.trim().to_string(),
            attachments: Vec::new(),
            labels: delivery_labels,
        })
        .send()
        .await
        .context("failed to submit connector delivery")?;
    let status = response.status();
    let body = response
        .json::<external_proto::ConnectorDeliveryResponse>()
        .await
        .context("failed to decode connector delivery response")?;
    if !status.is_success() || !body.accepted {
        anyhow::bail!(
            "connector delivery rejected: HTTP {status} disposition={} error={}",
            body.disposition,
            body.error
        );
    }
    Ok(())
}

fn connector_reply_mode_from_labels(labels: &HashMap<String, String>) -> Option<String> {
    labels
        .get(LABEL_CONNECTOR_REPLY_MODE)
        .map(|value| normalize_connector_reply_mode(value))
        .filter(|value| !value.is_empty())
}

fn connector_channel_reply_mode_from_labels(labels: &HashMap<String, String>) -> Option<String> {
    labels
        .get(LABEL_CHANNEL_REPLY_MODE)
        .map(|value| normalize_connector_reply_mode(value))
        .filter(|value| !value.is_empty())
}

fn normalize_connector_reply_mode(value: &str) -> String {
    match value.trim() {
        "review" | "hold_for_review" => CONNECTOR_REPLY_MODE_HOLD_FOR_REVIEW.to_string(),
        other => other.to_string(),
    }
}

fn copy_connector_delivery_context_labels(
    source: &HashMap<String, String>,
    destination: &mut HashMap<String, String>,
) {
    for key in [
        LABEL_MESSAGE_SOURCE,
        LABEL_CONNECTOR,
        LABEL_CONNECTOR_CLASS,
        LABEL_CONNECTOR_REGISTRATION,
        LABEL_CONNECTOR_EVENT,
        LABEL_EXTERNAL_CONVERSATION,
        LABEL_EXTERNAL_THREAD,
        LABEL_EXTERNAL_MESSAGE,
        LABEL_EXTERNAL_SENDER,
        LABEL_CONVERSATION_TYPE,
        LABEL_CONNECTOR_REPLY_MODE,
        LABEL_CHANNEL_TRIGGER,
        LABEL_CHANNEL_REPLY_MODE,
        LABEL_CHANNEL,
        LABEL_CHANNEL_MESSAGE,
        LABEL_CHANNEL_SUBSCRIPTION,
    ] {
        if let Some(value) = source.get(key).filter(|value| !value.trim().is_empty()) {
            destination.insert(key.to_string(), value.clone());
        }
    }
    for (key, value) in source {
        if key.starts_with(LABEL_CONNECTOR_MATCH_PREFIX) && !value.trim().is_empty() {
            destination.insert(key.clone(), value.clone());
        }
    }
}

async fn set_connector_delivery_status(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    message_id: &str,
    status: &str,
    error: Option<&str>,
) -> anyhow::Result<()> {
    mutate_connector_session_message_labels(cp, ns, agent, session_id, message_id, |labels| {
        labels.insert(
            LABEL_CONNECTOR_DELIVERY_STATUS.to_string(),
            status.to_string(),
        );
        if let Some(error) = error.filter(|value| !value.trim().is_empty()) {
            labels.insert(
                LABEL_CONNECTOR_DELIVERY_ERROR.to_string(),
                error.to_string(),
            );
        } else {
            labels.remove(LABEL_CONNECTOR_DELIVERY_ERROR);
        }
    })
    .await
}

async fn mutate_connector_session_message_labels(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    message_id: &str,
    mutate: impl FnOnce(&mut HashMap<String, String>),
) -> anyhow::Result<()> {
    let message_key = keys::session_message(ns, agent, session_id, message_id);
    let mut message = cp
        .kv
        .get_msg::<data_proto::SessionMessage>(&message_key)
        .await?
        .ok_or_else(|| anyhow::anyhow!("assistant message not found"))?;
    mutate(&mut message.labels);
    cp.kv.set_msg(&message_key, &message).await?;
    if let Err(error) = crate::control::search::publish_index_event(
        cp.pubsub.as_ref(),
        crate::control::events::IndexEvent {
            operation: crate::control::events::IndexOperation::Upsert as i32,
            key: message_key.canonical(),
            ..Default::default()
        },
    )
    .await
    {
        tracing::warn!(
            error = %error,
            namespace = %ns,
            agent = %agent,
            session_id = %session_id,
            message_id = %message_id,
            "failed to publish search index event for connector delivery label update"
        );
    }
    Ok(())
}

pub async fn send_connector_session_activity(
    cp: &ControlPlane,
    session: &data_proto::Session,
    activity_id: &str,
    phase: &str,
    status_text: &str,
) -> anyhow::Result<()> {
    let registration_id = required_label(&session.labels, LABEL_CONNECTOR_REGISTRATION)?;
    let connector_name = required_label(&session.labels, LABEL_CONNECTOR)?;
    let connector_class = required_label(&session.labels, LABEL_CONNECTOR_CLASS)?;
    let external_conversation_id = required_label(&session.labels, LABEL_EXTERNAL_CONVERSATION)?;
    let (runtime_endpoint, api_key) =
        connector_runtime_endpoint_and_api_key(cp, registration_id, connector_class).await?;

    let mut match_fields = HashMap::new();
    for (key, value) in &session.labels {
        if let Some(field) = key.strip_prefix(LABEL_CONNECTOR_MATCH_PREFIX) {
            match_fields.insert(field.to_string(), value.clone());
        }
    }

    let mut labels = HashMap::new();
    labels.insert("talon.session".to_string(), session.id.clone());
    labels.insert("talon.agent".to_string(), session.agent.clone());
    if let Some(source_event) = session.labels.get(LABEL_CONNECTOR_EVENT).cloned() {
        labels.insert("talon.connectorEvent".to_string(), source_event);
    }

    let response = connector_http_client()?
        .post(format!("{}/v1/activities", runtime_endpoint))
        .bearer_auth(api_key)
        .json(&external_proto::ConnectorActivityRequest {
            activity_id: activity_id.to_string(),
            registration_id: registration_id.to_string(),
            connector_class: connector_class.to_string(),
            namespace: session.ns.clone(),
            connector_name: connector_name.to_string(),
            match_fields,
            external_conversation_id: external_conversation_id.to_string(),
            external_thread_id: session
                .labels
                .get(LABEL_EXTERNAL_THREAD)
                .cloned()
                .or_else(|| session.labels.get(LABEL_EXTERNAL_MESSAGE).cloned()),
            kind: "typing".to_string(),
            phase: phase.to_string(),
            status_text: status_text.to_string(),
            labels,
        })
        .send()
        .await
        .context("failed to submit connector activity")?;
    let status = response.status();
    let body = response
        .json::<external_proto::ConnectorDeliveryResponse>()
        .await
        .context("failed to decode connector activity response")?;
    if !status.is_success() || !body.accepted {
        anyhow::bail!(
            "connector activity rejected: HTTP {status} disposition={} error={}",
            body.disposition,
            body.error
        );
    }
    Ok(())
}

fn required_label<'a>(labels: &'a HashMap<String, String>, name: &str) -> anyhow::Result<&'a str> {
    labels
        .get(name)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing connector label {name}"))
}

pub async fn connector_runtime_endpoint_and_api_key(
    cp: &ControlPlane,
    registration_id: &str,
    connector_class: &str,
) -> anyhow::Result<(String, String)> {
    let class = connector_class_registration(cp, registration_id, connector_class).await?;
    let runtime = class
        .spec
        .runtime
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("connector class runtime is required"))?;
    if runtime.endpoint.trim().is_empty() {
        anyhow::bail!("connector class runtime endpoint is required");
    }
    let auth = class
        .spec
        .auth
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("connector class auth is required"))?;
    if auth.kind != "apiKey" {
        anyhow::bail!("connector class auth.kind must be apiKey");
    }
    let api_key = auth
        .api_key
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("connector class auth.apiKey is required"))
        .and_then(resolve_connector_secret)?;
    Ok((runtime.endpoint.trim_end_matches('/').to_string(), api_key))
}

async fn connector_class_registration(
    cp: &ControlPlane,
    registration_id: &str,
    requested_class: &str,
) -> anyhow::Result<ConnectorClassRegistration> {
    let (namespace, name) = keys::parse_connector_registration_id(registration_id)?;
    if !requested_class.trim().is_empty() && requested_class != name {
        anyhow::bail!("connector_class conflicts with ConnectorClass registration");
    }
    let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    let class = store
        .get(&namespace, "ConnectorClass", &name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("ConnectorClass registration not found"))?;
    match class
        .status
        .as_ref()
        .and_then(|status| status.kind.as_ref())
    {
        Some(resources_proto::resource_status::Kind::ConnectorClass(status))
            if status.phase == "Ready" => {}
        _ => anyhow::bail!("ConnectorClass registration is not Ready"),
    }
    let spec = match class.spec.as_ref().and_then(|spec| spec.kind.as_ref()) {
        Some(resources_proto::resource_spec::Kind::ConnectorClass(spec)) => spec.clone(),
        _ => anyhow::bail!("ConnectorClass registration missing spec"),
    };
    Ok(ConnectorClassRegistration {
        namespace,
        name,
        spec,
    })
}

fn resolve_connector_secret(
    secret: &resources_proto::ConnectorSecretRef,
) -> anyhow::Result<String> {
    match (
        secret
            .plain
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        secret
            .env
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    ) {
        (Some(value), None) => Ok(value.to_string()),
        (None, Some(env)) => std::env::var(env)
            .with_context(|| format!("ConnectorClass auth.apiKey env '{env}' is not set")),
        (Some(_), Some(_)) => {
            anyhow::bail!("ConnectorClass auth.apiKey must set only one of plain or env")
        }
        (None, None) => anyhow::bail!("ConnectorClass auth.apiKey must set plain or env"),
    }
}

fn connector_http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(CONNECTOR_HTTP_TIMEOUT)
        .build()
        .context("failed to build connector HTTP client")
}

fn route_connector_ref(
    route: &data_proto::Route,
) -> Result<&data_proto::ResourceRef, tonic::Status> {
    let connector = route
        .connector
        .as_ref()
        .ok_or_else(|| tonic::Status::failed_precondition("Route connector is missing"))?;
    if connector.namespace.trim().is_empty() || connector.name.trim().is_empty() {
        return Err(tonic::Status::failed_precondition(
            "Route connector reference is incomplete",
        ));
    }
    Ok(connector)
}

fn consumer_ref_namespace(reference: &data_proto::ResourceRef, default_namespace: &str) -> String {
    reference
        .namespace
        .trim()
        .is_empty()
        .then(|| default_namespace.to_string())
        .unwrap_or_else(|| reference.namespace.clone())
}

fn consumer_ref_name<'a>(
    reference: &'a data_proto::ResourceRef,
    message: &'static str,
) -> Result<&'a str, tonic::Status> {
    reference
        .name
        .trim()
        .is_empty()
        .then_some(())
        .map(|_| Err(tonic::Status::failed_precondition(message)))
        .unwrap_or_else(|| Ok(reference.name.as_str()))
}

async fn dispatch_connector_message(
    cp: &ControlPlane,
    route: &data_proto::Route,
    consumer: &data_proto::MessageConsumer,
    event: &external_proto::ConnectorMessageEvent,
) -> Result<(), tonic::Status> {
    match (
        consumer.session.as_ref(),
        consumer.channel.as_ref(),
        consumer.workflow.as_ref(),
    ) {
        (Some(session), None, None) => dispatch_to_session(cp, route, session, event).await,
        (None, Some(channel), None) => dispatch_to_channel(cp, route, channel, event).await,
        (None, None, Some(workflow)) => dispatch_to_workflow(cp, route, workflow, event).await,
        (Some(_), _, _) | (_, Some(_), Some(_)) => Err(tonic::Status::failed_precondition(
            "MessageConsumer must set only one of session, channel, or workflow",
        )),
        (None, None, None) => Err(tonic::Status::failed_precondition(
            "MessageConsumer must set session, channel, or workflow",
        )),
    }
}

fn message_consumer_kind(consumer: &Option<data_proto::MessageConsumer>) -> &'static str {
    let Some(consumer) = consumer else {
        return "missing";
    };
    match (
        consumer.session.as_ref(),
        consumer.channel.as_ref(),
        consumer.workflow.as_ref(),
    ) {
        (Some(_), None, None) => "session",
        (None, Some(_), None) => "channel",
        (None, None, Some(_)) => "workflow",
        (None, None, None) => "empty",
        _ => "invalid",
    }
}

async fn dispatch_to_session(
    cp: &ControlPlane,
    route: &data_proto::Route,
    consumer: &data_proto::SessionMessageConsumer,
    event: &external_proto::ConnectorMessageEvent,
) -> Result<(), tonic::Status> {
    let started = Instant::now();
    let connector = route_connector_ref(route)?;
    let agent = consumer
        .agent
        .as_ref()
        .ok_or_else(|| tonic::Status::failed_precondition("Session consumer requires agent"))?;
    let agent_namespace = consumer_ref_namespace(agent, &connector.namespace);
    let agent_name = consumer_ref_name(agent, "Session consumer requires agent name")?;
    let mut labels = connector_labels(route, event)?;
    labels.insert(LABEL_MESSAGE_SOURCE.to_string(), "connector".to_string());
    if !consumer.reply_mode.trim().is_empty() {
        labels.insert(
            LABEL_CONNECTOR_REPLY_MODE.to_string(),
            normalize_connector_reply_mode(&consumer.reply_mode),
        );
    }
    tracing::info!(
        registration_id = %event.registration_id,
        connector_class = %event.connector_class,
        event_id = %event.event_id,
        route_uid = %route.connector_uid,
        connector_namespace = %connector.namespace,
        connector_name = %connector.name,
        agent_namespace = %agent_namespace,
        agent_name = %agent_name,
        configured_continuity = %consumer.continuity,
        configured_session_id = %consumer.session_id,
        configured_reply_mode = %consumer.reply_mode,
        elapsed_ms = started.elapsed().as_millis(),
        "connector session dispatch selecting session"
    );
    let session_id = connector_session_id(
        cp,
        route,
        consumer,
        &agent_namespace,
        agent_name,
        event,
        labels.clone(),
    )
    .await?;
    let message = connector_session_message(event, labels)?;
    let message_id = message.id.clone();
    tracing::info!(
        registration_id = %event.registration_id,
        connector_class = %event.connector_class,
        event_id = %event.event_id,
        route_uid = %route.connector_uid,
        agent_namespace = %agent_namespace,
        agent_name = %agent_name,
        session_id = %session_id,
        message_id = %message_id,
        elapsed_ms = started.elapsed().as_millis(),
        "connector message dispatching to session"
    );
    scheduling::send_session_message(
        cp.kv.as_ref(),
        cp.pubsub.as_ref(),
        &agent_namespace,
        agent_name,
        &session_id,
        message,
        chrono::Utc::now(),
    )
    .await
    .map_err(map_dispatch_error)?;
    tracing::info!(
        registration_id = %event.registration_id,
        connector_class = %event.connector_class,
        event_id = %event.event_id,
        route_uid = %route.connector_uid,
        agent_namespace = %agent_namespace,
        agent_name = %agent_name,
        session_id = %session_id,
        message_id = %message_id,
        elapsed_ms = started.elapsed().as_millis(),
        "connector message queued for session dispatch"
    );
    Ok(())
}

async fn dispatch_to_channel(
    cp: &ControlPlane,
    route: &data_proto::Route,
    consumer: &data_proto::ChannelMessageConsumer,
    event: &external_proto::ConnectorMessageEvent,
) -> Result<(), tonic::Status> {
    let connector = route_connector_ref(route)?;
    let channel_ref = consumer
        .channel
        .as_ref()
        .ok_or_else(|| tonic::Status::failed_precondition("Channel consumer requires channel"))?;
    let channel_namespace = consumer_ref_namespace(channel_ref, &connector.namespace);
    let channel_name = consumer_ref_name(channel_ref, "Channel consumer requires channel name")?;
    let agent_ref = consumer
        .agent
        .as_ref()
        .ok_or_else(|| tonic::Status::failed_precondition("Channel consumer requires agent"))?;
    let agent_namespace = consumer_ref_namespace(agent_ref, &connector.namespace);
    if agent_namespace != channel_namespace {
        return Err(tonic::Status::failed_precondition(
            "Channel consumer agent namespace must match channel namespace",
        ));
    }
    let agent_name = consumer_ref_name(agent_ref, "Channel consumer requires agent name")?;
    let channel = cp
        .kv
        .get_msg::<resources_proto::Channel>(&keys::channel(&channel_namespace, channel_name))
        .await
        .map_err(internal_error)?
        .ok_or_else(|| tonic::Status::not_found("Connector consumer channel not found"))?;
    if channel.phase() == "closed" {
        return Err(tonic::Status::failed_precondition(
            "Connector consumer channel is closed",
        ));
    }

    let message = super::channels::persist_channel_message(
        cp,
        data_proto::ChannelMessage {
            id: crate::control::uuid::channel_message_id(),
            ns: channel_namespace.clone(),
            channel: channel_name.to_string(),
            author_kind: connector_author_kind(event),
            author: connector_author(event),
            content: connector_channel_content(event),
            created_at: event_time_micros(event),
            source_agent: String::new(),
            source_session_id: String::new(),
            labels: connector_labels(route, event)?,
        },
    )
    .await
    .map_err(internal_error)?;

    super::channels::route_connector_channel_message(
        cp,
        &message,
        agent_name,
        &connector.name,
        &consumer.reply_policy,
    )
    .await
    .map_err(map_dispatch_error)?;
    Ok(())
}

async fn dispatch_to_workflow(
    cp: &ControlPlane,
    route: &data_proto::Route,
    consumer: &data_proto::WorkflowMessageConsumer,
    event: &external_proto::ConnectorMessageEvent,
) -> Result<(), tonic::Status> {
    let connector = route_connector_ref(route)?;
    let workflow_namespace = if consumer.namespace.trim().is_empty() {
        connector.namespace.clone()
    } else {
        consumer.namespace.clone()
    };
    let workflow_name = consumer.name.trim();
    if workflow_name.is_empty() {
        return Err(tonic::Status::failed_precondition(
            "Workflow consumer requires workflow name",
        ));
    }
    let workflow = cp
        .kv
        .get_msg::<resources_proto::Workflow>(&keys::workflow(&workflow_namespace, workflow_name))
        .await
        .map_err(internal_error)?
        .ok_or_else(|| tonic::Status::not_found("Connector consumer workflow not found"))?;

    let mut labels = connector_labels(route, event)?;
    labels.insert(
        LABEL_MESSAGE_SOURCE.to_string(),
        "connector.workflow".to_string(),
    );
    if !consumer.reply_mode.trim().is_empty() {
        labels.insert(
            crate::harness::connector::LABEL_CONNECTOR_REPLY_MODE.to_string(),
            consumer.reply_mode.trim().to_string(),
        );
    }

    let input = connector_workflow_input(connector, event).map_err(internal_error)?;
    crate::worker::workflows::create_run(cp, &workflow, input, labels)
        .await
        .map_err(map_dispatch_error)?;
    Ok(())
}

async fn connector_session_id(
    cp: &ControlPlane,
    route: &data_proto::Route,
    consumer: &data_proto::SessionMessageConsumer,
    agent_namespace: &str,
    agent_name: &str,
    event: &external_proto::ConnectorMessageEvent,
    labels: HashMap<String, String>,
) -> Result<String, tonic::Status> {
    let started = Instant::now();
    if !consumer.session_id.trim().is_empty() {
        if !consumer.continuity.trim().is_empty()
            && !consumer.continuity.eq_ignore_ascii_case("pinned")
        {
            return Err(tonic::Status::failed_precondition(
                "Session consumer session_id requires pinned continuity",
            ));
        }
        ensure_connector_session_exists(cp, agent_namespace, agent_name, &consumer.session_id)
            .await?;
        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            route_uid = %route.connector_uid,
            agent_namespace = %agent_namespace,
            agent_name = %agent_name,
            session_mode = "pinned",
            session_outcome = "existing",
            session_id = %consumer.session_id,
            elapsed_ms = started.elapsed().as_millis(),
            "connector session selected"
        );
        Ok(consumer.session_id.clone())
    } else if consumer.continuity.eq_ignore_ascii_case("pinned") {
        Err(tonic::Status::failed_precondition(
            "Session consumer pinned continuity requires session_id",
        ))
    } else if consumer.continuity.eq_ignore_ascii_case("reuse") {
        let (class_namespace, class_name) =
            keys::parse_connector_registration_id(&event.registration_id)
                .map_err(|err| tonic::Status::invalid_argument(err.to_string()))?;
        let key = keys::connector_session(
            &class_namespace,
            &class_name,
            &connector_session_pointer_name(route, agent_namespace, agent_name, event)?,
        );
        if let Some(session_id) =
            existing_connector_session(cp, &key, agent_namespace, agent_name).await?
        {
            tracing::info!(
                registration_id = %event.registration_id,
                connector_class = %event.connector_class,
                event_id = %event.event_id,
                route_uid = %route.connector_uid,
                agent_namespace = %agent_namespace,
                agent_name = %agent_name,
                session_mode = "reuse",
                session_outcome = "existing",
                session_id = %session_id,
                session_pointer_key = %key,
                elapsed_ms = started.elapsed().as_millis(),
                "connector session selected"
            );
            return Ok(session_id);
        }
        let reservation = format!(
            "{CONNECTOR_SESSION_RESERVATION_PREFIX}{}",
            crate::control::uuid::session_id()
        );
        if !cp
            .kv
            .compare_and_swap(&key, None, reservation.as_bytes())
            .await
            .map_err(internal_error)?
        {
            tracing::info!(
                registration_id = %event.registration_id,
                connector_class = %event.connector_class,
                event_id = %event.event_id,
                route_uid = %route.connector_uid,
                agent_namespace = %agent_namespace,
                agent_name = %agent_name,
                session_mode = "reuse",
                session_outcome = "waiting_for_reservation",
                session_pointer_key = %key,
                elapsed_ms = started.elapsed().as_millis(),
                "connector session reservation already exists"
            );
            return wait_for_connector_session(
                cp,
                &key,
                agent_namespace,
                agent_name,
                &event.registration_id,
                &event.connector_class,
                &event.event_id,
                &route.connector_uid,
            )
            .await;
        }
        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            route_uid = %route.connector_uid,
            agent_namespace = %agent_namespace,
            agent_name = %agent_name,
            session_mode = "reuse",
            session_outcome = "reserved",
            session_pointer_key = %key,
            elapsed_ms = started.elapsed().as_millis(),
            "connector session reservation created"
        );
        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            route_uid = %route.connector_uid,
            agent_namespace = %agent_namespace,
            agent_name = %agent_name,
            session_mode = "reuse",
            session_pointer_key = %key,
            elapsed_ms = started.elapsed().as_millis(),
            "connector session creating"
        );
        let session_id =
            scheduling::create_session_with_labels(cp, agent_namespace, agent_name, labels)
                .await
                .map_err(map_dispatch_error)?;
        if !cp
            .kv
            .compare_and_swap(&key, Some(reservation.as_bytes()), session_id.as_bytes())
            .await
            .map_err(internal_error)?
        {
            cp.kv.delete(&key).await.map_err(internal_error)?;
            return Err(tonic::Status::aborted(
                "connector session reservation was lost",
            ));
        }
        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            route_uid = %route.connector_uid,
            agent_namespace = %agent_namespace,
            agent_name = %agent_name,
            session_mode = "reuse",
            session_outcome = "created",
            session_id = %session_id,
            session_pointer_key = %key,
            elapsed_ms = started.elapsed().as_millis(),
            "connector session selected"
        );
        Ok(session_id)
    } else {
        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            route_uid = %route.connector_uid,
            agent_namespace = %agent_namespace,
            agent_name = %agent_name,
            session_mode = "new",
            elapsed_ms = started.elapsed().as_millis(),
            "connector session creating"
        );
        let session_id =
            scheduling::create_session_with_labels(cp, agent_namespace, agent_name, labels)
                .await
                .map_err(map_dispatch_error)?;
        tracing::info!(
            registration_id = %event.registration_id,
            connector_class = %event.connector_class,
            event_id = %event.event_id,
            route_uid = %route.connector_uid,
            agent_namespace = %agent_namespace,
            agent_name = %agent_name,
            session_mode = "new",
            session_outcome = "created",
            session_id = %session_id,
            elapsed_ms = started.elapsed().as_millis(),
            "connector session selected"
        );
        Ok(session_id)
    }
}

fn connector_workflow_input(
    connector: &data_proto::ResourceRef,
    event: &external_proto::ConnectorMessageEvent,
) -> anyhow::Result<String> {
    let value = json!({
        "connector": {
            "namespace": &connector.namespace,
            "name": &connector.name,
            "class": &event.connector_class,
            "registrationId": &event.registration_id,
            "matchFields": &event.match_fields,
        },
        "message": {
            "text": &event.text,
            "attachments": &event.attachments,
            "sender": &event.sender,
            "externalConversationId": &event.external_conversation_id,
            "externalThreadId": &event.external_thread_id,
            "externalMessageId": &event.external_message_id,
            "conversationType": &event.conversation_type,
            "eventTimeMs": event.event_time_ms,
        },
        "event": {
            "id": &event.event_id,
            "kind": event.event_kind,
            "labels": &event.labels,
        }
    });
    Ok(serde_json::to_string(&value)?)
}

async fn ensure_connector_session_exists(
    cp: &ControlPlane,
    agent_namespace: &str,
    agent_name: &str,
    session_id: &str,
) -> Result<(), tonic::Status> {
    if cp
        .kv
        .get(&keys::session(agent_namespace, agent_name, session_id))
        .await
        .map_err(internal_error)?
        .is_none()
    {
        return Err(tonic::Status::not_found(
            "Connector pinned Session consumer session not found",
        ));
    }
    Ok(())
}

fn connector_session_pointer_name(
    route: &data_proto::Route,
    agent_namespace: &str,
    agent_name: &str,
    event: &external_proto::ConnectorMessageEvent,
) -> Result<String, tonic::Status> {
    let connector = route_connector_ref(route)?;
    let mut source = format!(
        "{}\x1f{}\x1f{}\x1f{}\x1f{}",
        route.connector_uid,
        agent_namespace,
        agent_name,
        event.external_conversation_id,
        event.external_thread_id.as_deref().unwrap_or_default()
    );
    if event.external_conversation_id.is_empty()
        && event
            .external_thread_id
            .as_deref()
            .unwrap_or_default()
            .is_empty()
    {
        let mut fields = event.match_fields.iter().collect::<Vec<_>>();
        fields.sort_by(|left, right| left.0.cmp(right.0));
        for (key, value) in fields {
            source.push('\x1f');
            source.push_str(key);
            source.push('=');
            source.push_str(value);
        }
    }
    Ok(format!(
        "{}-{}-{}",
        route.connector_uid,
        connector.name,
        hex_sha256(source.as_bytes())
    ))
}

async fn existing_connector_session(
    cp: &ControlPlane,
    key: &crate::control::keys::ResourceKey,
    agent_namespace: &str,
    agent_name: &str,
) -> Result<Option<String>, tonic::Status> {
    let Some(bytes) = cp.kv.get(key).await.map_err(internal_error)? else {
        return Ok(None);
    };
    let Ok(session_id) = String::from_utf8(bytes) else {
        cp.kv.delete(key).await.map_err(internal_error)?;
        return Ok(None);
    };
    if session_id.starts_with(CONNECTOR_SESSION_RESERVATION_PREFIX) {
        return Ok(None);
    }
    if cp
        .kv
        .get(&keys::session(agent_namespace, agent_name, &session_id))
        .await
        .map_err(internal_error)?
        .is_some()
    {
        return Ok(Some(session_id));
    }
    cp.kv.delete(key).await.map_err(internal_error)?;
    Ok(None)
}

async fn wait_for_connector_session(
    cp: &ControlPlane,
    key: &crate::control::keys::ResourceKey,
    agent_namespace: &str,
    agent_name: &str,
    registration_id: &str,
    connector_class: &str,
    event_id: &str,
    route_uid: &str,
) -> Result<String, tonic::Status> {
    let started = Instant::now();
    for attempt in 0..40 {
        if let Some(session_id) =
            existing_connector_session(cp, key, agent_namespace, agent_name).await?
        {
            tracing::info!(
                registration_id = %registration_id,
                connector_class = %connector_class,
                event_id = %event_id,
                route_uid = %route_uid,
                agent_namespace = %agent_namespace,
                agent_name = %agent_name,
                session_id = %session_id,
                session_pointer_key = %key,
                wait_attempt = attempt,
                elapsed_ms = started.elapsed().as_millis(),
                "connector session reservation resolved"
            );
            return Ok(session_id);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tracing::warn!(
        registration_id = %registration_id,
        connector_class = %connector_class,
        event_id = %event_id,
        route_uid = %route_uid,
        agent_namespace = %agent_namespace,
        agent_name = %agent_name,
        session_pointer_key = %key,
        elapsed_ms = started.elapsed().as_millis(),
        "connector session reservation wait timed out"
    );
    Err(tonic::Status::aborted(
        "timed out waiting for connector session reservation",
    ))
}

fn connector_session_message(
    event: &external_proto::ConnectorMessageEvent,
    labels: HashMap<String, String>,
) -> Result<data_proto::SessionMessage, tonic::Status> {
    let mut parts = Vec::new();
    let now = event_time_micros(event);
    if !event.text.trim().is_empty() {
        parts.push(data_proto::SessionMessagePart {
            id: "000000".to_string(),
            part_type: data_proto::SessionMessagePartType::Text as i32,
            content: event.text.trim().to_string(),
            name: String::new(),
            payload_json: String::new(),
            created_at: now,
            object: None,
        });
    }
    for attachment in &event.attachments {
        if attachment.key.trim().is_empty() {
            continue;
        }
        parts.push(data_proto::SessionMessagePart {
            id: format!("{:06}", parts.len()),
            part_type: connector_attachment_part_type(attachment) as i32,
            content: String::new(),
            name: attachment.filename.clone(),
            payload_json: String::new(),
            created_at: now,
            object: Some(attachment.clone()),
        });
    }
    if parts.is_empty() {
        return Err(tonic::Status::invalid_argument(
            "connector message event must include text or attachments",
        ));
    }
    Ok(data_proto::SessionMessage {
        id: crate::control::uuid::session_message_id(),
        role: data_proto::MessageRole::RoleUser as i32,
        created_at: now,
        labels,
        parts,
    })
}

fn connector_attachment_part_type(
    attachment: &data_proto::ObjectRef,
) -> data_proto::SessionMessagePartType {
    let media_type = attachment.media_type.to_ascii_lowercase();
    if media_type.starts_with("image/") {
        data_proto::SessionMessagePartType::Image
    } else if media_type.starts_with("audio/") {
        data_proto::SessionMessagePartType::Audio
    } else if media_type.starts_with("video/") {
        data_proto::SessionMessagePartType::Video
    } else {
        data_proto::SessionMessagePartType::File
    }
}

fn connector_labels(
    route: &data_proto::Route,
    event: &external_proto::ConnectorMessageEvent,
) -> Result<HashMap<String, String>, tonic::Status> {
    let connector = route_connector_ref(route)?;
    let (_, class_name) = keys::parse_connector_registration_id(&event.registration_id)
        .map_err(|err| tonic::Status::invalid_argument(err.to_string()))?;
    let mut labels = HashMap::new();
    labels.insert(LABEL_CONNECTOR.to_string(), connector.name.clone());
    labels.insert(LABEL_CONNECTOR_CLASS.to_string(), class_name);
    labels.insert(
        LABEL_CONNECTOR_REGISTRATION.to_string(),
        event.registration_id.clone(),
    );
    labels.insert(LABEL_CONNECTOR_EVENT.to_string(), event.event_id.clone());
    insert_label(
        &mut labels,
        LABEL_EXTERNAL_CONVERSATION,
        &event.external_conversation_id,
    );
    if let Some(thread_id) = event.external_thread_id.as_deref() {
        insert_label(&mut labels, LABEL_EXTERNAL_THREAD, thread_id);
    }
    insert_label(
        &mut labels,
        LABEL_EXTERNAL_MESSAGE,
        &event.external_message_id,
    );
    insert_label(
        &mut labels,
        LABEL_EXTERNAL_SENDER,
        &connector_external_sender(event),
    );
    insert_label(
        &mut labels,
        LABEL_CONVERSATION_TYPE,
        &event.conversation_type,
    );
    for (key, value) in &event.match_fields {
        if !key.trim().is_empty() && !value.trim().is_empty() {
            labels.insert(
                format!("{LABEL_CONNECTOR_MATCH_PREFIX}{key}"),
                value.clone(),
            );
        }
    }
    Ok(labels)
}

fn insert_label(labels: &mut HashMap<String, String>, key: &str, value: &str) {
    if !value.trim().is_empty() {
        labels.insert(key.to_string(), value.to_string());
    }
}

fn connector_author_kind(event: &external_proto::ConnectorMessageEvent) -> String {
    event
        .sender
        .as_ref()
        .map(|sender| sender.kind.trim())
        .filter(|kind| !kind.is_empty())
        .unwrap_or("user")
        .to_string()
}

fn connector_author(event: &external_proto::ConnectorMessageEvent) -> String {
    event
        .sender
        .as_ref()
        .and_then(|sender| {
            [
                sender.display_name.as_str(),
                sender.external_id.as_str(),
                sender.address.as_str(),
            ]
            .into_iter()
            .find(|value| !value.trim().is_empty())
        })
        .unwrap_or("external")
        .to_string()
}

fn connector_external_sender(event: &external_proto::ConnectorMessageEvent) -> String {
    event
        .sender
        .as_ref()
        .and_then(|sender| {
            [
                sender.external_id.as_str(),
                sender.address.as_str(),
                sender.display_name.as_str(),
            ]
            .into_iter()
            .find(|value| !value.trim().is_empty())
        })
        .unwrap_or_default()
        .to_string()
}

fn connector_text_projection(event: &external_proto::ConnectorMessageEvent) -> String {
    if !event.text.trim().is_empty() {
        return event.text.trim().to_string();
    }
    let attachment_names = event
        .attachments
        .iter()
        .map(|attachment| {
            if attachment.filename.trim().is_empty() {
                attachment.media_type.as_str()
            } else {
                attachment.filename.as_str()
            }
        })
        .filter(|name| !name.trim().is_empty())
        .collect::<Vec<_>>();
    if attachment_names.is_empty() {
        "[non-text connector message]".to_string()
    } else {
        format!("[Attachment: {}]", attachment_names.join(", "))
    }
}

fn connector_channel_content(event: &external_proto::ConnectorMessageEvent) -> String {
    let mut content = connector_text_projection(event);
    let attachment_lines = event
        .attachments
        .iter()
        .filter(|attachment| !attachment.key.trim().is_empty())
        .enumerate()
        .map(|(index, attachment)| {
            format!(
                "[Attachment {}: filename=\"{}\" media_type=\"{}\" key=\"{}\"]",
                index + 1,
                attachment.filename,
                attachment.media_type,
                attachment.key
            )
        })
        .collect::<Vec<_>>();
    if !attachment_lines.is_empty() {
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&attachment_lines.join("\n"));
    }
    content
}

fn event_time_micros(event: &external_proto::ConnectorMessageEvent) -> i64 {
    if event.event_time_ms > 0 {
        event.event_time_ms.saturating_mul(1_000)
    } else {
        chrono::Utc::now().timestamp_micros()
    }
}

fn hex_sha256(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn map_dispatch_error(err: anyhow::Error) -> tonic::Status {
    if err
        .downcast_ref::<scheduling::SessionCurrentlyProcessingError>()
        .is_some()
    {
        tonic::Status::resource_exhausted("Session is currently generating a response.")
    } else if err
        .downcast_ref::<scheduling::EmptyMessageError>()
        .is_some()
    {
        tonic::Status::invalid_argument("message content is required")
    } else if err
        .downcast_ref::<scheduling::SessionNotFoundError>()
        .is_some()
    {
        tonic::Status::not_found("Session not found")
    } else {
        tonic::Status::internal(format!("failed to dispatch connector message: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connector_session_message_uses_chronological_uuid_message_id() {
        let message = connector_session_message(
            &external_proto::ConnectorMessageEvent {
                event_id: "provider-event-1".to_string(),
                event_kind: external_proto::ConnectorMessageEventKind::Created as i32,
                registration_id: "Namespace/conic/ConnectorClass/slack".to_string(),
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
}
