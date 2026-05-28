// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::connectors::mcp::{
    list_tools_for_config, McpAuthBrokerConfig, McpConnectionConfig, McpTool,
};
use crate::control::{keys, ns, ControlPlane, ProtoKeyValueStoreExt};
use crate::gateway::rpc::manifests;

#[derive(Debug, Clone)]
pub struct ResolvedMcpServer {
    pub config: McpConnectionConfig,
    pub tools: Vec<McpTool>,
}

#[derive(Default)]
pub struct McpRegistry {
    cache: RwLock<HashMap<String, HashMap<String, Arc<ResolvedMcpServer>>>>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn invalidate(&self, ns: &str, name: Option<&str>) {
        let mut cache = self.cache.write().await;
        if let Some(name) = name {
            if let Some(ns_cache) = cache.get_mut(ns) {
                ns_cache.remove(name);
                if ns_cache.is_empty() {
                    cache.remove(ns);
                }
            }
        } else {
            cache.remove(ns);
        }
    }

    pub async fn invalidate_all(&self) {
        self.cache.write().await.clear();
    }

    pub async fn resolve_server(
        &self,
        cp: &ControlPlane,
        name: &str,
        namespace: &str,
    ) -> Result<Arc<ResolvedMcpServer>> {
        if let Some(existing) = self
            .cache
            .read()
            .await
            .get(namespace)
            .and_then(|ns_cache| ns_cache.get(name))
            .cloned()
        {
            return Ok(existing);
        }

        let binding_key = keys::mcp_server_binding(namespace, name);
        let binding = cp
            .kv
            .get_msg::<manifests::McpServerBinding>(&binding_key)
            .await?;
        let (server_ref, extra_args, extra_headers, disabled, auth_broker, allowed_tool_names) =
            match binding {
                Some(binding) => {
                    let binding_spec = binding
                        .spec
                        .as_ref()
                        .ok_or_else(|| anyhow!("McpServerBinding '{}' missing spec", name))?;
                    (
                        binding_spec.server_ref.clone(),
                        binding_spec.args.clone(),
                        binding_spec.headers.clone(),
                        binding_spec.disabled,
                        binding_spec
                            .auth_broker
                            .as_ref()
                            .map(|broker| McpAuthBrokerConfig {
                                kind: broker.kind.clone(),
                                url: broker.url.clone(),
                                cache_ttl_seconds: broker.cache_ttl_seconds,
                                audience: broker.audience.clone(),
                            }),
                        binding_spec.allowed_tool_names.clone(),
                    )
                }
                None => (
                    name.to_string(),
                    Vec::new(),
                    HashMap::new(),
                    false,
                    None,
                    Vec::new(),
                ),
            };

        let key = keys::mcp_server(&server_ref);
        let server = cp
            .kv
            .get_msg::<manifests::McpServer>(&key)
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "MCPServer '{}' not found in namespace '{}'",
                    server_ref,
                    ns::TALON_SYSTEM
                )
            })?;
        let mut config = McpConnectionConfig::try_from(&server)?;
        for arg in &extra_args {
            config.args.push(arg.clone());
        }
        for (header, value) in &extra_headers {
            config.headers.insert(header.clone(), value.clone());
        }
        config.disabled = config.disabled || disabled;
        config.server_ref = server_ref;
        config.namespace = Some(namespace.to_string());
        config.binding_name = Some(name.to_string());
        config.auth_broker = auth_broker;
        let tools =
            filter_allowed_tools(list_tools_for_config(&config).await?, &allowed_tool_names);
        let server = Arc::new(ResolvedMcpServer { config, tools });

        let mut cache = self.cache.write().await;
        cache
            .entry(namespace.to_string())
            .or_default()
            .insert(name.to_string(), server.clone());

        Ok(server)
    }
}

fn filter_allowed_tools(tools: Vec<McpTool>, allowed_tool_names: &[String]) -> Vec<McpTool> {
    if allowed_tool_names.is_empty() {
        return tools;
    }

    let allowed: HashSet<&str> = allowed_tool_names
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .collect();

    tools
        .into_iter()
        .filter(|tool| allowed.contains(tool.name.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{filter_allowed_tools, McpRegistry};
    use crate::connectors::mcp::McpTool;
    use crate::control::{
        scheduler::NoopSchedulerBackend, ControlPlane, KeyValueStore, MessagePublisher,
        ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::manifests;
    use serde_json::json;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockKvStore {
        data: Mutex<HashMap<String, Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.data.lock().await.get(key).cloned())
        }

        async fn set(&self, key: &str, value: &[u8]) -> anyhow::Result<()> {
            self.data
                .lock()
                .await
                .insert(key.to_string(), value.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            _key: &str,
            _expected: Option<&[u8]>,
            _value: &[u8],
        ) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn delete(&self, key: &str) -> anyhow::Result<()> {
            self.data.lock().await.remove(key);
            Ok(())
        }

        async fn list_keys(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|key| key.starts_with(prefix).then(|| key.clone()))
                .collect::<Vec<_>>();
            keys.sort();
            Ok(keys)
        }
    }

    #[derive(Default)]
    struct MockPubSub;

    #[async_trait::async_trait]
    impl MessagePublisher for MockPubSub {
        async fn publish(&self, _topic: &str, _message: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            Ok(Box::pin(futures::stream::empty()))
        }
    }

    fn tool(name: &str) -> McpTool {
        McpTool {
            name: name.to_string(),
            description: String::new(),
            input_schema: json!({"type": "object"}),
        }
    }

    fn control_plane(kv: Arc<MockKvStore>) -> ControlPlane {
        ControlPlane {
            kv,
            pubsub: Arc::new(MockPubSub),
            scheduler: Arc::new(NoopSchedulerBackend),
        }
    }

    #[test]
    fn filter_allowed_tools_returns_all_when_allowlist_is_empty() {
        let tools = vec![tool("get_file_contents"), tool("search_code")];

        let filtered = filter_allowed_tools(tools.clone(), &[]);

        assert_eq!(filtered.len(), tools.len());
        assert_eq!(filtered[0].name, "get_file_contents");
        assert_eq!(filtered[1].name, "search_code");
    }

    #[test]
    fn filter_allowed_tools_keeps_only_allowed_entries() {
        let tools = vec![tool("get_file_contents"), tool("create_pull_request")];
        let allowed = vec!["get_file_contents".to_string()];

        let filtered = filter_allowed_tools(tools, &allowed);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "get_file_contents");
    }

    #[test]
    fn filter_allowed_tools_trims_allowed_entries() {
        let tools = vec![tool("get_file_contents"), tool("create_pull_request")];
        let allowed = vec![" get_file_contents ".to_string()];

        let filtered = filter_allowed_tools(tools, &allowed);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "get_file_contents");
    }

    #[tokio::test]
    async fn invalidate_removes_named_entry_and_namespace_cache() {
        let registry = McpRegistry::new();
        registry.cache.write().await.insert(
            "conic".to_string(),
            HashMap::from([(
                "github".to_string(),
                Arc::new(super::ResolvedMcpServer {
                    config: crate::connectors::mcp::McpConnectionConfig {
                        server_name: "github".to_string(),
                        server_ref: "github".to_string(),
                        transport: "http".to_string(),
                        target: "https://example.com".to_string(),
                        args: Vec::new(),
                        headers: HashMap::new(),
                        disabled: false,
                        namespace: Some("conic".to_string()),
                        binding_name: Some("github".to_string()),
                        agent_name: None,
                        auth_broker: None,
                    },
                    tools: Vec::new(),
                }),
            )]),
        );

        registry.invalidate("conic", Some("github")).await;
        assert!(registry.cache.read().await.get("conic").is_none());

        registry.cache.write().await.insert(
            "conic".to_string(),
            HashMap::from([(
                "docs".to_string(),
                Arc::new(super::ResolvedMcpServer {
                    config: crate::connectors::mcp::McpConnectionConfig {
                        server_name: "docs".to_string(),
                        server_ref: "docs".to_string(),
                        transport: "http".to_string(),
                        target: "https://example.com".to_string(),
                        args: Vec::new(),
                        headers: HashMap::new(),
                        disabled: false,
                        namespace: Some("conic".to_string()),
                        binding_name: Some("docs".to_string()),
                        agent_name: None,
                        auth_broker: None,
                    },
                    tools: Vec::new(),
                }),
            )]),
        );
        registry.invalidate("conic", None).await;
        assert!(registry.cache.read().await.is_empty());
    }

    #[tokio::test]
    async fn invalidate_all_clears_every_namespace() {
        let registry = McpRegistry::new();
        registry
            .cache
            .write()
            .await
            .insert("one".to_string(), HashMap::new());
        registry
            .cache
            .write()
            .await
            .insert("two".to_string(), HashMap::new());

        registry.invalidate_all().await;

        assert!(registry.cache.read().await.is_empty());
    }

    #[tokio::test]
    async fn resolve_server_errors_for_missing_binding_spec_or_server() {
        let kv = Arc::new(MockKvStore::default());
        let cp = control_plane(kv.clone());
        let registry = McpRegistry::new();

        kv.set_msg(
            &crate::control::keys::mcp_server_binding("conic", "github"),
            &manifests::McpServerBinding {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "McpServerBinding".to_string(),
                metadata: Some(manifests::ObjectMeta {
                    name: "github".to_string(),
                    namespace: "conic".to_string(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                }),
                spec: None,
            },
        )
        .await
        .unwrap();

        let missing_spec = registry
            .resolve_server(&cp, "github", "conic")
            .await
            .unwrap_err();
        assert!(missing_spec.to_string().contains("missing spec"));

        kv.delete(&crate::control::keys::mcp_server_binding("conic", "github"))
            .await
            .unwrap();
        let missing_server = registry
            .resolve_server(&cp, "github", "conic")
            .await
            .unwrap_err();
        assert!(missing_server.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn resolve_server_merges_binding_and_returns_disabled_error_before_connecting() {
        let kv = Arc::new(MockKvStore::default());
        let cp = control_plane(kv.clone());
        let registry = McpRegistry::new();

        kv.set_msg(
            &crate::control::keys::mcp_server("docs-server"),
            &manifests::McpServer {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "McpServer".to_string(),
                metadata: Some(manifests::ObjectMeta {
                    name: "docs-server".to_string(),
                    namespace: String::new(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                }),
                spec: Some(manifests::McpServerSpec {
                    transport: "http".to_string(),
                    target: "https://example.com/mcp".to_string(),
                    args: vec!["--server".to_string()],
                    headers: HashMap::from([(
                        "Authorization".to_string(),
                        "Bearer token".to_string(),
                    )]),
                    disabled: false,
                }),
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &crate::control::keys::mcp_server_binding("conic", "docs"),
            &manifests::McpServerBinding {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "McpServerBinding".to_string(),
                metadata: Some(manifests::ObjectMeta {
                    name: "docs".to_string(),
                    namespace: "conic".to_string(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                }),
                spec: Some(manifests::McpServerBindingSpec {
                    server_ref: "docs-server".to_string(),
                    args: vec!["--binding".to_string()],
                    headers: HashMap::from([(
                        "Authorization".to_string(),
                        "Bearer override".to_string(),
                    )]),
                    disabled: true,
                    auth_broker: Some(manifests::McpAuthBrokerSpec {
                        kind: "oauth".to_string(),
                        url: "https://example.com/auth".to_string(),
                        cache_ttl_seconds: 60,
                        audience: "docs".to_string(),
                    }),
                    allowed_tool_names: vec!["search".to_string()],
                }),
            },
        )
        .await
        .unwrap();

        let err = registry
            .resolve_server(&cp, "docs", "conic")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("disabled"));
        assert!(registry.cache.read().await.is_empty());
    }
}
