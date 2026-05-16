// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::KeyValueStore;
use anyhow::Result;
use sqlx::{PgPool, Row};

const RESERVED_IDENTIFIERS: &[&str] = &[
    "all", "analyse", "analyze", "and", "any", "array", "as", "asc", "asymmetric",
    "authorization", "between", "binary", "both", "case", "cast", "check", "collate",
    "column", "constraint", "create", "current_catalog", "current_date", "current_role",
    "current_schema", "current_time", "current_timestamp", "current_user", "default",
    "deferrable", "desc", "distinct", "do", "else", "end", "except", "false", "fetch",
    "for", "foreign", "from", "grant", "group", "having", "in", "initially", "intersect",
    "into", "is", "leading", "limit", "localtime", "localtimestamp", "not", "null",
    "offset", "on", "only", "or", "order", "placing", "primary", "references", "returning",
    "select", "session_user", "some", "symmetric", "table", "then", "to", "trailing", "true",
    "union", "unique", "user", "using", "variadic", "when", "where", "window", "with",
];

fn validate_identifier(table: &str) -> Result<()> {
    if table.is_empty() || table.len() > 63 {
        anyhow::bail!(
            "Invalid table name '{}': must be between 1 and 63 characters",
            table
        );
    }
    let mut chars = table.chars();
    let first = chars.next().expect("table name is not empty");
    if !first.is_ascii_alphabetic() && first != '_' {
        anyhow::bail!(
            "Invalid table name '{}': must start with a letter or underscore",
            table
        );
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        anyhow::bail!(
            "Invalid table name '{}': only ASCII letters, numbers, and underscores are allowed",
            table
        );
    }
    if RESERVED_IDENTIFIERS
        .binary_search(&table.to_ascii_lowercase().as_str())
        .is_ok()
    {
        anyhow::bail!(
            "Invalid table name '{}': SQL reserved keywords are not allowed",
            table
        );
    }
    Ok(())
}

fn quoted_identifier(table: &str) -> String {
    format!("\"{}\"", table)
}

fn like_prefix_pattern(prefix: &str) -> String {
    let mut escaped = String::with_capacity(prefix.len() + 1);
    for ch in prefix.chars() {
        match ch {
            '\\' | '%' | '_' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped.push('%');
    escaped
}

fn create_table_statement(table: &str) -> String {
    let table = quoted_identifier(table);
    format!(
        "CREATE TABLE IF NOT EXISTS {} (
                namespace VARCHAR(255) NOT NULL,
                key VARCHAR(255) NOT NULL,
                value BYTEA NOT NULL,
                PRIMARY KEY (namespace, key)
            )",
        table
    )
}

fn get_query(table: &str) -> String {
    format!(
        "SELECT value FROM {} WHERE namespace = $1 AND key = $2",
        quoted_identifier(table)
    )
}

fn set_query(table: &str) -> String {
    let table = quoted_identifier(table);
    format!(
        "INSERT INTO {} (namespace, key, value) VALUES ($1, $2, $3) 
             ON CONFLICT (namespace, key) DO UPDATE SET value = $3",
        table
    )
}

fn compare_and_swap_query(table: &str, expected: bool) -> String {
    let table = quoted_identifier(table);
    if expected {
        format!(
            "UPDATE {} SET value = $3
                 WHERE namespace = $1 AND key = $2 AND value = $4",
            table
        )
    } else {
        format!(
            "INSERT INTO {} (namespace, key, value) VALUES ($1, $2, $3)
                 ON CONFLICT (namespace, key) DO NOTHING",
            table
        )
    }
}

fn delete_query(table: &str) -> String {
    format!(
        "DELETE FROM {} WHERE namespace = $1 AND key = $2",
        quoted_identifier(table)
    )
}

fn list_keys_query(table: &str) -> String {
    format!(
        "SELECT key FROM {} WHERE namespace = $1 AND key LIKE $2 ESCAPE '\\'",
        quoted_identifier(table)
    )
}

fn list_entries_query(table: &str) -> String {
    format!(
        "SELECT key, value FROM {} WHERE namespace = $1 AND key LIKE $2 ESCAPE '\\'",
        quoted_identifier(table)
    )
}

fn delete_prefix_query(table: &str) -> String {
    format!(
        "DELETE FROM {} WHERE namespace = $1 AND key LIKE $2 ESCAPE '\\'",
        quoted_identifier(table)
    )
}

pub struct PostgresKvStore {
    pool: PgPool,
    table: String,
}

impl PostgresKvStore {
    pub async fn new(url: &str, table: &str) -> Result<Self> {
        validate_identifier(table)?;
        let pool = PgPool::connect(url).await?;

        let create_stmt = create_table_statement(table);
        sqlx::query(&create_stmt).execute(&pool).await?;

        Ok(Self {
            pool,
            table: table.to_string(),
        })
    }
}

#[async_trait::async_trait]
impl KeyValueStore for PostgresKvStore {
    async fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let query = get_query(&self.table);
        let row = sqlx::query(&query)
            .bind(namespace)
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            let value: Vec<u8> = row.try_get("value")?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    async fn set(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()> {
        let query = set_query(&self.table);
        sqlx::query(&query)
            .bind(namespace)
            .bind(key)
            .bind(value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn compare_and_swap(
        &self,
        namespace: &str,
        key: &str,
        expected: Option<&[u8]>,
        value: &[u8],
    ) -> Result<bool> {
        let query = compare_and_swap_query(&self.table, expected.is_some());

        let mut q = sqlx::query(&query).bind(namespace).bind(key).bind(value);
        if let Some(expected) = expected {
            q = q.bind(expected);
        }
        let rows_affected = q.execute(&self.pool).await?.rows_affected();
        Ok(rows_affected == 1)
    }

    async fn delete(&self, namespace: &str, key: &str) -> Result<()> {
        let query = delete_query(&self.table);
        sqlx::query(&query)
            .bind(namespace)
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_keys(&self, namespace: &str, prefix: &str) -> Result<Vec<String>> {
        let query = list_keys_query(&self.table);
        let prefix_pattern = like_prefix_pattern(prefix);
        let rows = sqlx::query(&query)
            .bind(namespace)
            .bind(prefix_pattern)
            .fetch_all(&self.pool)
            .await?;

        let mut keys = Vec::new();
        for row in rows {
            keys.push(row.try_get("key")?);
        }
        Ok(keys)
    }

    async fn list_entries(&self, namespace: &str, prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let query = list_entries_query(&self.table);
        let prefix_pattern = like_prefix_pattern(prefix);
        let rows = sqlx::query(&query)
            .bind(namespace)
            .bind(prefix_pattern)
            .fetch_all(&self.pool)
            .await?;

        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            entries.push((row.try_get("key")?, row.try_get("value")?));
        }
        Ok(entries)
    }

    async fn delete_prefix(&self, namespace: &str, prefix: &str) -> Result<()> {
        let query = delete_prefix_query(&self.table);
        let prefix_pattern = like_prefix_pattern(prefix);
        sqlx::query(&query)
            .bind(namespace)
            .bind(prefix_pattern)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compare_and_swap_query, create_table_statement, delete_prefix_query, delete_query,
        get_query, like_prefix_pattern, list_entries_query, list_keys_query, set_query,
        validate_identifier, PostgresKvStore,
    };
    use crate::control::KeyValueStore;
    use crate::test_support::{docker_test_guard, PostgresContainer};
    use std::time::Duration;

    #[test]
    fn like_prefix_pattern_appends_sql_wildcard() {
        assert_eq!(like_prefix_pattern("Agent/test"), "Agent/test%");
    }

    #[test]
    fn like_prefix_pattern_escapes_like_metacharacters() {
        assert_eq!(like_prefix_pattern(r"Agent_%\path"), r"Agent\_\%\\path%");
    }

    #[test]
    fn sql_builders_use_expected_table_and_clauses() {
        let create = create_table_statement("talon_kv");
        assert!(create.contains("CREATE TABLE IF NOT EXISTS \"talon_kv\""));
        assert!(create.contains("PRIMARY KEY (namespace, key)"));

        assert_eq!(
            get_query("talon_kv"),
            "SELECT value FROM \"talon_kv\" WHERE namespace = $1 AND key = $2"
        );
        assert!(set_query("talon_kv").contains("ON CONFLICT (namespace, key) DO UPDATE"));
        assert!(compare_and_swap_query("talon_kv", true).contains("AND value = $4"));
        assert!(compare_and_swap_query("talon_kv", false).contains("DO NOTHING"));
        assert_eq!(
            delete_query("talon_kv"),
            "DELETE FROM \"talon_kv\" WHERE namespace = $1 AND key = $2"
        );
        assert!(list_keys_query("talon_kv").contains("LIKE $2 ESCAPE '\\'"));
        assert!(list_entries_query("talon_kv").contains("SELECT key, value"));
        assert!(delete_prefix_query("talon_kv").contains("DELETE FROM \"talon_kv\""));
    }

    #[test]
    fn validate_identifier_rejects_invalid_table_names() {
        validate_identifier("talon_kv").expect("underscores should be allowed");
        validate_identifier("talon123").expect("alphanumeric names should be allowed");
        validate_identifier("_talon").expect("leading underscore should be allowed");

        let err = validate_identifier("talon-kv").unwrap_err();
        assert!(err.to_string().contains("Invalid table name"));

        let empty = validate_identifier("").unwrap_err();
        assert!(empty.to_string().contains("Invalid table name"));

        let starts_with_digit = validate_identifier("1talon").unwrap_err();
        assert!(starts_with_digit.to_string().contains("must start"));

        let too_long = validate_identifier(&"a".repeat(64)).unwrap_err();
        assert!(too_long.to_string().contains("between 1 and 63"));

        let reserved = validate_identifier("select").unwrap_err();
        assert!(reserved.to_string().contains("reserved keywords"));

        validate_identifier("select_log").expect("non-keyword identifiers should be allowed");
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
    async fn postgres_kv_round_trip_compare_and_swap_and_prefix_ops() {
        let _guard = docker_test_guard();
        let pg = PostgresContainer::start("talon-kv-pg");
        let store = init_test_store(&pg.database_url()).await;

        assert!(store.get("ns", "missing").await.unwrap().is_none());

        store.set("ns", "prefix/a", b"one").await.unwrap();
        store.set("ns", "prefix/b", b"two").await.unwrap();
        store.set("ns", "other/c", b"three").await.unwrap();
        assert_eq!(
            store.get("ns", "prefix/a").await.unwrap(),
            Some(b"one".to_vec())
        );

        let mut keys = store.list_keys("ns", "prefix/").await.unwrap();
        keys.sort();
        assert_eq!(keys, vec!["prefix/a".to_string(), "prefix/b".to_string()]);

        let mut entries = store.list_entries("ns", "prefix/").await.unwrap();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(entries[0], ("prefix/a".to_string(), b"one".to_vec()));
        assert_eq!(entries[1], ("prefix/b".to_string(), b"two".to_vec()));

        assert!(store
            .compare_and_swap("ns", "prefix/a", Some(b"one"), b"updated")
            .await
            .unwrap());
        assert!(!store
            .compare_and_swap("ns", "prefix/a", Some(b"wrong"), b"nope")
            .await
            .unwrap());
        assert!(store
            .compare_and_swap("ns", "new/key", None, b"created")
            .await
            .unwrap());
        assert!(!store
            .compare_and_swap("ns", "new/key", None, b"duplicate")
            .await
            .unwrap());

        store.delete("ns", "new/key").await.unwrap();
        assert!(store.get("ns", "new/key").await.unwrap().is_none());

        store.delete_prefix("ns", "prefix/").await.unwrap();
        assert!(store.get("ns", "prefix/a").await.unwrap().is_none());
        assert!(store.get("ns", "prefix/b").await.unwrap().is_none());
        assert_eq!(
            store.get("ns", "other/c").await.unwrap(),
            Some(b"three".to_vec())
        );
    }
}
