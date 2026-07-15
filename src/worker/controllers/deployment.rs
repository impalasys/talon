// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::resource_model::{NamespaceResourceExt, TypedResource};
use crate::control::resources::ResourceStore;
use crate::control::{keys, ControlPlane, ProtoKeyValueStoreExt};
use crate::gateway::rpc::resources_proto;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DeploymentSpecJson {
    pub placement: DeploymentPlacementJson,
    pub templates: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DeploymentPlacementJson {
    pub namespace_selector: NamespaceSelectorJson,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NamespaceSelectorJson {
    pub parent: String,
    pub match_labels: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TemplateSpecJson {
    pub kind: String,
    pub metadata: TemplateMetadataJson,
    pub spec: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TemplateMetadataJson {
    pub name: String,
    pub labels: std::collections::HashMap<String, String>,
    pub annotations: std::collections::HashMap<String, String>,
}

pub struct DeploymentController {
    store: ResourceStore,
}

impl DeploymentController {
    pub fn new(store: ResourceStore) -> Self {
        Self { store }
    }

    pub async fn reconcile_deployment(
        &self,
        deployment: &resources_proto::Resource,
    ) -> Result<resources_proto::DeploymentSpec> {
        if deployment.kind != "Deployment" {
            return Err(anyhow!("expected Deployment, got {}", deployment.kind));
        }
        let Some(resources_proto::resource_spec::Kind::Deployment(spec)) =
            deployment.spec.as_ref().and_then(|spec| spec.kind.as_ref())
        else {
            return Err(anyhow!(
                "Deployment resource is missing typed Deployment spec"
            ));
        };
        Ok(spec.clone())
    }

    pub async fn reconcile_once(
        &self,
        deployment: &resources_proto::Resource,
        cp: &ControlPlane,
    ) -> Result<()> {
        let spec = self.reconcile_deployment(deployment).await?;
        let deployment_meta = deployment
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("Deployment metadata is required"))?;
        let selector = spec
            .placement
            .as_ref()
            .and_then(|placement| placement.namespace_selector.as_ref())
            .ok_or_else(|| anyhow!("Deployment placement.namespaceSelector is required"))?;
        let targets = self.target_namespaces(cp, selector).await?;
        let mut existing_replicas = self
            .replicas_for_deployment(&deployment_meta.namespace, &deployment_meta.name)
            .await?;
        let target_names = targets
            .iter()
            .map(|target| target.name().to_string())
            .collect::<HashSet<_>>();
        let mut replica_counts = resources_proto::DeploymentReplicaCounts {
            desired: target_names.len() as u64,
            ..Default::default()
        };
        let stale_target_names = existing_replicas
            .keys()
            .filter(|target_namespace| !target_names.contains(*target_namespace))
            .cloned()
            .collect::<Vec<_>>();

        for target_namespace in stale_target_names {
            let Some(replica) = existing_replicas.remove(&target_namespace) else {
                continue;
            };
            self.delete_replica_outputs(&replica, cp).await?;
            if let Some(meta) = replica.metadata.as_ref() {
                self.store
                    .delete(&deployment_meta.namespace, "DeploymentReplica", &meta.name)
                    .await?;
            }
        }

        for target in targets {
            let mut rendered_refs = Vec::new();
            let mut rendered_hashes = HashMap::new();
            let mut last_rendered_json = HashMap::new();
            let mut conflicts = Vec::new();
            for template_name in &spec.templates {
                let Some(template) = self
                    .store
                    .get(&deployment_meta.namespace, "Template", template_name)
                    .await?
                else {
                    conflicts.push(format!(
                        "Template '{}' not found in namespace '{}'",
                        template_name, deployment_meta.namespace
                    ));
                    continue;
                };
                let rendered = self.render_template_with_namespace(
                    &deployment_meta.namespace,
                    target.name(),
                    &target,
                    deployment,
                    &template,
                )?;
                let rendered_meta = rendered
                    .metadata
                    .as_ref()
                    .ok_or_else(|| anyhow!("rendered resource metadata is required"))?;
                let rendered_key = format!("{}/{}", rendered.kind, rendered_meta.name);
                let rendered_json = crate::control::manifest::render_resource_yaml(&rendered)?;
                rendered_hashes.insert(rendered_key.clone(), stable_hash(&rendered_json));
                last_rendered_json.insert(rendered_key.clone(), rendered_json);
                rendered_refs.push(format!(
                    "{}/{}/{}",
                    rendered_meta.namespace, rendered.kind, rendered_meta.name
                ));
                self.apply_rendered(target.name(), rendered, cp).await?;
            }

            let rendered_ref_set = rendered_refs.iter().cloned().collect::<HashSet<_>>();
            if let Some(previous) = existing_replicas.get(target.name()) {
                self.delete_removed_outputs(previous, &rendered_ref_set, cp)
                    .await?;
            }

            let replica_name = replica_name(&deployment_meta.name, target.name());
            let phase = if conflicts.is_empty() {
                "Ready"
            } else {
                "Degraded"
            };
            replica_counts.updated += 1;
            if phase == "Ready" {
                replica_counts.ready += 1;
            } else {
                replica_counts.degraded += 1;
            }
            let replica = resources_proto::Resource {
                api_version: deployment.api_version.clone(),
                kind: "DeploymentReplica".to_string(),
                metadata: Some(resources_proto::ResourceMeta {
                    name: replica_name,
                    namespace: deployment_meta.namespace.clone(),
                    labels: std::collections::HashMap::from([
                        (
                            "talon.impalasys.com/deployment".to_string(),
                            deployment_meta.name.clone(),
                        ),
                        (
                            "talon.impalasys.com/target-namespace".to_string(),
                            target.name().to_string(),
                        ),
                    ]),
                    annotations: std::collections::HashMap::new(),
                    owner_references: vec![resources_proto::OwnerReference {
                        api_version: deployment.api_version.clone(),
                        kind: "Deployment".to_string(),
                        namespace: deployment_meta.namespace.clone(),
                        name: deployment_meta.name.clone(),
                        uid: deployment_meta.uid.clone(),
                        controller: true,
                        block_owner_deletion: true,
                    }],
                    finalizers: Vec::new(),
                    generation: 0,
                    resource_version: String::new(),
                    uid: String::new(),
                    deletion_timestamp: None,
                }),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(resources_proto::resource_spec::Kind::DeploymentReplica(
                        resources_proto::DeploymentReplicaSpec {
                            deployment_ref: Some(resources_proto::ResourceRef {
                                namespace: deployment_meta.namespace.clone(),
                                name: deployment_meta.name.clone(),
                            }),
                            target_namespace: target.name().to_string(),
                        },
                    )),
                }),
                status: Some(resources_proto::ResourceStatus {
                    kind: Some(resources_proto::resource_status::Kind::DeploymentReplica(
                        resources_proto::DeploymentReplicaStatus {
                            observed_generation: deployment_meta.generation,
                            phase: phase.to_string(),
                            conditions: Vec::new(),
                            rendered_resources: rendered_refs,
                            rendered_hashes,
                            conflicts,
                            last_rendered_json,
                            owned_json_pointers: Vec::new(),
                        },
                    )),
                }),
            };
            self.store
                .upsert(&deployment_meta.namespace, replica)
                .await?;
        }

        replica_counts.pending = replica_counts
            .desired
            .saturating_sub(replica_counts.updated);
        let phase = deployment_phase(&replica_counts);
        let status = resources_proto::ResourceStatus {
            kind: Some(resources_proto::resource_status::Kind::Deployment(
                resources_proto::DeploymentStatus {
                    observed_generation: deployment_meta.generation,
                    phase: phase.to_string(),
                    conditions: Vec::new(),
                    replicas: Vec::new(),
                    replica_counts: Some(replica_counts),
                },
            )),
        };
        if deployment.status.as_ref() != Some(&status) {
            self.store
                .patch_status(
                    &deployment_meta.namespace,
                    "Deployment",
                    &deployment_meta.name,
                    None,
                    status,
                )
                .await?;
        }

        Ok(())
    }

    async fn replicas_for_deployment(
        &self,
        deployment_namespace: &str,
        deployment_name: &str,
    ) -> Result<HashMap<String, resources_proto::Resource>> {
        let mut replicas = HashMap::new();
        for replica in self
            .store
            .list(deployment_namespace, Some("DeploymentReplica"))
            .await?
        {
            let Some(meta) = replica.metadata.as_ref() else {
                continue;
            };
            if meta
                .labels
                .get("talon.impalasys.com/deployment")
                .map(String::as_str)
                != Some(deployment_name)
            {
                continue;
            }
            if let Some(target_namespace) = meta.labels.get("talon.impalasys.com/target-namespace")
            {
                replicas.insert(target_namespace.clone(), replica);
            }
        }
        Ok(replicas)
    }

    async fn delete_replica_outputs(
        &self,
        replica: &resources_proto::Resource,
        cp: &ControlPlane,
    ) -> Result<()> {
        let Some(resources_proto::resource_status::Kind::DeploymentReplica(status)) = replica
            .status
            .as_ref()
            .and_then(|status| status.kind.as_ref())
        else {
            return Ok(());
        };

        for rendered_ref in &status.rendered_resources {
            self.delete_rendered_ref(rendered_ref, cp).await?;
        }
        Ok(())
    }

    async fn delete_removed_outputs(
        &self,
        previous: &resources_proto::Resource,
        desired_refs: &HashSet<String>,
        cp: &ControlPlane,
    ) -> Result<()> {
        let Some(resources_proto::resource_status::Kind::DeploymentReplica(status)) = previous
            .status
            .as_ref()
            .and_then(|status| status.kind.as_ref())
        else {
            return Ok(());
        };

        for rendered_ref in &status.rendered_resources {
            if !desired_refs.contains(rendered_ref) {
                self.delete_rendered_ref(rendered_ref, cp).await?;
            }
        }
        Ok(())
    }

    async fn delete_rendered_ref(&self, rendered_ref: &str, cp: &ControlPlane) -> Result<()> {
        let Some((namespace, kind, name)) = parse_rendered_ref(rendered_ref) else {
            tracing::warn!(
                rendered_ref,
                "skipping malformed DeploymentReplica rendered resource ref"
            );
            return Ok(());
        };
        if kind == "Schedule" {
            if let Some(schedule) = self.store.get(namespace, kind, name).await? {
                if let Some(handle) =
                    schedule
                        .status
                        .and_then(|status| status.kind)
                        .and_then(|kind| match kind {
                            resources_proto::resource_status::Kind::Schedule(status) => {
                                status.backend_handle
                            }
                            _ => None,
                        })
                {
                    cp.scheduler.cancel(&handle).await?;
                }
            }
        }
        self.store.delete(namespace, kind, name).await?;
        Ok(())
    }

    async fn target_namespaces(
        &self,
        cp: &ControlPlane,
        selector: &resources_proto::NamespaceSelector,
    ) -> Result<Vec<resources_proto::Namespace>> {
        let refs = cp
            .kv
            .list_entries(&keys::namespace_ref_prefix(Some(&selector.parent)), None)
            .await?;
        let mut namespaces = Vec::new();
        for (_, value) in refs {
            let name = String::from_utf8(value)?;
            let Some(namespace) = cp
                .kv
                .get_msg::<resources_proto::Namespace>(&keys::namespace_metadata(&name))
                .await?
            else {
                continue;
            };
            if namespace.is_deleted() {
                continue;
            }
            if selector
                .match_labels
                .iter()
                .all(|(key, value)| namespace.labels().get(key) == Some(value))
            {
                namespaces.push(namespace);
            }
        }
        Ok(namespaces)
    }

    pub fn render_template(
        &self,
        source_namespace: &str,
        target_namespace: &str,
        template: &resources_proto::Resource,
    ) -> Result<resources_proto::Resource> {
        if template.kind != "Template" {
            return Err(anyhow!("expected Template, got {}", template.kind));
        }
        let Some(resources_proto::resource_spec::Kind::Template(spec)) =
            template.spec.as_ref().and_then(|spec| spec.kind.as_ref())
        else {
            return Err(anyhow!("Template resource is missing typed Template spec"));
        };
        if spec.kind.trim().is_empty() {
            return Err(anyhow!("Template spec.kind is required"));
        }
        let template_meta = template
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("Template metadata is required"))?;
        let rendered_meta = spec.metadata.clone().unwrap_or_default();
        let rendered_name = if rendered_meta.name.trim().is_empty() {
            template_meta.name.clone()
        } else {
            rendered_meta.name.clone()
        };
        let (rendered_spec, rendered_status) =
            crate::control::manifest::resource_spec_status_from_json(
                &spec.kind,
                &spec.spec_json,
                "{}",
            )?;

        Ok(resources_proto::Resource {
            api_version: template.api_version.clone(),
            kind: spec.kind.clone(),
            metadata: Some(resources_proto::ResourceMeta {
                name: rendered_name,
                namespace: target_namespace.to_string(),
                labels: rendered_meta.labels,
                annotations: rendered_meta.annotations,
                owner_references: vec![resources_proto::OwnerReference {
                    api_version: template.api_version.clone(),
                    kind: "Template".to_string(),
                    namespace: source_namespace.to_string(),
                    name: template_meta.name.clone(),
                    uid: template_meta.uid.clone(),
                    controller: false,
                    block_owner_deletion: false,
                }],
                finalizers: Vec::new(),
                generation: 0,
                resource_version: String::new(),
                uid: String::new(),
                deletion_timestamp: None,
            }),
            spec: Some(rendered_spec),
            status: Some(rendered_status),
        })
    }

    pub fn render_template_with_namespace(
        &self,
        source_namespace: &str,
        target_namespace: &str,
        namespace: &resources_proto::Namespace,
        deployment: &resources_proto::Resource,
        template: &resources_proto::Resource,
    ) -> Result<resources_proto::Resource> {
        let Some(resources_proto::resource_spec::Kind::Template(spec)) =
            template.spec.as_ref().and_then(|spec| spec.kind.as_ref())
        else {
            return Err(anyhow!("Template resource is missing typed Template spec"));
        };
        let deployment_meta = deployment
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("Deployment metadata is required"))?;
        let template_meta = template
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("Template metadata is required"))?;
        let namespace_meta = namespace
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("Namespace metadata is required"))?;
        let customer_name = namespace_meta
            .annotations
            .get("vibiz.io/customer-name")
            .map(String::as_str)
            .unwrap_or_else(|| namespace.name());
        let rendered_spec = crate::control::manifest::templating::render_resource_template(
            &spec.spec_json,
            serde_json::json!({
                "namespace": {
                    "name": namespace.name(),
                    "parent": namespace.parent(),
                    "customerName": customer_name,
                    "metadata": {
                        "labels": namespace_meta.labels,
                        "annotations": namespace_meta.annotations,
                    }
                },
                "deployment": {
                    "metadata": {
                        "name": deployment_meta.name,
                        "namespace": deployment_meta.namespace,
                        "labels": deployment_meta.labels,
                        "annotations": deployment_meta.annotations,
                    }
                },
                "template": {
                    "metadata": {
                        "name": template_meta.name,
                        "namespace": template_meta.namespace,
                        "labels": template_meta.labels,
                        "annotations": template_meta.annotations,
                    }
                },
            }),
        )?;
        let mut rendered_template = template.clone();
        if let Some(resources_proto::resource_spec::Kind::Template(rendered)) = rendered_template
            .spec
            .as_mut()
            .and_then(|spec| spec.kind.as_mut())
        {
            rendered.spec_json = rendered_spec;
        }
        self.render_template(source_namespace, target_namespace, &rendered_template)
    }

    pub async fn apply_rendered(
        &self,
        namespace: &str,
        resource: resources_proto::Resource,
        cp: &ControlPlane,
    ) -> Result<resources_proto::Resource> {
        if resource.kind == "Schedule" {
            return self.apply_rendered_schedule(namespace, resource, cp).await;
        }
        self.store.upsert(namespace, resource).await
    }

    async fn apply_rendered_schedule(
        &self,
        namespace: &str,
        resource: resources_proto::Resource,
        cp: &ControlPlane,
    ) -> Result<resources_proto::Resource> {
        use resources_proto::resource_spec::Kind as SpecKind;
        use resources_proto::resource_status::Kind as StatusKind;

        let api_version = resource.api_version.clone();
        let metadata = resource
            .metadata
            .clone()
            .ok_or_else(|| anyhow!("rendered Schedule metadata is required"))?;
        let schedule_name = metadata.name.clone();
        let spec = match resource.spec.as_ref().and_then(|spec| spec.kind.as_ref()) {
            Some(SpecKind::Schedule(spec)) => spec.clone(),
            _ => return Err(anyhow!("rendered Schedule is missing typed Schedule spec")),
        };
        let rendered_status = resource
            .status
            .as_ref()
            .and_then(|status| status.kind.as_ref())
            .and_then(|kind| match kind {
                StatusKind::Schedule(status) => Some(status.clone()),
                _ => None,
            });
        let existing_status = self
            .store
            .get(namespace, "Schedule", &metadata.name)
            .await?
            .and_then(|existing| existing.status)
            .and_then(|status| status.kind)
            .and_then(|kind| match kind {
                StatusKind::Schedule(status) => Some(status),
                _ => None,
            });

        let mut schedule = resources_proto::Schedule {
            metadata: Some(metadata),
            spec: Some(spec),
            status: Some(
                existing_status
                    .or(rendered_status)
                    .unwrap_or_else(resources_proto::ScheduleStatus::default),
            ),
        };
        let next_run_at =
            crate::control::scheduling::initialize_schedule(&mut schedule, chrono::Utc::now())?;
        let stored = self
            .store
            .upsert(namespace, schedule_resource(&api_version, &schedule))
            .await?;
        let expected_resource_version = stored
            .metadata
            .as_ref()
            .map(|meta| meta.resource_version.clone());
        crate::control::scheduling::arm_schedule(cp.scheduler.as_ref(), &mut schedule, next_run_at)
            .await?;

        let status = resources_proto::ResourceStatus {
            kind: Some(resources_proto::resource_status::Kind::Schedule(
                schedule.status.clone().unwrap_or_default(),
            )),
        };
        let patch_result = self
            .store
            .patch_status(
                namespace,
                "Schedule",
                &schedule_name,
                expected_resource_version.as_deref(),
                status,
            )
            .await;
        match patch_result {
            Ok(resource) => Ok(resource),
            Err(err) => {
                if let Some(handle) = schedule
                    .status
                    .as_ref()
                    .and_then(|status| status.backend_handle.as_deref())
                {
                    if let Err(cancel_err) = cp.scheduler.cancel(handle).await {
                        tracing::warn!(
                            schedule = %schedule_name,
                            handle,
                            error = %cancel_err,
                            "failed to cancel newly armed schedule after status patch failure"
                        );
                    }
                }
                Err(err)
            }
        }
    }
}

fn deployment_phase(counts: &resources_proto::DeploymentReplicaCounts) -> &'static str {
    if counts.degraded > 0 {
        "Degraded"
    } else if counts.pending > 0 {
        "Pending"
    } else {
        "Ready"
    }
}

fn schedule_resource(
    api_version: &str,
    schedule: &resources_proto::Schedule,
) -> resources_proto::Resource {
    use resources_proto::resource_spec::Kind as SpecKind;
    use resources_proto::resource_status::Kind as StatusKind;

    resources_proto::Resource {
        api_version: api_version.to_string(),
        kind: "Schedule".to_string(),
        metadata: schedule.metadata.clone(),
        spec: Some(resources_proto::ResourceSpec {
            kind: Some(SpecKind::Schedule(
                schedule.spec.clone().unwrap_or_default(),
            )),
        }),
        status: Some(resources_proto::ResourceStatus {
            kind: Some(StatusKind::Schedule(
                schedule.status.clone().unwrap_or_default(),
            )),
        }),
    }
}

pub(crate) fn replica_name(deployment_name: &str, target_namespace: &str) -> String {
    format!(
        "{}--{}",
        deployment_name,
        escape_replica_namespace(target_namespace)
    )
}

fn escape_replica_namespace(namespace: &str) -> String {
    namespace.replace(':', "-").replace('/', "-")
}

fn stable_hash(value: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn parse_rendered_ref(rendered_ref: &str) -> Option<(&str, &str, &str)> {
    let mut parts = rendered_ref.splitn(3, '/');
    Some((parts.next()?, parts.next()?, parts.next()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ProtoKeyValueStoreExt;
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use std::sync::Arc;

    fn controller() -> DeploymentController {
        DeploymentController::new(ResourceStore::new(
            Arc::new(MockKvStore::default()),
            Arc::new(RecordingPubSub::default()),
        ))
    }

    async fn add_namespace(cp: &ControlPlane, namespace: resources_proto::Namespace) {
        let name = namespace.name().to_string();
        let parent = namespace.parent().to_string();
        let child_segment = name.rsplit(':').next().unwrap_or(&name);
        cp.kv
            .set_msg(&keys::namespace_metadata(&name), &namespace)
            .await
            .expect("set namespace metadata");
        cp.kv
            .set(
                &keys::namespace_ref(
                    (!parent.is_empty()).then_some(parent.as_str()),
                    child_segment,
                ),
                name.as_bytes(),
            )
            .await
            .expect("set namespace ref");
    }

    fn deployment() -> resources_proto::Resource {
        resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Deployment".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "company-builder".to_string(),
                namespace: "customers".to_string(),
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::Deployment(
                    resources_proto::DeploymentSpec::default(),
                )),
            }),
            status: None,
        }
    }

    fn deployment_with_templates(templates: &[&str]) -> resources_proto::Resource {
        let mut deployment = deployment();
        let Some(resources_proto::resource_spec::Kind::Deployment(spec)) =
            deployment.spec.as_mut().and_then(|spec| spec.kind.as_mut())
        else {
            panic!("expected deployment spec");
        };
        spec.templates = templates.iter().map(|name| name.to_string()).collect();
        spec.placement = Some(resources_proto::DeploymentPlacement {
            namespace_selector: Some(resources_proto::NamespaceSelector {
                parent: "customers".to_string(),
                match_labels: std::collections::HashMap::from([(
                    "tier".to_string(),
                    "prod".to_string(),
                )]),
            }),
        });
        deployment
    }

    fn coding_template() -> resources_proto::Resource {
        resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Template".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "coding-agent".to_string(),
                namespace: "customers".to_string(),
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::Template(
                    resources_proto::TemplateSpec {
                        kind: "Agent".to_string(),
                        metadata: Some(resources_proto::ResourceMeta {
                            name: "coding".to_string(),
                            ..Default::default()
                        }),
                        spec_json: serde_json::json!({
                            "systemPrompt": "You are the coding agent for {{ namespace.customerName }}."
                        })
                        .to_string(),
                    },
                )),
            }),
            status: None,
        }
    }

    fn named_agent_template(template_name: &str, agent_name: &str) -> resources_proto::Resource {
        let mut template = coding_template();
        template.metadata.as_mut().unwrap().name = template_name.to_string();
        let Some(resources_proto::resource_spec::Kind::Template(spec)) =
            template.spec.as_mut().and_then(|spec| spec.kind.as_mut())
        else {
            panic!("expected template spec");
        };
        spec.metadata.as_mut().unwrap().name = agent_name.to_string();
        template
    }

    fn wakeup_template() -> resources_proto::Resource {
        resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Template".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "daily-wakeup".to_string(),
                namespace: "customers".to_string(),
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::Template(
                    resources_proto::TemplateSpec {
                        kind: "Schedule".to_string(),
                        metadata: Some(resources_proto::ResourceMeta {
                            name: "daily-wakeup".to_string(),
                            ..Default::default()
                        }),
                        spec_json: serde_json::json!({
                            "kind": "cron",
                            "cron": "0 9 * * *",
                            "timezone": "America/Los_Angeles",
                            "target": {
                                "agent": "cmo",
                                "sessionMode": "new"
                            },
                            "inputMessage": "Review {{ namespace.customerName }}.",
                            "enabled": true
                        })
                        .to_string(),
                    },
                )),
            }),
            status: None,
        }
    }

    fn workflow_template() -> resources_proto::Resource {
        resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Template".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "backlink-outreach".to_string(),
                namespace: "customers".to_string(),
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::Template(
                    resources_proto::TemplateSpec {
                        kind: "Workflow".to_string(),
                        metadata: Some(resources_proto::ResourceMeta {
                            name: "backlink-outreach".to_string(),
                            ..Default::default()
                        }),
                        spec_json: serde_json::json!({
                            "description": "Prepare outreach drafts for {{ namespace.customerName }}.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "instruction": {
                                        "type": "string"
                                    }
                                }
                            },
                            "steps": [
                                {
                                    "id": "copy",
                                    "type": "transform",
                                    "input": {
                                        "instruction": "${$.input.instruction}"
                                    }
                                }
                            ],
                            "output": {
                                "instruction": "${$.steps.copy.output.instruction}"
                            }
                        })
                        .to_string(),
                    },
                )),
            }),
            status: None,
        }
    }

    fn target_namespace(annotation: Option<&str>) -> resources_proto::Namespace {
        let mut annotations = std::collections::HashMap::new();
        if let Some(value) = annotation {
            annotations.insert("vibiz.io/customer-name".to_string(), value.to_string());
        }
        resources_proto::Namespace {
            metadata: Some(resources_proto::ResourceMeta {
                name: "customers:acme".to_string(),
                annotations,
                ..Default::default()
            }),
            spec: Some(resources_proto::NamespaceSpec {
                parent: "customers".to_string(),
            }),
            status: Some(resources_proto::NamespaceStatus::default()),
        }
    }

    fn labeled_target_namespace(tier: &str) -> resources_proto::Namespace {
        let mut namespace = target_namespace(Some("Acme"));
        namespace
            .metadata
            .as_mut()
            .unwrap()
            .labels
            .insert("tier".to_string(), tier.to_string());
        namespace
    }

    fn rendered_prompt(resource: resources_proto::Resource) -> String {
        let Some(resources_proto::resource_spec::Kind::Agent(spec)) =
            resource.spec.and_then(|spec| spec.kind)
        else {
            panic!("expected rendered Agent");
        };
        spec.system_prompt
    }

    fn rendered_schedule_spec(
        resource: resources_proto::Resource,
    ) -> resources_proto::ScheduleSpec {
        let Some(resources_proto::resource_spec::Kind::Schedule(spec)) =
            resource.spec.and_then(|spec| spec.kind)
        else {
            panic!("expected rendered Schedule");
        };
        spec
    }

    fn rendered_workflow_spec(
        resource: resources_proto::Resource,
    ) -> resources_proto::WorkflowSpec {
        let Some(resources_proto::resource_spec::Kind::Workflow(spec)) =
            resource.spec.and_then(|spec| spec.kind)
        else {
            panic!("expected rendered Workflow");
        };
        spec
    }

    fn replica_status(
        resource: resources_proto::Resource,
    ) -> resources_proto::DeploymentReplicaStatus {
        let Some(resources_proto::resource_status::Kind::DeploymentReplica(status)) =
            resource.status.and_then(|status| status.kind)
        else {
            panic!("expected deployment replica status");
        };
        status
    }

    fn deployment_status(resource: resources_proto::Resource) -> resources_proto::DeploymentStatus {
        let Some(resources_proto::resource_status::Kind::Deployment(status)) =
            resource.status.and_then(|status| status.kind)
        else {
            panic!("expected deployment status");
        };
        status
    }

    fn replica_counts(
        status: &resources_proto::DeploymentStatus,
    ) -> &resources_proto::DeploymentReplicaCounts {
        status
            .replica_counts
            .as_ref()
            .expect("deployment replica counts")
    }

    fn deployment_meta_generation(resource: &resources_proto::Resource) -> u64 {
        resource
            .metadata
            .as_ref()
            .expect("deployment metadata")
            .generation
    }

    #[test]
    fn deployment_phase_prefers_degraded_then_pending_then_ready() {
        assert_eq!(
            deployment_phase(&resources_proto::DeploymentReplicaCounts {
                desired: 3,
                updated: 2,
                ready: 2,
                pending: 1,
                degraded: 0,
            }),
            "Pending"
        );
        assert_eq!(
            deployment_phase(&resources_proto::DeploymentReplicaCounts {
                desired: 3,
                updated: 3,
                ready: 2,
                pending: 0,
                degraded: 1,
            }),
            "Degraded"
        );
        assert_eq!(
            deployment_phase(&resources_proto::DeploymentReplicaCounts {
                desired: 3,
                updated: 3,
                ready: 3,
                pending: 0,
                degraded: 0,
            }),
            "Ready"
        );
    }

    #[test]
    fn render_template_uses_namespace_annotation_with_name_fallback() {
        let rendered = controller()
            .render_template_with_namespace(
                "customers",
                "customers:acme",
                &target_namespace(Some("Acme")),
                &deployment(),
                &coding_template(),
            )
            .expect("render with annotation");
        assert_eq!(
            rendered_prompt(rendered),
            "You are the coding agent for Acme."
        );

        let rendered = controller()
            .render_template_with_namespace(
                "customers",
                "customers:acme",
                &target_namespace(None),
                &deployment(),
                &coding_template(),
            )
            .expect("render without annotation");
        assert_eq!(
            rendered_prompt(rendered),
            "You are the coding agent for customers:acme."
        );
    }

    #[test]
    fn render_template_preserves_runtime_prompt_variables() {
        let mut template = coding_template();
        let Some(resources_proto::resource_spec::Kind::Template(spec)) =
            template.spec.as_mut().and_then(|spec| spec.kind.as_mut())
        else {
            panic!("expected template spec");
        };
        spec.spec_json = serde_json::json!({
            "systemPrompt": "Agent for {{ namespace.customerName }} at {{ talon.now }}."
        })
        .to_string();

        let rendered = controller()
            .render_template_with_namespace(
                "customers",
                "customers:acme",
                &target_namespace(Some("Acme")),
                &deployment(),
                &template,
            )
            .expect("render with runtime variable");

        assert_eq!(
            rendered_prompt(rendered),
            "Agent for Acme at {{ talon.now }}."
        );
    }

    #[test]
    fn render_template_supports_schedule_specs() {
        let rendered = controller()
            .render_template_with_namespace(
                "customers",
                "customers:acme",
                &target_namespace(Some("Acme")),
                &deployment(),
                &wakeup_template(),
            )
            .expect("render schedule");

        assert_eq!(rendered.kind, "Schedule");
        let spec = rendered_schedule_spec(rendered);
        assert_eq!(spec.kind, "cron");
        assert_eq!(spec.cron, "0 9 * * *");
        assert_eq!(spec.timezone, "America/Los_Angeles");
        assert_eq!(spec.input_message, "Review Acme.");
        let target = spec.target.expect("schedule target");
        assert_eq!(target.agent, "cmo");
        assert_eq!(target.session_mode, "new");
        assert!(spec.enabled);
    }

    #[test]
    fn render_template_supports_workflow_specs() {
        let rendered = controller()
            .render_template_with_namespace(
                "customers",
                "customers:acme",
                &target_namespace(Some("Acme")),
                &deployment(),
                &workflow_template(),
            )
            .expect("render workflow");

        assert_eq!(rendered.kind, "Workflow");
        let metadata = rendered.metadata.as_ref().expect("workflow metadata");
        assert_eq!(metadata.namespace, "customers:acme");
        assert_eq!(metadata.name, "backlink-outreach");
        let spec = rendered_workflow_spec(rendered);
        assert_eq!(spec.description, "Prepare outreach drafts for Acme.");
        assert_eq!(spec.steps.len(), 1);
        assert_eq!(spec.steps[0].id, "copy");
    }

    #[tokio::test]
    async fn reconcile_deletes_outputs_for_removed_template() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let store = ResourceStore::new(kv.clone(), pubsub.clone());
        let controller = DeploymentController::new(store.clone());
        let cp = ControlPlane::builder(kv, pubsub).build();
        add_namespace(&cp, labeled_target_namespace("prod")).await;

        let deployment = deployment_with_templates(&["coding-agent", "legacy-agent"]);
        let deployment = store
            .upsert("customers", deployment.clone())
            .await
            .expect("upsert deployment");
        store
            .upsert("customers", coding_template())
            .await
            .expect("upsert coding template");
        store
            .upsert("customers", named_agent_template("legacy-agent", "legacy"))
            .await
            .expect("upsert legacy template");

        controller
            .reconcile_once(&deployment, &cp)
            .await
            .expect("initial reconcile");
        assert!(store
            .get("customers:acme", "Agent", "legacy")
            .await
            .expect("get legacy")
            .is_some());

        store
            .delete("customers", "Template", "legacy-agent")
            .await
            .expect("delete legacy template");
        controller
            .reconcile_once(&deployment, &cp)
            .await
            .expect("reconcile missing template");

        assert!(store
            .get("customers:acme", "Agent", "coding")
            .await
            .expect("get coding")
            .is_some());
        assert!(store
            .get("customers:acme", "Agent", "legacy")
            .await
            .expect("get legacy")
            .is_none());
        let replica = store
            .get(
                "customers",
                "DeploymentReplica",
                &replica_name("company-builder", "customers:acme"),
            )
            .await
            .expect("get replica")
            .expect("replica exists");
        let status = replica_status(replica);
        assert_eq!(
            status.observed_generation,
            deployment_meta_generation(&deployment)
        );
        assert_eq!(status.phase, "Degraded");
        assert_eq!(status.rendered_resources.len(), 1);
        assert_eq!(status.conflicts.len(), 1);
        let deployment = store
            .get("customers", "Deployment", "company-builder")
            .await
            .expect("get deployment")
            .expect("deployment exists");
        let status = deployment_status(deployment);
        assert_eq!(status.phase, "Degraded");
        assert!(status.replicas.is_empty());
        let counts = replica_counts(&status);
        assert_eq!(counts.desired, 1);
        assert_eq!(counts.updated, 1);
        assert_eq!(counts.ready, 0);
        assert_eq!(counts.pending, 0);
        assert_eq!(counts.degraded, 1);
    }

    #[tokio::test]
    async fn reconcile_deletes_outputs_for_unmatched_namespace() {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(RecordingPubSub::default());
        let store = ResourceStore::new(kv.clone(), pubsub.clone());
        let controller = DeploymentController::new(store.clone());
        let cp = ControlPlane::builder(kv, pubsub).build();
        add_namespace(&cp, labeled_target_namespace("prod")).await;

        let deployment = deployment_with_templates(&["coding-agent"]);
        let deployment = store
            .upsert("customers", deployment.clone())
            .await
            .expect("upsert deployment");
        store
            .upsert("customers", coding_template())
            .await
            .expect("upsert coding template");

        controller
            .reconcile_once(&deployment, &cp)
            .await
            .expect("initial reconcile");
        assert!(store
            .get("customers:acme", "Agent", "coding")
            .await
            .expect("get coding")
            .is_some());
        let deployment_after_initial_reconcile = store
            .get("customers", "Deployment", "company-builder")
            .await
            .expect("get deployment")
            .expect("deployment exists");
        let status = deployment_status(deployment_after_initial_reconcile);
        assert_eq!(
            status.observed_generation,
            deployment_meta_generation(&deployment)
        );
        assert_eq!(status.phase, "Ready");
        let counts = replica_counts(&status);
        assert_eq!(counts.desired, 1);
        assert_eq!(counts.updated, 1);
        assert_eq!(counts.ready, 1);
        assert_eq!(counts.pending, 0);
        assert_eq!(counts.degraded, 0);

        add_namespace(&cp, labeled_target_namespace("staging")).await;
        controller
            .reconcile_once(&deployment, &cp)
            .await
            .expect("reconcile unmatched namespace");

        assert!(store
            .get("customers:acme", "Agent", "coding")
            .await
            .expect("get coding")
            .is_none());
        assert!(store
            .get(
                "customers",
                "DeploymentReplica",
                &replica_name("company-builder", "customers:acme"),
            )
            .await
            .expect("get replica")
            .is_none());
        let deployment = store
            .get("customers", "Deployment", "company-builder")
            .await
            .expect("get deployment")
            .expect("deployment exists");
        let status = deployment_status(deployment);
        assert_eq!(status.phase, "Ready");
        let counts = replica_counts(&status);
        assert_eq!(counts.desired, 0);
        assert_eq!(counts.updated, 0);
        assert_eq!(counts.ready, 0);
        assert_eq!(counts.pending, 0);
        assert_eq!(counts.degraded, 0);
    }
}
