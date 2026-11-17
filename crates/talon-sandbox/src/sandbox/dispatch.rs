// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use super::utils::{provider_for_prefix, split_backend_id};
use super::{
    DaytonaSandboxBackend, DockerSandboxBackend, E2bSandboxBackend, ExecSpec, MockSandboxBackend,
    ProcessHandle, ProcessOutput, SandboxBackend, SandboxClassSpecJson, SandboxHandle,
    SandboxPolicySpecJson,
};

#[derive(Clone, Default)]
pub struct DispatchingSandboxBackend {
    docker: DockerSandboxBackend,
}

#[async_trait]
impl SandboxBackend for DispatchingSandboxBackend {
    async fn create(
        &self,
        class: &SandboxClassSpecJson,
        policy: &SandboxPolicySpecJson,
    ) -> Result<SandboxHandle> {
        let provider = class.provider.trim().to_ascii_lowercase();
        let handle = match provider.as_str() {
            "" | "mock" => MockSandboxBackend.create(class, policy).await?,
            "docker" | "local-docker" => self.docker.create(class, policy).await?,
            "daytona" => DaytonaSandboxBackend.create(class, policy).await?,
            "e2b" => E2bSandboxBackend.create(class, policy).await?,
            other => return Err(anyhow!("unsupported SandboxClass provider '{}'", other)),
        };
        Ok(SandboxHandle {
            backend_id: format!("{}:{}", provider_for_prefix(&provider), handle.backend_id),
        })
    }

    async fn destroy(&self, backend_id: &str) -> Result<()> {
        let (provider, id) = split_backend_id(backend_id);
        match provider {
            "mock" => MockSandboxBackend.destroy(id).await,
            "docker" => self.docker.destroy(id).await,
            "daytona" => DaytonaSandboxBackend.destroy(id).await,
            "e2b" => E2bSandboxBackend.destroy(id).await,
            other => Err(anyhow!("unsupported sandbox backend provider '{}'", other)),
        }
    }

    async fn exec(&self, backend_id: &str, spec: ExecSpec) -> Result<ProcessHandle> {
        let (provider, id) = split_backend_id(backend_id);
        match provider {
            "mock" => MockSandboxBackend.exec(id, spec).await,
            "docker" => self.docker.exec(id, spec).await,
            "daytona" => DaytonaSandboxBackend.exec(id, spec).await,
            "e2b" => E2bSandboxBackend.exec(id, spec).await,
            other => Err(anyhow!("unsupported sandbox backend provider '{}'", other)),
        }
    }

    async fn spawn_process(&self, backend_id: &str, spec: ExecSpec) -> Result<ProcessHandle> {
        let (provider, id) = split_backend_id(backend_id);
        match provider {
            "mock" => MockSandboxBackend.spawn_process(id, spec).await,
            "docker" => self.docker.spawn_process(id, spec).await,
            "daytona" => DaytonaSandboxBackend.spawn_process(id, spec).await,
            "e2b" => E2bSandboxBackend.spawn_process(id, spec).await,
            other => Err(anyhow!("unsupported sandbox backend provider '{}'", other)),
        }
    }

    async fn write_stdin(&self, backend_id: &str, process_id: &str, input: &[u8]) -> Result<()> {
        let (provider, id) = split_backend_id(backend_id);
        match provider {
            "mock" => MockSandboxBackend.write_stdin(id, process_id, input).await,
            "docker" => self.docker.write_stdin(id, process_id, input).await,
            "daytona" => {
                DaytonaSandboxBackend
                    .write_stdin(id, process_id, input)
                    .await
            }
            "e2b" => E2bSandboxBackend.write_stdin(id, process_id, input).await,
            other => Err(anyhow!("unsupported sandbox backend provider '{}'", other)),
        }
    }

    async fn read_process_output(
        &self,
        backend_id: &str,
        process_id: &str,
    ) -> Result<ProcessOutput> {
        let (provider, id) = split_backend_id(backend_id);
        match provider {
            "mock" => MockSandboxBackend.read_process_output(id, process_id).await,
            "docker" => self.docker.read_process_output(id, process_id).await,
            "daytona" => {
                DaytonaSandboxBackend
                    .read_process_output(id, process_id)
                    .await
            }
            "e2b" => E2bSandboxBackend.read_process_output(id, process_id).await,
            other => Err(anyhow!("unsupported sandbox backend provider '{}'", other)),
        }
    }

    async fn kill_process(&self, backend_id: &str, process_id: &str) -> Result<()> {
        let (provider, id) = split_backend_id(backend_id);
        match provider {
            "mock" => MockSandboxBackend.kill_process(id, process_id).await,
            "docker" => self.docker.kill_process(id, process_id).await,
            "daytona" => DaytonaSandboxBackend.kill_process(id, process_id).await,
            "e2b" => E2bSandboxBackend.kill_process(id, process_id).await,
            other => Err(anyhow!("unsupported sandbox backend provider '{}'", other)),
        }
    }

    async fn read_file(&self, backend_id: &str, path: &str) -> Result<Vec<u8>> {
        let (provider, id) = split_backend_id(backend_id);
        match provider {
            "mock" => MockSandboxBackend.read_file(id, path).await,
            "docker" => self.docker.read_file(id, path).await,
            "daytona" => DaytonaSandboxBackend.read_file(id, path).await,
            "e2b" => E2bSandboxBackend.read_file(id, path).await,
            other => Err(anyhow!("unsupported sandbox backend provider '{}'", other)),
        }
    }

    async fn write_file(&self, backend_id: &str, path: &str, content: &[u8]) -> Result<()> {
        let (provider, id) = split_backend_id(backend_id);
        match provider {
            "mock" => MockSandboxBackend.write_file(id, path, content).await,
            "docker" => self.docker.write_file(id, path, content).await,
            "daytona" => DaytonaSandboxBackend.write_file(id, path, content).await,
            "e2b" => E2bSandboxBackend.write_file(id, path, content).await,
            other => Err(anyhow!("unsupported sandbox backend provider '{}'", other)),
        }
    }
}
