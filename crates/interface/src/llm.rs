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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: Option<String>,
    pub schema: Option<serde_json::Value>,
}

impl ToolDefinition {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            schema: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_schema(mut self, schema: serde_json::Value) -> Self {
        self.schema = Some(schema);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    pub call_id: crate::tool::ToolCallId,
    pub fn_name: String,
    pub fn_arguments: serde_json::Value,
    /// Opaque provider continuation state preserved across tool-call turns without leaking vendor schema.
    pub provider_metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RawLlmResponse {
    pub texts: Vec<String>,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Default, Clone)]
pub struct RawLlmOptions {
    pub reasoning_effort: Option<ReasoningEffort>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub resolved_tools: Option<Vec<(ToolCall, String)>>,
    pub output_schema: Option<schemars::Schema>,
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
    ) -> Result<RawLlmResponse, String>;
}

/// Capability trait for singleton LLM providers.
/// LLM providers do NOT implement Plugin and have no actor or channel capabilities.
pub trait LlmProvider: Send + Sync + 'static {
    type Config: serde::de::DeserializeOwned;

    fn init(
        config: Self::Config,
        credentials: crate::credentials::CredentialsHandle,
    ) -> Result<Self, String>
    where
        Self: Sized;

    fn raw_llm_executor(&self) -> std::sync::Arc<dyn RawLlmExecutor>;
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
    ) -> Result<RawLlmResponse, String> {
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
    ) -> Result<RawLlmResponse, String> {
        self.backend
            .execute_raw(model, system_prompt, prompt, options)
            .await
    }
}
