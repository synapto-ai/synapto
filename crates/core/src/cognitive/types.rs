use std::marker::PhantomData;
use std::sync::Arc;
use synapto_interface::cognitive::{
    CognitiveDirectState, CognitiveDirectStateUpdate, CognitiveReasoning,
};
use synapto_interface::document::DocumentId;
use synapto_interface::interaction::CognitiveSpoken;
use synapto_interface::peer_input::MessageText;
use synapto_interface::peer_input::{PeerInput, Speaker};
use synapto_interface::peer_input_text::SenderId;
use synapto_interface::plugin::MessageChannel;
use synapto_interface::sync::broadcast;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use synapto_interface::llm::{LLMSafe, genai::chat::ToolCall};
use synapto_llm::{LLM, ToolExecutor, ToolOutput};

use crate::{
    interactions::{InFlightTool, Interaction, InteractionMemory, SpeakerName},
    users::Users,
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
pub(super) struct RegistryToolExecutor {
    pub tool_resolved_tx: tokio::sync::mpsc::Sender<(ToolOutput, ToolCall)>,
    pub tools: Arc<synapto_interface::tool::ToolRegistryBuilder>,
    pub cognitive_direct_state_tx: Option<broadcast::Sender<CognitiveDirectStateUpdate>>,
}

impl ToolExecutor for RegistryToolExecutor {
    fn execute(
        &self,
        ctx_request: synapto_interface::context::ContextRequest,
        tool_calls: Vec<ToolCall>,
    ) -> impl std::future::Future<Output = ()> + Send {
        let tool_resolved_tx = self.tool_resolved_tx.clone();
        let tools = self.tools.clone();
        let cognitive_direct_state_tx = self.cognitive_direct_state_tx.clone();

        async move {
            for call in tool_calls {
                if let Some(ref tx) = cognitive_direct_state_tx {
                    tx.send(CognitiveDirectStateUpdate {
                        state: CognitiveDirectState::ToolExecuting {
                            tool_name: call.fn_name.clone(),
                            call_id: call.call_id.clone(),
                        },
                    })
                    .ok();
                }

                if let Some(tool) = tools.get(&call.fn_name) {
                    let call_clone = call.clone();
                    let tool_resolved_tx = tool_resolved_tx.clone();
                    let ctx_req_clone = ctx_request.clone();
                    let cognitive_direct_state_tx_clone = cognitive_direct_state_tx.clone();

                    tokio::spawn(async move {
                        tracing::debug!("Tool called: {}", call_clone.fn_name);

                        // Attempt to parse the arguments provided by the LLM into a standard JSON Value
                        let parsed_args_res = serde_json::from_value::<serde_json::Value>(
                            call_clone.fn_arguments.clone(),
                        );

                        match parsed_args_res {
                            Ok(args) => {
                                // Execute the tool with a timeout
                                let exec_future = tool.erased_execute(&ctx_req_clone, args);
                                let exec_res = tokio::time::timeout(
                                    std::time::Duration::from_secs(60),
                                    exec_future,
                                )
                                .await;

                                tracing::debug!("Tool resolved: {}", call_clone.fn_name);

                                let is_error = !matches!(exec_res, Ok(Ok(_)));
                                if let Some(ref tx) = cognitive_direct_state_tx_clone {
                                    tx.send(CognitiveDirectStateUpdate {
                                        state: CognitiveDirectState::ToolResolved {
                                            tool_name: call_clone.fn_name.clone(),
                                            call_id: call_clone.call_id.clone(),
                                            is_error,
                                        },
                                    })
                                    .ok();
                                }

                                match exec_res {
                                    Ok(Ok(result)) => {
                                        // Happy path: Tool executed successfully
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
                                        // Tool-side error: The tool itself failed during execution (behaved badly)
                                        tracing::warn!(
                                            "Tool '{}' behaved badly/execution error: {}",
                                            call_clone.fn_name,
                                            e
                                        );
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
                                        // Timeout error: The tool took longer than 60 seconds
                                        tracing::warn!(
                                            "Tool execution timed out for '{}'",
                                            call_clone.fn_name
                                        );
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
                                // LLM-side error: The LLM hallucinated bad arguments or formatted them incorrectly
                                tracing::warn!(
                                    "LLM called tool '{}' incorrectly: {}",
                                    call_clone.fn_name,
                                    e
                                );
                                if let Some(ref tx) = cognitive_direct_state_tx_clone {
                                    tx.send(CognitiveDirectStateUpdate {
                                        state: CognitiveDirectState::ToolResolved {
                                            tool_name: call_clone.fn_name.clone(),
                                            call_id: call_clone.call_id.clone(),
                                            is_error: true,
                                        },
                                    })
                                    .ok();
                                }
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
                    // LLM-side error: The LLM tried to invoke a tool that doesn't exist in the registry
                    tracing::warn!("LLM tried to call unknown tool: '{}'", call.fn_name);
                    if let Some(ref tx) = cognitive_direct_state_tx {
                        tx.send(CognitiveDirectStateUpdate {
                            state: CognitiveDirectState::ToolResolved {
                                tool_name: call.fn_name.clone(),
                                call_id: call.call_id.clone(),
                                is_error: true,
                            },
                        })
                        .ok();
                    }
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
pub enum LLMUser {
    /// A known person (e.g., John Doe)
    /// We have a real name for them.
    Known(SpeakerName),

    /// An unknown but distinguishable person
    /// We don't know their name, but we know that "OS456" from document A
    /// is the same person as "OS456" from document B.
    Distinguishable(SpeakerName), // e.g., "OS456"

    /// An unknown and indistinguishable person
    /// For example, a voice from a crowd. In the next sentence, "another voice" could be someone completely different.
    Indistinguishable,
}

impl From<Speaker> for LLMUser {
    fn from(speaker: Speaker) -> Self {
        match speaker {
            Speaker::Unknown(_) => LLMUser::Indistinguishable,
            Speaker::Recognized(speaker_id) => match Users::get_by_speaker_id(&speaker_id) {
                Some(user) => LLMUser::Known(SpeakerName(user.full_name)),
                None => LLMUser::Distinguishable(SpeakerName(format!("Some user {}", speaker_id))),
            },
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub enum LLMUserMessage {
    Speech {
        speaker: LLMUser,
        transcript: MessageText,
    },
    Text {
        channel: MessageChannel,
        sender: SenderId,
        text: MessageText,
        attached_documents: Vec<DocumentId>,
        #[schemars(
            description = "True if the message was explicitly addressed to the assistant (e.g. via direct message or @mention). If this is true, the assistant is invoked and should respond."
        )]
        explicitly_addressed: bool,
    },
}

impl From<PeerInput> for LLMUserMessage {
    fn from(user_message: PeerInput) -> Self {
        match user_message {
            PeerInput::Speech(speech) => LLMUserMessage::Speech {
                speaker: speech.speaker.into(),
                transcript: speech.transcript,
            },
            PeerInput::Text(text_msg) => LLMUserMessage::Text {
                channel: text_msg.channel,
                sender: text_msg.sender_id,
                text: text_msg.text,
                attached_documents: text_msg.attached_documents,
                explicitly_addressed: text_msg.explicitly_addressed,
            },
        }
    }
}

#[derive(
    Clone, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq, LLMSafe,
)]
struct LlmSafeInFlightTool {
    name: String,
    arguments: serde_json::Value,
}

impl From<&InFlightTool> for LlmSafeInFlightTool {
    fn from(tool: &InFlightTool) -> Self {
        Self {
            name: tool.name.clone(),
            arguments: tool.arguments.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
#[serde(rename = "Interaction")]
struct CognitiveLLMInteraction {
    user_messages: Vec<LLMUserMessage>,

    #[schemars(
        description = "The communication channel context for this interaction. Use this for the 'write' command target_channel when responding to resolved_tools from this interaction."
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<MessageChannel>,

    #[schemars(description = "What AI says to human")]
    cognitive_spoken: Option<CognitiveSpoken>,
    cognitive_reasoning: Option<CognitiveReasoning>,

    #[schemars(
        description = "Tools triggered during this interaction that are currently processing in the background. If populated, the AI should acknowledge they are still working if asked, and wait for their resolution before answering questions reliant on them."
    )]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    in_flight_tools: Vec<LlmSafeInFlightTool>,

    #[schemars(
        description = "Tools triggered during this interaction that have just resolved this turn. You MUST fulfill the user's original request based on these results and ongoing conversation."
    )]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    resolved_tools: Vec<LlmSafeInFlightTool>,
}

impl From<&Interaction> for CognitiveLLMInteraction {
    fn from(interaction: &Interaction) -> Self {
        let channel = interaction.user_messages.iter().find_map(|msg| match msg {
            PeerInput::Text(t) => Some(t.channel.clone()),
            _ => None,
        });

        Self {
            user_messages: interaction
                .user_messages
                .clone()
                .into_iter()
                .map(Into::into)
                .collect(),
            channel,
            cognitive_spoken: interaction.cognitive_spoken.clone(),
            cognitive_reasoning: interaction.cognitive_reasoning.clone(),
            in_flight_tools: interaction.in_flight_tools.iter().map(Into::into).collect(),
            resolved_tools: interaction.resolved_tools.iter().map(Into::into).collect(),
        }
    }
}

#[derive(JsonSchema, Serialize, PartialEq, Eq, Debug, Clone, Default)]
#[serde(rename = "InteractionMemory")]
#[schemars(description = "Your last interactions with the user")]
pub struct CognitiveLLMInteractionMemory(Vec<CognitiveLLMInteraction>);

impl From<InteractionMemory> for CognitiveLLMInteractionMemory {
    fn from(value: InteractionMemory) -> Self {
        Self(value.iter().map(CognitiveLLMInteraction::from).collect())
    }
}

// #[derive(JsonSchema, Serialize, Clone)]
// struct CognitiveLLMDocument {
//     name: String,
//     content: String,
// }

#[derive(JsonSchema, Serialize, Debug, LLMSafe)]
pub struct CognitiveLLMContent {
    #[serde(flatten)]
    pub historical_contexts: std::collections::BTreeMap<String, serde_json::Value>,

    #[serde(flatten)]
    pub current_contexts: std::collections::BTreeMap<String, serde_json::Value>,

    #[serde(flatten)]
    pub prospective_contexts: std::collections::BTreeMap<String, serde_json::Value>,

    pub interaction_memory: CognitiveLLMInteractionMemory,

    pub user_messages: Vec<LLMUserMessage>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
#[schemars(transform = flatten_enum)]
#[schemars(
    description = "Evaluation of active user_messages. Determines if the newly arrived messages require a response."
)]
pub(super) enum UsersMessagesEvaluation {
    #[schemars(
        description = "All active messages are clearly understandable, complete, and directly actionable."
    )]
    Actionable,

    #[schemars(
        description = "The user has finished speaking, but the input is ambiguous, distorted, or incomplete, requiring a clarification question or confirmation back to the user before proceeding."
    )]
    NeedsClarification,

    #[schemars(
        description = "The active user_messages require no action or response from you. Use this when the user buffer is empty, or if they are ambient discussion or self-talk."
    )]
    NonActionable,

    #[schemars(
        description = "The user is currently speaking, hesitating mid-sentence, or paused mid-thought. You must REMAIN SILENT to avoid interrupting. Output commands will be dropped."
    )]
    WaitingForMoreInput,

    #[schemars(
        description = "All messages are not meaningful language due to audio noise, mumbles, or stutters. The input will be discarded entirely."
    )]
    Unintelligible,
}

impl UsersMessagesEvaluation {
    pub(super) fn from_choice(choice: &str) -> Option<Self> {
        match choice {
            "Actionable" => Some(Self::Actionable),
            "NeedsClarification" => Some(Self::NeedsClarification),
            "NonActionable" => Some(Self::NonActionable),
            "WaitingForMoreInput" => Some(Self::WaitingForMoreInput),
            "Unintelligible" => Some(Self::Unintelligible),
            _ => None,
        }
    }

    pub(super) fn to_choice_question() -> synapto_interface::decision::ChoiceQuestion {
        let schema = schemars::schema_for!(Self);
        let mut criteria = std::collections::BTreeMap::new();

        let one_of = schema
            .get("oneOf")
            .and_then(|v| v.as_array())
            .expect("UsersMessagesEvaluation schema must have oneOf variants");

        for variant in one_of {
            let variant_obj = variant
                .as_object()
                .expect("Variant in oneOf must be an object");
            let name = variant_obj
                .get("const")
                .and_then(|v| v.as_str())
                .expect("Variant in oneOf must have const string name");
            let desc = variant_obj
                .get("description")
                .and_then(|v| v.as_str())
                .expect("Variant in oneOf must have description");

            criteria.insert(name.to_string(), desc.to_string());
        }

        let instructions = schema
            .get("description")
            .and_then(|v| v.as_str())
            .and_then(|d| d.split("\n\nOutput exactly ONE").next())
            .unwrap_or("Evaluation of active user_messages. Determines if the newly arrived messages require a response.")
            .trim()
            .to_string();

        synapto_interface::decision::ChoiceQuestion {
            instructions,
            criteria,
        }
    }
}

pub(super) fn generate_turn_evaluation_question() -> synapto_interface::decision::ChoiceQuestion {
    UsersMessagesEvaluation::to_choice_question()
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub(super) struct CognitiveLLMOutput<CognitiveCommands> {
    pub commands: CognitiveCommands,
    pub reasoning: CognitiveReasoning,

    #[schemars(
        description = "Evaluation of the active user_messages. Use Actionable when fulfilling a clear request, or NeedsClarification when asking a clarification question. If there are no active messages and no output commands, choose NonActionable or WaitingForMoreInput. Output commands (say, write) will be dropped if you choose WaitingForMoreInput, NonActionable, or Unintelligible (unless fulfilling a resolved tool)."
    )]
    pub users_messages_evaluation: UsersMessagesEvaluation,
}

pub(super) struct CognitiveLLM<CognitiveCommands> {
    _marker: PhantomData<CognitiveCommands>,
}

impl<CognitiveCommands: LLMSafe + Clone + DeserializeOwned + JsonSchema> LLM
    for CognitiveLLM<CognitiveCommands>
{
    type Content = CognitiveLLMContent;
    type Output = CognitiveLLMOutput<CognitiveCommands>;
}

pub(super) async fn evaluate_dynamic_tools(
    tools: &synapto_interface::tool::ToolRegistryBuilder,
    request: &synapto_interface::context::ContextRequest,
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
            clean_json_schema_for_llm(&mut schema);
            dynamic_tools.push(
                synapto_interface::llm::genai::chat::Tool::new(tool.name())
                    .with_description(tool.description())
                    .with_schema(schema),
            );
        }
    }
    dynamic_tools
}

pub(super) fn clean_json_schema_for_llm(val: &mut serde_json::Value) {
    const ALLOWED_KEYS: &[&str] = &[
        "type",
        "properties",
        "required",
        "items",
        "description",
        "enum",
        "nullable",
        "format",
        "anyOf",
        "oneOf",
    ];

    match val {
        serde_json::Value::Object(map) => {
            // If the schema uses anyOf or oneOf, simplify nullable types like [ { "type": "object", ... }, { "type": "null" } ]
            for union_key in ["anyOf", "oneOf"] {
                let mut should_set_nullable = false;
                let mut inlined_variant = None;

                if let Some(serde_json::Value::Array(variants)) = map.get_mut(union_key) {
                    let has_null = variants.iter().any(|v| {
                        v.as_object()
                            .and_then(|o| o.get("type"))
                            .and_then(|t| t.as_str())
                            == Some("null")
                    });
                    if has_null {
                        variants.retain(|v| {
                            v.as_object()
                                .and_then(|o| o.get("type"))
                                .and_then(|t| t.as_str())
                                != Some("null")
                        });
                        should_set_nullable = true;
                    }

                    if variants.len() == 1 {
                        inlined_variant = Some(variants.remove(0));
                    }
                }

                if should_set_nullable {
                    map.insert("nullable".to_string(), serde_json::Value::Bool(true));
                }

                if let Some(single_variant) = inlined_variant {
                    map.remove(union_key);
                    if let serde_json::Value::Object(inner) = single_variant {
                        for (k, v) in inner {
                            map.entry(k).or_insert(v);
                        }
                    }
                }
            }

            map.retain(|k, _| ALLOWED_KEYS.contains(&k.as_str()));

            if let Some(serde_json::Value::Object(props)) = map.get_mut("properties") {
                for prop_val in props.values_mut() {
                    clean_json_schema_for_llm(prop_val);
                }
            }

            if let Some(items_val) = map.get_mut("items") {
                clean_json_schema_for_llm(items_val);
            }

            if let Some(serde_json::Value::Array(any_of)) = map.get_mut("anyOf") {
                for v in any_of {
                    clean_json_schema_for_llm(v);
                }
            }

            if let Some(serde_json::Value::Array(one_of)) = map.get_mut("oneOf") {
                for v in one_of {
                    clean_json_schema_for_llm(v);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                clean_json_schema_for_llm(v);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_clean_json_schema_for_llm() {
        let mut schema = serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "title": "SerpApiSearchTool",
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "params": {
                    "type": "object",
                    "additionalProperties": true,
                    "default": null,
                    "description": "Engine parameters"
                }
            },
            "required": ["params"]
        });

        clean_json_schema_for_llm(&mut schema);

        assert_eq!(
            schema,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "params": {
                        "type": "object",
                        "description": "Engine parameters"
                    }
                },
                "required": ["params"]
            })
        );
    }

    #[test]
    fn test_clean_json_schema_for_llm_anyof_nullable() {
        let mut schema = serde_json::json!({
            "type": "object",
            "properties": {
                "params": {
                    "anyOf": [
                        { "type": "object", "additionalProperties": true },
                        { "type": "null" }
                    ],
                    "description": "Dictionary of SerpApi engine-specific parameters."
                }
            },
            "required": ["params"]
        });

        clean_json_schema_for_llm(&mut schema);

        assert_eq!(
            schema,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "params": {
                        "type": "object",
                        "nullable": true,
                        "description": "Dictionary of SerpApi engine-specific parameters."
                    }
                },
                "required": ["params"]
            })
        );
    }

    #[test]
    fn test_generate_turn_evaluation_question() {
        let question = generate_turn_evaluation_question();
        assert!(
            question
                .instructions
                .contains("Evaluation of active user_messages")
        );
        assert_eq!(question.criteria.len(), 5);
        assert!(question.criteria.contains_key("Actionable"));
        assert!(question.criteria.contains_key("NeedsClarification"));
        assert!(question.criteria.contains_key("WaitingForMoreInput"));
        assert!(question.criteria.contains_key("NonActionable"));
        assert!(question.criteria.contains_key("Unintelligible"));
    }
}
