use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use synapto_interface::storage::{RecordStore, StorageConnection};

use crate::BehavioralContextProvider;
use crate::BehavioralInsightMemory;
use synapto_interface::interaction::{InteractionObserver, ObservedInteraction, Timestamp};
use synapto_interface::plugin::{Plugin, PluginRegistry};
use synapto_interface::rollout::RolloutController;
use synapto_interface::sync::{mpsc, watch};
use synapto_llm::Instruction;
use synapto_llm::{LLM, LLMClient, WithoutTools};

use synapto_interface::llm::ModelConfig;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BehavioralMemoryConfig {
    pub insight: ModelConfig,
}

pub struct BehavioralMemoryPlugin<S: RecordStore + StorageConnection> {
    provider: Arc<BehavioralContextProvider>,
    llm_client: Arc<
        LLMClient<
            crate::BehavioralInsightLLMContent,
            crate::BehavioralInsightLLMOutput,
            WithoutTools,
        >,
    >,
    behavioral_insight_memory_tx: std::sync::Mutex<Option<watch::Sender<BehavioralInsightMemory>>>,
    rollout_tx_rx:
        std::sync::Mutex<Option<tokio::sync::oneshot::Receiver<watch::Sender<Timestamp>>>>,
    rollout_tx_tx: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<watch::Sender<Timestamp>>>>,
    store: Arc<S>,
    _marker: std::marker::PhantomData<S>,
}

#[async_trait::async_trait]
impl<S: RecordStore + StorageConnection + Send + Sync> Plugin for BehavioralMemoryPlugin<S> {
    async fn create(
        context: &synapto_interface::plugin::PluginInitContext<'_>,
    ) -> Result<Self, String> {
        let config: BehavioralMemoryConfig = context.config()?;
        let store = context.store::<S>().await?;

        let (behavioral_insight_memory_tx, behavioral_insight_memory_rx) =
            watch::channel(BehavioralInsightMemory::default());

        let provider = Arc::new(BehavioralContextProvider::new(behavioral_insight_memory_rx));

        let system_prompt = vec![
                Instruction::ImportantSection(
                    Box::new(Instruction::Text("Psychologist and Behavioral Analyst".to_string())),
                    vec![
                        Instruction::Text("You are a Psychologist and Behavioral Analyst.".to_string()),
                        Instruction::Text("You are reading a record of a just-finished conversation (Session). Your task is NOT to summarize what was discussed. Your task is to analyze communication style, preferences, and group dynamics.".to_string()),
                        Instruction::Text("If a behavioral insight is already present in `current_insights` and hasn't changed, do not create a new one.".to_string()),
                        Instruction::Section(
                            Box::new(Instruction::Text("Focus On".to_string())),
                            vec![
                                Instruction::Item("Formatting: Did the user ask for a specific format? (e.g., \"Write it directly as code\")".to_string()),
                                Instruction::Item("Tone and emotion: Is the conversation informal, full of jokes, or professional?".to_string()),
                                Instruction::Item("Prohibitions: Did the user correct you? (e.g., \"Don't keep saying good day to me\")".to_string()),
                            ]
                        ),
                        Instruction::Section(
                            Box::new(Instruction::Text("Output".to_string())),
                            vec![
                                Instruction::Text("If you found new, permanent rules for communication, generate new insights in `new_insights`.".to_string()),
                                Instruction::ImportantText("These insights must be in the form of an instruction for future AI behavior (e.g., \"When communicating with this group, use only bullet points and a technical tone. Do not use greetings.\").".to_string()),
                            ]
                        ),
                        Instruction::ImportantSection(
                            Box::new(Instruction::Text("Logical Cut-off Rule".to_string())),
                            vec![
                                Instruction::NumberedItem("Find the latest interaction that forms a logical end to an action or a coherent block.".to_string()),
                                Instruction::NumberedItem("Process and summarize only interactions up to this point.".to_string()),
                                Instruction::NumberedItem("Ignore the remaining (unfinished) interactions at the end of the batch.".to_string()),
                            ]
                        )
                    ]
                )
            ];

        let llm_client = Arc::new(crate::BehavioralInsightLLMPrompt::create_client(
            context.llm_executor(),
            config.insight.clone(),
            system_prompt,
        ));

        let (rollout_tx_tx, rollout_tx_rx) = tokio::sync::oneshot::channel();

        Ok(Self {
            provider,
            llm_client,
            behavioral_insight_memory_tx: std::sync::Mutex::new(Some(behavioral_insight_memory_tx)),
            rollout_tx_rx: std::sync::Mutex::new(Some(rollout_tx_rx)),
            rollout_tx_tx: std::sync::Mutex::new(Some(rollout_tx_tx)),
            store,
            _marker: std::marker::PhantomData,
        })
    }

    fn register<R: PluginRegistry + ?Sized>(self: std::sync::Arc<Self>, registry: &mut R)
    where
        Self: Sized,
    {
        registry.register_interaction_observer(self.clone());
        registry.register_rollout_controller(self.clone());
        registry.register_context_provider(self.provider.clone());
    }
}

#[async_trait]
impl<S: RecordStore + StorageConnection> RolloutController for BehavioralMemoryPlugin<S> {
    async fn start(&self, rollout_tx: watch::Sender<Timestamp>) -> Result<(), String> {
        if let Some(tx) = self
            .rollout_tx_tx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e))
            .take()
        {
            tx.send(rollout_tx)
                .inspect_err(|e| tracing::error!("{:?}", e))
                .ok();
        }
        Ok(())
    }
}

#[async_trait]
impl<S: RecordStore + StorageConnection> InteractionObserver for BehavioralMemoryPlugin<S> {
    async fn start(
        &self,
        interaction_rx: mpsc::Receiver<ObservedInteraction>,
    ) -> Result<(), String> {
        let memory_tx = self
            .behavioral_insight_memory_tx
            .lock()
            .unwrap_or_else(|e| panic!("Error: {:?}", e))
            .take()
            .ok_or("Plugin already started")?;

        let rx = self
            .rollout_tx_rx
            .lock()
            .unwrap_or_else(|e| panic!("Error: {:?}", e))
            .take()
            .ok_or("Plugin already started")?;

        let store = self.store.clone();
        let llm_client = self.llm_client.clone();

        tokio::spawn(async move {
            let rollout_tx = match rx.await {
                Ok(tx) => tx,
                Err(e) => {
                    tracing::error!("Failed to receive rollout_tx: {}", e);
                    return;
                }
            };

            crate::behavioral_insight_memory_task(
                llm_client,
                store,
                interaction_rx,
                memory_tx,
                rollout_tx,
            )
            .await;
        });

        Ok(())
    }
}
