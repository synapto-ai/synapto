#![allow(incomplete_features)]
#![feature(adt_const_params)]
#![feature(unsized_const_params)]
#![allow(clippy::disallowed_methods)]

use synapto::prompt_provider::CognitivePromptProvider;
use synapto_llm::Instruction;
use synapto_prompt_embedded::{EmbeddedPromptConfig, EmbeddedPromptProvider};

#[test]
fn test_embedded_prompt_interpolation() {
    type TestPrompt = EmbeddedPromptProvider<"Hello {{name}}, welcome to {{service}}!">;

    let mut config = EmbeddedPromptConfig::default();
    config
        .values
        .insert("name".to_string(), "Alice".to_string());
    config
        .values
        .insert("service".to_string(), "Synapto".to_string());

    let instructions = TestPrompt::get_system_prompt(&config);
    assert_eq!(instructions.len(), 1);
    match &instructions[0] {
        Instruction::Markdown(content) => {
            assert_eq!(content, "Hello Alice, welcome to Synapto!");
        }
        _ => panic!("Expected Instruction::Markdown"),
    }
}

#[test]
fn test_embedded_prompt_no_placeholders() {
    type TestPrompt = EmbeddedPromptProvider<"Static prompt text">;

    let config = EmbeddedPromptConfig::default();
    let instructions = TestPrompt::get_system_prompt(&config);
    assert_eq!(instructions.len(), 1);
    match &instructions[0] {
        Instruction::Markdown(content) => {
            assert_eq!(content, "Static prompt text");
        }
        _ => panic!("Expected Instruction::Markdown"),
    }
}
