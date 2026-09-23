pub mod config;
pub mod context;
pub mod discovery;
pub mod model;
pub mod plugin;
pub mod tool;

pub use config::SkillsPluginConfig;
pub use context::{ActiveSkill, SkillSummary, SkillsContext, SkillsContextProvider};
pub use discovery::discover_skills;
pub use model::{Skill, SkillFrontmatter, parse_skill_md};
pub use plugin::SkillsPlugin;
pub use tool::{LoadSkillArgs, LoadSkillOutput, LoadSkillTool};
