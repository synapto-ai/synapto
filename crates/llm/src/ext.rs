use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use synapto_interface::llm::{LLMSafe, LlmExecutor, RawLlmOptions, ReasoningEffort};

/// The strict type bounds required for any structural input to the LLM.
pub trait LlmInput: Serialize + JsonSchema + Send + Sync + LLMSafe {}
impl<T: Serialize + JsonSchema + Send + Sync + LLMSafe> LlmInput for T {}

/// The strict type bounds required for any structural output from the LLM.
pub trait LlmOutput: DeserializeOwned + JsonSchema + Send + Sync {}
impl<T: DeserializeOwned + JsonSchema + Send + Sync> LlmOutput for T {}

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
