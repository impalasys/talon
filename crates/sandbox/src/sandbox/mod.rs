// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

mod daytona;
mod dispatch;
mod docker;
mod e2b;
mod mock;
#[cfg(test)]
mod tests;
mod utils;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub use daytona::DaytonaSandboxBackend;
pub use dispatch::DispatchingSandboxBackend;
pub use docker::DockerSandboxBackend;
pub use e2b::E2bSandboxBackend;
pub use mock::MockSandboxBackend;
pub use utils::{shell_command, shell_escape, split_backend_id};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SandboxClassSpecJson {
    pub provider: String,
    pub provider_config: Value,
    pub credentials: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SandboxPolicySpecJson {
    pub class_ref: ResourceRefJson,
    pub template: SandboxPolicyTemplateJson,
    pub quota: SandboxQuotaJson,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ResourceRefJson {
    pub namespace: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SandboxPolicyTemplateJson {
    pub spec: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SandboxQuotaJson {
    pub max_concurrent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxHandle {
    pub backend_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecSpec {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessHandle {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

#[async_trait]
pub trait SandboxBackend: Send + Sync {
    async fn create(
        &self,
        class: &SandboxClassSpecJson,
        policy: &SandboxPolicySpecJson,
    ) -> Result<SandboxHandle>;
    async fn destroy(&self, backend_id: &str) -> Result<()>;
    async fn exec(&self, backend_id: &str, spec: ExecSpec) -> Result<ProcessHandle>;
    async fn spawn_process(&self, backend_id: &str, spec: ExecSpec) -> Result<ProcessHandle> {
        self.exec(backend_id, spec).await
    }
    async fn write_stdin(&self, _backend_id: &str, _process_id: &str, _input: &[u8]) -> Result<()> {
        Err(anyhow!(
            "sandbox backend does not support interactive stdin"
        ))
    }
    async fn read_process_output(
        &self,
        _backend_id: &str,
        _process_id: &str,
    ) -> Result<ProcessOutput> {
        Err(anyhow!(
            "sandbox backend does not support process output reads"
        ))
    }
    async fn kill_process(&self, _backend_id: &str, _process_id: &str) -> Result<()> {
        Err(anyhow!("sandbox backend does not support process kill"))
    }
    async fn read_file(&self, backend_id: &str, path: &str) -> Result<Vec<u8>>;
    async fn write_file(&self, backend_id: &str, path: &str, content: &[u8]) -> Result<()>;
}

#[async_trait]
impl<T: SandboxBackend + ?Sized> SandboxBackend for &T {
    async fn create(
        &self,
        class: &SandboxClassSpecJson,
        policy: &SandboxPolicySpecJson,
    ) -> Result<SandboxHandle> {
        (**self).create(class, policy).await
    }

    async fn destroy(&self, backend_id: &str) -> Result<()> {
        (**self).destroy(backend_id).await
    }

    async fn exec(&self, backend_id: &str, spec: ExecSpec) -> Result<ProcessHandle> {
        (**self).exec(backend_id, spec).await
    }

    async fn spawn_process(&self, backend_id: &str, spec: ExecSpec) -> Result<ProcessHandle> {
        (**self).spawn_process(backend_id, spec).await
    }

    async fn write_stdin(&self, backend_id: &str, process_id: &str, input: &[u8]) -> Result<()> {
        (**self).write_stdin(backend_id, process_id, input).await
    }

    async fn read_process_output(
        &self,
        backend_id: &str,
        process_id: &str,
    ) -> Result<ProcessOutput> {
        (**self).read_process_output(backend_id, process_id).await
    }

    async fn kill_process(&self, backend_id: &str, process_id: &str) -> Result<()> {
        (**self).kill_process(backend_id, process_id).await
    }

    async fn read_file(&self, backend_id: &str, path: &str) -> Result<Vec<u8>> {
        (**self).read_file(backend_id, path).await
    }

    async fn write_file(&self, backend_id: &str, path: &str, content: &[u8]) -> Result<()> {
        (**self).write_file(backend_id, path, content).await
    }
}
