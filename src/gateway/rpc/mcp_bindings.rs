// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{manifests, proto, GrpcGatewayHandler};
use crate::control::{events, keys, topics, ProtoKeyValueStoreExt};
use prost::Message;

impl GrpcGatewayHandler {
    pub async fn handle_create_mcp_server_binding(
        &self,
        req: tonic::Request<proto::CreateMcpServerBindingRequest>,
    ) -> Result<tonic::Response<proto::McpServerBindingResponse>, tonic::Status> {
        crate::require_auth!(self, req, crate::control::ns::TALON_SYSTEM);
        let msg = req.into_inner();
        if msg.ns.is_empty() {
            return Err(tonic::Status::invalid_argument("namespace is required"));
        }

        let mut binding = msg
            .binding
            .ok_or_else(|| tonic::Status::invalid_argument("missing MCP server binding"))?;
        if binding.api_version.is_empty() {
            binding.api_version = "v1".to_string();
        }
        if binding.kind.is_empty() {
            binding.kind = "McpServerBinding".to_string();
        }
        {
            let meta = binding.metadata.as_mut().ok_or_else(|| {
                tonic::Status::invalid_argument("McpServerBinding missing metadata")
            })?;
            if meta.name.is_empty() {
                return Err(tonic::Status::invalid_argument(
                    "McpServerBinding missing metadata.name",
                ));
            }
            if meta.namespace.is_empty() {
                meta.namespace = msg.ns.clone();
            } else if meta.namespace != msg.ns {
                return Err(tonic::Status::invalid_argument(
                    "McpServerBinding metadata.namespace must match request namespace",
                ));
            }
        }
        let binding_name = binding
            .metadata
            .as_ref()
            .map(|meta| meta.name.clone())
            .unwrap_or_default();

        let spec = binding
            .spec
            .as_ref()
            .ok_or_else(|| tonic::Status::invalid_argument("McpServerBinding missing spec"))?;
        if spec.server_ref.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "McpServerBinding spec.serverRef is required",
            ));
        }

        let server_key = keys::mcp_server(&spec.server_ref);
        let server = self
            .gateway
            .kv
            .get_msg::<manifests::McpServer>(&server_key)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?
            .ok_or_else(|| tonic::Status::failed_precondition("Referenced MCPServer not found"))?;
        let server_spec = server.spec.as_ref().ok_or_else(|| {
            tonic::Status::failed_precondition("Referenced MCPServer missing spec")
        })?;

        if let Some(auth_broker) = spec.auth_broker.as_ref() {
            let kind = if auth_broker.kind.trim().is_empty() {
                "http_bearer"
            } else {
                auth_broker.kind.trim()
            };
            if kind != "http_bearer" {
                return Err(tonic::Status::invalid_argument(
                    "McpServerBinding authBroker.kind must be 'http_bearer'",
                ));
            }
            if auth_broker.url.trim().is_empty() {
                return Err(tonic::Status::invalid_argument(
                    "McpServerBinding authBroker.url is required",
                ));
            }
            let parsed_url = reqwest::Url::parse(auth_broker.url.trim()).map_err(|_| {
                tonic::Status::invalid_argument(
                    "McpServerBinding authBroker.url must be an absolute http:// or https:// URL",
                )
            })?;
            if !matches!(parsed_url.scheme(), "http" | "https") || parsed_url.host().is_none() {
                return Err(tonic::Status::invalid_argument(
                    "McpServerBinding authBroker.url must be an absolute http:// or https:// URL",
                ));
            }
            if auth_broker.cache_ttl_seconds < 0 {
                return Err(tonic::Status::invalid_argument(
                    "McpServerBinding authBroker.cacheTtlSeconds must be non-negative",
                ));
            }
            if server_spec.transport != "http" {
                return Err(tonic::Status::failed_precondition(
                    "McpServerBinding authBroker is only supported for HTTP MCP servers",
                ));
            }
            if !spec.headers.is_empty() {
                return Err(tonic::Status::invalid_argument(
                    "McpServerBinding cannot define both spec.headers and spec.authBroker",
                ));
            }
        }

        if spec
            .allowed_tool_names
            .iter()
            .any(|name| name.trim().is_empty())
        {
            return Err(tonic::Status::invalid_argument(
                "McpServerBinding spec.allowedToolNames cannot contain empty values",
            ));
        }
        if spec
            .allowed_tool_names
            .iter()
            .any(|name| name != name.trim())
        {
            return Err(tonic::Status::invalid_argument(
                "McpServerBinding spec.allowedToolNames entries must not contain surrounding whitespace",
            ));
        }

        let key = keys::mcp_server_binding(&msg.ns, &binding_name);
        let action = if self
            .gateway
            .kv
            .get_msg::<manifests::McpServerBinding>(&key)
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
            .set_msg(&key, &binding)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;

        let event = events::LifecycleEvent {
            resource_type: "McpServerBinding".to_string(),
            name: binding_name,
            ns: msg.ns,
            action: action as i32,
            timestamp: chrono::Utc::now().timestamp_micros(),
        };
        self.gateway
            .pubsub
            .publish(topics::RESOURCE_LIFECYCLE_TOPIC, &event.encode_to_vec())
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to publish event: {}", e)))?;

        Ok(tonic::Response::new(proto::McpServerBindingResponse {
            binding: Some(binding),
        }))
    }

    pub async fn handle_get_mcp_server_binding(
        &self,
        req: tonic::Request<proto::GetMcpServerBindingRequest>,
    ) -> Result<tonic::Response<proto::McpServerBindingResponse>, tonic::Status> {
        crate::require_auth!(self, req, crate::control::ns::TALON_SYSTEM);
        let msg = req.into_inner();
        if msg.ns.is_empty() {
            return Err(tonic::Status::invalid_argument("namespace is required"));
        }
        if msg.name.is_empty() {
            return Err(tonic::Status::invalid_argument("name is required"));
        }
        let key = keys::mcp_server_binding(&msg.ns, &msg.name);

        let binding = self
            .gateway
            .kv
            .get_msg::<manifests::McpServerBinding>(&key)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?
            .ok_or_else(|| tonic::Status::not_found("McpServerBinding not found"))?;

        Ok(tonic::Response::new(proto::McpServerBindingResponse {
            binding: Some(binding),
        }))
    }

    pub async fn handle_list_mcp_server_bindings(
        &self,
        req: tonic::Request<proto::ListMcpServerBindingsRequest>,
    ) -> Result<tonic::Response<proto::ListMcpServerBindingsResponse>, tonic::Status> {
        crate::require_auth!(self, req, crate::control::ns::TALON_SYSTEM);
        let msg = req.into_inner();
        if msg.ns.is_empty() {
            return Err(tonic::Status::invalid_argument("namespace is required"));
        }

        let keys = self
            .gateway
            .kv
            .list_keys(&keys::mcp_server_binding_prefix(&msg.ns))
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;

        let mut bindings = Vec::new();
        for key in keys {
            if let Some(binding) = self
                .gateway
                .kv
                .get_msg::<manifests::McpServerBinding>(&key)
                .await
                .map_err(|e| tonic::Status::internal(e.to_string()))?
            {
                bindings.push(binding);
            }
        }

        Ok(tonic::Response::new(proto::ListMcpServerBindingsResponse {
            bindings,
        }))
    }

    pub async fn handle_delete_mcp_server_binding(
        &self,
        req: tonic::Request<proto::DeleteMcpServerBindingRequest>,
    ) -> Result<tonic::Response<proto::DeleteMcpServerBindingResponse>, tonic::Status> {
        crate::require_auth!(self, req, crate::control::ns::TALON_SYSTEM);
        let msg = req.into_inner();
        if msg.ns.is_empty() {
            return Err(tonic::Status::invalid_argument("namespace is required"));
        }
        if msg.name.is_empty() {
            return Err(tonic::Status::invalid_argument("name is required"));
        }
        let key = keys::mcp_server_binding(&msg.ns, &msg.name);

        if self
            .gateway
            .kv
            .get_msg::<manifests::McpServerBinding>(&key)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?
            .is_none()
        {
            return Err(tonic::Status::not_found("McpServerBinding not found"));
        }

        self.gateway
            .kv
            .delete(&key)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;

        let event = events::LifecycleEvent {
            resource_type: "McpServerBinding".to_string(),
            name: msg.name,
            ns: msg.ns,
            action: events::SystemAction::Delete as i32,
            timestamp: chrono::Utc::now().timestamp_micros(),
        };
        self.gateway
            .pubsub
            .publish(topics::RESOURCE_LIFECYCLE_TOPIC, &event.encode_to_vec())
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to publish event: {}", e)))?;

        Ok(tonic::Response::new(
            proto::DeleteMcpServerBindingResponse { success: true },
        ))
    }
}
