use serde::{Deserialize, Serialize};

use synapto_interface::llm::ModelConfig;
use synapto_interface::llm::ReasoningEffort;

pub mod env;
pub mod provider;
pub use provider::*;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GoogleServiceAccountCredentials(serde_json::Value);

impl From<GoogleServiceAccountCredentials> for String {
    fn from(value: GoogleServiceAccountCredentials) -> Self {
        serde_json::to_string(&value.0).unwrap_or_else(|e| {
            panic!("Failed to serialize GoogleServiceAccountCredentials: {}", e)
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct AssistantConfig {
    pub chats: std::collections::BTreeMap<String, String>,
    pub full_name: String,
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

    #[serde(default)]
    pub google_vertex_ai_location: Option<String>,

    #[serde(default)]
    pub google_project_id: String,

    #[serde(default)]
    pub gemini_api_key: Option<String>,

    #[serde(default)]
    pub data_dir: std::path::PathBuf,

    #[serde(default)]
    pub barge_in: bool,

    #[serde(default)]
    pub speakers: Vec<String>,

    #[serde(default)]
    pub initial_run: InitialRunConfig,

    #[serde(default)]
    pub google_service_account_credentials: GoogleServiceAccountCredentials,

    #[serde(default)]
    pub disable_cognitive_direct: bool,

    #[serde(default)]
    pub disable_cognitive_side: bool,

    #[serde(default)]
    pub diarization_warn_when_uncertain: bool,

    #[serde(default = "default_prompt_config")]
    pub prompt: serde_json::Value,
}

fn default_prompt_config() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

fn default_audience() -> String {
    "reasonably intelligent human".to_string()
}
