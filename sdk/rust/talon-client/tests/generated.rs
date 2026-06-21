// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use talon_client::v1::ListResourcesRequest;
use talon_client::{data::SessionJournalEntryPayloadLlmResponse, harness::ChatResponse};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn generated_talon_v1_types_are_available() {
    let request = ListResourcesRequest {
        ns: "default".to_string(),
        kind: Some("Agent".to_string()),
        ..Default::default()
    };
    assert_eq!(request.ns, "default");
    assert_eq!(request.kind.as_deref(), Some("Agent"));
}

#[test]
fn generated_data_types_can_reference_harness_types() {
    let payload = SessionJournalEntryPayloadLlmResponse {
        response: Some(ChatResponse {
            content: "ok".to_string(),
            ..Default::default()
        }),
    };
    assert_eq!(payload.response.unwrap().content, "ok");
}

#[test]
fn generated_clientset_exposes_all_talon_v1_services_and_streaming_methods() {
    let _ = talon_client::TalonClient::get_namespace;
    let _ = talon_client::TalonClient::append_session_message;
    let _ = talon_client::TalonClient::stream_session_parts_batch;
    let _ = talon_client::TalonClient::submit_session_turn;
    let _ = talon_client::TalonClient::post_channel_message;
    let _ = talon_client::TalonClient::stream_channel_events;
    let _ = talon_client::TalonClient::stream_workflow_events;
    let _ = talon_client::TalonClient::exchange_oidc_token;
}

fn grpc_web_response_body(message: &impl prost::Message) -> Vec<u8> {
    let mut message_bytes = Vec::new();
    message.encode(&mut message_bytes).expect("encode message");

    let trailers = b"grpc-status: 0\r\n";
    let mut body = Vec::new();
    body.push(0);
    body.extend_from_slice(&(message_bytes.len() as u32).to_be_bytes());
    body.extend_from_slice(&message_bytes);
    body.push(0x80);
    body.extend_from_slice(&(trailers.len() as u32).to_be_bytes());
    body.extend_from_slice(trailers);
    body
}

#[tokio::test]
async fn grpc_web_talon_client_uses_http1_grpc_web_requests() {
    let response_body = grpc_web_response_body(&talon_client::v1::ListNamespacesResponse {
        namespaces: vec![talon_client::v1::NamespaceResponse {
            name: "from-grpc-web".to_string(),
            parent: None,
            is_deleted: false,
            deleted_at: 0,
            labels: std::collections::HashMap::new(),
        }],
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept request");
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let bytes_read = stream.read(&mut buffer).await.expect("read request");
            if bytes_read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..bytes_read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let request = String::from_utf8_lossy(&request);
        assert!(request.starts_with("POST /talon.v1.NamespaceService/List "));
        assert!(request.contains("content-type: application/grpc-web"));
        assert!(request.contains("x-grpc-web: 1"));

        let headers = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/grpc-web+proto\r\ncontent-length: {}\r\n\r\n",
            response_body.len()
        );
        stream
            .write_all(headers.as_bytes())
            .await
            .expect("write response headers");
        stream
            .write_all(&response_body)
            .await
            .expect("write response body");
    });

    let gateway = format!("http://{addr}");
    let mut client = talon_client::GrpcWebTalonClient::connect_grpc_web(gateway)
        .expect("connect gRPC-Web client");
    let response = client
        .namespaces
        .list(talon_client::v1::ListNamespacesRequest { parent: None })
        .await
        .expect("list namespaces")
        .into_inner();

    assert_eq!(response.namespaces[0].name, "from-grpc-web");
    server.await.expect("server task");
}
