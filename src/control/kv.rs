use crate::control::KeyValueStore;
use anyhow::Result;
use serde::{de::DeserializeOwned, Serialize};
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

pub struct PostgresKvStore {
    pool: PgPool,
    table: String,
}

impl PostgresKvStore {
    pub async fn new(url: &str, table: &str) -> Result<Self> {
        let pool = PgPool::connect(url).await?;

        let create_stmt = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                namespace VARCHAR(255) NOT NULL,
                key VARCHAR(255) NOT NULL,
                value BYTEA NOT NULL,
                PRIMARY KEY (namespace, key)
            )",
            table
        );
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
        let query = format!(
            "SELECT value FROM {} WHERE namespace = $1 AND key = $2",
            self.table
        );
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
        let query = format!(
            "INSERT INTO {} (namespace, key, value) VALUES ($1, $2, $3) 
             ON CONFLICT (namespace, key) DO UPDATE SET value = $3",
            self.table
        );
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
        let query = match expected {
            Some(_) => format!(
                "UPDATE {} SET value = $3
                 WHERE namespace = $1 AND key = $2 AND value = $4",
                self.table
            ),
            None => format!(
                "INSERT INTO {} (namespace, key, value) VALUES ($1, $2, $3)
                 ON CONFLICT (namespace, key) DO NOTHING",
                self.table
            ),
        };

        let mut q = sqlx::query(&query).bind(namespace).bind(key).bind(value);
        if let Some(expected) = expected {
            q = q.bind(expected);
        }
        let rows_affected = q.execute(&self.pool).await?.rows_affected();
        Ok(rows_affected == 1)
    }

    async fn delete(&self, namespace: &str, key: &str) -> Result<()> {
        let query = format!(
            "DELETE FROM {} WHERE namespace = $1 AND key = $2",
            self.table
        );
        sqlx::query(&query)
            .bind(namespace)
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_keys(&self, namespace: &str, prefix: &str) -> Result<Vec<String>> {
        let query = format!(
            "SELECT key FROM {} WHERE namespace = $1 AND key LIKE $2 ESCAPE '\\'",
            self.table
        );
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
        let query = format!(
            "SELECT key, value FROM {} WHERE namespace = $1 AND key LIKE $2 ESCAPE '\\'",
            self.table
        );
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
        let query = format!(
            "DELETE FROM {} WHERE namespace = $1 AND key LIKE $2 ESCAPE '\\'",
            self.table
        );
        let prefix_pattern = like_prefix_pattern(prefix);
        sqlx::query(&query)
            .bind(namespace)
            .bind(prefix_pattern)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
