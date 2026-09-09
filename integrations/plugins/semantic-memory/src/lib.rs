use async_trait::async_trait;
use derive_more::{Deref, DerefMut, IntoIterator};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use synapto_interface::llm::LLMSafe;
use tracing::instrument;

use synapto_interface::cognitive::CognitiveReasoning;
use synapto_interface::context::{ContextProvider, ContextRequest, TemporalScope};
use synapto_interface::interaction::{
    CognitiveSpoken, NotClearInteraction, NotClearInteractionMemory, ObservedInteraction, Timestamp,
};
use synapto_interface::peer_input::{PeerInput, Speaker};
use synapto_interface::storage::{RecordStore, StorageConnection};
use synapto_interface::sync::{mpsc, watch};
use synapto_llm::LLM;

pub mod plugin;
pub use plugin::{SemanticMemoryConfig, SemanticMemoryPlugin};

#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
pub struct Insight {
    pub id: u32,
    pub text: String,
    pub is_active: bool,
    pub activity_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ActivityId(pub u32);

#[derive(IntoIterator, Deref, DerefMut, Serialize, Deserialize, Default, Clone)]
pub struct InsightMemory(Vec<Insight>);

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub enum ActivityStatus {
    Ongoing,
    Blocked,
    Completed,
    Discarded,
}

#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
pub struct Activity {
    pub id: u32,
    pub title: String,
    pub status: ActivityStatus,
    pub state_summary: String,
}

pub type ActivityMemory = HashMap<u32, Activity>;

#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
pub struct ActivityShortInfo {
    pub id: u32,
    pub title: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
pub struct LLMVisibleInsight {
    pub id: u32,
    pub text: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, Eq, PartialEq)]
pub struct CognitiveLLMInsight(pub String);

#[derive(Serialize, schemars::JsonSchema, Debug, Clone, Eq, PartialEq, LLMSafe)]
pub struct CognitiveLLMInsightMemory(pub Vec<CognitiveLLMInsight>);

pub struct SemanticContextProvider {
    insight_memory_rx: watch::Receiver<InsightMemory>,
}

impl SemanticContextProvider {
    pub fn new(insight_memory_rx: watch::Receiver<InsightMemory>) -> Self {
        Self { insight_memory_rx }
    }
}

#[async_trait]
impl ContextProvider for SemanticContextProvider {
    type Context = CognitiveLLMInsightMemory;
    const NAME: &'static str = "insight_memory";
    const SCOPE: TemporalScope = TemporalScope::Historical;

    async fn context(&self, _request: &ContextRequest) -> Result<Self::Context, String> {
        let memory = self.insight_memory_rx.borrow().clone();
        Ok(CognitiveLLMInsightMemory(
            memory
                .iter()
                .filter(|i| i.is_active)
                .map(|i| CognitiveLLMInsight(i.text.clone()))
                .collect(),
        ))
    }

    fn subscribe(&self) -> Option<tokio::sync::watch::Receiver<()>> {
        let mut rx = self.insight_memory_rx.clone();
        let (tx, out_rx) = tokio::sync::watch::channel(());
        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                tx.send(()).inspect_err(|e| tracing::error!("{}", e)).ok();
            }
        });
        Some(out_rx)
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub enum LLMUserMessage {
    Speech { speaker: String, transcript: String },
    Text { sender: String, text: String },
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct CognitiveLLMInteraction {
    pub user_messages: Vec<LLMUserMessage>,
    pub cognitive_spoken: Option<CognitiveSpoken>,
    pub cognitive_reasoning: Option<CognitiveReasoning>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct SummaryLLMInteraction {
    pub timestamp: Timestamp,
    pub interaction: CognitiveLLMInteraction,
}

impl From<&ObservedInteraction> for SummaryLLMInteraction {
    fn from(interaction: &ObservedInteraction) -> Self {
        let user_messages = interaction
            .user_messages
            .iter()
            .map(|msg| match msg {
                PeerInput::Speech(s) => LLMUserMessage::Speech {
                    speaker: match &s.speaker {
                        Speaker::Recognized(id) => id.0.to_string(),
                        Speaker::Unknown(None) => "Unknown".to_string(),
                        Speaker::Unknown(Some(id)) => id.0.to_string(),
                    },
                    transcript: s.transcript.to_string(),
                },
                PeerInput::Text(t) => LLMUserMessage::Text {
                    sender: t.sender_id.to_string(),
                    text: t.text.to_string(),
                },
            })
            .collect();

        Self {
            timestamp: interaction.timestamp,
            interaction: CognitiveLLMInteraction {
                user_messages,
                cognitive_spoken: interaction.cognitive_spoken.clone(),
                cognitive_reasoning: interaction.cognitive_reasoning.clone(),
            },
        }
    }
}

#[derive(JsonSchema, Serialize, Clone, Debug, LLMSafe)]
#[cfg_attr(feature = "assistant", serde(rename = "InsightLLMContent"))]
pub struct InsightLLMContent {
    pub current_active_insights: Vec<LLMVisibleInsight>,
    pub active_activities: Vec<ActivityShortInfo>,
    pub new_interactions: Vec<SummaryLLMInteraction>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct ExtractedInsight {
    pub text: String,
    #[schemars(
        description = "ID of the activity this insight belongs to. Null if it requires a NEW activity."
    )]
    pub target_activity_id: Option<u32>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, LLMSafe)]
#[cfg_attr(feature = "assistant", serde(rename = "InsightLLMOutput"))]
pub struct InsightLLMOutput {
    pub new_insights: Vec<ExtractedInsight>,
    pub obsoleted_insight_ids: Vec<u32>,
    #[schemars(description = "None if no interaction was included")]
    pub last_included_interaction: Option<Timestamp>,
}

pub struct InsightLLMPrompt {}

impl LLM for InsightLLMPrompt {
    type Content = InsightLLMContent;
    type Output = InsightLLMOutput;
}

#[derive(JsonSchema, Serialize, Clone, Debug, LLMSafe)]
#[cfg_attr(feature = "assistant", serde(rename = "ActivityLLMContent"))]
pub struct ActivityLLMContent {
    pub current_title: Option<String>,
    pub current_status: Option<ActivityStatus>,
    pub current_state_summary: Option<String>,
    pub active_connected_insights: Vec<LLMVisibleInsight>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, LLMSafe)]
#[cfg_attr(feature = "assistant", serde(rename = "ActivityLLMOutput"))]
pub struct ActivityLLMOutput {
    #[schemars(description = "A concise name for the project/activity")]
    pub new_title: String,
    #[schemars(description = "A synthesized The Big Picture of the project status")]
    pub new_state_summary: String,
    pub status: ActivityStatus,
}

pub struct ActivityLLMPrompt;

impl LLM for ActivityLLMPrompt {
    type Content = ActivityLLMContent;
    type Output = ActivityLLMOutput;
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct ConsolidationLLMVisibleInsight {
    pub id: u32,
    pub text: String,
    pub activity_id: u32,
}

impl From<&Insight> for ConsolidationLLMVisibleInsight {
    fn from(insight: &Insight) -> Self {
        Self {
            id: insight.id,
            text: insight.text.clone(),
            activity_id: insight.activity_id,
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, LLMSafe)]
pub struct ConsolidationLLMContent {
    #[schemars(description = "The newly generated insight that triggered this check.")]
    pub new_insight: ConsolidationLLMVisibleInsight,

    #[schemars(
        description = "Past interactions that were marked as not currently understandable and have not been resolved yet."
    )]
    pub unresolved_context: Vec<SummaryLLMInteraction>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, LLMSafe)]
pub struct ConsolidationLLMOutput {
    #[schemars(
        description = "List of new insights extracted from previously unresolved context, based on the trigger insight."
    )]
    pub new_insights: Vec<ExtractedInsight>,

    #[schemars(
        description = "List of interaction timestamps that have been successfully re-evaluated (whether they yielded new insights or were confirmed as truly irrelevant in this new context). They will be marked as resolved."
    )]
    pub resolved_interaction_timestamps: Vec<i64>,
}

pub struct ConsolidationLLMPrompt {}

impl LLM for ConsolidationLLMPrompt {
    type Content = ConsolidationLLMContent;
    type Output = ConsolidationLLMOutput;
}

#[allow(clippy::too_many_arguments)]
#[instrument(skip_all, fields(subsystem))]
pub async fn insight_memory_task<S: RecordStore + StorageConnection>(
    llm_client: Arc<
        synapto_llm::LLMClient<InsightLLMContent, InsightLLMOutput, synapto_llm::WithoutTools>,
    >,
    store: Arc<S>,
    mut interaction_rx: mpsc::Receiver<ObservedInteraction>,
    insight_memory_tx: watch::Sender<InsightMemory>,
    insight_interaction_rollout_tx: watch::Sender<Timestamp>,
    new_insight_tx: mpsc::Sender<Insight>,
    mut add_insight_rx: mpsc::Receiver<Insight>,
    activity_memory_rx: watch::Receiver<ActivityMemory>,
    activity_dirty_tx: mpsc::Sender<ActivityId>,
) {
    let mut insight_memory: InsightMemory = InsightMemory::default();
    if let Ok(records) = store
        .get_ordered_records::<Insight>(
            "insights",
            None,
            synapto_interface::storage::SortOrder::Ascending,
        )
        .await
    {
        insight_memory = InsightMemory(records.into_iter().map(|(_, v)| v).collect());
    }

    insight_memory_tx.send_replace(insight_memory.clone());

    loop {
        let process_interactions = better_tokio_select::tokio_select!(match .. {
            .. if let res = interaction_rx.recv() => {
                match res {
                    Some(first_interaction) => {
                        let mut batch = vec![first_interaction];
                        while let Ok(next_interaction) = interaction_rx.try_recv() {
                            batch.push(next_interaction);
                        }
                        batch
                    }
                    None => {
                        tracing::error!("interaction_rx closed");
                        return;
                    }
                }
            }
            .. if let res = add_insight_rx.recv() => {
                let new_insight = match res {
                    Some(insight) => insight,
                    None => {
                        tracing::error!("add_insight_rx closed");
                        return;
                    }
                };
                let key = format!("{:010}", new_insight.id);
                if let Err(e) = store
                    .upsert_record("insights", &key, new_insight.clone())
                    .await
                {
                    tracing::error!("Failed to save insight to store: {:?}", e);
                }
                insight_memory.push(new_insight.clone());
                if let Err(e) = insight_memory_tx.send(insight_memory.clone()) {
                    tracing::error!("Failed to send insight_memory: {:?}", e);
                    return;
                }
                activity_dirty_tx
                    .send(ActivityId(new_insight.activity_id))
                    .await
                    .inspect_err(|e| tracing::error!("{}", e))
                    .ok();
                continue;
            }
        });

        let drain_count: i16 = process_interactions.len() as i16 - 8;
        if drain_count >= 0 {
            let mut interactions_to_drain = process_interactions;
            let drained: Vec<_> = interactions_to_drain
                .drain(..drain_count as usize)
                .collect();
            let new_interactions: Vec<_> =
                drained.iter().map(SummaryLLMInteraction::from).collect();

            if new_interactions.is_empty() {
                if let Some(last_drained) = drained.last() {
                    insight_interaction_rollout_tx
                        .send(last_drained.timestamp)
                        .unwrap_or_else(|e| panic!("Error: {:?}", e));
                }
                continue;
            }

            let active_activities = activity_memory_rx.borrow().clone();
            let current_active_insights = insight_memory
                .iter()
                .filter(|i| i.is_active)
                .map(|i| LLMVisibleInsight {
                    id: i.id,
                    text: i.text.clone(),
                })
                .collect();

            let active_activities_info = active_activities
                .values()
                .map(|a| ActivityShortInfo {
                    id: a.id,
                    title: a.title.clone(),
                })
                .collect();

            let llm_output = match llm_client
                .call(
                    InsightLLMContent {
                        new_interactions,
                        current_active_insights,
                        active_activities: active_activities_info,
                    },
                    None,
                    None,
                )
                .await
            {
                Ok(output) => output,
                Err(e) => {
                    tracing::error!("Failed to extract semantic insights: {:?}", e);
                    continue;
                }
            };

            let mut next_insight_id = insight_memory.iter().map(|i| i.id).max().unwrap_or(0) + 1;
            let mut next_activity_id = active_activities.keys().max().copied().unwrap_or(0) + 1;

            let mut dirtied_activities = std::collections::HashSet::new();

            for obs_id in llm_output.obsoleted_insight_ids {
                if let Some(insight) = insight_memory.iter_mut().find(|i| i.id == obs_id) {
                    insight.is_active = false;
                    dirtied_activities.insert(insight.activity_id);
                }
            }

            for extracted in llm_output.new_insights {
                let target_activity_id = match extracted.target_activity_id {
                    Some(id) => id,
                    None => {
                        let new_id = next_activity_id;
                        next_activity_id += 1;
                        new_id
                    }
                };

                let new_insight = Insight {
                    id: next_insight_id,
                    text: extracted.text,
                    is_active: true,
                    activity_id: target_activity_id,
                };
                next_insight_id += 1;

                insight_memory.push(new_insight.clone());
                new_insight_tx
                    .send(new_insight)
                    .await
                    .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                    .ok();
                dirtied_activities.insert(target_activity_id);
            }

            for act_id in dirtied_activities {
                activity_dirty_tx
                    .send(ActivityId(act_id))
                    .await
                    .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                    .ok();
            }

            if let Some(last_included_interaction) = llm_output.last_included_interaction {
                insight_interaction_rollout_tx
                    .send(last_included_interaction)
                    .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                    .ok();
                insight_memory_tx
                    .send(insight_memory.clone())
                    .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                    .ok();
            }

            for insight in &insight_memory.0 {
                let key = format!("{:010}", insight.id);
                if let Err(e) = store.upsert_record("insights", &key, insight.clone()).await {
                    tracing::error!("Failed to save insight to store: {:?}", e);
                }
            }
        }
    }
}

#[instrument(skip_all, fields(subsystem))]
#[instrument(skip_all, fields(subsystem))]
pub async fn activity_memory_task<S: RecordStore + StorageConnection>(
    llm_client: Arc<
        synapto_llm::LLMClient<ActivityLLMContent, ActivityLLMOutput, synapto_llm::WithoutTools>,
    >,
    store: Arc<S>,
    mut activity_dirty_rx: mpsc::Receiver<ActivityId>,
    insight_memory_rx: watch::Receiver<InsightMemory>,
    activity_memory_tx: watch::Sender<ActivityMemory>,
) {
    let mut activities: ActivityMemory = HashMap::new();
    if let Ok(records) = store
        .get_ordered_records::<Activity>(
            "activities",
            None,
            synapto_interface::storage::SortOrder::Ascending,
        )
        .await
    {
        for (_, record) in records {
            activities.insert(record.id, record);
        }
    }

    activity_memory_tx
        .send(activities.clone())
        .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
        .ok();

    while let Some(ActivityId(dirty_activity_id)) = activity_dirty_rx.recv().await {
        let mut activity = activities
            .entry(dirty_activity_id)
            .or_insert_with(|| Activity {
                id: dirty_activity_id,
                title: "New activity".to_string(),
                status: ActivityStatus::Ongoing,
                state_summary: "".to_string(),
            })
            .clone();

        let insight_memory = insight_memory_rx.borrow().clone();
        let active_connected_insights: Vec<LLMVisibleInsight> = insight_memory
            .iter()
            .filter(|i| i.is_active && i.activity_id == dirty_activity_id)
            .map(|i| LLMVisibleInsight {
                id: i.id,
                text: i.text.clone(),
            })
            .collect();

        if active_connected_insights.is_empty() {
            activity.status = ActivityStatus::Discarded;
        } else {
            let llm_output = match llm_client
                .call(
                    ActivityLLMContent {
                        current_title: Some(activity.title.clone()),
                        current_status: Some(activity.status.clone()),
                        current_state_summary: Some(activity.state_summary.clone()),
                        active_connected_insights,
                    },
                    None,
                    None,
                )
                .await
            {
                Ok(output) => output,
                Err(e) => {
                    tracing::error!("Failed to generate activity summary: {:?}", e);
                    continue;
                }
            };

            activity.title = llm_output.new_title;
            activity.state_summary = llm_output.new_state_summary;
            activity.status = llm_output.status;
        }

        activities.insert(dirty_activity_id, activity.clone());

        let key = format!("{:010}", activity.id);
        if let Err(e) = store.upsert_record("activities", &key, activity).await {
            tracing::error!("Failed to save activity memory to store: {:?}", e);
        }

        activity_memory_tx
            .send(activities.clone())
            .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
            .ok();
    }
    tracing::error!("Activity dirty channel closed");
}
pub async fn semantic_consolidation_task<S: RecordStore + StorageConnection>(
    llm_client: Arc<
        synapto_llm::LLMClient<
            ConsolidationLLMContent,
            ConsolidationLLMOutput,
            synapto_llm::WithoutTools,
        >,
    >,
    _store: Arc<S>,
    mut new_insight_rx: mpsc::Receiver<Insight>,
    not_clear_memory_rx: watch::Receiver<NotClearInteractionMemory>,
    new_insight_tx: mpsc::Sender<Insight>,
    resolve_not_clear_tx: mpsc::Sender<Timestamp>,
) {
    while let Some(new_insight) = new_insight_rx.recv().await {
        let not_clear_memory = not_clear_memory_rx.borrow().clone();

        let unresolved_context: Vec<NotClearInteraction> =
            not_clear_memory.0.iter().cloned().collect();

        if unresolved_context.is_empty() {
            continue;
        }

        let summary_unresolved_context: Vec<SummaryLLMInteraction> = unresolved_context
            .iter()
            .map(|interaction| {
                let user_messages = interaction
                    .user_messages
                    .iter()
                    .map(|msg| match msg {
                        PeerInput::Speech(s) => LLMUserMessage::Speech {
                            speaker: match &s.speaker {
                                Speaker::Recognized(id) => id.0.to_string(),
                                Speaker::Unknown(None) => "Unknown".to_string(),
                                Speaker::Unknown(Some(id)) => id.0.to_string(),
                            },
                            transcript: s.transcript.to_string(),
                        },
                        PeerInput::Text(t) => LLMUserMessage::Text {
                            sender: t.sender_id.to_string(),
                            text: t.text.to_string(),
                        },
                    })
                    .collect();

                SummaryLLMInteraction {
                    timestamp: interaction.timestamp,
                    interaction: CognitiveLLMInteraction {
                        user_messages,
                        cognitive_spoken: interaction.cognitive_spoken.clone(),
                        cognitive_reasoning: None,
                    },
                }
            })
            .collect();

        let content = ConsolidationLLMContent {
            new_insight: ConsolidationLLMVisibleInsight::from(&new_insight),
            unresolved_context: summary_unresolved_context,
        };

        match llm_client.call(content, None, None).await {
            Ok(output) => {
                for ext in output.new_insights {
                    let generated_insight = Insight {
                        id: chrono::Utc::now().timestamp_subsec_nanos(),
                        text: ext.text,
                        is_active: true,
                        activity_id: ext.target_activity_id.unwrap_or(new_insight.activity_id),
                    };
                    new_insight_tx
                        .send(generated_insight)
                        .await
                        .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                        .ok();
                }

                for ts in output.resolved_interaction_timestamps {
                    if unresolved_context.iter().any(|i| i.timestamp.0 == ts) {
                        resolve_not_clear_tx
                            .send(Timestamp(ts))
                            .await
                            .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                            .ok();
                    }
                }

                tracing::info!(
                    "✅ Consolidation task finished checking {} unresolved context entries.",
                    unresolved_context.len()
                );
            }
            Err(e) => {
                tracing::error!("Semantic consolidation error: {:?}", e);
            }
        }
    }
}
