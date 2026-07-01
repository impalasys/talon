// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::gateway::server::Gateway;
use std::sync::Arc;

use std::pin::Pin;

pub mod auth;
pub mod channels;
pub mod connectors;
pub mod knowledge;
pub mod namespaces;
pub mod resources;
pub mod search;
pub mod sessions;
pub mod workflows;

#[cfg(not(feature = "bazel"))]
pub mod generated {
    pub mod config {
        tonic::include_proto!("talon.config");
    }
    pub mod data {
        tonic::include_proto!("talon.data");
    }
    pub mod harness {
        tonic::include_proto!("talon.harness");
    }
    pub mod external {
        tonic::include_proto!("talon.external");
    }
    pub mod events {
        pub use crate::control::events::*;
    }
    pub mod resources {
        tonic::include_proto!("talon.resources");
    }
    pub mod proto {
        tonic::include_proto!("talon.v1");
    }
    pub mod worker_proto {
        tonic::include_proto!("talon.worker.v1");
    }
}

#[cfg(feature = "bazel")]
pub mod generated {
    pub mod config {
        pub use talon_config_proto::talon::config::*;
    }
    pub mod data {
        pub use talon_data_proto::talon::data::*;
    }
    pub mod harness {
        pub use talon_harness_proto::talon::harness::*;
    }
    pub mod external {
        pub use talon_external_proto::talon::external::*;
    }
    pub mod resources {
        pub use talon_resources_proto::talon::resources::*;
    }
    pub mod proto {
        pub use talon_api_proto::talon::v1::*;
    }
    pub mod worker_proto {
        pub use talon_worker_proto::talon::worker::v1::*;
    }
}

pub mod proto {
    pub use super::generated::proto::*;
}

pub mod data_proto {
    pub use super::generated::data::*;
}

pub mod harness_proto {
    pub use super::generated::harness::*;
}

pub mod external_proto {
    pub use super::generated::external::*;
}

pub mod resources_proto {
    pub use super::generated::resources::*;
}

pub mod worker_proto {
    pub use super::generated::worker_proto::*;
}

pub mod manifests {
    pub use super::resources_proto::*;
    pub type ObjectMeta = super::resources_proto::ResourceMeta;
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
    (read, $handler:expr, $req:expr, $ns:expr) => {
        if let Some(auth_config) = &$handler.gateway.auth_config {
            crate::gateway::auth::check_auth_for_operation(
                $req.metadata(),
                auth_config,
                crate::gateway::auth::AuthzOperation::Read,
                $ns,
                None,
                None,
            )?;
        }
    };
    (read, $handler:expr, $req:expr, $ns:expr, $agent:expr) => {
        if let Some(auth_config) = &$handler.gateway.auth_config {
            crate::gateway::auth::check_auth_for_operation(
                $req.metadata(),
                auth_config,
                crate::gateway::auth::AuthzOperation::Read,
                $ns,
                Some($agent),
                None,
            )?;
        }
    };
    (read, $handler:expr, $req:expr, $ns:expr, $agent:expr, $session:expr) => {
        if let Some(auth_config) = &$handler.gateway.auth_config {
            crate::gateway::auth::check_auth_for_operation(
                $req.metadata(),
                auth_config,
                crate::gateway::auth::AuthzOperation::Read,
                $ns,
                Some($agent),
                Some($session),
            )?;
        }
    };
    ($handler:expr, $req:expr, $ns:expr) => {
        if let Some(auth_config) = &$handler.gateway.auth_config {
            crate::gateway::auth::check_auth_for_operation(
                $req.metadata(),
                auth_config,
                crate::gateway::auth::AuthzOperation::ReadWrite,
                $ns,
                None,
                None,
            )?;
        }
    };
    ($handler:expr, $req:expr, $ns:expr, $agent:expr) => {
        if let Some(auth_config) = &$handler.gateway.auth_config {
            crate::gateway::auth::check_auth_for_operation(
                $req.metadata(),
                auth_config,
                crate::gateway::auth::AuthzOperation::ReadWrite,
                $ns,
                Some($agent),
                None,
            )?;
        }
    };
    ($handler:expr, $req:expr, $ns:expr, $agent:expr, $session:expr) => {
        if let Some(auth_config) = &$handler.gateway.auth_config {
            crate::gateway::auth::check_auth_for_operation(
                $req.metadata(),
                auth_config,
                crate::gateway::auth::AuthzOperation::ReadWrite,
                $ns,
                Some($agent),
                Some($session),
            )?;
        }
    };
}

#[derive(Clone)]
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
        dyn futures::Stream<Item = std::result::Result<data_proto::WorkflowRunEvent, tonic::Status>>
            + Send,
    >,
>;

pub type SessionPartsStream = Pin<
    Box<
        dyn futures::Stream<
                Item = std::result::Result<
                    crate::control::events::SessionMessagePartEvent,
                    tonic::Status,
                >,
            > + Send,
    >,
>;

#[tonic::async_trait]
impl proto::resource_service_server::ResourceService for GrpcGatewayHandler {
    async fn create(
        &self,
        req: tonic::Request<proto::CreateResourceRequest>,
    ) -> std::result::Result<tonic::Response<proto::ResourceResponse>, tonic::Status> {
        self.handle_create_resource(req).await
    }

    async fn get(
        &self,
        req: tonic::Request<proto::GetResourceRequest>,
    ) -> std::result::Result<tonic::Response<proto::ResourceResponse>, tonic::Status> {
        self.handle_get_resource(req).await
    }

    async fn list(
        &self,
        req: tonic::Request<proto::ListResourcesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListResourcesResponse>, tonic::Status> {
        self.handle_list_resources(req).await
    }

    async fn delete(
        &self,
        req: tonic::Request<proto::DeleteResourceRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteResourceResponse>, tonic::Status> {
        self.handle_delete_resource(req).await
    }
}

#[tonic::async_trait]
impl proto::connector_service_server::ConnectorService for GrpcGatewayHandler {
    async fn ingest_message_event(
        &self,
        req: tonic::Request<external_proto::ConnectorMessageEvent>,
    ) -> std::result::Result<
        tonic::Response<external_proto::ConnectorMessageEventResponse>,
        tonic::Status,
    > {
        self.handle_ingest_connector_message_event(req).await
    }

    async fn report_status(
        &self,
        req: tonic::Request<external_proto::ConnectorStatusEvent>,
    ) -> std::result::Result<tonic::Response<external_proto::ConnectorAckResponse>, tonic::Status>
    {
        self.handle_report_connector_status(req).await
    }
}

#[tonic::async_trait]
impl proto::session_service_server::SessionService for GrpcGatewayHandler {
    type StreamPartsStream = SessionPartsStream;
    type StreamPartsBatchStream = SessionPartsStream;
    type SubmitTurnStream = SessionPartsStream;

    async fn create(
        &self,
        req: tonic::Request<proto::CreateSessionRequest>,
    ) -> std::result::Result<tonic::Response<proto::SessionResponse>, tonic::Status> {
        self.handle_create_session(req).await
    }

    async fn get(
        &self,
        req: tonic::Request<proto::GetSessionRequest>,
    ) -> std::result::Result<tonic::Response<proto::SessionResponse>, tonic::Status> {
        self.handle_get_session(req).await
    }

    async fn list_messages(
        &self,
        req: tonic::Request<proto::ListSessionMessagesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListSessionMessagesResponse>, tonic::Status>
    {
        self.handle_list_session_messages(req).await
    }

    async fn list(
        &self,
        req: tonic::Request<proto::ListSessionsRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListSessionsResponse>, tonic::Status> {
        self.handle_list_sessions(req).await
    }

    async fn delete(
        &self,
        req: tonic::Request<proto::DeleteSessionRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteSessionResponse>, tonic::Status> {
        self.handle_delete_session(req).await
    }

    async fn clear(
        &self,
        req: tonic::Request<proto::ClearSessionRequest>,
    ) -> std::result::Result<tonic::Response<proto::ClearSessionResponse>, tonic::Status> {
        self.handle_clear_session(req).await
    }

    async fn send_message(
        &self,
        req: tonic::Request<proto::SendMessageRequest>,
    ) -> std::result::Result<tonic::Response<proto::SendMessageResponse>, tonic::Status> {
        self.handle_send_message(req).await
    }

    async fn append_message(
        &self,
        req: tonic::Request<proto::AppendSessionMessageRequest>,
    ) -> std::result::Result<tonic::Response<proto::AppendSessionMessageResponse>, tonic::Status>
    {
        self.handle_append_session_message(req).await
    }

    async fn answer_permission(
        &self,
        req: tonic::Request<proto::AnswerSessionPermissionRequest>,
    ) -> std::result::Result<tonic::Response<proto::AnswerSessionPermissionResponse>, tonic::Status>
    {
        self.handle_answer_session_permission(req).await
    }

    async fn stop_generation(
        &self,
        req: tonic::Request<proto::StopSessionGenerationRequest>,
    ) -> std::result::Result<tonic::Response<proto::StopSessionGenerationResponse>, tonic::Status>
    {
        self.handle_stop_session_generation(req).await
    }

    async fn stream_parts(
        &self,
        req: tonic::Request<proto::StreamSessionPartsRequest>,
    ) -> std::result::Result<tonic::Response<Self::StreamPartsStream>, tonic::Status> {
        self.handle_stream_session_parts(req).await
    }

    async fn stream_parts_batch(
        &self,
        req: tonic::Request<proto::StreamSessionPartsBatchRequest>,
    ) -> std::result::Result<tonic::Response<Self::StreamPartsBatchStream>, tonic::Status> {
        self.handle_stream_session_parts_batch(req).await
    }

    async fn submit_turn(
        &self,
        req: tonic::Request<proto::SubmitSessionTurnRequest>,
    ) -> std::result::Result<tonic::Response<Self::SubmitTurnStream>, tonic::Status> {
        self.handle_submit_session_turn(req).await
    }
}

#[tonic::async_trait]
impl proto::workflow_service_server::WorkflowService for GrpcGatewayHandler {
    type StreamEventsStream = WorkflowEventStream;

    async fn create_run(
        &self,
        req: tonic::Request<proto::CreateWorkflowRunRequest>,
    ) -> std::result::Result<tonic::Response<proto::WorkflowRunResponse>, tonic::Status> {
        self.handle_create_workflow_run(req).await
    }

    async fn get_run(
        &self,
        req: tonic::Request<proto::GetWorkflowRunRequest>,
    ) -> std::result::Result<tonic::Response<proto::WorkflowRunResponse>, tonic::Status> {
        self.handle_get_workflow_run(req).await
    }

    async fn list_runs(
        &self,
        req: tonic::Request<proto::ListWorkflowRunsRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListWorkflowRunsResponse>, tonic::Status> {
        self.handle_list_workflow_runs(req).await
    }

    async fn resume_run(
        &self,
        req: tonic::Request<proto::ResumeWorkflowRunRequest>,
    ) -> std::result::Result<tonic::Response<proto::WorkflowRunResponse>, tonic::Status> {
        self.handle_resume_workflow_run(req).await
    }

    async fn cancel_run(
        &self,
        req: tonic::Request<proto::CancelWorkflowRunRequest>,
    ) -> std::result::Result<tonic::Response<proto::WorkflowRunResponse>, tonic::Status> {
        self.handle_cancel_workflow_run(req).await
    }

    async fn stream_events(
        &self,
        req: tonic::Request<proto::StreamWorkflowEventsRequest>,
    ) -> std::result::Result<tonic::Response<Self::StreamEventsStream>, tonic::Status> {
        self.handle_stream_workflow_events(req).await
    }
}

#[tonic::async_trait]
impl proto::knowledge_service_server::KnowledgeService for GrpcGatewayHandler {
    async fn get(
        &self,
        req: tonic::Request<proto::GetKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::KnowledgeResponse>, tonic::Status> {
        self.handle_get_knowledge(req).await
    }

    async fn search(
        &self,
        req: tonic::Request<proto::SearchKnowledgeRequest>,
    ) -> std::result::Result<tonic::Response<proto::SearchKnowledgeResponse>, tonic::Status> {
        self.handle_search_knowledge(req).await
    }
}

#[tonic::async_trait]
impl proto::search_service_server::SearchService for GrpcGatewayHandler {
    async fn search(
        &self,
        req: tonic::Request<proto::SearchRequest>,
    ) -> std::result::Result<tonic::Response<proto::SearchResponse>, tonic::Status> {
        self.handle_search(req).await
    }

    async fn get_result(
        &self,
        req: tonic::Request<proto::GetSearchResultRequest>,
    ) -> std::result::Result<tonic::Response<proto::GetSearchResultResponse>, tonic::Status> {
        self.handle_get_search_result(req).await
    }
}

#[tonic::async_trait]
impl proto::namespace_service_server::NamespaceService for GrpcGatewayHandler {
    async fn create(
        &self,
        req: tonic::Request<proto::CreateNamespaceRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceResponse>, tonic::Status> {
        self.handle_create_namespace(req).await
    }

    async fn get(
        &self,
        req: tonic::Request<proto::GetNamespaceRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceResponse>, tonic::Status> {
        self.handle_get_namespace(req).await
    }

    async fn delete(
        &self,
        req: tonic::Request<proto::DeleteNamespaceRequest>,
    ) -> std::result::Result<tonic::Response<proto::NamespaceResponse>, tonic::Status> {
        self.handle_delete_namespace(req).await
    }

    async fn list(
        &self,
        req: tonic::Request<proto::ListNamespacesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListNamespacesResponse>, tonic::Status> {
        self.handle_list_namespaces(req).await
    }
}

#[tonic::async_trait]
impl proto::channel_service_server::ChannelService for GrpcGatewayHandler {
    type StreamEventsStream = ChannelEventStream;

    async fn post_message(
        &self,
        req: tonic::Request<proto::PostChannelMessageRequest>,
    ) -> std::result::Result<tonic::Response<proto::PostChannelMessageResponse>, tonic::Status>
    {
        self.handle_post_channel_message(req).await
    }

    async fn get_message(
        &self,
        req: tonic::Request<proto::GetChannelMessageRequest>,
    ) -> std::result::Result<tonic::Response<proto::ChannelMessageResponse>, tonic::Status> {
        self.handle_get_channel_message(req).await
    }

    async fn list_messages(
        &self,
        req: tonic::Request<proto::ListChannelMessagesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListChannelMessagesResponse>, tonic::Status>
    {
        self.handle_list_channel_messages(req).await
    }

    async fn stream_events(
        &self,
        req: tonic::Request<proto::StreamChannelEventsRequest>,
    ) -> std::result::Result<tonic::Response<Self::StreamEventsStream>, tonic::Status> {
        self.handle_stream_channel_events(req).await
    }
}

#[tonic::async_trait]
impl proto::auth_service_server::AuthService for GrpcGatewayHandler {
    async fn get_sso_config(
        &self,
        req: tonic::Request<proto::GetSsoConfigRequest>,
    ) -> std::result::Result<tonic::Response<proto::GetSsoConfigResponse>, tonic::Status> {
        self.handle_get_sso_config(req).await
    }

    async fn exchange_oidc_token(
        &self,
        req: tonic::Request<proto::ExchangeOidcTokenRequest>,
    ) -> std::result::Result<tonic::Response<proto::ExchangeOidcTokenResponse>, tonic::Status> {
        self.handle_exchange_oidc_token(req).await
    }

    async fn mint_access_token(
        &self,
        req: tonic::Request<proto::MintAccessTokenRequest>,
    ) -> std::result::Result<tonic::Response<proto::MintAccessTokenResponse>, tonic::Status> {
        self.handle_mint_access_token(req).await
    }

    async fn create_api_key(
        &self,
        req: tonic::Request<proto::CreateApiKeyRequest>,
    ) -> std::result::Result<tonic::Response<proto::CreateApiKeyResponse>, tonic::Status> {
        self.handle_create_api_key(req).await
    }

    async fn list_api_keys(
        &self,
        req: tonic::Request<proto::ListApiKeysRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListApiKeysResponse>, tonic::Status> {
        self.handle_list_api_keys(req).await
    }

    async fn revoke_api_key(
        &self,
        req: tonic::Request<proto::RevokeApiKeyRequest>,
    ) -> std::result::Result<tonic::Response<proto::RevokeApiKeyResponse>, tonic::Status> {
        self.handle_revoke_api_key(req).await
    }

    async fn exchange_api_key(
        &self,
        req: tonic::Request<proto::ExchangeApiKeyRequest>,
    ) -> std::result::Result<tonic::Response<proto::ExchangeApiKeyResponse>, tonic::Status> {
        self.handle_exchange_api_key(req).await
    }
}
