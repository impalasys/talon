// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::control::security::platform_jwt;
use crate::control::{keys, ns, ControlPlane, ProtoKeyValueStoreExt};
use crate::gateway::rpc::manifests;
use crate::harness::mcp::{list_tools_for_config, McpConnectionConfig, McpTool};

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

        let server = resolve_server_from_ancestry(cp, namespace, name).await?;
        let config =
            config_for_resolution_namespace(McpConnectionConfig::try_from(&server)?, namespace)?;
        let allowlist = server
            .spec
            .as_ref()
            .and_then(|spec| spec.policy.as_ref())
            .and_then(|policy| policy.tools.as_ref())
            .map(|tools| tools.allowlist.as_slice())
            .unwrap_or_default();
        let tools = filter_allowed_tools(list_tools_for_config(&config).await?, allowlist);
        let server = Arc::new(ResolvedMcpServer { config, tools });

        let mut cache = self.cache.write().await;
        cache
            .entry(namespace.to_string())
            .or_default()
            .insert(name.to_string(), server.clone());

        Ok(server)
    }
}

async fn resolve_server_from_ancestry(
    cp: &ControlPlane,
    namespace: &str,
    name: &str,
) -> Result<manifests::McpServer> {
    for candidate_ns in ns::ancestry(namespace) {
        let key = keys::mcp_server(&candidate_ns, name);
        if let Some(server) = cp.kv.get_msg::<manifests::McpServer>(&key).await? {
            return Ok(server);
        }
    }

    Err(anyhow!(
        "MCPServer '{}' not found in namespace ancestry for '{}'",
        name,
        namespace
    ))
}

fn filter_allowed_tools(tools: Vec<McpTool>, allowlist: &[String]) -> Vec<McpTool> {
    if allowlist.is_empty() {
        return tools;
    }

    let allowed: HashSet<&str> = allowlist
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .collect();

    tools
        .into_iter()
        .filter(|tool| allowed.contains(tool.name.as_str()))
        .collect()
}

fn config_for_resolution_namespace(
    mut config: McpConnectionConfig,
    namespace: &str,
) -> Result<McpConnectionConfig> {
    config.namespace = Some(namespace.to_string());
    config.jwt_issuer = Some(platform_jwt::issuer()?);
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::{config_for_resolution_namespace, filter_allowed_tools, McpRegistry};
    use crate::control::{
        keys::{ResourceKey, ResourceList},
        security::platform_jwt,
        ControlPlane, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
    };
    use crate::gateway::rpc::manifests;
    use crate::harness::mcp::McpTool;
    use serde_json::json;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockKvStore {
        data: Mutex<HashMap<ResourceKey, Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, key: &ResourceKey) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.data.lock().await.get(key).cloned())
        }

        async fn set(&self, key: &ResourceKey, value: &[u8]) -> anyhow::Result<()> {
            self.data.lock().await.insert(key.clone(), value.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            _key: &ResourceKey,
            _expected: Option<&[u8]>,
            _value: &[u8],
        ) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn delete(&self, key: &ResourceKey) -> anyhow::Result<()> {
            self.data.lock().await.remove(key);
            Ok(())
        }

        async fn list_keys(
            &self,
            list: &ResourceList,
            _order: crate::control::Order,
        ) -> anyhow::Result<Vec<ResourceKey>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|key| list.matches(key).then(|| key.clone()))
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

    async fn seed_server(kv: &MockKvStore, namespace: &str, name: &str, target: &str) {
        kv.set_msg(
            &crate::control::keys::mcp_server(namespace, name),
            &manifests::McpServer {
                metadata: Some(manifests::ObjectMeta {
                    name: name.to_string(),
                    namespace: namespace.to_string(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                    ..Default::default()
                }),
                spec: Some(manifests::McpServerSpec {
                    transport: "http".to_string(),
                    target: target.to_string(),
                    args: Vec::new(),
                    headers: HashMap::new(),
                    disabled: false,
                    auth_broker: None,
                    policy: None,
                }),
                status: Some(crate::control::resource_model::common_status(String::new())),
            },
        )
        .await
        .unwrap();
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

    #[test]
    fn config_for_resolution_namespace_uses_calling_namespace_for_inherited_server() {
        let config = crate::harness::mcp::McpConnectionConfig {
            server_name: "conic".to_string(),
            server_ref: "conic".to_string(),
            transport: "http".to_string(),
            target: "https://api.useconic.com/mcp".to_string(),
            args: Vec::new(),
            headers: HashMap::new(),
            disabled: false,
            namespace: Some("Tenant:conic:Customers".to_string()),
            mcp_server_name: Some("conic".to_string()),
            agent_name: None,
            jwt_issuer: None,
            auth_broker: None,
        };

        let _env_lock = crate::test_support::env_lock();
        let expected_issuer = platform_jwt::issuer().unwrap();
        let scoped = config_for_resolution_namespace(config, "Tenant:conic:Customers:12").unwrap();

        assert_eq!(
            scoped.namespace.as_deref(),
            Some("Tenant:conic:Customers:12")
        );
        assert_eq!(scoped.mcp_server_name.as_deref(), Some("conic"));
        assert_eq!(scoped.jwt_issuer.as_deref(), Some(expected_issuer.as_str()));
    }

    #[tokio::test]
    async fn invalidate_removes_named_entry_and_namespace_cache() {
        let registry = McpRegistry::new();
        registry.cache.write().await.insert(
            "conic".to_string(),
            HashMap::from([(
                "github".to_string(),
                Arc::new(super::ResolvedMcpServer {
                    config: crate::harness::mcp::McpConnectionConfig {
                        server_name: "github".to_string(),
                        server_ref: "github".to_string(),
                        transport: "http".to_string(),
                        target: "https://example.com".to_string(),
                        args: Vec::new(),
                        headers: HashMap::new(),
                        disabled: false,
                        namespace: Some("conic".to_string()),
                        mcp_server_name: Some("github".to_string()),
                        agent_name: None,
                        jwt_issuer: None,
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
                    config: crate::harness::mcp::McpConnectionConfig {
                        server_name: "docs".to_string(),
                        server_ref: "docs".to_string(),
                        transport: "http".to_string(),
                        target: "https://example.com".to_string(),
                        args: Vec::new(),
                        headers: HashMap::new(),
                        disabled: false,
                        namespace: Some("conic".to_string()),
                        mcp_server_name: Some("docs".to_string()),
                        agent_name: None,
                        jwt_issuer: None,
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
    async fn resolve_server_errors_for_missing_server() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        let registry = McpRegistry::new();

        let missing_server = registry
            .resolve_server(&cp, "github", "conic")
            .await
            .unwrap_err();
        assert!(missing_server.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn resolve_server_from_ancestry_prefers_exact_namespace() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        seed_server(&kv, "conic", "docs", "https://parent.example.com").await;
        seed_server(&kv, "conic:child", "docs", "https://child.example.com").await;

        let resolved = super::resolve_server_from_ancestry(&cp, "conic:child", "docs")
            .await
            .unwrap();

        assert_eq!(
            resolved
                .metadata
                .as_ref()
                .map(|meta| meta.namespace.as_str()),
            Some("conic:child")
        );
        assert_eq!(
            resolved.spec.as_ref().map(|spec| spec.target.as_str()),
            Some("https://child.example.com")
        );
    }

    #[tokio::test]
    async fn resolve_server_from_ancestry_uses_parent_for_child_namespace() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        seed_server(&kv, "conic", "docs", "https://parent.example.com").await;

        let resolved = super::resolve_server_from_ancestry(&cp, "conic:child:leaf", "docs")
            .await
            .unwrap();

        assert_eq!(
            resolved
                .metadata
                .as_ref()
                .map(|meta| meta.namespace.as_str()),
            Some("conic")
        );
    }

    #[tokio::test]
    async fn resolve_server_from_ancestry_ignores_sys_fallback() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        seed_server(&kv, "Sys", "docs", "https://sys.example.com").await;

        let missing = super::resolve_server_from_ancestry(&cp, "conic:child", "docs")
            .await
            .unwrap_err();

        assert!(missing.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn resolve_server_uses_parent_namespace_and_returns_disabled_error_before_connecting() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(MockPubSub)).build();
        let registry = McpRegistry::new();

        kv.set_msg(
            &crate::control::keys::mcp_server("conic", "docs"),
            &manifests::McpServer {
                metadata: Some(manifests::ObjectMeta {
                    name: "docs".to_string(),
                    namespace: "conic".to_string(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                    ..Default::default()
                }),
                spec: Some(manifests::McpServerSpec {
                    transport: "http".to_string(),
                    target: "https://example.com/mcp".to_string(),
                    args: vec!["--server".to_string()],
                    headers: HashMap::from([(
                        "Authorization".to_string(),
                        "Bearer token".to_string(),
                    )]),
                    disabled: true,
                    auth_broker: Some(manifests::McpAuthBrokerSpec {
                        kind: "oauth".to_string(),
                        url: "https://example.com/auth".to_string(),
                        cache_ttl_seconds: 60,
                        audience: "docs".to_string(),
                    }),
                    policy: Some(manifests::McpServerPolicy {
                        tools: Some(manifests::McpToolPolicy {
                            allowlist: vec!["search".to_string()],
                        }),
                    }),
                }),
                status: Some(crate::control::resource_model::common_status(String::new())),
            },
        )
        .await
        .unwrap();

        let err = registry
            .resolve_server(&cp, "docs", "conic:child")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("disabled"));
        assert!(registry.cache.read().await.is_empty());
    }
}
