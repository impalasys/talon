use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct KnowledgeEntry {
    pub namespace: String,
    pub name: String,
    pub path: String,
    pub content: String,
    pub updated_at: i64,
}

/// A single search result from the KnowledgeBook.
pub struct KnowledgeResult {
    pub namespace: String,
    pub path: String,
    pub excerpt: String,
    pub updated_at: i64,
}

pub const KNOWLEDGE_WRITE_TOOL: &str = "knowledge_write";
pub const KNOWLEDGE_SEARCH_TOOL: &str = "knowledge_search";
pub const KNOWLEDGE_GET_TOOL: &str = "knowledge_get";

/// KnowledgeBook manages namespace-scoped knowledge artifacts for Talon agents.
/// Artifacts are stored as Markdown in the platform KV store under the key prefix
/// `Knowledge/{path}` within the agent's home namespace.
#[async_trait::async_trait]
pub trait KnowledgeBook: Send + Sync {
    /// Fetch a single artifact at the given path within the namespace.
    async fn get(&self, ns: &str, path: &str) -> Result<Option<KnowledgeEntry>>;

    /// Write or overwrite an artifact at the given path within the namespace.
    async fn write(&self, ns: &str, path: &str, content: &str) -> Result<()>;

    /// Keyword-scan the namespace for artifacts matching the query.
    async fn search(&self, ns: &str, query: &str, limit: usize) -> Result<Vec<KnowledgeResult>>;
}

pub fn register_tools(registry: &mut crate::skills::registry::ToolRegistry) {
    registry.register_builtin(
        KNOWLEDGE_WRITE_TOOL,
        "Write or overwrite an artifact in the agent's KnowledgeBook. Use this to persist curated facts, research findings, or guides that other agents or future sessions may need.",
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Artifact path, e.g. 'seo/2025-best-practices.md' or 'goals.md'" },
                "content": { "type": "string", "description": "Full Markdown content of the artifact" }
            },
            "required": ["path", "content"]
        }),
    );

    registry.register_builtin(
        KNOWLEDGE_SEARCH_TOOL,
        "Search the agent's KnowledgeBook for artifacts matching a keyword query.",
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Keyword or topic to search for" }
            },
            "required": ["query"]
        }),
    );

    registry.register_builtin(
        KNOWLEDGE_GET_TOOL,
        "Read a specific artifact from the agent's KnowledgeBook by its path.",
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Artifact path, e.g. 'seo/2025-best-practices.md' or 'goals.md'" }
            },
            "required": ["path"]
        }),
    );
}

pub async fn execute_tool(
    book: &dyn KnowledgeBook,
    namespace: &str,
    name: &str,
    args: &Value,
) -> Result<Option<String>> {
    match name {
        KNOWLEDGE_WRITE_TOOL => {
            let path = args["path"].as_str().unwrap_or("untitled.md");
            let content = args["content"].as_str().unwrap_or("");
            book.write(namespace, path, content).await?;
            Ok(Some(format!("KnowledgeBook: wrote artifact '{}'.", path)))
        }
        KNOWLEDGE_GET_TOOL => {
            let path = args["path"].as_str().unwrap_or("");
            match book.get(namespace, path).await? {
                Some(entry) => Ok(Some(format!(
                    "[{}:{}]\n{}",
                    entry.namespace,
                    entry.path(),
                    entry.content
                ))),
                None => Ok(Some(format!(
                    "KnowledgeBook: artifact '{}' not found.",
                    path
                ))),
            }
        }
        KNOWLEDGE_SEARCH_TOOL => {
            let query = args["query"].as_str().unwrap_or("");
            let results = book.search(namespace, query, 5).await?;
            if results.is_empty() {
                Ok(Some(
                    "KnowledgeBook: no matching artifacts found.".to_string(),
                ))
            } else {
                Ok(Some(
                    results
                        .iter()
                        .map(|r| format!("[{}:{}] {}", r.namespace, r.path, r.excerpt))
                        .collect::<Vec<_>>()
                        .join("\n---\n"),
                ))
            }
        }
        _ => Ok(None),
    }
}

/// KV-backed implementation of KnowledgeBook.
pub struct KvKnowledgeBook {
    pub kv: Arc<dyn crate::control::KeyValueStore>,
}

impl KvKnowledgeBook {
    pub fn new(kv: Arc<dyn crate::control::KeyValueStore>) -> Self {
        Self { kv }
    }

    pub(crate) fn normalize_entry(namespace: &str, path: &str, bytes: &[u8]) -> KnowledgeEntry {
        serde_json::from_slice(bytes).unwrap_or_else(|_| KnowledgeEntry {
            namespace: namespace.to_string(),
            name: path.to_string(),
            path: path.to_string(),
            content: String::from_utf8_lossy(bytes).to_string(),
            updated_at: 0,
        })
    }

    async fn find_entry(&self, ns: &str, path: &str) -> Result<Option<KnowledgeEntry>> {
        let key = crate::control::keys::knowledge(path);
        if let Some(bytes) = self.kv.get(ns, &key).await? {
            return Ok(Some(Self::normalize_entry(ns, path, &bytes)));
        }

        let prefix = crate::control::keys::knowledge_prefix();
        let path_lower = path.to_lowercase();
        let matches = self
            .kv
            .list_keys(ns, prefix)
            .await?
            .into_iter()
            .filter(|candidate| {
                let artifact_path = candidate.strip_prefix(prefix).unwrap_or(candidate);
                let artifact_lower = artifact_path.to_lowercase();
                artifact_lower == path_lower
                    || artifact_lower.ends_with(&format!("/{}", path_lower))
            })
            .collect::<Vec<_>>();

        if matches.len() != 1 {
            return Ok(None);
        }

        let Some(bytes) = self.kv.get(ns, &matches[0]).await? else {
            return Ok(None);
        };
        let artifact_path = matches[0].strip_prefix(prefix).unwrap_or(&matches[0]);
        Ok(Some(Self::normalize_entry(ns, artifact_path, &bytes)))
    }
}

#[async_trait::async_trait]
impl KnowledgeBook for KvKnowledgeBook {
    async fn get(&self, ns: &str, path: &str) -> Result<Option<KnowledgeEntry>> {
        for candidate_ns in crate::control::ns::ancestry(ns) {
            if let Some(entry) = self.find_entry(&candidate_ns, path).await? {
                return Ok(Some(entry));
            }
        }
        Ok(None)
    }

    async fn write(&self, ns: &str, path: &str, content: &str) -> Result<()> {
        let key = crate::control::keys::knowledge(path);
        let entry = KnowledgeEntry {
            namespace: ns.to_string(),
            name: path.to_string(),
            path: path.to_string(),
            content: content.to_string(),
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        };
        let bytes = serde_json::to_vec(&entry)?;
        self.kv.set(ns, &key, &bytes).await?;
        Ok(())
    }

    async fn search(&self, ns: &str, query: &str, limit: usize) -> Result<Vec<KnowledgeResult>> {
        let prefix = crate::control::keys::knowledge_prefix();
        let query_lower = query.to_lowercase();
        let mut scored_results: Vec<(i32, usize, KnowledgeResult)> = Vec::new();
        let mut seen_paths = std::collections::HashSet::new();

        for (depth, candidate_ns) in crate::control::ns::ancestry(ns).into_iter().enumerate() {
            let keys = self.kv.list_keys(&candidate_ns, prefix).await?;

            for key in keys {
                if let Some(bytes) = self.kv.get(&candidate_ns, &key).await.unwrap_or(None) {
                    let entry = Self::normalize_entry(
                        &candidate_ns,
                        key.strip_prefix(prefix).unwrap_or(&key),
                        &bytes,
                    );
                    let path = entry.path();
                    if !seen_paths.insert(path.clone()) {
                        continue;
                    }
                    let content = entry.content;
                    let path_lower = path.to_lowercase();
                    let basename_lower = path_lower.rsplit('/').next().unwrap_or(&path_lower);
                    let content_lower = content.to_lowercase();

                    let score = if basename_lower == query_lower {
                        4
                    } else if path_lower.ends_with(&format!("/{}", query_lower))
                        || path_lower == query_lower
                    {
                        3
                    } else if path_lower.contains(&query_lower) {
                        2
                    } else if content_lower.contains(&query_lower) {
                        1
                    } else {
                        0
                    };

                    if score == 0 {
                        continue;
                    }

                    let excerpt = if score >= 2 {
                        format!("Matched artifact path '{}'.", path)
                    } else if content_lower.contains(&query_lower) {
                        let preview = content.chars().take(200).collect::<String>();
                        format!("...{}...", preview)
                    } else {
                        content.chars().take(200).collect()
                    };

                    scored_results.push((
                        score,
                        depth,
                        KnowledgeResult {
                            namespace: candidate_ns.clone(),
                            path,
                            excerpt,
                            updated_at: entry.updated_at,
                        },
                    ));
                }
            }
        }

        scored_results.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.path.cmp(&b.2.path))
        });
        Ok(scored_results
            .into_iter()
            .take(limit)
            .map(|(_, _, result)| result)
            .collect())
    }
}

impl KnowledgeEntry {
    pub fn path(&self) -> String {
        if self.path.is_empty() {
            self.name.clone()
        } else {
            self.path.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{KnowledgeBook, KnowledgeEntry, KvKnowledgeBook};
    use crate::control::KeyValueStore;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    struct MockKvStore {
        store: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MockKvStore {
        fn new() -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
            }
        }

        fn make_key(ns: &str, key: &str) -> String {
            format!("{}/{}", ns, key)
        }
    }

    #[async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
            let map = self.store.lock().await;
            Ok(map.get(&Self::make_key(namespace, key)).cloned())
        }

        async fn set(&self, namespace: &str, key: &str, value: &[u8]) -> anyhow::Result<()> {
            let mut map = self.store.lock().await;
            map.insert(Self::make_key(namespace, key), value.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            namespace: &str,
            key: &str,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut map = self.store.lock().await;
            let full_key = Self::make_key(namespace, key);
            let current = map.get(&full_key).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if !matches {
                return Ok(false);
            }
            map.insert(full_key, value.to_vec());
            Ok(true)
        }

        async fn delete(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
            let mut map = self.store.lock().await;
            map.remove(&Self::make_key(namespace, key));
            Ok(())
        }

        async fn list_keys(&self, namespace: &str, prefix: &str) -> anyhow::Result<Vec<String>> {
            let map = self.store.lock().await;
            let ns_prefix = format!("{}/{}", namespace, prefix);
            let ns_root = format!("{}/", namespace);
            let mut results = Vec::new();
            for key in map.keys() {
                if key.starts_with(&ns_prefix) {
                    results.push(key.strip_prefix(&ns_root).unwrap().to_string());
                }
            }
            Ok(results)
        }
    }

    #[tokio::test]
    async fn search_handles_unicode_content_without_panicking() {
        let kv = Arc::new(MockKvStore::new());
        let book = KvKnowledgeBook::new(kv.clone());

        let entry = KnowledgeEntry {
            namespace: "conic".to_string(),
            name: "unicode.md".to_string(),
            path: "unicode.md".to_string(),
            content: "Hello 👋 café résumé 東京 unicode body".to_string(),
            updated_at: 0,
        };
        let bytes = serde_json::to_vec(&entry).unwrap();
        kv.set("conic", "Knowledge/unicode.md", &bytes)
            .await
            .unwrap();

        let results = book.search("conic", "café", 5).await.unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].excerpt.contains("café"));
        assert_eq!(results[0].namespace, "conic");
    }

    #[tokio::test]
    async fn get_inherits_from_ancestor_namespace() {
        let kv = Arc::new(MockKvStore::new());
        let book = KvKnowledgeBook::new(kv.clone());

        let entry = KnowledgeEntry {
            namespace: "conic".to_string(),
            name: "playbooks/aeo.md".to_string(),
            path: "playbooks/aeo.md".to_string(),
            content: "Shared AEO guidance".to_string(),
            updated_at: 0,
        };
        let bytes = serde_json::to_vec(&entry).unwrap();
        kv.set("conic", "Knowledge/playbooks/aeo.md", &bytes)
            .await
            .unwrap();

        let result = book.get("conic:wks:13", "playbooks/aeo.md").await.unwrap();
        let result = result.expect("expected inherited knowledge");
        assert_eq!(result.namespace, "conic");
        assert_eq!(result.content, "Shared AEO guidance");
    }

    #[tokio::test]
    async fn search_prefers_local_override_over_ancestor() {
        let kv = Arc::new(MockKvStore::new());
        let book = KvKnowledgeBook::new(kv.clone());

        for (ns, content) in [
            ("conic", "Shared prompt framework"),
            ("conic:wks:13", "Workspace-specific prompt framework"),
        ] {
            let entry = KnowledgeEntry {
                namespace: ns.to_string(),
                name: "playbooks/framework.md".to_string(),
                path: "playbooks/framework.md".to_string(),
                content: content.to_string(),
                updated_at: 0,
            };
            let bytes = serde_json::to_vec(&entry).unwrap();
            kv.set(ns, "Knowledge/playbooks/framework.md", &bytes)
                .await
                .unwrap();
        }

        let results = book.search("conic:wks:13", "framework", 5).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].namespace, "conic:wks:13");
    }
}
