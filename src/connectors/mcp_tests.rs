#[cfg(test)]
mod tests {
    use crate::connectors::mcp::{
        authorization_bearer_token, authorization_header, clear_broker_auth_cache_for_test,
        content_type_matches, format_tool_result, invalidate_all_broker_auth_cache,
        resolve_http_headers, validate_http_headers, AuthenticatedReqwestClient,
        McpAuthBrokerConfig, McpClient, McpConnectionConfig,
    };
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::{get, post},
        Json, Router,
    };
    use futures::StreamExt;
    use rmcp::model::{Content, ResourceContents};
    use rmcp::transport::streamable_http_client::StreamableHttpClient;
    use serde_json::json;
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::OnceLock;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio::sync::{Barrier, Mutex};

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
            &[Content::text("File content here"), Content::text("More text")],
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
                        assert_eq!(payload["binding_name"], "github");
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
            binding_name: Some("github".to_string()),
            agent_name: Some("cmo".to_string()),
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
                        assert_eq!(payload["binding_name"], "github");
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
            binding_name: Some("github".to_string()),
            agent_name: Some("cmo".to_string()),
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

    fn broker_auth_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct BrokerAuthTestGuard {
        _guard: tokio::sync::MutexGuard<'static, ()>,
        previous_secret: Option<String>,
    }

    impl BrokerAuthTestGuard {
        async fn acquire() -> Self {
            let guard = broker_auth_test_lock().lock().await;
            let previous_secret = std::env::var("TALON_JWT_SECRET").ok();
            std::env::set_var("TALON_JWT_SECRET", "test-secret");
            invalidate_all_broker_auth_cache().await;
            Self {
                _guard: guard,
                previous_secret,
            }
        }
    }

    impl Drop for BrokerAuthTestGuard {
        fn drop(&mut self) {
            clear_broker_auth_cache_for_test();
            if let Some(previous_secret) = &self.previous_secret {
                std::env::set_var("TALON_JWT_SECRET", previous_secret);
            } else {
                std::env::remove_var("TALON_JWT_SECRET");
            }
        }
    }
}
