// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{
    keys::{ResourceKey, ResourceList},
    KeyValueStore, ListOptions, Order,
};
use anyhow::{bail, Result};
use sqlx::{pool::PoolConnection, postgres::PgPoolOptions, PgConnection, PgPool, Postgres, Row};
use std::time::Instant;
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

fn list_keys_page_query(table: &str) -> String {
    format!(
        "SELECT namespace, parent_path, kind, name FROM {}
         WHERE namespace = $1 AND parent_path = $2 AND kind = $3
           AND ($4 IS NULL OR name < $4)
         ORDER BY name DESC
         LIMIT $5",
        quoted_identifier(table)
    )
}

fn list_entries_page_query(table: &str) -> String {
    format!(
        "SELECT namespace, parent_path, kind, name, value FROM {}
         WHERE namespace = $1 AND parent_path = $2 AND kind = $3
           AND ($4 IS NULL OR name < $4)
         ORDER BY name DESC
         LIMIT $5",
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
    table: String,
    settings: PostgresPoolSettings,
}

#[derive(Debug, Clone, Copy)]
struct PostgresPoolSettings {
    max_connections: u32,
}

impl PostgresPoolSettings {
    fn from_env() -> Self {
        let max_connections = std::env::var("TALON_POSTGRES_MAX_CONNECTIONS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(10);
        Self { max_connections }
    }
}

impl PostgresKvStore {
    pub async fn new(url: &str, table: &str) -> Result<Self> {
        validate_identifier(table)?;
        let settings = PostgresPoolSettings::from_env();
        let pool = PgPoolOptions::new()
            .max_connections(settings.max_connections)
            .connect(url)
            .await?;

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
            table: table.to_string(),
            settings,
        })
    }
}

async fn acquire_connection(
    pool: &PgPool,
    settings: PostgresPoolSettings,
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
            let mut conn = acquire_connection(&self.pool, self.settings, &span_for_body).await?;
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
            let mut conn = acquire_connection(&self.pool, self.settings, &span_for_body).await?;
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
        let mut conn = acquire_connection(&self.pool, self.settings, &span).await?;
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
        let mut conn = acquire_connection(&self.pool, self.settings, &span).await?;
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
        let mut conn = acquire_connection(&self.pool, self.settings, &span).await?;
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
        let mut conn = acquire_connection(&self.pool, self.settings, &span).await?;
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
            "PostgresKvStore.list_keys_page",
            "db.system" = "postgresql",
            "db.operation" = "list_keys_page",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = %kind,
            "postgres.pool.max_connections" = u64::from(self.settings.max_connections),
            "postgres.pool.size_before" = field::Empty,
            "postgres.pool.idle_before" = field::Empty,
            "postgres.pool.size_after" = field::Empty,
            "postgres.pool.idle_after" = field::Empty,
            pool_wait_us = field::Empty,
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
            limit,
        );
        let mut conn = acquire_connection(&self.pool, self.settings, &span).await?;
        let query_span = tracing::debug_span!(
            parent: &span,
            "PostgresKvStore.query",
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
            "PostgresKvStore.list_entries_page",
            "db.system" = "postgresql",
            "db.operation" = "list_entries_page",
            "talon.kv.table" = %self.table,
            "talon.resource.kind" = %kind,
            "postgres.pool.max_connections" = u64::from(self.settings.max_connections),
            "postgres.pool.size_before" = field::Empty,
            "postgres.pool.idle_before" = field::Empty,
            "postgres.pool.size_after" = field::Empty,
            "postgres.pool.idle_after" = field::Empty,
            pool_wait_us = field::Empty,
            query_elapsed_us = field::Empty,
            rows_returned = field::Empty,
            value_bytes = field::Empty,
            limit,
        );
        let mut conn = acquire_connection(&self.pool, self.settings, &span).await?;
        let query_span = tracing::debug_span!(
            parent: &span,
            "PostgresKvStore.query",
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
        rename_migration_index_statement, set_query, PostgresKvStore,
    };
    use crate::control::{keys, KeyValueStore, ListOptions, Order};
    use crate::test_support::{docker_test_guard, PostgresContainer};
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
        assert!(list_keys_query("talon_kv", true, Order::Asc.into()).contains("AND kind = $3"));
        assert!(list_keys_query("talon_kv", true, Order::Desc.into())
            .contains("ORDER BY kind DESC, name DESC"));
        assert!(list_keys_page_query("talon_kv").contains("ORDER BY name DESC"));
        assert!(list_entries_page_query("talon_kv")
            .contains("SELECT namespace, parent_path, kind, name, value"));
        assert!(list_entries_query("talon_kv", false, Order::Asc.into())
            .contains("ORDER BY kind ASC, name ASC"));
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

    async fn init_test_store(database_url: &str) -> PostgresKvStore {
        let mut last_error = None;
        for _ in 0..20 {
            match PostgresKvStore::new(database_url, "talon_kv_store_test").await {
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
}
