// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, bail, Context, Result};
use prost::Message;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::control::resources::ResourceStore;
use crate::control::{keys, ns, ControlPlane, KeyValueStore, ProtoKeyValueStoreExt};
use crate::gateway::rpc::resources_proto;

const CONNECTOR_INDEX_NAME_SEP: &str = "|";
const CONNECTOR_INDEX_FIELD_SEP: &str = "\x1f";

#[derive(Debug, Serialize)]
struct RegisterClusterRequest {
    #[serde(rename = "clusterId")]
    cluster_id: String,
    #[serde(rename = "callbackBaseUrl")]
    callback_base_url: String,
    #[serde(rename = "protocolVersion")]
    protocol_version: String,
}

#[derive(Debug, Deserialize)]
struct RegisterClusterResponse {
    #[serde(rename = "registrationId")]
    registration_id: Option<String>,
}

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
        let registration_id = register_connector_class(spec, meta.name.as_str(), config).await?;
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
            registration_id,
        };
        self.store
            .patch_status(
                ns::TALON_SYSTEM,
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
        for namespace_key in cp.kv.list_keys(&keys::namespace_metadata_prefix()).await? {
            let namespace = namespace_key.name;
            for connector in self.store.list(&namespace, Some("Connector")).await? {
                if connector_references_class(&connector, &meta.name) {
                    self.reconcile_connector(&connector, cp).await?;
                }
            }
        }
        Ok(())
    }

    pub async fn reconcile_class_error(
        &self,
        class: &resources_proto::Resource,
        message: String,
    ) -> Result<()> {
        let meta = class
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("ConnectorClass metadata is required"))?;
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
            registration_id: String::new(),
        };
        self.store
            .patch_status(
                ns::TALON_SYSTEM,
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
        delete_match_entries_for_uid(cp.kv.as_ref(), &meta.uid).await?;

        if !spec.enabled {
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
        let class_namespace = if class_ref.namespace.trim().is_empty() {
            ns::TALON_SYSTEM
        } else {
            class_ref.namespace.as_str()
        };
        let class = self
            .store
            .get(class_namespace, "ConnectorClass", &class_ref.name)
            .await?
            .ok_or_else(|| anyhow!("ConnectorClass '{}' not found", class_ref.name))?;
        let class_meta = class
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("ConnectorClass metadata is required"))?;
        let class_spec = connector_class_spec(&class)?;
        let class_status = connector_class_status(&class)?;
        if class_status.phase != "Ready" || class_status.registration_id.trim().is_empty() {
            bail!("ConnectorClass '{}' is not Ready", class_ref.name);
        }

        validate_target(spec.target.as_ref())?;
        if spec.match_fields.is_empty() {
            bail!("Connector spec.matchFields must not be empty");
        }

        let compiled = compile_match_keys(
            &class_status.registration_id,
            class_meta.name.as_str(),
            class_spec,
            &spec.match_fields,
        )?;
        if compiled.is_empty() {
            bail!("Connector matchFields do not satisfy any ConnectorClass match index");
        }

        let mut written = Vec::new();
        for key_name in compiled {
            let key = keys::connector_match(&key_name);
            let entry = resources_proto::ConnectorMatchEntry {
                connector_uid: meta.uid.clone(),
                namespace: meta.namespace.clone(),
                connector_name: meta.name.clone(),
                class_name: class_meta.name.clone(),
                generation: meta.generation,
                target: spec.target.clone(),
            };
            match cp
                .kv
                .get_msg::<resources_proto::ConnectorMatchEntry>(&key)
                .await?
            {
                Some(existing) if existing.connector_uid != meta.uid => {
                    bail!(
                        "Connector match conflicts with {}/{}",
                        existing.namespace,
                        existing.connector_name
                    );
                }
                _ => {
                    cp.kv.set_msg(&key, &entry).await?;
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
        delete_match_entries_for_uid(cp.kv.as_ref(), &meta.uid).await?;
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
        compiled_match_keys: Vec<String>,
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
                            compiled_match_keys,
                        },
                    )),
                },
            )
            .await?;
        Ok(())
    }
}

pub async fn delete_match_entries_for_uid(
    kv: &dyn KeyValueStore,
    connector_uid: &str,
) -> Result<()> {
    if connector_uid.is_empty() {
        return Ok(());
    }
    for (key, bytes) in kv.list_entries(&keys::connector_match_prefix()).await? {
        let Ok(entry) = resources_proto::ConnectorMatchEntry::decode(bytes.as_slice()) else {
            continue;
        };
        if entry.connector_uid == connector_uid {
            kv.delete(&key).await?;
        }
    }
    Ok(())
}

pub async fn resolve_match(
    kv: &dyn KeyValueStore,
    registration_id: &str,
    class_name: &str,
    class_spec: &resources_proto::ConnectorClassSpec,
    fields: &HashMap<String, String>,
) -> Result<Option<resources_proto::ConnectorMatchEntry>> {
    for key_name in compile_match_keys(registration_id, class_name, class_spec, fields)? {
        let key = keys::connector_match(&key_name);
        if let Some(entry) = kv
            .get_msg::<resources_proto::ConnectorMatchEntry>(&key)
            .await?
        {
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

pub fn compile_match_keys(
    registration_id: &str,
    class_name: &str,
    class_spec: &resources_proto::ConnectorClassSpec,
    fields: &HashMap<String, String>,
) -> Result<Vec<String>> {
    let mut keys = Vec::new();
    let mut seen = HashSet::new();
    for index in &class_spec.match_indexes {
        if index.name.trim().is_empty() || index.fields.is_empty() {
            continue;
        }
        let mut segments = Vec::new();
        let mut complete = true;
        for field in &index.fields {
            match fields.get(field).filter(|value| !value.trim().is_empty()) {
                Some(value) => segments.push(format!("{}={}", field, value)),
                None => {
                    complete = false;
                    break;
                }
            }
        }
        if complete {
            let key = format!(
                "{}{}{}{}{}{}{}",
                registration_id,
                CONNECTOR_INDEX_NAME_SEP,
                class_name,
                CONNECTOR_INDEX_NAME_SEP,
                index.name,
                CONNECTOR_INDEX_FIELD_SEP,
                segments.join(CONNECTOR_INDEX_FIELD_SEP)
            );
            if seen.insert(key.clone()) {
                keys.push(key);
            }
        }
    }
    Ok(keys)
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

fn connector_references_class(resource: &resources_proto::Resource, class_name: &str) -> bool {
    connector_spec(resource)
        .ok()
        .and_then(|spec| spec.class_ref.as_ref())
        .map(|class_ref| class_ref.name == class_name)
        .unwrap_or(false)
}

fn validate_target(target: Option<&resources_proto::ConnectorTarget>) -> Result<()> {
    let target = target.ok_or_else(|| anyhow!("Connector spec.target is required"))?;
    match target.destination.as_ref() {
        Some(resources_proto::connector_target::Destination::Session(session)) => {
            if session.agent.trim().is_empty() {
                bail!("Connector session target requires agent");
            }
        }
        Some(resources_proto::connector_target::Destination::Channel(channel)) => {
            if channel.channel.trim().is_empty() {
                bail!("Connector channel target requires channel");
            }
            if channel.agent.trim().is_empty() {
                bail!("Connector channel target requires agent");
            }
        }
        None => bail!("Connector target destination is required"),
    }
    Ok(())
}

async fn register_connector_class(
    spec: &resources_proto::ConnectorClassSpec,
    class_name: &str,
    config: &crate::control::config::Config,
) -> Result<String> {
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
        .resolve_connector_secret()
        .context("failed to resolve ConnectorClass api key")?;
    let callback_base_url = std::env::var("TALON_CONNECTOR_CALLBACK_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
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
    let url = format!(
        "{}/v1/clusters/register",
        runtime.endpoint.trim_end_matches('/')
    );
    let response = reqwest::Client::new()
        .post(url)
        .bearer_auth(api_key)
        .json(&RegisterClusterRequest {
            cluster_id,
            callback_base_url,
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
        .json::<RegisterClusterResponse>()
        .await
        .context("failed to decode connector runtime registration response")?;
    body.registration_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("connector runtime registration response missing registrationId"))
}

trait ConnectorSecretExt {
    fn resolve_connector_secret(&self) -> Result<String>;
}

impl ConnectorSecretExt for crate::gateway::rpc::generated::config::Secret {
    fn resolve_connector_secret(&self) -> Result<String> {
        use crate::gateway::rpc::generated::config::{secret, secret_ref};

        match self.source.as_ref() {
            Some(secret::Source::Plain(value)) => Ok(value.clone()),
            Some(secret::Source::Ref(reference)) => {
                let source = secret_ref::Source::try_from(reference.source)
                    .map_err(|_| anyhow!("invalid secret source"))?;
                match source {
                    secret_ref::Source::Env => std::env::var(&reference.key)
                        .map_err(|_| anyhow!("Env var {} not set", reference.key)),
                    other => bail!(
                        "ConnectorClass auth.apiKey currently supports plain and env refs; got {}",
                        other.as_str_name()
                    ),
                }
            }
            None => bail!("secret source missing"),
        }
    }
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
    fn compile_match_keys_uses_declared_index_order_and_complete_fields() {
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

        let keys = compile_match_keys("reg_1", "slack", &class_spec, &fields).unwrap();

        assert_eq!(
            keys,
            vec![
                "reg_1|slack|slack-channel\u{1f}teamId=T123\u{1f}channelId=C999".to_string(),
                "reg_1|slack|slack-team\u{1f}teamId=T123".to_string(),
            ]
        );
    }
}
