use crate::plugin::{MessageChannel, Plugin};
use crate::sync::broadcast;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[doc = " Gating evaluation of speech input before invoking the generative LLM."]
#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub enum DirectTurnEvaluation {
    #[doc = " User is hesitating or mid-sentence; waiting for further speech."]
    WaitingForMoreInput,
    #[doc = " Audio is noise, mumble, or nonsense; discarded."]
    Unintelligible,
    #[doc = " Speech was ambient or self-talk; ignored without action."]
    NonActionable,
    #[doc = " Speech is ambiguous; clarification will be requested."]
    NeedsClarification,
    #[doc = " Speech is clear and directly actionable."]
    Actionable,
}

#[doc = " Current operational state of the direct (voice) cognitive loop."]
#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub enum CognitiveDirectState {
    #[doc = " Assistant is idle and listening for input."]
    Idle,
    #[doc = " Speech evaluated by turn gating."]
    TurnGated(DirectTurnEvaluation),
    #[doc = " Assistant is running inference (thinking)."]
    Thinking,
    #[doc = " Assistant invoked a tool and is waiting for execution."]
    ToolExecuting { tool_name: String, call_id: String },
    #[doc = " Tool execution finished and returned to the cognitive loop."]
    ToolResolved {
        tool_name: String,
        call_id: String,
        is_error: bool,
    },
    #[doc = " Assistant is speaking response aloud."]
    Speaking,
}

#[doc = " State transition event emitted by the direct cognitive loop."]
#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct CognitiveDirectStateUpdate {
    pub state: CognitiveDirectState,
}

#[async_trait]
pub trait CognitiveDirectStateObserver: Plugin + Send + Sync {
    async fn start(
        &self,
        direct_state_rx: broadcast::Receiver<CognitiveDirectStateUpdate>,
    ) -> Result<(), String>;
}

#[async_trait]
pub trait CognitiveSideStateObserver: Plugin + Send + Sync {
    async fn start(
        &self,
        side_state_rx: broadcast::Receiver<CognitiveSideStateUpdate>,
    ) -> Result<(), String>;
}

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
