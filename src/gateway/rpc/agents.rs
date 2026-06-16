// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{proto, resources_proto, GrpcGatewayHandler};
use crate::control::keys;
use crate::control::resource_model::{self, NamespaceResourceExt, TypedResource};
use crate::control::resources::ResourceStore;
use crate::control::ProtoKeyValueStoreExt;
use crate::harness::agents::resolver::resolve_agent_spec;

impl GrpcGatewayHandler {
    pub async fn handle_create_agent(
        &self,
        req: tonic::Request<proto::CreateAgentRequest>,
    ) -> std::result::Result<tonic::Response<proto::AgentResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();

        let agent = req
            .name
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let validate_k8s_name = |name: &str| -> bool {
            if name.is_empty() || name.len() > 63 {
                return false;
            }
            let chars: Vec<char> = name.chars().collect();
            if !chars.first().unwrap().is_ascii_lowercase()
                && !chars.first().unwrap().is_ascii_digit()
            {
                return false;
            }
            if !chars.last().unwrap().is_ascii_lowercase()
                && !chars.last().unwrap().is_ascii_digit()
            {
                return false;
            }
            chars
                .iter()
                .all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        };

        if !validate_k8s_name(&agent) {
            return Err(tonic::Status::invalid_argument(
                "Agent name must consist of lower case alphanumeric characters or '-', and must start and end with an alphanumeric character.",
            ));
        }
        if req.ns.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "Namespace cannot be empty.",
            ));
        }

        let meta_key = keys::namespace_metadata(&req.ns);
        let ns_check = self
            .gateway
            .kv
            .get_msg::<resources_proto::Namespace>(&meta_key)
            .await;
        match ns_check {
            Ok(Some(ns_record)) if !ns_record.is_deleted() => {
                // valid
            }
            Ok(Some(_)) => {
                return Err(tonic::Status::failed_precondition(format!(
                    "Namespace '{}' is deleted.",
                    req.ns
                )));
            }
            Ok(None) => {
                return Err(tonic::Status::failed_precondition(format!(
                    "Namespace '{}' does not exist.",
                    req.ns
                )));
            }
            Err(e) => {
                return Err(tonic::Status::internal(format!(
                    "Database error checking namespace '{}': {}",
                    req.ns, e
                )));
            }
        }

        let spec = req
            .spec
            .ok_or_else(|| tonic::Status::invalid_argument("Agent spec must be provided"))?;
        let spec = resolve_agent_spec(spec)
            .map_err(|e| tonic::Status::invalid_argument(format!("Invalid agent spec: {e}")))?;

        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        store
            .upsert(
                &req.ns,
                resource_model::agent_resource(
                    req.ns.clone(),
                    agent.clone(),
                    spec,
                    req.labels.clone(),
                ),
            )
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to save agent state: {}", e)))?;

        Ok(tonic::Response::new(proto::AgentResponse {
            agent,
            ns: req.ns,
            labels: req.labels,
        }))
    }
    pub async fn handle_get_agent(
        &self,
        req: tonic::Request<proto::GetAgentRequest>,
    ) -> std::result::Result<tonic::Response<proto::GetAgentResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns, &req.get_ref().name);
        let req = req.into_inner();

        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        let agent = store
            .get_agent(&req.ns, &req.name)
            .await
            .map_err(|e| tonic::Status::internal(format!("Database error: {}", e)))?
            .ok_or_else(|| {
                tonic::Status::not_found(format!(
                    "Agent '{}' not found in namespace '{}'",
                    req.name, req.ns
                ))
            })?;

        Ok(tonic::Response::new(proto::GetAgentResponse {
            agent: Some(agent),
        }))
    }

    pub async fn handle_modify_agent(
        &self,
        req: tonic::Request<proto::ModifyAgentRequest>,
    ) -> std::result::Result<tonic::Response<proto::AgentResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns, &req.get_ref().agent);
        let req = req.into_inner();

        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        let mut agent = store
            .get_agent(&req.ns, &req.agent)
            .await
            .map_err(|e| tonic::Status::internal(format!("Database error: {}", e)))?
            .ok_or_else(|| {
                tonic::Status::not_found(format!(
                    "Agent '{}' not found in namespace '{}'",
                    req.agent, req.ns
                ))
            })?;

        if let Some(spec) = req.spec {
            let spec = resolve_agent_spec(spec)
                .map_err(|e| tonic::Status::invalid_argument(format!("Invalid agent spec: {e}")))?;
            agent.spec = Some(spec);
        }
        if !req.labels.is_empty() {
            if let Some(labels) = agent.labels_mut() {
                *labels = req.labels.clone();
            }
        }

        let metadata = agent
            .metadata
            .clone()
            .ok_or_else(|| tonic::Status::internal("Agent metadata missing"))?;
        let spec = agent
            .spec
            .clone()
            .ok_or_else(|| tonic::Status::internal("Agent spec missing"))?;
        let status = agent.status.clone().unwrap_or_default();
        store
            .upsert(
                &req.ns,
                resources_proto::Resource {
                    api_version: "talon.impalasys.com/v1".to_string(),
                    kind: "Agent".to_string(),
                    metadata: Some(metadata),
                    spec: Some(resources_proto::ResourceSpec {
                        kind: Some(resources_proto::resource_spec::Kind::Agent(spec)),
                    }),
                    status: Some(resources_proto::ResourceStatus {
                        kind: Some(resources_proto::resource_status::Kind::Agent(status)),
                    }),
                },
            )
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to update agent state: {}", e)))?;

        Ok(tonic::Response::new(proto::AgentResponse {
            agent: agent.name().to_string(),
            ns: agent.namespace().to_string(),
            labels: agent.labels().clone(),
        }))
    }

    pub async fn handle_list_agents(
        &self,
        req: tonic::Request<proto::ListAgentsRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListAgentsResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();

        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        let mut agents: Vec<String> = store
            .list(&req.ns, Some("Agent"))
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to list agents: {}", e)))?
            .into_iter()
            .filter_map(|resource| resource.metadata.map(|metadata| metadata.name))
            .collect();
        agents.sort();

        Ok(tonic::Response::new(proto::ListAgentsResponse { agents }))
    }
}
