// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

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

/// A single listed artifact from the KnowledgeBook.
pub struct KnowledgeListEntry {
    pub namespace: String,
    pub path: String,
    pub updated_at: i64,
    pub inherited: bool,
}

pub const KNOWLEDGE_WRITE_TOOL: &str = "knowledge_write";
pub const KNOWLEDGE_SEARCH_TOOL: &str = "knowledge_search";
pub const KNOWLEDGE_GET_TOOL: &str = "knowledge_get";
pub const KNOWLEDGE_LIST_TOOL: &str = "knowledge_list";

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

    /// List artifacts under an optional path prefix within the namespace ancestry.
    async fn list(
        &self,
        ns: &str,
        path_prefix: &str,
        local_only: bool,
        recursive: bool,
        limit: usize,
    ) -> Result<Vec<KnowledgeListEntry>>;
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

    registry.register_builtin(
        KNOWLEDGE_LIST_TOOL,
        "List artifacts from the agent's KnowledgeBook under an optional folder-like path prefix. Use this before reading when you only know the area of knowledge you need.",
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Optional path prefix to list under, e.g. 'seo/' or 'playbooks'." },
                "local": { "type": "boolean", "description": "Whether to restrict results to the current namespace only. Defaults to true. Set to false to include inherited knowledge from ancestor namespaces." },
                "recursive": { "type": "boolean", "description": "Whether to include nested descendants. Defaults to true." },
                "limit": { "type": "integer", "description": "Maximum number of artifacts to return. Defaults to 50." }
            }
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
        KNOWLEDGE_LIST_TOOL => {
            let path_prefix = args["path"].as_str().unwrap_or("");
            let local_only = args
                .get("local")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let recursive = args
                .get("recursive")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|value| value.clamp(1, 200) as usize)
                .unwrap_or(50);
            let entries = book
                .list(namespace, path_prefix, local_only, recursive, limit)
                .await?;
            if entries.is_empty() {
                Ok(Some(format!(
                    "KnowledgeBook: no artifacts found under '{}'.",
                    if path_prefix.is_empty() { "/" } else { path_prefix }
                )))
            } else {
                Ok(Some(serde_json::to_string_pretty(&json!({
                    "path": path_prefix,
                    "local": local_only,
                    "recursive": recursive,
                    "entries": entries.into_iter().map(|entry| json!({
                        "namespace": entry.namespace,
                        "path": entry.path,
                        "updated_at": entry.updated_at,
                        "scope": if entry.inherited { "inherited" } else { "local" },
                    })).collect::<Vec<_>>(),
                }))?))
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

    fn normalize_list_prefix(path_prefix: &str) -> String {
        path_prefix.trim_matches('/').to_lowercase()
    }

    fn entry_matches_prefix(path: &str, normalized_prefix: &str, recursive: bool) -> bool {
        if normalized_prefix.is_empty() {
            return true;
        }

        let lower_path = path.to_lowercase();
        if lower_path == normalized_prefix {
            return true;
        }

        let Some(remainder) = lower_path
            .strip_prefix(normalized_prefix)
            .and_then(|suffix| suffix.strip_prefix('/'))
        else {
            return false;
        };

        recursive || !remainder.contains('/')
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

    async fn list(
        &self,
        ns: &str,
        path_prefix: &str,
        local_only: bool,
        recursive: bool,
        limit: usize,
    ) -> Result<Vec<KnowledgeListEntry>> {
        let prefix = crate::control::keys::knowledge_prefix();
        let normalized_prefix = Self::normalize_list_prefix(path_prefix);
        let mut entries = Vec::new();
        let mut seen_paths = std::collections::HashSet::new();

        let namespaces = if local_only {
            vec![ns.to_string()]
        } else {
            crate::control::ns::ancestry(ns)
        };

        for candidate_ns in namespaces {
            let mut keys = self.kv.list_keys(&candidate_ns, prefix).await?;
            keys.sort();

            for key in keys {
                let artifact_path = key.strip_prefix(prefix).unwrap_or(&key);
                if !Self::entry_matches_prefix(artifact_path, &normalized_prefix, recursive) {
                    continue;
                }
                if !seen_paths.insert(artifact_path.to_string()) {
                    continue;
                }
                let Some(bytes) = self.kv.get(&candidate_ns, &key).await? else {
                    continue;
                };
                let entry = Self::normalize_entry(&candidate_ns, artifact_path, &bytes);
                let namespace = entry.namespace.clone();
                let path = entry.path();
                entries.push(KnowledgeListEntry {
                    namespace,
                    path,
                    updated_at: entry.updated_at,
                    inherited: candidate_ns != ns,
                });
                if entries.len() >= limit {
                    return Ok(entries);
                }
            }
        }

        Ok(entries)
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
    use super::{
        execute_tool, KnowledgeBook, KnowledgeEntry, KvKnowledgeBook,
        KNOWLEDGE_GET_TOOL, KNOWLEDGE_LIST_TOOL, KNOWLEDGE_SEARCH_TOOL, KNOWLEDGE_WRITE_TOOL,
    };
    use crate::control::KeyValueStore;
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    struct MockKvStore {
        store: Mutex<HashMap<String, Vec<u8>>>,
    }

    #[tokio::test]
    async fn list_inherits_namespace_and_respects_prefix_modes() {
        let kv = Arc::new(MockKvStore::new());
        let book = KvKnowledgeBook::new(kv.clone());

        for (ns, path, content) in [
            ("conic", "playbooks/root.md", "shared root"),
            ("conic", "playbooks/nested/child.md", "shared child"),
            ("conic:wks:13", "playbooks/local.md", "local root"),
        ] {
            let entry = KnowledgeEntry {
                namespace: ns.to_string(),
                name: path.to_string(),
                path: path.to_string(),
                content: content.to_string(),
                updated_at: 7,
            };
            kv.set(ns, &format!("Knowledge/{}", path), &serde_json::to_vec(&entry).unwrap())
                .await
                .unwrap();
        }

        let non_recursive = book
            .list("conic:wks:13", "playbooks", false, false, 10)
            .await
            .unwrap();
        assert_eq!(non_recursive.len(), 2);
        assert_eq!(non_recursive[0].path, "playbooks/local.md");
        assert_eq!(non_recursive[1].path, "playbooks/root.md");
        assert!(!non_recursive[0].inherited);
        assert!(non_recursive[1].inherited);

        let recursive = book
            .list("conic:wks:13", "playbooks", false, true, 10)
            .await
            .unwrap();
        assert_eq!(recursive.len(), 3);
        let recursive_paths = recursive.into_iter().map(|entry| entry.path).collect::<Vec<_>>();
        assert_eq!(
            recursive_paths,
            vec![
                "playbooks/local.md".to_string(),
                "playbooks/nested/child.md".to_string(),
                "playbooks/root.md".to_string(),
            ]
        );

        let local_only = book
            .list("conic:wks:13", "playbooks", true, true, 10)
            .await
            .unwrap();
        assert_eq!(local_only.len(), 1);
        assert_eq!(local_only[0].path, "playbooks/local.md");
        assert!(!local_only[0].inherited);
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

    #[tokio::test]
    async fn normalize_entry_and_find_entry_cover_fallback_and_ambiguous_matches() {
        let kv = Arc::new(MockKvStore::new());
        let book = KvKnowledgeBook::new(kv.clone());

        kv.set("conic", "Knowledge/notes/Plan.md", b"plain text body")
            .await
            .unwrap();
        let resolved = book.find_entry("conic", "plan.md").await.unwrap().unwrap();
        assert_eq!(resolved.namespace, "conic");
        assert_eq!(resolved.path(), "notes/Plan.md");
        assert_eq!(resolved.content, "plain text body");

        kv.set("conic", "Knowledge/other/Plan.md", b"another body")
            .await
            .unwrap();
        let ambiguous = book.find_entry("conic", "plan.md").await.unwrap();
        assert!(ambiguous.is_none());
    }

    #[tokio::test]
    async fn search_scores_path_matches_and_respects_limit_ordering() {
        let kv = Arc::new(MockKvStore::new());
        let book = KvKnowledgeBook::new(kv.clone());

        for (path, content) in [
            ("framework", "exact basename"),
            ("playbooks/framework", "suffix match"),
            ("notes/framework-guide", "contains path"),
            ("misc/ideas", "mentions framework in body"),
        ] {
            let entry = KnowledgeEntry {
                namespace: "conic".to_string(),
                name: path.to_string(),
                path: path.to_string(),
                content: content.to_string(),
                updated_at: 0,
            };
            kv.set(
                "conic",
                &format!("Knowledge/{}", path),
                &serde_json::to_vec(&entry).unwrap(),
            )
            .await
            .unwrap();
        }

        let results = book.search("conic", "framework", 3).await.unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].path, "framework");
        assert_eq!(results[1].path, "playbooks/framework");
        assert_eq!(results[2].path, "notes/framework-guide");
        assert!(results[0].excerpt.contains("Matched artifact path"));
    }

    #[tokio::test]
    async fn execute_tool_covers_write_get_search_and_unknown_paths() {
        let kv = Arc::new(MockKvStore::new());
        let book = KvKnowledgeBook::new(kv.clone());

        let wrote = execute_tool(
            &book,
            "conic",
            KNOWLEDGE_WRITE_TOOL,
            &json!({"path":"goals.md","content":"ship it"}),
        )
        .await
        .unwrap();
        assert_eq!(wrote.as_deref(), Some("KnowledgeBook: wrote artifact 'goals.md'."));

        let got = execute_tool(
            &book,
            "conic",
            KNOWLEDGE_GET_TOOL,
            &json!({"path":"goals.md"}),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(got.contains("[conic:goals.md]"));
        assert!(got.contains("ship it"));

        let missing = execute_tool(
            &book,
            "conic",
            KNOWLEDGE_GET_TOOL,
            &json!({"path":"missing.md"}),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(missing.contains("artifact 'missing.md' not found"));

        let search = execute_tool(
            &book,
            "conic",
            KNOWLEDGE_SEARCH_TOOL,
            &json!({"query":"ship"}),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(search.contains("[conic:goals.md]"));

        let empty_search = execute_tool(
            &book,
            "conic",
            KNOWLEDGE_SEARCH_TOOL,
            &json!({"query":"absent"}),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(empty_search.contains("no matching artifacts found"));

        let list = execute_tool(&book, "conic", KNOWLEDGE_LIST_TOOL, &json!({}))
            .await
            .unwrap()
            .unwrap();
        let list_payload: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(list_payload["entries"][0]["path"], "goals.md");
        assert_eq!(list_payload["entries"][0]["scope"], "local");
        assert_eq!(list_payload["local"], true);

        let inherited_list = execute_tool(
            &book,
            "conic:wks:13",
            KNOWLEDGE_LIST_TOOL,
            &json!({"local":false}),
        )
        .await
        .unwrap()
        .unwrap();
        let inherited_payload: serde_json::Value = serde_json::from_str(&inherited_list).unwrap();
        assert_eq!(inherited_payload["local"], false);
        assert_eq!(inherited_payload["entries"][0]["scope"], "inherited");

        let empty_list = execute_tool(
            &book,
            "conic",
            KNOWLEDGE_LIST_TOOL,
            &json!({"path":"missing/"}),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(empty_list.contains("no artifacts found"));

        let unknown = execute_tool(&book, "conic", "unknown_tool", &json!({}))
            .await
            .unwrap();
        assert!(unknown.is_none());
    }
}
