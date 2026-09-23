use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use synapto_interface::plugin::{Plugin, PluginInitContext, PluginRegistry};

use crate::config::SkillsPluginConfig;
use crate::context::SkillsContextProvider;
use crate::discovery::discover_skills;
use crate::model::Skill;
use crate::tool::LoadSkillTool;

pub struct SkillsPlugin {
    skills: Arc<HashMap<String, Skill>>,
    context_provider: Arc<SkillsContextProvider>,
    tool: Arc<LoadSkillTool>,
}

impl SkillsPlugin {
    pub fn skills(&self) -> &Arc<HashMap<String, Skill>> {
        &self.skills
    }
}

#[async_trait]
impl Plugin for SkillsPlugin {
    async fn create(context: &PluginInitContext<'_>) -> Result<Self, String> {
        let config: SkillsPluginConfig = context.optional_config()?.unwrap_or_default();
        let project_root = config.project_root.unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });

        let discovered = discover_skills(&project_root);
        tracing::info!("Discovered {} skill(s)", discovered.len());

        let skills = Arc::new(discovered);
        let decision_handle = context.decision_handle();
        let context_provider = Arc::new(SkillsContextProvider::new(
            skills.clone(),
            decision_handle,
            config.candidate_threshold,
            config.activation_threshold,
        ));
        let tool = Arc::new(LoadSkillTool::new(skills.clone()));

        Ok(Self {
            skills,
            context_provider,
            tool,
        })
    }

    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R)
    where
        Self: Sized,
    {
        registry.register_context_provider(self.context_provider.clone());
        registry.register_tool((*self.tool).clone());
    }
}
