// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::Mutex;

use super::utils::{
    connect_json_values, docker_parent_dir, json_bool_at, json_string_at, json_u64_at, shell_escape,
};
use super::{
    ExecSpec, ProcessHandle, ProcessOutput, SandboxBackend, SandboxClassSpecJson, SandboxHandle,
    SandboxPolicySpecJson,
};

pub struct E2bSandboxBackend;

#[async_trait]
impl SandboxBackend for E2bSandboxBackend {
    async fn create(
        &self,
        class: &SandboxClassSpecJson,
        policy: &SandboxPolicySpecJson,
    ) -> Result<SandboxHandle> {
        let api_key = e2b_api_key(class)?;
        let template_id = e2b_template_id(class, policy);
        let timeout = json_u64_at(&class.provider_config, "/timeoutSeconds").unwrap_or(3600);
        let secure = json_bool_at(&class.provider_config, "/secure").unwrap_or(true);
        let allow_internet_access = !matches!(
            json_string_at(&policy.template.spec, "/network/mode").as_deref(),
            Some("restricted") | Some("none")
        );
        let mut body = serde_json::json!({
            "templateID": template_id,
            "timeout": timeout,
            "secure": secure,
            "allow_internet_access": allow_internet_access,
            "metadata": {
                "talon.impalasys.com/sandbox": "true"
            }
        });
        if let Some(env_vars) = class
            .provider_config
            .get("envVars")
            .or_else(|| class.provider_config.get("env"))
        {
            body["envVars"] = env_vars.clone();
        }
        let response: serde_json::Value = reqwest::Client::new()
            .post(format!("{}/sandboxes", e2b_api_base(class)))
            .header("X-API-Key", api_key)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let sandbox_id = response
            .get("sandboxID")
            .or_else(|| response.get("id"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("E2B create sandbox response missing sandboxID"))?;
        let access_token = response
            .get("envdAccessToken")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        Ok(SandboxHandle {
            backend_id: e2b_backend_id(sandbox_id, access_token, &e2b_sandbox_base(class)),
        })
    }

    async fn destroy(&self, backend_id: &str) -> Result<()> {
        let handle = E2bHandle::parse(backend_id)?;
        let api_key = std::env::var("E2B_API_KEY")
            .map_err(|_| anyhow!("E2B_API_KEY is required for E2B sandbox destroy"))?;
        let api_base =
            std::env::var("E2B_API_URL").unwrap_or_else(|_| "https://api.e2b.app".into());
        let response = reqwest::Client::new()
            .delete(format!(
                "{}/sandboxes/{}",
                api_base.trim_end_matches('/'),
                urlencoding::encode(&handle.sandbox_id)
            ))
            .header("X-API-Key", api_key)
            .send()
            .await?;
        if !response.status().is_success() && response.status() != reqwest::StatusCode::NOT_FOUND {
            return Err(anyhow!(
                "E2B sandbox destroy failed: {}",
                response.text().await.unwrap_or_default()
            ));
        }
        Ok(())
    }

    async fn exec(&self, backend_id: &str, spec: ExecSpec) -> Result<ProcessHandle> {
        let output = e2b_run_command(backend_id, spec).await?;
        let process_id = crate::control::uuid::process_id();
        e2b_process_outputs()
            .lock()
            .await
            .insert(process_id.clone(), output);
        Ok(ProcessHandle { id: process_id })
    }

    async fn read_process_output(
        &self,
        _backend_id: &str,
        process_id: &str,
    ) -> Result<ProcessOutput> {
        e2b_process_outputs()
            .lock()
            .await
            .get(process_id)
            .cloned()
            .ok_or_else(|| anyhow!("E2B process output '{}' not found", process_id))
    }

    async fn kill_process(&self, _backend_id: &str, process_id: &str) -> Result<()> {
        e2b_process_outputs().lock().await.remove(process_id);
        Ok(())
    }

    async fn read_file(&self, backend_id: &str, path: &str) -> Result<Vec<u8>> {
        let output = e2b_run_command(
            backend_id,
            ExecSpec {
                command: "sh".to_string(),
                args: vec![
                    "-lc".to_string(),
                    format!("base64 < {}", shell_escape(path)),
                ],
                cwd: String::new(),
                env: HashMap::new(),
            },
        )
        .await?;
        if output.exit_code != Some(0) {
            return Err(anyhow!("E2B read_file failed: {}", output.stderr.trim()));
        }
        Ok(general_purpose::STANDARD.decode(output.stdout.replace('\n', ""))?)
    }

    async fn write_file(&self, backend_id: &str, path: &str, content: &[u8]) -> Result<()> {
        let encoded = general_purpose::STANDARD.encode(content);
        let output = e2b_run_command(
            backend_id,
            ExecSpec {
                command: "sh".to_string(),
                args: vec![
                    "-lc".to_string(),
                    format!(
                        "mkdir -p {} && printf %s {} | base64 -d > {}",
                        shell_escape(&docker_parent_dir(path)),
                        shell_escape(&encoded),
                        shell_escape(path)
                    ),
                ],
                cwd: String::new(),
                env: HashMap::new(),
            },
        )
        .await?;
        if output.exit_code != Some(0) {
            return Err(anyhow!("E2B write_file failed: {}", output.stderr.trim()));
        }
        Ok(())
    }
}

fn e2b_api_key(class: &SandboxClassSpecJson) -> Result<String> {
    let env_key = class
        .credentials
        .pointer("/apiKey/key")
        .and_then(|value| value.as_str())
        .unwrap_or("E2B_API_KEY");
    std::env::var(env_key).map_err(|_| anyhow!("{} is required for E2B sandbox backend", env_key))
}

fn e2b_api_base(class: &SandboxClassSpecJson) -> String {
    json_string_at(&class.provider_config, "/apiBaseUrl")
        .or_else(|| std::env::var("E2B_API_URL").ok())
        .unwrap_or_else(|| "https://api.e2b.app".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn e2b_sandbox_base(class: &SandboxClassSpecJson) -> String {
    json_string_at(&class.provider_config, "/sandboxBaseUrl")
        .or_else(|| std::env::var("E2B_SANDBOX_URL").ok())
        .unwrap_or_else(|| "https://49983-{sandboxID}.e2b.app".to_string())
}

fn e2b_template_id(class: &SandboxClassSpecJson, policy: &SandboxPolicySpecJson) -> String {
    json_string_at(&class.provider_config, "/templateID")
        .or_else(|| json_string_at(&class.provider_config, "/templateId"))
        .or_else(|| json_string_at(&policy.template.spec, "/image"))
        .unwrap_or_else(|| "base".to_string())
}

fn e2b_backend_id(sandbox_id: &str, access_token: &str, sandbox_base_url: &str) -> String {
    format!("{sandbox_id}|{access_token}|{sandbox_base_url}")
}

#[derive(Debug)]
struct E2bHandle {
    sandbox_id: String,
    access_token: String,
    sandbox_base_url: String,
}

impl E2bHandle {
    fn parse(backend_id: &str) -> Result<Self> {
        let mut parts = backend_id.splitn(3, '|');
        let sandbox_id = parts
            .next()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("E2B backend id missing sandbox id"))?;
        let access_token = parts.next().unwrap_or_default();
        let sandbox_base_url = parts
            .next()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("https://49983-{sandboxID}.e2b.app");
        Ok(Self {
            sandbox_id: sandbox_id.to_string(),
            access_token: access_token.to_string(),
            sandbox_base_url: sandbox_base_url.to_string(),
        })
    }

    fn envd_url(&self) -> String {
        self.sandbox_base_url
            .replace("{sandboxID}", &self.sandbox_id)
            .trim_end_matches('/')
            .to_string()
    }
}

fn e2b_process_outputs() -> &'static Mutex<HashMap<String, ProcessOutput>> {
    static OUTPUTS: OnceLock<Mutex<HashMap<String, ProcessOutput>>> = OnceLock::new();
    OUTPUTS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn e2b_run_command(backend_id: &str, spec: ExecSpec) -> Result<ProcessOutput> {
    let handle = E2bHandle::parse(backend_id)?;
    let body = serde_json::json!({
        "process": {
            "cmd": spec.command,
            "args": spec.args,
            "envs": spec.env,
            "cwd": spec.cwd,
        },
        "stdin": false,
    });
    let mut headers = HeaderMap::new();
    headers.insert("Connect-Protocol-Version", HeaderValue::from_static("1"));
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/connect+json"),
    );
    if !handle.access_token.is_empty() {
        headers.insert(
            "X-Access-Token",
            HeaderValue::from_str(&handle.access_token)?,
        );
    }
    let response = reqwest::Client::new()
        .post(format!("{}/process.Process/Start", handle.envd_url()))
        .headers(headers)
        .body(serde_json::to_vec(&body)?)
        .send()
        .await?
        .error_for_status()?;
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk?);
    }
    Ok(process_output_from_connect_stream(&bytes))
}

fn process_output_from_connect_stream(bytes: &[u8]) -> ProcessOutput {
    let mut output = ProcessOutput {
        stdout: String::new(),
        stderr: String::new(),
        exit_code: None,
    };
    for value in connect_json_values(bytes) {
        collect_process_output(&value, &mut output);
    }
    if output.exit_code.is_none() {
        output.exit_code = Some(0);
    }
    output
}

fn collect_process_output(value: &Value, output: &mut ProcessOutput) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                match key.as_str() {
                    "stdout" | "out" => {
                        if let Some(text) = process_text(child) {
                            output.stdout.push_str(&text);
                        }
                    }
                    "stderr" | "err" => {
                        if let Some(text) = process_text(child) {
                            output.stderr.push_str(&text);
                        }
                    }
                    "exitCode" | "exit_code" => {
                        if let Some(code) = child.as_i64() {
                            output.exit_code = Some(code as i32);
                        }
                    }
                    _ => collect_process_output(child, output),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_process_output(item, output);
            }
        }
        _ => {}
    }
}

fn process_text(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    general_purpose::STANDARD
        .decode(text)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .or_else(|| Some(text.to_string()))
}
