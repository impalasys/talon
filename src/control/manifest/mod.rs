// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

include!("meta.rs");
include!("agent.rs");
include!("mcp.rs");
include!("core_resources.rs");
include!("workflows.rs");
include!("resource.rs");
include!("legacy.rs");
include!("meta_impls.rs");
include!("mcp_impls.rs");
include!("workflow_impls.rs");
include!("yaml.rs");
include!("agent_impls.rs");
pub mod templating;
#[cfg(test)]
include!("tests.rs");
