// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use talon::control::{keys as legacy, ns as legacy_ns};
use talon_identifiers::{keys, ns};

#[test]
fn existing_import_paths_expose_the_same_types_as_the_leaf_crate() {
    let key: keys::ResourceKey = legacy::session("acme:prod", "agent/one", "session one");
    let parent: legacy::ResourceParent = key.as_parent();
    let list: keys::ResourceList = legacy::session_prefix("acme:prod", "agent/one");
    let segment: legacy::ResourceSegment = keys::ResourceSegment::new("Session", "session one");

    assert!(list.matches(&key));
    assert_eq!(parent.namespace, "acme:prod");
    assert_eq!(key.parent_segments().unwrap()[0].name, "agent/one");
    assert_eq!(segment.name, key.name);
    assert_eq!(legacy_ns::TALON_SYSTEM, ns::TALON_SYSTEM);
    assert_eq!(legacy_ns::DEFAULT, ns::DEFAULT);
    assert_eq!(legacy_ns::TALON_DOMAIN, ns::TALON_DOMAIN);
    assert_eq!(legacy_ns::ancestry("acme:prod"), ns::ancestry("acme:prod"));
}

#[test]
fn storage_keys_round_trip_across_the_existing_and_new_import_paths() {
    let expected = "@Namespace/acme:prod/Agent/agent%2Fone/@/Session/session%20one";
    let legacy_key = legacy::session("acme:prod", "agent/one", "session one");
    assert_eq!(legacy_key.canonical(), expected);
    let key: legacy::ResourceKey = keys::ResourceKey::parse_canonical(expected).unwrap();
    assert_eq!(key, legacy_key);
    assert_eq!(key.canonical(), expected);
    assert_eq!(
        legacy::namespace_metadata("acme").namespace,
        ns::TALON_SYSTEM
    );
    assert!(keys::ResourceKey::parse_canonical("Namespace/acme/Agent/a").is_err());
}
