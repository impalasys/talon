// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use talon_client::gateway::ListResourcesRequest;
use talon_client::{data::SessionJournalEntryPayloadLlmResponse, harness::ChatResponse};

#[test]
fn generated_gateway_types_are_available() {
    let request = ListResourcesRequest {
        ns: "default".to_string(),
        kind: Some("Agent".to_string()),
        ..Default::default()
    };
    assert_eq!(request.ns, "default");
    assert_eq!(request.kind.as_deref(), Some("Agent"));
}

#[test]
fn generated_data_types_can_reference_harness_types() {
    let payload = SessionJournalEntryPayloadLlmResponse {
        response: Some(ChatResponse {
            content: "ok".to_string(),
            ..Default::default()
        }),
    };
    assert_eq!(payload.response.unwrap().content, "ok");
}
