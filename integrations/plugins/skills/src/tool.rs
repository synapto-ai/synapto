use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use synapto_interface::context::ContextRequest;
use synapto_interface::llm::LLMSafe;
use synapto_interface::tool::Tool;

use crate::model::Skill;

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct LoadSkillArgs {
    /// The unique name of the skill to load (e.g. "format-pr").
    pub skill_name: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct LoadSkillOutput {
    pub skill_name: String,
    pub instructions: String,
    pub scripts: Vec<String>,
    pub references: Vec<String>,
    pub assets: Vec<String>,
}

#[derive(Clone)]
pub struct LoadSkillTool {
    skills: Arc<HashMap<String, Skill>>,
}

impl LoadSkillTool {
    pub fn new(skills: Arc<HashMap<String, Skill>>) -> Self {
        Self { skills }
    }
}

#[async_trait]
impl Tool for LoadSkillTool {
    type Arguments = LoadSkillArgs;
    const NAME: &'static str = "load_skill";
    const DESCRIPTION: &'static str =
        "Loads the detailed instructions and lists bundled helper files for a specialized skill.";

    async fn is_available(
        &self,
        _ctx_request: &ContextRequest,
        compiled_context: &serde_json::Value,
    ) -> Result<bool, String> {
        // If a skill is already auto-activated, hide the load_skill tool for this turn.
        let has_active_skill = compiled_context
            .get("active_skill")
            .or_else(|| {
                compiled_context
                    .get("skills")
                    .and_then(|s| s.get("active_skill"))
            })
            .is_some_and(|v| !v.is_null());

        if has_active_skill {
            return Ok(false);
        }

        // Expose load_skill only when there are candidate skills in context.
        let has_available = compiled_context
            .get("available_skills")
            .or_else(|| {
                compiled_context
                    .get("skills")
                    .and_then(|s| s.get("available_skills"))
            })
            .and_then(|arr| arr.as_array())
            .is_some_and(|arr| !arr.is_empty());

        Ok(has_available)
    }

    async fn execute(
        &self,
        _ctx_request: &ContextRequest,
        args: Self::Arguments,
    ) -> Result<serde_json::Value, String> {
        // Path traversal guard: reject separators and traversal sequences.
        if args.skill_name.contains('/')
            || args.skill_name.contains('\\')
            || args.skill_name.contains("..")
        {
            return Err("Invalid skill name: Path traversal sequences are forbidden".to_string());
        }

        let skill = match self.skills.get(&args.skill_name) {
            Some(s) => s,
            None => {
                let mut available: Vec<_> = self.skills.keys().cloned().collect();
                available.sort();
                return Err(format!(
                    "Skill '{}' not found. Available skills: [{}]",
                    args.skill_name,
                    available.join(", ")
                ));
            }
        };

        let instructions = skill.load_instructions().await?;

        let scripts = collect_files_recursive(&skill.path.join("scripts"));
        let references = collect_files_recursive(&skill.path.join("references"));
        let assets = collect_files_recursive(&skill.path.join("assets"));

        let output = LoadSkillOutput {
            skill_name: skill.name.clone(),
            instructions,
            scripts,
            references,
            assets,
        };

        serde_json::to_value(output).map_err(|e| format!("Serialization error: {}", e))
    }
}

pub fn collect_files_recursive(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_files_inner(dir, &mut files);
    files.sort();
    files
}

fn collect_files_inner(dir: &Path, acc: &mut Vec<String>) {
    if !dir.is_dir() {
        return;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let path_str = path
                    .canonicalize()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());
                acc.push(path_str);
            } else if path.is_dir() {
                collect_files_inner(&path, acc);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};

    #[tokio::test]
    async fn test_load_skill_success() {
        let temp_dir = tempfile::tempdir().unwrap();
        let skill_dir = temp_dir.path().join("my-skill");
        create_dir_all(&skill_dir).unwrap();

        let scripts_dir = skill_dir.join("scripts");
        create_dir_all(&scripts_dir).unwrap();
        write(scripts_dir.join("run.sh"), "#!/bin/sh\necho hi").unwrap();

        let references_dir = skill_dir.join("references");
        create_dir_all(&references_dir).unwrap();
        write(references_dir.join("doc.md"), "Reference doc").unwrap();

        let skill_md = skill_dir.join("SKILL.md");
        write(
            &skill_md,
            "---\nname: my-skill\ndescription: Test skill\n---\n## Instructions\nDo this.",
        )
        .unwrap();

        let skill = Skill::from_file(&skill_md).unwrap();
        let mut map = HashMap::new();
        map.insert(skill.name.clone(), skill);

        let tool = LoadSkillTool::new(Arc::new(map));
        let ctx = ContextRequest::default();
        let val = tool
            .execute(
                &ctx,
                LoadSkillArgs {
                    skill_name: "my-skill".to_string(),
                },
            )
            .await
            .unwrap();

        let output: LoadSkillOutput = serde_json::from_value(val).unwrap();
        assert_eq!(output.skill_name, "my-skill");
        assert_eq!(output.instructions, "## Instructions\nDo this.");
        assert_eq!(output.scripts.len(), 1);
        assert_eq!(output.references.len(), 1);
    }

    #[tokio::test]
    async fn test_load_skill_traversal_rejected() {
        let tool = LoadSkillTool::new(Arc::new(HashMap::new()));
        let ctx = ContextRequest::default();
        let res = tool
            .execute(
                &ctx,
                LoadSkillArgs {
                    skill_name: "../evil".to_string(),
                },
            )
            .await;

        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Path traversal"));
    }
}
