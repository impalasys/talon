use super::{models, proto, GrpcGatewayHandler};
use crate::agents::resolver::resolve_agent_definition;
use crate::control::keys;
use crate::control::topics;
use crate::control::ProtoKeyValueStoreExt;
use prost::Message;

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
            return Err(tonic::Status::invalid_argument("Agent name must consist of lower case alphanumeric characters or '-', and must start and end with an alphanumeric character."));
        }
        if req.ns.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "Namespace cannot be empty.",
            ));
        }

        let meta_key = format!("Namespace/{}", req.ns);
        let ns_check = self
            .gateway
            .kv
            .get_msg::<models::Namespace>("talon-system:ns", &meta_key)
            .await;
        match ns_check {
            Ok(Some(ns_record)) if !ns_record.is_deleted => {
                // valid
            }
            Ok(Some(_)) => {
                return Err(tonic::Status::failed_precondition(format!(
                    "Namespace '{}' is deleted.",
                    req.ns
                )))
            }
            Ok(None) => {
                return Err(tonic::Status::failed_precondition(format!(
                    "Namespace '{}' does not exist.",
                    req.ns
                )))
            }
            Err(e) => {
                return Err(tonic::Status::internal(format!(
                    "Database error checking namespace '{}': {}",
                    req.ns, e
                )))
            }
        }

        let definition = req
            .definition
            .ok_or_else(|| tonic::Status::invalid_argument("Agent definition must be provided"))?;
        let resolved = resolve_agent_definition(self.gateway.kv.as_ref(), &definition)
            .await
            .map_err(|e| {
                tonic::Status::invalid_argument(format!("Invalid agent definition: {e}"))
            })?;

        let agent_model = models::Agent {
            name: agent.clone(),
            ns: req.ns.clone(),
            definition: Some(definition),
            effective_spec: Some(resolved.effective_spec),
            template_deps: resolved.template_deps,
            labels: req.labels.clone(),
        };

        let agent_db_key = keys::agent(&agent);

        self.gateway
            .kv
            .set_msg(&req.ns, &agent_db_key, &agent_model)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to save agent state: {}", e)))?;

        let event = crate::control::events::LifecycleEvent {
            resource_type: "Agent".to_string(),
            name: agent.clone(),
            ns: req.ns.clone(),
            action: crate::control::events::SystemAction::Create as i32,
            timestamp: chrono::Utc::now().timestamp_micros(),
        };
        self.gateway
            .pubsub
            .publish(topics::RESOURCE_LIFECYCLE_TOPIC, &event.encode_to_vec())
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to publish event: {}", e)))?;

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

        let agent_db_key = keys::agent(&req.name);

        let agent = self
            .gateway
            .kv
            .get_msg::<models::Agent>(&req.ns, &agent_db_key)
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

        let agent_db_key = keys::agent(&req.agent);
        let mut agent = self
            .gateway
            .kv
            .get_msg::<models::Agent>(&req.ns, &agent_db_key)
            .await
            .map_err(|e| tonic::Status::internal(format!("Database error: {}", e)))?
            .ok_or_else(|| {
                tonic::Status::not_found(format!(
                    "Agent '{}' not found in namespace '{}'",
                    req.agent, req.ns
                ))
            })?;

        if let Some(definition) = req.definition {
            let resolved = resolve_agent_definition(self.gateway.kv.as_ref(), &definition)
                .await
                .map_err(|e| {
                    tonic::Status::invalid_argument(format!("Invalid agent definition: {e}"))
                })?;
            agent.definition = Some(definition);
            agent.effective_spec = Some(resolved.effective_spec);
            agent.template_deps = resolved.template_deps;
        }
        if !req.labels.is_empty() {
            agent.labels = req.labels.clone();
        }

        self.gateway
            .kv
            .set_msg(&req.ns, &agent_db_key, &agent)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to update agent state: {}", e)))?;

        Ok(tonic::Response::new(proto::AgentResponse {
            agent: agent.name,
            ns: agent.ns,
            labels: agent.labels,
        }))
    }

    pub async fn handle_list_agents(
        &self,
        req: tonic::Request<proto::ListAgentsRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListAgentsResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();

        let prefix = "Agent/";

        let keys = self
            .gateway
            .kv
            .list_keys(&req.ns, prefix)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to list agents: {}", e)))?;

        let agents: Vec<String> = keys
            .into_iter()
            .filter_map(|k| {
                let stripped = k.strip_prefix(prefix).unwrap_or(&k);
                if stripped.contains('/') {
                    None // Skip nested resources like Agent/test/Session/default
                } else {
                    Some(stripped.to_string())
                }
            })
            .collect();

        Ok(tonic::Response::new(proto::ListAgentsResponse { agents }))
    }
}
