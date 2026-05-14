use super::{models, proto, GrpcGatewayHandler};
use crate::control::{keys, ProtoKeyValueStoreExt};
use crate::scheduling;
use futures::{stream, StreamExt};

impl GrpcGatewayHandler {
    pub async fn handle_create_schedule(
        &self,
        req: tonic::Request<proto::CreateScheduleRequest>,
    ) -> std::result::Result<tonic::Response<proto::ScheduleResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();

        let mut schedule = req
            .schedule
            .ok_or_else(|| tonic::Status::invalid_argument("schedule is required"))?;
        schedule.ns = req.ns.clone();
        if schedule.name.is_empty() {
            return Err(tonic::Status::invalid_argument("schedule.name is required"));
        }
        if self
            .gateway
            .kv
            .get_msg::<models::Schedule>(&req.ns, &keys::schedule(&schedule.name))
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("failed to check schedule existence: {}", e))
            })?
            .is_some()
        {
            return Err(tonic::Status::already_exists("schedule already exists"));
        }

        scheduling::initialize_schedule(&mut schedule, chrono::Utc::now())
            .map_err(|e| tonic::Status::invalid_argument(e.to_string()))?;
        let next_run = schedule
            .status
            .as_ref()
            .and_then(|status| status.next_run_at)
            .and_then(chrono::DateTime::from_timestamp_micros);
        scheduling::arm_schedule(self.gateway.scheduler.as_ref(), &mut schedule, next_run)
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to arm schedule: {}", e)))?;
        scheduling::persist_schedule(self.gateway.kv.as_ref(), &schedule)
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to persist schedule: {}", e)))?;

        Ok(tonic::Response::new(proto::ScheduleResponse {
            schedule: Some(schedule),
        }))
    }

    pub async fn handle_get_schedule(
        &self,
        req: tonic::Request<proto::GetScheduleRequest>,
    ) -> std::result::Result<tonic::Response<proto::ScheduleResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let schedule = self
            .gateway
            .kv
            .get_msg::<models::Schedule>(&req.ns, &keys::schedule(&req.name))
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to load schedule: {}", e)))?
            .ok_or_else(|| tonic::Status::not_found("schedule not found"))?;

        Ok(tonic::Response::new(proto::ScheduleResponse {
            schedule: Some(schedule),
        }))
    }

    pub async fn handle_modify_schedule(
        &self,
        req: tonic::Request<proto::ModifyScheduleRequest>,
    ) -> std::result::Result<tonic::Response<proto::ScheduleResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();

        let existing = self
            .gateway
            .kv
            .get_msg::<models::Schedule>(&req.ns, &keys::schedule(&req.name))
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to load schedule: {}", e)))?
            .ok_or_else(|| tonic::Status::not_found("schedule not found"))?;

        let request_schedule = req
            .schedule
            .ok_or_else(|| tonic::Status::invalid_argument("schedule is required"))?;
        let requested_labels = request_schedule.labels.clone();
        let mut schedule = request_schedule;
        schedule.name = req.name.clone();
        schedule.ns = req.ns.clone();
        schedule.labels = requested_labels;
        schedule.status = existing.status.clone();

        scheduling::initialize_schedule(&mut schedule, chrono::Utc::now())
            .map_err(|e| tonic::Status::invalid_argument(e.to_string()))?;
        let next_run = schedule
            .status
            .as_ref()
            .and_then(|status| status.next_run_at)
            .and_then(chrono::DateTime::from_timestamp_micros);
        scheduling::arm_schedule(self.gateway.scheduler.as_ref(), &mut schedule, next_run)
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to arm schedule: {}", e)))?;
        scheduling::persist_schedule(self.gateway.kv.as_ref(), &schedule)
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to persist schedule: {}", e)))?;

        Ok(tonic::Response::new(proto::ScheduleResponse {
            schedule: Some(schedule),
        }))
    }

    pub async fn handle_list_schedules(
        &self,
        req: tonic::Request<proto::ListSchedulesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListSchedulesResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();

        let mut keys = self
            .gateway
            .kv
            .list_keys(&req.ns, keys::schedule_prefix())
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to list schedules: {}", e)))?;
        keys.sort();

        let schedule_keys = keys
            .into_iter()
            .filter(|key| {
                let stripped = key.strip_prefix(keys::schedule_prefix()).unwrap_or(key);
                !stripped.contains('/')
            })
            .collect::<Vec<_>>();
        let kv = self.gateway.kv.clone();
        let namespace = req.ns.clone();
        let fetched = stream::iter(schedule_keys.into_iter().map(move |key| {
            let kv = kv.clone();
            let namespace = namespace.clone();
            async move { kv.get_msg::<models::Schedule>(&namespace, &key).await }
        }))
        .buffer_unordered(32)
        .collect::<Vec<_>>()
        .await;

        let mut schedules = Vec::new();
        for schedule in fetched {
            if let Some(schedule) = schedule.map_err(|e| {
                tonic::Status::internal(format!("failed to load schedule during list: {}", e))
            })? {
                schedules.push(schedule);
            }
        }

        Ok(tonic::Response::new(proto::ListSchedulesResponse {
            schedules,
        }))
    }

    pub async fn handle_delete_schedule(
        &self,
        req: tonic::Request<proto::DeleteScheduleRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteScheduleResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let key = keys::schedule(&req.name);
        if let Some(schedule) = self
            .gateway
            .kv
            .get_msg::<models::Schedule>(&req.ns, &key)
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to load schedule: {}", e)))?
        {
            if let Some(handle) = schedule.status.and_then(|s| s.backend_handle) {
                if let Err(err) = self.gateway.scheduler.cancel(&handle).await {
                    tracing::warn!(handle = %handle, error = %err, "failed to cancel schedule handle");
                }
            }
        }

        self.gateway
            .kv
            .delete(&req.ns, &key)
            .await
            .map_err(|e| tonic::Status::internal(format!("failed to delete schedule: {}", e)))?;

        Ok(tonic::Response::new(proto::DeleteScheduleResponse {
            success: true,
        }))
    }
}
