// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::utils::connect_json_values;
use super::{
    DispatchingSandboxBackend, DockerSandboxBackend, ExecSpec, SandboxBackend,
    SandboxClassSpecJson, SandboxPolicySpecJson, SandboxPolicyTemplateJson, SandboxQuotaJson,
};
use crate::test_support::docker_test_guard;
use anyhow::Result;
use base64::Engine as _;

#[tokio::test]
async fn dispatching_backend_prefixes_mock_sandboxes() {
    let backend = DispatchingSandboxBackend::default();
    let handle = backend
        .create(
            &SandboxClassSpecJson {
                provider: "mock".to_string(),
                provider_config: serde_json::json!({}),
                credentials: serde_json::json!({}),
            },
            &SandboxPolicySpecJson::default(),
        )
        .await
        .unwrap();
    assert!(handle.backend_id.starts_with("mock:mock-mock-"));
    backend.destroy(&handle.backend_id).await.unwrap();
}

#[test]
fn parses_connect_json_frames() {
    let payload = serde_json::to_vec(&serde_json::json!({
        "event": { "data": { "stdout": base64::engine::general_purpose::STANDARD.encode("hello") } }
    }))
    .unwrap();
    let mut frame = vec![0];
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    let values = connect_json_values(&frame);
    assert_eq!(values.len(), 1);
}

#[tokio::test]
async fn docker_backend_smoke_when_enabled() {
    if std::env::var("TALON_DOCKER_SANDBOX_TEST").ok().as_deref() != Some("1") {
        return;
    }
    let _guard = docker_test_guard();
    let backend = DockerSandboxBackend::default();
    let class = SandboxClassSpecJson {
        provider: "docker".to_string(),
        provider_config: serde_json::json!({ "image": "alpine:3.20" }),
        credentials: serde_json::json!({}),
    };
    let policy = SandboxPolicySpecJson {
        class_ref: Default::default(),
        template: SandboxPolicyTemplateJson {
            spec: serde_json::json!({
                "workspace": { "mountPath": "/workspace" },
                "setup": {
                    "commands": ["printf setup-ok > /workspace/setup.txt"]
                },
                "network": { "mode": "restricted" },
                "filesystem": { "writable": ["/workspace", "/tmp"] }
            }),
        },
        quota: SandboxQuotaJson { max_concurrent: 1 },
    };
    let handle = backend.create(&class, &policy).await.unwrap();
    let test_result: Result<()> = async {
        backend
            .write_file(&handle.backend_id, "/workspace/hello.txt", b"hello docker")
            .await?;
        let content = backend
            .read_file(&handle.backend_id, "/workspace/hello.txt")
            .await?;
        assert_eq!(content, b"hello docker");
        let setup_content = backend
            .read_file(&handle.backend_id, "/workspace/setup.txt")
            .await?;
        assert_eq!(setup_content, b"setup-ok");
        let process = backend
            .exec(
                &handle.backend_id,
                ExecSpec {
                    command: "sh".to_string(),
                    args: vec![
                        "-lc".to_string(),
                        "cat /workspace/hello.txt && printf ' ok'".to_string(),
                    ],
                    cwd: "/workspace".to_string(),
                    env: Default::default(),
                },
            )
            .await?;
        let output = backend
            .read_process_output(&handle.backend_id, &process.id)
            .await?;
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(output.stdout, "hello docker ok");
        Ok(())
    }
    .await;
    let destroy_result = backend.destroy(&handle.backend_id).await;
    test_result.unwrap();
    destroy_result.unwrap();
}
