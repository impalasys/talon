// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{
    config::proto::AwsEventBridgeSchedulerConfig, pubsub::TALON_TOPIC_ATTRIBUTE, topics,
};
use anyhow::{anyhow, Context, Result};
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_scheduler::{
    config::Credentials,
    types::{
        ActionAfterCompletion, DeadLetterConfig, FlexibleTimeWindow, FlexibleTimeWindowMode,
        RetryPolicy, Target,
    },
    Client,
};
use base64::{engine::general_purpose, Engine as _};
use serde::Serialize;

use super::{ScheduleWakeupRequest, ScheduledWakeup, SchedulerBackend};

const DEFAULT_GROUP_NAME: &str = "default";
const DEFAULT_SCHEDULE_NAME_PREFIX: &str = "talon";
const DEFAULT_MAX_EVENT_AGE_SECONDS: i32 = 3600;
const DEFAULT_MAX_RETRY_ATTEMPTS: i32 = 3;
const SQS_SEND_MESSAGE_TARGET_ARN: &str = "arn:aws:scheduler:::aws-sdk:sqs:sendMessage";

pub struct AwsEventBridgeSchedulerBackend {
    client: Client,
    group_name: String,
    queue_url: String,
    execution_role_arn: String,
    schedule_name_prefix: String,
    dlq_arn: Option<String>,
    maximum_event_age_seconds: i32,
    maximum_retry_attempts: i32,
}

impl AwsEventBridgeSchedulerBackend {
    pub async fn new(cfg: &AwsEventBridgeSchedulerConfig) -> Result<Self> {
        let group_name = non_empty(cfg.group_name.clone())
            .or_else(|| std::env::var("TALON_AWS_SCHEDULER_GROUP_NAME").ok())
            .unwrap_or_else(|| DEFAULT_GROUP_NAME.to_string());
        let queue_url = non_empty(cfg.queue_url.clone())
            .or_else(|| std::env::var("TALON_AWS_SCHEDULER_QUEUE_URL").ok())
            .or_else(|| std::env::var("TALON_SQS_QUEUE_URL").ok())
            .unwrap_or_default();
        let execution_role_arn = non_empty(cfg.execution_role_arn.clone())
            .or_else(|| std::env::var("TALON_AWS_SCHEDULER_EXECUTION_ROLE_ARN").ok())
            .unwrap_or_default();
        if queue_url.is_empty() || execution_role_arn.is_empty() {
            return Err(anyhow!(
                "aws_eventbridge_scheduler scheduler requires queue_url and execution_role_arn"
            ));
        }

        let schedule_name_prefix = non_empty(cfg.schedule_name_prefix.clone())
            .or_else(|| std::env::var("TALON_AWS_SCHEDULER_NAME_PREFIX").ok())
            .unwrap_or_else(|| DEFAULT_SCHEDULE_NAME_PREFIX.to_string());
        let dlq_arn = non_empty(cfg.dlq_arn.clone())
            .or_else(|| std::env::var("TALON_AWS_SCHEDULER_DLQ_ARN").ok());
        let maximum_event_age_seconds = positive_i32(
            cfg.maximum_event_age_seconds,
            "TALON_AWS_SCHEDULER_MAX_EVENT_AGE_SECONDS",
            DEFAULT_MAX_EVENT_AGE_SECONDS,
        );
        let maximum_retry_attempts = non_negative_i32(
            cfg.maximum_retry_attempts,
            "TALON_AWS_SCHEDULER_MAX_RETRY_ATTEMPTS",
            DEFAULT_MAX_RETRY_ATTEMPTS,
        );

        let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(RegionProviderChain::default_provider().or_else("us-east-1"));
        let endpoint_url = non_empty(cfg.endpoint_url.clone())
            .or_else(|| std::env::var("TALON_AWS_SCHEDULER_ENDPOINT_URL").ok());
        if let Some(endpoint_url) = endpoint_url {
            loader = loader
                .credentials_provider(Credentials::new("fake", "fake", None, None, "local"))
                .endpoint_url(endpoint_url);
        }
        let config = loader.load().await;

        Ok(Self {
            client: Client::new(&config),
            group_name,
            queue_url,
            execution_role_arn,
            schedule_name_prefix,
            dlq_arn,
            maximum_event_age_seconds,
            maximum_retry_attempts,
        })
    }

    fn target_for_payload(&self, payload: &[u8]) -> Result<Target> {
        target_for_sqs_payload(
            &self.queue_url,
            &self.execution_role_arn,
            self.dlq_arn.as_deref(),
            self.maximum_event_age_seconds,
            self.maximum_retry_attempts,
            payload,
        )
    }
}

#[async_trait::async_trait]
impl SchedulerBackend for AwsEventBridgeSchedulerBackend {
    async fn schedule(&self, req: ScheduleWakeupRequest) -> Result<ScheduledWakeup> {
        let name = schedule_name(
            &self.schedule_name_prefix,
            &req.namespace,
            &req.schedule_id,
            req.revision,
            req.fire_at.timestamp_micros(),
        );
        let expression = format!("at({})", req.fire_at.format("%Y-%m-%dT%H:%M:%S"));
        let target = self.target_for_payload(&req.payload)?;

        self.client
            .create_schedule()
            .group_name(&self.group_name)
            .name(&name)
            .schedule_expression(expression)
            .schedule_expression_timezone("UTC")
            .flexible_time_window(
                FlexibleTimeWindow::builder()
                    .mode(FlexibleTimeWindowMode::Off)
                    .build()?,
            )
            .action_after_completion(ActionAfterCompletion::Delete)
            .target(target)
            .send()
            .await
            .context("failed to create EventBridge Scheduler schedule")?;

        Ok(ScheduledWakeup {
            handle: Some(format!("{}/{}", self.group_name, name)),
            armed: true,
        })
    }

    async fn cancel(&self, handle: &str) -> Result<()> {
        let (group_name, name) = parse_handle(handle)
            .ok_or_else(|| anyhow!("invalid EventBridge Scheduler handle '{}'", handle))?;
        let result = self
            .client
            .delete_schedule()
            .group_name(group_name)
            .name(name)
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(err)
                if err
                    .as_service_error()
                    .is_some_and(|err| err.is_resource_not_found_exception()) =>
            {
                Ok(())
            }
            Err(err) => Err(err).context("failed to delete EventBridge Scheduler schedule"),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct SendMessageInput<'a> {
    queue_url: &'a str,
    message_body: String,
    message_attributes: std::collections::HashMap<&'static str, MessageAttribute<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct MessageAttribute<'a> {
    data_type: &'static str,
    string_value: &'a str,
}

fn sqs_send_message_input(queue_url: &str, payload: &[u8]) -> Result<String> {
    let mut message_attributes = std::collections::HashMap::new();
    message_attributes.insert(
        TALON_TOPIC_ATTRIBUTE,
        MessageAttribute {
            data_type: "String",
            string_value: topics::SCHEDULE_FIRE_TOPIC,
        },
    );
    Ok(serde_json::to_string(&SendMessageInput {
        queue_url,
        message_body: general_purpose::STANDARD.encode(payload),
        message_attributes,
    })?)
}

fn target_for_sqs_payload(
    queue_url: &str,
    execution_role_arn: &str,
    dlq_arn: Option<&str>,
    maximum_event_age_seconds: i32,
    maximum_retry_attempts: i32,
    payload: &[u8],
) -> Result<Target> {
    let input = sqs_send_message_input(queue_url, payload)?;
    let mut target = Target::builder()
        .arn(SQS_SEND_MESSAGE_TARGET_ARN)
        .role_arn(execution_role_arn)
        .input(input)
        .retry_policy(
            RetryPolicy::builder()
                .maximum_event_age_in_seconds(maximum_event_age_seconds)
                .maximum_retry_attempts(maximum_retry_attempts)
                .build(),
        );
    if let Some(dlq_arn) = dlq_arn {
        target = target.dead_letter_config(DeadLetterConfig::builder().arn(dlq_arn).build());
    }
    target.build().map_err(Into::into)
}

fn schedule_name(
    prefix: &str,
    namespace: &str,
    schedule_id: &str,
    revision: u64,
    fire_at_micros: i64,
) -> String {
    let raw = format!("{prefix}-{namespace}-{schedule_id}-{revision}-{fire_at_micros}");
    let mut sanitized = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        sanitized = DEFAULT_SCHEDULE_NAME_PREFIX.to_string();
    }
    sanitized.truncate(64);
    sanitized
}

fn parse_handle(handle: &str) -> Option<(&str, &str)> {
    let (group_name, name) = handle.split_once('/')?;
    if group_name.trim().is_empty() || name.trim().is_empty() || name.contains('/') {
        return None;
    }
    Some((group_name, name))
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn positive_i32(config_value: u32, env_name: &str, default_value: i32) -> i32 {
    if config_value > 0 {
        return config_value.min(i32::MAX as u32) as i32;
    }
    std::env::var(env_name)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

fn non_negative_i32(config_value: Option<u32>, env_name: &str, default_value: i32) -> i32 {
    if let Some(config_value) = config_value {
        return config_value.min(i32::MAX as u32) as i32;
    }
    std::env::var(env_name)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|value| *value >= 0)
        .unwrap_or(default_value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn schedule_names_are_scheduler_safe_and_bounded() {
        let name = schedule_name("talon", "conic:test/team", "nightly report!", 42, 1_234_567);

        assert!(name.len() <= 64);
        assert!(name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')));
        assert!(name.starts_with("talon-conic-test-team-nightly-report"));
    }

    #[test]
    fn handles_round_trip_group_and_name() {
        assert_eq!(
            parse_handle("default/talon-schedule"),
            Some(("default", "talon-schedule"))
        );
        assert_eq!(parse_handle("default/"), None);
        assert_eq!(parse_handle("/name"), None);
        assert_eq!(parse_handle("default/name/extra"), None);
    }

    #[test]
    fn sqs_input_matches_worker_pull_envelope() {
        let input = sqs_send_message_input(
            "https://sqs.us-east-1.amazonaws.com/123/talon",
            br#"{"kind":"schedule"}"#,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&input).unwrap();

        assert_eq!(
            value["QueueUrl"],
            "https://sqs.us-east-1.amazonaws.com/123/talon"
        );
        assert_eq!(
            value["MessageBody"],
            general_purpose::STANDARD.encode(br#"{"kind":"schedule"}"#)
        );
        assert_eq!(
            value["MessageAttributes"][TALON_TOPIC_ATTRIBUTE]["StringValue"],
            topics::SCHEDULE_FIRE_TOPIC
        );
    }

    #[test]
    fn target_uses_scheduler_universal_sqs_send_message() {
        let target = target_for_sqs_payload(
            "https://sqs.us-east-1.amazonaws.com/123/talon",
            "arn:aws:iam::123:role/scheduler",
            None,
            DEFAULT_MAX_EVENT_AGE_SECONDS,
            DEFAULT_MAX_RETRY_ATTEMPTS,
            b"payload",
        )
        .unwrap();

        assert_eq!(target.arn(), SQS_SEND_MESSAGE_TARGET_ARN);
        assert_eq!(target.role_arn(), "arn:aws:iam::123:role/scheduler");
        assert!(target
            .input()
            .unwrap()
            .contains(topics::SCHEDULE_FIRE_TOPIC));
    }

    #[test]
    fn retry_attempts_can_be_explicitly_disabled() {
        assert_eq!(
            non_negative_i32(Some(0), "TALON_AWS_SCHEDULER_MAX_RETRY_ATTEMPTS", 3),
            0
        );
    }
}
