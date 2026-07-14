use crate::interactions::{InFlightTool, Interaction};

use schemars::JsonSchema;
use serde::Serialize;
use synapto_interface::{
    interaction::{AiSpoken, AiWritten},
    peer_input::PeerInput,
    sync::mpsc,
};
use synapto_llm::LLMResult;

use super::types::{CognitiveLLMOutput, UsersMessagesEvaluation};

use std::future::Future;

#[derive(Debug)]
pub(super) struct SideEffectMetadata {
    pub ai_spoken: Option<AiSpoken>,
    pub ai_written: Option<AiWritten>,
}

pub(super) trait CognitiveOutputProcessor<Cmd>: Send
where
    Cmd: Send + Clone + Default + Serialize + JsonSchema + std::fmt::Debug,
{
    fn sanitize_commands(
        &self,
        commands: &mut Cmd,
        evaluation: &UsersMessagesEvaluation,
        has_resolved_tools: bool,
    );

    // Returns None if the cycle should be aborted without creating an Interaction.
    fn execute_side_effects(
        &mut self,
        model_response: &CognitiveLLMOutput<Cmd>,
    ) -> impl Future<Output = Option<SideEffectMetadata>> + Send;

    // Hook for when the model gives up on input.
    fn on_unintelligible_input(&mut self);

    // Finalization hook.
    fn on_cycle_finished(&mut self);
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn process_llm_output<Cmd, P>(
    new_messages: Vec<PeerInput>,
    pending_user_messages: &mut Vec<PeerInput>,
    generated_text_result: Result<LLMResult<CognitiveLLMOutput<Cmd>>, anyhow::Error>,
    processor: &mut P,
    new_interaction_tx: &mpsc::Sender<Interaction>,
    discard_interaction: bool,
    in_flight_tools: Vec<InFlightTool>,
    has_resolved_tools: bool,
) where
    P: CognitiveOutputProcessor<Cmd>,
    Cmd: Send + Clone + Default + Serialize + JsonSchema + std::fmt::Debug,
{
    if !new_messages.is_empty() {
        let modality = if new_messages
            .iter()
            .all(|msg| matches!(msg, PeerInput::Speech(_)))
        {
            "speech"
        } else if new_messages
            .iter()
            .all(|msg| matches!(msg, PeerInput::Text(_)))
        {
            "text"
        } else {
            "mixed"
        };

        tracing::info!(
            target: "telemetry",
            role = "user",
            modality = modality,
            messages = ?new_messages,
            "User Input"
        );
    }

    match generated_text_result {
        Ok(LLMResult::Interrupted(Some(mut model_response), _)) => {
            tracing::debug!("Interrupted response with output: {:?}", model_response);

            // Apply similar processing for side effects on interrupted responses
            pending_user_messages.extend(new_messages);

            processor.sanitize_commands(
                &mut model_response.commands,
                &model_response.users_messages_evaluation,
                has_resolved_tools,
            );

            let reasoning = model_response.reasoning.clone();
            let is_actionable =
                model_response.users_messages_evaluation == UsersMessagesEvaluation::Actionable;

            if let Some(metadata) = processor.execute_side_effects(&model_response).await {
                if !discard_interaction {
                    tracing::info!(
                        target: "telemetry",
                        role = "assistant",
                        reasoning = *reasoning,
                        messages = ?metadata,
                        "Interaction created"
                    );

                    let interaction = Interaction::new(
                        pending_user_messages.clone(),
                        metadata.ai_spoken,
                        metadata.ai_written,
                        Some(reasoning),
                        is_actionable,
                        in_flight_tools,
                    );

                    if let Err(e) = new_interaction_tx.send(interaction).await {
                        tracing::error!("Failed to send interaction to memory: {:?}", e);
                    }
                }
                pending_user_messages.clear();
            } else {
                tracing::info!("Interrupted turn skipped creating interaction (no side effects).");
                pending_user_messages.clear(); // We still clear the messages to avoid duplicates next turn
            }
        }
        Ok(LLMResult::Interrupted(None, _)) => {
            tracing::debug!("Interrupted response with no output.");
            pending_user_messages.extend(new_messages);

            if !discard_interaction && !in_flight_tools.is_empty() {
                // We still must save an interaction to mark the tools as in-flight
                tracing::info!(
                    target: "telemetry",
                    role = "assistant",
                    tools = format!("{:?}", in_flight_tools),
                    "Interaction created"
                );

                let interaction = Interaction::new(
                    pending_user_messages.clone(),
                    None,
                    None,
                    None,
                    false,
                    in_flight_tools,
                );
                new_interaction_tx
                    .send(interaction)
                    .await
                    .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                    .ok();
                pending_user_messages.clear();
            }
        }
        Ok(LLMResult::Output(mut model_response)) => {
            tracing::debug!("Response: {:?}", model_response);

            let mut messages_for_interaction = pending_user_messages.clone();
            let mut clear_pending_at_end = true;

            match model_response.users_messages_evaluation {
                UsersMessagesEvaluation::Actionable | UsersMessagesEvaluation::NonActionable => {
                    pending_user_messages.extend(new_messages);
                    messages_for_interaction = pending_user_messages.clone();
                }
                UsersMessagesEvaluation::WaitingForMoreInput => {
                    if has_resolved_tools {
                        tracing::info!(
                            "User mid-sentence but tool resolved. Processing tool result, delaying input."
                        );
                        pending_user_messages.extend(new_messages);
                        messages_for_interaction.clear();
                        clear_pending_at_end = false;
                    } else {
                        tracing::info!("Waiting for more input or uncomplete sentence...");
                        pending_user_messages.extend(new_messages);
                        return;
                    }
                }
                UsersMessagesEvaluation::Unintelligible => {
                    if has_resolved_tools {
                        tracing::info!(
                            "Unintelligible input but tool resolved. Dropping input, processing tool."
                        );
                        pending_user_messages.clear();
                        messages_for_interaction.clear();
                    } else {
                        tracing::info!("Unintelligible input, discarding current turn.");
                        processor.on_unintelligible_input();
                        return;
                    }
                }
            }

            // 2. Sanitization: Prevent action hallucinations
            processor.sanitize_commands(
                &mut model_response.commands,
                &model_response.users_messages_evaluation,
                has_resolved_tools,
            );

            let reasoning = model_response.reasoning.clone();

            let is_actionable =
                model_response.users_messages_evaluation == UsersMessagesEvaluation::Actionable;

            if let Some(metadata) = processor.execute_side_effects(&model_response).await {
                if !discard_interaction {
                    tracing::info!(
                        target: "telemetry",
                        role = "assistant",
                        reasoning = *reasoning,
                        messages = ?metadata,
                        "Interaction created"
                    );

                    let interaction = Interaction::new(
                        messages_for_interaction,
                        metadata.ai_spoken,
                        metadata.ai_written,
                        Some(reasoning),
                        is_actionable,
                        in_flight_tools,
                    );

                    if let Err(e) = new_interaction_tx.send(interaction).await {
                        tracing::error!("Failed to send interaction to memory: {:?}", e);
                    }
                } else {
                    tracing::info!(
                        target: "telemetry",
                        role = "assistant",
                        reasoning = *reasoning,
                        "Interaction discarded"
                    );
                }

                if clear_pending_at_end {
                    pending_user_messages.clear();
                }
                processor.on_cycle_finished();
            } else {
                tracing::info!(
                    target: "telemetry",
                    role = "assistant",
                    reasoning = *reasoning,
                    "Interaction not created. Messages added to pending ones."
                );
            }
        }
        Err(e) => {
            tracing::error!("LLM Error: {:?}", e);
            pending_user_messages.extend(new_messages);
        }
    }
}
