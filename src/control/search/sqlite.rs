// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::store::{sort_results, DocumentStore};
use super::{
    document_attributes, document_source, next_page_token, page_offset, query_terms, search_limit,
    search_mode, search_namespaces, search_sort, DeleteScope, Document, SearchResponse,
    SearchResult, ATTR_AGENT, ATTR_CHANNEL, ATTR_MESSAGE_ID, ATTR_PART_ID, ATTR_PART_TYPE,
    ATTR_ROLE, ATTR_RUN_ID, ATTR_SESSION_ID,
};
use crate::gateway::rpc::proto;
use anyhow::Result;
use sqlx::{
    sqlite::{
        SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow, SqliteSynchronous,
    },
    QueryBuilder, Row, Sqlite, SqlitePool,
};
use std::{collections::HashMap, str::FromStr};

pub struct SqliteDocumentStore {
    pool: SqlitePool,
}

impl SqliteDocumentStore {
    pub async fn new(url: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(std::time::Duration::from_millis(5_000))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        init_schema(&pool).await?;
        Ok(Self { pool })
    }
}

#[async_trait::async_trait]
impl DocumentStore for SqliteDocumentStore {
    async fn upsert_documents(&self, documents: &[Document]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for document in documents {
            let labels_json = serde_json::to_string(&document.labels)?;
            let metadata_json = json_or_empty_object(&document.metadata_json);
            let acl_scope_json = json_or_empty_object(&document.acl_scope_json);
            sqlx::query(
                r#"
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
                "#,
            )
            .bind(document.namespace())
            .bind(&document.id)
            .bind(document.resource_kind())
            .bind(document.resource_key())
            .bind(&document.document_kind)
            .bind(document.parent_kind())
            .bind(document.parent_key())
            .bind(document.agent())
            .bind(document.session_id())
            .bind(document.channel())
            .bind(document.message_id())
            .bind(document.run_id())
            .bind(document.part_id())
            .bind(document.part_type())
            .bind(document.role())
            .bind(&document.title)
            .bind(&document.text)
            .bind(&document.snippet)
            .bind(labels_json)
            .bind(metadata_json)
            .bind(acl_scope_json)
            .bind(document.created_at)
            .bind(document.updated_at)
            .bind(document.indexed_at)
            .bind(document.generation as i64)
            .bind(&document.embedding_ref)
            .execute(&mut *tx)
            .await?;

            sqlx::query("DELETE FROM talon_documents_fts WHERE namespace = ? AND id = ?")
                .bind(document.namespace())
                .bind(&document.id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT INTO talon_documents_fts(namespace, id, title, text, snippet) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(document.namespace())
            .bind(&document.id)
            .bind(&document.title)
            .bind(&document.text)
            .bind(&document.snippet)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn delete(&self, scope: &DeleteScope) -> Result<u64> {
        if scope.namespace.trim().is_empty() {
            return Ok(0);
        }
        let mut tx = self.pool.begin().await?;
        let mut fts_delete =
            QueryBuilder::<Sqlite>::new("DELETE FROM talon_documents_fts WHERE namespace = ");
        fts_delete.push_bind(&scope.namespace);
        fts_delete.push(" AND id IN (SELECT id FROM talon_documents WHERE namespace = ");
        fts_delete.push_bind(&scope.namespace);
        push_delete_filters(&mut fts_delete, scope);
        fts_delete.push(")");
        fts_delete.build().execute(&mut *tx).await?;

        let mut document_delete =
            QueryBuilder::<Sqlite>::new("DELETE FROM talon_documents WHERE namespace = ");
        document_delete.push_bind(&scope.namespace);
        push_delete_filters(&mut document_delete, scope);
        let deleted = document_delete
            .build()
            .execute(&mut *tx)
            .await?
            .rows_affected();
        tx.commit().await?;
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
        let mut builder = if use_fts {
            QueryBuilder::<Sqlite>::new(
                "SELECT d.*, -bm25(talon_documents_fts) AS score FROM talon_documents d \
                 JOIN talon_documents_fts ON talon_documents_fts.namespace = d.namespace \
                 AND talon_documents_fts.id = d.id WHERE talon_documents_fts MATCH ",
            )
        } else {
            QueryBuilder::<Sqlite>::new(
                "SELECT d.*, 1.0 AS score FROM talon_documents d WHERE 1 = 1",
            )
        };
        if let Some(fts_query) = fts_query {
            builder.push_bind(fts_query);
        }
        push_query_filters(&mut builder, query);
        match search_sort(query) {
            proto::SearchSort::Recency => {
                builder.push(" ORDER BY d.updated_at DESC");
            }
            proto::SearchSort::Unspecified | proto::SearchSort::Relevance => {
                if use_fts {
                    builder.push(" ORDER BY score DESC, d.updated_at DESC");
                } else {
                    builder.push(" ORDER BY d.updated_at DESC");
                }
            }
        }
        let limit = search_limit(query);
        let offset = page_offset(&query.page_token)?;
        builder.push(" LIMIT ");
        builder.push_bind((limit + 1) as i64);
        builder.push(" OFFSET ");
        builder.push_bind(offset as i64);
        let rows = builder.build().fetch_all(&self.pool).await?;
        let mut results = rows
            .into_iter()
            .map(|row| {
                let score = row.try_get::<f64, _>("score").unwrap_or(1.0) as f32;
                Ok(SearchResult {
                    document: document_from_sqlite_row(&row)?,
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
        let row = sqlx::query("SELECT * FROM talon_documents WHERE namespace = ? AND id = ?")
            .bind(namespace)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| document_from_sqlite_row(&row)).transpose()
    }
}

async fn init_schema(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
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
    )
    .execute(pool)
    .await?;
    let columns = sqlx::query("PRAGMA table_info(talon_documents)")
        .fetch_all(pool)
        .await?
        .into_iter()
        .filter_map(|row| row.try_get::<String, _>("name").ok())
        .collect::<std::collections::HashSet<_>>();
    if !columns.contains("document_kind") {
        sqlx::query(
            "ALTER TABLE talon_documents ADD COLUMN document_kind TEXT NOT NULL DEFAULT ''",
        )
        .execute(pool)
        .await?;
    }
    sqlx::query(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS talon_documents_fts
        USING fts5(namespace UNINDEXED, id UNINDEXED, title, text, snippet)
        "#,
    )
    .execute(pool)
    .await?;
    for statement in [
        "CREATE INDEX IF NOT EXISTS idx_talon_documents_resource ON talon_documents(namespace, resource_kind, resource_key)",
        "CREATE INDEX IF NOT EXISTS idx_talon_documents_agent_session ON talon_documents(namespace, agent, session_id)",
        "CREATE INDEX IF NOT EXISTS idx_talon_documents_channel ON talon_documents(namespace, channel)",
        "CREATE INDEX IF NOT EXISTS idx_talon_documents_updated ON talon_documents(namespace, updated_at)",
    ] {
        sqlx::query(statement).execute(pool).await?;
    }
    Ok(())
}

fn push_query_filters<'a>(builder: &mut QueryBuilder<'a, Sqlite>, query: &'a proto::SearchRequest) {
    builder.push(" AND d.namespace IN (");
    let mut separated = builder.separated(", ");
    for namespace in search_namespaces(query) {
        separated.push_bind(namespace);
    }
    separated.push_unseparated(")");
    if let Some(source) = query.source.as_ref() {
        if !source.key.is_empty() {
            builder
                .push(" AND d.resource_key = ")
                .push_bind(&source.key);
        }
        if !source.key_prefix.is_empty() {
            builder
                .push(" AND d.resource_key LIKE ")
                .push_bind(like_prefix_pattern(&source.key_prefix))
                .push(" ESCAPE '\\'");
        }
        if !source.parent_key.is_empty() {
            builder
                .push(" AND d.parent_key = ")
                .push_bind(&source.parent_key);
        }
        if !source.kinds.is_empty() {
            builder.push(" AND d.resource_kind IN (");
            let mut separated = builder.separated(", ");
            for kind in &source.kinds {
                separated.push_bind(kind);
            }
            separated.push_unseparated(")");
        }
    }
    push_attribute_filter(builder, "agent", &query.attributes);
    push_attribute_filter(builder, "session_id", &query.attributes);
    push_attribute_filter(builder, "channel", &query.attributes);
    push_attribute_filter(builder, "role", &query.attributes);
    push_attribute_filter(builder, "part_type", &query.attributes);
    if let Some(start) = query.start_time {
        builder.push(" AND d.created_at >= ").push_bind(start);
    }
    if let Some(end) = query.end_time {
        builder.push(" AND d.created_at <= ").push_bind(end);
    }
    for (key, value) in &query.labels {
        builder
            .push(" AND EXISTS (SELECT 1 FROM json_each(d.labels_json) WHERE key = ")
            .push_bind(key)
            .push(" AND CAST(value AS TEXT) = ")
            .push_bind(value);
        builder.push(")");
    }
}

fn push_attribute_filter<'a>(
    builder: &mut QueryBuilder<'a, Sqlite>,
    key: &'static str,
    attributes: &'a HashMap<String, String>,
) {
    let Some(value) = attributes.get(key).filter(|value| !value.is_empty()) else {
        return;
    };
    builder.push(" AND d.");
    builder.push(key);
    builder.push(" = ");
    builder.push_bind(value);
}

fn push_delete_filters<'a>(builder: &mut QueryBuilder<'a, Sqlite>, scope: &'a DeleteScope) {
    if !scope.resource_kind.is_empty() {
        builder
            .push(" AND resource_kind = ")
            .push_bind(&scope.resource_kind);
    }
    if !scope.resource_key.is_empty() {
        builder
            .push(" AND resource_key = ")
            .push_bind(&scope.resource_key);
    }
    if !scope.resource_key_prefix.is_empty() {
        builder
            .push(" AND resource_key LIKE ")
            .push_bind(like_prefix_pattern(&scope.resource_key_prefix))
            .push(" ESCAPE '\\'");
    }
    if !scope.agent.is_empty() {
        builder.push(" AND agent = ").push_bind(&scope.agent);
    }
    if !scope.session_id.is_empty() {
        builder
            .push(" AND session_id = ")
            .push_bind(&scope.session_id);
    }
    if !scope.channel.is_empty() {
        builder.push(" AND channel = ").push_bind(&scope.channel);
    }
    if scope.max_source_generation > 0 {
        builder
            .push(" AND source_generation <= ")
            .push_bind(scope.max_source_generation as i64);
    }
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

fn document_from_sqlite_row(row: &SqliteRow) -> Result<Document> {
    let labels_json: String = row.try_get("labels_json")?;
    let part_id: String = row.try_get("part_id")?;
    Ok(Document {
        id: row.try_get("id")?,
        source: document_source(
            row.try_get("namespace")?,
            row.try_get(DOCUMENTS_TABLE_FIELD_RESOURCE_KIND)?,
            row.try_get("resource_key")?,
            row.try_get("parent_kind")?,
            row.try_get("parent_key")?,
        ),
        document_kind: row.try_get("document_kind").unwrap_or_default(),
        subdocument_id: part_id.clone(),
        attributes: document_attributes([
            (ATTR_AGENT, row.try_get("agent")?),
            (ATTR_SESSION_ID, row.try_get("session_id")?),
            (ATTR_CHANNEL, row.try_get("channel")?),
            (ATTR_MESSAGE_ID, row.try_get("message_id")?),
            (ATTR_RUN_ID, row.try_get("run_id")?),
            (ATTR_PART_ID, part_id),
            (ATTR_PART_TYPE, row.try_get("part_type")?),
            (ATTR_ROLE, row.try_get("role")?),
        ]),
        title: row.try_get("title")?,
        text: row.try_get("text")?,
        snippet: row.try_get("snippet")?,
        labels: serde_json::from_str::<HashMap<String, String>>(&labels_json).unwrap_or_default(),
        metadata_json: row.try_get("metadata_json")?,
        acl_scope_json: row.try_get("acl_scope_json")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        indexed_at: row.try_get("indexed_at")?,
        generation: row.try_get::<i64, _>("source_generation")? as u64,
        embedding_ref: row.try_get("embedding_ref")?,
    })
}

const DOCUMENTS_TABLE_FIELD_RESOURCE_KIND: &str = "resource_kind";

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
    use crate::control::kv::sqlite_url_for_path;
    use crate::control::search::DOCUMENT_KIND_MESSAGE_PART;

    #[tokio::test]
    async fn sqlite_document_store_searches_filters_and_deletes_documents() {
        let dir = tempfile::tempdir().unwrap();
        let url = sqlite_url_for_path(&dir.path().join("documents.db"));
        let store = SqliteDocumentStore::new(&url).await.unwrap();
        let mut labels = HashMap::new();
        labels.insert("tier".to_string(), "gold".to_string());
        labels.insert("talon.io/agent".to_string(), "support".to_string());
        let document = Document {
            id: "doc-1".to_string(),
            source: document_source(
                "acme".to_string(),
                "SessionMessage".to_string(),
                "@Namespace/acme/Agent/support/Session/s1/@/SessionMessage/m1".to_string(),
                "Session".to_string(),
                "@Namespace/acme/Agent/support/@/Session/s1".to_string(),
            ),
            document_kind: DOCUMENT_KIND_MESSAGE_PART.to_string(),
            attributes: document_attributes([
                (ATTR_AGENT, "support".to_string()),
                (ATTR_SESSION_ID, "s1".to_string()),
            ]),
            title: "Refund policy".to_string(),
            text: "Refund policy details for gold customers".to_string(),
            snippet: "Refund policy details".to_string(),
            labels,
            created_at: 10,
            updated_at: 10,
            indexed_at: 20,
            generation: 1,
            ..Default::default()
        };

        store.upsert_documents(&[document.clone()]).await.unwrap();
        store
            .upsert_documents(&[Document {
                text: "Refund policy details updated".to_string(),
                generation: 2,
                ..document.clone()
            }])
            .await
            .unwrap();

        let response = store
            .search(&proto::SearchRequest {
                query: "refund".to_string(),
                source: Some(proto::SearchSourceFilter {
                    namespace: "acme".to_string(),
                    kinds: vec!["SessionMessage".to_string()],
                    ..Default::default()
                }),
                attributes: document_attributes([(ATTR_AGENT, "support".to_string())]),
                labels: [
                    ("tier".to_string(), "gold".to_string()),
                    ("talon.io/agent".to_string(), "support".to_string()),
                ]
                .into_iter()
                .collect(),
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].document.generation, 2);
        assert_eq!(
            response.results[0].document.document_kind,
            DOCUMENT_KIND_MESSAGE_PART
        );

        let prefix_response = store
            .search(&proto::SearchRequest {
                query: "ref".to_string(),
                source: Some(proto::SearchSourceFilter {
                    namespace: "acme".to_string(),
                    kinds: vec!["SessionMessage".to_string()],
                    ..Default::default()
                }),
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(prefix_response.results.len(), 1);

        let deleted = store
            .delete(&DeleteScope {
                namespace: "acme".to_string(),
                resource_key_prefix: "@Namespace/acme/Agent/support".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(deleted, 1);
        assert!(store.get_document("acme", "doc-1").await.unwrap().is_none());
    }

    #[test]
    fn sqlite_like_prefix_pattern_escapes_wildcards() {
        assert_eq!(like_prefix_pattern(r"a_b%c\d"), r"a\_b\%c\\d%");
    }
}
