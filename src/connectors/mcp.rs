// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::gateway::rpc::manifests;
use anyhow::{anyhow, Result};
use axum::http::{HeaderName, HeaderValue};
use base64::{engine::general_purpose, Engine as _};
use futures::{stream::BoxStream, StreamExt};
use jsonwebtoken::{encode, EncodingKey, Header};
use reqwest::header::ACCEPT;
use rmcp::{
    model::{CallToolRequestParams, Content, ResourceContents},
    model::{ClientJsonRpcMessage, ServerJsonRpcMessage},
    service::{RoleClient, RunningService, ServiceExt},
    transport::{
        streamable_http_client::{
            StreamableHttpClient, StreamableHttpClientTransportConfig, StreamableHttpError,
            StreamableHttpPostResponse,
        },
        ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sse_stream::{Error as SseError, Sse, SseStream};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

enum MockResponse {
    Tools(Vec<McpTool>),
    CallResult(String),
    Error(String),
}

enum McpBackend {
    Rmcp(RunningService<RoleClient, ()>),
    Mock(Mutex<Vec<MockResponse>>),
}

pub struct McpClient {
    backend: McpBackend,
}

const MCP_EVENT_STREAM_MIME_TYPE: &str = "text/event-stream";
const MCP_JSON_MIME_TYPE: &str = "application/json";
const MCP_POST_ACCEPT_HEADER: &str = "text/event-stream, application/json";
const MCP_HEADER_LAST_EVENT_ID: &str = "Last-Event-ID";
const MCP_HEADER_SESSION_ID: &str = "Mcp-Session-Id";
const MCP_TOOL_RESULT_MAX_CHARS: usize = 30_000;

#[derive(Debug, Clone)]
pub struct McpConnectionConfig {
    pub server_name: String,
    pub server_ref: String,
    pub transport: String,
    pub target: String,
    pub args: Vec<String>,
    pub headers: HashMap<String, String>,
    pub disabled: bool,
    pub namespace: Option<String>,
    pub binding_name: Option<String>,
    pub agent_name: Option<String>,
    pub auth_broker: Option<McpAuthBrokerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpAuthBrokerConfig {
    pub kind: String,
    pub url: String,
    pub cache_ttl_seconds: i32,
    pub audience: String,
}

impl McpClient {
    pub async fn connect(command: &str, args: &[&str]) -> Result<Self> {
        let transport = TokioChildProcess::new(Command::new(command).configure(|cmd| {
            cmd.args(args);
        }))?;
        let service = ().serve(transport).await?;

        Ok(Self {
            backend: McpBackend::Rmcp(service),
        })
    }

    pub async fn connect_http(target: &str, headers: &HashMap<String, String>) -> Result<Self> {
        validate_http_headers(headers)?;
        let mut transport_config =
            StreamableHttpClientTransportConfig::with_uri(target.to_string());

        let auth_header = authorization_bearer_token(headers)?;
        if let Some(auth_header) = auth_header.clone() {
            transport_config = transport_config.auth_header(auth_header);
        }

        let transport = StreamableHttpClientTransport::with_client(
            AuthenticatedReqwestClient::new(reqwest::Client::default(), auth_header),
            transport_config,
        );
        let service = ().serve(transport).await?;

        Ok(Self {
            backend: McpBackend::Rmcp(service),
        })
    }

    pub fn new_mock(data: Value) -> Self {
        let response = parse_mock_response(data);
        Self {
            backend: McpBackend::Mock(Mutex::new(vec![response])),
        }
    }

    pub async fn list_tools(&self) -> Result<Vec<McpTool>> {
        match &self.backend {
            McpBackend::Rmcp(service) => {
                let tools = service.peer().list_all_tools().await?;
                tools.into_iter().map(convert_rmcp_tool).collect()
            }
            McpBackend::Mock(responses) => match responses.lock().await.pop() {
                Some(MockResponse::Tools(tools)) => Ok(tools),
                Some(MockResponse::Error(err)) => Err(anyhow!("MCP error: {}", err)),
                Some(MockResponse::CallResult(_)) => {
                    Err(anyhow!("Invalid mock response: expected tools/list result"))
                }
                None => Err(anyhow!("Mock MCP error: no more responses")),
            },
        }
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<String> {
        match &self.backend {
            McpBackend::Rmcp(service) => {
                let arguments = match arguments {
                    Value::Null => None,
                    other => Some(serde_json::from_value::<Map<String, Value>>(other)?),
                };

                let result = service
                    .peer()
                    .call_tool(
                        CallToolRequestParams::new(name.to_string())
                            .with_arguments(arguments.unwrap_or_default()),
                    )
                    .await?;

                format_tool_result(
                    &result.content,
                    result.structured_content,
                    serde_json::to_value(&result.content)?,
                )
            }
            McpBackend::Mock(responses) => match responses.lock().await.pop() {
                Some(MockResponse::CallResult(result)) => Ok(result),
                Some(MockResponse::Error(err)) => Err(anyhow!("MCP error: {}", err)),
                Some(MockResponse::Tools(_)) => {
                    Err(anyhow!("Invalid mock response: expected tools/call result"))
                }
                None => Err(anyhow!("Mock MCP error: no more responses")),
            },
        }
    }
}

pub(crate) fn format_tool_result(
    content_blocks: &[Content],
    structured_content: Option<Value>,
    fallback_content: Value,
) -> Result<String> {
    let mut composite_output = String::new();

    for block in content_blocks {
        if let Some(text) = block.raw.as_text() {
            composite_output.push_str(&text.text);
            composite_output.push_str("\n\n");
            continue;
        }

        if let Some(resource) = block.raw.as_resource() {
            let (uri, body) = match &resource.resource {
                ResourceContents::TextResourceContents { uri, text, .. } => {
                    (uri.as_str(), text.clone())
                }
                ResourceContents::BlobResourceContents { uri, blob, .. } => {
                    let decoded = general_purpose::STANDARD
                        .decode(blob)
                        .ok()
                        .and_then(|bytes| String::from_utf8(bytes).ok())
                        .unwrap_or_else(|| blob.clone());
                    (uri.as_str(), decoded)
                }
            };
            composite_output.push_str(&format!(
                "<resource uri=\"{}\">\n{}\n</resource>\n\n",
                uri, body
            ));
            continue;
        }

        composite_output.push_str(&serde_json::to_string_pretty(block)?);
        composite_output.push_str("\n\n");
    }

    if let Some(structured) = structured_content {
        composite_output.push_str("```json\n");
        composite_output.push_str(&serde_json::to_string_pretty(&structured)?);
        composite_output.push_str("\n```\n\n");
    }

    if composite_output.is_empty() {
        composite_output = serde_json::to_string_pretty(&fallback_content)?;
    }

    if composite_output.len() > MCP_TOOL_RESULT_MAX_CHARS {
        let mut end = MCP_TOOL_RESULT_MAX_CHARS;
        while end > 0 && !composite_output.is_char_boundary(end) {
            end -= 1;
        }
        composite_output.truncate(end);
        composite_output.push_str("\n\n...[CONTENT TRUNCATED DUE TO LENGTH LIMIT]");
    }

    Ok(composite_output)
}

#[derive(Clone)]
pub(crate) struct AuthenticatedReqwestClient {
    inner: reqwest::Client,
    default_bearer_token: Option<String>,
}

impl AuthenticatedReqwestClient {
    pub(crate) fn new(inner: reqwest::Client, default_bearer_token: Option<String>) -> Self {
        Self {
            inner,
            default_bearer_token,
        }
    }

    fn bearer_token(&self, auth_token: Option<String>) -> Option<String> {
        auth_token.or_else(|| self.default_bearer_token.clone())
    }

    fn apply_custom_headers(
        mut request_builder: reqwest::RequestBuilder,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> reqwest::RequestBuilder {
        for (name, value) in custom_headers {
            let name = reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes())
                .expect("header names from http::HeaderName are valid");
            let value = reqwest::header::HeaderValue::from_bytes(value.as_bytes())
                .expect("header values from http::HeaderValue are valid");
            request_builder = request_builder.header(name, value);
        }
        request_builder
    }
}

pub(crate) fn content_type_matches(value: &reqwest::header::HeaderValue, expected: &str) -> bool {
    value
        .to_str()
        .ok()
        .and_then(|content_type| content_type.split(';').next())
        .map(|media_type| media_type.trim().eq_ignore_ascii_case(expected))
        .unwrap_or(false)
}

impl StreamableHttpClient for AuthenticatedReqwestClient {
    type Error = reqwest::Error;

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        last_event_id: Option<String>,
        auth_token: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, StreamableHttpError<Self::Error>> {
        let mut request_builder = self
            .inner
            .get(uri.as_ref())
            .header(ACCEPT, MCP_EVENT_STREAM_MIME_TYPE)
            .header(MCP_HEADER_SESSION_ID, session_id.as_ref());
        request_builder = Self::apply_custom_headers(request_builder, custom_headers);
        if let Some(last_event_id) = last_event_id {
            request_builder = request_builder.header(MCP_HEADER_LAST_EVENT_ID, last_event_id);
        }
        if let Some(auth_header) = self.bearer_token(auth_token) {
            request_builder = request_builder.bearer_auth(auth_header);
        }

        let response = request_builder
            .send()
            .await
            .map_err(StreamableHttpError::Client)?;
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            return Err(StreamableHttpError::ServerDoesNotSupportSse);
        }
        let response = response
            .error_for_status()
            .map_err(StreamableHttpError::Client)?;
        match response.headers().get(reqwest::header::CONTENT_TYPE) {
            Some(ct) if content_type_matches(ct, MCP_EVENT_STREAM_MIME_TYPE) => {}
            Some(ct) => {
                return Err(StreamableHttpError::UnexpectedContentType(Some(
                    String::from_utf8_lossy(ct.as_bytes()).to_string(),
                )))
            }
            None => return Err(StreamableHttpError::UnexpectedContentType(None)),
        }

        Ok(SseStream::from_byte_stream(response.bytes_stream()).boxed())
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session: Arc<str>,
        auth_token: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        let mut request_builder = self.inner.delete(uri.as_ref());
        request_builder = Self::apply_custom_headers(request_builder, custom_headers);
        if let Some(auth_header) = self.bearer_token(auth_token) {
            request_builder = request_builder.bearer_auth(auth_header);
        }

        let response = request_builder
            .header(MCP_HEADER_SESSION_ID, session.as_ref())
            .send()
            .await
            .map_err(StreamableHttpError::Client)?;

        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            tracing::debug!("this server doesn't support deleting session");
            return Ok(());
        }

        response
            .error_for_status()
            .map_err(StreamableHttpError::Client)?;
        Ok(())
    }

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_token: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        let mut request = self
            .inner
            .post(uri.as_ref())
            .header(ACCEPT, MCP_POST_ACCEPT_HEADER);
        request = Self::apply_custom_headers(request, custom_headers);
        if let Some(auth_header) = self.bearer_token(auth_token) {
            request = request.bearer_auth(auth_header);
        }
        if let Some(session_id) = session_id {
            request = request.header(MCP_HEADER_SESSION_ID, session_id.as_ref());
        }

        let response = request
            .json(&message)
            .send()
            .await
            .map_err(StreamableHttpError::Client)?
            .error_for_status()
            .map_err(StreamableHttpError::Client)?;
        if response.status() == reqwest::StatusCode::ACCEPTED {
            return Ok(StreamableHttpPostResponse::Accepted);
        }

        let content_type = response.headers().get(reqwest::header::CONTENT_TYPE);
        let session_id = response
            .headers()
            .get(MCP_HEADER_SESSION_ID)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string());

        match content_type {
            Some(ct) if content_type_matches(ct, MCP_EVENT_STREAM_MIME_TYPE) => {
                Ok(StreamableHttpPostResponse::Sse(
                    SseStream::from_byte_stream(response.bytes_stream()).boxed(),
                    session_id,
                ))
            }
            Some(ct) if content_type_matches(ct, MCP_JSON_MIME_TYPE) => {
                let message: ServerJsonRpcMessage =
                    response.json().await.map_err(StreamableHttpError::Client)?;
                Ok(StreamableHttpPostResponse::Json(message, session_id))
            }
            _ => Err(StreamableHttpError::UnexpectedContentType(
                content_type.map(|ct| String::from_utf8_lossy(ct.as_bytes()).to_string()),
            )),
        }
    }
}

impl TryFrom<&manifests::McpServer> for McpConnectionConfig {
    type Error = anyhow::Error;

    fn try_from(server: &manifests::McpServer) -> Result<Self> {
        let meta = server
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("MCPServer missing metadata"))?;
        let spec = server
            .spec
            .as_ref()
            .ok_or_else(|| anyhow!("MCPServer missing spec"))?;
        Ok(Self {
            server_name: meta.name.clone(),
            server_ref: meta.name.clone(),
            transport: spec.transport.clone(),
            target: spec.target.clone(),
            args: spec.args.clone(),
            headers: spec.headers.clone(),
            disabled: spec.disabled,
            namespace: None,
            binding_name: None,
            agent_name: None,
            auth_broker: None,
        })
    }
}

async fn connect_configured(config: &McpConnectionConfig) -> Result<McpClient> {
    if config.disabled {
        return Err(anyhow!("MCP server '{}' is disabled", config.server_name));
    }

    match config.transport.as_str() {
        "stdio" => {
            let args = config.args.iter().map(String::as_str).collect::<Vec<_>>();
            McpClient::connect(&config.target, &args).await
        }
        "http" => {
            let headers = resolve_http_headers(config).await?;
            McpClient::connect_http(&config.target, &headers).await
        }
        other => Err(anyhow!(
            "Unsupported MCP transport '{}' for server '{}'",
            other,
            config.server_name
        )),
    }
}

pub async fn list_tools_for_config(config: &McpConnectionConfig) -> Result<Vec<McpTool>> {
    let client = connect_configured(config).await?;
    client.list_tools().await
}

pub async fn call_tool_for_config(
    config: &McpConnectionConfig,
    name: &str,
    arguments: Value,
) -> Result<String> {
    let client = connect_configured(config).await?;
    client.call_tool(name, arguments).await
}

pub async fn invalidate_broker_auth_cache(ns: &str, binding_name: Option<&str>) {
    if let Some(binding_name) = binding_name {
        {
            let mut cache = auth_cache().write().await;
            cache.retain(|key, _| !(key.namespace == ns && key.binding_name == binding_name));
        }
        auth_fetch_locks()
            .write()
            .await
            .retain(|key, _| !(key.namespace == ns && key.binding_name == binding_name));
        return;
    }

    {
        let mut cache = auth_cache().write().await;
        cache.retain(|key, _| key.namespace != ns);
    }
    auth_fetch_locks()
        .write()
        .await
        .retain(|key, _| key.namespace != ns);
}

pub async fn invalidate_all_broker_auth_cache() {
    auth_cache().write().await.clear();
    auth_fetch_locks().write().await.clear();
}

fn convert_rmcp_tool(tool: rmcp::model::Tool) -> Result<McpTool> {
    Ok(McpTool {
        name: tool.name.to_string(),
        description: tool
            .description
            .map(|desc| desc.to_string())
            .unwrap_or_default(),
        input_schema: serde_json::to_value(tool.input_schema.as_ref())?,
    })
}

fn parse_mock_response(data: Value) -> MockResponse {
    if let Some(error) = data.get("error") {
        return MockResponse::Error(
            error["message"]
                .as_str()
                .unwrap_or("Unknown MCP error")
                .to_string(),
        );
    }

    if let Some(tools) = data
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
    {
        let tools = tools
            .iter()
            .map(|tool| McpTool {
                name: tool["name"].as_str().unwrap_or_default().to_string(),
                description: tool["description"].as_str().unwrap_or_default().to_string(),
                input_schema: tool["inputSchema"].clone(),
            })
            .collect();
        return MockResponse::Tools(tools);
    }

    if let Some(text) = data
        .get("result")
        .and_then(|result| result.get("content"))
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
    {
        return MockResponse::CallResult(text.to_string());
    }

    MockResponse::Error("Unsupported mock MCP response".to_string())
}

pub(crate) fn authorization_header(headers: &HashMap<String, String>) -> Option<String> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.clone())
}

pub(crate) fn authorization_bearer_token(
    headers: &HashMap<String, String>,
) -> Result<Option<String>> {
    let Some(value) = authorization_header(headers) else {
        return Ok(None);
    };

    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("Authorization header cannot be empty"));
    }

    if let Some((scheme, token)) = trimmed.split_once(' ') {
        if !scheme.eq_ignore_ascii_case("bearer") {
            return Err(anyhow!(
                "Unsupported Authorization scheme '{}' for MCP transport; only Bearer is supported",
                scheme
            ));
        }

        let token = token.trim();
        if token.is_empty() {
            return Err(anyhow!("Authorization Bearer token cannot be empty"));
        }
        return Ok(Some(token.to_string()));
    }

    Ok(Some(trimmed.to_string()))
}

pub(crate) fn validate_http_headers(headers: &HashMap<String, String>) -> Result<()> {
    let unsupported = headers
        .keys()
        .find(|name| !name.eq_ignore_ascii_case("authorization"));
    if let Some(name) = unsupported {
        return Err(anyhow!(
            "Unsupported HTTP header '{}' for MCP transport; only Authorization is currently supported",
            name
        ));
    }

    Ok(())
}

pub(crate) async fn resolve_http_headers(
    config: &McpConnectionConfig,
) -> Result<HashMap<String, String>> {
    validate_http_headers(&config.headers)?;

    let mut headers = config.headers.clone();
    if let Some(auth_broker) = &config.auth_broker {
        if authorization_header(&headers).is_some() {
            return Err(anyhow!(
                "MCP server '{}' cannot use both static Authorization headers and auth_broker",
                config.server_name
            ));
        }

        let token = resolve_broker_bearer_token(config, auth_broker).await?;
        headers.insert("Authorization".to_string(), format!("Bearer {}", token));
    }

    Ok(headers)
}

async fn resolve_broker_bearer_token(
    config: &McpConnectionConfig,
    auth_broker: &McpAuthBrokerConfig,
) -> Result<String> {
    let kind = auth_broker.kind.trim();
    if !kind.is_empty() && kind != "http_bearer" {
        return Err(anyhow!(
            "Unsupported MCP auth broker kind '{}' for server '{}'",
            kind,
            config.server_name
        ));
    }

    let namespace = config
        .namespace
        .as_ref()
        .ok_or_else(|| anyhow!("MCP auth broker requires config namespace"))?;
    let binding_name = config
        .binding_name
        .as_ref()
        .ok_or_else(|| anyhow!("MCP auth broker requires binding name"))?;
    let agent_name = config.agent_name.clone();
    let key = AuthCacheKey {
        namespace: namespace.clone(),
        binding_name: binding_name.clone(),
        agent_name: agent_name.clone(),
    };

    if let Some(cached) = get_cached_auth_entry(&key).await {
        if !cached.is_expired() {
            return Ok(cached.token);
        }
    }

    let fetch_lock = auth_fetch_lock(&key).await;
    let _fetch_guard = fetch_lock.lock().await;
    if let Some(cached) = get_cached_auth_entry(&key).await {
        if !cached.is_expired() {
            return Ok(cached.token);
        }
    }

    let fetch_result: Result<String> = async {
        let request = AuthBrokerRequest {
            namespace: namespace.clone(),
            binding_name: binding_name.clone(),
            server_ref: config.server_ref.clone(),
            agent_name: agent_name.clone(),
            audience: empty_to_none(auth_broker.audience.trim()),
        };
        let token = mint_auth_broker_jwt(namespace, binding_name, agent_name.as_deref())?;
        let mut response = auth_broker_client()
            .post(auth_broker.url.trim())
            .bearer_auth(token)
            .json(&request)
            .send()
            .await
            .map_err(|err| {
                anyhow!(
                    "MCP auth broker request failed for binding '{}'/{}: {}",
                    namespace,
                    binding_name,
                    err
                )
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = read_response_body_limited(&mut response, 8 << 10).await;
            return Err(anyhow!(
                "MCP auth broker returned {} for binding '{}'/{}: {}",
                status,
                namespace,
                binding_name,
                body.trim()
            ));
        }

        let payload: AuthBrokerResponse = response.json().await.map_err(|err| {
            anyhow!(
                "Failed to decode MCP auth broker response for binding '{}'/{}: {}",
                namespace,
                binding_name,
                err
            )
        })?;

        let bearer = payload.authorization_bearer_token.trim();
        if bearer.is_empty() {
            return Err(anyhow!(
                "MCP auth broker returned an empty bearer token for binding '{}'/{}'",
                namespace,
                binding_name
            ));
        }

        let expires_at_unix =
            resolve_auth_expiry(payload.expires_at_unix, auth_broker.cache_ttl_seconds)?;
        let mut cache = auth_cache().write().await;
        prune_expired_auth_cache(&mut cache);
        cache.insert(
            key.clone(),
            CachedAuthEntry {
                token: bearer.to_string(),
                expires_at_unix,
            },
        );

        Ok(bearer.to_string())
    }
    .await;

    if fetch_result.is_err() {
        auth_fetch_locks().write().await.remove(&key);
    }

    fetch_result
}

async fn read_response_body_limited(response: &mut reqwest::Response, max_bytes: usize) -> String {
    let mut body = Vec::with_capacity(max_bytes.min(1024));
    let mut truncated = false;

    while let Ok(Some(chunk)) = response.chunk().await {
        let remaining = max_bytes.saturating_sub(body.len());
        if remaining == 0 {
            truncated = true;
            break;
        }

        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }

        body.extend_from_slice(&chunk);
    }

    let mut text = String::from_utf8_lossy(&body).trim().to_string();
    if truncated {
        text.push_str("...(truncated)");
    }
    text
}

fn resolve_auth_expiry(expires_at_unix: Option<i64>, cache_ttl_seconds: i32) -> Result<i64> {
    if let Some(expires_at_unix) = expires_at_unix {
        if expires_at_unix <= current_unix_timestamp() {
            return Err(anyhow!("MCP auth broker returned an already expired token"));
        }
        return Ok(expires_at_unix);
    }

    if cache_ttl_seconds > 0 {
        return Ok(current_unix_timestamp() + i64::from(cache_ttl_seconds));
    }

    Err(anyhow!(
        "MCP auth broker response omitted expires_at_unix and binding provided no cache_ttl_seconds fallback"
    ))
}

fn mint_auth_broker_jwt(
    namespace: &str,
    binding_name: &str,
    agent_name: Option<&str>,
) -> Result<String> {
    crate::security::install_jwt_crypto_provider();
    let secret = broker_jwt_secret()?;
    let now = current_unix_timestamp();
    let claims = BrokerJwtClaims {
        sub: "talon-mcp-client".to_string(),
        aud: "conic-mcp-auth-broker".to_string(),
        exp: now + 300,
        iat: now,
        talon_ns: namespace.to_string(),
        talon_binding: binding_name.to_string(),
        talon_agent: agent_name.map(str::to_string),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|err| anyhow!("failed to mint MCP auth broker JWT: {}", err))
}

fn broker_jwt_secret() -> Result<String> {
    std::env::var("TALON_JWT_SECRET")
        .or_else(|_| std::env::var("GATEWAY_JWT_SECRET"))
        .map_err(|_| {
            anyhow!("TALON_JWT_SECRET or GATEWAY_JWT_SECRET must be set for MCP auth broker")
        })
}

fn current_unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs() as i64
}

fn auth_broker_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("failed to build MCP auth broker HTTP client")
    })
}

fn auth_cache() -> &'static RwLock<HashMap<AuthCacheKey, CachedAuthEntry>> {
    static CACHE: OnceLock<RwLock<HashMap<AuthCacheKey, CachedAuthEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn auth_fetch_locks() -> &'static RwLock<HashMap<AuthCacheKey, Arc<Mutex<()>>>> {
    static LOCKS: OnceLock<RwLock<HashMap<AuthCacheKey, Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| RwLock::new(HashMap::new()))
}

async fn auth_fetch_lock(key: &AuthCacheKey) -> Arc<Mutex<()>> {
    if let Some(lock) = auth_fetch_locks().read().await.get(key).cloned() {
        return lock;
    }

    let mut locks = auth_fetch_locks().write().await;
    locks
        .entry(key.clone())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

async fn get_cached_auth_entry(key: &AuthCacheKey) -> Option<CachedAuthEntry> {
    prune_expired_auth_state().await;
    auth_cache().read().await.get(key).cloned()
}

fn prune_expired_auth_cache(cache: &mut HashMap<AuthCacheKey, CachedAuthEntry>) {
    cache.retain(|_, entry| !entry.is_expired());
}

async fn prune_expired_auth_state() {
    let expired_keys = {
        let cache = auth_cache().read().await;
        cache
            .iter()
            .filter_map(|(key, entry)| {
                if entry.is_expired() {
                    Some(key.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };

    if expired_keys.is_empty() {
        return;
    }

    {
        let mut cache = auth_cache().write().await;
        for key in &expired_keys {
            if cache.get(key).is_some_and(CachedAuthEntry::is_expired) {
                cache.remove(key);
            }
        }
    }

    let mut locks = auth_fetch_locks().write().await;
    for key in expired_keys {
        locks.remove(&key);
    }
}

fn empty_to_none(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
pub(crate) fn clear_broker_auth_cache_for_test() {
    auth_cache()
        .try_write()
        .expect("broker auth cache should not be locked during test cleanup")
        .clear();
    auth_fetch_locks()
        .try_write()
        .expect("broker auth lock map should not be locked during test cleanup")
        .clear();
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct AuthCacheKey {
    namespace: String,
    binding_name: String,
    agent_name: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedAuthEntry {
    token: String,
    expires_at_unix: i64,
}

impl CachedAuthEntry {
    fn is_expired(&self) -> bool {
        self.expires_at_unix <= current_unix_timestamp() + 60
    }
}

#[derive(Debug, Serialize)]
struct AuthBrokerRequest {
    namespace: String,
    binding_name: String,
    server_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audience: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthBrokerResponse {
    authorization_bearer_token: String,
    expires_at_unix: Option<i64>,
    #[allow(dead_code)]
    issued_at_unix: Option<i64>,
    #[allow(dead_code)]
    debug_reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct BrokerJwtClaims {
    sub: String,
    aud: String,
    exp: i64,
    iat: i64,
    #[serde(rename = "talon:ns")]
    talon_ns: String,
    #[serde(rename = "talon:binding")]
    talon_binding: String,
    #[serde(rename = "talon:agent", skip_serializing_if = "Option::is_none")]
    talon_agent: Option<String>,
}
