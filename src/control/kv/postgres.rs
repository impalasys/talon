// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{
    keys::{ResourceKey, ResourceList},
    KeyValueStore, ListOptions, Order,
};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use sqlx::{
    pool::PoolConnection,
    postgres::{PgConnectOptions, PgPoolOptions},
    PgConnection, PgPool, Postgres, Row,
};
use std::{
    fmt,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::task::AbortOnDropHandle;
use tracing::{field, Instrument, Span};

use super::shared::{quoted_identifier, validate_identifier};

pub(crate) fn postgres_credential_provider(
    config: &crate::control::config::proto::DatabaseConfig,
) -> anyhow::Result<Arc<dyn CredentialProvider>> {
    match config.auth_mode.as_str() {
        "" | AUTH_MODE_PASSWORD => Ok(Arc::new(PasswordCredentialProvider)),
        AUTH_MODE_IAM => Ok(Arc::new(GcpMetadataCredentialProvider::new()?)),
        _ => Err(anyhow::anyhow!(
            "invalid PostgreSQL auth mode: expected password or iam"
        )),
    }
}

const AUTH_MODE_PASSWORD: &str = "password";
const AUTH_MODE_IAM: &str = "iam";

const IAM_TOKEN_REFRESH: Duration = Duration::from_secs(45 * 60);
const TOKEN_EXPIRY_MARGIN: Duration = Duration::from_secs(5 * 60);
const REFRESH_RETRY_INITIAL: Duration = Duration::from_secs(5);
const REFRESH_RETRY_MAX: Duration = Duration::from_secs(2 * 60);

fn next_refresh_retry(current: Duration) -> Duration {
    current
        .saturating_mul(2)
        .max(REFRESH_RETRY_INITIAL)
        .min(REFRESH_RETRY_MAX)
}

#[async_trait]
pub trait CredentialProvider: Send + Sync + 'static {
    async fn credential(&self, options: PgConnectOptions) -> Result<DatabaseCredential>;
}

pub struct PasswordCredentialProvider;

#[async_trait]
impl CredentialProvider for PasswordCredentialProvider {
    async fn credential(&self, options: PgConnectOptions) -> Result<DatabaseCredential> {
        Ok(DatabaseCredential {
            options,
            valid_for: None,
        })
    }
}

pub struct GcpMetadataCredentialProvider {
    client: reqwest::Client,
}

impl GcpMetadataCredentialProvider {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .context("failed to configure Cloud SQL metadata client")?;
        Ok(Self { client })
    }
}

#[async_trait::async_trait]
impl CredentialProvider for GcpMetadataCredentialProvider {
    async fn credential(&self, options: PgConnectOptions) -> Result<DatabaseCredential> {
        #[derive(serde::Deserialize)]
        struct MetadataToken {
            access_token: String,
            expires_in: u64,
        }

        let token: MetadataToken = self
            .client
            .get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token")
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| anyhow!("failed to obtain Cloud SQL IAM access token"))?
            .json()
            .await
            .map_err(|_| anyhow!("invalid Cloud SQL IAM metadata response"))?;
        Ok(DatabaseCredential {
            options: iam_options(options, &token.access_token),
            valid_for: Some(Duration::from_secs(token.expires_in)),
        })
    }
}

pub struct DatabaseCredential {
    pub options: PgConnectOptions,
    pub valid_for: Option<Duration>,
}

impl fmt::Debug for DatabaseCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DatabaseCredential([REDACTED])")
    }
}

impl DatabaseCredential {
    fn refresh_delay(&self) -> Option<Duration> {
        self.valid_for.map(|valid_for| {
            let before_expiry = valid_for.saturating_sub(TOKEN_EXPIRY_MARGIN);
            if before_expiry.is_zero() {
                (valid_for / 2).max(Duration::from_secs(1))
            } else {
                before_expiry.min(IAM_TOKEN_REFRESH)
            }
        })
    }
}

pub(crate) type CredentialRefresh = Arc<AbortOnDropHandle<()>>;

pub(crate) async fn connect_postgres_pool(
    url: &str,
    settings: &PostgresPoolSettings,
) -> Result<(PgPool, Option<CredentialRefresh>)> {
    let options = PgConnectOptions::from_str(url)
        .map_err(|_| anyhow!("invalid PostgreSQL connection options"))?;
    let credential = settings
        .credential_provider
        .credential(options)
        .await
        .map_err(|_| anyhow!("failed to obtain PostgreSQL credentials"))?;
    let refresh_after = credential.refresh_delay();
    let pool = PgPoolOptions::new()
        .max_connections(settings.max_connections)
        .connect_with(credential.options)
        .await
        .map_err(|_| anyhow!("failed to connect to PostgreSQL using configured credentials"))?;
    let refresh = refresh_after.map(|delay| {
        spawn_credential_refresh(
            pool.clone(),
            settings.credential_provider.clone(),
            delay,
            REFRESH_RETRY_INITIAL,
        )
    });
    Ok((pool, refresh))
}

fn spawn_credential_refresh(
    pool: PgPool,
    provider: Arc<dyn CredentialProvider>,
    mut refresh_after: Duration,
    retry_initial: Duration,
) -> CredentialRefresh {
    let mut close_event = pool.close_event();
    let task = tokio::spawn(async move {
        let _ = close_event
            .do_until(async move {
                let mut retry_after = retry_initial;
                loop {
                    tokio::time::sleep(refresh_after).await;
                    let options = pool.connect_options().as_ref().clone();
                    let credential = match provider.credential(options).await {
                        Ok(credential) => credential,
                        Err(_) => {
                            tracing::warn!("failed to refresh PostgreSQL credentials; will retry");
                            refresh_after = retry_after;
                            retry_after = next_refresh_retry(retry_after);
                            continue;
                        }
                    };
                    let next_refresh = credential.refresh_delay();
                    pool.set_connect_options(credential.options);
                    let Some(delay) = next_refresh else {
                        break;
                    };
                    refresh_after = delay;
                    retry_after = retry_initial;
                }
            })
            .await;
    });
    Arc::new(AbortOnDropHandle::new(task))
}

fn iam_options(options: PgConnectOptions, token: &str) -> PgConnectOptions {
    options.password(token)
}

fn create_table_statement(table: &str) -> String {
    let table = quoted_identifier(table);
    format!(
        "CREATE TABLE IF NOT EXISTS {} (
                namespace TEXT NOT NULL,
                parent_path TEXT NOT NULL,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                value BYTEA NOT NULL,
                PRIMARY KEY (namespace, parent_path, kind, name)
            )",
        table
    )
}

fn get_query(table: &str) -> String {
    format!(
        "SELECT value FROM {}
         WHERE namespace = $1 AND parent_path = $2 AND kind = $3 AND name = $4",
        quoted_identifier(table)
    )
}

fn set_query(table: &str) -> String {
    let table = quoted_identifier(table);
    format!(
        "INSERT INTO {} (namespace, parent_path, kind, name, value)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (namespace, parent_path, kind, name)
             DO UPDATE SET value = excluded.value",
        table
    )
}

fn compare_and_swap_query(table: &str, expected: bool) -> String {
    let table = quoted_identifier(table);
    if expected {
        format!(
            "UPDATE {} SET value = $5
             WHERE namespace = $1 AND parent_path = $2 AND kind = $3 AND name = $4 AND value = $6",
            table
        )
    } else {
        format!(
            "INSERT INTO {} (namespace, parent_path, kind, name, value)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (namespace, parent_path, kind, name) DO NOTHING",
            table
        )
    }
}

fn delete_query(table: &str) -> String {
    format!(
        "DELETE FROM {}
         WHERE namespace = $1 AND parent_path = $2 AND kind = $3 AND name = $4",
        quoted_identifier(table)
    )
}

fn list_order_sql(order: Order) -> &'static str {
    if order == Order::Desc {
        "DESC"
    } else {
        "ASC"
    }
}

fn list_filter_clause(has_kind: bool, options: ListOptions<'_>) -> String {
    let mut next_bind = if has_kind { 4 } else { 3 };
    let mut clauses = Vec::new();
    if has_kind {
        clauses.push("AND kind = $3".to_string());
    }
    if options.before_name.is_some() {
        clauses.push(format!("AND name < ${next_bind}"));
        next_bind += 1;
    }
    if options.after_name.is_some() {
        clauses.push(format!("AND name > ${next_bind}"));
    }
    clauses.join(" ")
}

fn list_limit_clause(has_kind: bool, options: ListOptions<'_>) -> String {
    let mut next_bind = if has_kind { 4 } else { 3 };
    if options.before_name.is_some() {
        next_bind += 1;
    }
    if options.after_name.is_some() {
        next_bind += 1;
    }
    if options.limit.is_some() {
        format!(" LIMIT ${next_bind}")
    } else {
        String::new()
    }
}

fn list_keys_query(table: &str, has_kind: bool, options: ListOptions<'_>) -> String {
    let filter_clause = list_filter_clause(has_kind, options);
    let limit_clause = list_limit_clause(has_kind, options);
    let direction = list_order_sql(options.order);
    format!(
        "SELECT namespace, parent_path, kind, name FROM {}
         WHERE namespace = $1 AND parent_path = $2 {filter_clause}
         ORDER BY kind {direction}, name {direction}{limit_clause}",
        quoted_identifier(table)
    )
}

fn list_entries_query(table: &str, has_kind: bool, options: ListOptions<'_>) -> String {
    let filter_clause = list_filter_clause(has_kind, options);
    let limit_clause = list_limit_clause(has_kind, options);
    let direction = list_order_sql(options.order);
    format!(
        "SELECT namespace, parent_path, kind, name, value FROM {}
         WHERE namespace = $1 AND parent_path = $2 {filter_clause}
         ORDER BY kind {direction}, name {direction}{limit_clause}",
        quoted_identifier(table)
    )
}

fn legacy_columns_query() -> &'static str {
    "SELECT column_name FROM information_schema.columns WHERE table_schema = current_schema() AND table_name = $1"
}

fn create_migration_table_statement(table: &str) -> String {
    create_table_statement(table).replacen("CREATE TABLE IF NOT EXISTS", "CREATE TABLE", 1)
}

fn rename_migration_index_statement(table: &str, migration_table: &str) -> String {
    format!(
        "ALTER INDEX IF EXISTS {} RENAME TO {}",
        quoted_identifier(&format!("{migration_table}_pkey")),
        quoted_identifier(&format!("{table}_pkey"))
    )
}

fn key_from_row(row: &sqlx::postgres::PgRow) -> Result<ResourceKey> {
    Ok(ResourceKey {
        namespace: row.try_get("namespace")?,
        parent_path: row.try_get("parent_path")?,
        kind: row.try_get("kind")?,
        name: row.try_get("name")?,
    })
}

fn insert_query(table: &str) -> String {
    format!(
        "INSERT INTO {} (namespace, parent_path, kind, name, value)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (namespace, parent_path, kind, name)
         DO UPDATE SET value = excluded.value",
        quoted_identifier(table)
    )
}

async fn migrate_legacy_table(conn: &mut PgConnection, table: &str) -> Result<()> {
    let rows = sqlx::query(legacy_columns_query())
        .bind(table)
        .fetch_all(&mut *conn)
        .await?;
    let columns = rows
        .iter()
        .filter_map(|row| row.try_get::<String, _>("column_name").ok())
        .collect::<std::collections::HashSet<_>>();
    let has_namespace = columns.contains("namespace");
    let has_parent_path = columns.contains("parent_path");
    let has_key = columns.contains("key");

    if has_namespace && has_parent_path {
        return Ok(());
    }
    if columns.is_empty() {
        return Ok(());
    }
    if !has_key {
        bail!("cannot migrate {table}: expected legacy key column");
    }

    let migration_table = format!("{table}_structured_key_migration");
    sqlx::query(&format!(
        "DROP TABLE IF EXISTS {}",
        quoted_identifier(&migration_table)
    ))
    .execute(&mut *conn)
    .await?;
    sqlx::query(&create_migration_table_statement(&migration_table))
        .execute(&mut *conn)
        .await?;

    let old_rows = if has_namespace {
        sqlx::query(&format!(
            "SELECT namespace, key, value FROM {}",
            quoted_identifier(table)
        ))
        .fetch_all(&mut *conn)
        .await?
    } else {
        sqlx::query(&format!(
            "SELECT key, value FROM {}",
            quoted_identifier(table)
        ))
        .fetch_all(&mut *conn)
        .await?
    };
    let insert = insert_query(&migration_table);
    for row in old_rows {
        let old_key: String = row.try_get("key")?;
        let value: Vec<u8> = row.try_get("value")?;
        let key = if has_namespace {
            let namespace: String = row.try_get("namespace")?;
            let canonical = super::legacy::namespaced_key(&namespace, &old_key)?;
            ResourceKey::parse_canonical(&canonical)?
        } else {
            ResourceKey::parse_canonical(&old_key)?
        };
        sqlx::query(&insert)
            .bind(&key.namespace)
            .bind(&key.parent_path)
            .bind(&key.kind)
            .bind(&key.name)
            .bind(value)
            .execute(&mut *conn)
            .await?;
    }

    sqlx::query(&format!("DROP TABLE {}", quoted_identifier(table)))
        .execute(&mut *conn)
        .await?;
    sqlx::query(&format!(
        "ALTER TABLE {} RENAME TO {}",
        quoted_identifier(&migration_table),
        quoted_identifier(table)
    ))
    .execute(&mut *conn)
    .await?;
    sqlx::query(&rename_migration_index_statement(table, &migration_table))
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub struct PostgresKvStore {
    pool: PgPool,
    _credential_refresh: Option<CredentialRefresh>,
    table: String,
    settings: PostgresPoolSettings,
}

#[derive(Clone)]
pub struct PostgresPoolSettings {
    pub max_connections: u32,
    pub credential_provider: Arc<dyn CredentialProvider>,
}

impl Default for PostgresPoolSettings {
    fn default() -> Self {
        Self {
            max_connections: 10,
            credential_provider: Arc::new(PasswordCredentialProvider),
        }
    }
}

impl PostgresPoolSettings {
    pub fn from_env() -> Self {
        let mut settings = Self::default();
        if let Some(max_connections) = std::env::var("TALON_POSTGRES_MAX_CONNECTIONS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value > 0)
        {
            settings.max_connections = max_connections;
        }
        settings
    }
}

impl PostgresKvStore {
    pub async fn new(url: &str, table: &str, settings: PostgresPoolSettings) -> Result<Self> {
        validate_identifier(table)?;
        let (pool, credential_refresh) = connect_postgres_pool(url, &settings).await?;
        let mut conn = pool.acquire().await?;
        let lock_key = format!("talon_kv_store:{table}:schema");
        sqlx::query("SELECT pg_advisory_lock(hashtext($1))")
            .bind(&lock_key)
            .execute(&mut *conn)
            .await?;
        migrate_legacy_table(&mut *conn, table).await?;
        let create_stmt = create_table_statement(table);
        sqlx::query(&create_stmt).execute(&mut *conn).await?;
        sqlx::query("SELECT pg_advisory_unlock(hashtext($1))")
            .bind(&lock_key)
            .execute(&mut *conn)
            .await?;
        drop(conn);

        Ok(Self {
            pool,
            _credential_refresh: credential_refresh,
            table: table.to_string(),
            settings,
        })
    }
}

async fn acquire_connection(
    pool: &PgPool,
    settings: &PostgresPoolSettings,
    parent_span: &Span,
) -> Result<PoolConnection<Postgres>> {
    let pool_size_before = pool.size();
    let pool_idle_before = pool.num_idle();
    parent_span.record("postgres.pool.size_before", u64::from(pool_size_before));
    parent_span.record("postgres.pool.idle_before", pool_idle_before as u64);

    let span = tracing::debug_span!(
        parent: parent_span,
        "PostgresKvStore.acquire_connection",
        "postgres.pool.max_connections" = u64::from(settings.max_connections),
        "postgres.pool.size_before" = u64::from(pool_size_before),
        "postgres.pool.idle_before" = pool_idle_before as u64,
        "postgres.pool.size_after" = field::Empty,
        "postgres.pool.idle_after" = field::Empty,
        pool_wait_us = field::Empty,
    );
    let started_at = Instant::now();
    let conn = pool.acquire().instrument(span.clone()).await?;
    let pool_wait_us = started_at.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
    let pool_size_after = pool.size();
    let pool_idle_after = pool.num_idle();

    span.record("pool_wait_us", pool_wait_us);
    span.record("postgres.pool.size_after", u64::from(pool_size_after));
    span.record("postgres.pool.idle_after", pool_idle_after as u64);
    parent_span.record("pool_wait_us", pool_wait_us);
    parent_span.record("postgres.pool.size_after", u64::from(pool_size_after));
    parent_span.record("postgres.pool.idle_after", pool_idle_after as u64);

    Ok(conn)
}

fn record_query_elapsed(span: &Span, parent_span: &Span, started_at: Instant) {
    let query_elapsed_us = started_at.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
    span.record("query_elapsed_us", query_elapsed_us);
    parent_span.record("query_elapsed_us", query_elapsed_us);
}

fn record_rows(span: &Span, parent_span: &Span, rows_returned: usize) {
    span.record("rows_returned", rows_returned as u64);
    parent_span.record("rows_returned", rows_returned as u64);
}

#[async_trait::async_trait]
impl KeyValueStore for PostgresKvStore {
    async fn get(&self, key: &ResourceKey) -> Result<Option<Vec<u8>>> {
        let query = get_query(&self.table);
        let span = tracing::debug_span!(
            "PostgresKvStore.get",
            "db.system" = "postgresql",
            "db.operation" = "get",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = %key.kind,
            "postgres.pool.max_connections" = u64::from(self.settings.max_connections),
            "postgres.pool.size_before" = field::Empty,
            "postgres.pool.idle_before" = field::Empty,
            "postgres.pool.size_after" = field::Empty,
            "postgres.pool.idle_after" = field::Empty,
            pool_wait_us = field::Empty,
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
            value_bytes = field::Empty,
        );
        let span_for_body = span.clone();
        async move {
            let mut conn = acquire_connection(&self.pool, &self.settings, &span_for_body).await?;
            let query_span = tracing::debug_span!(
                parent: &span_for_body,
                "PostgresKvStore.query",
                query_elapsed_us = field::Empty,
                rows_returned = field::Empty,
                value_bytes = field::Empty,
            );
            let query_started_at = Instant::now();
            let row = sqlx::query(&query)
                .bind(&key.namespace)
                .bind(&key.parent_path)
                .bind(&key.kind)
                .bind(&key.name)
                .fetch_optional(&mut *conn)
                .instrument(query_span.clone())
                .await?;
            record_query_elapsed(&query_span, &span_for_body, query_started_at);

            if let Some(row) = row {
                let value: Vec<u8> = row.try_get("value")?;
                query_span.record("value_bytes", value.len() as u64);
                span_for_body.record("value_bytes", value.len() as u64);
                record_rows(&query_span, &span_for_body, 1);
                Ok(Some(value))
            } else {
                record_rows(&query_span, &span_for_body, 0);
                Ok(None)
            }
        }
        .instrument(span)
        .await
    }

    async fn set(&self, key: &ResourceKey, value: &[u8]) -> Result<()> {
        let query = set_query(&self.table);
        let span = tracing::debug_span!(
            "PostgresKvStore.set",
            "db.system" = "postgresql",
            "db.operation" = "set",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = %key.kind,
            "postgres.pool.max_connections" = u64::from(self.settings.max_connections),
            "postgres.pool.size_before" = field::Empty,
            "postgres.pool.idle_before" = field::Empty,
            "postgres.pool.size_after" = field::Empty,
            "postgres.pool.idle_after" = field::Empty,
            pool_wait_us = field::Empty,
            query_elapsed_us = field::Empty,
            rows_affected = field::Empty,
            value_bytes = value.len(),
        );
        let span_for_body = span.clone();
        async move {
            let mut conn = acquire_connection(&self.pool, &self.settings, &span_for_body).await?;
            let query_span = tracing::debug_span!(
                parent: &span_for_body,
                "PostgresKvStore.query",
                query_elapsed_us = field::Empty,
                rows_affected = field::Empty,
            );
            let query_started_at = Instant::now();
            let result = sqlx::query(&query)
                .bind(&key.namespace)
                .bind(&key.parent_path)
                .bind(&key.kind)
                .bind(&key.name)
                .bind(value)
                .execute(&mut *conn)
                .instrument(query_span.clone())
                .await?;
            record_query_elapsed(&query_span, &span_for_body, query_started_at);
            query_span.record("rows_affected", result.rows_affected());
            span_for_body.record("rows_affected", result.rows_affected());
            Ok(())
        }
        .instrument(span)
        .await
    }

    async fn compare_and_swap(
        &self,
        key: &ResourceKey,
        expected: Option<&[u8]>,
        value: &[u8],
    ) -> Result<bool> {
        let query = compare_and_swap_query(&self.table, expected.is_some());
        let span = tracing::debug_span!(
            "PostgresKvStore.compare_and_swap",
            "db.system" = "postgresql",
            "db.operation" = "compare_and_swap",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = %key.kind,
            "postgres.pool.max_connections" = u64::from(self.settings.max_connections),
            "postgres.pool.size_before" = field::Empty,
            "postgres.pool.idle_before" = field::Empty,
            "postgres.pool.size_after" = field::Empty,
            "postgres.pool.idle_after" = field::Empty,
            pool_wait_us = field::Empty,
            query_elapsed_us = field::Empty,
            rows_affected = field::Empty,
            expected_present = expected.is_some(),
            value_bytes = value.len(),
        );

        let q = if let Some(expected) = expected {
            sqlx::query(&query)
                .bind(&key.namespace)
                .bind(&key.parent_path)
                .bind(&key.kind)
                .bind(&key.name)
                .bind(value)
                .bind(expected)
        } else {
            sqlx::query(&query)
                .bind(&key.namespace)
                .bind(&key.parent_path)
                .bind(&key.kind)
                .bind(&key.name)
                .bind(value)
        };
        let mut conn = acquire_connection(&self.pool, &self.settings, &span).await?;
        let query_span = tracing::debug_span!(
            parent: &span,
            "PostgresKvStore.query",
            query_elapsed_us = field::Empty,
            rows_affected = field::Empty,
        );
        let query_started_at = Instant::now();
        let rows_affected = q
            .execute(&mut *conn)
            .instrument(query_span.clone())
            .instrument(span.clone())
            .await?
            .rows_affected();
        record_query_elapsed(&query_span, &span, query_started_at);
        query_span.record("rows_affected", rows_affected);
        span.record("rows_affected", rows_affected);
        Ok(rows_affected == 1)
    }

    async fn delete(&self, key: &ResourceKey) -> Result<()> {
        let query = delete_query(&self.table);
        let span = tracing::debug_span!(
            "PostgresKvStore.delete",
            "db.system" = "postgresql",
            "db.operation" = "delete",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = %key.kind,
            "postgres.pool.max_connections" = u64::from(self.settings.max_connections),
            "postgres.pool.size_before" = field::Empty,
            "postgres.pool.idle_before" = field::Empty,
            "postgres.pool.size_after" = field::Empty,
            "postgres.pool.idle_after" = field::Empty,
            pool_wait_us = field::Empty,
            query_elapsed_us = field::Empty,
            rows_affected = field::Empty,
        );
        let mut conn = acquire_connection(&self.pool, &self.settings, &span).await?;
        let query_span = tracing::debug_span!(
            parent: &span,
            "PostgresKvStore.query",
            query_elapsed_us = field::Empty,
            rows_affected = field::Empty,
        );
        let query_started_at = Instant::now();
        let result = sqlx::query(&query)
            .bind(&key.namespace)
            .bind(&key.parent_path)
            .bind(&key.kind)
            .bind(&key.name)
            .execute(&mut *conn)
            .instrument(query_span.clone())
            .instrument(span.clone())
            .await?;
        record_query_elapsed(&query_span, &span, query_started_at);
        query_span.record("rows_affected", result.rows_affected());
        span.record("rows_affected", result.rows_affected());
        Ok(())
    }

    async fn list_keys(
        &self,
        list: &ResourceList,
        options: Option<ListOptions<'_>>,
    ) -> Result<Vec<ResourceKey>> {
        let options = options.unwrap_or_default();
        let query = list_keys_query(&self.table, list.kind.is_some(), options);
        let span = tracing::debug_span!(
            "PostgresKvStore.list_keys",
            "db.system" = "postgresql",
            "db.operation" = "list_keys",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = list.kind.as_deref().unwrap_or("*"),
            "postgres.pool.max_connections" = u64::from(self.settings.max_connections),
            "postgres.pool.size_before" = field::Empty,
            "postgres.pool.idle_before" = field::Empty,
            "postgres.pool.size_after" = field::Empty,
            "postgres.pool.idle_after" = field::Empty,
            pool_wait_us = field::Empty,
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
        );
        let mut query = sqlx::query(&query)
            .bind(&list.parent.namespace)
            .bind(&list.parent.parent_path);
        if let Some(kind) = &list.kind {
            query = query.bind(kind);
        }
        if let Some(before_name) = options.before_name {
            query = query.bind(before_name);
        }
        if let Some(after_name) = options.after_name {
            query = query.bind(after_name);
        }
        if let Some(limit) = options.limit {
            query = query.bind(limit as i64);
        }
        let mut conn = acquire_connection(&self.pool, &self.settings, &span).await?;
        let query_span = tracing::debug_span!(
            parent: &span,
            "PostgresKvStore.query",
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
        );
        let query_started_at = Instant::now();
        let rows = query
            .fetch_all(&mut *conn)
            .instrument(query_span.clone())
            .instrument(span.clone())
            .await?;
        record_query_elapsed(&query_span, &span, query_started_at);
        record_rows(&query_span, &span, rows.len());

        let mut keys = Vec::with_capacity(rows.len());
        for row in rows {
            keys.push(key_from_row(&row)?);
        }
        Ok(keys)
    }

    async fn list_entries(
        &self,
        list: &ResourceList,
        options: Option<ListOptions<'_>>,
    ) -> Result<Vec<(ResourceKey, Vec<u8>)>> {
        let options = options.unwrap_or_default();
        let query = list_entries_query(&self.table, list.kind.is_some(), options);
        let span = tracing::debug_span!(
            "PostgresKvStore.list_entries",
            "db.system" = "postgresql",
            "db.operation" = "list_entries",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = list.kind.as_deref().unwrap_or("*"),
            "postgres.pool.max_connections" = u64::from(self.settings.max_connections),
            "postgres.pool.size_before" = field::Empty,
            "postgres.pool.idle_before" = field::Empty,
            "postgres.pool.size_after" = field::Empty,
            "postgres.pool.idle_after" = field::Empty,
            pool_wait_us = field::Empty,
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
            value_bytes = field::Empty,
        );
        let mut query = sqlx::query(&query)
            .bind(&list.parent.namespace)
            .bind(&list.parent.parent_path);
        if let Some(kind) = &list.kind {
            query = query.bind(kind);
        }
        if let Some(before_name) = options.before_name {
            query = query.bind(before_name);
        }
        if let Some(after_name) = options.after_name {
            query = query.bind(after_name);
        }
        if let Some(limit) = options.limit {
            query = query.bind(limit as i64);
        }
        let mut conn = acquire_connection(&self.pool, &self.settings, &span).await?;
        let query_span = tracing::debug_span!(
            parent: &span,
            "PostgresKvStore.query",
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
            value_bytes = field::Empty,
        );
        let query_started_at = Instant::now();
        let rows = query
            .fetch_all(&mut *conn)
            .instrument(query_span.clone())
            .instrument(span.clone())
            .await?;
        record_query_elapsed(&query_span, &span, query_started_at);
        record_rows(&query_span, &span, rows.len());

        let mut entries = Vec::with_capacity(rows.len());
        let mut total_value_bytes = 0usize;
        for row in rows {
            let value: Vec<u8> = row.try_get("value")?;
            total_value_bytes += value.len();
            entries.push((key_from_row(&row)?, value));
        }
        query_span.record("value_bytes", total_value_bytes as u64);
        span.record("value_bytes", total_value_bytes as u64);
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compare_and_swap_query, connect_postgres_pool, create_table_statement, delete_query,
        get_query, iam_options, list_entries_query, list_keys_query, next_refresh_retry,
        postgres_credential_provider, rename_migration_index_statement, set_query,
        spawn_credential_refresh, CredentialProvider, DatabaseCredential, PostgresKvStore,
        IAM_TOKEN_REFRESH, REFRESH_RETRY_INITIAL,
    };
    use crate::control::{config::proto, keys, KeyValueStore, ListOptions};
    use crate::test_support::{docker_test_guard, PostgresContainer};
    use anyhow::{anyhow, Result};
    use async_trait::async_trait;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
    use std::str::FromStr;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;

    #[test]
    fn sql_builders_use_expected_table_and_clauses() {
        let create = create_table_statement("talon_kv");
        assert!(create.contains("CREATE TABLE IF NOT EXISTS \"talon_kv\""));
        assert!(create.contains("PRIMARY KEY (namespace, parent_path, kind, name)"));

        assert!(get_query("talon_kv").contains("WHERE namespace = $1"));
        assert!(set_query("talon_kv").contains("ON CONFLICT (namespace, parent_path, kind, name)"));
        assert!(compare_and_swap_query("talon_kv", true).contains("AND value = $6"));
        assert!(compare_and_swap_query("talon_kv", false).contains("DO NOTHING"));
        assert!(delete_query("talon_kv").contains("WHERE namespace = $1"));
        assert!(list_keys_query("talon_kv", true, ListOptions::default()).contains("AND kind = $3"));
        assert!(list_keys_query("talon_kv", true, ListOptions::desc())
            .contains("ORDER BY kind DESC, name DESC"));
        assert!(list_keys_query(
            "talon_kv",
            true,
            ListOptions::desc().before_name(Some("b")).limit(10)
        )
        .contains("AND name < $4"));
        assert!(
            list_entries_query("talon_kv", false, ListOptions::default())
                .contains("ORDER BY kind ASC, name ASC")
        );
        assert_eq!(
            rename_migration_index_statement("talon_kv", "talon_kv_structured_key_migration"),
            "ALTER INDEX IF EXISTS \"talon_kv_structured_key_migration_pkey\" RENAME TO \"talon_kv_pkey\""
        );
    }

    #[test]
    fn postgres_legacy_migration_maps_old_system_namespace_names() {
        assert_eq!(
            super::super::legacy::namespaced_key("talon-system:ns", "Namespace/quickstart")
                .unwrap(),
            crate::control::keys::namespace_metadata("quickstart").canonical()
        );
        assert_eq!(
            super::super::legacy::namespaced_key(
                "talon-system:ns:internal",
                "NamespaceRef/quickstart"
            )
            .unwrap(),
            crate::control::keys::namespace_ref(None, "quickstart").canonical()
        );
    }

    #[tokio::test]
    async fn postgres_required_tls_sends_client_hello() {
        use sqlx::{Connection, PgConnection};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "postgres://talon:password@{}/talon?sslmode=require",
            listener.local_addr().unwrap()
        );

        // Accept PostgreSQL's SSL upgrade, then observe the TLS ClientHello.
        // This catches missing SQLx TLS features without a database or certificate.
        let server = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 8];
            stream
                .read_exact(&mut request)
                .await
                .expect("client should send PostgreSQL SSLRequest");
            assert_eq!(request, [0, 0, 0, 8, 4, 210, 22, 47]);
            stream.write_all(b"S").await.unwrap();

            let mut hello = [0; 6];
            stream
                .read_exact(&mut hello)
                .await
                .expect("client should start the TLS handshake");
            assert_eq!(hello[0], 22, "expected a TLS handshake record");
            assert_eq!(hello[1], 3, "expected the TLS record version");
            assert_eq!(hello[5], 1, "expected a TLS ClientHello");
        };

        tokio::time::timeout(Duration::from_secs(5), async {
            let (_, connection) = tokio::join!(server, PgConnection::connect(&url));
            // The mock server closes before completing the TLS handshake.
            assert!(connection.is_err());
        })
        .await
        .expect("PostgreSQL TLS negotiation should finish promptly");
    }

    async fn init_test_store(database_url: &str) -> PostgresKvStore {
        let mut last_error = None;
        for _ in 0..20 {
            match PostgresKvStore::new(
                database_url,
                "talon_kv_store_test",
                super::PostgresPoolSettings::from_env(),
            )
            .await
            {
                Ok(store) => return store,
                Err(err) => {
                    last_error = Some(err);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
        panic!(
            "store should initialize: {}",
            last_error.expect("expected initialization error")
        );
    }

    #[tokio::test]
    async fn postgres_kv_round_trip_compare_and_swap_and_direct_list_ops() {
        let _guard = docker_test_guard();
        let pg = PostgresContainer::start("talon-kv-pg");
        let store = init_test_store(&pg.database_url()).await;

        let missing = keys::session("quickstart", "hello-agent", "missing");
        let a = keys::session("quickstart", "hello-agent", "a");
        let b = keys::session("quickstart", "hello-agent", "b");
        let other = keys::session("quickstart", "other-agent", "c");
        let list = keys::session_prefix("quickstart", "hello-agent");

        assert!(store.get(&missing).await.unwrap().is_none());

        store.set(&a, b"one").await.unwrap();
        store.set(&b, b"two").await.unwrap();
        store.set(&other, b"three").await.unwrap();
        assert_eq!(store.get(&a).await.unwrap(), Some(b"one".to_vec()));

        let listed = store.list_keys(&list, None).await.unwrap();
        assert_eq!(listed, vec![a.clone(), b.clone()]);
        assert_eq!(
            store
                .list_keys(&list, Some(ListOptions::desc()))
                .await
                .unwrap(),
            vec![b.clone(), a.clone()]
        );

        assert_eq!(
            store
                .list_keys(&list, Some(ListOptions::desc().limit(10)))
                .await
                .unwrap(),
            vec![b.clone(), a.clone()]
        );
        assert_eq!(
            store
                .list_keys(
                    &list,
                    Some(ListOptions::desc().before_name(Some("b")).limit(10)),
                )
                .await
                .unwrap(),
            vec![a.clone()]
        );
        assert_eq!(
            store
                .list_entries(&list, Some(ListOptions::desc().limit(10)))
                .await
                .unwrap(),
            vec![(b.clone(), b"two".to_vec()), (a.clone(), b"one".to_vec())]
        );

        let entries = store.list_entries(&list, None).await.unwrap();
        assert_eq!(entries[0], (a.clone(), b"one".to_vec()));
        assert_eq!(entries[1], (b.clone(), b"two".to_vec()));
        assert_eq!(
            store
                .list_entries(&list, Some(ListOptions::desc()))
                .await
                .unwrap(),
            vec![(b.clone(), b"two".to_vec()), (a.clone(), b"one".to_vec())]
        );

        assert!(store
            .compare_and_swap(&a, Some(b"one"), b"updated")
            .await
            .unwrap());
        assert!(!store
            .compare_and_swap(&a, Some(b"wrong"), b"nope")
            .await
            .unwrap());
        let new_key = keys::session("quickstart", "hello-agent", "new");
        assert!(store
            .compare_and_swap(&new_key, None, b"created")
            .await
            .unwrap());
        assert!(!store
            .compare_and_swap(&new_key, None, b"duplicate")
            .await
            .unwrap());

        store.delete(&new_key).await.unwrap();
        assert!(store.get(&new_key).await.unwrap().is_none());

        store.delete(&a).await.unwrap();
        store.delete(&b).await.unwrap();
        assert!(store.get(&a).await.unwrap().is_none());
        assert!(store.get(&b).await.unwrap().is_none());
        assert_eq!(store.get(&other).await.unwrap(), Some(b"three".to_vec()));
    }

    #[test]
    fn postgres_auth_mode_accepts_strings_and_rejects_unknown_values() {
        for mode in ["", "password", "iam"] {
            let config = proto::DatabaseConfig {
                auth_mode: mode.to_string(),
                ..Default::default()
            };
            assert!(postgres_credential_provider(&config).is_ok());
        }
        for mode in ["gcp_iam", "PASSWORD", "IAM", "unknown", "0", "1"] {
            let config = proto::DatabaseConfig {
                auth_mode: mode.to_string(),
                ..Default::default()
            };
            let error = postgres_credential_provider(&config)
                .err()
                .expect("unknown auth modes must be rejected");
            assert!(error.to_string().contains("invalid PostgreSQL auth mode"));
        }
    }

    struct FailOnceProvider(AtomicUsize);

    #[async_trait]
    impl CredentialProvider for FailOnceProvider {
        async fn credential(&self, options: PgConnectOptions) -> Result<DatabaseCredential> {
            let n = self.0.fetch_add(1, Ordering::SeqCst);
            if n == 1 {
                return Err(anyhow!("transient credential source failure"));
            }
            Ok(DatabaseCredential {
                options: iam_options(options, &format!("fake-token-{n}")),
                valid_for: Some(Duration::from_secs(60 * 60)),
            })
        }
    }

    #[test]
    fn credential_refresh_delay_and_retry_are_bounded() {
        assert!(IAM_TOKEN_REFRESH < Duration::from_secs(60 * 60));
        let mut credential = DatabaseCredential {
            options: PgConnectOptions::new_without_pgpass(),
            valid_for: None,
        };
        assert_eq!(credential.refresh_delay(), None);
        credential.valid_for = Some(Duration::from_secs(60 * 60));
        assert_eq!(credential.refresh_delay(), Some(IAM_TOKEN_REFRESH));
        let mut retry = REFRESH_RETRY_INITIAL;
        for expected in [10, 20, 40, 80, 120, 120] {
            retry = next_refresh_retry(retry);
            assert_eq!(retry, Duration::from_secs(expected));
        }
    }

    #[tokio::test]
    async fn password_and_iam_share_credentials_and_refresh_keeps_the_pool() {
        use sqlx::Connection;
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
                    while stream.read_exact(&mut message).await.is_ok() {
                        if stream.read_exact(&mut length).await.is_err() {
                            return;
                        }
                        let mut body = vec![0; u32::from_be_bytes(length) as usize - 4];
                        if stream.read_exact(&mut body).await.is_err() {
                            return;
                        }
                        match message[0] {
                            b'S' => backend_message(&mut stream, b'Z', b"I").await,
                            b'X' => return,
                            _ => panic!("unexpected mock PostgreSQL message"),
                        }
                    }
                });
            }
        });

        let (password_pool, refresh) = connect_postgres_pool(
            &format!("postgres://iam-user:static%40pass%3A%25%2F@{address}/talon?sslmode=disable"),
            &super::PostgresPoolSettings::default(),
        )
        .await
        .unwrap();
        assert!(
            refresh.is_none(),
            "static credentials must not start a refresh task"
        );
        assert_eq!(tokens_rx.recv().await.unwrap(), "static@pass:%/");
        assert!(matches!(
            password_pool.connect_options().get_ssl_mode(),
            PgSslMode::Disable
        ));
        password_pool.close().await;

        let provider = Arc::new(FailOnceProvider(AtomicUsize::new(0)));
        let options = PgConnectOptions::from_str(&format!(
            "postgres://iam-user@{address}/talon?sslmode=disable"
        ))
        .unwrap();
        let credential = provider.credential(options).await.unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(credential.options)
            .await
            .unwrap();
        let refresh = spawn_credential_refresh(
            pool.clone(),
            provider.clone(),
            Duration::from_millis(500),
            Duration::from_millis(100),
        );
        let first = tokio::time::timeout(Duration::from_secs(2), tokens_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first, "fake-token-0");

        // Hold a connection and a native SQLx pool clone across the refresh.
        let original_pool = pool.clone();
        let mut existing = original_pool.acquire().await.unwrap();
        let initial_options = original_pool.connect_options();
        tokio::time::timeout(Duration::from_secs(2), async {
            while provider.0.load(Ordering::SeqCst) < 2 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the first refresh should fail");
        existing
            .ping()
            .await
            .expect("metadata failures must not interrupt authenticated connections");
        tokio::time::timeout(Duration::from_millis(350), async {
            while Arc::ptr_eq(&initial_options, &original_pool.connect_options()) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("credentials should refresh on the short retry interval");
        assert_eq!(provider.0.load(Ordering::SeqCst), 3);
        assert!(matches!(
            original_pool.connect_options().get_ssl_mode(),
            PgSslMode::Disable
        ));
        assert_eq!(
            original_pool.size(),
            1,
            "refresh must not create a replacement connection"
        );
        assert!(tokens_rx.try_recv().is_err());

        // Keeping the first connection checked out forces a new physical
        // connection from the original pool, using the refreshed token.
        let new_connection = original_pool.acquire().await.unwrap();
        let second = tokio::time::timeout(Duration::from_secs(2), tokens_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second, "fake-token-2");
        assert_eq!(original_pool.size(), 2);
        existing
            .ping()
            .await
            .expect("refresh must preserve the old connection");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            provider.0.load(Ordering::SeqCst),
            3,
            "success must restore the normal refresh interval"
        );

        new_connection.close().await.unwrap();
        existing.close().await.unwrap();
        drop(original_pool);
        let abort = refresh.abort_handle();
        let other_owner = refresh.clone();
        drop(refresh);
        assert!(
            !abort.is_finished(),
            "another backend clone still owns the task"
        );
        drop(other_owner);
        tokio::time::timeout(Duration::from_secs(1), async {
            while !abort.is_finished() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("dropping the last backend owner must abort refresh");
        assert!(
            !pool.is_closed(),
            "task cancellation must not close native pool clones"
        );
        let attempts = provider.0.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert_eq!(provider.0.load(Ordering::SeqCst), attempts);
        pool.close().await;
        server.abort();
    }

    struct FailingProvider;

    #[async_trait]
    impl CredentialProvider for FailingProvider {
        async fn credential(&self, _options: PgConnectOptions) -> Result<DatabaseCredential> {
            Err(anyhow!("credential source unavailable"))
        }
    }

    #[tokio::test]
    async fn credential_failure_is_returned_without_secret_details() {
        let error = connect_postgres_pool(
            "postgres://iam-user@localhost/talon",
            &super::PostgresPoolSettings {
                credential_provider: Arc::new(FailingProvider),
                ..Default::default()
            },
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.to_string(), "failed to obtain PostgreSQL credentials");
    }

    #[test]
    fn credentials_redact_passwords_and_tokens() {
        let options = PgConnectOptions::from_str(
            "postgres://iam-user:ignored@db.example:5432/talon?sslmode=require",
        )
        .unwrap();
        let password = DatabaseCredential {
            options: options.clone(),
            valid_for: None,
        };
        let iam = DatabaseCredential {
            options: iam_options(options, "fresh-token"),
            valid_for: Some(Duration::from_secs(3600)),
        };
        for credential in [password, iam] {
            let rendered = format!("{credential:?}");
            assert!(!rendered.contains("ignored"));
            assert!(!rendered.contains("fresh-token"));
        }
    }

    #[test]
    fn iam_options_preserve_url_tls_configuration() {
        for mode in [
            None,
            Some("disable"),
            Some("allow"),
            Some("prefer"),
            Some("require"),
            Some("verify-ca"),
            Some("verify-full"),
        ] {
            let mut url =
                url::Url::parse("postgres://iam-user:old-token@db.example/talon").unwrap();
            if let Some(mode) = mode {
                url.query_pairs_mut().append_pair("sslmode", mode);
            }
            url.query_pairs_mut()
                .append_pair("sslrootcert", "/etc/ssl/cloudsql/ca.pem");
            let parsed = PgConnectOptions::from_str(url.as_str()).unwrap();
            let expected_ssl_mode = std::mem::discriminant(&parsed.get_ssl_mode());
            let options = iam_options(parsed, "token");
            assert_eq!(
                std::mem::discriminant(&options.get_ssl_mode()),
                expected_ssl_mode,
                "IAM must preserve the configured SSL mode: {mode:?}"
            );
            assert_eq!(options.get_host(), "db.example");
            assert_eq!(options.get_username(), "iam-user");
            assert_eq!(options.get_database(), Some("talon"));
            assert_eq!(
                sqlx::ConnectOptions::to_url_lossy(&options).password(),
                Some("token")
            );
            assert!(sqlx::ConnectOptions::to_url_lossy(&options)
                .query_pairs()
                .any(|(key, value)| key == "sslrootcert"
                    && value == "file: /etc/ssl/cloudsql/ca.pem"));
        }
    }

    #[tokio::test]
    async fn iam_connection_with_required_tls_rejects_a_plaintext_endpoint() {
        use sqlx::Connection;
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 8];
            stream.read_exact(&mut request).await.unwrap();
            assert_eq!(request, [0, 0, 0, 8, 4, 210, 22, 47]);
            stream.write_all(b"N").await.unwrap();
            let mut remaining = Vec::new();
            stream.read_to_end(&mut remaining).await.unwrap();
            assert!(
                remaining.is_empty(),
                "no startup or password may follow TLS rejection"
            );
        });
        let options = iam_options(
            PgConnectOptions::from_str(&format!(
                "postgres://iam-user@{address}/talon?sslmode=require"
            ))
            .unwrap(),
            "secret-iam-token",
        );
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            sqlx::PgConnection::connect_with(&options),
        )
        .await
        .unwrap();
        assert!(result.is_err());
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }
}
