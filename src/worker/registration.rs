// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{ns, ControlPlane};
use crate::gateway::rpc::resources_proto;
use anyhow::{Context, Result};
use std::sync::{Arc, OnceLock};
use tokio_util::sync::CancellationToken;

pub const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);
pub const HEARTBEAT_TTL: chrono::Duration = chrono::Duration::seconds(30);

static GENERATED_WORKER_ID: OnceLock<String> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct WorkerRegistration {
    pub worker_id: String,
    pub started_at: i64,
    pub version: String,
}

impl WorkerRegistration {
    pub fn new(worker_id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            worker_id: worker_id.into(),
            started_at: chrono::Utc::now().timestamp_micros(),
            version: version.into(),
        }
    }
}

pub fn worker_id() -> String {
    GENERATED_WORKER_ID
        .get_or_init(|| uuid::Uuid::now_v7().to_string())
        .clone()
}

pub async fn upsert_worker(cp: &ControlPlane, registration: &WorkerRegistration) -> Result<()> {
    let store = crate::control::resources::ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    store
        .upsert_manifest(
            ns::TALON_SYSTEM,
            resources_proto::ResourceManifest {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "Worker".to_string(),
                metadata: Some(resources_proto::ResourceMeta {
                    name: registration.worker_id.clone(),
                    namespace: ns::TALON_SYSTEM.to_string(),
                    ..Default::default()
                }),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(resources_proto::resource_spec::Kind::Worker(
                        resources_proto::WorkerSpec {},
                    )),
                }),
            },
        )
        .await
        .with_context(|| format!("failed to upsert Worker '{}'", registration.worker_id))?;
    tracing::info!(worker_id = %registration.worker_id, "Worker registered");
    Ok(())
}

pub async fn patch_worker_status(
    cp: &ControlPlane,
    registration: &WorkerRegistration,
    phase: &str,
) -> Result<()> {
    let store = crate::control::resources::ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    store
        .patch_status(
            ns::TALON_SYSTEM,
            "Worker",
            &registration.worker_id,
            None,
            resources_proto::ResourceStatus {
                kind: Some(resources_proto::resource_status::Kind::Worker(
                    worker_status(registration, phase),
                )),
            },
        )
        .await
        .with_context(|| format!("failed to patch Worker '{}' status", registration.worker_id))?;
    Ok(())
}

pub fn worker_status(
    registration: &WorkerRegistration,
    phase: &str,
) -> resources_proto::WorkerStatus {
    let now = chrono::Utc::now();
    resources_proto::WorkerStatus {
        observed_generation: 0,
        phase: phase.to_string(),
        conditions: Vec::new(),
        started_at: registration.started_at,
        heartbeat_at: now.timestamp_micros(),
        expires_at: (now + HEARTBEAT_TTL).timestamp_micros(),
        version: registration.version.clone(),
        endpoints: Vec::new(),
    }
}

pub fn worker_is_live(status: &resources_proto::WorkerStatus, now_micros: i64) -> bool {
    status.expires_at > now_micros
}

pub fn worker_is_stale(status: &resources_proto::WorkerStatus, now_micros: i64) -> bool {
    !worker_is_live(status, now_micros)
}

pub async fn run_worker_heartbeat(
    cp: Arc<ControlPlane>,
    registration: WorkerRegistration,
    shutdown_token: CancellationToken,
) {
    register_and_patch_ready(cp.as_ref(), &registration).await;

    loop {
        tokio::select! {
            _ = shutdown_token.cancelled() => break,
            _ = tokio::time::sleep(HEARTBEAT_INTERVAL) => {
                patch_ready_with_registration_retry(cp.as_ref(), &registration).await;
            }
        }
    }

    if let Err(err) = patch_worker_status(cp.as_ref(), &registration, "draining").await {
        tracing::warn!(worker_id = %registration.worker_id, error = %err, "Worker draining status update failed");
    }
}

async fn register_and_patch_ready(cp: &ControlPlane, registration: &WorkerRegistration) {
    if let Err(err) = upsert_worker(cp, registration).await {
        tracing::warn!(worker_id = %registration.worker_id, error = %err, "Worker registration failed");
        return;
    }

    if let Err(err) = patch_worker_status(cp, registration, "ready").await {
        tracing::warn!(worker_id = %registration.worker_id, error = %err, "Worker heartbeat failed");
    }
}

async fn patch_ready_with_registration_retry(cp: &ControlPlane, registration: &WorkerRegistration) {
    match patch_worker_status(cp, registration, "ready").await {
        Ok(()) => return,
        Err(err) => {
            tracing::warn!(worker_id = %registration.worker_id, error = %err, "Worker heartbeat failed");
        }
    }

    if let Err(err) = upsert_worker(cp, registration).await {
        tracing::warn!(worker_id = %registration.worker_id, error = %err, "Worker registration failed");
        return;
    }

    if let Err(err) = patch_worker_status(cp, registration, "ready").await {
        tracing::warn!(worker_id = %registration.worker_id, error = %err, "Worker heartbeat retry failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{object_store, scheduler::NoopSchedulerBackend};
    use crate::test_support::{EmptyPubSub, MockKvStore};

    fn control_plane() -> ControlPlane {
        ControlPlane {
            kv: Arc::new(MockKvStore::default()),
            pubsub: Arc::new(EmptyPubSub),
            scheduler: Arc::new(NoopSchedulerBackend),
            objects: object_store::default_object_store(),
            documents: crate::control::search::memory_document_store(),
        }
    }

    #[test]
    fn worker_id_generates_stable_process_uuid() {
        let generated = worker_id();
        assert!(uuid::Uuid::parse_str(&generated).is_ok());
        assert_eq!(worker_id(), generated);
    }

    #[tokio::test]
    async fn worker_status_patch_preserves_spec_generation() {
        let cp = control_plane();
        let registration = WorkerRegistration::new("worker-a", "1.2.3");
        upsert_worker(&cp, &registration).await.unwrap();
        patch_worker_status(&cp, &registration, "ready")
            .await
            .unwrap();

        let store = crate::control::resources::ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
        let worker = store
            .get(ns::TALON_SYSTEM, "Worker", "worker-a")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(worker.metadata.as_ref().unwrap().generation, 1);
        assert!(matches!(
            worker.spec.as_ref().and_then(|spec| spec.kind.as_ref()),
            Some(resources_proto::resource_spec::Kind::Worker(_))
        ));
        let Some(resources_proto::resource_status::Kind::Worker(status)) =
            worker.status.and_then(|status| status.kind)
        else {
            panic!("expected Worker status");
        };
        assert_eq!(status.phase, "ready");
        assert!(status.heartbeat_at > 0);
        assert!(status.expires_at > status.heartbeat_at);
        assert_eq!(status.version, "1.2.3");
        assert!(status.endpoints.is_empty());
    }

    #[test]
    fn draining_status_clears_endpoints() {
        let registration = WorkerRegistration::new("worker-a", "1.2.3");
        let ready = worker_status(&registration, "ready");
        assert!(ready.endpoints.is_empty());
        let draining = worker_status(&registration, "draining");
        assert_eq!(draining.phase, "draining");
        assert!(draining.endpoints.is_empty());
    }

    #[tokio::test]
    async fn heartbeat_recreates_missing_worker_record() {
        let cp = control_plane();
        let registration = WorkerRegistration::new("worker-a", "1.2.3");
        patch_ready_with_registration_retry(&cp, &registration).await;

        let store = crate::control::resources::ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
        assert!(store
            .get(ns::TALON_SYSTEM, "Worker", "worker-a")
            .await
            .unwrap()
            .is_some());
    }

    #[test]
    fn worker_liveness_is_based_on_expiry() {
        let mut status = resources_proto::WorkerStatus {
            expires_at: 1_001,
            ..Default::default()
        };
        assert!(worker_is_live(&status, 1_000));
        assert!(!worker_is_stale(&status, 1_000));

        status.expires_at = 1_000;
        assert!(!worker_is_live(&status, 1_000));
        assert!(worker_is_stale(&status, 1_000));
    }
}
