// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::keys;
use crate::gateway::rpc::{data_proto, worker_proto};
use crate::gateway::server::Gateway;
use prost::Message;

pub(crate) async fn cancel_session_generation(
    gateway: &Gateway,
    ns: &str,
    agent: &str,
    session_id: &str,
) -> Result<(), tonic::Status> {
    let mut targets = vec![(ns.to_string(), agent.to_string(), session_id.to_string())];
    let cp = gateway.control_plane();
    targets.extend(
        crate::control::delegation::open_a2a_connection_descendants(&cp, ns, agent, session_id)
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to resolve A2A connections: {e}"))
            })?,
    );
    for (ns, agent, session_id) in targets {
        cancel_one(gateway, &ns, &agent, &session_id).await?;
    }
    Ok(())
}

async fn cancel_one(
    gateway: &Gateway,
    ns: &str,
    agent: &str,
    session_id: &str,
) -> Result<(), tonic::Status> {
    let prefix = keys::session_submission_prefix(ns, agent, session_id);
    let now = chrono::Utc::now().timestamp_micros();
    let mut active = Vec::new();
    for (_, bytes) in gateway
        .kv
        .list_entries(&prefix, None)
        .await
        .map_err(|e| tonic::Status::internal(format!("Failed to list submissions: {e}")))?
    {
        let submission = data_proto::SessionSubmission::decode(bytes.as_slice())
            .map_err(|e| tonic::Status::internal(format!("Failed to decode submission: {e}")))?;
        if submission.status == data_proto::SessionSubmissionStatus::Claimed as i32
            && submission
                .claim_expires_at
                .is_some_and(|expires| expires > now)
            && !submission.claim_worker_id.is_empty()
        {
            active.push(submission);
        }
    }
    let Some(submission) = active
        .into_iter()
        .max_by_key(|s| (s.created_at, s.updated_at))
    else {
        return Ok(());
    };
    let endpoints = crate::gateway::worker_conn::WorkerConnectionPool::worker_endpoints(
        gateway.kv.as_ref(),
        &submission.claim_worker_id,
    )
    .await?;
    let mut last_error = None;
    for endpoint in endpoints {
        let result = async {
            let mut client = gateway
                .worker_connections
                .session_control_client(&endpoint)
                .await?;
            client
                .cancel_session_generation(worker_proto::CancelSessionGenerationRequest {
                    ns: ns.into(),
                    agent: agent.into(),
                    session_id: session_id.into(),
                    submission_id: submission.submission_id.clone(),
                    attempt_id: submission.attempt_id.clone(),
                })
                .await
        }
        .await;
        match result {
            Ok(response) => {
                if response.into_inner().cancelled {
                    return Ok(());
                }
                // A false acknowledgement is not success: the caller must
                // retry rather than lose a stop during claim registration.
                return Err(tonic::Status::unavailable(
                    "worker no longer has the claimed session attempt",
                ));
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| tonic::Status::unavailable("worker has no endpoints")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{ControlPlane, ProtoKeyValueStoreExt};
    use crate::gateway::rpc::resources_proto;
    use crate::test_support::{EmptyPubSub, MockKvStore};
    use crate::worker::session_control::{
        SessionCancellationKey, SessionCancellationRegistry, SessionControlServiceImpl,
    };
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::net::UnixListener;
    use tokio_stream::wrappers::UnixListenerStream;
    use tokio_util::sync::CancellationToken;
    use tonic::transport::Server;

    async fn serve(
        path: &std::path::Path,
        registry: Arc<SessionCancellationRegistry>,
        shutdown: CancellationToken,
    ) {
        let listener = UnixListener::bind(path).unwrap();
        let service =
            worker_proto::session_control_service_server::SessionControlServiceServer::new(
                SessionControlServiceImpl::new(registry),
            );
        tokio::spawn(async move {
            Server::builder()
                .add_service(service)
                .serve_with_incoming_shutdown(
                    UnixListenerStream::new(listener),
                    shutdown.cancelled_owned(),
                )
                .await
                .unwrap();
        });
    }

    async fn put_worker(kv: &MockKvStore, id: &str, path: &std::path::Path) {
        kv.set_msg(
            &keys::ResourceKey::new(crate::control::ns::TALON_SYSTEM, &[], "Worker", id),
            &resources_proto::Worker {
                status: Some(resources_proto::WorkerStatus {
                    phase: "ready".into(),
                    endpoints: vec![resources_proto::WorkerEndpoint {
                        url: format!("unix://{}", path.display()),
                        protocol: "grpc".into(),
                        audience: String::new(),
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn routes_cancellation_to_claiming_worker_not_another_worker() {
        let dir = tempdir().unwrap();
        let path_a = dir.path().join("a.sock");
        let path_b = dir.path().join("b.sock");
        let registry_a = Arc::new(SessionCancellationRegistry::default());
        let registry_b = Arc::new(SessionCancellationRegistry::default());
        let shutdown_a = CancellationToken::new();
        let shutdown_b = CancellationToken::new();
        serve(&path_a, registry_a.clone(), shutdown_a.clone()).await;
        serve(&path_b, registry_b.clone(), shutdown_b.clone()).await;
        let kv = Arc::new(MockKvStore::default());
        put_worker(&kv, "worker-a", &path_a).await;
        put_worker(&kv, "worker-b", &path_b).await;
        let token_a = CancellationToken::new();
        let key = SessionCancellationKey::new("ns", "agent", "session", "submission", "attempt");
        registry_a.insert(key, token_a.clone()).await;
        let token_b = CancellationToken::new();
        let child_key = SessionCancellationKey::new(
            "ns",
            "child",
            "child-session",
            "child-submission",
            "child-attempt",
        );
        registry_b.insert(child_key, token_b.clone()).await;
        kv.set_msg(
            &keys::session_submission("ns", "agent", "session", "submission"),
            &data_proto::SessionSubmission {
                submission_id: "submission".into(),
                session_id: "session".into(),
                status: data_proto::SessionSubmissionStatus::Claimed as i32,
                claim_worker_id: "worker-a".into(),
                attempt_id: "attempt".into(),
                claim_expires_at: Some(chrono::Utc::now().timestamp_micros() + 60_000_000),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::session("ns", "agent", "session"),
            &data_proto::Session {
                id: "session".into(),
                ns: "ns".into(),
                agent: "agent".into(),
                metadata: std::collections::HashMap::from([(
                    "wire.a2a.talon.impalasys.com/child".into(),
                    "ns/child/child-session".into(),
                )]),
                context_tokens: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::session("ns", "child", "child-session"),
            &data_proto::Session {
                id: "child-session".into(),
                ns: "ns".into(),
                agent: "child".into(),
                metadata: std::collections::HashMap::from([(
                    "wire.a2a.talon.impalasys.com/owner".into(),
                    "ns/agent/session".into(),
                )]),
                context_tokens: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        kv.set_msg(
            &keys::session_submission("ns", "child", "child-session", "child-submission"),
            &data_proto::SessionSubmission {
                submission_id: "child-submission".into(),
                session_id: "child-session".into(),
                status: data_proto::SessionSubmissionStatus::Claimed as i32,
                claim_worker_id: "worker-b".into(),
                attempt_id: "child-attempt".into(),
                claim_expires_at: Some(chrono::Utc::now().timestamp_micros() + 60_000_000),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let gateway = Gateway::from_control_plane(
            None,
            ControlPlane::builder(kv, Arc::new(EmptyPubSub)).build(),
        );
        cancel_session_generation(&gateway, "ns", "agent", "session")
            .await
            .unwrap();
        assert!(token_a.is_cancelled());
        assert!(token_b.is_cancelled());
        shutdown_a.cancel();
        shutdown_b.cancel();
    }
}
