// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};

const DOCKER_READY_FILE: &str = "/tmp/.talon-sandbox-ready";

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

#[derive(Clone, Default)]
pub struct DockerSandboxBackend {
    processes: Arc<Mutex<HashMap<String, ProcessOutput>>>,
}

#[async_trait]
impl SandboxBackend for DockerSandboxBackend {
    async fn create(
        &self,
        class: &SandboxClassSpecJson,
        policy: &SandboxPolicySpecJson,
    ) -> Result<SandboxHandle> {
        let image = docker_image(class, policy);
        let container_name = format!("talon-sandbox-{}", uuid::Uuid::now_v7());
        let mut args = vec![
            "run".to_string(),
            "-d".to_string(),
            "--name".to_string(),
            container_name,
            "--label".to_string(),
            "talon.impalasys.com/sandbox=true".to_string(),
        ];

        if let Some(network_mode) = docker_network_mode(policy) {
            args.push("--network".to_string());
            args.push(network_mode);
        }
        if let Some(platform) = docker_platform(class, policy) {
            args.push("--platform".to_string());
            args.push(platform);
        }

        let bootstrap = docker_bootstrap_command(policy);
        args.push(image);
        args.push("sh".to_string());
        args.push("-lc".to_string());
        args.push(bootstrap);

        let output = Command::new("docker").args(&args).output().await?;
        if !output.status.success() {
            return Err(anyhow!(
                "docker sandbox create failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let backend_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if backend_id.is_empty() {
            return Err(anyhow!(
                "docker sandbox create returned an empty container id"
            ));
        }
        if let Err(err) = wait_for_docker_bootstrap(&backend_id, class).await {
            let _ = self.destroy(&backend_id).await;
            return Err(err);
        }
        Ok(SandboxHandle { backend_id })
    }

    async fn destroy(&self, backend_id: &str) -> Result<()> {
        let output = Command::new("docker")
            .args(["rm", "-f", backend_id])
            .output()
            .await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("No such container") {
                return Err(anyhow!("docker sandbox destroy failed: {}", stderr.trim()));
            }
        }
        Ok(())
    }

    async fn exec(&self, backend_id: &str, spec: ExecSpec) -> Result<ProcessHandle> {
        let mut args = vec!["exec".to_string()];
        if !spec.cwd.trim().is_empty() {
            args.push("-w".to_string());
            args.push(spec.cwd.clone());
        }
        for (key, value) in &spec.env {
            args.push("-e".to_string());
            args.push(format!("{key}={value}"));
        }
        args.push(backend_id.to_string());
        args.push(spec.command.clone());
        args.extend(spec.args.clone());

        let output = Command::new("docker").args(&args).output().await?;
        let process_id = uuid::Uuid::now_v7().to_string();
        self.processes.lock().await.insert(
            process_id.clone(),
            ProcessOutput {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code(),
            },
        );
        Ok(ProcessHandle { id: process_id })
    }

    async fn read_process_output(
        &self,
        _backend_id: &str,
        process_id: &str,
    ) -> Result<ProcessOutput> {
        self.processes
            .lock()
            .await
            .get(process_id)
            .cloned()
            .ok_or_else(|| anyhow!("docker process output '{}' not found", process_id))
    }

    async fn kill_process(&self, _backend_id: &str, process_id: &str) -> Result<()> {
        self.processes.lock().await.remove(process_id);
        Ok(())
    }

    async fn read_file(&self, backend_id: &str, path: &str) -> Result<Vec<u8>> {
        let command = format!("base64 < {}", shell_escape(path));
        let output = Command::new("docker")
            .args(["exec", backend_id, "sh", "-lc", &command])
            .output()
            .await?;
        if !output.status.success() {
            return Err(anyhow!(
                "docker sandbox read_file failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let encoded = String::from_utf8_lossy(&output.stdout).replace('\n', "");
        Ok(general_purpose::STANDARD.decode(encoded)?)
    }

    async fn write_file(&self, backend_id: &str, path: &str, content: &[u8]) -> Result<()> {
        let command = format!(
            "mkdir -p {} && cat > {}",
            shell_escape(&docker_parent_dir(path)),
            shell_escape(path)
        );
        let mut child = Command::new("docker")
            .args(["exec", "-i", backend_id, "sh", "-lc", &command])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("docker exec stdin unavailable"))?;
        stdin.write_all(content).await?;
        drop(stdin);
        let output = child.wait_with_output().await?;
        if !output.status.success() {
            return Err(anyhow!(
                "docker sandbox write_file failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(())
    }
}

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
        let process_id = uuid::Uuid::now_v7().to_string();
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
    static OUTPUTS: std::sync::OnceLock<Mutex<HashMap<String, ProcessOutput>>> =
        std::sync::OnceLock::new();
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

fn connect_json_values(bytes: &[u8]) -> Vec<Value> {
    let mut values = Vec::new();
    let mut cursor = 0;
    while cursor + 5 <= bytes.len() {
        let length = u32::from_be_bytes([
            bytes[cursor + 1],
            bytes[cursor + 2],
            bytes[cursor + 3],
            bytes[cursor + 4],
        ]) as usize;
        cursor += 5;
        if cursor + length > bytes.len() {
            values.clear();
            break;
        }
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes[cursor..cursor + length]) {
            values.push(value);
        }
        cursor += length;
    }
    if values.is_empty() {
        values.extend(
            String::from_utf8_lossy(bytes)
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok()),
        );
    }
    values
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

pub fn shell_command(spec: &ExecSpec) -> String {
    let mut command = shell_escape(&spec.command);
    for arg in &spec.args {
        command.push(' ');
        command.push_str(&shell_escape(arg));
    }
    if spec.cwd.trim().is_empty() {
        command
    } else {
        format!("cd {} && {}", shell_escape(&spec.cwd), command)
    }
}

pub fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn provider_for_prefix(provider: &str) -> &str {
    match provider {
        "" | "mock" => "mock",
        "local-docker" => "docker",
        other => other,
    }
}

pub fn split_backend_id(backend_id: &str) -> (&str, &str) {
    match backend_id.split_once(':') {
        Some((provider @ ("mock" | "docker" | "daytona" | "e2b"), id)) => (provider, id),
        _ => ("mock", backend_id),
    }
}

fn docker_image(class: &SandboxClassSpecJson, policy: &SandboxPolicySpecJson) -> String {
    json_string_at(&policy.template.spec, "/image")
        .or_else(|| json_string_at(&class.provider_config, "/image"))
        .unwrap_or_else(|| "debian:bookworm-slim".to_string())
}

fn docker_network_mode(policy: &SandboxPolicySpecJson) -> Option<String> {
    match json_string_at(&policy.template.spec, "/network/mode").as_deref() {
        Some("restricted") | Some("none") => Some("none".to_string()),
        Some("host") => Some("host".to_string()),
        Some(value) if !value.trim().is_empty() => Some(value.to_string()),
        _ => None,
    }
}

fn docker_platform(class: &SandboxClassSpecJson, policy: &SandboxPolicySpecJson) -> Option<String> {
    json_string_at(&policy.template.spec, "/platform")
        .or_else(|| json_string_at(&class.provider_config, "/platform"))
}

fn docker_bootstrap_command(policy: &SandboxPolicySpecJson) -> String {
    let mut dirs = vec![docker_workspace(policy), "/tmp".to_string()];
    if let Some(writable) = policy
        .template
        .spec
        .pointer("/filesystem/writable")
        .and_then(|value| value.as_array())
    {
        dirs.extend(
            writable
                .iter()
                .filter_map(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string),
        );
    }
    dirs.sort();
    dirs.dedup();
    let mkdirs = dirs
        .iter()
        .map(|dir| shell_escape(dir))
        .collect::<Vec<_>>()
        .join(" ");
    let mut commands = vec![
        "set -e".to_string(),
        format!("rm -f {}", shell_escape(DOCKER_READY_FILE)),
        format!("mkdir -p {}", mkdirs),
    ];
    let packages = docker_setup_packages(policy);
    if !packages.is_empty() {
        commands.push(docker_package_install_command(&packages));
    }
    commands.extend(docker_setup_commands(policy));
    commands.push(format!("touch {}", shell_escape(DOCKER_READY_FILE)));
    commands.push("exec sleep infinity".to_string());
    commands.join("\n")
}

async fn wait_for_docker_bootstrap(backend_id: &str, class: &SandboxClassSpecJson) -> Result<()> {
    let timeout_seconds =
        json_u64_at(&class.provider_config, "/setupTimeoutSeconds").unwrap_or(300);
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    loop {
        let ready = Command::new("docker")
            .args([
                "exec",
                backend_id,
                "sh",
                "-lc",
                &format!("test -f {}", shell_escape(DOCKER_READY_FILE)),
            ])
            .output()
            .await?;
        if ready.status.success() {
            return Ok(());
        }

        if !docker_container_running(backend_id).await? {
            return Err(anyhow!(
                "docker sandbox setup failed before ready: {}",
                docker_logs(backend_id).await.trim()
            ));
        }

        if Instant::now() >= deadline {
            return Err(anyhow!(
                "docker sandbox setup timed out after {}s: {}",
                timeout_seconds,
                docker_logs(backend_id).await.trim()
            ));
        }
        sleep(Duration::from_millis(250)).await;
    }
}

async fn docker_container_running(backend_id: &str) -> Result<bool> {
    let output = Command::new("docker")
        .args(["inspect", "-f", "{{.State.Running}}", backend_id])
        .output()
        .await?;
    if !output.status.success() {
        return Ok(false);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
}

async fn docker_logs(backend_id: &str) -> String {
    match Command::new("docker")
        .args(["logs", backend_id])
        .output()
        .await
    {
        Ok(output) => {
            let mut logs = String::from_utf8_lossy(&output.stdout).to_string();
            logs.push_str(&String::from_utf8_lossy(&output.stderr));
            logs
        }
        Err(err) => err.to_string(),
    }
}

fn docker_workspace(policy: &SandboxPolicySpecJson) -> String {
    json_string_at(&policy.template.spec, "/workspace/mountPath")
        .unwrap_or_else(|| "/workspace".to_string())
}

fn docker_setup_packages(policy: &SandboxPolicySpecJson) -> Vec<String> {
    json_string_array_at(&policy.template.spec, "/setup/packages")
}

fn docker_setup_commands(policy: &SandboxPolicySpecJson) -> Vec<String> {
    json_string_array_at(&policy.template.spec, "/setup/commands")
}

fn docker_package_install_command(packages: &[String]) -> String {
    let packages = packages
        .iter()
        .map(|package| shell_escape(package))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "if command -v apt-get >/dev/null 2>&1; then \
             apt-get update && \
             DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends {packages} && \
             rm -rf /var/lib/apt/lists/*; \
         elif command -v apk >/dev/null 2>&1; then \
             apk add --no-cache {packages}; \
         elif command -v dnf >/dev/null 2>&1; then \
             dnf install -y {packages} && dnf clean all; \
         elif command -v yum >/dev/null 2>&1; then \
             yum install -y {packages} && yum clean all; \
         else \
             echo 'talon docker sandbox setup could not find apt-get, apk, dnf, or yum' >&2; \
             exit 127; \
         fi"
    )
}

fn docker_parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_string(),
        Some(index) => path[..index].to_string(),
        None => ".".to_string(),
    }
}

fn json_string_at(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn json_string_array_at(value: &Value, pointer: &str) -> Vec<String> {
    value
        .pointer(pointer)
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .map(|items: Vec<String>| {
            items
                .into_iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn json_bool_at(value: &Value, pointer: &str) -> Option<bool> {
    value.pointer(pointer).and_then(|value| value.as_bool())
}

fn json_u64_at(value: &Value, pointer: &str) -> Option<u64> {
    value.pointer(pointer).and_then(|value| value.as_u64())
}

#[cfg(test)]
mod tests {
    use super::{
        connect_json_values, DispatchingSandboxBackend, DockerSandboxBackend, ExecSpec,
        SandboxBackend, SandboxClassSpecJson, SandboxPolicySpecJson, SandboxPolicyTemplateJson,
        SandboxQuotaJson,
    };
    use crate::test_support::docker_test_guard;
    use anyhow::Result;
    use base64::Engine as _;

    #[tokio::test]
    async fn dispatching_backend_prefixes_mock_sandboxes() {
        let backend = DispatchingSandboxBackend::default();
        let handle = backend
            .create(
                &SandboxClassSpecJson {
                    provider: "mock".to_string(),
                    provider_config: serde_json::json!({}),
                    credentials: serde_json::json!({}),
                },
                &SandboxPolicySpecJson::default(),
            )
            .await
            .unwrap();
        assert!(handle.backend_id.starts_with("mock:mock-mock-"));
        backend.destroy(&handle.backend_id).await.unwrap();
    }

    #[test]
    fn parses_connect_json_frames() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "event": { "data": { "stdout": base64::engine::general_purpose::STANDARD.encode("hello") } }
        }))
        .unwrap();
        let mut frame = vec![0];
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(&payload);
        let values = connect_json_values(&frame);
        assert_eq!(values.len(), 1);
    }

    #[tokio::test]
    async fn docker_backend_smoke_when_enabled() {
        if std::env::var("TALON_DOCKER_SANDBOX_TEST").ok().as_deref() != Some("1") {
            return;
        }
        let _guard = docker_test_guard();
        let backend = DockerSandboxBackend::default();
        let class = SandboxClassSpecJson {
            provider: "docker".to_string(),
            provider_config: serde_json::json!({ "image": "alpine:3.20" }),
            credentials: serde_json::json!({}),
        };
        let policy = SandboxPolicySpecJson {
            class_ref: Default::default(),
            template: SandboxPolicyTemplateJson {
                spec: serde_json::json!({
                    "workspace": { "mountPath": "/workspace" },
                    "setup": {
                        "commands": ["printf setup-ok > /workspace/setup.txt"]
                    },
                    "network": { "mode": "restricted" },
                    "filesystem": { "writable": ["/workspace", "/tmp"] }
                }),
            },
            quota: SandboxQuotaJson { max_concurrent: 1 },
        };
        let handle = backend.create(&class, &policy).await.unwrap();
        let test_result: Result<()> = async {
            backend
                .write_file(&handle.backend_id, "/workspace/hello.txt", b"hello docker")
                .await?;
            let content = backend
                .read_file(&handle.backend_id, "/workspace/hello.txt")
                .await?;
            assert_eq!(content, b"hello docker");
            let setup_content = backend
                .read_file(&handle.backend_id, "/workspace/setup.txt")
                .await?;
            assert_eq!(setup_content, b"setup-ok");
            let process = backend
                .exec(
                    &handle.backend_id,
                    ExecSpec {
                        command: "sh".to_string(),
                        args: vec![
                            "-lc".to_string(),
                            "cat /workspace/hello.txt && printf ' ok'".to_string(),
                        ],
                        cwd: "/workspace".to_string(),
                        env: Default::default(),
                    },
                )
                .await?;
            let output = backend
                .read_process_output(&handle.backend_id, &process.id)
                .await?;
            assert_eq!(output.exit_code, Some(0));
            assert_eq!(output.stdout, "hello docker ok");
            Ok(())
        }
        .await;
        let destroy_result = backend.destroy(&handle.backend_id).await;
        test_result.unwrap();
        destroy_result.unwrap();
    }
}
