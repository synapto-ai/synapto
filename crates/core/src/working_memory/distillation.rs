use super::store::{WorkingMemoryEntry, WorkingMemoryState, WorkingMemoryStore};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use synapto_interface::decision::{
    ChoiceQuestion, DecisionAnswer, DecisionHandle, DecisionQuestion,
};
use synapto_interface::interaction::ObservedInteraction;
use synapto_interface::llm::{LLMSafe, LlmExecutor, ModelConfig};
use synapto_interface::peer_input::{PeerInput, Speaker};
use synapto_interface::sync::mpsc;
use synapto_llm::{Instruction, LLM, LLMClient, WithoutTools};
use tracing::instrument;

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

// --- Generative Prompt for Condensing an Original Entry ---

#[derive(JsonSchema, Serialize, Clone, Debug, LLMSafe)]
struct CondenseEntryContent {
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub output: serde_json::Value,
    pub interaction: SummaryLLMInteraction,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, LLMSafe)]
struct CondenseEntryOutput {
    #[schemars(
        description = "A concise, compact summary of the salient facts/numbers from the raw output that remain relevant."
    )]
    pub condensed_output: serde_json::Value,
}

struct CondenseEntryPrompt;

impl LLM for CondenseEntryPrompt {
    type Content = CondenseEntryContent;
    type Output = CondenseEntryOutput;
}

// --- Generative Fallback for Evaluating an Original Entry (when DecisionHandle unavailable) ---

#[derive(JsonSchema, Serialize, Clone, Debug, LLMSafe)]
struct EvaluateOriginalContent {
    pub entry: WorkingMemoryEntry,
    pub interaction: SummaryLLMInteraction,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, LLMSafe, PartialEq, Eq)]
enum OriginalAction {
    Keep,
    Remove,
    Condense,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, LLMSafe)]
struct EvaluateOriginalOutput {
    pub action: OriginalAction,
}

struct EvaluateOriginalPrompt;

impl LLM for EvaluateOriginalPrompt {
    type Content = EvaluateOriginalContent;
    type Output = EvaluateOriginalOutput;
}

// --- Generative Fallback for Evaluating a Condensed Entry (when DecisionHandle unavailable) ---

#[derive(JsonSchema, Serialize, Clone, Debug, LLMSafe)]
struct EvaluateCondensedContent {
    pub entry: WorkingMemoryEntry,
    pub interaction: SummaryLLMInteraction,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, LLMSafe, PartialEq, Eq)]
enum CondensedAction {
    Keep,
    Remove,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, LLMSafe)]
struct EvaluateCondensedOutput {
    pub action: CondensedAction,
}

struct EvaluateCondensedPrompt;

impl LLM for EvaluateCondensedPrompt {
    type Content = EvaluateCondensedContent;
    type Output = EvaluateCondensedOutput;
}

#[instrument(skip_all, fields(subsystem = "working_memory_distillation"))]
pub(super) async fn distillation_task(
    mut observer_rx: mpsc::Receiver<ObservedInteraction>,
    store: WorkingMemoryStore,
    llm_executor: LlmExecutor,
    decision_handle: DecisionHandle,
    model_config: ModelConfig,
) {
    let condense_client: LLMClient<CondenseEntryContent, CondenseEntryOutput, WithoutTools> =
        CondenseEntryPrompt::create_client(
            llm_executor.clone(),
            model_config.clone(),
            vec![Instruction::Text(
                "You are a memory condensation worker. Summarize the raw tool output into a compact representation retaining only essential facts and numbers.".to_string(),
            )],
        );

    let eval_orig_client: LLMClient<EvaluateOriginalContent, EvaluateOriginalOutput, WithoutTools> =
        EvaluateOriginalPrompt::create_client(
            llm_executor.clone(),
            model_config.clone(),
            vec![Instruction::Text(
                "Evaluate what action to take for a raw (Original) tool memory record: 'Keep' if still needed verbatim, 'Remove' if fully answered and obsolete, or 'Condense' if verbose data should be summarized.".to_string(),
            )],
        );

    let eval_cond_client: LLMClient<EvaluateCondensedContent, EvaluateCondensedOutput, WithoutTools> =
        EvaluateCondensedPrompt::create_client(
            llm_executor.clone(),
            model_config.clone(),
            vec![Instruction::Text(
                "Evaluate what action to take for an already condensed tool memory record: 'Keep' if still relevant, or 'Remove' if already answered or obsolete. Note: re-condensing is not permitted.".to_string(),
            )],
        );

    while let Some(interaction) = observer_rx.recv().await {
        let current = store.get().await;
        if current.is_empty() {
            continue;
        }

        let summary_interaction = SummaryLLMInteraction::from(&interaction);
        let mut updated_entries = Vec::new();

        for entry in current.0 {
            match entry.state {
                WorkingMemoryState::Condensed => {
                    // State: Condensed -> Actions: Keep or Remove (NEVER re-condense)
                    let should_remove = if decision_handle.is_available() {
                        let mut criteria = BTreeMap::new();
                        criteria.insert(
                            "keep".to_string(),
                            "The condensed facts remain relevant or useful for upcoming conversation.".to_string(),
                        );
                        criteria.insert(
                            "remove".to_string(),
                            "The facts have been fully consumed, answered, or are no longer relevant to the conversation.".to_string(),
                        );
                        let question = DecisionQuestion::Choice(ChoiceQuestion {
                            instructions: format!(
                                "Evaluate if this condensed tool memory from tool '{}' should be kept or removed based on the latest interaction:",
                                entry.tool_name
                            ),
                            criteria,
                        });
                        let mut questions = BTreeMap::new();
                        questions.insert("action".to_string(), question);
                        let state = serde_json::json!({
                            "entry": entry,
                            "interaction": summary_interaction,
                        });
                        match decision_handle.evaluate(None, state, questions).await {
                            Ok(mut answers) => match answers.remove("action") {
                                Some(DecisionAnswer::Choice { choice, .. }) => choice == "remove",
                                _ => false,
                            },
                            Err(e) => {
                                tracing::warn!(
                                    "Decision evaluation failed for condensed entry, falling back: {}",
                                    e
                                );
                                match eval_cond_client
                                    .call(
                                        EvaluateCondensedContent {
                                            entry: entry.clone(),
                                            interaction: summary_interaction.clone(),
                                        },
                                        None,
                                        None,
                                    )
                                    .await
                                {
                                    Ok(res) => res.action == CondensedAction::Remove,
                                    Err(_) => false,
                                }
                            }
                        }
                    } else {
                        match eval_cond_client
                            .call(
                                EvaluateCondensedContent {
                                    entry: entry.clone(),
                                    interaction: summary_interaction.clone(),
                                },
                                None,
                                None,
                            )
                            .await
                        {
                            Ok(res) => res.action == CondensedAction::Remove,
                            Err(e) => {
                                tracing::warn!("Fallback LLM evaluation failed: {}", e);
                                false
                            }
                        }
                    };

                    if should_remove {
                        tracing::debug!(
                            "Pruned condensed working memory entry: {}",
                            entry.tool_name
                        );
                    } else {
                        updated_entries.push(entry);
                    }
                }
                WorkingMemoryState::Original => {
                    // State: Original -> Actions: Keep, Remove, or Condense
                    let action = if decision_handle.is_available() {
                        let mut criteria = BTreeMap::new();
                        criteria.insert(
                            "keep".to_string(),
                            "The full raw output is still needed as-is because the user has not asked about it yet or full fidelity is required.".to_string(),
                        );
                        criteria.insert(
                            "condense".to_string(),
                            "The tool output contains verbose data where only specific salient facts/numbers need to be preserved compactly.".to_string(),
                        );
                        criteria.insert(
                            "remove".to_string(),
                            "The tool output has been fully used, answered, or is completely obsolete.".to_string(),
                        );
                        let question = DecisionQuestion::Choice(ChoiceQuestion {
                            instructions: format!(
                                "Evaluate what action to take for raw tool output '{}' based on the latest interaction:",
                                entry.tool_name
                            ),
                            criteria,
                        });
                        let mut questions = BTreeMap::new();
                        questions.insert("action".to_string(), question);
                        let state = serde_json::json!({
                            "entry": entry,
                            "interaction": summary_interaction,
                        });
                        match decision_handle.evaluate(None, state, questions).await {
                            Ok(mut answers) => match answers.remove("action") {
                                Some(DecisionAnswer::Choice { choice, .. }) => {
                                    match choice.as_str() {
                                        "remove" => OriginalAction::Remove,
                                        "condense" => OriginalAction::Condense,
                                        _ => OriginalAction::Keep,
                                    }
                                }
                                _ => OriginalAction::Keep,
                            },
                            Err(e) => {
                                tracing::warn!(
                                    "Decision evaluation failed for original entry, falling back: {}",
                                    e
                                );
                                match eval_orig_client
                                    .call(
                                        EvaluateOriginalContent {
                                            entry: entry.clone(),
                                            interaction: summary_interaction.clone(),
                                        },
                                        None,
                                        None,
                                    )
                                    .await
                                {
                                    Ok(res) => res.action,
                                    Err(_) => OriginalAction::Keep,
                                }
                            }
                        }
                    } else {
                        match eval_orig_client
                            .call(
                                EvaluateOriginalContent {
                                    entry: entry.clone(),
                                    interaction: summary_interaction.clone(),
                                },
                                None,
                                None,
                            )
                            .await
                        {
                            Ok(res) => res.action,
                            Err(e) => {
                                tracing::warn!("Fallback LLM evaluation failed: {}", e);
                                OriginalAction::Keep
                            }
                        }
                    };

                    match action {
                        OriginalAction::Remove => {
                            tracing::debug!(
                                "Pruned original working memory entry: {}",
                                entry.tool_name
                            );
                        }
                        OriginalAction::Keep => {
                            updated_entries.push(entry);
                        }
                        OriginalAction::Condense => {
                            // Synthesize condensation via Generative LLM
                            let condense_content = CondenseEntryContent {
                                tool_name: entry.tool_name.clone(),
                                arguments: entry.arguments.clone(),
                                output: entry.output.clone(),
                                interaction: summary_interaction.clone(),
                            };
                            match condense_client.call(condense_content, None, None).await {
                                Ok(res) => {
                                    tracing::debug!(
                                        "Condensed original working memory entry: {}",
                                        entry.tool_name
                                    );
                                    updated_entries.push(WorkingMemoryEntry {
                                        tool_name: entry.tool_name,
                                        arguments: entry.arguments,
                                        output: res.condensed_output,
                                        state: WorkingMemoryState::Condensed,
                                    });
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Condensation LLM call failed, keeping original: {}",
                                        e
                                    );
                                    updated_entries.push(entry);
                                }
                            }
                        }
                    }
                }
            }
        }

        store.replace(updated_entries).await;
    }
}
