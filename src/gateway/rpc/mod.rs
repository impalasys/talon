// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::gateway::server::Gateway;
use std::sync::Arc;

use std::pin::Pin;

pub mod channels;
pub mod knowledge;
pub mod namespaces;
pub mod resources;
pub mod sessions;
pub mod workflows;

#[cfg(not(feature = "bazel"))]
pub mod generated {
    pub mod data {
        tonic::include_proto!("talon.data");
    }
    pub mod events {
        pub use crate::control::events::*;
    }
    pub mod resources {
        tonic::include_proto!("talon.resources");
    }
    pub mod proto {
        tonic::include_proto!("talon.gateway");
    }
}

#[cfg(feature = "bazel")]
pub mod generated {
    pub mod data {
        pub use talon_data_proto::talon::data::*;
    }
    pub mod resources {
        pub use talon_resources_proto::talon::resources::*;
    }
    pub mod proto {
        pub use talon_gateway_proto::talon::gateway::*;
    }
}

pub mod proto {
    pub use super::generated::proto::*;
}

pub mod data_proto {
    pub use super::generated::data::*;
}

pub mod resources_proto {
    pub use super::generated::resources::*;
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
        dyn futures::Stream<Item = std::result::Result<data_proto::WorkflowRunEvent, tonic::Status>>
            + Send,
    >,
>;

#[tonic::async_trait]
impl proto::gateway_service_server::GatewayService for GrpcGatewayHandler {
    async fn create_resource(
        &self,
        req: tonic::Request<proto::CreateResourceRequest>,
    ) -> std::result::Result<tonic::Response<proto::ResourceResponse>, tonic::Status> {
        self.handle_create_resource(req).await
    }

    async fn get_resource(
        &self,
        req: tonic::Request<proto::GetResourceRequest>,
    ) -> std::result::Result<tonic::Response<proto::ResourceResponse>, tonic::Status> {
        self.handle_get_resource(req).await
    }

    async fn list_resources(
        &self,
        req: tonic::Request<proto::ListResourcesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListResourcesResponse>, tonic::Status> {
        self.handle_list_resources(req).await
    }

    async fn delete_resource(
        &self,
        req: tonic::Request<proto::DeleteResourceRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteResourceResponse>, tonic::Status> {
        self.handle_delete_resource(req).await
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
    async fn answer_session_permission(
        &self,
        req: tonic::Request<proto::AnswerSessionPermissionRequest>,
    ) -> std::result::Result<tonic::Response<proto::AnswerSessionPermissionResponse>, tonic::Status>
    {
        self.handle_answer_session_permission(req).await
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
