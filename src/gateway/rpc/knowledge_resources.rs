// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::keys;
use crate::control::ProtoKeyValueStoreExt;
use crate::gateway::rpc::{manifests, proto, GrpcGatewayHandler};

async fn hydrate_knowledge_manifest(
    kv: &std::sync::Arc<dyn crate::control::KeyValueStore + Send + Sync>,
    namespace: &str,
    mut knowledge: manifests::Knowledge,
) -> std::result::Result<manifests::Knowledge, tonic::Status> {
    let Some(spec) = knowledge.spec.as_mut() else {
        return Ok(knowledge);
    };
    let path_key = keys::knowledge(namespace, &spec.path);
    if let Some(bytes) = kv
        .get(&path_key)
        .await
        .map_err(|e| tonic::Status::internal(format!("Failed to read knowledge artifact: {}", e)))?
    {
        let entry =
            crate::knowledge::KvKnowledgeBook::normalize_entry(namespace, &spec.path, &bytes);
        spec.path = entry.path();
        spec.content = entry.content;
    }
    Ok(knowledge)
}

impl GrpcGatewayHandler {
    pub async fn handle_create_namespace_knowledge(
        &self,
        req: tonic::Request<proto::CreateNamespaceKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceKnowledgeResponse>, tonic::Status>
    {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();

        let mut knowledge = req
            .knowledge
            .ok_or_else(|| tonic::Status::invalid_argument("Knowledge manifest missing"))?;
        if knowledge.kind.is_empty() {
            knowledge.kind = "Knowledge".to_string();
        }

        let meta = knowledge
            .metadata
            .as_mut()
            .ok_or_else(|| tonic::Status::invalid_argument("Knowledge missing metadata"))?;
        if meta.name.trim().is_empty() {
            return Err(tonic::Status::invalid_argument(
                "Knowledge metadata.name is required",
            ));
        }
        if meta.namespace.is_empty() {
            meta.namespace = req.ns.clone();
        } else if meta.namespace != req.ns {
            return Err(tonic::Status::invalid_argument(
                "Knowledge metadata.namespace must match request namespace",
            ));
        }

        let spec = knowledge
            .spec
            .as_ref()
            .ok_or_else(|| tonic::Status::invalid_argument("Knowledge missing spec"))?;
        if spec.path.trim().is_empty() {
            return Err(tonic::Status::invalid_argument(
                "Knowledge spec.path is required",
            ));
        }
        let path_key = keys::knowledge(&req.ns, &spec.path);

        if let Some(existing_bytes) =
            self.gateway.kv.get(&path_key).await.map_err(|e| {
                tonic::Status::internal(format!("Failed to read knowledge path: {}", e))
            })?
        {
            let existing_entry = crate::knowledge::KvKnowledgeBook::normalize_entry(
                &req.ns,
                &spec.path,
                &existing_bytes,
            );
            if existing_entry.name != meta.name {
                return Err(tonic::Status::already_exists(format!(
                    "Knowledge path '{}' is already claimed by resource '{}'",
                    spec.path, existing_entry.name
                )));
            }
        }

        let resource_key = keys::knowledge_resource(&req.ns, &meta.name);
        if let Some(existing) = self
            .gateway
            .kv
            .get_msg::<manifests::Knowledge>(&resource_key)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to read knowledge resource: {}", e))
            })?
        {
            if let Some(existing_spec) = existing.spec {
                if existing_spec.path != spec.path && !existing_spec.path.is_empty() {
                    let old_key = keys::knowledge(&req.ns, &existing_spec.path);
                    self.gateway.kv.delete(&old_key).await.map_err(|e| {
                        tonic::Status::internal(format!(
                            "Failed to delete previous knowledge artifact: {}",
                            e
                        ))
                    })?;
                }
            }
        }

        let entry = crate::knowledge::KnowledgeEntry {
            namespace: req.ns.clone(),
            name: meta.name.clone(),
            path: spec.path.clone(),
            content: spec.content.clone(),
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        };
        let bytes = serde_json::to_vec(&entry).map_err(|e| {
            tonic::Status::internal(format!("Failed to serialize knowledge artifact: {}", e))
        })?;
        self.gateway
            .kv
            .set(&path_key, &bytes)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to write knowledge: {}", e)))?;
        self.gateway
            .kv
            .set_msg(&resource_key, &knowledge)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to write knowledge resource: {}", e))
            })?;

        Ok(tonic::Response::new(proto::NamespaceKnowledgeResponse {
            knowledge: Some(knowledge),
        }))
    }

    pub async fn handle_get_namespace_knowledge(
        &self,
        req: tonic::Request<proto::GetNamespaceKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceKnowledgeResponse>, tonic::Status>
    {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let key = keys::knowledge_resource(&req.ns, &req.name);
        let knowledge = self
            .gateway
            .kv
            .get_msg::<manifests::Knowledge>(&key)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to read knowledge resource: {}", e))
            })?
            .ok_or_else(|| tonic::Status::not_found("Knowledge not found"))?;

        let knowledge = hydrate_knowledge_manifest(&self.gateway.kv, &req.ns, knowledge).await?;

        Ok(tonic::Response::new(proto::NamespaceKnowledgeResponse {
            knowledge: Some(knowledge),
        }))
    }

    pub async fn handle_list_namespace_knowledge(
        &self,
        req: tonic::Request<proto::ListNamespaceKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListNamespaceKnowledgeResponse>, tonic::Status>
    {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let mut knowledge = Vec::new();

        for key in self
            .gateway
            .kv
            .list_keys(&keys::knowledge_resource_prefix(&req.ns))
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to list knowledge resources: {}", e))
            })?
        {
            if let Some(entry) = self
                .gateway
                .kv
                .get_msg::<manifests::Knowledge>(&key)
                .await
                .map_err(|e| {
                    tonic::Status::internal(format!("Failed to fetch knowledge resource: {}", e))
                })?
            {
                knowledge.push(hydrate_knowledge_manifest(&self.gateway.kv, &req.ns, entry).await?);
            }
        }

        Ok(tonic::Response::new(
            proto::ListNamespaceKnowledgeResponse { knowledge },
        ))
    }

    pub async fn handle_delete_namespace_knowledge(
        &self,
        req: tonic::Request<proto::DeleteNamespaceKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteNamespaceKnowledgeResponse>, tonic::Status>
    {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let resource_key = keys::knowledge_resource(&req.ns, &req.name);
        let Some(knowledge) = self
            .gateway
            .kv
            .get_msg::<manifests::Knowledge>(&resource_key)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to read knowledge resource: {}", e))
            })?
        else {
            return Err(tonic::Status::not_found("Knowledge not found"));
        };

        if let Some(spec) = knowledge.spec {
            self.gateway
                .kv
                .delete(&keys::knowledge(&req.ns, &spec.path))
                .await
                .map_err(|e| {
                    tonic::Status::internal(format!("Failed to delete knowledge: {}", e))
                })?;
        }
        self.gateway.kv.delete(&resource_key).await.map_err(|e| {
            tonic::Status::internal(format!("Failed to delete knowledge resource: {}", e))
        })?;

        Ok(tonic::Response::new(
            proto::DeleteNamespaceKnowledgeResponse { success: true },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::hydrate_knowledge_manifest;
    use crate::control::keys::{self, ResourceKey, ResourceList};
    use crate::control::{KeyValueStore, ProtoKeyValueStoreExt};
    use crate::gateway::rpc::{manifests, proto, GrpcGatewayHandler};
    use crate::gateway::{server::Gateway, session_streams::SessionStreamHub};
    use async_trait::async_trait;
    use futures::stream;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockKvStore {
        data: Mutex<HashMap<ResourceKey, Vec<u8>>>,
    }

    #[async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, k: &ResourceKey) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.data.lock().await.get(k).cloned())
        }

        async fn set(&self, k: &ResourceKey, v: &[u8]) -> anyhow::Result<()> {
            self.data.lock().await.insert(k.clone(), v.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            k: &ResourceKey,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let current = data.get(k).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(k.clone(), value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, k: &ResourceKey) -> anyhow::Result<()> {
            self.data.lock().await.remove(k);
            Ok(())
        }

        async fn list_keys(&self, list: &ResourceList) -> anyhow::Result<Vec<ResourceKey>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|key| list.matches(key).then(|| key.clone()))
                .collect::<Vec<_>>();
            keys.sort();
            Ok(keys)
        }
    }

    #[derive(Default)]
    struct MockPubSub;

    #[async_trait]
    impl crate::control::MessagePublisher for MockPubSub {
        async fn publish(&self, _topic: &str, _message: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            Ok(Box::pin(stream::empty()))
        }
    }

    fn metadata(name: &str, namespace: &str) -> manifests::ObjectMeta {
        manifests::ObjectMeta {
            name: name.to_string(),
            namespace: namespace.to_string(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
        }
    }

    fn manifest(name: &str, namespace: &str, path: &str, content: &str) -> manifests::Knowledge {
        manifests::Knowledge {
            api_version: String::new(),
            kind: String::new(),
            metadata: Some(metadata(name, namespace)),
            spec: Some(manifests::KnowledgeSpec {
                path: path.to_string(),
                content: content.to_string(),
            }),
        }
    }

    fn handler(kv: Arc<MockKvStore>) -> GrpcGatewayHandler {
        let pubsub = Arc::new(MockPubSub);
        GrpcGatewayHandler {
            gateway: Arc::new(Gateway {
                auth_config: None,
                kv,
                pubsub: pubsub.clone(),
                scheduler: Arc::new(crate::control::scheduler::NoopSchedulerBackend),
                session_streams: Arc::new(SessionStreamHub::new(pubsub)),
            }),
        }
    }

    #[tokio::test]
    async fn hydrate_knowledge_manifest_reads_normalized_content_from_kv() {
        let kv = Arc::new(MockKvStore::default());
        let entry = crate::knowledge::KnowledgeEntry {
            namespace: "acme".to_string(),
            name: "guide".to_string(),
            path: "folder/guide.md".to_string(),
            content: "normalized content".to_string(),
            updated_at: 1,
        };
        kv.set(
            &keys::knowledge("acme", "guide.md"),
            &serde_json::to_vec(&entry).unwrap(),
        )
        .await
        .unwrap();

        let hydrated = hydrate_knowledge_manifest(
            &(kv.clone() as Arc<dyn KeyValueStore + Send + Sync>),
            "acme",
            manifest("guide", "acme", "guide.md", "stale"),
        )
        .await
        .unwrap();

        let spec = hydrated.spec.unwrap();
        assert_eq!(spec.path, "folder/guide.md");
        assert_eq!(spec.content, "normalized content");
    }

    #[tokio::test]
    async fn create_namespace_knowledge_rejects_path_claim_conflicts() {
        let kv = Arc::new(MockKvStore::default());
        kv.set(
            &keys::knowledge("acme", "guide.md"),
            &serde_json::to_vec(&crate::knowledge::KnowledgeEntry {
                namespace: "acme".to_string(),
                name: "other".to_string(),
                path: "guide.md".to_string(),
                content: "existing".to_string(),
                updated_at: 1,
            })
            .unwrap(),
        )
        .await
        .unwrap();

        let status = handler(kv)
            .handle_create_namespace_knowledge(tonic::Request::new(
                proto::CreateNamespaceKnowledgeRequest {
                    ns: "acme".to_string(),
                    knowledge: Some(manifest("guide", "acme", "guide.md", "new")),
                },
            ))
            .await
            .expect_err("path conflict should fail");

        assert_eq!(status.code(), tonic::Code::AlreadyExists);
    }

    #[tokio::test]
    async fn create_namespace_knowledge_replaces_old_path_for_same_resource() {
        let kv = Arc::new(MockKvStore::default());
        kv.set_msg(
            &keys::knowledge_resource("acme", "guide"),
            &manifest("guide", "acme", "old.md", "old content"),
        )
        .await
        .unwrap();
        kv.set(
            &keys::knowledge("acme", "old.md"),
            &serde_json::to_vec(&crate::knowledge::KnowledgeEntry {
                namespace: "acme".to_string(),
                name: "guide".to_string(),
                path: "old.md".to_string(),
                content: "old content".to_string(),
                updated_at: 1,
            })
            .unwrap(),
        )
        .await
        .unwrap();

        let handler = handler(kv.clone());
        let response = handler
            .handle_create_namespace_knowledge(tonic::Request::new(
                proto::CreateNamespaceKnowledgeRequest {
                    ns: "acme".to_string(),
                    knowledge: Some(manifest("guide", "acme", "new.md", "new content")),
                },
            ))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(
            response
                .knowledge
                .as_ref()
                .and_then(|entry| entry.spec.as_ref())
                .map(|spec| spec.path.as_str()),
            Some("new.md")
        );
        assert!(kv
            .get(&keys::knowledge("acme", "old.md"))
            .await
            .unwrap()
            .is_none());
        assert!(kv
            .get(&keys::knowledge("acme", "new.md"))
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn delete_namespace_knowledge_returns_not_found_for_missing_resource() {
        let status = handler(Arc::new(MockKvStore::default()))
            .handle_delete_namespace_knowledge(tonic::Request::new(
                proto::DeleteNamespaceKnowledgeRequest {
                    ns: "acme".to_string(),
                    name: "missing".to_string(),
                },
            ))
            .await
            .expect_err("missing resource should fail");

        assert_eq!(status.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn create_namespace_knowledge_validates_required_fields() {
        let handler = handler(Arc::new(MockKvStore::default()));

        let missing = handler
            .handle_create_namespace_knowledge(tonic::Request::new(
                proto::CreateNamespaceKnowledgeRequest {
                    ns: "acme".to_string(),
                    knowledge: None,
                },
            ))
            .await
            .expect_err("missing manifest should fail");
        assert_eq!(missing.code(), tonic::Code::InvalidArgument);

        let wrong_namespace = handler
            .handle_create_namespace_knowledge(tonic::Request::new(
                proto::CreateNamespaceKnowledgeRequest {
                    ns: "acme".to_string(),
                    knowledge: Some(manifest("guide", "other", "guide.md", "text")),
                },
            ))
            .await
            .expect_err("namespace mismatch should fail");
        assert_eq!(wrong_namespace.code(), tonic::Code::InvalidArgument);

        let mut missing_path = manifest("guide", "acme", "guide.md", "text");
        missing_path.spec.as_mut().unwrap().path.clear();
        let missing_path = handler
            .handle_create_namespace_knowledge(tonic::Request::new(
                proto::CreateNamespaceKnowledgeRequest {
                    ns: "acme".to_string(),
                    knowledge: Some(missing_path),
                },
            ))
            .await
            .expect_err("missing path should fail");
        assert_eq!(missing_path.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn get_and_list_namespace_knowledge_round_trip() {
        let kv = Arc::new(MockKvStore::default());
        let handler = handler(kv.clone());

        handler
            .handle_create_namespace_knowledge(tonic::Request::new(
                proto::CreateNamespaceKnowledgeRequest {
                    ns: "acme".to_string(),
                    knowledge: Some(manifest("guide", "acme", "guide.md", "content")),
                },
            ))
            .await
            .unwrap();

        let fetched = handler
            .handle_get_namespace_knowledge(tonic::Request::new(
                proto::GetNamespaceKnowledgeRequest {
                    ns: "acme".to_string(),
                    name: "guide".to_string(),
                },
            ))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            fetched
                .knowledge
                .as_ref()
                .and_then(|knowledge| knowledge.metadata.as_ref())
                .map(|metadata| metadata.name.as_str()),
            Some("guide")
        );

        let listed = handler
            .handle_list_namespace_knowledge(tonic::Request::new(
                proto::ListNamespaceKnowledgeRequest {
                    ns: "acme".to_string(),
                },
            ))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(listed.knowledge.len(), 1);
        assert_eq!(
            listed.knowledge[0]
                .metadata
                .as_ref()
                .map(|metadata| metadata.name.as_str()),
            Some("guide")
        );
    }

    #[tokio::test]
    async fn get_namespace_knowledge_returns_not_found_for_missing_resource() {
        let status = handler(Arc::new(MockKvStore::default()))
            .handle_get_namespace_knowledge(tonic::Request::new(
                proto::GetNamespaceKnowledgeRequest {
                    ns: "acme".to_string(),
                    name: "missing".to_string(),
                },
            ))
            .await
            .expect_err("missing knowledge should fail");

        assert_eq!(status.code(), tonic::Code::NotFound);
    }
}
