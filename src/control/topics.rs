// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

pub const SESSION_DISPATCH_TOPIC: &str = "talon.session.dispatch";
pub const SESSION_CONTROL_TOPIC: &str = "talon.session.control";
pub const RESOURCE_LIFECYCLE_TOPIC: &str = "talon.resource.lifecycle";
pub const SESSION_PARTS_TOPIC_PREFIX: &str = "talon.session.parts";
pub const DEFAULT_SESSION_PART_SHARDS: u32 = 32;

pub fn session_part_shard_count() -> u32 {
    static CACHE: OnceLock<u32> = OnceLock::new();

    *CACHE.get_or_init(|| {
        std::env::var("TALON_SESSION_PART_SHARDS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|count| *count > 0)
            .unwrap_or(DEFAULT_SESSION_PART_SHARDS)
    })
}

pub fn session_part_shard(session_id: &str) -> u32 {
    let shard_count = session_part_shard_count();
    let mut hasher = DefaultHasher::new();
    session_id.hash(&mut hasher);
    (hasher.finish() % shard_count as u64) as u32
}

pub fn session_part_topic_for_shard(shard: u32) -> String {
    format!("{}.{}", SESSION_PARTS_TOPIC_PREFIX, shard)
}

pub fn session_part_topic_for_session(session_id: &str) -> String {
    session_part_topic_for_shard(session_part_shard(session_id))
}
