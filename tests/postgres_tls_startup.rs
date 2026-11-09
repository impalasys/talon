// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use rustls::pki_types::{pem::PemObject, PrivateKeyDer};
use std::{process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    process::Command,
};

async fn postgres_tls_startup(binary: &str, mode: &str, trusts_ca: bool, accepts: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Talon test CA");
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    ca_params.not_before = time::OffsetDateTime::now_utc() - time::Duration::hours(1);
    ca_params.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(7);
    let ca_key = KeyPair::generate().unwrap();
    let ca = ca_params.clone().self_signed(&ca_key).unwrap();
    let mut server_params = CertificateParams::new(vec!["database.example.test".into()]).unwrap();
    server_params
        .distinguished_name
        .push(DnType::CommonName, "database.example.test");
    server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    server_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    server_params.not_before = ca_params.not_before;
    server_params.not_after = ca_params.not_after;
    let key = KeyPair::generate().unwrap();
    let certificate = server_params.signed_by(&key, &ca, &ca_key).unwrap();
    let ca_path = directory.path().join("ca.pem");
    let trusted_ca = if trusts_ca {
        ca.pem()
    } else {
        ca_params
            .self_signed(&KeyPair::generate().unwrap())
            .unwrap()
            .pem()
    };
    std::fs::write(&ca_path, trusted_ca).unwrap();
    let config = format!(
        "storage:\n  control:\n    driver: postgres\n    url: postgres://talon:password@{address}/talon?sslmode={mode}&sslrootcert={}\npubsub:\n  driver: local_socket\n",
        ca_path.display()
    );
    // The certificate is valid for database.example.test, not the loopback IP.
    // verify-ca must validate its chain without imposing hostname verification.
    let certificates = vec![certificate.der().clone()];
    let key = PrivateKeyDer::from_pem_slice(key.serialize_pem().as_bytes()).unwrap();
    let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(certificates, key)
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));

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

        let mut tls = acceptor.accept(stream).await?;
        // ClientHello alone misses certificate-validation failures. Require
        // PostgreSQL's startup packet inside a completed TLS session instead.
        let length = tls.read_u32().await?;
        assert!(length >= 8);
        assert_eq!(tls.read_u32().await?, 196608);
        Ok::<(), std::io::Error>(())
    };
    let (handshake, output) = tokio::join!(
        tokio::time::timeout(Duration::from_secs(10), server),
        tokio::time::timeout(Duration::from_secs(10), child.wait_with_output()),
    );
    let output = output
        .expect("runtime should exit after the mock closes TLS")
        .unwrap();
    let handshake = handshake.expect("TLS handshake must finish before timeout");
    assert_eq!(
        handshake.is_ok(),
        accepts,
        "unexpected TLS result {handshake:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "mock ends before database authentication"
    );
}

#[tokio::test]
async fn gateway_starts_verified_postgres_tls() {
    postgres_tls_startup(env!("CARGO_BIN_EXE_talon-server"), "verify-ca", true, true).await;
}

#[tokio::test]
async fn worker_starts_verified_postgres_tls() {
    postgres_tls_startup(env!("CARGO_BIN_EXE_talon-worker"), "verify-ca", true, true).await;
}

#[tokio::test]
async fn node_starts_verified_postgres_tls() {
    postgres_tls_startup(env!("CARGO_BIN_EXE_talon-node"), "verify-ca", true, true).await;
}

#[tokio::test]
async fn gateway_rejects_untrusted_postgres_ca() {
    postgres_tls_startup(
        env!("CARGO_BIN_EXE_talon-server"),
        "verify-ca",
        false,
        false,
    )
    .await;
}

#[tokio::test]
async fn gateway_verify_full_rejects_hostname_mismatch() {
    postgres_tls_startup(
        env!("CARGO_BIN_EXE_talon-server"),
        "verify-full",
        true,
        false,
    )
    .await;
}
