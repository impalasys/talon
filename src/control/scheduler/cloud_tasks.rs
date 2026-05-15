// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::config::{proto::CloudTasksSchedulerConfig, SecretExt};
use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use chrono::{DateTime, Utc};
use google_cloud_auth::credentials::{AccessTokenCredentials, Builder as CredentialsBuilder};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use super::{ScheduleWakeupRequest, ScheduledWakeup, SchedulerBackend, SCHEDULER_AUTH_HEADER};

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const MAX_CLOUD_TASK_SCHEDULE_AHEAD_SECS: i64 = 30 * 24 * 60 * 60;
const CLOUD_TASKS_HTTP_TIMEOUT_SECS: u64 = 10;

pub struct CloudTasksSchedulerBackend {
    client: reqwest::Client,
    credentials: AccessTokenCredentials,
    parent: String,
    target_url: String,
    auth_token: Option<String>,
    oidc_service_account_email: Option<String>,
    oidc_audience: Option<String>,
}

impl CloudTasksSchedulerBackend {
    pub async fn new(cfg: &CloudTasksSchedulerConfig) -> Result<Self> {
        let project_id = non_empty(cfg.project_id.clone())
            .or_else(|| std::env::var("TALON_SCHEDULER_PROJECT_ID").ok())
            .or_else(|| std::env::var("GCP_PROJECT_ID").ok())
            .unwrap_or_default();
        let location = non_empty(cfg.location.clone())
            .or_else(|| std::env::var("TALON_SCHEDULER_LOCATION").ok())
            .unwrap_or_default();
        let queue = non_empty(cfg.queue.clone())
            .or_else(|| std::env::var("TALON_SCHEDULER_QUEUE").ok())
            .unwrap_or_default();
        let target_url = non_empty(cfg.target_url.clone())
            .or_else(|| std::env::var("TALON_SCHEDULER_TARGET_URL").ok())
            .unwrap_or_default();
        if project_id.is_empty() || location.is_empty() || queue.is_empty() || target_url.is_empty()
        {
            return Err(anyhow!(
                "cloud_tasks scheduler requires project_id, location, queue, and target_url"
            ));
        }

        let credentials = CredentialsBuilder::default()
            .with_scopes([CLOUD_PLATFORM_SCOPE])
            .build_access_token_credentials()
            .context("failed to build Google access token credentials for Cloud Tasks")?;

        let (auth_token, oidc_service_account_email, oidc_audience) =
            resolve_callback_auth(cfg.callback_auth.as_ref()).await?;

        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(
                    CLOUD_TASKS_HTTP_TIMEOUT_SECS,
                ))
                .build()
                .context("failed to build Cloud Tasks HTTP client")?,
            credentials,
            parent: format!(
                "projects/{}/locations/{}/queues/{}",
                project_id, location, queue
            ),
            target_url,
            auth_token,
            oidc_service_account_email,
            oidc_audience,
        })
    }

    async fn bearer_token(&self) -> Result<String> {
        Ok(self.credentials.access_token().await?.token)
    }
}

fn compute_cloud_tasks_schedule_at(
    now: DateTime<Utc>,
    fire_at: DateTime<Utc>,
) -> (DateTime<Utc>, bool) {
    let max_schedule_at = now + chrono::Duration::seconds(MAX_CLOUD_TASK_SCHEDULE_AHEAD_SECS - 60);
    let schedule_at = std::cmp::min(fire_at, max_schedule_at);
    (schedule_at, schedule_at != fire_at)
}

async fn resolve_callback_auth(
    cfg: Option<&crate::config::proto::SchedulerCallbackAuthConfig>,
) -> Result<(Option<String>, Option<String>, Option<String>)> {
    if let Some(cfg) = cfg {
        return match cfg.auth.as_ref() {
            Some(crate::config::proto::scheduler_callback_auth_config::Auth::SharedSecret(
                secret,
            )) => Ok((Some(secret.resolve().await?), None, None)),
            Some(crate::config::proto::scheduler_callback_auth_config::Auth::GoogleOidc(oidc)) => {
                Ok((
                    None,
                    non_empty(oidc.service_account_email.clone()),
                    non_empty(oidc.audience.clone()),
                ))
            }
            None => Ok((None, None, None)),
        };
    }

    Ok((
        std::env::var("TALON_SCHEDULER_AUTH_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty()),
        std::env::var("TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL")
            .ok()
            .filter(|value| !value.trim().is_empty()),
        std::env::var("TALON_SCHEDULER_AUDIENCE")
            .ok()
            .filter(|value| !value.trim().is_empty()),
    ))
}

#[async_trait::async_trait]
impl SchedulerBackend for CloudTasksSchedulerBackend {
    async fn schedule(&self, req: ScheduleWakeupRequest) -> Result<ScheduledWakeup> {
        let now = Utc::now();
        let (schedule_at, is_checkpoint) = compute_cloud_tasks_schedule_at(now, req.fire_at);
        if is_checkpoint {
            tracing::info!(
                schedule_id = %req.schedule_id,
                schedule_at = %schedule_at,
                fire_at = %req.fire_at,
                "Scheduling Cloud Tasks checkpoint before the final intended fire time"
            );
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct OidcToken<'a> {
            service_account_email: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            audience: Option<&'a str>,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct HttpRequest<'a> {
            http_method: &'static str,
            url: &'a str,
            headers: std::collections::HashMap<&'static str, String>,
            body: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            oidc_token: Option<OidcToken<'a>>,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Task<'a> {
            schedule_time: String,
            http_request: HttpRequest<'a>,
        }

        #[derive(Serialize)]
        struct CreateTaskRequest<'a> {
            task: Task<'a>,
        }

        #[derive(Deserialize)]
        struct TaskResponse {
            name: Option<String>,
        }

        let mut headers = std::collections::HashMap::new();
        headers.insert("Content-Type", "application/json".to_string());
        if let Some(token) = &self.auth_token {
            headers.insert(SCHEDULER_AUTH_HEADER, token.clone());
        }

        let body = base64::engine::general_purpose::STANDARD.encode(req.payload);
        let oidc_token = self
            .oidc_service_account_email
            .as_deref()
            .map(|service_account_email| OidcToken {
                service_account_email,
                audience: self.oidc_audience.as_deref(),
            });
        let create_req = CreateTaskRequest {
            task: Task {
                schedule_time: schedule_at.to_rfc3339(),
                http_request: HttpRequest {
                    http_method: "POST",
                    url: &self.target_url,
                    headers,
                    body,
                    oidc_token,
                },
            },
        };

        let url = format!("https://cloudtasks.googleapis.com/v2/{}/tasks", self.parent);
        let resp = self
            .client
            .post(url)
            .bearer_auth(self.bearer_token().await?)
            .json(&create_req)
            .send()
            .await
            .context("failed to create Cloud Task")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "cloud tasks create failed: status={} body={}",
                status,
                body
            ));
        }

        let parsed: TaskResponse = resp
            .json()
            .await
            .context("failed to parse Cloud Tasks response")?;
        Ok(ScheduledWakeup {
            handle: parsed.name,
            armed: true,
        })
    }

    async fn cancel(&self, handle: &str) -> Result<()> {
        let resp = self
            .client
            .delete(format!("https://cloudtasks.googleapis.com/v2/{}", handle))
            .bearer_auth(self.bearer_token().await?)
            .send()
            .await
            .context("failed to delete Cloud Task")?;

        if resp.status().is_success() || resp.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(anyhow!(
            "cloud tasks delete failed: status={} body={}",
            status,
            body
        ))
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::sync::Mutex;

    static SCHEDULER_AUTH_ENV_MUTEX: Mutex<()> = Mutex::new(());

    fn clear_scheduler_auth_env() {
        unsafe {
            std::env::remove_var("TALON_SCHEDULER_AUTH_TOKEN");
            std::env::remove_var("TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL");
            std::env::remove_var("TALON_SCHEDULER_AUDIENCE");
        }
    }

    #[test]
    fn cloud_tasks_schedule_at_uses_final_fire_when_within_horizon() {
        let now = Utc.with_ymd_and_hms(2026, 5, 4, 12, 0, 0).unwrap();
        let fire_at = now + chrono::Duration::days(7);

        let (schedule_at, is_checkpoint) = compute_cloud_tasks_schedule_at(now, fire_at);

        assert_eq!(schedule_at, fire_at);
        assert!(!is_checkpoint);
    }

    #[test]
    fn cloud_tasks_schedule_at_creates_checkpoint_before_far_future_fire() {
        let now = Utc.with_ymd_and_hms(2026, 5, 4, 12, 0, 0).unwrap();
        let fire_at = now + chrono::Duration::days(45);

        let (schedule_at, is_checkpoint) = compute_cloud_tasks_schedule_at(now, fire_at);

        assert_eq!(
            schedule_at,
            now + chrono::Duration::seconds(MAX_CLOUD_TASK_SCHEDULE_AHEAD_SECS - 60)
        );
        assert!(is_checkpoint);
        assert!(schedule_at < fire_at);
    }

    #[tokio::test]
    async fn resolve_callback_auth_prefers_explicit_google_oidc_over_env_secret() {
        let _guard = SCHEDULER_AUTH_ENV_MUTEX.lock().unwrap();
        clear_scheduler_auth_env();
        unsafe {
            std::env::set_var("TALON_SCHEDULER_AUTH_TOKEN", "stale-secret");
            std::env::set_var("TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL", "stale@example.com");
            std::env::set_var("TALON_SCHEDULER_AUDIENCE", "https://stale.example.com");
        }

        let cfg = crate::config::proto::SchedulerCallbackAuthConfig {
            auth: Some(
                crate::config::proto::scheduler_callback_auth_config::Auth::GoogleOidc(
                    crate::config::proto::GoogleOidcAuthConfig {
                        service_account_email: "scheduler@example.com".to_string(),
                        audience: "https://worker.example.com".to_string(),
                    },
                ),
            ),
        };

        let (auth_token, service_account_email, audience) =
            resolve_callback_auth(Some(&cfg)).await.unwrap();

        assert_eq!(auth_token, None);
        assert_eq!(
            service_account_email.as_deref(),
            Some("scheduler@example.com")
        );
        assert_eq!(audience.as_deref(), Some("https://worker.example.com"));

        clear_scheduler_auth_env();
    }

    #[tokio::test]
    async fn resolve_callback_auth_uses_env_when_config_absent() {
        let _guard = SCHEDULER_AUTH_ENV_MUTEX.lock().unwrap();
        clear_scheduler_auth_env();
        unsafe {
            std::env::set_var("TALON_SCHEDULER_AUTH_TOKEN", "shared-secret");
            std::env::set_var(
                "TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL",
                "scheduler@example.com",
            );
            std::env::set_var("TALON_SCHEDULER_AUDIENCE", "https://worker.example.com");
        }

        let (auth_token, service_account_email, audience) = resolve_callback_auth(None).await.unwrap();

        assert_eq!(auth_token.as_deref(), Some("shared-secret"));
        assert_eq!(
            service_account_email.as_deref(),
            Some("scheduler@example.com")
        );
        assert_eq!(audience.as_deref(), Some("https://worker.example.com"));

        clear_scheduler_auth_env();
    }
}
