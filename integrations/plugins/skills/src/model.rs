use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SkillFrontmatter {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_true", rename = "user-invocable")]
    pub user_invocable: bool,
    #[serde(default, rename = "allowed-tools")]
    pub allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub user_invocable: bool,
    pub allowed_tools: Option<Vec<String>>,
    pub path: PathBuf,
    pub skill_md_path: PathBuf,
}

impl Skill {
    pub fn from_file(skill_md_path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(skill_md_path)
            .map_err(|e| format!("Failed to read {}: {}", skill_md_path.display(), e))?;

        let parent_dir = skill_md_path
            .parent()
            .ok_or_else(|| format!("Invalid parent directory for {}", skill_md_path.display()))?;

        let dir_name = parent_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown-skill");

        let (frontmatter, _) = parse_skill_md(&content, dir_name)?;

        let name = frontmatter.name.unwrap_or_else(|| dir_name.to_string());
        let description = frontmatter.description.unwrap_or_default();

        Ok(Self {
            name,
            description,
            user_invocable: frontmatter.user_invocable,
            allowed_tools: frontmatter.allowed_tools,
            path: parent_dir.to_path_buf(),
            skill_md_path: skill_md_path.to_path_buf(),
        })
    }

    pub async fn load_instructions(&self) -> Result<String, String> {
        let content = tokio::fs::read_to_string(&self.skill_md_path)
            .await
            .map_err(|e| format!("Failed to read {}: {}", self.skill_md_path.display(), e))?;
        let (_, instructions) = parse_skill_md(&content, &self.name)?;
        Ok(instructions)
    }
}

pub fn parse_skill_md(
    content: &str,
    default_name: &str,
) -> Result<(SkillFrontmatter, String), String> {
    let normalized = content.replace("\r\n", "\n");
    let trimmed = normalized.trim_start();

    if !trimmed.starts_with("---") {
        let frontmatter = SkillFrontmatter {
            name: Some(default_name.to_string()),
            description: None,
            user_invocable: true,
            allowed_tools: None,
        };
        return Ok((frontmatter, normalized.trim().to_string()));
    }

    // Must be preceded by "---" on the first line
    let lines: Vec<&str> = normalized.lines().collect();
    if lines.is_empty() || lines[0].trim() != "---" {
        let frontmatter = SkillFrontmatter {
            name: Some(default_name.to_string()),
            description: None,
            user_invocable: true,
            allowed_tools: None,
        };
        return Ok((frontmatter, normalized.trim().to_string()));
    }

    let mut closing_index = None;
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            closing_index = Some(i);
            break;
        }
    }

    let closing = match closing_index {
        Some(idx) => idx,
        None => return Err("Missing closing frontmatter delimiter '---'".to_string()),
    };

    let yaml_slice = lines[1..closing].join("\n");
    let instructions = lines[(closing + 1)..].join("\n").trim().to_string();

    let frontmatter: SkillFrontmatter = if yaml_slice.trim().is_empty() {
        SkillFrontmatter::default()
    } else {
        serde_saphyr::from_str(&yaml_slice)
            .map_err(|e| format!("Failed to parse YAML frontmatter: {}", e))?
    };

    Ok((frontmatter, instructions))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_frontmatter() {
        let text = r#"---
name: format-pr
description: Formats git commit history.
user-invocable: false
allowed-tools: ["bash", "read_file"]
---

## Instructions
Run git log.
"#;
        let (fm, body) = parse_skill_md(text, "default-name").unwrap();
        assert_eq!(fm.name, Some("format-pr".to_string()));
        assert_eq!(
            fm.description,
            Some("Formats git commit history.".to_string())
        );
        assert!(!fm.user_invocable);
        assert_eq!(
            fm.allowed_tools,
            Some(vec!["bash".to_string(), "read_file".to_string()])
        );
        assert_eq!(body, "## Instructions\nRun git log.");
    }

    #[test]
    fn test_parse_missing_frontmatter() {
        let text = "Just raw markdown content\nLine 2";
        let (fm, body) = parse_skill_md(text, "fallback-dir").unwrap();
        assert_eq!(fm.name, Some("fallback-dir".to_string()));
        assert_eq!(fm.description, None);
        assert!(fm.user_invocable);
        assert_eq!(body, "Just raw markdown content\nLine 2");
    }

    #[test]
    fn test_parse_unclosed_frontmatter() {
        let text = "---\nname: unclosed\n";
        let res = parse_skill_md(text, "test");
        assert!(res.is_err());
    }

    #[test]
    fn test_parse_default_name_when_omitted_in_yaml() {
        let text = "---\ndescription: Has no name field.\n---\nBody here.";
        let (fm, body) = parse_skill_md(text, "inferred-name").unwrap();
        assert_eq!(fm.name, None);
        assert_eq!(fm.description, Some("Has no name field.".to_string()));
        assert_eq!(body, "Body here.");
    }
}
