// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::manifests;
use super::proto;
use super::GrpcGatewayHandler;
use crate::control::{keys, ns};
use anyhow::Result;

impl GrpcGatewayHandler {
    pub async fn handle_create_agent_template(
        &self,
        req: tonic::Request<proto::CreateAgentTemplateRequest>,
    ) -> Result<tonic::Response<proto::AgentTemplateResponse>, tonic::Status> {
        crate::require_auth!(self, req, ns::TALON_SYSTEM);
        let msg = req.into_inner();
        let mut template = msg
            .template
            .ok_or_else(|| tonic::Status::invalid_argument("missing template"))?;
        let name = template
            .metadata
            .as_ref()
            .map(|m| m.name.clone())
            .unwrap_or_default();
        if name.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "template missing metadata.name",
            ));
        }
        if template.definition.is_none() {
            return Err(tonic::Status::invalid_argument(
                "template missing definition",
            ));
        }

        // Apply defaults if fields are missing
        if template.api_version.is_empty() {
            template.api_version = "v1".to_string();
        }
        if template.kind.is_empty() {
            template.kind = "AgentTemplate".to_string();
        }
        if let Some(meta) = template.metadata.as_mut() {
            if meta.namespace.is_empty() {
                meta.namespace = ns::TALON_SYSTEM.to_string();
            } else if meta.namespace != ns::TALON_SYSTEM {
                return Err(tonic::Status::invalid_argument(format!(
                    "AgentTemplate metadata.namespace must be empty or '{}'",
                    ns::TALON_SYSTEM
                )));
            }
        }

        let key = keys::agent_template(&name);

        // We allow Apply/Upsert semantics, so we don't return an error if it exists.
        // The KV store will just overwrite the existing key.
        use crate::control::ProtoKeyValueStoreExt;
        self.gateway
            .kv
            .set_msg(ns::TALON_SYSTEM, &key, &template)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;

        Ok(tonic::Response::new(proto::AgentTemplateResponse {
            template: Some(template),
        }))
    }

    pub async fn handle_get_agent_template(
        &self,
        req: tonic::Request<proto::GetAgentTemplateRequest>,
    ) -> Result<tonic::Response<proto::AgentTemplateResponse>, tonic::Status> {
        crate::require_auth!(self, req, ns::TALON_SYSTEM);
        let msg = req.into_inner();
        let key = keys::agent_template(&msg.name);

        use crate::control::ProtoKeyValueStoreExt;
        if let Ok(Some(template)) = self
            .gateway
            .kv
            .get_msg::<manifests::AgentTemplate>(ns::TALON_SYSTEM, &key)
            .await
        {
            Ok(tonic::Response::new(proto::AgentTemplateResponse {
                template: Some(template),
            }))
        } else {
            Err(tonic::Status::not_found("Agent Template not found"))
        }
    }

    pub async fn handle_list_agent_templates(
        &self,
        _req: tonic::Request<proto::ListAgentTemplatesRequest>,
    ) -> Result<tonic::Response<proto::ListAgentTemplatesResponse>, tonic::Status> {
        crate::require_auth!(self, _req, ns::TALON_SYSTEM);

        let prefix = "AgentTemplate/";
        let keys = self
            .gateway
            .kv
            .list_keys(ns::TALON_SYSTEM, prefix)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;

        use crate::control::ProtoKeyValueStoreExt;
        let mut templates = Vec::new();
        for key in keys {
            if let Ok(Some(template)) = self
                .gateway
                .kv
                .get_msg::<manifests::AgentTemplate>(ns::TALON_SYSTEM, &key)
                .await
            {
                templates.push(template);
            }
        }

        Ok(tonic::Response::new(proto::ListAgentTemplatesResponse {
            templates,
        }))
    }

    pub async fn handle_delete_agent_template(
        &self,
        req: tonic::Request<proto::DeleteAgentTemplateRequest>,
    ) -> Result<tonic::Response<proto::DeleteAgentTemplateResponse>, tonic::Status> {
        crate::require_auth!(self, req, ns::TALON_SYSTEM);
        let msg = req.into_inner();
        let key = keys::agent_template(&msg.name);

        use crate::control::ProtoKeyValueStoreExt;
        if let Ok(Some(_)) = self
            .gateway
            .kv
            .get_msg::<manifests::AgentTemplate>(ns::TALON_SYSTEM, &key)
            .await
        {
            self.gateway
                .kv
                .delete(ns::TALON_SYSTEM, &key)
                .await
                .map_err(|e| tonic::Status::internal(e.to_string()))?;
            Ok(tonic::Response::new(proto::DeleteAgentTemplateResponse {
                success: true,
            }))
        } else {
            Err(tonic::Status::not_found("Agent Template not found"))
        }
    }
}
