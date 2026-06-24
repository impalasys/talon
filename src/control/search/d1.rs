// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::store::{sort_results, DocumentStore};
use super::{
    document_attributes, document_ref, document_source, next_page_token, page_offset, query_terms,
    search_limit, search_mode, search_namespaces, search_sort, snippet, DeleteScope, Document,
    DocumentExt, SearchResponse, SearchResult, ATTR_AGENT, ATTR_CHANNEL, ATTR_MESSAGE_ID,
    ATTR_PART_ID, ATTR_PART_TYPE, ATTR_ROLE, ATTR_RUN_ID, ATTR_SESSION_ID,
};
use crate::gateway::rpc::proto;
use anyhow::{anyhow, Result};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

const CLOUDFLARE_HTTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct D1DocumentStore {
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

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum D1Param {
    Text { value: String },
    Number { value: f64 },
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
    fn text(value: impl AsRef<str>) -> Self {
        Self::Text {
            value: value.as_ref().to_string(),
        }
    }

    fn integer(value: i64) -> Self {
        Self::Number {
            value: value as f64,
        }
    }
}

impl D1DocumentStore {
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
        for statement in schema_statements() {
            self.execute_run(statement.to_string(), vec![]).await?;
        }
        let columns = self
            .execute_all("PRAGMA table_info(talon_documents)".to_string(), vec![])
            .await?
            .into_iter()
            .filter_map(|row| cell_string(&row, "name").ok())
            .collect::<std::collections::HashSet<_>>();
        if !columns.contains("document_kind") {
            self.execute_run(
                "ALTER TABLE talon_documents ADD COLUMN document_kind TEXT NOT NULL DEFAULT ''"
                    .to_string(),
                vec![],
            )
            .await?;
        }
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

#[async_trait::async_trait]
impl DocumentStore for D1DocumentStore {
    async fn upsert_documents(&self, documents: &[Document]) -> Result<()> {
        for document in documents {
            let labels_json = serde_json::to_string(document.labels())?;
            let metadata_json = json_or_empty_object(document.metadata_json()).to_string();
            let acl_scope_json = json_or_empty_object(document.acl_scope_json()).to_string();
            let snippet = snippet(&document.text);
            self.execute_run(
                UPSERT_DOCUMENT_SQL.to_string(),
                vec![
                    D1Param::text(document.namespace()),
                    D1Param::text(document.id()),
                    D1Param::text(document.resource_kind()),
                    D1Param::text(document.resource_key()),
                    D1Param::text(document.document_kind()),
                    D1Param::text(document.parent_kind()),
                    D1Param::text(document.parent_key()),
                    D1Param::text(document.agent()),
                    D1Param::text(document.session_id()),
                    D1Param::text(document.channel()),
                    D1Param::text(document.message_id()),
                    D1Param::text(document.run_id()),
                    D1Param::text(document.part_id()),
                    D1Param::text(document.part_type()),
                    D1Param::text(document.role()),
                    D1Param::text(document.title()),
                    D1Param::text(&document.text),
                    D1Param::text(&snippet),
                    D1Param::text(labels_json),
                    D1Param::text(metadata_json),
                    D1Param::text(acl_scope_json),
                    D1Param::integer(document.created_at()),
                    D1Param::integer(document.updated_at()),
                    D1Param::integer(document.indexed_at()),
                    D1Param::integer(document.generation() as i64),
                    D1Param::text(document.embedding_ref()),
                ],
            )
            .await?;

            self.execute_run(
                "DELETE FROM talon_documents_fts WHERE namespace = ? AND id = ?".to_string(),
                vec![
                    D1Param::text(document.namespace()),
                    D1Param::text(document.id()),
                ],
            )
            .await?;
            self.execute_run(
                "INSERT INTO talon_documents_fts(namespace, id, title, text, snippet) VALUES (?, ?, ?, ?, ?)".to_string(),
                vec![
                    D1Param::text(document.namespace()),
                    D1Param::text(document.id()),
                    D1Param::text(document.title()),
                    D1Param::text(&document.text),
                    D1Param::text(&snippet),
                ],
            )
            .await?;
        }
        Ok(())
    }

    async fn delete(&self, scope: &DeleteScope) -> Result<u64> {
        if scope.namespace.trim().is_empty() {
            return Ok(0);
        }
        let mut fts = SqlParts::new(
            "DELETE FROM talon_documents_fts WHERE namespace = ? AND id IN (SELECT id FROM talon_documents WHERE namespace = ?",
        );
        fts.params.push(D1Param::text(&scope.namespace));
        fts.params.push(D1Param::text(&scope.namespace));
        push_delete_filters(&mut fts, scope);
        fts.sql.push(')');
        self.execute_run(fts.sql, fts.params).await?;

        let mut docs = SqlParts::new("DELETE FROM talon_documents WHERE namespace = ?");
        docs.params.push(D1Param::text(&scope.namespace));
        push_delete_filters(&mut docs, scope);
        let deleted = self
            .execute_run(docs.sql, docs.params)
            .await?
            .changes
            .unwrap_or_default();
        Ok(deleted)
    }

    async fn search(&self, query: &proto::SearchRequest) -> Result<SearchResponse> {
        self.capabilities().require_mode(search_mode(query))?;
        if search_namespaces(query)
            .iter()
            .all(|namespace| namespace.is_empty())
        {
            return Ok(SearchResponse::default());
        }
        let fts_query = fts5_query(&query.query);
        let use_fts = fts_query.is_some();
        let mut parts = if use_fts {
            SqlParts::new(
                "SELECT d.*, -bm25(talon_documents_fts) AS score FROM talon_documents d \
                 JOIN talon_documents_fts ON talon_documents_fts.namespace = d.namespace \
                 AND talon_documents_fts.id = d.id WHERE talon_documents_fts MATCH ?",
            )
        } else {
            SqlParts::new("SELECT d.*, 1.0 AS score FROM talon_documents d WHERE 1 = 1")
        };
        if let Some(fts_query) = fts_query {
            parts.params.push(D1Param::text(fts_query));
        }
        push_query_filters(&mut parts, query);
        match search_sort(query) {
            proto::SearchSort::Recency => parts.sql.push_str(" ORDER BY d.updated_at DESC"),
            proto::SearchSort::Unspecified | proto::SearchSort::Relevance => {
                if use_fts {
                    parts
                        .sql
                        .push_str(" ORDER BY score DESC, d.updated_at DESC");
                } else {
                    parts.sql.push_str(" ORDER BY d.updated_at DESC");
                }
            }
        }
        let limit = search_limit(query);
        let offset = page_offset(&query.page_token)?;
        parts.sql.push_str(" LIMIT ? OFFSET ?");
        parts.params.push(D1Param::integer((limit + 1) as i64));
        parts.params.push(D1Param::integer(offset as i64));
        let mut results = self
            .execute_all(parts.sql, parts.params)
            .await?
            .into_iter()
            .map(|row| {
                let score = cell_f64(&row, "score").unwrap_or(1.0) as f32;
                Ok(SearchResult {
                    document: document_from_row(&row)?,
                    snippet: cell_string(&row, "snippet")?,
                    score,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if !use_fts {
            sort_results(&mut results, search_sort(query));
        }
        let token = next_page_token(offset, limit, results.len());
        results.truncate(limit);
        Ok(SearchResponse {
            results,
            next_page_token: token,
        })
    }

    async fn get_document(&self, namespace: &str, id: &str) -> Result<Option<Document>> {
        self.execute_first(
            "SELECT * FROM talon_documents WHERE namespace = ? AND id = ?".to_string(),
            vec![D1Param::text(namespace), D1Param::text(id)],
        )
        .await?
        .as_ref()
        .map(document_from_row)
        .transpose()
    }
}

struct SqlParts {
    sql: String,
    params: Vec<D1Param>,
}

impl SqlParts {
    fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            params: Vec::new(),
        }
    }
}

fn push_query_filters(parts: &mut SqlParts, query: &proto::SearchRequest) {
    parts.sql.push_str(" AND d.namespace IN (");
    push_placeholders(parts, search_namespaces(query).into_iter());
    parts.sql.push(')');
    if let Some(source) = query.source.as_ref() {
        if !source.key.is_empty() {
            parts.sql.push_str(" AND d.resource_key = ?");
            parts.params.push(D1Param::text(&source.key));
        }
        if !source.key_prefix.is_empty() {
            parts.sql.push_str(" AND d.resource_key LIKE ? ESCAPE '\\'");
            parts
                .params
                .push(D1Param::text(like_prefix_pattern(&source.key_prefix)));
        }
        if !source.parent_key.is_empty() {
            parts.sql.push_str(" AND d.parent_key = ?");
            parts.params.push(D1Param::text(&source.parent_key));
        }
        if !source.kinds.is_empty() {
            parts.sql.push_str(" AND d.resource_kind IN (");
            push_placeholders(parts, source.kinds.iter().map(String::as_str));
            parts.sql.push(')');
        }
    }
    push_attribute_filter(parts, "agent", &query.attributes);
    push_attribute_filter(parts, "session_id", &query.attributes);
    push_attribute_filter(parts, "channel", &query.attributes);
    push_attribute_filter(parts, "role", &query.attributes);
    push_attribute_filter(parts, "part_type", &query.attributes);
    if let Some(start) = query.start_time {
        parts.sql.push_str(" AND d.created_at >= ?");
        parts.params.push(D1Param::integer(start));
    }
    if let Some(end) = query.end_time {
        parts.sql.push_str(" AND d.created_at <= ?");
        parts.params.push(D1Param::integer(end));
    }
    for (key, value) in &query.labels {
        parts.sql.push_str(
            " AND EXISTS (SELECT 1 FROM json_each(d.labels_json) WHERE key = ? AND CAST(value AS TEXT) = ?)",
        );
        parts.params.push(D1Param::text(key));
        parts.params.push(D1Param::text(value));
    }
}

fn push_attribute_filter(
    parts: &mut SqlParts,
    key: &'static str,
    attributes: &HashMap<String, String>,
) {
    let Some(value) = attributes.get(key).filter(|value| !value.is_empty()) else {
        return;
    };
    parts.sql.push_str(" AND d.");
    parts.sql.push_str(key);
    parts.sql.push_str(" = ?");
    parts.params.push(D1Param::text(value));
}

fn push_delete_filters(parts: &mut SqlParts, scope: &DeleteScope) {
    if !scope.resource_kind.is_empty() {
        parts.sql.push_str(" AND resource_kind = ?");
        parts.params.push(D1Param::text(&scope.resource_kind));
    }
    if !scope.resource_key.is_empty() {
        parts.sql.push_str(" AND resource_key = ?");
        parts.params.push(D1Param::text(&scope.resource_key));
    }
    if !scope.resource_key_prefix.is_empty() {
        parts.sql.push_str(" AND resource_key LIKE ? ESCAPE '\\'");
        parts.params.push(D1Param::text(like_prefix_pattern(
            &scope.resource_key_prefix,
        )));
    }
    if !scope.agent.is_empty() {
        parts.sql.push_str(" AND agent = ?");
        parts.params.push(D1Param::text(&scope.agent));
    }
    if !scope.session_id.is_empty() {
        parts.sql.push_str(" AND session_id = ?");
        parts.params.push(D1Param::text(&scope.session_id));
    }
    if !scope.channel.is_empty() {
        parts.sql.push_str(" AND channel = ?");
        parts.params.push(D1Param::text(&scope.channel));
    }
    if scope.max_source_generation > 0 {
        parts.sql.push_str(" AND source_generation <= ?");
        parts
            .params
            .push(D1Param::integer(scope.max_source_generation as i64));
    }
}

fn push_placeholders<'a>(parts: &mut SqlParts, values: impl Iterator<Item = &'a str>) {
    let mut first = true;
    for value in values {
        if !first {
            parts.sql.push_str(", ");
        }
        first = false;
        parts.sql.push('?');
        parts.params.push(D1Param::text(value));
    }
}

fn schema_statements() -> &'static [&'static str] {
    &[
        r#"
        CREATE TABLE IF NOT EXISTS talon_documents (
            namespace TEXT NOT NULL,
            id TEXT NOT NULL,
            resource_kind TEXT NOT NULL,
            resource_key TEXT NOT NULL,
            document_kind TEXT NOT NULL DEFAULT '',
            parent_kind TEXT NOT NULL,
            parent_key TEXT NOT NULL,
            agent TEXT NOT NULL,
            session_id TEXT NOT NULL,
            channel TEXT NOT NULL,
            message_id TEXT NOT NULL,
            run_id TEXT NOT NULL,
            part_id TEXT NOT NULL,
            part_type TEXT NOT NULL,
            role TEXT NOT NULL,
            title TEXT NOT NULL,
            text TEXT NOT NULL,
            snippet TEXT NOT NULL,
            labels_json TEXT NOT NULL,
            metadata_json TEXT NOT NULL,
            acl_scope_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            indexed_at INTEGER NOT NULL,
            source_generation INTEGER NOT NULL,
            embedding_ref TEXT NOT NULL,
            PRIMARY KEY(namespace, id)
        )
        "#,
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS talon_documents_fts
        USING fts5(namespace UNINDEXED, id UNINDEXED, title, text, snippet)
        "#,
        "CREATE INDEX IF NOT EXISTS idx_talon_documents_resource ON talon_documents(namespace, resource_kind, resource_key)",
        "CREATE INDEX IF NOT EXISTS idx_talon_documents_agent_session ON talon_documents(namespace, agent, session_id)",
        "CREATE INDEX IF NOT EXISTS idx_talon_documents_channel ON talon_documents(namespace, channel)",
        "CREATE INDEX IF NOT EXISTS idx_talon_documents_updated ON talon_documents(namespace, updated_at)",
    ]
}

const UPSERT_DOCUMENT_SQL: &str = r#"
INSERT INTO talon_documents (
    namespace, id, resource_kind, resource_key, document_kind, parent_kind, parent_key,
    agent, session_id, channel, message_id, run_id, part_id, part_type, role,
    title, text, snippet, labels_json, metadata_json, acl_scope_json,
    created_at, updated_at, indexed_at, source_generation, embedding_ref
)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(namespace, id) DO UPDATE SET
    resource_kind=excluded.resource_kind,
    resource_key=excluded.resource_key,
    document_kind=excluded.document_kind,
    parent_kind=excluded.parent_kind,
    parent_key=excluded.parent_key,
    agent=excluded.agent,
    session_id=excluded.session_id,
    channel=excluded.channel,
    message_id=excluded.message_id,
    run_id=excluded.run_id,
    part_id=excluded.part_id,
    part_type=excluded.part_type,
    role=excluded.role,
    title=excluded.title,
    text=excluded.text,
    snippet=excluded.snippet,
    labels_json=excluded.labels_json,
    metadata_json=excluded.metadata_json,
    acl_scope_json=excluded.acl_scope_json,
    created_at=excluded.created_at,
    updated_at=excluded.updated_at,
    indexed_at=excluded.indexed_at,
    source_generation=excluded.source_generation,
    embedding_ref=excluded.embedding_ref
"#;

fn document_from_row(row: &D1Row) -> Result<Document> {
    let labels_json = cell_string(row, "labels_json")?;
    let part_id = cell_string(row, "part_id")?;
    Ok(Document {
        r#ref: Some(crate::gateway::rpc::data_proto::DocumentRef {
            attributes: document_attributes([
                (ATTR_AGENT, cell_string(row, "agent")?),
                (ATTR_SESSION_ID, cell_string(row, "session_id")?),
                (ATTR_CHANNEL, cell_string(row, "channel")?),
                (ATTR_MESSAGE_ID, cell_string(row, "message_id")?),
                (ATTR_RUN_ID, cell_string(row, "run_id")?),
                (ATTR_PART_ID, part_id.clone()),
                (ATTR_PART_TYPE, cell_string(row, "part_type")?),
                (ATTR_ROLE, cell_string(row, "role")?),
            ]),
            title: cell_string(row, "title")?,
            labels: serde_json::from_str::<HashMap<String, String>>(&labels_json)
                .unwrap_or_default(),
            metadata_json: cell_string(row, "metadata_json")?,
            acl_scope_json: cell_string(row, "acl_scope_json")?,
            created_at: cell_i64(row, "created_at")?,
            updated_at: cell_i64(row, "updated_at")?,
            indexed_at: cell_i64(row, "indexed_at")?,
            generation: cell_i64(row, "source_generation")? as u64,
            embedding_ref: cell_string(row, "embedding_ref")?,
            ..document_ref(
                cell_string(row, "id")?,
                document_source(
                    cell_string(row, "namespace")?,
                    cell_string(row, "resource_kind")?,
                    cell_string(row, "resource_key")?,
                    cell_string(row, "parent_kind")?,
                    cell_string(row, "parent_key")?,
                ),
                cell_string(row, "document_kind").unwrap_or_default(),
                part_id,
            )
        }),
        text: cell_string(row, "text")?,
    })
}

fn cell_string(row: &D1Row, column: &str) -> Result<String> {
    match row.get(column) {
        Some(D1Cell::Text { value }) => Ok(value.clone()),
        Some(D1Cell::Number { value }) => Ok(value.to_string()),
        Some(D1Cell::Bool { value }) => Ok(value.to_string()),
        Some(D1Cell::Null) => Err(anyhow!("Cloudflare D1 column {column} was null")),
        Some(D1Cell::Bytes { value_base64 }) => Err(anyhow!(
            "Cloudflare D1 column {column} was bytes ({} base64 chars)",
            value_base64.len()
        )),
        None => Err(anyhow!("Cloudflare D1 row missing column {column}")),
    }
}

fn cell_i64(row: &D1Row, column: &str) -> Result<i64> {
    match row.get(column) {
        Some(D1Cell::Number { value }) => Ok(*value as i64),
        Some(D1Cell::Text { value }) => Ok(value.parse()?),
        Some(D1Cell::Bool { value }) => Ok(i64::from(*value)),
        Some(D1Cell::Null) => Err(anyhow!("Cloudflare D1 column {column} was null")),
        Some(D1Cell::Bytes { value_base64 }) => Err(anyhow!(
            "Cloudflare D1 column {column} was bytes ({} base64 chars)",
            value_base64.len()
        )),
        None => Err(anyhow!("Cloudflare D1 row missing column {column}")),
    }
}

fn cell_f64(row: &D1Row, column: &str) -> Result<f64> {
    match row.get(column) {
        Some(D1Cell::Number { value }) => Ok(*value),
        Some(D1Cell::Text { value }) => Ok(value.parse()?),
        Some(D1Cell::Bool { value }) => Ok(if *value { 1.0 } else { 0.0 }),
        Some(D1Cell::Null) => Err(anyhow!("Cloudflare D1 column {column} was null")),
        Some(D1Cell::Bytes { value_base64 }) => Err(anyhow!(
            "Cloudflare D1 column {column} was bytes ({} base64 chars)",
            value_base64.len()
        )),
        None => Err(anyhow!("Cloudflare D1 row missing column {column}")),
    }
}

fn fts5_query(query: &str) -> Option<String> {
    let terms = query_terms(query);
    if terms.is_empty() {
        return None;
    }
    Some(
        terms
            .into_iter()
            .map(|term| format!("{term}*"))
            .collect::<Vec<_>>()
            .join(" AND "),
    )
}

fn like_prefix_pattern(prefix: &str) -> String {
    let mut escaped = String::with_capacity(prefix.len() + 1);
    for ch in prefix.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped.push('%');
    escaped
}

fn json_or_empty_object(value: &str) -> &str {
    if value.trim().is_empty() {
        "{}"
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d1_like_prefix_pattern_escapes_wildcards() {
        assert_eq!(like_prefix_pattern(r"a_b%c\d"), r"a\_b\%c\\d%");
    }

    #[test]
    fn d1_search_query_builds_label_and_namespace_filters() {
        let mut parts = SqlParts::new("SELECT d.* FROM talon_documents d WHERE 1 = 1");
        push_query_filters(
            &mut parts,
            &proto::SearchRequest {
                source: Some(proto::SearchSourceFilter {
                    namespaces: vec!["acme".to_string(), "acme/dev".to_string()],
                    kinds: vec!["Knowledge".to_string()],
                    ..Default::default()
                }),
                labels: [("talon.io/agent".to_string(), "support".to_string())]
                    .into_iter()
                    .collect(),
                ..Default::default()
            },
        );
        assert!(parts.sql.contains("d.namespace IN (?, ?)"));
        assert!(parts.sql.contains("json_each(d.labels_json)"));
        assert_eq!(parts.params.len(), 5);
    }
}
