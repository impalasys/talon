// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::events;
use crate::control::keys;
use crate::control::resource_model::ChannelResourceExt;
use crate::control::resource_model::ChannelSubscriptionResourceExt;
use crate::control::resource_model::TypedResource;
use crate::control::scheduling;
use crate::control::topics;
use crate::control::ControlPlane;
use crate::control::KeyValueStore;
use crate::control::ListOptions;
use crate::control::MessagePublisher;
use crate::control::ProtoKeyValueStoreExt;
use ::protocol::{data_proto, external_proto, proto, resources_proto};
use anyhow::Context;
use futures::StreamExt;
use prost::Message;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

pub const DEFAULT_CHANNEL_MESSAGES_LIMIT: usize = 50;
pub const MAX_CHANNEL_MESSAGES_LIMIT: usize = 200;
pub const DEFAULT_CHANNEL_CONTEXT_MESSAGES: usize = 20;
pub const MAX_CHANNEL_CONTEXT_MESSAGES: usize = 50;
pub const CHANNEL_TIMESTAMP_CAS_RETRIES: usize = 8;
pub const MAX_RESOURCE_NAME_LEN: usize = 253;

pub const LABEL_CHANNEL: &str = "talon.impalasys.com/channel";
pub const LABEL_CHANNEL_MESSAGE: &str = "talon.impalasys.com/channel-message";
pub const LABEL_CHANNEL_SUBSCRIPTION: &str = "talon.impalasys.com/channel-subscription";
pub const LABEL_CHANNEL_TRIGGER: &str = "talon.impalasys.com/channel-trigger";
pub const LABEL_CHANNEL_REPLY_MODE: &str = "talon.impalasys.com/channel-reply-mode";
pub const LABEL_MESSAGE_SOURCE: &str = "talon.impalasys.com/message-source";
pub const LABEL_CONNECTOR: &str = "talon.impalasys.com/connector";
pub const LABEL_CONNECTOR_CLASS: &str = "talon.impalasys.com/connector-class";
pub const LABEL_CONNECTOR_MATCH_PREFIX: &str = "talon.impalasys.com/connector-match/";
pub const LABEL_CONNECTOR_REGISTRATION: &str = "talon.impalasys.com/connector-registration";
pub const LABEL_EXTERNAL_CONVERSATION: &str = "talon.impalasys.com/external-conversation";
pub const CONNECTOR_HTTP_TIMEOUT: Duration = Duration::from_secs(15);
pub const LABEL_EXTERNAL_MESSAGE: &str = "talon.impalasys.com/external-message";
pub const LABEL_EXTERNAL_THREAD: &str = "talon.impalasys.com/external-thread";
pub const CHANNEL_REPLY_MODE_HOLD_FOR_REVIEW: &str = "hold_for_review";

pub fn validate_resource_name(kind: &str, name: &str) -> Result<(), tonic::Status> {
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

pub fn validated_channel_messages_page_size(
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

pub fn normalize_trigger(trigger: &str) -> Result<String, tonic::Status> {
    let trigger = trigger.trim().to_ascii_lowercase();
    match trigger.as_str() {
        "" => Ok("mention".to_string()),
        "mention" | "manual" | "all" | "routed" | "disabled" => Ok(trigger),
        other => Err(tonic::Status::invalid_argument(format!(
            "channel subscription trigger must be mention, manual, all, routed, or disabled; got {other}"
        ))),
    }
}

pub fn mention_boundary(ch: char) -> bool {
    !(ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '.' || ch == ':')
}

pub fn contains_mention(content: &str, target: &str) -> bool {
    let target = target.trim();
    if target.is_empty() {
        return false;
    }

    let slack_needle = format!("<@{target}>");
    if content.contains(&slack_needle) {
        return true;
    }

    let needle = format!("@{target}");
    let mut offset = 0;
    while let Some(match_offset) = content[offset..].find(&needle) {
        let start = offset + match_offset;
        let end = start + needle.len();
        let previous = content[..start].chars().next_back();
        let start_ok = previous.map_or(true, |ch| ch != '@' && ch != '<' && mention_boundary(ch));
        let end_ok = content[end..].chars().next().map_or(true, mention_boundary);
        if start_ok && end_ok {
            return true;
        }
        offset = end;
    }
    false
}

pub fn context_limit(subscription: &resources_proto::ChannelSubscription) -> usize {
    subscription
        .context_policy()
        .map(|policy| policy.max_messages as usize)
        .filter(|limit| *limit > 0)
        .unwrap_or(DEFAULT_CHANNEL_CONTEXT_MESSAGES)
        .min(MAX_CHANNEL_CONTEXT_MESSAGES)
}

pub async fn publish_channel_event(
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

pub async fn update_channel_timestamp(
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

pub async fn persist_channel_message(
    cp: &ControlPlane,
    mut message: data_proto::ChannelMessage,
) -> anyhow::Result<data_proto::ChannelMessage> {
    if message.id.is_empty() {
        message.id = crate::control::uuid::channel_message_id();
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
    let message = persist_channel_message(
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
    .await?;
    deliver_connector_channel_message(cp, &session, &message).await?;
    Ok(message)
}

pub async fn deliver_connector_channel_message(
    cp: &ControlPlane,
    session: &data_proto::Session,
    message: &data_proto::ChannelMessage,
) -> anyhow::Result<()> {
    if session
        .labels
        .get(LABEL_CHANNEL_TRIGGER)
        .map(|trigger| trigger != "connector")
        .unwrap_or(true)
    {
        return Ok(());
    }

    let registration_id = required_label(&session.labels, LABEL_CONNECTOR_REGISTRATION)?;
    let connector_name = required_label(&session.labels, LABEL_CONNECTOR)?;
    let connector_class = required_label(&session.labels, LABEL_CONNECTOR_CLASS)?;
    let external_conversation_id = required_label(&session.labels, LABEL_EXTERNAL_CONVERSATION)?;
    let (runtime_endpoint, api_key) =
        crate::connector_runtime::connector_runtime_endpoint_and_api_key(
            cp,
            registration_id,
            connector_class,
        )
        .await?;

    let mut match_fields = HashMap::new();
    for (key, value) in &session.labels {
        if let Some(field) = key.strip_prefix(LABEL_CONNECTOR_MATCH_PREFIX) {
            match_fields.insert(field.to_string(), value.clone());
        }
    }

    let reply_mode = session
        .labels
        .get(LABEL_CHANNEL_REPLY_MODE)
        .map(|value| value.as_str())
        .unwrap_or_default();
    let external_message_id = session.labels.get(LABEL_EXTERNAL_MESSAGE).cloned();
    let external_thread_id = if reply_mode == "thread" {
        session
            .labels
            .get(LABEL_EXTERNAL_THREAD)
            .cloned()
            .or_else(|| external_message_id.clone())
    } else {
        session.labels.get(LABEL_EXTERNAL_THREAD).cloned()
    };
    let reply_to_external_message_id = if reply_mode == "thread" {
        external_message_id
    } else {
        None
    };

    let mut delivery_labels = HashMap::new();
    delivery_labels.insert("talon.channel".to_string(), message.channel.clone());
    delivery_labels.insert("talon.channelMessage".to_string(), message.id.clone());
    if let Some(source_message) = session.labels.get(LABEL_CHANNEL_MESSAGE) {
        delivery_labels.insert(
            "talon.sourceChannelMessage".to_string(),
            source_message.clone(),
        );
    }
    if let Some(source_event) = session
        .labels
        .get("talon.impalasys.com/connector-event")
        .cloned()
    {
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
            namespace: message.ns.clone(),
            connector_name: connector_name.to_string(),
            match_fields,
            external_conversation_id: external_conversation_id.to_string(),
            external_thread_id,
            reply_to_external_message_id,
            text: message.content.clone(),
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

pub fn required_label<'a>(
    labels: &'a HashMap<String, String>,
    name: &str,
) -> anyhow::Result<&'a str> {
    labels
        .get(name)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing connector label {name}"))
}

pub fn connector_http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(CONNECTOR_HTTP_TIMEOUT)
        .build()
        .context("failed to build connector HTTP client")
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

pub async fn route_channel_message(
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

pub async fn matching_subscriptions(
    cp: &ControlPlane,
    message: &data_proto::ChannelMessage,
    manual_subscriptions: &[String],
) -> anyhow::Result<Vec<resources_proto::ChannelSubscription>> {
    let manual: HashSet<&str> = manual_subscriptions.iter().map(String::as_str).collect();
    let entries = cp
        .kv
        .list_entries(
            &keys::channel_subscription_prefix(&message.ns, &message.channel),
            None,
        )
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

pub async fn route_to_subscription(
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

pub async fn route_connector_channel_message(
    cp: &ControlPlane,
    message: &data_proto::ChannelMessage,
    agent: &str,
    connector_name: &str,
    reply_mode: &str,
) -> anyhow::Result<String> {
    let normalized_reply_mode = normalize_channel_reply_mode(reply_mode);
    let reply_mode = if normalized_reply_mode.is_empty() {
        "auto"
    } else {
        normalized_reply_mode.as_str()
    };

    let mut labels = HashMap::new();
    labels.insert(LABEL_CHANNEL.to_string(), message.channel.clone());
    labels.insert(LABEL_CHANNEL_MESSAGE.to_string(), message.id.clone());
    labels.insert(
        LABEL_CHANNEL_SUBSCRIPTION.to_string(),
        connector_name.to_string(),
    );
    labels.insert(LABEL_CHANNEL_TRIGGER.to_string(), "connector".to_string());
    labels.insert(LABEL_CHANNEL_REPLY_MODE.to_string(), reply_mode.to_string());
    labels.extend(message.labels.clone());
    labels.insert(LABEL_MESSAGE_SOURCE.to_string(), "connector".to_string());

    let session_id =
        scheduling::create_session_with_labels(cp, &message.ns, agent, labels.clone()).await?;

    let prompt = format_connector_channel_prompt(message, agent, reply_mode);
    if let Err(error) = scheduling::send_message(
        cp.kv.as_ref(),
        cp.pubsub.as_ref(),
        &message.ns,
        agent,
        &session_id,
        &prompt,
        labels,
        chrono::Utc::now(),
    )
    .await
    {
        if let Err(cleanup_error) =
            delete_routed_session(cp.kv.as_ref(), &message.ns, agent, &session_id).await
        {
            tracing::warn!(
                error = %cleanup_error,
                ns = %message.ns,
                agent = %agent,
                session_id = %session_id,
                "failed to clean up connector-routed channel session after scheduling error"
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
            agent: agent.to_string(),
            subscription: connector_name.to_string(),
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
            agent = %agent,
            connector = %connector_name,
            "failed to publish channel event for connector-routed session"
        );
    }

    Ok(session_id)
}

pub fn format_connector_channel_prompt(
    message: &data_proto::ChannelMessage,
    agent: &str,
    reply_mode: &str,
) -> String {
    let reply_instruction = if reply_mode == "none" {
        "Normal assistant text stays private in your session and will not be posted to the channel. This connector is configured with replyPolicy none, so no public channel reply is expected."
    } else if reply_mode == CHANNEL_REPLY_MODE_HOLD_FOR_REVIEW {
        "Your final assistant response will be held for operator review before it is posted through the connector. Do not call channel_publish. If no public reply is appropriate, answer with the no-reply token <talon:no-reply>."
    } else if reply_mode == "thread" {
        "Normal assistant text stays private in your session and will not be posted to the channel. This connector is configured with replyPolicy thread, so answer by calling channel_publish exactly once with the public response content. Only call channel_skip_reply for spam, bot loops, or messages that clearly require no response."
    } else {
        "Normal assistant text stays private in your session and will not be posted to the channel. If a public channel reply is needed, call channel_publish with the response content. Only call channel_skip_reply when no public reply is appropriate."
    };
    format!(
        "You are handling a connector message in Talon channel '{}' as agent '{}'. {}\n\nTriggering channel message id: {}\nTriggering author: {}:{}\nTriggering content:\n{}",
        message.channel,
        agent,
        reply_instruction,
        message.id,
        message.author_kind,
        message.author,
        message.content,
    )
}

pub fn normalize_channel_reply_mode(value: &str) -> String {
    match value.trim() {
        "review" | "hold_for_review" => CHANNEL_REPLY_MODE_HOLD_FOR_REVIEW.to_string(),
        other => other.to_string(),
    }
}

pub async fn delete_routed_session(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
) -> anyhow::Result<()> {
    let mut stack = vec![keys::session_parent(ns, agent, session_id)];
    while let Some(parent) = stack.pop() {
        let list = parent.list(None);
        let children = kv.list_keys(&list, None).await?;
        for child in children {
            stack.push(child.as_parent());
            kv.delete(&child).await?;
        }
    }
    kv.delete(&keys::session(ns, agent, session_id)).await?;
    Ok(())
}

pub async fn recent_channel_messages(
    cp: &ControlPlane,
    message: &data_proto::ChannelMessage,
    limit: usize,
) -> anyhow::Result<Vec<data_proto::ChannelMessage>> {
    let entries = cp
        .kv
        .list_entries(
            &keys::channel_message_prefix(&message.ns, &message.channel),
            Some(ListOptions::desc().limit(limit)),
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

pub fn format_channel_prompt(
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
