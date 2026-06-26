use serde::{Deserialize, Serialize};

use synapto_interface::llm::ModelConfig;
use synapto_interface::llm::ReasoningEffort;

mod data_dir;
mod dotenv;
pub mod env;
mod json;
mod provider;

pub use data_dir::DataDirProvider;
pub use dotenv::DotEnv;
pub use env::Env;
pub use json::ConfigJson;
pub use provider::ConfigProvider;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GoogleServiceAccountCredentials(serde_json::Value);

impl From<GoogleServiceAccountCredentials> for String {
    fn from(value: GoogleServiceAccountCredentials) -> Self {
        serde_json::to_string(&value.0).unwrap_or_else(|e| {
            panic!("Failed to serialize GoogleServiceAccountCredentials: {}", e)
        })
    }
}

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
    pub gemini_api_key: Option<String>,

    // FIXME pub
    #[serde(default)]
    pub data_dir: std::path::PathBuf,

    #[serde(default)]
    pub barge_in: bool,

    // FIXME
    #[serde(default)]
    speakers: Vec<String>,

    #[serde(default)]
    pub initial_run: InitialRunConfig,

    // FIXME
    #[serde(default)]
    pub google_service_account_credentials: GoogleServiceAccountCredentials,

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
