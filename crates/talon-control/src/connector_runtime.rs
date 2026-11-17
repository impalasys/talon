// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{keys, resources::ResourceStore, ControlPlane};
use anyhow::Context;
use talon_protocol::resources_proto;
pub struct ConnectorClassRegistration {
    pub namespace: String,
    pub name: String,
    pub spec: resources_proto::ConnectorClassSpec,
}

pub async fn connector_runtime_endpoint_and_api_key(
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

pub async fn connector_class_registration(
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
    Ok(ConnectorClassRegistration {
        namespace,
        name,
        spec,
    })
}

pub fn resolve_connector_secret(
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
