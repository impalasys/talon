// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::events;
use crate::control::{keys, topics, KeyValueStore, MessagePublisher};
use crate::gateway::rpc::resources_proto;
use anyhow::{anyhow, Context, Result};
use prost::Message;
use std::sync::Arc;

const API_VERSION: &str = "talon.impalasys.com/v1";

/// Canonical control-plane resource facade.
///
/// Resource storage normalizes typed resource protos such as `Agent`,
/// `Workflow`, and `File` into the generic `Resource` envelope with a
/// `ResourceSpec`/`ResourceStatus` union. Callers that only need to read an
/// already-known KV key may decode the stored bytes directly with
/// `ResourceStore::decode_stored_resource` and avoid constructing this store.
#[derive(Clone)]
pub struct ResourceStore {
    kv: Arc<dyn KeyValueStore + Send + Sync>,
    pubsub: Arc<dyn MessagePublisher + Send + Sync>,
}

impl ResourceStore {
    pub fn new(
        kv: Arc<dyn KeyValueStore + Send + Sync>,
        pubsub: Arc<dyn MessagePublisher + Send + Sync>,
    ) -> Self {
        Self { kv, pubsub }
    }

    pub fn decode_stored_resource(kind: &str, bytes: &[u8]) -> Result<resources_proto::Resource> {
        match kind {
            "Agent" => decode_typed_resource::<resources_proto::Agent, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::Agent,
                resources_proto::resource_status::Kind::Agent,
            ),
            "Workflow" => decode_typed_resource::<resources_proto::Workflow, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::Workflow,
                resources_proto::resource_status::Kind::Workflow,
            ),
            "Schedule" => decode_typed_resource::<resources_proto::Schedule, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::Schedule,
                resources_proto::resource_status::Kind::Schedule,
            ),
            "Channel" => decode_typed_resource::<resources_proto::Channel, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::Channel,
                resources_proto::resource_status::Kind::Channel,
            ),
            "ChannelSubscription" => {
                decode_typed_resource::<resources_proto::ChannelSubscription, _, _, _, _>(
                    kind,
                    bytes,
                    resources_proto::resource_spec::Kind::ChannelSubscription,
                    resources_proto::resource_status::Kind::ChannelSubscription,
                )
            }
            "ConnectorClass" => {
                decode_typed_resource::<resources_proto::ConnectorClass, _, _, _, _>(
                    kind,
                    bytes,
                    resources_proto::resource_spec::Kind::ConnectorClass,
                    resources_proto::resource_status::Kind::ConnectorClass,
                )
            }
            "Connector" => decode_typed_resource::<resources_proto::Connector, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::Connector,
                resources_proto::resource_status::Kind::Connector,
            ),
            "McpServer" => decode_typed_resource::<resources_proto::McpServer, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::McpServer,
                resources_proto::resource_status::Kind::McpServer,
            ),
            "Knowledge" => decode_typed_resource::<resources_proto::Knowledge, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::Knowledge,
                resources_proto::resource_status::Kind::Knowledge,
            ),
            "File" => decode_typed_resource::<resources_proto::File, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::File,
                resources_proto::resource_status::Kind::File,
            ),
            "Namespace" => decode_typed_resource::<resources_proto::Namespace, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::Namespace,
                resources_proto::resource_status::Kind::Namespace,
            ),
            "Session" => decode_typed_resource::<resources_proto::Session, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::Session,
                resources_proto::resource_status::Kind::Session,
            ),
            "Skill" => decode_typed_resource::<resources_proto::Skill, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::Skill,
                resources_proto::resource_status::Kind::Skill,
            ),
            "Template" => decode_typed_resource::<resources_proto::Template, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::Template,
                resources_proto::resource_status::Kind::Template,
            ),
            "Deployment" => decode_typed_resource::<resources_proto::Deployment, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::Deployment,
                resources_proto::resource_status::Kind::Deployment,
            ),
            "DeploymentReplica" => {
                decode_typed_resource::<resources_proto::DeploymentReplica, _, _, _, _>(
                    kind,
                    bytes,
                    resources_proto::resource_spec::Kind::DeploymentReplica,
                    resources_proto::resource_status::Kind::DeploymentReplica,
                )
            }
            "SandboxClass" => decode_typed_resource::<resources_proto::SandboxClass, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::SandboxClass,
                resources_proto::resource_status::Kind::SandboxClass,
            ),
            "SandboxPolicy" => decode_typed_resource::<resources_proto::SandboxPolicy, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::SandboxPolicy,
                resources_proto::resource_status::Kind::SandboxPolicy,
            ),
            "Sandbox" => decode_typed_resource::<resources_proto::Sandbox, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::Sandbox,
                resources_proto::resource_status::Kind::Sandbox,
            ),
            "Worker" => decode_typed_resource::<resources_proto::Worker, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::Worker,
                resources_proto::resource_status::Kind::Worker,
            ),
            "UsagePolicy" => decode_typed_resource::<resources_proto::UsagePolicy, _, _, _, _>(
                kind,
                bytes,
                resources_proto::resource_spec::Kind::UsagePolicy,
                resources_proto::resource_status::Kind::UsagePolicy,
            ),
            _ => resources_proto::Resource::decode(bytes).map_err(Into::into),
        }
    }

    pub async fn upsert(
        &self,
        namespace: &str,
        resource: resources_proto::Resource,
    ) -> Result<resources_proto::Resource> {
        self.upsert_resource(namespace, resource, false).await
    }

    pub async fn upsert_manifest(
        &self,
        namespace: &str,
        manifest: resources_proto::ResourceManifest,
    ) -> Result<resources_proto::Resource> {
        let resource = resources_proto::Resource {
            api_version: manifest.api_version,
            kind: manifest.kind,
            metadata: manifest.metadata,
            spec: manifest.spec,
            status: None,
        };
        self.upsert_resource(namespace, resource, true).await
    }

    async fn upsert_resource(
        &self,
        namespace: &str,
        mut resource: resources_proto::Resource,
        preserve_existing_status: bool,
    ) -> Result<resources_proto::Resource> {
        normalize_resource(namespace, &mut resource)?;
        let key = resource_key(&resource);
        validate_resource_kind(&resource)?;
        let existing = match self.kv.get(&key).await? {
            Some(bytes) => match Self::decode_stored_resource(&key.kind, bytes.as_slice()) {
                Ok(resource) => Some(resource),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        kind = %key.kind,
                        namespace = %key.namespace,
                        parent_path = %key.parent_path,
                        name = %key.name,
                        "overwriting undecodable stored resource during upsert"
                    );
                    None
                }
            },
            None => None,
        };
        let change_type = if existing.is_some() {
            events::ResourceChangeType::Updated
        } else {
            events::ResourceChangeType::Created
        };
        let generation = existing
            .as_ref()
            .and_then(|existing| existing.metadata.as_ref())
            .map(|meta| meta.generation.saturating_add(1))
            .unwrap_or(1);
        let uid = existing
            .as_ref()
            .and_then(|existing| existing.metadata.as_ref())
            .map(|meta| meta.uid.clone())
            .filter(|uid| !uid.is_empty())
            .unwrap_or_else(crate::control::uuid::v7);
        let resource_version = crate::control::uuid::resource_version();

        if preserve_existing_status {
            resource.status = existing
                .as_ref()
                .and_then(|existing| existing.status.clone())
                .or_else(|| Some(default_status_for_resource(&resource)));
        }

        let meta = resource
            .metadata
            .as_mut()
            .ok_or_else(|| anyhow!("resource metadata missing after normalization"))?;
        meta.generation = generation;
        meta.uid = uid;
        meta.resource_version = resource_version;

        self.kv
            .set(&key, &encode_stored_resource(&resource)?)
            .await?;
        self.publish_index_event(&resource, events::ResourceChangeType::Updated, &key)
            .await;
        self.publish_changed(&resource, change_type, &["metadata", "spec"])
            .await?;
        Ok(resource)
    }

    pub async fn patch_spec(
        &self,
        namespace: &str,
        kind: &str,
        name: &str,
        expected_resource_version: Option<&str>,
        spec: resources_proto::ResourceSpec,
    ) -> Result<resources_proto::Resource> {
        self.patch_resource(
            namespace,
            kind,
            name,
            expected_resource_version,
            |resource| {
                resource.spec = Some(spec.clone());
                bump_generation(resource);
                Ok(vec!["metadata", "spec"])
            },
        )
        .await
    }

    pub async fn patch_status(
        &self,
        namespace: &str,
        kind: &str,
        name: &str,
        expected_resource_version: Option<&str>,
        status: resources_proto::ResourceStatus,
    ) -> Result<resources_proto::Resource> {
        self.patch_resource(
            namespace,
            kind,
            name,
            expected_resource_version,
            |resource| {
                resource.status = Some(status.clone());
                Ok(vec!["metadata", "status"])
            },
        )
        .await
    }

    async fn patch_resource<F>(
        &self,
        namespace: &str,
        kind: &str,
        name: &str,
        expected_resource_version: Option<&str>,
        mut update: F,
    ) -> Result<resources_proto::Resource>
    where
        F: FnMut(&mut resources_proto::Resource) -> Result<Vec<&'static str>>,
    {
        let key = keys::ResourceKey::new(namespace, &[], kind, name);
        for _ in 0..8 {
            let current = self.kv.get(&key).await?.ok_or_else(|| {
                anyhow!("{} '{}' not found in namespace '{}'", kind, name, namespace)
            })?;
            let mut resource = Self::decode_stored_resource(kind, current.as_slice())?;
            if let Some(expected) = expected_resource_version {
                let actual = resource
                    .metadata
                    .as_ref()
                    .map(|meta| meta.resource_version.as_str())
                    .unwrap_or_default();
                if actual != expected {
                    return Err(anyhow!(
                        "resourceVersion conflict for {}/{}: expected '{}', got '{}'",
                        kind,
                        name,
                        expected,
                        actual
                    ));
                }
            }

            let changed_sections = update(&mut resource)?;
            let meta = resource
                .metadata
                .as_mut()
                .ok_or_else(|| anyhow!("resource metadata missing"))?;
            meta.resource_version = crate::control::uuid::resource_version();
            validate_resource_kind(&resource)?;
            let next = encode_stored_resource(&resource)?;
            if self
                .kv
                .compare_and_swap(&key, Some(current.as_slice()), &next)
                .await?
            {
                self.publish_index_event(&resource, events::ResourceChangeType::Updated, &key)
                    .await;
                self.publish_changed(
                    &resource,
                    events::ResourceChangeType::Updated,
                    &changed_sections,
                )
                .await?;
                return Ok(resource);
            }
        }

        Err(anyhow!(
            "failed to patch {} '{}' in namespace '{}' after compare-and-swap retries",
            kind,
            name,
            namespace
        ))
    }

    pub async fn get(
        &self,
        namespace: &str,
        kind: &str,
        name: &str,
    ) -> Result<Option<resources_proto::Resource>> {
        self.get_by_key(&keys::ResourceKey::new(namespace, &[], kind, name))
            .await
    }

    pub async fn list(
        &self,
        namespace: &str,
        kind: Option<&str>,
    ) -> Result<Vec<resources_proto::Resource>> {
        let entries = self
            .kv
            .list_entries(&keys::ResourceParent::root(namespace).list(kind))
            .await?;
        let mut status_fetches = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            if let Ok(mut resource) = Self::decode_stored_resource(&key.kind, value.as_slice()) {
                status_fetches.push(async move {
                    self.populate_computed_status(&key, &mut resource).await?;
                    Ok::<_, anyhow::Error>(resource)
                });
            }
        }
        futures::future::try_join_all(status_fetches).await
    }

    pub async fn delete(&self, namespace: &str, kind: &str, name: &str) -> Result<bool> {
        let key = keys::ResourceKey::new(namespace, &[], kind, name);
        let Some(mut resource) = self.get_by_key(&key).await? else {
            return Ok(false);
        };
        let has_finalizers = resource
            .metadata
            .as_ref()
            .map(|meta| !meta.finalizers.is_empty())
            .unwrap_or(false);
        if has_finalizers {
            let now = chrono::Utc::now().timestamp_micros();
            let meta = resource
                .metadata
                .as_mut()
                .ok_or_else(|| anyhow!("resource metadata missing"))?;
            meta.deletion_timestamp = Some(now);
            meta.resource_version = crate::control::uuid::resource_version();
            self.kv
                .set(&key, &encode_stored_resource(&resource)?)
                .await?;
            self.publish_index_event(&resource, events::ResourceChangeType::Updated, &key)
                .await;
            self.publish_changed(
                &resource,
                events::ResourceChangeType::Updated,
                &["metadata"],
            )
            .await?;
        } else {
            self.kv.delete(&key).await?;
            self.publish_index_event(&resource, events::ResourceChangeType::Deleted, &key)
                .await;
            self.publish_changed(
                &resource,
                events::ResourceChangeType::Deleted,
                &["metadata"],
            )
            .await?;
        }
        Ok(true)
    }

    async fn publish_changed(
        &self,
        resource: &resources_proto::Resource,
        change_type: events::ResourceChangeType,
        changed_sections: &[&str],
    ) -> Result<()> {
        let meta = resource
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("resource metadata missing"))?;
        let event = events::ResourceChangedEvent {
            namespace: meta.namespace.clone(),
            resource_kind: resource.kind.clone(),
            name: meta.name.clone(),
            uid: meta.uid.clone(),
            resource_version: meta.resource_version.clone(),
            generation: meta.generation,
            change_type: change_type as i32,
            changed_sections: changed_sections
                .iter()
                .map(|section| section.to_string())
                .collect(),
            timestamp: chrono::Utc::now().timestamp_micros(),
        };
        self.pubsub
            .publish(topics::RESOURCE_LIFECYCLE_TOPIC, &event.encode_to_vec())
            .await
    }

    async fn publish_index_event(
        &self,
        resource: &resources_proto::Resource,
        change_type: events::ResourceChangeType,
        key: &keys::ResourceKey,
    ) {
        let Some(meta) = resource.metadata.as_ref() else {
            return;
        };
        let operation = if change_type == events::ResourceChangeType::Deleted {
            events::IndexOperation::Delete
        } else {
            events::IndexOperation::Upsert
        };
        let event = events::IndexEvent {
            operation: operation as i32,
            key: key.canonical(),
            generation: meta.generation,
            ..Default::default()
        };
        if let Err(error) =
            crate::control::search::publish_index_event(self.pubsub.as_ref(), event).await
        {
            tracing::warn!(
                error = %error,
                namespace = %meta.namespace,
                kind = %resource.kind,
                name = %meta.name,
                "failed to publish search index event"
            );
        }
    }

    pub async fn get_agent(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<resources_proto::Agent>> {
        self.get(namespace, "Agent", name)
            .await?
            .map(agent_from_resource)
            .transpose()
    }

    async fn get_by_key(
        &self,
        key: &keys::ResourceKey,
    ) -> Result<Option<resources_proto::Resource>> {
        let mut resource = self
            .kv
            .get(key)
            .await?
            .map(|bytes| {
                Self::decode_stored_resource(&key.kind, bytes.as_slice()).with_context(|| {
                    format!(
                        "failed to decode stored {} resource {}/{}/{}",
                        key.kind, key.namespace, key.parent_path, key.name
                    )
                })
            })
            .transpose()?;
        if let Some(resource) = resource.as_mut() {
            self.populate_computed_status(key, resource).await?;
        }
        Ok(resource)
    }

    async fn populate_computed_status(
        &self,
        key: &keys::ResourceKey,
        resource: &mut resources_proto::Resource,
    ) -> Result<()> {
        if key.kind == "UsagePolicy" {
            if let Err(err) = crate::control::usage::populate_usage_policy_status(
                self.kv.as_ref(),
                resource,
                chrono::Utc::now().timestamp(),
            )
            .await
            {
                tracing::error!(
                    error = %err,
                    namespace = %key.namespace,
                    name = %key.name,
                    "failed to populate UsagePolicy status"
                );
            }
        }
        Ok(())
    }
}

pub fn agent_from_resource(resource: resources_proto::Resource) -> Result<resources_proto::Agent> {
    if resource.kind != "Agent" {
        return Err(anyhow!("expected Agent resource, got {}", resource.kind));
    }
    let metadata = resource
        .metadata
        .ok_or_else(|| anyhow!("Agent resource metadata is required"))?;
    let Some(resources_proto::resource_spec::Kind::Agent(spec)) =
        resource.spec.and_then(|spec| spec.kind)
    else {
        return Err(anyhow!("Agent resource is missing typed Agent spec"));
    };
    let status = match resource.status.and_then(|status| status.kind) {
        Some(resources_proto::resource_status::Kind::Agent(status)) => status,
        _ => resources_proto::AgentStatus {
            observed_generation: 0,
            phase: String::new(),
            conditions: Vec::new(),
            last_session_id: None,
        },
    };
    Ok(resources_proto::Agent {
        metadata: Some(metadata),
        spec: Some(spec),
        status: Some(status),
    })
}

trait StoredTypedResource<S, T>: prost::Message + Default {
    fn into_parts(self) -> (Option<resources_proto::ResourceMeta>, Option<S>, Option<T>);
    fn from_parts(
        metadata: Option<resources_proto::ResourceMeta>,
        spec: Option<S>,
        status: Option<T>,
    ) -> Self;
}

macro_rules! impl_stored_typed_resource {
    ($ty:ty, $spec:ty, $status:ty) => {
        impl StoredTypedResource<$spec, $status> for $ty {
            fn into_parts(
                self,
            ) -> (
                Option<resources_proto::ResourceMeta>,
                Option<$spec>,
                Option<$status>,
            ) {
                (self.metadata, self.spec, self.status)
            }

            fn from_parts(
                metadata: Option<resources_proto::ResourceMeta>,
                spec: Option<$spec>,
                status: Option<$status>,
            ) -> Self {
                Self {
                    metadata,
                    spec,
                    status,
                }
            }
        }
    };
}

impl_stored_typed_resource!(
    resources_proto::Agent,
    resources_proto::AgentSpec,
    resources_proto::AgentStatus
);
impl_stored_typed_resource!(
    resources_proto::Workflow,
    resources_proto::WorkflowSpec,
    resources_proto::WorkflowStatus
);
impl_stored_typed_resource!(
    resources_proto::Schedule,
    resources_proto::ScheduleSpec,
    resources_proto::ScheduleStatus
);
impl_stored_typed_resource!(
    resources_proto::Channel,
    resources_proto::ChannelSpec,
    resources_proto::ChannelStatus
);
impl_stored_typed_resource!(
    resources_proto::ChannelSubscription,
    resources_proto::ChannelSubscriptionSpec,
    resources_proto::CommonResourceStatus
);
impl_stored_typed_resource!(
    resources_proto::ConnectorClass,
    resources_proto::ConnectorClassSpec,
    resources_proto::ConnectorClassStatus
);
impl_stored_typed_resource!(
    resources_proto::Connector,
    resources_proto::ConnectorSpec,
    resources_proto::ConnectorStatus
);
impl_stored_typed_resource!(
    resources_proto::McpServer,
    resources_proto::McpServerSpec,
    resources_proto::CommonResourceStatus
);
impl_stored_typed_resource!(
    resources_proto::Knowledge,
    resources_proto::KnowledgeSpec,
    resources_proto::CommonResourceStatus
);
impl_stored_typed_resource!(
    resources_proto::File,
    resources_proto::FileSpec,
    resources_proto::FileStatus
);
impl_stored_typed_resource!(
    resources_proto::Namespace,
    resources_proto::NamespaceSpec,
    resources_proto::NamespaceStatus
);
impl_stored_typed_resource!(
    resources_proto::Session,
    resources_proto::SessionSpec,
    resources_proto::SessionStatus
);
impl_stored_typed_resource!(
    resources_proto::Skill,
    resources_proto::SkillSpec,
    resources_proto::CommonResourceStatus
);
impl_stored_typed_resource!(
    resources_proto::Template,
    resources_proto::TemplateSpec,
    resources_proto::CommonResourceStatus
);
impl_stored_typed_resource!(
    resources_proto::Deployment,
    resources_proto::DeploymentSpec,
    resources_proto::DeploymentStatus
);
impl_stored_typed_resource!(
    resources_proto::DeploymentReplica,
    resources_proto::DeploymentReplicaSpec,
    resources_proto::DeploymentReplicaStatus
);
impl_stored_typed_resource!(
    resources_proto::SandboxClass,
    resources_proto::SandboxClassSpec,
    resources_proto::CommonResourceStatus
);
impl_stored_typed_resource!(
    resources_proto::SandboxPolicy,
    resources_proto::SandboxPolicySpec,
    resources_proto::CommonResourceStatus
);
impl_stored_typed_resource!(
    resources_proto::Sandbox,
    resources_proto::SandboxSpec,
    resources_proto::SandboxStatus
);
impl_stored_typed_resource!(
    resources_proto::Worker,
    resources_proto::WorkerSpec,
    resources_proto::WorkerStatus
);
impl_stored_typed_resource!(
    resources_proto::UsagePolicy,
    resources_proto::UsagePolicySpec,
    resources_proto::UsagePolicyStatus
);
fn decode_typed_resource<W, S, T, SpecArm, StatusArm>(
    kind: &str,
    bytes: &[u8],
    spec_arm: SpecArm,
    status_arm: StatusArm,
) -> Result<resources_proto::Resource>
where
    W: StoredTypedResource<S, T>,
    S: Default,
    T: Default,
    SpecArm: FnOnce(S) -> resources_proto::resource_spec::Kind,
    StatusArm: FnOnce(T) -> resources_proto::resource_status::Kind,
{
    let (metadata, spec, status) = W::decode(bytes)?.into_parts();
    if metadata.is_none() {
        return Err(anyhow!("{kind} stored payload is missing metadata"));
    }
    Ok(resources_proto::Resource {
        api_version: API_VERSION.to_string(),
        kind: kind.to_string(),
        metadata,
        spec: spec.map(|spec| resources_proto::ResourceSpec {
            kind: Some(spec_arm(spec)),
        }),
        status: Some(resources_proto::ResourceStatus {
            kind: Some(status_arm(status.unwrap_or_default())),
        }),
    })
}

fn encode_stored_resource(resource: &resources_proto::Resource) -> Result<Vec<u8>> {
    match resource.kind.as_str() {
        "Agent" => encode_typed_resource::<
            resources_proto::Agent,
            resources_proto::AgentSpec,
            resources_proto::AgentStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::Agent(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::Agent(status) => Some(status),
                _ => None,
            },
        ),
        "Workflow" => encode_typed_resource::<
            resources_proto::Workflow,
            resources_proto::WorkflowSpec,
            resources_proto::WorkflowStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::Workflow(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::Workflow(status) => Some(status),
                _ => None,
            },
        ),
        "Schedule" => encode_typed_resource::<
            resources_proto::Schedule,
            resources_proto::ScheduleSpec,
            resources_proto::ScheduleStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::Schedule(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::Schedule(status) => Some(status),
                _ => None,
            },
        ),
        "Channel" => encode_typed_resource::<
            resources_proto::Channel,
            resources_proto::ChannelSpec,
            resources_proto::ChannelStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::Channel(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::Channel(status) => Some(status),
                _ => None,
            },
        ),
        "ChannelSubscription" => encode_typed_resource::<
            resources_proto::ChannelSubscription,
            resources_proto::ChannelSubscriptionSpec,
            resources_proto::CommonResourceStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::ChannelSubscription(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::ChannelSubscription(status) => Some(status),
                _ => None,
            },
        ),
        "ConnectorClass" => encode_typed_resource::<
            resources_proto::ConnectorClass,
            resources_proto::ConnectorClassSpec,
            resources_proto::ConnectorClassStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::ConnectorClass(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::ConnectorClass(status) => Some(status),
                _ => None,
            },
        ),
        "Connector" => encode_typed_resource::<
            resources_proto::Connector,
            resources_proto::ConnectorSpec,
            resources_proto::ConnectorStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::Connector(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::Connector(status) => Some(status),
                _ => None,
            },
        ),
        "McpServer" => encode_typed_resource::<
            resources_proto::McpServer,
            resources_proto::McpServerSpec,
            resources_proto::CommonResourceStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::McpServer(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::McpServer(status) => Some(status),
                _ => None,
            },
        ),
        "Knowledge" => encode_typed_resource::<
            resources_proto::Knowledge,
            resources_proto::KnowledgeSpec,
            resources_proto::CommonResourceStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::Knowledge(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::Knowledge(status) => Some(status),
                _ => None,
            },
        ),
        "File" => encode_typed_resource::<
            resources_proto::File,
            resources_proto::FileSpec,
            resources_proto::FileStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::File(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::File(status) => Some(status),
                _ => None,
            },
        ),
        "Namespace" => encode_typed_resource::<
            resources_proto::Namespace,
            resources_proto::NamespaceSpec,
            resources_proto::NamespaceStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::Namespace(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::Namespace(status) => Some(status),
                _ => None,
            },
        ),
        "Session" => encode_typed_resource::<
            resources_proto::Session,
            resources_proto::SessionSpec,
            resources_proto::SessionStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::Session(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::Session(status) => Some(status),
                _ => None,
            },
        ),
        "Skill" => encode_typed_resource::<
            resources_proto::Skill,
            resources_proto::SkillSpec,
            resources_proto::CommonResourceStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::Skill(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::Skill(status) => Some(status),
                _ => None,
            },
        ),
        "Template" => encode_typed_resource::<
            resources_proto::Template,
            resources_proto::TemplateSpec,
            resources_proto::CommonResourceStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::Template(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::Template(status) => Some(status),
                _ => None,
            },
        ),
        "Deployment" => encode_typed_resource::<
            resources_proto::Deployment,
            resources_proto::DeploymentSpec,
            resources_proto::DeploymentStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::Deployment(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::Deployment(status) => Some(status),
                _ => None,
            },
        ),
        "DeploymentReplica" => encode_typed_resource::<
            resources_proto::DeploymentReplica,
            resources_proto::DeploymentReplicaSpec,
            resources_proto::DeploymentReplicaStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::DeploymentReplica(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::DeploymentReplica(status) => Some(status),
                _ => None,
            },
        ),
        "SandboxClass" => encode_typed_resource::<
            resources_proto::SandboxClass,
            resources_proto::SandboxClassSpec,
            resources_proto::CommonResourceStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::SandboxClass(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::SandboxClass(status) => Some(status),
                _ => None,
            },
        ),
        "SandboxPolicy" => encode_typed_resource::<
            resources_proto::SandboxPolicy,
            resources_proto::SandboxPolicySpec,
            resources_proto::CommonResourceStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::SandboxPolicy(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::SandboxPolicy(status) => Some(status),
                _ => None,
            },
        ),
        "Sandbox" => encode_typed_resource::<
            resources_proto::Sandbox,
            resources_proto::SandboxSpec,
            resources_proto::SandboxStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::Sandbox(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::Sandbox(status) => Some(status),
                _ => None,
            },
        ),
        "Worker" => encode_typed_resource::<
            resources_proto::Worker,
            resources_proto::WorkerSpec,
            resources_proto::WorkerStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::Worker(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::Worker(status) => Some(status),
                _ => None,
            },
        ),
        "UsagePolicy" => encode_typed_resource::<
            resources_proto::UsagePolicy,
            resources_proto::UsagePolicySpec,
            resources_proto::UsagePolicyStatus,
            _,
            _,
        >(
            resource,
            |kind| match kind {
                resources_proto::resource_spec::Kind::UsagePolicy(spec) => Some(spec),
                _ => None,
            },
            |kind| match kind {
                resources_proto::resource_status::Kind::UsagePolicy(status) => Some(status),
                _ => None,
            },
        ),
        _ => Ok(resource.encode_to_vec()),
    }
}

fn encode_typed_resource<W, S, T, SpecExtract, StatusExtract>(
    resource: &resources_proto::Resource,
    spec_extract: SpecExtract,
    status_extract: StatusExtract,
) -> Result<Vec<u8>>
where
    W: StoredTypedResource<S, T>,
    S: Default,
    T: Default,
    SpecExtract: FnOnce(resources_proto::resource_spec::Kind) -> Option<S>,
    StatusExtract: FnOnce(resources_proto::resource_status::Kind) -> Option<T>,
{
    let spec = resource
        .spec
        .clone()
        .and_then(|spec| spec.kind)
        .and_then(spec_extract)
        .ok_or_else(|| anyhow!("{} resource is missing matching spec", resource.kind))?;
    let status = resource
        .status
        .clone()
        .and_then(|status| status.kind)
        .and_then(status_extract)
        .unwrap_or_default();
    Ok(W::from_parts(resource.metadata.clone(), Some(spec), Some(status)).encode_to_vec())
}

pub fn normalize_resource(namespace: &str, resource: &mut resources_proto::Resource) -> Result<()> {
    if resource.api_version.trim().is_empty() {
        resource.api_version = API_VERSION.to_string();
    }
    if resource.api_version != API_VERSION {
        return Err(anyhow!(
            "unsupported apiVersion '{}', expected '{}'",
            resource.api_version,
            API_VERSION
        ));
    }
    if resource.kind.trim().is_empty() {
        return Err(anyhow!("resource kind is required"));
    }
    let meta = resource
        .metadata
        .as_mut()
        .ok_or_else(|| anyhow!("resource metadata is required"))?;
    if meta.name.trim().is_empty() {
        return Err(anyhow!("metadata.name is required"));
    }
    if meta.namespace.trim().is_empty() {
        meta.namespace = namespace.to_string();
    }
    if meta.namespace != namespace {
        return Err(anyhow!(
            "metadata.namespace '{}' must match request namespace '{}'",
            meta.namespace,
            namespace
        ));
    }
    Ok(())
}

fn bump_generation(resource: &mut resources_proto::Resource) {
    if let Some(meta) = resource.metadata.as_mut() {
        meta.generation = meta.generation.saturating_add(1).max(1);
    }
}

fn validate_resource_kind(resource: &resources_proto::Resource) -> Result<()> {
    use resources_proto::resource_spec::Kind;

    let Some(spec) = resource.spec.as_ref().and_then(|spec| spec.kind.as_ref()) else {
        return Ok(());
    };
    let expected = match spec {
        Kind::Agent(_) => "Agent",
        Kind::Workflow(_) => "Workflow",
        Kind::Schedule(_) => "Schedule",
        Kind::Channel(_) => "Channel",
        Kind::ChannelSubscription(_) => "ChannelSubscription",
        Kind::ConnectorClass(_) => "ConnectorClass",
        Kind::Connector(_) => "Connector",
        Kind::McpServer(_) => "McpServer",
        Kind::Knowledge(_) => "Knowledge",
        Kind::File(spec) => {
            validate_file_resource_name(resource, spec)?;
            "File"
        }
        Kind::Namespace(_) => "Namespace",
        Kind::Session(_) => "Session",
        Kind::Skill(_) => "Skill",
        Kind::Template(_) => "Template",
        Kind::Deployment(_) => "Deployment",
        Kind::DeploymentReplica(_) => "DeploymentReplica",
        Kind::SandboxClass(_) => "SandboxClass",
        Kind::SandboxPolicy(_) => "SandboxPolicy",
        Kind::Sandbox(_) => "Sandbox",
        Kind::Worker(_) => "Worker",
        Kind::UsagePolicy(spec) => {
            crate::control::usage::validate_usage_policy_spec(spec)?;
            "UsagePolicy"
        }
        Kind::Raw(_) => return Ok(()),
    };
    if resource.kind != expected {
        return Err(anyhow!(
            "resource kind '{}' does not match spec arm '{}'",
            resource.kind,
            expected
        ));
    }
    Ok(())
}

fn validate_file_resource_name(
    resource: &resources_proto::Resource,
    spec: &resources_proto::FileSpec,
) -> Result<()> {
    let Some(meta) = resource.metadata.as_ref() else {
        return Ok(());
    };
    let expected = keys::file_name_for_path(&spec.path);
    if meta.name != expected {
        return Err(anyhow!(
            "File metadata.name '{}' must match path-derived name '{}'",
            meta.name,
            expected
        ));
    }
    Ok(())
}

fn default_status_for_resource(
    resource: &resources_proto::Resource,
) -> resources_proto::ResourceStatus {
    use resources_proto::resource_spec::Kind as SpecKind;
    use resources_proto::resource_status::Kind as StatusKind;

    let kind = match resource.spec.as_ref().and_then(|spec| spec.kind.as_ref()) {
        Some(SpecKind::Agent(_)) => StatusKind::Agent(Default::default()),
        Some(SpecKind::Workflow(_)) => StatusKind::Workflow(Default::default()),
        Some(SpecKind::Schedule(_)) => StatusKind::Schedule(Default::default()),
        Some(SpecKind::Channel(_)) => StatusKind::Channel(Default::default()),
        Some(SpecKind::ChannelSubscription(_)) => {
            StatusKind::ChannelSubscription(Default::default())
        }
        Some(SpecKind::ConnectorClass(_)) => StatusKind::ConnectorClass(Default::default()),
        Some(SpecKind::Connector(_)) => StatusKind::Connector(Default::default()),
        Some(SpecKind::McpServer(_)) => StatusKind::McpServer(Default::default()),
        Some(SpecKind::Knowledge(_)) => StatusKind::Knowledge(Default::default()),
        Some(SpecKind::File(_)) => StatusKind::File(Default::default()),
        Some(SpecKind::Namespace(_)) => StatusKind::Namespace(Default::default()),
        Some(SpecKind::Session(_)) => StatusKind::Session(Default::default()),
        Some(SpecKind::Skill(_)) => StatusKind::Skill(Default::default()),
        Some(SpecKind::Template(_)) => StatusKind::Template(Default::default()),
        Some(SpecKind::Deployment(_)) => StatusKind::Deployment(Default::default()),
        Some(SpecKind::DeploymentReplica(_)) => StatusKind::DeploymentReplica(Default::default()),
        Some(SpecKind::SandboxClass(_)) => StatusKind::SandboxClass(Default::default()),
        Some(SpecKind::SandboxPolicy(_)) => StatusKind::SandboxPolicy(Default::default()),
        Some(SpecKind::Sandbox(_)) => StatusKind::Sandbox(Default::default()),
        Some(SpecKind::Worker(_)) => StatusKind::Worker(Default::default()),
        Some(SpecKind::UsagePolicy(_)) => StatusKind::UsagePolicy(Default::default()),
        Some(SpecKind::Raw(_)) | None => StatusKind::Raw(resources_proto::RawResourceStatus {
            json: "{}".to_string(),
        }),
    };
    resources_proto::ResourceStatus { kind: Some(kind) }
}

fn resource_key(resource: &resources_proto::Resource) -> keys::ResourceKey {
    let meta = resource
        .metadata
        .as_ref()
        .expect("resource metadata should be normalized");
    keys::ResourceKey::new(&meta.namespace, &[], &resource.kind, &meta.name)
}

#[cfg(test)]
mod tests {
    use super::ResourceStore;
    use crate::control::events;
    use crate::control::keys;
    use crate::control::topics;
    use crate::control::KeyValueStore;
    use crate::gateway::rpc::resources_proto;
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use prost::Message;
    use std::sync::Arc;

    #[tokio::test]
    async fn resource_store_upserts_lists_and_publishes_change_events() {
        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        let store = ResourceStore::new(kv, pubsub.clone());

        let resource = resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Template".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "coding-agent".to_string(),
                namespace: "customers".to_string(),
                labels: Default::default(),
                annotations: Default::default(),
                owner_references: Vec::new(),
                finalizers: Vec::new(),
                generation: 0,
                resource_version: String::new(),
                uid: String::new(),
                deletion_timestamp: None,
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::Template(
                    resources_proto::TemplateSpec {
                        kind: "Agent".to_string(),
                        metadata: None,
                        spec_json: "{}".to_string(),
                    },
                )),
            }),
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resources_proto::resource_status::Kind::Template(
                    resources_proto::CommonResourceStatus {
                        observed_generation: 0,
                        phase: String::new(),
                        conditions: Vec::new(),
                    },
                )),
            }),
        };

        let created = store.upsert("customers", resource).await.unwrap();
        assert_eq!(created.metadata.as_ref().unwrap().generation, 1);
        assert!(!created.metadata.as_ref().unwrap().uid.is_empty());

        let listed = store.list("customers", Some("Template")).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].kind, "Template");

        let published = pubsub.published.lock().await;
        assert_eq!(published.len(), 2);
        let lifecycle = published
            .iter()
            .find(|(topic, _)| topic == topics::RESOURCE_LIFECYCLE_TOPIC)
            .expect("resource lifecycle event should be published");
        let event = events::ResourceChangedEvent::decode(lifecycle.1.as_slice()).unwrap();
        assert_eq!(event.resource_kind, "Template");
        assert_eq!(
            event.change_type,
            events::ResourceChangeType::Created as i32
        );
    }

    #[tokio::test]
    async fn file_resources_require_path_derived_names() {
        let kv = Arc::new(MockKvStore::new());
        let pubsub = Arc::new(RecordingPubSub::default());
        let store = ResourceStore::new(kv, pubsub);
        let path = "/memory/brand-guidelines.md";
        let mut resource = resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "File".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "custom-name".to_string(),
                namespace: "customers".to_string(),
                labels: Default::default(),
                annotations: Default::default(),
                owner_references: Vec::new(),
                finalizers: Vec::new(),
                generation: 0,
                resource_version: String::new(),
                uid: String::new(),
                deletion_timestamp: None,
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::File(
                    resources_proto::FileSpec {
                        path: path.to_string(),
                        media_type: "text/markdown".to_string(),
                        purpose: resources_proto::FilePurpose::Memory as i32,
                        index_policy: resources_proto::FileIndexPolicy::Retrieval as i32,
                        retention: resources_proto::FileRetention::Retained as i32,
                    },
                )),
            }),
            status: None,
        };

        assert!(store.upsert("customers", resource.clone()).await.is_err());

        resource.metadata.as_mut().unwrap().name = keys::file_name_for_path(path);
        assert!(store.upsert("customers", resource).await.is_ok());
    }

    #[tokio::test]
    async fn resource_store_returns_agent_typed_view_from_resource() {
        let kv = Arc::new(MockKvStore::new());
        let store = ResourceStore::new(kv.clone(), Arc::new(RecordingPubSub::default()));
        let resource = resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Agent".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "coding".to_string(),
                namespace: "customers:acme".to_string(),
                labels: Default::default(),
                annotations: Default::default(),
                owner_references: Vec::new(),
                finalizers: Vec::new(),
                generation: 0,
                resource_version: String::new(),
                uid: String::new(),
                deletion_timestamp: None,
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::Agent(
                    resources_proto::AgentSpec {
                        system_prompt: "You are a coding agent.".to_string(),
                        runtime: Some(resources_proto::AgentRuntime {
                            kind: "acp".to_string(),
                            acp: Some(resources_proto::AcpRuntime {
                                command: "talon-mock-acp".to_string(),
                                sandbox_policy_ref: "coding".to_string(),
                                cwd: "/workspace".to_string(),
                                permission_policy: std::collections::HashMap::from([(
                                    "default".to_string(),
                                    "allow".to_string(),
                                )]),
                                ..Default::default()
                            }),
                        }),
                        ..Default::default()
                    },
                )),
            }),
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resources_proto::resource_status::Kind::Agent(
                    resources_proto::AgentStatus {
                        phase: "Ready".to_string(),
                        ..Default::default()
                    },
                )),
            }),
        };

        store.upsert("customers:acme", resource).await.unwrap();
        let stored = kv
            .get(&crate::control::keys::agent("customers:acme", "coding"))
            .await
            .unwrap()
            .expect("stored Agent payload");
        let stored_agent =
            resources_proto::Agent::decode(stored.as_slice()).expect("stored payload is Agent");
        assert_eq!(
            stored_agent.spec.as_ref().unwrap().system_prompt,
            "You are a coding agent."
        );
        if let Ok(decoded_resource) = resources_proto::Resource::decode(stored.as_slice()) {
            assert_ne!(decoded_resource.kind, "Agent");
        }

        let agent = store
            .get_agent("customers:acme", "coding")
            .await
            .unwrap()
            .expect("typed Agent view should be returned");
        assert_eq!(
            agent.spec.as_ref().unwrap().runtime.as_ref().unwrap().kind,
            "acp"
        );
        assert_eq!(agent.status.as_ref().unwrap().phase, "Ready");
    }

    #[tokio::test]
    async fn resource_store_rejects_generic_payloads_for_known_kinds() {
        let kv = Arc::new(MockKvStore::new());
        let store = ResourceStore::new(kv.clone(), Arc::new(RecordingPubSub::default()));
        let legacy = resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Agent".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "legacy".to_string(),
                namespace: "customers:acme".to_string(),
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::Agent(
                    resources_proto::AgentSpec {
                        system_prompt: "legacy payload".to_string(),
                        ..Default::default()
                    },
                )),
            }),
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resources_proto::resource_status::Kind::Agent(
                    Default::default(),
                )),
            }),
        };
        kv.set(
            &crate::control::keys::agent("customers:acme", "legacy"),
            &legacy.encode_to_vec(),
        )
        .await
        .unwrap();

        let err = store
            .get("customers:acme", "Agent", "legacy")
            .await
            .expect_err("known kinds must use kind-specific protobuf storage");
        assert!(err.to_string().contains("failed to decode stored Agent"));
    }

    #[tokio::test]
    async fn resource_store_stores_skill_as_typed_payload() {
        let kv = Arc::new(MockKvStore::new());
        let store = ResourceStore::new(kv.clone(), Arc::new(RecordingPubSub::default()));
        store
            .upsert(
                "customers:acme",
                resources_proto::Resource {
                    api_version: "talon.impalasys.com/v1".to_string(),
                    kind: "Skill".to_string(),
                    metadata: Some(resources_proto::ResourceMeta {
                        name: "review".to_string(),
                        namespace: "customers:acme".to_string(),
                        ..Default::default()
                    }),
                    spec: Some(resources_proto::ResourceSpec {
                        kind: Some(resources_proto::resource_spec::Kind::Skill(
                            resources_proto::SkillSpec {
                                description: "Review code".to_string(),
                                instructions: "Look for regressions.".to_string(),
                            },
                        )),
                    }),
                    status: None,
                },
            )
            .await
            .unwrap();

        let stored = kv
            .get(&crate::control::keys::skill("customers:acme", "review"))
            .await
            .unwrap()
            .expect("stored Skill payload");
        let stored_skill =
            resources_proto::Skill::decode(stored.as_slice()).expect("stored payload is Skill");
        assert_eq!(
            stored_skill.spec.as_ref().unwrap().instructions,
            "Look for regressions."
        );
    }

    #[tokio::test]
    async fn manifest_upsert_preserves_controller_owned_status() {
        let store = ResourceStore::new(
            Arc::new(MockKvStore::new()),
            Arc::new(RecordingPubSub::default()),
        );
        let manifest = resources_proto::ResourceManifest {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Agent".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "coding".to_string(),
                namespace: "customers:acme".to_string(),
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::Agent(
                    resources_proto::AgentSpec {
                        system_prompt: "first".to_string(),
                        ..Default::default()
                    },
                )),
            }),
        };

        let created = store
            .upsert_manifest("customers:acme", manifest.clone())
            .await
            .unwrap();
        assert_eq!(created.metadata.as_ref().unwrap().generation, 1);
        assert!(matches!(
            created
                .status
                .as_ref()
                .and_then(|status| status.kind.as_ref()),
            Some(resources_proto::resource_status::Kind::Agent(_))
        ));

        store
            .patch_status(
                "customers:acme",
                "Agent",
                "coding",
                None,
                resources_proto::ResourceStatus {
                    kind: Some(resources_proto::resource_status::Kind::Agent(
                        resources_proto::AgentStatus {
                            observed_generation: 1,
                            phase: "Ready".to_string(),
                            conditions: Vec::new(),
                            last_session_id: Some("session-1".to_string()),
                        },
                    )),
                },
            )
            .await
            .unwrap();

        let mut next = manifest;
        next.spec = Some(resources_proto::ResourceSpec {
            kind: Some(resources_proto::resource_spec::Kind::Agent(
                resources_proto::AgentSpec {
                    system_prompt: "second".to_string(),
                    ..Default::default()
                },
            )),
        });
        let updated = store.upsert_manifest("customers:acme", next).await.unwrap();
        assert_eq!(updated.metadata.as_ref().unwrap().generation, 2);
        match updated
            .status
            .as_ref()
            .and_then(|status| status.kind.as_ref())
        {
            Some(resources_proto::resource_status::Kind::Agent(status)) => {
                assert_eq!(status.phase, "Ready");
                assert_eq!(status.last_session_id.as_deref(), Some("session-1"));
            }
            other => panic!("expected Agent status, got {other:?}"),
        }
        match updated.spec.as_ref().and_then(|spec| spec.kind.as_ref()) {
            Some(resources_proto::resource_spec::Kind::Agent(spec)) => {
                assert_eq!(spec.system_prompt, "second");
            }
            other => panic!("expected Agent spec, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn manifest_upsert_overwrites_undecodable_typed_resource_payload() {
        let kv = Arc::new(MockKvStore::new());
        let store = ResourceStore::new(kv.clone(), Arc::new(RecordingPubSub::default()));
        let legacy = resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Deployment".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "conic-cmo".to_string(),
                namespace: "Conic:Customers".to_string(),
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::Deployment(
                    resources_proto::DeploymentSpec {
                        templates: vec!["old-template".to_string()],
                        ..Default::default()
                    },
                )),
            }),
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resources_proto::resource_status::Kind::Deployment(
                    Default::default(),
                )),
            }),
        };
        let key = crate::control::keys::ResourceKey::new(
            "Conic:Customers",
            &[],
            "Deployment",
            "conic-cmo",
        );
        kv.set(&key, &legacy.encode_to_vec()).await.unwrap();

        let updated = store
            .upsert_manifest(
                "Conic:Customers",
                resources_proto::ResourceManifest {
                    api_version: "talon.impalasys.com/v1".to_string(),
                    kind: "Deployment".to_string(),
                    metadata: Some(resources_proto::ResourceMeta {
                        name: "conic-cmo".to_string(),
                        namespace: "Conic:Customers".to_string(),
                        ..Default::default()
                    }),
                    spec: Some(resources_proto::ResourceSpec {
                        kind: Some(resources_proto::resource_spec::Kind::Deployment(
                            resources_proto::DeploymentSpec {
                                templates: vec!["conic-cmo".to_string()],
                                ..Default::default()
                            },
                        )),
                    }),
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.metadata.as_ref().unwrap().generation, 1);
        match updated.spec.as_ref().and_then(|spec| spec.kind.as_ref()) {
            Some(resources_proto::resource_spec::Kind::Deployment(spec)) => {
                assert_eq!(spec.templates, vec!["conic-cmo"]);
            }
            other => panic!("expected Deployment spec, got {other:?}"),
        }
        let stored = kv.get(&key).await.unwrap().expect("stored Deployment");
        let stored_deployment =
            resources_proto::Deployment::decode(stored.as_slice()).expect("typed Deployment");
        assert_eq!(
            stored_deployment.spec.as_ref().unwrap().templates,
            vec!["conic-cmo"]
        );
    }
}
