use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::EpisodicMemoryConfig;
use crate::session::{CognitiveLLMSession, Session};
use synapto_interface::interaction::Timestamp;
use synapto_interface::llm::LLMSafe;
use synapto_interface::storage::RecordStore;
use synapto_interface::sync::{mpsc, watch};
use synapto_llm::{Instruction, LLM};

// -------------------------------------------------------------
// Base Episodic Progression Models
// -------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Progression {
    pub timestamp: Timestamp,
    pub text: String,
    pub first_session: Timestamp,
    pub last_session: Timestamp,
}

impl Progression {
    pub fn new(text: String, first_session: Timestamp) -> Self {
        Self {
            timestamp: Timestamp(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_else(|e| panic!("Error: {:?}", e))
                    .as_secs() as i64,
            ),
            text,
            first_session,
            last_session: first_session,
        }
    }
}

impl synapto_interface::llm::LLMSafe for Progression {}

#[derive(
    derive_more::Deref,
    derive_more::DerefMut,
    derive_more::IntoIterator,
    Serialize,
    Deserialize,
    Default,
    Clone,
    Debug,
    PartialEq,
    Eq,
)]
pub struct ProgressionMemory(pub Vec<Progression>);

// -------------------------------------------------------------
// LLM View Models
// -------------------------------------------------------------

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct CognitiveLLMProgression(pub String);

impl From<&Progression> for CognitiveLLMProgression {
    fn from(progression: &Progression) -> Self {
        Self(progression.text.clone())
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct CognitiveLLMProgressionMemory {
    pub current_progression: Option<CognitiveLLMProgression>,
}

impl From<ProgressionMemory> for CognitiveLLMProgressionMemory {
    fn from(progression_memory: ProgressionMemory) -> Self {
        let current_progression = progression_memory
            .iter()
            .last()
            .map(CognitiveLLMProgression::from);
        Self {
            current_progression,
        }
    }
}

// -------------------------------------------------------------
// Background Task: Progression Memory
// -------------------------------------------------------------

#[derive(JsonSchema, Serialize, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct ProgressionLLMContent {
    new_session: CognitiveLLMSession,
    active_progression: Option<CognitiveLLMProgression>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct ProgressionLLMOutput {
    #[schemars(description = "None if active_progression is None or does not need updates")]
    active_progression_update: Option<CognitiveLLMProgression>,
    #[schemars(description = "New progression milestone when a phase or chapter concludes")]
    new_progression: Option<CognitiveLLMProgression>,
}

pub struct ProgressionLLMPrompt {}
impl LLM for ProgressionLLMPrompt {
    type Content = ProgressionLLMContent;
    type Output = ProgressionLLMOutput;
}

#[instrument(skip_all, fields(subsystem))]
pub async fn progression_memory_task<S: RecordStore>(
    config: EpisodicMemoryConfig,
    store: std::sync::Arc<S>,
    mut new_session_rx: mpsc::Receiver<Session>,
    progression_memory_tx: watch::Sender<ProgressionMemory>,
    new_progression_tx: mpsc::Sender<Progression>,
    llm_executor: synapto_interface::llm::LlmExecutor,
) {
    let mut progression_memory: ProgressionMemory = match store
        .get_ordered_records(
            "progressions",
            None,
            synapto_interface::storage::SortOrder::Ascending,
        )
        .await
    {
        Ok(items) => ProgressionMemory(items.into_iter().map(|(_, item)| item).collect()),
        Err(e) => {
            tracing::error!("Failed to load progressions: {}", e);
            ProgressionMemory::default()
        }
    };

    progression_memory_tx.send_replace(progression_memory.clone());

    let llm_client = ProgressionLLMPrompt::create_client(
        llm_executor,
        config.progression,
        vec![Instruction::Section(
            Box::new(Instruction::Text(
                "Story Chronicler / Tech Lead".to_string(),
            )),
            vec![
                Instruction::Text(
                    "You are a chronicler analyzing progressions and high-level milestones."
                        .to_string(),
                ),
                Instruction::Text(
                    "Read the incoming scene/session and incorporate it into the current chapter plotline or milestone."
                        .to_string(),
                ),
                Instruction::Text(
                    "Do not repeat details step by step. Summarize ONLY the causality, decisions, and outcomes."
                        .to_string(),
                ),
            ],
        )],
    );

    while let Some(new_session) = new_session_rx.recv().await {
        let response_res = llm_client
            .call(
                ProgressionLLMContent {
                    new_session: CognitiveLLMSession::from(&new_session),
                    active_progression: progression_memory
                        .0
                        .clone()
                        .iter()
                        .last()
                        .map(CognitiveLLMProgression::from),
                },
                None,
                None,
            )
            .await;

        let ProgressionLLMOutput {
            active_progression_update,
            new_progression: new_progression_creation,
        } = match response_res {
            Ok(output) => output,
            Err(e) => {
                tracing::error!("LLM Call Failed in progression_memory_task: {}", e);
                continue;
            }
        };

        if let Some(update) = active_progression_update {
            if let Some(active_progression) = progression_memory.last_mut() {
                active_progression.text = update.0;
            } else if new_progression_creation.is_none() {
                progression_memory.push(Progression::new(update.0, new_session.timestamp));
            } else {
                tracing::error!(
                    "Received both progression update and creation while memory is empty"
                );
            }
        }

        if let Some(creation) = new_progression_creation {
            let new_progression = Progression::new(creation.0, new_session.timestamp);
            progression_memory.push(new_progression.clone());
            new_progression_tx
                .send(new_progression)
                .await
                .inspect_err(|e| tracing::error!("{}", e))
                .ok();
        } else if let Some(active_progression) = progression_memory.last_mut() {
            active_progression.last_session = new_session.timestamp;
        }

        progression_memory_tx
            .send(progression_memory.clone())
            .inspect_err(|e| tracing::error!("{}", e))
            .ok();

        if let Err(e) = store
            .trim_records_before("progressions", "REPLACE_ME_TODO")
            .await
        {
            tracing::error!("Failed to write progression memory: {:?}", e);
        }
    }
}
