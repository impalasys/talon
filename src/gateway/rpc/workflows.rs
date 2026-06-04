// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{models, proto, GrpcGatewayHandler};
use crate::control::{keys, topics, KeyValueStore, ProtoKeyValueStoreExt};
use crate::workflows;
use futures::{stream, StreamExt};
use prost::Message;
use rand::Rng;
use std::collections::HashSet;

const MAX_WORKFLOW_UPSERT_RETRIES: usize = 8;
const WORKFLOW_UPSERT_RETRY_BACKOFF_MS: u64 = 10;
const MAX_WORKFLOW_UPSERT_RETRY_BACKOFF_MS: u64 = 500;
const DEFAULT_WORKFLOW_RUNS_PAGE_SIZE: usize = 50;
const MAX_WORKFLOW_RUNS_PAGE_SIZE: usize = 200;
const WORKFLOW_RUN_KEY_SCAN_BATCH_SIZE: usize = 512;
const MAX_WORKFLOW_RUN_LIST_PAGES_SCANNED: usize = 10;

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
        let payload = workflow.encode_to_vec();
        for attempt in 0..MAX_WORKFLOW_UPSERT_RETRIES {
            let current = self
                .gateway
                .kv
                .get(&key)
                .await
                .map_err(|err| tonic::Status::internal(err.to_string()))?;
            let updated = self
                .gateway
                .kv
                .compare_and_swap(&key, current.as_deref(), &payload)
                .await
                .map_err(|err| tonic::Status::internal(err.to_string()))?;
            if updated {
                return Ok(tonic::Response::new(proto::WorkflowResponse {
                    workflow: Some(workflow),
                }));
            }
            if attempt + 1 < MAX_WORKFLOW_UPSERT_RETRIES {
                tokio::time::sleep(std::time::Duration::from_millis(
                    workflow_upsert_retry_backoff_ms(attempt),
                ))
                .await;
            }
        }
        Err(tonic::Status::internal(
            "failed to upsert workflow after concurrent modifications",
        ))
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
        for (key, bytes) in entries {
            match models::Workflow::decode(bytes.as_slice()) {
                Ok(workflow) => workflows.push(workflow),
                Err(err) => {
                    tracing::warn!(
                        workflow_key = %key,
                        error = %err,
                        "failed to decode workflow while listing; skipping entry"
                    );
                }
            }
        }
        Ok(tonic::Response::new(proto::ListWorkflowsResponse {
            workflows,
        }))
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
            .get_msg::<models::WorkflowRun>(&keys::workflow_run(
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
        crate::require_auth!(self, req, &req.get_ref().ns);
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
                match models::WorkflowRun::decode(bytes.as_slice()) {
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
            <GrpcGatewayHandler as proto::gateway_service_server::GatewayService>::StreamWorkflowEventsStream,
        >,
        tonic::Status,
    >{
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        self.gateway
            .kv
            .get_msg::<models::WorkflowRun>(&keys::workflow_run(
                &req.ns,
                &req.workflow,
                &req.run_id,
            ))
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?
            .ok_or_else(|| tonic::Status::not_found("workflow run not found"))?;
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
        let mut entries = self
            .gateway
            .kv
            .list_entries(&keys::workflow_run_event_prefix(
                &req.ns,
                &req.workflow,
                &req.run_id,
            ))
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?;
        entries.sort_by(|left, right| left.0.cmp(&right.0));

        let mut historical = Vec::new();
        for (key, bytes) in entries {
            match models::WorkflowRunEvent::decode(bytes.as_slice()) {
                Ok(event) => historical.push(event),
                Err(err) => {
                    tracing::warn!(
                        event_key = %key,
                        error = %err,
                        "failed to decode workflow run event while listing history; skipping entry"
                    );
                }
            }
        }
        historical.sort_by(|left, right| {
            left.timestamp
                .cmp(&right.timestamp)
                .then_with(|| left.id.cmp(&right.id))
        });

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

        let historical_stream = stream::iter(historical_items);
        if terminal_seen {
            return Ok(tonic::Response::new(Box::pin(historical_stream)));
        }

        let live_stream = stream
            .scan((seen, false), |(seen, terminated), bytes| {
                if *terminated {
                    return futures::future::ready(None);
                }
                let item = match models::WorkflowRunEvent::decode(bytes.as_slice()) {
                    Ok(event) => {
                        if !seen.insert(event.id.clone()) {
                            return futures::future::ready(Some(None));
                        }
                        if is_terminal_workflow_event(&event.r#type) {
                            *terminated = true;
                        }
                        Some(Ok(event))
                    }
                    Err(err) => {
                        tracing::warn!(
                            error = %err,
                            "failed to decode workflow run event while streaming; skipping entry"
                        );
                        None
                    }
                };
                futures::future::ready(Some(item))
            })
            .filter_map(|event| async move { event });
        let event_stream = historical_stream.chain(live_stream);
        Ok(tonic::Response::new(Box::pin(event_stream)))
    }
}

async fn load_sorted_workflow_step_runs(
    kv: &dyn KeyValueStore,
    run: &models::WorkflowRun,
) -> Result<Vec<models::WorkflowStepRun>, tonic::Status> {
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

fn workflow_run_mutation_status(err: anyhow::Error) -> tonic::Status {
    let message = err.to_string();
    if err
        .downcast_ref::<workflows::WorkflowNotFoundError>()
        .is_some()
    {
        tonic::Status::not_found(message)
    } else {
        tonic::Status::invalid_argument(message)
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

fn workflow_upsert_retry_backoff_ms(attempt: usize) -> u64 {
    let shift = attempt.min(5) as u32;
    let exponential = WORKFLOW_UPSERT_RETRY_BACKOFF_MS
        .saturating_mul(1_u64 << shift)
        .min(MAX_WORKFLOW_UPSERT_RETRY_BACKOFF_MS);
    let jitter = rand::thread_rng().gen_range(0..=(exponential / 2));
    (exponential + jitter).min(MAX_WORKFLOW_UPSERT_RETRY_BACKOFF_MS)
}

fn is_terminal_workflow_event(event_type: &str) -> bool {
    matches!(event_type, "run_completed" | "run_failed" | "run_cancelled")
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
