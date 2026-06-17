// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdout, Command};
use tokio_util::sync::CancellationToken;

use crate::control::config::Config;
use crate::control::ControlPlane;
use crate::gateway::rpc::{manifests, resources_proto};
use crate::harness::executor::ExecutionSink;
use crate::harness::sandbox::{DispatchingSandboxBackend, ExecSpec, SandboxBackend};
use crate::worker::controllers::sandbox::SandboxLeaseService;

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcMessage {
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: Value,
}

#[derive(Clone)]
pub struct AcpAgentRuntime {
    ns: String,
    agent_id: String,
    session_id: String,
    acp: manifests::AcpRuntime,
    cp: ControlPlane,
    _config: Arc<Config>,
}

impl AcpAgentRuntime {
    pub async fn build(
        ns: &str,
        agent_id: &str,
        session_id: &str,
        cp: &ControlPlane,
        config: &Arc<Config>,
    ) -> Result<Self> {
        let store = crate::control::resources::ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
        let agent = store
            .get_agent(ns, agent_id)
            .await?
            .ok_or_else(|| anyhow!("Agent '{}' not found in ns '{}'", agent_id, ns))?;
        let spec = agent
            .spec
            .ok_or_else(|| anyhow!("Agent '{}' has no spec", agent_id))?;
        let runtime = spec
            .runtime
            .as_ref()
            .ok_or_else(|| anyhow!("Agent '{}' has no runtime config", agent_id))?;
        if runtime.kind != "acp" {
            return Err(anyhow!("Agent '{}' runtime is not ACP", agent_id));
        }
        let acp = runtime
            .acp
            .clone()
            .ok_or_else(|| anyhow!("Agent '{}' ACP runtime config is missing", agent_id))?;
        Ok(Self {
            ns: ns.to_string(),
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            acp,
            cp: cp.clone(),
            _config: config.clone(),
        })
    }

    pub async fn execute(
        &self,
        prompt: &str,
        sink: &dyn ExecutionSink,
        cancellation_token: Option<&CancellationToken>,
    ) -> Result<String> {
        let store = crate::control::resources::ResourceStore::new(
            self.cp.kv.clone(),
            self.cp.pubsub.clone(),
        );
        let sandbox_backend = DispatchingSandboxBackend::default();
        let lease_service =
            SandboxLeaseService::new(store, self.cp.kv.clone(), sandbox_backend.clone());
        let leased = lease_service
            .acquire(
                &self.ns,
                &self.agent_id,
                &self.session_id,
                &self.acp.sandbox_policy_ref,
            )
            .await?;
        let sandbox_backend_id = sandbox_backend_id(&leased.sandbox)?;

        let command = if self.acp.command.trim().is_empty() {
            self.acp.harness_ref.clone()
        } else {
            self.acp.command.clone()
        };
        if command.trim().is_empty() {
            let _ = lease_service.release(&leased.sandbox, &leased.token).await;
            return Err(anyhow!("ACP runtime requires command or harnessRef"));
        }

        let mut child = acp_harness_command(&self.acp, &sandbox_backend_id, &command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("ACP harness stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("ACP harness stdout unavailable"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("ACP harness stderr unavailable"))?;
        let stderr_task = tokio::spawn(async move {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text).await;
            text
        });
        let mut lines = BufReader::new(stdout).lines();

        send_request(
            &mut stdin,
            1,
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": {
                    "name": "talon",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )
        .await?;
        let _ = read_response(&mut lines, 1).await?;
        send_request(
            &mut stdin,
            2,
            "authenticate",
            json!({ "methodId": acp_auth_method(&self.acp) }),
        )
        .await?;
        let _ = read_optional_capability_response(&mut lines, 2).await?;
        send_request(
            &mut stdin,
            3,
            if self.acp.persist_session {
                "session/load"
            } else {
                "session/new"
            },
            session_open_params(&self.session_id, &self.acp),
        )
        .await?;
        let open_response = read_response(&mut lines, 3).await?;
        let acp_session_id =
            extract_session_id(&open_response.result).unwrap_or_else(|| self.session_id.clone());
        let mut prompt_request_id = 4;
        if acp_full_access_allowed(&self.acp) {
            send_request(
                &mut stdin,
                prompt_request_id,
                "session/set_mode",
                json!({
                    "sessionId": acp_session_id,
                    "modeId": "full-access",
                }),
            )
            .await?;
            let _ = read_optional_capability_response(&mut lines, prompt_request_id).await?;
            prompt_request_id += 1;
        }
        send_request(
            &mut stdin,
            prompt_request_id,
            "session/prompt",
            json!({
                "sessionId": acp_session_id,
                "prompt": [{ "type": "text", "text": prompt }],
            }),
        )
        .await?;

        let mut reply = String::new();
        loop {
            let next = if let Some(token) = cancellation_token {
                tokio::select! {
                    _ = token.cancelled() => {
                        let _ = send_request(&mut stdin, prompt_request_id + 1, "session/cancel", json!({"sessionId": acp_session_id})).await;
                        let _ = child.kill().await;
                        sink.on_error("ACP session cancelled").await;
                        lease_service.release(&leased.sandbox, &leased.token).await?;
                        return Err(anyhow!("ACP session cancelled"));
                    }
                    line = lines.next_line() => line?,
                }
            } else {
                lines.next_line().await?
            };

            let Some(line) = next else {
                break;
            };
            if line.trim().is_empty() {
                continue;
            }
            let message: JsonRpcMessage = match serde_json::from_str(&line) {
                Ok(message) => message,
                Err(err) => {
                    tracing::warn!(error = %err, line = %line, "Ignoring invalid ACP JSON-RPC line");
                    continue;
                }
            };
            if !message.error.is_null() {
                let err = message.error.to_string();
                sink.on_error(&err).await;
                lease_service
                    .release(&leased.sandbox, &leased.token)
                    .await?;
                return Err(anyhow!("ACP harness error: {}", err));
            }
            if is_session_update(&message) {
                apply_session_update(message.params, sink, &mut reply).await;
            } else if let (Some(id), Some(method)) =
                (message.id.as_ref(), message.method.as_deref())
            {
                let response = self
                    .handle_client_request(
                        method,
                        message.params,
                        &sandbox_backend,
                        &sandbox_backend_id,
                        &leased.sandbox,
                    )
                    .await
                    .unwrap_or_else(|err| json!({"error": err.to_string()}));
                send_response(&mut stdin, id.clone(), response).await?;
            } else if message.id.as_ref().and_then(|id| id.as_u64()) == Some(prompt_request_id) {
                if let Some(text) = extract_text(&message.result) {
                    sink.on_token(&text).await;
                    reply.push_str(&text);
                }
                break;
            }
        }

        let _ = send_request(
            &mut stdin,
            prompt_request_id + 2,
            "session/close",
            json!({"sessionId": acp_session_id}),
        )
        .await;
        drop(stdin);
        let mut forced_shutdown = false;
        let status =
            match tokio::time::timeout(std::time::Duration::from_secs(2), child.wait()).await {
                Ok(status) => status?,
                Err(_) => {
                    forced_shutdown = true;
                    let _ = child.kill().await;
                    child.wait().await?
                }
            };
        let stderr_text = stderr_task.await.unwrap_or_default();
        if !status.success() && reply.is_empty() {
            let message = format!("ACP harness exited with {status}: {}", stderr_text.trim());
            sink.on_error(&message).await;
            lease_service
                .release(&leased.sandbox, &leased.token)
                .await?;
            return Err(anyhow!(message));
        }
        if !status.success() && !forced_shutdown {
            tracing::warn!(
                status = %status,
                stderr = %stderr_text.trim(),
                "ACP harness exited after producing a reply"
            );
        }
        if reply.is_empty() {
            let message = if stderr_text.trim().is_empty() {
                "ACP harness exited without producing a reply".to_string()
            } else {
                format!(
                    "ACP harness exited without producing a reply: {}",
                    stderr_text.trim()
                )
            };
            sink.on_error(&message).await;
            lease_service
                .release(&leased.sandbox, &leased.token)
                .await?;
            return Err(anyhow!(message));
        }
        sink.on_done(&reply).await;
        lease_service
            .release(&leased.sandbox, &leased.token)
            .await?;
        Ok(reply)
    }

    async fn handle_client_request(
        &self,
        method: &str,
        params: Value,
        sandbox_backend: &dyn SandboxBackend,
        sandbox_backend_id: &str,
        sandbox: &resources_proto::Resource,
    ) -> Result<Value> {
        match method {
            "fs/read_text_file" | "fs/readTextFile" => {
                self.ensure_permission("filesystemRead", &params).await?;
                let path = params
                    .get("path")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| anyhow!("fs/read_text_file requires path"))?;
                let content = sandbox_backend.read_file(sandbox_backend_id, path).await?;
                Ok(json!({ "content": String::from_utf8(content)? }))
            }
            "fs/write_text_file" | "fs/writeTextFile" => {
                self.ensure_permission("filesystemWrite", &params).await?;
                let path = params
                    .get("path")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| anyhow!("fs/write_text_file requires path"))?;
                let content = params
                    .get("content")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                sandbox_backend
                    .write_file(sandbox_backend_id, path, content.as_bytes())
                    .await?;
                Ok(json!({ "ok": true }))
            }
            "terminal/exec" | "terminal/run" => {
                self.ensure_permission("terminal", &params).await?;
                let command = params
                    .get("command")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| anyhow!("terminal/exec requires command"))?;
                let process = sandbox_backend
                    .exec(
                        sandbox_backend_id,
                        ExecSpec {
                            command: "sh".to_string(),
                            args: vec!["-lc".to_string(), command.to_string()],
                            cwd: self.acp.cwd.clone(),
                            env: self.acp.env.clone(),
                        },
                    )
                    .await?;
                let output = sandbox_backend
                    .read_process_output(sandbox_backend_id, &process.id)
                    .await?;
                self.record_sandbox_process(
                    sandbox,
                    &process.id,
                    "sh",
                    &["-lc".to_string(), command.to_string()],
                    "terminal",
                    "Succeeded",
                )
                .await?;
                Ok(json!({
                    "stdout": output.stdout,
                    "stderr": output.stderr,
                    "exitCode": output.exit_code,
                }))
            }
            "permission/request" => {
                let action = params
                    .get("action")
                    .and_then(|value| value.as_str())
                    .unwrap_or("default");
                self.ensure_permission(action, &params).await?;
                Ok(json!({ "decision": "allow" }))
            }
            "session/request_permission"
            | "session/requestPermission"
            | "session/request_permissions"
            | "session/requestPermissions" => {
                self.ensure_permission("terminal", &params).await?;
                Ok(json!({
                    "outcome": {
                        "outcome": "selected",
                        "optionId": "approved",
                    }
                }))
            }
            _ => Err(anyhow!("unsupported ACP client request '{}'", method)),
        }
    }

    async fn record_sandbox_process(
        &self,
        sandbox: &resources_proto::Resource,
        process_id: &str,
        command: &str,
        args: &[String],
        protocol: &str,
        phase: &str,
    ) -> Result<()> {
        let meta = sandbox
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("Sandbox metadata is required"))?;
        let store = crate::control::resources::ResourceStore::new(
            self.cp.kv.clone(),
            self.cp.pubsub.clone(),
        );
        let current = store
            .get(&meta.namespace, "Sandbox", &meta.name)
            .await?
            .ok_or_else(|| anyhow!("Sandbox '{}' not found", meta.name))?;
        let Some(resources_proto::resource_status::Kind::Sandbox(mut status)) =
            current.status.and_then(|status| status.kind)
        else {
            return Err(anyhow!("Sandbox '{}' missing typed status", meta.name));
        };
        status
            .processes
            .push(resources_proto::SandboxProcessStatus {
                id: process_id.to_string(),
                command: command.to_string(),
                args: args.to_vec(),
                protocol: protocol.to_string(),
                phase: phase.to_string(),
            });
        store
            .patch_status(
                &meta.namespace,
                "Sandbox",
                &meta.name,
                None,
                resources_proto::ResourceStatus {
                    kind: Some(resources_proto::resource_status::Kind::Sandbox(status)),
                },
            )
            .await?;
        Ok(())
    }

    async fn ensure_permission(&self, action: &str, params: &Value) -> Result<()> {
        let decision = self
            .acp
            .permission_policy
            .get(action)
            .or_else(|| self.acp.permission_policy.get("default"))
            .map(String::as_str)
            .unwrap_or("ask");
        match decision {
            "allow" => Ok(()),
            "deny" => Err(anyhow!("permission denied for {}", action)),
            "ask" | _ => {
                self.create_permission_request(action, params).await?;
                Err(anyhow!("permission requires approval for {}", action))
            }
        }
    }

    async fn create_permission_request(&self, action: &str, params: &Value) -> Result<()> {
        let store = crate::control::resources::ResourceStore::new(
            self.cp.kv.clone(),
            self.cp.pubsub.clone(),
        );
        let name = format!("{}-{}", self.session_id, uuid::Uuid::now_v7());
        let resource = resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "PermissionRequest".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name,
                namespace: self.ns.clone(),
                labels: std::collections::HashMap::from([
                    (
                        "talon.impalasys.com/agent".to_string(),
                        self.agent_id.clone(),
                    ),
                    (
                        "talon.impalasys.com/session".to_string(),
                        self.session_id.clone(),
                    ),
                ]),
                annotations: std::collections::HashMap::new(),
                owner_references: Vec::new(),
                finalizers: Vec::new(),
                generation: 0,
                resource_version: String::new(),
                uid: String::new(),
                deletion_timestamp: None,
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::PermissionRequest(
                    resources_proto::PermissionRequestSpec {
                        agent: self.agent_id.clone(),
                        session_id: self.session_id.clone(),
                        action: action.to_string(),
                        prompt: format!("ACP harness requested permission for {}", action),
                        payload_json: serde_json::to_string(params)?,
                    },
                )),
            }),
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resources_proto::resource_status::Kind::PermissionRequest(
                    resources_proto::PermissionRequestStatus {
                        observed_generation: 0,
                        phase: "Pending".to_string(),
                        conditions: Vec::new(),
                        decision: String::new(),
                        decided_by: String::new(),
                        decided_at: 0,
                    },
                )),
            }),
        };
        store.upsert(&self.ns, resource).await?;
        Ok(())
    }
}

fn sandbox_backend_id(resource: &resources_proto::Resource) -> Result<String> {
    let Some(resources_proto::resource_status::Kind::Sandbox(status)) = resource
        .status
        .as_ref()
        .and_then(|status| status.kind.as_ref())
    else {
        return Err(anyhow!("leased Sandbox is missing typed status"));
    };
    if status.backend_id.trim().is_empty() {
        return Err(anyhow!("leased Sandbox is missing backend id"));
    }
    Ok(status.backend_id.clone())
}

fn acp_harness_command(
    acp: &manifests::AcpRuntime,
    sandbox_backend_id: &str,
    command: &str,
) -> Command {
    if let Some(container_id) = sandbox_backend_id.strip_prefix("docker:") {
        let mut docker = Command::new("docker");
        docker.arg("exec").arg("-i");
        if !acp.cwd.trim().is_empty() {
            docker.arg("-w").arg(&acp.cwd);
        }
        for (key, value) in &acp.env {
            docker.arg("-e").arg(format!("{key}={value}"));
        }
        docker.arg(container_id).arg(command).args(&acp.args);
        return docker;
    }

    let mut local = Command::new(command);
    local.args(&acp.args);
    local.current_dir(if acp.cwd.trim().is_empty() {
        "."
    } else {
        acp.cwd.as_str()
    });
    local.envs(&acp.env);
    local
}

async fn send_request(
    stdin: &mut tokio::process::ChildStdin,
    id: u64,
    method: &str,
    params: Value,
) -> Result<()> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0",
        id,
        method: method.to_string(),
        params,
    };
    let line = serde_json::to_string(&request)?;
    stdin.write_all(line.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    Ok(())
}

async fn read_response(
    lines: &mut Lines<BufReader<ChildStdout>>,
    expected_id: u64,
) -> Result<JsonRpcMessage> {
    let message = read_response_message(lines, expected_id).await?;
    if !message.error.is_null() {
        return Err(anyhow!("ACP harness error: {}", message.error));
    }
    Ok(message)
}

async fn read_optional_capability_response(
    lines: &mut Lines<BufReader<ChildStdout>>,
    expected_id: u64,
) -> Result<Option<JsonRpcMessage>> {
    let message = read_response_message(lines, expected_id).await?;
    if is_method_not_found(&message.error) {
        return Ok(None);
    }
    if !message.error.is_null() {
        return Err(anyhow!("ACP harness error: {}", message.error));
    }
    Ok(Some(message))
}

async fn read_response_message(
    lines: &mut Lines<BufReader<ChildStdout>>,
    expected_id: u64,
) -> Result<JsonRpcMessage> {
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let message: JsonRpcMessage = serde_json::from_str(&line)?;
        if message.id.as_ref().and_then(|id| id.as_u64()) == Some(expected_id) {
            return Ok(message);
        }
    }
    Err(anyhow!(
        "ACP harness exited before response id {}",
        expected_id
    ))
}

fn is_method_not_found(error: &Value) -> bool {
    error.get("code").and_then(|value| value.as_i64()) == Some(-32601)
}

async fn send_response(
    stdin: &mut tokio::process::ChildStdin,
    id: Value,
    result: Value,
) -> Result<()> {
    let line = serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    }))?;
    stdin.write_all(line.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    Ok(())
}

fn session_open_params(session_id: &str, acp: &manifests::AcpRuntime) -> Value {
    if acp.persist_session {
        json!({
            "sessionId": session_id,
            "cwd": acp.cwd,
            "mcpServers": [],
        })
    } else {
        json!({
            "cwd": acp.cwd,
            "mcpServers": [],
        })
    }
}

fn acp_auth_method(acp: &manifests::AcpRuntime) -> &'static str {
    if acp.env.contains_key("CODEX_API_KEY") {
        "codex-api-key"
    } else if acp.env.contains_key("OPENAI_API_KEY") {
        "openai-api-key"
    } else {
        "codex-api-key"
    }
}

fn acp_full_access_allowed(acp: &manifests::AcpRuntime) -> bool {
    acp.permission_policy.get("default").map(String::as_str) == Some("allow")
}

fn extract_session_id(value: &Value) -> Option<String> {
    value
        .get("sessionId")
        .or_else(|| value.get("session_id"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn is_session_update(message: &JsonRpcMessage) -> bool {
    message.method.as_deref() == Some("session/update")
        || message.params.get("update").is_some()
        || message.params.get("sessionUpdate").is_some()
}

async fn apply_session_update(params: Value, sink: &dyn ExecutionSink, reply: &mut String) {
    let update = params.get("update").unwrap_or(&params);
    if let Some(text) = extract_text(update).or_else(|| extract_text(&params)) {
        sink.on_token(&text).await;
        reply.push_str(&text);
    }
    if let Some(reasoning) = update
        .get("reasoning")
        .or_else(|| update.get("thought"))
        .or_else(|| {
            update.pointer("/content/text").filter(|_| {
                update.get("sessionUpdate").and_then(|value| value.as_str())
                    == Some("agent_thought_chunk")
            })
        })
        .and_then(|value| value.as_str())
        .or_else(|| {
            params
                .get("reasoning")
                .or_else(|| params.get("thought"))
                .and_then(|value| value.as_str())
        })
    {
        sink.on_reasoning(reasoning).await;
    }
    if let Some(error) = params.get("error").and_then(|value| value.as_str()) {
        sink.on_error(error).await;
    }
}

fn extract_text(value: &Value) -> Option<String> {
    value
        .pointer("/content/text")
        .filter(|_| {
            let is_agent_text = value
                .get("sessionUpdate")
                .and_then(|value| value.as_str())
                .map(|kind| kind == "agent_message_chunk")
                .unwrap_or(true);
            is_agent_text
        })
        .or_else(|| value.pointer("/agentMessageChunk/content/text"))
        .or_else(|| value.pointer("/agent_message_chunk/content/text"))
        .or_else(|| value.pointer("/delta/text"))
        .or_else(|| value.pointer("/message/content"))
        .or_else(|| value.get("text"))
        .or_else(|| value.get("content"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::sandbox::{
        DockerSandboxBackend, SandboxClassSpecJson, SandboxPolicySpecJson,
        SandboxPolicyTemplateJson, SandboxQuotaJson,
    };
    use crate::test_support::docker_test_guard;
    use tokio::io::AsyncReadExt;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn codex_acp_starts_inside_docker_sandbox_when_enabled() {
        if std::env::var("TALON_CODEX_ACP_TEST").ok().as_deref() != Some("1") {
            return;
        }
        load_dotenv_for_codex_smoke();
        let image = std::env::var("TALON_CODEX_ACP_IMAGE")
            .unwrap_or_else(|_| "talon-zed-codex-acp:local".into());
        let platform = std::env::var("TALON_CODEX_ACP_PLATFORM").ok();
        let command =
            std::env::var("TALON_CODEX_ACP_COMMAND").unwrap_or_else(|_| "codex-acp".into());
        let args = std::env::var("TALON_CODEX_ACP_ARGS")
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let _guard = docker_test_guard();
        let backend = DockerSandboxBackend::default();
        let mut provider_config = serde_json::json!({ "image": image });
        if let Some(platform) = platform {
            provider_config["platform"] = serde_json::Value::String(platform);
        }
        let class = SandboxClassSpecJson {
            provider: "docker".to_string(),
            provider_config,
            credentials: serde_json::json!({}),
        };
        let policy = SandboxPolicySpecJson {
            class_ref: Default::default(),
            template: SandboxPolicyTemplateJson {
                spec: serde_json::json!({
                    "image": image,
                    "workspace": { "mountPath": "/workspace" },
                    "filesystem": { "writable": ["/workspace", "/tmp"] }
                }),
            },
            quota: SandboxQuotaJson { max_concurrent: 1 },
        };
        let handle = backend.create(&class, &policy).await.unwrap();
        let test_result: Result<()> = async {
            let mut env = std::collections::HashMap::new();
            for key in ["CODEX_API_KEY", "OPENAI_API_KEY"] {
                if let Ok(value) = std::env::var(key) {
                    env.insert(key.to_string(), value);
                }
            }
            let acp = manifests::AcpRuntime {
                harness_ref: String::new(),
                command: command.clone(),
                args,
                cwd: "/workspace".to_string(),
                sandbox_policy_ref: "coding".to_string(),
                persist_session: false,
                env,
                permission_policy: Default::default(),
            };
            let mut child =
                acp_harness_command(&acp, &format!("docker:{}", handle.backend_id), &command)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()?;
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| anyhow!("ACP harness stdin unavailable"))?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| anyhow!("ACP harness stdout unavailable"))?;
            let mut stderr = child
                .stderr
                .take()
                .ok_or_else(|| anyhow!("ACP harness stderr unavailable"))?;
            let mut lines = BufReader::new(stdout).lines();
            send_request(&mut stdin, 1, "initialize", json!({"protocolVersion": 1})).await?;
            let line = timeout(Duration::from_secs(10), lines.next_line())
                .await
                .map_err(|_| anyhow!("timed out waiting for Codex ACP initialize response"))??;
            let Some(line) = line else {
                let mut stderr_text = String::new();
                let _ = stderr.read_to_string(&mut stderr_text).await;
                let status = child.wait().await?;
                return Err(anyhow!(
                    "Codex ACP exited before initialize response: {status}; stderr: {}",
                    stderr_text.trim()
                ));
            };
            let _message: JsonRpcMessage = serde_json::from_str(&line)
                .map_err(|err| anyhow!("Codex ACP emitted non-JSON-RPC line {line:?}: {err}"))?;
            let _ = child.kill().await;
            Ok(())
        }
        .await;
        let destroy_result = backend.destroy(&handle.backend_id).await;
        test_result.unwrap();
        destroy_result.unwrap();
    }

    fn load_dotenv_for_codex_smoke() {
        let Ok(contents) = std::fs::read_to_string(".env") else {
            return;
        };
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            if key != "OPENAI_API_KEY" && key != "CODEX_API_KEY" {
                continue;
            }
            if std::env::var_os(key).is_some() {
                continue;
            }
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if !value.is_empty() {
                std::env::set_var(key, value);
            }
        }
    }
}
