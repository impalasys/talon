// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{data_proto, proto, resources_proto, worker_proto, GrpcGatewayHandler};
use crate::control::resources::ResourceStore;
use crate::control::{keys, KeyValueStore, Order, ProtoKeyValueStoreExt};
use crate::gateway::worker_conn::WorkerConnectionPool;
use crate::worker::workflows;
use futures::StreamExt;
use prost::Message;
use std::collections::HashSet;
use std::time::Duration;

const DEFAULT_WORKFLOW_RUNS_PAGE_SIZE: usize = 50;
const MAX_WORKFLOW_RUNS_PAGE_SIZE: usize = 200;
const WORKFLOW_RUN_KEY_SCAN_BATCH_SIZE: usize = 512;
const MAX_WORKFLOW_RUN_LIST_PAGES_SCANNED: usize = 10;
const WORKFLOW_FANOUT_ATTACH_RETRY: Duration = Duration::from_millis(250);
const WORKFLOW_TERMINAL_EVENT_CATCH_UP_ATTEMPTS: usize = 5;
const WORKFLOW_TERMINAL_EVENT_CATCH_UP_DELAY: Duration = Duration::from_millis(100);

fn workflow_from_resource(
    resource: resources_proto::Resource,
) -> Option<resources_proto::Workflow> {
    let spec = resource.spec.and_then(|spec| match spec.kind {
        Some(resources_proto::resource_spec::Kind::Workflow(spec)) => Some(spec),
        _ => None,
    })?;
    let status = resource.status.and_then(|status| match status.kind {
        Some(resources_proto::resource_status::Kind::Workflow(status)) => Some(status),
        _ => None,
    });
    Some(resources_proto::Workflow {
        metadata: resource.metadata,
        spec: Some(spec),
        status,
    })
}

impl GrpcGatewayHandler {
    pub async fn handle_create_workflow_run(
        &self,
        req: tonic::Request<proto::CreateWorkflowRunRequest>,
    ) -> Result<tonic::Response<proto::WorkflowRunResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        let resource = store
            .get(&req.ns, "Workflow", &req.workflow)
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?
            .ok_or_else(|| tonic::Status::not_found("workflow not found"))?;
        let workflow = workflow_from_resource(resource).ok_or_else(|| {
            tonic::Status::invalid_argument("Workflow resource missing workflow spec")
        })?;
        let input = if req.input_json.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&req.input_json)
                .map_err(|err| tonic::Status::invalid_argument(err.to_string()))?
        };
        let spec = workflow
            .spec
            .as_ref()
            .ok_or_else(|| tonic::Status::invalid_argument("workflow spec is required"))?;
        workflows::validate_input(&spec.input_schema_json, &input)
            .map_err(|err| tonic::Status::invalid_argument(err.to_string()))?;
        let run = workflows::create_run(
            &self.gateway.control_plane(),
            &workflow,
            serde_json::to_string(&input)
                .map_err(|err| tonic::Status::internal(err.to_string()))?,
            req.labels,
        )
        .await
        .map_err(|err| tonic::Status::internal(err.to_string()))?;
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
            .get_msg::<data_proto::WorkflowRun>(&keys::workflow_run(
                &req.ns,
                &req.workflow,
                &req.run_id,
            ))
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?
            .ok_or_else(|| tonic::Status::not_found("workflow run not found"))?;
        let steps = load_sorted_workflow_step_runs(self.gateway.kv.as_ref(), &run).await?;
        Ok(tonic::Response::new(proto::WorkflowRunResponse {
            run: Some(run),
            steps,
        }))
    }

    pub async fn handle_list_workflow_runs(
        &self,
        req: tonic::Request<proto::ListWorkflowRunsRequest>,
    ) -> Result<tonic::Response<proto::ListWorkflowRunsResponse>, tonic::Status> {
        crate::require_auth!(read, self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let page_size = validated_workflow_runs_page_size(req.page_size)?;
        let prefix = keys::workflow_run_prefix(&req.ns, &req.workflow);
        let target_run_count = page_size + 1;
        let mut scan_before_name = if req.before_run_id.is_empty() {
            None
        } else {
            Some(req.before_run_id.clone())
        };
        let mut runs = Vec::with_capacity(target_run_count);
        let mut pages_scanned = 0;
        let mut scan_limit_reached = false;

        while runs.len() < target_run_count {
            if pages_scanned >= MAX_WORKFLOW_RUN_LIST_PAGES_SCANNED {
                scan_limit_reached = true;
                break;
            }
            pages_scanned += 1;
            let entries = self
                .gateway
                .kv
                .list_entries_page(
                    &prefix,
                    scan_before_name.as_deref(),
                    WORKFLOW_RUN_KEY_SCAN_BATCH_SIZE,
                )
                .await
                .map_err(|err| tonic::Status::internal(err.to_string()))?;
            if entries.is_empty() {
                break;
            }
            scan_before_name = entries.last().map(|(key, _)| key.name.clone());

            for (key, bytes) in entries {
                if runs.len() >= target_run_count {
                    break;
                }
                match data_proto::WorkflowRun::decode(bytes.as_slice()) {
                    Ok(run) => runs.push(run),
                    Err(err) => {
                        tracing::warn!(
                            run_key = %key,
                            error = %err,
                            "failed to decode workflow run while listing; skipping entry"
                        );
                    }
                }
            }
        }

        let has_extra_run = runs.len() > page_size;
        if has_extra_run {
            runs.truncate(page_size);
        }
        let next_before_run_id = if has_extra_run {
            runs.last().map(|run| run.id.clone()).unwrap_or_default()
        } else if scan_limit_reached {
            scan_before_name.unwrap_or_default()
        } else {
            String::new()
        };
        let has_more = has_extra_run || (scan_limit_reached && !next_before_run_id.is_empty());

        Ok(tonic::Response::new(proto::ListWorkflowRunsResponse {
            runs,
            has_more,
            next_before_run_id,
        }))
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
        .map_err(workflow_run_mutation_status)?;
        let steps = load_sorted_workflow_step_runs(self.gateway.kv.as_ref(), &run).await?;
        Ok(tonic::Response::new(proto::WorkflowRunResponse {
            run: Some(run),
            steps,
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
        .map_err(workflow_run_mutation_status)?;
        let steps = load_sorted_workflow_step_runs(self.gateway.kv.as_ref(), &run).await?;
        Ok(tonic::Response::new(proto::WorkflowRunResponse {
            run: Some(run),
            steps,
        }))
    }

    pub async fn handle_stream_workflow_events(
        &self,
        req: tonic::Request<proto::StreamWorkflowEventsRequest>,
    ) -> Result<
        tonic::Response<
            <GrpcGatewayHandler as proto::workflow_service_server::WorkflowService>::StreamEventsStream,
        >,
        tonic::Status,
    >{
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let run = self
            .gateway
            .kv
            .get_msg::<data_proto::WorkflowRun>(&keys::workflow_run(
                &req.ns,
                &req.workflow,
                &req.run_id,
            ))
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?
            .ok_or_else(|| tonic::Status::not_found("workflow run not found"))?;
        let historical = load_sorted_workflow_run_events(
            self.gateway.kv.as_ref(),
            &req.ns,
            &req.workflow,
            &req.run_id,
        )
        .await?;

        let mut seen = HashSet::new();
        let mut terminal_seen = false;
        let mut historical_items = Vec::new();
        for event in historical {
            if !seen.insert(event.id.clone()) {
                continue;
            }
            if is_terminal_workflow_event(&event.r#type) {
                terminal_seen = true;
            }
            historical_items.push(Ok(event));
            if terminal_seen {
                break;
            }
        }

        let kv = self.gateway.kv.clone();
        let worker_connections = self.gateway.worker_connections.clone();
        let ns = req.ns;
        let workflow = req.workflow;
        let run_id = req.run_id;
        let event_stream = async_stream::stream! {
            let mut seen = seen;
            for item in historical_items {
                yield item;
            }
            if terminal_seen {
                return;
            }

            let mut current_run = run;
            loop {
                if is_terminal_workflow_status(&current_run.status) {
                    for _ in 0..WORKFLOW_TERMINAL_EVENT_CATCH_UP_ATTEMPTS {
                        match load_sorted_workflow_run_events(kv.as_ref(), &ns, &workflow, &run_id).await {
                            Ok(catch_up_events) => {
                                for event in catch_up_events {
                                    if !seen.insert(event.id.clone()) {
                                        continue;
                                    }
                                    let terminal = is_terminal_workflow_event(&event.r#type);
                                    yield Ok(event);
                                    if terminal {
                                        return;
                                    }
                                }
                            }
                            Err(status) => {
                                yield Err(status);
                                return;
                            }
                        }
                        tokio::time::sleep(WORKFLOW_TERMINAL_EVENT_CATCH_UP_DELAY).await;
                    }
                    return;
                }
                if current_run.claim_owner.trim().is_empty() {
                    tokio::time::sleep(WORKFLOW_FANOUT_ATTACH_RETRY).await;
                    match kv
                        .get_msg::<data_proto::WorkflowRun>(&keys::workflow_run(&ns, &workflow, &run_id))
                        .await
                    {
                        Ok(Some(updated)) => current_run = updated,
                        Ok(None) => {
                            yield Err(tonic::Status::not_found("workflow run not found"));
                            return;
                        }
                        Err(err) => {
                            yield Err(tonic::Status::internal(err.to_string()));
                            return;
                        }
                    }
                    continue;
                }

                match connect_workflow_event_stream(
                    kv.as_ref(),
                    worker_connections.as_ref(),
                    &current_run,
                )
                .await
                {
                    Ok(mut stream) => {
                        match load_sorted_workflow_run_events(kv.as_ref(), &ns, &workflow, &run_id).await {
                            Ok(catch_up_events) => {
                                for event in catch_up_events {
                                    if !seen.insert(event.id.clone()) {
                                        continue;
                                    }
                                    let terminal = is_terminal_workflow_event(&event.r#type);
                                    yield Ok(event);
                                    if terminal {
                                        return;
                                    }
                                }
                            }
                            Err(status) => {
                                yield Err(status);
                                return;
                            }
                        }
                        while let Some(item) = stream.next().await {
                            match item {
                                Ok(response) => {
                                    let Some(event) = response.event else {
                                        continue;
                                    };
                                    if !seen.insert(event.id.clone()) {
                                        continue;
                                    }
                                    let terminal = is_terminal_workflow_event(&event.r#type);
                                    yield Ok(event);
                                    if terminal {
                                        return;
                                    }
                                }
                                Err(status) => {
                                    if status.code() != tonic::Code::NotFound
                                        && status.code() != tonic::Code::Unavailable
                                    {
                                        yield Err(status);
                                        return;
                                    }
                                    break;
                                }
                            }
                        }
                    }
                    Err(status) => {
                        if status.code() != tonic::Code::NotFound
                            && status.code() != tonic::Code::Unavailable
                        {
                            yield Err(status);
                            return;
                        }
                    }
                }

                match load_sorted_workflow_run_events(kv.as_ref(), &ns, &workflow, &run_id).await {
                    Ok(catch_up_events) => {
                        for event in catch_up_events {
                            if !seen.insert(event.id.clone()) {
                                continue;
                            }
                            let terminal = is_terminal_workflow_event(&event.r#type);
                            yield Ok(event);
                            if terminal {
                                return;
                            }
                        }
                    }
                    Err(status) => {
                        yield Err(status);
                        return;
                    }
                }

                tokio::time::sleep(WORKFLOW_FANOUT_ATTACH_RETRY).await;
                match kv
                    .get_msg::<data_proto::WorkflowRun>(&keys::workflow_run(&ns, &workflow, &run_id))
                    .await
                {
                    Ok(Some(updated)) => current_run = updated,
                    Ok(None) => {
                        yield Err(tonic::Status::not_found("workflow run not found"));
                        return;
                    }
                    Err(err) => {
                        yield Err(tonic::Status::internal(err.to_string()));
                        return;
                    }
                }
            }
        };
        Ok(tonic::Response::new(Box::pin(event_stream)))
    }
}

async fn load_sorted_workflow_step_runs(
    kv: &dyn KeyValueStore,
    run: &data_proto::WorkflowRun,
) -> Result<Vec<data_proto::WorkflowStepRun>, tonic::Status> {
    let mut steps = workflows::load_step_runs(kv, run)
        .await
        .map_err(|err| tonic::Status::internal(err.to_string()))?
        .into_values()
        .collect::<Vec<_>>();
    steps.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(steps)
}

async fn load_sorted_workflow_run_events(
    kv: &dyn KeyValueStore,
    ns: &str,
    workflow: &str,
    run_id: &str,
) -> Result<Vec<data_proto::WorkflowRunEvent>, tonic::Status> {
    let entries = kv
        .list_entries(
            &keys::workflow_run_event_prefix(ns, workflow, run_id),
            Order::Asc.into(),
        )
        .await
        .map_err(|err| tonic::Status::internal(err.to_string()))?;

    let mut events = Vec::new();
    for (key, bytes) in entries {
        match data_proto::WorkflowRunEvent::decode(bytes.as_slice()) {
            Ok(event) => events.push(event),
            Err(err) => {
                tracing::warn!(
                    event_key = %key,
                    error = %err,
                    "failed to decode workflow run event while listing history; skipping entry"
                );
            }
        }
    }
    events.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(events)
}

async fn connect_workflow_event_stream(
    kv: &dyn KeyValueStore,
    worker_connections: &WorkerConnectionPool,
    run: &data_proto::WorkflowRun,
) -> Result<tonic::Streaming<worker_proto::StreamWorkflowEventsResponse>, tonic::Status> {
    let endpoints = WorkerConnectionPool::worker_endpoints(kv, &run.claim_owner).await?;
    let mut last_status = None;
    for endpoint in endpoints {
        match stream_workflow_events_from_endpoint(worker_connections, &endpoint, run).await {
            Ok(stream) => return Ok(stream),
            Err(status) => last_status = Some(status),
        }
    }
    Err(last_status.unwrap_or_else(|| tonic::Status::unavailable("worker has no endpoints")))
}

async fn stream_workflow_events_from_endpoint(
    worker_connections: &WorkerConnectionPool,
    endpoint: &resources_proto::WorkerEndpoint,
    run: &data_proto::WorkflowRun,
) -> Result<tonic::Streaming<worker_proto::StreamWorkflowEventsResponse>, tonic::Status> {
    let mut client = worker_connections.fanout_client(endpoint).await?;
    let response = client
        .stream_workflow_events(worker_proto::StreamWorkflowEventsRequest {
            ns: run.ns.clone(),
            workflow: run.workflow.clone(),
            run_id: run.id.clone(),
            after_sequence: 0,
        })
        .await?;
    Ok(response.into_inner())
}

fn workflow_run_mutation_status(err: anyhow::Error) -> tonic::Status {
    let message = err.to_string();
    if err
        .downcast_ref::<workflows::WorkflowNotFoundError>()
        .is_some()
    {
        tonic::Status::not_found(message)
    } else if err
        .downcast_ref::<workflows::WorkflowInvalidArgumentError>()
        .is_some()
    {
        tonic::Status::invalid_argument(message)
    } else {
        tonic::Status::internal(message)
    }
}

fn validated_workflow_runs_page_size(page_size: i32) -> Result<usize, tonic::Status> {
    if page_size < 0 {
        return Err(tonic::Status::invalid_argument(
            "page_size must be non-negative",
        ));
    }
    let page_size = if page_size == 0 {
        DEFAULT_WORKFLOW_RUNS_PAGE_SIZE
    } else {
        page_size as usize
    };
    Ok(page_size.min(MAX_WORKFLOW_RUNS_PAGE_SIZE))
}

fn is_terminal_workflow_event(event_type: &str) -> bool {
    matches!(event_type, "run_completed" | "run_failed" | "run_cancelled")
}

fn is_terminal_workflow_status(status: &str) -> bool {
    matches!(status, "COMPLETED" | "FAILED" | "CANCELLED")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_run_mutation_status_maps_unknown_errors_to_internal() {
        let status = workflow_run_mutation_status(anyhow::anyhow!("kv unavailable"));
        assert_eq!(status.code(), tonic::Code::Internal);
    }
}
