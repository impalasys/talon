// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{proto, GrpcGatewayHandler};
use crate::control::cas::{object_ref_from_stored_object, parse_session_object_key, CasStore};
use std::time::Duration;

const CAS_SIGNED_URL_TTL: Duration = Duration::from_secs(5 * 60);

impl GrpcGatewayHandler {
    #[allow(deprecated)]
    pub async fn handle_get_cas_object(
        &self,
        req: tonic::Request<proto::GetCasObjectRequest>,
    ) -> std::result::Result<tonic::Response<proto::GetCasObjectResponse>, tonic::Status> {
        let body = req.get_ref();

        if body.key.trim().is_empty() {
            return Err(tonic::Status::invalid_argument("key is required"));
        }
        parse_session_object_key(&body.key)
            .map_err(|err| tonic::Status::invalid_argument(format!("Invalid CAS key: {err}")))?;

        let cas = CasStore::new(self.gateway.objects.clone());
        let (scope, object) = cas
            .get_session_object_by_key(&body.key)
            .await
            .map_err(|err| {
                if err.to_string().contains("does not match key scope")
                    || err.to_string().contains("metadata is missing agent")
                {
                    tonic::Status::permission_denied("CAS object is outside the authorized session")
                } else {
                    tonic::Status::internal(format!("Failed to load CAS object: {err}"))
                }
            })?
            .ok_or_else(|| tonic::Status::not_found("CAS object not found"))?;
        crate::require_auth!(read, self, req, &scope.ns, &scope.agent, &scope.session_id);

        let signed = cas
            .signed_get_url(&body.key, CAS_SIGNED_URL_TTL)
            .await
            .map_err(|err| {
                tonic::Status::internal(format!("Failed to sign CAS object URL: {err}"))
            })?;
        let (data, signed_url, signed_url_expires_at_unix_seconds) = if let Some(signed) = signed {
            (Vec::new(), signed.url, signed.expires_at_unix_seconds)
        } else {
            (object.bytes.clone(), String::new(), 0)
        };
        let object_ref = object_ref_from_stored_object(&body.key, &object);
        Ok(tonic::Response::new(proto::GetCasObjectResponse {
            data,
            signed_url,
            signed_url_expires_at_unix_seconds,
            metadata: object_ref.metadata,
            media_type: object_ref.media_type,
            size_bytes: object_ref.size_bytes,
            sha256: object_ref.sha256,
            filename: object_ref.filename,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cas::CasStore;
    use crate::control::object_store::InMemoryObjectStore;
    use crate::control::ControlPlane;
    use crate::gateway::Gateway;
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
        let object = cas
            .put_tool_result(
                "acme",
                "agent",
                "session-1",
                "message-1",
                "000001",
                "call-1",
                "search",
                b"hello",
            )
            .await
            .unwrap();

        let response = handler(objects)
            .handle_get_cas_object(tonic::Request::new(proto::GetCasObjectRequest {
                key: object.key.clone(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(response.data, b"hello");
        assert_eq!(response.media_type, object.media_type);
        assert_eq!(response.size_bytes, object.size_bytes);
    }

    #[tokio::test]
    async fn get_cas_object_returns_stored_tool_result_bytes() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let cas = CasStore::new(objects.clone());
        let raw = "large-result".repeat(1024);
        let object = cas
            .put_tool_result_if_raw_at_least(
                "acme",
                "agent",
                "session-1",
                "message-1",
                "000001",
                "call-1",
                "search",
                raw.as_bytes(),
                crate::harness::tool_results::tool_result_object_threshold_bytes(),
            )
            .await
            .unwrap()
            .expect("large result should be object-backed");
        assert_eq!(object.metadata["content_encoding"], "gzip");

        let response = handler(objects)
            .handle_get_cas_object(tonic::Request::new(proto::GetCasObjectRequest {
                key: object.key,
            }))
            .await
            .unwrap()
            .into_inner();

        assert_ne!(response.data, raw.as_bytes());
        assert_eq!(&response.data[..2], &[0x1f, 0x8b]);
        assert_eq!(response.metadata["content_encoding"], "gzip");
    }

    #[tokio::test]
    async fn get_cas_object_rejects_invalid_key() {
        let objects = Arc::new(InMemoryObjectStore::default());
        let err = handler(objects)
            .handle_get_cas_object(tonic::Request::new(proto::GetCasObjectRequest {
                key: "../outside.txt".to_string(),
            }))
            .await
            .unwrap_err();

        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }
}
