// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use base64::Engine;
use clap::{Parser, Subcommand};
use futures::StreamExt;
use jsonwebtoken::{EncodingKey, Header};
use minijinja::{context, Environment, UndefinedBehavior};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use talon::gateway::rpc::manifests::{Knowledge, KnowledgeSpec, ObjectMeta};
use talon::gateway::rpc::models;
use talon::gateway::rpc::proto::gateway_service_client::GatewayServiceClient;
use talon::gateway::rpc::proto::{
    CancelWorkflowRunRequest, CreateAgentRequest, CreateAgentTemplateRequest, CreateChannelRequest,
    CreateChannelSubscriptionRequest, CreateMcpServerRequest, CreateNamespaceKnowledgeRequest,
    CreateWorkflowRequest, CreateWorkflowRunRequest, DeleteAgentTemplateRequest,
    DeleteChannelRequest, DeleteChannelSubscriptionRequest, DeleteMcpServerRequest,
    DeleteNamespaceKnowledgeRequest, DeleteWorkflowRequest, GetAgentTemplateRequest,
    GetChannelRequest, GetChannelSubscriptionRequest, GetMcpServerRequest,
    GetNamespaceKnowledgeRequest, GetScheduleRequest, GetWorkflowRequest, GetWorkflowRunRequest,
    ListNamespaceKnowledgeRequest, ListWorkflowRunsRequest, ModifyAgentRequest,
    ModifyChannelRequest, ModifyChannelSubscriptionRequest, ResumeWorkflowRunRequest,
    StreamWorkflowEventsRequest,
};
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::{Request, Status};

const MAX_REST_STREAM_BUFFER_BYTES: usize = 10 * 1024 * 1024;

#[derive(Parser)]
#[command(name = "talon-cli")]
#[command(about = "Administration CLI for the Talon system", long_about = None)]
struct Cli {
    /// gRPC gateway address (e.g. http://localhost:50051)
    #[arg(long, default_value = "http://localhost:50051")]
    gateway: String,

    /// Gateway password for Basic auth. Uses username "" and password value.
    #[arg(long)]
    password: Option<String>,

    /// Gateway bearer token.
    #[arg(long)]
    token: Option<String>,

    /// Shared JWT secret for minting a short-lived Talon admin token.
    #[arg(long)]
    jwt_secret: Option<String>,

    /// Use the REST-transcoded public HTTP endpoints instead of native gRPC.
    #[arg(long, default_value_t = false)]
    rest: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone)]
struct AuthInterceptor {
    authorization: Option<MetadataValue<tonic::metadata::Ascii>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CliClaims {
    sub: String,
    aud: String,
    exp: u64,
    #[serde(rename = "talon:ns", skip_serializing_if = "Option::is_none")]
    ns: Option<String>,
    #[serde(rename = "talon:agent", skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    #[serde(rename = "talon:session", skip_serializing_if = "Option::is_none")]
    session: Option<String>,
    #[serde(rename = "talon:channel", skip_serializing_if = "Option::is_none")]
    channel: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct KnowledgeManifestFile {
    spec: KnowledgeSpecFile,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct KnowledgeSpecFile {
    path: String,
    content: Option<String>,
    content_from_file: Option<String>,
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: Request<()>) -> std::result::Result<Request<()>, Status> {
        if let Some(auth) = &self.authorization {
            req.metadata_mut().insert("authorization", auth.clone());
        }
        Ok(req)
    }
}

fn resolve_gateway_password(cli: &Cli) -> Option<String> {
    cli.password
        .clone()
        .or_else(|| std::env::var("TALON_GATEWAY_PASSWORD").ok())
        .or_else(|| std::env::var("GATEWAY_PASSWORD").ok())
}

fn resolve_gateway_token(cli: &Cli) -> Option<String> {
    cli.token
        .clone()
        .or_else(|| std::env::var("TALON_GATEWAY_TOKEN").ok())
        .or_else(|| std::env::var("GATEWAY_TOKEN").ok())
}

fn resolve_gateway_jwt_secret(cli: &Cli) -> Option<String> {
    cli.jwt_secret
        .clone()
        .or_else(|| std::env::var("TALON_JWT_SECRET").ok())
        .or_else(|| std::env::var("GATEWAY_JWT_SECRET").ok())
}

fn mint_gateway_jwt(secret: &str) -> Result<String> {
    mint_root_jwt(secret, "talon-cli", 3600)
}

fn mint_scoped_jwt(
    secret: &str,
    subject: &str,
    ttl_seconds: u64,
    ns: Option<&str>,
    agent: Option<&str>,
    session: Option<&str>,
    channel: Option<&str>,
) -> Result<String> {
    let subject = subject.trim();
    if subject.is_empty() {
        anyhow::bail!("subject cannot be empty");
    }
    if ttl_seconds == 0 {
        anyhow::bail!("ttl-seconds must be greater than zero");
    }
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        + ttl_seconds;
    let claims = CliClaims {
        sub: subject.to_string(),
        aud: "talon".to_string(),
        exp,
        ns: ns.map(str::to_string),
        agent: agent.map(str::to_string),
        session: session.map(str::to_string),
        channel: channel.map(str::to_string),
    };
    jsonwebtoken::encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .context("Failed to sign Talon JWT")
}

fn mint_root_jwt(secret: &str, subject: &str, ttl_seconds: u64) -> Result<String> {
    mint_scoped_jwt(secret, subject, ttl_seconds, None, None, None, None)
        .context("Failed to sign Talon root JWT")
}

fn validate_token_part<'a>(value: &'a str, name: &str) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        anyhow::bail!("{name} cannot be empty");
    }
    Ok(value)
}

fn mint_agent_jwt(
    secret: &str,
    namespace: &str,
    agent: &str,
    subject: &str,
    ttl_seconds: u64,
) -> Result<String> {
    let namespace = validate_token_part(namespace, "namespace")?;
    let agent = validate_token_part(agent, "agent")?;
    mint_scoped_jwt(
        secret,
        subject,
        ttl_seconds,
        Some(namespace),
        Some(agent),
        None,
        None,
    )
    .context("Failed to sign Talon agent JWT")
}

fn mint_session_jwt(
    secret: &str,
    namespace: &str,
    agent: &str,
    session: &str,
    subject: &str,
    ttl_seconds: u64,
) -> Result<String> {
    let namespace = validate_token_part(namespace, "namespace")?;
    let agent = validate_token_part(agent, "agent")?;
    let session = validate_token_part(session, "session")?;
    mint_scoped_jwt(
        secret,
        subject,
        ttl_seconds,
        Some(namespace),
        Some(agent),
        Some(session),
        None,
    )
    .context("Failed to sign Talon session JWT")
}

fn mint_channel_jwt(
    secret: &str,
    namespace: &str,
    channel: &str,
    subject: &str,
    ttl_seconds: u64,
) -> Result<String> {
    let namespace = validate_token_part(namespace, "namespace")?;
    let channel = validate_token_part(channel, "channel")?;
    mint_scoped_jwt(
        secret,
        subject,
        ttl_seconds,
        Some(namespace),
        None,
        None,
        Some(channel),
    )
    .context("Failed to sign Talon channel JWT")
}

fn auth_interceptor(cli: &Cli) -> Result<AuthInterceptor> {
    let authorization = if let Some(token) = resolve_gateway_token(cli) {
        Some(
            MetadataValue::try_from(format!("Bearer {}", token))
                .context("Failed to encode bearer authorization header")?,
        )
    } else if let Some(secret) = resolve_gateway_jwt_secret(cli) {
        let token = mint_gateway_jwt(&secret)?;
        Some(
            MetadataValue::try_from(format!("Bearer {}", token))
                .context("Failed to encode JWT authorization header")?,
        )
    } else {
        resolve_gateway_password(cli)
            .map(|password| {
                let token =
                    base64::engine::general_purpose::STANDARD.encode(format!(":{}", password));
                MetadataValue::try_from(format!("Basic {}", token))
            })
            .transpose()
            .context("Failed to encode basic authorization header")?
    };

    Ok(AuthInterceptor { authorization })
}

async fn connect_gateway(
    cli: &Cli,
) -> Result<
    GatewayServiceClient<
        tonic::service::interceptor::InterceptedService<tonic::transport::Channel, AuthInterceptor>,
    >,
> {
    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(30))
        .connect()
        .await
        .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
    Ok(GatewayServiceClient::with_interceptor(
        channel,
        auth_interceptor(cli)?,
    ))
}

fn resolve_authorization_header(cli: &Cli) -> Result<Option<String>> {
    if let Some(token) = resolve_gateway_token(cli) {
        Ok(Some(format!("Bearer {}", token)))
    } else if let Some(secret) = resolve_gateway_jwt_secret(cli) {
        let token = mint_gateway_jwt(&secret)?;
        Ok(Some(format!("Bearer {}", token)))
    } else if let Some(password) = resolve_gateway_password(cli) {
        let token = base64::engine::general_purpose::STANDARD.encode(format!(":{}", password));
        Ok(Some(format!("Basic {}", token)))
    } else {
        Ok(None)
    }
}

fn rest_client(cli: &Cli) -> Result<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(auth) = resolve_authorization_header(cli)? {
        headers.insert(
            AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&auth)
                .context("Failed to encode REST authorization header")?,
        );
    }
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .context("Failed to build REST client")
}

fn schedule_json(schedule: &models::Schedule) -> serde_json::Value {
    let spec = schedule.spec.as_ref();
    let target = spec.and_then(|spec| spec.target.as_ref());
    let status = schedule.status.as_ref();

    json!({
        "name": schedule.name,
        "ns": schedule.ns,
        "labels": schedule.labels,
        "spec": spec.map(|spec| json!({
            "kind": spec.kind,
            "cron": spec.cron,
            "intervalSeconds": spec.interval_seconds,
            "runAt": spec.run_at,
            "timezone": spec.timezone,
            "target": target.map(|target| json!({
                "agent": target.agent,
                "workflow": target.workflow,
                "sessionMode": target.session_mode,
                "sessionId": target.session_id,
            })),
            "inputMessage": spec.input_message,
            "inputJson": spec.input_json,
            "enabled": spec.enabled,
        })),
        "status": status.map(|status| json!({
            "revision": status.revision,
            "nextRunAt": status.next_run_at,
            "backendHandle": status.backend_handle,
            "backendArmed": status.backend_armed,
            "lastRunAt": status.last_run_at,
            "lastSessionId": status.last_session_id,
            "lastError": status.last_error,
            "claimedRunAt": status.claimed_run_at,
            "claimExpiresAt": status.claim_expires_at,
            "recentEvents": status.recent_events.iter().map(|event| json!({
                "timestamp": event.timestamp,
                "phase": event.phase,
                "outcome": event.outcome,
                "detail": event.detail,
            })).collect::<Vec<_>>(),
        })),
    })
}

fn workflow_run_json(
    run: &models::WorkflowRun,
    steps: &[models::WorkflowStepRun],
) -> serde_json::Value {
    json!({
        "id": run.id,
        "workflow": run.workflow,
        "ns": run.ns,
        "status": run.status,
        "input": parse_json_field(&run.input_json),
        "state": parse_json_field(&run.state_json),
        "output": parse_json_field(&run.output_json),
        "createdAt": run.created_at,
        "updatedAt": run.updated_at,
        "labels": run.labels,
        "claimExpiresAt": run.claim_expires_at,
        "error": run.error,
        "workflowRevision": run.workflow_revision,
        "claimOwner": run.claim_owner,
        "claimAttempt": run.claim_attempt,
        "lastDispatchReason": run.last_dispatch_reason,
        "steps": steps.iter().map(workflow_step_run_json).collect::<Vec<_>>(),
    })
}

fn workflow_step_run_json(step: &models::WorkflowStepRun) -> serde_json::Value {
    json!({
        "id": step.id,
        "stepId": step.step_id,
        "attempt": step.attempt,
        "status": step.status,
        "input": parse_json_field(&step.input_json),
        "output": parse_json_field(&step.output_json),
        "error": step.error,
        "childSessionId": step.child_session_id,
        "childWorkflowRunId": step.child_workflow_run_id,
        "resume": parse_json_field(&step.resume_json),
        "suspend": parse_json_field(&step.suspend_json),
        "createdAt": step.created_at,
        "updatedAt": step.updated_at,
        "nextRetryAt": step.next_retry_at,
        "timeoutAt": step.timeout_at,
        "waitWakeupHandle": step.wait_wakeup_handle,
        "waitUntilAt": step.wait_until_at,
    })
}

fn workflow_event_json(event: &models::WorkflowRunEvent) -> serde_json::Value {
    json!({
        "id": event.id,
        "ns": event.ns,
        "workflow": event.workflow,
        "runId": event.run_id,
        "type": event.r#type,
        "stepId": event.step_id,
        "message": event.message,
        "payload": parse_json_field(&event.payload_json),
        "timestamp": event.timestamp,
    })
}

fn parse_json_field(value: &str) -> serde_json::Value {
    if value.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(value).unwrap_or_else(|_| serde_json::Value::String(value.to_string()))
    }
}

async fn rest_request_json(
    cli: &Cli,
    method: reqwest::Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    let client = rest_client(cli)?;
    let url = format!("{}{}", cli.gateway.trim_end_matches('/'), path);
    let mut request = client.request(method, &url);
    if let Some(payload) = body {
        request = request
            .header(CONTENT_TYPE, "application/json")
            .json(&payload);
    }
    let response = request
        .send()
        .await
        .with_context(|| format!("Failed to call REST endpoint {}", url))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .with_context(|| format!("Failed to read REST response body from {}", url))?;
    if !status.is_success() {
        anyhow::bail!(
            "REST {} {} failed: status={} body={}",
            path,
            url,
            status,
            text.trim()
        );
    }
    if text.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(&text)
        .with_context(|| format!("Failed to parse REST response JSON from {}", url))
}

fn manifest_json_payload(content: &str) -> Result<(String, serde_json::Value)> {
    let raw = parse_raw_manifest(content)?;
    let manifest_value: serde_yaml::Value =
        serde_yaml::from_str(content).context("Failed to parse rendered manifest")?;
    match raw.kind.as_str() {
        "AgentTemplate" => Ok((
            "template".to_string(),
            json!({ "template": manifest_value }),
        )),
        "MCPServer" | "McpServer" => {
            Ok(("server".to_string(), json!({ "server": manifest_value })))
        }
        "Agent" => {
            let agent = talon::manifest::parse_agent(content)?;
            let definition = manifest_value
                .get("definition")
                .cloned()
                .context("Agent manifest missing definition")?;
            Ok((
                "agent".to_string(),
                json!({
                    "ns": agent.ns,
                    "name": agent.name,
                    "labels": agent.labels,
                    "definition": definition,
                }),
            ))
        }
        "McpServerBinding" => {
            let binding = talon::manifest::parse_mcp_server_binding(content)?;
            let namespace = binding
                .metadata
                .as_ref()
                .map(|meta| meta.namespace.clone())
                .filter(|namespace| !namespace.is_empty())
                .context("McpServerBinding missing metadata.namespace")?;
            Ok((
                "binding".to_string(),
                json!({
                    "ns": namespace,
                    "binding": binding,
                }),
            ))
        }
        "Namespace" => {
            let namespace = talon::manifest::parse_namespace(content)?;
            Ok((
                "namespace".to_string(),
                json!({
                    "name": namespace.name,
                    "recursive": true,
                    "labels": namespace.labels,
                }),
            ))
        }
        "Knowledge" => Ok((
            "knowledge".to_string(),
            json!({ "knowledge": manifest_value }),
        )),
        "Channel" => {
            let channel = talon::manifest::parse_channel(content)?;
            Ok((
                "channel".to_string(),
                json!({
                    "ns": channel.ns,
                    "channel": channel,
                }),
            ))
        }
        "ChannelSubscription" => {
            let subscription = talon::manifest::parse_channel_subscription(content)?;
            Ok((
                "subscription".to_string(),
                json!({
                    "ns": subscription.ns,
                    "channel": subscription.channel,
                    "subscription": subscription,
                }),
            ))
        }
        "Workflow" => {
            let workflow = talon::manifest::parse_workflow(content)?;
            Ok((
                "workflow".to_string(),
                json!({
                    "ns": workflow.ns,
                    "workflow": workflow,
                }),
            ))
        }
        other => anyhow::bail!("Unsupported manifest kind '{}'", other),
    }
}

fn agent_lookup_target(name: &str, namespace: Option<&String>) -> (String, String) {
    let mut parts = name.splitn(2, '/');
    let ns_part = parts.next().unwrap_or("default");
    let agent_name = parts.next().unwrap_or(ns_part);
    let (mut final_ns, final_name) = if agent_name == ns_part {
        ("default".to_string(), ns_part.to_string())
    } else {
        (ns_part.to_string(), agent_name.to_string())
    };
    if let Some(n) = namespace {
        final_ns = n.clone();
    }
    (final_ns, final_name)
}

fn rest_get_path(
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<(String, &'static str)> {
    match kind.to_lowercase().as_str() {
        "agenttemplate" | "templates" | "template" => Ok((
            format!("/v1/templates/{}", urlencoding::encode(name)),
            "template",
        )),
        "mcpserver" | "mcpservers" | "mcp" => Ok((
            format!("/v1/mcp-servers/{}", urlencoding::encode(name)),
            "server",
        )),
        "agent" | "agents" => {
            let (ns, agent_name) = agent_lookup_target(name, namespace);
            Ok((
                format!(
                    "/v1/ns/{}/agents/{}",
                    urlencoding::encode(&ns),
                    urlencoding::encode(&agent_name)
                ),
                "agent",
            ))
        }
        "mcpserverbinding" | "mcpbindings" | "mcpbinding" => {
            let ns = namespace
                .as_ref()
                .context("namespace is required for McpServerBinding get")?;
            Ok((
                format!(
                    "/v1/namespaces/{}/mcp-bindings/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "binding",
            ))
        }
        "namespace" | "namespaces" => Ok((
            format!("/v1/namespaces/{}", urlencoding::encode(name)),
            "namespace",
        )),
        "knowledge" | "knowledgeartifact" | "knowledgeartifacts" => {
            let ns = namespace
                .as_ref()
                .context("Knowledge get requires --namespace")?;
            Ok((
                format!(
                    "/v1/namespaces/{}/knowledge/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "knowledge",
            ))
        }
        "schedule" | "schedules" => {
            let ns = namespace
                .as_ref()
                .context("Schedule get requires --namespace")?;
            Ok((
                format!(
                    "/v1/ns/{}/schedules/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "schedule",
            ))
        }
        "channel" | "channels" => {
            let ns = namespace
                .as_ref()
                .context("Channel get requires --namespace")?;
            Ok((
                format!(
                    "/v1/ns/{}/channels/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "channel",
            ))
        }
        "channelsubscription"
        | "channelsubscriptions"
        | "channel-subscription"
        | "channel-subscriptions" => {
            let ns = namespace
                .as_ref()
                .context("ChannelSubscription get requires --namespace")?;
            let (channel, subscription) = name
                .split_once('/')
                .context("ChannelSubscription name must be '<channel>/<subscription>'")?;
            Ok((
                format!(
                    "/v1/ns/{}/channels/{}/subscriptions/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(channel),
                    urlencoding::encode(subscription)
                ),
                "subscription",
            ))
        }
        "workflow" | "workflows" => {
            let ns = namespace
                .as_ref()
                .context("Workflow get requires --namespace")?;
            Ok((
                format!(
                    "/v1/ns/{}/workflows/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "workflow",
            ))
        }
        other => anyhow::bail!("Unsupported resource kind '{}' for REST mode", other),
    }
}

fn rest_delete_path(kind: &str, name: &str, namespace: Option<&String>) -> Result<String> {
    match kind.to_lowercase().as_str() {
        "agenttemplate" | "templates" | "template" => {
            Ok(format!("/v1/templates/{}", urlencoding::encode(name)))
        }
        "mcpserver" | "mcpservers" | "mcp" => {
            Ok(format!("/v1/mcp-servers/{}", urlencoding::encode(name)))
        }
        "agent" | "agents" => {
            let ns = namespace
                .as_ref()
                .context("namespace is required for Agent delete")?;
            Ok(format!(
                "/v1/ns/{}/agents/{}",
                urlencoding::encode(ns),
                urlencoding::encode(name)
            ))
        }
        "mcpserverbinding" | "mcpbindings" | "mcpbinding" => {
            let ns = namespace
                .as_ref()
                .context("namespace is required for McpServerBinding delete")?;
            Ok(format!(
                "/v1/namespaces/{}/mcp-bindings/{}",
                urlencoding::encode(ns),
                urlencoding::encode(name)
            ))
        }
        "namespace" | "namespaces" => Ok(format!("/v1/namespaces/{}", urlencoding::encode(name))),
        "knowledge" | "knowledgeartifact" | "knowledgeartifacts" => {
            let ns = namespace
                .as_ref()
                .context("Knowledge delete requires --namespace")?;
            Ok(format!(
                "/v1/namespaces/{}/knowledge/{}",
                urlencoding::encode(ns),
                urlencoding::encode(name)
            ))
        }
        "channel" | "channels" => {
            let ns = namespace
                .as_ref()
                .context("Channel delete requires --namespace")?;
            Ok(format!(
                "/v1/ns/{}/channels/{}",
                urlencoding::encode(ns),
                urlencoding::encode(name)
            ))
        }
        "channelsubscription"
        | "channelsubscriptions"
        | "channel-subscription"
        | "channel-subscriptions" => {
            let ns = namespace
                .as_ref()
                .context("ChannelSubscription delete requires --namespace")?;
            let (channel, subscription) = name
                .split_once('/')
                .context("ChannelSubscription name must be '<channel>/<subscription>'")?;
            Ok(format!(
                "/v1/ns/{}/channels/{}/subscriptions/{}",
                urlencoding::encode(ns),
                urlencoding::encode(channel),
                urlencoding::encode(subscription)
            ))
        }
        "workflow" | "workflows" => {
            let ns = namespace
                .as_ref()
                .context("Workflow delete requires --namespace")?;
            Ok(format!(
                "/v1/ns/{}/workflows/{}",
                urlencoding::encode(ns),
                urlencoding::encode(name)
            ))
        }
        other => anyhow::bail!("Unsupported resource kind '{}' for REST mode", other),
    }
}

fn render_json_payload(content: &str) -> Result<serde_json::Value> {
    let raw = parse_raw_manifest(content)?;
    let manifest_value: serde_yaml::Value =
        serde_yaml::from_str(content).context("Failed to parse rendered manifest")?;
    match raw.kind.as_str() {
        "AgentTemplate" => Ok(json!({ "template": manifest_value })),
        "MCPServer" | "McpServer" => Ok(json!({ "server": manifest_value })),
        "Agent" => Ok(json!({ "agent": manifest_value })),
        "McpServerBinding" => {
            let binding = talon::manifest::parse_mcp_server_binding(content)?;
            let namespace = binding
                .metadata
                .as_ref()
                .map(|meta| meta.namespace.clone())
                .filter(|namespace| !namespace.is_empty())
                .context("McpServerBinding missing metadata.namespace")?;
            Ok(json!({
                "ns": namespace,
                "binding": binding,
            }))
        }
        "Namespace" => {
            let namespace = talon::manifest::parse_namespace(content)?;
            Ok(json!({
                "name": namespace.name,
                "recursive": true,
                "labels": namespace.labels,
            }))
        }
        "Knowledge" => Ok(json!({ "knowledge": manifest_value })),
        "Channel" => {
            let channel = talon::manifest::parse_channel(content)?;
            Ok(json!({ "ns": channel.ns, "channel": channel }))
        }
        "ChannelSubscription" => {
            let subscription = talon::manifest::parse_channel_subscription(content)?;
            Ok(json!({
                "ns": subscription.ns,
                "channel": subscription.channel,
                "subscription": subscription,
            }))
        }
        "Workflow" => {
            let workflow = talon::manifest::parse_workflow(content)?;
            Ok(json!({ "ns": workflow.ns, "workflow": workflow }))
        }
        other => anyhow::bail!("Unsupported manifest kind '{}'", other),
    }
}

async fn rest_get_yaml(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let (path, response_key) = rest_get_path(kind, name, namespace)?;
    let resp = rest_request_json(cli, reqwest::Method::GET, &path, None)
        .await
        .with_context(|| format!("Failed to fetch {} '{}'", kind, name))?;
    let value = if response_key == "namespace" {
        resp
    } else {
        resp.get(response_key)
            .cloned()
            .with_context(|| format!("REST response missing {}", response_key))?
    };
    render_rest_get_yaml(response_key, value)
        .with_context(|| format!("Failed to serialize {} YAML", kind))
}

fn render_rest_get_yaml(response_key: &str, value: serde_json::Value) -> Result<String> {
    match response_key {
        "agent" => render_rest_agent_yaml(value),
        "namespace" => render_rest_namespace_yaml(value),
        "server" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let server: talon::gateway::rpc::manifests::McpServer =
                serde_json::from_value(value).context("Failed to decode MCPServer JSON")?;
            talon::manifest::render_mcp_server_yaml(&server)
        }
        "binding" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let binding: talon::gateway::rpc::manifests::McpServerBinding =
                serde_json::from_value(value).context("Failed to decode McpServerBinding JSON")?;
            talon::manifest::render_mcp_server_binding_yaml(&binding)
        }
        "knowledge" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let knowledge: talon::gateway::rpc::manifests::Knowledge =
                serde_json::from_value(value).context("Failed to decode Knowledge JSON")?;
            talon::manifest::render_knowledge_yaml(&knowledge)
        }
        "template" => {
            let mut value = value;
            normalize_manifest_metadata_maps(&mut value);
            let template_json =
                serde_json::to_string(&value).context("Failed to serialize AgentTemplate JSON")?;
            let template = talon::manifest::parse_agent_template(&template_json)
                .context("Failed to decode AgentTemplate manifest")?;
            talon::manifest::render_agent_template_yaml(&template)
        }
        "schedule" => serde_yaml::to_string(&value).context("Failed to serialize Schedule YAML"),
        "channel" => {
            let mut value = value;
            normalize_json_int64_fields(
                &mut value,
                &["createdAt", "created_at", "updatedAt", "updated_at"],
            )?;
            let channel: models::Channel =
                serde_json::from_value(value).context("Failed to decode Channel JSON")?;
            talon::manifest::render_channel_yaml(&channel)
        }
        "subscription" => {
            let subscription: models::ChannelSubscription = serde_json::from_value(value)
                .context("Failed to decode ChannelSubscription JSON")?;
            talon::manifest::render_channel_subscription_yaml(&subscription)
        }
        "workflow" => {
            let workflow: models::Workflow =
                serde_json::from_value(value).context("Failed to decode Workflow JSON")?;
            talon::manifest::render_workflow_yaml(&workflow)
        }
        other => anyhow::bail!("Unsupported REST response resource '{}'", other),
    }
}

fn normalize_manifest_metadata_maps(value: &mut serde_json::Value) {
    let Some(metadata) = value
        .get_mut("metadata")
        .and_then(|metadata| metadata.as_object_mut())
    else {
        return;
    };

    for key in ["labels", "annotations"] {
        if metadata.get(key).is_some_and(|value| value.is_null()) {
            metadata.insert(key.to_string(), json!({}));
        }
    }
}

fn normalize_json_int64_fields(value: &mut serde_json::Value, fields: &[&str]) -> Result<()> {
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };

    for field in fields {
        let Some(field_value) = object.get_mut(*field) else {
            continue;
        };
        let Some(raw) = field_value.as_str() else {
            continue;
        };
        let parsed = raw
            .parse::<i64>()
            .with_context(|| format!("Failed to parse {field} as int64"))?;
        *field_value = serde_json::Value::Number(parsed.into());
    }

    Ok(())
}

fn render_rest_agent_yaml(agent: serde_json::Value) -> Result<String> {
    let name = agent
        .get("name")
        .or_else(|| agent.get("agent"))
        .and_then(|name| name.as_str())
        .context("Agent response missing name")?;
    let namespace = agent
        .get("ns")
        .and_then(|namespace| namespace.as_str())
        .context("Agent response missing ns")?;
    let definition = agent
        .get("definition")
        .cloned()
        .context("Agent response missing definition")?;
    let labels = agent
        .get("labels")
        .filter(|labels| !labels.is_null())
        .cloned()
        .unwrap_or_else(|| json!({}));

    serde_yaml::to_string(&json!({
        "apiVersion": "talon.impalasys.com/v1",
        "kind": "Agent",
        "metadata": {
            "name": name,
            "namespace": namespace,
            "labels": labels,
        },
        "definition": definition,
    }))
    .context("Failed to serialize Agent YAML")
}

fn render_rest_namespace_yaml(namespace: serde_json::Value) -> Result<String> {
    let name = namespace
        .get("name")
        .and_then(|name| name.as_str())
        .context("Namespace response missing name")?;
    let labels = namespace
        .get("labels")
        .filter(|labels| !labels.is_null())
        .cloned()
        .unwrap_or_else(|| json!({}));

    serde_yaml::to_string(&json!({
        "apiVersion": "talon.impalasys.com/v1",
        "kind": "Namespace",
        "metadata": {
            "name": name,
            "labels": labels,
        },
    }))
    .context("Failed to serialize Namespace YAML")
}

async fn rest_delete_resource(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let path = rest_delete_path(kind, name, namespace)?;
    rest_request_json(cli, reqwest::Method::DELETE, &path, None)
        .await
        .with_context(|| format!("Failed to delete {} '{}'", kind, name))?;
    Ok(format!("✓ {} '{}' deleted successfully.", kind, name))
}

async fn rest_apply_manifest(cli: &Cli, content: &str, agent_exists: bool) -> Result<String> {
    let (_, payload) = manifest_json_payload(content)?;
    let plan = build_rest_apply_plan(content, payload, agent_exists)?;
    rest_request_json(cli, plan.method, &plan.path, Some(plan.payload))
        .await
        .with_context(|| format!("Gateway rejected {}", plan.success_label))?;
    Ok(format!("✓ {} applied successfully.", plan.success_label))
}

fn knowledge_resource_name(path: &str) -> String {
    path.to_string()
}

fn build_knowledge(namespace: &str, path: &str, content: String) -> Knowledge {
    Knowledge {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "Knowledge".to_string(),
        metadata: Some(ObjectMeta {
            name: knowledge_resource_name(path),
            namespace: namespace.to_string(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
        }),
        spec: Some(KnowledgeSpec {
            path: path.to_string(),
            content,
        }),
    }
}

fn read_knowledge_content(file: &Option<String>, content: &Option<String>) -> Result<String> {
    match (file, content) {
        (Some(path), None) => fs::read_to_string(path)
            .with_context(|| format!("Failed to read knowledge content from '{}'", path)),
        (None, Some(value)) => Ok(value.clone()),
        (Some(_), Some(_)) => anyhow::bail!("Specify only one of --file or --content"),
        (None, None) => anyhow::bail!("One of --file or --content is required"),
    }
}

fn relative_knowledge_path(root: &Path, file: &Path) -> Result<String> {
    let relative = file.strip_prefix(root).with_context(|| {
        format!(
            "Knowledge file '{}' is not inside '{}'",
            file.display(),
            root.display()
        )
    })?;
    let path = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    if path.is_empty() {
        anyhow::bail!("Knowledge path cannot be empty for '{}'", file.display());
    }
    Ok(path)
}

fn collect_markdown_files(dir: &Path) -> Result<Vec<PathBuf>> {
    fn walk(current: &Path, acc: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(current)
            .with_context(|| format!("Failed to read directory '{}'", current.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walk(&path, acc)?;
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
            {
                acc.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    walk(dir, &mut files)?;
    files.sort();
    Ok(files)
}

async fn knowledge_get(cli: &Cli, namespace: &str, path: &str) -> Result<Option<Knowledge>> {
    if cli.rest {
        let resp = rest_request_json(
            cli,
            reqwest::Method::GET,
            &format!(
                "/v1/namespaces/{}/knowledge/{}",
                urlencoding::encode(namespace),
                urlencoding::encode(&knowledge_resource_name(path))
            ),
            None,
        )
        .await?;
        let Some(knowledge) = resp.get("knowledge").cloned() else {
            return Ok(None);
        };
        Ok(Some(
            serde_json::from_value(knowledge).context("Failed to decode Knowledge JSON")?,
        ))
    } else {
        let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
            .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
            .connect()
            .await
            .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
        let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);
        let response = client
            .get_namespace_knowledge(GetNamespaceKnowledgeRequest {
                ns: namespace.to_string(),
                name: knowledge_resource_name(path),
            })
            .await;
        match response {
            Ok(resp) => Ok(resp.into_inner().knowledge),
            Err(status) if status.code() == tonic::Code::NotFound => Ok(None),
            Err(status) => Err(status).context(format!(
                "Failed to fetch Knowledge '{}/{}'",
                namespace, path
            )),
        }
    }
}

async fn knowledge_set(cli: &Cli, namespace: &str, path: &str, content: String) -> Result<()> {
    let knowledge = build_knowledge(namespace, path, content);
    if cli.rest {
        rest_request_json(
            cli,
            reqwest::Method::POST,
            &format!(
                "/v1/namespaces/{}/knowledge",
                urlencoding::encode(namespace)
            ),
            Some(json!({ "knowledge": knowledge })),
        )
        .await
        .with_context(|| format!("Failed to write Knowledge '{}/{}'", namespace, path))?;
    } else {
        let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
            .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
            .connect()
            .await
            .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
        let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);
        client
            .create_namespace_knowledge(CreateNamespaceKnowledgeRequest {
                ns: namespace.to_string(),
                knowledge: Some(knowledge),
            })
            .await
            .with_context(|| format!("Failed to write Knowledge '{}/{}'", namespace, path))?;
    }
    Ok(())
}

async fn knowledge_delete(cli: &Cli, namespace: &str, path: &str) -> Result<()> {
    let name = knowledge_resource_name(path);
    if cli.rest {
        rest_request_json(
            cli,
            reqwest::Method::DELETE,
            &format!(
                "/v1/namespaces/{}/knowledge/{}",
                urlencoding::encode(namespace),
                urlencoding::encode(&name)
            ),
            None,
        )
        .await
        .with_context(|| format!("Failed to delete Knowledge '{}/{}'", namespace, path))?;
    } else {
        let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
            .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
            .connect()
            .await
            .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
        let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);
        client
            .delete_namespace_knowledge(DeleteNamespaceKnowledgeRequest {
                ns: namespace.to_string(),
                name,
            })
            .await
            .with_context(|| format!("Failed to delete Knowledge '{}/{}'", namespace, path))?;
    }
    Ok(())
}

async fn knowledge_list(cli: &Cli, namespace: &str) -> Result<Vec<Knowledge>> {
    if cli.rest {
        let resp = rest_request_json(
            cli,
            reqwest::Method::GET,
            &format!(
                "/v1/namespaces/{}/knowledge",
                urlencoding::encode(namespace)
            ),
            None,
        )
        .await?;
        let knowledge = resp
            .get("knowledge")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
        Ok(serde_json::from_value(knowledge).context("Failed to decode knowledge list JSON")?)
    } else {
        let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
            .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
            .connect()
            .await
            .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
        let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);
        Ok(client
            .list_namespace_knowledge(ListNamespaceKnowledgeRequest {
                ns: namespace.to_string(),
            })
            .await
            .with_context(|| format!("Failed to list Knowledge for '{}'", namespace))?
            .into_inner()
            .knowledge)
    }
}

fn read_json_arg(value: &Option<String>, file: &Option<String>) -> Result<String> {
    let raw = if let Some(file) = file {
        fs::read_to_string(file).with_context(|| format!("Failed to read JSON file '{}'", file))?
    } else {
        value.clone().unwrap_or_else(|| "{}".to_string())
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).context("Argument must be valid JSON")?;
    serde_json::to_string(&parsed).context("Failed to normalize JSON argument")
}

async fn workflow_run_create(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    input_json: String,
) -> Result<serde_json::Value> {
    if cli.rest {
        return rest_request_json(
            cli,
            reqwest::Method::POST,
            &format!(
                "/v1/ns/{}/workflows/{}/runs",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow)
            ),
            Some(json!({ "ns": namespace, "workflow": workflow, "inputJson": input_json })),
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .create_workflow_run(CreateWorkflowRunRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            input_json,
            labels: HashMap::new(),
        })
        .await
        .context("Failed to create workflow run")?
        .into_inner();
    let run = response.run.context("Workflow run missing from response")?;
    Ok(workflow_run_json(&run, &response.steps))
}

async fn workflow_run_get(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
) -> Result<serde_json::Value> {
    if cli.rest {
        return rest_request_json(
            cli,
            reqwest::Method::GET,
            &format!(
                "/v1/ns/{}/workflows/{}/runs/{}",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow),
                urlencoding::encode(run_id)
            ),
            None,
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .get_workflow_run(GetWorkflowRunRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            run_id: run_id.to_string(),
        })
        .await
        .context("Failed to get workflow run")?
        .into_inner();
    let run = response.run.context("Workflow run missing from response")?;
    Ok(workflow_run_json(&run, &response.steps))
}

async fn workflow_run_list(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    page_size: i32,
    before_run_id: &str,
) -> Result<serde_json::Value> {
    if cli.rest {
        let mut query = Vec::new();
        if page_size != 0 {
            query.push(format!("page_size={page_size}"));
        }
        if !before_run_id.is_empty() {
            query.push(format!(
                "before_run_id={}",
                urlencoding::encode(before_run_id)
            ));
        }
        let query = if query.is_empty() {
            String::new()
        } else {
            format!("?{}", query.join("&"))
        };
        return rest_request_json(
            cli,
            reqwest::Method::GET,
            &format!(
                "/v1/ns/{}/workflows/{}/runs{}",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow),
                query
            ),
            None,
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .list_workflow_runs(ListWorkflowRunsRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            page_size,
            before_run_id: before_run_id.to_string(),
        })
        .await
        .context("Failed to list workflow runs")?
        .into_inner();
    Ok(json!({
        "runs": response.runs.iter().map(|run| workflow_run_json(run, &[])).collect::<Vec<_>>(),
        "hasMore": response.has_more,
        "nextBeforeRunId": response.next_before_run_id,
    }))
}

async fn workflow_run_resume(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
    step_id: &str,
    resume_json: String,
) -> Result<serde_json::Value> {
    if cli.rest {
        return rest_request_json(
            cli,
            reqwest::Method::POST,
            &format!(
                "/v1/ns/{}/workflows/{}/runs/{}:resume",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow),
                urlencoding::encode(run_id)
            ),
            Some(json!({
                "ns": namespace,
                "workflow": workflow,
                "runId": run_id,
                "stepId": step_id,
                "resumeJson": resume_json,
            })),
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .resume_workflow_run(ResumeWorkflowRunRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            run_id: run_id.to_string(),
            step_id: step_id.to_string(),
            resume_json,
        })
        .await
        .context("Failed to resume workflow run")?
        .into_inner();
    let run = response.run.context("Workflow run missing from response")?;
    Ok(workflow_run_json(&run, &response.steps))
}

async fn workflow_run_cancel(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
) -> Result<serde_json::Value> {
    if cli.rest {
        return rest_request_json(
            cli,
            reqwest::Method::POST,
            &format!(
                "/v1/ns/{}/workflows/{}/runs/{}:cancel",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow),
                urlencoding::encode(run_id)
            ),
            Some(json!({ "ns": namespace, "workflow": workflow, "runId": run_id })),
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .cancel_workflow_run(CancelWorkflowRunRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            run_id: run_id.to_string(),
        })
        .await
        .context("Failed to cancel workflow run")?
        .into_inner();
    let run = response.run.context("Workflow run missing from response")?;
    Ok(workflow_run_json(&run, &response.steps))
}

async fn workflow_run_events(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
) -> Result<()> {
    if cli.rest {
        rest_stream_workflow_events(cli, namespace, workflow, run_id).await?;
        return Ok(());
    }
    let mut client = connect_gateway(cli).await?;
    let mut stream = client
        .stream_workflow_events(StreamWorkflowEventsRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            run_id: run_id.to_string(),
        })
        .await
        .context("Failed to stream workflow events")?
        .into_inner();
    while let Some(event) = stream
        .message()
        .await
        .context("Failed to read workflow event")?
    {
        println!("{}", serde_json::to_string(&workflow_event_json(&event))?);
    }
    Ok(())
}

async fn rest_stream_workflow_events(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
) -> Result<()> {
    let client = rest_client(cli)?;
    let path = format!(
        "/v1/ns/{}/workflows/{}/runs/{}/stream",
        urlencoding::encode(namespace),
        urlencoding::encode(workflow),
        urlencoding::encode(run_id)
    );
    let url = format!("{}{}", cli.gateway.trim_end_matches('/'), path);
    let response = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to call REST endpoint {}", url))?;
    let status = response.status();
    if !status.is_success() {
        let text = response
            .text()
            .await
            .with_context(|| format!("Failed to read REST response body from {}", url))?;
        anyhow::bail!(
            "REST {} {} failed: status={} body={}",
            path,
            url,
            status,
            text.trim()
        );
    }

    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("Failed to read REST stream from {}", url))?;
        buffer.extend_from_slice(&chunk);
        ensure_rest_stream_buffer_within_limit(buffer.len())?;
        let mut last_index = 0;
        while let Some(newline) = buffer[last_index..].iter().position(|byte| *byte == b'\n') {
            let absolute_newline = last_index + newline;
            let line = String::from_utf8_lossy(&buffer[last_index..absolute_newline])
                .trim_end_matches('\r')
                .to_string();
            print_stream_event_line(&line)?;
            last_index = absolute_newline + 1;
        }
        if last_index > 0 {
            buffer.drain(..last_index);
        }
    }
    if !buffer.is_empty() {
        let line = String::from_utf8_lossy(&buffer)
            .trim_end_matches('\r')
            .to_string();
        print_stream_event_line(&line)?;
    }
    Ok(())
}

fn ensure_rest_stream_buffer_within_limit(buffer_len: usize) -> Result<()> {
    if buffer_len > MAX_REST_STREAM_BUFFER_BYTES {
        anyhow::bail!(
            "REST stream exceeded maximum buffer limit of {} bytes without a newline",
            MAX_REST_STREAM_BUFFER_BYTES
        );
    }
    Ok(())
}

fn print_stream_event_line(line: &str) -> Result<()> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') || line.starts_with("event:") {
        return Ok(());
    }
    let payload = line.strip_prefix("data:").map(str::trim).unwrap_or(line);
    if payload.is_empty() || payload == "[DONE]" {
        return Ok(());
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
        println!("{}", serde_json::to_string(&json)?);
    } else {
        println!("{}", payload);
    }
    Ok(())
}

async fn sync_knowledge_dir(cli: &Cli, namespace: &str, dir: &str) -> Result<(usize, Vec<String>)> {
    let root = Path::new(dir);
    let files = collect_markdown_files(root)?;
    let existing: Vec<Knowledge> = knowledge_list(cli, namespace).await?;
    let existing_paths = existing
        .into_iter()
        .filter_map(|knowledge| knowledge.spec.map(|spec| spec.path))
        .collect::<std::collections::HashSet<_>>();
    let mut synced_paths = Vec::new();

    for file in files {
        let knowledge_path = relative_knowledge_path(root, &file)?;
        let content = fs::read_to_string(&file)
            .with_context(|| format!("Failed to read knowledge file '{}'", file.display()))?;
        knowledge_set(cli, namespace, &knowledge_path, content).await?;
        synced_paths.push(knowledge_path);
    }

    let unsynced_existing = existing_paths
        .into_iter()
        .filter(|path| !synced_paths.iter().any(|synced| synced == path))
        .collect::<Vec<_>>();

    Ok((synced_paths.len(), unsynced_existing))
}

#[derive(Subcommand)]
enum Commands {
    /// Mint scoped auth tokens for clients.
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },
    /// Manage namespace knowledge artifacts directly by path.
    Knowledge {
        #[command(subcommand)]
        command: KnowledgeCommands,
    },
    /// Create and inspect workflow runs.
    Workflow {
        #[command(subcommand)]
        command: WorkflowCommands,
    },
    /// Applies a manifest file (e.g. AgentTemplate)
    Apply {
        #[arg(short, long)]
        file: String,
        /// Template variables in KEY=VALUE form.
        #[arg(long = "var", value_name = "KEY=VALUE")]
        vars: Vec<String>,
    },
    /// Renders a manifest file after template substitution.
    Render {
        #[arg(short, long)]
        file: String,
        /// Template variables in KEY=VALUE form.
        #[arg(long = "var", value_name = "KEY=VALUE")]
        vars: Vec<String>,
        #[arg(long, default_value = "yaml")]
        format: RenderFormat,
    },
    /// Retrieves a manifest from the gateway
    Get {
        /// Type of resource to get (e.g., config, agenttemplate)
        kind: String,
        /// Name of the resource
        name: String,
        /// Namespace of the resource
        #[arg(short, long)]
        namespace: Option<String>,
    },
    /// Deletes a manifest from the gateway
    Delete {
        /// Type of resource to delete (e.g., config, agenttemplate)
        kind: String,
        /// Name of the resource
        name: String,
        /// Namespace of the resource
        #[arg(short, long)]
        namespace: Option<String>,
    },
    /// Generates a TypeScript client SDK from manifest files
    Gen {
        #[arg(long, default_value = "conic/manifests")]
        dir: String,
        #[arg(long, default_value = "client.ts")]
        out: String,
    },
}

#[derive(Subcommand)]
enum AuthCommands {
    /// Mint a root JWT with unrestricted gateway scope.
    RootToken {
        #[arg(long, default_value = "talon-root-client")]
        subject: String,
        #[arg(long, default_value_t = 3600)]
        ttl_seconds: u64,
    },
    /// Mint a JWT that can only access one agent in a namespace.
    AgentToken {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        #[arg(long, default_value = "talon-agent-client")]
        subject: String,
        #[arg(long, default_value_t = 3600)]
        ttl_seconds: u64,
    },
    /// Mint a JWT that can only access one session for one agent.
    SessionToken {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        #[arg(short, long)]
        session: String,
        #[arg(long, default_value = "talon-session-client")]
        subject: String,
        #[arg(long, default_value_t = 3600)]
        ttl_seconds: u64,
    },
    /// Mint a JWT that can only access messages in one channel.
    ChannelToken {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        channel: String,
        #[arg(long, default_value = "talon-channel-client")]
        subject: String,
        #[arg(long, default_value_t = 3600)]
        ttl_seconds: u64,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum RenderFormat {
    Yaml,
    Json,
}

#[derive(Subcommand)]
enum KnowledgeCommands {
    /// Read a knowledge artifact by path.
    Get {
        #[arg(short, long)]
        namespace: String,
        #[arg(long)]
        path: String,
    },
    /// Write a knowledge artifact from inline content or a file.
    Set {
        #[arg(short, long)]
        namespace: String,
        #[arg(long)]
        path: String,
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,
        #[arg(long, conflicts_with = "file")]
        content: Option<String>,
    },
    /// Delete a knowledge artifact by path.
    Delete {
        #[arg(short, long)]
        namespace: String,
        #[arg(long)]
        path: String,
    },
    /// Sync all markdown files in a directory into namespace knowledge.
    Sync {
        #[arg(short, long)]
        namespace: String,
        #[arg(long)]
        dir: String,
    },
}

#[derive(Subcommand)]
enum WorkflowCommands {
    /// Create a workflow run.
    RunCreate {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        #[arg(long, conflicts_with = "input_file")]
        input: Option<String>,
        #[arg(long, conflicts_with = "input")]
        input_file: Option<String>,
    },
    /// Get one workflow run and its step runs.
    RunGet {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        run_id: String,
    },
    /// List workflow runs.
    RunList {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        #[arg(long, default_value_t = 0)]
        page_size: i32,
        #[arg(long, default_value = "")]
        before_run_id: String,
    },
    /// Resume a suspended workflow step.
    RunResume {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        run_id: String,
        step_id: String,
        #[arg(long, conflicts_with = "resume_file")]
        resume: Option<String>,
        #[arg(long, conflicts_with = "resume")]
        resume_file: Option<String>,
    },
    /// Cancel a workflow run.
    RunCancel {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        run_id: String,
    },
    /// Stream workflow run events.
    RunEvents {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        run_id: String,
    },
}

#[derive(Debug)]
struct RestApplyPlan {
    method: reqwest::Method,
    path: String,
    payload: serde_json::Value,
    success_label: String,
}

#[derive(Debug, PartialEq, Eq)]
enum GrpcGetTarget {
    AgentTemplate {
        name: String,
    },
    Agent {
        ns: String,
        name: String,
    },
    McpServer {
        name: String,
    },
    Knowledge {
        ns: String,
        name: String,
    },
    Schedule {
        ns: String,
        name: String,
    },
    Channel {
        ns: String,
        name: String,
    },
    ChannelSubscription {
        ns: String,
        channel: String,
        name: String,
    },
    Workflow {
        ns: String,
        name: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
enum GrpcDeleteTarget {
    AgentTemplate {
        name: String,
    },
    McpServer {
        name: String,
    },
    Knowledge {
        ns: String,
        name: String,
    },
    Channel {
        ns: String,
        name: String,
    },
    ChannelSubscription {
        ns: String,
        channel: String,
        name: String,
    },
    Workflow {
        ns: String,
        name: String,
    },
}

#[derive(Debug)]
enum GrpcApplyPlan {
    Agent {
        ns: String,
        name: String,
        labels: HashMap<String, String>,
        definition: talon::gateway::rpc::manifests::AgentDefinition,
    },
    AgentTemplate {
        name: String,
        template: talon::gateway::rpc::manifests::AgentTemplate,
    },
    McpServer {
        name: String,
        server: talon::gateway::rpc::manifests::McpServer,
    },
    Knowledge {
        ns: String,
        name: String,
        knowledge: talon::gateway::rpc::manifests::Knowledge,
    },
    Channel {
        ns: String,
        name: String,
        channel: models::Channel,
    },
    ChannelSubscription {
        ns: String,
        channel_name: String,
        name: String,
        subscription: models::ChannelSubscription,
    },
    Workflow {
        ns: String,
        name: String,
        workflow: models::Workflow,
    },
}

fn build_rest_apply_plan(
    content: &str,
    payload: serde_json::Value,
    agent_exists: bool,
) -> Result<RestApplyPlan> {
    let raw = parse_raw_manifest(content)?;
    match raw.kind.as_str() {
        "AgentTemplate" => {
            let template = talon::manifest::parse_agent_template(content)?;
            let name = template
                .metadata
                .as_ref()
                .map(|m| m.name.clone())
                .unwrap_or_default();
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: "/v1/templates".to_string(),
                payload,
                success_label: format!("AgentTemplate '{}'", name),
            })
        }
        "MCPServer" | "McpServer" => {
            let server = talon::manifest::parse_mcp_server(content)?;
            let meta = server
                .metadata
                .as_ref()
                .context("MCPServer missing metadata")?;
            if !meta.namespace.is_empty() {
                anyhow::bail!(
                    "MCPServer metadata.namespace is not supported; MCP servers are system resources in Sys"
                );
            }
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: "/v1/mcp-servers".to_string(),
                payload,
                success_label: format!("MCPServer '{}'", meta.name),
            })
        }
        "Agent" => {
            let agent = talon::manifest::parse_agent(content)?;
            let definition = payload
                .get("definition")
                .cloned()
                .context("Agent payload missing definition")?;
            let labels = payload
                .get("labels")
                .cloned()
                .context("Agent payload missing labels")?;
            let path = if agent_exists {
                format!(
                    "/v1/ns/{}/agents/{}",
                    urlencoding::encode(&agent.ns),
                    urlencoding::encode(&agent.name)
                )
            } else {
                format!("/v1/ns/{}/agents", urlencoding::encode(&agent.ns))
            };
            let payload = if agent_exists {
                json!({
                    "ns": agent.ns,
                    "agent": agent.name,
                    "labels": labels,
                    "definition": definition,
                })
            } else {
                json!({
                    "ns": agent.ns,
                    "name": agent.name,
                    "labels": labels,
                    "definition": definition,
                })
            };
            Ok(RestApplyPlan {
                method: if agent_exists {
                    reqwest::Method::PUT
                } else {
                    reqwest::Method::POST
                },
                path,
                payload,
                success_label: format!("Agent '{}/{}'", agent.ns, agent.name),
            })
        }
        "McpServerBinding" => {
            let binding = talon::manifest::parse_mcp_server_binding(content)?;
            let meta = binding
                .metadata
                .as_ref()
                .context("McpServerBinding missing metadata")?;
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: format!(
                    "/v1/namespaces/{}/mcp-bindings",
                    urlencoding::encode(&meta.namespace)
                ),
                payload: json!({ "ns": meta.namespace, "binding": binding }),
                success_label: format!("McpServerBinding '{}/{}'", meta.namespace, meta.name),
            })
        }
        "Namespace" => {
            let namespace = talon::manifest::parse_namespace(content)?;
            let name = namespace.name;
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: format!("/v1/namespaces/{}", urlencoding::encode(&name)),
                payload: json!({
                    "name": name,
                    "recursive": true,
                    "labels": namespace.labels,
                }),
                success_label: format!("Namespace '{}'", name),
            })
        }
        "Knowledge" => {
            let knowledge = talon::manifest::parse_knowledge(content)?;
            let meta = knowledge
                .metadata
                .as_ref()
                .context("Knowledge missing metadata")?;
            if meta.namespace.is_empty() {
                anyhow::bail!("Knowledge metadata.namespace is required");
            }
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: format!(
                    "/v1/namespaces/{}/knowledge",
                    urlencoding::encode(&meta.namespace)
                ),
                payload,
                success_label: format!("Knowledge '{}/{}'", meta.namespace, meta.name),
            })
        }
        "Channel" => {
            let channel = talon::manifest::parse_channel(content)?;
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: format!("/v1/ns/{}/channels", urlencoding::encode(&channel.ns)),
                payload: json!({ "ns": channel.ns, "channel": channel }),
                success_label: "Channel".to_string(),
            })
        }
        "ChannelSubscription" => {
            let subscription = talon::manifest::parse_channel_subscription(content)?;
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: format!(
                    "/v1/ns/{}/channels/{}/subscriptions",
                    urlencoding::encode(&subscription.ns),
                    urlencoding::encode(&subscription.channel)
                ),
                payload: json!({
                    "ns": subscription.ns,
                    "channel": subscription.channel,
                    "subscription": subscription,
                }),
                success_label: "ChannelSubscription".to_string(),
            })
        }
        "Workflow" => {
            let workflow = talon::manifest::parse_workflow(content)?;
            Ok(RestApplyPlan {
                method: reqwest::Method::POST,
                path: format!("/v1/ns/{}/workflows", urlencoding::encode(&workflow.ns)),
                payload: json!({ "ns": workflow.ns, "workflow": workflow }),
                success_label: "Workflow".to_string(),
            })
        }
        other => anyhow::bail!("Unsupported manifest kind '{}'", other),
    }
}

fn feature_ts_type(kind: &str) -> &'static str {
    match kind {
        "integer" | "number" | "float" => "number",
        "boolean" => "boolean",
        _ => "string",
    }
}

fn sdk_method_for_template(
    template: &talon::gateway::rpc::manifests::AgentTemplate,
) -> Option<String> {
    let name = template.metadata.as_ref()?.name.clone();
    let method_name = format!("create{}", to_camel_case(&name));
    let mut args = Vec::new();

    if let Some(definition) = &template.definition {
        if let Some(talon::gateway::rpc::manifests::agent_definition::Source::CustomSpec(spec)) =
            definition.source.as_ref()
        {
            for f in &spec.features {
                let opt = if f.required { "" } else { "?" };
                args.push(format!("{}{}: {}", f.name, opt, feature_ts_type(&f.r#type)));
            }
        }
    }

    let param_str = if args.is_empty() {
        String::new()
    } else {
        format!(", inputs: {{ {} }}", args.join(", "))
    };
    let input_pass = if args.is_empty() {
        "inputs: {}"
    } else {
        "inputs"
    };

    Some(format!(
        r#"  async {method_name}(workspaceId: string{param_str}): Promise<any> {{
    return fetch(`${{this.endpoint}}/api/agents`, {{
      method: "POST",
      headers: {{ "Content-Type": "application/json" }},
      body: JSON.stringify({{ template: "{raw_name}", namespace: workspaceId, {input_pass} }})
    }}).then(r => r.json());
  }}"#,
        method_name = method_name,
        param_str = param_str,
        raw_name = name,
        input_pass = input_pass,
    ))
}

fn sdk_methods_from_dir(dir: &str) -> Result<Vec<String>> {
    let mut class_methods = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().unwrap_or_default() != "yaml" {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        if let Ok(template) = talon::manifest::parse_agent_template(&content) {
            if let Some(method) = sdk_method_for_template(&template) {
                class_methods.push(method);
            }
        }
    }
    Ok(class_methods)
}

fn grpc_get_target(kind: &str, name: &str, namespace: Option<&String>) -> Result<GrpcGetTarget> {
    match kind.to_lowercase().as_str() {
        "agenttemplate" | "templates" | "template" => Ok(GrpcGetTarget::AgentTemplate {
            name: name.to_string(),
        }),
        "agent" | "agents" => {
            let (ns, agent_name) = agent_lookup_target(name, namespace);
            Ok(GrpcGetTarget::Agent {
                ns,
                name: agent_name,
            })
        }
        "mcpserver" | "mcpservers" | "mcp" => Ok(GrpcGetTarget::McpServer {
            name: name.to_string(),
        }),
        "knowledge" | "knowledgeartifact" | "knowledgeartifacts" => {
            let ns = namespace
                .cloned()
                .context("Knowledge get requires --namespace")?;
            Ok(GrpcGetTarget::Knowledge {
                ns,
                name: name.to_string(),
            })
        }
        "schedule" | "schedules" => {
            let ns = namespace
                .cloned()
                .context("Schedule get requires --namespace")?;
            Ok(GrpcGetTarget::Schedule {
                ns,
                name: name.to_string(),
            })
        }
        "channel" | "channels" => {
            let ns = namespace
                .cloned()
                .context("Channel get requires --namespace")?;
            Ok(GrpcGetTarget::Channel {
                ns,
                name: name.to_string(),
            })
        }
        "channelsubscription"
        | "channelsubscriptions"
        | "channel-subscription"
        | "channel-subscriptions" => {
            let ns = namespace
                .cloned()
                .context("ChannelSubscription get requires --namespace")?;
            let (channel, subscription) = name
                .split_once('/')
                .context("ChannelSubscription name must be '<channel>/<subscription>'")?;
            Ok(GrpcGetTarget::ChannelSubscription {
                ns,
                channel: channel.to_string(),
                name: subscription.to_string(),
            })
        }
        "workflow" | "workflows" => {
            let ns = namespace
                .cloned()
                .context("Workflow get requires --namespace")?;
            Ok(GrpcGetTarget::Workflow {
                ns,
                name: name.to_string(),
            })
        }
        other => anyhow::bail!("Unsupported resource kind '{}'", other),
    }
}

fn grpc_delete_target(
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<GrpcDeleteTarget> {
    match kind.to_lowercase().as_str() {
        "agenttemplate" | "templates" | "template" => Ok(GrpcDeleteTarget::AgentTemplate {
            name: name.to_string(),
        }),
        "mcpserver" | "mcpservers" | "mcp" => Ok(GrpcDeleteTarget::McpServer {
            name: name.to_string(),
        }),
        "knowledge" | "knowledgeartifact" | "knowledgeartifacts" => {
            let ns = namespace
                .cloned()
                .context("Knowledge delete requires --namespace")?;
            Ok(GrpcDeleteTarget::Knowledge {
                ns,
                name: name.to_string(),
            })
        }
        "channel" | "channels" => {
            let ns = namespace
                .cloned()
                .context("Channel delete requires --namespace")?;
            Ok(GrpcDeleteTarget::Channel {
                ns,
                name: name.to_string(),
            })
        }
        "channelsubscription"
        | "channelsubscriptions"
        | "channel-subscription"
        | "channel-subscriptions" => {
            let ns = namespace
                .cloned()
                .context("ChannelSubscription delete requires --namespace")?;
            let (channel, subscription) = name
                .split_once('/')
                .context("ChannelSubscription name must be '<channel>/<subscription>'")?;
            Ok(GrpcDeleteTarget::ChannelSubscription {
                ns,
                channel: channel.to_string(),
                name: subscription.to_string(),
            })
        }
        "workflow" | "workflows" => {
            let ns = namespace
                .cloned()
                .context("Workflow delete requires --namespace")?;
            Ok(GrpcDeleteTarget::Workflow {
                ns,
                name: name.to_string(),
            })
        }
        other => anyhow::bail!("Unsupported resource kind '{}'", other),
    }
}

fn build_grpc_apply_plan(content: &str) -> Result<GrpcApplyPlan> {
    match parse_raw_manifest(content)?.kind.as_str() {
        "Agent" => {
            let agent = talon::manifest::parse_agent(content)?;
            let definition = agent
                .definition
                .clone()
                .context("Agent definition must be provided")?;
            Ok(GrpcApplyPlan::Agent {
                ns: agent.ns,
                name: agent.name,
                labels: agent.labels,
                definition,
            })
        }
        "AgentTemplate" => {
            let template = talon::manifest::parse_agent_template(content)?;
            let name = template
                .metadata
                .as_ref()
                .map(|m| m.name.clone())
                .unwrap_or_default();
            Ok(GrpcApplyPlan::AgentTemplate { name, template })
        }
        "MCPServer" | "McpServer" => {
            let server = talon::manifest::parse_mcp_server(content)?;
            let meta = server
                .metadata
                .as_ref()
                .context("MCPServer missing metadata")?;
            if !meta.namespace.is_empty() {
                anyhow::bail!(
                    "MCPServer metadata.namespace is not supported; MCP servers are system resources in Sys"
                );
            }
            Ok(GrpcApplyPlan::McpServer {
                name: meta.name.clone(),
                server,
            })
        }
        "Knowledge" => {
            let knowledge = talon::manifest::parse_knowledge(content)?;
            let meta = knowledge
                .metadata
                .as_ref()
                .context("Knowledge missing metadata")?;
            if meta.namespace.is_empty() {
                anyhow::bail!("Knowledge metadata.namespace is required");
            }
            Ok(GrpcApplyPlan::Knowledge {
                ns: meta.namespace.clone(),
                name: meta.name.clone(),
                knowledge,
            })
        }
        "Channel" => {
            let channel = talon::manifest::parse_channel(content)?;
            Ok(GrpcApplyPlan::Channel {
                ns: channel.ns.clone(),
                name: channel.name.clone(),
                channel,
            })
        }
        "ChannelSubscription" => {
            let subscription = talon::manifest::parse_channel_subscription(content)?;
            Ok(GrpcApplyPlan::ChannelSubscription {
                ns: subscription.ns.clone(),
                channel_name: subscription.channel.clone(),
                name: subscription.name.clone(),
                subscription,
            })
        }
        "Workflow" => {
            let workflow = talon::manifest::parse_workflow(content)?;
            Ok(GrpcApplyPlan::Workflow {
                ns: workflow.ns.clone(),
                name: workflow.name.clone(),
                workflow,
            })
        }
        other => anyhow::bail!("Unsupported manifest kind '{}'", other),
    }
}

async fn grpc_get_yaml(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
        .connect()
        .await
        .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
    let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);

    match grpc_get_target(kind, name, namespace)? {
        GrpcGetTarget::AgentTemplate { name } => {
            let resp = client
                .get_agent_template(GetAgentTemplateRequest { name: name.clone() })
                .await
                .with_context(|| format!("Failed to fetch AgentTemplate '{}'", name))?;
            let template = resp.into_inner().template.context("Resource not found.")?;
            talon::manifest::render_agent_template_yaml(&template)
        }
        GrpcGetTarget::Agent { ns, name } => {
            let resp = client
                .get_agent(talon::gateway::rpc::proto::GetAgentRequest {
                    ns,
                    name: name.clone(),
                })
                .await
                .with_context(|| format!("Failed to fetch Agent '{}'", name))?;
            let agent = resp.into_inner().agent.context("Agent not found.")?;
            talon::manifest::render_agent_yaml(&agent)
        }
        GrpcGetTarget::McpServer { name } => {
            let resp = client
                .get_mcp_server(GetMcpServerRequest { name: name.clone() })
                .await
                .with_context(|| format!("Failed to fetch MCPServer '{}'", name))?;
            let server = resp.into_inner().server.context("Resource not found.")?;
            talon::manifest::render_mcp_server_yaml(&server)
        }
        GrpcGetTarget::Knowledge { ns, name } => {
            let resp = client
                .get_namespace_knowledge(GetNamespaceKnowledgeRequest {
                    ns: ns.clone(),
                    name: name.clone(),
                })
                .await
                .with_context(|| format!("Failed to fetch Knowledge '{}/{}'", ns, name))?;
            let knowledge = resp
                .into_inner()
                .knowledge
                .context("Knowledge not found.")?;
            talon::manifest::render_knowledge_yaml(&knowledge)
        }
        GrpcGetTarget::Schedule { ns, name } => {
            let resp = client
                .get_schedule(GetScheduleRequest {
                    ns: ns.clone(),
                    name: name.clone(),
                })
                .await
                .with_context(|| format!("Failed to fetch Schedule '{}/{}'", ns, name))?;
            let schedule = resp.into_inner().schedule.context("Schedule not found.")?;
            serde_yaml::to_string(&schedule_json(&schedule))
                .context("Failed to serialize Schedule YAML")
        }
        GrpcGetTarget::Channel { ns, name } => {
            let resp = client
                .get_channel(GetChannelRequest {
                    ns: ns.clone(),
                    name: name.clone(),
                })
                .await
                .with_context(|| format!("Failed to fetch Channel '{}/{}'", ns, name))?;
            let channel = resp.into_inner().channel.context("Channel not found.")?;
            talon::manifest::render_channel_yaml(&channel)
        }
        GrpcGetTarget::ChannelSubscription { ns, channel, name } => {
            let resp = client
                .get_channel_subscription(GetChannelSubscriptionRequest {
                    ns: ns.clone(),
                    channel: channel.clone(),
                    name: name.clone(),
                })
                .await
                .with_context(|| {
                    format!(
                        "Failed to fetch ChannelSubscription '{}/{}/{}'",
                        ns, channel, name
                    )
                })?;
            let subscription = resp
                .into_inner()
                .subscription
                .context("ChannelSubscription not found.")?;
            talon::manifest::render_channel_subscription_yaml(&subscription)
        }
        GrpcGetTarget::Workflow { ns, name } => {
            let resp = client
                .get_workflow(GetWorkflowRequest {
                    ns: ns.clone(),
                    name: name.clone(),
                })
                .await
                .with_context(|| format!("Failed to fetch Workflow '{}/{}'", ns, name))?;
            let workflow = resp.into_inner().workflow.context("Workflow not found.")?;
            talon::manifest::render_workflow_yaml(&workflow)
        }
    }
}

async fn grpc_delete_resource(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
        .connect()
        .await
        .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
    let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);

    match grpc_delete_target(kind, name, namespace)? {
        GrpcDeleteTarget::AgentTemplate { name } => {
            client
                .delete_agent_template(DeleteAgentTemplateRequest { name: name.clone() })
                .await
                .with_context(|| format!("Failed to delete AgentTemplate '{}'", name))?;
            Ok(format!("✓ AgentTemplate '{}' deleted successfully.", name))
        }
        GrpcDeleteTarget::McpServer { name } => {
            client
                .delete_mcp_server(DeleteMcpServerRequest { name: name.clone() })
                .await
                .with_context(|| format!("Failed to delete MCPServer '{}'", name))?;
            Ok(format!("✓ MCPServer '{}' deleted successfully.", name))
        }
        GrpcDeleteTarget::Knowledge { ns, name } => {
            client
                .delete_namespace_knowledge(DeleteNamespaceKnowledgeRequest {
                    ns: ns.clone(),
                    name: name.clone(),
                })
                .await
                .with_context(|| format!("Failed to delete Knowledge '{}/{}'", ns, name))?;
            Ok(format!(
                "✓ Knowledge '{}/{}' deleted successfully.",
                ns, name
            ))
        }
        GrpcDeleteTarget::Channel { ns, name } => {
            client
                .delete_channel(DeleteChannelRequest {
                    ns: ns.clone(),
                    name: name.clone(),
                })
                .await
                .with_context(|| format!("Failed to delete Channel '{}/{}'", ns, name))?;
            Ok(format!("✓ Channel '{}/{}' deleted successfully.", ns, name))
        }
        GrpcDeleteTarget::ChannelSubscription { ns, channel, name } => {
            client
                .delete_channel_subscription(DeleteChannelSubscriptionRequest {
                    ns: ns.clone(),
                    channel: channel.clone(),
                    name: name.clone(),
                })
                .await
                .with_context(|| {
                    format!(
                        "Failed to delete ChannelSubscription '{}/{}/{}'",
                        ns, channel, name
                    )
                })?;
            Ok(format!(
                "✓ ChannelSubscription '{}/{}/{}' deleted successfully.",
                ns, channel, name
            ))
        }
        GrpcDeleteTarget::Workflow { ns, name } => {
            client
                .delete_workflow(DeleteWorkflowRequest {
                    ns: ns.clone(),
                    name: name.clone(),
                })
                .await
                .with_context(|| format!("Failed to delete Workflow '{}/{}'", ns, name))?;
            Ok(format!(
                "✓ Workflow '{}/{}' deleted successfully.",
                ns, name
            ))
        }
    }
}

async fn grpc_apply_manifest(cli: &Cli, content: &str) -> Result<String> {
    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
        .connect()
        .await
        .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
    let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);

    match build_grpc_apply_plan(content)? {
        GrpcApplyPlan::Agent {
            ns,
            name,
            labels,
            definition,
        } => {
            let existing = client
                .get_agent(talon::gateway::rpc::proto::GetAgentRequest {
                    ns: ns.clone(),
                    name: name.clone(),
                })
                .await;

            match existing {
                Ok(_) => {
                    client
                        .modify_agent(ModifyAgentRequest {
                            ns: ns.clone(),
                            agent: name.clone(),
                            definition: Some(definition),
                            labels,
                        })
                        .await
                        .with_context(|| format!("Gateway rejected Agent '{}/{}'", ns, name))?;
                }
                Err(status) if status.code() == tonic::Code::NotFound => {
                    client
                        .create_agent(CreateAgentRequest {
                            ns: ns.clone(),
                            name: Some(name.clone()),
                            definition: Some(definition),
                            labels,
                        })
                        .await
                        .with_context(|| format!("Gateway rejected Agent '{}/{}'", ns, name))?;
                }
                Err(status) => return Err(status.into()),
            }
            Ok(format!("✓ Agent '{}/{}' applied successfully.", ns, name))
        }
        GrpcApplyPlan::AgentTemplate { name, template } => {
            client
                .create_agent_template(CreateAgentTemplateRequest {
                    template: Some(template),
                })
                .await
                .with_context(|| format!("Gateway rejected template '{}'", name))?;
            Ok(format!("✓ AgentTemplate '{}' applied successfully.", name))
        }
        GrpcApplyPlan::McpServer { name, server } => {
            client
                .create_mcp_server(CreateMcpServerRequest {
                    server: Some(server),
                })
                .await
                .context("Gateway rejected MCPServer")?;
            Ok(format!("✓ MCPServer '{}' applied successfully.", name))
        }
        GrpcApplyPlan::Knowledge {
            ns,
            name,
            knowledge,
        } => {
            client
                .create_namespace_knowledge(CreateNamespaceKnowledgeRequest {
                    ns: ns.clone(),
                    knowledge: Some(knowledge),
                })
                .await
                .with_context(|| format!("Gateway rejected Knowledge '{}/{}'", ns, name))?;
            Ok(format!(
                "✓ Knowledge '{}/{}' applied successfully.",
                ns, name
            ))
        }
        GrpcApplyPlan::Channel { ns, name, channel } => {
            let existing = client
                .get_channel(GetChannelRequest {
                    ns: ns.clone(),
                    name: name.clone(),
                })
                .await;
            match existing {
                Ok(_) => {
                    client
                        .modify_channel(ModifyChannelRequest {
                            ns: ns.clone(),
                            name: name.clone(),
                            channel: Some(channel),
                        })
                        .await
                        .with_context(|| format!("Gateway rejected Channel '{}/{}'", ns, name))?;
                }
                Err(status) if status.code() == tonic::Code::NotFound => {
                    client
                        .create_channel(CreateChannelRequest {
                            ns: ns.clone(),
                            channel: Some(channel),
                        })
                        .await
                        .with_context(|| format!("Gateway rejected Channel '{}/{}'", ns, name))?;
                }
                Err(status) => return Err(status.into()),
            }
            Ok(format!("✓ Channel '{}/{}' applied successfully.", ns, name))
        }
        GrpcApplyPlan::ChannelSubscription {
            ns,
            channel_name,
            name,
            subscription,
        } => {
            let existing = client
                .get_channel_subscription(GetChannelSubscriptionRequest {
                    ns: ns.clone(),
                    channel: channel_name.clone(),
                    name: name.clone(),
                })
                .await;
            match existing {
                Ok(_) => {
                    client
                        .modify_channel_subscription(ModifyChannelSubscriptionRequest {
                            ns: ns.clone(),
                            channel: channel_name.clone(),
                            name: name.clone(),
                            subscription: Some(subscription),
                        })
                        .await
                        .with_context(|| {
                            format!(
                                "Gateway rejected ChannelSubscription '{}/{}/{}'",
                                ns, channel_name, name
                            )
                        })?;
                }
                Err(status) if status.code() == tonic::Code::NotFound => {
                    client
                        .create_channel_subscription(CreateChannelSubscriptionRequest {
                            ns: ns.clone(),
                            channel: channel_name.clone(),
                            subscription: Some(subscription),
                        })
                        .await
                        .with_context(|| {
                            format!(
                                "Gateway rejected ChannelSubscription '{}/{}/{}'",
                                ns, channel_name, name
                            )
                        })?;
                }
                Err(status) => return Err(status.into()),
            }
            Ok(format!(
                "✓ ChannelSubscription '{}/{}/{}' applied successfully.",
                ns, channel_name, name
            ))
        }
        GrpcApplyPlan::Workflow { ns, name, workflow } => {
            client
                .create_workflow(CreateWorkflowRequest {
                    ns: ns.clone(),
                    workflow: Some(workflow),
                })
                .await
                .with_context(|| format!("Gateway rejected Workflow '{}/{}'", ns, name))?;
            Ok(format!(
                "✓ Workflow '{}/{}' applied successfully.",
                ns, name
            ))
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    talon::security::install_jwt_crypto_provider();
    let mut cli = Cli::parse();

    if let Ok(env_gateway) = std::env::var("TALON_GATEWAY") {
        cli.gateway = env_gateway;
    }
    if cli.password.is_none() {
        cli.password = resolve_gateway_password(&cli);
    }

    let outcome = run_cli(&cli).await?;
    if let Some(code) = outcome.exit_code {
        std::process::exit(code);
    }

    Ok(())
}

struct RunOutcome {
    exit_code: Option<i32>,
}

async fn run_cli(cli: &Cli) -> Result<RunOutcome> {
    match &cli.command {
        Commands::Auth { command } => {
            let secret = resolve_gateway_jwt_secret(cli)
                .context("TALON_JWT_SECRET or GATEWAY_JWT_SECRET is required")?;
            let token = match command {
                AuthCommands::RootToken {
                    subject,
                    ttl_seconds,
                } => mint_root_jwt(&secret, subject, *ttl_seconds)?,
                AuthCommands::AgentToken {
                    namespace,
                    agent,
                    subject,
                    ttl_seconds,
                } => mint_agent_jwt(&secret, namespace, agent, subject, *ttl_seconds)?,
                AuthCommands::SessionToken {
                    namespace,
                    agent,
                    session,
                    subject,
                    ttl_seconds,
                } => mint_session_jwt(&secret, namespace, agent, session, subject, *ttl_seconds)?,
                AuthCommands::ChannelToken {
                    namespace,
                    channel,
                    subject,
                    ttl_seconds,
                } => mint_channel_jwt(&secret, namespace, channel, subject, *ttl_seconds)?,
            };
            println!("{}", token);
            return Ok(RunOutcome { exit_code: None });
        }
        Commands::Knowledge { command } => match command {
            KnowledgeCommands::Get { namespace, path } => {
                let knowledge = knowledge_get(&cli, namespace, path).await?;
                let Some(knowledge) = knowledge else {
                    eprintln!("Knowledge '{}/{}' not found.", namespace, path);
                    return Ok(RunOutcome { exit_code: Some(1) });
                };
                let content = knowledge
                    .spec
                    .as_ref()
                    .map(|spec| spec.content.clone())
                    .unwrap_or_default();
                print!("{}", content);
                if !content.ends_with('\n') {
                    println!();
                }
                return Ok(RunOutcome { exit_code: None });
            }
            KnowledgeCommands::Set {
                namespace,
                path,
                file,
                content,
            } => {
                let value = read_knowledge_content(file, content)?;
                knowledge_set(&cli, namespace, path, value).await?;
                println!("✓ Knowledge '{}/{}' written successfully.", namespace, path);
                return Ok(RunOutcome { exit_code: None });
            }
            KnowledgeCommands::Delete { namespace, path } => {
                knowledge_delete(&cli, namespace, path).await?;
                println!("✓ Knowledge '{}/{}' deleted successfully.", namespace, path);
                return Ok(RunOutcome { exit_code: None });
            }
            KnowledgeCommands::Sync { namespace, dir } => {
                let root = Path::new(dir);
                let (synced_count, unsynced_existing) =
                    sync_knowledge_dir(&cli, namespace, dir).await?;
                println!(
                    "✓ Synced {} knowledge artifact(s) into '{}'.",
                    synced_count, namespace
                );
                if !unsynced_existing.is_empty() {
                    eprintln!(
                        "Note: {} existing knowledge artifact(s) in '{}' were left untouched because they are not present in '{}'.",
                        unsynced_existing.len(),
                        namespace,
                        root.display()
                    );
                }
                return Ok(RunOutcome { exit_code: None });
            }
        },
        Commands::Workflow { command } => match command {
            WorkflowCommands::RunCreate {
                namespace,
                workflow,
                input,
                input_file,
            } => {
                let value = workflow_run_create(
                    cli,
                    namespace,
                    workflow,
                    read_json_arg(input, input_file)?,
                )
                .await?;
                println!("{}", serde_json::to_string_pretty(&value)?);
                return Ok(RunOutcome { exit_code: None });
            }
            WorkflowCommands::RunGet {
                namespace,
                workflow,
                run_id,
            } => {
                let value = workflow_run_get(cli, namespace, workflow, run_id).await?;
                println!("{}", serde_json::to_string_pretty(&value)?);
                return Ok(RunOutcome { exit_code: None });
            }
            WorkflowCommands::RunList {
                namespace,
                workflow,
                page_size,
                before_run_id,
            } => {
                let value =
                    workflow_run_list(cli, namespace, workflow, *page_size, before_run_id).await?;
                println!("{}", serde_json::to_string_pretty(&value)?);
                return Ok(RunOutcome { exit_code: None });
            }
            WorkflowCommands::RunResume {
                namespace,
                workflow,
                run_id,
                step_id,
                resume,
                resume_file,
            } => {
                let value = workflow_run_resume(
                    cli,
                    namespace,
                    workflow,
                    run_id,
                    step_id,
                    read_json_arg(resume, resume_file)?,
                )
                .await?;
                println!("{}", serde_json::to_string_pretty(&value)?);
                return Ok(RunOutcome { exit_code: None });
            }
            WorkflowCommands::RunCancel {
                namespace,
                workflow,
                run_id,
            } => {
                let value = workflow_run_cancel(cli, namespace, workflow, run_id).await?;
                println!("{}", serde_json::to_string_pretty(&value)?);
                return Ok(RunOutcome { exit_code: None });
            }
            WorkflowCommands::RunEvents {
                namespace,
                workflow,
                run_id,
            } => {
                workflow_run_events(cli, namespace, workflow, run_id).await?;
                return Ok(RunOutcome { exit_code: None });
            }
        },
        Commands::Apply { file, vars } => {
            let content = render_manifest_file(file, vars)?;

            if cli.rest {
                let agent_exists = if parse_raw_manifest(&content)?.kind == "Agent" {
                    let agent = talon::manifest::parse_agent(&content)?;
                    let get_path = format!(
                        "/v1/ns/{}/agents/{}",
                        urlencoding::encode(&agent.ns),
                        urlencoding::encode(&agent.name)
                    );
                    rest_request_json(&cli, reqwest::Method::GET, &get_path, None)
                        .await
                        .is_ok()
                } else {
                    false
                };
                println!(
                    "{}",
                    rest_apply_manifest(&cli, &content, agent_exists).await?
                );
                return Ok(RunOutcome { exit_code: None });
            }

            println!("{}", grpc_apply_manifest(&cli, &content).await?);
        }

        Commands::Render { file, vars, format } => {
            let content = render_manifest_file(file, vars)?;
            match format {
                RenderFormat::Yaml => {
                    print!("{}", content);
                }
                RenderFormat::Json => {
                    let payload = render_json_payload(&content)?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&payload)
                            .context("Failed to serialize manifest JSON")?
                    );
                }
            }
        }

        Commands::Get {
            kind,
            name,
            namespace,
        } => {
            if cli.rest {
                println!(
                    "{}",
                    rest_get_yaml(&cli, kind, name, namespace.as_ref()).await?
                );
                return Ok(RunOutcome { exit_code: None });
            }
            println!(
                "{}",
                grpc_get_yaml(&cli, kind, name, namespace.as_ref()).await?
            );
        }

        Commands::Delete {
            kind,
            name,
            namespace,
        } => {
            if cli.rest {
                println!(
                    "{}",
                    rest_delete_resource(&cli, kind, name, namespace.as_ref()).await?
                );
                return Ok(RunOutcome { exit_code: None });
            }
            println!(
                "{}",
                grpc_delete_resource(&cli, kind, name, namespace.as_ref()).await?
            );
        }

        Commands::Gen { dir, out } => {
            println!("Generating Talon Client SDK from: {}", dir);
            let class_methods = sdk_methods_from_dir(dir)?;

            let full_file = format!(
                r#"// Auto-generated by talon-cli gen
export class TalonClient {{
  constructor(private endpoint: string) {{}}

{}
}}
"#,
                class_methods.join("\n\n")
            );

            fs::write(out, full_file)?;
            println!("Wrote API client to {}", out);
        }
    }

    Ok(RunOutcome { exit_code: None })
}

fn parse_raw_manifest(content: &str) -> Result<talon::manifest::RawManifest> {
    serde_yaml::from_str(content).context("Failed to parse manifest YAML")
}

fn render_manifest_file(file: &str, vars: &[String]) -> Result<String> {
    let content = fs::read_to_string(file)
        .with_context(|| format!("Failed to read manifest file: {}", file))?;
    let vars = parse_vars(vars)?;
    let rendered = render_manifest_template(&content, &vars)
        .with_context(|| format!("Failed to render manifest file: {}", file))?;
    resolve_manifest_sources(file, &rendered)
}

fn resolve_manifest_sources(file: &str, rendered: &str) -> Result<String> {
    let raw = parse_raw_manifest(rendered)?;
    if raw.kind != "Knowledge" {
        return Ok(rendered.to_string());
    }

    let mut manifest: serde_yaml::Value =
        serde_yaml::from_str(rendered).context("Failed to parse rendered Knowledge manifest")?;
    let file_manifest: KnowledgeManifestFile = serde_yaml::from_str(rendered)
        .context("Failed to parse Knowledge manifest source directives")?;

    let content = match (
        file_manifest.spec.content.clone(),
        file_manifest.spec.content_from_file.clone(),
    ) {
        (Some(content), None) => content,
        (None, Some(path)) => {
            let base_dir = Path::new(file).parent().unwrap_or_else(|| Path::new("."));
            let full_path = canonicalize_manifest_path(base_dir, &path);
            fs::read_to_string(&full_path).with_context(|| {
                format!(
                    "Failed to read Knowledge contentFromFile '{}'",
                    full_path.display()
                )
            })?
        }
        (Some(_), Some(_)) => {
            anyhow::bail!("Knowledge manifest spec can set only one of content or contentFromFile")
        }
        (None, None) => {
            anyhow::bail!("Knowledge manifest spec must set one of content or contentFromFile")
        }
    };

    if let Some(spec) = manifest
        .get_mut("spec")
        .and_then(|value| value.as_mapping_mut())
    {
        spec.remove(&serde_yaml::Value::String("contentFromFile".to_string()));
        spec.insert(
            serde_yaml::Value::String("content".to_string()),
            serde_yaml::Value::String(content),
        );
    }

    serde_yaml::to_string(&manifest).context("Failed to serialize resolved Knowledge manifest")
}

fn canonicalize_manifest_path(base_dir: &Path, raw_path: &str) -> PathBuf {
    let path = Path::new(raw_path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

fn parse_vars(entries: &[String]) -> Result<HashMap<String, String>> {
    let mut vars = HashMap::new();
    for entry in entries {
        let (key, value) = entry
            .split_once('=')
            .with_context(|| format!("Invalid --var '{}', expected KEY=VALUE", entry))?;
        if key.is_empty() {
            anyhow::bail!("Invalid --var '{}', key cannot be empty", entry);
        }
        vars.insert(key.to_string(), value.to_string());
    }
    Ok(vars)
}

fn render_manifest_template(template: &str, vars: &HashMap<String, String>) -> Result<String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.add_template("manifest", template)
        .context("Failed to compile manifest template")?;
    let rendered = env
        .get_template("manifest")
        .context("Missing manifest template")?
        .render(context! { vars => vars })
        .context("Failed to render manifest template")?;
    Ok(rendered)
}

fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '-' || c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{
        agent_lookup_target, auth_interceptor, build_grpc_apply_plan, build_knowledge,
        build_rest_apply_plan, canonicalize_manifest_path, collect_markdown_files,
        ensure_rest_stream_buffer_within_limit, feature_ts_type, grpc_apply_manifest,
        grpc_delete_resource, grpc_delete_target, grpc_get_target, grpc_get_yaml, knowledge_delete,
        knowledge_get, knowledge_list, knowledge_resource_name, knowledge_set,
        manifest_json_payload, mint_agent_jwt, mint_channel_jwt, mint_root_jwt, mint_session_jwt,
        parse_raw_manifest, parse_vars, print_stream_event_line, read_json_arg,
        read_knowledge_content, relative_knowledge_path, render_json_payload,
        render_manifest_file, render_manifest_template, render_rest_get_yaml,
        resolve_authorization_header, resolve_manifest_sources, rest_apply_manifest, rest_client,
        rest_delete_path, rest_delete_resource, rest_get_path, rest_get_yaml, rest_request_json,
        run_cli, schedule_json, sdk_method_for_template, sdk_methods_from_dir, sync_knowledge_dir,
        to_camel_case, workflow_run_list, AuthCommands, Cli, Commands, GrpcApplyPlan,
        GrpcDeleteTarget, GrpcGetTarget, KnowledgeCommands, RenderFormat, WorkflowCommands,
        MAX_REST_STREAM_BUFFER_BYTES,
    };
    use axum::{
        extract::{Path as AxumPath, State},
        http::StatusCode,
        routing::{delete, get, post},
        Json, Router,
    };
    use futures::{stream, Stream};
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::net::SocketAddr;
    use std::path::Path;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::net::TcpListener;
    use tokio::sync::RwLock;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::service::Interceptor;
    use tonic::transport::Server;

    use talon::control::keys;
    use talon::control::scheduler::NoopSchedulerBackend;
    use talon::control::scheduler::SchedulerBackend;
    use talon::control::{KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt};
    use talon::gateway::rpc::{models, proto, GrpcGatewayHandler};
    use talon::gateway::server::Gateway;
    use talon::gateway::session_streams::SessionStreamHub;

    fn env_mutex() -> &'static Mutex<()> {
        static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        MUTEX.get_or_init(|| Mutex::new(()))
    }

    fn temp_root(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn cli() -> Cli {
        Cli {
            gateway: "http://localhost:50051".to_string(),
            password: None,
            token: None,
            jwt_secret: None,
            rest: false,
            command: Commands::Knowledge {
                command: KnowledgeCommands::Get {
                    namespace: "conic".to_string(),
                    path: "docs/test.md".to_string(),
                },
            },
        }
    }

    fn rest_cli(gateway: String) -> Cli {
        Cli {
            gateway,
            rest: true,
            ..cli()
        }
    }

    #[derive(Default)]
    struct MockKvStore {
        data: RwLock<HashMap<keys::ResourceKey, Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, k: &keys::ResourceKey) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.data.read().await.get(k).cloned())
        }

        async fn set(&self, k: &keys::ResourceKey, v: &[u8]) -> anyhow::Result<()> {
            self.data.write().await.insert(k.clone(), v.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            k: &keys::ResourceKey,
            old: Option<&[u8]>,
            new: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.write().await;
            let matches = match (data.get(k), old) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(k.clone(), new.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, k: &keys::ResourceKey) -> anyhow::Result<()> {
            self.data.write().await.remove(k);
            Ok(())
        }

        async fn list_keys(
            &self,
            list: &keys::ResourceList,
        ) -> anyhow::Result<Vec<keys::ResourceKey>> {
            let mut keys = self
                .data
                .read()
                .await
                .keys()
                .filter_map(|key| list.matches(key).then(|| key.clone()))
                .collect::<Vec<_>>();
            keys.sort();
            Ok(keys)
        }

        async fn list_keys_page(
            &self,
            list: &keys::ResourceList,
            before_name: Option<&str>,
            limit: usize,
        ) -> anyhow::Result<Vec<keys::ResourceKey>> {
            Ok(talon::control::page_keys_desc(
                self.list_keys(list).await?,
                before_name,
                limit,
            ))
        }

        async fn list_entries_page(
            &self,
            list: &keys::ResourceList,
            before_name: Option<&str>,
            limit: usize,
        ) -> anyhow::Result<Vec<(keys::ResourceKey, Vec<u8>)>> {
            Ok(talon::control::page_entries_desc(
                self.list_entries(list).await?,
                before_name,
                limit,
            ))
        }
    }

    #[derive(Default)]
    struct MockPubSub;

    #[async_trait::async_trait]
    impl MessagePublisher for MockPubSub {
        async fn publish(&self, _topic: &str, _message: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn Stream<Item = Vec<u8>> + Send>>> {
            Ok(Box::pin(stream::empty()))
        }
    }

    async fn serve_grpc_gateway() -> (SocketAddr, Arc<MockKvStore>) {
        let kv = Arc::new(MockKvStore::default());
        let pubsub = Arc::new(MockPubSub);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let gateway = Arc::new(Gateway {
            auth_config: None,
            kv: kv.clone(),
            pubsub: pubsub.clone(),
            scheduler: Arc::new(NoopSchedulerBackend) as Arc<dyn SchedulerBackend>,
            objects: talon::control::object_store::default_object_store(),
            session_streams: Arc::new(SessionStreamHub::new(pubsub)),
        });
        let handler = GrpcGatewayHandler { gateway };
        tokio::spawn(async move {
            Server::builder()
                .add_service(proto::gateway_service_server::GatewayServiceServer::new(
                    handler,
                ))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (addr, kv)
    }

    async fn seed_namespace(kv: &Arc<MockKvStore>, namespace: &str) {
        let value = models::Namespace {
            name: namespace.to_string(),
            parent: String::new(),
            is_deleted: false,
            deleted_at: 0,
            labels: HashMap::new(),
        };
        kv.set_msg(&keys::namespace_metadata(namespace), &value)
            .await
            .unwrap();
    }

    #[test]
    fn renders_minijinja_vars() {
        let mut vars = HashMap::new();
        vars.insert(
            "conic_mcp_target_url".to_string(),
            "https://api.example.com/mcp".to_string(),
        );
        let rendered = render_manifest_template("target: {{ vars.conic_mcp_target_url }}\n", &vars)
            .expect("render should succeed");
        assert_eq!(rendered.trim_end(), "target: https://api.example.com/mcp");
    }

    #[test]
    fn parse_vars_rejects_invalid_pairs() {
        let err = parse_vars(&["missing-separator".to_string()]).expect_err("should fail");
        assert!(err.to_string().contains("expected KEY=VALUE"));
    }

    #[test]
    fn parse_vars_rejects_empty_key() {
        let err = parse_vars(&["=value".to_string()]).expect_err("empty key should fail");
        assert!(err.to_string().contains("key cannot be empty"));
    }

    #[test]
    fn read_json_arg_normalizes_json_and_uses_generic_errors() {
        let normalized = read_json_arg(&Some(r#"{ "ok": true }"#.to_string()), &None).unwrap();
        assert_eq!(normalized, r#"{"ok":true}"#);

        let err = read_json_arg(&Some("{".to_string()), &None).expect_err("invalid JSON fails");
        assert!(format!("{err:#}").contains("Argument must be valid JSON"));
    }

    #[test]
    fn resolves_knowledge_content_from_file() {
        let temp_root = temp_root("talon-cli-knowledge");
        fs::create_dir_all(temp_root.join("knowledge")).unwrap();
        fs::write(temp_root.join("knowledge/playbook.md"), "# Shared\n").unwrap();
        let manifest_path = temp_root.join("manifest.yaml");
        let manifest = "apiVersion: talon.impalasys.com/v1\nkind: Knowledge\nmetadata:\n  name: test\n  namespace: conic\nspec:\n  path: playbooks/test.md\n  contentFromFile: knowledge/playbook.md\n";

        let resolved = resolve_manifest_sources(manifest_path.to_str().unwrap(), manifest).unwrap();
        assert!(resolved.contains("content:"));
        assert!(resolved.contains("# Shared"));
        assert!(!resolved.contains("contentFromFile"));

        fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn parse_raw_manifest_extracts_kind() {
        let raw = parse_raw_manifest(
            "apiVersion: talon.impalasys.com/v1\nkind: Namespace\nmetadata:\n  name: conic\n",
        )
        .unwrap();
        assert_eq!(raw.kind, "Namespace");
    }

    #[test]
    fn render_rest_get_yaml_canonicalizes_template_and_ignores_null_labels() {
        let template_yaml = render_rest_get_yaml(
            "template",
            json!({
                "apiVersion": "talon.impalasys.com/v1",
                "kind": "AgentTemplate",
                "metadata": {
                    "name": "writer",
                    "labels": null,
                },
                "definition": {
                    "customSpec": {
                        "systemPrompt": "Write"
                    }
                }
            }),
        )
        .unwrap();
        let template =
            talon::manifest::parse_agent_template(&template_yaml).expect("template should parse");
        assert_eq!(
            template.metadata.as_ref().map(|meta| meta.name.as_str()),
            Some("writer")
        );

        let agent_yaml = render_rest_get_yaml(
            "agent",
            json!({
                "name": "writer",
                "ns": "conic",
                "labels": null,
                "definition": {
                    "customSpec": {
                        "systemPrompt": "Write"
                    }
                }
            }),
        )
        .unwrap();
        assert!(!agent_yaml.contains("labels: null"));
        let agent = talon::manifest::parse_agent(&agent_yaml).expect("agent should parse");
        assert_eq!(agent.name, "writer");
        assert!(agent.labels.is_empty());

        let namespace_yaml = render_rest_get_yaml(
            "namespace",
            json!({
                "name": "conic",
                "labels": null,
            }),
        )
        .unwrap();
        assert!(!namespace_yaml.contains("labels: null"));
        let namespace =
            talon::manifest::parse_namespace(&namespace_yaml).expect("namespace should parse");
        assert_eq!(namespace.name, "conic");
        assert!(namespace.labels.is_empty());

        let channel_yaml = render_rest_get_yaml(
            "channel",
            json!({
                "name": "match",
                "ns": "codewords:main",
                "title": "Match",
                "status": "open",
                "createdAt": "1780272630893454",
                "updatedAt": "1780272631893454",
                "metadata": {},
                "labels": {},
            }),
        )
        .unwrap();
        let channel = talon::manifest::parse_channel(&channel_yaml).expect("channel should parse");
        assert_eq!(channel.name, "match");
        assert_eq!(channel.ns, "codewords:main");
    }

    #[test]
    fn resolve_authorization_header_prefers_token_then_jwt_then_password() {
        let _guard = env_mutex().lock().unwrap();
        unsafe {
            std::env::remove_var("TALON_GATEWAY_TOKEN");
            std::env::remove_var("GATEWAY_TOKEN");
            std::env::remove_var("TALON_JWT_SECRET");
            std::env::remove_var("GATEWAY_JWT_SECRET");
            std::env::remove_var("TALON_GATEWAY_PASSWORD");
            std::env::remove_var("GATEWAY_PASSWORD");
        }

        let token_cli = Cli {
            token: Some("token-1".to_string()),
            ..cli()
        };
        assert_eq!(
            resolve_authorization_header(&token_cli).unwrap().as_deref(),
            Some("Bearer token-1")
        );

        let jwt_cli = Cli {
            jwt_secret: Some("jwt-secret".to_string()),
            ..cli()
        };
        let jwt_header = resolve_authorization_header(&jwt_cli).unwrap().unwrap();
        assert!(jwt_header.starts_with("Bearer "));

        let password_cli = Cli {
            password: Some("pw".to_string()),
            ..cli()
        };
        assert!(resolve_authorization_header(&password_cli)
            .unwrap()
            .unwrap()
            .starts_with("Basic "));
    }

    #[test]
    fn mint_channel_jwt_scopes_token_to_namespace_and_channel() {
        let token = mint_channel_jwt("secret", "ops", "incident-room", "web-client", 60).unwrap();
        let claims = talon::gateway::auth::verify_jwt(&token, "secret").unwrap();
        assert_eq!(claims.sub, "web-client");
        assert_eq!(claims.ns.as_deref(), Some("ops"));
        assert_eq!(claims.agent.as_deref(), None);
        assert_eq!(claims.session.as_deref(), None);
        assert_eq!(claims.channel.as_deref(), Some("incident-room"));
        assert!(claims.exp > 0);

        assert!(mint_channel_jwt("secret", "", "incident-room", "web-client", 60).is_err());
        assert!(mint_channel_jwt("secret", "ops", "", "web-client", 60).is_err());
        assert!(mint_channel_jwt("secret", "ops", "incident-room", "", 60).is_err());
        assert!(mint_channel_jwt("secret", "ops", "incident-room", "web-client", 0).is_err());
    }

    #[test]
    fn mint_root_jwt_has_no_resource_scope() {
        let token = mint_root_jwt("secret", "operator", 60).unwrap();
        let claims = talon::gateway::auth::verify_jwt(&token, "secret").unwrap();
        assert_eq!(claims.sub, "operator");
        assert_eq!(claims.ns.as_deref(), None);
        assert_eq!(claims.agent.as_deref(), None);
        assert_eq!(claims.session.as_deref(), None);
        assert_eq!(claims.channel.as_deref(), None);

        assert!(mint_root_jwt("secret", "", 60).is_err());
        assert!(mint_root_jwt("secret", "operator", 0).is_err());
    }

    #[test]
    fn mint_agent_jwt_scopes_token_to_namespace_and_agent() {
        let token = mint_agent_jwt("secret", "ops", "triage", "agent-client", 60).unwrap();
        let claims = talon::gateway::auth::verify_jwt(&token, "secret").unwrap();
        assert_eq!(claims.sub, "agent-client");
        assert_eq!(claims.ns.as_deref(), Some("ops"));
        assert_eq!(claims.agent.as_deref(), Some("triage"));
        assert_eq!(claims.session.as_deref(), None);
        assert_eq!(claims.channel.as_deref(), None);

        assert!(mint_agent_jwt("secret", "", "triage", "agent-client", 60).is_err());
        assert!(mint_agent_jwt("secret", "ops", "", "agent-client", 60).is_err());
        assert!(mint_agent_jwt("secret", "ops", "triage", "", 60).is_err());
        assert!(mint_agent_jwt("secret", "ops", "triage", "agent-client", 0).is_err());
    }

    #[test]
    fn mint_session_jwt_scopes_token_to_namespace_agent_and_session() {
        let token =
            mint_session_jwt("secret", "ops", "triage", "sess-1", "session-client", 60).unwrap();
        let claims = talon::gateway::auth::verify_jwt(&token, "secret").unwrap();
        assert_eq!(claims.sub, "session-client");
        assert_eq!(claims.ns.as_deref(), Some("ops"));
        assert_eq!(claims.agent.as_deref(), Some("triage"));
        assert_eq!(claims.session.as_deref(), Some("sess-1"));
        assert_eq!(claims.channel.as_deref(), None);

        assert!(mint_session_jwt("secret", "", "triage", "sess-1", "session-client", 60).is_err());
        assert!(mint_session_jwt("secret", "ops", "", "sess-1", "session-client", 60).is_err());
        assert!(mint_session_jwt("secret", "ops", "triage", "", "session-client", 60).is_err());
        assert!(mint_session_jwt("secret", "ops", "triage", "sess-1", "", 60).is_err());
        assert!(
            mint_session_jwt("secret", "ops", "triage", "sess-1", "session-client", 0).is_err()
        );
    }

    #[test]
    fn auth_interceptor_inserts_authorization_metadata() {
        let cli = Cli {
            token: Some("token-1".to_string()),
            ..cli()
        };
        let mut interceptor = auth_interceptor(&cli).unwrap();
        let request = tonic::Request::new(());
        let request = interceptor.call(request).unwrap();
        assert_eq!(
            request
                .metadata()
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap(),
            "Bearer token-1"
        );
    }

    #[test]
    fn auth_interceptor_supports_jwt_and_basic_password() {
        let jwt_cli = Cli {
            jwt_secret: Some("jwt-secret".to_string()),
            ..cli()
        };
        let mut jwt = auth_interceptor(&jwt_cli).unwrap();
        let jwt_req = jwt.call(tonic::Request::new(())).unwrap();
        assert!(jwt_req
            .metadata()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("Bearer "));

        let password_cli = Cli {
            password: Some("pw".to_string()),
            ..cli()
        };
        let mut password = auth_interceptor(&password_cli).unwrap();
        let password_req = password.call(tonic::Request::new(())).unwrap();
        assert!(password_req
            .metadata()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("Basic "));
    }

    #[tokio::test]
    async fn run_cli_dispatches_auth_token_commands() {
        let root = Cli {
            jwt_secret: Some("secret".to_string()),
            command: Commands::Auth {
                command: AuthCommands::RootToken {
                    subject: "operator".to_string(),
                    ttl_seconds: 60,
                },
            },
            ..cli()
        };
        assert!(run_cli(&root).await.unwrap().exit_code.is_none());

        let agent = Cli {
            jwt_secret: Some("secret".to_string()),
            command: Commands::Auth {
                command: AuthCommands::AgentToken {
                    namespace: "ops".to_string(),
                    agent: "triage".to_string(),
                    subject: "agent-client".to_string(),
                    ttl_seconds: 60,
                },
            },
            ..cli()
        };
        assert!(run_cli(&agent).await.unwrap().exit_code.is_none());

        let session = Cli {
            jwt_secret: Some("secret".to_string()),
            command: Commands::Auth {
                command: AuthCommands::SessionToken {
                    namespace: "ops".to_string(),
                    agent: "triage".to_string(),
                    session: "sess-1".to_string(),
                    subject: "session-client".to_string(),
                    ttl_seconds: 60,
                },
            },
            ..cli()
        };
        assert!(run_cli(&session).await.unwrap().exit_code.is_none());

        let channel = Cli {
            jwt_secret: Some("secret".to_string()),
            command: Commands::Auth {
                command: AuthCommands::ChannelToken {
                    namespace: "ops".to_string(),
                    channel: "incident-room".to_string(),
                    subject: "channel-client".to_string(),
                    ttl_seconds: 60,
                },
            },
            ..cli()
        };
        assert!(run_cli(&channel).await.unwrap().exit_code.is_none());
    }

    #[test]
    fn manifest_json_payload_supports_namespace_agent_and_knowledge() {
        let namespace = manifest_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: Namespace\nmetadata:\n  name: conic\n",
        )
        .unwrap();
        assert_eq!(namespace.0, "namespace");
        assert_eq!(namespace.1["name"], "conic");

        let agent = manifest_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: Agent\nmetadata:\n  name: ctl\n  namespace: conic\ndefinition:\n  customSpec:\n    systemPrompt: test\n",
        )
        .unwrap();
        assert_eq!(agent.0, "agent");
        assert_eq!(agent.1["name"], "ctl");
        assert_eq!(agent.1["ns"], "conic");

        let knowledge = manifest_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: Knowledge\nmetadata:\n  name: doc\n  namespace: conic\nspec:\n  path: docs/a.md\n  content: hello\n",
        )
        .unwrap();
        assert_eq!(knowledge.0, "knowledge");
        assert_eq!(knowledge.1["knowledge"]["kind"], "Knowledge");

        let binding = manifest_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: McpServerBinding\nmetadata:\n  name: github\n  namespace: conic\nspec:\n  serverRef: github\n",
        )
        .unwrap();
        assert_eq!(binding.0, "binding");
        assert_eq!(binding.1["ns"], "conic");
        assert_eq!(binding.1["binding"]["kind"], "McpServerBinding");
    }

    #[test]
    fn knowledge_helpers_build_expected_shape() {
        assert_eq!(knowledge_resource_name("docs/a.md"), "docs/a.md");
        let knowledge = build_knowledge("conic", "docs/a.md", "hello".to_string());
        assert_eq!(knowledge.metadata.as_ref().unwrap().namespace, "conic");
        assert_eq!(knowledge.spec.as_ref().unwrap().path, "docs/a.md");
        assert_eq!(knowledge.spec.as_ref().unwrap().content, "hello");
    }

    #[test]
    fn read_knowledge_content_validates_sources() {
        let temp_root = temp_root("talon-cli-content");
        fs::create_dir_all(&temp_root).unwrap();
        let file = temp_root.join("note.md");
        fs::write(&file, "hello").unwrap();

        assert_eq!(
            read_knowledge_content(&Some(file.display().to_string()), &None).unwrap(),
            "hello"
        );
        assert_eq!(
            read_knowledge_content(&None, &Some("inline".to_string())).unwrap(),
            "inline"
        );
        assert!(read_knowledge_content(&Some("a".to_string()), &Some("b".to_string())).is_err());
        assert!(read_knowledge_content(&None, &None).is_err());

        fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn relative_knowledge_path_and_markdown_collection_work() {
        let temp_root = temp_root("talon-cli-paths");
        fs::create_dir_all(temp_root.join("docs/nested")).unwrap();
        fs::write(temp_root.join("docs/one.md"), "one").unwrap();
        fs::write(temp_root.join("docs/nested/two.MD"), "two").unwrap();
        fs::write(temp_root.join("docs/skip.txt"), "skip").unwrap();

        let relative = relative_knowledge_path(
            &temp_root.join("docs"),
            &temp_root.join("docs/nested/two.MD"),
        )
        .unwrap();
        assert_eq!(relative, "nested/two.MD");

        let files = collect_markdown_files(&temp_root.join("docs")).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|path| path.extension().is_some()));

        fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn relative_knowledge_path_rejects_invalid_inputs() {
        let temp_root = temp_root("talon-cli-invalid-paths");
        fs::create_dir_all(temp_root.join("docs")).unwrap();
        fs::write(temp_root.join("outside.md"), "outside").unwrap();

        let outside =
            relative_knowledge_path(&temp_root.join("docs"), &temp_root.join("outside.md"))
                .expect_err("outside file should fail");
        assert!(outside.to_string().contains("is not inside"));

        let empty = relative_knowledge_path(&temp_root.join("docs"), &temp_root.join("docs"))
            .expect_err("root path should fail");
        assert!(empty.to_string().contains("cannot be empty"));

        fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn canonicalize_manifest_path_and_to_camel_case_cover_edge_cases() {
        let base = Path::new("/tmp/work");
        assert_eq!(
            canonicalize_manifest_path(base, "nested/file.md"),
            base.join("nested/file.md")
        );
        assert_eq!(
            canonicalize_manifest_path(base, "/abs/file.md"),
            std::path::PathBuf::from("/abs/file.md")
        );
        assert_eq!(to_camel_case("mcp_server_binding"), "McpServerBinding");
        assert_eq!(to_camel_case("agent-template"), "AgentTemplate");
    }

    #[test]
    fn agent_lookup_target_defaults_and_overrides_namespace() {
        assert_eq!(
            agent_lookup_target("writer", None),
            ("default".to_string(), "writer".to_string())
        );
        assert_eq!(
            agent_lookup_target("ops/writer", None),
            ("ops".to_string(), "writer".to_string())
        );
        let override_ns = "prod".to_string();
        assert_eq!(
            agent_lookup_target("ops/writer", Some(&override_ns)),
            ("prod".to_string(), "writer".to_string())
        );
    }

    #[test]
    fn rest_route_helpers_cover_supported_resources_and_validation() {
        let team = "team-a".to_string();

        assert_eq!(
            rest_get_path("template", "starter", None).unwrap(),
            ("/v1/templates/starter".to_string(), "template")
        );
        assert_eq!(
            rest_get_path("agent", "ops/writer", None).unwrap(),
            ("/v1/ns/ops/agents/writer".to_string(), "agent")
        );
        assert_eq!(
            rest_get_path("agent", "writer", Some(&team)).unwrap(),
            ("/v1/ns/team-a/agents/writer".to_string(), "agent")
        );
        assert_eq!(
            rest_get_path("schedule", "nightly sync", Some(&team)).unwrap(),
            (
                "/v1/ns/team-a/schedules/nightly%20sync".to_string(),
                "schedule"
            )
        );
        assert_eq!(
            rest_delete_path("namespace", "team/a", None).unwrap(),
            "/v1/namespaces/team%2Fa".to_string()
        );
        assert_eq!(
            rest_delete_path("knowledge", "docs/guide.md", Some(&team)).unwrap(),
            "/v1/namespaces/team-a/knowledge/docs%2Fguide.md".to_string()
        );

        let err = rest_get_path("mcpbinding", "writer-tools", None).unwrap_err();
        assert!(format!("{err:#}").contains("namespace is required"));

        let err = rest_delete_path("agent", "writer", None).unwrap_err();
        assert!(format!("{err:#}").contains("namespace is required"));

        let err = rest_get_path("unknown-kind", "writer", None).unwrap_err();
        assert!(format!("{err:#}").contains("Unsupported resource kind"));
    }

    #[test]
    fn grpc_target_helpers_cover_supported_resources_and_validation() {
        let team = "team-a".to_string();

        assert_eq!(
            grpc_get_target("template", "starter", None).unwrap(),
            GrpcGetTarget::AgentTemplate {
                name: "starter".to_string()
            }
        );
        assert_eq!(
            grpc_get_target("agent", "ops/writer", None).unwrap(),
            GrpcGetTarget::Agent {
                ns: "ops".to_string(),
                name: "writer".to_string()
            }
        );
        assert_eq!(
            grpc_get_target("agent", "writer", Some(&team)).unwrap(),
            GrpcGetTarget::Agent {
                ns: "team-a".to_string(),
                name: "writer".to_string()
            }
        );
        assert_eq!(
            grpc_get_target("schedule", "nightly", Some(&team)).unwrap(),
            GrpcGetTarget::Schedule {
                ns: "team-a".to_string(),
                name: "nightly".to_string()
            }
        );
        assert_eq!(
            grpc_delete_target("mcp", "docs", None).unwrap(),
            GrpcDeleteTarget::McpServer {
                name: "docs".to_string()
            }
        );
        assert_eq!(
            grpc_delete_target("knowledge", "docs/a.md", Some(&team)).unwrap(),
            GrpcDeleteTarget::Knowledge {
                ns: "team-a".to_string(),
                name: "docs/a.md".to_string()
            }
        );

        let err = grpc_get_target("knowledge", "docs/a.md", None).unwrap_err();
        assert!(format!("{err:#}").contains("Knowledge get requires --namespace"));

        let err = grpc_delete_target("knowledge", "docs/a.md", None).unwrap_err();
        assert!(format!("{err:#}").contains("Knowledge delete requires --namespace"));

        let err = grpc_get_target("unknown-kind", "writer", None).unwrap_err();
        assert!(format!("{err:#}").contains("Unsupported resource kind"));
    }

    #[test]
    fn render_manifest_file_applies_vars_and_resolves_sources() {
        let temp_root = temp_root("talon-cli-render");
        fs::create_dir_all(temp_root.join("knowledge")).unwrap();
        fs::write(temp_root.join("knowledge/doc.md"), "rendered").unwrap();
        let manifest_path = temp_root.join("manifest.yaml");
        fs::write(
            &manifest_path,
            "apiVersion: talon.impalasys.com/v1\nkind: Knowledge\nmetadata:\n  name: test\n  namespace: {{ vars.ns }}\nspec:\n  path: docs/test.md\n  contentFromFile: knowledge/doc.md\n",
        )
        .unwrap();

        let rendered =
            render_manifest_file(manifest_path.to_str().unwrap(), &["ns=conic".to_string()])
                .unwrap();
        assert!(rendered.contains("namespace: conic"));
        assert!(rendered.contains("content: rendered"));

        fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn render_json_payload_supports_multiple_manifest_kinds() {
        let namespace_json = render_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: Namespace\nmetadata:\n  name: team-a\n  labels:\n    owner: docs\n",
        )
        .unwrap();
        assert_eq!(namespace_json["name"], "team-a");
        assert_eq!(namespace_json["recursive"], true);

        let agent_json = render_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: Agent\nmetadata:\n  name: writer\n  namespace: team-a\n  labels:\n    role: editor\ndefinition:\n  customSpec:\n    systemPrompt: Write crisply\n",
        )
        .unwrap();
        assert_eq!(agent_json["agent"]["metadata"]["namespace"], "team-a");
        assert_eq!(agent_json["agent"]["metadata"]["name"], "writer");

        let binding_json = render_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: McpServerBinding\nmetadata:\n  namespace: team-a\n  name: docs\nspec:\n  serverRef: docs-server\n",
        )
        .unwrap();
        assert_eq!(binding_json["ns"], "team-a");
        assert_eq!(binding_json["binding"]["metadata"]["name"], "docs");
    }

    #[test]
    fn build_rest_apply_plan_covers_create_update_and_validation() {
        let (_, agent_payload) = manifest_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: Agent\nmetadata:\n  name: writer\n  namespace: team-a\n  labels:\n    tier: prod\ndefinition:\n  customSpec:\n    systemPrompt: Write crisply\n",
        )
        .unwrap();
        let create_plan = build_rest_apply_plan(
            "apiVersion: talon.impalasys.com/v1\nkind: Agent\nmetadata:\n  name: writer\n  namespace: team-a\n  labels:\n    tier: prod\ndefinition:\n  customSpec:\n    systemPrompt: Write crisply\n",
            agent_payload.clone(),
            false,
        )
        .unwrap();
        assert_eq!(create_plan.method, reqwest::Method::POST);
        assert_eq!(create_plan.path, "/v1/ns/team-a/agents");
        assert_eq!(create_plan.payload["name"], "writer");
        assert_eq!(create_plan.success_label, "Agent 'team-a/writer'");

        let update_plan = build_rest_apply_plan(
            "apiVersion: talon.impalasys.com/v1\nkind: Agent\nmetadata:\n  name: writer\n  namespace: team-a\n  labels:\n    tier: prod\ndefinition:\n  customSpec:\n    systemPrompt: Write crisply\n",
            agent_payload,
            true,
        )
        .unwrap();
        assert_eq!(update_plan.method, reqwest::Method::PUT);
        assert_eq!(update_plan.path, "/v1/ns/team-a/agents/writer");
        assert_eq!(update_plan.payload["agent"], "writer");

        let (_, binding_payload) = manifest_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: McpServerBinding\nmetadata:\n  name: docs\n  namespace: team-a\nspec:\n  serverRef: docs-server\n",
        )
        .unwrap();
        let binding_plan = build_rest_apply_plan(
            "apiVersion: talon.impalasys.com/v1\nkind: McpServerBinding\nmetadata:\n  name: docs\n  namespace: team-a\nspec:\n  serverRef: docs-server\n",
            binding_payload,
            false,
        )
        .unwrap();
        assert_eq!(binding_plan.path, "/v1/namespaces/team-a/mcp-bindings");
        assert_eq!(binding_plan.payload["ns"], "team-a");

        let (_, namespace_payload) = manifest_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: Namespace\nmetadata:\n  name: team-a\n",
        )
        .unwrap();
        let namespace_plan = build_rest_apply_plan(
            "apiVersion: talon.impalasys.com/v1\nkind: Namespace\nmetadata:\n  name: team-a\n",
            namespace_payload,
            false,
        )
        .unwrap();
        assert_eq!(namespace_plan.path, "/v1/namespaces/team-a");
        assert_eq!(namespace_plan.payload["recursive"], true);

        let (_, mcp_payload) = manifest_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: McpServer\nmetadata:\n  name: docs\nspec:\n  transport: streamable-http\n  target: https://example.com/mcp\n",
        )
        .unwrap();
        let mcp_plan = build_rest_apply_plan(
            "apiVersion: talon.impalasys.com/v1\nkind: McpServer\nmetadata:\n  name: docs\nspec:\n  transport: streamable-http\n  target: https://example.com/mcp\n",
            mcp_payload,
            false,
        )
        .unwrap();
        assert_eq!(mcp_plan.path, "/v1/mcp-servers");

        let err = build_rest_apply_plan(
            "apiVersion: talon.impalasys.com/v1\nkind: McpServer\nmetadata:\n  name: docs\n  namespace: team-a\nspec:\n  transport: streamable-http\n  target: https://example.com/mcp\n",
            json!({}),
            false,
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("metadata.namespace is not supported"));

        let err = build_rest_apply_plan(
            "apiVersion: talon.impalasys.com/v1\nkind: Knowledge\nmetadata:\n  name: doc\nspec:\n  path: docs/a.md\n  content: hi\n",
            json!({}),
            false,
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("metadata.namespace is required"));
    }

    #[test]
    fn build_grpc_apply_plan_covers_supported_manifests_and_validation() {
        match build_grpc_apply_plan(
            "apiVersion: talon.impalasys.com/v1\nkind: Agent\nmetadata:\n  name: writer\n  namespace: team-a\n  labels:\n    tier: prod\ndefinition:\n  customSpec:\n    systemPrompt: Write crisply\n",
        )
        .unwrap()
        {
            GrpcApplyPlan::Agent {
                ns,
                name,
                labels,
                definition,
            } => {
                assert_eq!(ns, "team-a");
                assert_eq!(name, "writer");
                assert_eq!(labels.get("tier").map(String::as_str), Some("prod"));
                assert!(definition.source.is_some());
            }
            other => panic!("unexpected plan: {other:?}"),
        }

        match build_grpc_apply_plan(
            "apiVersion: talon.impalasys.com/v1\nkind: AgentTemplate\nmetadata:\n  name: release-writer\ndefinition:\n  customSpec:\n    instructions: Write\n",
        )
        .unwrap()
        {
            GrpcApplyPlan::AgentTemplate { name, template } => {
                assert_eq!(name, "release-writer");
                assert_eq!(
                    template.metadata.as_ref().map(|m| m.name.as_str()),
                    Some("release-writer")
                );
            }
            other => panic!("unexpected plan: {other:?}"),
        }

        match build_grpc_apply_plan(
            "apiVersion: talon.impalasys.com/v1\nkind: McpServer\nmetadata:\n  name: docs\nspec:\n  transport: streamable-http\n  target: https://example.com/mcp\n",
        )
        .unwrap()
        {
            GrpcApplyPlan::McpServer { name, server } => {
                assert_eq!(name, "docs");
                assert_eq!(
                    server.metadata.as_ref().map(|m| m.name.as_str()),
                    Some("docs")
                );
            }
            other => panic!("unexpected plan: {other:?}"),
        }

        match build_grpc_apply_plan(
            "apiVersion: talon.impalasys.com/v1\nkind: Knowledge\nmetadata:\n  name: doc\n  namespace: team-a\nspec:\n  path: docs/a.md\n  content: hi\n",
        )
        .unwrap()
        {
            GrpcApplyPlan::Knowledge {
                ns,
                name,
                knowledge,
            } => {
                assert_eq!(ns, "team-a");
                assert_eq!(name, "doc");
                assert_eq!(
                    knowledge.metadata.as_ref().map(|m| m.namespace.as_str()),
                    Some("team-a")
                );
            }
            other => panic!("unexpected plan: {other:?}"),
        }

        let err = build_grpc_apply_plan(
            "apiVersion: talon.impalasys.com/v1\nkind: McpServer\nmetadata:\n  name: docs\n  namespace: team-a\nspec:\n  transport: streamable-http\n  target: https://example.com/mcp\n",
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("metadata.namespace is not supported"));

        let err = build_grpc_apply_plan(
            "apiVersion: talon.impalasys.com/v1\nkind: Knowledge\nmetadata:\n  name: doc\nspec:\n  path: docs/a.md\n  content: hi\n",
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("metadata.namespace is required"));

        let err = build_grpc_apply_plan(
            "apiVersion: talon.impalasys.com/v1\nkind: UnknownThing\nmetadata:\n  name: nope\n",
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("Unsupported manifest kind"));
    }

    #[test]
    fn sdk_generation_helpers_build_methods_and_filter_inputs() {
        assert_eq!(feature_ts_type("integer"), "number");
        assert_eq!(feature_ts_type("boolean"), "boolean");
        assert_eq!(feature_ts_type("string"), "string");

        let template = talon::manifest::parse_agent_template(
            "apiVersion: talon.impalasys.com/v1\nkind: AgentTemplate\nmetadata:\n  name: release-writer\ndefinition:\n  customSpec:\n    instructions: Write\n    features:\n      - name: title\n        type: string\n        required: true\n      - name: dryRun\n        type: boolean\n        required: false\n",
        )
        .unwrap();
        let method = sdk_method_for_template(&template).unwrap();
        assert!(method.contains("async createReleaseWriter"));
        assert!(method.contains("title: string"));
        assert!(method.contains("dryRun?: boolean"));
        assert!(method.contains("template: \"release-writer\""));
    }

    #[test]
    fn sdk_methods_from_dir_reads_yaml_templates_and_skips_others() {
        let root = temp_root("talon-cli-sdk");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("writer.yaml"),
            "apiVersion: talon.impalasys.com/v1\nkind: AgentTemplate\nmetadata:\n  name: writer\ndefinition:\n  customSpec:\n    instructions: Write\n",
        )
        .unwrap();
        fs::write(
            root.join("research.yaml"),
            "apiVersion: talon.impalasys.com/v1\nkind: AgentTemplate\nmetadata:\n  name: research-helper\ndefinition:\n  customSpec:\n    instructions: Research\n    features:\n      - name: query\n        type: string\n        required: true\n",
        )
        .unwrap();
        fs::write(root.join("notes.txt"), "ignored").unwrap();
        fs::write(root.join("broken.yaml"), "kind: NotATemplate").unwrap();

        let methods = sdk_methods_from_dir(root.to_str().unwrap()).unwrap();
        assert_eq!(methods.len(), 2);
        assert!(methods.iter().any(|m| m.contains("createWriter")));
        assert!(methods.iter().any(|m| m.contains("createResearchHelper")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_manifest_sources_rejects_invalid_knowledge_source_combinations() {
        let err = resolve_manifest_sources(
            "manifest.yaml",
            "apiVersion: talon.impalasys.com/v1\nkind: Knowledge\nmetadata:\n  name: test\n  namespace: conic\nspec:\n  path: docs/test.md\n",
        )
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("must set one of content or contentFromFile"));
    }

    #[test]
    fn resolve_manifest_sources_passes_through_non_knowledge_and_rejects_duplicate_sources() {
        let namespace =
            "apiVersion: talon.impalasys.com/v1\nkind: Namespace\nmetadata:\n  name: conic\n";
        assert_eq!(
            resolve_manifest_sources("manifest.yaml", namespace).unwrap(),
            namespace
        );

        let err = resolve_manifest_sources(
            "manifest.yaml",
            "apiVersion: talon.impalasys.com/v1\nkind: Knowledge\nmetadata:\n  name: test\n  namespace: conic\nspec:\n  path: docs/test.md\n  content: inline\n  contentFromFile: docs/test.md\n",
        )
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("only one of content or contentFromFile"));
    }

    #[test]
    fn manifest_json_payload_rejects_unsupported_or_incomplete_manifests() {
        let unsupported = manifest_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: UnknownThing\nmetadata:\n  name: test\n",
        )
        .unwrap_err();
        assert!(unsupported
            .to_string()
            .contains("Unsupported manifest kind"));

        let missing_namespace = manifest_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: McpServerBinding\nmetadata:\n  name: github\nspec:\n  serverRef: github\n",
        )
        .unwrap_err();
        assert!(missing_namespace.to_string().contains("namespace"));

        let render_err = render_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: UnknownThing\nmetadata:\n  name: test\n",
        )
        .unwrap_err();
        assert!(render_err.to_string().contains("Unsupported manifest kind"));
    }

    #[tokio::test]
    async fn rest_client_sends_basic_authorization_header_from_password() {
        let app = Router::new().route(
            "/auth-check",
            get(|headers: axum::http::HeaderMap| async move {
                let auth = headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_string();
                Json(json!({ "authorization": auth }))
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cli = Cli {
            gateway: format!("http://{addr}"),
            password: Some("pw".to_string()),
            rest: true,
            ..cli()
        };
        let client = rest_client(&cli).unwrap();
        let value: serde_json::Value = client
            .get(format!("http://{addr}/auth-check"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(value["authorization"], "Basic OnB3");

        server.abort();
    }

    #[test]
    fn schedule_json_renders_target_and_status_fields() {
        let schedule = super::models::Schedule {
            name: "nightly".to_string(),
            ns: "conic".to_string(),
            labels: HashMap::from([("tier".to_string(), "prod".to_string())]),
            spec: Some(super::models::ScheduleSpec {
                kind: "cron".to_string(),
                cron: "0 0 * * *".to_string(),
                interval_seconds: 0,
                run_at: String::new(),
                timezone: "UTC".to_string(),
                target: Some(super::models::ScheduleTarget {
                    agent: "ctl".to_string(),
                    workflow: String::new(),
                    session_mode: "new".to_string(),
                    session_id: String::new(),
                }),
                input_message: "ping".to_string(),
                input_json: String::new(),
                enabled: true,
            }),
            status: Some(super::models::ScheduleStatus {
                revision: 3,
                next_run_at: Some(123),
                backend_handle: Some("handle".to_string()),
                backend_armed: true,
                last_run_at: Some(111),
                last_session_id: Some("session-1".to_string()),
                last_error: None,
                claimed_run_at: None,
                claim_expires_at: None,
                recent_events: vec![super::models::ScheduleEvent {
                    timestamp: 99,
                    phase: "armed".to_string(),
                    outcome: "ok".to_string(),
                    detail: "scheduled".to_string(),
                }],
            }),
        };

        let json = schedule_json(&schedule);
        assert_eq!(json["spec"]["target"]["agent"], "ctl");
        assert_eq!(json["status"]["backendHandle"], "handle");
        assert_eq!(json["status"]["recentEvents"][0]["phase"], "armed");
    }

    #[test]
    fn stream_event_line_parser_accepts_sse_and_ndjson_lines() {
        print_stream_event_line("").unwrap();
        print_stream_event_line(": keepalive").unwrap();
        print_stream_event_line("event: workflow").unwrap();
        print_stream_event_line(r#"data: {"type":"run_completed"}"#).unwrap();
        print_stream_event_line(r#"{"type":"step_completed"}"#).unwrap();
        print_stream_event_line("data: [DONE]").unwrap();
    }

    #[test]
    fn rest_stream_buffer_limit_rejects_unbounded_lines() {
        ensure_rest_stream_buffer_within_limit(MAX_REST_STREAM_BUFFER_BYTES).unwrap();
        let err = ensure_rest_stream_buffer_within_limit(MAX_REST_STREAM_BUFFER_BYTES + 1)
            .expect_err("oversized stream buffer should fail");
        assert!(err.to_string().contains("maximum buffer limit"));
    }

    #[tokio::test]
    async fn knowledge_rest_helpers_round_trip_and_handle_missing() {
        #[derive(Clone, Default)]
        struct AppState {
            store: std::sync::Arc<tokio::sync::Mutex<HashMap<String, super::Knowledge>>>,
        }

        let state = AppState::default();
        let app = Router::new()
            .route(
                "/v1/namespaces/:ns/knowledge",
                get(
                    |State(state): State<AppState>, AxumPath(ns): AxumPath<String>| async move {
                        let values = state
                            .store
                            .lock()
                            .await
                            .values()
                            .filter(|item| {
                                item.metadata
                                    .as_ref()
                                    .map(|meta| meta.namespace == ns)
                                    .unwrap_or(false)
                            })
                            .cloned()
                            .collect::<Vec<_>>();
                        Json(json!({ "knowledge": values }))
                    },
                )
                .post(
                    |State(state): State<AppState>,
                     AxumPath(ns): AxumPath<String>,
                     Json(payload): Json<serde_json::Value>| async move {
                        let knowledge: super::Knowledge =
                            serde_json::from_value(payload["knowledge"].clone()).unwrap();
                        let name = knowledge.metadata.as_ref().unwrap().name.clone();
                        assert_eq!(knowledge.metadata.as_ref().unwrap().namespace, ns);
                        let encoded_name = urlencoding::encode(&name).into_owned();
                        let mut store = state.store.lock().await;
                        store.insert(name, knowledge.clone());
                        store.insert(encoded_name, knowledge);
                        Json(json!({ "ok": true }))
                    },
                ),
            )
            .route(
                "/v1/namespaces/:ns/knowledge/*name",
                get(
                    |State(state): State<AppState>,
                     AxumPath((_ns, name)): AxumPath<(String, String)>| async move {
                        let decoded_name = urlencoding::decode(name.trim_start_matches('/'))
                            .unwrap()
                            .into_owned();
                        match state.store.lock().await.get(&decoded_name).cloned() {
                            Some(knowledge) => {
                                (StatusCode::OK, Json(json!({ "knowledge": knowledge })))
                            }
                            None => (StatusCode::NOT_FOUND, Json(json!({ "error": "missing" }))),
                        }
                    },
                )
                .delete(
                    |State(state): State<AppState>,
                     AxumPath((_ns, name)): AxumPath<(String, String)>| async move {
                        let decoded_name = urlencoding::decode(name.trim_start_matches('/'))
                            .unwrap()
                            .into_owned();
                        state.store.lock().await.remove(&decoded_name);
                        Json(json!({ "deleted": true }))
                    },
                ),
            )
            .with_state(state.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cli = rest_cli(format!("http://{addr}"));
        let path = "docs-test.md";
        let missing = knowledge_get(&cli, "conic", path)
            .await
            .unwrap_err()
            .to_string();
        assert!(missing.contains("status=404 Not Found"));

        knowledge_set(&cli, "conic", path, "hello".to_string())
            .await
            .unwrap();

        let listed = knowledge_list(&cli, "conic").await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].metadata.as_ref().unwrap().name, path);
        assert_eq!(listed[0].spec.as_ref().unwrap().content, "hello");

        knowledge_delete(&cli, "conic", path).await.unwrap();

        server.abort();
    }

    #[tokio::test]
    async fn knowledge_rest_helpers_surface_server_errors() {
        let app = Router::new()
            .route(
                "/v1/namespaces/:ns/knowledge",
                post(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "nope") }),
            )
            .route(
                "/v1/namespaces/:ns/knowledge/:name",
                delete(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "delete failed") }),
            );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cli = rest_cli(format!("http://{addr}"));
        let set_err = knowledge_set(&cli, "conic", "docs/test.md", "hello".to_string())
            .await
            .unwrap_err()
            .to_string();
        assert!(set_err.contains("Failed to write Knowledge"));

        let delete_err = knowledge_delete(&cli, "conic", "docs/test.md")
            .await
            .unwrap_err()
            .to_string();
        assert!(delete_err.contains("Failed to delete Knowledge"));

        server.abort();
    }

    #[tokio::test]
    async fn knowledge_grpc_helpers_round_trip() {
        let (addr, kv) = serve_grpc_gateway().await;
        seed_namespace(&kv, "conic").await;

        let cli = Cli {
            gateway: format!("http://{addr}"),
            ..cli()
        };

        knowledge_set(&cli, "conic", "docs/grpc.md", "hello grpc".to_string())
            .await
            .unwrap();

        let got = knowledge_get(&cli, "conic", "docs/grpc.md")
            .await
            .unwrap()
            .expect("knowledge should exist");
        assert_eq!(got.spec.as_ref().unwrap().content, "hello grpc");

        let listed = knowledge_list(&cli, "conic").await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].spec.as_ref().unwrap().path, "docs/grpc.md");

        knowledge_delete(&cli, "conic", "docs/grpc.md")
            .await
            .unwrap();
        assert!(knowledge_get(&cli, "conic", "docs/grpc.md")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn grpc_get_and_delete_helpers_round_trip_knowledge() {
        let (addr, kv) = serve_grpc_gateway().await;
        seed_namespace(&kv, "conic").await;
        let namespace = "conic".to_string();

        let cli = Cli {
            gateway: format!("http://{addr}"),
            ..cli()
        };

        knowledge_set(&cli, "conic", "docs/view.md", "grpc body".to_string())
            .await
            .unwrap();

        let yaml = grpc_get_yaml(&cli, "knowledge", "docs/view.md", Some(&namespace))
            .await
            .unwrap();
        assert!(yaml.contains("kind: Knowledge"));
        assert!(yaml.contains("content: grpc body"));

        let deleted = grpc_delete_resource(&cli, "knowledge", "docs/view.md", Some(&namespace))
            .await
            .unwrap();
        assert!(deleted.contains("deleted successfully"));
        assert!(knowledge_get(&cli, "conic", "docs/view.md")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn grpc_apply_manifest_round_trips_knowledge_and_agent() {
        let (addr, kv) = serve_grpc_gateway().await;
        seed_namespace(&kv, "conic").await;

        let cli = Cli {
            gateway: format!("http://{addr}"),
            ..cli()
        };

        let knowledge_message = grpc_apply_manifest(
            &cli,
            "apiVersion: talon.impalasys.com/v1\nkind: Knowledge\nmetadata:\n  name: docs/applied.md\n  namespace: conic\nspec:\n  path: docs/applied.md\n  content: from apply\n",
        )
        .await
        .unwrap();
        assert!(
            knowledge_message.contains("Knowledge 'conic/docs/applied.md' applied successfully.")
        );
        let got = knowledge_get(&cli, "conic", "docs/applied.md")
            .await
            .unwrap()
            .expect("knowledge should exist");
        assert_eq!(got.spec.as_ref().unwrap().content, "from apply");

        let created = grpc_apply_manifest(
            &cli,
            "apiVersion: talon.impalasys.com/v1\nkind: Agent\nmetadata:\n  name: writer\n  namespace: conic\n  labels:\n    tier: prod\ndefinition:\n  customSpec:\n    systemPrompt: First version\n    modelPolicy:\n      profiles:\n        - name: default\n          model:\n            provider: mock\n            name: test-model\n            temperature: 0.0\n",
        )
        .await
        .unwrap();
        assert!(created.contains("Agent 'conic/writer' applied successfully."));

        let initial_yaml = grpc_get_yaml(&cli, "agent", "conic/writer", None)
            .await
            .unwrap();
        assert_eq!(parse_raw_manifest(&initial_yaml).unwrap().kind, "Agent");
        assert!(initial_yaml.contains("apiVersion: talon.impalasys.com/v1"));
        assert!(initial_yaml.contains("namespace: conic"));
        assert!(initial_yaml.contains("systemPrompt: First version"));
        let reapplied = grpc_apply_manifest(&cli, &initial_yaml).await.unwrap();
        assert!(reapplied.contains("Agent 'conic/writer' applied successfully."));

        let updated = grpc_apply_manifest(
            &cli,
            "apiVersion: talon.impalasys.com/v1\nkind: Agent\nmetadata:\n  name: writer\n  namespace: conic\n  labels:\n    tier: prod\ndefinition:\n  customSpec:\n    systemPrompt: Updated version\n    modelPolicy:\n      profiles:\n        - name: default\n          model:\n            provider: mock\n            name: test-model\n            temperature: 0.0\n",
        )
        .await
        .unwrap();
        assert!(updated.contains("Agent 'conic/writer' applied successfully."));

        let updated_yaml = grpc_get_yaml(&cli, "agent", "conic/writer", None)
            .await
            .unwrap();
        assert!(updated_yaml.contains("systemPrompt: Updated version"));
    }

    #[tokio::test]
    async fn grpc_apply_get_and_delete_round_trip_template_and_mcp_server() {
        let (addr, _kv) = serve_grpc_gateway().await;
        let cli = Cli {
            gateway: format!("http://{addr}"),
            ..cli()
        };

        let template_message = grpc_apply_manifest(
            &cli,
            "apiVersion: talon.impalasys.com/v1\nkind: AgentTemplate\nmetadata:\n  name: release-writer\ndefinition:\n  customSpec:\n    systemPrompt: Ship it\n    modelPolicy:\n      profiles:\n        - name: default\n          model:\n            provider: mock\n            name: template-model\n            temperature: 0.0\n",
        )
        .await
        .unwrap();
        assert!(template_message.contains("AgentTemplate 'release-writer' applied successfully."));

        let template_yaml = grpc_get_yaml(&cli, "template", "release-writer", None)
            .await
            .unwrap();
        assert!(template_yaml.contains("kind: AgentTemplate"));
        assert!(template_yaml.contains("name: release-writer"));

        let deleted_template = grpc_delete_resource(&cli, "template", "release-writer", None)
            .await
            .unwrap();
        assert!(deleted_template.contains("deleted successfully"));

        let mcp_message = grpc_apply_manifest(
            &cli,
            "apiVersion: talon.impalasys.com/v1\nkind: McpServer\nmetadata:\n  name: docs-server\nspec:\n  transport: streamable-http\n  target: https://example.com/mcp\n",
        )
        .await
        .unwrap();
        assert!(mcp_message.contains("MCPServer 'docs-server' applied successfully."));

        let mcp_yaml = grpc_get_yaml(&cli, "mcp", "docs-server", None)
            .await
            .unwrap();
        assert_eq!(parse_raw_manifest(&mcp_yaml).unwrap().kind, "McpServer");
        assert!(mcp_yaml.contains("name: docs-server"));
        assert!(mcp_yaml.contains("transport: streamable-http"));
        let reapplied_mcp = grpc_apply_manifest(&cli, &mcp_yaml).await.unwrap();
        assert!(reapplied_mcp.contains("MCPServer 'docs-server' applied successfully."));

        let deleted_mcp = grpc_delete_resource(&cli, "mcp", "docs-server", None)
            .await
            .unwrap();
        assert!(deleted_mcp.contains("deleted successfully"));
    }

    #[tokio::test]
    async fn grpc_get_yaml_renders_schedule_yaml() {
        let (addr, kv) = serve_grpc_gateway().await;
        seed_namespace(&kv, "conic").await;
        let namespace = "conic".to_string();
        kv.set_msg(
            &keys::schedule(&namespace, "nightly"),
            &models::Schedule {
                name: "nightly".to_string(),
                ns: namespace.clone(),
                labels: HashMap::new(),
                spec: Some(models::ScheduleSpec {
                    kind: "cron".to_string(),
                    cron: "0 0 * * *".to_string(),
                    interval_seconds: 0,
                    run_at: String::new(),
                    timezone: "UTC".to_string(),
                    target: Some(models::ScheduleTarget {
                        agent: "writer".to_string(),
                        workflow: String::new(),
                        session_mode: "new".to_string(),
                        session_id: String::new(),
                    }),
                    input_message: "publish".to_string(),
                    input_json: String::new(),
                    enabled: true,
                }),
                status: Some(models::ScheduleStatus {
                    revision: 2,
                    next_run_at: Some(123),
                    backend_handle: Some("handle".to_string()),
                    backend_armed: true,
                    last_run_at: None,
                    last_session_id: None,
                    last_error: None,
                    claimed_run_at: None,
                    claim_expires_at: None,
                    recent_events: Vec::new(),
                }),
            },
        )
        .await
        .unwrap();

        let cli = Cli {
            gateway: format!("http://{addr}"),
            ..cli()
        };
        let yaml = grpc_get_yaml(&cli, "schedule", "nightly", Some(&namespace))
            .await
            .unwrap();
        assert!(yaml.contains("backendHandle: handle"));
        assert!(yaml.contains("cron: 0 0 * * *"));
        assert!(yaml.contains("agent: writer"));
    }

    #[tokio::test]
    async fn rest_apply_get_and_delete_helpers_cover_multiple_resource_kinds() {
        #[derive(Clone, Default)]
        struct RestState {
            templates: Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>,
            agents: Arc<tokio::sync::Mutex<HashMap<(String, String), serde_json::Value>>>,
            workflows: Arc<tokio::sync::Mutex<HashMap<(String, String), serde_json::Value>>>,
        }

        let state = RestState::default();
        let app = Router::new()
            .route(
                "/v1/templates",
                post(
                    |State(state): State<RestState>, Json(payload): Json<serde_json::Value>| async move {
                        let template = payload["template"].clone();
                        let name = template["metadata"]["name"].as_str().unwrap().to_string();
                        state.templates.lock().await.insert(name, template);
                        Json(json!({ "ok": true }))
                    },
                ),
            )
            .route(
                "/v1/templates/:name",
                get(
                    |State(state): State<RestState>, AxumPath(name): AxumPath<String>| async move {
                        let value = state.templates.lock().await.get(&name).cloned();
                        match value {
                            Some(template) => (StatusCode::OK, Json(json!({ "template": template }))),
                            None => (StatusCode::NOT_FOUND, Json(json!({ "error": "missing" }))),
                        }
                    },
                )
                .delete(
                    |State(state): State<RestState>, AxumPath(name): AxumPath<String>| async move {
                        state.templates.lock().await.remove(&name);
                        Json(json!({ "deleted": true }))
                    },
                ),
            )
            .route(
                "/v1/ns/:ns/agents",
                post(
                    |State(state): State<RestState>,
                     AxumPath(ns): AxumPath<String>,
                     Json(payload): Json<serde_json::Value>| async move {
                        let name = payload["name"].as_str().unwrap().to_string();
                        state.agents.lock().await.insert((ns, name), payload);
                        Json(json!({ "ok": true }))
                    },
                ),
            )
            .route(
                "/v1/ns/:ns/agents/:name",
                get(
                    |State(state): State<RestState>,
                     AxumPath((ns, name)): AxumPath<(String, String)>| async move {
                        let value = state.agents.lock().await.get(&(ns, name)).cloned();
                        match value {
                            Some(agent) => (StatusCode::OK, Json(json!({ "agent": agent }))),
                            None => (StatusCode::NOT_FOUND, Json(json!({ "error": "missing" }))),
                        }
                    },
                )
                .put(
                    |State(state): State<RestState>,
                     AxumPath((ns, name)): AxumPath<(String, String)>,
                     Json(payload): Json<serde_json::Value>| async move {
                        state.agents.lock().await.insert((ns, name), payload);
                        Json(json!({ "ok": true }))
                    },
                )
                .delete(
                    |State(state): State<RestState>,
                     AxumPath((ns, name)): AxumPath<(String, String)>| async move {
                        state.agents.lock().await.remove(&(ns, name));
                        Json(json!({ "deleted": true }))
                    },
                ),
            )
            .route(
                "/v1/ns/:ns/workflows",
                post(
                    |State(state): State<RestState>,
                     AxumPath(ns): AxumPath<String>,
                     Json(payload): Json<serde_json::Value>| async move {
                        let workflow = payload["workflow"].clone();
                        let name = workflow["name"].as_str().unwrap().to_string();
                        state.workflows.lock().await.insert((ns, name), workflow);
                        Json(json!({ "ok": true }))
                    },
                ),
            )
            .route(
                "/v1/ns/:ns/workflows/:name",
                get(
                    |State(state): State<RestState>,
                     AxumPath((ns, name)): AxumPath<(String, String)>| async move {
                        let value = state.workflows.lock().await.get(&(ns, name)).cloned();
                        match value {
                            Some(workflow) => (StatusCode::OK, Json(json!({ "workflow": workflow }))),
                            None => (StatusCode::NOT_FOUND, Json(json!({ "error": "missing" }))),
                        }
                    },
                )
                .delete(
                    |State(state): State<RestState>,
                     AxumPath((ns, name)): AxumPath<(String, String)>| async move {
                        state.workflows.lock().await.remove(&(ns, name));
                        Json(json!({ "deleted": true }))
                    },
                ),
            )
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cli = rest_cli(format!("http://{addr}"));
        let namespace = "conic".to_string();

        let template_message = rest_apply_manifest(
            &cli,
            "apiVersion: talon.impalasys.com/v1\nkind: AgentTemplate\nmetadata:\n  name: rest-template\ndefinition:\n  customSpec:\n    systemPrompt: REST template\n",
            false,
        )
        .await
        .unwrap();
        assert!(template_message.contains("AgentTemplate 'rest-template' applied successfully."));
        let template_yaml = rest_get_yaml(&cli, "template", "rest-template", None)
            .await
            .unwrap();
        assert!(template_yaml.contains("name: rest-template"));
        let deleted_template = rest_delete_resource(&cli, "template", "rest-template", None)
            .await
            .unwrap();
        assert!(deleted_template.contains("deleted successfully"));

        let created_agent = rest_apply_manifest(
            &cli,
            "apiVersion: talon.impalasys.com/v1\nkind: Agent\nmetadata:\n  name: writer\n  namespace: conic\ndefinition:\n  customSpec:\n    systemPrompt: REST create\n",
            false,
        )
        .await
        .unwrap();
        assert!(created_agent.contains("Agent 'conic/writer' applied successfully."));
        let created_yaml = rest_get_yaml(&cli, "agent", "conic/writer", None)
            .await
            .unwrap();
        assert_eq!(parse_raw_manifest(&created_yaml).unwrap().kind, "Agent");
        assert!(created_yaml.contains("apiVersion: talon.impalasys.com/v1"));
        assert!(created_yaml.contains("namespace: conic"));
        assert!(created_yaml.contains("systemPrompt: REST create"));
        let reapplied_agent = rest_apply_manifest(&cli, &created_yaml, true)
            .await
            .unwrap();
        assert!(reapplied_agent.contains("Agent 'conic/writer' applied successfully."));

        let updated_agent = rest_apply_manifest(
            &cli,
            "apiVersion: talon.impalasys.com/v1\nkind: Agent\nmetadata:\n  name: writer\n  namespace: conic\ndefinition:\n  customSpec:\n    systemPrompt: REST update\n",
            true,
        )
        .await
        .unwrap();
        assert!(updated_agent.contains("Agent 'conic/writer' applied successfully."));
        let deleted_agent = rest_delete_resource(&cli, "agent", "writer", Some(&namespace))
            .await
            .unwrap();
        assert!(deleted_agent.contains("deleted successfully"));

        let created_workflow = rest_apply_manifest(
            &cli,
            "apiVersion: talon.impalasys.com/v1\nkind: Workflow\nmetadata:\n  name: retention\n  namespace: conic\nspec:\n  steps:\n    - id: copy\n      type: transform\n      input:\n        answer: ${$.input.answer}\n  output:\n    answer: ${$.steps.copy.output.answer}\n",
            false,
        )
        .await
        .unwrap();
        assert!(created_workflow.contains("Workflow applied successfully."));
        let workflow_yaml = rest_get_yaml(&cli, "workflow", "retention", Some(&namespace))
            .await
            .unwrap();
        assert_eq!(parse_raw_manifest(&workflow_yaml).unwrap().kind, "Workflow");
        assert!(workflow_yaml.contains("namespace: conic"));
        let deleted_workflow =
            rest_delete_resource(&cli, "workflow", "retention", Some(&namespace))
                .await
                .unwrap();
        assert!(deleted_workflow.contains("deleted successfully"));

        server.abort();
    }

    #[tokio::test]
    async fn rest_apply_get_and_delete_helpers_cover_remaining_resource_kinds() {
        #[derive(Clone, Default)]
        struct RestState {
            namespaces: Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>,
            knowledge: Arc<tokio::sync::Mutex<HashMap<(String, String), serde_json::Value>>>,
            bindings: Arc<tokio::sync::Mutex<HashMap<(String, String), serde_json::Value>>>,
            servers: Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>,
        }

        let state = RestState::default();
        let app = Router::new()
            .route(
                "/v1/namespaces/:name",
                post(
                    |State(state): State<RestState>,
                     AxumPath(name): AxumPath<String>,
                     Json(payload): Json<serde_json::Value>| async move {
                        state.namespaces.lock().await.insert(name, payload.clone());
                        Json(json!({ "ok": true }))
                    },
                )
                .get(
                    |State(state): State<RestState>, AxumPath(name): AxumPath<String>| async move {
                        let value = state.namespaces.lock().await.get(&name).cloned();
                        match value {
                            Some(namespace) => (StatusCode::OK, Json(namespace)),
                            None => (StatusCode::NOT_FOUND, Json(json!({ "error": "missing" }))),
                        }
                    },
                )
                .delete(
                    |State(state): State<RestState>, AxumPath(name): AxumPath<String>| async move {
                        state.namespaces.lock().await.remove(&name);
                        Json(json!({ "deleted": true }))
                    },
                ),
            )
            .route(
                "/v1/namespaces/:ns/knowledge",
                post(
                    |State(state): State<RestState>,
                     AxumPath(ns): AxumPath<String>,
                     Json(payload): Json<serde_json::Value>| async move {
                        let knowledge = payload["knowledge"].clone();
                        let name = knowledge["metadata"]["name"].as_str().unwrap().to_string();
                        state.knowledge.lock().await.insert((ns, name), knowledge);
                        Json(json!({ "ok": true }))
                    },
                ),
            )
            .route(
                "/v1/namespaces/:ns/knowledge/:name",
                get(
                    |State(state): State<RestState>,
                     AxumPath((ns, name)): AxumPath<(String, String)>| async move {
                        let key = (ns, urlencoding::decode(&name).unwrap().into_owned());
                        let value = state.knowledge.lock().await.get(&key).cloned();
                        match value {
                            Some(knowledge) => {
                                (StatusCode::OK, Json(json!({ "knowledge": knowledge })))
                            }
                            None => (StatusCode::NOT_FOUND, Json(json!({ "error": "missing" }))),
                        }
                    },
                )
                .delete(
                    |State(state): State<RestState>,
                     AxumPath((ns, name)): AxumPath<(String, String)>| async move {
                        let key = (ns, urlencoding::decode(&name).unwrap().into_owned());
                        state.knowledge.lock().await.remove(&key);
                        Json(json!({ "deleted": true }))
                    },
                ),
            )
            .route(
                "/v1/namespaces/:ns/mcp-bindings",
                post(
                    |State(state): State<RestState>,
                     AxumPath(ns): AxumPath<String>,
                     Json(payload): Json<serde_json::Value>| async move {
                        let binding = payload["binding"].clone();
                        let name = binding["metadata"]["name"].as_str().unwrap().to_string();
                        state.bindings.lock().await.insert((ns, name), binding);
                        Json(json!({ "ok": true }))
                    },
                ),
            )
            .route(
                "/v1/namespaces/:ns/mcp-bindings/:name",
                get(
                    |State(state): State<RestState>,
                     AxumPath((ns, name)): AxumPath<(String, String)>| async move {
                        let value = state.bindings.lock().await.get(&(ns, name)).cloned();
                        match value {
                            Some(binding) => (StatusCode::OK, Json(json!({ "binding": binding }))),
                            None => (StatusCode::NOT_FOUND, Json(json!({ "error": "missing" }))),
                        }
                    },
                )
                .delete(
                    |State(state): State<RestState>,
                     AxumPath((ns, name)): AxumPath<(String, String)>| async move {
                        state.bindings.lock().await.remove(&(ns, name));
                        Json(json!({ "deleted": true }))
                    },
                ),
            )
            .route(
                "/v1/mcp-servers",
                post(
                    |State(state): State<RestState>, Json(payload): Json<serde_json::Value>| async move {
                        let server = payload["server"].clone();
                        let name = server["metadata"]["name"].as_str().unwrap().to_string();
                        state.servers.lock().await.insert(name, server);
                        Json(json!({ "ok": true }))
                    },
                ),
            )
            .route(
                "/v1/mcp-servers/:name",
                get(
                    |State(state): State<RestState>, AxumPath(name): AxumPath<String>| async move {
                        let value = state.servers.lock().await.get(&name).cloned();
                        match value {
                            Some(server) => (StatusCode::OK, Json(json!({ "server": server }))),
                            None => (StatusCode::NOT_FOUND, Json(json!({ "error": "missing" }))),
                        }
                    },
                )
                .delete(
                    |State(state): State<RestState>, AxumPath(name): AxumPath<String>| async move {
                        state.servers.lock().await.remove(&name);
                        Json(json!({ "deleted": true }))
                    },
                ),
            )
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cli = rest_cli(format!("http://{addr}"));
        let namespace = "conic".to_string();

        let namespace_message = rest_apply_manifest(
            &cli,
            "apiVersion: talon.impalasys.com/v1\nkind: Namespace\nmetadata:\n  name: conic\n",
            false,
        )
        .await
        .unwrap();
        assert!(namespace_message.contains("Namespace 'conic' applied successfully."));
        let namespace_yaml = rest_get_yaml(&cli, "namespace", "conic", None)
            .await
            .unwrap();
        assert_eq!(
            parse_raw_manifest(&namespace_yaml).unwrap().kind,
            "Namespace"
        );
        assert!(namespace_yaml.contains("name: conic"));
        let namespace_deleted = rest_delete_resource(&cli, "namespace", "conic", None)
            .await
            .unwrap();
        assert!(namespace_deleted.contains("deleted successfully"));

        let knowledge_message = rest_apply_manifest(
            &cli,
            "apiVersion: talon.impalasys.com/v1\nkind: Knowledge\nmetadata:\n  name: docs/rest.md\n  namespace: conic\nspec:\n  path: docs/rest.md\n  content: rest body\n",
            false,
        )
        .await
        .unwrap();
        assert!(knowledge_message.contains("Knowledge 'conic/docs/rest.md' applied successfully."));
        let knowledge_yaml = rest_get_yaml(&cli, "knowledge", "docs/rest.md", Some(&namespace))
            .await
            .unwrap();
        assert!(knowledge_yaml.contains("content: rest body"));
        let knowledge_deleted =
            rest_delete_resource(&cli, "knowledge", "docs/rest.md", Some(&namespace))
                .await
                .unwrap();
        assert!(knowledge_deleted.contains("deleted successfully"));

        let binding_message = rest_apply_manifest(
            &cli,
            "apiVersion: talon.impalasys.com/v1\nkind: McpServerBinding\nmetadata:\n  name: docs\n  namespace: conic\nspec:\n  serverRef: docs-server\n",
            false,
        )
        .await
        .unwrap();
        assert!(binding_message.contains("McpServerBinding 'conic/docs' applied successfully."));
        let binding_yaml = rest_get_yaml(&cli, "mcpbinding", "docs", Some(&namespace))
            .await
            .unwrap();
        assert_eq!(
            parse_raw_manifest(&binding_yaml).unwrap().kind,
            "McpServerBinding"
        );
        assert!(binding_yaml.contains("name: docs"));
        let binding_deleted = rest_delete_resource(&cli, "mcpbinding", "docs", Some(&namespace))
            .await
            .unwrap();
        assert!(binding_deleted.contains("deleted successfully"));

        let server_message = rest_apply_manifest(
            &cli,
            "apiVersion: talon.impalasys.com/v1\nkind: McpServer\nmetadata:\n  name: docs-server\nspec:\n  transport: streamable-http\n  target: https://example.com/mcp\n",
            false,
        )
        .await
        .unwrap();
        assert!(server_message.contains("MCPServer 'docs-server' applied successfully."));
        let server_yaml = rest_get_yaml(&cli, "mcp", "docs-server", None)
            .await
            .unwrap();
        assert_eq!(parse_raw_manifest(&server_yaml).unwrap().kind, "McpServer");
        assert!(server_yaml.contains("transport: streamable-http"));
        let server_deleted = rest_delete_resource(&cli, "mcp", "docs-server", None)
            .await
            .unwrap();
        assert!(server_deleted.contains("deleted successfully"));

        server.abort();
    }

    #[tokio::test]
    async fn sync_knowledge_dir_writes_markdown_and_reports_unsynced_paths() {
        #[derive(Clone, Default)]
        struct AppState {
            store: Arc<tokio::sync::Mutex<HashMap<String, super::Knowledge>>>,
        }

        let state = AppState::default();
        state.store.lock().await.insert(
            "legacy.md".to_string(),
            build_knowledge("conic", "legacy.md", "old".to_string()),
        );

        let app = Router::new()
            .route(
                "/v1/namespaces/:ns/knowledge",
                get(
                    |State(state): State<AppState>, AxumPath(ns): AxumPath<String>| async move {
                        let values = state
                            .store
                            .lock()
                            .await
                            .values()
                            .filter(|item| {
                                item.metadata
                                    .as_ref()
                                    .map(|meta| meta.namespace == ns)
                                    .unwrap_or(false)
                            })
                            .cloned()
                            .collect::<Vec<_>>();
                        Json(json!({ "knowledge": values }))
                    },
                )
                .post(
                    |State(state): State<AppState>, Json(payload): Json<serde_json::Value>| async move {
                        let knowledge: super::Knowledge =
                            serde_json::from_value(payload["knowledge"].clone()).unwrap();
                        let name = knowledge.metadata.as_ref().unwrap().name.clone();
                        state.store.lock().await.insert(name, knowledge);
                        Json(json!({ "ok": true }))
                    },
                ),
            )
            .with_state(state.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let root = temp_root("talon-cli-sync");
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("one.md"), "one").unwrap();
        fs::write(root.join("nested/two.md"), "two").unwrap();
        fs::write(root.join("skip.txt"), "skip").unwrap();

        let cli = rest_cli(format!("http://{addr}"));
        let (synced_count, unsynced_existing) =
            sync_knowledge_dir(&cli, "conic", root.to_str().unwrap())
                .await
                .unwrap();
        assert_eq!(synced_count, 2);
        assert_eq!(unsynced_existing, vec!["legacy.md".to_string()]);

        let stored = state.store.lock().await;
        assert!(stored.contains_key("one.md"));
        assert!(stored.contains_key("nested/two.md"));

        drop(stored);
        fs::remove_dir_all(root).unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn rest_helpers_surface_missing_fields_and_server_errors() {
        let app = Router::new()
            .route(
                "/v1/templates/:name",
                get(|| async { Json(json!({ "wrong": {} })) }),
            )
            .route(
                "/v1/ns/:ns/agents/:name",
                delete(|| async { (StatusCode::BAD_REQUEST, "cannot delete") }),
            )
            .route(
                "/v1/mcp-servers",
                post(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "bad server") }),
            );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cli = rest_cli(format!("http://{addr}"));
        let namespace = "conic".to_string();

        let get_err = rest_get_yaml(&cli, "template", "missing-template", None)
            .await
            .unwrap_err()
            .to_string();
        assert!(get_err.contains("REST response missing template"));

        let delete_err = rest_delete_resource(&cli, "agent", "writer", Some(&namespace))
            .await
            .unwrap_err()
            .to_string();
        assert!(delete_err.contains("Failed to delete agent 'writer'"));

        let apply_err = rest_apply_manifest(
            &cli,
            "apiVersion: talon.impalasys.com/v1\nkind: McpServer\nmetadata:\n  name: docs-server\nspec:\n  transport: streamable-http\n  target: https://example.com/mcp\n",
            false,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(apply_err.contains("Gateway rejected MCPServer 'docs-server'"));

        server.abort();
    }

    #[tokio::test]
    async fn run_cli_dispatches_render_and_gen_commands() {
        let root = temp_root("talon-cli-run-render");
        fs::create_dir_all(&root).unwrap();
        let manifest = root.join("agent.yaml");
        fs::write(
            &manifest,
            "apiVersion: talon.impalasys.com/v1\nkind: AgentTemplate\nmetadata:\n  name: generated-writer\ndefinition:\n  customSpec:\n    systemPrompt: {{ vars.prompt }}\n",
        )
        .unwrap();

        let render_yaml = Cli {
            command: Commands::Render {
                file: manifest.display().to_string(),
                vars: vec!["prompt=Ship it".to_string()],
                format: RenderFormat::Yaml,
            },
            ..cli()
        };
        assert!(run_cli(&render_yaml).await.unwrap().exit_code.is_none());

        let render_json = Cli {
            command: Commands::Render {
                file: manifest.display().to_string(),
                vars: vec!["prompt=Ship it".to_string()],
                format: RenderFormat::Json,
            },
            ..cli()
        };
        assert!(run_cli(&render_json).await.unwrap().exit_code.is_none());

        let out = root.join("client.ts");
        let gen = Cli {
            command: Commands::Gen {
                dir: root.display().to_string(),
                out: out.display().to_string(),
            },
            ..cli()
        };
        assert!(run_cli(&gen).await.unwrap().exit_code.is_none());
        let generated = fs::read_to_string(&out).unwrap();
        assert!(generated.contains("class TalonClient"));

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn run_cli_dispatches_knowledge_commands_and_missing_get() {
        #[derive(Clone, Default)]
        struct AppState {
            store: Arc<tokio::sync::Mutex<HashMap<String, super::Knowledge>>>,
        }

        let state = AppState::default();
        let app = Router::new()
            .route(
                "/v1/namespaces/:ns/knowledge",
                get(
                    |State(state): State<AppState>, AxumPath(ns): AxumPath<String>| async move {
                        let values = state
                            .store
                            .lock()
                            .await
                            .values()
                            .filter(|item| {
                                item.metadata
                                    .as_ref()
                                    .map(|meta| meta.namespace == ns)
                                    .unwrap_or(false)
                            })
                            .cloned()
                            .collect::<Vec<_>>();
                        Json(json!({ "knowledge": values }))
                    },
                )
                .post(
                    |State(state): State<AppState>, Json(payload): Json<serde_json::Value>| async move {
                        let knowledge: super::Knowledge =
                            serde_json::from_value(payload["knowledge"].clone()).unwrap();
                        let name = knowledge.metadata.as_ref().unwrap().name.clone();
                        state.store.lock().await.insert(name, knowledge);
                        Json(json!({ "ok": true }))
                    },
                ),
            )
            .route(
                "/v1/namespaces/:ns/knowledge/:name",
                get(
                    |State(state): State<AppState>,
                     AxumPath((ns, name)): AxumPath<(String, String)>| async move {
                        let key = (ns, urlencoding::decode(&name).unwrap().into_owned());
                        let value = state.store.lock().await.get(&key.1).cloned().filter(|item| {
                            item.metadata
                                .as_ref()
                                .map(|meta| meta.namespace == key.0)
                                .unwrap_or(false)
                        });
                        match value {
                            Some(knowledge) => {
                                (StatusCode::OK, Json(json!({ "knowledge": knowledge })))
                            }
                            None => (StatusCode::NOT_FOUND, Json(json!({ "error": "missing" }))),
                        }
                    },
                )
                .delete(
                    |State(state): State<AppState>,
                     AxumPath((_ns, name)): AxumPath<(String, String)>| async move {
                        let name = urlencoding::decode(&name).unwrap().into_owned();
                        state.store.lock().await.remove(&name);
                        Json(json!({ "deleted": true }))
                    },
                ),
            )
            .with_state(state.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let root = temp_root("talon-cli-run-knowledge");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("article.md");
        fs::write(&file, "knowledge body").unwrap();

        let set = Cli {
            gateway: format!("http://{addr}"),
            rest: true,
            command: Commands::Knowledge {
                command: KnowledgeCommands::Set {
                    namespace: "conic".to_string(),
                    path: "docs/article.md".to_string(),
                    file: Some(file.display().to_string()),
                    content: None,
                },
            },
            ..cli()
        };
        assert!(run_cli(&set).await.unwrap().exit_code.is_none());

        let get = Cli {
            gateway: format!("http://{addr}"),
            rest: true,
            command: Commands::Knowledge {
                command: KnowledgeCommands::Get {
                    namespace: "conic".to_string(),
                    path: "docs/article.md".to_string(),
                },
            },
            ..cli()
        };
        assert!(run_cli(&get).await.unwrap().exit_code.is_none());

        let sync = Cli {
            gateway: format!("http://{addr}"),
            rest: true,
            command: Commands::Knowledge {
                command: KnowledgeCommands::Sync {
                    namespace: "conic".to_string(),
                    dir: root.display().to_string(),
                },
            },
            ..cli()
        };
        assert!(run_cli(&sync).await.unwrap().exit_code.is_none());

        let delete = Cli {
            gateway: format!("http://{addr}"),
            rest: true,
            command: Commands::Knowledge {
                command: KnowledgeCommands::Delete {
                    namespace: "conic".to_string(),
                    path: "docs/article.md".to_string(),
                },
            },
            ..cli()
        };
        assert!(run_cli(&delete).await.unwrap().exit_code.is_none());

        server.abort();

        let (grpc_addr, _kv) = serve_grpc_gateway().await;
        let missing = Cli {
            gateway: format!("http://{grpc_addr}"),
            command: Commands::Knowledge {
                command: KnowledgeCommands::Get {
                    namespace: "conic".to_string(),
                    path: "docs/missing.md".to_string(),
                },
            },
            ..cli()
        };
        assert_eq!(run_cli(&missing).await.unwrap().exit_code, Some(1));

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn run_cli_dispatches_rest_apply_get_and_delete() {
        #[derive(Clone, Default)]
        struct RestState {
            templates: Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>,
        }

        let state = RestState::default();
        let app = Router::new()
            .route(
                "/v1/templates",
                post(
                    |State(state): State<RestState>, Json(payload): Json<serde_json::Value>| async move {
                        let template = payload["template"].clone();
                        let name = template["metadata"]["name"].as_str().unwrap().to_string();
                        state.templates.lock().await.insert(name, template);
                        Json(json!({ "ok": true }))
                    },
                ),
            )
            .route(
                "/v1/templates/:name",
                get(
                    |State(state): State<RestState>, AxumPath(name): AxumPath<String>| async move {
                        let value = state.templates.lock().await.get(&name).cloned();
                        match value {
                            Some(template) => (StatusCode::OK, Json(json!({ "template": template }))),
                            None => (StatusCode::NOT_FOUND, Json(json!({ "error": "missing" }))),
                        }
                    },
                )
                .delete(
                    |State(state): State<RestState>, AxumPath(name): AxumPath<String>| async move {
                        state.templates.lock().await.remove(&name);
                        Json(json!({ "deleted": true }))
                    },
                ),
            )
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let root = temp_root("talon-cli-run-rest");
        fs::create_dir_all(&root).unwrap();
        let manifest = root.join("template.yaml");
        fs::write(
            &manifest,
            "apiVersion: talon.impalasys.com/v1\nkind: AgentTemplate\nmetadata:\n  name: rest-dispatch\ndefinition:\n  customSpec:\n    systemPrompt: REST dispatch\n",
        )
        .unwrap();

        let apply = Cli {
            gateway: format!("http://{addr}"),
            rest: true,
            command: Commands::Apply {
                file: manifest.display().to_string(),
                vars: Vec::new(),
            },
            ..cli()
        };
        assert!(run_cli(&apply).await.unwrap().exit_code.is_none());

        let get = Cli {
            gateway: format!("http://{addr}"),
            rest: true,
            command: Commands::Get {
                kind: "template".to_string(),
                name: "rest-dispatch".to_string(),
                namespace: None,
            },
            ..cli()
        };
        assert!(run_cli(&get).await.unwrap().exit_code.is_none());

        let delete = Cli {
            gateway: format!("http://{addr}"),
            rest: true,
            command: Commands::Delete {
                kind: "template".to_string(),
                name: "rest-dispatch".to_string(),
                namespace: None,
            },
            ..cli()
        };
        assert!(run_cli(&delete).await.unwrap().exit_code.is_none());

        fs::remove_dir_all(root).unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn run_cli_dispatches_grpc_apply_get_and_delete() {
        let (addr, _kv) = serve_grpc_gateway().await;

        let root = temp_root("talon-cli-run-grpc");
        fs::create_dir_all(&root).unwrap();
        let manifest = root.join("template.yaml");
        fs::write(
            &manifest,
            "apiVersion: talon.impalasys.com/v1\nkind: AgentTemplate\nmetadata:\n  name: grpc-dispatch\ndefinition:\n  customSpec:\n    systemPrompt: gRPC dispatch\n",
        )
        .unwrap();

        let apply = Cli {
            gateway: format!("http://{addr}"),
            command: Commands::Apply {
                file: manifest.display().to_string(),
                vars: Vec::new(),
            },
            ..cli()
        };
        assert!(run_cli(&apply).await.unwrap().exit_code.is_none());

        let get = Cli {
            gateway: format!("http://{addr}"),
            command: Commands::Get {
                kind: "template".to_string(),
                name: "grpc-dispatch".to_string(),
                namespace: None,
            },
            ..cli()
        };
        assert!(run_cli(&get).await.unwrap().exit_code.is_none());

        let delete = Cli {
            gateway: format!("http://{addr}"),
            command: Commands::Delete {
                kind: "template".to_string(),
                name: "grpc-dispatch".to_string(),
                namespace: None,
            },
            ..cli()
        };
        assert!(run_cli(&delete).await.unwrap().exit_code.is_none());

        let workflow_manifest = root.join("workflow.yaml");
        fs::write(
            &workflow_manifest,
            "apiVersion: talon.impalasys.com/v1\nkind: Workflow\nmetadata:\n  name: grpc-workflow\n  namespace: conic\nspec:\n  steps:\n    - id: copy\n      type: transform\n      input:\n        answer: ${$.input.answer}\n  output:\n    answer: ${$.steps.copy.output.answer}\n",
        )
        .unwrap();
        let apply_workflow = Cli {
            gateway: format!("http://{addr}"),
            command: Commands::Apply {
                file: workflow_manifest.display().to_string(),
                vars: Vec::new(),
            },
            ..cli()
        };
        assert!(run_cli(&apply_workflow).await.unwrap().exit_code.is_none());

        let get_workflow = Cli {
            gateway: format!("http://{addr}"),
            command: Commands::Get {
                kind: "workflow".to_string(),
                name: "grpc-workflow".to_string(),
                namespace: Some("conic".to_string()),
            },
            ..cli()
        };
        assert!(run_cli(&get_workflow).await.unwrap().exit_code.is_none());

        let create_run = Cli {
            gateway: format!("http://{addr}"),
            command: Commands::Workflow {
                command: WorkflowCommands::RunCreate {
                    namespace: "conic".to_string(),
                    workflow: "grpc-workflow".to_string(),
                    input: Some(r#"{"answer":"cli"}"#.to_string()),
                    input_file: None,
                },
            },
            ..cli()
        };
        assert!(run_cli(&create_run).await.unwrap().exit_code.is_none());

        let runs = workflow_run_list(
            &Cli {
                gateway: format!("http://{addr}"),
                ..cli()
            },
            "conic",
            "grpc-workflow",
            0,
            "",
        )
        .await
        .unwrap();
        let run_id = runs["runs"][0]["id"].as_str().unwrap().to_string();

        let get_run = Cli {
            gateway: format!("http://{addr}"),
            command: Commands::Workflow {
                command: WorkflowCommands::RunGet {
                    namespace: "conic".to_string(),
                    workflow: "grpc-workflow".to_string(),
                    run_id: run_id.clone(),
                },
            },
            ..cli()
        };
        assert!(run_cli(&get_run).await.unwrap().exit_code.is_none());

        let list_runs = Cli {
            gateway: format!("http://{addr}"),
            command: Commands::Workflow {
                command: WorkflowCommands::RunList {
                    namespace: "conic".to_string(),
                    workflow: "grpc-workflow".to_string(),
                    page_size: 0,
                    before_run_id: String::new(),
                },
            },
            ..cli()
        };
        assert!(run_cli(&list_runs).await.unwrap().exit_code.is_none());

        let cancel_run = Cli {
            gateway: format!("http://{addr}"),
            command: Commands::Workflow {
                command: WorkflowCommands::RunCancel {
                    namespace: "conic".to_string(),
                    workflow: "grpc-workflow".to_string(),
                    run_id,
                },
            },
            ..cli()
        };
        assert!(run_cli(&cancel_run).await.unwrap().exit_code.is_none());

        let delete_workflow = Cli {
            gateway: format!("http://{addr}"),
            command: Commands::Delete {
                kind: "workflow".to_string(),
                name: "grpc-workflow".to_string(),
                namespace: Some("conic".to_string()),
            },
            ..cli()
        };
        assert!(run_cli(&delete_workflow).await.unwrap().exit_code.is_none());

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn grpc_helpers_surface_invalid_gateway_and_missing_resources() {
        let invalid_cli = Cli {
            gateway: "not-a-url".to_string(),
            ..cli()
        };
        let invalid_get = grpc_get_yaml(&invalid_cli, "template", "starter", None)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            invalid_get.contains("Invalid gateway URL")
                || invalid_get.contains("Could not connect to gateway")
                || invalid_get.contains("transport error")
        );

        let invalid_delete = grpc_delete_resource(&invalid_cli, "template", "starter", None)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            invalid_delete.contains("Invalid gateway URL")
                || invalid_delete.contains("Could not connect to gateway")
                || invalid_delete.contains("transport error")
        );

        let invalid_apply = grpc_apply_manifest(
            &invalid_cli,
            "apiVersion: talon.impalasys.com/v1\nkind: AgentTemplate\nmetadata:\n  name: starter\ndefinition:\n  customSpec:\n    systemPrompt: hi\n",
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(
            invalid_apply.contains("Invalid gateway URL")
                || invalid_apply.contains("Could not connect to gateway")
                || invalid_apply.contains("transport error")
        );

        let (addr, _kv) = serve_grpc_gateway().await;
        let cli = Cli {
            gateway: format!("http://{addr}"),
            ..cli()
        };

        let missing_template = grpc_get_yaml(&cli, "template", "missing-template", None)
            .await
            .unwrap_err()
            .to_string();
        assert!(missing_template.contains("Failed to fetch AgentTemplate"));

        let missing_mcp = grpc_delete_resource(&cli, "mcp", "missing-server", None)
            .await
            .unwrap_err()
            .to_string();
        assert!(missing_mcp.contains("Failed to delete MCPServer"));
    }

    #[tokio::test]
    async fn rest_request_json_handles_success_empty_body_and_errors() {
        let app = Router::new()
            .route(
                "/json",
                get(|| async { Json(json!({ "ok": true, "value": 7 })) }),
            )
            .route("/empty", delete(|| async { StatusCode::NO_CONTENT }))
            .route("/bad-json", get(|| async { "not-json" }))
            .route(
                "/fail",
                post(|| async { (StatusCode::BAD_REQUEST, "broken request") }),
            );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cli = rest_cli(format!("http://{addr}"));

        let ok = rest_request_json(&cli, reqwest::Method::GET, "/json", None)
            .await
            .unwrap();
        assert_eq!(ok["ok"], true);
        assert_eq!(ok["value"], 7);

        let empty = rest_request_json(&cli, reqwest::Method::DELETE, "/empty", None)
            .await
            .unwrap();
        assert_eq!(empty, serde_json::Value::Null);

        let parse_err = rest_request_json(&cli, reqwest::Method::GET, "/bad-json", None)
            .await
            .unwrap_err()
            .to_string();
        assert!(parse_err.contains("Failed to parse REST response JSON"));

        let fail_err = rest_request_json(
            &cli,
            reqwest::Method::POST,
            "/fail",
            Some(json!({ "x": 1 })),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(fail_err.contains("status=400 Bad Request"));
        assert!(fail_err.contains("broken request"));

        server.abort();
    }
}
