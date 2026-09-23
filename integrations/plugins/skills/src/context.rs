use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use synapto_interface::context::{ContextProvider, ContextRequest, TemporalScope};
use synapto_interface::decision::{
    ChoiceQuestion, DecisionAnswer, DecisionHandle, DecisionQuestion,
};
use synapto_interface::llm::LLMSafe;

use crate::model::Skill;
use crate::tool::collect_files_recursive;

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct SkillSummary {
    /// Unique identifier name of the skill.
    pub name: String,
    /// Summary of what the skill does and when the agent should use it.
    pub description: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct ActiveSkill {
    /// Unique identifier name of the active skill.
    pub name: String,
    /// Summary of what the skill does.
    pub description: String,
    /// Detailed prompt instructions loaded from SKILL.md.
    pub instructions: String,
    /// Absolute paths to executable scripts bundled with this skill.
    pub scripts: Vec<String>,
    /// Absolute paths to reference documents bundled with this skill.
    pub references: Vec<String>,
    /// Absolute paths to template assets bundled with this skill.
    pub assets: Vec<String>,
    /// Optional tool restriction list specified by the skill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe, Default)]
pub struct SkillsContext {
    /// Lightweight catalog of candidate skills currently available to be loaded.
    pub available_skills: Vec<SkillSummary>,
    /// Pre-loaded skill instructions when the decision provider identifies a high-confidence match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_skill: Option<ActiveSkill>,
}

pub struct SkillsContextProvider {
    skills: Arc<HashMap<String, Skill>>,
    decision_handle: DecisionHandle,
    candidate_threshold: f64,
    activation_threshold: f64,
}

impl SkillsContextProvider {
    pub fn new(
        skills: Arc<HashMap<String, Skill>>,
        decision_handle: DecisionHandle,
        candidate_threshold: f64,
        activation_threshold: f64,
    ) -> Self {
        Self {
            skills,
            decision_handle,
            candidate_threshold,
            activation_threshold,
        }
    }
}

#[async_trait]
impl ContextProvider for SkillsContextProvider {
    type Context = SkillsContext;
    const NAME: &'static str = "skills";
    const SCOPE: TemporalScope = TemporalScope::Current;

    async fn context(
        &self,
        request: &ContextRequest,
    ) -> Result<<Self as ContextProvider>::Context, String> {
        let eligible_skills: Vec<&Skill> = self
            .skills
            .values()
            .filter(|skill| !skill.description.trim().is_empty())
            .collect();

        if eligible_skills.is_empty() {
            return Ok(SkillsContext::default());
        }

        let query = request
            .recent_interactions
            .iter()
            .filter_map(|i| i.peer_input.as_deref())
            .collect::<Vec<_>>()
            .join("\n");

        if self.decision_handle.is_available()
            && !query.trim().is_empty()
            && eligible_skills.len() > 1
        {
            let mut criteria = BTreeMap::new();
            for skill in &eligible_skills {
                criteria.insert(skill.name.clone(), skill.description.clone());
            }
            criteria.insert(
                "none".to_string(),
                "No specialized skill is relevant to the user's current request.".to_string(),
            );

            let question = DecisionQuestion::Choice(ChoiceQuestion {
                instructions: "Select the most relevant specialized skill for the user's request, or 'none' if no specialized skill applies:".to_string(),
                criteria,
            });

            let mut questions = BTreeMap::new();
            questions.insert("skill_routing".to_string(), question);

            let state = serde_json::json!({
                "conversation": query,
            });

            match self.decision_handle.evaluate(None, state, questions).await {
                Ok(mut answers) => {
                    if let Some(DecisionAnswer::Choice {
                        choice,
                        probabilities,
                        ..
                    }) = answers.remove("skill_routing")
                    {
                        // 1. Auto-activation check (probability >= activation_threshold)
                        if choice != "none" {
                            let top_prob = probabilities.get(&choice).copied().unwrap_or(0.0);
                            if top_prob >= self.activation_threshold
                                && let Some(skill) = self.skills.get(&choice)
                            {
                                let instructions = skill.load_instructions().await?;
                                let scripts = collect_files_recursive(&skill.path.join("scripts"));
                                let references =
                                    collect_files_recursive(&skill.path.join("references"));
                                let assets = collect_files_recursive(&skill.path.join("assets"));

                                let active = ActiveSkill {
                                    name: skill.name.clone(),
                                    description: skill.description.clone(),
                                    instructions,
                                    scripts,
                                    references,
                                    assets,
                                    allowed_tools: skill.allowed_tools.clone(),
                                };

                                let mut other_candidates = Vec::new();
                                for other in &eligible_skills {
                                    if other.name != choice {
                                        let prob =
                                            probabilities.get(&other.name).copied().unwrap_or(0.0);
                                        if prob >= self.candidate_threshold {
                                            other_candidates.push(SkillSummary {
                                                name: other.name.clone(),
                                                description: other.description.clone(),
                                            });
                                        }
                                    }
                                }
                                other_candidates.sort_by(|a, b| a.name.cmp(&b.name));

                                return Ok(SkillsContext {
                                    available_skills: other_candidates,
                                    active_skill: Some(active),
                                });
                            }
                        }

                        // 2. Rejection check: if 'none' was selected with dominant probability
                        let none_prob = probabilities.get("none").copied().unwrap_or(0.0);
                        if choice == "none" && none_prob > 0.6 {
                            return Ok(SkillsContext {
                                available_skills: Vec::new(),
                                active_skill: None,
                            });
                        }

                        // 3. Candidate filtering check (probability >= candidate_threshold)
                        let mut candidates = Vec::new();
                        for skill in &eligible_skills {
                            let prob = probabilities.get(&skill.name).copied().unwrap_or(0.0);
                            if skill.name == choice || prob >= self.candidate_threshold {
                                candidates.push(SkillSummary {
                                    name: skill.name.clone(),
                                    description: skill.description.clone(),
                                });
                            }
                        }

                        if !candidates.is_empty() {
                            candidates.sort_by(|a, b| a.name.cmp(&b.name));
                            return Ok(SkillsContext {
                                available_skills: candidates,
                                active_skill: None,
                            });
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Decision provider skill evaluation failed, falling back to full index: {}",
                        e
                    );
                }
            }
        }

        // Baseline fallback: return all eligible skills with active_skill = None
        let mut summaries: Vec<_> = eligible_skills
            .into_iter()
            .map(|s| SkillSummary {
                name: s.name.clone(),
                description: s.description.clone(),
            })
            .collect();
        summaries.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(SkillsContext {
            available_skills: summaries,
            active_skill: None,
        })
    }
}
