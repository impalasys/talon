// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};

use crate::gateway::rpc::manifests;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedInternalConnection {
    pub connection_name: String,
    pub target_namespace: String,
    pub target_agent: String,
}

pub fn internal_connection_names(spec: &manifests::AgentSpec) -> Vec<String> {
    spec.a2a
        .as_ref()
        .map(|a2a| {
            a2a.connections
                .iter()
                .filter(|connection| {
                    connection
                        .target
                        .as_ref()
                        .and_then(|target| target.internal.as_ref())
                        .is_some()
                })
                .map(|connection| connection.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

pub fn resolve_internal_connection(
    spec: &manifests::AgentSpec,
    name: &str,
) -> Result<ResolvedInternalConnection> {
    let requested = name.trim();
    if requested.is_empty() {
        return Err(anyhow!("A2A connection name is required"));
    }
    let Some(a2a) = spec.a2a.as_ref() else {
        return Err(anyhow!(
            "agent has no A2A connections; cannot delegate to '{}'",
            requested
        ));
    };
    let Some(connection) = a2a
        .connections
        .iter()
        .find(|connection| connection.name == requested)
    else {
        let valid = internal_connection_names(spec);
        if valid.is_empty() {
            return Err(anyhow!(
                "A2A connection '{}' is not declared; no internal A2A connections are available",
                requested
            ));
        }
        return Err(anyhow!(
            "A2A connection '{}' is not declared as an internal delegation target; valid connections: {}",
            requested,
            valid.join(", ")
        ));
    };
    let Some(target) = connection.target.as_ref() else {
        return Err(anyhow!("A2A connection '{}' has no target", requested));
    };
    if target.external.is_some() {
        return Err(anyhow!(
            "external A2A connection '{}' is not supported by delegate_task yet",
            requested
        ));
    }
    let Some(internal) = target.internal.as_ref() else {
        return Err(anyhow!(
            "A2A connection '{}' is not an internal delegation target",
            requested
        ));
    };
    Ok(ResolvedInternalConnection {
        connection_name: connection.name.clone(),
        target_namespace: internal.namespace.clone(),
        target_agent: internal.agent.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(name: &str, target: manifests::ConnectionRef) -> manifests::Connection {
        manifests::Connection {
            name: name.to_string(),
            target: Some(target),
            ..Default::default()
        }
    }

    fn internal(namespace: &str, agent: &str) -> manifests::ConnectionRef {
        manifests::ConnectionRef {
            internal: Some(manifests::InternalConnectionRef {
                namespace: namespace.to_string(),
                agent: agent.to_string(),
            }),
            external: None,
        }
    }

    fn external(url: &str) -> manifests::ConnectionRef {
        manifests::ConnectionRef {
            internal: None,
            external: Some(manifests::ExternalConnectionRef {
                agent_card_url: url.to_string(),
            }),
        }
    }

    #[test]
    fn lists_only_internal_connection_names() {
        let spec = manifests::AgentSpec {
            a2a: Some(manifests::A2a {
                connections: vec![
                    connection("worker", internal("Tenant:acme:Ops", "worker-agent")),
                    connection("external", external("https://example.com/card.json")),
                ],
                agent_card: None,
            }),
            ..Default::default()
        };

        assert_eq!(internal_connection_names(&spec), vec!["worker"]);
    }

    #[test]
    fn resolves_internal_connection() {
        let spec = manifests::AgentSpec {
            a2a: Some(manifests::A2a {
                connections: vec![connection(
                    "worker",
                    internal("Tenant:acme:Ops", "worker-agent"),
                )],
                agent_card: None,
            }),
            ..Default::default()
        };

        let resolved = resolve_internal_connection(&spec, "worker").unwrap();
        assert_eq!(resolved.connection_name, "worker");
        assert_eq!(resolved.target_namespace, "Tenant:acme:Ops");
        assert_eq!(resolved.target_agent, "worker-agent");
    }

    #[test]
    fn trims_requested_connection_name() {
        let spec = manifests::AgentSpec {
            a2a: Some(manifests::A2a {
                connections: vec![connection(
                    "worker",
                    internal("Tenant:acme:Ops", "worker-agent"),
                )],
                agent_card: None,
            }),
            ..Default::default()
        };

        let resolved = resolve_internal_connection(&spec, " worker ").unwrap();
        assert_eq!(resolved.connection_name, "worker");
    }

    #[test]
    fn rejects_unknown_connection_with_valid_names() {
        let spec = manifests::AgentSpec {
            a2a: Some(manifests::A2a {
                connections: vec![connection(
                    "worker",
                    internal("Tenant:acme:Ops", "worker-agent"),
                )],
                agent_card: None,
            }),
            ..Default::default()
        };

        let err = resolve_internal_connection(&spec, "missing").unwrap_err();
        assert!(err.to_string().contains("valid connections: worker"));
    }

    #[test]
    fn rejects_external_connection() {
        let spec = manifests::AgentSpec {
            a2a: Some(manifests::A2a {
                connections: vec![connection(
                    "remote",
                    external("https://example.com/card.json"),
                )],
                agent_card: None,
            }),
            ..Default::default()
        };

        let err = resolve_internal_connection(&spec, "remote").unwrap_err();
        assert!(err.to_string().contains("external A2A connection"));
    }
}
