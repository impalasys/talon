// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::resource_model::{NamespaceResourceExt, TypedResource};
use crate::control::resources::ResourceStore;
use crate::control::{keys, ControlPlane, ProtoKeyValueStoreExt};
use crate::gateway::rpc::resources_proto;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

        for target in targets {
            let mut rendered_refs = Vec::new();
            let mut rendered_hashes = std::collections::HashMap::new();
            let mut last_rendered_json = std::collections::HashMap::new();
            for template_name in &spec.templates {
                let template = self
                    .store
                    .get(&deployment_meta.namespace, "Template", template_name)
                    .await?
                    .ok_or_else(|| {
                        anyhow!(
                            "Template '{}' not found in namespace '{}'",
                            template_name,
                            deployment_meta.namespace
                        )
                    })?;
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
                self.apply_rendered(target.name(), rendered).await?;
            }

            let replica_name = format!(
                "{}--{}",
                deployment_meta.name,
                escape_replica_namespace(target.name())
            );
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
                            rendered_resources: rendered_refs,
                            rendered_hashes,
                            conflicts: Vec::new(),
                            last_rendered_json,
                            owned_json_pointers: Vec::new(),
                            phase: "Ready".to_string(),
                        },
                    )),
                }),
            };
            self.store
                .upsert(&deployment_meta.namespace, replica)
                .await?;
        }

        Ok(())
    }

    async fn target_namespaces(
        &self,
        cp: &ControlPlane,
        selector: &resources_proto::NamespaceSelector,
    ) -> Result<Vec<resources_proto::Namespace>> {
        let refs = cp
            .kv
            .list_entries(&keys::namespace_ref_prefix(Some(&selector.parent)))
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
        let env = minijinja::Environment::new();
        let rendered_spec = env.render_str(
            &spec.spec_json,
            minijinja::context! {
                namespace => serde_json::json!({
                    "name": namespace.name(),
                    "parent": namespace.parent(),
                    "customerName": customer_name,
                    "metadata": {
                        "labels": namespace_meta.labels,
                        "annotations": namespace_meta.annotations,
                    }
                }),
                deployment => serde_json::json!({
                    "metadata": {
                        "name": deployment_meta.name,
                        "namespace": deployment_meta.namespace,
                        "labels": deployment_meta.labels,
                        "annotations": deployment_meta.annotations,
                    }
                }),
                template => serde_json::json!({
                    "metadata": {
                        "name": template_meta.name,
                        "namespace": template_meta.namespace,
                        "labels": template_meta.labels,
                        "annotations": template_meta.annotations,
                    }
                }),
            },
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
    ) -> Result<resources_proto::Resource> {
        self.store.upsert(namespace, resource).await
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use std::sync::Arc;

    fn controller() -> DeploymentController {
        DeploymentController::new(ResourceStore::new(
            Arc::new(MockKvStore::default()),
            Arc::new(RecordingPubSub::default()),
        ))
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

    fn rendered_prompt(resource: resources_proto::Resource) -> String {
        let Some(resources_proto::resource_spec::Kind::Agent(spec)) =
            resource.spec.and_then(|spec| spec.kind)
        else {
            panic!("expected rendered Agent");
        };
        spec.system_prompt
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
}
