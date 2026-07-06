use serde::de::DeserializeOwned;
use std::path::Path;

pub use crate::cognitive::CognitiveLLMContent;

pub enum CognitiveTarget {
    Direct,
    Side,
}

pub trait CognitivePromptProvider: Send + Sync + 'static {
    type Config: DeserializeOwned + Default + Send + Sync;

    /// Generates the static system prompt at boot. This is fed into `LLMClient::new`
    /// and represents the foundational, unmoving identity and rules of the agent.
    fn get_system_prompt(
        data_dir: &Path,
        prompt_config: &Self::Config,
    ) -> Vec<synapto_llm::Instruction> {
        Vec::new()
    }

    /// Generates dynamic instructions for the current turn based on the compiled context.
    /// This is fed into `LLMClient::call` on every cycle, overriding or steering immediate behavior.
    fn get_dynamic_instructions(
        prompt_config: &Self::Config,
        compiled_context: &CognitiveLLMContent,
        is_initial_run: bool,
        target: CognitiveTarget,
    ) -> Vec<synapto_llm::Instruction> {
        Vec::new()
    }
}

pub struct EmptyPromptProvider;

impl CognitivePromptProvider for EmptyPromptProvider {
    type Config = ();
}
