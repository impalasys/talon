// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::gateway::rpc::worker_proto::{
    session_control_service_server::SessionControlService, CancelSessionGenerationRequest,
    CancelSessionGenerationResponse,
};
use std::collections::HashMap;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SessionCancellationKey {
    pub ns: String,
    pub agent: String,
    pub session_id: String,
    pub submission_id: String,
    pub attempt_id: String,
}

impl SessionCancellationKey {
    pub fn new(
        ns: &str,
        agent: &str,
        session_id: &str,
        submission_id: &str,
        attempt_id: &str,
    ) -> Self {
        Self {
            ns: ns.into(),
            agent: agent.into(),
            session_id: session_id.into(),
            submission_id: submission_id.into(),
            attempt_id: attempt_id.into(),
        }
    }
}

#[derive(Default)]
pub struct SessionCancellationRegistry {
    tokens: Mutex<HashMap<SessionCancellationKey, CancellationToken>>,
}

impl SessionCancellationRegistry {
    pub async fn insert(&self, key: SessionCancellationKey, token: CancellationToken) {
        self.tokens.lock().await.insert(key, token);
    }
    pub async fn remove(&self, key: &SessionCancellationKey) {
        self.tokens.lock().await.remove(key);
    }
    pub async fn is_empty(&self) -> bool {
        self.tokens.lock().await.is_empty()
    }
    pub async fn cancel(&self, key: &SessionCancellationKey) -> bool {
        let token = self.tokens.lock().await.get(key).cloned();
        if let Some(token) = token {
            token.cancel();
            true
        } else {
            false
        }
    }
}

#[derive(Clone)]
pub struct SessionControlServiceImpl {
    registry: std::sync::Arc<SessionCancellationRegistry>,
}

impl SessionControlServiceImpl {
    pub fn new(registry: std::sync::Arc<SessionCancellationRegistry>) -> Self {
        Self { registry }
    }
}

#[tonic::async_trait]
impl SessionControlService for SessionControlServiceImpl {
    async fn cancel_session_generation(
        &self,
        request: Request<CancelSessionGenerationRequest>,
    ) -> Result<Response<CancelSessionGenerationResponse>, Status> {
        let request = request.into_inner();
        let key = SessionCancellationKey::new(
            &request.ns,
            &request.agent,
            &request.session_id,
            &request.submission_id,
            &request.attempt_id,
        );
        Ok(Response::new(CancelSessionGenerationResponse {
            cancelled: self.registry.cancel(&key).await,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancels_only_the_exact_registered_attempt() {
        let registry = std::sync::Arc::new(SessionCancellationRegistry::default());
        let token = CancellationToken::new();
        let key = SessionCancellationKey::new("ns", "agent", "session", "submission", "attempt-a");
        registry.insert(key.clone(), token.clone()).await;
        assert!(
            !registry
                .cancel(&SessionCancellationKey::new(
                    "ns",
                    "agent",
                    "session",
                    "submission",
                    "attempt-b"
                ))
                .await
        );
        assert!(!token.is_cancelled());
        assert!(registry.cancel(&key).await);
        assert!(token.is_cancelled());
    }
}
