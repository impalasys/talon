// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub const SESSION_DISPATCH_TOPIC: &str = "talon.session.dispatch";
pub const SESSION_CONTROL_TOPIC: &str = "talon.session.control";
pub const RESOURCE_LIFECYCLE_TOPIC: &str = "talon.resource.lifecycle";
pub const SESSION_STEPS_TOPIC_PREFIX: &str = "talon.session.steps";
pub const DEFAULT_SESSION_STEP_SHARDS: u32 = 32;

pub fn session_step_shard_count() -> u32 {
    std::env::var("TALON_SESSION_STEP_SHARDS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|count| *count > 0)
        .unwrap_or(DEFAULT_SESSION_STEP_SHARDS)
}

pub fn session_step_shard(session_id: &str) -> u32 {
    let shard_count = session_step_shard_count();
    let mut hasher = DefaultHasher::new();
    session_id.hash(&mut hasher);
    (hasher.finish() % shard_count as u64) as u32
}

pub fn session_step_topic_for_shard(shard: u32) -> String {
    format!("{}.{}", SESSION_STEPS_TOPIC_PREFIX, shard)
}

pub fn session_step_topic_for_session(session_id: &str) -> String {
    session_step_topic_for_shard(session_step_shard(session_id))
}
