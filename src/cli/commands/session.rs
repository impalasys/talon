// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use futures::StreamExt;
use serde_json::json;
use std::collections::HashMap;

use super::{Cli, RunOutcome};
use crate::cli::connect_gateway;
use talon_client::data as data_proto;
use talon_client::events::{SessionMessagePartEvent, SessionMessagePartEventKind};
use talon_client::gateway::{
    AnswerSessionPermissionRequest, ClearSessionRequest, CreateSessionRequest,
    DeleteSessionRequest, GetSessionRequest, ListSessionMessagesRequest, ListSessionsRequest,
    SendMessageRequest, StopSessionGenerationRequest, StreamSessionPartsRequest,
};

#[derive(Args)]
pub(crate) struct SessionCommand {
    #[command(subcommand)]
    command: SessionCommands,
}

#[derive(Subcommand)]
enum SessionCommands {
    /// Create a session for an agent.
    Create {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        #[arg(long = "label", value_name = "KEY=VALUE")]
        labels: Vec<String>,
    },
    /// Create or reuse a session, send a prompt, and optionally stream the response.
    Prompt {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long = "label", value_name = "KEY=VALUE")]
        labels: Vec<String>,
        #[arg(long)]
        stream: bool,
        #[arg(long)]
        json: bool,
        #[arg(required = true)]
        message: Vec<String>,
    },
    /// Send a message to an existing session.
    Send {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        session_id: String,
        #[arg(long = "label", value_name = "KEY=VALUE")]
        labels: Vec<String>,
        #[arg(long)]
        stream: bool,
        #[arg(long)]
        json: bool,
        #[arg(required = true)]
        message: Vec<String>,
    },
    /// Stream live session updates.
    Stream {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        session_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Get one session.
    Get {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        session_id: String,
        /// 0 returns all messages; negative values return no messages.
        #[arg(long, default_value_t = 0)]
        message_limit: i32,
    },
    /// List sessions for an agent.
    List {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
    },
    /// Page session messages.
    Messages {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        session_id: String,
        #[arg(long, default_value_t = 50)]
        page_size: i32,
        #[arg(long)]
        before_message_id: Option<String>,
    },
    /// Answer or cancel a pending session permission request.
    Permission {
        #[command(subcommand)]
        command: PermissionCommands,
    },
    /// Ask the runtime to stop the current generation.
    Stop {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        session_id: String,
    },
    /// Clear all messages from a session.
    Clear {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        session_id: String,
    },
    /// Delete a session and its messages.
    Delete {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        session_id: String,
    },
}

#[derive(Subcommand)]
enum PermissionCommands {
    /// Select an option for a pending permission request.
    Answer {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        session_id: String,
        request_id: String,
        #[arg(long, default_value = "approved")]
        option_id: String,
        #[arg(long, default_value = "user")]
        decided_by: String,
    },
    /// Cancel a pending permission request.
    Cancel {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        session_id: String,
        request_id: String,
        #[arg(long, default_value = "user")]
        decided_by: String,
    },
}

pub(super) async fn run(cli: &Cli, command: &SessionCommand) -> Result<RunOutcome> {
    match &command.command {
        SessionCommands::Create {
            namespace,
            agent,
            labels,
        } => {
            let labels = parse_labels(labels)?;
            let value = session_create(cli, namespace, agent, labels).await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        SessionCommands::Prompt {
            namespace,
            agent,
            session_id,
            labels,
            stream,
            json,
            message,
        } => {
            let labels = parse_labels(labels)?;
            let session_id = match session_id {
                Some(session_id) => session_id.clone(),
                None => {
                    let created = session_create(cli, namespace, agent, labels.clone()).await?;
                    created
                        .get("sessionId")
                        .and_then(|value| value.as_str())
                        .context("CreateSession response missing sessionId")?
                        .to_string()
                }
            };
            session_send(
                cli,
                namespace,
                agent,
                &session_id,
                &message.join(" "),
                labels,
                *stream,
                *json,
            )
            .await?;
        }
        SessionCommands::Send {
            namespace,
            agent,
            session_id,
            labels,
            stream,
            json,
            message,
        } => {
            session_send(
                cli,
                namespace,
                agent,
                session_id,
                &message.join(" "),
                parse_labels(labels)?,
                *stream,
                *json,
            )
            .await?;
        }
        SessionCommands::Stream {
            namespace,
            agent,
            session_id,
            json,
        } => {
            session_stream(cli, namespace, agent, session_id, *json).await?;
        }
        SessionCommands::Get {
            namespace,
            agent,
            session_id,
            message_limit,
        } => {
            let value = session_get(cli, namespace, agent, session_id, *message_limit).await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        SessionCommands::List { namespace, agent } => {
            let value = session_list(cli, namespace, agent).await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        SessionCommands::Messages {
            namespace,
            agent,
            session_id,
            page_size,
            before_message_id,
        } => {
            let value = session_messages(
                cli,
                namespace,
                agent,
                session_id,
                *page_size,
                before_message_id,
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        SessionCommands::Permission { command } => match command {
            PermissionCommands::Answer {
                namespace,
                agent,
                session_id,
                request_id,
                option_id,
                decided_by,
            } => {
                let value = session_answer_permission(
                    cli, namespace, agent, session_id, request_id, "selected", option_id,
                    decided_by,
                )
                .await?;
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            PermissionCommands::Cancel {
                namespace,
                agent,
                session_id,
                request_id,
                decided_by,
            } => {
                let value = session_answer_permission(
                    cli,
                    namespace,
                    agent,
                    session_id,
                    request_id,
                    "cancelled",
                    "",
                    decided_by,
                )
                .await?;
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
        },
        SessionCommands::Stop {
            namespace,
            agent,
            session_id,
        } => {
            let value = session_stop(cli, namespace, agent, session_id).await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        SessionCommands::Clear {
            namespace,
            agent,
            session_id,
        } => {
            let value = session_clear(cli, namespace, agent, session_id).await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        SessionCommands::Delete {
            namespace,
            agent,
            session_id,
        } => {
            let value = session_delete(cli, namespace, agent, session_id).await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
    }

    Ok(RunOutcome { exit_code: None })
}

fn parse_labels(labels: &[String]) -> Result<HashMap<String, String>> {
    labels
        .iter()
        .map(|entry| {
            let (key, value) = entry
                .split_once('=')
                .with_context(|| format!("label must be KEY=VALUE, got {entry:?}"))?;
            let key = key.trim();
            if key.is_empty() {
                anyhow::bail!("label key cannot be empty");
            }
            Ok((key.to_string(), value.to_string()))
        })
        .collect()
}

async fn session_create(
    cli: &Cli,
    namespace: &str,
    agent: &str,
    labels: HashMap<String, String>,
) -> Result<serde_json::Value> {
    let mut client = connect_gateway(cli).await?;
    let response = client
        .create_session(CreateSessionRequest {
            agent: agent.to_string(),
            ns: namespace.to_string(),
            labels,
        })
        .await
        .with_context(|| format!("Failed to create session for {namespace}/{agent}"))?
        .into_inner();
    Ok(session_response_json(&response))
}

async fn session_send(
    cli: &Cli,
    namespace: &str,
    agent: &str,
    session_id: &str,
    message: &str,
    labels: HashMap<String, String>,
    stream: bool,
    json_output: bool,
) -> Result<()> {
    if stream {
        let mut stream_client = connect_gateway(cli).await?;
        let mut events = stream_client
            .stream_session_parts(StreamSessionPartsRequest {
                session_id: session_id.to_string(),
                agent: agent.to_string(),
                ns: namespace.to_string(),
            })
            .await
            .with_context(|| format!("Failed to stream session {namespace}/{agent}/{session_id}"))?
            .into_inner();

        send_message(cli, namespace, agent, session_id, message, labels).await?;

        while let Some(event) = events.next().await {
            let event = event.context("Session stream failed")?;
            print_session_event(&event, json_output)?;
            if event.kind == SessionMessagePartEventKind::Done as i32
                || event.kind == SessionMessagePartEventKind::Error as i32
            {
                break;
            }
        }
        return Ok(());
    }

    let value = send_message(cli, namespace, agent, session_id, message, labels).await?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

async fn send_message(
    cli: &Cli,
    namespace: &str,
    agent: &str,
    session_id: &str,
    message: &str,
    labels: HashMap<String, String>,
) -> Result<serde_json::Value> {
    let mut client = connect_gateway(cli).await?;
    let response = client
        .send_message(SendMessageRequest {
            session_id: session_id.to_string(),
            agent: agent.to_string(),
            ns: namespace.to_string(),
            message: message.to_string(),
            labels,
        })
        .await
        .with_context(|| format!("Failed to send message to {namespace}/{agent}/{session_id}"))?
        .into_inner();
    Ok(json!({
        "sessionId": response.session_id,
        "reply": response.reply,
    }))
}

async fn session_stream(
    cli: &Cli,
    namespace: &str,
    agent: &str,
    session_id: &str,
    json_output: bool,
) -> Result<()> {
    let mut client = connect_gateway(cli).await?;
    let mut events = client
        .stream_session_parts(StreamSessionPartsRequest {
            session_id: session_id.to_string(),
            agent: agent.to_string(),
            ns: namespace.to_string(),
        })
        .await
        .with_context(|| format!("Failed to stream session {namespace}/{agent}/{session_id}"))?
        .into_inner();

    while let Some(event) = events.next().await {
        let event = event.context("Session stream failed")?;
        print_session_event(&event, json_output)?;
        if event.kind == SessionMessagePartEventKind::Done as i32
            || event.kind == SessionMessagePartEventKind::Error as i32
        {
            break;
        }
    }
    Ok(())
}

async fn session_get(
    cli: &Cli,
    namespace: &str,
    agent: &str,
    session_id: &str,
    message_limit: i32,
) -> Result<serde_json::Value> {
    let mut client = connect_gateway(cli).await?;
    let response = client
        .get_session(GetSessionRequest {
            session_id: session_id.to_string(),
            agent: agent.to_string(),
            ns: namespace.to_string(),
            message_limit,
        })
        .await
        .with_context(|| format!("Failed to get session {namespace}/{agent}/{session_id}"))?
        .into_inner();
    Ok(session_response_json(&response))
}

async fn session_list(cli: &Cli, namespace: &str, agent: &str) -> Result<serde_json::Value> {
    let mut client = connect_gateway(cli).await?;
    let response = client
        .list_sessions(ListSessionsRequest {
            agent: agent.to_string(),
            ns: namespace.to_string(),
        })
        .await
        .with_context(|| format!("Failed to list sessions for {namespace}/{agent}"))?
        .into_inner();
    Ok(json!({
        "sessionIds": response.session_ids,
        "sessions": response.sessions.into_iter().map(|session| {
            json!({
                "sessionId": session.session_id,
                "updatedAt": session.updated_at,
                "labels": session.labels,
            })
        }).collect::<Vec<_>>(),
    }))
}

async fn session_messages(
    cli: &Cli,
    namespace: &str,
    agent: &str,
    session_id: &str,
    page_size: i32,
    before_message_id: &Option<String>,
) -> Result<serde_json::Value> {
    let mut client = connect_gateway(cli).await?;
    let response = client
        .list_session_messages(ListSessionMessagesRequest {
            session_id: session_id.to_string(),
            agent: agent.to_string(),
            ns: namespace.to_string(),
            page_size,
            before_message_id: before_message_id.clone(),
        })
        .await
        .with_context(|| format!("Failed to list messages for {namespace}/{agent}/{session_id}"))?
        .into_inner();
    Ok(json!({
        "sessionId": response.session_id,
        "agent": response.agent,
        "state": response.state,
        "items": response.items.into_iter().filter_map(|item| {
            item.message.as_ref().map(session_message_json)
        }).collect::<Vec<_>>(),
        "hasMore": response.has_more,
        "nextBeforeMessageId": response.next_before_message_id,
    }))
}

async fn session_answer_permission(
    cli: &Cli,
    namespace: &str,
    agent: &str,
    session_id: &str,
    request_id: &str,
    outcome: &str,
    option_id: &str,
    decided_by: &str,
) -> Result<serde_json::Value> {
    let mut client = connect_gateway(cli).await?;
    let response = client
        .answer_session_permission(AnswerSessionPermissionRequest {
            session_id: session_id.to_string(),
            agent: agent.to_string(),
            ns: namespace.to_string(),
            request_id: request_id.to_string(),
            outcome: outcome.to_string(),
            option_id: option_id.to_string(),
            decided_by: decided_by.to_string(),
        })
        .await
        .with_context(|| {
            format!("Failed to answer permission {request_id} for {namespace}/{agent}/{session_id}")
        })?
        .into_inner();
    Ok(json!({
        "sessionId": response.session_id,
        "requestId": response.request_id,
        "outcome": response.outcome,
        "optionId": response.option_id,
    }))
}

async fn session_stop(
    cli: &Cli,
    namespace: &str,
    agent: &str,
    session_id: &str,
) -> Result<serde_json::Value> {
    let mut client = connect_gateway(cli).await?;
    let response = client
        .stop_session_generation(StopSessionGenerationRequest {
            session_id: session_id.to_string(),
            agent: agent.to_string(),
            ns: namespace.to_string(),
        })
        .await
        .with_context(|| format!("Failed to stop session {namespace}/{agent}/{session_id}"))?
        .into_inner();
    Ok(json!({ "success": response.success }))
}

async fn session_clear(
    cli: &Cli,
    namespace: &str,
    agent: &str,
    session_id: &str,
) -> Result<serde_json::Value> {
    let mut client = connect_gateway(cli).await?;
    let response = client
        .clear_session(ClearSessionRequest {
            session_id: session_id.to_string(),
            agent: agent.to_string(),
            ns: namespace.to_string(),
        })
        .await
        .with_context(|| format!("Failed to clear session {namespace}/{agent}/{session_id}"))?
        .into_inner();
    Ok(json!({ "success": response.success }))
}

async fn session_delete(
    cli: &Cli,
    namespace: &str,
    agent: &str,
    session_id: &str,
) -> Result<serde_json::Value> {
    let mut client = connect_gateway(cli).await?;
    let response = client
        .delete_session(DeleteSessionRequest {
            session_id: session_id.to_string(),
            agent: agent.to_string(),
            ns: namespace.to_string(),
        })
        .await
        .with_context(|| format!("Failed to delete session {namespace}/{agent}/{session_id}"))?
        .into_inner();
    Ok(json!({ "success": response.success }))
}

fn session_response_json(
    response: &talon_client::gateway::SessionResponse,
) -> serde_json::Value {
    json!({
        "sessionId": response.session_id,
        "agent": response.agent,
        "state": response.state,
        "labels": response.labels,
        "messages": response.messages.iter().map(session_message_json).collect::<Vec<_>>(),
    })
}

fn session_message_json(message: &data_proto::SessionMessage) -> serde_json::Value {
    json!({
        "id": message.id,
        "role": role_name(message.role),
        "createdAt": message.created_at,
        "labels": message.labels,
        "parts": message.parts.iter().map(session_part_json).collect::<Vec<_>>(),
    })
}

fn session_part_json(part: &data_proto::SessionMessagePart) -> serde_json::Value {
    json!({
        "id": part.id,
        "type": part_type_name(part.part_type),
        "content": part.content,
        "name": part.name,
        "payload": parse_json_field(&part.payload_json),
        "createdAt": part.created_at,
        "object": part.object.as_ref().map(|object| json!({
            "key": object.key,
            "mediaType": object.media_type,
            "sizeBytes": object.size_bytes,
            "sha256": object.sha256,
            "filename": object.filename,
            "metadata": object.metadata,
        })),
    })
}

fn print_session_event(
    event: &SessionMessagePartEvent,
    json_output: bool,
) -> Result<()> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "sessionId": event.session_id,
                "kind": event_kind_name(event.kind),
                "agent": event.agent,
                "namespace": event.ns,
                "messageId": event.message_id,
                "timestamp": event.timestamp,
                "part": event.part.as_ref().map(session_part_json),
            }))?
        );
        return Ok(());
    }

    if let Some(part) = &event.part {
        match data_proto::SessionMessagePartType::try_from(part.part_type).ok() {
            Some(data_proto::SessionMessagePartType::Text) => {
                print!("{}", part.content);
            }
            Some(data_proto::SessionMessagePartType::Reasoning) => {
                eprint!("{}", part.content);
            }
            Some(data_proto::SessionMessagePartType::Error) => {
                eprintln!("{}", part.content);
            }
            Some(data_proto::SessionMessagePartType::ToolCall)
            | Some(data_proto::SessionMessagePartType::ToolResult) => {
                eprintln!(
                    "\n[{} {}] {}",
                    part_type_name(part.part_type),
                    part.name,
                    part.payload_json
                );
            }
            Some(data_proto::SessionMessagePartType::RequestPermission)
            | Some(data_proto::SessionMessagePartType::PermissionResult) => {
                eprintln!(
                    "\n[{}] {}",
                    part_type_name(part.part_type),
                    part.payload_json
                );
            }
            _ => {}
        }
    }
    if event.kind == SessionMessagePartEventKind::Done as i32 {
        println!();
    }
    Ok(())
}

fn parse_json_field(value: &str) -> serde_json::Value {
    if value.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(value).unwrap_or_else(|_| serde_json::Value::String(value.to_string()))
    }
}

fn role_name(role: i32) -> &'static str {
    match data_proto::MessageRole::try_from(role).ok() {
        Some(data_proto::MessageRole::RoleUser) => "user",
        Some(data_proto::MessageRole::RoleAssistant) => "assistant",
        Some(data_proto::MessageRole::RoleSystem) => "system",
        _ => "unspecified",
    }
}

fn part_type_name(part_type: i32) -> &'static str {
    match data_proto::SessionMessagePartType::try_from(part_type).ok() {
        Some(data_proto::SessionMessagePartType::Text) => "text",
        Some(data_proto::SessionMessagePartType::Reasoning) => "reasoning",
        Some(data_proto::SessionMessagePartType::ToolCall) => "tool_call",
        Some(data_proto::SessionMessagePartType::ToolResult) => "tool_result",
        Some(data_proto::SessionMessagePartType::Usage) => "usage",
        Some(data_proto::SessionMessagePartType::Error) => "error",
        Some(data_proto::SessionMessagePartType::Image) => "image",
        Some(data_proto::SessionMessagePartType::Audio) => "audio",
        Some(data_proto::SessionMessagePartType::Video) => "video",
        Some(data_proto::SessionMessagePartType::File) => "file",
        Some(data_proto::SessionMessagePartType::RequestPermission) => "request_permission",
        Some(data_proto::SessionMessagePartType::PermissionResult) => "permission_result",
        _ => "unspecified",
    }
}

fn event_kind_name(kind: i32) -> &'static str {
    match SessionMessagePartEventKind::try_from(kind).ok() {
        Some(SessionMessagePartEventKind::Delta) => "delta",
        Some(SessionMessagePartEventKind::Done) => "done",
        Some(SessionMessagePartEventKind::Error) => "error",
        _ => "unspecified",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_labels_requires_key_value_pairs() {
        assert_eq!(
            parse_labels(&["source=cli".to_string()]).unwrap(),
            HashMap::from([("source".to_string(), "cli".to_string())])
        );
        assert!(parse_labels(&["broken".to_string()]).is_err());
    }

    #[test]
    fn session_message_json_renders_text_parts() {
        let message = data_proto::SessionMessage {
            id: "m1".to_string(),
            role: data_proto::MessageRole::RoleAssistant as i32,
            created_at: 123,
            labels: HashMap::new(),
            parts: vec![data_proto::SessionMessagePart {
                id: "p1".to_string(),
                part_type: data_proto::SessionMessagePartType::Text as i32,
                content: "hello".to_string(),
                name: String::new(),
                payload_json: String::new(),
                created_at: 123,
                object: None,
            }],
        };

        let value = session_message_json(&message);

        assert_eq!(value["role"], "assistant");
        assert_eq!(value["parts"][0]["type"], "text");
        assert_eq!(value["parts"][0]["content"], "hello");
    }
}
