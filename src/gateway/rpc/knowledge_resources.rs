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
    let path_key = keys::knowledge(&spec.path);
    if let Some(bytes) = kv
        .get(namespace, &path_key)
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
        let path_key = keys::knowledge(&spec.path);

        if let Some(existing_bytes) =
            self.gateway.kv.get(&req.ns, &path_key).await.map_err(|e| {
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

        let resource_key = keys::knowledge_resource(&meta.name);
        if let Some(existing) = self
            .gateway
            .kv
            .get_msg::<manifests::Knowledge>(&req.ns, &resource_key)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to read knowledge resource: {}", e))
            })?
        {
            if let Some(existing_spec) = existing.spec {
                if existing_spec.path != spec.path && !existing_spec.path.is_empty() {
                    let old_key = keys::knowledge(&existing_spec.path);
                    self.gateway
                        .kv
                        .delete(&req.ns, &old_key)
                        .await
                        .map_err(|e| {
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
            .set(&req.ns, &path_key, &bytes)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to write knowledge: {}", e)))?;
        self.gateway
            .kv
            .set_msg(&req.ns, &resource_key, &knowledge)
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
        let key = keys::knowledge_resource(&req.name);
        let knowledge = self
            .gateway
            .kv
            .get_msg::<manifests::Knowledge>(&req.ns, &key)
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
            .list_keys(&req.ns, keys::knowledge_resource_prefix())
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to list knowledge resources: {}", e))
            })?
        {
            if let Some(entry) = self
                .gateway
                .kv
                .get_msg::<manifests::Knowledge>(&req.ns, &key)
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
        let resource_key = keys::knowledge_resource(&req.name);
        let Some(knowledge) = self
            .gateway
            .kv
            .get_msg::<manifests::Knowledge>(&req.ns, &resource_key)
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
                .delete(&req.ns, &keys::knowledge(&spec.path))
                .await
                .map_err(|e| {
                    tonic::Status::internal(format!("Failed to delete knowledge: {}", e))
                })?;
        }
        self.gateway
            .kv
            .delete(&req.ns, &resource_key)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to delete knowledge resource: {}", e))
            })?;

        Ok(tonic::Response::new(
            proto::DeleteNamespaceKnowledgeResponse { success: true },
        ))
    }
}
