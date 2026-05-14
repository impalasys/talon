use anyhow::Result;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub type Embedding = Vec<f32>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorEntry {
    pub id: String,
    pub text: String,
    pub embedding: Embedding,
}

#[async_trait::async_trait]
pub trait VectorStore: Send + Sync {
    async fn insert(&self, entry: VectorEntry) -> Result<()>;
    async fn search(&self, query: Embedding, limit: usize) -> Result<Vec<VectorEntry>>;
}

const VECTOR_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("vectors");

pub struct RedbVectorStore {
    db: Database,
}

impl RedbVectorStore {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let db = Database::create(path)?;
        Ok(Self { db })
    }

    fn cosine_similarity(a: &Embedding, b: &Embedding) -> f32 {
        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }
        dot_product / (norm_a * norm_b)
    }
}

#[async_trait::async_trait]
impl VectorStore for RedbVectorStore {
    async fn insert(&self, entry: VectorEntry) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(VECTOR_TABLE)?;
            let data = serde_json::to_vec(&entry)?;
            table.insert(entry.id.as_str(), data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    async fn search(&self, query: Embedding, limit: usize) -> Result<Vec<VectorEntry>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(VECTOR_TABLE)?;

        let mut scored_entries: Vec<(f32, VectorEntry)> = Vec::new();

        for result in table.iter()? {
            let (_key, value) = result?;
            let entry: VectorEntry = serde_json::from_slice(value.value())?;
            let score = Self::cosine_similarity(&query, &entry.embedding);
            scored_entries.push((score, entry));
        }

        scored_entries.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored_entries
            .into_iter()
            .take(limit)
            .map(|e| e.1)
            .collect())
    }
}

pub struct InMemoryVectorStore {
    entries: tokio::sync::RwLock<Vec<VectorEntry>>,
}

impl InMemoryVectorStore {
    pub fn new() -> Self {
        Self {
            entries: tokio::sync::RwLock::new(Vec::new()),
        }
    }

    fn cosine_similarity(a: &Embedding, b: &Embedding) -> f32 {
        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }
        dot_product / (norm_a * norm_b)
    }
}

#[async_trait::async_trait]
impl VectorStore for InMemoryVectorStore {
    async fn insert(&self, entry: VectorEntry) -> Result<()> {
        let mut entries = self.entries.write().await;
        entries.push(entry);
        Ok(())
    }

    async fn search(&self, query: Embedding, limit: usize) -> Result<Vec<VectorEntry>> {
        let entries = self.entries.read().await;
        let mut scored_entries: Vec<(f32, VectorEntry)> = entries
            .iter()
            .map(|e| {
                let score = Self::cosine_similarity(&query, &e.embedding);
                (score, e.clone())
            })
            .collect();

        scored_entries.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored_entries
            .into_iter()
            .take(limit)
            .map(|e| e.1)
            .collect())
    }
}
