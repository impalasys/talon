// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

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
}

#[derive(Debug, Deserialize)]
struct CodexJsonLine {
    #[serde(rename = "type")]
    type_: String,
    #[serde(default)]
    item: Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut lines = BufReader::new(stdin).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let message: JsonRpcMessage = serde_json::from_str(&line)?;
        let Some(id) = message.id else {
            continue;
        };
        match message.method.as_deref().unwrap_or_default() {
            "initialize" => {
                write_response(
                    &mut stdout,
                    id,
                    json!({
                        "protocolVersion": 1,
                        "agent": {
                            "name": "talon-codex-acp",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "capabilities": {
                            "session": true,
                            "fs": true,
                            "terminal": true
                        }
                    }),
                )
                .await?;
            }
            "session/new" | "session/load" => {
                write_response(
                    &mut stdout,
                    id,
                    json!({ "sessionId": session_id(&message.params) }),
                )
                .await?;
            }
            "session/prompt" => {
                let session_id = session_id(&message.params);
                let prompt = prompt_text(&message.params);
                let cwd = message
                    .params
                    .get("cwd")
                    .and_then(|value| value.as_str())
                    .unwrap_or(".");
                let codex_text = strip_code_fence(&run_codex(&prompt, cwd).await?);
                let file_path = std::env::var("TALON_CODEX_ACP_FILE_PATH")
                    .unwrap_or_else(|_| "/workspace/codex-acp.txt".to_string());
                write_request(
                    &mut stdout,
                    2001,
                    "fs/write_text_file",
                    json!({
                        "sessionId": session_id,
                        "path": &file_path,
                        "content": &codex_text
                    }),
                )
                .await?;
                let _ = next_response(&mut lines).await?;
                write_request(
                    &mut stdout,
                    2002,
                    "fs/read_text_file",
                    json!({
                        "sessionId": session_id,
                        "path": &file_path
                    }),
                )
                .await?;
                let file_content = next_response(&mut lines)
                    .await?
                    .and_then(|response| {
                        response
                            .result
                            .get("content")
                            .and_then(|value| value.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_default();
                let terminal_command = std::env::var("TALON_CODEX_ACP_TERMINAL_COMMAND")
                    .unwrap_or_else(|_| "printf codex-terminal-ok".to_string());
                write_request(
                    &mut stdout,
                    2003,
                    "terminal/exec",
                    json!({
                        "sessionId": session_id,
                        "command": terminal_command
                    }),
                )
                .await?;
                let terminal_stdout = next_response(&mut lines)
                    .await?
                    .and_then(|response| {
                        response
                            .result
                            .get("stdout")
                            .and_then(|value| value.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_default();
                let observed = format!(
                    "codex response={} file={} terminal={}",
                    single_line(&codex_text),
                    single_line(&file_content),
                    single_line(&terminal_stdout)
                );
                write_notification(
                    &mut stdout,
                    "session/update",
                    json!({
                        "sessionId": session_id,
                        "reasoning": "codex exec completed and sandbox effects were inspected",
                        "text": observed
                    }),
                )
                .await?;
                write_response(&mut stdout, id, json!({ "text": observed })).await?;
            }
            "session/cancel" => {
                write_response(&mut stdout, id, json!({ "cancelled": true })).await?;
            }
            method => {
                write_error(&mut stdout, id, -32601, &format!("unknown method {method}")).await?;
            }
        }
    }
    Ok(())
}

async fn run_codex(prompt: &str, cwd: &str) -> Result<String> {
    let command = std::env::var("TALON_CODEX_COMMAND").unwrap_or_else(|_| "codex".to_string());
    let timeout_seconds = std::env::var("TALON_CODEX_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(120);
    let codex_prompt = format!(
        "{prompt}\n\nRespond with the exact requested final answer only. Do not include markdown."
    );
    let mut args = vec![
        "exec",
        "--json",
        "--skip-git-repo-check",
        "--ephemeral",
        "--dangerously-bypass-approvals-and-sandbox",
        "-C",
        cwd,
        "-",
    ];
    let model = std::env::var("TALON_CODEX_MODEL").ok();
    if let Some(model) = model.as_deref().filter(|value| !value.trim().is_empty()) {
        args.splice(1..1, ["-m", model]);
    }
    let mut codex = Command::new(command);
    codex.args(args);
    if std::env::var_os("CODEX_API_KEY").is_none() {
        if let Some(openai_api_key) = std::env::var_os("OPENAI_API_KEY") {
            codex.env("CODEX_API_KEY", openai_api_key);
        }
    }
    let mut child = codex
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("codex stdin unavailable"))?;
    stdin.write_all(codex_prompt.as_bytes()).await?;
    drop(stdin);
    let output = match timeout(
        Duration::from_secs(timeout_seconds),
        child.wait_with_output(),
    )
    .await
    {
        Ok(output) => output?,
        Err(_) => return Err(anyhow!("codex exec timed out after {timeout_seconds}s")),
    };
    if !output.status.success() {
        return Err(anyhow!(
            "codex exec failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut final_text = String::new();
    for line in stdout.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let Ok(event) = serde_json::from_str::<CodexJsonLine>(line) else {
            continue;
        };
        if event.type_ == "item.completed"
            && event.item.get("type").and_then(|value| value.as_str()) == Some("agent_message")
        {
            if let Some(text) = event.item.get("text").and_then(|value| value.as_str()) {
                final_text = text.trim().to_string();
            }
        }
    }
    if final_text.is_empty() {
        return Err(anyhow!("codex exec produced no agent_message"));
    }
    Ok(final_text)
}

fn prompt_text(params: &Value) -> String {
    let Some(prompt) = params.get("prompt") else {
        return String::new();
    };
    if let Some(text) = prompt.as_str() {
        return text.to_string();
    }
    prompt
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| {
                    block
                        .get("text")
                        .or_else(|| block.get("content"))
                        .and_then(|value| value.as_str())
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

fn session_id(params: &Value) -> String {
    params
        .get("sessionId")
        .and_then(|value| value.as_str())
        .unwrap_or("codex-session")
        .to_string()
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_code_fence(value: &str) -> String {
    let trimmed = value.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed.to_string();
    };
    let Some(end) = rest.rfind("```") else {
        return trimmed.to_string();
    };
    let inner = &rest[..end];
    let inner = match inner.split_once('\n') {
        Some((_, body)) => body,
        None => inner,
    };
    inner.trim().to_string()
}

async fn next_response(
    lines: &mut tokio::io::Lines<BufReader<tokio::io::Stdin>>,
) -> Result<Option<JsonRpcMessage>> {
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let message: JsonRpcMessage = serde_json::from_str(&line)?;
        if message.method.is_none() && !message.result.is_null() {
            return Ok(Some(message));
        }
    }
    Ok(None)
}

async fn write_response(stdout: &mut tokio::io::Stdout, id: Value, result: Value) -> Result<()> {
    write_json(
        stdout,
        json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    )
    .await
}

async fn write_error(
    stdout: &mut tokio::io::Stdout,
    id: Value,
    code: i64,
    message: &str,
) -> Result<()> {
    write_json(
        stdout,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message }
        }),
    )
    .await
}

async fn write_request(
    stdout: &mut tokio::io::Stdout,
    id: u64,
    method: &str,
    params: Value,
) -> Result<()> {
    write_json(
        stdout,
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
    )
    .await
}

async fn write_notification(
    stdout: &mut tokio::io::Stdout,
    method: &str,
    params: Value,
) -> Result<()> {
    write_json(
        stdout,
        json!({ "jsonrpc": "2.0", "method": method, "params": params }),
    )
    .await
}

async fn write_json(stdout: &mut tokio::io::Stdout, value: Value) -> Result<()> {
    stdout
        .write_all(serde_json::to_string(&value)?.as_bytes())
        .await?;
    stdout.write_all(b"\n").await?;
    stdout.flush().await?;
    Ok(())
}
