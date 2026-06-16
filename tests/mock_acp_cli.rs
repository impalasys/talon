// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

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

#[tokio::test]
async fn mock_acp_cli_handles_initialize_and_prompt() -> Result<()> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_talon-mock-acp"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("mock ACP stdin should be piped");
    let stdout = child
        .stdout
        .take()
        .expect("mock ACP stdout should be piped");
    let mut lines = BufReader::new(stdout).lines();

    send_request(&mut stdin, 1, "initialize", json!({ "protocolVersion": 1 })).await?;
    let initialize = read_message(&mut lines).await?;
    assert_eq!(initialize.id.as_ref().and_then(|id| id.as_u64()), Some(1));
    assert_eq!(
        initialize
            .result
            .pointer("/agent/name")
            .and_then(|value| value.as_str()),
        Some("talon-mock-acp")
    );

    send_request(
        &mut stdin,
        2,
        "session/new",
        json!({ "sessionId": "session-1" }),
    )
    .await?;
    let new_session = read_message(&mut lines).await?;
    assert_eq!(new_session.id.as_ref().and_then(|id| id.as_u64()), Some(2));

    send_request(
        &mut stdin,
        3,
        "session/prompt",
        json!({ "sessionId": "session-1", "prompt": "hello" }),
    )
    .await?;
    let update = read_message(&mut lines).await?;
    assert_eq!(update.method.as_deref(), Some("session/update"));
    assert_eq!(
        update.params.get("text").and_then(|value| value.as_str()),
        Some("mock response file= terminal=")
    );
    let response = read_message(&mut lines).await?;
    assert_eq!(response.id.as_ref().and_then(|id| id.as_u64()), Some(3));
    assert_eq!(
        response.result.get("text").and_then(|value| value.as_str()),
        Some("mock response file= terminal=")
    );

    let _ = child.kill().await;
    Ok(())
}

async fn send_request(
    stdin: &mut tokio::process::ChildStdin,
    id: u64,
    method: &str,
    params: Value,
) -> Result<()> {
    let line = serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    }))?;
    stdin.write_all(line.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    Ok(())
}

async fn read_message(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
) -> Result<JsonRpcMessage> {
    let line = lines
        .next_line()
        .await?
        .expect("mock ACP should emit a JSON-RPC line");
    Ok(serde_json::from_str(&line)?)
}
