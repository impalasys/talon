// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

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
        let method = message.method.as_deref().unwrap_or_default();
        match method {
            "initialize" => {
                write_response(
                    &mut stdout,
                    id,
                    json!({
                        "protocolVersion": 1,
                        "agent": {
                            "name": "talon-mock-acp",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "capabilities": {
                            "session": true,
                            "fs": true,
                            "terminal": true,
                            "permissions": true
                        }
                    }),
                )
                .await?;
            }
            "session/new" | "session/load" => {
                let session_id = session_id(&message.params);
                write_response(&mut stdout, id, json!({ "sessionId": session_id })).await?;
            }
            "session/prompt" => {
                let prompt = prompt_text(&message.params);
                let session_id = session_id(&message.params);
                let mut file_content = String::new();
                let mut terminal_stdout = String::new();
                if prompt.contains("request-permission") {
                    write_request(
                        &mut stdout,
                        1001,
                        "permission/request",
                        json!({
                            "sessionId": session_id,
                            "action": "terminal",
                            "prompt": "Mock ACP permission request"
                        }),
                    )
                    .await?;
                    let _ = next_response(&mut lines).await?;
                }
                if prompt.contains("write-file") {
                    write_request(
                        &mut stdout,
                        1002,
                        "fs/write_text_file",
                        json!({
                            "sessionId": session_id,
                            "path": "/workspace/mock-acp.txt",
                            "content": "written by talon-mock-acp"
                        }),
                    )
                    .await?;
                    let _ = next_response(&mut lines).await?;
                }
                if prompt.contains("read-file") {
                    write_request(
                        &mut stdout,
                        1004,
                        "fs/read_text_file",
                        json!({
                            "sessionId": session_id,
                            "path": "/workspace/mock-acp.txt"
                        }),
                    )
                    .await?;
                    if let Some(response) = next_response(&mut lines).await? {
                        file_content = response
                            .result
                            .get("content")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default()
                            .to_string();
                    }
                }
                if prompt.contains("terminal") {
                    write_request(
                        &mut stdout,
                        1003,
                        "terminal/exec",
                        json!({
                            "sessionId": session_id,
                            "command": "printf terminal-ok"
                        }),
                    )
                    .await?;
                    if let Some(response) = next_response(&mut lines).await? {
                        terminal_stdout = response
                            .result
                            .get("stdout")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default()
                            .to_string();
                    }
                }
                let observed = format!(
                    "mock response file={} terminal={}",
                    file_content, terminal_stdout
                );
                write_notification(
                    &mut stdout,
                    "session/update",
                    json!({
                        "sessionId": session_id,
                        "reasoning": "mock reasoning",
                        "text": observed
                    }),
                )
                .await?;
                write_response(&mut stdout, id, json!({ "text": observed })).await?;
            }
            "session/cancel" => {
                write_response(&mut stdout, id, json!({ "cancelled": true })).await?;
            }
            _ => {
                write_error(&mut stdout, id, -32601, &format!("unknown method {method}")).await?;
            }
        }
    }
    Ok(())
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
        .unwrap_or("mock-session")
        .to_string()
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
