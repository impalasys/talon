// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::sync::Arc;

use anyhow::Result;

use crate::control::config::Config;
use crate::control::events::ResourceChangedEvent;
use crate::control::resources::ResourceStore;
use crate::control::ControlPlane;

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
                // The deployment controller has typed render/apply primitives now. A full
                // resync worker will fan this out across all affected deployments; for direct
                // Deployment changes reconcile the named deployment immediately.
                if event.resource_kind == "Deployment" {
                    let store = ResourceStore::new(self.cp.kv.clone(), self.cp.pubsub.clone());
                    if let Some(deployment) = store
                        .get(&event.namespace, "Deployment", &event.name)
                        .await?
                    {
                        let controller =
                            crate::worker::controllers::deployment::DeploymentController::new(
                                store.clone(),
                            );
                        controller
                            .reconcile_once(&deployment, self.cp.as_ref())
                            .await?;
                    }
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
            _ => {}
        }
        Ok(())
    }
}

fn controller_for_kind(kind: &str) -> &'static str {
    match kind {
        "Deployment" | "Template" | "Namespace" => "deployment",
        "Sandbox" | "SandboxPolicy" | "SandboxClass" => "sandbox",
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
