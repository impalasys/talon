// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::Mutex;

use super::utils::shell_command;
use super::{
    ExecSpec, ProcessHandle, ProcessOutput, SandboxBackend, SandboxClassSpecJson, SandboxHandle,
    SandboxPolicySpecJson,
};

#[derive(Clone, Default)]
pub struct MockSandboxBackend;

#[derive(Default)]
struct MockSandboxState {
    files: HashMap<(String, String), Vec<u8>>,
    processes: HashMap<String, ProcessOutput>,
}

fn mock_sandbox_state() -> &'static Mutex<MockSandboxState> {
    static STATE: OnceLock<Mutex<MockSandboxState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(MockSandboxState::default()))
}

#[async_trait]
impl SandboxBackend for MockSandboxBackend {
    async fn create(
        &self,
        class: &SandboxClassSpecJson,
        _policy: &SandboxPolicySpecJson,
    ) -> Result<SandboxHandle> {
        Ok(SandboxHandle {
            backend_id: format!("mock-{}-{}", class.provider, uuid::Uuid::now_v7()),
        })
    }

    async fn destroy(&self, _backend_id: &str) -> Result<()> {
        Ok(())
    }

    async fn exec(&self, _backend_id: &str, spec: ExecSpec) -> Result<ProcessHandle> {
        let process_id = uuid::Uuid::now_v7().to_string();
        mock_sandbox_state()
            .lock()
            .await
            .processes
            .insert(process_id.clone(), mock_process_output(&spec));
        Ok(ProcessHandle { id: process_id })
    }

    async fn read_process_output(
        &self,
        _backend_id: &str,
        process_id: &str,
    ) -> Result<ProcessOutput> {
        mock_sandbox_state()
            .lock()
            .await
            .processes
            .get(process_id)
            .cloned()
            .ok_or_else(|| anyhow!("mock process output '{}' not found", process_id))
    }

    async fn read_file(&self, backend_id: &str, path: &str) -> Result<Vec<u8>> {
        mock_sandbox_state()
            .lock()
            .await
            .files
            .get(&(backend_id.to_string(), path.to_string()))
            .cloned()
            .ok_or_else(|| anyhow!("mock sandbox file '{}' not found", path))
    }

    async fn write_file(&self, backend_id: &str, path: &str, content: &[u8]) -> Result<()> {
        mock_sandbox_state()
            .lock()
            .await
            .files
            .insert((backend_id.to_string(), path.to_string()), content.to_vec());
        Ok(())
    }
}

fn mock_process_output(spec: &ExecSpec) -> ProcessOutput {
    let script = if spec.command == "sh" && spec.args.first().map(String::as_str) == Some("-lc") {
        spec.args.get(1).map(String::as_str).unwrap_or_default()
    } else {
        ""
    };
    let stdout = if let Some(value) = script.strip_prefix("printf ") {
        value.trim_matches('"').trim_matches('\'').to_string()
    } else {
        format!("mock-exec:{}", shell_command(spec))
    };
    ProcessOutput {
        stdout,
        stderr: String::new(),
        exit_code: Some(0),
    }
}
