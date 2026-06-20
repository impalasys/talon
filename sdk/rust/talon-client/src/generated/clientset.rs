// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::v1::{
    namespace_service_client::NamespaceServiceClient,
    resource_service_client::ResourceServiceClient,
    session_service_client::SessionServiceClient,
    channel_service_client::ChannelServiceClient,
    workflow_service_client::WorkflowServiceClient,
    knowledge_service_client::KnowledgeServiceClient,
    search_service_client::SearchServiceClient,
    auth_service_client::AuthServiceClient,
};

#[derive(Debug)]
pub struct TalonClientset<T> {
    pub namespaces: NamespaceServiceClient<T>,
    pub resources: ResourceServiceClient<T>,
    pub sessions: SessionServiceClient<T>,
    pub channels: ChannelServiceClient<T>,
    pub workflows: WorkflowServiceClient<T>,
    pub knowledge: KnowledgeServiceClient<T>,
    pub searches: SearchServiceClient<T>,
    pub auth: AuthServiceClient<T>,
}

impl<T> TalonClientset<T>
where
    T: tonic::client::GrpcService<tonic::body::BoxBody> + Clone,
    T::Error: Into<tonic::codegen::StdError>,
    T::ResponseBody: tonic::codegen::Body<Data = tonic::codegen::Bytes> + Send + 'static,
    <T::ResponseBody as tonic::codegen::Body>::Error: Into<tonic::codegen::StdError> + Send,
{
    pub fn from_service(service: T) -> Self {
        Self {
            namespaces: NamespaceServiceClient::new(service.clone()),
            resources: ResourceServiceClient::new(service.clone()),
            sessions: SessionServiceClient::new(service.clone()),
            channels: ChannelServiceClient::new(service.clone()),
            workflows: WorkflowServiceClient::new(service.clone()),
            knowledge: KnowledgeServiceClient::new(service.clone()),
            searches: SearchServiceClient::new(service.clone()),
            auth: AuthServiceClient::new(service.clone()),
        }
    }
}

macro_rules! delegate_dynamic_unary_rpc {
    ($name:ident, $field:ident, $method:ident, $request:ty, $response:ty $(,)?) => {
        pub async fn $name(
            &mut self,
            request: $request,
        ) -> Result<tonic::Response<$response>, tonic::Status> {
            match self {
                crate::TalonClient::Native(client) => client.$field.$method(request).await,
                crate::TalonClient::GrpcWeb(client) => client.$field.$method(request).await,
            }
        }
    };
}

macro_rules! delegate_dynamic_server_streaming_rpc {
    ($name:ident, $field:ident, $method:ident, $request:ty, $response:ty $(,)?) => {
        pub async fn $name(
            &mut self,
            request: $request,
        ) -> Result<tonic::Response<tonic::codec::Streaming<$response>>, tonic::Status> {
            match self {
                crate::TalonClient::Native(client) => client.$field.$method(request).await,
                crate::TalonClient::GrpcWeb(client) => client.$field.$method(request).await,
            }
        }
    };
}

impl crate::TalonClient {
    delegate_dynamic_unary_rpc!(
        create_namespace,
        namespaces,
        create,
        crate::v1::CreateNamespaceRequest,
        crate::v1::NamespaceResponse,
    );
    delegate_dynamic_unary_rpc!(
        get_namespace,
        namespaces,
        get,
        crate::v1::GetNamespaceRequest,
        crate::v1::NamespaceResponse,
    );
    delegate_dynamic_unary_rpc!(
        delete_namespace,
        namespaces,
        delete,
        crate::v1::DeleteNamespaceRequest,
        crate::v1::NamespaceResponse,
    );
    delegate_dynamic_unary_rpc!(
        list_namespaces,
        namespaces,
        list,
        crate::v1::ListNamespacesRequest,
        crate::v1::ListNamespacesResponse,
    );
    delegate_dynamic_unary_rpc!(
        create_resource,
        resources,
        create,
        crate::v1::CreateResourceRequest,
        crate::v1::ResourceResponse,
    );
    delegate_dynamic_unary_rpc!(
        get_resource,
        resources,
        get,
        crate::v1::GetResourceRequest,
        crate::v1::ResourceResponse,
    );
    delegate_dynamic_unary_rpc!(
        list_resources,
        resources,
        list,
        crate::v1::ListResourcesRequest,
        crate::v1::ListResourcesResponse,
    );
    delegate_dynamic_unary_rpc!(
        delete_resource,
        resources,
        delete,
        crate::v1::DeleteResourceRequest,
        crate::v1::DeleteResourceResponse,
    );
    delegate_dynamic_unary_rpc!(
        create_session,
        sessions,
        create,
        crate::v1::CreateSessionRequest,
        crate::v1::SessionResponse,
    );
    delegate_dynamic_unary_rpc!(
        get_session,
        sessions,
        get,
        crate::v1::GetSessionRequest,
        crate::v1::SessionResponse,
    );
    delegate_dynamic_unary_rpc!(
        list_sessions,
        sessions,
        list,
        crate::v1::ListSessionsRequest,
        crate::v1::ListSessionsResponse,
    );
    delegate_dynamic_unary_rpc!(
        list_session_messages,
        sessions,
        list_messages,
        crate::v1::ListSessionMessagesRequest,
        crate::v1::ListSessionMessagesResponse,
    );
    delegate_dynamic_unary_rpc!(
        delete_session,
        sessions,
        delete,
        crate::v1::DeleteSessionRequest,
        crate::v1::DeleteSessionResponse,
    );
    delegate_dynamic_unary_rpc!(
        clear_session,
        sessions,
        clear,
        crate::v1::ClearSessionRequest,
        crate::v1::ClearSessionResponse,
    );
    delegate_dynamic_unary_rpc!(
        send_message,
        sessions,
        send_message,
        crate::v1::SendMessageRequest,
        crate::v1::SendMessageResponse,
    );
    delegate_dynamic_unary_rpc!(
        append_session_message,
        sessions,
        append_message,
        crate::v1::AppendSessionMessageRequest,
        crate::v1::AppendSessionMessageResponse,
    );
    delegate_dynamic_unary_rpc!(
        answer_session_permission,
        sessions,
        answer_permission,
        crate::v1::AnswerSessionPermissionRequest,
        crate::v1::AnswerSessionPermissionResponse,
    );
    delegate_dynamic_unary_rpc!(
        stop_session_generation,
        sessions,
        stop_generation,
        crate::v1::StopSessionGenerationRequest,
        crate::v1::StopSessionGenerationResponse,
    );
    delegate_dynamic_server_streaming_rpc!(
        stream_session_parts,
        sessions,
        stream_parts,
        crate::v1::StreamSessionPartsRequest,
        crate::events::SessionMessagePartEvent,
    );
    delegate_dynamic_server_streaming_rpc!(
        stream_session_parts_batch,
        sessions,
        stream_parts_batch,
        crate::v1::StreamSessionPartsBatchRequest,
        crate::events::SessionMessagePartEvent,
    );
    delegate_dynamic_server_streaming_rpc!(
        submit_session_turn,
        sessions,
        submit_turn,
        crate::v1::SubmitSessionTurnRequest,
        crate::events::SessionMessagePartEvent,
    );
    delegate_dynamic_unary_rpc!(
        post_channel_message,
        channels,
        post_message,
        crate::v1::PostChannelMessageRequest,
        crate::v1::PostChannelMessageResponse,
    );
    delegate_dynamic_unary_rpc!(
        get_channel_message,
        channels,
        get_message,
        crate::v1::GetChannelMessageRequest,
        crate::v1::ChannelMessageResponse,
    );
    delegate_dynamic_unary_rpc!(
        list_channel_messages,
        channels,
        list_messages,
        crate::v1::ListChannelMessagesRequest,
        crate::v1::ListChannelMessagesResponse,
    );
    delegate_dynamic_server_streaming_rpc!(
        stream_channel_events,
        channels,
        stream_events,
        crate::v1::StreamChannelEventsRequest,
        crate::events::ChannelEvent,
    );
    delegate_dynamic_unary_rpc!(
        create_workflow_run,
        workflows,
        create_run,
        crate::v1::CreateWorkflowRunRequest,
        crate::v1::WorkflowRunResponse,
    );
    delegate_dynamic_unary_rpc!(
        get_workflow_run,
        workflows,
        get_run,
        crate::v1::GetWorkflowRunRequest,
        crate::v1::WorkflowRunResponse,
    );
    delegate_dynamic_unary_rpc!(
        list_workflow_runs,
        workflows,
        list_runs,
        crate::v1::ListWorkflowRunsRequest,
        crate::v1::ListWorkflowRunsResponse,
    );
    delegate_dynamic_unary_rpc!(
        resume_workflow_run,
        workflows,
        resume_run,
        crate::v1::ResumeWorkflowRunRequest,
        crate::v1::WorkflowRunResponse,
    );
    delegate_dynamic_unary_rpc!(
        cancel_workflow_run,
        workflows,
        cancel_run,
        crate::v1::CancelWorkflowRunRequest,
        crate::v1::WorkflowRunResponse,
    );
    delegate_dynamic_server_streaming_rpc!(
        stream_workflow_events,
        workflows,
        stream_events,
        crate::v1::StreamWorkflowEventsRequest,
        crate::data::WorkflowRunEvent,
    );
    delegate_dynamic_unary_rpc!(
        get_knowledge,
        knowledge,
        get,
        crate::v1::GetKnowledgeRequest,
        crate::v1::KnowledgeResponse,
    );
    delegate_dynamic_unary_rpc!(
        search_knowledge,
        knowledge,
        search,
        crate::v1::SearchKnowledgeRequest,
        crate::v1::SearchKnowledgeResponse,
    );
    delegate_dynamic_unary_rpc!(
        search,
        searches,
        search,
        crate::v1::SearchRequest,
        crate::v1::SearchResponse,
    );
    delegate_dynamic_unary_rpc!(
        get_search_result,
        searches,
        get_result,
        crate::v1::GetSearchResultRequest,
        crate::v1::GetSearchResultResponse,
    );
    delegate_dynamic_unary_rpc!(
        get_sso_config,
        auth,
        get_sso_config,
        crate::v1::GetSsoConfigRequest,
        crate::v1::GetSsoConfigResponse,
    );
    delegate_dynamic_unary_rpc!(
        exchange_oidc_token,
        auth,
        exchange_oidc_token,
        crate::v1::ExchangeOidcTokenRequest,
        crate::v1::ExchangeOidcTokenResponse,
    );
}
