// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub use talon_auth::*;

impl talon_auth::AuthState for crate::gateway::server::Gateway {
    fn auth_config(&self) -> Option<&AuthConfig> {
        self.auth_config.as_ref()
    }
}

pub async fn auth_layer(
    state: axum::extract::State<std::sync::Arc<crate::gateway::server::Gateway>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    talon_auth::auth_layer(state, request, next).await
}
