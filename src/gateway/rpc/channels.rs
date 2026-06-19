// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{data_proto, proto, resources_proto, GrpcGatewayHandler};
use crate::control::resource_model::{
    ChannelResourceExt, ChannelSubscriptionResourceExt, TypedResource,
};
use crate::control::scheduling;
use crate::control::topics;
use crate::control::{events, keys, ControlPlane, KeyValueStore};
use crate::control::{MessagePublisher, ProtoKeyValueStoreExt};
use futures::StreamExt;
use prost::Message;
use std::collections::{HashMap, HashSet};

const DEFAULT_CHANNEL_MESSAGES_LIMIT: usize = 50;
const MAX_CHANNEL_MESSAGES_LIMIT: usize = 200;
const DEFAULT_CHANNEL_CONTEXT_MESSAGES: usize = 20;
const MAX_CHANNEL_CONTEXT_MESSAGES: usize = 50;
const CHANNEL_TIMESTAMP_CAS_RETRIES: usize = 8;
const MAX_RESOURCE_NAME_LEN: usize = 253;

pub const LABEL_CHANNEL: &str = "talon.impalasys.com/channel";
const LABEL_CHANNEL_MESSAGE: &str = "talon.impalasys.com/channel-message";
const LABEL_CHANNEL_SUBSCRIPTION: &str = "talon.impalasys.com/channel-subscription";
const LABEL_CHANNEL_TRIGGER: &str = "talon.impalasys.com/channel-trigger";
pub const LABEL_CHANNEL_REPLY_MODE: &str = "talon.impalasys.com/channel-reply-mode";
const LABEL_MESSAGE_SOURCE: &str = "talon.impalasys.com/message-source";

fn validate_resource_name(kind: &str, name: &str) -> Result<(), tonic::Status> {
    if name.trim().is_empty() {
        return Err(tonic::Status::invalid_argument(format!(
            "{kind} name is required"
        )));
    }
    if name.len() > MAX_RESOURCE_NAME_LEN {
        return Err(tonic::Status::invalid_argument(format!(
            "{kind} name cannot exceed {MAX_RESOURCE_NAME_LEN} characters"
        )));
    }
    if name.trim() != name {
        return Err(tonic::Status::invalid_argument(format!(
            "{kind} name cannot contain leading or trailing whitespace"
        )));
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
    {
        return Err(tonic::Status::invalid_argument(format!(
            "{kind} name can only contain ASCII alphanumeric characters, '-', '_', '.', and ':'"
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
    !(ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '.' || ch == ':')
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
        let previous = content[..start].chars().next_back();
        let start_ok = previous.map_or(true, |ch| ch != '@' && mention_boundary(ch));
        let end_ok = content[end..].chars().next().map_or(true, mention_boundary);
        if start_ok && end_ok {
            return true;
        }
        offset = end;
    }
    false
}

fn context_limit(subscription: &resources_proto::ChannelSubscription) -> usize {
    subscription
        .context_policy()
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
        let mut channel = resources_proto::Channel::decode(current_bytes.as_slice())?;
        if channel.updated_at() >= now {
            return Ok(());
        }
        channel.set_updated_at(now);
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
    mut message: data_proto::ChannelMessage,
) -> anyhow::Result<data_proto::ChannelMessage> {
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
) -> anyhow::Result<data_proto::ChannelMessage> {
    if content.trim().is_empty() {
        anyhow::bail!("channel.publish content is required");
    }
    let session = cp
        .kv
        .get_msg::<data_proto::Session>(&keys::session(ns, agent, session_id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("session not found"))?;
    let channel = session
        .labels
        .get(LABEL_CHANNEL)
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("session is not linked to a channel"))?;
    let channel_obj = cp
        .kv
        .get_msg::<resources_proto::Channel>(&keys::channel(ns, &channel))
        .await?
        .ok_or_else(|| anyhow::anyhow!("channel not found"))?;
    if channel_obj.phase() == "closed" {
        anyhow::bail!("channel is closed");
    }
    if session
        .labels
        .get(LABEL_CHANNEL_REPLY_MODE)
        .map(|mode| mode == "none")
        .unwrap_or(false)
    {
        anyhow::bail!("channel replies are disabled for this subscription");
    }
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
        data_proto::ChannelMessage {
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
        .get_msg::<data_proto::Session>(&keys::session(ns, agent, session_id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("session not found"))?;
    let channel = session
        .labels
        .get(LABEL_CHANNEL)
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("session is not linked to a channel"))?;
    if session
        .labels
        .get(LABEL_CHANNEL_REPLY_MODE)
        .map(|mode| mode == "none")
        .unwrap_or(false)
    {
        anyhow::bail!("channel replies are disabled for this subscription");
    }
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
        let message = persist_channel_message(
            &ControlPlane {
                kv: self.gateway.kv.clone(),
                pubsub: self.gateway.pubsub.clone(),
                scheduler: self.gateway.scheduler.clone(),
                objects: self.gateway.objects.clone(),
            },
            data_proto::ChannelMessage {
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
                objects: self.gateway.objects.clone(),
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
            crate::gateway::auth::check_channel_auth(
                req.metadata(),
                auth_config,
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
                .list_entries_page(
                    &keys::channel_message_prefix(&req.ns, &req.channel),
                    scan_before_name.as_deref(),
                    remaining,
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
            crate::gateway::auth::check_channel_auth(
                req.metadata(),
                auth_config,
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

async fn route_channel_message(
    cp: &ControlPlane,
    message: &data_proto::ChannelMessage,
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
            tracing::warn!(
                error = %error,
                ns = %message.ns,
                channel = %message.channel,
                message_id = %message.id,
                "failed to load recent channel context; proceeding with empty context"
            );
            Vec::new()
        }
    };

    for subscription in subscriptions {
        let result = route_to_subscription(cp, message, &subscription, &recent_messages).await;
        results.push(match result {
            Ok(session_id) => proto::RoutedChannelSession {
                subscription: subscription.name().to_string(),
                agent: subscription.agent().to_string(),
                session_id,
                error: String::new(),
            },
            Err(error) => proto::RoutedChannelSession {
                subscription: subscription.name().to_string(),
                agent: subscription.agent().to_string(),
                session_id: String::new(),
                error: error.to_string(),
            },
        });
    }
    results
}

async fn matching_subscriptions(
    cp: &ControlPlane,
    message: &data_proto::ChannelMessage,
    manual_subscriptions: &[String],
) -> anyhow::Result<Vec<resources_proto::ChannelSubscription>> {
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
        let subscription = match resources_proto::ChannelSubscription::decode(value.as_slice()) {
            Ok(subscription) => subscription,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    ns = %message.ns,
                    channel = %message.channel,
                    message_id = %message.id,
                    "failed to decode channel subscription; skipping"
                );
                continue;
            }
        };
        if !subscription.enabled() {
            continue;
        }
        let trigger = match normalize_trigger(subscription.trigger()) {
            Ok(trigger) => trigger,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    ns = %message.ns,
                    channel = %message.channel,
                    subscription = %subscription.name(),
                    "failed to normalize channel subscription trigger; skipping"
                );
                continue;
            }
        };
        let is_self = message.author_kind == "agent" && message.author == subscription.agent();
        let should_route = match trigger.as_str() {
            "manual" => manual.contains(subscription.name()),
            "all" => !is_self,
            "mention" => {
                !is_self
                    && (contains_mention(&message.content, subscription.agent())
                        || contains_mention(&message.content, subscription.name()))
            }
            "routed" | "disabled" => false,
            _ => false,
        };
        if should_route {
            subscriptions.push(subscription);
        }
    }
    subscriptions.sort_by(|a, b| a.name().cmp(b.name()));
    Ok(subscriptions)
}

async fn route_to_subscription(
    cp: &ControlPlane,
    message: &data_proto::ChannelMessage,
    subscription: &resources_proto::ChannelSubscription,
    recent_messages: &[data_proto::ChannelMessage],
) -> anyhow::Result<String> {
    let mut labels = HashMap::new();
    labels.insert(LABEL_CHANNEL.to_string(), message.channel.clone());
    labels.insert(LABEL_CHANNEL_MESSAGE.to_string(), message.id.clone());
    labels.insert(
        LABEL_CHANNEL_SUBSCRIPTION.to_string(),
        subscription.name().to_string(),
    );
    labels.insert(
        LABEL_CHANNEL_TRIGGER.to_string(),
        subscription.trigger().to_string(),
    );
    labels.insert(
        LABEL_CHANNEL_REPLY_MODE.to_string(),
        subscription.reply_mode().to_string(),
    );

    let session_id = scheduling::create_session_with_labels(
        cp,
        &message.ns,
        subscription.agent(),
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
    if let Err(error) = scheduling::send_message(
        cp.kv.as_ref(),
        cp.pubsub.as_ref(),
        &message.ns,
        subscription.agent(),
        &session_id,
        &prompt,
        labels,
        chrono::Utc::now(),
    )
    .await
    {
        if let Err(cleanup_error) = delete_routed_session(
            cp.kv.as_ref(),
            &message.ns,
            subscription.agent(),
            &session_id,
        )
        .await
        {
            tracing::warn!(
                error = %cleanup_error,
                ns = %message.ns,
                agent = %subscription.agent(),
                session_id = %session_id,
                "failed to clean up routed channel session after scheduling error"
            );
        }
        return Err(error);
    }
    if let Err(error) = publish_channel_event(
        cp.pubsub.as_ref(),
        events::ChannelEvent {
            ns: message.ns.clone(),
            channel: message.channel.clone(),
            kind: events::ChannelEventKind::SessionRouted as i32,
            message: None,
            session_id: session_id.clone(),
            agent: subscription.agent().to_string(),
            subscription: subscription.name().to_string(),
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
            agent = %subscription.agent(),
            subscription = %subscription.name(),
            "failed to publish channel event for routed session"
        );
    }
    Ok(session_id)
}

async fn delete_routed_session(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
) -> anyhow::Result<()> {
    let mut stack = vec![keys::session_parent(ns, agent, session_id)];
    while let Some(parent) = stack.pop() {
        let list = parent.list(None);
        let children = kv.list_keys(&list).await?;
        for child in children {
            stack.push(child.as_parent());
            kv.delete(&child).await?;
        }
    }
    kv.delete(&keys::session(ns, agent, session_id)).await?;
    Ok(())
}

async fn recent_channel_messages(
    cp: &ControlPlane,
    message: &data_proto::ChannelMessage,
    limit: usize,
) -> anyhow::Result<Vec<data_proto::ChannelMessage>> {
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
        recent.push(data_proto::ChannelMessage::decode(value.as_slice())?);
    }
    recent.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(recent)
}

fn format_channel_prompt(
    message: &data_proto::ChannelMessage,
    subscription: &resources_proto::ChannelSubscription,
    recent_messages: &[data_proto::ChannelMessage],
    limit: usize,
) -> String {
    let start = recent_messages.len().saturating_sub(limit);
    let mut context = String::new();
    for item in &recent_messages[start..] {
        if item.id == message.id {
            continue;
        }
        context.push_str(&format!(
            "- {}:{}: {}\n",
            item.author_kind, item.author, item.content
        ));
    }
    let context_section = if context.is_empty() {
        String::new()
    } else {
        format!("\n\nRecent public channel context:\n{}", context)
    };
    let reply_instruction = if subscription.reply_mode() == "none" {
        "Normal assistant text stays private in your session and will not be posted to the channel. This subscription is configured with replyMode none, so no public channel reply is expected."
    } else {
        "Normal assistant text stays private in your session and will not be posted to the channel. If a public channel reply is needed, call channel_publish with the response content. If no public reply is needed, call channel_skip_reply."
    };
    format!(
        "You are subscribed to Talon channel '{}' as agent '{}'. {}\n\nTriggering channel message id: {}\nTriggering author: {}:{}\nTriggering content:\n{}{}",
        message.channel,
        subscription.agent(),
        reply_instruction,
        message.id,
        message.author_kind,
        message.author,
        message.content,
        context_section
    )
}
