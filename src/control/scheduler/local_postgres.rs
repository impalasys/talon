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
            let Some(target_url) = runner_target_url.filter(|value| !value.trim().is_empty()) else {
                warn!("local_postgres scheduler runner enabled without target URL; wakeups will not fire");
                return Ok(Self { pool, table });
            };
            let runner = Self {
                pool: pool.clone(),
                table: table.clone(),
            };
            tokio::spawn(async move {
                runner
                    .run_loop(target_url, auth_token.filter(|value| !value.trim().is_empty()))
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
                                anyhow!(
                                    "timed out delivering scheduler wakeup to {}",
                                    target_url
                                )
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
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
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
        let handle = uuid::Uuid::now_v7().to_string();
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
