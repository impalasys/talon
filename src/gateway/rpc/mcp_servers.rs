// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{manifests, proto, GrpcGatewayHandler};
use crate::control::{events, keys, ns, topics, ProtoKeyValueStoreExt};
use prost::Message;

impl GrpcGatewayHandler {
    pub async fn handle_create_mcp_server(
        &self,
        req: tonic::Request<proto::CreateMcpServerRequest>,
    ) -> Result<tonic::Response<proto::McpServerResponse>, tonic::Status> {
        crate::require_auth!(self, req, ns::TALON_SYSTEM);
        let msg = req.into_inner();
        let mut server = msg
            .server
            .ok_or_else(|| tonic::Status::invalid_argument("missing MCP server"))?;

        let meta = server
            .metadata
            .as_mut()
            .ok_or_else(|| tonic::Status::invalid_argument("MCPServer missing metadata"))?;
        if meta.name.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "MCPServer missing metadata.name",
            ));
        }

        if server.api_version.is_empty() {
            server.api_version = "v1".to_string();
        }
        if server.kind.is_empty() {
            server.kind = "MCPServer".to_string();
        }
        if !meta.namespace.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "MCPServer metadata.namespace is not supported; MCP servers are stored in Sys",
            ));
        }

        let server_name = meta.name.clone();
        let spec = server
            .spec
            .as_ref()
            .ok_or_else(|| tonic::Status::invalid_argument("MCPServer missing spec"))?;
        if spec.transport.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "MCPServer spec.transport is required",
            ));
        }
        if spec.target.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "MCPServer spec.target is required",
            ));
        }

        let key = keys::mcp_server(&server_name);
        let action = if self
            .gateway
            .kv
            .get_msg::<manifests::McpServer>(&key)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?
            .is_some()
        {
            events::SystemAction::Update
        } else {
            events::SystemAction::Create
        };
        self.gateway
            .kv
            .set_msg(&key, &server)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;

        let event = events::LifecycleEvent {
            resource_type: "McpServer".to_string(),
            name: server_name,
            ns: ns::TALON_SYSTEM.to_string(),
            action: action as i32,
            timestamp: chrono::Utc::now().timestamp_micros(),
        };
        self.gateway
            .pubsub
            .publish(topics::RESOURCE_LIFECYCLE_TOPIC, &event.encode_to_vec())
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to publish event: {}", e)))?;

        Ok(tonic::Response::new(proto::McpServerResponse {
            server: Some(server),
        }))
    }

    pub async fn handle_get_mcp_server(
        &self,
        req: tonic::Request<proto::GetMcpServerRequest>,
    ) -> Result<tonic::Response<proto::McpServerResponse>, tonic::Status> {
        crate::require_auth!(self, req, ns::TALON_SYSTEM);
        let msg = req.into_inner();
        let key = keys::mcp_server(&msg.name);

        let server = self
            .gateway
            .kv
            .get_msg::<manifests::McpServer>(&key)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?
            .ok_or_else(|| tonic::Status::not_found("MCPServer not found"))?;

        Ok(tonic::Response::new(proto::McpServerResponse {
            server: Some(server),
        }))
    }

    pub async fn handle_list_mcp_servers(
        &self,
        req: tonic::Request<proto::ListMcpServersRequest>,
    ) -> Result<tonic::Response<proto::ListMcpServersResponse>, tonic::Status> {
        crate::require_auth!(self, req, ns::TALON_SYSTEM);
        let _msg = req.into_inner();
        let keys = self
            .gateway
            .kv
            .list_keys(&keys::mcp_server_prefix())
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;

        let mut servers = Vec::new();
        for key in keys {
            if let Some(server) = self
                .gateway
                .kv
                .get_msg::<manifests::McpServer>(&key)
                .await
                .map_err(|e| tonic::Status::internal(e.to_string()))?
            {
                servers.push(server);
            }
        }

        Ok(tonic::Response::new(proto::ListMcpServersResponse {
            servers,
        }))
    }

    pub async fn handle_delete_mcp_server(
        &self,
        req: tonic::Request<proto::DeleteMcpServerRequest>,
    ) -> Result<tonic::Response<proto::DeleteMcpServerResponse>, tonic::Status> {
        crate::require_auth!(self, req, ns::TALON_SYSTEM);
        let msg = req.into_inner();
        let key = keys::mcp_server(&msg.name);

        if self
            .gateway
            .kv
            .get_msg::<manifests::McpServer>(&key)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?
            .is_none()
        {
            return Err(tonic::Status::not_found("MCPServer not found"));
        }

        self.gateway
            .kv
            .delete(&key)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;

        let event = events::LifecycleEvent {
            resource_type: "McpServer".to_string(),
            name: msg.name,
            ns: ns::TALON_SYSTEM.to_string(),
            action: events::SystemAction::Delete as i32,
            timestamp: chrono::Utc::now().timestamp_micros(),
        };
        self.gateway
            .pubsub
            .publish(topics::RESOURCE_LIFECYCLE_TOPIC, &event.encode_to_vec())
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to publish event: {}", e)))?;

        Ok(tonic::Response::new(proto::DeleteMcpServerResponse {
            success: true,
        }))
    }
}
