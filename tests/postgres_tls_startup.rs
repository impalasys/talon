// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    process::Command,
};

async fn starts_verified_postgres_tls(binary: &str) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let config = format!(
        "storage:\n  control:\n    driver: postgres\n    url: postgres://talon:password@{address}/talon?sslmode=verify-ca\npubsub:\n  driver: local_socket\n"
    );

    // Launch a real runtime in a fresh process: another test's TLS client must
    // not install a global provider and hide missing runtime initialization.
    let child = Command::new(binary)
        .current_dir(directory.path())
        .env("TALON_CONFIG_INLINE_YAML", config)
        .env(
            "TALON_JWT_PRIVATE_KEY_PEM",
            include_str!("../src/control/security/test_rsa_private_key.pem"),
        )
        .env("TALON_JWT_ISSUER", "https://talon.example.com")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    let server = async {
        let (mut stream, _) = listener.accept().await?;
        let mut request = [0; 8];
        stream.read_exact(&mut request).await?;
        assert_eq!(request, [0, 0, 0, 8, 4, 210, 22, 47]);
        stream.write_all(b"S").await?;

        // SQLx builds the CA verifier before sending ClientHello. The missing
        // Rustls provider used to panic at that point with mixed TLS features.
        let mut hello = [0; 6];
        stream.read_exact(&mut hello).await?;
        assert_eq!(hello[0], 22);
        assert_eq!(hello[1], 3);
        assert_eq!(hello[5], 1);
        Ok::<(), std::io::Error>(())
    };
    let (handshake, output) = tokio::join!(
        tokio::time::timeout(Duration::from_secs(10), server),
        tokio::time::timeout(Duration::from_secs(10), child.wait_with_output()),
    );
    let output = output
        .expect("runtime should exit after the mock closes TLS")
        .unwrap();
    assert!(
        matches!(handshake, Ok(Ok(()))),
        "runtime did not start verified PostgreSQL TLS: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "mock ends before TLS completes");
}

#[tokio::test]
async fn gateway_starts_verified_postgres_tls() {
    starts_verified_postgres_tls(env!("CARGO_BIN_EXE_talon-server")).await;
}

#[tokio::test]
async fn worker_starts_verified_postgres_tls() {
    starts_verified_postgres_tls(env!("CARGO_BIN_EXE_talon-worker")).await;
}

#[tokio::test]
async fn node_starts_verified_postgres_tls() {
    starts_verified_postgres_tls(env!("CARGO_BIN_EXE_talon-node")).await;
}
