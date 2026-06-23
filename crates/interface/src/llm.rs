use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};

pub use genai;

pub trait LLMSafe {}

impl<T: LLMSafe> LLMSafe for Vec<T> {}
impl<T: LLMSafe> LLMSafe for Option<T> {}
impl<T: LLMSafe> LLMSafe for Box<T> {}
impl<T: LLMSafe> LLMSafe for &T {}
impl<T: LLMSafe> LLMSafe for [T] {}
impl<T: LLMSafe, const N: usize> LLMSafe for [T; N] {}
use std::collections::HashMap;
impl<K: LLMSafe, V: LLMSafe> LLMSafe for HashMap<K, V> {}

pub use synapto_derive::LLMSafe;

#[derive(
    Clone,
    Copy,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    PartialEq,
    Eq,
    Default,
)]
pub enum ReasoningEffort {
    #[default]
    None,
    Minimal,
    Low,
    Medium,
    High,
}

#[derive(
    Clone, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct ModelConfig {
    pub model: String,
    #[serde(default)]
    pub thinking_level: ReasoningEffort,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub enum Instruction {
    /// Standard Blocks (Normal Priority)
    Text(String),
    Item(String),
    NumberedItem(String),
    Section(Box<Instruction>, Vec<Instruction>),

    /// Important Blocks (High Priority)
    ImportantText(String),
    ImportantItem(String),
    ImportantNumberedItem(String),
    ImportantSection(Box<Instruction>, Vec<Instruction>),

    /// Raw Markdown injection
    Markdown(String),
}

impl Instruction {
    pub fn render(instructions: &[Instruction], depth: usize) -> String {
        Self::render_internal(instructions, 0, depth)
    }

    fn render_internal(
        instructions: &[Instruction],
        header_level: usize,
        indent_level: usize,
    ) -> String {
        let mut output = String::new();
        let mut numbered_count = 0;
        for (i, instruction) in instructions.iter().enumerate() {
            if i > 0 {
                output.push_str("\n\n");
            }

            if matches!(
                instruction,
                Instruction::NumberedItem(_) | Instruction::ImportantNumberedItem(_)
            ) {
                numbered_count += 1;
            } else {
                numbered_count = 0;
            }

            output.push_str(&instruction.render_single(header_level, indent_level, numbered_count));
        }
        output
    }

    fn render_single(
        &self,
        header_level: usize,
        indent_level: usize,
        numbered_index: usize,
    ) -> String {
        let indent = "  ".repeat(indent_level);
        match self {
            Instruction::Text(s) => format!("{}{}", indent, s),
            Instruction::Item(s) => format!("{}- {}", indent, s),
            Instruction::NumberedItem(s) => format!("{}{}. {}", indent, numbered_index, s),
            Instruction::Section(title, children) => {
                let mut output =
                    self.render_title(title, header_level, indent_level, false, numbered_index);

                let (next_header, next_indent) = if matches!(
                    **title,
                    Instruction::Text(_) | Instruction::ImportantText(_)
                ) {
                    (header_level + 1, indent_level)
                } else {
                    (header_level, indent_level + 1)
                };

                if !children.is_empty() {
                    output.push_str("\n\n");
                    output.push_str(&Self::render_internal(children, next_header, next_indent));
                }
                output
            }
            Instruction::ImportantText(s) => {
                format!("{}> [!IMPORTANT]\n{}> **{}**", indent, indent, s)
            }
            Instruction::ImportantItem(s) => {
                format!("{}- > [!IMPORTANT]\n{}  > **{}**", indent, indent, s)
            }
            Instruction::ImportantNumberedItem(s) => {
                format!(
                    "{}{}. > [!IMPORTANT]\n{}   > **{}**",
                    indent, numbered_index, indent, s
                )
            }
            Instruction::ImportantSection(title, children) => {
                let mut inner = self.render_title(title, header_level, 0, true, numbered_index);
                let (next_header, next_indent) = if matches!(
                    **title,
                    Instruction::Text(_) | Instruction::ImportantText(_)
                ) {
                    (header_level + 1, 0)
                } else {
                    (header_level, 1)
                };

                if !children.is_empty() {
                    inner.push_str("\n\n");
                    inner.push_str(&Self::render_internal(children, next_header, next_indent));
                }
                let mut output = format!("{}> [!IMPORTANT]\n", indent);
                for line in inner.lines() {
                    output.push_str(&format!("{}> {}\n", indent, line));
                }
                output.trim_end().to_string()
            }
            Instruction::Markdown(s) => format!("{}{}", indent, s),
        }
    }

    fn render_title(
        &self,
        title: &Instruction,
        header_level: usize,
        indent_level: usize,
        force_plain: bool,
        numbered_index: usize,
    ) -> String {
        match title {
            Instruction::Text(s) | Instruction::ImportantText(s) => {
                if force_plain {
                    format!("{} {}", "#".repeat(header_level + 1), s.to_uppercase())
                } else {
                    format!("{} {}", "#".repeat(header_level + 1), s)
                }
            }
            Instruction::Item(s) | Instruction::ImportantItem(s) => {
                format!("{}- {}", "  ".repeat(indent_level), s)
            }
            Instruction::NumberedItem(s) | Instruction::ImportantNumberedItem(s) => {
                format!("{}{}. {}", "  ".repeat(indent_level), numbered_index, s)
            }
            Instruction::Section(inner_title, _)
            | Instruction::ImportantSection(inner_title, _) => self.render_title(
                inner_title,
                header_level,
                indent_level,
                force_plain,
                numbered_index,
            ),
            Instruction::Markdown(s) => format!("{}{}", "  ".repeat(indent_level), s),
        }
    }
}

/// The strict type bounds required for any structural input to the LLM.
pub trait LlmInput: Serialize + JsonSchema + Send + Sync + LLMSafe {}
impl<T: Serialize + JsonSchema + Send + Sync + LLMSafe> LlmInput for T {}

/// The strict type bounds required for any structural output from the LLM.
pub trait LlmOutput: DeserializeOwned + JsonSchema + Send + Sync {}
impl<T: DeserializeOwned + JsonSchema + Send + Sync> LlmOutput for T {}

#[derive(Debug, Default, Clone)]
pub struct RawLlmOptions {
    pub reasoning_effort: Option<genai::chat::ReasoningEffort>,
    pub tools: Option<Vec<genai::chat::Tool>>,
    pub resolved_tools: Option<Vec<(genai::chat::ToolCall, String)>>,
    pub output_schema: Option<schemars::Schema>,
    pub messages: Option<Vec<genai::chat::ChatMessage>>,
}

/// Abstract, object-safe raw executor contract, completely decoupled from any specific client library.
#[async_trait::async_trait]
pub trait LlmExecutor: Send + Sync + 'static {
    async fn execute_raw(
        &self,
        model: &str,
        system_prompt: &str,
        prompt: &str,
        options: RawLlmOptions,
    ) -> Result<genai::chat::ChatResponse, String>;
}

/// Generic helper extension trait implemented as a blanket impl for any type implementing LlmExecutor.
#[async_trait::async_trait]
pub trait LlmExecutorExt {
    async fn generate_typed<In, Out>(
        &self,
        model: &str,
        system_prompt: Option<&str>,
        input: In,
        reasoning_effort: Option<ReasoningEffort>,
    ) -> Result<Out, String>
    where
        In: LlmInput,
        Out: LlmOutput;
}

// TODO this is not interface, it doesn't belong here (at least the implementation)
#[async_trait::async_trait]
impl<E: LlmExecutor + ?Sized> LlmExecutorExt for E {
    async fn generate_typed<In, Out>(
        &self,
        model: &str,
        system_prompt: Option<&str>,
        input: In,
        reasoning_effort: Option<ReasoningEffort>,
    ) -> Result<Out, String>
    where
        In: LlmInput,
        Out: LlmOutput,
    {
        let input_data_json = serde_json::to_string(&input).map_err(|e| e.to_string())?;

        let input_schema = schemars::schema_for!(In);
        let input_schema_yaml = serde_yaml::to_string(&input_schema).map_err(|e| e.to_string())?;

        let prompt = format!(
            "## Input Data Types & Descriptions (YAML):\n\n```yaml\n{}\n```\n\n## Input data (JSON):\n\n```json\n{}\n```",
            input_schema_yaml, input_data_json
        );

        let options = RawLlmOptions {
            reasoning_effort: reasoning_effort.map(|e| match e {
                ReasoningEffort::None => genai::chat::ReasoningEffort::None,
                ReasoningEffort::Minimal => genai::chat::ReasoningEffort::Minimal,
                ReasoningEffort::Low => genai::chat::ReasoningEffort::Low,
                ReasoningEffort::Medium => genai::chat::ReasoningEffort::Medium,
                ReasoningEffort::High => genai::chat::ReasoningEffort::High,
            }),
            tools: None,
            resolved_tools: None,
            output_schema: Some(schemars::schema_for!(Out)),
            messages: None,
        };

        let sys_prompt = system_prompt.unwrap_or_default();

        let raw_response = self
            .execute_raw(model, sys_prompt, &prompt, options)
            .await?;

        serde_json::from_str(&raw_response.texts().join("")).map_err(|e| e.to_string())
    }
}

#[async_trait::async_trait]
impl<T: LlmExecutor + ?Sized> LlmExecutor for std::sync::Arc<T> {
    async fn execute_raw(
        &self,
        model: &str,
        system_prompt: &str,
        prompt: &str,
        options: RawLlmOptions,
    ) -> Result<genai::chat::ChatResponse, String> {
        (**self)
            .execute_raw(model, system_prompt, prompt, options)
            .await
    }
}
