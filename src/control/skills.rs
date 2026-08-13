// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Shared invariants for namespace Skill resources and their File packages.

use crate::gateway::rpc::resources_proto;
use anyhow::{bail, Result};

pub const SKILLS_ROOT: &str = "/skills";
pub const SKILL_ENTRYPOINT: &str = "SKILL.md";

/// Validate the portable, path-safe Skill identifier format.
pub fn validate_skill_id(name: &str) -> Result<()> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 || !bytes[0].is_ascii_lowercase() {
        bail!("Skill metadata.name must be a lowercase slug up to 63 characters");
    }
    if !bytes
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        || bytes.last() == Some(&b'-')
    {
        bail!("Skill metadata.name must contain only lowercase letters, digits, and hyphens");
    }
    Ok(())
}

pub fn package_root(skill_id: &str) -> String {
    format!("{SKILLS_ROOT}/{skill_id}")
}

pub fn entrypoint_path(skill_id: &str) -> String {
    format!("{}/{SKILL_ENTRYPOINT}", package_root(skill_id))
}

pub fn validate_skill_file_spec(spec: &resources_proto::FileSpec) -> Result<()> {
    let is_skill = spec.purpose == resources_proto::FilePurpose::Skill as i32;
    let in_skills_root = spec.path == SKILLS_ROOT || spec.path.starts_with("/skills/");
    if in_skills_root && !is_skill {
        bail!("Files under /skills must use purpose SKILL");
    }
    if !is_skill {
        return Ok(());
    }
    let Some(relative) = spec.path.strip_prefix("/skills/") else {
        bail!("Skill Files must live under /skills/<skill-id>/");
    };
    let Some((skill_id, remainder)) = relative.split_once('/') else {
        bail!("Skill Files must live under /skills/<skill-id>/");
    };
    validate_skill_id(skill_id)?;
    if remainder.is_empty() {
        bail!("Skill File path must name a package file");
    }
    if spec.index_policy != resources_proto::FileIndexPolicy::None as i32 {
        bail!("Skill Files must use indexPolicy NONE");
    }
    if spec.retention != resources_proto::FileRetention::Retained as i32 {
        bail!("Skill Files must use retention RETAINED");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill_file(path: &str) -> resources_proto::FileSpec {
        resources_proto::FileSpec {
            path: path.to_string(),
            purpose: resources_proto::FilePurpose::Skill as i32,
            index_policy: resources_proto::FileIndexPolicy::None as i32,
            retention: resources_proto::FileRetention::Retained as i32,
            ..Default::default()
        }
    }

    #[test]
    fn accepts_safe_skill_ids_and_package_files() {
        validate_skill_id("review-code-2").unwrap();
        validate_skill_file_spec(&skill_file("/skills/review-code-2/SKILL.md")).unwrap();
        validate_skill_file_spec(&skill_file("/skills/review-code-2/references/checklist.md"))
            .unwrap();
    }

    #[test]
    fn rejects_unsafe_skill_ids_and_wrong_package_purposes() {
        for invalid in ["", "Review", "review_1", "-review", "review-"] {
            assert!(
                validate_skill_id(invalid).is_err(),
                "{invalid} should be rejected"
            );
        }

        let mut ordinary_file = skill_file("/docs/readme.md");
        ordinary_file.purpose = resources_proto::FilePurpose::Artifact as i32;
        assert!(validate_skill_file_spec(&ordinary_file).is_ok());

        ordinary_file.path = "/skills/review/SKILL.md".to_string();
        assert!(validate_skill_file_spec(&ordinary_file).is_err());
    }

    #[test]
    fn skill_files_require_declared_package_policy() {
        let mut file = skill_file("/skills/review/SKILL.md");
        file.index_policy = resources_proto::FileIndexPolicy::Search as i32;
        assert!(validate_skill_file_spec(&file).is_err());

        file.index_policy = resources_proto::FileIndexPolicy::None as i32;
        file.retention = resources_proto::FileRetention::Unspecified as i32;
        assert!(validate_skill_file_spec(&file).is_err());
    }
}
