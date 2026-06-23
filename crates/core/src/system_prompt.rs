pub use synapto_llm::Instruction;

pub fn get_system_prompt(
    config: &crate::config::Config,
    instructions: Vec<Instruction>,
) -> Vec<Instruction> {
    let mut prompt_parts = Vec::new();

    prompt_parts.push(Instruction::Text(format!(
        "Speak in a way that a {} can understand.",
        config.audience
    )));

    if !instructions.is_empty() {
        prompt_parts.push(Instruction::Section(
            Box::new(Instruction::Text("Instructions".to_string())),
            instructions,
        ));
    }

    prompt_parts
}
