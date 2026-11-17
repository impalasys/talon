// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{data_proto, external_proto, proto, resources_proto, GrpcGatewayHandler};
use anyhow::Context;
use futures::StreamExt;
use prost::Message;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use talon_control::control::events;
use talon_control::control::keys;
use talon_control::control::resource_model::ChannelResourceExt;
use talon_control::control::resource_model::ChannelSubscriptionResourceExt;
use talon_control::control::resource_model::TypedResource;
use talon_control::control::scheduling;
use talon_control::control::topics;
use talon_control::control::ControlPlane;
use talon_control::control::KeyValueStore;
use talon_control::control::ListOptions;
use talon_control::control::MessagePublisher;
use talon_control::control::ProtoKeyValueStoreExt;

pub use talon_control::channels::*;

impl GrpcGatewayHandler {
    pub async fn handle_post_channel_message(
        &self,
        req: tonic::Request<proto::PostChannelMessageRequest>,
    ) -> Result<tonic::Response<proto::PostChannelMessageResponse>, tonic::Status> {
        if let Some(auth_config) = &self.gateway.auth_config {
            crate::gateway::auth::check_channel_auth(
                req.metadata(),
                auth_config,
                &req.get_ref().ns,
                &req.get_ref().channel,
            )?;
        }
        let req = req.into_inner();
        validate_resource_name("channel", &req.channel)?;
        if req.content.trim().is_empty() {
            return Err(tonic::Status::invalid_argument("content is required"));
        }
        let channel = self
            .gateway
            .kv
            .get_msg::<resources_proto::Channel>(&keys::channel(&req.ns, &req.channel))
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to load channel: {e}")))?
            .ok_or_else(|| tonic::Status::not_found("channel not found"))?;
        if channel.phase() == "closed" {
            return Err(tonic::Status::failed_precondition("channel is closed"));
        }
        let cp = self.gateway.control_plane();
        let message = persist_channel_message(
            &cp,
            data_proto::ChannelMessage {
                id: talon_control::control::uuid::channel_message_id(),
                ns: req.ns.clone(),
                channel: req.channel.clone(),
                author_kind: if req.author_kind.is_empty() {
                    "user".to_string()
                } else {
                    req.author_kind.clone()
                },
                author: req.author.clone(),
                content: req.content.trim().to_string(),
                created_at: chrono::Utc::now().timestamp_micros(),
                source_agent: String::new(),
                source_session_id: String::new(),
                labels: req.labels.clone(),
            },
        )
        .await
        .map_err(|e| tonic::Status::internal(format!("failed to persist channel message: {e}")))?;

        let routed_sessions = route_channel_message(&cp, &message, &req.subscription_names).await;

        Ok(tonic::Response::new(proto::PostChannelMessageResponse {
            message: Some(message),
            routed_sessions,
        }))
    }

    pub async fn handle_get_channel_message(
        &self,
        req: tonic::Request<proto::GetChannelMessageRequest>,
    ) -> Result<tonic::Response<proto::ChannelMessageResponse>, tonic::Status> {
        if let Some(auth_config) = &self.gateway.auth_config {
            crate::gateway::auth::check_channel_auth_for_operation(
                req.metadata(),
                auth_config,
                crate::gateway::auth::AuthzOperation::Read,
                &req.get_ref().ns,
                &req.get_ref().channel,
            )?;
        }
        let req = req.into_inner();
        validate_resource_name("channel", &req.channel)?;
        let message = self
            .gateway
            .kv
            .get_msg::<data_proto::ChannelMessage>(&keys::channel_message(
                &req.ns,
                &req.channel,
                &req.message_id,
            ))
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to load channel message: {e}")))?
            .ok_or_else(|| tonic::Status::not_found("channel message not found"))?;
        Ok(tonic::Response::new(proto::ChannelMessageResponse {
            message: Some(message),
        }))
    }

    pub async fn handle_list_channel_messages(
        &self,
        req: tonic::Request<proto::ListChannelMessagesRequest>,
    ) -> Result<tonic::Response<proto::ListChannelMessagesResponse>, tonic::Status> {
        if let Some(auth_config) = &self.gateway.auth_config {
            crate::gateway::auth::check_channel_auth_for_operation(
                req.metadata(),
                auth_config,
                crate::gateway::auth::AuthzOperation::Read,
                &req.get_ref().ns,
                &req.get_ref().channel,
            )?;
        }
        let req = req.into_inner();
        validate_resource_name("channel", &req.channel)?;
        let page_size = validated_channel_messages_page_size(req.page_size, req.limit)?;
        let before_name = req
            .before_message_id
            .as_deref()
            .filter(|before_message_id| !before_message_id.is_empty())
            .map(str::to_string);
        let target_message_count = page_size + 1;
        let mut scan_before_name = before_name;
        let mut messages = Vec::with_capacity(target_message_count);

        while messages.len() < target_message_count {
            let remaining = target_message_count.saturating_sub(messages.len());
            let entries = self
                .gateway
                .kv
                .list_entries(
                    &keys::channel_message_prefix(&req.ns, &req.channel),
                    Some(
                        ListOptions::desc()
                            .before_name(scan_before_name.as_deref())
                            .limit(remaining),
                    ),
                )
                .await
                .map_err(|e| {
                    tonic::Status::internal(format!("failed to list channel messages: {e}"))
                })?;

            if entries.is_empty() {
                break;
            }

            scan_before_name = entries.last().map(|(key, _)| key.name.clone());
            for (_, value) in entries.into_iter().take(remaining) {
                messages.push(
                    data_proto::ChannelMessage::decode(value.as_slice()).map_err(|e| {
                        tonic::Status::internal(format!("failed to decode channel message: {e}"))
                    })?,
                );
            }
        }

        let has_more = messages.len() > page_size;
        if has_more {
            messages.truncate(page_size);
        }

        messages.reverse();
        let next_before_message_id = if has_more {
            messages.first().map(|message| message.id.clone())
        } else {
            None
        };

        Ok(tonic::Response::new(proto::ListChannelMessagesResponse {
            messages,
            has_more,
            next_before_message_id,
        }))
    }

    pub async fn handle_stream_channel_events(
        &self,
        req: tonic::Request<proto::StreamChannelEventsRequest>,
    ) -> Result<tonic::Response<super::ChannelEventStream>, tonic::Status> {
        if let Some(auth_config) = &self.gateway.auth_config {
            crate::gateway::auth::check_channel_auth_for_operation(
                req.metadata(),
                auth_config,
                crate::gateway::auth::AuthzOperation::Read,
                &req.get_ref().ns,
                &req.get_ref().channel,
            )?;
        }
        let req = req.into_inner();
        validate_resource_name("channel", &req.channel)?;
        let topic = topics::channel_events_topic(&req.ns, &req.channel);
        let stream = self
            .gateway
            .pubsub
            .subscribe(&topic)
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to subscribe: {e}")))?
            .filter_map(|bytes| async move {
                match events::ChannelEvent::decode(bytes.as_slice()) {
                    Ok(event) => Some(Ok(event)),
                    Err(err) => Some(Err(tonic::Status::internal(format!(
                        "failed to decode channel event: {err}"
                    )))),
                }
            });
        Ok(tonic::Response::new(Box::pin(stream)))
    }
}

#[cfg(test)]
mod tests {
    use super::contains_mention;

    #[test]
    fn contains_mention_accepts_slack_style_mentions() {
        assert!(contains_mention(
            "Please check this <@assistant>.",
            "assistant"
        ));
        assert!(contains_mention(
            "<@support.bot:primary> can you help?",
            "support.bot:primary"
        ));
        assert!(!contains_mention(
            "<@assistant-extra> is different",
            "assistant"
        ));
        assert!(!contains_mention("<@assistant", "assistant"));
    }

    #[test]
    fn contains_mention_keeps_legacy_at_mentions_for_compatibility() {
        assert!(contains_mention(
            "Please check this @assistant, thanks",
            "assistant"
        ));
        assert!(!contains_mention(
            "Please check this @assistant-extra.",
            "assistant"
        ));
        assert!(!contains_mention(
            "Please check this @@assistant.",
            "assistant"
        ));
    }
}
