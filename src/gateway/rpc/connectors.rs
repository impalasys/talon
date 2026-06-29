// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{data_proto, proto, resources_proto, GrpcGatewayHandler};
use crate::control::resource_model::ChannelResourceExt;
use crate::control::scheduling;
use crate::control::{keys, ControlPlane, ProtoKeyValueStoreExt};
use crate::worker::controllers::connectors;
use anyhow::Context;
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Serialize)]
struct ConnectorDeliveryRequest {
    #[serde(rename = "deliveryId")]
    delivery_id: String,
    #[serde(rename = "registrationId")]
    registration_id: String,
    #[serde(rename = "connectorClass")]
    connector_class: String,
    namespace: String,
    #[serde(rename = "connectorName")]
    connector_name: String,
    #[serde(rename = "matchFields")]
    match_fields: HashMap<String, String>,
    #[serde(rename = "externalConversationId")]
    external_conversation_id: String,
    #[serde(rename = "externalThreadId", skip_serializing_if = "Option::is_none")]
    external_thread_id: Option<String>,
    #[serde(
        rename = "replyToExternalMessageId",
        skip_serializing_if = "Option::is_none"
    )]
    reply_to_external_message_id: Option<String>,
    text: String,
    attachments: Vec<serde_json::Value>,
    labels: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ConnectorDeliveryResponse {
    accepted: bool,
    disposition: String,
    error: String,
}

#[derive(Debug, Serialize)]
struct ConnectorActivityRequest {
    #[serde(rename = "activityId")]
    activity_id: String,
    #[serde(rename = "registrationId")]
    registration_id: String,
    #[serde(rename = "connectorClass")]
    connector_class: String,
    namespace: String,
    #[serde(rename = "connectorName")]
    connector_name: String,
    #[serde(rename = "matchFields")]
    match_fields: HashMap<String, String>,
    #[serde(rename = "externalConversationId")]
    external_conversation_id: String,
    #[serde(rename = "externalThreadId", skip_serializing_if = "Option::is_none")]
    external_thread_id: Option<String>,
    kind: String,
    phase: String,
    #[serde(rename = "statusText")]
    status_text: String,
    labels: HashMap<String, String>,
}

impl GrpcGatewayHandler {
    pub async fn handle_ingest_connector_message_event(
        &self,
        req: tonic::Request<proto::ConnectorMessageEvent>,
    ) -> Result<tonic::Response<proto::ConnectorMessageEventResponse>, tonic::Status> {
        let event = req.into_inner();
        if event.registration_id.trim().is_empty() {
            return Err(tonic::Status::invalid_argument(
                "registration_id is required",
            ));
        }
        if event.event_id.trim().is_empty() {
            return Err(tonic::Status::invalid_argument("event_id is required"));
        }
        if !event.event_kind.trim().is_empty() && event.event_kind != "message_created" {
            return Err(tonic::Status::invalid_argument(format!(
                "unsupported connector event_kind '{}'",
                event.event_kind
            )));
        }

        let (class_namespace, class_name, class_spec) = self
            .class_for_registration(&event.registration_id, &event.connector_class)
            .await?;

        let event_key = keys::connector_event(&event.registration_id, &event.event_id);
        if !self
            .gateway
            .kv
            .compare_and_swap(&event_key, None, b"reserved")
            .await
            .map_err(internal_error)?
        {
            return Ok(tonic::Response::new(proto::ConnectorMessageEventResponse {
                accepted: true,
                duplicate: true,
                disposition: "duplicate".to_string(),
                namespace: String::new(),
                connector_name: String::new(),
                target: None,
            }));
        }

        let Some(match_entry) = connectors::resolve_match(
            self.gateway.kv.as_ref(),
            &event.registration_id,
            &class_namespace,
            &class_name,
            &class_spec,
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
            return Ok(tonic::Response::new(proto::ConnectorMessageEventResponse {
                accepted: false,
                duplicate: false,
                disposition: "unmatched".to_string(),
                namespace: String::new(),
                connector_name: String::new(),
                target: None,
            }));
        };

        let target = match_entry
            .target
            .clone()
            .ok_or_else(|| tonic::Status::failed_precondition("Connector target is missing"))?;
        if let Err(err) =
            dispatch_connector_message(&self.gateway.control_plane(), &match_entry, &target, &event)
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

        Ok(tonic::Response::new(proto::ConnectorMessageEventResponse {
            accepted: true,
            duplicate: false,
            disposition: "dispatched".to_string(),
            namespace: match_entry.namespace,
            connector_name: match_entry.connector_name,
            target: Some(target),
        }))
    }

    pub async fn handle_report_connector_status(
        &self,
        req: tonic::Request<proto::ConnectorStatusEvent>,
    ) -> Result<tonic::Response<proto::ConnectorAckResponse>, tonic::Status> {
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
        Ok(tonic::Response::new(proto::ConnectorAckResponse {
            accepted: true,
            disposition: "accepted".to_string(),
        }))
    }

    async fn class_for_registration(
        &self,
        registration_id: &str,
        requested_class: &str,
    ) -> Result<(String, String, resources_proto::ConnectorClassSpec), tonic::Status> {
        let entry = self
            .gateway
            .kv
            .get_msg::<resources_proto::ConnectorRegistrationEntry>(&keys::connector_registration(
                registration_id,
            ))
            .await
            .map_err(internal_error)?
            .ok_or_else(|| tonic::Status::not_found("ConnectorClass registration not found"))?;

        if entry.registration_id != registration_id {
            return Err(tonic::Status::failed_precondition(
                "ConnectorClass registration index is inconsistent",
            ));
        }

        if entry.class_namespace.trim().is_empty() || entry.class_name.trim().is_empty() {
            return Err(tonic::Status::failed_precondition(
                "ConnectorClass registration index is incomplete",
            ));
        }

        if !requested_class.trim().is_empty() && requested_class != entry.class_name {
            return Err(tonic::Status::failed_precondition(
                "connector_class conflicts with ConnectorClass registration",
            ));
        }

        let class_spec = entry.class_spec.ok_or_else(|| {
            tonic::Status::failed_precondition("ConnectorClass registration missing spec snapshot")
        })?;
        Ok((entry.class_namespace, entry.class_name, class_spec))
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
    let registration = cp
        .kv
        .get_msg::<resources_proto::ConnectorRegistrationEntry>(&keys::connector_registration(
            registration_id,
        ))
        .await?
        .ok_or_else(|| anyhow::anyhow!("connector registration not found"))?;
    let class_spec = registration
        .class_spec
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("connector registration missing class spec"))?;
    let runtime = class_spec
        .runtime
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("connector class runtime is required"))?;
    if runtime.endpoint.trim().is_empty() {
        anyhow::bail!("connector class runtime endpoint is required");
    }
    let auth = class_spec
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

    let url = format!("{}/v1/deliveries", runtime.endpoint.trim_end_matches('/'));
    let response = connector_http_client()?
        .post(url)
        .bearer_auth(api_key)
        .json(&ConnectorDeliveryRequest {
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
        .json::<ConnectorDeliveryResponse>()
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
    let registration = cp
        .kv
        .get_msg::<resources_proto::ConnectorRegistrationEntry>(&keys::connector_registration(
            registration_id,
        ))
        .await?
        .ok_or_else(|| anyhow::anyhow!("connector registration not found"))?;
    let class_spec = registration
        .class_spec
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("connector registration missing class spec"))?;
    let runtime = class_spec
        .runtime
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("connector class runtime is required"))?;
    if runtime.endpoint.trim().is_empty() {
        anyhow::bail!("connector class runtime endpoint is required");
    }
    let auth = class_spec
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
        .post(format!(
            "{}/v1/activities",
            runtime.endpoint.trim_end_matches('/')
        ))
        .bearer_auth(api_key)
        .json(&ConnectorActivityRequest {
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
        .json::<ConnectorDeliveryResponse>()
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

async fn dispatch_connector_message(
    cp: &ControlPlane,
    entry: &resources_proto::ConnectorMatchEntry,
    target: &resources_proto::ConnectorTarget,
    event: &proto::ConnectorMessageEvent,
) -> Result<(), tonic::Status> {
    match target.destination.as_ref() {
        Some(resources_proto::connector_target::Destination::Session(session)) => {
            dispatch_to_session(cp, entry, session, event).await
        }
        Some(resources_proto::connector_target::Destination::Channel(channel)) => {
            dispatch_to_channel(cp, entry, channel, event).await
        }
        None => Err(tonic::Status::failed_precondition(
            "Connector target destination is missing",
        )),
    }
}

async fn dispatch_to_session(
    cp: &ControlPlane,
    entry: &resources_proto::ConnectorMatchEntry,
    target: &resources_proto::ConnectorSessionTarget,
    event: &proto::ConnectorMessageEvent,
) -> Result<(), tonic::Status> {
    if target.agent.trim().is_empty() {
        return Err(tonic::Status::failed_precondition(
            "Connector session target requires agent",
        ));
    }
    let mut labels = connector_labels(entry, event);
    labels.insert(LABEL_MESSAGE_SOURCE.to_string(), "connector".to_string());
    let session_id = connector_session_id(cp, entry, target, event, labels.clone()).await?;
    let message = connector_session_message(event, labels)?;
    scheduling::send_session_message(
        cp.kv.as_ref(),
        cp.pubsub.as_ref(),
        &entry.namespace,
        &target.agent,
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
    entry: &resources_proto::ConnectorMatchEntry,
    target: &resources_proto::ConnectorChannelTarget,
    event: &proto::ConnectorMessageEvent,
) -> Result<(), tonic::Status> {
    if target.channel.trim().is_empty() {
        return Err(tonic::Status::failed_precondition(
            "Connector channel target requires channel",
        ));
    }
    if target.agent.trim().is_empty() {
        return Err(tonic::Status::failed_precondition(
            "Connector channel target requires agent",
        ));
    }
    let channel = cp
        .kv
        .get_msg::<resources_proto::Channel>(&keys::channel(&entry.namespace, &target.channel))
        .await
        .map_err(internal_error)?
        .ok_or_else(|| tonic::Status::not_found("Connector target channel not found"))?;
    if channel.phase() == "closed" {
        return Err(tonic::Status::failed_precondition(
            "Connector target channel is closed",
        ));
    }

    let message = super::channels::persist_channel_message(
        cp,
        data_proto::ChannelMessage {
            id: connector_message_id(event),
            ns: entry.namespace.clone(),
            channel: target.channel.clone(),
            author_kind: connector_author_kind(event),
            author: connector_author(event),
            content: connector_channel_content(event),
            created_at: event_time_micros(event),
            source_agent: String::new(),
            source_session_id: String::new(),
            labels: connector_labels(entry, event),
        },
    )
    .await
    .map_err(internal_error)?;

    super::channels::route_connector_channel_message(
        cp,
        &message,
        &target.agent,
        &entry.connector_name,
        &target.reply_policy,
    )
    .await
    .map_err(map_dispatch_error)?;
    Ok(())
}

async fn connector_session_id(
    cp: &ControlPlane,
    entry: &resources_proto::ConnectorMatchEntry,
    target: &resources_proto::ConnectorSessionTarget,
    event: &proto::ConnectorMessageEvent,
    labels: HashMap<String, String>,
) -> Result<String, tonic::Status> {
    if target.continuity.eq_ignore_ascii_case("reuse") {
        let key = keys::connector_session(&connector_session_pointer_name(entry, target, event));
        if let Some(session_id) = existing_connector_session(cp, &key, entry, target).await? {
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
            return wait_for_connector_session(cp, &key, entry, target).await;
        }
        let session_id =
            scheduling::create_session_with_labels(cp, &entry.namespace, &target.agent, labels)
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
        scheduling::create_session_with_labels(cp, &entry.namespace, &target.agent, labels)
            .await
            .map_err(map_dispatch_error)
    }
}

fn connector_session_pointer_name(
    entry: &resources_proto::ConnectorMatchEntry,
    target: &resources_proto::ConnectorSessionTarget,
    event: &proto::ConnectorMessageEvent,
) -> String {
    let mut source = format!(
        "{}\x1f{}\x1f{}\x1f{}\x1f{}",
        entry.connector_uid,
        entry.namespace,
        target.agent,
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
    format!("{}-{}", entry.connector_uid, hex_sha256(source.as_bytes()))
}

async fn existing_connector_session(
    cp: &ControlPlane,
    key: &crate::control::keys::ResourceKey,
    entry: &resources_proto::ConnectorMatchEntry,
    target: &resources_proto::ConnectorSessionTarget,
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
        .get(&keys::session(&entry.namespace, &target.agent, &session_id))
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
    entry: &resources_proto::ConnectorMatchEntry,
    target: &resources_proto::ConnectorSessionTarget,
) -> Result<String, tonic::Status> {
    for _ in 0..40 {
        if let Some(session_id) = existing_connector_session(cp, key, entry, target).await? {
            return Ok(session_id);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(tonic::Status::aborted(
        "timed out waiting for connector session reservation",
    ))
}

fn connector_session_message(
    event: &proto::ConnectorMessageEvent,
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
        if attachment.object_key.trim().is_empty() {
            continue;
        }
        let mut metadata = HashMap::new();
        if !attachment.id.is_empty() {
            metadata.insert("connectorAttachmentId".to_string(), attachment.id.clone());
        }
        if !attachment.external_url.is_empty() {
            metadata.insert("externalUrl".to_string(), attachment.external_url.clone());
        }
        if attachment.expires_at != 0 {
            metadata.insert("expiresAt".to_string(), attachment.expires_at.to_string());
        }
        parts.push(data_proto::SessionMessagePart {
            id: format!("{:06}", parts.len()),
            part_type: connector_attachment_part_type(attachment) as i32,
            content: String::new(),
            name: attachment.filename.clone(),
            payload_json: String::new(),
            created_at: now,
            object: Some(data_proto::ObjectRef {
                key: attachment.object_key.clone(),
                media_type: attachment.media_type.clone(),
                size_bytes: attachment.size_bytes,
                sha256: String::new(),
                filename: attachment.filename.clone(),
                metadata,
            }),
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
    attachment: &proto::ConnectorAttachment,
) -> data_proto::SessionMessagePartType {
    let media_type = attachment.media_type.to_ascii_lowercase();
    let kind = attachment.kind.to_ascii_lowercase();
    if media_type.starts_with("image/") || kind == "image" {
        data_proto::SessionMessagePartType::Image
    } else if media_type.starts_with("audio/") || kind == "audio" {
        data_proto::SessionMessagePartType::Audio
    } else if media_type.starts_with("video/") || kind == "video" {
        data_proto::SessionMessagePartType::Video
    } else {
        data_proto::SessionMessagePartType::File
    }
}

fn connector_labels(
    entry: &resources_proto::ConnectorMatchEntry,
    event: &proto::ConnectorMessageEvent,
) -> HashMap<String, String> {
    let mut labels = HashMap::new();
    labels.insert(LABEL_CONNECTOR.to_string(), entry.connector_name.clone());
    labels.insert(LABEL_CONNECTOR_CLASS.to_string(), entry.class_name.clone());
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
    labels
}

fn insert_label(labels: &mut HashMap<String, String>, key: &str, value: &str) {
    if !value.trim().is_empty() {
        labels.insert(key.to_string(), value.to_string());
    }
}

fn connector_message_id(event: &proto::ConnectorMessageEvent) -> String {
    let source = if !event.event_id.trim().is_empty() {
        format!("{}\x1f{}", event.registration_id, event.event_id)
    } else if !event.external_message_id.trim().is_empty() {
        format!("{}\x1f{}", event.registration_id, event.external_message_id)
    } else {
        return uuid::Uuid::now_v7().to_string();
    };
    format!("connector-{}", hex_sha256(source.as_bytes()))
}

fn connector_author_kind(event: &proto::ConnectorMessageEvent) -> String {
    event
        .sender
        .as_ref()
        .map(|sender| sender.kind.trim())
        .filter(|kind| !kind.is_empty())
        .unwrap_or("user")
        .to_string()
}

fn connector_author(event: &proto::ConnectorMessageEvent) -> String {
    event
        .sender
        .as_ref()
        .and_then(|sender| {
            [
                sender.display_name.as_str(),
                sender.external_user_id.as_str(),
                sender.external_address.as_str(),
            ]
            .into_iter()
            .find(|value| !value.trim().is_empty())
        })
        .unwrap_or("external")
        .to_string()
}

fn connector_external_sender(event: &proto::ConnectorMessageEvent) -> String {
    event
        .sender
        .as_ref()
        .and_then(|sender| {
            [
                sender.external_user_id.as_str(),
                sender.external_address.as_str(),
                sender.display_name.as_str(),
            ]
            .into_iter()
            .find(|value| !value.trim().is_empty())
        })
        .unwrap_or_default()
        .to_string()
}

fn connector_text_projection(event: &proto::ConnectorMessageEvent) -> String {
    if !event.text.trim().is_empty() {
        return event.text.trim().to_string();
    }
    let attachment_names = event
        .attachments
        .iter()
        .map(|attachment| {
            if attachment.filename.trim().is_empty() {
                attachment.kind.as_str()
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

fn connector_channel_content(event: &proto::ConnectorMessageEvent) -> String {
    let mut content = connector_text_projection(event);
    let attachment_lines = event
        .attachments
        .iter()
        .filter(|attachment| !attachment.object_key.trim().is_empty())
        .enumerate()
        .map(|(index, attachment)| {
            format!(
                "[Attachment {}: filename=\"{}\" kind=\"{}\" media_type=\"{}\" object_key=\"{}\"]",
                index + 1,
                attachment.filename,
                attachment.kind,
                attachment.media_type,
                attachment.object_key
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

fn event_time_micros(event: &proto::ConnectorMessageEvent) -> i64 {
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
