use synapto_interface::cognitive::CognitiveReasoning;
use synapto_interface::interaction::{AiSpoken, AiWritten, NotClearInteraction};
use synapto_interface::peer_input::PeerInput;
use synapto_interface::{
    interaction::ObservedInteraction,
    interaction::Timestamp,
    sync::{mpsc, watch},
};
use tracing::instrument;

use crate::config::Config;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// use crate::cognitive::CognitiveLLMInteraction;

#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub(crate) struct InFlightTool {
    pub id: String,   // The thought_signature or tool_call_id
    pub name: String, // The fn_name
    pub arguments: serde_json::Value,
}

#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub(crate) struct Interaction {
    pub timestamp: Timestamp,
    pub user_messages: Vec<PeerInput>,
    pub ai_spoken: Option<AiSpoken>,
    pub ai_written: Option<AiWritten>,
    pub ai_reasoning: Option<CognitiveReasoning>,
    is_actionable: bool,
    #[serde(skip)]
    pub in_flight_tools: Vec<InFlightTool>,
    #[serde(skip)]
    pub resolved_tools: Vec<InFlightTool>,
}

impl Interaction {
    pub(crate) fn new(
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
            resolved_tools: Vec::new(),
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

// #[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
// struct SummaryLLMInteraction {
//     timestamp: Timestamp,
//     interaction: CognitiveLLMInteraction,
// }

// impl From<&Interaction> for SummaryLLMInteraction {
//     fn from(interaction: &Interaction) -> Self {
//         Self {
//             timestamp: interaction.timestamp,
//             interaction: CognitiveLLMInteraction::from(interaction),
//         }
//     }
// }

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
pub(crate) struct InteractionMemory(std::collections::VecDeque<Interaction>);

impl InteractionMemory {
    fn resolve_in_flight_tool(&mut self, tool_call_id: &str) -> Result<(), String> {
        let mut found = false;
        for interaction in self.0.iter_mut() {
            if let Some(idx) = interaction
                .in_flight_tools
                .iter()
                .position(|t| t.id == *tool_call_id)
            {
                let tool = interaction.in_flight_tools.remove(idx);
                interaction.resolved_tools.push(tool);
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
pub(super) async fn interaction_memory_task<
    S: synapto_interface::storage::KeyValueStore + synapto_interface::storage::RecordStore,
>(
    _config: Config,
    mut new_interaction_rx: mpsc::Receiver<Interaction>,
    mut rollout_receivers: Vec<(String, watch::Receiver<Timestamp>)>,
    observers_tx: Vec<mpsc::Sender<synapto_interface::interaction::ObservedInteraction>>,
    interaction_memory_tx: watch::Sender<InteractionMemory>,
    not_clear_tx: mpsc::Sender<NotClearInteraction>,
    mut resolve_in_flight_tool_rx: mpsc::Receiver<synapto_interface::tool::ToolCallId>,
    storage: std::sync::Arc<S>,
) {
    let mut interaction_memory: InteractionMemory = if let Ok(records) = storage
        .get_ordered_records::<Interaction>("interactions", None, false)
        .await
    {
        InteractionMemory(records.into_iter().map(|(_, v)| v).collect())
    } else {
        InteractionMemory::default()
    };

    interaction_memory_tx.send_replace(interaction_memory.clone());

    let mut last_sent_timestamp: Option<Timestamp> = if let Ok(Some(ts)) = storage
        .get::<Timestamp>("memory", "last_sent_timestamp")
        .await
    {
        Some(ts)
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
        let do_rollout_timestamp = better_tokio_select::tokio_select!(
            biased,
            match .. {
                .. if let res = new_interaction_rx.recv() => {
                    match res {
                        Some(new_interaction) => {
                            let key = format!("{:020}", new_interaction.timestamp.0);
                            if let Err(e) = storage
                                .upsert_record("interactions", &key, new_interaction.clone())
                                .await
                            {
                                tracing::error!("Failed to save new interaction: {}", e);
                            }
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
                .. if let res = resolve_in_flight_tool_rx.recv() => {
                    if let Some(tool_call_id) = res {
                        if let Err(e) = interaction_memory.resolve_in_flight_tool(&tool_call_id) {
                            tracing::warn!("Failed to resolve in-flight tool marker: {}", e);
                        } else {
                            // Find the interaction that got updated
                            if let Some(interaction) = interaction_memory
                                .0
                                .iter()
                                .find(|i| i.resolved_tools.iter().any(|t| t.id == *tool_call_id))
                            {
                                let key = format!("{:020}", interaction.timestamp.0);
                                if let Err(e) = storage
                                    .upsert_record("interactions", &key, interaction.clone())
                                    .await
                                {
                                    tracing::error!(
                                        "Failed to update resolved tool interaction: {}",
                                        e
                                    );
                                }
                            }

                            // send an update to subscribers when memory changes
                            interaction_memory_tx
                                .send(interaction_memory.clone())
                                .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                                .ok();
                        }
                    }
                    false
                }
                .. if let res = wait_for_any_rollout(&mut rollout_receivers) => {
                    if let Err((name, e)) = res {
                        tracing::error!("A rollout receiver has closed for plugin {}: {}", name, e);
                        return;
                    }
                    true
                }
            }
        );

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
                let cutoff_key = format!("{:020}", timestamp.0);
                if let Err(e) = storage
                    .trim_records_before("interactions", &cutoff_key)
                    .await
                {
                    tracing::error!("Failed to trim interaction records: {}", e);
                }
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
                        tx.send(synapto_interface::interaction::ObservedInteraction::from(
                            &*interaction,
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

            if did_dispatch {
                if let Err(e) = storage
                    .set("memory", "last_sent_timestamp", last_sent_timestamp)
                    .await
                {
                    tracing::error!("Failed to save last_sent_timestamp: {:?}", e);
                }
            }
        }
    }
}

// #[derive(Serialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
// struct LLMInteractionMemory(Vec<SummaryLLMInteraction>);

// impl From<InteractionMemory> for LLMInteractionMemory {
//     fn from(value: InteractionMemory) -> Self {
//         Self(value.iter().map(SummaryLLMInteraction::from).collect())
//     }
// }

// struct InteractionMemoryContextProvider {
//     interaction_memory_rx: watch::Receiver<InteractionMemory>,
// }

// impl InteractionMemoryContextProvider {
//     fn new(interaction_memory_rx: watch::Receiver<InteractionMemory>) -> Self {
//         Self {
//             interaction_memory_rx,
//         }
//     }
// }

// #[async_trait::async_trait]
// impl synapto_interface::context::ContextProvider for InteractionMemoryContextProvider {
//     type Context = LLMInteractionMemory;
//     const NAME: &'static str = "interaction_memory";
//     const SCOPE: synapto_interface::context::TemporalScope =
//         synapto_interface::context::TemporalScope::Current;

//     async fn context(
//         &self,
//         _request: &synapto_interface::context::ContextRequest,
//     ) -> Result<Self::Context, String> {
//         let mem = self.interaction_memory_rx.borrow().clone();
//         Ok(LLMInteractionMemory::from(mem))
//     }

//     fn subscribe(&self) -> Option<tokio::sync::watch::Receiver<()>> {
//         let mut rx = self.interaction_memory_rx.clone();
//         let (tx, out_rx) = tokio::sync::watch::channel(());
//         tokio::spawn(async move {
//             while rx.changed().await.is_ok() {
//                 tx.send(())
//                     .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
//                     .ok();
//             }
//         });
//         Some(out_rx)
//     }
// }
