// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{
    keys::{ResourceKey, ResourceList},
    KeyValueStore,
};
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

use super::sqlite_sql::{
    compare_and_swap_query, create_table_statement, delete_query, get_query,
    list_entries_page_query, list_entries_query, list_keys_page_query, list_keys_query, set_query,
};

const TABLE: &str = "talon_kv_store";
const CLOUDFLARE_HTTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct D1KvStore {
    client: reqwest::Client,
    endpoint: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteRequest {
    mode: ExecuteMode,
    sql: String,
    params: Vec<D1Param>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum ExecuteMode {
    Run,
    All,
    First,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum D1Param {
    Null,
    Text {
        value: String,
    },
    Number {
        value: f64,
    },
    Bytes {
        #[serde(rename = "valueBase64")]
        value_base64: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum D1Cell {
    Null,
    Text {
        value: String,
    },
    Number {
        value: f64,
    },
    Bool {
        value: bool,
    },
    Bytes {
        #[serde(rename = "valueBase64")]
        value_base64: String,
    },
}

#[derive(Debug, Default, Deserialize)]
struct ExecuteMeta {
    changes: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RunResponse {
    meta: ExecuteMeta,
}

#[derive(Debug, Deserialize)]
struct FirstResponse {
    row: Option<D1Row>,
}

#[derive(Debug, Deserialize)]
struct AllResponse {
    results: Vec<D1Row>,
}

type D1Row = HashMap<String, D1Cell>;

impl D1Param {
    fn text(value: impl Into<String>) -> Self {
        Self::Text {
            value: value.into(),
        }
    }

    fn integer(value: i64) -> Self {
        Self::Number {
            value: value as f64,
        }
    }

    fn bytes(value: &[u8]) -> Self {
        Self::Bytes {
            value_base64: general_purpose::STANDARD.encode(value),
        }
    }
}

impl D1KvStore {
    pub fn from_env() -> Self {
        let endpoint = std::env::var("TALON_CLOUDFLARE_D1_URL")
            .unwrap_or_else(|_| "http://talon-d1.internal".to_string());
        Self::new(endpoint)
    }

    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(CLOUDFLARE_HTTP_TIMEOUT)
                .build()
                .expect("Cloudflare D1 HTTP client should build"),
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
        }
    }

    pub async fn init(&self) -> Result<()> {
        self.execute_run(create_table_statement(TABLE), vec![])
            .await?;
        Ok(())
    }

    async fn execute_run(&self, sql: String, params: Vec<D1Param>) -> Result<ExecuteMeta> {
        let response: RunResponse = self
            .post_json(
                "/execute",
                &ExecuteRequest {
                    mode: ExecuteMode::Run,
                    sql,
                    params,
                },
            )
            .await?;
        Ok(response.meta)
    }

    async fn execute_first(&self, sql: String, params: Vec<D1Param>) -> Result<Option<D1Row>> {
        let response: FirstResponse = self
            .post_json(
                "/execute",
                &ExecuteRequest {
                    mode: ExecuteMode::First,
                    sql,
                    params,
                },
            )
            .await?;
        Ok(response.row)
    }

    async fn execute_all(&self, sql: String, params: Vec<D1Param>) -> Result<Vec<D1Row>> {
        let response: AllResponse = self
            .post_json(
                "/execute",
                &ExecuteRequest {
                    mode: ExecuteMode::All,
                    sql,
                    params,
                },
            )
            .await?;
        Ok(response.results)
    }

    async fn post_json<TReq, TResp>(&self, path: &str, body: &TReq) -> Result<TResp>
    where
        TReq: Serialize + ?Sized,
        TResp: for<'de> Deserialize<'de>,
    {
        let response = self
            .client
            .post(format!("{}{}", self.endpoint, path))
            .timeout(CLOUDFLARE_HTTP_TIMEOUT)
            .json(body)
            .send()
            .await?;
        if response.status() == StatusCode::NO_CONTENT {
            return serde_json::from_str("null").map_err(Into::into);
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Cloudflare D1 request {path} failed with HTTP {status}: {body}"
            ));
        }
        Ok(response.json::<TResp>().await?)
    }
}

fn key_params(key: &ResourceKey) -> Vec<D1Param> {
    vec![
        D1Param::text(key.namespace.as_str()),
        D1Param::text(key.parent_path.as_str()),
        D1Param::text(key.kind.as_str()),
        D1Param::text(key.name.as_str()),
    ]
}

fn list_params(list: &ResourceList) -> Vec<D1Param> {
    vec![
        D1Param::text(list.parent.namespace.as_str()),
        D1Param::text(list.parent.parent_path.as_str()),
    ]
}

fn cell_string(row: &D1Row, column: &str) -> Result<String> {
    match row.get(column) {
        Some(D1Cell::Text { value }) => Ok(value.clone()),
        Some(D1Cell::Number { value }) => Ok(value.to_string()),
        Some(D1Cell::Bool { value }) => Ok(value.to_string()),
        Some(D1Cell::Null) => Err(anyhow!("Cloudflare D1 column {column} was null")),
        Some(D1Cell::Bytes { .. }) => Err(anyhow!("Cloudflare D1 column {column} was bytes")),
        None => Err(anyhow!("Cloudflare D1 row missing column {column}")),
    }
}

fn cell_bytes(row: &D1Row, column: &str) -> Result<Vec<u8>> {
    match row.get(column) {
        Some(D1Cell::Bytes { value_base64 }) => Ok(general_purpose::STANDARD.decode(value_base64)?),
        Some(D1Cell::Text { value }) => Ok(value.as_bytes().to_vec()),
        Some(D1Cell::Null) => Err(anyhow!("Cloudflare D1 column {column} was null")),
        Some(D1Cell::Number { .. } | D1Cell::Bool { .. }) => {
            Err(anyhow!("Cloudflare D1 column {column} was not bytes"))
        }
        None => Err(anyhow!("Cloudflare D1 row missing column {column}")),
    }
}

fn key_from_row(row: &D1Row) -> Result<ResourceKey> {
    Ok(ResourceKey {
        namespace: cell_string(row, "namespace")?,
        parent_path: cell_string(row, "parent_path")?,
        kind: cell_string(row, "kind")?,
        name: cell_string(row, "name")?,
    })
}

#[async_trait::async_trait]
impl KeyValueStore for D1KvStore {
    async fn get(&self, key: &ResourceKey) -> Result<Option<Vec<u8>>> {
        let row = self
            .execute_first(get_query(TABLE), key_params(key))
            .await?;
        row.as_ref().map(|row| cell_bytes(row, "value")).transpose()
    }

    async fn set(&self, key: &ResourceKey, value: &[u8]) -> Result<()> {
        let mut params = key_params(key);
        params.push(D1Param::bytes(value));
        self.execute_run(set_query(TABLE), params).await?;
        Ok(())
    }

    async fn compare_and_swap(
        &self,
        key: &ResourceKey,
        expected: Option<&[u8]>,
        value: &[u8],
    ) -> Result<bool> {
        let mut params = key_params(key);
        params.push(D1Param::bytes(value));
        if let Some(expected) = expected {
            params.push(D1Param::bytes(expected));
        }
        let meta = self
            .execute_run(compare_and_swap_query(TABLE, expected.is_some()), params)
            .await?;
        Ok(meta.changes.unwrap_or_default() == 1)
    }

    async fn delete(&self, key: &ResourceKey) -> Result<()> {
        self.execute_run(delete_query(TABLE), key_params(key))
            .await?;
        Ok(())
    }

    async fn list_keys(&self, list: &ResourceList) -> Result<Vec<ResourceKey>> {
        let mut params = list_params(list);
        if let Some(kind) = &list.kind {
            params.push(D1Param::text(kind.as_str()));
        }
        self.execute_all(list_keys_query(TABLE, list.kind.is_some()), params)
            .await?
            .iter()
            .map(key_from_row)
            .collect()
    }

    async fn list_entries(&self, list: &ResourceList) -> Result<Vec<(ResourceKey, Vec<u8>)>> {
        let mut params = list_params(list);
        if let Some(kind) = &list.kind {
            params.push(D1Param::text(kind.as_str()));
        }
        self.execute_all(list_entries_query(TABLE, list.kind.is_some()), params)
            .await?
            .iter()
            .map(|row| Ok((key_from_row(row)?, cell_bytes(row, "value")?)))
            .collect()
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
        let kind = list
            .kind
            .as_ref()
            .ok_or_else(|| anyhow!("d1 list_keys_page requires a resource kind"))?;
        let mut params = list_params(list);
        params.push(D1Param::text(kind.as_str()));
        params.push(match before_name {
            Some(before_name) => D1Param::text(before_name),
            None => D1Param::Null,
        });
        params.push(D1Param::integer(i64::try_from(limit)?));
        self.execute_all(list_keys_page_query(TABLE), params)
            .await?
            .iter()
            .map(key_from_row)
            .collect()
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
        let kind = list
            .kind
            .as_ref()
            .ok_or_else(|| anyhow!("d1 list_entries_page requires a resource kind"))?;
        let mut params = list_params(list);
        params.push(D1Param::text(kind.as_str()));
        params.push(match before_name {
            Some(before_name) => D1Param::text(before_name),
            None => D1Param::Null,
        });
        params.push(D1Param::integer(i64::try_from(limit)?));
        self.execute_all(list_entries_page_query(TABLE), params)
            .await?
            .iter()
            .map(|row| Ok((key_from_row(row)?, cell_bytes(row, "value")?)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, routing::post, Json, Router};
    use serde_json::{json, Value};
    use std::sync::Arc;
    use tokio::{net::TcpListener, sync::Mutex};

    #[derive(Clone, Default)]
    struct Capture {
        request: Arc<Mutex<Option<Value>>>,
    }

    async fn execute_handler(
        State(capture): State<Capture>,
        Json(payload): Json<Value>,
    ) -> Json<Value> {
        *capture.request.lock().await = Some(payload);
        Json(json!({ "meta": { "changes": 1 } }))
    }

    #[tokio::test]
    async fn set_uses_generic_d1_execute_contract() {
        let capture = Capture::default();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/execute", post(execute_handler))
            .with_state(capture.clone());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let store = D1KvStore::new(format!("http://{addr}"));
        store
            .set(
                &ResourceKey {
                    namespace: "default".to_string(),
                    parent_path: "Agent/demo".to_string(),
                    kind: "Session".to_string(),
                    name: "s1".to_string(),
                },
                b"hello",
            )
            .await
            .unwrap();

        let request = capture.request.lock().await.clone().unwrap();
        assert_eq!(request["mode"], "run");
        assert!(request["sql"]
            .as_str()
            .unwrap()
            .contains("INSERT INTO \"talon_kv_store\""));
        assert_eq!(
            request["params"],
            json!([
                { "type": "text", "value": "default" },
                { "type": "text", "value": "Agent/demo" },
                { "type": "text", "value": "Session" },
                { "type": "text", "value": "s1" },
                { "type": "bytes", "valueBase64": "aGVsbG8=" }
            ])
        );
    }
}
