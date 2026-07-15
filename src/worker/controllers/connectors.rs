// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, bail, Context, Result};
use prost::Message;
use std::collections::{HashMap, HashSet};

use crate::control::resources::ResourceStore;
use crate::control::security::platform_jwt;
use crate::control::{keys, ControlPlane, KeyValueStore, ProtoKeyValueStoreExt};
use crate::gateway::rpc::{data_proto, external_proto, resources_proto};

const CONNECTOR_INDEX_FIELD_SEP: &str = "\x1f";
const CONNECTOR_CALLBACK_TOKEN_TTL_SECONDS: u64 = 365 * 24 * 60 * 60;

pub struct ConnectorController {
    store: ResourceStore,
}

impl ConnectorController {
    pub fn new(store: ResourceStore) -> Self {
        Self { store }
    }

    pub async fn reconcile_class(
        &self,
        class: &resources_proto::Resource,
        cp: &ControlPlane,
        config: &crate::control::config::Config,
    ) -> Result<()> {
        let meta = class
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("ConnectorClass metadata is required"))?;
        let spec = connector_class_spec(class)?;
        let registration_id = keys::connector_registration_id(&meta.namespace, &meta.name);
        if let Ok(status) = connector_class_status(class) {
            if status.observed_generation == meta.generation && status.phase == "Ready" {
                return Ok(());
            }
        }
        delete_connector_class_entries(cp.kv.as_ref(), &meta.namespace, &meta.name).await?;
        register_connector_class(
            spec,
            &meta.namespace,
            meta.name.as_str(),
            &registration_id,
            config,
        )
        .await?;
        let status = resources_proto::ConnectorClassStatus {
            observed_generation: meta.generation,
            phase: "Ready".to_string(),
            conditions: vec![condition(
                "Ready",
                "True",
                "Registered",
                "ConnectorClass registered with connector runtime",
                meta.generation,
            )],
        };
        self.store
            .patch_status(
                &meta.namespace,
                "ConnectorClass",
                &meta.name,
                None,
                resources_proto::ResourceStatus {
                    kind: Some(resources_proto::resource_status::Kind::ConnectorClass(
                        status,
                    )),
                },
            )
            .await?;
        // Reconcile Connectors that may have been waiting for this class.
        for namespace_key in cp
            .kv
            .list_keys(&keys::namespace_metadata_prefix(), None)
            .await?
        {
            let namespace = namespace_key.name;
            for connector in self.store.list(&namespace, Some("Connector")).await? {
                if connector_references_class(&connector, &meta.namespace, &meta.name) {
                    if let Err(err) = self.reconcile_connector(&connector, cp).await {
                        tracing::warn!(
                            error = %err,
                            namespace = %namespace,
                            name = %connector.metadata.as_ref().map(|meta| meta.name.as_str()).unwrap_or_default(),
                            class_namespace = %meta.namespace,
                            class_name = %meta.name,
                            "Connector reconcile failed while reconciling ConnectorClass"
                        );
                        self.reconcile_connector_error(&connector, cp, err.to_string())
                            .await?;
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn reconcile_class_error(
        &self,
        class: &resources_proto::Resource,
        cp: &ControlPlane,
        message: String,
    ) -> Result<()> {
        let meta = class
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("ConnectorClass metadata is required"))?;
        delete_connector_class_entries(cp.kv.as_ref(), &meta.namespace, &meta.name).await?;
        let status = resources_proto::ConnectorClassStatus {
            observed_generation: meta.generation,
            phase: "Error".to_string(),
            conditions: vec![condition(
                "Ready",
                "False",
                "RegistrationFailed",
                &message,
                meta.generation,
            )],
        };
        self.store
            .patch_status(
                &meta.namespace,
                "ConnectorClass",
                &meta.name,
                None,
                resources_proto::ResourceStatus {
                    kind: Some(resources_proto::resource_status::Kind::ConnectorClass(
                        status,
                    )),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn reconcile_connector(
        &self,
        connector: &resources_proto::Resource,
        cp: &ControlPlane,
    ) -> Result<()> {
        let meta = connector
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("Connector metadata is required"))?;
        let spec = connector_spec(connector)?;

        if !spec.enabled {
            delete_connector_routes_from_spec(cp.kv.as_ref(), meta, spec).await?;
            self.patch_connector_status(
                meta,
                "Disabled",
                vec![condition(
                    "Ready",
                    "False",
                    "Disabled",
                    "Connector is disabled",
                    meta.generation,
                )],
                Vec::new(),
            )
            .await?;
            return Ok(());
        }

        let class_ref = spec
            .class_ref
            .as_ref()
            .ok_or_else(|| anyhow!("Connector spec.classRef is required"))?;
        let class_namespace = connector_class_namespace(&meta.namespace, class_ref)?;
        let class = self
            .store
            .get(&class_namespace, "ConnectorClass", &class_ref.name)
            .await?
            .ok_or_else(|| anyhow!("ConnectorClass '{}' not found", class_ref.name))?;
        let class_meta = class
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("ConnectorClass metadata is required"))?;
        let class_spec = connector_class_spec(&class)?;
        let class_status = connector_class_status(&class)?;
        if class_status.phase != "Ready" {
            bail!("ConnectorClass '{}' is not Ready", class_ref.name);
        }

        if let Ok(status) = connector_status(connector) {
            if status.observed_generation == meta.generation
                && status.phase == "Ready"
                && !status.compiled_route_ids.is_empty()
            {
                let mut intact = true;
                for key_name in &status.compiled_route_ids {
                    match cp
                        .kv
                        .get_msg::<data_proto::Route>(&keys::connector_route(
                            &class_meta.namespace,
                            &class_meta.name,
                            key_name,
                        ))
                        .await?
                    {
                        Some(entry) if entry.connector_uid == meta.uid => {}
                        _ => {
                            intact = false;
                            break;
                        }
                    }
                }
                if intact {
                    return Ok(());
                }
            }
        }
        delete_route_entries_for_uid(
            cp.kv.as_ref(),
            &class_meta.namespace,
            &class_meta.name,
            &meta.uid,
        )
        .await?;

        validate_consumer(meta.namespace.as_str(), spec.consumer.as_ref())?;
        if spec.match_fields.is_empty() {
            bail!("Connector spec.matchFields must not be empty");
        }

        let compiled = compile_connector_route_ids(class_spec, &spec.match_fields)?;
        if compiled.is_empty() {
            bail!("Connector matchFields do not satisfy any ConnectorClass match index");
        }

        let mut written = Vec::new();
        for key_name in compiled {
            let key = keys::connector_route(&class_meta.namespace, &class_meta.name, &key_name);
            let route = data_proto::Route {
                connector_uid: meta.uid.clone(),
                connector: Some(data_proto::ResourceRef {
                    namespace: meta.namespace.clone(),
                    name: meta.name.clone(),
                }),
                consumer: spec.consumer.clone(),
            };
            match cp.kv.get_msg::<data_proto::Route>(&key).await? {
                Some(existing) if existing.connector_uid != meta.uid => {
                    let connector_ref = existing.connector.as_ref();
                    bail!(
                        "Connector match conflicts with {}/{}",
                        connector_ref
                            .map(|reference| reference.namespace.as_str())
                            .unwrap_or_default(),
                        connector_ref
                            .map(|reference| reference.name.as_str())
                            .unwrap_or_default()
                    );
                }
                _ => {
                    cp.kv.set_msg(&key, &route).await?;
                    written.push(key_name);
                }
            }
        }

        self.patch_connector_status(
            meta,
            "Ready",
            vec![condition(
                "Ready",
                "True",
                "Indexed",
                "Connector match keys indexed",
                meta.generation,
            )],
            written,
        )
        .await?;
        Ok(())
    }

    pub async fn reconcile_connector_error(
        &self,
        connector: &resources_proto::Resource,
        cp: &ControlPlane,
        message: String,
    ) -> Result<()> {
        let meta = connector
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("Connector metadata is required"))?;
        if let Ok(spec) = connector_spec(connector) {
            delete_connector_routes_from_spec(cp.kv.as_ref(), meta, spec).await?;
        }
        self.patch_connector_status(
            meta,
            "Error",
            vec![condition(
                "Ready",
                "False",
                "IndexFailed",
                &message,
                meta.generation,
            )],
            Vec::new(),
        )
        .await
    }

    async fn patch_connector_status(
        &self,
        meta: &resources_proto::ResourceMeta,
        phase: &str,
        conditions: Vec<resources_proto::ResourceCondition>,
        compiled_route_ids: Vec<String>,
    ) -> Result<()> {
        self.store
            .patch_status(
                &meta.namespace,
                "Connector",
                &meta.name,
                None,
                resources_proto::ResourceStatus {
                    kind: Some(resources_proto::resource_status::Kind::Connector(
                        resources_proto::ConnectorStatus {
                            observed_generation: meta.generation,
                            phase: phase.to_string(),
                            conditions,
                            compiled_route_ids,
                        },
                    )),
                },
            )
            .await?;
        Ok(())
    }
}

pub async fn delete_route_entries_for_uid(
    kv: &dyn KeyValueStore,
    class_namespace: &str,
    class_name: &str,
    connector_uid: &str,
) -> Result<()> {
    if connector_uid.is_empty() {
        return Ok(());
    }
    for (key, bytes) in kv
        .list_entries(
            &keys::connector_route_prefix(class_namespace, class_name),
            None,
        )
        .await?
    {
        let Ok(route) = data_proto::Route::decode(bytes.as_slice()) else {
            continue;
        };
        if route.connector_uid == connector_uid {
            kv.delete(&key).await?;
        }
    }
    Ok(())
}

async fn delete_connector_routes_from_spec(
    kv: &dyn KeyValueStore,
    meta: &resources_proto::ResourceMeta,
    spec: &resources_proto::ConnectorSpec,
) -> Result<()> {
    let Some(class_ref) = spec.class_ref.as_ref() else {
        return Ok(());
    };
    let class_namespace = if class_ref.namespace.trim().is_empty() {
        meta.namespace.as_str()
    } else {
        class_ref.namespace.as_str()
    };
    delete_route_entries_for_uid(kv, class_namespace, &class_ref.name, &meta.uid).await
}

pub async fn delete_connector_class_entries(
    kv: &dyn KeyValueStore,
    class_namespace: &str,
    class_name: &str,
) -> Result<()> {
    delete_entries(
        kv,
        &keys::connector_route_prefix(class_namespace, class_name),
    )
    .await?;
    delete_entries(
        kv,
        &keys::connector_event_prefix(class_namespace, class_name),
    )
    .await?;
    delete_entries(
        kv,
        &keys::connector_session_prefix(class_namespace, class_name),
    )
    .await?;
    Ok(())
}

async fn delete_entries(kv: &dyn KeyValueStore, prefix: &keys::ResourceList) -> Result<()> {
    for key in kv.list_keys(prefix, None).await? {
        kv.delete(&key).await?;
    }
    Ok(())
}

pub async fn resolve_route(
    kv: &dyn KeyValueStore,
    class_namespace: &str,
    class_name: &str,
    class_spec: &resources_proto::ConnectorClassSpec,
    fields: &HashMap<String, String>,
) -> Result<Option<data_proto::Route>> {
    for key_name in compile_route_ids(class_spec, fields)? {
        let key = keys::connector_route(class_namespace, class_name, &key_name);
        if let Some(entry) = kv.get_msg::<data_proto::Route>(&key).await? {
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

pub fn compile_route_ids(
    class_spec: &resources_proto::ConnectorClassSpec,
    fields: &HashMap<String, String>,
) -> Result<Vec<String>> {
    Ok(compile_satisfied_route_ids(class_spec, fields)?
        .into_iter()
        .map(|route| route.key)
        .collect())
}

fn compile_connector_route_ids(
    class_spec: &resources_proto::ConnectorClassSpec,
    fields: &HashMap<String, String>,
) -> Result<Vec<String>> {
    let routes = compile_satisfied_route_ids(class_spec, fields)?;
    Ok(routes
        .iter()
        .filter(|route| {
            !routes.iter().any(|candidate| {
                candidate.fields.len() > route.fields.len()
                    && candidate.fields.is_superset(&route.fields)
            })
        })
        .map(|route| route.key.clone())
        .collect())
}

#[derive(Clone, Debug)]
struct CompiledRouteId {
    key: String,
    fields: HashSet<String>,
}

fn compile_satisfied_route_ids(
    class_spec: &resources_proto::ConnectorClassSpec,
    fields: &HashMap<String, String>,
) -> Result<Vec<CompiledRouteId>> {
    let mut routes = Vec::new();
    let mut seen = HashSet::new();
    for index in &class_spec.match_indexes {
        if index.name.trim().is_empty() || index.fields.is_empty() {
            continue;
        }
        let mut segments = Vec::new();
        let mut route_fields = HashSet::new();
        let mut complete = true;
        for field in &index.fields {
            match fields.get(field).filter(|value| !value.trim().is_empty()) {
                Some(value) => {
                    route_fields.insert(field.clone());
                    segments.push(format!(
                        "{}={}",
                        encode_match_component(field),
                        encode_match_component(value)
                    ));
                }
                None => {
                    complete = false;
                    break;
                }
            }
        }
        if complete {
            let key = format!(
                "{}{}{}",
                encode_match_component(&index.name),
                CONNECTOR_INDEX_FIELD_SEP,
                segments.join(CONNECTOR_INDEX_FIELD_SEP)
            );
            if seen.insert(key.clone()) {
                routes.push(CompiledRouteId {
                    key,
                    fields: route_fields,
                });
            }
        }
    }
    Ok(routes)
}

fn encode_match_component(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}

fn connector_spec(resource: &resources_proto::Resource) -> Result<&resources_proto::ConnectorSpec> {
    match resource.spec.as_ref().and_then(|spec| spec.kind.as_ref()) {
        Some(resources_proto::resource_spec::Kind::Connector(spec)) => Ok(spec),
        _ => Err(anyhow!("Connector resource is missing Connector spec")),
    }
}

fn connector_class_spec(
    resource: &resources_proto::Resource,
) -> Result<&resources_proto::ConnectorClassSpec> {
    match resource.spec.as_ref().and_then(|spec| spec.kind.as_ref()) {
        Some(resources_proto::resource_spec::Kind::ConnectorClass(spec)) => Ok(spec),
        _ => Err(anyhow!(
            "ConnectorClass resource is missing ConnectorClass spec"
        )),
    }
}

fn connector_class_status(
    resource: &resources_proto::Resource,
) -> Result<&resources_proto::ConnectorClassStatus> {
    match resource
        .status
        .as_ref()
        .and_then(|status| status.kind.as_ref())
    {
        Some(resources_proto::resource_status::Kind::ConnectorClass(status)) => Ok(status),
        _ => Err(anyhow!("ConnectorClass resource is missing status")),
    }
}

fn connector_status(
    resource: &resources_proto::Resource,
) -> Result<&resources_proto::ConnectorStatus> {
    match resource
        .status
        .as_ref()
        .and_then(|status| status.kind.as_ref())
    {
        Some(resources_proto::resource_status::Kind::Connector(status)) => Ok(status),
        _ => Err(anyhow!("Connector resource is missing status")),
    }
}

fn connector_references_class(
    resource: &resources_proto::Resource,
    class_namespace: &str,
    class_name: &str,
) -> bool {
    let connector_namespace = resource
        .metadata
        .as_ref()
        .map(|meta| meta.namespace.as_str())
        .unwrap_or_default();
    connector_spec(resource)
        .ok()
        .and_then(|spec| spec.class_ref.as_ref())
        .map(|class_ref| {
            let Ok(referenced_namespace) =
                connector_class_namespace(connector_namespace, class_ref)
            else {
                return false;
            };
            class_ref.name == class_name && referenced_namespace == class_namespace
        })
        .unwrap_or(false)
}

fn connector_class_namespace(
    connector_namespace: &str,
    class_ref: &resources_proto::ResourceRef,
) -> Result<String> {
    let class_namespace = class_ref.namespace.trim();
    if class_namespace.is_empty() {
        return Ok(connector_namespace.to_string());
    }
    if namespace_is_self_or_ancestor(connector_namespace, class_namespace) {
        return Ok(class_namespace.to_string());
    }
    bail!(
        "Connector spec.classRef.namespace must be empty, match Connector namespace, or name an ancestor namespace"
    )
}

fn namespace_is_self_or_ancestor(namespace: &str, candidate_ancestor: &str) -> bool {
    namespace == candidate_ancestor
        || namespace
            .strip_prefix(candidate_ancestor)
            .is_some_and(|suffix| suffix.starts_with(':'))
}

fn validate_consumer(
    connector_namespace: &str,
    consumer: Option<&data_proto::MessageConsumer>,
) -> Result<()> {
    let consumer = consumer.ok_or_else(|| anyhow!("Connector spec.consumer is required"))?;
    match (
        consumer.session.as_ref(),
        consumer.channel.as_ref(),
        consumer.workflow.as_ref(),
    ) {
        (Some(session), None, None) => {
            let agent = session
                .agent
                .as_ref()
                .ok_or_else(|| anyhow!("Connector session consumer requires agent"))?;
            validate_local_ref(connector_namespace, "session consumer agent", agent)?;
            if !session.session_id.trim().is_empty() {
                if !session.continuity.trim().is_empty()
                    && !session.continuity.eq_ignore_ascii_case("pinned")
                {
                    bail!("Connector session consumer sessionId requires pinned continuity");
                }
            } else if session.continuity.eq_ignore_ascii_case("pinned") {
                bail!("Connector session consumer pinned continuity requires sessionId");
            }
        }
        (None, Some(channel), None) => {
            let channel_ref = channel
                .channel
                .as_ref()
                .ok_or_else(|| anyhow!("Connector channel consumer requires channel"))?;
            validate_local_ref(connector_namespace, "channel consumer channel", channel_ref)?;
            let agent = channel
                .agent
                .as_ref()
                .ok_or_else(|| anyhow!("Connector channel consumer requires agent"))?;
            validate_local_ref(connector_namespace, "channel consumer agent", agent)?;
        }
        (None, None, Some(workflow)) => {
            validate_local_name_namespace(
                connector_namespace,
                "workflow consumer workflow",
                &workflow.name,
                &workflow.namespace,
            )?;
        }
        (Some(_), _, _) | (_, Some(_), Some(_)) => {
            bail!("Connector consumer must set only one of session, channel, or workflow")
        }
        (None, None, None) => bail!("Connector consumer must set session, channel, or workflow"),
    }
    Ok(())
}

fn validate_local_ref(
    connector_namespace: &str,
    field: &str,
    reference: &data_proto::ResourceRef,
) -> Result<()> {
    validate_local_name_namespace(
        connector_namespace,
        field,
        &reference.name,
        &reference.namespace,
    )
}

fn validate_local_name_namespace(
    connector_namespace: &str,
    field: &str,
    name: &str,
    namespace: &str,
) -> Result<()> {
    if name.trim().is_empty() {
        bail!("Connector {field} requires name");
    }
    if !namespace.trim().is_empty() && namespace != connector_namespace {
        bail!("Connector {field} namespace must be empty or match Connector namespace");
    }
    Ok(())
}

async fn register_connector_class(
    spec: &resources_proto::ConnectorClassSpec,
    class_namespace: &str,
    class_name: &str,
    registration_id: &str,
    config: &crate::control::config::Config,
) -> Result<()> {
    let runtime = spec
        .runtime
        .as_ref()
        .ok_or_else(|| anyhow!("ConnectorClass runtime is required"))?;
    if runtime.endpoint.trim().is_empty() {
        bail!("ConnectorClass runtime.endpoint is required");
    }
    let auth = spec
        .auth
        .as_ref()
        .ok_or_else(|| anyhow!("ConnectorClass auth is required"))?;
    if auth.kind != "apiKey" {
        bail!("ConnectorClass auth.kind must be apiKey");
    }
    let api_key = auth
        .api_key
        .as_ref()
        .ok_or_else(|| anyhow!("ConnectorClass auth.apiKey is required"))?
        .resolve_connector_api_key()
        .context("failed to resolve ConnectorClass api key")?;
    let callback_base_url = std::env::var("TALON_GATEWAY_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("{}/v1/connectors", value.trim_end_matches('/')))
        .unwrap_or_else(|| {
            let server = config.server.as_ref();
            let configured_host = server.map(|server| server.host.trim()).unwrap_or_default();
            let host = if configured_host.is_empty() {
                "127.0.0.1"
            } else {
                configured_host
            };
            let configured_port = server.map(|server| server.port).unwrap_or_default();
            let port = if configured_port == 0 {
                8080
            } else {
                configured_port
            };
            format!("http://{host}:{port}/v1/connectors")
        });
    let cluster_id =
        std::env::var("TALON_CONNECTOR_CLUSTER_ID").unwrap_or_else(|_| "talon-cluster".into());
    let callback_auth_key = mint_connector_callback_token(class_namespace)
        .context("failed to mint connector callback auth token")?;
    let url = format!(
        "{}/v1/clusters/register",
        runtime.endpoint.trim_end_matches('/')
    );
    let response = reqwest::Client::new()
        .post(url)
        .bearer_auth(api_key)
        .json(&external_proto::RegisterClusterRequest {
            cluster_id,
            registration_id: registration_id.to_string(),
            namespace: class_namespace.to_string(),
            connector_class: class_name.to_string(),
            callback_base_url,
            callback_auth_kind: "bearer".to_string(),
            callback_auth_key,
            protocol_version: "v1".to_string(),
        })
        .send()
        .await
        .context("failed to register ConnectorClass with connector runtime")?;
    if !response.status().is_success() {
        bail!(
            "connector runtime registration failed for {class_name}: HTTP {}",
            response.status()
        );
    }
    let body = response
        .json::<external_proto::RegisterClusterResponse>()
        .await
        .context("failed to decode connector runtime registration response")?;
    if let Some(returned_registration_id) = body
        .registration_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if returned_registration_id != registration_id {
            bail!(
                "connector runtime returned registrationId '{}' but Talon assigned '{}'",
                returned_registration_id,
                registration_id
            );
        }
    }
    Ok(())
}

trait ConnectorSecretExt {
    fn resolve_connector_api_key(&self) -> Result<String>;
}

impl ConnectorSecretExt for resources_proto::ConnectorSecretRef {
    fn resolve_connector_api_key(&self) -> Result<String> {
        match (
            self.plain
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
            self.env
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        ) {
            (Some(value), None) => Ok(value.to_string()),
            (None, Some(env)) => std::env::var(env)
                .with_context(|| format!("ConnectorClass auth.apiKey env '{env}' is not set")),
            (Some(_), Some(_)) => {
                bail!("ConnectorClass auth.apiKey must set only one of plain or env")
            }
            (None, None) => bail!("ConnectorClass auth.apiKey must set plain or env"),
        }
    }
}

fn mint_connector_callback_token(namespace: &str) -> Result<String> {
    let issuer = platform_jwt::issuer()
        .context("platform JWT issuer is required for connector callbacks")?;
    let key =
        platform_jwt::load_key().context("platform JWT key is required for connector callbacks")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let claims = crate::gateway::auth::Claims {
        iss: Some(issuer),
        sub: "connector-runtime".to_string(),
        aud: platform_jwt::TALON_GATEWAY_AUDIENCE.to_string(),
        iat: Some(now as usize),
        exp: now
            .checked_add(CONNECTOR_CALLBACK_TOKEN_TTL_SECONDS)
            .context("connector callback token ttl is too large")? as usize,
        ns: Some(namespace.to_string()),
        agent: None,
        session: None,
        channel: None,
        origins: Vec::new(),
        grants: vec![crate::gateway::auth::TalonGrantClaim {
            kind: "readwrite".to_string(),
            namespace: Some(namespace.to_string()),
            agent: None,
            session: None,
            channel: None,
        }],
    };
    key.sign(&claims)
        .context("failed to sign connector callback token")
}

fn condition(
    condition_type: &str,
    status: &str,
    reason: &str,
    message: &str,
    observed_generation: u64,
) -> resources_proto::ResourceCondition {
    resources_proto::ResourceCondition {
        r#type: condition_type.to_string(),
        status: status.to_string(),
        reason: reason.to_string(),
        message: message.to_string(),
        last_transition_time: chrono::Utc::now().timestamp_micros(),
        observed_generation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_route_ids_for_events_uses_declared_index_order_and_complete_fields() {
        let class_spec = resources_proto::ConnectorClassSpec {
            platform: "slack".to_string(),
            runtime: None,
            auth: None,
            match_indexes: vec![
                resources_proto::ConnectorMatchIndex {
                    name: "slack-channel".to_string(),
                    fields: vec!["teamId".to_string(), "channelId".to_string()],
                },
                resources_proto::ConnectorMatchIndex {
                    name: "slack-team".to_string(),
                    fields: vec!["teamId".to_string()],
                },
                resources_proto::ConnectorMatchIndex {
                    name: "missing-enterprise".to_string(),
                    fields: vec!["enterpriseId".to_string(), "teamId".to_string()],
                },
            ],
        };
        let fields = HashMap::from([
            ("teamId".to_string(), "T123".to_string()),
            ("channelId".to_string(), "C999".to_string()),
        ]);

        let keys = compile_route_ids(&class_spec, &fields).unwrap();

        assert_eq!(
            keys,
            vec![
                "slack-channel\u{1f}teamId=T123\u{1f}channelId=C999".to_string(),
                "slack-team\u{1f}teamId=T123".to_string(),
            ]
        );
    }

    #[test]
    fn compile_connector_route_ids_uses_only_most_specific_imessage_route() {
        let class_spec = imessage_class_spec();
        let fields = HashMap::from([
            ("lineId".to_string(), "shared".to_string()),
            ("participantId".to_string(), "+13025073162".to_string()),
        ]);

        let keys = compile_connector_route_ids(&class_spec, &fields).unwrap();

        assert_eq!(
            keys,
            vec!["line-space\u{1f}lineId=shared\u{1f}participantId=%2B13025073162".to_string()]
        );
    }

    #[test]
    fn compile_connector_route_ids_keeps_broad_imessage_route_when_only_broad_fields_match() {
        let class_spec = imessage_class_spec();
        let fields = HashMap::from([("lineId".to_string(), "shared".to_string())]);

        let keys = compile_connector_route_ids(&class_spec, &fields).unwrap();

        assert_eq!(keys, vec!["line\u{1f}lineId=shared".to_string()]);
    }

    #[test]
    fn compile_connector_route_ids_keeps_multiple_equally_specific_routes() {
        let class_spec = resources_proto::ConnectorClassSpec {
            platform: "chat".to_string(),
            runtime: None,
            auth: None,
            match_indexes: vec![
                resources_proto::ConnectorMatchIndex {
                    name: "team-channel".to_string(),
                    fields: vec!["teamId".to_string(), "channelId".to_string()],
                },
                resources_proto::ConnectorMatchIndex {
                    name: "team-thread".to_string(),
                    fields: vec!["teamId".to_string(), "threadId".to_string()],
                },
                resources_proto::ConnectorMatchIndex {
                    name: "team".to_string(),
                    fields: vec!["teamId".to_string()],
                },
            ],
        };
        let fields = HashMap::from([
            ("teamId".to_string(), "T123".to_string()),
            ("channelId".to_string(), "C999".to_string()),
            ("threadId".to_string(), "THR".to_string()),
        ]);

        let keys = compile_connector_route_ids(&class_spec, &fields).unwrap();

        assert_eq!(
            keys,
            vec![
                "team-channel\u{1f}teamId=T123\u{1f}channelId=C999".to_string(),
                "team-thread\u{1f}teamId=T123\u{1f}threadId=THR".to_string(),
            ]
        );
    }

    #[test]
    fn compile_connector_route_ids_drops_strict_subset_indexes_regardless_of_order() {
        let class_spec = resources_proto::ConnectorClassSpec {
            platform: "chat".to_string(),
            runtime: None,
            auth: None,
            match_indexes: vec![
                resources_proto::ConnectorMatchIndex {
                    name: "team".to_string(),
                    fields: vec!["teamId".to_string()],
                },
                resources_proto::ConnectorMatchIndex {
                    name: "team-channel".to_string(),
                    fields: vec!["teamId".to_string(), "channelId".to_string()],
                },
            ],
        };
        let fields = HashMap::from([
            ("teamId".to_string(), "T123".to_string()),
            ("channelId".to_string(), "C999".to_string()),
        ]);

        let keys = compile_connector_route_ids(&class_spec, &fields).unwrap();

        assert_eq!(
            keys,
            vec!["team-channel\u{1f}teamId=T123\u{1f}channelId=C999".to_string()]
        );
    }

    #[test]
    fn compile_route_ids_escapes_separator_characters() {
        let class_spec = resources_proto::ConnectorClassSpec {
            platform: "slack".to_string(),
            runtime: None,
            auth: None,
            match_indexes: vec![resources_proto::ConnectorMatchIndex {
                name: "team|channel".to_string(),
                fields: vec!["team|id".to_string(), "channel\u{1f}id".to_string()],
            }],
        };
        let fields = HashMap::from([
            ("team|id".to_string(), "T|123".to_string()),
            (
                "channel\u{1f}id".to_string(),
                "C\u{1f}999=value".to_string(),
            ),
        ]);

        let keys = compile_route_ids(&class_spec, &fields).unwrap();

        assert_eq!(
            keys,
            vec![
                "team%7Cchannel\u{1f}team%7Cid=T%7C123\u{1f}channel%1Fid=C%1F999%3Dvalue"
                    .to_string()
            ]
        );
    }

    #[test]
    fn connector_class_namespace_defaults_to_connector_namespace() {
        let class_ref = resources_proto::ResourceRef {
            namespace: String::new(),
            name: "slack".to_string(),
        };

        let namespace = connector_class_namespace("Tenant:conic:Customers:13", &class_ref).unwrap();

        assert_eq!(namespace, "Tenant:conic:Customers:13");
    }

    #[test]
    fn connector_class_namespace_accepts_self_or_ancestor_namespace() {
        let self_ref = resources_proto::ResourceRef {
            namespace: "Tenant:conic:Customers:13".to_string(),
            name: "slack".to_string(),
        };
        let parent_ref = resources_proto::ResourceRef {
            namespace: "Tenant:conic:Customers".to_string(),
            name: "slack".to_string(),
        };

        assert_eq!(
            connector_class_namespace("Tenant:conic:Customers:13", &self_ref).unwrap(),
            "Tenant:conic:Customers:13"
        );
        assert_eq!(
            connector_class_namespace("Tenant:conic:Customers:13", &parent_ref).unwrap(),
            "Tenant:conic:Customers"
        );
    }

    #[test]
    fn connector_class_namespace_rejects_sibling_child_and_prefix_matches() {
        for class_namespace in [
            "Tenant:conic:Customers:12",
            "Tenant:conic:Customers:13:Child",
            "Tenant:conic:Customers2",
        ] {
            let class_ref = resources_proto::ResourceRef {
                namespace: class_namespace.to_string(),
                name: "slack".to_string(),
            };
            let err = connector_class_namespace("Tenant:conic:Customers:13", &class_ref)
                .unwrap_err()
                .to_string();

            assert!(err.contains("ancestor namespace"), "{err}");
        }
    }

    #[test]
    fn connector_references_class_allows_parent_class_namespace() {
        let connector = resources_proto::Resource {
            metadata: Some(resources_proto::ResourceMeta {
                namespace: "Tenant:conic:Customers:13".to_string(),
                name: "slack".to_string(),
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::Connector(
                    resources_proto::ConnectorSpec {
                        class_ref: Some(resources_proto::ResourceRef {
                            namespace: "Tenant:conic:Customers".to_string(),
                            name: "slack".to_string(),
                        }),
                        enabled: true,
                        ..Default::default()
                    },
                )),
            }),
            ..Default::default()
        };

        assert!(connector_references_class(
            &connector,
            "Tenant:conic:Customers",
            "slack"
        ));
        assert!(!connector_references_class(
            &connector,
            "Tenant:conic:Customers:13",
            "slack"
        ));
    }

    fn imessage_class_spec() -> resources_proto::ConnectorClassSpec {
        resources_proto::ConnectorClassSpec {
            platform: "imessage".to_string(),
            runtime: None,
            auth: None,
            match_indexes: vec![
                resources_proto::ConnectorMatchIndex {
                    name: "line-space".to_string(),
                    fields: vec!["lineId".to_string(), "participantId".to_string()],
                },
                resources_proto::ConnectorMatchIndex {
                    name: "line".to_string(),
                    fields: vec!["lineId".to_string()],
                },
            ],
        }
    }
}
