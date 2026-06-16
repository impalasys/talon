// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{proto, GrpcGatewayHandler};
use crate::control::resources::ResourceStore;

impl GrpcGatewayHandler {
    pub async fn handle_create_resource(
        &self,
        req: tonic::Request<proto::CreateResourceRequest>,
    ) -> std::result::Result<tonic::Response<proto::ResourceResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let resource = req
            .resource
            .ok_or_else(|| tonic::Status::invalid_argument("resource is required"))?;
        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        let resource = store
            .upsert(&req.ns, resource)
            .await
            .map_err(|err| tonic::Status::invalid_argument(err.to_string()))?;
        Ok(tonic::Response::new(proto::ResourceResponse {
            resource: Some(resource),
        }))
    }

    pub async fn handle_get_resource(
        &self,
        req: tonic::Request<proto::GetResourceRequest>,
    ) -> std::result::Result<tonic::Response<proto::ResourceResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        let resource = store
            .get(&req.ns, &req.kind, &req.name)
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?
            .ok_or_else(|| tonic::Status::not_found("resource not found"))?;
        Ok(tonic::Response::new(proto::ResourceResponse {
            resource: Some(resource),
        }))
    }

    pub async fn handle_list_resources(
        &self,
        req: tonic::Request<proto::ListResourcesRequest>,
    ) -> std::result::Result<tonic::Response<proto::ListResourcesResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        let resources = store
            .list(&req.ns, req.kind.as_deref())
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?;
        Ok(tonic::Response::new(proto::ListResourcesResponse {
            resources,
        }))
    }

    pub async fn handle_delete_resource(
        &self,
        req: tonic::Request<proto::DeleteResourceRequest>,
    ) -> std::result::Result<tonic::Response<proto::DeleteResourceResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        let success = store
            .delete(&req.ns, &req.kind, &req.name)
            .await
            .map_err(|err| tonic::Status::internal(err.to_string()))?;
        if !success {
            return Err(tonic::Status::not_found("resource not found"));
        }
        Ok(tonic::Response::new(proto::DeleteResourceResponse {
            success,
        }))
    }
}
