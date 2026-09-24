use serde::{Deserialize, Serialize};

use synapto_interface::llm::ModelConfig;
use synapto_interface::llm::ReasoningEffort;

mod dotenv;
pub mod env;
mod json;
mod provider;

pub use dotenv::DotEnv;
pub use env::Env;
pub use json::ConfigJson;
pub use provider::ConfigProvider;
use synapto_interface::secrets::Secret;

use crate::google_credentials::GoogleServiceAccountCredentials;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InitialRunConfig {
    #[serde(default)]
    pub automatic_cognitive_trigger: bool,

    #[serde(default)]
    pub discard_interaction: bool,

    #[serde(default)]
    pub reasoning_effort: ReasoningEffort,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveConfig {
    pub model: String,

    #[serde(default)]
    pub reasoning_effort: ReasoningEffort,

    #[serde(default)]
    pub disable_preflight_decision: bool,
}

impl From<CognitiveConfig> for ModelConfig {
    fn from(c: CognitiveConfig) -> Self {
        Self {
            model: c.model,
            reasoning_effort: c.reasoning_effort,
        }
    }
}

impl From<&CognitiveConfig> for ModelConfig {
    fn from(c: &CognitiveConfig) -> Self {
        Self {
            model: c.model.clone(),
            reasoning_effort: c.reasoning_effort,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub cognitive: CognitiveConfig,

    #[serde(default = "default_audience")]
    pub audience: String,

    // FIXME
    #[serde(default)]
    pub google_vertex_ai_location: Option<String>,

    // FIXME
    #[serde(default)]
    pub google_project_id: String,

    // FIXME
    #[serde(default)]
    pub gemini_api_key: Option<Secret<String>>,

    // FIXME pub
    #[serde(default)]
    pub data_dir: std::path::PathBuf,

    #[serde(default)]
    pub barge_in: bool,

    #[serde(default)]
    pub initial_run: InitialRunConfig,

    // FIXME
    #[serde(default)]
    pub google_service_account_credentials: Option<Secret<GoogleServiceAccountCredentials>>,

    #[serde(default)]
    pub disable_cognitive_direct: bool,

    #[serde(default)]
    pub disable_cognitive_side: bool,

    #[serde(default)]
    pub disable_ctrl_c: bool,

    #[serde(default = "default_plugin_init_timeout_secs")]
    pub plugin_init_timeout_secs: u64,

    #[serde(default = "default_prompt_config")]
    pub prompt: serde_json::Value,
}

fn default_plugin_init_timeout_secs() -> u64 {
    30
}

fn default_prompt_config() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

fn default_audience() -> String {
    "reasonably intelligent human".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cognitive_config_default_disable_preflight_decision() {
        let json = serde_json::json!({
            "model": "gemini-3.7-flash",
            "reasoning_effort": "Minimal"
        });
        let config: CognitiveConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.model, "gemini-3.7-flash");
        assert_eq!(config.reasoning_effort, ReasoningEffort::Minimal);
        assert!(!config.disable_preflight_decision);

        let model_config: ModelConfig = config.into();
        assert_eq!(model_config.model, "gemini-3.7-flash");
        assert_eq!(model_config.reasoning_effort, ReasoningEffort::Minimal);
    }

    #[test]
    fn test_cognitive_config_disable_preflight_decision_true() {
        let json = serde_json::json!({
            "model": "gemini-3.7-flash",
            "reasoning_effort": "Low",
            "disable_preflight_decision": true
        });
        let config: CognitiveConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.model, "gemini-3.7-flash");
        assert_eq!(config.reasoning_effort, ReasoningEffort::Low);
        assert!(config.disable_preflight_decision);
    }
}
