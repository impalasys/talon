// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[cfg(test)]
mod tests {
    use crate::gateway::rpc::manifests;
    use crate::harness::mcp::{
        authorization_bearer_token, authorization_header, call_tool_for_config,
        clear_broker_auth_cache_for_test, content_type_matches, format_tool_result,
        invalidate_all_broker_auth_cache, invalidate_broker_auth_cache, list_tools_for_config,
        resolve_http_headers, validate_http_headers, AuthenticatedReqwestClient,
        McpAuthBrokerConfig, McpClient, McpConnectionConfig,
    };
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::{delete, get, post},
        Json, Router,
    };
    use futures::StreamExt;
    use rmcp::model::{
        ClientJsonRpcMessage, ClientRequest, Content, PingRequest, RequestId, ResourceContents,
    };
    use rmcp::transport::streamable_http_client::{
        StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio::sync::Barrier;

    const TEST_PLATFORM_ISSUER: &str = "https://talon.example.com";

    #[test]
    fn test_content_type_matches_ignores_case_and_parameters() {
        let event_stream =
            reqwest::header::HeaderValue::from_static("Text/Event-Stream; charset=utf-8");
        let json = reqwest::header::HeaderValue::from_static("Application/Json");
        let other = reqwest::header::HeaderValue::from_static("text/plain");

        assert!(content_type_matches(&event_stream, "text/event-stream"));
        assert!(content_type_matches(&json, "application/json"));
        assert!(!content_type_matches(&other, "application/json"));
    }

    #[derive(Clone)]
    struct HttpAuthState {
        get_hits: Arc<AtomicUsize>,
        delete_hits: Arc<AtomicUsize>,
    }

    fn ping_message() -> ClientJsonRpcMessage {
        ClientJsonRpcMessage::request(
            ClientRequest::PingRequest(PingRequest::default()),
            RequestId::Number(1),
        )
    }

    #[tokio::test]
    async fn test_mcp_client_tool_discovery() {
        let mock_response = json!({
            "jsonrpc": "2.0",
            "result": {
                "tools": [
                    {
                        "name": "read_file",
                        "description": "Reads a file from disk",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" }
                            }
                        }
                    }
                ]
            },
            "id": 1
        });

        let client = McpClient::new_mock(mock_response);
        let tools = client.list_tools().await.unwrap();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "read_file");
    }

    #[tokio::test]
    async fn test_mcp_client_tool_invocation() {
        let mock_result = json!({
            "jsonrpc": "2.0",
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": "File content here"
                    }
                ]
            },
            "id": 2
        });

        let client = McpClient::new_mock(mock_result);
        let result = client
            .call_tool("read_file", json!({"path": "test.txt"}))
            .await
            .unwrap();

        assert!(result.contains("File content here"));
    }

    #[tokio::test]
    async fn test_mock_client_call_tool() {
        let mock_response = json!({
            "jsonrpc": "2.0",
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": "Success"
                    }
                ]
            },
            "id": 1
        });

        let client = McpClient::new_mock(mock_response);
        let result = client.call_tool("test_tool", json!({})).await.unwrap();

        assert_eq!(result, "Success");
    }

    #[tokio::test]
    async fn test_mock_client_invalid_response_and_exhaustion_paths() {
        let list_client = McpClient::new_mock(json!({
            "jsonrpc": "2.0",
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": "not-a-tool-list"
                    }
                ]
            },
            "id": 1
        }));
        let err = list_client.list_tools().await.unwrap_err().to_string();
        assert!(err.contains("expected tools/list result"));
        let err = list_client.list_tools().await.unwrap_err().to_string();
        assert!(err.contains("no more responses"));

        let call_client = McpClient::new_mock(json!({
            "jsonrpc": "2.0",
            "result": {
                "tools": [
                    {
                        "name": "read_file",
                        "description": "Reads a file",
                        "inputSchema": {"type": "object"}
                    }
                ]
            },
            "id": 2
        }));
        let err = call_client
            .call_tool("read_file", json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("expected tools/call result"));
        let err = call_client
            .call_tool("read_file", json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("no more responses"));
    }

    #[test]
    fn test_format_tool_result_includes_structured_content_and_summary_text() {
        let result = format_tool_result(
            &[],
            Some(json!({
                "content": "export const blogs = true;",
                "path": "source.config.ts"
            })),
            json!([]),
        )
        .unwrap();

        assert!(result.contains("```json"));
        assert!(result.contains("\"content\": \"export const blogs = true;\""));
    }

    #[test]
    fn test_format_tool_result_uses_text_when_structured_content_is_missing() {
        let result = format_tool_result(
            &[
                Content::text("File content here"),
                Content::text("More text"),
            ],
            None,
            json!([]),
        )
        .unwrap();

        assert_eq!(result, "File content here\n\nMore text\n\n");
    }

    #[test]
    fn test_mcp_composite_github_shape() {
        let result = format_tool_result(
            &[
                Content::text("Summary: SHA 123"),
                Content::resource(ResourceContents::text("Hello World", "file.txt")),
            ],
            None,
            json!([]),
        )
        .unwrap();

        assert!(result.contains("Summary: SHA 123"));
        assert!(result.contains("<resource uri=\"file.txt\">\nHello World\n</resource>"));
    }

    #[test]
    fn test_format_tool_result_includes_summary_text_and_embedded_resource() {
        let result = format_tool_result(
            &[
                Content::text("successfully downloaded text file (SHA: deadbeef)"),
                Content::resource(ResourceContents::text(
                    "export const blogs = true;",
                    "file:///source.config.ts",
                )),
            ],
            None,
            json!([]),
        )
        .unwrap();

        assert!(result.contains("successfully downloaded text file (SHA: deadbeef)"));
        assert!(result.contains(
            "<resource uri=\"file:///source.config.ts\">\nexport const blogs = true;\n</resource>"
        ));
    }

    #[test]
    fn test_format_tool_result_truncates_large_composite_output() {
        let large_text = "a".repeat(31_000);
        let result = format_tool_result(&[Content::text(large_text)], None, json!([])).unwrap();

        assert!(result.contains("...[CONTENT TRUNCATED DUE TO LENGTH LIMIT]"));
        assert!(result.len() <= 30_000 + "\n\n...[CONTENT TRUNCATED DUE TO LENGTH LIMIT]".len());
    }

    #[test]
    fn test_format_tool_result_decodes_blob_and_serializes_non_text_blocks() {
        let result = format_tool_result(
            &[
                Content::resource(ResourceContents::blob(
                    "SGVsbG8gYmxvYg==",
                    "file:///blob.txt",
                )),
                Content::image("ZmFrZQ==", "image/png"),
            ],
            None,
            json!([]),
        )
        .unwrap();

        assert!(result.contains("<resource uri=\"file:///blob.txt\">\nHello blob\n</resource>"));
        assert!(result.contains("\"type\": \"image\""));
        assert!(result.contains("\"mimeType\": \"image/png\""));
    }

    #[test]
    fn test_format_tool_result_falls_back_to_pretty_printed_json() {
        let result =
            format_tool_result(&[], None, json!({"ok": true, "items": [1, 2, 3]})).unwrap();
        assert!(result.contains("\"ok\": true"));
        assert!(result.contains("\"items\": ["));
    }

    #[tokio::test]
    async fn test_mock_client_error_handling() {
        let error_response = json!({
            "jsonrpc": "2.0",
            "error": {
                "code": -32601,
                "message": "Method not found"
            },
            "id": 1
        });

        let client = McpClient::new_mock(error_response);
        let result = client.list_tools().await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Method not found"));
    }

    #[tokio::test]
    async fn test_mcp_client_connect_factory() {
        let result = McpClient::connect("definitely-not-a-real-command", &[]).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_http_headers_allows_authorization_only() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer token".to_string());

        assert!(validate_http_headers(&headers).is_ok());
    }

    #[test]
    fn test_validate_http_headers_rejects_non_authorization_headers() {
        let mut headers = HashMap::new();
        headers.insert("x-conic-workspace".to_string(), "demo".to_string());

        let err = validate_http_headers(&headers).unwrap_err().to_string();
        assert!(err.contains("Unsupported HTTP header"));
    }

    #[test]
    fn test_authorization_header_lookup_is_case_insensitive() {
        let mut headers = HashMap::new();
        headers.insert("AUTHORIZATION".to_string(), "Bearer token".to_string());

        assert_eq!(
            authorization_header(&headers).as_deref(),
            Some("Bearer token")
        );
    }

    #[test]
    fn test_authorization_bearer_token_strips_bearer_prefix() {
        let mut headers = HashMap::new();
        headers.insert(
            "Authorization".to_string(),
            "Bearer secret-token".to_string(),
        );

        assert_eq!(
            authorization_bearer_token(&headers).unwrap().as_deref(),
            Some("secret-token")
        );
    }

    #[test]
    fn test_authorization_bearer_token_allows_bare_token() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "secret-token".to_string());

        assert_eq!(
            authorization_bearer_token(&headers).unwrap().as_deref(),
            Some("secret-token")
        );
    }

    #[test]
    fn test_authorization_bearer_token_rejects_other_schemes() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Basic abc123".to_string());

        let err = authorization_bearer_token(&headers)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Unsupported Authorization scheme"));
    }

    #[test]
    fn test_authorization_bearer_token_rejects_empty_values() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "   ".to_string());
        assert!(authorization_bearer_token(&headers)
            .unwrap_err()
            .to_string()
            .contains("cannot be empty"));
    }

    #[test]
    fn test_authorization_bearer_token_handles_missing_and_empty_bearer_tokens() {
        let headers = HashMap::new();
        assert_eq!(authorization_bearer_token(&headers).unwrap(), None);
    }

    #[test]
    fn test_connection_config_try_from_manifest_reads_fields_and_rejects_missing_parts() {
        let server = manifests::McpServer {
            metadata: Some(manifests::ObjectMeta {
                name: "github".to_string(),
                namespace: String::new(),
                labels: HashMap::new(),
                annotations: HashMap::new(),
                ..Default::default()
            }),
            spec: Some(manifests::McpServerSpec {
                transport: "http".to_string(),
                target: "https://example.com/mcp".to_string(),
                args: vec!["--flag".to_string()],
                headers: HashMap::from([("Authorization".to_string(), "Bearer token".to_string())]),
                disabled: true,
                auth_broker: None,
                policy: None,
            }),
            status: Some(crate::control::resource_model::common_status(String::new())),
        };

        let config = McpConnectionConfig::try_from(&server).unwrap();
        assert_eq!(config.server_name, "github");
        assert_eq!(config.server_ref, "github");
        assert_eq!(config.transport, "http");
        assert_eq!(config.target, "https://example.com/mcp");
        assert_eq!(config.args, vec!["--flag"]);
        assert!(config.disabled);

        let missing_meta = manifests::McpServer {
            metadata: None,
            ..server.clone()
        };
        assert!(McpConnectionConfig::try_from(&missing_meta)
            .unwrap_err()
            .to_string()
            .contains("missing metadata"));

        let missing_spec = manifests::McpServer {
            spec: None,
            ..server
        };
        assert!(McpConnectionConfig::try_from(&missing_spec)
            .unwrap_err()
            .to_string()
            .contains("missing spec"));
    }

    #[tokio::test]
    async fn test_list_tools_for_config_rejects_disabled_and_unsupported_transports() {
        let disabled = McpConnectionConfig {
            server_name: "github".to_string(),
            server_ref: "github".to_string(),
            transport: "http".to_string(),
            target: "https://example.com/mcp".to_string(),
            args: Vec::new(),
            headers: HashMap::new(),
            disabled: true,
            namespace: None,
            mcp_server_name: None,
            agent_name: None,
            jwt_issuer: None,
            auth_broker: None,
        };
        assert!(list_tools_for_config(&disabled)
            .await
            .unwrap_err()
            .to_string()
            .contains("is disabled"));

        let unsupported = McpConnectionConfig {
            disabled: false,
            transport: "gopher".to_string(),
            ..disabled.clone()
        };
        assert!(list_tools_for_config(&unsupported)
            .await
            .unwrap_err()
            .to_string()
            .contains("Unsupported MCP transport"));

        assert!(call_tool_for_config(&disabled, "read_file", json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("is disabled"));
        assert!(call_tool_for_config(&unsupported, "read_file", json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("Unsupported MCP transport"));
    }

    #[tokio::test]
    async fn test_resolve_http_headers_uses_auth_broker_and_caches() {
        let _test_guard = BrokerAuthTestGuard::acquire().await;

        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route(
                "/broker",
                post(
                    move |State(hits): State<Arc<AtomicUsize>>,
                          headers: HeaderMap,
                          Json(payload): Json<serde_json::Value>| async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        assert!(headers.get("authorization").is_some());
                        assert_eq!(payload["namespace"], "conic:wks:42");
                        assert_eq!(payload["mcp_server_name"], "github");
                        assert_eq!(payload["agent_name"], "cmo");
                        Json(json!({
                            "authorization_bearer_token": "ghs_brokered_token",
                            "expires_at_unix": 4_102_444_800i64
                        }))
                    },
                ),
            )
            .with_state(hits.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let config = McpConnectionConfig {
            server_name: "github".to_string(),
            server_ref: "github".to_string(),
            transport: "http".to_string(),
            target: "https://api.githubcopilot.com/mcp/".to_string(),
            args: Vec::new(),
            headers: HashMap::new(),
            disabled: false,
            namespace: Some("conic:wks:42".to_string()),
            mcp_server_name: Some("github".to_string()),
            agent_name: Some("cmo".to_string()),
            jwt_issuer: Some(TEST_PLATFORM_ISSUER.to_string()),
            auth_broker: Some(McpAuthBrokerConfig {
                kind: "http_bearer".to_string(),
                url: format!("http://{}/broker", addr),
                cache_ttl_seconds: 60,
                audience: "github".to_string(),
            }),
        };

        let headers = resolve_http_headers(&config).await.unwrap();
        assert_eq!(
            authorization_header(&headers).as_deref(),
            Some("Bearer ghs_brokered_token")
        );
        let headers = resolve_http_headers(&config).await.unwrap();
        assert_eq!(
            authorization_header(&headers).as_deref(),
            Some("Bearer ghs_brokered_token")
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        server.abort();
    }

    #[tokio::test]
    async fn test_resolve_http_headers_rejects_conflicting_or_incomplete_broker_config() {
        let _test_guard = BrokerAuthTestGuard::acquire().await;

        let with_static_auth = McpConnectionConfig {
            server_name: "github".to_string(),
            server_ref: "github".to_string(),
            transport: "http".to_string(),
            target: "https://api.githubcopilot.com/mcp/".to_string(),
            args: Vec::new(),
            headers: HashMap::from([("Authorization".to_string(), "Bearer static".to_string())]),
            disabled: false,
            namespace: Some("conic:wks:42".to_string()),
            mcp_server_name: Some("github".to_string()),
            agent_name: Some("cmo".to_string()),
            jwt_issuer: Some(TEST_PLATFORM_ISSUER.to_string()),
            auth_broker: Some(McpAuthBrokerConfig {
                kind: "http_bearer".to_string(),
                url: "http://127.0.0.1:9/broker".to_string(),
                cache_ttl_seconds: 60,
                audience: "github".to_string(),
            }),
        };
        assert!(resolve_http_headers(&with_static_auth)
            .await
            .unwrap_err()
            .to_string()
            .contains("cannot use both static Authorization headers and auth_broker"));

        let missing_namespace = McpConnectionConfig {
            headers: HashMap::new(),
            namespace: None,
            ..with_static_auth.clone()
        };
        assert!(resolve_http_headers(&missing_namespace)
            .await
            .unwrap_err()
            .to_string()
            .contains("requires config namespace"));

        let missing_mcp_server_name = McpConnectionConfig {
            headers: HashMap::new(),
            namespace: Some("conic:wks:42".to_string()),
            mcp_server_name: None,
            ..with_static_auth.clone()
        };
        assert!(resolve_http_headers(&missing_mcp_server_name)
            .await
            .unwrap_err()
            .to_string()
            .contains("requires MCP server name"));
    }

    #[tokio::test]
    async fn test_resolve_http_headers_rejects_unsupported_broker_kind_and_missing_private_key() {
        let _test_guard = BrokerAuthTestGuard::acquire().await;

        let unsupported_kind = McpConnectionConfig {
            server_name: "github".to_string(),
            server_ref: "github".to_string(),
            transport: "http".to_string(),
            target: "https://api.githubcopilot.com/mcp/".to_string(),
            args: Vec::new(),
            headers: HashMap::new(),
            disabled: false,
            namespace: Some("conic:wks:42".to_string()),
            mcp_server_name: Some("github".to_string()),
            agent_name: Some("cmo".to_string()),
            jwt_issuer: Some(TEST_PLATFORM_ISSUER.to_string()),
            auth_broker: Some(McpAuthBrokerConfig {
                kind: "custom".to_string(),
                url: "http://127.0.0.1:9/broker".to_string(),
                cache_ttl_seconds: 60,
                audience: "github".to_string(),
            }),
        };
        assert!(resolve_http_headers(&unsupported_kind)
            .await
            .unwrap_err()
            .to_string()
            .contains("Unsupported MCP auth broker kind"));

        std::env::remove_var(crate::control::security::platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV);
        let missing_secret = McpConnectionConfig {
            auth_broker: Some(McpAuthBrokerConfig {
                kind: "http_bearer".to_string(),
                url: "http://127.0.0.1:9/broker".to_string(),
                cache_ttl_seconds: 60,
                audience: "github".to_string(),
            }),
            ..unsupported_kind
        };
        assert!(resolve_http_headers(&missing_secret)
            .await
            .unwrap_err()
            .to_string()
            .contains("TALON_JWT_PRIVATE_KEY_PEM is required"));
    }

    #[tokio::test]
    async fn test_resolve_http_headers_deduplicates_concurrent_broker_fetches() {
        let _test_guard = BrokerAuthTestGuard::acquire().await;

        let hits = Arc::new(AtomicUsize::new(0));
        let start_barrier = Arc::new(Barrier::new(10));
        let app = Router::new()
            .route(
                "/broker",
                post(
                    move |State(hits): State<Arc<AtomicUsize>>,
                          headers: HeaderMap,
                          Json(payload): Json<serde_json::Value>| async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        assert!(headers.get("authorization").is_some());
                        assert_eq!(payload["namespace"], "conic:wks:42");
                        assert_eq!(payload["mcp_server_name"], "github");
                        assert_eq!(payload["agent_name"], "cmo");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        Json(json!({
                            "authorization_bearer_token": "ghs_brokered_token",
                            "expires_at_unix": 4_102_444_800i64
                        }))
                    },
                ),
            )
            .with_state(hits.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let config = Arc::new(McpConnectionConfig {
            server_name: "github".to_string(),
            server_ref: "github".to_string(),
            transport: "http".to_string(),
            target: "https://api.githubcopilot.com/mcp/".to_string(),
            args: Vec::new(),
            headers: HashMap::new(),
            disabled: false,
            namespace: Some("conic:wks:42".to_string()),
            mcp_server_name: Some("github".to_string()),
            agent_name: Some("cmo".to_string()),
            jwt_issuer: Some(TEST_PLATFORM_ISSUER.to_string()),
            auth_broker: Some(McpAuthBrokerConfig {
                kind: "http_bearer".to_string(),
                url: format!("http://{}/broker", addr),
                cache_ttl_seconds: 60,
                audience: "github".to_string(),
            }),
        });

        let mut tasks = Vec::new();
        for _ in 0..10 {
            let config = config.clone();
            let start_barrier = start_barrier.clone();
            tasks.push(tokio::spawn(async move {
                start_barrier.wait().await;
                let headers = resolve_http_headers(&config).await.unwrap();
                assert_eq!(
                    authorization_header(&headers).as_deref(),
                    Some("Bearer ghs_brokered_token")
                );
            }));
        }

        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        server.abort();
    }

    #[tokio::test]
    async fn test_resolve_http_headers_handles_broker_failure_payloads_and_ttl_fallback() {
        let _test_guard = BrokerAuthTestGuard::acquire().await;

        async fn serve(app: Router) -> SocketAddr {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            addr
        }

        let status_addr = serve(Router::new().route(
            "/broker",
            post(|| async {
                (
                    StatusCode::BAD_GATEWAY,
                    "upstream broker refused request".repeat(512),
                )
            }),
        ))
        .await;
        let invalid_json_addr =
            serve(Router::new().route("/broker", post(|| async { (StatusCode::OK, "not-json") })))
                .await;
        let empty_token_addr = serve(Router::new().route(
            "/broker",
            post(|| async {
                Json(json!({
                    "authorization_bearer_token": "   ",
                    "expires_at_unix": 4_102_444_800i64
                }))
            }),
        ))
        .await;
        let expired_token_addr = serve(Router::new().route(
            "/broker",
            post(|| async {
                Json(json!({
                    "authorization_bearer_token": "expired-token",
                    "expires_at_unix": 1i64
                }))
            }),
        ))
        .await;
        let no_expiry_addr = serve(Router::new().route(
            "/broker",
            post(|| async {
                Json(json!({
                    "authorization_bearer_token": "ttl-token"
                }))
            }),
        ))
        .await;

        let base_config = McpConnectionConfig {
            server_name: "github".to_string(),
            server_ref: "github".to_string(),
            transport: "http".to_string(),
            target: "https://api.githubcopilot.com/mcp/".to_string(),
            args: Vec::new(),
            headers: HashMap::new(),
            disabled: false,
            namespace: Some("conic:wks:42".to_string()),
            mcp_server_name: Some("github".to_string()),
            agent_name: Some("cmo".to_string()),
            jwt_issuer: Some(TEST_PLATFORM_ISSUER.to_string()),
            auth_broker: None,
        };

        invalidate_all_broker_auth_cache().await;
        let status_config = McpConnectionConfig {
            agent_name: Some("status-case".to_string()),
            auth_broker: Some(McpAuthBrokerConfig {
                kind: "http_bearer".to_string(),
                url: format!("http://{status_addr}/broker"),
                cache_ttl_seconds: 60,
                audience: "github".to_string(),
            }),
            ..base_config.clone()
        };
        let err = resolve_http_headers(&status_config)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("returned 502 Bad Gateway"));
        assert!(err.contains("truncated"));

        invalidate_all_broker_auth_cache().await;
        let invalid_json_config = McpConnectionConfig {
            agent_name: Some("invalid-json-case".to_string()),
            auth_broker: Some(McpAuthBrokerConfig {
                kind: "http_bearer".to_string(),
                url: format!("http://{invalid_json_addr}/broker"),
                cache_ttl_seconds: 60,
                audience: "github".to_string(),
            }),
            ..base_config.clone()
        };
        assert!(resolve_http_headers(&invalid_json_config)
            .await
            .unwrap_err()
            .to_string()
            .contains("Failed to decode MCP auth broker response"));

        invalidate_all_broker_auth_cache().await;
        let empty_token_config = McpConnectionConfig {
            agent_name: Some("empty-token-case".to_string()),
            auth_broker: Some(McpAuthBrokerConfig {
                kind: "http_bearer".to_string(),
                url: format!("http://{empty_token_addr}/broker"),
                cache_ttl_seconds: 60,
                audience: "github".to_string(),
            }),
            ..base_config.clone()
        };
        assert!(resolve_http_headers(&empty_token_config)
            .await
            .unwrap_err()
            .to_string()
            .contains("returned an empty bearer token"));

        invalidate_all_broker_auth_cache().await;
        let expired_token_config = McpConnectionConfig {
            agent_name: Some("expired-token-case".to_string()),
            auth_broker: Some(McpAuthBrokerConfig {
                kind: "http_bearer".to_string(),
                url: format!("http://{expired_token_addr}/broker"),
                cache_ttl_seconds: 60,
                audience: "github".to_string(),
            }),
            ..base_config.clone()
        };
        assert!(resolve_http_headers(&expired_token_config)
            .await
            .unwrap_err()
            .to_string()
            .contains("already expired token"));

        invalidate_all_broker_auth_cache().await;
        let no_expiry_config = McpConnectionConfig {
            agent_name: Some("no-expiry-case".to_string()),
            auth_broker: Some(McpAuthBrokerConfig {
                kind: "http_bearer".to_string(),
                url: format!("http://{no_expiry_addr}/broker"),
                cache_ttl_seconds: 60,
                audience: String::new(),
            }),
            ..base_config
        };
        let headers = resolve_http_headers(&no_expiry_config).await.unwrap();
        assert_eq!(
            authorization_header(&headers).as_deref(),
            Some("Bearer ttl-token")
        );
    }

    #[tokio::test]
    async fn test_authenticated_reqwest_client_uses_default_token_for_stream_and_delete() {
        let state = HttpAuthState {
            get_hits: Arc::new(AtomicUsize::new(0)),
            delete_hits: Arc::new(AtomicUsize::new(0)),
        };
        let app = Router::new()
            .route(
                "/mcp",
                get(
                    move |State(state): State<HttpAuthState>, headers: HeaderMap| async move {
                        state.get_hits.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(
                            headers
                                .get("authorization")
                                .and_then(|value| value.to_str().ok()),
                            Some("Bearer secret-token")
                        );
                        assert_eq!(
                            headers
                                .get("mcp-session-id")
                                .and_then(|value| value.to_str().ok()),
                            Some("session-123")
                        );
                        (
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            "",
                        )
                    },
                )
                .delete(
                    move |State(state): State<HttpAuthState>, headers: HeaderMap| async move {
                        state.delete_hits.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(
                            headers
                                .get("authorization")
                                .and_then(|value| value.to_str().ok()),
                            Some("Bearer secret-token")
                        );
                        assert_eq!(
                            headers
                                .get("mcp-session-id")
                                .and_then(|value| value.to_str().ok()),
                            Some("session-123")
                        );
                        StatusCode::NO_CONTENT
                    },
                ),
            )
            .with_state(state.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = AuthenticatedReqwestClient::new(
            reqwest::Client::default(),
            Some("secret-token".to_string()),
        );
        let uri: Arc<str> = format!("http://{addr}/mcp").into();
        let session_id: Arc<str> = "session-123".into();

        let mut stream = client
            .get_stream(uri.clone(), session_id.clone(), None, None, HashMap::new())
            .await
            .unwrap();
        let _ = stream.next().await;
        client
            .delete_session(uri, session_id, None, HashMap::new())
            .await
            .unwrap();

        assert_eq!(state.get_hits.load(Ordering::SeqCst), 1);
        assert_eq!(state.delete_hits.load(Ordering::SeqCst), 1);

        server.abort();
    }

    #[tokio::test]
    async fn test_authenticated_reqwest_client_get_stream_handles_method_not_allowed_and_bad_content_type(
    ) {
        let app = Router::new().route(
            "/mcp",
            get(|| async {
                (
                    StatusCode::OK,
                    [(axum::http::header::CONTENT_TYPE, "text/plain")],
                    "nope",
                )
            })
            .post(|| async { StatusCode::METHOD_NOT_ALLOWED }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = AuthenticatedReqwestClient::new(reqwest::Client::default(), None);
        let uri: Arc<str> = format!("http://{addr}/mcp").into();

        let bad_content_type = client
            .get_stream(uri.clone(), "session-1".into(), None, None, HashMap::new())
            .await
            .err()
            .expect("unexpected content type should fail");
        assert!(matches!(
            bad_content_type,
            StreamableHttpError::UnexpectedContentType(Some(_))
        ));

        let method_not_allowed = client
            .post_message(uri, ping_message(), None, None, HashMap::new())
            .await
            .unwrap_err();
        assert!(matches!(method_not_allowed, StreamableHttpError::Client(_)));

        server.abort();
    }

    #[tokio::test]
    async fn test_authenticated_reqwest_client_get_stream_uses_custom_headers_last_event_id_and_auth_override(
    ) {
        let app = Router::new().route(
            "/mcp",
            get(|headers: HeaderMap| async move {
                assert_eq!(
                    headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok()),
                    Some("Bearer override-token")
                );
                assert_eq!(
                    headers
                        .get("last-event-id")
                        .and_then(|value| value.to_str().ok()),
                    Some("evt-42")
                );
                assert_eq!(
                    headers
                        .get("x-test-header")
                        .and_then(|value| value.to_str().ok()),
                    Some("demo")
                );
                (
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    "data: {}\n\n",
                )
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = AuthenticatedReqwestClient::new(
            reqwest::Client::default(),
            Some("default-token".to_string()),
        );
        let uri: Arc<str> = format!("http://{addr}/mcp").into();
        let custom_headers = HashMap::from([(
            axum::http::HeaderName::from_static("x-test-header"),
            axum::http::HeaderValue::from_static("demo"),
        )]);

        let mut stream = client
            .get_stream(
                uri,
                "session-1".into(),
                Some("evt-42".to_string()),
                Some("override-token".to_string()),
                custom_headers,
            )
            .await
            .unwrap();
        let _ = stream.next().await;

        server.abort();
    }

    #[tokio::test]
    async fn test_authenticated_reqwest_client_get_stream_rejects_missing_content_type_and_method_not_allowed(
    ) {
        let missing_content_type_app = Router::new().route(
            "/mcp",
            get(|| async {
                axum::http::Response::builder()
                    .status(StatusCode::OK)
                    .body(axum::body::Body::from("data: {}\n\n"))
                    .unwrap()
            }),
        );
        let method_not_allowed_app =
            Router::new().route("/mcp", get(|| async { StatusCode::METHOD_NOT_ALLOWED }));

        async fn serve(app: Router) -> SocketAddr {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            addr
        }

        let missing_addr = serve(missing_content_type_app).await;
        let disallowed_addr = serve(method_not_allowed_app).await;
        let client = AuthenticatedReqwestClient::new(reqwest::Client::default(), None);

        let err = match client
            .get_stream(
                format!("http://{missing_addr}/mcp").into(),
                "session-1".into(),
                None,
                None,
                HashMap::new(),
            )
            .await
        {
            Ok(_) => panic!("expected missing content-type to fail"),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            StreamableHttpError::UnexpectedContentType(None)
        ));

        let err = match client
            .get_stream(
                format!("http://{disallowed_addr}/mcp").into(),
                "session-1".into(),
                None,
                None,
                HashMap::new(),
            )
            .await
        {
            Ok(_) => panic!("expected method-not-allowed to fail"),
            Err(err) => err,
        };
        assert!(matches!(err, StreamableHttpError::ServerDoesNotSupportSse));
    }

    #[tokio::test]
    async fn test_authenticated_reqwest_client_delete_session_tolerates_method_not_allowed() {
        let app = Router::new().route("/mcp", delete(|| async { StatusCode::METHOD_NOT_ALLOWED }));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = AuthenticatedReqwestClient::new(reqwest::Client::default(), None);
        client
            .delete_session(
                format!("http://{addr}/mcp").into(),
                "session-1".into(),
                None,
                HashMap::new(),
            )
            .await
            .unwrap();

        server.abort();
    }

    #[tokio::test]
    async fn test_authenticated_reqwest_client_delete_session_honors_custom_headers_and_surfaces_errors(
    ) {
        let app = Router::new().route(
            "/mcp",
            delete(|headers: HeaderMap| async move {
                assert_eq!(
                    headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok()),
                    Some("Bearer explicit-token")
                );
                assert_eq!(
                    headers
                        .get("mcp-session-id")
                        .and_then(|value| value.to_str().ok()),
                    Some("session-9")
                );
                assert_eq!(
                    headers
                        .get("x-delete-header")
                        .and_then(|value| value.to_str().ok()),
                    Some("present")
                );
                StatusCode::INTERNAL_SERVER_ERROR
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = AuthenticatedReqwestClient::new(
            reqwest::Client::default(),
            Some("default-token".to_string()),
        );
        let err = client
            .delete_session(
                format!("http://{addr}/mcp").into(),
                "session-9".into(),
                Some("explicit-token".to_string()),
                HashMap::from([(
                    axum::http::HeaderName::from_static("x-delete-header"),
                    axum::http::HeaderValue::from_static("present"),
                )]),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, StreamableHttpError::Client(_)));

        server.abort();
    }

    #[tokio::test]
    async fn test_authenticated_reqwest_client_post_message_handles_accepted_json_and_sse() {
        let accepted_app =
            Router::new().route("/accepted", post(|| async { StatusCode::ACCEPTED }));
        let json_app = Router::new().route(
            "/json",
            post(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
                )
            }),
        );
        let sse_app = Router::new().route(
            "/sse",
            post(|| async {
                (
                    [
                        (axum::http::header::CONTENT_TYPE, "text/event-stream"),
                        (
                            axum::http::header::HeaderName::from_static("mcp-session-id"),
                            "session-xyz",
                        ),
                    ],
                    "data: {\"jsonrpc\":\"2.0\",\"method\":\"ping\"}\n\n",
                )
            }),
        );

        async fn serve(app: Router) -> SocketAddr {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            addr
        }

        let accepted_addr = serve(accepted_app).await;
        let json_addr = serve(json_app).await;
        let sse_addr = serve(sse_app).await;
        let client = AuthenticatedReqwestClient::new(reqwest::Client::default(), None);

        let accepted = client
            .post_message(
                format!("http://{accepted_addr}/accepted").into(),
                ping_message(),
                None,
                None,
                HashMap::new(),
            )
            .await
            .unwrap();
        assert!(matches!(accepted, StreamableHttpPostResponse::Accepted));

        let json_response = client
            .post_message(
                format!("http://{json_addr}/json").into(),
                ping_message(),
                None,
                None,
                HashMap::new(),
            )
            .await
            .unwrap();
        assert!(matches!(
            json_response,
            StreamableHttpPostResponse::Json(_, _)
        ));

        let sse_response = client
            .post_message(
                format!("http://{sse_addr}/sse").into(),
                ping_message(),
                None,
                None,
                HashMap::new(),
            )
            .await
            .unwrap();
        match sse_response {
            StreamableHttpPostResponse::Sse(_, session_id) => {
                assert_eq!(session_id.as_deref(), Some("session-xyz"));
            }
            other => panic!("expected SSE response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_authenticated_reqwest_client_post_message_uses_headers_and_rejects_unexpected_content_type(
    ) {
        let app = Router::new().route(
            "/mcp",
            post(|headers: HeaderMap| async move {
                assert_eq!(
                    headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok()),
                    Some("Bearer explicit-token")
                );
                assert_eq!(
                    headers
                        .get("mcp-session-id")
                        .and_then(|value| value.to_str().ok()),
                    Some("session-22")
                );
                assert_eq!(
                    headers
                        .get("x-post-header")
                        .and_then(|value| value.to_str().ok()),
                    Some("present")
                );
                (
                    [(axum::http::header::CONTENT_TYPE, "text/plain")],
                    "no structured response",
                )
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = AuthenticatedReqwestClient::new(
            reqwest::Client::default(),
            Some("default-token".to_string()),
        );
        let err = client
            .post_message(
                format!("http://{addr}/mcp").into(),
                ping_message(),
                Some("session-22".into()),
                Some("explicit-token".to_string()),
                HashMap::from([(
                    axum::http::HeaderName::from_static("x-post-header"),
                    axum::http::HeaderValue::from_static("present"),
                )]),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            StreamableHttpError::UnexpectedContentType(Some(_))
        ));

        server.abort();
    }

    #[tokio::test]
    async fn test_invalidate_broker_auth_cache_evicts_server_and_namespace_entries() {
        let _test_guard = BrokerAuthTestGuard::acquire().await;

        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route(
                "/broker",
                post(
                    move |State(hits): State<Arc<AtomicUsize>>,
                          Json(payload): Json<serde_json::Value>| async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        let server_name = payload["mcp_server_name"].as_str().unwrap_or("missing");
                        Json(json!({
                            "authorization_bearer_token": format!("token-{server_name}"),
                            "expires_at_unix": 4_102_444_800i64
                        }))
                    },
                ),
            )
            .with_state(hits.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let base = McpConnectionConfig {
            server_name: "github".to_string(),
            server_ref: "github".to_string(),
            transport: "http".to_string(),
            target: "https://api.githubcopilot.com/mcp/".to_string(),
            args: Vec::new(),
            headers: HashMap::new(),
            disabled: false,
            namespace: Some("conic:wks:42".to_string()),
            mcp_server_name: Some("github".to_string()),
            agent_name: Some("cmo".to_string()),
            jwt_issuer: Some(TEST_PLATFORM_ISSUER.to_string()),
            auth_broker: Some(McpAuthBrokerConfig {
                kind: "http_bearer".to_string(),
                url: format!("http://{addr}/broker"),
                cache_ttl_seconds: 60,
                audience: "github".to_string(),
            }),
        };
        let other_server = McpConnectionConfig {
            mcp_server_name: Some("jira".to_string()),
            ..base.clone()
        };

        assert_eq!(
            authorization_header(&resolve_http_headers(&base).await.unwrap()).as_deref(),
            Some("Bearer token-github")
        );
        assert_eq!(
            authorization_header(&resolve_http_headers(&other_server).await.unwrap()).as_deref(),
            Some("Bearer token-jira")
        );
        assert_eq!(hits.load(Ordering::SeqCst), 2);

        resolve_http_headers(&base).await.unwrap();
        resolve_http_headers(&other_server).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 2);

        invalidate_broker_auth_cache("conic:wks:42", Some("github")).await;
        resolve_http_headers(&base).await.unwrap();
        resolve_http_headers(&other_server).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 3);

        invalidate_broker_auth_cache("conic:wks:42", None).await;
        resolve_http_headers(&base).await.unwrap();
        resolve_http_headers(&other_server).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 5);

        server.abort();
    }

    struct BrokerAuthTestGuard {
        _guard: tokio::sync::MutexGuard<'static, ()>,
        previous_private_key: Option<String>,
    }

    impl BrokerAuthTestGuard {
        async fn acquire() -> Self {
            let guard = crate::test_support::async_env_mutex().lock().await;
            let previous_private_key = std::env::var(
                crate::control::security::platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
            )
            .ok();
            std::env::set_var(
                crate::control::security::platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
                crate::control::security::platform_jwt::TEST_RSA_PRIVATE_KEY,
            );
            invalidate_all_broker_auth_cache().await;
            Self {
                _guard: guard,
                previous_private_key,
            }
        }
    }

    impl Drop for BrokerAuthTestGuard {
        fn drop(&mut self) {
            clear_broker_auth_cache_for_test();
            if let Some(previous_private_key) = &self.previous_private_key {
                std::env::set_var(
                    crate::control::security::platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
                    previous_private_key,
                );
            } else {
                std::env::remove_var(
                    crate::control::security::platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
                );
            }
        }
    }
}
