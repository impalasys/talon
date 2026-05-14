// Skills Platform - directory-based skill loading with SKILL.md metadata
// This file is owned by: Agent 5 (Skills & Built-in Tools)

pub mod loader;
pub mod registry;

pub use loader::{Skill, SkillLoader};
pub use registry::{ToolDefinition, ToolRegistry, ToolSource};
