// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Shared PostgreSQL pool construction and credential rotation.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
    PgPool,
};
use std::fmt;
use std::{str::FromStr, sync::Arc, time::Duration};
use tokio::sync::RwLock;

const IAM_POOL_ROTATION: Duration = Duration::from_secs(45 * 60);
const TOKEN_EXPIRY_MARGIN: Duration = Duration::from_secs(5 * 60);
const ROTATION_RETRY_INITIAL: Duration = Duration::from_secs(5);
const ROTATION_RETRY_MAX: Duration = Duration::from_secs(2 * 60);

fn next_rotation_retry(current: Duration) -> Duration {
    current
        .saturating_mul(2)
        .max(ROTATION_RETRY_INITIAL)
        .min(ROTATION_RETRY_MAX)
}

pub mod gcp;

#[async_trait]
pub trait CredentialProvider: Send + Sync + 'static {
    async fn credential(&self) -> Result<DatabaseCredential>;
}

pub struct DatabaseCredential {
    pub value: String,
    pub valid_for: Duration,
}

impl DatabaseCredential {
    fn rotation_delay(&self) -> Duration {
        let before_expiry = self.valid_for.saturating_sub(TOKEN_EXPIRY_MARGIN);
        if before_expiry.is_zero() {
            (self.valid_for / 2).max(Duration::from_secs(1))
        } else {
            before_expiry.min(IAM_POOL_ROTATION)
        }
    }
}

#[derive(Clone)]
pub struct PostgresPool(Arc<RwLock<PoolState>>);

struct PoolState {
    pool: PgPool,
    rotation_error: Option<String>,
}

impl PostgresPool {
    /// Password mode follows the historical URL-based construction exactly.
    pub async fn password(url: &str, max_connections: u32) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(url)
            .await?;
        Ok(Self(Arc::new(RwLock::new(PoolState {
            pool,
            rotation_error: None,
        }))))
    }

    pub async fn with_credentials(
        url: &str,
        max_connections: u32,
        provider: Arc<dyn CredentialProvider>,
    ) -> Result<Self> {
        Self::with_credentials_and_rotation(url, max_connections, provider, None, None, true).await
    }

    async fn with_credentials_and_rotation(
        url: &str,
        max_connections: u32,
        provider: Arc<dyn CredentialProvider>,
        rotation_override: Option<Duration>,
        retry_override: Option<Duration>,
        // False only for the in-module plaintext mock server test.
        require_tls: bool,
    ) -> Result<Self> {
        let credential = provider
            .credential()
            .await
            .map_err(|_| anyhow!("failed to obtain PostgreSQL IAM credentials"))?;
        let mut refresh_after = credential.rotation_delay();
        let options = iam_options_with_tls(url, credential.value, require_tls)?;
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options.into_inner())
            .await
            .map_err(|_| anyhow!("failed to connect to PostgreSQL using IAM credentials"))?;
        let result = Self(Arc::new(RwLock::new(PoolState {
            pool,
            rotation_error: None,
        })));
        let state = result.0.clone();
        let url = url.to_string();
        tokio::spawn(async move {
            let mut retry_after = ROTATION_RETRY_INITIAL;
            loop {
                tokio::time::sleep(rotation_override.unwrap_or(refresh_after)).await;
                let credential = match provider.credential().await {
                    Ok(credential) => credential,
                    Err(_) => {
                        state.write().await.rotation_error =
                            Some("failed to refresh PostgreSQL IAM credentials".to_string());
                        refresh_after = retry_override.unwrap_or(retry_after);
                        retry_after = next_rotation_retry(retry_after);
                        continue;
                    }
                };
                let next_refresh_after = credential.rotation_delay();
                let replacement = match PgPoolOptions::new()
                    .max_connections(max_connections)
                    .connect_with(
                        iam_options_with_tls(&url, credential.value, require_tls)
                            .expect("pool URL was validated at startup")
                            .into_inner(),
                    )
                    .await
                {
                    Ok(pool) => pool,
                    Err(_) => {
                        state.write().await.rotation_error =
                            Some("failed to establish PostgreSQL IAM pool".to_string());
                        refresh_after = retry_override.unwrap_or(retry_after);
                        retry_after = next_rotation_retry(retry_after);
                        continue;
                    }
                };
                let mut current = state.write().await;
                current.pool = replacement;
                current.rotation_error = None;
                drop(current);
                refresh_after = rotation_override.unwrap_or(next_refresh_after);
                retry_after = ROTATION_RETRY_INITIAL;
            }
        });
        Ok(result)
    }

    pub async fn pool(&self) -> Result<PgPool> {
        let state = self.0.read().await;
        if let Some(error) = &state.rotation_error {
            return Err(anyhow!("{error}"));
        }
        Ok(state.pool.clone())
    }
}

struct RedactedIamConnectOptions(PgConnectOptions);

impl RedactedIamConnectOptions {
    fn into_inner(self) -> PgConnectOptions {
        self.0
    }
}

impl fmt::Debug for RedactedIamConnectOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("IamConnectOptions([REDACTED])")
    }
}

fn iam_options(url: &str, token: String) -> Result<RedactedIamConnectOptions> {
    iam_options_with_tls(url, token, true)
}

fn iam_options_with_tls(
    url: &str,
    token: String,
    require_tls: bool,
) -> Result<RedactedIamConnectOptions> {
    // Never retain or report a URL password in IAM mode.
    let mut parsed =
        url::Url::parse(url).context("invalid PostgreSQL URL for IAM authentication")?;
    let _ = parsed.set_password(None);
    let mut options = PgConnectOptions::from_str(parsed.as_str())
        .map_err(|_| anyhow!("invalid PostgreSQL connection options for IAM authentication"))?;
    if require_tls
        && matches!(
            options.get_ssl_mode(),
            PgSslMode::Disable | PgSslMode::Allow | PgSslMode::Prefer
        )
    {
        options = options.ssl_mode(PgSslMode::Require);
    }
    options = options.password(&token);
    Ok(RedactedIamConnectOptions(options))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeProvider(AtomicUsize);

    #[async_trait]
    impl CredentialProvider for FakeProvider {
        async fn credential(&self) -> Result<DatabaseCredential> {
            let n = self.0.fetch_add(1, Ordering::SeqCst);
            Ok(DatabaseCredential {
                value: format!("fake-token-{n}"),
                valid_for: Duration::from_secs(60 * 60),
            })
        }
    }

    struct FailOnceProvider(AtomicUsize);

    #[async_trait]
    impl CredentialProvider for FailOnceProvider {
        async fn credential(&self) -> Result<DatabaseCredential> {
            let n = self.0.fetch_add(1, Ordering::SeqCst);
            if n == 1 {
                return Err(anyhow!("transient credential source failure"));
            }
            Ok(DatabaseCredential {
                value: format!("fake-token-{n}"),
                valid_for: Duration::from_secs(60 * 60),
            })
        }
    }

    #[tokio::test]
    async fn credential_provider_is_queried_for_each_pool_generation() {
        let provider = FakeProvider(AtomicUsize::new(0));
        assert_eq!(provider.credential().await.unwrap().value, "fake-token-0");
        assert_eq!(provider.credential().await.unwrap().value, "fake-token-1");
        assert!(IAM_POOL_ROTATION < Duration::from_secs(60 * 60));
        assert_eq!(
            DatabaseCredential {
                value: "ignored".to_string(),
                valid_for: Duration::from_secs(60 * 60),
            }
            .rotation_delay(),
            IAM_POOL_ROTATION
        );
        let mut retry = ROTATION_RETRY_INITIAL;
        for expected in [10, 20, 40, 80, 120, 120] {
            retry = next_rotation_retry(retry);
            assert_eq!(retry, Duration::from_secs(expected));
        }
    }

    #[tokio::test]
    async fn pool_rotation_authenticates_with_a_new_token_before_expiry() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
            sync::mpsc,
        };

        async fn backend_message(stream: &mut tokio::net::TcpStream, kind: u8, body: &[u8]) {
            stream.write_all(&[kind]).await.unwrap();
            stream
                .write_all(&((body.len() as u32 + 4).to_be_bytes()))
                .await
                .unwrap();
            stream.write_all(body).await.unwrap();
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (tokens_tx, mut tokens_rx) = mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let tokens_tx = tokens_tx.clone();
                tokio::spawn(async move {
                    let mut length = [0; 4];
                    if stream.read_exact(&mut length).await.is_err() {
                        return;
                    }
                    let mut startup = vec![0; u32::from_be_bytes(length) as usize - 4];
                    if stream.read_exact(&mut startup).await.is_err() {
                        return;
                    }
                    backend_message(&mut stream, b'R', &3_u32.to_be_bytes()).await;
                    let mut tag = [0; 1];
                    if stream.read_exact(&mut tag).await.is_err() || tag[0] != b'p' {
                        return;
                    }
                    if stream.read_exact(&mut length).await.is_err() {
                        return;
                    }
                    let mut password = vec![0; u32::from_be_bytes(length) as usize - 4];
                    if stream.read_exact(&mut password).await.is_err() {
                        return;
                    }
                    let password = String::from_utf8_lossy(&password)
                        .trim_end_matches('\0')
                        .to_string();
                    let _ = tokens_tx.send(password);

                    backend_message(&mut stream, b'R', &0_u32.to_be_bytes()).await;
                    for (key, value) in [
                        ("server_version", "16.0"),
                        ("server_encoding", "UTF8"),
                        ("client_encoding", "UTF8"),
                        ("DateStyle", "ISO, MDY"),
                        ("integer_datetimes", "on"),
                        ("standard_conforming_strings", "on"),
                        ("TimeZone", "UTC"),
                    ] {
                        let mut body = key.as_bytes().to_vec();
                        body.push(0);
                        body.extend_from_slice(value.as_bytes());
                        body.push(0);
                        backend_message(&mut stream, b'S', &body).await;
                    }
                    let mut key_data = 1_i32.to_be_bytes().to_vec();
                    key_data.extend_from_slice(&1_i32.to_be_bytes());
                    backend_message(&mut stream, b'K', &key_data).await;
                    backend_message(&mut stream, b'Z', b"I").await;
                    let mut message = [0; 1];
                    let _ = stream.read(&mut message).await;
                });
            }
        });

        let pool = PostgresPool::with_credentials_and_rotation(
            &format!("postgres://iam-user@{address}/talon?sslmode=disable"),
            1,
            Arc::new(FailOnceProvider(AtomicUsize::new(0))),
            Some(Duration::from_millis(100)),
            Some(Duration::from_millis(100)),
            false,
        )
        .await
        .unwrap();
        let first = tokio::time::timeout(Duration::from_secs(2), tokens_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first, "fake-token-0");
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if pool.pool().await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("pool access should report the failed refresh");
        let failed_at = tokio::time::Instant::now();
        let second = tokio::time::timeout(Duration::from_secs(2), tokens_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second, "fake-token-2");
        assert!(failed_at.elapsed() >= Duration::from_millis(80));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if pool.pool().await.is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("pool should recover on the short retry interval");
        drop(pool);
        server.abort();
    }

    struct FailingProvider;

    #[async_trait]
    impl CredentialProvider for FailingProvider {
        async fn credential(&self) -> Result<DatabaseCredential> {
            Err(anyhow!("credential source unavailable"))
        }
    }

    #[tokio::test]
    async fn credential_failure_is_returned_without_secret_details() {
        let error = PostgresPool::with_credentials(
            "postgres://iam-user@localhost/talon",
            1,
            Arc::new(FailingProvider),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(
            error.to_string(),
            "failed to obtain PostgreSQL IAM credentials"
        );
    }

    #[test]
    fn iam_options_discards_url_password_and_keeps_connection_settings() {
        let options = iam_options(
            "postgres://iam-user:ignored@db.example:5432/talon?sslmode=require",
            "fresh-token".to_string(),
        )
        .unwrap();
        let rendered = format!("{options:?}");
        assert!(!rendered.contains("ignored"));
        assert!(!rendered.contains("fresh-token"));
    }

    #[test]
    fn iam_options_require_tls_and_preserve_verification_modes() {
        for url in [
            "postgres://iam-user@db.example/talon?sslmode=disable",
            "postgres://iam-user@db.example/talon?sslmode=prefer",
            "postgres://iam-user@db.example/talon",
        ] {
            let options = iam_options(url, "token".to_string()).unwrap();
            assert!(matches!(options.0.get_ssl_mode(), PgSslMode::Require));
        }

        let verified = iam_options(
            "postgres://iam-user@db.example/talon?sslmode=verify-full",
            "token".to_string(),
        )
        .unwrap();
        assert!(matches!(verified.0.get_ssl_mode(), PgSslMode::VerifyFull));
    }
}
