// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::gateway::server::Gateway;
use std::sync::Arc;

use std::pin::Pin;

pub mod agents;
pub mod channels;
pub mod knowledge;
pub mod knowledge_resources;
pub mod mcp_bindings;
pub mod mcp_servers;
pub mod namespaces;
pub mod schedules;
pub mod sessions;
pub mod templates;
pub mod workflows;

#[cfg(test)]
mod channels_tests;
#[cfg(test)]
mod crud_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod sessions_tests;

#[cfg(not(feature = "bazel"))]
pub mod proto {
    tonic::include_proto!("talon.gateway");
}
#[cfg(feature = "bazel")]
pub mod proto {
    pub use talon_gateway_proto::talon::gateway::*;
}

#[cfg(not(feature = "bazel"))]
pub mod models {
    tonic::include_proto!("talon.models");
}
#[cfg(feature = "bazel")]
pub mod models {
    pub use talon_models_proto::talon::models::*;
}

#[cfg(not(feature = "bazel"))]
pub mod manifests {
    tonic::include_proto!("talon.manifests");
}
#[cfg(feature = "bazel")]
pub mod manifests {
    pub use talon_manifests_proto::talon::manifests::*;
}

#[cfg(not(feature = "bazel"))]
pub mod protobuf_value {
    pub use prost_types::{value, ListValue, Value};
}
#[cfg(feature = "bazel")]
pub mod protobuf_value {
    pub use struct_proto::google::protobuf::{value, ListValue, Value};
}

pub use crate::control::events;

#[macro_export]
macro_rules! require_auth {
    ($handler:expr, $req:expr, $ns:expr) => {
        if let Some(auth_config) = &$handler.gateway.auth_config {
            crate::gateway::auth::check_auth($req.metadata(), auth_config, $ns, None, None)?;
        }
    };
    ($handler:expr, $req:expr, $ns:expr, $agent:expr) => {
        if let Some(auth_config) = &$handler.gateway.auth_config {
            crate::gateway::auth::check_auth(
                $req.metadata(),
                auth_config,
                $ns,
                Some($agent),
                None,
            )?;
        }
    };
    ($handler:expr, $req:expr, $ns:expr, $agent:expr, $session:expr) => {
        if let Some(auth_config) = &$handler.gateway.auth_config {
            crate::gateway::auth::check_auth(
                $req.metadata(),
                auth_config,
                $ns,
                Some($agent),
                Some($session),
            )?;
        }
    };
}

pub struct GrpcGatewayHandler {
    pub gateway: Arc<Gateway>,
}

pub type ChannelEventStream = Pin<
    Box<
        dyn futures::Stream<Item = std::result::Result<events::ChannelEvent, tonic::Status>> + Send,
    >,
>;

pub type WorkflowEventStream = Pin<
    Box<
        dyn futures::Stream<Item = std::result::Result<models::WorkflowRunEvent, tonic::Status>>
            + Send,
    >,
>;

#[tonic::async_trait]
impl proto::gateway_service_server::GatewayService for GrpcGatewayHandler {
    // Agents
    async fn create_agent(
        &self,
        req: tonic::Request<proto::CreateAgentRequest>,
    ) -> std::result::Result<tonic::Response<proto::AgentResponse>, tonic::Status> {
        self.handle_create_agent(req).await
    }

    async fn get_agent(
        &self,
        req: tonic::Request<proto::GetAgentRequest>,
    ) -> std::result::Result<tonic::Response<proto::GetAgentResponse>, tonic::Status> {
        self.handle_get_agent(req).await
    }

    async fn modify_agent(
        &self,
        req: tonic::Request<proto::ModifyAgentRequest>,
    ) -> std::result::Result<tonic::Response<proto::AgentResponse>, tonic::Status> {
        self.handle_modify_agent(req).await
    }
    async fn list_agents(
        &self,
        req: tonic::Request<proto::ListAgentsRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListAgentsResponse>, tonic::Status> {
        self.handle_list_agents(req).await
    }

    // Agent Templates
    async fn create_agent_template(
        &self,
        req: tonic::Request<proto::CreateAgentTemplateRequest>,
    ) -> std::result::Result<tonic::Response<proto::AgentTemplateResponse>, tonic::Status> {
        self.handle_create_agent_template(req).await
    }

    async fn get_agent_template(
        &self,
        req: tonic::Request<proto::GetAgentTemplateRequest>,
    ) -> std::result::Result<tonic::Response<proto::AgentTemplateResponse>, tonic::Status> {
        self.handle_get_agent_template(req).await
    }

    async fn list_agent_templates(
        &self,
        req: tonic::Request<proto::ListAgentTemplatesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListAgentTemplatesResponse>, tonic::Status>
    {
        self.handle_list_agent_templates(req).await
    }

    async fn delete_agent_template(
        &self,
        req: tonic::Request<proto::DeleteAgentTemplateRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteAgentTemplateResponse>, tonic::Status>
    {
        self.handle_delete_agent_template(req).await
    }

    async fn create_mcp_server(
        &self,
        req: tonic::Request<proto::CreateMcpServerRequest>,
    ) -> std::result::Result<tonic::Response<proto::McpServerResponse>, tonic::Status> {
        self.handle_create_mcp_server(req).await
    }

    async fn get_mcp_server(
        &self,
        req: tonic::Request<proto::GetMcpServerRequest>,
    ) -> std::result::Result<tonic::Response<proto::McpServerResponse>, tonic::Status> {
        self.handle_get_mcp_server(req).await
    }

    async fn list_mcp_servers(
        &self,
        req: tonic::Request<proto::ListMcpServersRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListMcpServersResponse>, tonic::Status> {
        self.handle_list_mcp_servers(req).await
    }

    async fn delete_mcp_server(
        &self,
        req: tonic::Request<proto::DeleteMcpServerRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteMcpServerResponse>, tonic::Status> {
        self.handle_delete_mcp_server(req).await
    }

    async fn create_mcp_server_binding(
        &self,
        req: tonic::Request<proto::CreateMcpServerBindingRequest>,
    ) -> std::result::Result<tonic::Response<proto::McpServerBindingResponse>, tonic::Status> {
        self.handle_create_mcp_server_binding(req).await
    }

    async fn get_mcp_server_binding(
        &self,
        req: tonic::Request<proto::GetMcpServerBindingRequest>,
    ) -> std::result::Result<tonic::Response<proto::McpServerBindingResponse>, tonic::Status> {
        self.handle_get_mcp_server_binding(req).await
    }

    async fn list_mcp_server_bindings(
        &self,
        req: tonic::Request<proto::ListMcpServerBindingsRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListMcpServerBindingsResponse>, tonic::Status>
    {
        self.handle_list_mcp_server_bindings(req).await
    }

    async fn delete_mcp_server_binding(
        &self,
        req: tonic::Request<proto::DeleteMcpServerBindingRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteMcpServerBindingResponse>, tonic::Status>
    {
        self.handle_delete_mcp_server_binding(req).await
    }

    // Sessions
    async fn create_session(
        &self,
        req: tonic::Request<proto::CreateSessionRequest>,
    ) -> std::result::Result<tonic::Response<proto::SessionResponse>, tonic::Status> {
        self.handle_create_session(req).await
    }
    async fn get_session(
        &self,
        req: tonic::Request<proto::GetSessionRequest>,
    ) -> std::result::Result<tonic::Response<proto::SessionResponse>, tonic::Status> {
        self.handle_get_session(req).await
    }
    async fn list_session_messages(
        &self,
        req: tonic::Request<proto::ListSessionMessagesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListSessionMessagesResponse>, tonic::Status>
    {
        self.handle_list_session_messages(req).await
    }
    async fn list_sessions(
        &self,
        req: tonic::Request<proto::ListSessionsRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListSessionsResponse>, tonic::Status> {
        self.handle_list_sessions(req).await
    }
    async fn delete_session(
        &self,
        req: tonic::Request<proto::DeleteSessionRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteSessionResponse>, tonic::Status> {
        self.handle_delete_session(req).await
    }
    async fn clear_session(
        &self,
        req: tonic::Request<proto::ClearSessionRequest>,
    ) -> std::result::Result<tonic::Response<proto::ClearSessionResponse>, tonic::Status> {
        self.handle_clear_session(req).await
    }
    async fn create_schedule(
        &self,
        req: tonic::Request<proto::CreateScheduleRequest>,
    ) -> std::result::Result<tonic::Response<proto::ScheduleResponse>, tonic::Status> {
        self.handle_create_schedule(req).await
    }
    async fn get_schedule(
        &self,
        req: tonic::Request<proto::GetScheduleRequest>,
    ) -> std::result::Result<tonic::Response<proto::ScheduleResponse>, tonic::Status> {
        self.handle_get_schedule(req).await
    }
    async fn modify_schedule(
        &self,
        req: tonic::Request<proto::ModifyScheduleRequest>,
    ) -> std::result::Result<tonic::Response<proto::ScheduleResponse>, tonic::Status> {
        self.handle_modify_schedule(req).await
    }
    async fn list_schedules(
        &self,
        req: tonic::Request<proto::ListSchedulesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListSchedulesResponse>, tonic::Status> {
        self.handle_list_schedules(req).await
    }
    async fn delete_schedule(
        &self,
        req: tonic::Request<proto::DeleteScheduleRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteScheduleResponse>, tonic::Status> {
        self.handle_delete_schedule(req).await
    }
    async fn create_workflow(
        &self,
        req: tonic::Request<proto::CreateWorkflowRequest>,
    ) -> std::result::Result<tonic::Response<proto::WorkflowResponse>, tonic::Status> {
        self.handle_create_workflow(req).await
    }
    async fn get_workflow(
        &self,
        req: tonic::Request<proto::GetWorkflowRequest>,
    ) -> std::result::Result<tonic::Response<proto::WorkflowResponse>, tonic::Status> {
        self.handle_get_workflow(req).await
    }
    async fn list_workflows(
        &self,
        req: tonic::Request<proto::ListWorkflowsRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListWorkflowsResponse>, tonic::Status> {
        self.handle_list_workflows(req).await
    }
    async fn delete_workflow(
        &self,
        req: tonic::Request<proto::DeleteWorkflowRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteWorkflowResponse>, tonic::Status> {
        self.handle_delete_workflow(req).await
    }
    async fn create_workflow_run(
        &self,
        req: tonic::Request<proto::CreateWorkflowRunRequest>,
    ) -> std::result::Result<tonic::Response<proto::WorkflowRunResponse>, tonic::Status> {
        self.handle_create_workflow_run(req).await
    }
    async fn get_workflow_run(
        &self,
        req: tonic::Request<proto::GetWorkflowRunRequest>,
    ) -> std::result::Result<tonic::Response<proto::WorkflowRunResponse>, tonic::Status> {
        self.handle_get_workflow_run(req).await
    }
    async fn list_workflow_runs(
        &self,
        req: tonic::Request<proto::ListWorkflowRunsRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListWorkflowRunsResponse>, tonic::Status> {
        self.handle_list_workflow_runs(req).await
    }
    async fn resume_workflow_run(
        &self,
        req: tonic::Request<proto::ResumeWorkflowRunRequest>,
    ) -> std::result::Result<tonic::Response<proto::WorkflowRunResponse>, tonic::Status> {
        self.handle_resume_workflow_run(req).await
    }
    async fn cancel_workflow_run(
        &self,
        req: tonic::Request<proto::CancelWorkflowRunRequest>,
    ) -> std::result::Result<tonic::Response<proto::WorkflowRunResponse>, tonic::Status> {
        self.handle_cancel_workflow_run(req).await
    }
    async fn send_message(
        &self,
        req: tonic::Request<proto::SendMessageRequest>,
    ) -> std::result::Result<tonic::Response<proto::SendMessageResponse>, tonic::Status> {
        self.handle_send_message(req).await
    }
    async fn append_session_message(
        &self,
        req: tonic::Request<proto::AppendSessionMessageRequest>,
    ) -> std::result::Result<tonic::Response<proto::AppendSessionMessageResponse>, tonic::Status>
    {
        self.handle_append_session_message(req).await
    }
    async fn stop_session_generation(
        &self,
        req: tonic::Request<proto::StopSessionGenerationRequest>,
    ) -> std::result::Result<tonic::Response<proto::StopSessionGenerationResponse>, tonic::Status>
    {
        self.handle_stop_session_generation(req).await
    }

    // Memory
    async fn get_knowledge(
        &self,
        req: tonic::Request<proto::GetKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::KnowledgeResponse>, tonic::Status> {
        self.handle_get_knowledge(req).await
    }
    async fn search_knowledge(
        &self,
        req: tonic::Request<proto::SearchKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::SearchKnowledgeResponse>, tonic::Status> {
        self.handle_search_knowledge(req).await
    }
    async fn create_namespace_knowledge(
        &self,
        req: tonic::Request<proto::CreateNamespaceKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceKnowledgeResponse>, tonic::Status>
    {
        self.handle_create_namespace_knowledge(req).await
    }
    async fn get_namespace_knowledge(
        &self,
        req: tonic::Request<proto::GetNamespaceKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceKnowledgeResponse>, tonic::Status>
    {
        self.handle_get_namespace_knowledge(req).await
    }
    async fn list_namespace_knowledge(
        &self,
        req: tonic::Request<proto::ListNamespaceKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListNamespaceKnowledgeResponse>, tonic::Status>
    {
        self.handle_list_namespace_knowledge(req).await
    }
    async fn delete_namespace_knowledge(
        &self,
        req: tonic::Request<proto::DeleteNamespaceKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteNamespaceKnowledgeResponse>, tonic::Status>
    {
        self.handle_delete_namespace_knowledge(req).await
    }

    // Namespaces
    async fn create_namespace(
        &self,
        req: tonic::Request<proto::CreateNamespaceRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceResponse>, tonic::Status> {
        self.handle_create_namespace(req).await
    }
    async fn get_namespace(
        &self,
        req: tonic::Request<proto::GetNamespaceRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceResponse>, tonic::Status> {
        self.handle_get_namespace(req).await
    }
    async fn delete_namespace(
        &self,
        req: tonic::Request<proto::DeleteNamespaceRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceResponse>, tonic::Status> {
        self.handle_delete_namespace(req).await
    }
    async fn list_namespaces(
        &self,
        req: tonic::Request<proto::ListNamespacesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListNamespacesResponse>, tonic::Status> {
        self.handle_list_namespaces(req).await
    }

    type StreamSessionPartsStream = Pin<
        Box<
            dyn futures::Stream<
                    Item = std::result::Result<
                        crate::control::events::SessionMessagePartEvent,
                        tonic::Status,
                    >,
                > + Send,
        >,
    >;

    async fn stream_session_parts(
        &self,
        req: tonic::Request<proto::StreamSessionPartsRequest>,
    ) -> std::result::Result<tonic::Response<Self::StreamSessionPartsStream>, tonic::Status> {
        self.handle_stream_session_parts(req).await
    }

    type StreamSessionPartsBatchStream = Pin<
        Box<
            dyn futures::Stream<
                    Item = std::result::Result<
                        crate::control::events::SessionMessagePartEvent,
                        tonic::Status,
                    >,
                > + Send,
        >,
    >;

    async fn stream_session_parts_batch(
        &self,
        req: tonic::Request<proto::StreamSessionPartsBatchRequest>,
    ) -> std::result::Result<tonic::Response<Self::StreamSessionPartsBatchStream>, tonic::Status>
    {
        self.handle_stream_session_parts_batch(req).await
    }

    async fn create_channel(
        &self,
        req: tonic::Request<proto::CreateChannelRequest>,
    ) -> std::result::Result<tonic::Response<proto::ChannelResponse>, tonic::Status> {
        self.handle_create_channel(req).await
    }

    async fn get_channel(
        &self,
        req: tonic::Request<proto::GetChannelRequest>,
    ) -> std::result::Result<tonic::Response<proto::ChannelResponse>, tonic::Status> {
        self.handle_get_channel(req).await
    }

    async fn modify_channel(
        &self,
        req: tonic::Request<proto::ModifyChannelRequest>,
    ) -> std::result::Result<tonic::Response<proto::ChannelResponse>, tonic::Status> {
        self.handle_modify_channel(req).await
    }

    async fn list_channels(
        &self,
        req: tonic::Request<proto::ListChannelsRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListChannelsResponse>, tonic::Status> {
        self.handle_list_channels(req).await
    }

    async fn delete_channel(
        &self,
        req: tonic::Request<proto::DeleteChannelRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteChannelResponse>, tonic::Status> {
        self.handle_delete_channel(req).await
    }

    async fn post_channel_message(
        &self,
        req: tonic::Request<proto::PostChannelMessageRequest>,
    ) -> std::result::Result<tonic::Response<proto::PostChannelMessageResponse>, tonic::Status>
    {
        self.handle_post_channel_message(req).await
    }

    async fn get_channel_message(
        &self,
        req: tonic::Request<proto::GetChannelMessageRequest>,
    ) -> std::result::Result<tonic::Response<proto::ChannelMessageResponse>, tonic::Status> {
        self.handle_get_channel_message(req).await
    }

    async fn list_channel_messages(
        &self,
        req: tonic::Request<proto::ListChannelMessagesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListChannelMessagesResponse>, tonic::Status>
    {
        self.handle_list_channel_messages(req).await
    }

    async fn create_channel_subscription(
        &self,
        req: tonic::Request<proto::CreateChannelSubscriptionRequest>,
    ) -> std::result::Result<tonic::Response<proto::ChannelSubscriptionResponse>, tonic::Status>
    {
        self.handle_create_channel_subscription(req).await
    }

    async fn get_channel_subscription(
        &self,
        req: tonic::Request<proto::GetChannelSubscriptionRequest>,
    ) -> std::result::Result<tonic::Response<proto::ChannelSubscriptionResponse>, tonic::Status>
    {
        self.handle_get_channel_subscription(req).await
    }

    async fn modify_channel_subscription(
        &self,
        req: tonic::Request<proto::ModifyChannelSubscriptionRequest>,
    ) -> std::result::Result<tonic::Response<proto::ChannelSubscriptionResponse>, tonic::Status>
    {
        self.handle_modify_channel_subscription(req).await
    }

    async fn list_channel_subscriptions(
        &self,
        req: tonic::Request<proto::ListChannelSubscriptionsRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListChannelSubscriptionsResponse>, tonic::Status>
    {
        self.handle_list_channel_subscriptions(req).await
    }

    async fn delete_channel_subscription(
        &self,
        req: tonic::Request<proto::DeleteChannelSubscriptionRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteChannelSubscriptionResponse>, tonic::Status>
    {
        self.handle_delete_channel_subscription(req).await
    }

    type StreamChannelEventsStream = ChannelEventStream;

    async fn stream_channel_events(
        &self,
        req: tonic::Request<proto::StreamChannelEventsRequest>,
    ) -> std::result::Result<tonic::Response<Self::StreamChannelEventsStream>, tonic::Status> {
        self.handle_stream_channel_events(req).await
    }

    type StreamWorkflowEventsStream = WorkflowEventStream;

    async fn stream_workflow_events(
        &self,
        req: tonic::Request<proto::StreamWorkflowEventsRequest>,
    ) -> std::result::Result<tonic::Response<Self::StreamWorkflowEventsStream>, tonic::Status> {
        self.handle_stream_workflow_events(req).await
    }
}
