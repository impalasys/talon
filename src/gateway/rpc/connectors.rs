// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{data_proto, external_proto, resources_proto, GrpcGatewayHandler};
use crate::control::resource_model::ChannelResourceExt;
use crate::control::resources::ResourceStore;
use crate::control::scheduling;
use crate::control::{keys, ControlPlane, ProtoKeyValueStoreExt};
use crate::worker::controllers::connectors;
use anyhow::Context;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;

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
const CONNECTOR_HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECTOR_SESSION_RESERVATION_PREFIX: &str = "reserved:";

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
        let event = req.into_inner();
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

        let class = self
            .class_for_registration(&event.registration_id, &event.connector_class)
            .await?;

        let event_key = keys::connector_event(&class.namespace, &class.name, &event.event_id);
        if !self
            .gateway
            .kv
            .compare_and_swap(&event_key, None, b"reserved")
            .await
            .map_err(internal_error)?
        {
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

        let consumer = route
            .consumer
            .clone()
            .ok_or_else(|| tonic::Status::failed_precondition("Route consumer is missing"))?;
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
            return Err(err);
        }

        self.gateway
            .kv
            .set(&event_key, b"dispatched")
            .await
            .map_err(internal_error)?;

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

pub async fn deliver_connector_session_message(
    cp: &ControlPlane,
    session: &data_proto::Session,
    message: &data_proto::SessionMessage,
    text: &str,
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

    let mut delivery_labels = HashMap::new();
    delivery_labels.insert("talon.session".to_string(), session.id.clone());
    delivery_labels.insert("talon.agent".to_string(), session.agent.clone());
    delivery_labels.insert("talon.sessionMessage".to_string(), message.id.clone());
    if let Some(source_event) = session.labels.get(LABEL_CONNECTOR_EVENT).cloned() {
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
            external_thread_id: session
                .labels
                .get(LABEL_EXTERNAL_THREAD)
                .cloned()
                .or_else(|| session.labels.get(LABEL_EXTERNAL_MESSAGE).cloned()),
            reply_to_external_message_id: session.labels.get(LABEL_EXTERNAL_MESSAGE).cloned(),
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
    secret: &crate::gateway::rpc::generated::config::Secret,
) -> anyhow::Result<String> {
    use crate::gateway::rpc::generated::config::secret;

    match secret.source.as_ref() {
        Some(secret::Source::Plain(value)) => Ok(value.clone()),
        Some(secret::Source::Ref(_)) => {
            anyhow::bail!("ConnectorClass auth.apiKey must be a plain value; secret refs are not allowed on namespace-scoped connector resources")
        }
        None => anyhow::bail!("secret source missing"),
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
    match consumer.consumer.as_ref() {
        Some(data_proto::message_consumer::Consumer::Session(session)) => {
            dispatch_to_session(cp, route, session, event).await
        }
        Some(data_proto::message_consumer::Consumer::Channel(channel)) => {
            dispatch_to_channel(cp, route, channel, event).await
        }
        None => Err(tonic::Status::failed_precondition(
            "MessageConsumer is missing",
        )),
    }
}

async fn dispatch_to_session(
    cp: &ControlPlane,
    route: &data_proto::Route,
    consumer: &data_proto::SessionMessageConsumer,
    event: &external_proto::ConnectorMessageEvent,
) -> Result<(), tonic::Status> {
    let connector = route_connector_ref(route)?;
    let agent = consumer
        .agent
        .as_ref()
        .ok_or_else(|| tonic::Status::failed_precondition("Session consumer requires agent"))?;
    let agent_namespace = consumer_ref_namespace(agent, &connector.namespace);
    let agent_name = consumer_ref_name(agent, "Session consumer requires agent name")?;
    let mut labels = connector_labels(route, event)?;
    labels.insert(LABEL_MESSAGE_SOURCE.to_string(), "connector".to_string());
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
            id: connector_message_id(event),
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

async fn connector_session_id(
    cp: &ControlPlane,
    route: &data_proto::Route,
    consumer: &data_proto::SessionMessageConsumer,
    agent_namespace: &str,
    agent_name: &str,
    event: &external_proto::ConnectorMessageEvent,
    labels: HashMap<String, String>,
) -> Result<String, tonic::Status> {
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
            return Ok(session_id);
        }
        let reservation = format!(
            "{CONNECTOR_SESSION_RESERVATION_PREFIX}{}",
            uuid::Uuid::now_v7()
        );
        if !cp
            .kv
            .compare_and_swap(&key, None, reservation.as_bytes())
            .await
            .map_err(internal_error)?
        {
            return wait_for_connector_session(cp, &key, agent_namespace, agent_name).await;
        }
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
        Ok(session_id)
    } else {
        scheduling::create_session_with_labels(cp, agent_namespace, agent_name, labels)
            .await
            .map_err(map_dispatch_error)
    }
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
) -> Result<String, tonic::Status> {
    for _ in 0..40 {
        if let Some(session_id) =
            existing_connector_session(cp, key, agent_namespace, agent_name).await?
        {
            return Ok(session_id);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
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
        id: connector_message_id(event),
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

fn connector_message_id(event: &external_proto::ConnectorMessageEvent) -> String {
    let source = if !event.event_id.trim().is_empty() {
        format!("{}\x1f{}", event.registration_id, event.event_id)
    } else if !event.external_message_id.trim().is_empty() {
        format!("{}\x1f{}", event.registration_id, event.external_message_id)
    } else {
        return uuid::Uuid::now_v7().to_string();
    };
    format!("connector-{}", hex_sha256(source.as_bytes()))
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
