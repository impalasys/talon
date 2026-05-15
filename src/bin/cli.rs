// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use base64::Engine;
use clap::{Parser, Subcommand};
use jsonwebtoken::{EncodingKey, Header};
use minijinja::{context, Environment, UndefinedBehavior};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use talon::gateway::rpc::models;
use talon::gateway::rpc::manifests::{Knowledge, KnowledgeSpec, ObjectMeta};
use talon::gateway::rpc::proto::gateway_service_client::GatewayServiceClient;
use talon::gateway::rpc::proto::{
    CreateAgentRequest, CreateAgentTemplateRequest, CreateMcpServerRequest,
    CreateNamespaceKnowledgeRequest, DeleteAgentTemplateRequest, DeleteMcpServerRequest,
    DeleteNamespaceKnowledgeRequest, GetAgentTemplateRequest, GetMcpServerRequest,
    GetNamespaceKnowledgeRequest, GetScheduleRequest, ListNamespaceKnowledgeRequest,
    ModifyAgentRequest,
};
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::{Request, Status};

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
    exp: usize,
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
    let exp = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        + 3600) as usize;
    let claims = CliClaims {
        sub: "talon-cli".to_string(),
        aud: "talon".to_string(),
        exp,
    };
    jsonwebtoken::encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .context("Failed to sign Talon CLI JWT")
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
                "sessionMode": target.session_mode,
                "sessionId": target.session_id,
            })),
            "inputMessage": spec.input_message,
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
        other => anyhow::bail!("Unsupported manifest kind '{}'", other),
    }
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

#[derive(Subcommand)]
enum Commands {
    /// Manage namespace knowledge artifacts directly by path.
    Knowledge {
        #[command(subcommand)]
        command: KnowledgeCommands,
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

    match &cli.command {
        Commands::Knowledge { command } => match command {
            KnowledgeCommands::Get { namespace, path } => {
                let knowledge = knowledge_get(&cli, namespace, path).await?;
                let Some(knowledge) = knowledge else {
                    eprintln!("Knowledge '{}/{}' not found.", namespace, path);
                    std::process::exit(1);
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
                return Ok(());
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
                return Ok(());
            }
            KnowledgeCommands::Delete { namespace, path } => {
                knowledge_delete(&cli, namespace, path).await?;
                println!("✓ Knowledge '{}/{}' deleted successfully.", namespace, path);
                return Ok(());
            }
            KnowledgeCommands::Sync { namespace, dir } => {
                let root = Path::new(dir);
                let files = collect_markdown_files(root)?;
                let existing: Vec<Knowledge> = knowledge_list(&cli, namespace).await?;
                let existing_paths = existing
                    .into_iter()
                    .filter_map(|knowledge| knowledge.spec.map(|spec| spec.path))
                    .collect::<std::collections::HashSet<_>>();
                let mut synced_paths = Vec::new();

                for file in files {
                    let knowledge_path = relative_knowledge_path(root, &file)?;
                    let content = fs::read_to_string(&file).with_context(|| {
                        format!("Failed to read knowledge file '{}'", file.display())
                    })?;
                    knowledge_set(&cli, namespace, &knowledge_path, content).await?;
                    synced_paths.push(knowledge_path);
                }

                println!(
                    "✓ Synced {} knowledge artifact(s) into '{}'.",
                    synced_paths.len(),
                    namespace
                );

                let unsynced_existing = existing_paths
                    .into_iter()
                    .filter(|path| !synced_paths.iter().any(|synced| synced == path))
                    .collect::<Vec<_>>();
                if !unsynced_existing.is_empty() {
                    eprintln!(
                        "Note: {} existing knowledge artifact(s) in '{}' were left untouched because they are not present in '{}'.",
                        unsynced_existing.len(),
                        namespace,
                        root.display()
                    );
                }
                return Ok(());
            }
        },
        Commands::Apply { file, vars } => {
            let content = render_manifest_file(file, vars)?;
            let raw = parse_raw_manifest(&content)?;

            if cli.rest {
                let (_, payload) = manifest_json_payload(&content)?;
                match raw.kind.as_str() {
                    "AgentTemplate" => {
                        let template = talon::manifest::parse_agent_template(&content)?;
                        let name = template
                            .metadata
                            .as_ref()
                            .map(|m| m.name.clone())
                            .unwrap_or_default();
                        println!(
                            "Applying AgentTemplate '{}' via REST gateway {}...",
                            name, cli.gateway
                        );
                        rest_request_json(
                            &cli,
                            reqwest::Method::POST,
                            "/v1/templates",
                            Some(payload),
                        )
                        .await
                        .with_context(|| format!("Gateway rejected template '{}'", name))?;
                        println!("✓ AgentTemplate '{}' applied successfully.", name);
                        return Ok(());
                    }
                    "MCPServer" | "McpServer" => {
                        let server = talon::manifest::parse_mcp_server(&content)?;
                        let meta = server
                            .metadata
                            .as_ref()
                            .context("MCPServer missing metadata")?;
                        let server_name = meta.name.clone();
                        if !meta.namespace.is_empty() {
                            anyhow::bail!(
                                "MCPServer metadata.namespace is not supported; MCP servers are system resources in talon-system"
                            );
                        }
                        println!(
                            "Applying MCPServer '{}' via REST gateway {}...",
                            server_name, cli.gateway
                        );
                        rest_request_json(
                            &cli,
                            reqwest::Method::POST,
                            "/v1/mcp-servers",
                            Some(payload),
                        )
                        .await
                        .with_context(|| format!("Gateway rejected MCPServer '{}'", server_name))?;
                        println!("✓ MCPServer '{}' applied successfully.", server_name);
                        return Ok(());
                    }
                    "Agent" => {
                        let agent = talon::manifest::parse_agent(&content)?;
                        let name = agent.name.clone();
                        let ns = agent.ns.clone();
                        let definition = payload
                            .get("definition")
                            .cloned()
                            .context("Agent payload missing definition")?;
                        let labels = payload
                            .get("labels")
                            .cloned()
                            .context("Agent payload missing labels")?;

                        println!(
                            "Applying Agent '{}/{}' via REST gateway {}...",
                            ns, name, cli.gateway
                        );
                        let get_path = format!(
                            "/v1/ns/{}/agents/{}",
                            urlencoding::encode(&ns),
                            urlencoding::encode(&name)
                        );
                        let exists = rest_request_json(&cli, reqwest::Method::GET, &get_path, None)
                            .await
                            .is_ok();
                        let (method, path, payload) = if exists {
                            (
                                reqwest::Method::PUT,
                                get_path,
                                json!({
                                    "ns": ns,
                                    "agent": name,
                                    "labels": labels,
                                    "definition": definition,
                                }),
                            )
                        } else {
                            (
                                reqwest::Method::POST,
                                format!("/v1/ns/{}/agents", urlencoding::encode(&ns)),
                                json!({
                                    "ns": ns,
                                    "name": name,
                                    "labels": labels,
                                    "definition": definition,
                                }),
                            )
                        };
                        rest_request_json(&cli, method, &path, Some(payload))
                            .await
                            .with_context(|| format!("Gateway rejected Agent '{}/{}'", ns, name))?;
                        println!("✓ Agent '{}/{}' applied successfully.", ns, name);
                        return Ok(());
                    }
                    "McpServerBinding" => {
                        let binding = talon::manifest::parse_mcp_server_binding(&content)?;
                        let meta = binding
                            .metadata
                            .as_ref()
                            .context("McpServerBinding missing metadata")?;
                        let ns = meta.namespace.clone();
                        let name = meta.name.clone();
                        println!(
                            "Applying McpServerBinding '{}/{}' via REST gateway {}...",
                            ns, name, cli.gateway
                        );
                        rest_request_json(
                            &cli,
                            reqwest::Method::POST,
                            &format!("/v1/namespaces/{}/mcp-bindings", urlencoding::encode(&ns)),
                            Some(json!({ "ns": ns, "binding": binding })),
                        )
                        .await
                        .with_context(|| {
                            format!("Gateway rejected McpServerBinding '{}/{}'", ns, name)
                        })?;
                        println!("✓ McpServerBinding '{}/{}' applied successfully.", ns, name);
                        return Ok(());
                    }
                    "Namespace" => {
                        let namespace = talon::manifest::parse_namespace(&content)?;
                        println!(
                            "Applying Namespace '{}' via REST gateway {}...",
                            namespace.name, cli.gateway
                        );
                        rest_request_json(
                            &cli,
                            reqwest::Method::POST,
                            &format!("/v1/namespaces/{}", urlencoding::encode(&namespace.name)),
                            Some(json!({
                                "name": namespace.name,
                                "recursive": true,
                                "labels": namespace.labels,
                            })),
                        )
                        .await
                        .with_context(|| {
                            format!("Gateway rejected Namespace '{}'", namespace.name)
                        })?;
                        println!("✓ Namespace '{}' applied successfully.", namespace.name);
                        return Ok(());
                    }
                    "Knowledge" => {
                        let knowledge = talon::manifest::parse_knowledge(&content)?;
                        let meta = knowledge
                            .metadata
                            .as_ref()
                            .context("Knowledge missing metadata")?;
                        let ns = meta.namespace.clone();
                        let knowledge_name = meta.name.clone();
                        if ns.is_empty() {
                            anyhow::bail!("Knowledge metadata.namespace is required");
                        }
                        println!(
                            "Applying Knowledge '{}/{}' via REST gateway {}...",
                            ns, knowledge_name, cli.gateway
                        );
                        rest_request_json(
                            &cli,
                            reqwest::Method::POST,
                            &format!("/v1/namespaces/{}/knowledge", urlencoding::encode(&ns)),
                            Some(payload),
                        )
                        .await
                        .with_context(|| {
                            format!("Gateway rejected Knowledge '{}/{}'", ns, knowledge_name)
                        })?;
                        println!(
                            "✓ Knowledge '{}/{}' applied successfully.",
                            ns, knowledge_name
                        );
                        return Ok(());
                    }
                    other => {
                        eprintln!("Error: Unsupported manifest kind '{}'", other);
                        std::process::exit(1);
                    }
                }
            }

            match raw.kind.as_str() {
                "Agent" => {
                    let agent = talon::manifest::parse_agent(&content)?;
                    let name = agent.name.clone();
                    let ns = agent.ns.clone();
                    let definition = agent
                        .definition
                        .clone()
                        .context("Agent definition must be provided")?;

                    println!("Applying Agent '{}/{}' via gateway {}...", ns, name, cli.gateway);

                    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
                        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
                        .connect()
                        .await
                        .with_context(|| {
                            format!("Could not connect to gateway at {}", cli.gateway)
                        })?;
                    let mut client =
                        GatewayServiceClient::with_interceptor(channel, auth_interceptor(&cli)?);

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
                                    labels: agent.labels.clone(),
                                })
                                .await
                                .with_context(|| {
                                    format!("Gateway rejected Agent '{}/{}'", ns, name)
                                })?;
                        }
                        Err(status) if status.code() == tonic::Code::NotFound => {
                            client
                                .create_agent(CreateAgentRequest {
                                    ns: ns.clone(),
                                    name: Some(name.clone()),
                                    definition: Some(definition),
                                    labels: agent.labels.clone(),
                                })
                                .await
                                .with_context(|| {
                                    format!("Gateway rejected Agent '{}/{}'", ns, name)
                                })?;
                        }
                        Err(status) => return Err(status.into()),
                    }

                    println!("✓ Agent '{}/{}' applied successfully.", ns, name);
                }
                "AgentTemplate" => {
                    let template = talon::manifest::parse_agent_template(&content)?;
                    let name = template
                        .metadata
                        .as_ref()
                        .map(|m| m.name.clone())
                        .unwrap_or_default();

                    println!(
                        "Applying AgentTemplate '{}' via gateway {}...",
                        name, cli.gateway
                    );

                    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
                        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
                        .connect()
                        .await
                        .with_context(|| {
                            format!("Could not connect to gateway at {}", cli.gateway)
                        })?;
                    let mut client =
                        GatewayServiceClient::with_interceptor(channel, auth_interceptor(&cli)?);

                    client
                        .create_agent_template(CreateAgentTemplateRequest {
                            template: Some(template),
                        })
                        .await
                        .with_context(|| format!("Gateway rejected template '{}'", name))?;

                    println!("✓ AgentTemplate '{}' applied successfully.", name);
                }
                "MCPServer" | "McpServer" => {
                    let server = talon::manifest::parse_mcp_server(&content)?;
                    let meta = server
                        .metadata
                        .as_ref()
                        .context("MCPServer missing metadata")?;
                    let server_name = meta.name.clone();
                    if !meta.namespace.is_empty() {
                        anyhow::bail!(
                            "MCPServer metadata.namespace is not supported; MCP servers are system resources in talon-system"
                        );
                    }

                    println!(
                        "Applying MCPServer '{}' via gateway {}...",
                        server_name, cli.gateway
                    );

                    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
                        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
                        .connect()
                        .await
                        .with_context(|| {
                            format!("Could not connect to gateway at {}", cli.gateway)
                        })?;
                    let mut client =
                        GatewayServiceClient::with_interceptor(channel, auth_interceptor(&cli)?);

                    client
                        .create_mcp_server(CreateMcpServerRequest {
                            server: Some(server),
                        })
                        .await
                        .context("Gateway rejected MCPServer")?;

                    println!("✓ MCPServer '{}' applied successfully.", server_name);
                }
                "Knowledge" => {
                    let knowledge = talon::manifest::parse_knowledge(&content)?;
                    let meta = knowledge
                        .metadata
                        .as_ref()
                        .context("Knowledge missing metadata")?;
                    let ns = meta.namespace.clone();
                    let knowledge_name = meta.name.clone();
                    if ns.is_empty() {
                        anyhow::bail!("Knowledge metadata.namespace is required");
                    }

                    println!(
                        "Applying Knowledge '{}/{}' via gateway {}...",
                        ns, knowledge_name, cli.gateway
                    );

                    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
                        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
                        .connect()
                        .await
                        .with_context(|| {
                            format!("Could not connect to gateway at {}", cli.gateway)
                        })?;
                    let mut client =
                        GatewayServiceClient::with_interceptor(channel, auth_interceptor(&cli)?);

                    client
                        .create_namespace_knowledge(CreateNamespaceKnowledgeRequest {
                            ns: ns.clone(),
                            knowledge: Some(knowledge),
                        })
                        .await
                        .with_context(|| {
                            format!("Gateway rejected Knowledge '{}/{}'", ns, knowledge_name)
                        })?;

                    println!(
                        "✓ Knowledge '{}/{}' applied successfully.",
                        ns, knowledge_name
                    );
                }
                other => {
                    eprintln!("Error: Unsupported manifest kind '{}'", other);
                    std::process::exit(1);
                }
            }
        }

        Commands::Render { file, vars, format } => {
            let content = render_manifest_file(file, vars)?;
            match format {
                RenderFormat::Yaml => {
                    print!("{}", content);
                }
                RenderFormat::Json => {
                    let raw = parse_raw_manifest(&content)?;
                    let manifest_value: serde_yaml::Value = serde_yaml::from_str(&content)
                        .context("Failed to parse rendered manifest")?;
                    match raw.kind.as_str() {
                        "AgentTemplate" => {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(
                                    &json!({ "template": manifest_value })
                                )
                                .context("Failed to serialize AgentTemplate JSON")?
                            );
                        }
                        "MCPServer" | "McpServer" => {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&json!({ "server": manifest_value }))
                                    .context("Failed to serialize MCPServer JSON")?
                            );
                        }
                        "Agent" => {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&json!({ "agent": manifest_value }))
                                .context("Failed to serialize Agent JSON")?
                            );
                        }
                        "McpServerBinding" => {
                            let binding = talon::manifest::parse_mcp_server_binding(&content)?;
                            let namespace = binding
                                .metadata
                                .as_ref()
                                .map(|meta| meta.namespace.clone())
                                .filter(|namespace| !namespace.is_empty())
                                .context("McpServerBinding missing metadata.namespace")?;
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&json!({
                                    "ns": namespace,
                                    "binding": binding,
                                }))
                                .context("Failed to serialize McpServerBinding JSON")?
                            );
                        }
                        "Namespace" => {
                            let namespace = talon::manifest::parse_namespace(&content)?;
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&json!({
                                    "name": namespace.name,
                                    "recursive": true,
                                    "labels": namespace.labels,
                                }))
                                .context("Failed to serialize Namespace JSON")?
                            );
                        }
                        "Knowledge" => {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(
                                    &json!({ "knowledge": manifest_value })
                                )
                                .context("Failed to serialize Knowledge JSON")?
                            );
                        }
                        other => {
                            eprintln!("Error: Unsupported manifest kind '{}'", other);
                            std::process::exit(1);
                        }
                    }
                }
            }
        }

        Commands::Get {
            kind,
            name,
            namespace,
        } => {
            if cli.rest {
                match kind.to_lowercase().as_str() {
                    "agenttemplate" | "templates" | "template" => {
                        let resp = rest_request_json(
                            &cli,
                            reqwest::Method::GET,
                            &format!("/v1/templates/{}", urlencoding::encode(name)),
                            None,
                        )
                        .await
                        .with_context(|| format!("Failed to fetch AgentTemplate '{}'", name))?;
                        let template = resp
                            .get("template")
                            .cloned()
                            .context("REST response missing template")?;
                        let yml = serde_yaml::to_string(&template)
                            .context("Failed to serialize AgentTemplate YAML")?;
                        println!("{}", yml);
                        return Ok(());
                    }
                    "mcpserver" | "mcpservers" | "mcp" => {
                        let resp = rest_request_json(
                            &cli,
                            reqwest::Method::GET,
                            &format!("/v1/mcp-servers/{}", urlencoding::encode(name)),
                            None,
                        )
                        .await
                        .with_context(|| format!("Failed to fetch MCPServer '{}'", name))?;
                        let server = resp
                            .get("server")
                            .cloned()
                            .context("REST response missing server")?;
                        let yml = serde_yaml::to_string(&server)
                            .context("Failed to serialize MCPServer YAML")?;
                        println!("{}", yml);
                        return Ok(());
                    }
                    "agent" | "agents" => {
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
                        let resp = rest_request_json(
                            &cli,
                            reqwest::Method::GET,
                            &format!(
                                "/v1/ns/{}/agents/{}",
                                urlencoding::encode(&final_ns),
                                urlencoding::encode(&final_name)
                            ),
                            None,
                        )
                        .await
                        .with_context(|| format!("Failed to fetch Agent '{}'", name))?;
                        let agent = resp
                            .get("agent")
                            .cloned()
                            .context("REST response missing agent")?;
                        let yml = serde_yaml::to_string(&agent)
                            .context("Failed to serialize Agent YAML")?;
                        println!("{}", yml);
                        return Ok(());
                    }
                    "mcpserverbinding" | "mcpbindings" | "mcpbinding" => {
                        let ns = namespace
                            .as_ref()
                            .context("namespace is required for McpServerBinding get")?;
                        let resp = rest_request_json(
                            &cli,
                            reqwest::Method::GET,
                            &format!(
                                "/v1/namespaces/{}/mcp-bindings/{}",
                                urlencoding::encode(ns),
                                urlencoding::encode(name)
                            ),
                            None,
                        )
                        .await
                        .with_context(|| {
                            format!("Failed to fetch McpServerBinding '{}/{}'", ns, name)
                        })?;
                        let binding = resp
                            .get("binding")
                            .cloned()
                            .context("REST response missing binding")?;
                        let yml = serde_yaml::to_string(&binding)
                            .context("Failed to serialize McpServerBinding YAML")?;
                        println!("{}", yml);
                        return Ok(());
                    }
                    "namespace" | "namespaces" => {
                        let resp = rest_request_json(
                            &cli,
                            reqwest::Method::GET,
                            &format!("/v1/namespaces/{}", urlencoding::encode(name)),
                            None,
                        )
                        .await
                        .with_context(|| format!("Failed to fetch Namespace '{}'", name))?;
                        let yml = serde_yaml::to_string(&resp)
                            .context("Failed to serialize Namespace YAML")?;
                        println!("{}", yml);
                        return Ok(());
                    }
                    "knowledge" | "knowledgeartifact" | "knowledgeartifacts" => {
                        let final_ns = namespace
                            .clone()
                            .context("Knowledge get requires --namespace")?;
                        let resp = rest_request_json(
                            &cli,
                            reqwest::Method::GET,
                            &format!(
                                "/v1/namespaces/{}/knowledge/{}",
                                urlencoding::encode(&final_ns),
                                urlencoding::encode(name)
                            ),
                            None,
                        )
                        .await
                        .with_context(|| {
                            format!("Failed to fetch Knowledge '{}/{}'", final_ns, name)
                        })?;
                        let knowledge = resp
                            .get("knowledge")
                            .cloned()
                            .context("REST response missing knowledge")?;
                        let yml = serde_yaml::to_string(&knowledge)
                            .context("Failed to serialize Knowledge YAML")?;
                        println!("{}", yml);
                        return Ok(());
                    }
                    "schedule" | "schedules" => {
                        let final_ns = namespace
                            .clone()
                            .context("Schedule get requires --namespace")?;
                        let resp = rest_request_json(
                            &cli,
                            reqwest::Method::GET,
                            &format!(
                                "/v1/ns/{}/schedules/{}",
                                urlencoding::encode(&final_ns),
                                urlencoding::encode(name)
                            ),
                            None,
                        )
                        .await
                        .with_context(|| {
                            format!("Failed to fetch Schedule '{}/{}'", final_ns, name)
                        })?;
                        let schedule = resp
                            .get("schedule")
                            .cloned()
                            .context("REST response missing schedule")?;
                        let yml = serde_yaml::to_string(&schedule)
                            .context("Failed to serialize Schedule YAML")?;
                        println!("{}", yml);
                        return Ok(());
                    }
                    other => {
                        eprintln!("Error: Unsupported resource kind '{}' for REST mode", other);
                        std::process::exit(1);
                    }
                }
            }
            match kind.to_lowercase().as_str() {
                "agenttemplate" | "templates" | "template" => {
                    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
                        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
                        .connect()
                        .await
                        .with_context(|| {
                            format!("Could not connect to gateway at {}", cli.gateway)
                        })?;
                    let mut client =
                        GatewayServiceClient::with_interceptor(channel, auth_interceptor(&cli)?);

                    let n = name.clone();
                    if let Some(_ns) = namespace {
                        // Optional: Prefix namespace if the API requires it, but current implementation uses name only.
                        // We still pass it as name. If needed later we can update the protobuf.
                    }

                    let resp = client
                        .get_agent_template(GetAgentTemplateRequest { name: n.clone() })
                        .await
                        .with_context(|| format!("Failed to fetch AgentTemplate '{}'", n))?;

                    if let Some(template) = resp.into_inner().template {
                        let yml = talon::manifest::render_agent_template_yaml(&template)?;
                        println!("{}", yml);
                    } else {
                        eprintln!("Resource not found.");
                        std::process::exit(1);
                    }
                }
                "agent" | "agents" => {
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

                    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
                        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
                        .connect()
                        .await
                        .with_context(|| {
                            format!("Could not connect to gateway at {}", cli.gateway)
                        })?;
                    let mut client =
                        GatewayServiceClient::with_interceptor(channel, auth_interceptor(&cli)?);

                    let resp = client
                        .get_agent(talon::gateway::rpc::proto::GetAgentRequest {
                            ns: final_ns,
                            name: final_name,
                        })
                        .await
                        .with_context(|| format!("Failed to fetch Agent '{}'", name))?;

                    if let Some(agent) = resp.into_inner().agent {
                        let yml = talon::manifest::render_agent_yaml(&agent)?;
                        println!("{}", yml);
                    } else {
                        eprintln!("Agent not found.");
                        std::process::exit(1);
                    }
                }
                "mcpserver" | "mcpservers" | "mcp" => {
                    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
                        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
                        .connect()
                        .await
                        .with_context(|| {
                            format!("Could not connect to gateway at {}", cli.gateway)
                        })?;
                    let mut client =
                        GatewayServiceClient::with_interceptor(channel, auth_interceptor(&cli)?);

                    let resp = client
                        .get_mcp_server(GetMcpServerRequest { name: name.clone() })
                        .await
                        .with_context(|| format!("Failed to fetch MCPServer '{}'", name))?;

                    if let Some(server) = resp.into_inner().server {
                        let yml = serde_yaml::to_string(&server)
                            .context("Failed to serialize to YAML")?;
                        println!("{}", yml);
                    } else {
                        eprintln!("Resource not found.");
                        std::process::exit(1);
                    }
                }
                "knowledge" | "knowledgeartifact" | "knowledgeartifacts" => {
                    let final_ns = namespace
                        .clone()
                        .context("Knowledge get requires --namespace")?;
                    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
                        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
                        .connect()
                        .await
                        .with_context(|| {
                            format!("Could not connect to gateway at {}", cli.gateway)
                        })?;
                    let mut client =
                        GatewayServiceClient::with_interceptor(channel, auth_interceptor(&cli)?);

                    let resp = client
                        .get_namespace_knowledge(GetNamespaceKnowledgeRequest {
                            ns: final_ns.clone(),
                            name: name.clone(),
                        })
                        .await
                        .with_context(|| {
                            format!("Failed to fetch Knowledge '{}/{}'", final_ns, name)
                        })?;

                    if let Some(knowledge) = resp.into_inner().knowledge {
                        let yml = talon::manifest::render_knowledge_yaml(&knowledge)?;
                        println!("{}", yml);
                    } else {
                        eprintln!("Knowledge not found.");
                        std::process::exit(1);
                    }
                }
                "schedule" | "schedules" => {
                    let final_ns = namespace
                        .clone()
                        .context("Schedule get requires --namespace")?;
                    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
                        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
                        .connect()
                        .await
                        .with_context(|| {
                            format!("Could not connect to gateway at {}", cli.gateway)
                        })?;
                    let mut client =
                        GatewayServiceClient::with_interceptor(channel, auth_interceptor(&cli)?);

                    let resp = client
                        .get_schedule(GetScheduleRequest {
                            ns: final_ns.clone(),
                            name: name.clone(),
                        })
                        .await
                        .with_context(|| {
                            format!("Failed to fetch Schedule '{}/{}'", final_ns, name)
                        })?;

                    if let Some(schedule) = resp.into_inner().schedule {
                        let yml = serde_yaml::to_string(&schedule_json(&schedule))
                            .context("Failed to serialize Schedule YAML")?;
                        println!("{}", yml);
                    } else {
                        eprintln!("Schedule not found.");
                        std::process::exit(1);
                    }
                }
                other => {
                    eprintln!("Error: Unsupported resource kind '{}'", other);
                    std::process::exit(1);
                }
            }
        }

        Commands::Delete {
            kind,
            name,
            namespace,
        } => {
            if cli.rest {
                match kind.to_lowercase().as_str() {
                    "agenttemplate" | "templates" | "template" => {
                        rest_request_json(
                            &cli,
                            reqwest::Method::DELETE,
                            &format!("/v1/templates/{}", urlencoding::encode(name)),
                            None,
                        )
                        .await
                        .with_context(|| format!("Failed to delete AgentTemplate '{}'", name))?;
                        println!("✓ AgentTemplate '{}' deleted successfully.", name);
                        return Ok(());
                    }
                    "mcpserver" | "mcpservers" | "mcp" => {
                        rest_request_json(
                            &cli,
                            reqwest::Method::DELETE,
                            &format!("/v1/mcp-servers/{}", urlencoding::encode(name)),
                            None,
                        )
                        .await
                        .with_context(|| format!("Failed to delete MCPServer '{}'", name))?;
                        println!("✓ MCPServer '{}' deleted successfully.", name);
                        return Ok(());
                    }
                    "agent" | "agents" => {
                        let ns = namespace
                            .as_ref()
                            .context("namespace is required for Agent delete")?;
                        rest_request_json(
                            &cli,
                            reqwest::Method::DELETE,
                            &format!(
                                "/v1/ns/{}/agents/{}",
                                urlencoding::encode(ns),
                                urlencoding::encode(name)
                            ),
                            None,
                        )
                        .await
                        .with_context(|| format!("Failed to delete Agent '{}/{}'", ns, name))?;
                        println!("✓ Agent '{}/{}' deleted successfully.", ns, name);
                        return Ok(());
                    }
                    "mcpserverbinding" | "mcpbindings" | "mcpbinding" => {
                        let ns = namespace
                            .as_ref()
                            .context("namespace is required for McpServerBinding delete")?;
                        rest_request_json(
                            &cli,
                            reqwest::Method::DELETE,
                            &format!(
                                "/v1/namespaces/{}/mcp-bindings/{}",
                                urlencoding::encode(ns),
                                urlencoding::encode(name)
                            ),
                            None,
                        )
                        .await
                        .with_context(|| {
                            format!("Failed to delete McpServerBinding '{}/{}'", ns, name)
                        })?;
                        println!("✓ McpServerBinding '{}/{}' deleted successfully.", ns, name);
                        return Ok(());
                    }
                    "namespace" | "namespaces" => {
                        rest_request_json(
                            &cli,
                            reqwest::Method::DELETE,
                            &format!("/v1/namespaces/{}", urlencoding::encode(name)),
                            None,
                        )
                        .await
                        .with_context(|| format!("Failed to delete Namespace '{}'", name))?;
                        println!("✓ Namespace '{}' deleted successfully.", name);
                        return Ok(());
                    }
                    "knowledge" | "knowledgeartifact" | "knowledgeartifacts" => {
                        let final_ns = namespace
                            .clone()
                            .context("Knowledge delete requires --namespace")?;
                        rest_request_json(
                            &cli,
                            reqwest::Method::DELETE,
                            &format!(
                                "/v1/namespaces/{}/knowledge/{}",
                                urlencoding::encode(&final_ns),
                                urlencoding::encode(name)
                            ),
                            None,
                        )
                        .await
                        .with_context(|| {
                            format!("Failed to delete Knowledge '{}/{}'", final_ns, name)
                        })?;
                        println!("✓ Knowledge '{}/{}' deleted successfully.", final_ns, name);
                        return Ok(());
                    }
                    other => {
                        eprintln!("Error: Unsupported resource kind '{}' for REST mode", other);
                        std::process::exit(1);
                    }
                }
            }
            match kind.to_lowercase().as_str() {
                "agenttemplate" | "templates" | "template" => {
                    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
                        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
                        .connect()
                        .await
                        .with_context(|| {
                            format!("Could not connect to gateway at {}", cli.gateway)
                        })?;
                    let mut client =
                        GatewayServiceClient::with_interceptor(channel, auth_interceptor(&cli)?);

                    let n = name.clone();
                    if let Some(_ns) = namespace {
                        // Optional prefixing if needed. Currently backend uses 'name' for agenttemplate ID.
                    }

                    client
                        .delete_agent_template(DeleteAgentTemplateRequest { name: n.clone() })
                        .await
                        .with_context(|| format!("Failed to delete AgentTemplate '{}'", n))?;

                    println!("✓ AgentTemplate '{}' deleted successfully.", n);
                }
                "mcpserver" | "mcpservers" | "mcp" => {
                    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
                        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
                        .connect()
                        .await
                        .with_context(|| {
                            format!("Could not connect to gateway at {}", cli.gateway)
                        })?;
                    let mut client =
                        GatewayServiceClient::with_interceptor(channel, auth_interceptor(&cli)?);

                    client
                        .delete_mcp_server(DeleteMcpServerRequest { name: name.clone() })
                        .await
                        .with_context(|| format!("Failed to delete MCPServer '{}'", name))?;

                    println!("✓ MCPServer '{}' deleted successfully.", name);
                }
                "knowledge" | "knowledgeartifact" | "knowledgeartifacts" => {
                    let final_ns = namespace
                        .clone()
                        .context("Knowledge delete requires --namespace")?;
                    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
                        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
                        .connect()
                        .await
                        .with_context(|| {
                            format!("Could not connect to gateway at {}", cli.gateway)
                        })?;
                    let mut client =
                        GatewayServiceClient::with_interceptor(channel, auth_interceptor(&cli)?);

                    client
                        .delete_namespace_knowledge(DeleteNamespaceKnowledgeRequest {
                            ns: final_ns.clone(),
                            name: name.clone(),
                        })
                        .await
                        .with_context(|| {
                            format!("Failed to delete Knowledge '{}/{}'", final_ns, name)
                        })?;

                    println!("✓ Knowledge '{}/{}' deleted successfully.", final_ns, name);
                }
                other => {
                    eprintln!("Error: Unsupported resource kind '{}'", other);
                    std::process::exit(1);
                }
            }
        }

        Commands::Gen { dir, out } => {
            println!("Generating Talon Client SDK from: {}", dir);
            let mut class_methods = Vec::new();

            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().unwrap_or_default() == "yaml" {
                    let content = fs::read_to_string(&path)?;
                    if let Ok(template) = talon::manifest::parse_agent_template(&content) {
                        let name = match template.metadata.as_ref() {
                            Some(m) => m.name.clone(),
                            None => continue,
                        };
                        let method_name = format!("create{}", to_camel_case(&name));
                        let mut args = Vec::new();

                        if let Some(definition) = &template.definition {
                            if let Some(
                                talon::gateway::rpc::manifests::agent_definition::Source::CustomSpec(spec),
                            ) = definition.source.as_ref()
                            {
                                for f in &spec.features {
                                    let ts_type = match f.r#type.as_str() {
                                        "integer" | "number" | "float" => "number",
                                        "boolean" => "boolean",
                                        _ => "string",
                                    };
                                    let opt = if f.required { "" } else { "?" };
                                    args.push(format!("{}{}: {}", f.name, opt, ts_type));
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

                        class_methods.push(format!(
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
                        ));
                    }
                }
            }

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

    Ok(())
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
    use super::{parse_vars, render_manifest_template, resolve_manifest_sources};
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn renders_minijinja_vars() {
        let mut vars = HashMap::new();
        vars.insert(
            "conic_mcp_target_url".to_string(),
            "https://api.useconic.com/mcp".to_string(),
        );
        let rendered = render_manifest_template("target: {{ vars.conic_mcp_target_url }}\n", &vars)
            .expect("render should succeed");
        assert_eq!(rendered.trim_end(), "target: https://api.useconic.com/mcp");
    }

    #[test]
    fn parse_vars_rejects_invalid_pairs() {
        let err = parse_vars(&["missing-separator".to_string()]).expect_err("should fail");
        assert!(err.to_string().contains("expected KEY=VALUE"));
    }

    #[test]
    fn resolves_knowledge_content_from_file() {
        let temp_root = std::env::temp_dir().join(format!(
            "talon-cli-knowledge-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
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
}
