use crate::memory::context::MemoryContext;
use anyhow::Result;
use std::sync::Arc;

pub struct KvMemoryStore {
    pub kv: Arc<dyn crate::control::KeyValueStore>,
}

impl KvMemoryStore {
    pub fn new(kv: Arc<dyn crate::control::KeyValueStore>) -> Self {
        Self { kv }
    }
}

#[async_trait::async_trait]
pub trait MemoryStore: Send + Sync {
    async fn load(&self, workspace_id: &str, thread_id: &str) -> Result<MemoryContext>;
    async fn save_thread_entry(&self, mc: &MemoryContext, role: &str, content: &str) -> Result<()>;
    async fn save_daily_entry(&self, mc: &MemoryContext, summary: &str) -> Result<()>;
    async fn update_long_term(&self, workspace_id: &str, content: &str) -> Result<()>;
    async fn compact_session(&self, mc: &MemoryContext) -> Result<String>;
    async fn search(&self, workspace_id: &str, query: &str, limit: usize) -> Result<Vec<String>>;
    async fn save_memory(&self, ns: &str, agent: &str, path: &str, content: &str) -> Result<()>;
}

#[async_trait::async_trait]
impl MemoryStore for KvMemoryStore {
    async fn load(&self, workspace_id: &str, thread_id: &str) -> Result<MemoryContext> {
        Ok(MemoryContext {
            long_term: String::new(),
            thread_log: String::new(),
            daily_log: String::new(),
            workspace_id: workspace_id.to_string(),
            thread_id: thread_id.to_string(),
        })
    }

    async fn save_thread_entry(
        &self,
        _mc: &MemoryContext,
        _role: &str,
        _content: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn save_daily_entry(&self, _mc: &MemoryContext, _summary: &str) -> Result<()> {
        Ok(())
    }

    async fn update_long_term(&self, _workspace_id: &str, _content: &str) -> Result<()> {
        Ok(())
    }

    async fn compact_session(&self, _mc: &MemoryContext) -> Result<String> {
        Ok(String::new())
    }

    async fn search(
        &self,
        _workspace_id: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn save_memory(&self, ns: &str, agent: &str, path: &str, content: &str) -> Result<()> {
        // Unify with the Gateway's layout structure defined by 'keys::agent_memory'
        let key = format!("Agent/{}/Memory/{}", agent, path);
        self.kv.set(ns, &key, content.as_bytes()).await?;
        Ok(())
    }
}
