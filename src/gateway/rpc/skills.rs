// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::keys;
use crate::control::ProtoKeyValueStoreExt;
use crate::gateway::rpc::{manifests, proto, GrpcGatewayHandler};
use futures::stream::{self, StreamExt, TryStreamExt};

const LIST_NAMESPACE_SKILLS_CONCURRENCY: usize = 32;

impl GrpcGatewayHandler {
    pub async fn handle_create_namespace_skill(
        &self,
        req: tonic::Request<proto::CreateNamespaceSkillRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceSkillResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();

        let mut skill = req
            .skill
            .ok_or_else(|| tonic::Status::invalid_argument("Skill manifest missing"))?;
        if skill.kind.is_empty() {
            skill.kind = "Skill".to_string();
        }

        let meta = skill
            .metadata
            .as_mut()
            .ok_or_else(|| tonic::Status::invalid_argument("Skill missing metadata"))?;
        if meta.name.trim().is_empty() {
            return Err(tonic::Status::invalid_argument(
                "Skill metadata.name is required",
            ));
        }
        if meta.namespace.is_empty() {
            meta.namespace = req.ns.clone();
        } else if meta.namespace != req.ns {
            return Err(tonic::Status::invalid_argument(
                "Skill metadata.namespace must match request namespace",
            ));
        }

        let spec = skill
            .spec
            .as_ref()
            .ok_or_else(|| tonic::Status::invalid_argument("Skill missing spec"))?;
        if spec.description.trim().is_empty() {
            return Err(tonic::Status::invalid_argument(
                "Skill spec.description is required",
            ));
        }
        if spec.instructions.trim().is_empty() {
            return Err(tonic::Status::invalid_argument(
                "Skill spec.instructions is required",
            ));
        }

        let key = keys::skill(&req.ns, &meta.name);
        self.gateway
            .kv
            .set_msg(&key, &skill)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to write skill: {}", e)))?;

        Ok(tonic::Response::new(proto::NamespaceSkillResponse {
            skill: Some(skill),
        }))
    }

    pub async fn handle_get_namespace_skill(
        &self,
        req: tonic::Request<proto::GetNamespaceSkillRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceSkillResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let key = keys::skill(&req.ns, &req.name);
        let skill = self
            .gateway
            .kv
            .get_msg::<manifests::Skill>(&key)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to read skill: {}", e)))?
            .ok_or_else(|| tonic::Status::not_found("Skill not found"))?;

        Ok(tonic::Response::new(proto::NamespaceSkillResponse {
            skill: Some(skill),
        }))
    }

    pub async fn handle_list_namespace_skills(
        &self,
        req: tonic::Request<proto::ListNamespaceSkillsRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListNamespaceSkillsResponse>, tonic::Status>
    {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let keys = self
            .gateway
            .kv
            .list_keys(&keys::skill_prefix(&req.ns))
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to list skills: {}", e)))?;
        let fetches = keys.into_iter().map(|key| {
            let kv = self.gateway.kv.clone();
            async move { kv.get_msg::<manifests::Skill>(&key).await }
        });
        let skills = stream::iter(fetches)
            .buffered(LIST_NAMESPACE_SKILLS_CONCURRENCY)
            .try_filter_map(|skill| async move { Ok(skill) })
            .try_collect::<Vec<_>>()
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to fetch skills: {}", e)))?;

        Ok(tonic::Response::new(proto::ListNamespaceSkillsResponse {
            skills,
        }))
    }

    pub async fn handle_delete_namespace_skill(
        &self,
        req: tonic::Request<proto::DeleteNamespaceSkillRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteNamespaceSkillResponse>, tonic::Status>
    {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let key = keys::skill(&req.ns, &req.name);
        if self
            .gateway
            .kv
            .get_msg::<manifests::Skill>(&key)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to read skill: {}", e)))?
            .is_none()
        {
            return Err(tonic::Status::not_found("Skill not found"));
        }

        self.gateway
            .kv
            .delete(&key)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to delete skill: {}", e)))?;

        Ok(tonic::Response::new(proto::DeleteNamespaceSkillResponse {
            success: true,
        }))
    }
}
