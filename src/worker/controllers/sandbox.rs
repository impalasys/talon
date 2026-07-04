// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::ns;
use crate::control::resources::ResourceStore;
use crate::gateway::rpc::resources_proto;
use crate::harness::sandbox::{
    ResourceRefJson, SandboxBackend, SandboxClassSpecJson, SandboxPolicySpecJson,
    SandboxPolicyTemplateJson, SandboxQuotaJson,
};
use anyhow::{anyhow, Result};

pub struct SandboxController<B> {
    store: ResourceStore,
    backend: B,
}

#[derive(Debug, Clone)]
pub struct LeasedSandbox {
    pub sandbox: resources_proto::Resource,
    pub token: String,
}

pub struct SandboxLeaseService<B> {
    store: ResourceStore,
    backend: B,
    lease_ttl_seconds: i64,
}

impl<B: SandboxBackend> SandboxLeaseService<B> {
    pub fn new(store: ResourceStore, backend: B) -> Self {
        Self {
            store,
            backend,
            lease_ttl_seconds: 60,
        }
    }

    pub async fn acquire(
        &self,
        namespace: &str,
        agent: &str,
        session_id: &str,
        policy_name: &str,
    ) -> Result<LeasedSandbox> {
        let policy = self.resolve_policy(namespace, policy_name).await?;
        let policy_spec = typed_sandbox_policy(&policy)?;
        let class_ref = policy_spec
            .class_ref
            .as_ref()
            .ok_or_else(|| anyhow!("SandboxPolicy '{}' missing classRef", policy_name))?;
        let class_namespace = if class_ref.namespace.trim().is_empty() {
            ns::TALON_SYSTEM
        } else {
            class_ref.namespace.trim()
        };
        let class = self
            .store
            .get(class_namespace, "SandboxClass", &class_ref.name)
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "SandboxClass '{}:{}' not found",
                    class_namespace,
                    class_ref.name
                )
            })?;
        self.enforce_quota(namespace, policy_name, policy_spec.max_concurrent)
            .await?;

        if let Some(sandbox) = self.find_available_sandbox(namespace, policy_name).await? {
            return self.lease_existing(sandbox, agent, session_id).await;
        }

        let controller = SandboxController::new(self.store.clone(), &self.backend);
        let sandbox = controller
            .create_from_policy(namespace, &policy, &class)
            .await?;
        self.lease_existing(sandbox, agent, session_id).await
    }

    pub async fn renew(
        &self,
        sandbox: &resources_proto::Resource,
        token: &str,
    ) -> Result<resources_proto::Resource> {
        self.update_lease(sandbox, token, |lease, now, ttl| {
            if lease.token != token {
                return Err(anyhow!("sandbox lease token mismatch"));
            }
            lease.heartbeat_at = now;
            lease.expires_at = now + ttl * 1_000_000;
            Ok(())
        })
        .await
    }

    pub async fn release(
        &self,
        sandbox: &resources_proto::Resource,
        token: &str,
    ) -> Result<resources_proto::Resource> {
        self.update_sandbox_status(sandbox, |status| {
            if status
                .lease
                .as_ref()
                .map(|lease| lease.token.as_str() != token)
                .unwrap_or(true)
            {
                return Err(anyhow!("sandbox lease token mismatch"));
            }
            status.lease = None;
            Ok(())
        })
        .await
    }

    async fn resolve_policy(
        &self,
        namespace: &str,
        policy_name: &str,
    ) -> Result<resources_proto::Resource> {
        for candidate in crate::control::ns::ancestry(namespace) {
            if let Some(policy) = self
                .store
                .get(&candidate, "SandboxPolicy", policy_name)
                .await?
            {
                return Ok(policy);
            }
        }
        Err(anyhow!(
            "SandboxPolicy '{}' not found in namespace ancestry for '{}'",
            policy_name,
            namespace
        ))
    }

    async fn enforce_quota(
        &self,
        namespace: &str,
        policy_name: &str,
        max_concurrent: u32,
    ) -> Result<()> {
        if max_concurrent == 0 {
            return Ok(());
        }
        let now = chrono::Utc::now().timestamp_micros();
        let active = self
            .store
            .list(namespace, Some("Sandbox"))
            .await?
            .into_iter()
            .filter(|sandbox| sandbox_for_policy(sandbox, policy_name))
            .filter(|sandbox| sandbox_active_lease(sandbox, now))
            .count();
        if active as u32 >= max_concurrent {
            return Err(anyhow!(
                "SandboxPolicy '{}' quota exceeded: {}/{} active",
                policy_name,
                active,
                max_concurrent
            ));
        }
        Ok(())
    }

    async fn find_available_sandbox(
        &self,
        namespace: &str,
        policy_name: &str,
    ) -> Result<Option<resources_proto::Resource>> {
        let now = chrono::Utc::now().timestamp_micros();
        Ok(self
            .store
            .list(namespace, Some("Sandbox"))
            .await?
            .into_iter()
            .find(|sandbox| {
                sandbox_for_policy(sandbox, policy_name) && !sandbox_active_lease(sandbox, now)
            }))
    }

    async fn lease_existing(
        &self,
        sandbox: resources_proto::Resource,
        agent: &str,
        session_id: &str,
    ) -> Result<LeasedSandbox> {
        let token = crate::control::uuid::v7();
        let leased = self
            .update_sandbox_status(&sandbox, |status| {
                let now = chrono::Utc::now().timestamp_micros();
                if status
                    .lease
                    .as_ref()
                    .map(|lease| lease.expires_at > now)
                    .unwrap_or(false)
                {
                    return Err(anyhow!("sandbox is already leased"));
                }
                status.lease = Some(resources_proto::SandboxLease {
                    owner_kind: "AgentSession".to_string(),
                    owner_agent: agent.to_string(),
                    owner_session_id: session_id.to_string(),
                    token: token.clone(),
                    acquired_at: now,
                    expires_at: now + self.lease_ttl_seconds * 1_000_000,
                    heartbeat_at: now,
                });
                Ok(())
            })
            .await?;
        Ok(LeasedSandbox {
            sandbox: leased,
            token,
        })
    }

    async fn update_lease<F>(
        &self,
        sandbox: &resources_proto::Resource,
        token: &str,
        mut update: F,
    ) -> Result<resources_proto::Resource>
    where
        F: FnMut(&mut resources_proto::SandboxLease, i64, i64) -> Result<()>,
    {
        self.update_sandbox_status(sandbox, |status| {
            let now = chrono::Utc::now().timestamp_micros();
            let lease = status
                .lease
                .as_mut()
                .ok_or_else(|| anyhow!("sandbox is not leased"))?;
            if lease.token != token {
                return Err(anyhow!("sandbox lease token mismatch"));
            }
            update(lease, now, self.lease_ttl_seconds)
        })
        .await
    }

    async fn update_sandbox_status<F>(
        &self,
        sandbox: &resources_proto::Resource,
        mut update: F,
    ) -> Result<resources_proto::Resource>
    where
        F: FnMut(&mut resources_proto::SandboxStatus) -> Result<()>,
    {
        let meta = sandbox
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("Sandbox metadata is required"))?;
        for _ in 0..8 {
            let mut resource = self
                .store
                .get(&meta.namespace, "Sandbox", &meta.name)
                .await?
                .ok_or_else(|| anyhow!("Sandbox '{}' not found", meta.name))?;
            let expected_resource_version = resource
                .metadata
                .as_ref()
                .map(|meta| meta.resource_version.clone())
                .ok_or_else(|| anyhow!("Sandbox metadata missing"))?;
            let Some(resources_proto::resource_status::Kind::Sandbox(status)) = resource
                .status
                .as_mut()
                .and_then(|status| status.kind.as_mut())
            else {
                return Err(anyhow!("Sandbox '{}' missing typed status", meta.name));
            };
            update(status)?;
            if let Some(status) = resource.status {
                match self
                    .store
                    .patch_status(
                        &meta.namespace,
                        "Sandbox",
                        &meta.name,
                        Some(&expected_resource_version),
                        status,
                    )
                    .await
                {
                    Ok(resource) => return Ok(resource),
                    Err(err)
                        if err
                            .to_string()
                            .contains("resourceVersion conflict for Sandbox") =>
                    {
                        continue;
                    }
                    Err(err) => return Err(err),
                }
            }
        }
        Err(anyhow!(
            "failed to update Sandbox '{}' lease by CAS",
            meta.name
        ))
    }
}

impl<B: SandboxBackend> SandboxController<B> {
    pub fn new(store: ResourceStore, backend: B) -> Self {
        Self { store, backend }
    }

    pub async fn reconcile_sandbox(
        &self,
        sandbox: &resources_proto::Resource,
    ) -> Result<resources_proto::Resource> {
        if sandbox.kind != "Sandbox" {
            return Err(anyhow!("expected Sandbox, got {}", sandbox.kind));
        }
        Ok(sandbox.clone())
    }

    pub async fn create_from_policy(
        &self,
        namespace: &str,
        policy: &resources_proto::Resource,
        class: &resources_proto::Resource,
    ) -> Result<resources_proto::Resource> {
        if policy.kind != "SandboxPolicy" {
            return Err(anyhow!("expected SandboxPolicy, got {}", policy.kind));
        }
        if class.kind != "SandboxClass" {
            return Err(anyhow!("expected SandboxClass, got {}", class.kind));
        }
        let Some(resources_proto::resource_spec::Kind::SandboxPolicy(policy_spec_proto)) =
            policy.spec.as_ref().and_then(|spec| spec.kind.as_ref())
        else {
            return Err(anyhow!(
                "SandboxPolicy resource is missing typed SandboxPolicy spec"
            ));
        };
        let Some(resources_proto::resource_spec::Kind::SandboxClass(class_spec_proto)) =
            class.spec.as_ref().and_then(|spec| spec.kind.as_ref())
        else {
            return Err(anyhow!(
                "SandboxClass resource is missing typed SandboxClass spec"
            ));
        };
        let policy_spec = sandbox_policy_json_from_proto(policy_spec_proto)?;
        let class_spec = sandbox_class_json_from_proto(class_spec_proto)?;
        let handle = self.backend.create(&class_spec, &policy_spec).await?;
        let policy_meta = policy
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("SandboxPolicy metadata is required"))?;
        let name = crate::control::uuid::unique_name(&policy_meta.name);
        let sandbox = resources_proto::Resource {
            api_version: policy.api_version.clone(),
            kind: "Sandbox".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: name.clone(),
                namespace: namespace.to_string(),
                labels: std::collections::HashMap::from([(
                    "talon.impalasys.com/sandbox-policy".to_string(),
                    policy_meta.name.clone(),
                )]),
                annotations: std::collections::HashMap::new(),
                owner_references: vec![resources_proto::OwnerReference {
                    api_version: policy.api_version.clone(),
                    kind: "SandboxPolicy".to_string(),
                    namespace: policy_meta.namespace.clone(),
                    name: policy_meta.name.clone(),
                    uid: policy_meta.uid.clone(),
                    controller: true,
                    block_owner_deletion: true,
                }],
                finalizers: vec!["talon.impalasys.com/sandbox-cleanup".to_string()],
                generation: 0,
                resource_version: String::new(),
                uid: String::new(),
                deletion_timestamp: None,
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::Sandbox(
                    resources_proto::SandboxSpec {
                        policy_ref: policy_meta.name.clone(),
                        class_ref: policy_spec_proto.class_ref.clone(),
                        runtime_template: policy_spec_proto.template.clone(),
                    },
                )),
            }),
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resources_proto::resource_status::Kind::Sandbox(
                    resources_proto::SandboxStatus {
                        observed_generation: 0,
                        phase: "Ready".to_string(),
                        conditions: Vec::new(),
                        backend_id: handle.backend_id,
                        lease: None,
                        processes: Vec::new(),
                    },
                )),
            }),
        };
        self.store.upsert(namespace, sandbox).await
    }
}

fn sandbox_class_json_from_proto(
    spec: &resources_proto::SandboxClassSpec,
) -> Result<SandboxClassSpecJson> {
    Ok(SandboxClassSpecJson {
        provider: spec.provider.clone(),
        provider_config: serde_json::from_str(&spec.provider_config_json)
            .unwrap_or_else(|_| serde_json::json!({})),
        credentials: serde_json::from_str(&spec.credentials_json)
            .unwrap_or_else(|_| serde_json::json!({})),
    })
}

fn typed_sandbox_policy(
    resource: &resources_proto::Resource,
) -> Result<&resources_proto::SandboxPolicySpec> {
    let Some(resources_proto::resource_spec::Kind::SandboxPolicy(spec)) =
        resource.spec.as_ref().and_then(|spec| spec.kind.as_ref())
    else {
        return Err(anyhow!(
            "SandboxPolicy resource is missing typed SandboxPolicy spec"
        ));
    };
    Ok(spec)
}

fn sandbox_for_policy(sandbox: &resources_proto::Resource, policy_name: &str) -> bool {
    sandbox
        .metadata
        .as_ref()
        .and_then(|meta| meta.labels.get("talon.impalasys.com/sandbox-policy"))
        .map(|value| value == policy_name)
        .unwrap_or(false)
}

fn sandbox_active_lease(sandbox: &resources_proto::Resource, now_micros: i64) -> bool {
    let Some(resources_proto::resource_status::Kind::Sandbox(status)) = sandbox
        .status
        .as_ref()
        .and_then(|status| status.kind.as_ref())
    else {
        return false;
    };
    status
        .lease
        .as_ref()
        .map(|lease| lease.expires_at > now_micros)
        .unwrap_or(false)
}

fn sandbox_policy_json_from_proto(
    spec: &resources_proto::SandboxPolicySpec,
) -> Result<SandboxPolicySpecJson> {
    Ok(SandboxPolicySpecJson {
        class_ref: ResourceRefJson {
            namespace: spec
                .class_ref
                .as_ref()
                .map(|class_ref| class_ref.namespace.clone())
                .unwrap_or_default(),
            name: spec
                .class_ref
                .as_ref()
                .map(|class_ref| class_ref.name.clone())
                .unwrap_or_default(),
        },
        template: SandboxPolicyTemplateJson {
            spec: sandbox_runtime_template_value(spec.template.as_ref()),
        },
        quota: SandboxQuotaJson {
            max_concurrent: spec.max_concurrent,
        },
    })
}

fn sandbox_runtime_template_value(
    template: Option<&resources_proto::SandboxRuntimeTemplateSpec>,
) -> serde_json::Value {
    let Some(template) = template else {
        return serde_json::json!({});
    };
    serde_json::json!({
        "image": template.image,
        "workspace": template.workspace.as_ref().map(|workspace| serde_json::json!({
            "mode": workspace.mode,
            "mountPath": workspace.mount_path,
        })),
        "setup": template.setup.as_ref().map(|setup| serde_json::json!({
            "packages": setup.packages,
            "commands": setup.commands,
        })),
        "network": template.network.as_ref().map(|network| serde_json::json!({
            "mode": network.mode,
        })),
        "filesystem": template.filesystem.as_ref().map(|filesystem| serde_json::json!({
            "writable": filesystem.writable,
            "readonly": filesystem.readonly,
        })),
        "leasePolicy": template.lease_policy.as_ref().map(|lease_policy| serde_json::json!({
            "mode": lease_policy.mode,
        })),
    })
}

#[cfg(test)]
mod tests {
    use super::{SandboxController, SandboxLeaseService};
    use crate::control::resources::ResourceStore;
    use crate::gateway::rpc::resources_proto;
    use crate::harness::sandbox::{DockerSandboxBackend, MockSandboxBackend, SandboxBackend};
    use crate::test_support::{docker_test_guard, MockKvStore, RecordingPubSub};
    use anyhow::Result;
    use std::sync::Arc;

    fn resource(
        kind: &str,
        namespace: &str,
        name: &str,
        spec: serde_json::Value,
    ) -> resources_proto::Resource {
        resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: kind.to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: name.to_string(),
                namespace: namespace.to_string(),
                labels: Default::default(),
                annotations: Default::default(),
                owner_references: Vec::new(),
                finalizers: Vec::new(),
                generation: 1,
                resource_version: "rv".to_string(),
                uid: format!("uid-{name}"),
                deletion_timestamp: None,
            }),
            spec: Some(
                crate::control::manifest::resource_spec_status_from_json(
                    kind,
                    &spec.to_string(),
                    "{}",
                )
                .unwrap()
                .0,
            ),
            status: Some(
                crate::control::manifest::resource_spec_status_from_json(
                    kind,
                    &spec.to_string(),
                    "{}",
                )
                .unwrap()
                .1,
            ),
        }
    }

    #[tokio::test]
    async fn sandbox_controller_creates_sandbox_from_policy_and_class() {
        let store = ResourceStore::new(
            Arc::new(MockKvStore::new()),
            Arc::new(RecordingPubSub::default()),
        );
        let controller = SandboxController::new(store, MockSandboxBackend);
        let class = resource(
            "SandboxClass",
            "system",
            "e2b-code",
            serde_json::json!({
                "provider": "e2b",
                "providerConfig": { "templateId": "node-22-rust" },
                "credentials": {}
            }),
        );
        let policy = resource(
            "SandboxPolicy",
            "customers",
            "coding",
            serde_json::json!({
                "classRef": { "namespace": "system", "name": "e2b-code" },
                "template": { "spec": { "image": "node-22-rust" } },
                "quota": { "maxConcurrent": 5 }
            }),
        );

        let sandbox = controller
            .create_from_policy("customers:acme", &policy, &class)
            .await
            .unwrap();
        assert_eq!(sandbox.kind, "Sandbox");
        assert_eq!(
            sandbox.metadata.as_ref().unwrap().namespace,
            "customers:acme"
        );
        let Some(resources_proto::resource_status::Kind::Sandbox(status)) = sandbox
            .status
            .as_ref()
            .and_then(|status| status.kind.as_ref())
        else {
            panic!("sandbox status should be typed");
        };
        assert!(status.backend_id.contains("mock-e2b"));
    }

    #[tokio::test]
    async fn sandbox_lease_service_spins_up_docker_sandbox_when_enabled() {
        if std::env::var("TALON_DOCKER_SANDBOX_TEST").ok().as_deref() != Some("1") {
            return;
        }
        let _guard = docker_test_guard();
        let kv = Arc::new(MockKvStore::new());
        let store = ResourceStore::new(kv.clone(), Arc::new(RecordingPubSub::default()));
        store
            .upsert(
                "system",
                resource(
                    "SandboxClass",
                    "system",
                    "docker-code",
                    serde_json::json!({
                        "provider": "docker",
                        "providerConfig": {
                            "image": "alpine:3.20",
                            "setupTimeoutSeconds": 30
                        },
                        "credentials": {}
                    }),
                ),
            )
            .await
            .unwrap();
        store
            .upsert(
                "customers",
                resource(
                    "SandboxPolicy",
                    "customers",
                    "coding",
                    serde_json::json!({
                        "classRef": { "namespace": "system", "name": "docker-code" },
                        "template": {
                            "spec": {
                                "workspace": { "mountPath": "/workspace" },
                                "network": { "mode": "restricted" },
                                "filesystem": { "writable": ["/workspace", "/tmp"] }
                            }
                        },
                        "quota": { "maxConcurrent": 1 }
                    }),
                ),
            )
            .await
            .unwrap();

        let backend = DockerSandboxBackend::default();
        let lease_service = SandboxLeaseService::new(store.clone(), backend.clone());
        let lease = lease_service
            .acquire("customers:acme", "coding", "session-1", "coding")
            .await
            .unwrap();
        let backend_id = sandbox_backend_id(&lease.sandbox);
        let test_result: Result<()> = async {
            assert!(!backend_id.is_empty());
            assert_eq!(
                store
                    .list("customers:acme", Some("Sandbox"))
                    .await
                    .unwrap()
                    .len(),
                1
            );
            let lease_status = sandbox_status(&lease.sandbox);
            assert_eq!(
                lease_status.lease.as_ref().unwrap().owner_session_id,
                "session-1"
            );
            let process = backend
                .exec(
                    &backend_id,
                    crate::harness::sandbox::ExecSpec {
                        command: "sh".to_string(),
                        args: vec!["-lc".to_string(), "printf talon-sandbox".to_string()],
                        cwd: "/workspace".to_string(),
                        env: Default::default(),
                    },
                )
                .await
                .unwrap();
            let output = backend
                .read_process_output(&backend_id, &process.id)
                .await?;
            assert_eq!(output.exit_code, Some(0));
            assert_eq!(output.stdout, "talon-sandbox");
            lease_service.release(&lease.sandbox, &lease.token).await?;
            Ok(())
        }
        .await;
        let destroy_result = backend.destroy(&backend_id).await;
        test_result.unwrap();
        destroy_result.unwrap();
    }

    fn sandbox_status(resource: &resources_proto::Resource) -> &resources_proto::SandboxStatus {
        let Some(resources_proto::resource_status::Kind::Sandbox(status)) = resource
            .status
            .as_ref()
            .and_then(|status| status.kind.as_ref())
        else {
            panic!("sandbox status should be typed");
        };
        status
    }

    fn sandbox_backend_id(resource: &resources_proto::Resource) -> String {
        sandbox_status(resource).backend_id.clone()
    }
}
