// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use chrono::Utc;
use futures::stream::{FuturesUnordered, StreamExt};
use reqwest::Client;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use std::time::Duration;
use tracing::{error, warn};

use super::{ScheduleWakeupRequest, ScheduledWakeup, SchedulerBackend, SCHEDULER_AUTH_HEADER};

const DEFAULT_TABLE: &str = "talon_local_scheduler_jobs";
const CLAIM_TIMEOUT_SECONDS: i64 = 60;
const DELIVERY_TIMEOUT_SECONDS: u64 = 15;
const POLL_INTERVAL_MILLIS: u64 = 1_000;
const MAX_CONCURRENT_DELIVERIES: usize = 10;
const MAX_POOL_CONNECTIONS: u32 = 10;
const MAX_DELIVERY_ATTEMPTS: i32 = 20;
const INITIAL_RETRY_DELAY_SECONDS: i64 = 5;
const MAX_RETRY_DELAY_SECONDS: i64 = 300;

#[derive(Clone)]
pub struct LocalPostgresSchedulerBackend {
    pool: PgPool,
    table: String,
}

#[derive(Clone)]
struct DueWakeup {
    handle: String,
    namespace: String,
    schedule_id: String,
    revision: i64,
    fire_at_micros: i64,
    payload: Vec<u8>,
    attempts: i32,
}

impl LocalPostgresSchedulerBackend {
    pub async fn new(
        database_url: &str,
        table: Option<String>,
        runner_target_url: Option<String>,
        auth_token: Option<String>,
        runner_enabled: bool,
    ) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(MAX_POOL_CONNECTIONS)
            .connect(database_url)
            .await?;
        let table = table.unwrap_or_else(|| DEFAULT_TABLE.to_string());
        validate_identifier(&table)?;
        let create_stmt = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                handle TEXT PRIMARY KEY,
                namespace TEXT NOT NULL,
                schedule_id TEXT NOT NULL,
                revision BIGINT NOT NULL,
                fire_at_micros BIGINT NOT NULL,
                payload BYTEA NOT NULL,
                canceled_at_micros BIGINT NULL,
                delivered_at_micros BIGINT NULL,
                claimed_at_micros BIGINT NULL,
                claim_until_micros BIGINT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                last_error TEXT NULL,
                created_at_micros BIGINT NOT NULL
            )",
            table
        );
        sqlx::query(&create_stmt).execute(&pool).await?;
        let index_stmt = format!(
            "CREATE INDEX IF NOT EXISTS {table}_due_idx ON {table} (fire_at_micros) WHERE canceled_at_micros IS NULL AND delivered_at_micros IS NULL",
            table = table
        );
        sqlx::query(&index_stmt).execute(&pool).await?;

        if runner_enabled {
            let Some(target_url) = runner_target_url.filter(|value| !value.trim().is_empty())
            else {
                warn!("local_postgres scheduler runner enabled without target URL; wakeups will not fire");
                return Ok(Self { pool, table });
            };
            let runner = Self {
                pool: pool.clone(),
                table: table.clone(),
            };
            tokio::spawn(async move {
                runner
                    .run_loop(
                        target_url,
                        auth_token.filter(|value| !value.trim().is_empty()),
                    )
                    .await;
            });
        }

        Ok(Self { pool, table })
    }

    async fn run_loop(self, target_url: String, auth_token: Option<String>) {
        let client = Client::builder()
            .timeout(Duration::from_secs(DELIVERY_TIMEOUT_SECONDS))
            .build()
            .expect("failed to build local scheduler client");
        loop {
            let mut has_more = false;
            match self
                .claim_due_wakeups(MAX_CONCURRENT_DELIVERIES as i64)
                .await
            {
                Ok(wakeups) => {
                    if !wakeups.is_empty() {
                        tracing::info!(
                            claimed_wakeups = wakeups.len(),
                            target_url = %target_url,
                            "local_postgres scheduler claimed due wakeups"
                        );
                    }
                    has_more = wakeups.len() == MAX_CONCURRENT_DELIVERIES;
                    let mut deliveries = FuturesUnordered::new();
                    for wakeup in wakeups {
                        let backend = self.clone();
                        let client = client.clone();
                        let target_url = target_url.clone();
                        let auth_token = auth_token.clone();
                        deliveries.push(async move {
                            let handle = wakeup.handle.clone();
                            let namespace = wakeup.namespace.clone();
                            let schedule_id = wakeup.schedule_id.clone();
                            let revision = wakeup.revision;
                            let fire_at_micros = wakeup.fire_at_micros;
                            let attempts = wakeup.attempts;
                            let result = tokio::time::timeout(
                                Duration::from_secs(DELIVERY_TIMEOUT_SECONDS + 5),
                                backend.deliver_wakeup(
                                    &client,
                                    &target_url,
                                    auth_token.as_deref(),
                                    &wakeup,
                                ),
                            )
                            .await
                            .map_err(|_| {
                                anyhow!("timed out delivering scheduler wakeup to {}", target_url)
                            })
                            .and_then(|result| result);
                            (
                                handle,
                                namespace,
                                schedule_id,
                                revision,
                                fire_at_micros,
                                attempts,
                                result,
                                backend,
                            )
                        });
                    }
                    while let Some((
                        handle,
                        namespace,
                        schedule_id,
                        revision,
                        fire_at_micros,
                        attempts,
                        result,
                        backend,
                    )) = deliveries.next().await
                    {
                        if let Err(err) = result {
                            warn!(
                                handle = %handle,
                                namespace = %namespace,
                                schedule_id = %schedule_id,
                                revision = revision,
                                fire_at_micros = fire_at_micros,
                                attempts = attempts,
                                error = %err,
                                "failed to deliver local scheduler wakeup"
                            );
                            if let Err(mark_err) = backend
                                .mark_delivery_failed(&handle, attempts, &err.to_string())
                                .await
                            {
                                error!(
                                    handle = %handle,
                                    namespace = %namespace,
                                    schedule_id = %schedule_id,
                                    revision = revision,
                                    fire_at_micros = fire_at_micros,
                                    attempts = attempts,
                                    error = %mark_err,
                                    "failed to record local scheduler delivery failure"
                                );
                            }
                        }
                    }
                }
                Err(err) => {
                    error!(error = %err, "local_postgres scheduler poll failed");
                }
            }
            if !has_more {
                tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
            }
        }
    }

    async fn claim_due_wakeups(&self, limit: i64) -> Result<Vec<DueWakeup>> {
        let now_micros = Utc::now().timestamp_micros();
        let query = format!(
            "WITH due AS (
                SELECT handle
                FROM {table}
                WHERE canceled_at_micros IS NULL
                  AND delivered_at_micros IS NULL
                  AND attempts < $4
                  AND fire_at_micros <= $2
                  AND (claim_until_micros IS NULL OR claim_until_micros < $2)
                ORDER BY fire_at_micros
                LIMIT $1
                FOR UPDATE SKIP LOCKED
            )
            UPDATE {table} AS jobs
            SET claimed_at_micros = $2,
                claim_until_micros = $2 + ($3 * 1000000),
                attempts = jobs.attempts + 1
            FROM due
            WHERE jobs.handle = due.handle
            RETURNING jobs.handle, jobs.namespace, jobs.schedule_id, jobs.revision, jobs.fire_at_micros, jobs.payload, jobs.attempts",
            table = self.table
        );
        let rows = sqlx::query(&query)
            .bind(limit)
            .bind(now_micros)
            .bind(CLAIM_TIMEOUT_SECONDS)
            .bind(MAX_DELIVERY_ATTEMPTS)
            .fetch_all(&self.pool)
            .await?;
        let mut wakeups = Vec::with_capacity(rows.len());
        for row in rows {
            wakeups.push(DueWakeup {
                handle: row.try_get("handle")?,
                namespace: row.try_get("namespace")?,
                schedule_id: row.try_get("schedule_id")?,
                revision: row.try_get("revision")?,
                fire_at_micros: row.try_get("fire_at_micros")?,
                payload: row.try_get("payload")?,
                attempts: row.try_get("attempts")?,
            });
        }
        Ok(wakeups)
    }

    async fn deliver_wakeup(
        &self,
        client: &Client,
        target_url: &str,
        auth_token: Option<&str>,
        wakeup: &DueWakeup,
    ) -> Result<()> {
        tracing::info!(
            handle = %wakeup.handle,
            namespace = %wakeup.namespace,
            schedule_id = %wakeup.schedule_id,
            revision = wakeup.revision,
            fire_at_micros = wakeup.fire_at_micros,
            target_url = %target_url,
            attempts = wakeup.attempts,
            "delivering local scheduler wakeup"
        );
        let mut request = client
            .post(target_url)
            .header("content-type", "application/json")
            .body(wakeup.payload.clone());
        if let Some(token) = auth_token {
            request = request.header(SCHEDULER_AUTH_HEADER, token);
        }
        request.send().await?.error_for_status()?;
        tracing::info!(
            handle = %wakeup.handle,
            namespace = %wakeup.namespace,
            schedule_id = %wakeup.schedule_id,
            revision = wakeup.revision,
            fire_at_micros = wakeup.fire_at_micros,
            "local scheduler wakeup HTTP delivery succeeded"
        );
        self.mark_delivered(&wakeup.handle).await
    }

    async fn mark_delivered(&self, handle: &str) -> Result<()> {
        let query = format!(
            "UPDATE {} SET delivered_at_micros = $2, claim_until_micros = NULL WHERE handle = $1",
            self.table
        );
        sqlx::query(&query)
            .bind(handle)
            .bind(Utc::now().timestamp_micros())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn mark_delivery_failed(
        &self,
        handle: &str,
        attempts: i32,
        error_message: &str,
    ) -> Result<()> {
        if attempts >= MAX_DELIVERY_ATTEMPTS {
            let query = format!(
                "UPDATE {} SET canceled_at_micros = $2, claim_until_micros = NULL, last_error = $3 WHERE handle = $1",
                self.table
            );
            sqlx::query(&query)
                .bind(handle)
                .bind(Utc::now().timestamp_micros())
                .bind(error_message)
                .execute(&self.pool)
                .await?;
            return Ok(());
        }
        let backoff_seconds = compute_retry_delay_seconds(attempts);
        let next_attempt_micros = Utc::now().timestamp_micros() + backoff_seconds * 1_000_000;
        let query = format!(
            "UPDATE {} SET claim_until_micros = $2, last_error = $3 WHERE handle = $1",
            self.table
        );
        sqlx::query(&query)
            .bind(handle)
            .bind(next_attempt_micros)
            .bind(error_message)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn validate_identifier(value: &str) -> Result<()> {
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        anyhow::bail!("invalid identifier: {}", value);
    }
    Ok(())
}

fn compute_retry_delay_seconds(attempts: i32) -> i64 {
    let exponent = attempts.saturating_sub(1).clamp(0, 10) as u32;
    (INITIAL_RETRY_DELAY_SECONDS * (1_i64 << exponent)).min(MAX_RETRY_DELAY_SECONDS)
}

#[async_trait::async_trait]
impl SchedulerBackend for LocalPostgresSchedulerBackend {
    async fn schedule(&self, req: ScheduleWakeupRequest) -> Result<ScheduledWakeup> {
        let handle = crate::control::uuid::scheduler_handle();
        let query = format!(
            "INSERT INTO {} (handle, namespace, schedule_id, revision, fire_at_micros, payload, created_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            self.table
        );
        sqlx::query(&query)
            .bind(&handle)
            .bind(&req.namespace)
            .bind(&req.schedule_id)
            .bind(req.revision as i64)
            .bind(req.fire_at.timestamp_micros())
            .bind(&req.payload)
            .bind(Utc::now().timestamp_micros())
            .execute(&self.pool)
            .await?;
        Ok(ScheduledWakeup {
            handle: Some(handle),
            armed: true,
        })
    }

    async fn cancel(&self, handle: &str) -> Result<()> {
        let query = format!(
            "UPDATE {} SET canceled_at_micros = $2, claim_until_micros = NULL WHERE handle = $1 AND delivered_at_micros IS NULL",
            self.table
        );
        sqlx::query(&query)
            .bind(handle)
            .bind(Utc::now().timestamp_micros())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{docker_test_guard, PostgresContainer};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::{oneshot, Mutex};

    async fn init_test_backend(database_url: &str) -> LocalPostgresSchedulerBackend {
        let mut last_error = None;
        for _ in 0..20 {
            match LocalPostgresSchedulerBackend::new(
                database_url,
                Some("talon_scheduler_test".to_string()),
                None,
                None,
                false,
            )
            .await
            {
                Ok(backend) => return backend,
                Err(err) => {
                    last_error = Some(err);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
        panic!(
            "backend should initialize: {}",
            last_error.expect("expected initialization error")
        );
    }

    async fn init_backend_with_options(
        database_url: &str,
        table: Option<String>,
        runner_target_url: Option<String>,
        auth_token: Option<String>,
        runner_enabled: bool,
    ) -> LocalPostgresSchedulerBackend {
        let mut last_error = None;
        for _ in 0..20 {
            match LocalPostgresSchedulerBackend::new(
                database_url,
                table.clone(),
                runner_target_url.clone(),
                auth_token.clone(),
                runner_enabled,
            )
            .await
            {
                Ok(backend) => return backend,
                Err(err) => {
                    last_error = Some(err);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
        panic!(
            "backend should initialize: {}",
            last_error.expect("expected initialization error")
        );
    }

    #[derive(Clone, Default)]
    struct ReceivedWakeup {
        header: Option<String>,
        body: Vec<u8>,
    }

    #[test]
    fn validate_identifier_rejects_invalid_names_and_retry_delay_caps() {
        assert!(validate_identifier("valid_table_01").is_ok());
        assert!(validate_identifier("").is_err());
        assert!(validate_identifier("bad-name").is_err());
        assert!(validate_identifier("bad name").is_err());

        assert_eq!(compute_retry_delay_seconds(0), 5);
        assert_eq!(compute_retry_delay_seconds(1), 5);
        assert_eq!(compute_retry_delay_seconds(2), 10);
        assert_eq!(compute_retry_delay_seconds(3), 20);
        assert_eq!(compute_retry_delay_seconds(20), 300);
    }

    #[tokio::test]
    async fn schedule_claim_fail_cancel_and_deliver_round_trip() {
        let _guard = docker_test_guard();
        let pg = PostgresContainer::start("talon-rust-pg");
        let backend = init_test_backend(&pg.database_url()).await;

        let scheduled = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "schedule-1".to_string(),
                revision: 7,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"ok":true}"#.to_vec(),
            })
            .await
            .expect("schedule should succeed");
        let handle = scheduled.handle.expect("handle should be present");

        let wakeups = backend
            .claim_due_wakeups(10)
            .await
            .expect("claim should succeed");
        assert_eq!(wakeups.len(), 1);
        assert_eq!(wakeups[0].handle, handle);
        assert_eq!(wakeups[0].namespace, "acme");
        assert_eq!(wakeups[0].schedule_id, "schedule-1");
        assert_eq!(wakeups[0].revision, 7);
        assert_eq!(wakeups[0].attempts, 1);

        backend
            .mark_delivery_failed(&handle, 1, "transient")
            .await
            .expect("mark failure should succeed");
        let failure_row = sqlx::query(&format!(
            "SELECT last_error, claim_until_micros FROM {} WHERE handle = $1",
            backend.table
        ))
        .bind(&handle)
        .fetch_one(&backend.pool)
        .await
        .expect("failed row should load");
        let last_error: String = failure_row.try_get("last_error").unwrap();
        let claim_until_micros: i64 = failure_row.try_get("claim_until_micros").unwrap();
        assert_eq!(last_error, "transient");
        assert!(claim_until_micros > Utc::now().timestamp_micros());

        backend
            .mark_delivery_failed(&handle, MAX_DELIVERY_ATTEMPTS, "permanent")
            .await
            .expect("terminal failure should succeed");
        let canceled_row = sqlx::query(&format!(
            "SELECT canceled_at_micros, claim_until_micros, last_error FROM {} WHERE handle = $1",
            backend.table
        ))
        .bind(&handle)
        .fetch_one(&backend.pool)
        .await
        .expect("canceled row should load");
        let canceled_at: i64 = canceled_row.try_get("canceled_at_micros").unwrap();
        let canceled_claim_until: Option<i64> = canceled_row.try_get("claim_until_micros").unwrap();
        let canceled_error: String = canceled_row.try_get("last_error").unwrap();
        assert!(canceled_at > 0);
        assert!(canceled_claim_until.is_none());
        assert_eq!(canceled_error, "permanent");

        let scheduled_deliver = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "schedule-2".to_string(),
                revision: 8,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"deliver":true}"#.to_vec(),
            })
            .await
            .expect("second schedule should succeed");
        let deliver_handle = scheduled_deliver.handle.expect("deliver handle");

        let wakeup = backend
            .claim_due_wakeups(10)
            .await
            .expect("second claim should succeed")
            .into_iter()
            .find(|wakeup| wakeup.handle == deliver_handle)
            .expect("expected deliver wakeup");

        let received = Arc::new(Mutex::new(None::<ReceivedWakeup>));
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().unwrap();
        let received_state = received.clone();
        let server = tokio::spawn(async move {
            tokio::select! {
                _ = async {
                    let (mut socket, _) = listener.accept().await.expect("accept should succeed");
                    let mut buffer = vec![0_u8; 4096];
                    let bytes_read = socket.read(&mut buffer).await.expect("read should succeed");
                    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
                    let expected_prefix = format!("{}:", SCHEDULER_AUTH_HEADER).to_ascii_lowercase();
                    let header = request.lines().find_map(|line| {
                        let lower = line.to_ascii_lowercase();
                        lower
                            .strip_prefix(&expected_prefix)
                            .map(|_| line.split_once(':').map(|(_, value)| value.trim().to_string()))
                            .flatten()
                    });
                    let body = request
                        .split("\r\n\r\n")
                        .nth(1)
                        .unwrap_or_default()
                        .as_bytes()
                        .to_vec();
                    *received_state.lock().await = Some(ReceivedWakeup { header, body });
                    socket
                        .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
                        .await
                        .expect("write should succeed");
                } => {}
                _ = shutdown_rx => {}
            }
        });

        let client = Client::builder().build().unwrap();
        backend
            .deliver_wakeup(
                &client,
                &format!("http://{}/", addr),
                Some("secret-token"),
                &wakeup,
            )
            .await
            .expect("delivery should succeed");
        let _ = shutdown_tx.send(());
        server.await.expect("server task should finish");

        let received = received
            .lock()
            .await
            .clone()
            .expect("request should be captured");
        assert_eq!(received.header.as_deref(), Some("secret-token"));
        assert_eq!(received.body, br#"{"deliver":true}"#.to_vec());

        let delivered_row = sqlx::query(&format!(
            "SELECT delivered_at_micros FROM {} WHERE handle = $1",
            backend.table
        ))
        .bind(&deliver_handle)
        .fetch_one(&backend.pool)
        .await
        .expect("delivered row should load");
        let delivered_at: i64 = delivered_row.try_get("delivered_at_micros").unwrap();
        assert!(delivered_at > 0);

        let scheduled_cancel = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "schedule-3".to_string(),
                revision: 9,
                fire_at: Utc::now() + chrono::Duration::seconds(60),
                payload: br#"{"cancel":true}"#.to_vec(),
            })
            .await
            .expect("cancel schedule should succeed");
        let cancel_handle = scheduled_cancel.handle.expect("cancel handle");
        backend
            .cancel(&cancel_handle)
            .await
            .expect("cancel should succeed");
        let cancel_row = sqlx::query(&format!(
            "SELECT canceled_at_micros FROM {} WHERE handle = $1",
            backend.table
        ))
        .bind(&cancel_handle)
        .fetch_one(&backend.pool)
        .await
        .expect("cancel row should load");
        let canceled_at: i64 = cancel_row.try_get("canceled_at_micros").unwrap();
        assert!(canceled_at > 0);
    }

    #[tokio::test]
    async fn claim_due_wakeups_skips_future_canceled_delivered_and_claimed_rows() {
        let _guard = docker_test_guard();
        let pg = PostgresContainer::start("talon-rust-pg");
        let backend = init_test_backend(&pg.database_url()).await;

        let active = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "active".to_string(),
                revision: 1,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"active":true}"#.to_vec(),
            })
            .await
            .expect("active schedule should succeed")
            .handle
            .expect("active handle");
        let future = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "future".to_string(),
                revision: 2,
                fire_at: Utc::now() + chrono::Duration::seconds(60),
                payload: br#"{"future":true}"#.to_vec(),
            })
            .await
            .expect("future schedule should succeed")
            .handle
            .expect("future handle");
        let canceled = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "canceled".to_string(),
                revision: 3,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"canceled":true}"#.to_vec(),
            })
            .await
            .expect("canceled schedule should succeed")
            .handle
            .expect("canceled handle");
        let delivered = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "delivered".to_string(),
                revision: 4,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"delivered":true}"#.to_vec(),
            })
            .await
            .expect("delivered schedule should succeed")
            .handle
            .expect("delivered handle");
        let claimed = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "claimed".to_string(),
                revision: 5,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"claimed":true}"#.to_vec(),
            })
            .await
            .expect("claimed schedule should succeed")
            .handle
            .expect("claimed handle");

        backend
            .cancel(&canceled)
            .await
            .expect("cancel should succeed");
        backend
            .mark_delivered(&delivered)
            .await
            .expect("mark delivered should succeed");
        sqlx::query(&format!(
            "UPDATE {} SET claim_until_micros = $2 WHERE handle = $1",
            backend.table
        ))
        .bind(&claimed)
        .bind(Utc::now().timestamp_micros() + 60_000_000)
        .execute(&backend.pool)
        .await
        .expect("claim lease update should succeed");

        let wakeups = backend
            .claim_due_wakeups(10)
            .await
            .expect("claim should succeed");
        let handles: Vec<_> = wakeups.into_iter().map(|w| w.handle).collect();
        assert_eq!(handles, vec![active]);

        for skipped in [future, canceled, delivered, claimed] {
            assert!(!handles.contains(&skipped));
        }
    }

    #[tokio::test]
    async fn deliver_wakeup_surfaces_http_error_without_marking_delivered() {
        let _guard = docker_test_guard();
        let pg = PostgresContainer::start("talon-rust-pg");
        let backend = init_test_backend(&pg.database_url()).await;

        let handle = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "error".to_string(),
                revision: 11,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"error":true}"#.to_vec(),
            })
            .await
            .expect("schedule should succeed")
            .handle
            .expect("handle");
        let wakeup = backend
            .claim_due_wakeups(1)
            .await
            .expect("claim should succeed")
            .into_iter()
            .find(|wakeup| wakeup.handle == handle)
            .expect("expected wakeup");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("addr should exist");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept should succeed");
            let mut buffer = vec![0_u8; 4096];
            let _ = socket.read(&mut buffer).await.expect("read should succeed");
            socket
                .write_all(b"HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\n\r\n")
                .await
                .expect("write should succeed");
        });

        let client = Client::builder().build().expect("client should build");
        let err = backend
            .deliver_wakeup(&client, &format!("http://{}/", addr), None, &wakeup)
            .await
            .expect_err("delivery should fail");
        assert!(err.to_string().contains("500"));
        server.await.expect("server task should finish");

        let row = sqlx::query(&format!(
            "SELECT delivered_at_micros, attempts FROM {} WHERE handle = $1",
            backend.table
        ))
        .bind(&handle)
        .fetch_one(&backend.pool)
        .await
        .expect("row should load");
        let delivered_at: Option<i64> = row.try_get("delivered_at_micros").unwrap();
        let attempts: i32 = row.try_get("attempts").unwrap();
        assert!(delivered_at.is_none());
        assert_eq!(attempts, 1);
    }

    #[tokio::test]
    async fn new_accepts_blank_runner_target_when_runner_disabled_and_rejects_bad_identifier() {
        let _guard = docker_test_guard();
        let pg = PostgresContainer::start("talon-rust-pg");

        let backend = init_backend_with_options(
            &pg.database_url(),
            None,
            Some("   ".to_string()),
            Some("secret".to_string()),
            false,
        )
        .await;
        assert_eq!(backend.table, DEFAULT_TABLE);

        let err = LocalPostgresSchedulerBackend::new(
            &pg.database_url(),
            Some("bad-table-name".to_string()),
            None,
            None,
            false,
        )
        .await
        .err()
        .expect("invalid table name should fail");
        assert!(err.to_string().contains("invalid identifier"));
    }

    #[tokio::test]
    async fn runner_enabled_delivers_due_wakeup_to_target() {
        let _guard = docker_test_guard();
        let pg = PostgresContainer::start("talon-rust-pg");

        let received = Arc::new(Mutex::new(None::<ReceivedWakeup>));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("listener addr");
        let received_state = received.clone();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept should succeed");
            let mut buffer = vec![0_u8; 4096];
            let bytes_read = socket.read(&mut buffer).await.expect("read should succeed");
            let request = String::from_utf8_lossy(&buffer[..bytes_read]);
            let expected_prefix = format!("{}:", SCHEDULER_AUTH_HEADER).to_ascii_lowercase();
            let header = request.lines().find_map(|line| {
                let lower = line.to_ascii_lowercase();
                lower.strip_prefix(&expected_prefix).and_then(|_| {
                    line.split_once(':')
                        .map(|(_, value)| value.trim().to_string())
                })
            });
            let body = request
                .split("\r\n\r\n")
                .nth(1)
                .unwrap_or_default()
                .as_bytes()
                .to_vec();
            *received_state.lock().await = Some(ReceivedWakeup { header, body });
            socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
                .await
                .expect("write should succeed");
        });

        let backend = init_backend_with_options(
            &pg.database_url(),
            Some("talon_scheduler_runner_test".to_string()),
            Some(format!("http://{}/", addr)),
            Some("runner-secret".to_string()),
            true,
        )
        .await;

        let scheduled = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "runner-schedule".to_string(),
                revision: 12,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"runner":true}"#.to_vec(),
            })
            .await
            .expect("schedule should succeed");
        let handle = scheduled.handle.expect("handle should be present");

        for _ in 0..40 {
            if received.lock().await.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let captured = received
            .lock()
            .await
            .clone()
            .expect("runner should deliver wakeup");
        assert_eq!(captured.header.as_deref(), Some("runner-secret"));
        assert_eq!(captured.body, br#"{"runner":true}"#.to_vec());
        server.await.expect("server should finish");

        let row = sqlx::query(&format!(
            "SELECT delivered_at_micros FROM {} WHERE handle = $1",
            backend.table
        ))
        .bind(&handle)
        .fetch_one(&backend.pool)
        .await
        .expect("delivered row should load");
        let delivered_at: i64 = row.try_get("delivered_at_micros").unwrap();
        assert!(delivered_at > 0);
    }
}
