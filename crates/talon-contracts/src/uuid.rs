// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

/// Generate a canonical UUIDv7 string.
///
/// Talon uses UUIDv7 for IDs that are also storage keys and therefore must
/// preserve chronological ordering under lexicographic key scans. Do not add
/// semantic prefixes, suffixes, hashes, or role names to these IDs.
pub fn v7() -> String {
    uuid::Uuid::now_v7().to_string()
}

pub fn session_id() -> String {
    v7()
}

pub fn session_message_id() -> String {
    v7()
}

pub fn channel_message_id() -> String {
    v7()
}

pub fn session_submission_id() -> String {
    v7()
}

pub fn session_attempt_id() -> String {
    v7()
}

pub fn resource_version() -> String {
    v7()
}

pub fn event_id() -> String {
    v7()
}

pub fn scheduler_handle() -> String {
    v7()
}

pub fn worker_id() -> String {
    v7()
}

pub fn process_id() -> String {
    v7()
}

pub fn request_id() -> String {
    v7()
}

pub fn auth_record_id() -> String {
    v7()
}

pub fn unique_name(prefix: &str) -> String {
    format!("{}-{}", prefix, v7())
}

#[cfg(test)]
mod tests {
    #[test]
    fn session_message_ids_are_canonical_uuid_v7() {
        let id = super::session_message_id();
        let parsed = uuid::Uuid::parse_str(&id).expect("id should parse as UUID");

        assert_eq!(parsed.get_version_num(), 7);
        assert_eq!(id, parsed.to_string());
    }

    #[test]
    fn generated_v7_ids_sort_in_creation_order_within_process() {
        let first = super::v7();
        let second = super::v7();

        assert!(first < second);
    }
}
