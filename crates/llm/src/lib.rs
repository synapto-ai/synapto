use std::future::Future;
use std::{marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use gcp_auth::TokenProvider;
use genai::{
    chat::{Tool, ToolCall},
    resolver::{AuthData, AuthResolver},
};
use serde::{Serialize, de::DeserializeOwned};
use synapto_interface::secrets::Secret;
use tracing::instrument;

use synapto_interface::llm::LLMSafe;
use synapto_interface::llm::ReasoningEffort;

pub mod ext;
pub mod instruction;

pub use ext::{LlmExecutorExt, LlmInput, LlmOutput};
pub use instruction::Instruction;

#[derive(Clone, Debug, Default)]
pub struct LLMClientConfig {
    pub google_vertex_ai_location: Option<String>,
    pub google_project_id: String,
    pub google_service_account_credentials: Option<Secret<String>>,
    pub gemini_api_key: Option<Secret<String>>,
}

pub trait ToolExecutor: Send + Sync {
    fn execute(
        &self,
        ctx: synapto_interface::context::ContextRequest,
        tool_calls: Vec<ToolCall>,
    ) -> impl Future<Output = ()> + Send;
}

pub trait ErasedToolOutput: std::fmt::Debug + Send + Sync {
    fn to_json_string(&self) -> String;
}

impl<T> ErasedToolOutput for T
where
    T: Serialize + std::fmt::Debug + Send + Sync,
{
    fn to_json_string(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|e| panic!("Failed to serialize ToolOutput: {:?}", e))
    }
}

#[derive(Clone)]
pub struct ToolOutput(pub std::sync::Arc<dyn ErasedToolOutput>);

impl std::fmt::Debug for ToolOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl ToolOutput {
    pub fn new<T>(value: T) -> Self
    where
        T: Serialize + std::fmt::Debug + Send + Sync + 'static,
    {
        Self(std::sync::Arc::new(value))
    }
}

pub enum LLMResult<Output> {
    Output(Output),
    Interrupted(Option<Output>, Vec<ToolCall>),
}

pub trait ToolMode: Send + Sync {
    type CallResult<Output>;
    fn success<Output>(output: Output) -> Self::CallResult<Output>;
    fn interrupted<Output>(
        output: Option<Output>,
        tool_calls: Vec<ToolCall>,
    ) -> Result<Self::CallResult<Output>, anyhow::Error>;
    fn execute(
        &self,
        ctx: synapto_interface::context::ContextRequest,
        tool_calls: Vec<ToolCall>,
    ) -> impl Future<Output = ()> + Send;
}

pub struct WithoutTools;
impl ToolMode for WithoutTools {
    type CallResult<Output> = Output;
    fn success<Output>(output: Output) -> Self::CallResult<Output> {
        output
    }
    fn interrupted<Output>(
        _output: Option<Output>,
        _tool_calls: Vec<ToolCall>,
    ) -> Result<Self::CallResult<Output>, anyhow::Error> {
        Err(anyhow::anyhow!("LLM returned tool calls unexpectedly"))
    }
    async fn execute(
        &self,
        _ctx: synapto_interface::context::ContextRequest,
        _tool_calls: Vec<ToolCall>,
    ) {
        unreachable!("WithoutTools executor should never be called");
    }
}

pub struct WithTools<E>(pub E);
impl<E: ToolExecutor> ToolMode for WithTools<E> {
    type CallResult<Output> = LLMResult<Output>;
    fn success<Output>(output: Output) -> Self::CallResult<Output> {
        LLMResult::Output(output)
    }
    fn interrupted<Output>(
        output: Option<Output>,
        tool_calls: Vec<ToolCall>,
    ) -> Result<Self::CallResult<Output>, anyhow::Error> {
        Ok(LLMResult::Interrupted(output, tool_calls))
    }
    async fn execute(
        &self,
        ctx: synapto_interface::context::ContextRequest,
        tool_calls: Vec<ToolCall>,
    ) {
        self.0.execute(ctx, tool_calls).await;
    }
}

pub struct LLMClient<Content, Output, Tools = WithoutTools> {
    executor: std::sync::Arc<dyn synapto_interface::llm::LlmExecutor>,
    pub name: String,
    pub model: String,
    input_schema: String,
    pub system_prompt: String,
    pub tools: Vec<Tool>,
    pub tools_state: Tools,
    pub output_schema: schemars::Schema,
    _marker: PhantomData<(Content, Output)>,
}

pub type ResolvedTools = Vec<(ToolCall, ToolOutput)>;

impl<Content: Serialize + std::fmt::Debug, Output: DeserializeOwned, Tools: ToolMode>
    LLMClient<Content, Output, Tools>
{
    #[instrument(
        level = "info",
        skip_all,
        fields(activate_subsystem, track_stats = true)
    )]
    async fn call_inner(
        &self,
        content: Content,
        instructions: Option<Vec<Instruction>>,
        override_reasoning_effort: Option<ReasoningEffort>,
        resolved_tools: Option<ResolvedTools>,
        override_tools: Option<Vec<Tool>>,
        ctx: Option<synapto_interface::context::ContextRequest>,
    ) -> Result<Tools::CallResult<Output>, anyhow::Error> {
        #[allow(dead_code)]
        struct ResolvedToolsFmt<'a>(&'a Option<ResolvedTools>);
        impl<'a> std::fmt::Debug for ResolvedToolsFmt<'a> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                if let Some(tools) = self.0 {
                    f.debug_list()
                        .entries(tools.iter().map(|(call, output)| {
                            struct ToolCallFmt<'a>(&'a genai::chat::ToolCall);
                            impl<'a> std::fmt::Debug for ToolCallFmt<'a> {
                                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                                    f.debug_struct("ToolCall")
                                        .field("call_id", &self.0.call_id)
                                        .field("fn_name", &self.0.fn_name)
                                        .field("fn_arguments", &self.0.fn_arguments)
                                        .field(
                                            "thought_signatures",
                                            &format_args!(
                                                "{}",
                                                if self.0.thought_signatures.is_some() {
                                                    "Some(...)"
                                                } else {
                                                    "None"
                                                }
                                            ),
                                        )
                                        .finish()
                                }
                            }
                            (ToolCallFmt(call), output)
                        }))
                        .finish()
                } else {
                    f.write_str("None")
                }
            }
        }

        #[cfg(feature = "rerun")]
        if let Ok(content) =
            serde_json::to_string_pretty(&content).inspect_err(|e| tracing::error!("{}", e))
        {
            synapto_telemetry::log_to_rerun(
                format!("llm/{}/content", self.name),
                &synapto_telemetry::rerun_core::archetypes::TextDocument::new(content),
            );
        };

        let prompt = format!(
            "## Input Data Types & Descriptions (YAML):\n\n```yaml\n{}\n```\n\n## Input data (JSON):\n\n```json\n{}\n```\n\n{}",
            self.input_schema,
            serde_json::to_string(&content).unwrap_or_else(|e| panic!(
                "Failed to serialize content: {e} | Content: {content:?}"
            )),
            if let Some(ref instructions) = instructions
                && !instructions.is_empty()
            {
                format!(
                    "# Instructions:\n\n[CRITICAL RULES]:\n{}",
                    Instruction::render(instructions, 0)
                )
            } else {
                "".to_string()
            }
        );

        tracing::trace!("full prompt: {}", prompt);

        let reasoning_effort = override_reasoning_effort.map(|e| match e {
            ReasoningEffort::None => genai::chat::ReasoningEffort::None,
            ReasoningEffort::Minimal => genai::chat::ReasoningEffort::Minimal,
            ReasoningEffort::Low => genai::chat::ReasoningEffort::Low,
            ReasoningEffort::Medium => genai::chat::ReasoningEffort::Medium,
            ReasoningEffort::High => genai::chat::ReasoningEffort::High,
        });

        let current_tools = override_tools.unwrap_or_else(|| self.tools.clone());

        let has_tools = !current_tools.is_empty();

        let raw_options = synapto_interface::llm::RawLlmOptions {
            reasoning_effort,
            tools: if has_tools && resolved_tools.as_ref().is_none_or(|rt| rt.is_empty()) {
                Some(current_tools)
            } else {
                None
            },
            resolved_tools: resolved_tools.as_ref().map(|rt| {
                rt.iter()
                    .map(|(call, output)| (call.clone(), output.0.to_json_string()))
                    .collect::<Vec<_>>()
            }),
            output_schema: Some(self.output_schema.clone()),
            messages: None,
        };

        // Call the abstract, injected executor
        let response_obj = self
            .executor
            .execute_raw(&self.model, &self.system_prompt, &prompt, raw_options)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let tool_calls = response_obj.tool_calls();

        if !tool_calls.is_empty() {
            if !has_tools || resolved_tools.as_ref().is_some_and(|rt| !rt.is_empty()) {
                tracing::error!("LLM returned tool calls unexpectedly");
                return Tools::interrupted(None, vec![]);
            }

            let tool_calls_cloned: Vec<_> = tool_calls.into_iter().cloned().collect();
            self.tools_state
                .execute(ctx.unwrap_or_default(), tool_calls_cloned.clone())
                .await;

            let joined_text = response_obj.texts().join("");
            tracing::debug!("LLM interrupted output raw text: {}", joined_text);
            let parsed_output = if joined_text.trim().is_empty() {
                None
            } else {
                match serde_json::from_str(&joined_text) {
                    Ok(output) => Some(output),
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse LLM response during tool interruption, soft-failing output: {}",
                            e
                        );
                        None
                    }
                }
            };

            return Tools::interrupted(parsed_output, tool_calls_cloned);
        }

        match serde_json::from_str(&response_obj.texts().join("")) {
            Ok(output) => Ok(Tools::success(output)),
            Err(e) => Err(anyhow::anyhow!("Failed to parse LLM response: {}", e)),
        }
    }
}

impl<Content: Serialize + std::fmt::Debug, Output: DeserializeOwned>
    LLMClient<Content, Output, WithoutTools>
{
    pub async fn call(
        &self,
        content: Content,
        instructions: Option<Vec<Instruction>>,
        override_reasoning_effort: Option<ReasoningEffort>,
    ) -> Result<Output, anyhow::Error> {
        self.call_inner(
            content,
            instructions,
            override_reasoning_effort,
            None,
            None,
            None,
        )
        .await
    }
}

impl<Content: Serialize + std::fmt::Debug, Output: DeserializeOwned, Executor: ToolExecutor>
    LLMClient<Content, Output, WithTools<Executor>>
{
    pub async fn call(
        &self,
        content: Content,
        instructions: Option<Vec<Instruction>>,
        override_reasoning_effort: Option<ReasoningEffort>,
        resolved_tools: Option<ResolvedTools>,
        override_tools: Option<Vec<Tool>>,
        ctx: synapto_interface::context::ContextRequest,
    ) -> Result<LLMResult<Output>, anyhow::Error> {
        self.call_inner(
            content,
            instructions,
            override_reasoning_effort,
            resolved_tools,
            override_tools,
            Some(ctx),
        )
        .await
    }
}

#[allow(clippy::upper_case_acronyms)]
pub trait LLM {
    type Content: schemars::JsonSchema + Serialize + LLMSafe;
    type Output: schemars::JsonSchema + DeserializeOwned + Clone + LLMSafe;

    #[instrument(level = "trace", skip_all)]
    fn create_client(
        executor: std::sync::Arc<dyn synapto_interface::llm::LlmExecutor>,
        config: synapto_interface::llm::ModelConfig,
        system_prompt: Vec<Instruction>,
    ) -> LLMClient<Self::Content, Self::Output, WithoutTools> {
        struct NoopExecutor;
        impl ToolExecutor for NoopExecutor {
            async fn execute(
                &self,
                _ctx: synapto_interface::context::ContextRequest,
                _calls: Vec<ToolCall>,
            ) {
            }
        }

        let client =
            Self::create_client_with_tools(executor, config, system_prompt, NoopExecutor, vec![]);
        LLMClient {
            executor: client.executor,
            name: client.name,
            model: client.model,
            input_schema: client.input_schema,
            system_prompt: client.system_prompt,
            tools: client.tools,
            tools_state: WithoutTools,
            output_schema: client.output_schema,
            _marker: PhantomData,
        }
    }

    #[instrument(level = "trace", skip_all)]
    fn create_client_with_tools<Executor>(
        executor: std::sync::Arc<dyn synapto_interface::llm::LlmExecutor>,
        config: synapto_interface::llm::ModelConfig,
        system_prompt: Vec<Instruction>,
        tool_executor: Executor,
        tools: Vec<Tool>,
    ) -> LLMClient<Self::Content, Self::Output, WithTools<Executor>>
    where
        Executor: ToolExecutor,
    {
        let output_schema = schemars::schema_for!(Self::Output);

        // FIXME what is it
        // let mut output_value = serde_json::to_value(&output_schema).unwrap();

        // Strict JSON Schema Preservation Protocol for output commands
        // if let Some(obj) = output_value.as_object_mut()
        //     && let Some(properties) = obj.get_mut("properties").and_then(|p| p.as_object_mut())
        //     && let Some(cmd_prop) = properties
        //         .get_mut("commands")
        //         .and_then(|c| c.as_object_mut())
        //     && let Some(cmd_properties) = cmd_prop
        //         .get_mut("properties")
        //         .and_then(|cp| cp.as_object_mut())
        // {
        //     cmd_properties.remove("commands");
        // }

        // let final_output_schema: schemars::Schema =
        //     serde_json::from_value(output_value.clone()).unwrap();

        tracing::trace!(
            "Output schema: {}",
            serde_yaml::to_string(&output_schema).unwrap_or_else(|e| panic!(
                "Failed to serialize output schema: {e} | Output schema: {output_schema:?}"
            ))
        );

        let input_schema = schemars::schema_for!(Self::Content);
        let input_schema = serde_yaml::to_string(&input_schema).unwrap_or_else(|e| {
            unreachable!("Failed to serialize input schema: {e} | Input schema: {input_schema:?}")
        });

        let system_prompt_rendered = Instruction::render(&system_prompt, 0);

        let full_name = std::any::type_name::<Self>();
        let name = full_name
            .rsplit("::")
            .next()
            .unwrap_or(full_name)
            .to_string();

        LLMClient {
            executor,
            name,
            model: config.model,
            input_schema,
            system_prompt: system_prompt_rendered,
            tools,
            tools_state: WithTools(tool_executor),
            output_schema,
            _marker: PhantomData,
        }
    }
}

pub struct ConcreteLlmExecutor {
    client: genai::Client,
}

impl ConcreteLlmExecutor {
    pub fn new(config: LLMClientConfig) -> Self {
        let location = config.google_vertex_ai_location.clone();
        let account = config.google_service_account_credentials.and_then(|creds| {
            gcp_auth::CustomServiceAccount::from_json(creds.expose_secret()).ok()
        });

        let auth_resolver = if let (Some(location), Some(account)) = (location, account) {
            let project_id = config.google_project_id.clone();
            let arc_account = Arc::new(account);
            AuthResolver::from_resolver_async_fn(
                move |model: genai::ModelIden| -> std::pin::Pin<
                    Box<
                        dyn Future<Output = Result<Option<AuthData>, genai::resolver::Error>>
                            + Send
                            + 'static,
                    >,
                > {
                    let project_id = project_id.clone();
                    let location = location.clone();
                    let account = arc_account.clone();
                    Box::pin(async move {
                        let scopes = &["https://www.googleapis.com/auth/cloud-platform"];
                        let token = account
                            .token(scopes)
                            .await
                            .map_err(|e| genai::resolver::Error::Custom(e.to_string()))?;

                        let url = if location == "global" {
                            format!(
                                "https://aiplatform.googleapis.com/v1/projects/{}/locations/{}/publishers/google/models/{}:generateContent",
                                project_id, location, model.model_name
                            )
                        } else {
                            format!(
                                "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/publishers/google/models/{}:generateContent",
                                location, project_id, location, model.model_name
                            )
                        };

                        let auth_value = format!("Bearer {}", token.as_str());
                        let auth_header = genai::Headers::from(("Authorization", auth_value));
                        Ok(Some(AuthData::RequestOverride {
                            headers: auth_header,
                            url,
                        }))
                    })
                },
            )
        } else {
            let gemini_api_key = config.gemini_api_key.expect("No credentials found. Please provide either Google Service Account credentials or a Gemini API key.");
            AuthResolver::from_resolver_async_fn(
                move |_model: genai::ModelIden| -> std::pin::Pin<
                    Box<
                        dyn Future<Output = Result<Option<AuthData>, genai::resolver::Error>>
                            + Send
                            + 'static,
                    >,
                > {
                    let gemini_api_key = gemini_api_key.clone();
                    Box::pin(async move {
                        Ok(Some(AuthData::Key(gemini_api_key.expose_secret().into())))
                    })
                },
            )
        };

        let client_config = genai::ClientConfig::default().with_auth_resolver(auth_resolver);
        let client = genai::Client::builder().with_config(client_config).build();

        Self { client }
    }
}

#[async_trait]
impl synapto_interface::llm::LlmExecutor for ConcreteLlmExecutor {
    async fn execute_raw(
        &self,
        model: &str,
        system_prompt: &str,
        prompt: &str,
        options: synapto_interface::llm::RawLlmOptions,
    ) -> Result<genai::chat::ChatResponse, String> {
        let mut chat_req = if prompt.is_empty() {
            genai::chat::ChatRequest::default()
        } else {
            genai::chat::ChatRequest::from_user(prompt.to_string())
        };
        if !system_prompt.is_empty() {
            chat_req = chat_req.with_system(system_prompt.to_string());
        }

        if let Some(messages) = options.messages.clone() {
            for msg in messages {
                chat_req = chat_req.append_message(msg);
            }
        }

        let mut chat_options = genai::chat::ChatOptions::default()
            .with_temperature(0.0)
            .with_top_p(0.95);

        if let Some(ref effort) = options.reasoning_effort {
            chat_options = chat_options.with_reasoning_effort(effort.clone());
        }

        if let Some(ref output_schema) = options.output_schema {
            chat_options =
                chat_options.with_response_format(genai::chat::ChatResponseFormat::JsonSpec(
                    genai::chat::JsonSpec::new("schema", output_schema.clone()),
                ));
        }

        if let Some(tools) = options.tools
            && !tools.is_empty()
        {
            chat_req = chat_req.with_tools(tools);
        }

        if let Some(resolved_tools) = options.resolved_tools
            && !resolved_tools.is_empty()
        {
            let mut tool_calls = Vec::new();
            let mut thought_signatures = Vec::new();
            for (call, _) in &resolved_tools {
                if let Some(signatures) = &call.thought_signatures {
                    thought_signatures.extend(signatures.clone());
                }
                tool_calls.push(call.clone());
            }
            chat_req = chat_req.append_message(
                genai::chat::ChatMessage::assistant_tool_calls_with_thoughts(
                    tool_calls,
                    thought_signatures,
                ),
            );

            for (call, response) in resolved_tools {
                chat_req = chat_req.append_message(genai::chat::ChatMessage::from(
                    genai::chat::ToolResponse::new(call.call_id.clone(), response.clone()),
                ));
            }
        }

        let mut retry_count = 0;

        let response_obj = loop {
            match self
                .client
                .exec_chat(model, chat_req.clone(), Some(&chat_options))
                .await
            {
                Ok(val) => {
                    break val;
                }
                Err(err) => {
                    let err_msg = format!("{:?}", err);
                    if retry_count < 5
                        && (err_msg.contains("503")
                            || err_msg.contains("UNAVAILABLE")
                            || err_msg.contains("high demand"))
                    {
                        retry_count += 1;
                        tracing::warn!(?err, "Retry {}/5: Gemini high demand (503)", retry_count);
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue;
                    }
                    tracing::error!(?err, "Failed");
                    return Err(format!("Model call failed: {:?}", err));
                }
            }
        };

        Ok(response_obj)
    }
}
