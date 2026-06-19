// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};

use super::utils::{docker_parent_dir, json_string_array_at, json_string_at, shell_escape};
use super::{
    ExecSpec, ProcessHandle, ProcessOutput, SandboxBackend, SandboxClassSpecJson, SandboxHandle,
    SandboxPolicySpecJson,
};

const DOCKER_READY_FILE: &str = "/tmp/.talon-sandbox-ready";

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

        let output = Command::new("docker")
            .args(&args)
            .output()
            .await
            .context(
                "failed to run Docker CLI while creating sandbox; ensure talon-worker has the Docker CLI and /var/run/docker.sock mounted",
            )?;
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
        super::utils::json_u64_at(&class.provider_config, "/setupTimeoutSeconds").unwrap_or(300);
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
            .await
            .context(
                "failed to run Docker CLI while waiting for sandbox bootstrap; ensure talon-worker has the Docker CLI and /var/run/docker.sock mounted",
            )?;
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
        .await
        .context("failed to run Docker CLI while inspecting sandbox container")?;
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
