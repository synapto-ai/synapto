pub use synapto_interface::cognitive_output_text::types::CognitiveOutputText;
pub use synapto_interface::peer_input_text::types::PeerInputText;
pub use synapto_interface::types::{
    AiSpoken, AiWritten, CognitiveOutputSpeech, CognitiveReasoning, CognitiveState,
    CognitiveStateUpdate, DocumentId, MessageChannel, MessageId, MessageText, NotClearInteraction,
    NotClearInteractionMemory, ObservedInteraction, PeerInput, PeerInputSpeech, SenderId, SpaceId,
    Speaker, SpeakerId, ThreadId, Timestamp,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct InFlightTool {
    pub id: String,   // The thought_signature or tool_call_id
    pub name: String, // The fn_name
    pub arguments: serde_json::Value,
}

#[derive(
    Clone, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct LlmSafeInFlightTool {
    pub name: String,
    pub arguments: serde_json::Value,
}

impl synapto_interface::llm::LLMSafe for LlmSafeInFlightTool {}

impl From<&InFlightTool> for LlmSafeInFlightTool {
    fn from(tool: &InFlightTool) -> Self {
        Self {
            name: tool.name.clone(),
            arguments: tool.arguments.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct Interaction {
    pub timestamp: Timestamp,
    pub user_messages: Vec<PeerInput>,
    pub ai_spoken: Option<AiSpoken>,
    pub ai_written: Option<AiWritten>,
    pub ai_reasoning: Option<CognitiveReasoning>,
    pub is_actionable: bool,
    #[serde(skip)]
    pub in_flight_tools: Vec<InFlightTool>,
}

synapto_interface::register_channel_name!(Interaction, "interaction");

impl Interaction {
    pub fn new(
        user_messages: Vec<PeerInput>,
        ai_spoken: Option<AiSpoken>,
        ai_written: Option<AiWritten>,
        ai_reasoning: Option<CognitiveReasoning>,
        is_actionable: bool,
        in_flight_tools: Vec<InFlightTool>,
    ) -> Self {
        Self {
            timestamp: Timestamp(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_else(|e| panic!("System clock was before UNIX EPOCH: {}", e))
                    .as_millis() as i64,
            ),
            user_messages,
            ai_spoken,
            ai_written,
            ai_reasoning,
            is_actionable,
            in_flight_tools,
        }
    }
}

impl From<&Interaction> for ObservedInteraction {
    fn from(interaction: &Interaction) -> Self {
        Self {
            timestamp: interaction.timestamp,
            user_messages: interaction.user_messages.clone(),
            ai_spoken: interaction.ai_spoken.clone(),
            ai_written: interaction.ai_written.clone(),
            ai_reasoning: interaction.ai_reasoning.clone(),
        }
    }
}

impl From<&Interaction> for NotClearInteraction {
    fn from(interaction: &Interaction) -> Self {
        Self {
            timestamp: interaction.timestamp,
            user_messages: interaction.user_messages.clone(),
            ai_spoken: interaction.ai_spoken.clone(),
            ai_written: interaction.ai_written.clone(),
        }
    }
}
