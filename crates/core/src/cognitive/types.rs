use std::marker::PhantomData;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use synapto_interface::llm::{LLMSafe, genai::chat::ToolCall};
use synapto_llm_client::{LLM, ToolExecutor, ToolOutput};

use crate::{
    interactions::{
        Interaction, InteractionMemory,
        recent::LLMUserMessage,
        types::{AiSpoken, CognitiveReasoning, LlmSafeInFlightTool},
    },
    utils::schema::flatten_enum,
};

#[derive(Clone)]
/// Executes tools in the background and handles the return routing to the originating cognitive loop.
///
/// **Routing Mechanism:**
/// There is no global lookup table mapping tool calls to cognitive tasks. The routing information
/// is kept entirely within the `tool_resolved_tx` channel. Because each cognitive loop (`direct` or `side`)
/// creates and passes its own unique transmitter when instantiating this executor, the background
/// task is hard-wired to wake up only the loop that spawned it.
pub struct RegistryToolExecutor {
    pub tool_resolved_tx: tokio::sync::mpsc::Sender<(ToolOutput, ToolCall)>,
    pub tools: Arc<synapto_interface::types::ToolRegistryBuilder>,
}

impl ToolExecutor for RegistryToolExecutor {
    fn execute(
        &self,
        ctx_request: synapto_interface::types::ContextRequest,
        tool_calls: Vec<ToolCall>,
    ) -> impl std::future::Future<Output = ()> + Send {
        let tool_resolved_tx = self.tool_resolved_tx.clone();
        let tools = self.tools.clone();

        async move {
            for call in tool_calls {
                if let Some(tool) = tools.get(&call.fn_name) {
                    let call_clone = call.clone();
                    let tool_resolved_tx = tool_resolved_tx.clone();
                    let ctx_req_clone = ctx_request.clone(); // needs clone? We can wrap it in arc or just clone if it's clonable...
                    // ContextRequest is relatively cheap to clone, but wait, it has Vec<ContextInteraction>.
                    // For now let's clone it.

                    tokio::spawn(async move {
                        let parsed_args_res = serde_json::from_value::<serde_json::Value>(
                            call_clone.fn_arguments.clone(),
                        );
                        match parsed_args_res {
                            Ok(args) => {
                                let exec_future = tool.erased_execute(&ctx_req_clone, args);
                                match tokio::time::timeout(
                                    std::time::Duration::from_secs(60),
                                    exec_future,
                                )
                                .await
                                {
                                    Ok(Ok(result)) => {
                                        let output = ToolOutput::new(result);
                                        tool_resolved_tx
                                            .send((output, call_clone))
                                            .await
                                            .inspect_err(|e| {
                                                tracing::error!("Channel send failed: {:?}", e)
                                            })
                                            .ok();
                                    }
                                    Ok(Err(e)) => {
                                        let output =
                                            ToolOutput::new(format!("Error executing tool: {}", e));
                                        tool_resolved_tx
                                            .send((output, call_clone))
                                            .await
                                            .inspect_err(|e| {
                                                tracing::error!("Channel send failed: {:?}", e)
                                            })
                                            .ok();
                                    }
                                    Err(_) => {
                                        let output = ToolOutput::new("Error: Execution timed out");
                                        tool_resolved_tx
                                            .send((output, call_clone))
                                            .await
                                            .inspect_err(|e| {
                                                tracing::error!("Channel send failed: {:?}", e)
                                            })
                                            .ok();
                                    }
                                }
                            }
                            Err(e) => {
                                let output =
                                    ToolOutput::new(format!("Error parsing arguments: {}", e));
                                tool_resolved_tx
                                    .send((output, call_clone))
                                    .await
                                    .inspect_err(|e| {
                                        tracing::error!("Channel send failed: {:?}", e)
                                    })
                                    .ok();
                            }
                        }
                    });
                } else {
                    let output = ToolOutput::new(format!(
                        "Error: Tool '{}' not found in registry.",
                        call.fn_name
                    ));
                    tool_resolved_tx
                        .send((output, call.clone()))
                        .await
                        .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                        .ok();
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
#[serde(rename = "Interaction")]
pub struct CognitiveLLMInteraction {
    pub user_messages: Vec<LLMUserMessage>,
    #[schemars(description = "What AI says to human")]
    pub ai_spoken: Option<AiSpoken>,
    pub ai_reasoning: Option<CognitiveReasoning>,

    #[schemars(
        description = "Tools triggered during this interaction that are currently processing in the background. If populated, the AI should acknowledge they are still working if asked, and wait for their resolution before answering questions reliant on them."
    )]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub in_flight_tools: Vec<LlmSafeInFlightTool>,
}

impl From<&Interaction> for CognitiveLLMInteraction {
    fn from(interaction: &Interaction) -> Self {
        Self {
            user_messages: interaction
                .user_messages
                .clone()
                .into_iter()
                .map(Into::into)
                .collect(),
            ai_spoken: interaction.ai_spoken.clone(),
            ai_reasoning: interaction.ai_reasoning.clone(),
            in_flight_tools: interaction.in_flight_tools.iter().map(Into::into).collect(),
        }
    }
}

#[derive(JsonSchema, Serialize, PartialEq, Eq, Debug, Clone, Default)]
#[serde(rename = "InteractionMemory")]
#[schemars(description = "Your last interactions with the user")]
pub struct CognitiveLLMInteractionMemory(pub Vec<CognitiveLLMInteraction>);

impl From<InteractionMemory> for CognitiveLLMInteractionMemory {
    fn from(value: InteractionMemory) -> Self {
        Self(value.iter().map(CognitiveLLMInteraction::from).collect())
    }
}

#[derive(JsonSchema, Serialize, Clone)]
pub struct CognitiveLLMDocument {
    pub name: String,
    pub content: String,
}

#[derive(JsonSchema, Serialize, Debug, LLMSafe)]
pub struct CognitiveLLMContent {
    // 1. The Past
    #[serde(flatten)]
    pub historical_contexts: std::collections::BTreeMap<String, serde_json::Value>,

    // 2. The Present
    #[serde(flatten)]
    pub current_contexts: std::collections::BTreeMap<String, serde_json::Value>,

    // 3. The Future / Intent
    #[serde(flatten)]
    pub prospective_contexts: std::collections::BTreeMap<String, serde_json::Value>,

    pub interaction_memory: CognitiveLLMInteractionMemory,

    pub user_messages: Vec<LLMUserMessage>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
#[schemars(transform = flatten_enum)]
#[schemars(
    description = "Only on SemanticallyClear should the LLM respond. Check that in other cases the write to chat and say commands are not used."
)]
pub enum UsersMessagesEvaluation {
    #[schemars(
        description = "All messages are clearly understandable and actionable. The underlying meaning is unambiguous. You will respond and the input buffer will be cleared."
    )]
    Actionable,

    #[schemars(
        description = "Use this when the user's input is a complete thought but requires no action or response from you. This includes ambient discussion, self-talk, when you are not explicitly addressed, or when a response would merely be an acknowledgment."
    )]
    NonActionable,

    #[schemars(
        description = "Discontinued sentence, incomplete thought, OR you are deliberately waiting for other users to speak before acting. The current input will be held in the buffer to be combined with future inputs."
    )]
    WaitingForMoreInput,

    #[schemars(
        description = "All messages are not meaningful language due to mumbles, stutters, or garbling. The input will be discarded entirely."
    )]
    Unintelligible,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct CognitiveLLMOutput<CognitiveCommands> {
    pub commands: CognitiveCommands,
    pub reasoning: CognitiveReasoning,

    #[schemars(
        description = "Evaluation of the users' messages based on their semantic clarity, completeness, and overall intelligibility. Only on Actionable should the LLM respond. Check that in other cases the write to chat and say commands are not used."
    )]
    pub users_messages_evaluation: UsersMessagesEvaluation,
}

pub struct CognitiveLLM<CognitiveCommands> {
    _marker: PhantomData<CognitiveCommands>,
}

impl<CognitiveCommands: LLMSafe + Clone + DeserializeOwned + JsonSchema> LLM
    for CognitiveLLM<CognitiveCommands>
{
    type Content = CognitiveLLMContent;
    type Output = CognitiveLLMOutput<CognitiveCommands>;
}

pub async fn evaluate_dynamic_tools(
    tools: &synapto_interface::types::ToolRegistryBuilder,
    request: &synapto_interface::types::ContextRequest,
    content_value: &serde_json::Value,
) -> Vec<synapto_interface::llm::genai::chat::Tool> {
    let available_tools_erased = tools.get_all();
    let mut dynamic_tools = vec![];
    for tool in available_tools_erased {
        if tool
            .erased_is_available(request, content_value)
            .await
            .unwrap_or(false)
        {
            let mut schema = serde_json::to_value(tool.schema())
                .unwrap_or_else(|e| panic!("Failed to serialize tool schema: {}", e));
            if let serde_json::Value::Object(ref mut map) = schema {
                map.remove("$schema");
            }
            dynamic_tools.push(
                synapto_interface::llm::genai::chat::Tool::new(tool.name())
                    .with_description(tool.description())
                    .with_schema(schema),
            );
        }
    }
    dynamic_tools
}
