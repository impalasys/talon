// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{data_proto, proto, resources_proto, GrpcGatewayHandler};
use crate::control::resource_model::ChannelResourceExt;
use crate::control::resources::ResourceStore;
use crate::control::scheduling;
use crate::control::{keys, ns, ControlPlane, ProtoKeyValueStoreExt};
use crate::worker::controllers::connectors;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

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

        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        let (class_name, class_spec) = self
            .class_for_registration(&store, &event.registration_id, &event.connector_class)
            .await?;

        let event_key = keys::connector_event(&event.registration_id, &event.event_id);
        if self
            .gateway
            .kv
            .get(&event_key)
            .await
            .map_err(internal_error)?
            .is_some()
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
        dispatch_connector_message(&self.gateway.control_plane(), &match_entry, &target, &event)
            .await?;

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
        store: &ResourceStore,
        registration_id: &str,
        requested_class: &str,
    ) -> Result<(String, resources_proto::ConnectorClassSpec), tonic::Status> {
        for class in store
            .list(ns::TALON_SYSTEM, Some("ConnectorClass"))
            .await
            .map_err(internal_error)?
        {
            let Some(meta) = class.metadata.as_ref() else {
                continue;
            };
            if !requested_class.trim().is_empty() && requested_class != meta.name {
                continue;
            }
            let Some(resources_proto::resource_status::Kind::ConnectorClass(status)) = class
                .status
                .as_ref()
                .and_then(|status| status.kind.as_ref())
            else {
                continue;
            };
            if status.registration_id != registration_id {
                continue;
            }
            let Some(resources_proto::resource_spec::Kind::ConnectorClass(spec)) =
                class.spec.and_then(|spec| spec.kind)
            else {
                continue;
            };
            return Ok((meta.name.clone(), spec));
        }
        Err(tonic::Status::not_found(
            "ConnectorClass registration not found",
        ))
    }
}

fn internal_error(err: impl std::fmt::Display) -> tonic::Status {
    tonic::Status::internal(err.to_string())
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
    let session_id = connector_session_id(cp, entry, target, event, labels.clone()).await?;
    labels.insert(LABEL_MESSAGE_SOURCE.to_string(), "connector".to_string());
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
            content: connector_text_projection(event),
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
        if let Some(bytes) = cp.kv.get(&key).await.map_err(internal_error)? {
            if let Ok(session_id) = String::from_utf8(bytes) {
                if cp
                    .kv
                    .get(&keys::session(&entry.namespace, &target.agent, &session_id))
                    .await
                    .map_err(internal_error)?
                    .is_some()
                {
                    return Ok(session_id);
                }
            }
        }
        let session_id =
            scheduling::create_session_with_labels(cp, &entry.namespace, &target.agent, labels)
                .await
                .map_err(map_dispatch_error)?;
        cp.kv
            .set(&key, session_id.as_bytes())
            .await
            .map_err(internal_error)?;
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
        "{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}",
        entry.connector_uid,
        entry.generation,
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
