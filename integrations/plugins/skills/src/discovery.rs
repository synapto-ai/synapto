use crate::model::Skill;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn discover_skills(project_root: &Path) -> HashMap<String, Skill> {
    let mut skills = HashMap::new();

    // Scan in reverse order of precedence so higher precedence overwrites lower precedence.
    // 4. User-level Neutral: ~/.agents/skills/*/SKILL.md
    if let Some(user_agents) = resolve_user_agents_dir() {
        scan_skills_directory(&user_agents, &mut skills);
    }

    // 3. User-level Claude: ~/.claude/skills/*/SKILL.md or $CLAUDE_CONFIG_DIR/skills/*/SKILL.md
    if let Some(user_claude) = resolve_user_claude_dir() {
        scan_skills_directory(&user_claude, &mut skills);
    }

    // 2. Project-level Neutral: <project_root>/.agents/skills/*/SKILL.md
    let project_agents = project_root.join(".agents").join("skills");
    scan_skills_directory(&project_agents, &mut skills);

    // 1. Project-level Claude: <project_root>/.claude/skills/*/SKILL.md
    let project_claude = project_root.join(".claude").join("skills");
    scan_skills_directory(&project_claude, &mut skills);

    skills
}

fn resolve_user_claude_dir() -> Option<PathBuf> {
    if let Ok(claude_config) = std::env::var("CLAUDE_CONFIG_DIR")
        && !claude_config.trim().is_empty()
    {
        return Some(PathBuf::from(claude_config).join("skills"));
    }

    dirs::home_dir().map(|home| home.join(".claude").join("skills"))
}

fn resolve_user_agents_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".agents").join("skills"))
}

pub fn scan_skills_directory(skills_dir: &Path, target: &mut HashMap<String, Skill>) {
    if !skills_dir.is_dir() {
        return;
    }

    let entries = match std::fs::read_dir(skills_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(
                "Failed to read skills directory {}: {}",
                skills_dir.display(),
                e
            );
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.is_file() {
                match Skill::from_file(&skill_md) {
                    Ok(skill) => {
                        target.insert(skill.name.clone(), skill);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse skill at {}: {}", skill_md.display(), e);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};

    #[test]
    fn test_precedence_project_claude_over_project_agents() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();

        let agents_skill = root.join(".agents").join("skills").join("test-skill");
        create_dir_all(&agents_skill).unwrap();
        write(
            agents_skill.join("SKILL.md"),
            "---\nname: test-skill\ndescription: From agents\n---\nAgents body",
        )
        .unwrap();

        let claude_skill = root.join(".claude").join("skills").join("test-skill");
        create_dir_all(&claude_skill).unwrap();
        write(
            claude_skill.join("SKILL.md"),
            "---\nname: test-skill\ndescription: From claude\n---\nClaude body",
        )
        .unwrap();

        let mut discovered = HashMap::new();
        scan_skills_directory(&root.join(".agents").join("skills"), &mut discovered);
        scan_skills_directory(&root.join(".claude").join("skills"), &mut discovered);

        let resolved = discovered.get("test-skill").unwrap();
        assert_eq!(resolved.description, "From claude");
    }
}
