// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

// Skills Platform - directory-based skill loading with SKILL.md metadata
// This file is owned by: Agent 5 (Skills & Built-in Tools)

pub mod loader;
pub mod namespace;
pub mod registry;

pub use loader::{Skill, SkillLoader};
pub use registry::{ToolDefinition, ToolRegistry, ToolSource};
