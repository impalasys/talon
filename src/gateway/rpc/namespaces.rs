// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::resource_model::{self, NamespaceResourceExt, TypedResource};
use crate::control::ProtoKeyValueStoreExt;
use crate::control::{events, keys, topics};
use crate::gateway::rpc::{proto, resources_proto, GrpcGatewayHandler};
use prost::Message;
use std::time::{SystemTime, UNIX_EPOCH};

fn namespace_response(ns: resources_proto::Namespace) -> proto::NamespaceResponse {
    proto::NamespaceResponse {
        name: ns.name().to_string(),
        parent: if ns.parent().is_empty() {
            None
        } else {
            Some(ns.parent().to_string())
        },
        is_deleted: ns.is_deleted(),
        deleted_at: ns.deleted_at(),
        labels: ns.labels().clone(),
    }
}

impl GrpcGatewayHandler {
    pub async fn handle_create_namespace(
        &self,
        req: tonic::Request<proto::CreateNamespaceRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceResponse>, tonic::Status> {
        let metadata = req.metadata().clone();
        let req = req.into_inner();

        let name = req.name.clone();
        if name.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "Namespace name cannot be empty",
            ));
        }
        if let Some(auth_config) = &self.gateway.auth_config {
            crate::gateway::auth::check_auth_for_operation(
                &metadata,
                auth_config,
                crate::gateway::auth::AuthzOperation::ReadWrite,
                &name,
                None,
                None,
            )?;
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
        let meta_key = keys::namespace_metadata(&name);
        if let Ok(Some(mut existing_ns)) = self
            .gateway
            .kv
            .get_msg::<resources_proto::Namespace>(&meta_key)
            .await
        {
            if !existing_ns.is_deleted() {
                if let Some(labels) = existing_ns.labels_mut() {
                    labels.extend(req.labels.clone());
                }
                self.gateway
                    .kv
                    .set_msg(&meta_key, &existing_ns)
                    .await
                    .map_err(|e| {
                        tonic::Status::internal(format!(
                            "Failed to update namespace metadata: {}",
                            e
                        ))
                    })?;
                self.warn_on_namespace_publish_error(
                    &existing_ns,
                    events::ResourceChangeType::Updated,
                    &["metadata", "spec"],
                )
                .await;

                return Ok(tonic::Response::new(namespace_response(existing_ns)));
            }
            // If it is tombstoned, falling through here will overwrite and resurrect it!
        }

        let ns = resource_model::namespace(
            name.clone(),
            parent.clone().unwrap_or_default(),
            req.labels.clone(),
        );

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

                let check_key = keys::namespace_metadata(&current_parent);
                if self
                    .gateway
                    .kv
                    .get_msg::<resources_proto::Namespace>(&check_key)
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

                    let p_ns = resource_model::namespace(
                        current_parent.clone(),
                        grandparent.clone().unwrap_or_default(),
                        std::collections::HashMap::new(),
                    );

                    let child_segment =
                        current_parent.rsplit(':').next().unwrap_or(&current_parent);
                    let edge_key = keys::namespace_ref(grandparent.as_deref(), child_segment);
                    let _ = self
                        .gateway
                        .kv
                        .set(&edge_key, current_parent.as_bytes())
                        .await;

                    let _ = self.gateway.kv.set_msg(&check_key, &p_ns).await;
                    self.warn_on_namespace_publish_error(
                        &p_ns,
                        events::ResourceChangeType::Created,
                        &["metadata", "spec"],
                    )
                    .await;
                }
            }
        }

        // If it has a parent, insert an edge reference under the parent
        if let Some(ref p) = parent {
            let child_segment = name.rsplit(':').next().unwrap_or(&name);
            let edge_key = keys::namespace_ref(Some(p), child_segment);
            self.gateway
                .kv
                .set(&edge_key, name.as_bytes())
                .await
                .map_err(|e| {
                    tonic::Status::internal(format!("Failed to write edge reference: {}", e))
                })?;
        } else {
            // Root namespace edge reference
            let edge_key = keys::namespace_ref(None, &name);
            self.gateway
                .kv
                .set(&edge_key, name.as_bytes())
                .await
                .map_err(|e| {
                    tonic::Status::internal(format!("Failed to write root edge reference: {}", e))
                })?;
        }

        // Save the metadata node
        let meta_key = keys::namespace_metadata(&name);
        self.gateway.kv.set_msg(&meta_key, &ns).await.map_err(|e| {
            tonic::Status::internal(format!("Failed to write namespace metadata: {}", e))
        })?;
        self.warn_on_namespace_publish_error(
            &ns,
            events::ResourceChangeType::Created,
            &["metadata", "spec"],
        )
        .await;

        Ok(tonic::Response::new(namespace_response(ns)))
    }

    pub async fn handle_get_namespace(
        &self,
        req: tonic::Request<proto::GetNamespaceRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceResponse>, tonic::Status> {
        let metadata = req.metadata().clone();
        let req = req.into_inner();

        let name = req.name;
        if let Some(auth_config) = &self.gateway.auth_config {
            crate::gateway::auth::check_auth_for_operation(
                &metadata,
                auth_config,
                crate::gateway::auth::AuthzOperation::Read,
                &name,
                None,
                None,
            )?;
        }

        let meta_key = keys::namespace_metadata(&name);

        let ns = self
            .gateway
            .kv
            .get_msg::<resources_proto::Namespace>(&meta_key)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to read namespace metadata: {}", e))
            })?
            .ok_or_else(|| tonic::Status::not_found("Namespace not found"))?;

        Ok(tonic::Response::new(namespace_response(ns)))
    }

    pub async fn handle_delete_namespace(
        &self,
        req: tonic::Request<proto::DeleteNamespaceRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceResponse>, tonic::Status> {
        let metadata = req.metadata().clone();
        let req = req.into_inner();

        let name = req.name;
        if let Some(auth_config) = &self.gateway.auth_config {
            crate::gateway::auth::check_auth_for_operation(
                &metadata,
                auth_config,
                crate::gateway::auth::AuthzOperation::ReadWrite,
                &name,
                None,
                None,
            )?;
        }

        let meta_key = keys::namespace_metadata(&name);

        let mut ns = self
            .gateway
            .kv
            .get_msg::<resources_proto::Namespace>(&meta_key)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to read namespace metadata: {}", e))
            })?
            .ok_or_else(|| tonic::Status::not_found("Namespace not found"))?;

        if ns.is_deleted() {
            return Err(tonic::Status::failed_precondition(
                "Namespace is already deleted",
            ));
        }

        ns.set_deleted(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        );

        self.gateway.kv.set_msg(&meta_key, &ns).await.map_err(|e| {
            tonic::Status::internal(format!("Failed to write namespace metadata: {}", e))
        })?;
        self.warn_on_namespace_publish_error(
            &ns,
            events::ResourceChangeType::Deleted,
            &["metadata", "status"],
        )
        .await;

        Ok(tonic::Response::new(namespace_response(ns)))
    }

    pub async fn handle_list_namespaces(
        &self,
        req: tonic::Request<proto::ListNamespacesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListNamespacesResponse>, tonic::Status> {
        let metadata = req.metadata().clone();
        let req = req.into_inner();

        let mut namespace_names = Vec::new();

        let parent = req.parent.unwrap_or_default();
        if let Some(auth_config) = &self.gateway.auth_config {
            crate::gateway::auth::check_auth_for_operation(
                &metadata,
                auth_config,
                crate::gateway::auth::AuthzOperation::Read,
                &parent,
                None,
                None,
            )?;
        }
        let edge_prefix =
            keys::namespace_ref_prefix((!parent.is_empty()).then_some(parent.as_str()));

        let keys = self
            .gateway
            .kv
            .list_keys(&edge_prefix, None)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to list namespace references: {}", e))
            })?;

        for k in keys {
            if let Ok(Some(bytes)) = self.gateway.kv.get(&k).await {
                if let Ok(name) = String::from_utf8(bytes) {
                    namespace_names.push(name);
                }
            }
        }

        // Fetch actual metadata to populate the full response
        let mut namespaces = Vec::new();

        for name in namespace_names {
            let meta_key = keys::namespace_metadata(&name);
            if let Ok(Some(ns)) = self
                .gateway
                .kv
                .get_msg::<resources_proto::Namespace>(&meta_key)
                .await
            {
                if !ns.is_deleted() {
                    namespaces.push(namespace_response(ns));
                }
            }
        }

        Ok(tonic::Response::new(proto::ListNamespacesResponse {
            namespaces,
        }))
    }

    async fn publish_namespace_changed(
        &self,
        namespace: &resources_proto::Namespace,
        change_type: events::ResourceChangeType,
        changed_sections: &[&str],
    ) -> std::result::Result<(), tonic::Status> {
        let meta = namespace
            .metadata
            .as_ref()
            .ok_or_else(|| tonic::Status::internal("Namespace metadata missing"))?;
        let event = events::ResourceChangedEvent {
            namespace: meta.namespace.clone(),
            resource_kind: "Namespace".to_string(),
            name: meta.name.clone(),
            uid: meta.uid.clone(),
            resource_version: meta.resource_version.clone(),
            generation: meta.generation,
            change_type: change_type as i32,
            changed_sections: changed_sections
                .iter()
                .map(|section| section.to_string())
                .collect(),
            timestamp: chrono::Utc::now().timestamp_micros(),
        };
        self.gateway
            .pubsub
            .publish(topics::RESOURCE_LIFECYCLE_TOPIC, &event.encode_to_vec())
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to publish namespace change: {}", e))
            })
    }

    async fn warn_on_namespace_publish_error(
        &self,
        namespace: &resources_proto::Namespace,
        change_type: events::ResourceChangeType,
        changed_sections: &[&str],
    ) {
        if let Err(err) = self
            .publish_namespace_changed(namespace, change_type, changed_sections)
            .await
        {
            tracing::warn!(
                namespace = %namespace.name(),
                error = %err,
                "namespace changed event publish failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::security::platform_jwt;
    use crate::control::{ControlPlane, MessagePublisher};
    use crate::gateway::auth::{AuthConfig, Claims};
    use crate::gateway::server::Gateway;
    use crate::test_support::{MockKvStore, PlatformJwtEnvGuard, TEST_PLATFORM_JWT_ISSUER};
    use std::collections::HashMap;
    use std::sync::Arc;

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
        setup_mock_gateway_handler_with_auth(None)
    }

    fn setup_mock_gateway_handler_with_auth(auth_config: Option<AuthConfig>) -> GrpcGatewayHandler {
        let pubsub = Arc::new(MockPubSub);
        let control_plane = ControlPlane::builder(Arc::new(MockKvStore::default()), pubsub).build();
        let gateway = Arc::new(Gateway::from_control_plane(auth_config, control_plane));
        GrpcGatewayHandler { gateway }
    }

    fn jwt_auth_config() -> AuthConfig {
        AuthConfig::jwt_platform()
    }

    fn scoped_token(ns: &str) -> String {
        let claims = Claims {
            iss: Some(TEST_PLATFORM_JWT_ISSUER.to_string()),
            sub: "tenant-admin".to_string(),
            aud: platform_jwt::TALON_GATEWAY_AUDIENCE.to_string(),
            iat: Some(1),
            exp: 10000000000,
            ns: Some(ns.to_string()),
            agent: None,
            session: None,
            channel: None,
            origins: Vec::new(),
            grants: Vec::new(),
        };
        platform_jwt::PlatformJwtKey::from_pem(platform_jwt::TEST_RSA_PRIVATE_KEY)
            .unwrap()
            .sign(&claims)
            .unwrap()
    }

    fn with_bearer<T>(mut req: tonic::Request<T>, token: &str) -> tonic::Request<T> {
        req.metadata_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
        req
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

    #[tokio::test]
    async fn test_namespace_validation_get_and_recreate_paths() {
        let handler = setup_mock_gateway_handler();

        let empty = handler
            .handle_create_namespace(tonic::Request::new(proto::CreateNamespaceRequest {
                name: String::new(),
                recursive: false,
                labels: HashMap::new(),
            }))
            .await
            .expect_err("empty namespace should fail");
        assert_eq!(empty.code(), tonic::Code::InvalidArgument);

        let missing = handler
            .handle_get_namespace(tonic::Request::new(proto::GetNamespaceRequest {
                name: "missing".to_string(),
            }))
            .await
            .expect_err("missing namespace should fail");
        assert_eq!(missing.code(), tonic::Code::NotFound);

        let created = handler
            .handle_create_namespace(tonic::Request::new(proto::CreateNamespaceRequest {
                name: "team".to_string(),
                recursive: false,
                labels: HashMap::from([("env".to_string(), "dev".to_string())]),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(created.labels.get("env").map(String::as_str), Some("dev"));

        let recreated = handler
            .handle_create_namespace(tonic::Request::new(proto::CreateNamespaceRequest {
                name: "team".to_string(),
                recursive: false,
                labels: HashMap::from([("owner".to_string(), "ops".to_string())]),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(recreated.labels.get("env").map(String::as_str), Some("dev"));
        assert_eq!(
            recreated.labels.get("owner").map(String::as_str),
            Some("ops")
        );

        let fetched = handler
            .handle_get_namespace(tonic::Request::new(proto::GetNamespaceRequest {
                name: "team".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(fetched.labels.get("env").map(String::as_str), Some("dev"));
        assert_eq!(fetched.labels.get("owner").map(String::as_str), Some("ops"));
    }

    #[tokio::test]
    async fn test_scoped_jwt_can_manage_descendant_namespaces_only() {
        let _guard = PlatformJwtEnvGuard::acquire().await;
        let token = scoped_token("Tenant:conic");
        let handler = setup_mock_gateway_handler_with_auth(Some(jwt_auth_config()));

        handler
            .handle_create_namespace(with_bearer(
                tonic::Request::new(proto::CreateNamespaceRequest {
                    name: "Tenant:conic:Customers:13".to_string(),
                    recursive: true,
                    labels: HashMap::new(),
                }),
                &token,
            ))
            .await
            .unwrap();

        handler
            .handle_get_namespace(with_bearer(
                tonic::Request::new(proto::GetNamespaceRequest {
                    name: "Tenant:conic:Customers:13".to_string(),
                }),
                &token,
            ))
            .await
            .unwrap();

        handler
            .handle_list_namespaces(with_bearer(
                tonic::Request::new(proto::ListNamespacesRequest {
                    parent: Some("Tenant:conic".to_string()),
                }),
                &token,
            ))
            .await
            .unwrap();

        let sibling = handler
            .handle_create_namespace(with_bearer(
                tonic::Request::new(proto::CreateNamespaceRequest {
                    name: "Tenant:conic2".to_string(),
                    recursive: false,
                    labels: HashMap::new(),
                }),
                &token,
            ))
            .await
            .expect_err("prefix sibling should not be authorized");
        assert_eq!(sibling.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn test_delete_namespace_rejects_missing_and_already_deleted() {
        let handler = setup_mock_gateway_handler();

        let missing = handler
            .handle_delete_namespace(tonic::Request::new(proto::DeleteNamespaceRequest {
                name: "missing".to_string(),
            }))
            .await
            .expect_err("missing namespace should fail");
        assert_eq!(missing.code(), tonic::Code::NotFound);

        handler
            .handle_create_namespace(tonic::Request::new(proto::CreateNamespaceRequest {
                name: "gone".to_string(),
                recursive: false,
                labels: HashMap::new(),
            }))
            .await
            .unwrap();
        handler
            .handle_delete_namespace(tonic::Request::new(proto::DeleteNamespaceRequest {
                name: "gone".to_string(),
            }))
            .await
            .unwrap();

        let deleted_again = handler
            .handle_delete_namespace(tonic::Request::new(proto::DeleteNamespaceRequest {
                name: "gone".to_string(),
            }))
            .await
            .expect_err("second delete should fail");
        assert_eq!(deleted_again.code(), tonic::Code::FailedPrecondition);
    }
}
