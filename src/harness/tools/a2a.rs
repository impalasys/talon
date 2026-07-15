// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use prost::Message;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

use crate::control::{keys, scheduling, ControlPlane, ProtoKeyValueStoreExt};
use crate::gateway::rpc::data_proto;
use crate::gateway::rpc::manifests;
use crate::harness::skills::registry::ToolRegistry;

const DEFAULT_ASK_AGENT_TIMEOUT_SECONDS: u64 = 180;
const MAX_ASK_AGENT_TIMEOUT_SECONDS: u64 = 600;
const POLL_INTERVAL_MILLIS: u64 = 250;

pub(super) fn register(registry: &mut ToolRegistry, spec: &manifests::AgentSpec) {
    let internal_connections = crate::harness::a2a::internal_connection_names(spec);
    if internal_connections.is_empty() {
        return;
    }

    registry.register_builtin(
        super::ASK_AGENT_TOOL,
        "Ask a declared A2A internal connection for a bounded synchronous reply without creating a Task.",
        json!({
            "type": "object",
            "properties": {
                "connection": {
                    "type": "string",
                    "description": "Declared A2A internal connection name to ask.",
                    "enum": internal_connections
                },
                "prompt": {
                    "type": "string",
                    "description": "Question, review request, or brief for the target agent."
                },
                "artifact_uri": {
                    "type": "string",
                    "description": "Optional artifact:// URI to grant to the target session and include in the prompt."
                },
                "artifact_uris": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional artifact:// URIs to grant to the target session and include in the prompt."
                },
                "timeout_seconds": {
                    "type": "integer",
                    "description": "Maximum seconds to wait for the target reply. Defaults to 180, max 600."
                }
            },
            "required": ["connection", "prompt"]
        }),
    );
}

pub(super) async fn execute(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    name: &str,
    args: &Value,
) -> Result<Option<String>> {
    match name {
        super::ASK_AGENT_TOOL => ask_agent(
            cp,
            current_namespace,
            current_agent,
            current_session,
            spec,
            args,
        )
        .await
        .map(Some),
        _ => Ok(None),
    }
}

async fn ask_agent(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    args: &Value,
) -> Result<String> {
    let connection_name = super::req_str(args, "connection")?;
    let target = crate::harness::a2a::resolve_internal_connection(spec, connection_name)?;
    let prompt = super::req_str(args, "prompt")?;
    let timeout_seconds = args
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_ASK_AGENT_TIMEOUT_SECONDS)
        .clamp(1, MAX_ASK_AGENT_TIMEOUT_SECONDS);
    let artifact_uris = requested_artifact_uris(args)?;

    let labels = HashMap::from([
        (
            crate::harness::a2a::LABEL_A2A_CALL.to_string(),
            "true".to_string(),
        ),
        (
            crate::harness::a2a::LABEL_CALLER_NAMESPACE.to_string(),
            current_namespace.to_string(),
        ),
        (
            crate::harness::a2a::LABEL_CALLER_NAME.to_string(),
            current_agent.to_string(),
        ),
        (
            crate::harness::a2a::LABEL_CALLER_SESSION_ID.to_string(),
            current_session.to_string(),
        ),
        (
            crate::harness::a2a::LABEL_A2A_CONNECTION.to_string(),
            target.connection_name.clone(),
        ),
    ]);
    let child_session_id = scheduling::create_session_with_labels(
        cp,
        &target.target_namespace,
        &target.target_agent,
        labels.clone(),
    )
    .await?;

    for artifact_uri in &artifact_uris {
        grant_artifact_to_child_session(
            cp,
            current_agent,
            current_session,
            artifact_uri,
            &target.target_agent,
            &child_session_id,
        )
        .await?;
    }

    let message = ask_agent_message(prompt, &artifact_uris);
    let submission_id = scheduling::send_message(
        cp.kv.as_ref(),
        cp.pubsub.as_ref(),
        &target.target_namespace,
        &target.target_agent,
        &child_session_id,
        &message,
        labels,
        chrono::Utc::now(),
    )
    .await?;

    let submission = wait_for_submission(
        cp,
        &target.target_namespace,
        &target.target_agent,
        &child_session_id,
        &submission_id,
        Duration::from_secs(timeout_seconds),
    )
    .await?;
    if submission.status != data_proto::SessionSubmissionStatus::Committed as i32 {
        return Err(anyhow!(
            "ask_agent target session ended without a committed reply: status={}",
            submission.status
        ));
    }

    let response_text = latest_assistant_text(
        cp,
        &target.target_namespace,
        &target.target_agent,
        &child_session_id,
    )
    .await?
    .unwrap_or_default();
    let response_artifact_uris = artifact_uris_from_text(&response_text);
    Ok(serde_json::to_string_pretty(&json!({
        "connection": target.connection_name,
        "namespace": target.target_namespace,
        "agent": target.target_agent,
        "sessionId": child_session_id,
        "submissionId": submission_id,
        "response": response_text,
        "artifactUris": response_artifact_uris
    }))?)
}

fn requested_artifact_uris(args: &Value) -> Result<Vec<String>> {
    let mut uris = Vec::new();
    if let Some(uri) = super::opt_str(args, "artifact_uri") {
        uris.push(uri.to_string());
    }
    if let Some(values) = args.get("artifact_uris") {
        let Some(values) = values.as_array() else {
            return Err(anyhow!("artifact_uris must be an array"));
        };
        for value in values {
            let Some(uri) = value.as_str() else {
                return Err(anyhow!("artifact_uris must contain strings"));
            };
            uris.push(uri.to_string());
        }
    }
    uris.sort();
    uris.dedup();
    Ok(uris)
}

async fn grant_artifact_to_child_session(
    cp: &ControlPlane,
    current_agent: &str,
    current_session: &str,
    artifact_uri: &str,
    target_agent: &str,
    target_session_id: &str,
) -> Result<()> {
    let (uri, _) = super::resolve_artifact_uri(
        cp,
        current_agent,
        current_session,
        artifact_uri,
        super::OP_READ,
    )
    .await?;
    let access = data_proto::ArtifactAccess {
        target_agent: target_agent.to_string(),
        target_session_id: target_session_id.to_string(),
        operations: vec![super::OP_READ.to_string(), super::OP_METADATA.to_string()],
        expires_at: super::default_access_expiry(),
        granted_by_agent: current_agent.to_string(),
        granted_by_session_id: current_session.to_string(),
        created_at: chrono::Utc::now().timestamp_micros(),
    };
    cp.kv
        .set_msg(
            &keys::artifact_access(
                &uri.namespace,
                &uri.agent,
                &uri.session_id,
                &uri.artifact_id,
                target_agent,
                target_session_id,
            ),
            &access,
        )
        .await?;
    Ok(())
}

fn ask_agent_message(prompt: &str, artifact_uris: &[String]) -> String {
    if artifact_uris.is_empty() {
        return prompt.to_string();
    }
    let artifacts = artifact_uris
        .iter()
        .map(|uri| format!("- {uri}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{prompt}\n\nContext artifact URIs granted to this session:\n{artifacts}\n\nUse read_artifact with the exact URI when you need to inspect an artifact."
    )
}

async fn wait_for_submission(
    cp: &ControlPlane,
    namespace: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
    timeout: Duration,
) -> Result<data_proto::SessionSubmission> {
    let start = tokio::time::Instant::now();
    loop {
        let submission = cp
            .kv
            .get_msg::<data_proto::SessionSubmission>(&keys::session_submission(
                namespace,
                agent,
                session_id,
                submission_id,
            ))
            .await?
            .ok_or_else(|| anyhow!("ask_agent submission '{}' not found", submission_id))?;
        if crate::harness::sessions::submission_is_terminal(&submission) {
            return Ok(submission);
        }
        if start.elapsed() >= timeout {
            return Err(anyhow!(
                "ask_agent timed out waiting for {}/{}/{} submission {}",
                namespace,
                agent,
                session_id,
                submission_id
            ));
        }
        tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
    }
}

async fn latest_assistant_text(
    cp: &ControlPlane,
    namespace: &str,
    agent: &str,
    session_id: &str,
) -> Result<Option<String>> {
    let prefix = keys::session_message_prefix(namespace, agent, session_id);
    let mut before_name = None;
    loop {
        let entries = cp
            .kv
            .list_entries_page(&prefix, before_name.as_deref(), 64)
            .await?;
        if entries.is_empty() {
            return Ok(None);
        }
        before_name = entries.last().map(|(key, _)| key.name.clone());
        for (_, bytes) in entries {
            let message = data_proto::SessionMessage::decode(bytes.as_slice())?;
            if message.role != data_proto::MessageRole::RoleAssistant as i32 {
                continue;
            }
            let text = message
                .parts
                .iter()
                .filter(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
                .map(|part| part.content.as_str())
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
            if !text.is_empty() {
                return Ok(Some(text));
            }
        }
    }
}

fn artifact_uris_from_text(text: &str) -> Vec<String> {
    let mut uris = text
        .split_whitespace()
        .filter_map(|token| {
            token
                .trim_matches(|ch: char| ch == ',' || ch == '.' || ch == ')' || ch == ']')
                .strip_prefix("artifact://")
                .map(|tail| format!("artifact://{tail}"))
        })
        .collect::<Vec<_>>();
    uris.sort();
    uris.dedup();
    uris
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{EmptyPubSub, MockKvStore};
    use std::sync::Arc;

    #[test]
    fn ask_agent_message_includes_granted_artifact_uris() {
        let message = ask_agent_message(
            "Review the draft.",
            &[
                "artifact://Tenant:acme:Ops/writer/session-1/draft".to_string(),
                "artifact://Tenant:acme:Ops/writer/session-1/brief".to_string(),
            ],
        );

        assert!(message.starts_with("Review the draft."));
        assert!(message.contains("Context artifact URIs granted to this session:"));
        assert!(message.contains("artifact://Tenant:acme:Ops/writer/session-1/draft"));
        assert!(message.contains("artifact://Tenant:acme:Ops/writer/session-1/brief"));
        assert!(message.contains("Use read_artifact with the exact URI"));
    }

    #[test]
    fn artifact_uris_from_text_extracts_and_deduplicates_uris() {
        let uris = artifact_uris_from_text(
            "Pass: artifact://Tenant:acme:Ops/writer/session-1/draft, \
             duplicate artifact://Tenant:acme:Ops/writer/session-1/draft. \
             Also artifact://Tenant:acme:Ops/writer/session-1/notes)",
        );

        assert_eq!(
            uris,
            vec![
                "artifact://Tenant:acme:Ops/writer/session-1/draft",
                "artifact://Tenant:acme:Ops/writer/session-1/notes",
            ]
        );
    }

    #[tokio::test]
    async fn grant_artifact_to_child_session_creates_target_session_access() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(EmptyPubSub)).build();
        let artifact = data_proto::Artifact {
            id: "draft".to_string(),
            session_id: "writer-session".to_string(),
            title: "Draft".to_string(),
            media_type: "text/markdown".to_string(),
            object_ref: Some(data_proto::ObjectRef {
                key: "cas/ns/artifacts/draft/sha".to_string(),
                media_type: "text/markdown".to_string(),
                size_bytes: 10,
                sha256: "sha".to_string(),
                filename: "draft.md".to_string(),
                content_encoding: String::new(),
                metadata: HashMap::new(),
            }),
            created_by_agent: "writer".to_string(),
            created_at: 1,
            labels: HashMap::new(),
            metadata: HashMap::new(),
        };
        kv.set_msg(
            &keys::artifact("Tenant:acme:Ops", "writer", "writer-session", "draft"),
            &artifact,
        )
        .await
        .unwrap();

        grant_artifact_to_child_session(
            &cp,
            "writer",
            "writer-session",
            "artifact://Tenant:acme:Ops/writer/writer-session/draft",
            "critic",
            "critic-session",
        )
        .await
        .unwrap();

        let access = kv
            .get_msg::<data_proto::ArtifactAccess>(&keys::artifact_access(
                "Tenant:acme:Ops",
                "writer",
                "writer-session",
                "draft",
                "critic",
                "critic-session",
            ))
            .await
            .unwrap()
            .expect("artifact access should be stored");
        assert_eq!(access.target_agent, "critic");
        assert_eq!(access.target_session_id, "critic-session");
        assert_eq!(
            access.operations,
            vec![super::super::OP_READ, super::super::OP_METADATA]
        );
    }
}
