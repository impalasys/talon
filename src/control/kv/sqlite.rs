// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::KeyValueStore;
use anyhow::Result;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Row, SqlitePool,
};
use std::{str::FromStr, time::Duration};

use super::shared::{like_prefix_pattern, quoted_identifier, validate_identifier};

fn create_table_statement(table: &str) -> String {
    let table = quoted_identifier(table);
    format!(
        "CREATE TABLE IF NOT EXISTS {} (
                key TEXT NOT NULL,
                value BLOB NOT NULL,
                PRIMARY KEY (key)
            )",
        table
    )
}

fn get_query(table: &str) -> String {
    format!(
        "SELECT value FROM {} WHERE key = ?1",
        quoted_identifier(table)
    )
}

fn set_query(table: &str) -> String {
    let table = quoted_identifier(table);
    format!(
        "INSERT INTO {} (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        table
    )
}

fn compare_and_swap_query(table: &str, expected: bool) -> String {
    let table = quoted_identifier(table);
    if expected {
        format!(
            "UPDATE {} SET value = ?3
                 WHERE key = ?1 AND value = ?2",
            table
        )
    } else {
        format!(
            "INSERT INTO {} (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO NOTHING",
            table
        )
    }
}

fn delete_query(table: &str) -> String {
    format!("DELETE FROM {} WHERE key = ?1", quoted_identifier(table))
}

fn list_keys_query(table: &str) -> String {
    format!(
        "SELECT key FROM {} WHERE key LIKE ?1 ESCAPE '\\'",
        quoted_identifier(table)
    )
}

fn list_entries_query(table: &str) -> String {
    format!(
        "SELECT key, value FROM {} WHERE key LIKE ?1 ESCAPE '\\'",
        quoted_identifier(table)
    )
}

fn list_keys_page_query(table: &str) -> String {
    format!(
        "SELECT key FROM {}
         WHERE key LIKE ?1 ESCAPE '\\'
           AND (?2 IS NULL OR key < ?2)
         ORDER BY key DESC
         LIMIT ?3",
        quoted_identifier(table)
    )
}

fn list_entries_page_query(table: &str) -> String {
    format!(
        "SELECT key, value FROM {}
         WHERE key LIKE ?1 ESCAPE '\\'
           AND (?2 IS NULL OR key < ?2)
         ORDER BY key DESC
         LIMIT ?3",
        quoted_identifier(table)
    )
}

fn delete_prefix_query(table: &str) -> String {
    format!(
        "DELETE FROM {} WHERE key LIKE ?1 ESCAPE '\\'",
        quoted_identifier(table)
    )
}

pub struct SqliteKvStore {
    pool: SqlitePool,
    table: String,
}

impl SqliteKvStore {
    pub async fn new(url: &str, table: &str) -> Result<Self> {
        validate_identifier(table)?;
        let pool = sqlite_pool(url).await?;

        migrate_legacy_table(&pool, table).await?;
        let create_stmt = create_table_statement(table);
        sqlx::query(&create_stmt).execute(&pool).await?;

        Ok(Self {
            pool,
            table: table.to_string(),
        })
    }
}

async fn migrate_legacy_table(pool: &SqlitePool, table: &str) -> Result<()> {
    let columns = sqlx::query(&format!("PRAGMA table_info({})", quoted_identifier(table)))
        .fetch_all(pool)
        .await?;
    let has_namespace = columns
        .iter()
        .filter_map(|row| row.try_get::<String, _>("name").ok())
        .any(|column| column == "namespace");
    if !has_namespace {
        return Ok(());
    }

    let mut tx = pool.begin().await?;
    let old_rows = sqlx::query(&format!(
        "SELECT namespace, key, value FROM {}",
        quoted_identifier(table)
    ))
    .fetch_all(&mut *tx)
    .await?;
    let mut converted_rows = Vec::with_capacity(old_rows.len());
    for row in old_rows {
        let namespace: String = row.try_get("namespace")?;
        let old_key: String = row.try_get("key")?;
        let value: Vec<u8> = row.try_get("value")?;
        let new_key = super::legacy::namespaced_key(&namespace, &old_key)?;
        converted_rows.push((new_key, value));
    }

    sqlx::query(&format!("DROP TABLE {}", quoted_identifier(table)))
        .execute(&mut *tx)
        .await?;
    sqlx::query(&create_table_statement(table))
        .execute(&mut *tx)
        .await?;
    let insert = set_query(table);
    for (key, value) in converted_rows {
        sqlx::query(&insert)
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn sqlite_pool(url: &str) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);
    Ok(SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?)
}

#[async_trait::async_trait]
impl KeyValueStore for SqliteKvStore {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let query = get_query(&self.table);
        let row = sqlx::query(&query)
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

    async fn set(&self, key: &str, value: &[u8]) -> Result<()> {
        let query = set_query(&self.table);
        sqlx::query(&query)
            .bind(key)
            .bind(value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn compare_and_swap(
        &self,
        key: &str,
        expected: Option<&[u8]>,
        value: &[u8],
    ) -> Result<bool> {
        let query = compare_and_swap_query(&self.table, expected.is_some());
        let q = if let Some(expected) = expected {
            sqlx::query(&query).bind(key).bind(expected).bind(value)
        } else {
            sqlx::query(&query).bind(key).bind(value)
        };
        let rows_affected = q.execute(&self.pool).await?.rows_affected();
        Ok(rows_affected == 1)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let query = delete_query(&self.table);
        sqlx::query(&query).bind(key).execute(&self.pool).await?;
        Ok(())
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>> {
        let query = list_keys_query(&self.table);
        let prefix_pattern = like_prefix_pattern(prefix);
        let rows = sqlx::query(&query)
            .bind(prefix_pattern)
            .fetch_all(&self.pool)
            .await?;

        let mut keys = Vec::new();
        for row in rows {
            keys.push(row.try_get("key")?);
        }
        Ok(keys)
    }

    async fn list_entries(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let query = list_entries_query(&self.table);
        let prefix_pattern = like_prefix_pattern(prefix);
        let rows = sqlx::query(&query)
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

    async fn delete_prefix(&self, prefix: &str) -> Result<()> {
        let query = delete_prefix_query(&self.table);
        let prefix_pattern = like_prefix_pattern(prefix);
        sqlx::query(&query)
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
        list_keys_query, set_query, sqlite_pool, SqliteKvStore,
    };
    use crate::control::kv::sqlite_url_for_path;
    use crate::control::KeyValueStore;
    use tempfile::tempdir;

    #[test]
    fn sql_builders_use_expected_table_and_clauses() {
        let create = create_table_statement("talon_kv");
        assert!(create.contains("CREATE TABLE IF NOT EXISTS \"talon_kv\""));
        assert!(create.contains("value BLOB NOT NULL"));

        assert_eq!(
            get_query("talon_kv"),
            "SELECT value FROM \"talon_kv\" WHERE key = ?1"
        );
        assert!(set_query("talon_kv").contains("excluded.value"));
        assert!(compare_and_swap_query("talon_kv", true).contains("AND value = ?2"));
        assert!(compare_and_swap_query("talon_kv", false).contains("DO NOTHING"));
        assert_eq!(
            delete_query("talon_kv"),
            "DELETE FROM \"talon_kv\" WHERE key = ?1"
        );
        assert!(list_keys_query("talon_kv").contains("LIKE ?1 ESCAPE '\\'"));
        assert!(list_keys_page_query("talon_kv").contains("ORDER BY key DESC"));
        assert!(list_entries_page_query("talon_kv").contains("SELECT key, value"));
        assert!(list_entries_page_query("talon_kv").contains("ORDER BY key DESC"));
        assert!(list_entries_query("talon_kv").contains("SELECT key, value"));
        assert!(delete_prefix_query("talon_kv").contains("DELETE FROM \"talon_kv\""));
    }

    #[test]
    fn sqlite_legacy_migration_maps_old_system_namespace_names() {
        assert_eq!(
            super::super::legacy::namespaced_key("talon-system:ns", "Namespace/quickstart")
                .unwrap(),
            crate::control::keys::namespace_metadata("quickstart")
        );
        assert_eq!(
            super::super::legacy::namespaced_key(
                "talon-system:ns:internal",
                "NamespaceRef/quickstart"
            )
            .unwrap(),
            crate::control::keys::namespace_ref(None, "quickstart")
        );
    }

    #[tokio::test]
    async fn sqlite_migrates_old_system_namespace_rows() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("talon-kv-legacy.db");
        let url = sqlite_url_for_path(&db_path);
        let setup_pool = sqlite_pool(&url).await.unwrap();
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
    async fn sqlite_kv_round_trip_compare_and_swap_and_prefix_ops() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("talon-kv.db");
        let store = SqliteKvStore::new(&sqlite_url_for_path(&db_path), "talon_kv_store_test")
            .await
            .unwrap();

        assert!(store.get("missing").await.unwrap().is_none());

        store.set("prefix/a", b"one").await.unwrap();
        store.set("prefix/b", b"two").await.unwrap();
        store.set("other/c", b"three").await.unwrap();
        assert_eq!(store.get("prefix/a").await.unwrap(), Some(b"one".to_vec()));

        let mut keys = store.list_keys("prefix/").await.unwrap();
        keys.sort();
        assert_eq!(keys, vec!["prefix/a".to_string(), "prefix/b".to_string()]);

        assert_eq!(
            store.list_keys_page("prefix/", None, 10).await.unwrap(),
            vec!["prefix/b".to_string(), "prefix/a".to_string()]
        );
        assert_eq!(
            store
                .list_keys_page("prefix/", Some("prefix/b"), 10)
                .await
                .unwrap(),
            vec!["prefix/a".to_string()]
        );
        assert_eq!(
            store.list_entries_page("prefix/", None, 10).await.unwrap(),
            vec![
                ("prefix/b".to_string(), b"two".to_vec()),
                ("prefix/a".to_string(), b"one".to_vec())
            ]
        );

        let mut entries = store.list_entries("prefix/").await.unwrap();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(entries[0], ("prefix/a".to_string(), b"one".to_vec()));
        assert_eq!(entries[1], ("prefix/b".to_string(), b"two".to_vec()));

        assert!(store
            .compare_and_swap("prefix/a", Some(b"one"), b"updated")
            .await
            .unwrap());
        assert!(!store
            .compare_and_swap("prefix/a", Some(b"wrong"), b"nope")
            .await
            .unwrap());
        assert!(store
            .compare_and_swap("new/key", None, b"created")
            .await
            .unwrap());
        assert!(!store
            .compare_and_swap("new/key", None, b"duplicate")
            .await
            .unwrap());

        store.delete("new/key").await.unwrap();
        assert!(store.get("new/key").await.unwrap().is_none());

        store.delete_prefix("prefix/").await.unwrap();
        assert!(store.get("prefix/a").await.unwrap().is_none());
        assert!(store.get("prefix/b").await.unwrap().is_none());
        assert_eq!(store.get("other/c").await.unwrap(), Some(b"three".to_vec()));
    }
}
