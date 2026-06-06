// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[cfg(test)]
mod tests {
    use crate::control::{
        events::{
            LifecycleEvent, SessionControlEvent, SessionMessagePartEvent,
            SessionMessagePartEventKind,
        },
        scheduler::SchedulerBackend,
        topics,
    };
    use crate::gateway::rpc::proto::gateway_service_server::GatewayService;
    use crate::gateway::rpc::{manifests, models, proto, GrpcGatewayHandler};
    use crate::gateway::server::Gateway;
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use futures::StreamExt;
    use prost::Message;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn session_part(
        session_id: &str,
        agent: &str,
        ns: &str,
        message_id: &str,
        kind: SessionMessagePartEventKind,
        part_type: models::SessionMessagePartType,
        content: impl Into<String>,
        name: impl Into<String>,
        payload_json: impl Into<String>,
        timestamp: i64,
    ) -> SessionMessagePartEvent {
        SessionMessagePartEvent {
            session_id: session_id.to_string(),
            kind: kind as i32,
            part: Some(models::SessionMessagePart {
                id: String::new(),
                part_type: part_type as i32,
                content: content.into(),
                name: name.into(),
                payload_json: payload_json.into(),
                created_at: timestamp,
            }),
            timestamp,
            agent: agent.to_string(),
            ns: ns.to_string(),
            message_id: message_id.to_string(),
        }
    }

    #[derive(Default)]
    struct RecordingScheduler {
        scheduled: Mutex<Vec<crate::control::scheduler::ScheduleWakeupRequest>>,
        canceled: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl SchedulerBackend for RecordingScheduler {
        async fn schedule(
            &self,
            req: crate::control::scheduler::ScheduleWakeupRequest,
        ) -> anyhow::Result<crate::control::scheduler::ScheduledWakeup> {
            self.scheduled.lock().await.push(req);
            Ok(crate::control::scheduler::ScheduledWakeup {
                handle: Some("handle-1".to_string()),
                armed: true,
            })
        }

        async fn cancel(&self, handle: &str) -> anyhow::Result<()> {
            self.canceled.lock().await.push(handle.to_string());
            Ok(())
        }
    }

    fn setup_handler() -> (
        GrpcGatewayHandler,
        Arc<MockKvStore>,
        Arc<RecordingScheduler>,
        Arc<RecordingPubSub>,
    ) {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(RecordingScheduler::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let gateway = Arc::new(Gateway::new(
            None,
            kv.clone(),
            pubsub.clone(),
            scheduler.clone(),
        ));
        (GrpcGatewayHandler { gateway }, kv, scheduler, pubsub)
    }

    fn custom_agent_definition(model_name: &str) -> manifests::AgentDefinition {
        manifests::AgentDefinition {
            source: Some(manifests::agent_definition::Source::CustomSpec(
                manifests::AgentSpec {
                    features: Vec::new(),
                    model_policy: Some(manifests::ModelPolicy {
                        profiles: vec![manifests::ModelProfile {
                            name: "default".to_string(),
                            model: Some(manifests::Model {
                                provider: "mock".to_string(),
                                name: model_name.to_string(),
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

    fn metadata(name: &str, namespace: &str) -> manifests::ObjectMeta {
        manifests::ObjectMeta {
            name: name.to_string(),
            namespace: namespace.to_string(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
        }
    }

    fn knowledge_manifest(
        name: &str,
        namespace: &str,
        path: &str,
        content: &str,
    ) -> manifests::Knowledge {
        manifests::Knowledge {
            api_version: String::new(),
            kind: String::new(),
            metadata: Some(metadata(name, namespace)),
            spec: Some(manifests::KnowledgeSpec {
                path: path.to_string(),
                content: content.to_string(),
            }),
        }
    }

    fn agent_template(name: &str, namespace: &str) -> manifests::AgentTemplate {
        manifests::AgentTemplate {
            api_version: String::new(),
            kind: String::new(),
            metadata: Some(metadata(name, namespace)),
            definition: Some(custom_agent_definition("gpt-5")),
        }
    }

    fn mcp_server(name: &str, namespace: &str) -> manifests::McpServer {
        manifests::McpServer {
            api_version: String::new(),
            kind: String::new(),
            metadata: Some(metadata(name, namespace)),
            spec: Some(manifests::McpServerSpec {
                transport: "http".to_string(),
                target: "https://mcp.example.com".to_string(),
                args: vec![],
                headers: HashMap::new(),
                disabled: false,
            }),
        }
    }

    fn mcp_binding(name: &str, namespace: &str, server_ref: &str) -> manifests::McpServerBinding {
        manifests::McpServerBinding {
            api_version: String::new(),
            kind: String::new(),
            metadata: Some(metadata(name, namespace)),
            spec: Some(manifests::McpServerBindingSpec {
                server_ref: server_ref.to_string(),
                args: Vec::new(),
                headers: HashMap::new(),
                disabled: false,
                auth_broker: None,
                allowed_tool_names: vec!["search".to_string()],
            }),
        }
    }

    fn schedule(name: &str, namespace: &str, agent: &str) -> models::Schedule {
        models::Schedule {
            name: name.to_string(),
            ns: namespace.to_string(),
            spec: Some(models::ScheduleSpec {
                kind: "every".to_string(),
                cron: String::new(),
                interval_seconds: 300,
                run_at: String::new(),
                timezone: "UTC".to_string(),
                target: Some(models::ScheduleTarget {
                    agent: agent.to_string(),
                    session_mode: "new".to_string(),
                    session_id: String::new(),
                }),
                input_message: "check in".to_string(),
                enabled: true,
            }),
            status: None,
            labels: HashMap::from([("team".to_string(), "ops".to_string())]),
        }
    }

    #[tokio::test]
    async fn gateway_service_forwards_resource_crud_methods() {
        let (handler, _kv, _scheduler, _pubsub) = setup_handler();

        let namespace = handler
            .create_namespace(tonic::Request::new(proto::CreateNamespaceRequest {
                name: "acme".to_string(),
                recursive: false,
                labels: HashMap::from([("tier".to_string(), "prod".to_string())]),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(namespace.name, "acme");

        let get_namespace = handler
            .get_namespace(tonic::Request::new(proto::GetNamespaceRequest {
                name: "acme".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(get_namespace.name, "acme");

        let listed_namespaces = handler
            .list_namespaces(tonic::Request::new(proto::ListNamespacesRequest {
                parent: None,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(listed_namespaces
            .namespaces
            .iter()
            .any(|entry| entry.name == "acme"));

        let agent = handler
            .create_agent(tonic::Request::new(proto::CreateAgentRequest {
                ns: "acme".to_string(),
                name: Some("agent-1".to_string()),
                definition: Some(custom_agent_definition("gpt-5")),
                labels: HashMap::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(agent.agent, "agent-1");

        let fetched_agent = handler
            .get_agent(tonic::Request::new(proto::GetAgentRequest {
                ns: "acme".to_string(),
                name: "agent-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            fetched_agent
                .agent
                .as_ref()
                .map(|agent| agent.name.as_str()),
            Some("agent-1")
        );

        let modified_agent = handler
            .modify_agent(tonic::Request::new(proto::ModifyAgentRequest {
                ns: "acme".to_string(),
                agent: "agent-1".to_string(),
                definition: Some(custom_agent_definition("gpt-5-mini")),
                labels: HashMap::from([("team".to_string(), "platform".to_string())]),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            modified_agent.labels.get("team").map(String::as_str),
            Some("platform")
        );

        let listed_agents = handler
            .list_agents(tonic::Request::new(proto::ListAgentsRequest {
                ns: "acme".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(listed_agents.agents.iter().any(|entry| entry == "agent-1"));

        let template = handler
            .create_agent_template(tonic::Request::new(proto::CreateAgentTemplateRequest {
                template: Some(agent_template(
                    "template-1",
                    crate::control::ns::TALON_SYSTEM,
                )),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            template
                .template
                .as_ref()
                .and_then(|t| t.metadata.as_ref())
                .map(|m| m.name.as_str()),
            Some("template-1")
        );

        assert_eq!(
            handler
                .get_agent_template(tonic::Request::new(proto::GetAgentTemplateRequest {
                    name: "template-1".to_string(),
                }))
                .await
                .unwrap()
                .into_inner()
                .template
                .as_ref()
                .and_then(|t| t.metadata.as_ref())
                .map(|m| m.name.as_str()),
            Some("template-1")
        );
        assert_eq!(
            handler
                .list_agent_templates(tonic::Request::new(proto::ListAgentTemplatesRequest {}))
                .await
                .unwrap()
                .into_inner()
                .templates
                .len(),
            1
        );
        assert!(
            handler
                .delete_agent_template(tonic::Request::new(proto::DeleteAgentTemplateRequest {
                    name: "template-1".to_string(),
                }))
                .await
                .unwrap()
                .into_inner()
                .success
        );

        let server = handler
            .create_mcp_server(tonic::Request::new(proto::CreateMcpServerRequest {
                server: Some(mcp_server("server-1", "")),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            server
                .server
                .as_ref()
                .and_then(|entry| entry.metadata.as_ref())
                .map(|m| m.name.as_str()),
            Some("server-1")
        );
        assert_eq!(
            handler
                .get_mcp_server(tonic::Request::new(proto::GetMcpServerRequest {
                    name: "server-1".to_string(),
                }))
                .await
                .unwrap()
                .into_inner()
                .server
                .as_ref()
                .and_then(|entry| entry.metadata.as_ref())
                .map(|m| m.name.as_str()),
            Some("server-1")
        );
        assert_eq!(
            handler
                .list_mcp_servers(tonic::Request::new(proto::ListMcpServersRequest {}))
                .await
                .unwrap()
                .into_inner()
                .servers
                .len(),
            1
        );

        let binding = handler
            .create_mcp_server_binding(tonic::Request::new(proto::CreateMcpServerBindingRequest {
                ns: "acme".to_string(),
                binding: Some(mcp_binding("binding-1", "acme", "server-1")),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            binding
                .binding
                .as_ref()
                .and_then(|entry| entry.metadata.as_ref())
                .map(|m| m.name.as_str()),
            Some("binding-1")
        );
        assert_eq!(
            handler
                .get_mcp_server_binding(tonic::Request::new(proto::GetMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    name: "binding-1".to_string(),
                },))
                .await
                .unwrap()
                .into_inner()
                .binding
                .as_ref()
                .and_then(|entry| entry.metadata.as_ref())
                .map(|m| m.name.as_str()),
            Some("binding-1")
        );
        assert_eq!(
            handler
                .list_mcp_server_bindings(tonic::Request::new(
                    proto::ListMcpServerBindingsRequest {
                        ns: "acme".to_string(),
                    },
                ))
                .await
                .unwrap()
                .into_inner()
                .bindings
                .len(),
            1
        );

        let created_knowledge = handler
            .create_namespace_knowledge(tonic::Request::new(
                proto::CreateNamespaceKnowledgeRequest {
                    ns: "acme".to_string(),
                    knowledge: Some(knowledge_manifest(
                        "guide",
                        "acme",
                        "guide.md",
                        "rust systems guide",
                    )),
                },
            ))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            created_knowledge
                .knowledge
                .as_ref()
                .and_then(|entry| entry.metadata.as_ref())
                .map(|m| m.name.as_str()),
            Some("guide")
        );

        assert_eq!(
            handler
                .get_namespace_knowledge(tonic::Request::new(proto::GetNamespaceKnowledgeRequest {
                    ns: "acme".to_string(),
                    name: "guide".to_string(),
                },))
                .await
                .unwrap()
                .into_inner()
                .knowledge
                .as_ref()
                .and_then(|entry| entry.spec.as_ref())
                .map(|spec| spec.content.as_str()),
            Some("rust systems guide")
        );
        assert_eq!(
            handler
                .list_namespace_knowledge(tonic::Request::new(
                    proto::ListNamespaceKnowledgeRequest {
                        ns: "acme".to_string(),
                    },
                ))
                .await
                .unwrap()
                .into_inner()
                .knowledge
                .len(),
            1
        );
        assert_eq!(
            handler
                .get_knowledge(tonic::Request::new(proto::GetKnowledgeRequest {
                    agent: "agent-1".to_string(),
                    ns: "acme".to_string(),
                    path: Some("guide.md".to_string()),
                }))
                .await
                .unwrap()
                .into_inner()
                .modules
                .len(),
            1
        );
        assert_eq!(
            handler
                .search_knowledge(tonic::Request::new(proto::SearchKnowledgeRequest {
                    agent: "agent-1".to_string(),
                    ns: "acme".to_string(),
                    query: "systems".to_string(),
                }))
                .await
                .unwrap()
                .into_inner()
                .results
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn gateway_service_forwards_session_and_schedule_methods() {
        let (handler, _kv, scheduler, pubsub) = setup_handler();

        handler
            .create_namespace(tonic::Request::new(proto::CreateNamespaceRequest {
                name: "acme".to_string(),
                recursive: false,
                labels: HashMap::new(),
            }))
            .await
            .unwrap();
        handler
            .create_agent(tonic::Request::new(proto::CreateAgentRequest {
                ns: "acme".to_string(),
                name: Some("agent-1".to_string()),
                definition: Some(custom_agent_definition("gpt-5")),
                labels: HashMap::new(),
            }))
            .await
            .unwrap();
        handler
            .create_mcp_server(tonic::Request::new(proto::CreateMcpServerRequest {
                server: Some(mcp_server("server-1", "")),
            }))
            .await
            .unwrap();
        handler
            .create_mcp_server_binding(tonic::Request::new(proto::CreateMcpServerBindingRequest {
                ns: "acme".to_string(),
                binding: Some(mcp_binding("binding-1", "acme", "server-1")),
            }))
            .await
            .unwrap();
        handler
            .create_namespace_knowledge(tonic::Request::new(
                proto::CreateNamespaceKnowledgeRequest {
                    ns: "acme".to_string(),
                    knowledge: Some(knowledge_manifest(
                        "guide",
                        "acme",
                        "guide.md",
                        "rust systems guide",
                    )),
                },
            ))
            .await
            .unwrap();

        let created_session = handler
            .create_session(tonic::Request::new(proto::CreateSessionRequest {
                agent: "agent-1".to_string(),
                ns: "acme".to_string(),
                labels: HashMap::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        let session_id = created_session.session_id.clone();
        let session_topic =
            topics::session_part_topic_for_shard(topics::session_part_shard(&session_id));
        pubsub.streams.lock().await.insert(
            session_topic,
            vec![
                session_part(
                    &session_id,
                    "agent-1",
                    "acme",
                    "msg-1",
                    SessionMessagePartEventKind::Delta,
                    models::SessionMessagePartType::ToolCall,
                    "",
                    "search",
                    r#"{"tool_call_id":"call-1","input":{"q":"rust"}}"#,
                    1,
                )
                .encode_to_vec(),
                session_part(
                    &session_id,
                    "agent-1",
                    "acme",
                    "msg-1",
                    SessionMessagePartEventKind::Done,
                    models::SessionMessagePartType::Text,
                    "",
                    "",
                    "",
                    2,
                )
                .encode_to_vec(),
            ],
        );

        assert_eq!(
            handler
                .get_session(tonic::Request::new(proto::GetSessionRequest {
                    session_id: session_id.clone(),
                    agent: "agent-1".to_string(),
                    ns: "acme".to_string(),
                    message_limit: 0,
                }))
                .await
                .unwrap()
                .into_inner()
                .session_id,
            session_id
        );
        assert_eq!(
            handler
                .list_sessions(tonic::Request::new(proto::ListSessionsRequest {
                    agent: "agent-1".to_string(),
                    ns: "acme".to_string(),
                }))
                .await
                .unwrap()
                .into_inner()
                .sessions
                .len(),
            1
        );
        assert_eq!(
            handler
                .send_message(tonic::Request::new(proto::SendMessageRequest {
                    session_id: session_id.clone(),
                    agent: "agent-1".to_string(),
                    ns: "acme".to_string(),
                    message: "hello".to_string(),
                    labels: HashMap::new(),
                }))
                .await
                .unwrap()
                .into_inner()
                .session_id,
            session_id
        );
        assert_eq!(
            handler
                .append_session_message(tonic::Request::new(proto::AppendSessionMessageRequest {
                    session_id: session_id.clone(),
                    agent: "agent-1".to_string(),
                    ns: "acme".to_string(),
                    message: Some(models::SessionMessage {
                        id: "manual-message".to_string(),
                        role: models::MessageRole::RoleSystem as i32,
                        created_at: 0,
                        labels: HashMap::new(),
                        parts: vec![models::SessionMessagePart {
                            id: String::new(),
                            part_type: models::SessionMessagePartType::Text as i32,
                            content: "manual note".to_string(),
                            name: String::new(),
                            payload_json: String::new(),
                            created_at: 0,
                        }],
                    }),
                }))
                .await
                .unwrap()
                .into_inner()
                .message
                .and_then(|message| message.parts.into_iter().next())
                .map(|part| part.content),
            Some("manual note".to_string())
        );
        assert!(
            handler
                .stop_session_generation(tonic::Request::new(proto::StopSessionGenerationRequest {
                    session_id: session_id.clone(),
                    agent: "agent-1".to_string(),
                    ns: "acme".to_string(),
                },))
                .await
                .unwrap()
                .into_inner()
                .success
        );
        let mut part_stream = handler
            .stream_session_parts(tonic::Request::new(proto::StreamSessionPartsRequest {
                session_id: session_id.clone(),
                agent: "agent-1".to_string(),
                ns: "acme".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            part_stream
                .next()
                .await
                .unwrap()
                .unwrap()
                .part
                .unwrap()
                .part_type,
            models::SessionMessagePartType::ToolCall as i32
        );
        assert_eq!(
            part_stream.next().await.unwrap().unwrap().kind,
            SessionMessagePartEventKind::Done as i32
        );

        let created_schedule = handler
            .create_schedule(tonic::Request::new(proto::CreateScheduleRequest {
                ns: "acme".to_string(),
                schedule: Some(schedule("schedule-1", "acme", "agent-1")),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            created_schedule
                .schedule
                .as_ref()
                .map(|entry| entry.name.as_str()),
            Some("schedule-1")
        );
        assert_eq!(scheduler.scheduled.lock().await.len(), 1);
        assert_eq!(
            handler
                .get_schedule(tonic::Request::new(proto::GetScheduleRequest {
                    ns: "acme".to_string(),
                    name: "schedule-1".to_string(),
                }))
                .await
                .unwrap()
                .into_inner()
                .schedule
                .as_ref()
                .map(|entry| entry.name.as_str()),
            Some("schedule-1")
        );
        assert_eq!(
            handler
                .modify_schedule(tonic::Request::new(proto::ModifyScheduleRequest {
                    ns: "acme".to_string(),
                    name: "schedule-1".to_string(),
                    schedule: Some(schedule("ignored", "ignored", "agent-1")),
                }))
                .await
                .unwrap()
                .into_inner()
                .schedule
                .as_ref()
                .map(|entry| entry.name.as_str()),
            Some("schedule-1")
        );
        assert_eq!(
            handler
                .list_schedules(tonic::Request::new(proto::ListSchedulesRequest {
                    ns: "acme".to_string(),
                }))
                .await
                .unwrap()
                .into_inner()
                .schedules
                .len(),
            1
        );
        assert!(
            handler
                .delete_schedule(tonic::Request::new(proto::DeleteScheduleRequest {
                    ns: "acme".to_string(),
                    name: "schedule-1".to_string(),
                }))
                .await
                .unwrap()
                .into_inner()
                .success
        );
        let canceled = scheduler.canceled.lock().await.clone();
        assert!(!canceled.is_empty());
        assert!(canceled.iter().all(|handle| handle == "handle-1"));

        assert!(
            handler
                .delete_session(tonic::Request::new(proto::DeleteSessionRequest {
                    session_id: session_id.clone(),
                    agent: "agent-1".to_string(),
                    ns: "acme".to_string(),
                }))
                .await
                .unwrap()
                .into_inner()
                .success
        );
        assert!(
            handler
                .delete_namespace_knowledge(tonic::Request::new(
                    proto::DeleteNamespaceKnowledgeRequest {
                        ns: "acme".to_string(),
                        name: "guide".to_string(),
                    },
                ))
                .await
                .unwrap()
                .into_inner()
                .success
        );
        assert!(
            handler
                .delete_mcp_server_binding(tonic::Request::new(
                    proto::DeleteMcpServerBindingRequest {
                        ns: "acme".to_string(),
                        name: "binding-1".to_string(),
                    },
                ))
                .await
                .unwrap()
                .into_inner()
                .success
        );
        assert!(
            handler
                .delete_mcp_server(tonic::Request::new(proto::DeleteMcpServerRequest {
                    name: "server-1".to_string(),
                }))
                .await
                .unwrap()
                .into_inner()
                .success
        );
        let deleted_namespace = handler
            .delete_namespace(tonic::Request::new(proto::DeleteNamespaceRequest {
                name: "acme".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(deleted_namespace.name, "acme");

        let published = pubsub.published.lock().await;
        assert!(published.iter().any(|(topic, payload)| {
            topic == topics::RESOURCE_LIFECYCLE_TOPIC
                && LifecycleEvent::decode(payload.as_slice())
                    .map(|event| event.resource_type == "Session")
                    .unwrap_or(false)
        }));
        assert!(published.iter().any(|(topic, payload)| {
            topic == topics::SESSION_CONTROL_TOPIC
                && SessionControlEvent::decode(payload.as_slice())
                    .map(|event| event.action == "stop_generation")
                    .unwrap_or(false)
        }));
    }

    #[tokio::test]
    async fn gateway_service_delete_schedule_succeeds_without_existing_record() {
        let (handler, _kv, scheduler, _pubsub) = setup_handler();

        let response = handler
            .delete_schedule(tonic::Request::new(proto::DeleteScheduleRequest {
                ns: "acme".to_string(),
                name: "missing".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(response.success);
        assert!(scheduler.canceled.lock().await.is_empty());
    }
}
