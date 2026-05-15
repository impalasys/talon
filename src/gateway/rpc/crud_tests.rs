// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[cfg(test)]
mod tests {
    use crate::control::{
        events,
        keys,
        ns,
        scheduler::NoopSchedulerBackend,
        topics,
        KeyValueStore,
        MessagePublisher,
        ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::{manifests, models, proto, GrpcGatewayHandler};
    use crate::gateway::{server::Gateway, session_streams::SessionStreamHub};
    use futures::stream;
    use prost::Message;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockKvStore {
        data: Mutex<HashMap<(String, String), Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, ns: &str, k: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self
                .data
                .lock()
                .await
                .get(&(ns.to_string(), k.to_string()))
                .cloned())
        }

        async fn set(&self, ns: &str, k: &str, v: &[u8]) -> anyhow::Result<()> {
            self.data
                .lock()
                .await
                .insert((ns.to_string(), k.to_string()), v.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            ns: &str,
            k: &str,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let key = (ns.to_string(), k.to_string());
            let current = data.get(&key).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(key, value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, ns: &str, k: &str) -> anyhow::Result<()> {
            self.data
                .lock()
                .await
                .remove(&(ns.to_string(), k.to_string()));
            Ok(())
        }

        async fn list_keys(&self, ns: &str, prefix: &str) -> anyhow::Result<Vec<String>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|(stored_ns, key)| {
                    (stored_ns == ns && key.starts_with(prefix)).then(|| key.clone())
                })
                .collect::<Vec<_>>();
            keys.sort();
            Ok(keys)
        }
    }

    struct MockPubSub {
        published: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    }

    #[async_trait::async_trait]
    impl MessagePublisher for MockPubSub {
        async fn publish(&self, topic: &str, message: &[u8]) -> anyhow::Result<()> {
            self.published
                .lock()
                .await
                .push((topic.to_string(), message.to_vec()));
            Ok(())
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            Ok(Box::pin(stream::empty()))
        }
    }

    fn setup_handler() -> (
        GrpcGatewayHandler,
        Arc<MockKvStore>,
        Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    ) {
        let kv = Arc::new(MockKvStore::default());
        let published = Arc::new(Mutex::new(Vec::new()));
        let pubsub = Arc::new(MockPubSub {
            published: published.clone(),
        });
        let gateway = Arc::new(Gateway {
            auth_config: None,
            kv: kv.clone(),
            pubsub: pubsub.clone(),
            scheduler: Arc::new(NoopSchedulerBackend),
            session_streams: Arc::new(SessionStreamHub::new(pubsub)),
        });
        (GrpcGatewayHandler { gateway }, kv, published)
    }

    async fn seed_namespace(kv: &Arc<MockKvStore>, namespace: &str) {
        kv.set_msg(
            "talon-system:ns",
            &format!("Namespace/{namespace}"),
            &models::Namespace {
                name: namespace.to_string(),
                parent: String::new(),
                is_deleted: false,
                deleted_at: 0,
                labels: HashMap::new(),
            },
        )
        .await
        .expect("namespace seed should succeed");
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
                                temperature: 0.7,
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

    fn agent_template(name: &str, namespace: &str) -> manifests::AgentTemplate {
        manifests::AgentTemplate {
            api_version: String::new(),
            kind: String::new(),
            metadata: Some(metadata(name, namespace)),
            definition: Some(custom_agent_definition("gpt-5")),
        }
    }

    fn mcp_server(name: &str, namespace: &str, transport: &str) -> manifests::McpServer {
        manifests::McpServer {
            api_version: String::new(),
            kind: String::new(),
            metadata: Some(metadata(name, namespace)),
            spec: Some(manifests::McpServerSpec {
                transport: transport.to_string(),
                target: "https://mcp.example.com".to_string(),
                args: vec!["--json".to_string()],
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

    #[tokio::test]
    async fn create_get_modify_and_list_agent_round_trip() {
        let (handler, kv, published) = setup_handler();
        seed_namespace(&kv, "acme").await;

        let create = handler
            .handle_create_agent(tonic::Request::new(proto::CreateAgentRequest {
                ns: "acme".to_string(),
                name: Some("agent-1".to_string()),
                definition: Some(custom_agent_definition("gpt-5")),
                labels: HashMap::from([("team".to_string(), "sales".to_string())]),
            }))
            .await
            .expect("create should succeed")
            .into_inner();
        assert_eq!(create.agent, "agent-1");

        let stored = kv
            .get_msg::<models::Agent>("acme", &keys::agent("agent-1"))
            .await
            .expect("agent lookup should succeed")
            .expect("agent should be persisted");
        assert_eq!(stored.labels.get("team").map(String::as_str), Some("sales"));

        let got = handler
            .handle_get_agent(tonic::Request::new(proto::GetAgentRequest {
                ns: "acme".to_string(),
                name: "agent-1".to_string(),
            }))
            .await
            .expect("get should succeed")
            .into_inner();
        assert_eq!(got.agent.as_ref().map(|agent| agent.name.as_str()), Some("agent-1"));

        let modified = handler
            .handle_modify_agent(tonic::Request::new(proto::ModifyAgentRequest {
                agent: "agent-1".to_string(),
                ns: "acme".to_string(),
                definition: Some(custom_agent_definition("gpt-5-mini")),
                labels: HashMap::from([("tier".to_string(), "gold".to_string())]),
            }))
            .await
            .expect("modify should succeed")
            .into_inner();
        assert_eq!(modified.labels.get("tier").map(String::as_str), Some("gold"));

        let listed = handler
            .handle_list_agents(tonic::Request::new(proto::ListAgentsRequest {
                ns: "acme".to_string(),
            }))
            .await
            .expect("list should succeed")
            .into_inner();
        assert_eq!(listed.agents, vec!["agent-1".to_string()]);

        let published = published.lock().await;
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, topics::RESOURCE_LIFECYCLE_TOPIC);
        let event = events::LifecycleEvent::decode(published[0].1.as_slice())
            .expect("event should decode");
        assert_eq!(event.resource_type, "Agent");
        assert_eq!(event.name, "agent-1");
    }

    #[tokio::test]
    async fn create_agent_rejects_invalid_name_and_missing_namespace() {
        let (handler, kv, _) = setup_handler();
        seed_namespace(&kv, "acme").await;

        let invalid_name = handler
            .handle_create_agent(tonic::Request::new(proto::CreateAgentRequest {
                ns: "acme".to_string(),
                name: Some("Bad_Name".to_string()),
                definition: Some(custom_agent_definition("gpt-5")),
                labels: HashMap::new(),
            }))
            .await
            .expect_err("invalid name should fail");
        assert_eq!(invalid_name.code(), tonic::Code::InvalidArgument);

        let missing_namespace = handler
            .handle_create_agent(tonic::Request::new(proto::CreateAgentRequest {
                ns: "ghost".to_string(),
                name: Some("agent-2".to_string()),
                definition: Some(custom_agent_definition("gpt-5")),
                labels: HashMap::new(),
            }))
            .await
            .expect_err("missing namespace should fail");
        assert_eq!(missing_namespace.code(), tonic::Code::FailedPrecondition);

        let empty_namespace = handler
            .handle_create_agent(tonic::Request::new(proto::CreateAgentRequest {
                ns: String::new(),
                name: Some("agent-2".to_string()),
                definition: Some(custom_agent_definition("gpt-5")),
                labels: HashMap::new(),
            }))
            .await
            .expect_err("empty namespace should fail");
        assert_eq!(empty_namespace.code(), tonic::Code::InvalidArgument);

        kv.set_msg(
            "talon-system:ns",
            "Namespace/deleted",
            &models::Namespace {
                name: "deleted".to_string(),
                parent: String::new(),
                is_deleted: true,
                deleted_at: 1,
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        let deleted_namespace = handler
            .handle_create_agent(tonic::Request::new(proto::CreateAgentRequest {
                ns: "deleted".to_string(),
                name: Some("agent-2".to_string()),
                definition: Some(custom_agent_definition("gpt-5")),
                labels: HashMap::new(),
            }))
            .await
            .expect_err("deleted namespace should fail");
        assert_eq!(deleted_namespace.code(), tonic::Code::FailedPrecondition);

        let missing_definition = handler
            .handle_create_agent(tonic::Request::new(proto::CreateAgentRequest {
                ns: "acme".to_string(),
                name: Some("agent-2".to_string()),
                definition: None,
                labels: HashMap::new(),
            }))
            .await
            .expect_err("missing definition should fail");
        assert_eq!(missing_definition.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn get_modify_and_list_agents_cover_not_found_and_nested_keys() {
        let (handler, kv, _) = setup_handler();
        seed_namespace(&kv, "acme").await;

        let missing_get = handler
            .handle_get_agent(tonic::Request::new(proto::GetAgentRequest {
                ns: "acme".to_string(),
                name: "missing".to_string(),
            }))
            .await
            .expect_err("missing agent should fail");
        assert_eq!(missing_get.code(), tonic::Code::NotFound);

        let missing_modify = handler
            .handle_modify_agent(tonic::Request::new(proto::ModifyAgentRequest {
                agent: "missing".to_string(),
                ns: "acme".to_string(),
                definition: None,
                labels: HashMap::new(),
            }))
            .await
            .expect_err("missing agent should fail");
        assert_eq!(missing_modify.code(), tonic::Code::NotFound);

        kv.set_msg(
            "acme",
            &keys::agent("agent-1"),
            &models::Agent {
                name: "agent-1".to_string(),
                ns: "acme".to_string(),
                definition: Some(custom_agent_definition("gpt-5")),
                effective_spec: None,
                template_deps: Vec::new(),
                labels: HashMap::new(),
            },
        )
        .await
        .unwrap();
        kv.set("acme", "Agent/agent-1/Session/test", b"nested")
            .await
            .unwrap();

        let listed = handler
            .handle_list_agents(tonic::Request::new(proto::ListAgentsRequest {
                ns: "acme".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(listed.agents, vec!["agent-1".to_string()]);
    }

    #[tokio::test]
    async fn template_crud_applies_defaults_and_validates_namespace() {
        let (handler, _, _) = setup_handler();

        let created = handler
            .handle_create_agent_template(tonic::Request::new(
                proto::CreateAgentTemplateRequest {
                    template: Some(agent_template("researcher", "")),
                },
            ))
            .await
            .expect("template create should succeed")
            .into_inner();
        let template = created.template.expect("template should be returned");
        assert_eq!(template.api_version, "v1");
        assert_eq!(template.kind, "AgentTemplate");
        assert_eq!(
            template
                .metadata
                .as_ref()
                .map(|meta| meta.namespace.as_str()),
            Some(ns::TALON_SYSTEM)
        );

        let listed = handler
            .handle_list_agent_templates(tonic::Request::new(
                proto::ListAgentTemplatesRequest {},
            ))
            .await
            .expect("template list should succeed")
            .into_inner();
        assert_eq!(listed.templates.len(), 1);

        let fetched = handler
            .handle_get_agent_template(tonic::Request::new(proto::GetAgentTemplateRequest {
                name: "researcher".to_string(),
            }))
            .await
            .expect("template get should succeed")
            .into_inner();
        assert_eq!(
            fetched
                .template
                .as_ref()
                .and_then(|template| template.metadata.as_ref())
                .map(|meta| meta.name.as_str()),
            Some("researcher")
        );

        let deleted = handler
            .handle_delete_agent_template(tonic::Request::new(
                proto::DeleteAgentTemplateRequest {
                    name: "researcher".to_string(),
                },
            ))
            .await
            .expect("template delete should succeed")
            .into_inner();
        assert!(deleted.success);

        let wrong_namespace = handler
            .handle_create_agent_template(tonic::Request::new(
                proto::CreateAgentTemplateRequest {
                    template: Some(agent_template("bad-template", "custom")),
                },
            ))
            .await
            .expect_err("custom namespace should fail");
        assert_eq!(wrong_namespace.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn mcp_server_crud_publishes_lifecycle_events() {
        let (handler, _, published) = setup_handler();

        let created = handler
            .handle_create_mcp_server(tonic::Request::new(proto::CreateMcpServerRequest {
                server: Some(mcp_server("search-api", "", "http")),
            }))
            .await
            .expect("server create should succeed")
            .into_inner();
        let server = created.server.expect("server should be returned");
        assert_eq!(server.api_version, "v1");
        assert_eq!(server.kind, "MCPServer");

        let listed = handler
            .handle_list_mcp_servers(tonic::Request::new(proto::ListMcpServersRequest {}))
            .await
            .expect("server list should succeed")
            .into_inner();
        assert_eq!(listed.servers.len(), 1);

        let fetched = handler
            .handle_get_mcp_server(tonic::Request::new(proto::GetMcpServerRequest {
                name: "search-api".to_string(),
            }))
            .await
            .expect("server get should succeed")
            .into_inner();
        assert_eq!(
            fetched
                .server
                .as_ref()
                .and_then(|server| server.metadata.as_ref())
                .map(|meta| meta.name.as_str()),
            Some("search-api")
        );

        let invalid = handler
            .handle_create_mcp_server(tonic::Request::new(proto::CreateMcpServerRequest {
                server: Some(mcp_server("broken", "not-allowed", "http")),
            }))
            .await
            .expect_err("namespaced server should fail");
        assert_eq!(invalid.code(), tonic::Code::InvalidArgument);

        let deleted = handler
            .handle_delete_mcp_server(tonic::Request::new(proto::DeleteMcpServerRequest {
                name: "search-api".to_string(),
            }))
            .await
            .expect("server delete should succeed")
            .into_inner();
        assert!(deleted.success);

        let published = published.lock().await;
        assert_eq!(published.len(), 2);
        let create_event = events::LifecycleEvent::decode(published[0].1.as_slice())
            .expect("create event should decode");
        let delete_event = events::LifecycleEvent::decode(published[1].1.as_slice())
            .expect("delete event should decode");
        assert_eq!(create_event.action, events::SystemAction::Create as i32);
        assert_eq!(delete_event.action, events::SystemAction::Delete as i32);
    }

    #[tokio::test]
    async fn mcp_binding_crud_and_validation_paths() {
        let (handler, _, published) = setup_handler();
        handler
            .handle_create_mcp_server(tonic::Request::new(proto::CreateMcpServerRequest {
                server: Some(mcp_server("http-server", "", "http")),
            }))
            .await
            .expect("server seed should succeed");

        let created = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(mcp_binding("search-binding", "acme", "http-server")),
                },
            ))
            .await
            .expect("binding create should succeed")
            .into_inner();
        let binding = created.binding.expect("binding should be returned");
        assert_eq!(binding.api_version, "v1");
        assert_eq!(binding.kind, "McpServerBinding");

        let listed = handler
            .handle_list_mcp_server_bindings(tonic::Request::new(
                proto::ListMcpServerBindingsRequest {
                    ns: "acme".to_string(),
                },
            ))
            .await
            .expect("binding list should succeed")
            .into_inner();
        assert_eq!(listed.bindings.len(), 1);

        let fetched = handler
            .handle_get_mcp_server_binding(tonic::Request::new(
                proto::GetMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    name: "search-binding".to_string(),
                },
            ))
            .await
            .expect("binding get should succeed")
            .into_inner();
        assert_eq!(
            fetched
                .binding
                .as_ref()
                .and_then(|binding| binding.metadata.as_ref())
                .map(|meta| meta.namespace.as_str()),
            Some("acme")
        );

        let mut invalid_binding = mcp_binding("bad-binding", "other", "http-server");
        let invalid_namespace = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(invalid_binding.clone()),
                },
            ))
            .await
            .expect_err("mismatched namespace should fail");
        assert_eq!(invalid_namespace.code(), tonic::Code::InvalidArgument);

        invalid_binding.metadata = Some(metadata("bad-auth", "acme"));
        invalid_binding.spec.as_mut().expect("spec should exist").auth_broker =
            Some(manifests::McpAuthBrokerSpec {
                kind: "http_bearer".to_string(),
                url: "not-a-url".to_string(),
                cache_ttl_seconds: 0,
                audience: String::new(),
            });
        let invalid_broker = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(invalid_binding),
                },
            ))
            .await
            .expect_err("invalid auth broker url should fail");
        assert_eq!(invalid_broker.code(), tonic::Code::InvalidArgument);

        let deleted = handler
            .handle_delete_mcp_server_binding(tonic::Request::new(
                proto::DeleteMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    name: "search-binding".to_string(),
                },
            ))
            .await
            .expect("binding delete should succeed")
            .into_inner();
        assert!(deleted.success);

        let published = published.lock().await;
        assert!(
            published
                .iter()
                .filter(|(topic, _)| topic == topics::RESOURCE_LIFECYCLE_TOPIC)
                .count()
                >= 3
        );
    }

    #[tokio::test]
    async fn mcp_binding_validation_covers_remaining_preconditions() {
        let (handler, _, _) = setup_handler();
        handler
            .handle_create_mcp_server(tonic::Request::new(proto::CreateMcpServerRequest {
                server: Some(mcp_server("http-server", "", "http")),
            }))
            .await
            .expect("http server seed should succeed");
        handler
            .handle_create_mcp_server(tonic::Request::new(proto::CreateMcpServerRequest {
                server: Some(mcp_server("stdio-server", "", "stdio")),
            }))
            .await
            .expect("stdio server seed should succeed");

        let missing_ns = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: String::new(),
                    binding: Some(mcp_binding("binding", "acme", "http-server")),
                },
            ))
            .await
            .expect_err("missing namespace should fail");
        assert_eq!(missing_ns.code(), tonic::Code::InvalidArgument);

        let missing_binding = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: None,
                },
            ))
            .await
            .expect_err("missing binding should fail");
        assert_eq!(missing_binding.code(), tonic::Code::InvalidArgument);

        let mut missing_meta = mcp_binding("binding", "acme", "http-server");
        missing_meta.metadata = None;
        let missing_meta_err = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(missing_meta),
                },
            ))
            .await
            .expect_err("missing metadata should fail");
        assert_eq!(missing_meta_err.code(), tonic::Code::InvalidArgument);

        let mut missing_name = mcp_binding("binding", "acme", "http-server");
        missing_name.metadata.as_mut().unwrap().name.clear();
        let missing_name_err = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(missing_name),
                },
            ))
            .await
            .expect_err("missing metadata.name should fail");
        assert_eq!(missing_name_err.code(), tonic::Code::InvalidArgument);

        let mut missing_spec = mcp_binding("binding", "acme", "http-server");
        missing_spec.spec = None;
        let missing_spec_err = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(missing_spec),
                },
            ))
            .await
            .expect_err("missing spec should fail");
        assert_eq!(missing_spec_err.code(), tonic::Code::InvalidArgument);

        let mut missing_ref = mcp_binding("binding", "acme", "http-server");
        missing_ref.spec.as_mut().unwrap().server_ref.clear();
        let missing_ref_err = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(missing_ref),
                },
            ))
            .await
            .expect_err("missing server ref should fail");
        assert_eq!(missing_ref_err.code(), tonic::Code::InvalidArgument);

        let missing_server = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(mcp_binding("binding", "acme", "missing-server")),
                },
            ))
            .await
            .expect_err("missing referenced server should fail");
        assert_eq!(missing_server.code(), tonic::Code::FailedPrecondition);

        let mut invalid_kind = mcp_binding("binding", "acme", "http-server");
        invalid_kind.spec.as_mut().unwrap().auth_broker = Some(manifests::McpAuthBrokerSpec {
            kind: "oauth".to_string(),
            url: "https://broker.example.com/token".to_string(),
            cache_ttl_seconds: 60,
            audience: String::new(),
        });
        let invalid_kind_err = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(invalid_kind),
                },
            ))
            .await
            .expect_err("invalid auth broker kind should fail");
        assert_eq!(invalid_kind_err.code(), tonic::Code::InvalidArgument);

        let mut blank_url = mcp_binding("binding", "acme", "http-server");
        blank_url.spec.as_mut().unwrap().auth_broker = Some(manifests::McpAuthBrokerSpec {
            kind: "http_bearer".to_string(),
            url: "   ".to_string(),
            cache_ttl_seconds: 0,
            audience: String::new(),
        });
        let blank_url_err = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(blank_url),
                },
            ))
            .await
            .expect_err("blank auth broker url should fail");
        assert_eq!(blank_url_err.code(), tonic::Code::InvalidArgument);

        let mut negative_ttl = mcp_binding("binding", "acme", "http-server");
        negative_ttl.spec.as_mut().unwrap().auth_broker = Some(manifests::McpAuthBrokerSpec {
            kind: "http_bearer".to_string(),
            url: "https://broker.example.com/token".to_string(),
            cache_ttl_seconds: -1,
            audience: String::new(),
        });
        let negative_ttl_err = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(negative_ttl),
                },
            ))
            .await
            .expect_err("negative cache ttl should fail");
        assert_eq!(negative_ttl_err.code(), tonic::Code::InvalidArgument);

        let mut broker_with_headers = mcp_binding("binding", "acme", "http-server");
        broker_with_headers
            .spec
            .as_mut()
            .unwrap()
            .headers
            .insert("authorization".to_string(), "Bearer static".to_string());
        broker_with_headers.spec.as_mut().unwrap().auth_broker =
            Some(manifests::McpAuthBrokerSpec {
                kind: "http_bearer".to_string(),
                url: "https://broker.example.com/token".to_string(),
                cache_ttl_seconds: 60,
                audience: String::new(),
            });
        let conflict_err = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(broker_with_headers),
                },
            ))
            .await
            .expect_err("headers and auth broker together should fail");
        assert_eq!(conflict_err.code(), tonic::Code::InvalidArgument);

        let mut non_http_broker = mcp_binding("binding", "acme", "stdio-server");
        non_http_broker.spec.as_mut().unwrap().auth_broker = Some(manifests::McpAuthBrokerSpec {
            kind: "http_bearer".to_string(),
            url: "https://broker.example.com/token".to_string(),
            cache_ttl_seconds: 60,
            audience: String::new(),
        });
        let non_http_broker_err = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(non_http_broker),
                },
            ))
            .await
            .expect_err("auth broker on non-http server should fail");
        assert_eq!(non_http_broker_err.code(), tonic::Code::FailedPrecondition);

        let mut empty_tool_name = mcp_binding("binding", "acme", "http-server");
        empty_tool_name.spec.as_mut().unwrap().allowed_tool_names =
            vec!["search".to_string(), " ".to_string()];
        let empty_tool_name_err = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(empty_tool_name),
                },
            ))
            .await
            .expect_err("blank allowed tool name should fail");
        assert_eq!(empty_tool_name_err.code(), tonic::Code::InvalidArgument);

        let mut spaced_tool_name = mcp_binding("binding", "acme", "http-server");
        spaced_tool_name.spec.as_mut().unwrap().allowed_tool_names =
            vec![" search ".to_string()];
        let spaced_tool_name_err = handler
            .handle_create_mcp_server_binding(tonic::Request::new(
                proto::CreateMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    binding: Some(spaced_tool_name),
                },
            ))
            .await
            .expect_err("surrounding whitespace in tool names should fail");
        assert_eq!(spaced_tool_name_err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn mcp_binding_get_list_and_delete_validate_arguments_and_not_found() {
        let (handler, _, _) = setup_handler();

        let get_missing_ns = handler
            .handle_get_mcp_server_binding(tonic::Request::new(
                proto::GetMcpServerBindingRequest {
                    ns: String::new(),
                    name: "binding".to_string(),
                },
            ))
            .await
            .expect_err("missing get namespace should fail");
        assert_eq!(get_missing_ns.code(), tonic::Code::InvalidArgument);

        let get_missing_name = handler
            .handle_get_mcp_server_binding(tonic::Request::new(
                proto::GetMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    name: String::new(),
                },
            ))
            .await
            .expect_err("missing get name should fail");
        assert_eq!(get_missing_name.code(), tonic::Code::InvalidArgument);

        let get_not_found = handler
            .handle_get_mcp_server_binding(tonic::Request::new(
                proto::GetMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    name: "missing".to_string(),
                },
            ))
            .await
            .expect_err("missing binding should fail");
        assert_eq!(get_not_found.code(), tonic::Code::NotFound);

        let list_missing_ns = handler
            .handle_list_mcp_server_bindings(tonic::Request::new(
                proto::ListMcpServerBindingsRequest { ns: String::new() },
            ))
            .await
            .expect_err("missing list namespace should fail");
        assert_eq!(list_missing_ns.code(), tonic::Code::InvalidArgument);

        let delete_missing_ns = handler
            .handle_delete_mcp_server_binding(tonic::Request::new(
                proto::DeleteMcpServerBindingRequest {
                    ns: String::new(),
                    name: "binding".to_string(),
                },
            ))
            .await
            .expect_err("missing delete namespace should fail");
        assert_eq!(delete_missing_ns.code(), tonic::Code::InvalidArgument);

        let delete_missing_name = handler
            .handle_delete_mcp_server_binding(tonic::Request::new(
                proto::DeleteMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    name: String::new(),
                },
            ))
            .await
            .expect_err("missing delete name should fail");
        assert_eq!(delete_missing_name.code(), tonic::Code::InvalidArgument);

        let delete_not_found = handler
            .handle_delete_mcp_server_binding(tonic::Request::new(
                proto::DeleteMcpServerBindingRequest {
                    ns: "acme".to_string(),
                    name: "missing".to_string(),
                },
            ))
            .await
            .expect_err("missing delete target should fail");
        assert_eq!(delete_not_found.code(), tonic::Code::NotFound);
    }
}
