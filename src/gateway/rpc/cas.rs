// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{proto, GrpcGatewayHandler};
use crate::control::cas::{
    logical_object_bytes, object_ref_from_stored_object, CasStore, SessionCasScope,
};

impl GrpcGatewayHandler {
    pub async fn handle_get_cas_object(
        &self,
        req: tonic::Request<proto::GetCasObjectRequest>,
    ) -> std::result::Result<tonic::Response<proto::GetCasObjectResponse>, tonic::Status> {
        let body = req.get_ref();
        crate::require_auth!(read, self, req, &body.ns, &body.agent, &body.session_id);

        if body.key.trim().is_empty() {
            return Err(tonic::Status::invalid_argument("key is required"));
        }

        let scope = SessionCasScope::new(&body.ns, &body.agent, &body.session_id);
        let cas = CasStore::new(self.gateway.objects.clone());
        let object = cas
            .get_session_object(&scope, &body.key)
            .await
            .map_err(|err| {
                if err
                    .to_string()
                    .contains("outside the requested session scope")
                    || err.to_string().contains("does not match requested scope")
                {
                    tonic::Status::permission_denied("CAS object is outside the requested session")
                } else {
                    tonic::Status::internal(format!("Failed to load CAS object: {err}"))
                }
            })?
            .ok_or_else(|| tonic::Status::not_found("CAS object not found"))?;

        let data = logical_object_bytes(&object, &body.key).map_err(|err| {
            tonic::Status::internal(format!("Failed to decode CAS object: {err}"))
        })?;
        Ok(tonic::Response::new(proto::GetCasObjectResponse {
            object: Some(object_ref_from_stored_object(&body.key, &object)),
            data,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cas::{CasStore, SessionCasScope, SessionObjectIdentity};
    use crate::control::object_store::InMemoryObjectStore;
    use crate::control::ControlPlane;
    use crate::gateway::Gateway;
    use crate::harness::tool_results::store_tool_result;
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use std::sync::Arc;

    fn handler(objects: Arc<InMemoryObjectStore>) -> GrpcGatewayHandler {
        let control_plane = ControlPlane::builder(
            Arc::new(MockKvStore::default()),
            Arc::new(RecordingPubSub::default()),
        )
        .objects(objects)
        .build();
        GrpcGatewayHandler {
            gateway: Arc::new(Gateway::from_control_plane(None, control_plane)),
        }
    }

    #[tokio::test]
    async fn get_cas_object_returns_session_scoped_bytes() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let cas = CasStore::new(objects.clone());
        let scope = SessionCasScope::new("acme", "agent", "session-1");
        let identity = SessionObjectIdentity::new("message-1", "000001");
        let object = cas
            .put_tool_result(
                &scope, &identity, "call-1", "search", b"hello", b"hello", None,
            )
            .await
            .unwrap();

        let response = handler(objects)
            .handle_get_cas_object(tonic::Request::new(proto::GetCasObjectRequest {
                ns: "acme".to_string(),
                agent: "agent".to_string(),
                session_id: "session-1".to_string(),
                key: object.key.clone(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(response.data, b"hello");
        assert_eq!(response.object.as_ref().unwrap().key, object.key);
    }

    #[tokio::test]
    async fn get_cas_object_returns_decompressed_tool_result_bytes() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let cas = CasStore::new(objects.clone());
        let raw = "large-result".repeat(1024);
        let stored = store_tool_result(
            &cas,
            "acme",
            "agent",
            "session-1",
            "message-1",
            "000001",
            "call-1",
            "search",
            &raw,
        )
        .await
        .unwrap();
        let object = stored.object.expect("large result should be object-backed");
        assert_eq!(object.metadata["content_encoding"], "gzip");

        let response = handler(objects)
            .handle_get_cas_object(tonic::Request::new(proto::GetCasObjectRequest {
                ns: "acme".to_string(),
                agent: "agent".to_string(),
                session_id: "session-1".to_string(),
                key: object.key,
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(String::from_utf8(response.data).unwrap(), raw);
    }

    #[tokio::test]
    async fn get_cas_object_rejects_cross_session_key() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let cas = CasStore::new(objects.clone());
        let scope = SessionCasScope::new("acme", "agent", "session-1");
        let object = cas
            .put_tool_result(
                &scope,
                &SessionObjectIdentity::new("message-1", "000001"),
                "call-1",
                "search",
                b"hello",
                b"hello",
                None,
            )
            .await
            .unwrap();

        let err = handler(objects)
            .handle_get_cas_object(tonic::Request::new(proto::GetCasObjectRequest {
                ns: "acme".to_string(),
                agent: "agent".to_string(),
                session_id: "session-2".to_string(),
                key: object.key,
            }))
            .await
            .unwrap_err();

        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }
}
