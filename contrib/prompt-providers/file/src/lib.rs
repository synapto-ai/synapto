use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use synapto::prompt_provider::{CognitivePromptProvider, CognitiveTarget};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct FilePromptConfig {
    #[serde(flatten)]
    pub values: BTreeMap<String, String>,
}

pub struct FilePromptProvider<P: synapto_interface::data_dir::DataDirProvider> {
    _marker: std::marker::PhantomData<P>,
}

impl<P: synapto_interface::data_dir::DataDirProvider> CognitivePromptProvider
    for FilePromptProvider<P>
{
    type Config = FilePromptConfig;

    fn get_system_prompt(prompt_config: &Self::Config) -> Vec<synapto_llm::Instruction> {
        let prompt_path = P::get_data_dir().join("PROMPT.md");

        let mut prompt_content = std::fs::read_to_string(&prompt_path).unwrap_or_else(|e| {
            panic!(
                "Failed to read cognitive prompt from {}: {}",
                prompt_path.display(),
                e
            );
        });

        for (key, value) in &prompt_config.values {
            let placeholder = format!("{{{{{}}}}}", key);
            prompt_content = prompt_content.replace(&placeholder, value);
        }

        vec![synapto_llm::Instruction::Markdown(prompt_content)]
    }

    fn get_dynamic_instructions(
        _prompt_config: &Self::Config,
        _compiled_context: &synapto::CognitiveLLMContent,
        _is_initial_run: bool,
        _target: CognitiveTarget,
    ) -> Vec<synapto_llm::Instruction> {
        Vec::new()
    }
}
