// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::KeyValueStore;
use anyhow::Result;
use sqlx::{PgPool, Row};

use super::shared::{like_prefix_pattern, quoted_identifier, validate_identifier};

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

fn list_keys_page_query(table: &str) -> String {
    format!(
        "SELECT key FROM {}
         WHERE namespace = $1
           AND key LIKE $2 ESCAPE '\\'
           AND ($3 IS NULL OR key < $3)
         ORDER BY key DESC
         LIMIT $4",
        quoted_identifier(table)
    )
}

fn list_entries_page_query(table: &str) -> String {
    format!(
        "SELECT key, value FROM {}
         WHERE namespace = $1
           AND key LIKE $2 ESCAPE '\\'
           AND ($3 IS NULL OR key < $3)
         ORDER BY key DESC
         LIMIT $4",
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

    async fn list_keys_page(
        &self,
        namespace: &str,
        prefix: &str,
        before_key: Option<&str>,
        limit: usize,
    ) -> Result<Vec<String>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let query = list_keys_page_query(&self.table);
        let prefix_pattern = like_prefix_pattern(prefix);
        let rows = sqlx::query(&query)
            .bind(namespace)
            .bind(prefix_pattern)
            .bind(before_key)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?;

        let mut keys = Vec::with_capacity(rows.len());
        for row in rows {
            keys.push(row.try_get("key")?);
        }
        Ok(keys)
    }

    async fn list_entries_page(
        &self,
        namespace: &str,
        prefix: &str,
        before_key: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let query = list_entries_page_query(&self.table);
        let prefix_pattern = like_prefix_pattern(prefix);
        let rows = sqlx::query(&query)
            .bind(namespace)
            .bind(prefix_pattern)
            .bind(before_key)
            .bind(limit as i64)
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
        get_query, list_entries_page_query, list_entries_query, list_keys_page_query,
        list_keys_query, set_query, PostgresKvStore,
    };
    use crate::control::KeyValueStore;
    use crate::test_support::{docker_test_guard, PostgresContainer};
    use std::time::Duration;

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
        assert!(list_keys_page_query("talon_kv").contains("ORDER BY key DESC"));
        assert!(list_entries_page_query("talon_kv").contains("SELECT key, value"));
        assert!(list_entries_page_query("talon_kv").contains("ORDER BY key DESC"));
        assert!(list_entries_query("talon_kv").contains("SELECT key, value"));
        assert!(delete_prefix_query("talon_kv").contains("DELETE FROM \"talon_kv\""));
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

        assert_eq!(
            store
                .list_keys_page("ns", "prefix/", None, 10)
                .await
                .unwrap(),
            vec!["prefix/b".to_string(), "prefix/a".to_string()]
        );
        assert_eq!(
            store
                .list_keys_page("ns", "prefix/", Some("prefix/b"), 10)
                .await
                .unwrap(),
            vec!["prefix/a".to_string()]
        );
        assert_eq!(
            store
                .list_entries_page("ns", "prefix/", None, 10)
                .await
                .unwrap(),
            vec![
                ("prefix/b".to_string(), b"two".to_vec()),
                ("prefix/a".to_string(), b"one".to_vec())
            ]
        );

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
