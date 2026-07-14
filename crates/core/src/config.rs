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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub cognitive: ModelConfig,

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

    #[serde(default = "default_prompt_config")]
    pub prompt: serde_json::Value,
}

fn default_prompt_config() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

fn default_audience() -> String {
    "reasonably intelligent human".to_string()
}
