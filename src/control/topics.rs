// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub const SESSION_DISPATCH_TOPIC: &str = "talon.session.dispatch";
pub const SESSION_CONTROL_TOPIC: &str = "talon.session.control";
pub const WORKFLOW_DISPATCH_TOPIC: &str = "talon.workflow.dispatch";
pub const RESOURCE_LIFECYCLE_TOPIC: &str = "talon.resource.lifecycle";
pub const INDEX_EVENTS_TOPIC: &str = "talon.index.events";
pub const SCHEDULE_FIRE_TOPIC: &str = "talon.schedule.fire";
pub const CHANNEL_EVENTS_TOPIC_PREFIX: &str = "talon.channel.events";

pub fn channel_events_topic(ns: &str, channel: &str) -> String {
    format!(
        "{}.{}.{}",
        CHANNEL_EVENTS_TOPIC_PREFIX,
        pubsub_topic_segment(ns),
        pubsub_topic_segment(channel)
    )
}

fn pubsub_topic_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == '~' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_events_topic_uses_pubsub_safe_segments() {
        assert_eq!(
            channel_events_topic("Tenant:acme:child", "slack/#general"),
            "talon.channel.events.Tenant-acme-child.slack--general"
        );
    }
}
