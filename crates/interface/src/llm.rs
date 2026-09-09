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
    pub reasoning_effort: ReasoningEffort,
}

#[derive(Debug, Default, Clone)]
pub struct RawLlmOptions {
    pub reasoning_effort: Option<genai::chat::ReasoningEffort>,
    pub tools: Option<Vec<genai::chat::Tool>>,
    pub resolved_tools: Option<Vec<(genai::chat::ToolCall, String)>>,
    pub output_schema: Option<schemars::Schema>,
    pub messages: Option<Vec<genai::chat::ChatMessage>>,
}

/// Internal raw executor contract, completely decoupled from any specific client library.
#[doc(hidden)]
#[async_trait::async_trait]
pub trait RawLlmExecutor: Send + Sync + 'static {
    async fn execute_raw(
        &self,
        model: &str,
        system_prompt: &str,
        prompt: &str,
        options: RawLlmOptions,
    ) -> Result<genai::chat::ChatResponse, String>;
}

#[doc(hidden)]
#[async_trait::async_trait]
impl<T: RawLlmExecutor + ?Sized> RawLlmExecutor for std::sync::Arc<T> {
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

/// Opaque handle to the LLM execution runtime.
///
/// This struct wraps the execution backend and exposes zero public execution methods.
/// Pass this handle to `LLM::create_client` to construct a typed, structured LLM client.
#[derive(Clone)]
pub struct LlmExecutor {
    backend: std::sync::Arc<dyn RawLlmExecutor>,
}

impl std::fmt::Debug for LlmExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmExecutor").finish_non_exhaustive()
    }
}

impl LlmExecutor {
    pub fn new<B: RawLlmExecutor + 'static>(backend: B) -> Self {
        Self {
            backend: std::sync::Arc::new(backend),
        }
    }

    pub fn from_arc(backend: std::sync::Arc<dyn RawLlmExecutor>) -> Self {
        Self { backend }
    }

    #[doc(hidden)]
    pub async fn execute_internal(
        &self,
        model: &str,
        system_prompt: &str,
        prompt: &str,
        options: RawLlmOptions,
    ) -> Result<genai::chat::ChatResponse, String> {
        self.backend
            .execute_raw(model, system_prompt, prompt, options)
            .await
    }
}
