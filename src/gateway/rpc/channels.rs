// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{models, proto, GrpcGatewayHandler};
use crate::control::topics;
use crate::control::{events, keys, keys::ResourceParent, ControlPlane, KeyValueStore};
use crate::control::{MessagePublisher, ProtoKeyValueStoreExt};
use crate::scheduling;
use futures::{stream, StreamExt, TryStreamExt};
use prost::Message;
use std::collections::{HashMap, HashSet};

const DEFAULT_CHANNEL_MESSAGES_LIMIT: usize = 50;
const MAX_CHANNEL_MESSAGES_LIMIT: usize = 200;
const CHANNEL_MESSAGE_KEY_SCAN_BATCH_SIZE: usize = 512;
const DEFAULT_CHANNEL_CONTEXT_MESSAGES: usize = 20;
const MAX_CHANNEL_CONTEXT_MESSAGES: usize = 50;
const CHANNEL_TIMESTAMP_CAS_RETRIES: usize = 8;
const CHANNEL_DELETE_DESCENDANTS_CONCURRENCY: usize = 32;

const LABEL_CHANNEL: &str = "talon.impalasys.com/channel";
const LABEL_CHANNEL_MESSAGE: &str = "talon.impalasys.com/channel-message";
const LABEL_CHANNEL_SUBSCRIPTION: &str = "talon.impalasys.com/channel-subscription";
const LABEL_CHANNEL_TRIGGER: &str = "talon.impalasys.com/channel-trigger";
const LABEL_MESSAGE_SOURCE: &str = "talon.impalasys.com/message-source";

async fn delete_descendants(kv: &dyn KeyValueStore, parent: ResourceParent) -> anyhow::Result<()> {
    let list = parent.list(None);
    let children = kv.list_keys(&list).await?;
    stream::iter(children)
        .map(|child| async move { kv.delete(&child).await })
        .buffer_unordered(CHANNEL_DELETE_DESCENDANTS_CONCURRENCY)
        .try_collect::<Vec<_>>()
        .await?;
    Ok(())
}

fn validate_resource_name(kind: &str, name: &str) -> Result<(), tonic::Status> {
    if name.trim().is_empty() {
        return Err(tonic::Status::invalid_argument(format!(
            "{kind} name is required"
        )));
    }
    if name.contains('/') {
        return Err(tonic::Status::invalid_argument(format!(
            "{kind} name cannot contain '/'"
        )));
    }
    Ok(())
}

fn validated_channel_messages_page_size(
    page_size: i32,
    legacy_limit: i32,
) -> Result<usize, tonic::Status> {
    if page_size < 0 {
        return Err(tonic::Status::invalid_argument(
            "page_size must be non-negative",
        ));
    }

    let requested = if page_size > 0 {
        page_size as usize
    } else if legacy_limit > 0 {
        legacy_limit as usize
    } else {
        DEFAULT_CHANNEL_MESSAGES_LIMIT
    };

    Ok(requested.min(MAX_CHANNEL_MESSAGES_LIMIT))
}

fn normalize_trigger(trigger: &str) -> Result<String, tonic::Status> {
    let trigger = trigger.trim().to_ascii_lowercase();
    match trigger.as_str() {
        "" => Ok("mention".to_string()),
        "mention" | "manual" | "all" | "routed" | "disabled" => Ok(trigger),
        other => Err(tonic::Status::invalid_argument(format!(
            "channel subscription trigger must be mention, manual, all, routed, or disabled; got {other}"
        ))),
    }
}

fn mention_boundary(ch: char) -> bool {
    !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn contains_mention(content: &str, target: &str) -> bool {
    let target = target.trim();
    if target.is_empty() {
        return false;
    }

    let needle = format!("@{target}");
    let mut offset = 0;
    while let Some(match_offset) = content[offset..].find(&needle) {
        let start = offset + match_offset;
        let end = start + needle.len();
        let start_ok = if start == 0 {
            true
        } else {
            content[..start]
                .chars()
                .next_back()
                .is_none_or(mention_boundary)
        };
        let end_ok = content[end..].chars().next().is_none_or(mention_boundary);
        if start_ok && end_ok {
            return true;
        }
        offset = end;
    }
    false
}

fn context_limit(subscription: &models::ChannelSubscription) -> usize {
    subscription
        .context_policy
        .as_ref()
        .map(|policy| policy.max_messages as usize)
        .filter(|limit| *limit > 0)
        .unwrap_or(DEFAULT_CHANNEL_CONTEXT_MESSAGES)
        .min(MAX_CHANNEL_CONTEXT_MESSAGES)
}

async fn publish_channel_event(
    pubsub: &dyn MessagePublisher,
    event: events::ChannelEvent,
) -> anyhow::Result<()> {
    pubsub
        .publish(
            &topics::channel_events_topic(&event.ns, &event.channel),
            &event.encode_to_vec(),
        )
        .await
}

async fn update_channel_timestamp(
    kv: &dyn KeyValueStore,
    ns: &str,
    channel_name: &str,
    now: i64,
) -> anyhow::Result<()> {
    let key = keys::channel(ns, channel_name);

    for _ in 0..CHANNEL_TIMESTAMP_CAS_RETRIES {
        let Some(current_bytes) = kv.get(&key).await? else {
            return Ok(());
        };
        let mut channel = models::Channel::decode(current_bytes.as_slice())?;
        if channel.updated_at >= now {
            return Ok(());
        }
        channel.updated_at = now;
        let updated = channel.encode_to_vec();
        if kv
            .compare_and_swap(&key, Some(current_bytes.as_slice()), &updated)
            .await?
        {
            return Ok(());
        }
    }

    Err(anyhow::anyhow!(
        "failed to update channel timestamp after concurrent modifications"
    ))
}

async fn persist_channel_message(
    cp: &ControlPlane,
    mut message: models::ChannelMessage,
) -> anyhow::Result<models::ChannelMessage> {
    if message.id.is_empty() {
        message.id = uuid::Uuid::now_v7().to_string();
    }
    let now = chrono::Utc::now().timestamp_micros();
    if message.created_at == 0 {
        message.created_at = now;
    }
    cp.kv
        .set_msg(
            &keys::channel_message(&message.ns, &message.channel, &message.id),
            &message,
        )
        .await?;
    if let Err(error) =
        update_channel_timestamp(cp.kv.as_ref(), &message.ns, &message.channel, now).await
    {
        tracing::warn!(
            error = %error,
            ns = %message.ns,
            channel = %message.channel,
            message_id = %message.id,
            "failed to update channel timestamp after channel message persistence"
        );
    }
    if let Err(error) = publish_channel_event(
        cp.pubsub.as_ref(),
        events::ChannelEvent {
            ns: message.ns.clone(),
            channel: message.channel.clone(),
            kind: events::ChannelEventKind::MessageCreated as i32,
            message: Some(message.clone()),
            session_id: message.source_session_id.clone(),
            agent: message.source_agent.clone(),
            subscription: String::new(),
            error: String::new(),
            timestamp: now,
        },
    )
    .await
    {
        tracing::warn!(
            error = %error,
            ns = %message.ns,
            channel = %message.channel,
            message_id = %message.id,
            "failed to publish channel event for created message"
        );
    }
    Ok(message)
}

pub async fn publish_channel_message_from_session(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    content: &str,
) -> anyhow::Result<models::ChannelMessage> {
    if content.trim().is_empty() {
        anyhow::bail!("channel.publish content is required");
    }
    let session = cp
        .kv
        .get_msg::<models::Session>(&keys::session(ns, agent, session_id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("session not found"))?;
    let channel = session
        .labels
        .get(LABEL_CHANNEL)
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("session is not linked to a channel"))?;
    let mut labels = HashMap::new();
    labels.insert(
        LABEL_MESSAGE_SOURCE.to_string(),
        "channel.publish".to_string(),
    );
    if let Some(source_message) = session.labels.get(LABEL_CHANNEL_MESSAGE) {
        labels.insert(LABEL_CHANNEL_MESSAGE.to_string(), source_message.clone());
    }
    if let Some(subscription) = session.labels.get(LABEL_CHANNEL_SUBSCRIPTION) {
        labels.insert(LABEL_CHANNEL_SUBSCRIPTION.to_string(), subscription.clone());
    }
    persist_channel_message(
        cp,
        models::ChannelMessage {
            id: String::new(),
            ns: ns.to_string(),
            channel,
            author_kind: "agent".to_string(),
            author: agent.to_string(),
            content: content.trim().to_string(),
            created_at: 0,
            source_agent: agent.to_string(),
            source_session_id: session_id.to_string(),
            labels,
        },
    )
    .await
}

pub async fn skip_channel_reply_from_session(
    cp: &ControlPlane,
    ns: &str,
    agent: &str,
    session_id: &str,
    reason: &str,
) -> anyhow::Result<()> {
    let session = cp
        .kv
        .get_msg::<models::Session>(&keys::session(ns, agent, session_id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("session not found"))?;
    let channel = session
        .labels
        .get(LABEL_CHANNEL)
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("session is not linked to a channel"))?;
    publish_channel_event(
        cp.pubsub.as_ref(),
        events::ChannelEvent {
            ns: ns.to_string(),
            channel,
            kind: events::ChannelEventKind::PublishSkipped as i32,
            message: None,
            session_id: session_id.to_string(),
            agent: agent.to_string(),
            subscription: session
                .labels
                .get(LABEL_CHANNEL_SUBSCRIPTION)
                .cloned()
                .unwrap_or_default(),
            error: reason.to_string(),
            timestamp: chrono::Utc::now().timestamp_micros(),
        },
    )
    .await
}

impl GrpcGatewayHandler {
    pub async fn handle_create_channel(
        &self,
        req: tonic::Request<proto::CreateChannelRequest>,
    ) -> Result<tonic::Response<proto::ChannelResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let mut channel = req
            .channel
            .ok_or_else(|| tonic::Status::invalid_argument("channel is required"))?;
        channel.ns = req.ns.clone();
        validate_resource_name("channel", &channel.name)?;
        if channel.status.is_empty() {
            channel.status = "open".to_string();
        }
        let now = chrono::Utc::now().timestamp_micros();
        channel.created_at = now;
        channel.updated_at = now;
        let key = keys::channel(&req.ns, &channel.name);
        if self
            .gateway
            .kv
            .get_msg::<models::Channel>(&key)
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to check channel: {e}")))?
            .is_some()
        {
            return Err(tonic::Status::already_exists("channel already exists"));
        }
        self.gateway
            .kv
            .set_msg(&key, &channel)
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to save channel: {e}")))?;
        Ok(tonic::Response::new(proto::ChannelResponse {
            channel: Some(channel),
        }))
    }

    pub async fn handle_get_channel(
        &self,
        req: tonic::Request<proto::GetChannelRequest>,
    ) -> Result<tonic::Response<proto::ChannelResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let channel = self
            .gateway
            .kv
            .get_msg::<models::Channel>(&keys::channel(&req.ns, &req.name))
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to load channel: {e}")))?
            .ok_or_else(|| tonic::Status::not_found("channel not found"))?;
        Ok(tonic::Response::new(proto::ChannelResponse {
            channel: Some(channel),
        }))
    }

    pub async fn handle_modify_channel(
        &self,
        req: tonic::Request<proto::ModifyChannelRequest>,
    ) -> Result<tonic::Response<proto::ChannelResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        validate_resource_name("channel", &req.name)?;
        let key = keys::channel(&req.ns, &req.name);
        let existing = self
            .gateway
            .kv
            .get_msg::<models::Channel>(&key)
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to load channel: {e}")))?
            .ok_or_else(|| tonic::Status::not_found("channel not found"))?;
        let mut channel = req
            .channel
            .ok_or_else(|| tonic::Status::invalid_argument("channel is required"))?;
        channel.ns = req.ns.clone();
        channel.name = req.name.clone();
        channel.created_at = existing.created_at;
        channel.updated_at = chrono::Utc::now().timestamp_micros();
        if channel.status.is_empty() {
            channel.status = "open".to_string();
        }
        self.gateway
            .kv
            .set_msg(&key, &channel)
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to save channel: {e}")))?;
        Ok(tonic::Response::new(proto::ChannelResponse {
            channel: Some(channel),
        }))
    }

    pub async fn handle_list_channels(
        &self,
        req: tonic::Request<proto::ListChannelsRequest>,
    ) -> Result<tonic::Response<proto::ListChannelsResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let mut entries = self
            .gateway
            .kv
            .list_entries(&keys::channel_prefix(&req.ns))
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to list channels: {e}")))?;
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut channels = Vec::new();
        for (_, value) in entries {
            channels.push(
                models::Channel::decode(value.as_slice()).map_err(|e| {
                    tonic::Status::internal(format!("failed to decode channel: {e}"))
                })?,
            );
        }
        Ok(tonic::Response::new(proto::ListChannelsResponse {
            channels,
        }))
    }

    pub async fn handle_delete_channel(
        &self,
        req: tonic::Request<proto::DeleteChannelRequest>,
    ) -> Result<tonic::Response<proto::DeleteChannelResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        delete_descendants(
            self.gateway.kv.as_ref(),
            keys::channel_parent(&req.ns, &req.name),
        )
        .await
        .map_err(|e| {
            tonic::Status::internal(format!("failed to delete channel descendants: {e}"))
        })?;
        self.gateway
            .kv
            .delete(&keys::channel(&req.ns, &req.name))
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to delete channel: {e}")))?;
        Ok(tonic::Response::new(proto::DeleteChannelResponse {
            success: true,
        }))
    }

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
        if req.content.trim().is_empty() {
            return Err(tonic::Status::invalid_argument("content is required"));
        }
        let channel = self
            .gateway
            .kv
            .get_msg::<models::Channel>(&keys::channel(&req.ns, &req.channel))
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to load channel: {e}")))?
            .ok_or_else(|| tonic::Status::not_found("channel not found"))?;
        if channel.status == "closed" {
            return Err(tonic::Status::failed_precondition("channel is closed"));
        }
        let message = persist_channel_message(
            &ControlPlane {
                kv: self.gateway.kv.clone(),
                pubsub: self.gateway.pubsub.clone(),
                scheduler: self.gateway.scheduler.clone(),
            },
            models::ChannelMessage {
                id: uuid::Uuid::now_v7().to_string(),
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

        let routed_sessions = route_channel_message(
            &ControlPlane {
                kv: self.gateway.kv.clone(),
                pubsub: self.gateway.pubsub.clone(),
                scheduler: self.gateway.scheduler.clone(),
            },
            &message,
            &req.subscription_names,
        )
        .await;

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
            crate::gateway::auth::check_channel_auth(
                req.metadata(),
                auth_config,
                &req.get_ref().ns,
                &req.get_ref().channel,
            )?;
        }
        let req = req.into_inner();
        let message = self
            .gateway
            .kv
            .get_msg::<models::ChannelMessage>(&keys::channel_message(
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
            crate::gateway::auth::check_channel_auth(
                req.metadata(),
                auth_config,
                &req.get_ref().ns,
                &req.get_ref().channel,
            )?;
        }
        let req = req.into_inner();
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
            let entries = self
                .gateway
                .kv
                .list_entries_page(
                    &keys::channel_message_prefix(&req.ns, &req.channel),
                    scan_before_name.as_deref(),
                    CHANNEL_MESSAGE_KEY_SCAN_BATCH_SIZE,
                )
                .await
                .map_err(|e| {
                    tonic::Status::internal(format!("failed to list channel messages: {e}"))
                })?;

            if entries.is_empty() {
                break;
            }

            scan_before_name = entries.last().map(|(key, _)| key.name.clone());
            let remaining = target_message_count.saturating_sub(messages.len());
            for (_, value) in entries.into_iter().take(remaining) {
                messages.push(
                    models::ChannelMessage::decode(value.as_slice()).map_err(|e| {
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

    pub async fn handle_create_channel_subscription(
        &self,
        req: tonic::Request<proto::CreateChannelSubscriptionRequest>,
    ) -> Result<tonic::Response<proto::ChannelSubscriptionResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let mut subscription = req
            .subscription
            .ok_or_else(|| tonic::Status::invalid_argument("subscription is required"))?;
        subscription.ns = req.ns.clone();
        subscription.channel = req.channel.clone();
        validate_subscription(self, &mut subscription).await?;
        let key = keys::channel_subscription(&req.ns, &req.channel, &subscription.name);
        if self
            .gateway
            .kv
            .get_msg::<models::ChannelSubscription>(&key)
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to check subscription: {e}")))?
            .is_some()
        {
            return Err(tonic::Status::already_exists(
                "channel subscription already exists",
            ));
        }
        self.gateway
            .kv
            .set_msg(&key, &subscription)
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to save subscription: {e}")))?;
        Ok(tonic::Response::new(proto::ChannelSubscriptionResponse {
            subscription: Some(subscription),
        }))
    }

    pub async fn handle_get_channel_subscription(
        &self,
        req: tonic::Request<proto::GetChannelSubscriptionRequest>,
    ) -> Result<tonic::Response<proto::ChannelSubscriptionResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let subscription = self
            .gateway
            .kv
            .get_msg::<models::ChannelSubscription>(&keys::channel_subscription(
                &req.ns,
                &req.channel,
                &req.name,
            ))
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to load subscription: {e}")))?
            .ok_or_else(|| tonic::Status::not_found("channel subscription not found"))?;
        Ok(tonic::Response::new(proto::ChannelSubscriptionResponse {
            subscription: Some(subscription),
        }))
    }

    pub async fn handle_modify_channel_subscription(
        &self,
        req: tonic::Request<proto::ModifyChannelSubscriptionRequest>,
    ) -> Result<tonic::Response<proto::ChannelSubscriptionResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let mut subscription = req
            .subscription
            .ok_or_else(|| tonic::Status::invalid_argument("subscription is required"))?;
        subscription.ns = req.ns.clone();
        subscription.channel = req.channel.clone();
        subscription.name = req.name.clone();
        validate_subscription(self, &mut subscription).await?;
        self.gateway
            .kv
            .set_msg(
                &keys::channel_subscription(&req.ns, &req.channel, &req.name),
                &subscription,
            )
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to save subscription: {e}")))?;
        Ok(tonic::Response::new(proto::ChannelSubscriptionResponse {
            subscription: Some(subscription),
        }))
    }

    pub async fn handle_list_channel_subscriptions(
        &self,
        req: tonic::Request<proto::ListChannelSubscriptionsRequest>,
    ) -> Result<tonic::Response<proto::ListChannelSubscriptionsResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let mut entries = self
            .gateway
            .kv
            .list_entries(&keys::channel_subscription_prefix(&req.ns, &req.channel))
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to list subscriptions: {e}")))?;
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut subscriptions = Vec::new();
        for (_, value) in entries {
            subscriptions.push(
                models::ChannelSubscription::decode(value.as_slice()).map_err(|e| {
                    tonic::Status::internal(format!("failed to decode subscription: {e}"))
                })?,
            );
        }
        Ok(tonic::Response::new(
            proto::ListChannelSubscriptionsResponse { subscriptions },
        ))
    }

    pub async fn handle_delete_channel_subscription(
        &self,
        req: tonic::Request<proto::DeleteChannelSubscriptionRequest>,
    ) -> Result<tonic::Response<proto::DeleteChannelSubscriptionResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        self.gateway
            .kv
            .delete(&keys::channel_subscription(
                &req.ns,
                &req.channel,
                &req.name,
            ))
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to delete subscription: {e}")))?;
        Ok(tonic::Response::new(
            proto::DeleteChannelSubscriptionResponse { success: true },
        ))
    }

    pub async fn handle_stream_channel_events(
        &self,
        req: tonic::Request<proto::StreamChannelEventsRequest>,
    ) -> Result<tonic::Response<super::ChannelEventStream>, tonic::Status> {
        if let Some(auth_config) = &self.gateway.auth_config {
            crate::gateway::auth::check_channel_auth(
                req.metadata(),
                auth_config,
                &req.get_ref().ns,
                &req.get_ref().channel,
            )?;
        }
        let req = req.into_inner();
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

async fn validate_subscription(
    handler: &GrpcGatewayHandler,
    subscription: &mut models::ChannelSubscription,
) -> Result<(), tonic::Status> {
    validate_resource_name("channel subscription", &subscription.name)?;
    validate_resource_name("channel", &subscription.channel)?;
    validate_resource_name("agent", &subscription.agent)?;
    subscription.trigger = normalize_trigger(&subscription.trigger)?;
    handler
        .gateway
        .kv
        .get_msg::<models::Channel>(&keys::channel(&subscription.ns, &subscription.channel))
        .await
        .map_err(|e| tonic::Status::internal(format!("failed to load channel: {e}")))?
        .ok_or_else(|| tonic::Status::not_found("channel not found"))?;
    handler
        .gateway
        .kv
        .get_msg::<models::Agent>(&keys::agent(&subscription.ns, &subscription.agent))
        .await
        .map_err(|e| tonic::Status::internal(format!("failed to load agent: {e}")))?
        .ok_or_else(|| tonic::Status::not_found("agent not found"))?;
    if subscription.context_policy.is_none() {
        subscription.context_policy = Some(models::ChannelContextPolicy {
            mode: "recent_public".to_string(),
            max_messages: DEFAULT_CHANNEL_CONTEXT_MESSAGES as u32,
        });
    }
    Ok(())
}

async fn route_channel_message(
    cp: &ControlPlane,
    message: &models::ChannelMessage,
    manual_subscriptions: &[String],
) -> Vec<proto::RoutedChannelSession> {
    let mut results = Vec::new();
    let subscriptions = match matching_subscriptions(cp, message, manual_subscriptions).await {
        Ok(subscriptions) => subscriptions,
        Err(error) => {
            tracing::error!(
                error = %error,
                ns = %message.ns,
                channel = %message.channel,
                message_id = %message.id,
                "failed to match channel subscriptions"
            );
            return results;
        }
    };
    if subscriptions.is_empty() {
        return results;
    }

    let max_context_limit = subscriptions
        .iter()
        .map(context_limit)
        .max()
        .unwrap_or(DEFAULT_CHANNEL_CONTEXT_MESSAGES);
    let recent_messages = match recent_channel_messages(cp, message, max_context_limit).await {
        Ok(messages) => messages,
        Err(error) => {
            tracing::error!(
                error = %error,
                ns = %message.ns,
                channel = %message.channel,
                message_id = %message.id,
                "failed to load recent channel context"
            );
            return results;
        }
    };

    for subscription in subscriptions {
        let result = route_to_subscription(cp, message, &subscription, &recent_messages).await;
        results.push(match result {
            Ok(session_id) => proto::RoutedChannelSession {
                subscription: subscription.name,
                agent: subscription.agent,
                session_id,
                error: String::new(),
            },
            Err(error) => proto::RoutedChannelSession {
                subscription: subscription.name,
                agent: subscription.agent,
                session_id: String::new(),
                error: error.to_string(),
            },
        });
    }
    results
}

async fn matching_subscriptions(
    cp: &ControlPlane,
    message: &models::ChannelMessage,
    manual_subscriptions: &[String],
) -> anyhow::Result<Vec<models::ChannelSubscription>> {
    let manual: HashSet<&str> = manual_subscriptions.iter().map(String::as_str).collect();
    let entries = cp
        .kv
        .list_entries(&keys::channel_subscription_prefix(
            &message.ns,
            &message.channel,
        ))
        .await?;
    let mut subscriptions = Vec::new();
    for (_, value) in entries {
        let subscription = models::ChannelSubscription::decode(value.as_slice())?;
        if !subscription.enabled {
            continue;
        }
        let trigger = normalize_trigger(&subscription.trigger)
            .map_err(|status| anyhow::anyhow!(status.message().to_string()))?;
        let should_route = match trigger.as_str() {
            "manual" => manual.contains(subscription.name.as_str()),
            "all" => message.author_kind != "agent",
            "mention" => {
                message.author_kind != "agent"
                    && (contains_mention(&message.content, &subscription.agent)
                        || contains_mention(&message.content, &subscription.name))
            }
            "routed" | "disabled" => false,
            _ => false,
        };
        if should_route {
            subscriptions.push(subscription);
        }
    }
    subscriptions.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(subscriptions)
}

async fn route_to_subscription(
    cp: &ControlPlane,
    message: &models::ChannelMessage,
    subscription: &models::ChannelSubscription,
    recent_messages: &[models::ChannelMessage],
) -> anyhow::Result<String> {
    let mut labels = HashMap::new();
    labels.insert(LABEL_CHANNEL.to_string(), message.channel.clone());
    labels.insert(LABEL_CHANNEL_MESSAGE.to_string(), message.id.clone());
    labels.insert(
        LABEL_CHANNEL_SUBSCRIPTION.to_string(),
        subscription.name.clone(),
    );
    labels.insert(
        LABEL_CHANNEL_TRIGGER.to_string(),
        subscription.trigger.clone(),
    );

    let session_id = scheduling::create_session_with_labels(
        cp,
        &message.ns,
        &subscription.agent,
        labels.clone(),
    )
    .await?;
    labels.insert(LABEL_MESSAGE_SOURCE.to_string(), "channel".to_string());
    let prompt = format_channel_prompt(
        message,
        subscription,
        recent_messages,
        context_limit(subscription),
    );
    scheduling::send_message(
        cp.kv.as_ref(),
        cp.pubsub.as_ref(),
        &message.ns,
        &subscription.agent,
        &session_id,
        &prompt,
        labels,
        chrono::Utc::now(),
    )
    .await?;
    if let Err(error) = publish_channel_event(
        cp.pubsub.as_ref(),
        events::ChannelEvent {
            ns: message.ns.clone(),
            channel: message.channel.clone(),
            kind: events::ChannelEventKind::SessionRouted as i32,
            message: None,
            session_id: session_id.clone(),
            agent: subscription.agent.clone(),
            subscription: subscription.name.clone(),
            error: String::new(),
            timestamp: chrono::Utc::now().timestamp_micros(),
        },
    )
    .await
    {
        tracing::warn!(
            error = %error,
            ns = %message.ns,
            channel = %message.channel,
            session_id = %session_id,
            agent = %subscription.agent,
            subscription = %subscription.name,
            "failed to publish channel event for routed session"
        );
    }
    Ok(session_id)
}

async fn recent_channel_messages(
    cp: &ControlPlane,
    message: &models::ChannelMessage,
    limit: usize,
) -> anyhow::Result<Vec<models::ChannelMessage>> {
    let entries = cp
        .kv
        .list_entries_page(
            &keys::channel_message_prefix(&message.ns, &message.channel),
            None,
            limit,
        )
        .await?;

    let mut recent = Vec::new();
    for (_, value) in entries {
        recent.push(models::ChannelMessage::decode(value.as_slice())?);
    }
    recent.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(recent)
}

fn format_channel_prompt(
    message: &models::ChannelMessage,
    subscription: &models::ChannelSubscription,
    recent_messages: &[models::ChannelMessage],
    limit: usize,
) -> String {
    let start = recent_messages.len().saturating_sub(limit);
    let mut context = String::new();
    for item in &recent_messages[start..] {
        context.push_str(&format!(
            "- {}:{}: {}\n",
            item.author_kind, item.author, item.content
        ));
    }
    format!(
        "You are subscribed to Talon channel '{}' as agent '{}'. Normal assistant text stays private in your session and will not be posted to the channel. If a public channel reply is needed, call channel_publish with the response content. If no public reply is needed, call channel_skip_reply.\n\nTriggering channel message id: {}\nTriggering author: {}:{}\nTriggering content:\n{}\n\nRecent public channel context:\n{}",
        message.channel,
        subscription.agent,
        message.id,
        message.author_kind,
        message.author,
        message.content,
        context
    )
}
