// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::sync::Arc;
use std::time::Duration;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;

use crate::gateway::server::Gateway;

mod card;
mod handlers;
mod tasks;
mod types;

pub(super) const A2A_BLOCKING_TIMEOUT: Duration = Duration::from_secs(60);
pub(super) const A2A_POLL_INTERVAL: Duration = Duration::from_millis(250);
pub(super) const A2A_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) fn router() -> Router<Arc<Gateway>> {
    Router::new()
        .route(
            "/a2a/:ns/:agent/agent-card.json",
            get(handlers::get_agent_card),
        )
        .route(
            "/a2a/:ns/:agent/message:operation",
            post(handlers::post_message_operation),
        )
        .route(
            "/a2a/:ns/:agent/v1/message:operation",
            post(handlers::post_message_operation),
        )
        .route("/a2a/:ns/:agent/tasks", get(handlers::list_tasks))
        .route("/a2a/:ns/:agent/v1/tasks", get(handlers::list_tasks))
        .route(
            "/a2a/:ns/:agent/tasks/*tail",
            get(handlers::get_task).post(handlers::post_task_operation),
        )
        .route(
            "/a2a/:ns/:agent/v1/tasks/*tail",
            get(handlers::get_task).post(handlers::post_task_operation),
        )
        .route(
            "/a2a/:ns/:agent/extendedAgentCard",
            get(handlers::unsupported_a2a_operation),
        )
}

pub(super) fn a2a_error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "code": status.as_u16(),
                "message": message.into(),
            }
        })),
    )
        .into_response()
}
