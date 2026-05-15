// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

// Skill directory scanner and SKILL.md parser
use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
}

pub struct SkillLoader {
    pub workspace_dir: PathBuf,
}

impl SkillLoader {
    pub fn new(workspace_dir: impl Into<PathBuf>) -> Self {
        Self {
            workspace_dir: workspace_dir.into(),
        }
    }

    pub async fn scan(&self) -> Result<Vec<Skill>> {
        let skills_dir = self.workspace_dir.join("skills");
        if !skills_dir.exists() {
            return Ok(vec![]);
        }

        let mut skills = Vec::new();
        let mut entries = fs::read_dir(skills_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                let skill_md_path = path.join("SKILL.md");
                if skill_md_path.exists() {
                    match self.parse_skill_file(&skill_md_path).await {
                        Ok(skill) => skills.push(skill),
                        Err(e) => {
                            eprintln!(
                                "Warning: Failed to parse skill at {}: {}",
                                skill_md_path.display(),
                                e
                            );
                        }
                    }
                }
            }
        }

        Ok(skills)
    }

    async fn parse_skill_file(&self, path: &Path) -> Result<Skill> {
        let content = fs::read_to_string(path).await?;

        if !content.starts_with("---") {
            return Err(anyhow!("Missing frontmatter marker (---) at start of file"));
        }

        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() < 3 {
            return Err(anyhow!(
                "Malformed frontmatter: could not find closing --- marker"
            ));
        }

        let yaml_str = parts[1];
        let body = parts[2].trim();

        let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml_str)
            .map_err(|e| anyhow!("Failed to parse YAML frontmatter: {}", e))?;

        Ok(Skill {
            name: frontmatter.name,
            description: frontmatter.description,
            instructions: body.to_string(),
            path: path.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs;

    #[tokio::test]
    async fn test_scan_empty_directory() {
        let dir = tempdir().unwrap();
        let loader = SkillLoader::new(dir.path());
        let skills = loader.scan().await.unwrap();
        assert!(skills.is_empty());
    }

    #[tokio::test]
    async fn test_scan_valid_skill() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join("skills");
        let my_skill_dir = skills_dir.join("my-skill");
        fs::create_dir_all(&my_skill_dir).await.unwrap();

        let skill_content = r#"---
name: my-skill
description: A test skill
---
Do some test stuff."#;
        fs::write(my_skill_dir.join("SKILL.md"), skill_content)
            .await
            .unwrap();

        let loader = SkillLoader::new(dir.path());
        let skills = loader.scan().await.unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "my-skill");
        assert_eq!(skills[0].description, "A test skill");
        assert_eq!(skills[0].instructions, "Do some test stuff.");
    }

    #[tokio::test]
    async fn test_scan_malformed_skill() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join("skills");
        let bad_skill_dir = skills_dir.join("bad-skill");
        fs::create_dir_all(&bad_skill_dir).await.unwrap();

        // Missing frontmatter
        fs::write(bad_skill_dir.join("SKILL.md"), "just some markdown")
            .await
            .unwrap();

        let loader = SkillLoader::new(dir.path());
        let skills = loader.scan().await.unwrap();
        assert!(skills.is_empty());
    }

    #[tokio::test]
    async fn test_scan_multiple_skills() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join("skills");
        fs::create_dir_all(&skills_dir).await.unwrap();

        for i in 1..=3 {
            let skill_dir = skills_dir.join(format!("skill-{}", i));
            fs::create_dir_all(&skill_dir).await.unwrap();
            let content = format!(
                r#"---
name: skill-{}
description: description-{}
---
body-{}"#,
                i, i, i
            );
            fs::write(skill_dir.join("SKILL.md"), content)
                .await
                .unwrap();
        }

        let loader = SkillLoader::new(dir.path());
        let skills = loader.scan().await.unwrap();
        assert_eq!(skills.len(), 3);
    }
}
