// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use chrono::Utc;
use futures::stream::{FuturesUnordered, StreamExt};
use reqwest::Client;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Row, SqlitePool,
};
use std::{str::FromStr, time::Duration};
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};

use super::{ScheduleWakeupRequest, ScheduledWakeup, SchedulerBackend, SCHEDULER_AUTH_HEADER};
use crate::control::kv::validate_identifier;

const DEFAULT_TABLE: &str = "talon_local_scheduler_jobs";
const CLAIM_TIMEOUT_SECONDS: i64 = 60;
const DELIVERY_TIMEOUT_SECONDS: u64 = 15;
const POLL_INTERVAL_MILLIS: u64 = 1_000;
const MAX_CONCURRENT_DELIVERIES: usize = 10;
const MAX_DELIVERY_ATTEMPTS: i32 = 20;
const INITIAL_RETRY_DELAY_SECONDS: i64 = 5;
const MAX_RETRY_DELAY_SECONDS: i64 = 300;
const MAX_POOL_CONNECTIONS: u32 = 5;

pub struct LocalSqliteSchedulerBackend {
    store: LocalSqliteSchedulerStore,
    runner_shutdown: CancellationToken,
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

#[derive(Clone)]
struct LocalSqliteSchedulerStore {
    pool: SqlitePool,
    table: String,
}

impl LocalSqliteSchedulerBackend {
    pub async fn new(
        database_url: &str,
        table: Option<String>,
        runner_target_url: Option<String>,
        auth_token: Option<String>,
        runner_enabled: bool,
    ) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(MAX_POOL_CONNECTIONS)
            .connect_with(options)
            .await?;
        let table = table.unwrap_or_else(|| DEFAULT_TABLE.to_string());
        let runner_shutdown = CancellationToken::new();
        validate_identifier(&table)?;
        let store = LocalSqliteSchedulerStore { pool, table };
        let create_stmt = format!(
            "CREATE TABLE IF NOT EXISTS {table} (
                handle TEXT PRIMARY KEY,
                namespace TEXT NOT NULL,
                schedule_id TEXT NOT NULL,
                revision INTEGER NOT NULL,
                fire_at_micros INTEGER NOT NULL,
                payload BLOB NOT NULL,
                canceled_at_micros INTEGER NULL,
                delivered_at_micros INTEGER NULL,
                claimed_at_micros INTEGER NULL,
                claim_until_micros INTEGER NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                last_error TEXT NULL,
                created_at_micros INTEGER NOT NULL
            )",
            table = store.table
        );
        sqlx::query(&create_stmt).execute(&store.pool).await?;
        let index_stmt = format!(
            "CREATE INDEX IF NOT EXISTS {table}_due_idx ON {table} (fire_at_micros) WHERE canceled_at_micros IS NULL AND delivered_at_micros IS NULL",
            table = store.table
        );
        sqlx::query(&index_stmt).execute(&store.pool).await?;

        if runner_enabled {
            let Some(target_url) = runner_target_url.filter(|value| !value.trim().is_empty())
            else {
                warn!("local_sqlite scheduler runner enabled without target URL; wakeups will not fire");
                return Ok(Self {
                    store,
                    runner_shutdown,
                });
            };
            let runner_store = store.clone();
            let shutdown = runner_shutdown.child_token();
            tokio::spawn(async move {
                runner_store
                    .run_loop(
                        target_url,
                        auth_token.filter(|value| !value.trim().is_empty()),
                        shutdown,
                    )
                    .await;
            });
        }

        Ok(Self {
            store,
            runner_shutdown,
        })
    }

    #[cfg(test)]
    async fn claim_due_wakeups(&self, limit: usize) -> Result<Vec<DueWakeup>> {
        self.store.claim_due_wakeups(limit).await
    }

    #[cfg(test)]
    async fn deliver_wakeup(
        &self,
        client: &Client,
        target_url: &str,
        auth_token: Option<&str>,
        wakeup: &DueWakeup,
    ) -> Result<()> {
        self.store
            .deliver_wakeup(client, target_url, auth_token, wakeup)
            .await
    }

    #[cfg(test)]
    async fn mark_delivered(&self, handle: &str) -> Result<()> {
        self.store.mark_delivered(handle).await
    }

    #[cfg(test)]
    async fn mark_delivery_failed(
        &self,
        handle: &str,
        attempts: i32,
        error_message: &str,
    ) -> Result<()> {
        self.store
            .mark_delivery_failed(handle, attempts, error_message)
            .await
    }
}

impl LocalSqliteSchedulerStore {
    async fn run_loop(
        self,
        target_url: String,
        auth_token: Option<String>,
        shutdown: CancellationToken,
    ) {
        let client = Client::builder()
            .timeout(Duration::from_secs(DELIVERY_TIMEOUT_SECONDS))
            .build()
            .expect("failed to build local scheduler client");
        loop {
            if shutdown.is_cancelled() {
                return;
            }
            let mut has_more = false;
            match self.claim_due_wakeups(MAX_CONCURRENT_DELIVERIES).await {
                Ok(wakeups) => {
                    if !wakeups.is_empty() {
                        tracing::info!(
                            claimed_wakeups = wakeups.len(),
                            target_url = %target_url,
                            "local_sqlite scheduler claimed due wakeups"
                        );
                    }
                    has_more = wakeups.len() == MAX_CONCURRENT_DELIVERIES;
                    let mut deliveries = FuturesUnordered::new();
                    for wakeup in wakeups {
                        let store = self.clone();
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
                                store.deliver_wakeup(
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
                                store,
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
                        store,
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
                            if let Err(mark_err) = store
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
                    error!(error = %err, "local_sqlite scheduler poll failed");
                }
            }
            if !has_more {
                tokio::select! {
                    _ = shutdown.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)) => {}
                }
            }
        }
    }

    async fn claim_due_wakeups(&self, limit: usize) -> Result<Vec<DueWakeup>> {
        let now_micros = Utc::now().timestamp_micros();
        let query = format!(
            "WITH due AS (
                SELECT handle
                FROM {table}
                WHERE canceled_at_micros IS NULL
                  AND delivered_at_micros IS NULL
                  AND attempts < ?2
                  AND fire_at_micros <= ?1
                  AND (claim_until_micros IS NULL OR claim_until_micros < ?1)
                ORDER BY fire_at_micros
                LIMIT ?3
            )
            UPDATE {table}
            SET claimed_at_micros = ?1,
                claim_until_micros = ?4,
                attempts = attempts + 1
            WHERE handle IN (SELECT handle FROM due)
              AND canceled_at_micros IS NULL
              AND delivered_at_micros IS NULL
              AND attempts < ?2
              AND fire_at_micros <= ?1
              AND (claim_until_micros IS NULL OR claim_until_micros < ?1)
            RETURNING handle, namespace, schedule_id, revision, fire_at_micros, payload, attempts",
            table = self.table
        );
        let claim_until_micros = now_micros + CLAIM_TIMEOUT_SECONDS * 1_000_000;
        let rows = sqlx::query(&query)
            .bind(now_micros)
            .bind(MAX_DELIVERY_ATTEMPTS)
            .bind(limit as i64)
            .bind(claim_until_micros)
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
            "UPDATE {} SET delivered_at_micros = ?2, claim_until_micros = NULL WHERE handle = ?1",
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
                "UPDATE {} SET canceled_at_micros = ?2, claim_until_micros = NULL, last_error = ?3 WHERE handle = ?1",
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
            "UPDATE {} SET claim_until_micros = ?2, last_error = ?3 WHERE handle = ?1",
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

    async fn schedule(&self, req: ScheduleWakeupRequest) -> Result<ScheduledWakeup> {
        let handle = uuid::Uuid::now_v7().to_string();
        let query = format!(
            "INSERT INTO {} (handle, namespace, schedule_id, revision, fire_at_micros, payload, created_at_micros)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
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
            "UPDATE {} SET canceled_at_micros = ?2, claim_until_micros = NULL WHERE handle = ?1 AND delivered_at_micros IS NULL",
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

fn compute_retry_delay_seconds(attempts: i32) -> i64 {
    let exponent = attempts.saturating_sub(1).clamp(0, 10) as u32;
    (INITIAL_RETRY_DELAY_SECONDS * (1_i64 << exponent)).min(MAX_RETRY_DELAY_SECONDS)
}

#[async_trait::async_trait]
impl SchedulerBackend for LocalSqliteSchedulerBackend {
    async fn schedule(&self, req: ScheduleWakeupRequest) -> Result<ScheduledWakeup> {
        self.store.schedule(req).await
    }

    async fn cancel(&self, handle: &str) -> Result<()> {
        self.store.cancel(handle).await
    }
}

impl Drop for LocalSqliteSchedulerBackend {
    fn drop(&mut self) {
        self.runner_shutdown.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::kv::sqlite_url_for_path;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::{oneshot, Mutex};

    async fn init_test_backend() -> LocalSqliteSchedulerBackend {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("scheduler.db");
        LocalSqliteSchedulerBackend::new(
            &sqlite_url_for_path(&db_path),
            Some("talon_scheduler_test".to_string()),
            None,
            None,
            false,
        )
        .await
        .unwrap()
    }

    async fn init_backend_with_options(
        table: Option<String>,
        runner_target_url: Option<String>,
        auth_token: Option<String>,
        runner_enabled: bool,
    ) -> LocalSqliteSchedulerBackend {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("scheduler.db");
        LocalSqliteSchedulerBackend::new(
            &sqlite_url_for_path(&db_path),
            table,
            runner_target_url,
            auth_token,
            runner_enabled,
        )
        .await
        .unwrap()
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
        let backend = init_test_backend().await;

        let scheduled = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "schedule-1".to_string(),
                revision: 7,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"ok":true}"#.to_vec(),
            })
            .await
            .unwrap();
        let handle = scheduled.handle.unwrap();

        let wakeups = backend.claim_due_wakeups(10).await.unwrap();
        assert_eq!(wakeups.len(), 1);
        assert_eq!(wakeups[0].handle, handle);
        assert_eq!(wakeups[0].attempts, 1);

        backend
            .mark_delivery_failed(&handle, 1, "transient")
            .await
            .unwrap();
        let failure_row = sqlx::query(&format!(
            "SELECT last_error, claim_until_micros FROM {} WHERE handle = ?1",
            backend.store.table
        ))
        .bind(&handle)
        .fetch_one(&backend.store.pool)
        .await
        .unwrap();
        let last_error: String = failure_row.try_get("last_error").unwrap();
        let claim_until_micros: i64 = failure_row.try_get("claim_until_micros").unwrap();
        assert_eq!(last_error, "transient");
        assert!(claim_until_micros > Utc::now().timestamp_micros());

        backend
            .mark_delivery_failed(&handle, MAX_DELIVERY_ATTEMPTS, "permanent")
            .await
            .unwrap();
        let canceled_row = sqlx::query(&format!(
            "SELECT canceled_at_micros, claim_until_micros, last_error FROM {} WHERE handle = ?1",
            backend.store.table
        ))
        .bind(&handle)
        .fetch_one(&backend.store.pool)
        .await
        .unwrap();
        let canceled_at: i64 = canceled_row.try_get("canceled_at_micros").unwrap();
        let canceled_claim_until: Option<i64> = canceled_row.try_get("claim_until_micros").unwrap();
        let canceled_error: String = canceled_row.try_get("last_error").unwrap();
        assert!(canceled_at > 0);
        assert!(canceled_claim_until.is_none());
        assert_eq!(canceled_error, "permanent");
    }

    #[tokio::test]
    async fn deliver_wakeup_surfaces_http_error_without_marking_delivered() {
        let backend = init_test_backend().await;

        let handle = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "error".to_string(),
                revision: 11,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"error":true}"#.to_vec(),
            })
            .await
            .unwrap()
            .handle
            .unwrap();
        let wakeup = backend
            .claim_due_wakeups(1)
            .await
            .unwrap()
            .into_iter()
            .find(|wakeup| wakeup.handle == handle)
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0_u8; 4096];
            let _ = socket.read(&mut buffer).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\n\r\n")
                .await
                .unwrap();
        });

        let client = Client::builder().build().unwrap();
        let err = backend
            .deliver_wakeup(&client, &format!("http://{}/", addr), None, &wakeup)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("500"));
        server.await.unwrap();

        let row = sqlx::query(&format!(
            "SELECT delivered_at_micros, attempts FROM {} WHERE handle = ?1",
            backend.store.table
        ))
        .bind(&handle)
        .fetch_one(&backend.store.pool)
        .await
        .unwrap();
        let delivered_at: Option<i64> = row.try_get("delivered_at_micros").unwrap();
        let attempts: i32 = row.try_get("attempts").unwrap();
        assert!(delivered_at.is_none());
        assert_eq!(attempts, 1);
    }

    #[tokio::test]
    async fn new_accepts_blank_runner_target_when_runner_disabled_and_rejects_bad_identifier() {
        let backend = init_backend_with_options(
            None,
            Some("   ".to_string()),
            Some("secret".to_string()),
            false,
        )
        .await;
        assert_eq!(backend.store.table, DEFAULT_TABLE);

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("scheduler.db");
        let err = LocalSqliteSchedulerBackend::new(
            &sqlite_url_for_path(&db_path),
            Some("bad-table-name".to_string()),
            None,
            None,
            false,
        )
        .await
        .err()
        .unwrap();
        assert!(err.to_string().contains("Invalid table name"));
    }

    #[tokio::test]
    async fn runner_enabled_delivers_due_wakeup_to_target() {
        let received = Arc::new(Mutex::new(None::<ReceivedWakeup>));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let received_state = received.clone();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0_u8; 4096];
            let bytes_read = socket.read(&mut buffer).await.unwrap();
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
                .unwrap();
        });

        let backend = init_backend_with_options(
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
            .unwrap();
        let handle = scheduled.handle.unwrap();

        for _ in 0..40 {
            if received.lock().await.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let captured = received.lock().await.clone().unwrap();
        assert_eq!(captured.header.as_deref(), Some("runner-secret"));
        assert_eq!(captured.body, br#"{"runner":true}"#.to_vec());
        server.await.unwrap();

        let row = sqlx::query(&format!(
            "SELECT delivered_at_micros FROM {} WHERE handle = ?1",
            backend.store.table
        ))
        .bind(&handle)
        .fetch_one(&backend.store.pool)
        .await
        .unwrap();
        let delivered_at: i64 = row.try_get("delivered_at_micros").unwrap();
        assert!(delivered_at > 0);
    }

    #[tokio::test]
    async fn claim_due_wakeups_skips_future_canceled_delivered_and_claimed_rows() {
        let backend = init_test_backend().await;

        let active = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "active".to_string(),
                revision: 1,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"active":true}"#.to_vec(),
            })
            .await
            .unwrap()
            .handle
            .unwrap();
        let future = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "future".to_string(),
                revision: 2,
                fire_at: Utc::now() + chrono::Duration::seconds(60),
                payload: br#"{"future":true}"#.to_vec(),
            })
            .await
            .unwrap()
            .handle
            .unwrap();
        let canceled = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "canceled".to_string(),
                revision: 3,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"canceled":true}"#.to_vec(),
            })
            .await
            .unwrap()
            .handle
            .unwrap();
        let delivered = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "delivered".to_string(),
                revision: 4,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"delivered":true}"#.to_vec(),
            })
            .await
            .unwrap()
            .handle
            .unwrap();
        let claimed = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "claimed".to_string(),
                revision: 5,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"claimed":true}"#.to_vec(),
            })
            .await
            .unwrap()
            .handle
            .unwrap();

        backend.cancel(&canceled).await.unwrap();
        backend.mark_delivered(&delivered).await.unwrap();
        sqlx::query(&format!(
            "UPDATE {} SET claim_until_micros = ?2 WHERE handle = ?1",
            backend.store.table
        ))
        .bind(&claimed)
        .bind(Utc::now().timestamp_micros() + 60_000_000)
        .execute(&backend.store.pool)
        .await
        .unwrap();

        let wakeups = backend.claim_due_wakeups(10).await.unwrap();
        let handles: Vec<_> = wakeups.into_iter().map(|w| w.handle).collect();
        assert_eq!(handles, vec![active]);

        for skipped in [future, canceled, delivered, claimed] {
            assert!(!handles.contains(&skipped));
        }
    }

    #[tokio::test]
    async fn claim_due_wakeups_honors_attempt_limit() {
        let backend = init_test_backend().await;
        let handle = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "exhausted".to_string(),
                revision: 6,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"exhausted":true}"#.to_vec(),
            })
            .await
            .unwrap()
            .handle
            .unwrap();

        sqlx::query(&format!(
            "UPDATE {} SET attempts = ?2 WHERE handle = ?1",
            backend.store.table
        ))
        .bind(&handle)
        .bind(MAX_DELIVERY_ATTEMPTS)
        .execute(&backend.store.pool)
        .await
        .unwrap();

        let wakeups = backend.claim_due_wakeups(10).await.unwrap();
        assert!(wakeups.is_empty());
    }

    #[tokio::test]
    async fn deliver_wakeup_sends_auth_header_and_marks_delivered() {
        let backend = init_test_backend().await;
        let handle = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "acme".to_string(),
                schedule_id: "deliver".to_string(),
                revision: 8,
                fire_at: Utc::now() - chrono::Duration::seconds(1),
                payload: br#"{"deliver":true}"#.to_vec(),
            })
            .await
            .unwrap()
            .handle
            .unwrap();
        let wakeup = backend
            .claim_due_wakeups(1)
            .await
            .unwrap()
            .into_iter()
            .find(|wakeup| wakeup.handle == handle)
            .unwrap();

        let received = Arc::new(Mutex::new(None::<ReceivedWakeup>));
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let received_state = received.clone();
        let server = tokio::spawn(async move {
            tokio::select! {
                _ = async {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut buffer = vec![0_u8; 4096];
                    let bytes_read = socket.read(&mut buffer).await.unwrap();
                    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
                    let expected_prefix = format!("{}:", SCHEDULER_AUTH_HEADER).to_ascii_lowercase();
                    let header = request.lines().find_map(|line| {
                        let lower = line.to_ascii_lowercase();
                        lower
                            .strip_prefix(&expected_prefix)
                            .and_then(|_| line.split_once(':').map(|(_, value)| value.trim().to_string()))
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
                        .unwrap();
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
            .unwrap();
        let _ = shutdown_tx.send(());
        server.await.unwrap();

        let received = received.lock().await.clone().unwrap();
        assert_eq!(received.header.as_deref(), Some("secret-token"));
        assert_eq!(received.body, br#"{"deliver":true}"#.to_vec());

        let delivered_row = sqlx::query(&format!(
            "SELECT delivered_at_micros FROM {} WHERE handle = ?1",
            backend.store.table
        ))
        .bind(&handle)
        .fetch_one(&backend.store.pool)
        .await
        .unwrap();
        let delivered_at: i64 = delivered_row.try_get("delivered_at_micros").unwrap();
        assert!(delivered_at > 0);
    }

    #[tokio::test]
    async fn runner_stops_when_backend_is_dropped() {
        let backend = init_test_backend().await;
        let shutdown = backend.runner_shutdown.child_token();
        let task = tokio::spawn(backend.store.clone().run_loop(
            "http://127.0.0.1:9/".to_string(),
            None,
            shutdown,
        ));

        drop(backend);
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    }
}
