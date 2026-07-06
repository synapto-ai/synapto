use crate::plugin::MessageChannel;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[doc = " Output message intended to be spoken by the system."]
#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct CognitiveOutputSpeech {
    #[doc = " The target channel for the speech output."]
    pub target_channel: MessageChannel,
    #[doc = " The text to be spoken."]
    pub text: String,
}

#[doc = " Current operational state of the cognitive loop."]
#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub enum CognitiveState {
    #[doc = " The AI is actively thinking/processing."]
    Thinking,
    #[doc = " The AI is performing document search or RAG."]
    Searching,
    #[doc = " The AI is executing an external command."]
    Acting,
    #[doc = " The AI is waiting for new input."]
    Idle,
}

#[doc = " Update event for the system's cognitive state."]
#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct CognitiveStateUpdate {
    #[doc = " The context of the state update (e.g. plugin-specific metadata)."]
    pub context: serde_json::Value,
    #[doc = " The new cognitive state."]
    pub state: CognitiveState,
}

#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    PartialEq,
    Eq,
    Debug,
    Clone,
    derive_more :: Display,
    derive_more :: From,
    derive_more :: Deref,
)]
pub struct CognitiveReasoning(pub String);
