// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::store::{sort_results, DocumentStore};
use super::{
    document_attributes, document_source, next_page_token, page_offset, query_terms, DeleteScope,
    Document, SearchQuery, SearchResponse, SearchResult, SearchSort, ATTR_AGENT, ATTR_CHANNEL,
    ATTR_MESSAGE_ID, ATTR_PART_ID, ATTR_PART_TYPE, ATTR_ROLE, ATTR_RUN_ID, ATTR_SESSION_ID,
};
use anyhow::Result;
use sqlx::{postgres::PgPoolOptions, PgPool, Postgres, QueryBuilder, Row};
use std::collections::HashMap;

pub struct PostgresDocumentStore {
    pool: PgPool,
}

impl PostgresDocumentStore {
    pub async fn new(url: &str) -> Result<Self> {
        let max_connections = std::env::var("TALON_POSTGRES_MAX_CONNECTIONS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(10);
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(url)
            .await?;
        init_schema(&pool).await?;
        Ok(Self { pool })
    }
}

#[async_trait::async_trait]
impl DocumentStore for PostgresDocumentStore {
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
                    created_at, updated_at, indexed_at, source_generation, embedding_ref,
                    search_vector
                )
                VALUES (
                    $1, $2, $3, $4, $5, $6, $7,
                    $8, $9, $10, $11, $12, $13, $14, $15,
                    $16, $17, $18, $19::jsonb, $20::jsonb, $21::jsonb,
                    $22, $23, $24, $25, $26,
                    to_tsvector('simple', coalesce($16, '') || ' ' || coalesce($17, '') || ' ' || coalesce($18, ''))
                )
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
                    embedding_ref=excluded.embedding_ref,
                    search_vector=excluded.search_vector
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
        }
        tx.commit().await?;
        Ok(())
    }

    async fn delete(&self, scope: &DeleteScope) -> Result<u64> {
        if scope.namespace.trim().is_empty() {
            return Ok(0);
        }
        let mut builder =
            QueryBuilder::<Postgres>::new("DELETE FROM talon_documents WHERE namespace = ");
        builder.push_bind(&scope.namespace);
        push_delete_filters(&mut builder, scope);
        let result = builder.build().execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchResponse> {
        self.capabilities().require_mode(query.mode)?;
        if query
            .source
            .namespaces()
            .iter()
            .all(|namespace| namespace.is_empty())
        {
            return Ok(SearchResponse::default());
        }
        let text_query = prefix_tsquery(&query.query);
        let use_text_query = text_query.is_some();
        let mut builder = QueryBuilder::<Postgres>::new("SELECT ");
        builder.push(SELECT_COLUMNS);
        if let Some(text_query) = text_query {
            builder.push(", ts_rank_cd(d.search_vector, to_tsquery('simple', ");
            builder.push_bind(text_query.clone());
            builder.push(
                ")) AS score FROM talon_documents d WHERE d.search_vector @@ to_tsquery('simple', ",
            );
            builder.push_bind(text_query);
            builder.push(")");
        } else {
            builder.push(", 1.0::real AS score FROM talon_documents d WHERE 1 = 1");
        }
        push_query_filters(&mut builder, query);
        match query.sort {
            SearchSort::Recency => {
                builder.push(" ORDER BY d.updated_at DESC");
            }
            SearchSort::Relevance => {
                if use_text_query {
                    builder.push(" ORDER BY score DESC, d.updated_at DESC");
                } else {
                    builder.push(" ORDER BY d.updated_at DESC");
                }
            }
        }
        let limit = query.limit.clamp(1, 100);
        let offset = page_offset(&query.page_token)?;
        builder.push(" LIMIT ");
        builder.push_bind((limit + 1) as i64);
        builder.push(" OFFSET ");
        builder.push_bind(offset as i64);
        let rows = builder.build().fetch_all(&self.pool).await?;
        let mut results = rows
            .into_iter()
            .map(|row| {
                let score = row.try_get::<f32, _>("score").unwrap_or(1.0);
                Ok(SearchResult {
                    document: document_from_row(&row)?,
                    score,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if !use_text_query {
            sort_results(&mut results, query.sort);
        }
        let token = next_page_token(offset, limit, results.len());
        results.truncate(limit);
        Ok(SearchResponse {
            results,
            next_page_token: token,
        })
    }

    async fn get_document(&self, namespace: &str, id: &str) -> Result<Option<Document>> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLUMNS} FROM talon_documents d WHERE d.namespace = $1 AND d.id = $2"
        ))
        .bind(namespace)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| document_from_row(&row)).transpose()
    }
}

const SELECT_COLUMNS: &str = r#"
    d.namespace,
    d.id,
    d.resource_kind,
    d.resource_key,
    d.document_kind,
    d.parent_kind,
    d.parent_key,
    d.agent,
    d.session_id,
    d.channel,
    d.message_id,
    d.run_id,
    d.part_id,
    d.part_type,
    d.role,
    d.title,
    d.text,
    d.snippet,
    d.labels_json::text AS labels_json,
    d.metadata_json::text AS metadata_json,
    d.acl_scope_json::text AS acl_scope_json,
    d.created_at,
    d.updated_at,
    d.indexed_at,
    d.source_generation,
    d.embedding_ref
"#;

async fn init_schema(pool: &PgPool) -> Result<()> {
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
            labels_json JSONB NOT NULL,
            metadata_json JSONB NOT NULL,
            acl_scope_json JSONB NOT NULL,
            created_at BIGINT NOT NULL,
            updated_at BIGINT NOT NULL,
            indexed_at BIGINT NOT NULL,
            source_generation BIGINT NOT NULL,
            embedding_ref TEXT NOT NULL,
            search_vector TSVECTOR NOT NULL,
            PRIMARY KEY(namespace, id)
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query("ALTER TABLE talon_documents ADD COLUMN IF NOT EXISTS document_kind TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await?;
    for statement in [
        "CREATE INDEX IF NOT EXISTS idx_talon_documents_search ON talon_documents USING GIN(search_vector)",
        "CREATE INDEX IF NOT EXISTS idx_talon_documents_resource ON talon_documents(namespace, resource_kind, resource_key)",
        "CREATE INDEX IF NOT EXISTS idx_talon_documents_agent_session ON talon_documents(namespace, agent, session_id)",
        "CREATE INDEX IF NOT EXISTS idx_talon_documents_channel ON talon_documents(namespace, channel)",
        "CREATE INDEX IF NOT EXISTS idx_talon_documents_updated ON talon_documents(namespace, updated_at)",
        "CREATE INDEX IF NOT EXISTS idx_talon_documents_labels ON talon_documents USING GIN(labels_json)",
    ] {
        sqlx::query(statement).execute(pool).await?;
    }
    Ok(())
}

fn push_query_filters<'a>(builder: &mut QueryBuilder<'a, Postgres>, query: &'a SearchQuery) {
    builder.push(" AND d.namespace IN (");
    let mut separated = builder.separated(", ");
    for namespace in query.source.namespaces() {
        separated.push_bind(namespace);
    }
    separated.push_unseparated(")");
    if !query.source.key.is_empty() {
        builder
            .push(" AND d.resource_key = ")
            .push_bind(&query.source.key);
    }
    if !query.source.key_prefix.is_empty() {
        builder
            .push(" AND d.resource_key LIKE ")
            .push_bind(like_prefix_pattern(&query.source.key_prefix))
            .push(" ESCAPE '\\'");
    }
    if !query.source.parent_key.is_empty() {
        builder
            .push(" AND d.parent_key = ")
            .push_bind(&query.source.parent_key);
    }
    if !query.source.kinds.is_empty() {
        builder.push(" AND d.resource_kind IN (");
        let mut separated = builder.separated(", ");
        for kind in &query.source.kinds {
            separated.push_bind(kind);
        }
        separated.push_unseparated(")");
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
        let filter_json = serde_json::json!({ key: value }).to_string();
        builder
            .push(" AND d.labels_json @> ")
            .push_bind(filter_json)
            .push("::jsonb");
    }
}

fn push_attribute_filter<'a>(
    builder: &mut QueryBuilder<'a, Postgres>,
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

fn push_delete_filters<'a>(builder: &mut QueryBuilder<'a, Postgres>, scope: &'a DeleteScope) {
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

fn document_from_row(row: &sqlx::postgres::PgRow) -> Result<Document> {
    let labels_json: String = row.try_get("labels_json")?;
    let part_id: String = row.try_get("part_id")?;
    Ok(Document {
        id: row.try_get("id")?,
        source: document_source(
            row.try_get("namespace")?,
            row.try_get("resource_kind")?,
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

fn json_or_empty_object(value: &str) -> &str {
    if value.trim().is_empty() {
        "{}"
    } else {
        value
    }
}

fn prefix_tsquery(query: &str) -> Option<String> {
    let terms = query_terms(query);
    if terms.is_empty() {
        return None;
    }
    Some(
        terms
            .into_iter()
            .map(|term| format!("{term}:*"))
            .collect::<Vec<_>>()
            .join(" & "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::search::DOCUMENT_KIND_CONTENT;
    use crate::test_support::{docker_test_guard, PostgresContainer};
    use std::time::Duration;

    async fn init_test_store(database_url: &str) -> PostgresDocumentStore {
        let mut last_error = None;
        for _ in 0..20 {
            match PostgresDocumentStore::new(database_url).await {
                Ok(store) => return store,
                Err(error) => {
                    last_error = Some(error);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
        panic!(
            "document store should initialize: {}",
            last_error.expect("expected initialization error")
        );
    }

    #[tokio::test]
    async fn postgres_document_store_round_trip() {
        let _guard = docker_test_guard();
        let pg = PostgresContainer::start("talon-documents-pg");
        let store = init_test_store(&pg.database_url()).await;
        let document = Document {
            id: "doc-1".to_string(),
            source: document_source(
                "acme".to_string(),
                "Knowledge".to_string(),
                "@Namespace/acme/Knowledge/refunds".to_string(),
                String::new(),
                String::new(),
            ),
            document_kind: DOCUMENT_KIND_CONTENT.to_string(),
            title: "Refunds".to_string(),
            text: "Refund policy for enterprise customers".to_string(),
            snippet: "Refund policy".to_string(),
            labels: [("tier".to_string(), "enterprise".to_string())]
                .into_iter()
                .collect(),
            created_at: 10,
            updated_at: 20,
            indexed_at: 30,
            generation: 4,
            ..Default::default()
        };

        store.upsert_documents(&[document]).await.unwrap();
        let response = store
            .search(&SearchQuery {
                query: "refund".to_string(),
                source: crate::control::search::SearchSourceFilter {
                    namespace: "acme".to_string(),
                    kinds: vec!["Knowledge".to_string()],
                    ..Default::default()
                },
                labels: [("tier".to_string(), "enterprise".to_string())]
                    .into_iter()
                    .collect(),
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].document.id, "doc-1");
        assert_eq!(
            response.results[0].document.document_kind,
            DOCUMENT_KIND_CONTENT
        );

        let prefix_response = store
            .search(&SearchQuery {
                query: "ref".to_string(),
                source: crate::control::search::SearchSourceFilter {
                    namespace: "acme".to_string(),
                    kinds: vec!["Knowledge".to_string()],
                    ..Default::default()
                },
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(prefix_response.results.len(), 1);

        assert_eq!(
            store
                .delete(&DeleteScope {
                    namespace: "acme".to_string(),
                    resource_key: "@Namespace/acme/Knowledge/refunds".to_string(),
                    ..Default::default()
                })
                .await
                .unwrap(),
            1
        );
    }

    #[test]
    fn postgres_like_prefix_pattern_escapes_wildcards() {
        assert_eq!(like_prefix_pattern(r"a_b%c\d"), r"a\_b\%c\\d%");
    }
}
