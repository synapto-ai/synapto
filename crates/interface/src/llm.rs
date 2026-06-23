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
