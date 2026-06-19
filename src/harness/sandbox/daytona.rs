// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};

use super::utils::{shell_command, shell_escape};
use super::{
    ExecSpec, ProcessHandle, ProcessOutput, SandboxBackend, SandboxClassSpecJson, SandboxHandle,
    SandboxPolicySpecJson,
};

pub struct DaytonaSandboxBackend;

#[async_trait]
impl SandboxBackend for DaytonaSandboxBackend {
    async fn create(
        &self,
        class: &SandboxClassSpecJson,
        _policy: &SandboxPolicySpecJson,
    ) -> Result<SandboxHandle> {
        let api_key = daytona_api_key(class)?;
        let api_base = daytona_api_base(class);
        let body = if class.provider_config.is_null() {
            serde_json::json!({})
        } else {
            class.provider_config.clone()
        };
        let response: serde_json::Value = reqwest::Client::new()
            .post(format!("{api_base}/api/sandbox"))
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let backend_id = response
            .get("id")
            .or_else(|| response.get("sandboxId"))
            .or_else(|| response.pointer("/sandbox/id"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("Daytona create sandbox response missing id"))?
            .to_string();
        Ok(SandboxHandle { backend_id })
    }

    async fn destroy(&self, backend_id: &str) -> Result<()> {
        let api_key = std::env::var("DAYTONA_API_KEY")
            .map_err(|_| anyhow!("DAYTONA_API_KEY is required for Daytona sandbox destroy"))?;
        reqwest::Client::new()
            .delete(format!(
                "https://app.daytona.io/api/sandbox/{}",
                urlencoding::encode(backend_id)
            ))
            .bearer_auth(api_key)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn exec(&self, backend_id: &str, spec: ExecSpec) -> Result<ProcessHandle> {
        let command = shell_command(&spec);
        let _ = daytona_execute(backend_id, &command).await?;
        Ok(ProcessHandle {
            id: uuid::Uuid::now_v7().to_string(),
        })
    }

    async fn read_process_output(
        &self,
        backend_id: &str,
        process_id: &str,
    ) -> Result<ProcessOutput> {
        let value = daytona_execute(backend_id, process_id).await?;
        Ok(ProcessOutput {
            stdout: value
                .get("result")
                .or_else(|| value.get("stdout"))
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            stderr: value
                .get("stderr")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            exit_code: value
                .get("exitCode")
                .or_else(|| value.get("exit_code"))
                .and_then(|value| value.as_i64())
                .map(|value| value as i32),
        })
    }

    async fn read_file(&self, backend_id: &str, path: &str) -> Result<Vec<u8>> {
        let encoded_path = shell_escape(path);
        let value = daytona_execute(backend_id, &format!("base64 < {}", encoded_path)).await?;
        let output = value
            .get("result")
            .or_else(|| value.get("stdout"))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .replace('\n', "");
        Ok(general_purpose::STANDARD.decode(output)?)
    }

    async fn write_file(&self, backend_id: &str, path: &str, content: &[u8]) -> Result<()> {
        let encoded = general_purpose::STANDARD.encode(content);
        let command = format!(
            "printf %s {} | base64 -d > {}",
            shell_escape(&encoded),
            shell_escape(path)
        );
        let _ = daytona_execute(backend_id, &command).await?;
        Ok(())
    }
}

fn daytona_api_key(class: &SandboxClassSpecJson) -> Result<String> {
    let env_key = class
        .credentials
        .pointer("/apiKey/key")
        .and_then(|value| value.as_str())
        .unwrap_or("DAYTONA_API_KEY");
    std::env::var(env_key)
        .map_err(|_| anyhow!("{} is required for Daytona sandbox backend", env_key))
}

fn daytona_api_base(class: &SandboxClassSpecJson) -> String {
    class
        .provider_config
        .get("apiBaseUrl")
        .and_then(|value| value.as_str())
        .unwrap_or("https://app.daytona.io")
        .trim_end_matches('/')
        .to_string()
}

async fn daytona_execute(backend_id: &str, command: &str) -> Result<serde_json::Value> {
    let api_key = std::env::var("DAYTONA_API_KEY")
        .map_err(|_| anyhow!("DAYTONA_API_KEY is required for Daytona toolbox execution"))?;
    Ok(reqwest::Client::new()
        .post(format!(
            "https://proxy.app.daytona.io/toolbox/{}/process/execute",
            urlencoding::encode(backend_id)
        ))
        .bearer_auth(api_key)
        .json(&serde_json::json!({ "command": command }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
