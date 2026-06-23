use synapto_interface::{llm::LLMSafe, sync::{mpsc, watch}};
use tracing::instrument;

use crate::{config::Config, users::Users};

use super::types::{
    DocumentId, Interaction, MessageChannel, MessageText, PeerInput, SenderId, Speaker, Timestamp,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cognitive::CognitiveLLMInteraction;

#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    PartialEq,
    Eq,
    Debug,
    Clone,
    derive_more::From,
    derive_more::Deref,
)]
pub struct SpeakerName(pub String);

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

#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct SummaryLLMInteraction {
    pub timestamp: Timestamp,
    pub interaction: CognitiveLLMInteraction,
}

impl From<&Interaction> for SummaryLLMInteraction {
    fn from(interaction: &Interaction) -> Self {
        Self {
            timestamp: interaction.timestamp,
            interaction: CognitiveLLMInteraction::from(interaction),
        }
    }
}

#[derive(
    derive_more::IntoIterator,
    derive_more::Deref,
    derive_more::DerefMut,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Debug,
    Clone,
    Default,
)]
pub struct InteractionMemory(pub std::collections::VecDeque<Interaction>);

impl InteractionMemory {
    pub fn resolve_in_flight_tool(&mut self, tool_call_id: &str) -> Result<(), String> {
        let mut found = false;
        for interaction in self.0.iter_mut() {
            if let Some(idx) = interaction
                .in_flight_tools
                .iter()
                .position(|t| t.id == tool_call_id)
            {
                interaction.in_flight_tools.remove(idx);
                found = true;
            }
        }
        if found {
            Ok(())
        } else {
            Err(format!("In-flight tool with id {} not found", tool_call_id))
        }
    }
}

// Helper to wait for any watch receiver to signal a change
async fn wait_for_any_rollout(
    receivers: &mut [(String, watch::Receiver<Timestamp>)],
) -> Result<(), (String, watch::error::RecvError)> {
    if receivers.is_empty() {
        std::future::pending::<Result<(), (String, watch::error::RecvError)>>().await
    } else {
        let futures = receivers
            .iter_mut()
            .map(|(name, rx)| {
                let name = name.clone();
                #[allow(clippy::type_complexity)]
                let f: std::pin::Pin<
                    Box<
                        dyn std::future::Future<
                                Output = Result<(), (String, watch::error::RecvError)>,
                            > + Send,
                    >,
                > = Box::pin(async move { rx.changed().await.map_err(|e| (name, e)) });
                f
            })
            .collect::<Vec<_>>();

        let (res, _, _) = futures::future::select_all(futures).await;
        res
    }
}

#[instrument(skip_all, fields(subsystem))]
pub async fn interaction_memory_task(
    config: Config,
    mut new_interaction_rx: mpsc::Receiver<Interaction>,
    mut rollout_receivers: Vec<(String, watch::Receiver<Timestamp>)>,
    observers_tx: Vec<mpsc::Sender<synapto_interface::types::ObservedInteraction>>,
    interaction_memory_tx: watch::Sender<InteractionMemory>,
    not_clear_tx: mpsc::Sender<super::NotClearInteraction>,
    mut resolve_in_flight_tool_rx: mpsc::Receiver<synapto_interface::types::ToolCallId>,
) {
    let memory_dir = config.data_dir.join("memory");
    if let Err(e) = tokio::fs::create_dir_all(&memory_dir).await {
        tracing::error!("Failed to create memory directory: {:?}", e);
    }
    let interaction_memory_file = memory_dir.join("interactions.json");

    let mut interaction_memory: InteractionMemory =
        if let Ok(content) = tokio::fs::read_to_string(&interaction_memory_file).await {
            serde_json::from_str::<InteractionMemory>(&content)
                .unwrap_or_else(|e| panic!("Failed to deserialize interaction memory: {}", e))
        } else {
            InteractionMemory::default()
        };

    interaction_memory_tx.send_replace(interaction_memory.clone());

    let last_sent_file = memory_dir.join("last_sent_timestamp.json");
    let mut last_sent_timestamp: Option<Timestamp> =
        if let Ok(content) = tokio::fs::read_to_string(&last_sent_file).await {
            serde_json::from_str(&content).ok()
        } else {
            // Fallback for backward compatibility
            if interaction_memory.len() > 8 {
                Some(interaction_memory.0[interaction_memory.len() - 8 - 1].timestamp)
            } else {
                None
            }
        };

    loop {
        let mut did_add = false;
        let do_rollout_timestamp = tokio::select! {
            res = resolve_in_flight_tool_rx.recv() => {
                if let Some(tool_call_id) = res {
                    if let Err(e) = interaction_memory.resolve_in_flight_tool(&tool_call_id) {
                        tracing::warn!("Failed to resolve in-flight tool marker: {}", e);
                    } else {
                        // send an update to subscribers when memory changes
                        interaction_memory_tx.send(interaction_memory.clone()).inspect_err(|e| tracing::error!("Channel send failed: {:?}", e)).ok();
                    }
                }
                false
            }
            res = new_interaction_rx.recv() => {
                match res {
                    Some(new_interaction) => {
                        interaction_memory.push_back(new_interaction);
                        did_add = true;
                        false
                    }
                    None => {
                        tracing::error!("new_interaction_rx closed");
                        return;
                    }
                }
            }
            res = wait_for_any_rollout(&mut rollout_receivers) => {
                if let Err((name, e)) = res {
                    tracing::error!("A rollout receiver has closed for plugin {}: {}", name, e);
                    return;
                }
                true
            }
        };

        let mut did_rollout = false;
        if do_rollout_timestamp {
            let timestamp = rollout_receivers
                .iter()
                .map(|(_, rx)| *rx.borrow())
                .min()
                .expect(
                    "Rollout timestamp can happen only if there is at least one rollout receiver.",
                );

            let original_len = interaction_memory.len();

            let mut not_clear_to_send = Vec::new();

            interaction_memory.retain(|interaction| {
                if interaction.timestamp > timestamp {
                    true
                } else {
                    if !interaction.is_actionable {
                        not_clear_to_send.push(super::NotClearInteraction::from(interaction));
                    }
                    false
                }
            });

            for interaction in not_clear_to_send {
                if let Err(e) = not_clear_tx.send(interaction).await {
                    tracing::error!("Failed to forward not-clear interaction: {:?}", e);
                }
            }

            if interaction_memory.len() < original_len {
                did_rollout = true;
            }
        }

        // --- Delayed Dispatch (Buffering) ---
        // We only dispatch interactions to the observer plugins if there are no in-flight tools.
        // This forces the observer plugins to naturally pause, anchoring the rollout window.
        // When the tools finally resolve, this buffer flushes instantly, creating a massive
        // batch that the plugins process highly efficiently.
        let has_in_flight = interaction_memory
            .0
            .iter()
            .any(|i| !i.in_flight_tools.is_empty());
        let mut did_dispatch = false;

        if !has_in_flight && interaction_memory.len() > 8 {
            let end_index = interaction_memory.len() - 8;
            for i in 0..end_index {
                let interaction = &interaction_memory[i];
                let is_new = match last_sent_timestamp {
                    Some(ts) => interaction.timestamp > ts,
                    None => true,
                };
                if is_new {
                    for tx in &observers_tx {
                        tx.send(synapto_interface::types::ObservedInteraction::from(
                            interaction,
                        ))
                        .await
                        .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                        .ok();
                    }
                    last_sent_timestamp = Some(interaction.timestamp);
                    did_dispatch = true;
                }
            }
        }

        if !do_rollout_timestamp || did_rollout || did_dispatch || did_add {
            if did_rollout || did_dispatch || did_add {
                interaction_memory_tx
                    .send(interaction_memory.clone())
                    .inspect_err(|e| tracing::error!("{}", e))
                    .ok();
            }
            if let Err(e) = tokio::fs::write(
                &interaction_memory_file,
                serde_json::to_string_pretty(&interaction_memory)
                    .unwrap_or_else(|e| panic!("Failed to serialize interaction memory: {}", e)),
            )
            .await
            {
                tracing::error!("Failed to write memory: {:?}", e);
            }

            if did_dispatch {
                tokio::fs::write(
                    &last_sent_file,
                    serde_json::to_string_pretty(&last_sent_timestamp).unwrap_or_else(|e| {
                        panic!("Failed to serialize last sent timestamp: {}", e)
                    }),
                )
                .await
                .inspect_err(|e| tracing::error!("Failed to write last_sent_timestamp: {:?}", e))
                .ok();
            }
        }
    }
}

#[derive(Serialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct LLMInteractionMemory(pub Vec<SummaryLLMInteraction>);

impl From<InteractionMemory> for LLMInteractionMemory {
    fn from(value: InteractionMemory) -> Self {
        Self(value.iter().map(SummaryLLMInteraction::from).collect())
    }
}

pub struct InteractionMemoryContextProvider {
    interaction_memory_rx: watch::Receiver<InteractionMemory>,
}

impl InteractionMemoryContextProvider {
    pub fn new(interaction_memory_rx: watch::Receiver<InteractionMemory>) -> Self {
        Self {
            interaction_memory_rx,
        }
    }
}

#[async_trait::async_trait]
impl synapto_interface::types::ContextProvider for InteractionMemoryContextProvider {
    type Context = LLMInteractionMemory;
    const NAME: &'static str = "interaction_memory";
    const SCOPE: synapto_interface::types::TemporalScope =
        synapto_interface::types::TemporalScope::Current;

    async fn context(
        &self,
        _request: &synapto_interface::types::ContextRequest,
    ) -> Result<Self::Context, String> {
        let mem = self.interaction_memory_rx.borrow().clone();
        Ok(LLMInteractionMemory::from(mem))
    }

    fn subscribe(&self) -> Option<tokio::sync::watch::Receiver<()>> {
        let mut rx = self.interaction_memory_rx.clone();
        let (tx, out_rx) = tokio::sync::watch::channel(());
        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                tx.send(())
                    .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                    .ok();
            }
        });
        Some(out_rx)
    }
}
