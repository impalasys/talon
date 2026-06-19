// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub(super) fn render_json_payload(content: &str) -> Result<serde_json::Value> {
    let raw = parse_raw_manifest(content)?;
    let manifest_value: serde_yaml::Value =
        serde_yaml::from_str(content).context("Failed to parse rendered manifest")?;
    match raw.kind.as_str() {
        "MCPServer" | "McpServer" => Ok(json!({ "server": manifest_value })),
        "Agent" => Ok(json!({ "agent": manifest_value })),
        "McpServerBinding" => {
            let binding = crate::control::manifest::parse_mcp_server_binding(content)?;
            let namespace = binding
                .metadata
                .as_ref()
                .map(|meta| meta.namespace.clone())
                .filter(|namespace| !namespace.is_empty())
                .context("McpServerBinding missing metadata.namespace")?;
            Ok(json!({
                "ns": namespace,
                "binding": binding,
            }))
        }
        "Namespace" => {
            let namespace = crate::control::manifest::parse_namespace(content)?;
            Ok(json!({
                "name": namespace.name(),
                "recursive": true,
                "labels": namespace.labels(),
            }))
        }
        "Knowledge" => Ok(json!({ "knowledge": manifest_value })),
        "Channel" => {
            let channel = crate::control::manifest::parse_channel(content)?;
            Ok(json!({ "ns": channel.namespace(), "channel": channel }))
        }
        "ChannelSubscription" => {
            let subscription = crate::control::manifest::parse_channel_subscription(content)?;
            Ok(json!({
                "ns": subscription.namespace(),
                "channel": subscription.channel(),
                "subscription": subscription,
            }))
        }
        "Workflow" => {
            let workflow = crate::control::manifest::parse_workflow(content)?;
            Ok(json!({ "ns": workflow.namespace(), "workflow": workflow }))
        }
        other => anyhow::bail!("Unsupported manifest kind '{}'", other),
    }
}

pub(super) async fn rest_get_yaml(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let (path, response_key) = rest_get_path(kind, name, namespace)?;
    let resp = rest_request_json(cli, reqwest::Method::GET, &path, None)
        .await
        .with_context(|| format!("Failed to fetch {} '{}'", kind, name))?;
    let value = if response_key == "namespace" {
        resp
    } else {
        resp.get(response_key)
            .cloned()
            .or_else(|| (response_key == "card" && resp.get("cards").is_some()).then_some(resp))
            .with_context(|| format!("REST response missing {}", response_key))?
    };
    render_rest_get_yaml(response_key, value)
        .with_context(|| format!("Failed to serialize {} YAML", kind))
}

pub(super) async fn rest_get_json(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<serde_json::Value> {
    let (path, response_key) = rest_get_path(kind, name, namespace)?;
    let resp = rest_request_json(cli, reqwest::Method::GET, &path, None)
        .await
        .with_context(|| format!("Failed to fetch {} '{}'", kind, name))?;
    let value = if response_key == "namespace" {
        resp
    } else {
        resp.get(response_key)
            .cloned()
            .or_else(|| (response_key == "card" && resp.get("cards").is_some()).then_some(resp))
            .with_context(|| format!("REST response missing {}", response_key))?
    };
    match response_key {
        "resource" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let resource: crate::gateway::rpc::resources_proto::Resource =
                serde_json::from_value(value).context("Failed to decode Resource JSON")?;
            resource_manifest_json(&resource)
        }
        _ => Ok(value),
    }
}

fn render_rest_get_yaml(response_key: &str, value: serde_json::Value) -> Result<String> {
    match response_key {
        "resource" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let resource: crate::gateway::rpc::resources_proto::Resource =
                serde_json::from_value(value).context("Failed to decode Resource JSON")?;
            crate::control::manifest::render_resource_yaml(&resource)
        }
        "agent" => render_rest_agent_yaml(value),
        "namespace" => render_rest_namespace_yaml(value),
        "server" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let server: crate::gateway::rpc::manifests::McpServer =
                serde_json::from_value(value).context("Failed to decode MCPServer JSON")?;
            crate::control::manifest::render_mcp_server_yaml(&server)
        }
        "binding" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let binding: crate::gateway::rpc::manifests::McpServerBinding =
                serde_json::from_value(value).context("Failed to decode McpServerBinding JSON")?;
            crate::control::manifest::render_mcp_server_binding_yaml(&binding)
        }
        "knowledge" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let knowledge: crate::gateway::rpc::manifests::Knowledge =
                serde_json::from_value(value).context("Failed to decode Knowledge JSON")?;
            crate::control::manifest::render_knowledge_yaml(&knowledge)
        }
        "schedule" => serde_yaml::to_string(&value).context("Failed to serialize Schedule YAML"),
        "channel" => {
            let mut value = value;
            normalize_json_int64_fields(
                &mut value,
                &["createdAt", "created_at", "updatedAt", "updated_at"],
            )?;
            let channel: resources_proto::Channel =
                serde_json::from_value(value).context("Failed to decode Channel JSON")?;
            crate::control::manifest::render_channel_yaml(&channel)
        }
        "subscription" => {
            let subscription: resources_proto::ChannelSubscription = serde_json::from_value(value)
                .context("Failed to decode ChannelSubscription JSON")?;
            crate::control::manifest::render_channel_subscription_yaml(&subscription)
        }
        "workflow" => {
            let workflow: resources_proto::Workflow =
                serde_json::from_value(value).context("Failed to decode Workflow JSON")?;
            crate::control::manifest::render_workflow_yaml(&workflow)
        }
        other => anyhow::bail!("Unsupported REST response resource '{}'", other),
    }
}

fn normalize_manifest_metadata_maps(value: &mut serde_json::Value) {
    let Some(metadata) = value
        .get_mut("metadata")
        .and_then(|metadata| metadata.as_object_mut())
    else {
        return;
    };

    for key in ["labels", "annotations"] {
        if metadata.get(key).is_some_and(|value| value.is_null()) {
            metadata.insert(key.to_string(), json!({}));
        }
    }
}

fn normalize_json_int64_fields(value: &mut serde_json::Value, fields: &[&str]) -> Result<()> {
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };

    for field in fields {
        let Some(field_value) = object.get_mut(*field) else {
            continue;
        };
        let Some(raw) = field_value.as_str() else {
            continue;
        };
        let parsed = raw
            .parse::<i64>()
            .with_context(|| format!("Failed to parse {field} as int64"))?;
        *field_value = serde_json::Value::Number(parsed.into());
    }

    Ok(())
}

fn render_rest_agent_yaml(agent: serde_json::Value) -> Result<String> {
    let name = agent
        .get("name")
        .or_else(|| agent.get("agent"))
        .and_then(|name| name.as_str())
        .context("Agent response missing name")?;
    let namespace = agent
        .get("ns")
        .and_then(|namespace| namespace.as_str())
        .context("Agent response missing ns")?;
    let spec = agent
        .get("spec")
        .cloned()
        .context("Agent response missing spec")?;
    let labels = agent
        .get("labels")
        .filter(|labels| !labels.is_null())
        .cloned()
        .unwrap_or_else(|| json!({}));

    serde_yaml::to_string(&json!({
        "apiVersion": "talon.impalasys.com/v1",
        "kind": "Agent",
        "metadata": {
            "name": name,
            "namespace": namespace,
            "labels": labels,
        },
        "spec": spec,
    }))
    .context("Failed to serialize Agent YAML")
}

fn render_rest_namespace_yaml(namespace: serde_json::Value) -> Result<String> {
    let name = namespace
        .get("name")
        .and_then(|name| name.as_str())
        .context("Namespace response missing name")?;
    let labels = namespace
        .get("labels")
        .filter(|labels| !labels.is_null())
        .cloned()
        .unwrap_or_else(|| json!({}));

    serde_yaml::to_string(&json!({
        "apiVersion": "talon.impalasys.com/v1",
        "kind": "Namespace",
        "metadata": {
            "name": name,
            "labels": labels,
        },
    }))
    .context("Failed to serialize Namespace YAML")
}
