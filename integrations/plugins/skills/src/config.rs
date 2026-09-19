use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_candidate_threshold() -> f64 {
    0.50
}

fn default_activation_threshold() -> f64 {
    0.85
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsPluginConfig {
    /// Optional project root directory. If omitted, defaults to the current working directory.
    #[serde(default)]
    pub project_root: Option<PathBuf>,

    /// Probability threshold for including a skill in the candidate index (default: 0.50).
    #[serde(default = "default_candidate_threshold")]
    pub candidate_threshold: f64,

    /// Probability threshold for auto-activating a skill directly into context (default: 0.85).
    #[serde(default = "default_activation_threshold")]
    pub activation_threshold: f64,
}

impl Default for SkillsPluginConfig {
    fn default() -> Self {
        Self {
            project_root: None,
            candidate_threshold: default_candidate_threshold(),
            activation_threshold: default_activation_threshold(),
        }
    }
}
