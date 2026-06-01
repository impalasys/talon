// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[cfg(test)]
mod tests {
    use crate::control::{
        events, keys, scheduler::NoopSchedulerBackend, topics, ControlPlane, KeyValueStore,
        ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::{manifests, models, proto, GrpcGatewayHandler};
    use crate::gateway::server::Gateway;
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use prost::Message;
    use std::collections::HashMap;
    use std::sync::Arc;

    const LABEL_CHANNEL: &str = "talon.impalasys.com/channel";
    const LABEL_CHANNEL_MESSAGE: &str = "talon.impalasys.com/channel-message";
    const LABEL_CHANNEL_SUBSCRIPTION: &str = "talon.impalasys.com/channel-subscription";
    const LABEL_CHANNEL_TRIGGER: &str = "talon.impalasys.com/channel-trigger";
    const LABEL_CHANNEL_REPLY_MODE: &str = "talon.impalasys.com/channel-reply-mode";
    const LABEL_MESSAGE_SOURCE: &str = "talon.impalasys.com/message-source";

    fn setup_handler() -> (GrpcGatewayHandler, Arc<MockKvStore>, Arc<RecordingPubSub>) {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let gateway = Arc::new(Gateway::new(
            None,
            kv.clone(),
            pubsub.clone(),
            Arc::new(NoopSchedulerBackend),
        ));
        (GrpcGatewayHandler { gateway }, kv, pubsub)
    }

    fn control_plane(kv: Arc<MockKvStore>, pubsub: Arc<RecordingPubSub>) -> ControlPlane {
        ControlPlane {
            kv,
            pubsub,
            scheduler: Arc::new(NoopSchedulerBackend),
        }
    }

    fn custom_agent_definition() -> manifests::AgentDefinition {
        manifests::AgentDefinition {
            source: Some(manifests::agent_definition::Source::CustomSpec(
                manifests::AgentSpec {
                    features: Vec::new(),
                    model_policy: Some(manifests::ModelPolicy {
                        profiles: vec![manifests::ModelProfile {
                            name: "default".to_string(),
                            model: Some(manifests::Model {
                                provider: "mock".to_string(),
                                name: "gpt-5".to_string(),
                                temperature: 0.0,
                                thinking: None,
                            }),
                        }],
                    }),
                    system_prompt: "You are helpful.".to_string(),
                    mcp_server_refs: Vec::new(),
                    capabilities: HashMap::new(),
                },
            )),
        }
    }

    async fn seed_agent(kv: &Arc<MockKvStore>, ns: &str, name: &str) {
        kv.set_msg(
            &keys::agent(ns, name),
            &models::Agent {
                name: name.to_string(),
                ns: ns.to_string(),
                definition: Some(custom_agent_definition()),
                effective_spec: None,
                template_deps: Vec::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .expect("agent seed should succeed");
    }

    async fn seed_channel(kv: &Arc<MockKvStore>, ns: &str, name: &str) {
        kv.set_msg(
            &keys::channel(ns, name),
            &models::Channel {
                name: name.to_string(),
                ns: ns.to_string(),
                title: "Incident room".to_string(),
                status: "open".to_string(),
                created_at: 1,
                updated_at: 1,
                metadata: HashMap::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .expect("channel seed should succeed");
    }

    async fn seed_channel_message(
        kv: &Arc<MockKvStore>,
        ns: &str,
        channel: &str,
        id: &str,
        content: &str,
    ) {
        seed_channel_message_at(kv, ns, channel, id, content, 1).await;
    }

    async fn seed_channel_message_at(
        kv: &Arc<MockKvStore>,
        ns: &str,
        channel: &str,
        id: &str,
        content: &str,
        created_at: i64,
    ) {
        kv.set_msg(
            &keys::channel_message(ns, channel, id),
            &models::ChannelMessage {
                id: id.to_string(),
                ns: ns.to_string(),
                channel: channel.to_string(),
                author_kind: "user".to_string(),
                author: "tester".to_string(),
                content: content.to_string(),
                created_at,
                source_agent: String::new(),
                source_session_id: String::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .expect("channel message seed should succeed");
    }

    fn subscription(
        name: &str,
        ns: &str,
        channel: &str,
        agent: &str,
        trigger: &str,
    ) -> models::ChannelSubscription {
        models::ChannelSubscription {
            name: name.to_string(),
            ns: ns.to_string(),
            channel: channel.to_string(),
            agent: agent.to_string(),
            enabled: true,
            trigger: trigger.to_string(),
            context_policy: None,
            metadata: HashMap::new(),
            labels: HashMap::new(),
            reply_mode: String::new(),
        }
    }

    #[tokio::test]
    async fn channel_resource_names_reject_edge_whitespace() {
        let (handler, kv, _) = setup_handler();
        seed_agent(&kv, "acme", "analyst").await;
        seed_channel(&kv, "acme", "incident-1").await;

        let invalid_channel = handler
            .handle_create_channel(tonic::Request::new(proto::CreateChannelRequest {
                ns: "acme".to_string(),
                channel: Some(models::Channel {
                    name: " incident-2".to_string(),
                    ns: String::new(),
                    title: "Incident 2".to_string(),
                    status: String::new(),
                    created_at: 0,
                    updated_at: 0,
                    metadata: HashMap::new(),
                    labels: HashMap::new(),
                }),
            }))
            .await
            .expect_err("leading whitespace channel name should fail");
        assert_eq!(invalid_channel.code(), tonic::Code::InvalidArgument);

        let invalid_special_channel = handler
            .handle_create_channel(tonic::Request::new(proto::CreateChannelRequest {
                ns: "acme".to_string(),
                channel: Some(models::Channel {
                    name: "incident?debug=true".to_string(),
                    ns: String::new(),
                    title: "Incident 2".to_string(),
                    status: String::new(),
                    created_at: 0,
                    updated_at: 0,
                    metadata: HashMap::new(),
                    labels: HashMap::new(),
                }),
            }))
            .await
            .expect_err("special URL characters in channel name should fail");
        assert_eq!(invalid_special_channel.code(), tonic::Code::InvalidArgument);

        let invalid_long_channel = handler
            .handle_create_channel(tonic::Request::new(proto::CreateChannelRequest {
                ns: "acme".to_string(),
                channel: Some(models::Channel {
                    name: "a".repeat(254),
                    ns: String::new(),
                    title: "Incident 2".to_string(),
                    status: String::new(),
                    created_at: 0,
                    updated_at: 0,
                    metadata: HashMap::new(),
                    labels: HashMap::new(),
                }),
            }))
            .await
            .expect_err("overlong channel name should fail");
        assert_eq!(invalid_long_channel.code(), tonic::Code::InvalidArgument);

        let invalid_subscription = handler
            .handle_create_channel_subscription(tonic::Request::new(
                proto::CreateChannelSubscriptionRequest {
                    ns: "acme".to_string(),
                    channel: "incident-1".to_string(),
                    subscription: Some(subscription("primary ", "", "", "analyst", "mention")),
                },
            ))
            .await
            .expect_err("trailing whitespace subscription name should fail");
        assert_eq!(invalid_subscription.code(), tonic::Code::InvalidArgument);

        let invalid_special_subscription = handler
            .handle_create_channel_subscription(tonic::Request::new(
                proto::CreateChannelSubscriptionRequest {
                    ns: "acme".to_string(),
                    channel: "incident-1".to_string(),
                    subscription: Some(subscription(
                        "primary&debug=true",
                        "",
                        "",
                        "analyst",
                        "mention",
                    )),
                },
            ))
            .await
            .expect_err("special URL characters in subscription name should fail");
        assert_eq!(
            invalid_special_subscription.code(),
            tonic::Code::InvalidArgument
        );
    }

    #[tokio::test]
    async fn channel_and_subscription_crud_round_trip() {
        let (handler, kv, _) = setup_handler();
        seed_agent(&kv, "acme", "analyst").await;

        let created = handler
            .handle_create_channel(tonic::Request::new(proto::CreateChannelRequest {
                ns: "acme".to_string(),
                channel: Some(models::Channel {
                    name: "incident-1".to_string(),
                    ns: String::new(),
                    title: "Incident 1".to_string(),
                    status: String::new(),
                    created_at: 0,
                    updated_at: 0,
                    metadata: HashMap::from([("priority".to_string(), "high".to_string())]),
                    labels: HashMap::new(),
                }),
            }))
            .await
            .expect("channel create should succeed")
            .into_inner()
            .channel
            .expect("channel response should include channel");
        assert_eq!(created.ns, "acme");
        assert_eq!(created.status, "open");
        assert!(created.created_at > 0);

        let listed = handler
            .handle_list_channels(tonic::Request::new(proto::ListChannelsRequest {
                ns: "acme".to_string(),
            }))
            .await
            .expect("channel list should succeed")
            .into_inner();
        assert_eq!(listed.channels.len(), 1);
        assert_eq!(listed.channels[0].name, "incident-1");

        let modified = handler
            .handle_modify_channel(tonic::Request::new(proto::ModifyChannelRequest {
                ns: "acme".to_string(),
                name: "incident-1".to_string(),
                channel: Some(models::Channel {
                    title: "Renamed incident".to_string(),
                    status: "closed".to_string(),
                    ..created.clone()
                }),
            }))
            .await
            .expect("channel modify should succeed")
            .into_inner()
            .channel
            .expect("channel response should include channel");
        assert_eq!(modified.title, "Renamed incident");
        assert_eq!(modified.created_at, created.created_at);
        assert!(modified.updated_at >= created.updated_at);

        let create_subscription = handler
            .handle_create_channel_subscription(tonic::Request::new(
                proto::CreateChannelSubscriptionRequest {
                    ns: "acme".to_string(),
                    channel: "incident-1".to_string(),
                    subscription: Some(subscription("primary", "", "", "analyst", "Mention")),
                },
            ))
            .await
            .expect("subscription create should succeed")
            .into_inner()
            .subscription
            .expect("subscription response should include subscription");
        assert_eq!(create_subscription.trigger, "mention");
        assert_eq!(
            create_subscription
                .context_policy
                .as_ref()
                .map(|policy| policy.max_messages),
            Some(20)
        );

        let invalid = handler
            .handle_create_channel_subscription(tonic::Request::new(
                proto::CreateChannelSubscriptionRequest {
                    ns: "acme".to_string(),
                    channel: "incident-1".to_string(),
                    subscription: Some(subscription("bad", "", "", "missing", "mention")),
                },
            ))
            .await
            .expect_err("missing agent should fail");
        assert_eq!(invalid.code(), tonic::Code::NotFound);

        let listed_subscriptions = handler
            .handle_list_channel_subscriptions(tonic::Request::new(
                proto::ListChannelSubscriptionsRequest {
                    ns: "acme".to_string(),
                    channel: "incident-1".to_string(),
                },
            ))
            .await
            .expect("subscription list should succeed")
            .into_inner();
        assert_eq!(listed_subscriptions.subscriptions.len(), 1);
        assert_eq!(listed_subscriptions.subscriptions[0].name, "primary");

        handler
            .handle_delete_channel(tonic::Request::new(proto::DeleteChannelRequest {
                ns: "acme".to_string(),
                name: "incident-1".to_string(),
            }))
            .await
            .expect("channel delete should succeed");
        assert!(kv
            .get_msg::<models::Channel>(&keys::channel("acme", "incident-1"))
            .await
            .unwrap()
            .is_none());
        assert!(kv
            .get_msg::<models::ChannelSubscription>(&keys::channel_subscription(
                "acme",
                "incident-1",
                "primary",
            ))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn delete_channel_removes_large_message_history_in_pages() {
        let (handler, kv, _) = setup_handler();
        seed_channel(&kv, "acme", "incident-1").await;
        kv.set_msg(
            &keys::channel_subscription("acme", "incident-1", "primary"),
            &subscription("primary", "acme", "incident-1", "analyst", "mention"),
        )
        .await
        .unwrap();
        for index in 0..520 {
            seed_channel_message(
                &kv,
                "acme",
                "incident-1",
                &format!("message-{index:03}"),
                &format!("message {index}"),
            )
            .await;
        }

        handler
            .handle_delete_channel(tonic::Request::new(proto::DeleteChannelRequest {
                ns: "acme".to_string(),
                name: "incident-1".to_string(),
            }))
            .await
            .expect("channel delete should succeed");

        assert!(kv
            .list_entries(&keys::channel_message_prefix("acme", "incident-1"))
            .await
            .unwrap()
            .is_empty());
        assert!(kv
            .list_entries(&keys::channel_subscription_prefix("acme", "incident-1"))
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn post_channel_message_routes_matching_subscriptions_to_agent_sessions() {
        let (handler, kv, pubsub) = setup_handler();
        seed_channel(&kv, "acme", "incident-1").await;
        seed_agent(&kv, "acme", "analyst").await;
        seed_agent(&kv, "acme", "bot").await;
        kv.set_msg(
            &keys::channel_subscription("acme", "incident-1", "primary"),
            &subscription("primary", "acme", "incident-1", "analyst", "mention"),
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::channel_subscription("acme", "incident-1", "manual-bot"),
            &subscription("manual-bot", "acme", "incident-1", "bot", "manual"),
        )
        .await
        .unwrap();

        let response = handler
            .handle_post_channel_message(tonic::Request::new(proto::PostChannelMessageRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                author_kind: "user".to_string(),
                author: "sre".to_string(),
                content: "@analyst please investigate".to_string(),
                subscription_names: vec!["manual-bot".to_string()],
                labels: HashMap::new(),
            }))
            .await
            .expect("post should succeed")
            .into_inner();

        let channel_message = response.message.expect("message should be returned");
        assert_eq!(channel_message.content, "@analyst please investigate");
        assert_eq!(response.routed_sessions.len(), 2);
        assert_eq!(response.routed_sessions[0].subscription, "manual-bot");
        assert_eq!(response.routed_sessions[1].subscription, "primary");

        for routed in &response.routed_sessions {
            assert!(routed.error.is_empty());
            let session = kv
                .get_msg::<models::Session>(&keys::session(
                    "acme",
                    &routed.agent,
                    &routed.session_id,
                ))
                .await
                .unwrap()
                .expect("routed session should be persisted");
            assert_eq!(
                session.labels.get(LABEL_CHANNEL).map(String::as_str),
                Some("incident-1")
            );
            assert_eq!(
                session
                    .labels
                    .get(LABEL_CHANNEL_MESSAGE)
                    .map(String::as_str),
                Some(channel_message.id.as_str())
            );
            assert_eq!(
                session
                    .labels
                    .get(LABEL_CHANNEL_SUBSCRIPTION)
                    .map(String::as_str),
                Some(routed.subscription.as_str())
            );
            assert_eq!(
                session
                    .labels
                    .get(LABEL_CHANNEL_TRIGGER)
                    .map(String::as_str),
                Some(if routed.subscription == "manual-bot" {
                    "manual"
                } else {
                    "mention"
                })
            );

            let entries = kv
                .list_entries(&keys::session_message_prefix(
                    "acme",
                    &routed.agent,
                    &routed.session_id,
                ))
                .await
                .unwrap();
            assert_eq!(entries.len(), 1);
            let user_message = models::SessionMessage::decode(entries[0].1.as_slice()).unwrap();
            assert_eq!(
                user_message
                    .labels
                    .get(LABEL_MESSAGE_SOURCE)
                    .map(String::as_str),
                Some("channel")
            );
            let prompt = user_message
                .parts
                .iter()
                .map(|part| part.content.as_str())
                .collect::<String>();
            assert!(prompt.contains("Normal assistant text stays private"));
            assert!(prompt.contains("@analyst please investigate"));
        }

        let published = pubsub.published.lock().await;
        let channel_events = published
            .iter()
            .filter(|(topic, _)| topic == &topics::channel_events_topic("acme", "incident-1"))
            .map(|(_, bytes)| events::ChannelEvent::decode(bytes.as_slice()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            channel_events
                .iter()
                .filter(|event| event.kind == events::ChannelEventKind::MessageCreated as i32)
                .count(),
            1
        );
        assert_eq!(
            channel_events
                .iter()
                .filter(|event| event.kind == events::ChannelEventKind::SessionRouted as i32)
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn reply_mode_none_routes_without_channel_reply_prompt() {
        let (handler, kv, _) = setup_handler();
        seed_channel(&kv, "acme", "incident-1").await;
        seed_agent(&kv, "acme", "observer").await;
        let mut no_reply = subscription("primary", "acme", "incident-1", "observer", "mention");
        no_reply.reply_mode = "none".to_string();
        kv.set_msg(
            &keys::channel_subscription("acme", "incident-1", "primary"),
            &no_reply,
        )
        .await
        .unwrap();

        let response = handler
            .handle_post_channel_message(tonic::Request::new(proto::PostChannelMessageRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                author_kind: "user".to_string(),
                author: "sre".to_string(),
                content: "@observer watch this".to_string(),
                subscription_names: Vec::new(),
                labels: HashMap::new(),
            }))
            .await
            .expect("post should succeed")
            .into_inner();

        assert_eq!(response.routed_sessions.len(), 1);
        let routed = &response.routed_sessions[0];
        let session = kv
            .get_msg::<models::Session>(&keys::session("acme", &routed.agent, &routed.session_id))
            .await
            .unwrap()
            .expect("routed session should be persisted");
        assert_eq!(
            session
                .labels
                .get(LABEL_CHANNEL_REPLY_MODE)
                .map(String::as_str),
            Some("none")
        );

        let entries = kv
            .list_entries(&keys::session_message_prefix(
                "acme",
                &routed.agent,
                &routed.session_id,
            ))
            .await
            .unwrap();
        let user_message = models::SessionMessage::decode(entries[0].1.as_slice()).unwrap();
        let prompt = user_message
            .parts
            .iter()
            .map(|part| part.content.as_str())
            .collect::<String>();
        assert!(prompt.contains("replyMode none"));
        assert!(!prompt.contains("channel_publish"));
        assert!(!prompt.contains("channel_skip_reply"));
    }

    #[tokio::test]
    async fn post_channel_message_skips_corrupt_subscriptions() {
        let (handler, kv, _) = setup_handler();
        seed_channel(&kv, "acme", "incident-1").await;
        seed_agent(&kv, "acme", "analyst").await;
        kv.set(
            &keys::channel_subscription("acme", "incident-1", "corrupt"),
            b"not-a-protobuf",
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::channel_subscription("acme", "incident-1", "primary"),
            &subscription("primary", "acme", "incident-1", "analyst", "mention"),
        )
        .await
        .unwrap();

        let response = handler
            .handle_post_channel_message(tonic::Request::new(proto::PostChannelMessageRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                author_kind: "user".to_string(),
                author: "sre".to_string(),
                content: "@analyst please investigate".to_string(),
                subscription_names: Vec::new(),
                labels: HashMap::new(),
            }))
            .await
            .expect("post should skip corrupt subscription and still route")
            .into_inner();

        assert_eq!(response.routed_sessions.len(), 1);
        assert_eq!(response.routed_sessions[0].subscription, "primary");
        assert!(response.routed_sessions[0].error.is_empty());
    }

    #[tokio::test]
    async fn mention_trigger_matches_whole_agent_or_subscription_name() {
        let (handler, kv, _) = setup_handler();
        seed_channel(&kv, "acme", "incident-1").await;
        seed_agent(&kv, "acme", "bot").await;
        seed_agent(&kv, "acme", "bot-helper").await;
        kv.set_msg(
            &keys::channel_subscription("acme", "incident-1", "bot"),
            &subscription("bot", "acme", "incident-1", "bot", "mention"),
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::channel_subscription("acme", "incident-1", "helper"),
            &subscription("helper", "acme", "incident-1", "bot-helper", "mention"),
        )
        .await
        .unwrap();

        let response = handler
            .handle_post_channel_message(tonic::Request::new(proto::PostChannelMessageRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                author_kind: "user".to_string(),
                author: "sre".to_string(),
                content: "@bot-helper please investigate".to_string(),
                subscription_names: Vec::new(),
                labels: HashMap::new(),
            }))
            .await
            .expect("post should succeed")
            .into_inner();

        assert_eq!(response.routed_sessions.len(), 1);
        assert_eq!(response.routed_sessions[0].subscription, "helper");
        assert_eq!(response.routed_sessions[0].agent, "bot-helper");

        let email_response = handler
            .handle_post_channel_message(tonic::Request::new(proto::PostChannelMessageRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                author_kind: "user".to_string(),
                author: "sre".to_string(),
                content: "email support@bot.com before paging".to_string(),
                subscription_names: Vec::new(),
                labels: HashMap::new(),
            }))
            .await
            .expect("post should succeed")
            .into_inner();
        assert!(email_response.routed_sessions.is_empty());

        let boundary_response = handler
            .handle_post_channel_message(tonic::Request::new(proto::PostChannelMessageRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                author_kind: "user".to_string(),
                author: "sre".to_string(),
                content: "paging @bot, please investigate".to_string(),
                subscription_names: Vec::new(),
                labels: HashMap::new(),
            }))
            .await
            .expect("post should succeed")
            .into_inner();
        assert_eq!(boundary_response.routed_sessions.len(), 1);
        assert_eq!(boundary_response.routed_sessions[0].subscription, "bot");

        let unicode_suffix_response = handler
            .handle_post_channel_message(tonic::Request::new(proto::PostChannelMessageRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                author_kind: "user".to_string(),
                author: "sre".to_string(),
                content: "paging @botПривет should not match".to_string(),
                subscription_names: Vec::new(),
                labels: HashMap::new(),
            }))
            .await
            .expect("post should succeed")
            .into_inner();
        assert!(unicode_suffix_response.routed_sessions.is_empty());

        let unicode_prefix_response = handler
            .handle_post_channel_message(tonic::Request::new(proto::PostChannelMessageRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                author_kind: "user".to_string(),
                author: "sre".to_string(),
                content: "paging Привет@bot should not match".to_string(),
                subscription_names: Vec::new(),
                labels: HashMap::new(),
            }))
            .await
            .expect("post should succeed")
            .into_inner();
        assert!(unicode_prefix_response.routed_sessions.is_empty());
    }

    #[tokio::test]
    async fn channel_prompt_context_uses_created_at_order() {
        let (handler, kv, _) = setup_handler();
        seed_channel(&kv, "acme", "incident-1").await;
        seed_agent(&kv, "acme", "reviewer").await;
        let mut sub = subscription("reviewer", "acme", "incident-1", "reviewer", "manual");
        sub.context_policy = Some(models::ChannelContextPolicy {
            mode: "recent_public".to_string(),
            max_messages: 3,
        });
        kv.set_msg(
            &keys::channel_subscription("acme", "incident-1", "reviewer"),
            &sub,
        )
        .await
        .unwrap();
        seed_channel_message_at(&kv, "acme", "incident-1", "000-old", "old", 10).await;
        seed_channel_message_at(&kv, "acme", "incident-1", "010-mid", "mid", 20).await;
        seed_channel_message_at(&kv, "acme", "incident-1", "020-new", "new", 30).await;

        let response = handler
            .handle_post_channel_message(tonic::Request::new(proto::PostChannelMessageRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                author_kind: "user".to_string(),
                author: "sre".to_string(),
                content: "current".to_string(),
                subscription_names: vec!["reviewer".to_string()],
                labels: HashMap::new(),
            }))
            .await
            .expect("post should succeed")
            .into_inner();

        let routed = response
            .routed_sessions
            .first()
            .expect("manual subscription should route");
        let entries = kv
            .list_entries(&keys::session_message_prefix(
                "acme",
                &routed.agent,
                &routed.session_id,
            ))
            .await
            .unwrap();
        let user_message = models::SessionMessage::decode(entries[0].1.as_slice()).unwrap();
        let prompt = user_message
            .parts
            .iter()
            .map(|part| part.content.as_str())
            .collect::<String>();

        assert!(!prompt.contains("- user:tester: old"));
        assert!(prompt.contains("Triggering content:\ncurrent"));
        assert!(!prompt.contains("- user:sre: current"));
        let mid = prompt
            .find("- user:tester: mid")
            .expect("mid should be present");
        let new = prompt
            .find("- user:tester: new")
            .expect("new should be present");
        assert!(mid < new);
    }

    #[tokio::test]
    async fn agent_authored_channel_messages_route_to_other_agents_but_not_self() {
        let (handler, kv, _) = setup_handler();
        seed_channel(&kv, "acme", "incident-1").await;
        seed_agent(&kv, "acme", "analyst").await;
        kv.set_msg(
            &keys::channel_subscription("acme", "incident-1", "primary"),
            &subscription("primary", "acme", "incident-1", "analyst", "all"),
        )
        .await
        .unwrap();

        let self_response = handler
            .handle_post_channel_message(tonic::Request::new(proto::PostChannelMessageRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                author_kind: "agent".to_string(),
                author: "analyst".to_string(),
                content: "self update".to_string(),
                subscription_names: Vec::new(),
                labels: HashMap::new(),
            }))
            .await
            .expect("post should succeed")
            .into_inner();
        assert!(self_response.routed_sessions.is_empty());

        let other_agent_response = handler
            .handle_post_channel_message(tonic::Request::new(proto::PostChannelMessageRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                author_kind: "agent".to_string(),
                author: "scribe".to_string(),
                content: "agent update".to_string(),
                subscription_names: Vec::new(),
                labels: HashMap::new(),
            }))
            .await
            .expect("post should succeed")
            .into_inner();
        assert_eq!(other_agent_response.routed_sessions.len(), 1);
        assert_eq!(
            other_agent_response.routed_sessions[0].subscription,
            "primary"
        );
    }

    #[tokio::test]
    async fn list_channel_messages_paginates_like_session_messages() {
        let (handler, kv, _) = setup_handler();
        seed_channel(&kv, "acme", "incident-1").await;

        for index in 1..=3 {
            seed_channel_message(
                &kv,
                "acme",
                "incident-1",
                &format!("019f0000-0000-7000-8000-00000000000{index}"),
                &format!("message-{index}"),
            )
            .await;
        }

        let newest_page = handler
            .handle_list_channel_messages(tonic::Request::new(proto::ListChannelMessagesRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                limit: 0,
                page_size: 2,
                before_message_id: None,
            }))
            .await
            .expect("newest page should succeed")
            .into_inner();

        assert!(newest_page.has_more);
        assert_eq!(
            newest_page.next_before_message_id.as_deref(),
            Some("019f0000-0000-7000-8000-000000000002")
        );
        assert_eq!(
            newest_page
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            vec!["message-2", "message-3"]
        );

        let older_page = handler
            .handle_list_channel_messages(tonic::Request::new(proto::ListChannelMessagesRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                limit: 0,
                page_size: 2,
                before_message_id: Some("019f0000-0000-7000-8000-000000000002".to_string()),
            }))
            .await
            .expect("older page should succeed")
            .into_inner();

        assert!(!older_page.has_more);
        assert_eq!(
            older_page
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            vec!["message-1"]
        );
    }

    #[tokio::test]
    async fn list_channel_messages_preserves_legacy_limit_and_validates_page_size() {
        let (handler, kv, _) = setup_handler();
        seed_channel(&kv, "acme", "incident-1").await;
        for index in 1..=3 {
            seed_channel_message(
                &kv,
                "acme",
                "incident-1",
                &format!("019f0000-0000-7000-8000-00000000000{index}"),
                &format!("message-{index}"),
            )
            .await;
        }

        let legacy_page = handler
            .handle_list_channel_messages(tonic::Request::new(proto::ListChannelMessagesRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                limit: 2,
                page_size: 0,
                before_message_id: None,
            }))
            .await
            .expect("legacy limit page should succeed")
            .into_inner();

        assert!(legacy_page.has_more);
        assert_eq!(legacy_page.messages.len(), 2);
        assert_eq!(
            legacy_page
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            vec!["message-2", "message-3"]
        );

        let err = handler
            .handle_list_channel_messages(tonic::Request::new(proto::ListChannelMessagesRequest {
                ns: "acme".to_string(),
                channel: "incident-1".to_string(),
                limit: 0,
                page_size: -1,
                before_message_id: None,
            }))
            .await
            .expect_err("negative page size should fail");
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn channel_publish_and_skip_are_bound_to_channel_sessions() {
        let (handler, kv, pubsub) = setup_handler();
        seed_channel(&kv, "acme", "incident-1").await;
        seed_agent(&kv, "acme", "analyst").await;
        let session_id = crate::scheduling::create_session_with_labels(
            &control_plane(kv.clone(), pubsub.clone()),
            "acme",
            "analyst",
            HashMap::from([
                (LABEL_CHANNEL.to_string(), "incident-1".to_string()),
                (LABEL_CHANNEL_MESSAGE.to_string(), "source-msg".to_string()),
                (
                    LABEL_CHANNEL_SUBSCRIPTION.to_string(),
                    "primary".to_string(),
                ),
            ]),
        )
        .await
        .expect("session create should succeed");

        let cp = ControlPlane {
            kv: handler.gateway.kv.clone(),
            pubsub: handler.gateway.pubsub.clone(),
            scheduler: handler.gateway.scheduler.clone(),
        };

        let published_message =
            crate::gateway::rpc::channels::publish_channel_message_from_session(
                &cp,
                "acme",
                "analyst",
                &session_id,
                " public answer ",
            )
            .await
            .expect("channel publish should succeed");
        assert_eq!(published_message.author_kind, "agent");
        assert_eq!(published_message.content, "public answer");
        assert_eq!(
            published_message
                .labels
                .get(LABEL_MESSAGE_SOURCE)
                .map(String::as_str),
            Some("channel.publish")
        );

        crate::gateway::rpc::channels::skip_channel_reply_from_session(
            &cp,
            "acme",
            "analyst",
            &session_id,
            "no public answer needed",
        )
        .await
        .expect("channel skip should succeed");

        let unlinked_session = crate::scheduling::create_session(
            &control_plane(kv.clone(), pubsub.clone()),
            "acme",
            "analyst",
        )
        .await
        .expect("unlinked session create should succeed");
        let err = crate::gateway::rpc::channels::publish_channel_message_from_session(
            &cp,
            "acme",
            "analyst",
            &unlinked_session,
            "not allowed",
        )
        .await
        .expect_err("unlinked session should fail");
        assert!(err.to_string().contains("not linked to a channel"));

        let no_reply_session = crate::scheduling::create_session_with_labels(
            &control_plane(kv.clone(), pubsub.clone()),
            "acme",
            "analyst",
            HashMap::from([
                (LABEL_CHANNEL.to_string(), "incident-1".to_string()),
                (LABEL_CHANNEL_REPLY_MODE.to_string(), "none".to_string()),
            ]),
        )
        .await
        .expect("no-reply session create should succeed");
        let err = crate::gateway::rpc::channels::publish_channel_message_from_session(
            &cp,
            "acme",
            "analyst",
            &no_reply_session,
            "not allowed",
        )
        .await
        .expect_err("no-reply session should not publish");
        assert!(err.to_string().contains("replies are disabled"));
        let err = crate::gateway::rpc::channels::skip_channel_reply_from_session(
            &cp,
            "acme",
            "analyst",
            &no_reply_session,
            "not needed",
        )
        .await
        .expect_err("no-reply session should not skip");
        assert!(err.to_string().contains("replies are disabled"));

        let channel_events = pubsub
            .published
            .lock()
            .await
            .iter()
            .filter(|(topic, _)| topic == &topics::channel_events_topic("acme", "incident-1"))
            .map(|(_, bytes)| events::ChannelEvent::decode(bytes.as_slice()).unwrap())
            .collect::<Vec<_>>();
        assert!(channel_events.iter().any(|event| event.kind
            == events::ChannelEventKind::MessageCreated as i32
            && event.session_id == session_id));
        assert!(channel_events.iter().any(|event| event.kind
            == events::ChannelEventKind::PublishSkipped as i32
            && event.error == "no public answer needed"));
    }
}
