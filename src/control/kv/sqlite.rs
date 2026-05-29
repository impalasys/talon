// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{
    keys::{ResourceKey, ResourceList},
    KeyValueStore,
};
use anyhow::{bail, Result};
use sqlx::{
    pool::PoolConnection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Row, Sqlite, SqlitePool,
};
use std::{
    str::FromStr,
    time::{Duration, Instant},
};
use tracing::{field, Instrument, Span};

use super::shared::{quoted_identifier, validate_identifier};

fn create_table_statement(table: &str) -> String {
    let table = quoted_identifier(table);
    format!(
        "CREATE TABLE IF NOT EXISTS {} (
                namespace TEXT NOT NULL,
                parent_path TEXT NOT NULL,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                value BLOB NOT NULL,
                PRIMARY KEY (namespace, parent_path, kind, name)
            ) WITHOUT ROWID",
        table
    )
}

fn get_query(table: &str) -> String {
    format!(
        "SELECT value FROM {}
         WHERE namespace = ?1 AND parent_path = ?2 AND kind = ?3 AND name = ?4",
        quoted_identifier(table)
    )
}

fn set_query(table: &str) -> String {
    let table = quoted_identifier(table);
    format!(
        "INSERT INTO {} (namespace, parent_path, kind, name, value)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (namespace, parent_path, kind, name)
             DO UPDATE SET value = excluded.value",
        table
    )
}

fn compare_and_swap_query(table: &str, expected: bool) -> String {
    let table = quoted_identifier(table);
    if expected {
        format!(
            "UPDATE {} SET value = ?5
             WHERE namespace = ?1 AND parent_path = ?2 AND kind = ?3 AND name = ?4 AND value = ?6",
            table
        )
    } else {
        format!(
            "INSERT INTO {} (namespace, parent_path, kind, name, value)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (namespace, parent_path, kind, name) DO NOTHING",
            table
        )
    }
}

fn delete_query(table: &str) -> String {
    format!(
        "DELETE FROM {}
         WHERE namespace = ?1 AND parent_path = ?2 AND kind = ?3 AND name = ?4",
        quoted_identifier(table)
    )
}

fn list_keys_query(table: &str, has_kind: bool) -> String {
    let kind_clause = if has_kind { "AND kind = ?3" } else { "" };
    format!(
        "SELECT namespace, parent_path, kind, name FROM {}
         WHERE namespace = ?1 AND parent_path = ?2 {kind_clause}
         ORDER BY kind ASC, name ASC",
        quoted_identifier(table)
    )
}

fn list_entries_query(table: &str, has_kind: bool) -> String {
    let kind_clause = if has_kind { "AND kind = ?3" } else { "" };
    format!(
        "SELECT namespace, parent_path, kind, name, value FROM {}
         WHERE namespace = ?1 AND parent_path = ?2 {kind_clause}
         ORDER BY kind ASC, name ASC",
        quoted_identifier(table)
    )
}

fn list_keys_page_query(table: &str) -> String {
    format!(
        "SELECT namespace, parent_path, kind, name FROM {}
         WHERE namespace = ?1 AND parent_path = ?2 AND kind = ?3
           AND (?4 IS NULL OR name < ?4)
         ORDER BY name DESC
         LIMIT ?5",
        quoted_identifier(table)
    )
}

fn list_entries_page_query(table: &str) -> String {
    format!(
        "SELECT namespace, parent_path, kind, name, value FROM {}
         WHERE namespace = ?1 AND parent_path = ?2 AND kind = ?3
           AND (?4 IS NULL OR name < ?4)
         ORDER BY name DESC
         LIMIT ?5",
        quoted_identifier(table)
    )
}

fn create_migration_table_statement(table: &str) -> String {
    create_table_statement(table).replacen("CREATE TABLE IF NOT EXISTS", "CREATE TABLE", 1)
}

fn key_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ResourceKey> {
    Ok(ResourceKey {
        namespace: row.try_get("namespace")?,
        parent_path: row.try_get("parent_path")?,
        kind: row.try_get("kind")?,
        name: row.try_get("name")?,
    })
}

pub struct SqliteKvStore {
    pool: SqlitePool,
    table: String,
    settings: SqlitePoolSettings,
}

#[derive(Debug, Clone, Copy)]
struct SqlitePoolSettings {
    max_connections: u32,
    busy_timeout: Duration,
}

impl SqlitePoolSettings {
    fn from_env() -> Self {
        let max_connections = std::env::var("TALON_SQLITE_MAX_CONNECTIONS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(5);
        let busy_timeout_ms = std::env::var("TALON_SQLITE_BUSY_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(5_000);

        Self {
            max_connections,
            busy_timeout: Duration::from_millis(busy_timeout_ms),
        }
    }
}

impl SqliteKvStore {
    pub async fn new(url: &str, table: &str) -> Result<Self> {
        validate_identifier(table)?;
        let settings = SqlitePoolSettings::from_env();
        let pool = sqlite_pool(url, settings).await?;

        migrate_legacy_table(&pool, table).await?;
        let create_stmt = create_table_statement(table);
        sqlx::query(&create_stmt).execute(&pool).await?;

        Ok(Self {
            pool,
            table: table.to_string(),
            settings,
        })
    }
}

async fn migrate_legacy_table(pool: &SqlitePool, table: &str) -> Result<()> {
    let columns = sqlx::query(&format!("PRAGMA table_info({})", quoted_identifier(table)))
        .fetch_all(pool)
        .await?;
    let columns = columns
        .iter()
        .filter_map(|row| row.try_get::<String, _>("name").ok())
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

    let mut tx = pool.begin().await?;
    let old_rows = if has_namespace {
        sqlx::query(&format!(
            "SELECT namespace, key, value FROM {}",
            quoted_identifier(table)
        ))
        .fetch_all(&mut *tx)
        .await?
    } else {
        sqlx::query(&format!(
            "SELECT key, value FROM {}",
            quoted_identifier(table)
        ))
        .fetch_all(&mut *tx)
        .await?
    };
    let mut converted_rows = Vec::with_capacity(old_rows.len());
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
        converted_rows.push((key, value));
    }

    sqlx::query(&format!("DROP TABLE {}", quoted_identifier(table)))
        .execute(&mut *tx)
        .await?;
    sqlx::query(&create_migration_table_statement(table))
        .execute(&mut *tx)
        .await?;
    let insert = set_query(table);
    for (key, value) in converted_rows {
        sqlx::query(&insert)
            .bind(&key.namespace)
            .bind(&key.parent_path)
            .bind(&key.kind)
            .bind(&key.name)
            .bind(value)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn sqlite_pool(url: &str, settings: SqlitePoolSettings) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(settings.busy_timeout)
        .foreign_keys(true);
    Ok(SqlitePoolOptions::new()
        .max_connections(settings.max_connections)
        .connect_with(options)
        .await?)
}

fn busy_timeout_ms(settings: SqlitePoolSettings) -> u64 {
    settings.busy_timeout.as_millis().min(u128::from(u64::MAX)) as u64
}

async fn acquire_connection(
    pool: &SqlitePool,
    settings: SqlitePoolSettings,
    parent_span: &Span,
) -> Result<PoolConnection<Sqlite>> {
    let pool_size_before = pool.size();
    let pool_idle_before = pool.num_idle();
    parent_span.record("sqlite.pool.size_before", u64::from(pool_size_before));
    parent_span.record("sqlite.pool.idle_before", pool_idle_before as u64);

    let span = tracing::debug_span!(
        parent: parent_span,
        "SqliteKvStore.acquire_connection",
        "sqlite.pool.max_connections" = u64::from(settings.max_connections),
        "sqlite.pool.size_before" = u64::from(pool_size_before),
        "sqlite.pool.idle_before" = pool_idle_before as u64,
        "sqlite.pool.size_after" = field::Empty,
        "sqlite.pool.idle_after" = field::Empty,
        pool_wait_us = field::Empty,
    );
    let started_at = Instant::now();
    let conn = pool.acquire().instrument(span.clone()).await?;
    let pool_wait_us = started_at.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
    let pool_size_after = pool.size();
    let pool_idle_after = pool.num_idle();

    span.record("pool_wait_us", pool_wait_us);
    span.record("sqlite.pool.size_after", u64::from(pool_size_after));
    span.record("sqlite.pool.idle_after", pool_idle_after as u64);
    parent_span.record("pool_wait_us", pool_wait_us);
    parent_span.record("sqlite.pool.size_after", u64::from(pool_size_after));
    parent_span.record("sqlite.pool.idle_after", pool_idle_after as u64);

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
impl KeyValueStore for SqliteKvStore {
    async fn get(&self, key: &ResourceKey) -> Result<Option<Vec<u8>>> {
        let query = get_query(&self.table);
        let span = tracing::debug_span!(
            "SqliteKvStore.get",
            "db.system" = "sqlite",
            "db.operation" = "get",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = %key.kind,
            "sqlite.pool.max_connections" = u64::from(self.settings.max_connections),
            "sqlite.busy_timeout_ms" = busy_timeout_ms(self.settings),
            "sqlite.pool.size_before" = field::Empty,
            "sqlite.pool.idle_before" = field::Empty,
            "sqlite.pool.size_after" = field::Empty,
            "sqlite.pool.idle_after" = field::Empty,
            pool_wait_us = field::Empty,
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
            value_bytes = field::Empty,
        );
        let mut conn = acquire_connection(&self.pool, self.settings, &span).await?;
        let query_span = tracing::debug_span!(
            parent: &span,
            "SqliteKvStore.query",
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
            .instrument(span.clone())
            .await?;
        record_query_elapsed(&query_span, &span, query_started_at);

        if let Some(row) = row {
            let value: Vec<u8> = row.try_get("value")?;
            query_span.record("value_bytes", value.len() as u64);
            span.record("value_bytes", value.len() as u64);
            record_rows(&query_span, &span, 1);
            Ok(Some(value))
        } else {
            record_rows(&query_span, &span, 0);
            Ok(None)
        }
    }

    async fn set(&self, key: &ResourceKey, value: &[u8]) -> Result<()> {
        let query = set_query(&self.table);
        let span = tracing::debug_span!(
            "SqliteKvStore.set",
            "db.system" = "sqlite",
            "db.operation" = "set",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = %key.kind,
            "sqlite.pool.max_connections" = u64::from(self.settings.max_connections),
            "sqlite.busy_timeout_ms" = busy_timeout_ms(self.settings),
            "sqlite.pool.size_before" = field::Empty,
            "sqlite.pool.idle_before" = field::Empty,
            "sqlite.pool.size_after" = field::Empty,
            "sqlite.pool.idle_after" = field::Empty,
            pool_wait_us = field::Empty,
            query_elapsed_us = field::Empty,
            rows_affected = field::Empty,
            value_bytes = value.len(),
        );
        let mut conn = acquire_connection(&self.pool, self.settings, &span).await?;
        let query_span = tracing::debug_span!(
            parent: &span,
            "SqliteKvStore.query",
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
            .instrument(span.clone())
            .await?;
        record_query_elapsed(&query_span, &span, query_started_at);
        query_span.record("rows_affected", result.rows_affected());
        span.record("rows_affected", result.rows_affected());
        Ok(())
    }

    async fn compare_and_swap(
        &self,
        key: &ResourceKey,
        expected: Option<&[u8]>,
        value: &[u8],
    ) -> Result<bool> {
        let query = compare_and_swap_query(&self.table, expected.is_some());
        let span = tracing::debug_span!(
            "SqliteKvStore.compare_and_swap",
            "db.system" = "sqlite",
            "db.operation" = "compare_and_swap",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = %key.kind,
            "sqlite.pool.max_connections" = u64::from(self.settings.max_connections),
            "sqlite.busy_timeout_ms" = busy_timeout_ms(self.settings),
            "sqlite.pool.size_before" = field::Empty,
            "sqlite.pool.idle_before" = field::Empty,
            "sqlite.pool.size_after" = field::Empty,
            "sqlite.pool.idle_after" = field::Empty,
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
        let mut conn = acquire_connection(&self.pool, self.settings, &span).await?;
        let query_span = tracing::debug_span!(
            parent: &span,
            "SqliteKvStore.query",
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
            "SqliteKvStore.delete",
            "db.system" = "sqlite",
            "db.operation" = "delete",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = %key.kind,
            "sqlite.pool.max_connections" = u64::from(self.settings.max_connections),
            "sqlite.busy_timeout_ms" = busy_timeout_ms(self.settings),
            "sqlite.pool.size_before" = field::Empty,
            "sqlite.pool.idle_before" = field::Empty,
            "sqlite.pool.size_after" = field::Empty,
            "sqlite.pool.idle_after" = field::Empty,
            pool_wait_us = field::Empty,
            query_elapsed_us = field::Empty,
            rows_affected = field::Empty,
        );
        let mut conn = acquire_connection(&self.pool, self.settings, &span).await?;
        let query_span = tracing::debug_span!(
            parent: &span,
            "SqliteKvStore.query",
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

    async fn list_keys(&self, list: &ResourceList) -> Result<Vec<ResourceKey>> {
        let query = list_keys_query(&self.table, list.kind.is_some());
        let span = tracing::debug_span!(
            "SqliteKvStore.list_keys",
            "db.system" = "sqlite",
            "db.operation" = "list_keys",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = list.kind.as_deref().unwrap_or("*"),
            "sqlite.pool.max_connections" = u64::from(self.settings.max_connections),
            "sqlite.busy_timeout_ms" = busy_timeout_ms(self.settings),
            "sqlite.pool.size_before" = field::Empty,
            "sqlite.pool.idle_before" = field::Empty,
            "sqlite.pool.size_after" = field::Empty,
            "sqlite.pool.idle_after" = field::Empty,
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
        let mut conn = acquire_connection(&self.pool, self.settings, &span).await?;
        let query_span = tracing::debug_span!(
            parent: &span,
            "SqliteKvStore.query",
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

    async fn list_entries(&self, list: &ResourceList) -> Result<Vec<(ResourceKey, Vec<u8>)>> {
        let query = list_entries_query(&self.table, list.kind.is_some());
        let span = tracing::debug_span!(
            "SqliteKvStore.list_entries",
            "db.system" = "sqlite",
            "db.operation" = "list_entries",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = list.kind.as_deref().unwrap_or("*"),
            "sqlite.pool.max_connections" = u64::from(self.settings.max_connections),
            "sqlite.busy_timeout_ms" = busy_timeout_ms(self.settings),
            "sqlite.pool.size_before" = field::Empty,
            "sqlite.pool.idle_before" = field::Empty,
            "sqlite.pool.size_after" = field::Empty,
            "sqlite.pool.idle_after" = field::Empty,
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
        let mut conn = acquire_connection(&self.pool, self.settings, &span).await?;
        let query_span = tracing::debug_span!(
            parent: &span,
            "SqliteKvStore.query",
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

    async fn list_keys_page(
        &self,
        list: &ResourceList,
        before_name: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ResourceKey>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let Some(kind) = &list.kind else {
            bail!("paged resource listing requires an explicit resource kind");
        };

        let query = list_keys_page_query(&self.table);
        let span = tracing::debug_span!(
            "SqliteKvStore.list_keys_page",
            "db.system" = "sqlite",
            "db.operation" = "list_keys_page",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = %kind,
            "sqlite.pool.max_connections" = u64::from(self.settings.max_connections),
            "sqlite.busy_timeout_ms" = busy_timeout_ms(self.settings),
            "sqlite.pool.size_before" = field::Empty,
            "sqlite.pool.idle_before" = field::Empty,
            "sqlite.pool.size_after" = field::Empty,
            "sqlite.pool.idle_after" = field::Empty,
            pool_wait_us = field::Empty,
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
            limit,
        );
        let mut conn = acquire_connection(&self.pool, self.settings, &span).await?;
        let query_span = tracing::debug_span!(
            parent: &span,
            "SqliteKvStore.query",
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
        );
        let query_started_at = Instant::now();
        let rows = sqlx::query(&query)
            .bind(&list.parent.namespace)
            .bind(&list.parent.parent_path)
            .bind(kind)
            .bind(before_name)
            .bind(limit as i64)
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

    async fn list_entries_page(
        &self,
        list: &ResourceList,
        before_name: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(ResourceKey, Vec<u8>)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let Some(kind) = &list.kind else {
            bail!("paged resource listing requires an explicit resource kind");
        };

        let query = list_entries_page_query(&self.table);
        let span = tracing::debug_span!(
            "SqliteKvStore.list_entries_page",
            "db.system" = "sqlite",
            "db.operation" = "list_entries_page",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = %kind,
            "sqlite.pool.max_connections" = u64::from(self.settings.max_connections),
            "sqlite.busy_timeout_ms" = busy_timeout_ms(self.settings),
            "sqlite.pool.size_before" = field::Empty,
            "sqlite.pool.idle_before" = field::Empty,
            "sqlite.pool.size_after" = field::Empty,
            "sqlite.pool.idle_after" = field::Empty,
            pool_wait_us = field::Empty,
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
            value_bytes = field::Empty,
            limit,
        );
        let mut conn = acquire_connection(&self.pool, self.settings, &span).await?;
        let query_span = tracing::debug_span!(
            parent: &span,
            "SqliteKvStore.query",
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
            value_bytes = field::Empty,
        );
        let query_started_at = Instant::now();
        let rows = sqlx::query(&query)
            .bind(&list.parent.namespace)
            .bind(&list.parent.parent_path)
            .bind(kind)
            .bind(before_name)
            .bind(limit as i64)
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
        compare_and_swap_query, create_table_statement, delete_query, get_query,
        list_entries_page_query, list_entries_query, list_keys_page_query, list_keys_query,
        set_query, sqlite_pool, SqliteKvStore,
    };
    use crate::control::kv::sqlite_url_for_path;
    use crate::control::{keys, KeyValueStore};
    use tempfile::tempdir;

    #[test]
    fn sql_builders_use_expected_table_and_clauses() {
        let create = create_table_statement("talon_kv");
        assert!(create.contains("CREATE TABLE IF NOT EXISTS \"talon_kv\""));
        assert!(create.contains("value BLOB NOT NULL"));
        assert!(create.contains("WITHOUT ROWID"));

        assert!(get_query("talon_kv").contains("WHERE namespace = ?1"));
        assert!(set_query("talon_kv").contains("excluded.value"));
        assert!(compare_and_swap_query("talon_kv", true).contains("AND value = ?6"));
        assert!(compare_and_swap_query("talon_kv", false).contains("DO NOTHING"));
        assert!(delete_query("talon_kv").contains("WHERE namespace = ?1"));
        assert!(list_keys_query("talon_kv", true).contains("AND kind = ?3"));
        assert!(list_keys_page_query("talon_kv").contains("ORDER BY name DESC"));
        assert!(list_entries_page_query("talon_kv")
            .contains("SELECT namespace, parent_path, kind, name, value"));
        assert!(list_entries_query("talon_kv", false).contains("ORDER BY kind ASC, name ASC"));
    }

    #[test]
    fn sqlite_legacy_migration_maps_old_system_namespace_names() {
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
    async fn sqlite_migrates_old_system_namespace_rows() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("talon-kv-legacy.db");
        let url = sqlite_url_for_path(&db_path);
        let setup_pool = sqlite_pool(&url, super::SqlitePoolSettings::from_env())
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE talon_kv_store_test (
                namespace TEXT NOT NULL,
                key TEXT NOT NULL,
                value BLOB NOT NULL,
                PRIMARY KEY (namespace, key)
            )",
        )
        .execute(&setup_pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO talon_kv_store_test (namespace, key, value)
             VALUES (?1, ?2, ?3)",
        )
        .bind("talon-system:ns")
        .bind("Namespace/quickstart")
        .bind(b"meta".to_vec())
        .execute(&setup_pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO talon_kv_store_test (namespace, key, value)
             VALUES (?1, ?2, ?3)",
        )
        .bind("talon-system:ns:internal")
        .bind("NamespaceRef/quickstart")
        .bind(b"edge".to_vec())
        .execute(&setup_pool)
        .await
        .unwrap();
        setup_pool.close().await;

        let store = SqliteKvStore::new(&url, "talon_kv_store_test")
            .await
            .unwrap();

        assert_eq!(
            store
                .get(&crate::control::keys::namespace_metadata("quickstart"))
                .await
                .unwrap(),
            Some(b"meta".to_vec())
        );
        assert_eq!(
            store
                .get(&crate::control::keys::namespace_ref(None, "quickstart"))
                .await
                .unwrap(),
            Some(b"edge".to_vec())
        );
    }

    #[tokio::test]
    async fn sqlite_kv_round_trip_compare_and_swap_and_direct_list_ops() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("talon-kv.db");
        let store = SqliteKvStore::new(&sqlite_url_for_path(&db_path), "talon_kv_store_test")
            .await
            .unwrap();

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

        let mut listed = store.list_keys(&list).await.unwrap();
        listed.sort_by(|left, right| left.name.cmp(&right.name));
        assert_eq!(listed, vec![a.clone(), b.clone()]);

        assert_eq!(
            store.list_keys_page(&list, None, 10).await.unwrap(),
            vec![b.clone(), a.clone()]
        );
        assert_eq!(
            store.list_keys_page(&list, Some("b"), 10).await.unwrap(),
            vec![a.clone()]
        );
        assert_eq!(
            store.list_entries_page(&list, None, 10).await.unwrap(),
            vec![(b.clone(), b"two".to_vec()), (a.clone(), b"one".to_vec())]
        );

        let mut entries = store.list_entries(&list).await.unwrap();
        entries.sort_by(|left, right| left.0.name.cmp(&right.0.name));
        assert_eq!(entries[0], (a.clone(), b"one".to_vec()));
        assert_eq!(entries[1], (b.clone(), b"two".to_vec()));

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
}
