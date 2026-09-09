use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::EpisodicMemoryConfig;
use crate::progression::{CognitiveLLMProgression, Progression};
use synapto_interface::interaction::Timestamp;
use synapto_interface::llm::LLMSafe;
use synapto_interface::storage::RecordStore;
use synapto_interface::sync::{mpsc, watch};
use synapto_llm::{Instruction, LLM};

// -------------------------------------------------------------
// Base Episodic Continuum Models
// -------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Continuum {
    pub timestamp: Timestamp,
    pub text: String,
    pub first_progression: Timestamp,
    pub last_progression: Timestamp,
}

impl Continuum {
    pub fn new(text: String, first_progression: Timestamp) -> Self {
        Self {
            timestamp: Timestamp(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_else(|e| panic!("Error: {:?}", e))
                    .as_secs() as i64,
            ),
            text,
            first_progression,
            last_progression: first_progression,
        }
    }
}

impl synapto_interface::llm::LLMSafe for Continuum {}

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
pub struct ContinuumMemory(pub Vec<Continuum>);

// -------------------------------------------------------------
// LLM View Models
// -------------------------------------------------------------

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct CognitiveLLMContinuum(pub String);

impl From<&Continuum> for CognitiveLLMContinuum {
    fn from(continuum: &Continuum) -> Self {
        Self(continuum.text.clone())
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct CognitiveLLMContinuumMemory {
    pub current_continuum: Option<CognitiveLLMContinuum>,
}

impl From<ContinuumMemory> for CognitiveLLMContinuumMemory {
    fn from(continuum_memory: ContinuumMemory) -> Self {
        let current_continuum = continuum_memory
            .iter()
            .last()
            .map(CognitiveLLMContinuum::from);
        Self { current_continuum }
    }
}

// -------------------------------------------------------------
// Background Task: Continuum Memory
// -------------------------------------------------------------

#[derive(JsonSchema, Serialize, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct ContinuumLLMContent {
    new_progression: CognitiveLLMProgression,

    active_continuum: Option<CognitiveLLMContinuum>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct ContinuumLLMOutput {
    #[schemars(description = "None if active_continuum is None or does not need updates")]
    active_continuum_update: Option<CognitiveLLMContinuum>,
    #[schemars(description = "New continuum when a global context shift occurs")]
    new_continuum: Option<CognitiveLLMContinuum>,
}

pub struct ContinuumLLMPrompt {}
impl LLM for ContinuumLLMPrompt {
    type Content = ContinuumLLMContent;
    type Output = ContinuumLLMOutput;
}

#[instrument(skip_all, fields(subsystem))]
pub async fn continuum_memory_task<S: RecordStore>(
    config: EpisodicMemoryConfig,
    store: std::sync::Arc<S>,
    mut new_progression_rx: mpsc::Receiver<Progression>,
    continuum_memory_tx: watch::Sender<ContinuumMemory>,
    llm_executor: synapto_interface::llm::LlmExecutor,
) {
    let mut continuum_memory: ContinuumMemory = match store
        .get_ordered_records(
            "continuums",
            None,
            synapto_interface::storage::SortOrder::Ascending,
        )
        .await
    {
        Ok(items) => ContinuumMemory(items.into_iter().map(|(_, item)| item).collect()),
        Err(e) => {
            tracing::error!("Failed to load continuums: {}", e);
            ContinuumMemory::default()
        }
    };

    continuum_memory_tx.send_replace(continuum_memory.clone());

    let llm_client = ContinuumLLMPrompt::create_client(
        llm_executor,
        config.continuum,
        vec![Instruction::Section(
            Box::new(Instruction::Text(
                "Epic Storyteller / Global Context Synthesizer".to_string(),
            )),
            vec![
                Instruction::Text(
                    "You are recording global long-term context (Saga / Global Context)."
                        .to_string(),
                ),
                Instruction::Text(
                    "Read the progression and determine whether the overall story thread or workspace focus has shifted."
                        .to_string(),
                ),
                Instruction::Text(
                    "Synthesize changes cleanly. Do not repeat micro actions.".to_string(),
                ),
            ],
        )],
    );

    while let Some(new_progression) = new_progression_rx.recv().await {
        let response_res = llm_client
            .call(
                ContinuumLLMContent {
                    new_progression: CognitiveLLMProgression::from(&new_progression),
                    active_continuum: continuum_memory
                        .0
                        .clone()
                        .iter()
                        .last()
                        .map(CognitiveLLMContinuum::from),
                },
                None,
                None,
            )
            .await;

        let ContinuumLLMOutput {
            active_continuum_update,
            new_continuum: new_continuum_creation,
        } = match response_res {
            Ok(output) => output,
            Err(e) => {
                tracing::error!("LLM Call Failed in continuum_memory_task: {}", e);
                continue;
            }
        };

        if let Some(update) = active_continuum_update {
            if let Some(active_continuum) = continuum_memory.last_mut() {
                active_continuum.text = update.0;
            } else if new_continuum_creation.is_none() {
                continuum_memory.push(Continuum::new(update.0, new_progression.timestamp));
            } else {
                tracing::error!(
                    "Received both continuum update and creation while memory is empty"
                );
            }
        }

        if let Some(creation) = new_continuum_creation {
            let new_continuum = Continuum::new(creation.0, new_progression.timestamp);
            continuum_memory.push(new_continuum);
        } else if let Some(active_continuum) = continuum_memory.last_mut() {
            active_continuum.last_progression = new_progression.timestamp;
        }

        continuum_memory_tx
            .send(continuum_memory.clone())
            .inspect_err(|e| tracing::error!("{}", e))
            .ok();

        if let Err(e) = store
            .trim_records_before("continuums", "REPLACE_ME_TODO")
            .await
        {
            tracing::error!("Failed to write continuum memory: {:?}", e);
        }
    }
}
