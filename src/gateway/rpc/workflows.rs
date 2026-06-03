// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{models, proto, GrpcGatewayHandler};
use crate::control::{keys, topics, ProtoKeyValueStoreExt};
use crate::workflows;
use futures::StreamExt;
use prost::Message;

impl GrpcGatewayHandler {
    pub async fn handle_create_workflow(
        &self,
        req: tonic::Request<proto::CreateWorkflowRequest>,
    ) -> Result<tonic::Response<proto::WorkflowResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let mut workflow = req
            .workflow
            .ok_or_else(|| tonic::Status::invalid_argument("workflow is required"))?;
        workflow.ns = req.ns.clone();
        workflows::validate_workflow(&workflow)
            .map_err(|err| tonic::Status::invalid_argument(err.to_string()))?;
        let key = keys::workflow(&req.ns, &workflow.name);
        if self
            .gateway
            .kv
            .get_msg::<models::Workflow>(&key)
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?
            .is_some()
        {
            return Err(tonic::Status::already_exists("workflow already exists"));
        }
        self.gateway
            .kv
            .set_msg(&key, &workflow)
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?;
        Ok(tonic::Response::new(proto::WorkflowResponse {
            workflow: Some(workflow),
        }))
    }

    pub async fn handle_get_workflow(
        &self,
        req: tonic::Request<proto::GetWorkflowRequest>,
    ) -> Result<tonic::Response<proto::WorkflowResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let workflow = self
            .gateway
            .kv
            .get_msg::<models::Workflow>(&keys::workflow(&req.ns, &req.name))
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?
            .ok_or_else(|| tonic::Status::not_found("workflow not found"))?;
        Ok(tonic::Response::new(proto::WorkflowResponse {
            workflow: Some(workflow),
        }))
    }

    pub async fn handle_list_workflows(
        &self,
        req: tonic::Request<proto::ListWorkflowsRequest>,
    ) -> Result<tonic::Response<proto::ListWorkflowsResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let mut entries = self
            .gateway
            .kv
            .list_entries(&keys::workflow_prefix(&req.ns))
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?;
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        let mut workflows = Vec::new();
        for (_, bytes) in entries {
            workflows.push(
                models::Workflow::decode(bytes.as_slice())
                    .map_err(|err| tonic::Status::internal(err.to_string()))?,
            );
        }
        Ok(tonic::Response::new(proto::ListWorkflowsResponse { workflows }))
    }

    pub async fn handle_delete_workflow(
        &self,
        req: tonic::Request<proto::DeleteWorkflowRequest>,
    ) -> Result<tonic::Response<proto::DeleteWorkflowResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        self.gateway
            .kv
            .delete(&keys::workflow(&req.ns, &req.name))
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?;
        Ok(tonic::Response::new(proto::DeleteWorkflowResponse {
            success: true,
        }))
    }

    pub async fn handle_create_workflow_run(
        &self,
        req: tonic::Request<proto::CreateWorkflowRunRequest>,
    ) -> Result<tonic::Response<proto::WorkflowRunResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let workflow = self
            .gateway
            .kv
            .get_msg::<models::Workflow>(&keys::workflow(&req.ns, &req.workflow))
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?
            .ok_or_else(|| tonic::Status::not_found("workflow not found"))?;
        let run =
            workflows::create_run(&self.gateway.control_plane(), &workflow, req.input_json, req.labels)
                .await
                .map_err(|err| tonic::Status::invalid_argument(err.to_string()))?;
        Ok(tonic::Response::new(proto::WorkflowRunResponse {
            run: Some(run),
            steps: Vec::new(),
        }))
    }

    pub async fn handle_get_workflow_run(
        &self,
        req: tonic::Request<proto::GetWorkflowRunRequest>,
    ) -> Result<tonic::Response<proto::WorkflowRunResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let run = self
            .gateway
            .kv
            .get_msg::<models::WorkflowRun>(&keys::workflow_run(
                &req.ns,
                &req.workflow,
                &req.run_id,
            ))
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?
            .ok_or_else(|| tonic::Status::not_found("workflow run not found"))?;
        let mut steps = workflows::load_step_runs(self.gateway.kv.as_ref(), &run)
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?
            .into_values()
            .collect::<Vec<_>>();
        steps.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(tonic::Response::new(proto::WorkflowRunResponse {
            run: Some(run),
            steps,
        }))
    }

    pub async fn handle_list_workflow_runs(
        &self,
        req: tonic::Request<proto::ListWorkflowRunsRequest>,
    ) -> Result<tonic::Response<proto::ListWorkflowRunsResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let mut entries = self
            .gateway
            .kv
            .list_entries(&keys::workflow_run_prefix(&req.ns, &req.workflow))
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?;
        entries.sort_by(|left, right| right.0.cmp(&left.0));
        let mut runs = Vec::new();
        for (_, bytes) in entries {
            runs.push(
                models::WorkflowRun::decode(bytes.as_slice())
                    .map_err(|err| tonic::Status::internal(err.to_string()))?,
            );
        }
        Ok(tonic::Response::new(proto::ListWorkflowRunsResponse { runs }))
    }

    pub async fn handle_resume_workflow_run(
        &self,
        req: tonic::Request<proto::ResumeWorkflowRunRequest>,
    ) -> Result<tonic::Response<proto::WorkflowRunResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let run = workflows::resume_run(
            &self.gateway.control_plane(),
            &req.ns,
            &req.workflow,
            &req.run_id,
            &req.step_id,
            &req.resume_json,
        )
        .await
        .map_err(|err| tonic::Status::invalid_argument(err.to_string()))?;
        Ok(tonic::Response::new(proto::WorkflowRunResponse {
            run: Some(run),
            steps: Vec::new(),
        }))
    }

    pub async fn handle_cancel_workflow_run(
        &self,
        req: tonic::Request<proto::CancelWorkflowRunRequest>,
    ) -> Result<tonic::Response<proto::WorkflowRunResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let run = workflows::cancel_run(
            &self.gateway.control_plane(),
            &req.ns,
            &req.workflow,
            &req.run_id,
        )
        .await
        .map_err(|err| tonic::Status::invalid_argument(err.to_string()))?;
        Ok(tonic::Response::new(proto::WorkflowRunResponse {
            run: Some(run),
            steps: Vec::new(),
        }))
    }

    pub async fn handle_stream_workflow_events(
        &self,
        req: tonic::Request<proto::StreamWorkflowEventsRequest>,
    ) -> Result<
        tonic::Response<
            <GrpcGatewayHandler as proto::gateway_service_server::GatewayService>::StreamWorkflowEventsStream,
        >,
        tonic::Status,
    > {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let stream = self
            .gateway
            .pubsub
            .subscribe(&topics::workflow_events_topic(
                &req.ns,
                &req.workflow,
                &req.run_id,
            ))
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?;
        let event_stream = stream.filter_map(|bytes| async move {
            match models::WorkflowRunEvent::decode(bytes.as_slice()) {
                Ok(event) => Some(Ok(event)),
                Err(err) => Some(Err(tonic::Status::internal(err.to_string()))),
            }
        });
        Ok(tonic::Response::new(Box::pin(event_stream)))
    }
}

trait GatewayControlPlaneExt {
    fn control_plane(&self) -> crate::control::ControlPlane;
}

impl GatewayControlPlaneExt for crate::gateway::server::Gateway {
    fn control_plane(&self) -> crate::control::ControlPlane {
        crate::control::ControlPlane {
            kv: self.kv.clone(),
            pubsub: self.pubsub.clone(),
            scheduler: self.scheduler.clone(),
        }
    }
}
