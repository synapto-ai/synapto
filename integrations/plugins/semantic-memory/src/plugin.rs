use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use synapto_interface::interaction::InteractionObserver;
use synapto_interface::interaction::RetrospectiveConsolidationPlugin;
use synapto_interface::interaction::{NotClearInteractionMemory, ObservedInteraction, Timestamp};
use synapto_interface::plugin::{Plugin, PluginRegistry};
use synapto_interface::rollout::RolloutController;
use synapto_interface::storage::{RecordStore, StorageConnection};
use synapto_interface::sync::{mpsc, watch};
use synapto_llm::Instruction;
use synapto_llm::{LLM, LLMClient, WithoutTools};

use crate::{
    ActivityId, ActivityLLMContent, ActivityLLMOutput, ActivityLLMPrompt, ActivityMemory,
    ConsolidationLLMContent, ConsolidationLLMOutput, ConsolidationLLMPrompt, Insight,
    InsightLLMContent, InsightLLMOutput, InsightLLMPrompt, InsightMemory, SemanticContextProvider,
};

use synapto_interface::llm::ModelConfig;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SemanticMemoryConfig {
    pub insight: ModelConfig,
}

pub struct SemanticMemoryPlugin<S: RecordStore + StorageConnection> {
    _config: SemanticMemoryConfig,
    store: Arc<S>,
    provider: Arc<SemanticContextProvider>,
    insight_llm_client: Arc<LLMClient<InsightLLMContent, InsightLLMOutput, WithoutTools>>,
    activity_llm_client: Arc<LLMClient<ActivityLLMContent, ActivityLLMOutput, WithoutTools>>,
    consolidation_llm_client:
        Arc<LLMClient<ConsolidationLLMContent, ConsolidationLLMOutput, WithoutTools>>,

    insight_memory_tx: Mutex<Option<watch::Sender<InsightMemory>>>,
    activity_memory_tx: Mutex<Option<watch::Sender<ActivityMemory>>>,
    activity_memory_rx: Mutex<Option<watch::Receiver<ActivityMemory>>>,
    rollout_tx_rx: Mutex<Option<tokio::sync::oneshot::Receiver<watch::Sender<Timestamp>>>>,
    rollout_tx_tx: Mutex<Option<tokio::sync::oneshot::Sender<watch::Sender<Timestamp>>>>,

    // Direct boot-time injected channels for Retrospective Consolidation
    not_clear_memory_rx: Mutex<Option<watch::Receiver<NotClearInteractionMemory>>>,
    resolve_not_clear_tx: Mutex<Option<mpsc::Sender<Timestamp>>>,
}

impl<S: RecordStore + StorageConnection> SemanticMemoryPlugin<S> {
    pub fn new_with_channels(
        config: SemanticMemoryConfig,
        store: Arc<S>,
        llm_executor: synapto_interface::llm::LlmExecutor,
        not_clear_memory_rx: Option<watch::Receiver<NotClearInteractionMemory>>,
        resolve_not_clear_tx: Option<mpsc::Sender<Timestamp>>,
    ) -> Result<Self, String> {
        let (insight_memory_tx, insight_memory_rx) = watch::channel(InsightMemory::default());
        let (activity_memory_tx, activity_memory_rx) = watch::channel(ActivityMemory::default());

        let provider = Arc::new(SemanticContextProvider::new(insight_memory_rx));

        // Initialize LLM prompts with descriptive roles
        let insight_system_prompt = vec![
            Instruction::ImportantSection(
                Box::new(Instruction::Text("Cognitive Analyst and Semantic Router".to_string())),
                vec![
                    Instruction::Text("You are a Cognitive Analyst and Semantic Router.".to_string()),
                    Instruction::Text("Your task is to analyze a block of interactions, extract Permanent insights (Insights), and assign them to existing Projects (Activities) or suggest a new one.".to_string()),
                    Instruction::Section(
                        Box::new(Instruction::Text("Focus Purely on Factual Content".to_string())),
                        vec![
                            Instruction::NumberedItem("Extract hard facts, architectural or business decisions, and findings.".to_string()),
                            Instruction::NumberedItem("For each extracted Insight, determine a `target_activity_id` from the `active_activities` list.".to_string()),
                        ]
                    ),
                    Instruction::Section(
                        Box::new(Instruction::Text("Memory Maintenance (Obsoletion)".to_string())),
                        vec![
                            Instruction::Text("If the interactions clearly contradict or change any of the existing `current_active_insights`, add its ID to the `obsoleted_insight_ids` field.".to_string()),
                        ]
                    ),
                    Instruction::Section(
                        Box::new(Instruction::Text("Forbidden Zone".to_string())),
                        vec![
                            Instruction::ImportantItem("Ignore HOW entities speak to each other (emotion, tone).".to_string()),
                            Instruction::ImportantItem("Do not write meeting minutes. Formulate as timeless facts.".to_string()),
                        ]
                    ),
                    Instruction::ImportantSection(
                        Box::new(Instruction::Text("Logical Cut-off Rule".to_string())),
                        vec![
                            Instruction::Text("Find the latest interaction that forms a logical end to a thought. Process the conversation only up to this point and return its timestamp.".to_string()),
                        ]
                    )
                ]
            )
        ];

        let activity_system_prompt = vec![
            Instruction::ImportantSection(
                Box::new(Instruction::Text("Tech Lead and Project Manager".to_string())),
                vec![
                    Instruction::Text("You are a Tech Lead and Project Manager. Your Activity (Project) includes the attached set of currently valid atomic Insights.".to_string()),
                    Instruction::Section(
                        Box::new(Instruction::Text("Your Task".to_string())),
                        vec![
                            Instruction::NumberedItem("Read the current state of the Activity and all connected Insights.".to_string()),
                            Instruction::NumberedItem("CREATE A SYNTHESIS: Create \"The Big Picture\" - an emergent bird's-eye view. Reformulate the current state of the project so that it logically connects all facts.".to_string()),
                            Instruction::NumberedItem("NAMING AND STATUS: If the activity is new, come up with a concise `new_title`. Determine a logical `status`.".to_string()),
                        ]
                    ),
                    Instruction::Section(
                        Box::new(Instruction::Text("Focus On".to_string())),
                        vec![
                            Instruction::Item("Causality across insights. Find out how the pieces influence each other.".to_string()),
                        ]
                    ),
                    Instruction::Section(
                        Box::new(Instruction::Text("Forbidden Zone".to_string())),
                        vec![
                            Instruction::ImportantItem("DO NOT write a list (bullet-points) of individual Insights. Write a coherent project status.".to_string()),
                            Instruction::ImportantItem("DO NOT describe history. Describe only the CURRENT valid state and outlook.".to_string()),
                        ]
                    )
                ]
            )
        ];

        let consolidation_system_prompt = vec![
            Instruction::ImportantSection(
                Box::new(Instruction::Text("Cognitive Consolidator".to_string())),
                vec![
                    Instruction::Text("Your role is to retrospectively analyze old interactions that were originally discarded as locally \"unintelligible\" because context was missing at the time and they didn't make sense.".to_string()),
                    Instruction::Text("Right now, the system has gained a new, confirmed insight (Insight).".to_string()),
                    Instruction::Section(
                        Box::new(Instruction::Text("Your Task".to_string())),
                        vec![
                            Instruction::NumberedItem("Read the new Insight.".to_string()),
                            Instruction::NumberedItem("Go through the provided list of old interactions with missing context.".to_string()),
                            Instruction::NumberedItem("Ask yourself: \"Does any of these old interactions make sense now in the light of this new insight?\"".to_string()),
                            Instruction::NumberedItem("If yes, extract new permanent insights (Insights) and assign them to the appropriate activity.".to_string()),
                            Instruction::NumberedItem("Return the timestamps of those interactions that you successfully analyzed and resolved.".to_string()),
                        ]
                    )
                ]
            )
        ];

        let insight_llm_client = Arc::new(InsightLLMPrompt::create_client(
            llm_executor.clone(),
            config.insight.clone(),
            insight_system_prompt,
        ));

        let activity_llm_client = Arc::new(ActivityLLMPrompt::create_client(
            llm_executor.clone(),
            config.insight.clone(),
            activity_system_prompt,
        ));

        let consolidation_llm_client = Arc::new(ConsolidationLLMPrompt::create_client(
            llm_executor,
            config.insight.clone(),
            consolidation_system_prompt,
        ));

        let (rollout_tx_tx, rollout_tx_rx) = tokio::sync::oneshot::channel();

        Ok(Self {
            _config: config,
            store,
            provider,
            insight_llm_client,
            activity_llm_client,
            consolidation_llm_client,
            insight_memory_tx: Mutex::new(Some(insight_memory_tx)),
            activity_memory_tx: Mutex::new(Some(activity_memory_tx)),
            activity_memory_rx: Mutex::new(Some(activity_memory_rx)),
            rollout_tx_rx: Mutex::new(Some(rollout_tx_rx)),
            rollout_tx_tx: Mutex::new(Some(rollout_tx_tx)),
            not_clear_memory_rx: Mutex::new(not_clear_memory_rx),
            resolve_not_clear_tx: Mutex::new(resolve_not_clear_tx),
        })
    }
}

#[async_trait::async_trait]
impl<S: RecordStore + StorageConnection> Plugin for SemanticMemoryPlugin<S> {
    async fn create(
        context: &synapto_interface::plugin::PluginInitContext<'_>,
    ) -> Result<Self, String> {
        let config: SemanticMemoryConfig = context.config()?;
        let store = context.store::<S>().await?;

        Self::new_with_channels(config, store, context.llm_executor(), None, None)
    }

    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R)
    where
        Self: Sized,
    {
        registry.register_interaction_observer(self.clone());
        registry.register_rollout_controller(self.clone());
        registry.register_context_provider(self.provider.clone());
        registry.register_retrospective_consolidation(self.clone());
    }
}

#[async_trait]
impl<S: RecordStore + StorageConnection> RetrospectiveConsolidationPlugin
    for SemanticMemoryPlugin<S>
{
    async fn start(
        &self,
        not_clear_memory_rx: watch::Receiver<NotClearInteractionMemory>,
        resolve_not_clear_tx: mpsc::Sender<Timestamp>,
    ) -> Result<(), String> {
        *self
            .not_clear_memory_rx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e)) = Some(not_clear_memory_rx);
        *self
            .resolve_not_clear_tx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e)) = Some(resolve_not_clear_tx);
        Ok(())
    }
}

#[async_trait]
impl<S: RecordStore + StorageConnection> RolloutController for SemanticMemoryPlugin<S> {
    async fn start(&self, rollout_tx: watch::Sender<Timestamp>) -> Result<(), String> {
        if let Some(tx) = self
            .rollout_tx_tx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e))
            .take()
        {
            tx.send(rollout_tx)
                .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                .ok();
        }
        Ok(())
    }
}

#[async_trait]
impl<S: RecordStore + StorageConnection> InteractionObserver for SemanticMemoryPlugin<S> {
    async fn start(
        &self,
        interaction_rx: mpsc::Receiver<ObservedInteraction>,
    ) -> Result<(), String> {
        let insight_memory_tx = self
            .insight_memory_tx
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

        let activity_memory_tx = self
            .activity_memory_tx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e))
            .take()
            .expect("Missing value");
        let activity_memory_rx = self
            .activity_memory_rx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e))
            .take()
            .expect("Missing value");

        let (new_insight_tx, new_insight_rx) = mpsc::channel::<Insight>(10);
        let (add_insight_tx, add_insight_rx) = mpsc::channel::<Insight>(10);
        let (activity_dirty_tx, activity_dirty_rx) = mpsc::channel::<ActivityId>(100);

        let store = self.store.clone();
        let insight_llm_client = self.insight_llm_client.clone();
        let activity_llm_client = self.activity_llm_client.clone();
        let consolidation_llm_client = self.consolidation_llm_client.clone();

        let insight_memory_rx = self.provider.insight_memory_rx.clone();

        let store_1 = store.clone();

        // Spawn insight memory task
        tokio::spawn(async move {
            let rollout_tx = match rx.await {
                Ok(tx) => tx,
                Err(e) => {
                    tracing::error!("Failed to receive rollout_tx: {}", e);
                    return;
                }
            };

            crate::insight_memory_task(
                insight_llm_client,
                store_1,
                interaction_rx,
                insight_memory_tx,
                rollout_tx,
                new_insight_tx,
                add_insight_rx,
                activity_memory_rx,
                activity_dirty_tx.clone(),
            )
            .await;
        });

        // Spawn activity memory task
        tokio::spawn(crate::activity_memory_task(
            activity_llm_client,
            store,
            activity_dirty_rx,
            insight_memory_rx,
            activity_memory_tx,
        ));

        // Spawn semantic consolidation task if channels are injected
        let not_clear_memory_rx = self
            .not_clear_memory_rx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e))
            .take();
        let resolve_not_clear_tx = self
            .resolve_not_clear_tx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e))
            .take();

        let store_2 = self.store.clone();

        if let (Some(not_clear_rx), Some(resolve_tx)) = (not_clear_memory_rx, resolve_not_clear_tx)
        {
            tokio::spawn(crate::semantic_consolidation_task(
                consolidation_llm_client,
                store_2,
                new_insight_rx,
                not_clear_rx,
                add_insight_tx,
                resolve_tx,
            ));
        }

        Ok(())
    }
}
