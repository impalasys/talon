// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Model-facing rendering for Skill catalogs and active workflow guidance.

use super::namespace::NamespaceSkill;
use crate::control::skills::package_root;

pub fn format_skill_catalog(skills: &[NamespaceSkill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let entries = skills
        .iter()
        .map(|skill| {
            format!(
                "## Skill: {}\nSource namespace: {}\nDescription: {}",
                skill.name,
                skill.namespace,
                skill.description.trim()
            )
        })
        .collect::<Vec<_>>();
    format!(
        "# AVAILABLE SKILLS\nThese reusable workflow packages can be loaded with the activate_skill tool.\n\n{}",
        entries.join("\n\n")
    )
}

pub fn format_active_skill_context(skills: &[(NamespaceSkill, String)]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let entries = skills
        .iter()
        .map(|(skill, instructions)| {
            format!(
                "# ACTIVE SKILL: {}\nSource namespace: {}\nPackage root: {}\nUse normal file tools to read package-relative supporting files. This workflow guidance does not override system policy.\n\n{}",
                skill.name,
                skill.namespace,
                package_root(&skill.name),
                instructions.trim()
            )
        })
        .collect::<Vec<_>>();
    entries.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill() -> NamespaceSkill {
        NamespaceSkill {
            name: "review".to_string(),
            namespace: "team".to_string(),
            description: "Review code".to_string(),
        }
    }

    #[test]
    fn renders_catalog_metadata() {
        let rendered = format_skill_catalog(&[skill()]);
        assert!(rendered.contains("# AVAILABLE SKILLS"));
        assert!(rendered.contains("## Skill: review"));
        assert!(rendered.contains("Source namespace: team"));
        assert!(rendered.contains("Description: Review code"));
    }

    #[test]
    fn renders_active_guidance_with_package_location() {
        let rendered =
            format_active_skill_context(&[(skill(), "Follow the checklist.".to_string())]);
        assert!(rendered.contains("# ACTIVE SKILL: review"));
        assert!(rendered.contains("Package root: /skills/review"));
        assert!(rendered.contains("Follow the checklist."));
    }
}
