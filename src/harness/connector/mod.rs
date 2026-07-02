// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::resources::ResourceStore;
use crate::control::{keys, ControlPlane};
use crate::gateway::rpc::{data_proto, external_proto, resources_proto};
use anyhow::Context;
use std::collections::HashMap;
use std::time::Duration;

pub const LABEL_CONNECTOR: &str = "talon.impalasys.com/connector";
pub const LABEL_CONNECTOR_CLASS: &str = "talon.impalasys.com/connector-class";
pub const LABEL_CONNECTOR_REGISTRATION: &str = "talon.impalasys.com/connector-registration";
pub const LABEL_CONNECTOR_EVENT: &str = "talon.impalasys.com/connector-event";
pub const LABEL_EXTERNAL_CONVERSATION: &str = "talon.impalasys.com/external-conversation";
pub const LABEL_EXTERNAL_THREAD: &str = "talon.impalasys.com/external-thread";
pub const LABEL_EXTERNAL_MESSAGE: &str = "talon.impalasys.com/external-message";
pub const LABEL_CONNECTOR_REPLY_MODE: &str = "talon.impalasys.com/connector-reply-mode";
pub const LABEL_CONNECTOR_MATCH_PREFIX: &str = "talon.impalasys.com/connector-match/";

const CONNECTOR_HTTP_TIMEOUT: Duration = Duration::from_secs(15);

struct ConnectorClassRegistration {
    spec: resources_proto::ConnectorClassSpec,
}

pub async fn deliver_connector_reply_from_labels(
    cp: &ControlPlane,
    labels: &HashMap<String, String>,
    namespace: &str,
    delivery_id: &str,
    text: &str,
    attachments: Vec<data_proto::ObjectRef>,
    reply_mode: &str,
) -> anyhow::Result<()> {
    let registration_id = required_label(labels, LABEL_CONNECTOR_REGISTRATION)?;
    let connector_name = required_label(labels, LABEL_CONNECTOR)?;
    let connector_class = required_label(labels, LABEL_CONNECTOR_CLASS)?;
    let external_conversation_id = required_label(labels, LABEL_EXTERNAL_CONVERSATION)?;
    let (runtime_endpoint, api_key) =
        connector_runtime_endpoint_and_api_key(cp, registration_id, connector_class).await?;

    let mut match_fields = HashMap::new();
    for (key, value) in labels {
        if let Some(field) = key.strip_prefix(LABEL_CONNECTOR_MATCH_PREFIX) {
            match_fields.insert(field.to_string(), value.clone());
        }
    }

    let source_external_message_id = labels.get(LABEL_EXTERNAL_MESSAGE).cloned();
    let external_thread_id = if reply_mode == "thread" {
        labels
            .get(LABEL_EXTERNAL_THREAD)
            .cloned()
            .or_else(|| source_external_message_id.clone())
    } else {
        labels.get(LABEL_EXTERNAL_THREAD).cloned()
    };
    let reply_to_external_message_id = if reply_mode == "thread" {
        source_external_message_id
    } else {
        None
    };

    let mut delivery_labels = HashMap::new();
    delivery_labels.insert("talon.replySource".to_string(), "workflow".to_string());
    if let Some(source_event) = labels.get(LABEL_CONNECTOR_EVENT).cloned() {
        delivery_labels.insert("talon.connectorEvent".to_string(), source_event);
    }

    let response = connector_http_client()?
        .post(format!("{}/v1/deliveries", runtime_endpoint))
        .bearer_auth(api_key)
        .json(&external_proto::ConnectorDeliveryRequest {
            delivery_id: delivery_id.to_string(),
            registration_id: registration_id.to_string(),
            connector_class: connector_class.to_string(),
            namespace: namespace.to_string(),
            connector_name: connector_name.to_string(),
            match_fields,
            external_conversation_id: external_conversation_id.to_string(),
            external_thread_id,
            reply_to_external_message_id,
            text: text.trim().to_string(),
            attachments,
            labels: delivery_labels,
        })
        .send()
        .await
        .context("failed to submit connector workflow reply delivery")?;
    let status = response.status();
    let body = response
        .json::<external_proto::ConnectorDeliveryResponse>()
        .await
        .context("failed to decode connector workflow reply response")?;
    if !status.is_success() || !body.accepted {
        anyhow::bail!(
            "connector workflow reply rejected: HTTP {status} disposition={} error={}",
            body.disposition,
            body.error
        );
    }
    Ok(())
}

fn required_label<'a>(labels: &'a HashMap<String, String>, name: &str) -> anyhow::Result<&'a str> {
    labels
        .get(name)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing connector label {name}"))
}

async fn connector_runtime_endpoint_and_api_key(
    cp: &ControlPlane,
    registration_id: &str,
    connector_class: &str,
) -> anyhow::Result<(String, String)> {
    let class = connector_class_registration(cp, registration_id, connector_class).await?;
    let runtime = class
        .spec
        .runtime
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("connector class runtime is required"))?;
    if runtime.endpoint.trim().is_empty() {
        anyhow::bail!("connector class runtime endpoint is required");
    }
    let auth = class
        .spec
        .auth
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("connector class auth is required"))?;
    if auth.kind != "apiKey" {
        anyhow::bail!("connector class auth.kind must be apiKey");
    }
    let api_key = auth
        .api_key
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("connector class auth.apiKey is required"))
        .and_then(resolve_connector_secret)?;
    Ok((runtime.endpoint.trim_end_matches('/').to_string(), api_key))
}

async fn connector_class_registration(
    cp: &ControlPlane,
    registration_id: &str,
    requested_class: &str,
) -> anyhow::Result<ConnectorClassRegistration> {
    let (namespace, name) = keys::parse_connector_registration_id(registration_id)?;
    if !requested_class.trim().is_empty() && requested_class != name {
        anyhow::bail!("connector_class conflicts with ConnectorClass registration");
    }
    let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    let class = store
        .get(&namespace, "ConnectorClass", &name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("ConnectorClass registration not found"))?;
    match class
        .status
        .as_ref()
        .and_then(|status| status.kind.as_ref())
    {
        Some(resources_proto::resource_status::Kind::ConnectorClass(status))
            if status.phase == "Ready" => {}
        _ => anyhow::bail!("ConnectorClass registration is not Ready"),
    }
    let spec = match class.spec.as_ref().and_then(|spec| spec.kind.as_ref()) {
        Some(resources_proto::resource_spec::Kind::ConnectorClass(spec)) => spec.clone(),
        _ => anyhow::bail!("ConnectorClass registration missing spec"),
    };
    Ok(ConnectorClassRegistration { spec })
}

fn resolve_connector_secret(
    secret: &resources_proto::ConnectorSecretRef,
) -> anyhow::Result<String> {
    match (
        secret
            .plain
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        secret
            .env
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    ) {
        (Some(value), None) => Ok(value.to_string()),
        (None, Some(env)) => std::env::var(env)
            .with_context(|| format!("ConnectorClass auth.apiKey env '{env}' is not set")),
        (Some(_), Some(_)) => {
            anyhow::bail!("ConnectorClass auth.apiKey must set only one of plain or env")
        }
        (None, None) => anyhow::bail!("ConnectorClass auth.apiKey must set plain or env"),
    }
}

fn connector_http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(CONNECTOR_HTTP_TIMEOUT)
        .build()
        .context("failed to build connector HTTP client")
}
