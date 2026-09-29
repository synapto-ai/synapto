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

#[doc = " Current operational state of the side (chat) cognitive loop."]
#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub enum CognitiveSideState {
    #[doc = " The AI is actively thinking/processing."]
    Thinking,
    #[doc = " The AI is waiting for new input."]
    Idle,
}

#[doc = " Update event for the side cognitive state."]
#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct CognitiveSideStateUpdate {
    #[doc = " The context of the state update (e.g. plugin-specific metadata)."]
    pub context: serde_json::Value,
    #[doc = " The new cognitive side state."]
    pub state: CognitiveSideState,
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
