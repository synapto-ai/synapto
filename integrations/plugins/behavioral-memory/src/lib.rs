pub mod plugin;
pub use plugin::{BehavioralMemoryConfig, BehavioralMemoryPlugin};

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use synapto_interface::llm::LLMSafe;
use tracing::instrument;

use synapto_interface::cognitive::CognitiveReasoning;
use synapto_interface::context::{ContextProvider, ContextRequest, TemporalScope};
use synapto_interface::decision::NoulQuestion;
use synapto_interface::interaction::{CognitiveSpoken, ObservedInteraction, Timestamp};
use synapto_interface::peer_input::{PeerInput, Speaker};
use synapto_interface::sync::{mpsc, watch};
use synapto_llm::LLM;

pub fn generate_behavioral_preflight_question() -> NoulQuestion {
    NoulQuestion {
        instructions: "Does this interaction reveal any user habit, behavioral pattern, personal preference, or constraint?".to_string(),
        criteria: Some(synapto_interface::decision::NoulCriteria {
            r#true: "The user expressed a personal habit, routine, explicit preference, or operational rule.".to_string(),
            r#false: "Routine conversational exchange, question, task command, or general chatter.".to_string(),
        }),
    }
}

/// A small insight about behavior that the background LLM extracts from the conversation (interactions)
#[derive(Serialize, Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct BehavioralInsight {
    pub content: String,
}

impl BehavioralInsight {
    pub fn new(content: String) -> Self {
        Self { content }
    }
}

#[derive(
    derive_more::IntoIterator,
    derive_more::Deref,
    derive_more::DerefMut,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    Default,
    Clone,
    Debug,
    PartialEq,
    Eq,
)]
pub struct BehavioralInsightMemory(pub Vec<BehavioralInsight>);

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct LLMBehavioralInsight(pub String);

#[derive(Serialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct LLMBehavioralInsightMemory(pub Vec<LLMBehavioralInsight>);

impl From<BehavioralInsightMemory> for LLMBehavioralInsightMemory {
    fn from(value: BehavioralInsightMemory) -> Self {
        Self(
            value
                .iter()
                .map(|i| LLMBehavioralInsight(i.content.clone()))
                .collect(),
        )
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

#[derive(JsonSchema, Serialize, Clone, Debug, PartialEq, Eq, LLMSafe)]
#[cfg_attr(feature = "assistant", serde(rename = "BehavioralInsightLLMContent"))]
pub struct BehavioralInsightLLMContent {
    pub current_insights: LLMBehavioralInsightMemory,
    pub new_interactions: Vec<SummaryLLMInteraction>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
#[cfg_attr(feature = "assistant", serde(rename = "BehavioralInsightLLMOutput"))]
pub struct BehavioralInsightLLMOutput {
    pub new_insights: Vec<LLMBehavioralInsight>,
    pub last_included_interaction: Option<Timestamp>,
}

pub struct BehavioralInsightLLMPrompt {}

impl LLM for BehavioralInsightLLMPrompt {
    type Content = BehavioralInsightLLMContent;
    type Output = BehavioralInsightLLMOutput;
}

pub struct BehavioralContextProvider {
    behavioral_insight_memory_rx: watch::Receiver<BehavioralInsightMemory>,
}

impl BehavioralContextProvider {
    pub fn new(behavioral_insight_memory_rx: watch::Receiver<BehavioralInsightMemory>) -> Self {
        Self {
            behavioral_insight_memory_rx,
        }
    }
}

#[async_trait]
impl ContextProvider for BehavioralContextProvider {
    type Context = LLMBehavioralInsightMemory;
    const NAME: &'static str = "behavioral_insights";
    const SCOPE: TemporalScope = TemporalScope::Current;

    async fn context(&self, _request: &ContextRequest) -> Result<Self::Context, String> {
        let memory = self.behavioral_insight_memory_rx.borrow().clone();
        Ok(LLMBehavioralInsightMemory::from(memory))
    }

    fn subscribe(&self) -> Option<tokio::sync::watch::Receiver<()>> {
        let mut rx = self.behavioral_insight_memory_rx.clone();
        let (tx, out_rx) = tokio::sync::watch::channel(());
        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                tx.send(()).inspect_err(|e| tracing::error!("{}", e)).ok();
            }
        });
        Some(out_rx)
    }
}

#[instrument(skip_all, fields(subsystem))]
pub async fn behavioral_insight_memory_task<S: synapto_interface::storage::RecordStore>(
    llm_client: Arc<
        synapto_llm::LLMClient<
            BehavioralInsightLLMContent,
            BehavioralInsightLLMOutput,
            synapto_llm::WithoutTools,
        >,
    >,
    decision_handle: synapto_interface::decision::DecisionHandle,
    decision_preflight_threshold: f64,
    store: Arc<S>,
    mut interaction_rx: mpsc::Receiver<ObservedInteraction>,
    behavioral_insight_memory_tx: watch::Sender<BehavioralInsightMemory>,
    rollout_tx: watch::Sender<Timestamp>,
) {
    let mut behavioral_insight_memory: BehavioralInsightMemory = match store
        .get_ordered_records(
            "insights",
            None,
            synapto_interface::storage::SortOrder::Ascending,
        )
        .await
    {
        Ok(insights) => {
            BehavioralInsightMemory(insights.into_iter().map(|(_, item)| item).collect())
        }
        Err(e) => {
            tracing::error!("Failed to load behavioral memory from store: {}", e);
            BehavioralInsightMemory::default()
        }
    };

    behavioral_insight_memory_tx.send_replace(behavioral_insight_memory.clone());

    while let Some(first_interaction) = interaction_rx.recv().await {
        // Drain any other immediately available interactions to process in batch!
        let mut batch = vec![first_interaction];
        while let Ok(next_interaction) = interaction_rx.try_recv() {
            batch.push(next_interaction);
        }

        let new_interactions: Vec<SummaryLLMInteraction> =
            batch.iter().map(SummaryLLMInteraction::from).collect();

        let last_interaction_in_batch = batch.last().expect("Missing last value");

        if decision_handle.is_available() {
            let question = generate_behavioral_preflight_question();
            let mut questions = std::collections::BTreeMap::new();
            questions.insert(
                "preflight".to_string(),
                synapto_interface::decision::DecisionQuestion::Noul(question),
            );
            let state = serde_json::json!({
                "interactions": new_interactions,
            });

            let prob = match decision_handle.evaluate(None, state, questions).await {
                Ok(mut answers) => match answers.remove("preflight") {
                    Some(synapto_interface::decision::DecisionAnswer::Noul { noul }) => noul,
                    _ => 1.0,
                },
                Err(e) => {
                    tracing::warn!(
                        "Decision preflight evaluation failed, falling back to generative LLM: {}",
                        e
                    );
                    1.0
                }
            };

            if prob < decision_preflight_threshold {
                tracing::debug!(
                    "Behavioral preflight prob ({}) < threshold ({}), skipping generative LLM",
                    prob,
                    decision_preflight_threshold
                );
                rollout_tx
                    .send(last_interaction_in_batch.timestamp)
                    .unwrap_or_else(|e| tracing::error!("{}", e));
                continue;
            }
        }

        let BehavioralInsightLLMOutput {
            new_insights: new_insights_creation,
            last_included_interaction,
        } = match llm_client
            .call(
                BehavioralInsightLLMContent {
                    new_interactions,
                    current_insights: LLMBehavioralInsightMemory::from(
                        behavioral_insight_memory.clone(),
                    ),
                },
                None,
                None,
            )
            .await
        {
            Ok(output) => output,
            Err(e) => {
                tracing::error!("{}", e);
                continue;
            }
        };

        for creation in new_insights_creation {
            let new_insight = BehavioralInsight::new(creation.0);

            // Push to the database store
            if let Err(e) = store
                .upsert_record("insights", &new_insight.content, new_insight.clone())
                .await
            {
                tracing::error!("Failed to save new behavioral insight to store: {}", e);
            }

            behavioral_insight_memory.0.push(new_insight);
        }

        if let Some(last_included_interaction) = last_included_interaction {
            rollout_tx
                .send(last_included_interaction)
                .unwrap_or_else(|e| tracing::error!("{}", e));
            behavioral_insight_memory_tx
                .send(behavioral_insight_memory.clone())
                .unwrap_or_else(|e| tracing::error!("{}", e));
        } else {
            rollout_tx
                .send(last_interaction_in_batch.timestamp)
                .unwrap_or_else(|e| tracing::error!("{}", e));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use synapto_interface::decision::{DecisionAnswer, DecisionQuestion, RawDecisionExecutor};

    struct MockDecisionBackend {
        prob: f64,
    }

    #[async_trait::async_trait]
    impl RawDecisionExecutor for MockDecisionBackend {
        async fn evaluate_raw(
            &self,
            _model: Option<&str>,
            _state: serde_json::Value,
            _questions: BTreeMap<String, DecisionQuestion>,
        ) -> Result<BTreeMap<String, DecisionAnswer>, String> {
            let mut map = BTreeMap::new();
            map.insert(
                "preflight".to_string(),
                DecisionAnswer::Noul { noul: self.prob },
            );
            Ok(map)
        }
    }

    #[test]
    fn test_generate_behavioral_preflight_question() {
        let question = generate_behavioral_preflight_question();
        assert!(question.instructions.contains("habit"));
        let criteria = question.criteria.expect("Missing criteria");
        assert!(criteria.r#true.contains("habit"));
        assert!(criteria.r#false.contains("Routine"));
    }

    #[tokio::test]
    async fn test_behavioral_decision_handle_evaluation() {
        let handle = synapto_interface::decision::DecisionHandle::empty();
        assert!(!handle.is_available());

        handle.set_backend(MockDecisionBackend { prob: 0.2 });
        assert!(handle.is_available());

        let mut questions = BTreeMap::new();
        questions.insert(
            "preflight".to_string(),
            DecisionQuestion::Noul(generate_behavioral_preflight_question()),
        );
        let res = handle
            .evaluate(None, serde_json::json!({}), questions)
            .await
            .unwrap();
        match res.get("preflight") {
            Some(DecisionAnswer::Noul { noul }) => assert_eq!(*noul, 0.2),
            _ => panic!("Expected Noul answer"),
        }
    }
}
