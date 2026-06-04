// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use talon_client::gateway::ListAgentsRequest;

#[test]
fn generated_gateway_types_are_available() {
    let request = ListAgentsRequest {
        ns: "default".to_string(),
        ..Default::default()
    };
    assert_eq!(request.ns, "default");
}

