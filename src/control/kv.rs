// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::KeyValueStore;
use anyhow::Result;
use sqlx::{PgPool, Row};

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
    format!("SELECT value FROM {} WHERE namespace = $1 AND key = $2", table)
}

fn set_query(table: &str) -> String {
    format!(
        "INSERT INTO {} (namespace, key, value) VALUES ($1, $2, $3) 
             ON CONFLICT (namespace, key) DO UPDATE SET value = $3",
        table
    )
}

fn compare_and_swap_query(table: &str, expected: bool) -> String {
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
    format!("DELETE FROM {} WHERE namespace = $1 AND key = $2", table)
}

fn list_keys_query(table: &str) -> String {
    format!(
        "SELECT key FROM {} WHERE namespace = $1 AND key LIKE $2 ESCAPE '\\'",
        table
    )
}

fn list_entries_query(table: &str) -> String {
    format!(
        "SELECT key, value FROM {} WHERE namespace = $1 AND key LIKE $2 ESCAPE '\\'",
        table
    )
}

fn delete_prefix_query(table: &str) -> String {
    format!(
        "DELETE FROM {} WHERE namespace = $1 AND key LIKE $2 ESCAPE '\\'",
        table
    )
}

pub struct PostgresKvStore {
    pool: PgPool,
    table: String,
}

impl PostgresKvStore {
    pub async fn new(url: &str, table: &str) -> Result<Self> {
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
        PostgresKvStore,
    };
    use crate::control::KeyValueStore;
    use std::process::Command;
    use std::sync::OnceLock;
    use std::time::Duration;

    #[test]
    fn like_prefix_pattern_appends_sql_wildcard() {
        assert_eq!(like_prefix_pattern("Agent/test"), "Agent/test%");
    }

    #[test]
    fn like_prefix_pattern_escapes_like_metacharacters() {
        assert_eq!(
            like_prefix_pattern(r"Agent_%\path"),
            r"Agent\_\%\\path%"
        );
    }

    #[test]
    fn sql_builders_use_expected_table_and_clauses() {
        let create = create_table_statement("talon_kv");
        assert!(create.contains("CREATE TABLE IF NOT EXISTS talon_kv"));
        assert!(create.contains("PRIMARY KEY (namespace, key)"));

        assert_eq!(
            get_query("talon_kv"),
            "SELECT value FROM talon_kv WHERE namespace = $1 AND key = $2"
        );
        assert!(set_query("talon_kv").contains("ON CONFLICT (namespace, key) DO UPDATE"));
        assert!(compare_and_swap_query("talon_kv", true).contains("AND value = $4"));
        assert!(compare_and_swap_query("talon_kv", false).contains("DO NOTHING"));
        assert_eq!(
            delete_query("talon_kv"),
            "DELETE FROM talon_kv WHERE namespace = $1 AND key = $2"
        );
        assert!(list_keys_query("talon_kv").contains("LIKE $2 ESCAPE '\\'"));
        assert!(list_entries_query("talon_kv").contains("SELECT key, value"));
        assert!(delete_prefix_query("talon_kv").contains("DELETE FROM talon_kv"));
    }

    fn docker_test_mutex() -> &'static std::sync::Mutex<()> {
        static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    fn docker_test_guard() -> std::sync::MutexGuard<'static, ()> {
        docker_test_mutex()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct PostgresContainer {
        name: String,
        port: u16,
    }

    impl PostgresContainer {
        fn start() -> Self {
            let name = format!("talon-kv-pg-{}", uuid::Uuid::now_v7());
            let run = Command::new("docker")
                .args([
                    "run",
                    "-d",
                    "--rm",
                    "--name",
                    &name,
                    "-e",
                    "POSTGRES_USER=talon",
                    "-e",
                    "POSTGRES_PASSWORD=password",
                    "-e",
                    "POSTGRES_DB=talon",
                    "-p",
                    "127.0.0.1::5432",
                    "postgres:15-alpine",
                ])
                .output()
                .expect("docker run should succeed");
            assert!(
                run.status.success(),
                "docker run failed: {}",
                String::from_utf8_lossy(&run.stderr)
            );

            let inspect = Command::new("docker")
                .args([
                    "inspect",
                    "-f",
                    "{{(index (index .NetworkSettings.Ports \"5432/tcp\") 0).HostPort}}",
                    &name,
                ])
                .output()
                .expect("docker inspect should succeed");
            assert!(
                inspect.status.success(),
                "docker inspect failed: {}",
                String::from_utf8_lossy(&inspect.stderr)
            );
            let port = String::from_utf8_lossy(&inspect.stdout)
                .trim()
                .parse::<u16>()
                .expect("host port should parse");

            for _ in 0..30 {
                let ready = Command::new("docker")
                    .args(["exec", &name, "pg_isready", "-U", "talon", "-d", "talon"])
                    .output()
                    .expect("docker exec should succeed");
                if ready.status.success() {
                    return Self { name, port };
                }
                std::thread::sleep(Duration::from_millis(500));
            }

            panic!("postgres container did not become ready");
        }

        fn database_url(&self) -> String {
            format!(
                "postgres://talon:password@127.0.0.1:{}/talon",
                self.port
            )
        }
    }

    impl Drop for PostgresContainer {
        fn drop(&mut self) {
            let _ = Command::new("docker")
                .args(["rm", "-f", &self.name])
                .output();
        }
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
        let pg = PostgresContainer::start();
        let store = init_test_store(&pg.database_url()).await;

        assert!(store.get("ns", "missing").await.unwrap().is_none());

        store.set("ns", "prefix/a", b"one").await.unwrap();
        store.set("ns", "prefix/b", b"two").await.unwrap();
        store.set("ns", "other/c", b"three").await.unwrap();
        assert_eq!(store.get("ns", "prefix/a").await.unwrap(), Some(b"one".to_vec()));

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
        assert_eq!(store.get("ns", "other/c").await.unwrap(), Some(b"three".to_vec()));
    }
}
