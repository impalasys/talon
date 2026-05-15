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

        let binding_key = keys::mcp_server_binding(name);
        let binding = cp
            .kv
            .get_msg::<manifests::McpServerBinding>(namespace, &binding_key)
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
            .get_msg::<manifests::McpServer>(ns::TALON_SYSTEM, &key)
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
    use super::filter_allowed_tools;
    use crate::connectors::mcp::McpTool;
    use serde_json::json;

    fn tool(name: &str) -> McpTool {
        McpTool {
            name: name.to_string(),
            description: String::new(),
            input_schema: json!({"type": "object"}),
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
}
