#![allow(incomplete_features)]
#![feature(adt_const_params)]
#![feature(unsized_const_params)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use synapto::prompt_provider::{CognitivePromptProvider, CognitiveTarget};
use synapto_llm::Instruction;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct EmbeddedPromptConfig {
    #[serde(flatten)]
    pub values: BTreeMap<String, String>,
}

pub struct EmbeddedPromptProvider<const PROMPT: &'static str>;

impl<const PROMPT: &'static str> CognitivePromptProvider for EmbeddedPromptProvider<PROMPT> {
    type Config = EmbeddedPromptConfig;

    fn get_system_prompt(prompt_config: &Self::Config) -> Vec<Instruction> {
        let mut prompt_content = PROMPT.to_string();

        for (key, value) in &prompt_config.values {
            let placeholder = format!("{{{{{}}}}}", key);
            prompt_content = prompt_content.replace(&placeholder, value);
        }

        vec![Instruction::Markdown(prompt_content)]
    }

    fn get_dynamic_instructions(
        _prompt_config: &Self::Config,
        _compiled_context: &synapto::CognitiveLLMContent,
        _is_initial_run: bool,
        _target: CognitiveTarget,
    ) -> Vec<Instruction> {
        Vec::new()
    }
}
