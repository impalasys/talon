// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::ns;
use crate::control::ProtoKeyValueStoreExt;
use crate::gateway::rpc::{models, proto, GrpcGatewayHandler};
use std::time::{SystemTime, UNIX_EPOCH};

const META_NS: &str = "talon-system:ns";
const ROOT_EDGE_NS: &str = "talon-system:ns:internal";

impl GrpcGatewayHandler {
    pub async fn handle_create_namespace(
        &self,
        req: tonic::Request<proto::CreateNamespaceRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceResponse>, tonic::Status> {
        // Namespace management is an admin-level operation. Require a valid JWT but
        // only allow broad (un-scoped) tokens — a token scoped to a specific
        // namespace cannot create/delete other namespaces.
        crate::require_auth!(self, req, ns::TALON_SYSTEM);
        let req = req.into_inner();

        let name = req.name.clone();
        if name.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "Namespace name cannot be empty",
            ));
        }

        // Deduce parent
        let mut parts: Vec<&str> = name.split(':').collect();
        let parent = if parts.len() > 1 {
            parts.pop(); // Remove the last segment (the child name)
            Some(parts.join(":"))
        } else {
            None
        };

        // Namespace creation is idempotent for active namespaces so bootstrap and
        // reconcile flows can safely backfill labels without failing on re-create.
        let meta_key = format!("Namespace/{}", name);
        if let Ok(Some(mut existing_ns)) = self
            .gateway
            .kv
            .get_msg::<models::Namespace>(META_NS, &meta_key)
            .await
        {
            if !existing_ns.is_deleted {
                existing_ns.labels.extend(req.labels.clone());
                self.gateway
                    .kv
                    .set_msg(META_NS, &meta_key, &existing_ns)
                    .await
                    .map_err(|e| {
                        tonic::Status::internal(format!(
                            "Failed to update namespace metadata: {}",
                            e
                        ))
                    })?;

                return Ok(tonic::Response::new(proto::NamespaceResponse {
                    name: existing_ns.name,
                    parent: if existing_ns.parent.is_empty() {
                        None
                    } else {
                        Some(existing_ns.parent)
                    },
                    is_deleted: existing_ns.is_deleted,
                    deleted_at: existing_ns.deleted_at,
                    labels: existing_ns.labels,
                }));
            }
            // If it is tombstoned, falling through here will overwrite and resurrect it!
        }

        let ns = models::Namespace {
            name: name.clone(),
            parent: parent.clone().unwrap_or_default(),
            is_deleted: false,
            deleted_at: 0,
            labels: req.labels.clone(),
        };

        if req.recursive {
            // Provision missing parents top-down
            let mut current_parent = String::new();
            let mut it = parts.iter().peekable();
            while let Some(part) = it.next() {
                if current_parent.is_empty() {
                    current_parent = part.to_string();
                } else {
                    current_parent = format!("{}:{}", current_parent, part);
                }

                let check_key = format!("Namespace/{}", current_parent);
                if self
                    .gateway
                    .kv
                    .get_msg::<models::Namespace>(META_NS, &check_key)
                    .await
                    .unwrap_or(None)
                    .is_none()
                {
                    // Re-calculate the grand-parent to correctly link edges
                    let gp_parts: Vec<&str> = current_parent.split(':').collect();
                    let grandparent = if gp_parts.len() > 1 {
                        let mut p = gp_parts.clone();
                        p.pop();
                        Some(p.join(":"))
                    } else {
                        None
                    };

                    let p_ns = models::Namespace {
                        name: current_parent.clone(),
                        parent: grandparent.clone().unwrap_or_default(),
                        is_deleted: false,
                        deleted_at: 0,
                        labels: std::collections::HashMap::new(),
                    };

                    if let Some(ref gp) = grandparent {
                        let edge_ns = format!("{}:ns:internal", gp);
                        let edge_key = format!("NamespaceRef/{}", current_parent);
                        let _ = self
                            .gateway
                            .kv
                            .set(&edge_ns, &edge_key, current_parent.as_bytes())
                            .await;
                    } else {
                        let edge_key = format!("NamespaceRef/{}", current_parent);
                        let _ = self
                            .gateway
                            .kv
                            .set(ROOT_EDGE_NS, &edge_key, current_parent.as_bytes())
                            .await;
                    }

                    let _ = self.gateway.kv.set_msg(META_NS, &check_key, &p_ns).await;
                }
            }
        }

        // If it has a parent, insert an edge reference under the parent
        if let Some(ref p) = parent {
            let edge_ns = format!("{}:ns:internal", p);
            let edge_key = format!("NamespaceRef/{}", name);
            self.gateway
                .kv
                .set(&edge_ns, &edge_key, name.as_bytes())
                .await
                .map_err(|e| {
                    tonic::Status::internal(format!("Failed to write edge reference: {}", e))
                })?;
        } else {
            // Root namespace edge reference
            let edge_key = format!("NamespaceRef/{}", name);
            self.gateway
                .kv
                .set(ROOT_EDGE_NS, &edge_key, name.as_bytes())
                .await
                .map_err(|e| {
                    tonic::Status::internal(format!("Failed to write root edge reference: {}", e))
                })?;
        }

        // Save the metadata node
        let meta_key = format!("Namespace/{}", name);
        self.gateway
            .kv
            .set_msg(META_NS, &meta_key, &ns)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to write namespace metadata: {}", e))
            })?;

        Ok(tonic::Response::new(proto::NamespaceResponse {
            name: ns.name,
            parent: if ns.parent.is_empty() {
                None
            } else {
                Some(ns.parent)
            },
            is_deleted: ns.is_deleted,
            deleted_at: ns.deleted_at,
            labels: ns.labels,
        }))
    }

    pub async fn handle_get_namespace(
        &self,
        req: tonic::Request<proto::GetNamespaceRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceResponse>, tonic::Status> {
        crate::require_auth!(self, req, ns::TALON_SYSTEM);
        let req = req.into_inner();

        let name = req.name;

        let meta_key = format!("Namespace/{}", name);

        let ns = self
            .gateway
            .kv
            .get_msg::<models::Namespace>(META_NS, &meta_key)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to read namespace metadata: {}", e))
            })?
            .ok_or_else(|| tonic::Status::not_found("Namespace not found"))?;

        Ok(tonic::Response::new(proto::NamespaceResponse {
            name: ns.name,
            parent: if ns.parent.is_empty() {
                None
            } else {
                Some(ns.parent)
            },
            is_deleted: ns.is_deleted,
            deleted_at: ns.deleted_at,
            labels: ns.labels,
        }))
    }

    pub async fn handle_delete_namespace(
        &self,
        req: tonic::Request<proto::DeleteNamespaceRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceResponse>, tonic::Status> {
        crate::require_auth!(self, req, ns::TALON_SYSTEM);
        let req = req.into_inner();

        let name = req.name;

        let meta_key = format!("Namespace/{}", name);

        let mut ns = self
            .gateway
            .kv
            .get_msg::<models::Namespace>(META_NS, &meta_key)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to read namespace metadata: {}", e))
            })?
            .ok_or_else(|| tonic::Status::not_found("Namespace not found"))?;

        if ns.is_deleted {
            return Err(tonic::Status::failed_precondition(
                "Namespace is already deleted",
            ));
        }

        ns.is_deleted = true;
        ns.deleted_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.gateway
            .kv
            .set_msg(META_NS, &meta_key, &ns)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to write namespace metadata: {}", e))
            })?;

        Ok(tonic::Response::new(proto::NamespaceResponse {
            name: ns.name,
            parent: if ns.parent.is_empty() {
                None
            } else {
                Some(ns.parent)
            },
            is_deleted: ns.is_deleted,
            deleted_at: ns.deleted_at,
            labels: ns.labels,
        }))
    }

    pub async fn handle_list_namespaces(
        &self,
        req: tonic::Request<proto::ListNamespacesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListNamespacesResponse>, tonic::Status> {
        crate::require_auth!(self, req, ns::TALON_SYSTEM);
        let req = req.into_inner();

        let mut namespace_names = Vec::new();

        let parent = req.parent.unwrap_or_default();
        let edge_ns = if parent.is_empty() {
            ROOT_EDGE_NS.to_string()
        } else {
            format!("{}:ns:internal", parent)
        };

        let keys = self
            .gateway
            .kv
            .list_keys(&edge_ns, "NamespaceRef/")
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to list namespace references: {}", e))
            })?;

        for k in keys {
            if let Some(stripped) = k.strip_prefix("NamespaceRef/") {
                namespace_names.push(stripped.to_string());
            }
        }

        // Fetch actual metadata to populate the full response
        let mut namespaces = Vec::new();

        for name in namespace_names {
            let meta_key = format!("Namespace/{}", name);
            if let Ok(Some(ns)) = self
                .gateway
                .kv
                .get_msg::<models::Namespace>(META_NS, &meta_key)
                .await
            {
                if !ns.is_deleted {
                    namespaces.push(proto::NamespaceResponse {
                        name: ns.name,
                        parent: if ns.parent.is_empty() {
                            None
                        } else {
                            Some(ns.parent)
                        },
                        is_deleted: ns.is_deleted,
                        deleted_at: ns.deleted_at,
                        labels: ns.labels,
                    });
                }
            }
        }

        Ok(tonic::Response::new(proto::ListNamespacesResponse {
            namespaces,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{scheduler::NoopSchedulerBackend, KeyValueStore, MessagePublisher};
    use crate::gateway::{server::Gateway, session_streams::SessionStreamHub};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::{Mutex, RwLock};

    struct MockKvStore {
        store: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MockKvStore {
        fn new() -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
            }
        }

        fn make_key(ns: &str, k: &str) -> String {
            format!("{}/{}", ns, k)
        }
    }

    #[async_trait::async_trait]
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
            for k in map.keys() {
                if k.starts_with(&ns_prefix) {
                    let stripped = k.strip_prefix(&ns_root).unwrap();
                    results.push(stripped.to_string());
                }
            }
            Ok(results)
        }
    }

    struct MockPubSub;
    #[async_trait::async_trait]
    impl MessagePublisher for MockPubSub {
        async fn publish(&self, _topic: &str, _message: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }
        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<std::pin::Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>>
        {
            Ok(Box::pin(futures::stream::empty()))
        }
    }

    fn setup_mock_gateway_handler() -> GrpcGatewayHandler {
        let pubsub = Arc::new(MockPubSub);
        let gateway = Arc::new(Gateway {
            auth_config: None,
            kv: Arc::new(MockKvStore::new()),
            pubsub: pubsub.clone(),
            scheduler: Arc::new(NoopSchedulerBackend),
            session_streams: Arc::new(SessionStreamHub::new(pubsub)),
        });
        GrpcGatewayHandler { gateway }
    }

    #[tokio::test]
    async fn test_create_namespace_recursive() {
        let handler = setup_mock_gateway_handler();

        let req = tonic::Request::new(proto::CreateNamespaceRequest {
            name: "org:team:prod".to_string(),
            recursive: true,
            labels: HashMap::new(),
        });

        let res = handler
            .handle_create_namespace(req)
            .await
            .unwrap()
            .into_inner();
        assert_eq!(res.parent.unwrap(), "org:team");
        assert_eq!(res.name, "org:team:prod");

        let list_req = tonic::Request::new(proto::ListNamespacesRequest { parent: None });
        let list_res = handler
            .handle_list_namespaces(list_req)
            .await
            .unwrap()
            .into_inner();
        assert_eq!(list_res.namespaces.len(), 1);
        assert_eq!(list_res.namespaces[0].name, "org");

        let list_req_org = tonic::Request::new(proto::ListNamespacesRequest {
            parent: Some("org".to_string()),
        });
        let list_res_org = handler
            .handle_list_namespaces(list_req_org)
            .await
            .unwrap()
            .into_inner();
        assert_eq!(list_res_org.namespaces.len(), 1);
        assert_eq!(list_res_org.namespaces[0].name, "org:team");
        assert_eq!(list_res_org.namespaces[0].parent.as_deref(), Some("org"));
    }

    #[tokio::test]
    async fn test_delete_namespace_tombstone() {
        let handler = setup_mock_gateway_handler();

        let req = tonic::Request::new(proto::CreateNamespaceRequest {
            name: "test-delete".to_string(),
            recursive: false,
            labels: HashMap::new(),
        });
        handler.handle_create_namespace(req).await.unwrap();

        let del_req = tonic::Request::new(proto::DeleteNamespaceRequest {
            name: "test-delete".to_string(),
        });
        let del_res = handler
            .handle_delete_namespace(del_req)
            .await
            .unwrap()
            .into_inner();
        assert_eq!(del_res.is_deleted, true);
        assert!(del_res.deleted_at > 0);

        let list_req = tonic::Request::new(proto::ListNamespacesRequest { parent: None });
        let list_res = handler
            .handle_list_namespaces(list_req)
            .await
            .unwrap()
            .into_inner();
        assert_eq!(list_res.namespaces.len(), 0);
    }
}
