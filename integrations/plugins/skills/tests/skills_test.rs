use async_trait::async_trait;
use std::collections::{BTreeMap, HashMap};
use std::fs::{create_dir_all, write};
use std::sync::Arc;
use synapto_interface::context::{ContextInteraction, ContextProvider, ContextRequest};
use synapto_interface::decision::{
    DecisionAnswer, DecisionHandle, DecisionQuestion, RawDecisionExecutor,
};
use synapto_interface::tool::Tool;
use synapto_plugin_skills::{
    LoadSkillArgs, LoadSkillOutput, LoadSkillTool, Skill, SkillsContextProvider,
    discovery::scan_skills_directory,
};

struct MockDecisionExecutor {
    choice: String,
    confidence: f64,
}

#[async_trait]
impl RawDecisionExecutor for MockDecisionExecutor {
    async fn evaluate_raw(
        &self,
        _model: Option<&str>,
        _state: serde_json::Value,
        _questions: BTreeMap<String, DecisionQuestion>,
    ) -> Result<BTreeMap<String, DecisionAnswer>, String> {
        let mut answers = BTreeMap::new();
        let mut probabilities = BTreeMap::new();
        probabilities.insert(self.choice.clone(), self.confidence);
        answers.insert(
            "skill_routing".to_string(),
            DecisionAnswer::Choice {
                choice: self.choice.clone(),
                probabilities,
                confidence: self.confidence,
            },
        );
        Ok(answers)
    }
}

#[tokio::test]
async fn test_skills_context_filters_empty_descriptions() {
    let temp_dir = tempfile::tempdir().unwrap();

    let skill1_md = temp_dir.path().join("skill1").join("SKILL.md");
    create_dir_all(skill1_md.parent().unwrap()).unwrap();
    write(
        &skill1_md,
        "---\nname: skill1\ndescription: Valid skill\n---\nBody",
    )
    .unwrap();

    let skill2_md = temp_dir.path().join("skill2").join("SKILL.md");
    create_dir_all(skill2_md.parent().unwrap()).unwrap();
    write(&skill2_md, "---\nname: skill2\n---\nNo description here").unwrap();

    let skill1 = Skill::from_file(&skill1_md).unwrap();
    let skill2 = Skill::from_file(&skill2_md).unwrap();

    let mut map = HashMap::new();
    map.insert(skill1.name.clone(), skill1);
    map.insert(skill2.name.clone(), skill2);

    let provider =
        SkillsContextProvider::new(Arc::new(map.clone()), DecisionHandle::empty(), 0.50, 0.85);
    let ctx = provider.context(&ContextRequest::default()).await.unwrap();

    // skill1 should be in the index, skill2 should be excluded because description is empty
    assert_eq!(ctx.available_skills.len(), 1);
    assert_eq!(ctx.available_skills[0].name, "skill1");
    assert_eq!(ctx.available_skills[0].description, "Valid skill");
    assert!(ctx.active_skill.is_none());

    // Both skills should still be loadable by tool
    let tool = LoadSkillTool::new(Arc::new(map));
    let val1 = tool
        .execute(
            &ContextRequest::default(),
            LoadSkillArgs {
                skill_name: "skill1".to_string(),
            },
        )
        .await
        .unwrap();
    let out1: LoadSkillOutput = serde_json::from_value(val1).unwrap();
    assert_eq!(out1.instructions, "Body");

    let val2 = tool
        .execute(
            &ContextRequest::default(),
            LoadSkillArgs {
                skill_name: "skill2".to_string(),
            },
        )
        .await
        .unwrap();
    let out2: LoadSkillOutput = serde_json::from_value(val2).unwrap();
    assert_eq!(out2.instructions, "No description here");
}

#[tokio::test]
async fn test_auto_activation_high_confidence() {
    let temp_dir = tempfile::tempdir().unwrap();

    let skill1_md = temp_dir.path().join("format-pr").join("SKILL.md");
    create_dir_all(skill1_md.parent().unwrap()).unwrap();
    write(
        &skill1_md,
        "---\nname: format-pr\ndescription: Formats git PR\n---\n## Instructions\nFormat PR cleanly.",
    )
    .unwrap();

    let skill2_md = temp_dir.path().join("run-tests").join("SKILL.md");
    create_dir_all(skill2_md.parent().unwrap()).unwrap();
    write(
        &skill2_md,
        "---\nname: run-tests\ndescription: Runs automated tests\n---\n## Instructions\nRun tests.",
    )
    .unwrap();

    let skill1 = Skill::from_file(&skill1_md).unwrap();
    let skill2 = Skill::from_file(&skill2_md).unwrap();

    let mut map = HashMap::new();
    map.insert(skill1.name.clone(), skill1);
    map.insert(skill2.name.clone(), skill2);

    // Mock decision provider with 0.95 confidence (>= 0.85 activation threshold)
    let decision_handle = DecisionHandle::empty();
    decision_handle.set_backend(MockDecisionExecutor {
        choice: "format-pr".to_string(),
        confidence: 0.95,
    });

    let provider = SkillsContextProvider::new(Arc::new(map.clone()), decision_handle, 0.50, 0.85);

    let req = ContextRequest {
        recent_interactions: vec![ContextInteraction {
            peer_input: Some("Please format my PR".to_string()),
            cognitive_reasoning: None,
            cognitive_output: None,
        }],
        initial_run: false,
    };

    let ctx = provider.context(&req).await.unwrap();

    // Auto-activated directly into active_skill
    assert!(ctx.active_skill.is_some());
    let active = ctx.active_skill.unwrap();
    assert_eq!(active.name, "format-pr");
    assert_eq!(
        active.instructions,
        "## Instructions\nFormat PR cleanly."
    );

    // available_skills does not redundantly repeat the auto-activated skill
    assert!(ctx.available_skills.is_empty());
}

#[tokio::test]
async fn test_candidate_filtering_medium_confidence() {
    let temp_dir = tempfile::tempdir().unwrap();

    let skill1_md = temp_dir.path().join("format-pr").join("SKILL.md");
    create_dir_all(skill1_md.parent().unwrap()).unwrap();
    write(
        &skill1_md,
        "---\nname: format-pr\ndescription: Formats git PR\n---\nBody 1",
    )
    .unwrap();

    let skill2_md = temp_dir.path().join("run-tests").join("SKILL.md");
    create_dir_all(skill2_md.parent().unwrap()).unwrap();
    write(
        &skill2_md,
        "---\nname: run-tests\ndescription: Runs automated tests\n---\nBody 2",
    )
    .unwrap();

    let skill1 = Skill::from_file(&skill1_md).unwrap();
    let skill2 = Skill::from_file(&skill2_md).unwrap();

    let mut map = HashMap::new();
    map.insert(skill1.name.clone(), skill1);
    map.insert(skill2.name.clone(), skill2);

    // Mock decision provider with 0.65 confidence (>= 0.50 candidate, but < 0.85 activation)
    let decision_handle = DecisionHandle::empty();
    decision_handle.set_backend(MockDecisionExecutor {
        choice: "format-pr".to_string(),
        confidence: 0.65,
    });

    let provider = SkillsContextProvider::new(Arc::new(map.clone()), decision_handle, 0.50, 0.85);

    let req = ContextRequest {
        recent_interactions: vec![ContextInteraction {
            peer_input: Some("Maybe check my commits".to_string()),
            cognitive_reasoning: None,
            cognitive_output: None,
        }],
        initial_run: false,
    };

    let ctx = provider.context(&req).await.unwrap();

    // NOT auto-activated
    assert!(ctx.active_skill.is_none());

    // But candidate index is filtered down to format-pr
    assert_eq!(ctx.available_skills.len(), 1);
    assert_eq!(ctx.available_skills[0].name, "format-pr");
}

#[tokio::test]
async fn test_state_locked_tool_availability() {
    let tool = LoadSkillTool::new(Arc::new(HashMap::new()));
    let req = ContextRequest::default();

    // 1. When a skill is already auto-activated, load_skill must be hidden
    let context_active = serde_json::json!({
        "skills": {
            "available_skills": [{ "name": "format-pr", "description": "Formats PR" }],
            "active_skill": { "name": "format-pr", "instructions": "..." }
        }
    });
    assert!(!tool.is_available(&req, &context_active).await.unwrap());

    // 2. When active_skill is absent and available_skills has candidates, load_skill is exposed
    let context_candidates = serde_json::json!({
        "skills": {
            "available_skills": [{ "name": "format-pr", "description": "Formats PR" }],
            "active_skill": null
        }
    });
    assert!(tool.is_available(&req, &context_candidates).await.unwrap());

    // 3. When available_skills is empty and active_skill is null, load_skill is hidden
    let context_empty = serde_json::json!({
        "skills": {
            "available_skills": [],
            "active_skill": null
        }
    });
    assert!(!tool.is_available(&req, &context_empty).await.unwrap());
}

#[tokio::test]
async fn test_unknown_skill_returns_available_list() {
    let temp_dir = tempfile::tempdir().unwrap();

    let skill1_md = temp_dir.path().join("format-pr").join("SKILL.md");
    create_dir_all(skill1_md.parent().unwrap()).unwrap();
    write(
        &skill1_md,
        "---\nname: format-pr\ndescription: Formats PR\n---\nInstructions",
    )
    .unwrap();

    let skill1 = Skill::from_file(&skill1_md).unwrap();
    let mut map = HashMap::new();
    map.insert(skill1.name.clone(), skill1);

    let tool = LoadSkillTool::new(Arc::new(map));
    let err = tool
        .execute(
            &ContextRequest::default(),
            LoadSkillArgs {
                skill_name: "non-existent".to_string(),
            },
        )
        .await
        .unwrap_err();

    assert!(err.contains("Skill 'non-existent' not found"));
    assert!(err.contains("format-pr"));
}

#[test]
fn test_precedence_ordering() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    // 1. User neutral
    let user_neutral = root.join("user_neutral").join("skills");
    let un_skill = user_neutral.join("common-skill");
    create_dir_all(&un_skill).unwrap();
    write(
        un_skill.join("SKILL.md"),
        "---\nname: common-skill\ndescription: User neutral\n---\nUN body",
    )
    .unwrap();

    // 2. User claude
    let user_claude = root.join("user_claude").join("skills");
    let uc_skill = user_claude.join("common-skill");
    create_dir_all(&uc_skill).unwrap();
    write(
        uc_skill.join("SKILL.md"),
        "---\nname: common-skill\ndescription: User claude\n---\nUC body",
    )
    .unwrap();

    // 3. Project neutral
    let proj_neutral = root.join("proj_neutral").join("skills");
    let pn_skill = proj_neutral.join("common-skill");
    create_dir_all(&pn_skill).unwrap();
    write(
        pn_skill.join("SKILL.md"),
        "---\nname: common-skill\ndescription: Project neutral\n---\nPN body",
    )
    .unwrap();

    // 4. Project claude
    let proj_claude = root.join("proj_claude").join("skills");
    let pc_skill = proj_claude.join("common-skill");
    create_dir_all(&pc_skill).unwrap();
    write(
        pc_skill.join("SKILL.md"),
        "---\nname: common-skill\ndescription: Project claude\n---\nPC body",
    )
    .unwrap();

    let mut skills = HashMap::new();
    scan_skills_directory(&user_neutral, &mut skills);
    assert_eq!(
        skills.get("common-skill").unwrap().description,
        "User neutral"
    );

    scan_skills_directory(&user_claude, &mut skills);
    assert_eq!(
        skills.get("common-skill").unwrap().description,
        "User claude"
    );

    scan_skills_directory(&proj_neutral, &mut skills);
    assert_eq!(
        skills.get("common-skill").unwrap().description,
        "Project neutral"
    );

    scan_skills_directory(&proj_claude, &mut skills);
    assert_eq!(
        skills.get("common-skill").unwrap().description,
        "Project claude"
    );
}
