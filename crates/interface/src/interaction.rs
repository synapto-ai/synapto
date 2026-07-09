#![doc = include_str!("interaction.md")]

use crate::cognitive::CognitiveReasoning;
use crate::peer_input::PeerInput;
use crate::plugin::MessageChannel;
use crate::plugin::Plugin;
use crate::sync::{mpsc, watch};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct AiSpoken(pub String);

#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct AiWritten {
    pub target_channel: MessageChannel,
    pub text: String,
}

#[derive(
    Clone, Debug, serde :: Serialize, serde :: Deserialize, schemars :: JsonSchema, PartialEq, Eq,
)]
pub struct ObservedInteraction {
    pub timestamp: Timestamp,
    pub user_messages: Vec<PeerInput>,
    pub ai_spoken: Option<AiSpoken>,
    pub ai_written: Option<AiWritten>,
    pub ai_reasoning: Option<CognitiveReasoning>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, schemars :: JsonSchema)]
pub struct NotClearInteraction {
    pub timestamp: Timestamp,
    pub user_messages: Vec<PeerInput>,
    pub ai_spoken: Option<AiSpoken>,
    pub ai_written: Option<AiWritten>,
}

#[derive(
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Debug,
    Clone,
    Default,
    schemars :: JsonSchema,
    derive_more :: Deref,
    derive_more :: DerefMut,
    derive_more :: IntoIterator,
)]
pub struct NotClearInteractionMemory(pub std::collections::VecDeque<NotClearInteraction>);

impl From<Vec<NotClearInteraction>> for NotClearInteractionMemory {
    fn from(value: Vec<NotClearInteraction>) -> Self {
        Self(value.into())
    }
}

#[async_trait]
pub trait RetrospectiveConsolidationPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        not_clear_memory_rx: watch::Receiver<crate::interaction::NotClearInteractionMemory>,
        resolve_not_clear_tx: mpsc::Sender<crate::interaction::Timestamp>,
    ) -> Result<(), String>;
}

#[async_trait]
pub trait InteractionObserver: Plugin + Send + Sync {
    async fn start(
        &self,
        interaction_rx: mpsc::Receiver<crate::interaction::ObservedInteraction>,
    ) -> Result<(), String>;
}

#[derive(
    Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone, PartialOrd, Ord, Copy,
)]
pub struct Timestamp(pub i64);
