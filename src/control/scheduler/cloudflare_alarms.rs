// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};

use super::{ScheduleWakeupRequest, ScheduledWakeup, SchedulerBackend};

pub struct CloudflareAlarmsSchedulerBackend {
    client: reqwest::Client,
    endpoint: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleRequest {
    namespace: String,
    schedule_id: String,
    revision: u64,
    fire_at_micros: i64,
    payload_base64: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleResponse {
    handle: Option<String>,
    armed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CancelRequest<'a> {
    handle: &'a str,
}

impl CloudflareAlarmsSchedulerBackend {
    pub fn from_env() -> Self {
        let endpoint = std::env::var("TALON_CLOUDFLARE_ALARMS_URL")
            .unwrap_or_else(|_| "http://talon-alarms.internal".to_string());
        Self::new(endpoint)
    }

    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
        }
    }

    async fn post_json<TReq, TResp>(&self, path: &str, body: &TReq) -> Result<TResp>
    where
        TReq: Serialize + ?Sized,
        TResp: for<'de> Deserialize<'de>,
    {
        let response = self
            .client
            .post(format!("{}{}", self.endpoint, path))
            .json(body)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Cloudflare alarms scheduler request {path} failed with HTTP {status}: {body}"
            ));
        }
        Ok(response.json::<TResp>().await?)
    }
}

#[async_trait::async_trait]
impl SchedulerBackend for CloudflareAlarmsSchedulerBackend {
    async fn schedule(&self, req: ScheduleWakeupRequest) -> Result<ScheduledWakeup> {
        let response: ScheduleResponse = self
            .post_json(
                "/schedule",
                &ScheduleRequest {
                    namespace: req.namespace,
                    schedule_id: req.schedule_id,
                    revision: req.revision,
                    fire_at_micros: req.fire_at.timestamp_micros(),
                    payload_base64: general_purpose::STANDARD.encode(req.payload),
                },
            )
            .await?;
        Ok(ScheduledWakeup {
            handle: response.handle,
            armed: response.armed,
        })
    }

    async fn cancel(&self, handle: &str) -> Result<()> {
        let _: serde_json::Value = self.post_json("/cancel", &CancelRequest { handle }).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, routing::post, Json, Router};
    use chrono::TimeZone;
    use serde_json::{json, Value};
    use std::sync::Arc;
    use tokio::{net::TcpListener, sync::Mutex};

    #[derive(Clone, Default)]
    struct CapturedRequests {
        schedule: Arc<Mutex<Option<Value>>>,
        cancel: Arc<Mutex<Option<Value>>>,
    }

    async fn schedule_handler(
        State(captured): State<CapturedRequests>,
        Json(payload): Json<Value>,
    ) -> Json<Value> {
        *captured.schedule.lock().await = Some(payload);
        Json(json!({ "handle": "alarm-123", "armed": true }))
    }

    async fn cancel_handler(
        State(captured): State<CapturedRequests>,
        Json(payload): Json<Value>,
    ) -> Json<Value> {
        *captured.cancel.lock().await = Some(payload);
        Json(json!({}))
    }

    #[tokio::test]
    async fn schedule_and_cancel_use_worker_alarm_http_contract() {
        let captured = CapturedRequests::default();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/schedule", post(schedule_handler))
            .route("/cancel", post(cancel_handler))
            .with_state(captured.clone());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let backend = CloudflareAlarmsSchedulerBackend::new(format!("http://{addr}"));
        let fire_at = chrono::Utc
            .with_ymd_and_hms(2026, 6, 13, 12, 30, 0)
            .unwrap();
        let scheduled = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "default".to_string(),
                schedule_id: "wake".to_string(),
                revision: 7,
                fire_at,
                payload: br#"{"kind":"scheduled"}"#.to_vec(),
            })
            .await
            .unwrap();

        assert_eq!(scheduled.handle.as_deref(), Some("alarm-123"));
        assert!(scheduled.armed);
        assert_eq!(
            *captured.schedule.lock().await,
            Some(json!({
                "namespace": "default",
                "scheduleId": "wake",
                "revision": 7,
                "fireAtMicros": fire_at.timestamp_micros(),
                "payloadBase64": "eyJraW5kIjoic2NoZWR1bGVkIn0="
            }))
        );

        backend.cancel("alarm-123").await.unwrap();
        assert_eq!(
            *captured.cancel.lock().await,
            Some(json!({ "handle": "alarm-123" }))
        );
    }
}
