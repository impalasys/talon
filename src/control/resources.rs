// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::events;
use crate::control::{keys, topics, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt};
use crate::gateway::rpc::resources_proto;
use anyhow::{anyhow, Result};
use prost::Message;
use std::sync::Arc;

const API_VERSION: &str = "talon.impalasys.com/v1";

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

    pub async fn upsert(
        &self,
        namespace: &str,
        mut resource: resources_proto::Resource,
    ) -> Result<resources_proto::Resource> {
        normalize_resource(namespace, &mut resource)?;
        let key = resource_key(&resource);
        validate_resource_kind(&resource)?;
        let existing = self.kv.get_msg::<resources_proto::Resource>(&key).await?;
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
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let resource_version = uuid::Uuid::now_v7().to_string();

        let meta = resource
            .metadata
            .as_mut()
            .ok_or_else(|| anyhow!("resource metadata missing after normalization"))?;
        meta.generation = generation;
        meta.uid = uid;
        meta.resource_version = resource_version;

        self.kv.set_msg(&key, &resource).await?;
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
            let mut resource = resources_proto::Resource::decode(current.as_slice())?;
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
            meta.resource_version = uuid::Uuid::now_v7().to_string();
            validate_resource_kind(&resource)?;
            let next = resource.encode_to_vec();
            if self
                .kv
                .compare_and_swap(&key, Some(current.as_slice()), &next)
                .await?
            {
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
        self.kv
            .get_msg::<resources_proto::Resource>(&keys::ResourceKey::new(
                namespace,
                &[],
                kind,
                name,
            ))
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
        let mut resources = Vec::with_capacity(entries.len());
        for (_, value) in entries {
            resources.push(resources_proto::Resource::decode(value.as_slice())?);
        }
        Ok(resources)
    }

    pub async fn delete(&self, namespace: &str, kind: &str, name: &str) -> Result<bool> {
        let key = keys::ResourceKey::new(namespace, &[], kind, name);
        let Some(mut resource) = self.kv.get_msg::<resources_proto::Resource>(&key).await? else {
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
            meta.resource_version = uuid::Uuid::now_v7().to_string();
            self.kv.set_msg(&key, &resource).await?;
            self.publish_changed(
                &resource,
                events::ResourceChangeType::Updated,
                &["metadata"],
            )
            .await?;
        } else {
            self.kv.delete(&key).await?;
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
        Kind::McpServer(_) => "McpServer",
        Kind::McpServerBinding(_) => "McpServerBinding",
        Kind::Knowledge(_) => "Knowledge",
        Kind::Namespace(_) => "Namespace",
        Kind::Session(_) => "Session",
        Kind::Template(_) => "Template",
        Kind::Deployment(_) => "Deployment",
        Kind::DeploymentReplica(_) => "DeploymentReplica",
        Kind::SandboxClass(_) => "SandboxClass",
        Kind::SandboxPolicy(_) => "SandboxPolicy",
        Kind::Sandbox(_) => "Sandbox",
        Kind::PermissionRequest(_) => "PermissionRequest",
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
    use crate::control::topics;
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
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, topics::RESOURCE_LIFECYCLE_TOPIC);
        let event = events::ResourceChangedEvent::decode(published[0].1.as_slice()).unwrap();
        assert_eq!(event.resource_kind, "Template");
        assert_eq!(
            event.change_type,
            events::ResourceChangeType::Created as i32
        );
    }

    #[tokio::test]
    async fn resource_store_returns_agent_typed_view_from_resource() {
        let store = ResourceStore::new(
            Arc::new(MockKvStore::new()),
            Arc::new(RecordingPubSub::default()),
        );
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
}
