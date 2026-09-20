use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use synapto_interface::interaction::ObservedInteraction;
use synapto_interface::llm::{LLMSafe, LlmExecutor, ModelConfig};
use synapto_interface::peer_input::{PeerInput, Speaker};
use synapto_interface::sync::mpsc;
use synapto_interface::working_memory::WorkingMemoryEntry;
use synapto_llm::{Instruction, LLM};
use tracing::instrument;

use super::store::WorkingMemoryStore;

#[derive(JsonSchema, Serialize, Clone, Debug)]
pub(super) struct LLMUserMessage {
    pub speaker: String,
    pub text: String,
}

#[derive(JsonSchema, Serialize, Clone, Debug)]
pub(super) struct CognitiveLLMInteraction {
    pub user_messages: Vec<LLMUserMessage>,
    pub cognitive_spoken: Option<String>,
    pub cognitive_written: Option<String>,
    pub cognitive_reasoning: Option<String>,
}

#[derive(JsonSchema, Serialize, Clone, Debug)]
pub(super) struct SummaryLLMInteraction {
    pub timestamp: i64,
    pub interaction: CognitiveLLMInteraction,
}

impl From<&ObservedInteraction> for SummaryLLMInteraction {
    fn from(interaction: &ObservedInteraction) -> Self {
        let user_messages = interaction
            .user_messages
            .iter()
            .map(|msg| match msg {
                PeerInput::Speech(s) => LLMUserMessage {
                    speaker: match &s.speaker {
                        Speaker::Recognized(id) => id.0.to_string(),
                        Speaker::Unknown(None) => "Unknown".to_string(),
                        Speaker::Unknown(Some(id)) => id.0.to_string(),
                    },
                    text: s.transcript.to_string(),
                },
                PeerInput::Text(t) => LLMUserMessage {
                    speaker: t.sender_id.to_string(),
                    text: t.text.to_string(),
                },
            })
            .collect();

        Self {
            timestamp: interaction.timestamp.0,
            interaction: CognitiveLLMInteraction {
                user_messages,
                cognitive_spoken: interaction.cognitive_spoken.as_ref().map(|s| s.0.clone()),
                cognitive_written: interaction
                    .cognitive_written
                    .as_ref()
                    .map(|w| w.text.clone()),
                cognitive_reasoning: interaction
                    .cognitive_reasoning
                    .as_ref()
                    .map(|r| r.0.clone()),
            },
        }
    }
}

#[derive(JsonSchema, Serialize, Clone, Debug, LLMSafe)]
struct WorkingMemoryDistillationContent {
    pub working_memory: Vec<WorkingMemoryEntry>,
    pub interaction: SummaryLLMInteraction,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, LLMSafe)]
struct WorkingMemoryDistillationOutput {
    #[schemars(
        description = "The updated working memory entries to retain. Omit entries whose facts were already answered or are no longer relevant to current conversation. You may condense the 'output' field of retained entries to save space if only parts of it are needed."
    )]
    pub retained_entries: Vec<WorkingMemoryEntry>,
}

struct WorkingMemoryDistillationPrompt;

impl LLM for WorkingMemoryDistillationPrompt {
    type Content = WorkingMemoryDistillationContent;
    type Output = WorkingMemoryDistillationOutput;
}

#[instrument(skip_all, fields(subsystem = "working_memory_distillation"))]
pub(super) async fn distillation_task(
    mut observer_rx: mpsc::Receiver<ObservedInteraction>,
    store: WorkingMemoryStore,
    llm_executor: LlmExecutor,
    model_config: ModelConfig,
) {
    let client = WorkingMemoryDistillationPrompt::create_client(
        llm_executor,
        model_config,
        vec![
            Instruction::Text(
                "You are an asynchronous memory distillation worker. Your job is to keep working_memory compact and relevant."
                    .to_string(),
            ),
            Instruction::Item(
                "Review the active working_memory entries against the latest interaction."
                    .to_string(),
            ),
            Instruction::Item(
                "If the assistant's speech or written output already fulfilled the user's request and the tool data is no longer needed for subsequent reasoning, OMIT that tool from retained_entries."
                    .to_string(),
            ),
            Instruction::Item(
                "If a tool output is still relevant for upcoming user questions, KEEP it in retained_entries. You may condense its output field to the key salient facts."
                    .to_string(),
            ),
        ],
    );

    while let Some(interaction) = observer_rx.recv().await {
        let current = store.get().await;
        if current.is_empty() {
            continue;
        }

        let content = WorkingMemoryDistillationContent {
            working_memory: current.0.clone(),
            interaction: SummaryLLMInteraction::from(&interaction),
        };

        match client.call(content, None, None).await {
            Ok(output) => {
                tracing::debug!(
                    "Working memory distilled from {} to {} entries",
                    current.len(),
                    output.retained_entries.len()
                );
                store.replace(output.retained_entries).await;
            }
            Err(e) => {
                tracing::warn!("Working memory distillation failed: {}", e);
            }
        }
    }
}
