// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::sync::Arc;

use anyhow::Result;

use crate::control::config::Config;
use crate::control::events::ResourceChangedEvent;
use crate::control::resource_model::{NamespaceResourceExt, TypedResource};
use crate::control::resources::ResourceStore;
use crate::control::{keys, ControlPlane, ProtoKeyValueStoreExt};
use crate::gateway::rpc::resources_proto;

#[derive(Clone)]
pub struct ControllerHost {
    cp: Arc<ControlPlane>,
    config: Arc<Config>,
}

impl ControllerHost {
    pub fn new(cp: Arc<ControlPlane>, config: Arc<Config>) -> Self {
        Self { cp, config }
    }

    pub async fn handle_resource_changed(&self, event: ResourceChangedEvent) -> Result<()> {
        if !controller_enabled(
            self.config.as_ref(),
            controller_for_kind(&event.resource_kind),
        ) {
            return Ok(());
        }

        match event.resource_kind.as_str() {
            "Deployment" | "Template" | "Namespace" => {
                tracing::info!(
                    namespace = %event.namespace,
                    kind = %event.resource_kind,
                    name = %event.name,
                    "Deployment controller observed resource change"
                );
                let store = ResourceStore::new(self.cp.kv.clone(), self.cp.pubsub.clone());
                match event.resource_kind.as_str() {
                    "Deployment" => {
                        if let Some(deployment) = store
                            .get(&event.namespace, "Deployment", &event.name)
                            .await?
                        {
                            self.reconcile_deployment(&store, &deployment).await?;
                        }
                    }
                    "Template" => {
                        self.reconcile_deployments_for_template(&store, &event)
                            .await?;
                    }
                    "Namespace" => {
                        self.reconcile_deployments_for_namespace(&store, &event)
                            .await?;
                    }
                    _ => {}
                }
            }
            "Sandbox" | "SandboxPolicy" | "SandboxClass" => {
                tracing::info!(
                    namespace = %event.namespace,
                    kind = %event.resource_kind,
                    name = %event.name,
                    "Sandbox controller observed resource change"
                );
            }
            "ConnectorClass" | "Connector" => {
                if is_status_only_update(&event) {
                    return Ok(());
                }
                tracing::info!(
                    namespace = %event.namespace,
                    kind = %event.resource_kind,
                    name = %event.name,
                    "Connector controller observed resource change"
                );
                let store = ResourceStore::new(self.cp.kv.clone(), self.cp.pubsub.clone());
                let controller =
                    crate::worker::controllers::connectors::ConnectorController::new(store.clone());
                match event.resource_kind.as_str() {
                    "ConnectorClass" => {
                        if let Some(class) = store
                            .get(&event.namespace, "ConnectorClass", &event.name)
                            .await?
                        {
                            if let Err(err) = controller
                                .reconcile_class(&class, self.cp.as_ref(), self.config.as_ref())
                                .await
                            {
                                tracing::warn!(error = %err, name = %event.name, "ConnectorClass reconcile failed");
                                controller
                                    .reconcile_class_error(
                                        &class,
                                        self.cp.as_ref(),
                                        err.to_string(),
                                    )
                                    .await?;
                            }
                        } else if event.change_type
                            == crate::control::events::ResourceChangeType::Deleted as i32
                        {
                            crate::worker::controllers::connectors::delete_registration_entries_for_class(
                                self.cp.kv.as_ref(),
                                &event.namespace,
                                &event.name,
                            )
                            .await?;
                        }
                    }
                    "Connector" => {
                        if let Some(connector) = store
                            .get(&event.namespace, "Connector", &event.name)
                            .await?
                        {
                            if let Err(err) = controller
                                .reconcile_connector(&connector, self.cp.as_ref())
                                .await
                            {
                                tracing::warn!(
                                    error = %err,
                                    namespace = %event.namespace,
                                    name = %event.name,
                                    "Connector reconcile failed"
                                );
                                controller
                                    .reconcile_connector_error(
                                        &connector,
                                        self.cp.as_ref(),
                                        err.to_string(),
                                    )
                                    .await?;
                            }
                        } else if event.change_type
                            == crate::control::events::ResourceChangeType::Deleted as i32
                        {
                            crate::worker::controllers::connectors::delete_match_entries_for_uid(
                                self.cp.kv.as_ref(),
                                &event.uid,
                            )
                            .await?;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn reconcile_deployment(
        &self,
        store: &ResourceStore,
        deployment: &resources_proto::Resource,
    ) -> Result<()> {
        let controller =
            crate::worker::controllers::deployment::DeploymentController::new(store.clone());
        controller
            .reconcile_once(deployment, self.cp.as_ref())
            .await
    }

    async fn reconcile_deployments_for_template(
        &self,
        store: &ResourceStore,
        event: &ResourceChangedEvent,
    ) -> Result<()> {
        for deployment in store.list(&event.namespace, Some("Deployment")).await? {
            if deployment_uses_template(&deployment, &event.name) {
                self.reconcile_deployment(store, &deployment).await?;
            }
        }
        Ok(())
    }

    async fn reconcile_deployments_for_namespace(
        &self,
        store: &ResourceStore,
        event: &ResourceChangedEvent,
    ) -> Result<()> {
        let Some(namespace) = self
            .cp
            .kv
            .get_msg::<resources_proto::Namespace>(&keys::namespace_metadata(&event.name))
            .await?
        else {
            return Ok(());
        };
        if namespace.parent().is_empty() {
            return Ok(());
        }

        for deployment in store.list(namespace.parent(), Some("Deployment")).await? {
            if deployment_matches_namespace(&deployment, &namespace)
                || deployment_has_replica_for_namespace(store, &deployment, namespace.name())
                    .await?
            {
                self.reconcile_deployment(store, &deployment).await?;
            }
        }
        Ok(())
    }
}

fn is_status_only_update(event: &ResourceChangedEvent) -> bool {
    event.change_type == crate::control::events::ResourceChangeType::Updated as i32
        && !event.changed_sections.is_empty()
        && event
            .changed_sections
            .iter()
            .all(|section| section == "status")
}

fn deployment_spec(
    deployment: &resources_proto::Resource,
) -> Option<&resources_proto::DeploymentSpec> {
    match deployment.spec.as_ref()?.kind.as_ref()? {
        resources_proto::resource_spec::Kind::Deployment(spec) => Some(spec),
        _ => None,
    }
}

fn deployment_uses_template(deployment: &resources_proto::Resource, template: &str) -> bool {
    deployment_spec(deployment)
        .map(|spec| spec.templates.iter().any(|name| name == template))
        .unwrap_or(false)
}

fn deployment_matches_namespace(
    deployment: &resources_proto::Resource,
    namespace: &resources_proto::Namespace,
) -> bool {
    let Some(selector) = deployment_spec(deployment)
        .and_then(|spec| spec.placement.as_ref())
        .and_then(|placement| placement.namespace_selector.as_ref())
    else {
        return false;
    };
    selector.parent == namespace.parent()
        && selector
            .match_labels
            .iter()
            .all(|(key, value)| namespace.labels().get(key) == Some(value))
}

async fn deployment_has_replica_for_namespace(
    store: &ResourceStore,
    deployment: &resources_proto::Resource,
    target_namespace: &str,
) -> Result<bool> {
    let Some(meta) = deployment.metadata.as_ref() else {
        return Ok(false);
    };
    let replica = store
        .get(
            &meta.namespace,
            "DeploymentReplica",
            &crate::worker::controllers::deployment::replica_name(&meta.name, target_namespace),
        )
        .await?
        .is_some();
    Ok(replica)
}

fn controller_for_kind(kind: &str) -> &'static str {
    match kind {
        "Deployment" | "Template" | "Namespace" => "deployment",
        "Sandbox" | "SandboxPolicy" | "SandboxClass" => "sandbox",
        "ConnectorClass" | "Connector" => "connectors",
        _ => "",
    }
}

fn controller_enabled(config: &Config, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    config
        .controllers
        .get(name)
        .map(|controller| controller.enabled)
        .unwrap_or(true)
}
