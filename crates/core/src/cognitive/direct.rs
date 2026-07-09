use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use synapto_interface::cognitive::CognitiveOutputSpeech;
use synapto_interface::cognitive_output_text::CognitiveOutputText;
use synapto_interface::interaction::{AiSpoken, AiWritten};
use synapto_interface::llm::LLMSafe;
use synapto_interface::peer_input::{PeerInput, PeerInputSpeech};
use synapto_interface::sync::{Notify, futures::Notified};
use synapto_interface::sync::{broadcast, mpsc, watch};
//
use synapto_llm::LLM;
use synapto_llm::LLMClient;
use tracing::instrument;

use crate::config::Config;
use crate::interactions::InteractionMemory;
use crate::interactions::{InFlightTool, Interaction};
use crate::prompt_provider::{CognitivePromptProvider, CognitiveTarget};

use super::{
    processor::{CognitiveOutputProcessor, SideEffectMetadata, process_llm_output},
    types::{CognitiveLLM, CognitiveLLMContent, CognitiveLLMOutput},
};

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Clone, Default, LLMSafe)]
#[serde(default)]
/// Commands to send to the robot you control. Omit not used fields.
struct CognitiveDirectCommands {
    #[serde(skip_serializing_if = "Option::is_none")]
    /// tell something loudly. Don't use abbreviation. Omit the field if you have nothing to say now.
    say: Option<CognitiveOutputSpeech>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Write something to the chat. Snippets, code, links. Everything that is not natural to say. Mention it in the speech when you have written something in the chat. Omit the field if you have nothing to write now.
    write: Option<CognitiveOutputText>,

    #[serde(flatten)]
    commands_map: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Default)]
pub(crate) struct CognitiveDirectTrigger(Arc<Notify>);

impl CognitiveDirectTrigger {
    #[track_caller]
    pub(crate) fn trigger(&self) {
        let caller = std::panic::Location::caller();
        tracing::debug!(
            "CognitiveDirectTrigger triggered from {}:{}:{}",
            caller.file(),
            caller.line(),
            caller.column()
        );
        self.0.notify_one();
    }

    fn triggered(&self) -> Notified<'_> {
        self.0.notified()
    }
}

#[derive(Clone, Default)]
pub(crate) struct CognitiveDirectInterrupt(Arc<Notify>);

impl CognitiveDirectInterrupt {
    #[allow(unused)]
    fn interrupt(&self) {
        self.0.notify_waiters();
    }

    fn interrupted(&self) -> Notified<'_> {
        self.0.notified()
    }

    pub(crate) fn inner(&self) -> &Arc<Notify> {
        &self.0
    }
}

struct DirectOutputProcessor<'a> {
    cognitive_speech_tx: &'a broadcast::Sender<CognitiveOutputSpeech>,
    cognitive_output_text_tx: Option<&'a mpsc::Sender<CognitiveOutputText>>,
    initial_cognitive_trigger: &'a mut bool,
    commands_registry: &'a Arc<synapto_interface::command::CommandRegistryBuilder>,
}

impl<'a> CognitiveOutputProcessor<CognitiveDirectCommands> for DirectOutputProcessor<'a> {
    fn sanitize_commands(
        &self,
        commands: &mut CognitiveDirectCommands,
        evaluation: &super::types::UsersMessagesEvaluation,
        has_resolved_tools: bool,
    ) {
        if evaluation != &super::types::UsersMessagesEvaluation::Actionable && !has_resolved_tools {
            commands.say = None;
            commands.write = None;
        }

        if self.cognitive_output_text_tx.is_none() && commands.write.is_some() {
            tracing::warn!("Chat plugin not wired; dropping write command.");
            commands.write = None;
        }
    }

    async fn execute_side_effects(
        &mut self,
        model_response: &CognitiveLLMOutput<CognitiveDirectCommands>,
    ) -> Option<SideEffectMetadata> {
        let say_command = model_response.commands.say.clone();

        if let Some(ref say) = say_command {
            self.cognitive_speech_tx
                .send(say.clone())
                .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                .ok();
            tracing::info!("\n🔊 '{:?}'", say);
        }

        if let Some(ref chat_message) = model_response.commands.write
            && let Some(tx) = self.cognitive_output_text_tx
        {
            tx.send(chat_message.clone())
                .await
                .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                .ok();
        }

        for (cmd_name, cmd_args) in &model_response.commands.commands_map {
            let cmd_opt = self
                .commands_registry
                .commands
                .read()
                .unwrap_or_else(|e| panic!("Commands lock poisoned: {:?}", e))
                .get(cmd_name)
                .cloned();
            if let Some(cmd) = cmd_opt {
                if let Err(e) = cmd.erased_execute(cmd_args.clone()).await {
                    tracing::error!("Failed to execute command '{}': {}", cmd_name, e);
                } else {
                    tracing::info!("Successfully executed command '{}'", cmd_name);
                }
            } else {
                tracing::warn!("No command registered for name '{}'", cmd_name);
            }
        }

        let ai_spoken = say_command.map(|cmd| AiSpoken(cmd.text.clone()));

        let ai_written = model_response.commands.write.as_ref().map(|cmd| AiWritten {
            target_channel: cmd.target_channel.clone(),
            text: cmd.text.clone(),
        });

        Some(SideEffectMetadata {
            ai_spoken,
            ai_written,
        })
    }

    fn on_unintelligible_input(&mut self) {
        *self.initial_cognitive_trigger = false;
    }

    fn on_cycle_finished(&mut self) {
        *self.initial_cognitive_trigger = false;
    }
}

#[instrument(skip_all, fields(subsystem))]
#[allow(clippy::too_many_arguments)]
pub(super) async fn cognitive_direct_task<P: CognitivePromptProvider>(
    config: Config,
    trigger: CognitiveDirectTrigger,
    interrupt: CognitiveDirectInterrupt,
    ai_speaking_semaphore: Arc<tokio::sync::Semaphore>,
    mut peer_input_speech_rx: mpsc::Receiver<PeerInputSpeech>,
    mut interaction_memory_rx: watch::Receiver<InteractionMemory>,
    cognitive_speech_tx: broadcast::Sender<CognitiveOutputSpeech>,
    new_interaction_tx: mpsc::Sender<Interaction>,
    video_rx: Option<watch::Receiver<synapto_interface::camera::CameraInputFrame>>,
    registries: Arc<synapto_interface::context::ContextRegistries>,
    tools: Arc<synapto_interface::tool::ToolRegistryBuilder>,
    commands: Arc<synapto_interface::command::CommandRegistryBuilder>,
    cognitive_output_text_tx: Option<mpsc::Sender<CognitiveOutputText>>,
    llm_executor: std::sync::Arc<dyn synapto_interface::llm::LlmExecutor>,
    resolve_in_flight_tool_tx: mpsc::Sender<synapto_interface::tool::ToolCallId>,
) {
    let (tool_resolved_tx, mut tool_resolved_rx) = tokio::sync::mpsc::channel(10);

    let executor = crate::cognitive::types::RegistryToolExecutor {
        tool_resolved_tx,
        tools: tools.clone(),
    };

    let llm_client: LLMClient<
        CognitiveLLMContent,
        CognitiveLLMOutput<CognitiveDirectCommands>,
        synapto_llm::WithTools<crate::cognitive::types::RegistryToolExecutor>,
    > = CognitiveLLM::create_client_with_tools(
        llm_executor,
        config.cognitive.clone(),
        super::get_cognitive_system_prompt::<P>(&config),
        executor,
        vec![], // dynamically provided each turn
    );

    // -- Build the chat request

    // model.safety_settings = Some(vec![
    //     google_ai_rs::proto::SafetySetting {
    //         category: google_ai_rs::proto::HarmCategory::Harassment.into(),
    //         threshold: google_ai_rs::proto::safety_setting::HarmBlockThreshold::BlockLowAndAbove
    //             .into(),
    //     },
    //     google_ai_rs::proto::SafetySetting {
    //         category: google_ai_rs::proto::HarmCategory::HateSpeech.into(),
    //         threshold: google_ai_rs::proto::safety_setting::HarmBlockThreshold::BlockLowAndAbove
    //             .into(),
    //     },
    //     google_ai_rs::proto::SafetySetting {
    //         category: google_ai_rs::proto::HarmCategory::SexuallyExplicit.into(),
    //         threshold: google_ai_rs::proto::safety_setting::HarmBlockThreshold::BlockLowAndAbove
    //             .into(),
    //     },
    //     google_ai_rs::proto::SafetySetting {
    //         category: google_ai_rs::proto::HarmCategory::DangerousContent.into(),
    //         threshold: google_ai_rs::proto::safety_setting::HarmBlockThreshold::BlockLowAndAbove
    //             .into(),
    //     },
    //     google_ai_rs::proto::SafetySetting {
    //         category: google_ai_rs::proto::HarmCategory::CivicIntegrity.into(),
    //         threshold: google_ai_rs::proto::safety_setting::HarmBlockThreshold::BlockLowAndAbove
    //             .into(),
    //     },
    // ]);

    let mut pending_user_messages: Vec<PeerInput> = Vec::new();

    let mut initial_cognitive_trigger = config.initial_run.automatic_cognitive_trigger;

    let mut run_after_unpaused = false;

    let mut historical_rx =
        registries.subscribe(synapto_interface::context::TemporalScope::Historical);

    // wait for all memories are loaded
    tokio::try_join!(
        interaction_memory_rx.changed(),
        async {
            if !registries.historical.is_empty() {
                historical_rx.changed().await
            } else {
                Ok(())
            }
        },
        async { Ok(()) },
    )
    .inspect_err(|e| tracing::error!("{}", e))
    .unwrap_or_else(|e| panic!("Historical provider background task failed: {:?}", e));

    loop {
        let mut resolved_tools = None;
        let mut _cycle_permit: Option<tokio::sync::OwnedSemaphorePermit> = None;

        // Ignore notifications when cognitive task is paused
        if initial_cognitive_trigger {
            tracing::debug!("First run: bypassing pause checks and proceeding immediately");
            _cycle_permit = Some(
                ai_speaking_semaphore
                    .clone()
                    .acquire_owned()
                    .await
                    .unwrap_or_else(|e| panic!("Semaphore closed: {:?}", e)),
            );
        } else {
            loop {
                better_tokio_select::tokio_select!(match .. {
                    .. if let _ = trigger.triggered() => {
                        if config.barge_in || ai_speaking_semaphore.available_permits() > 0 {
                            tracing::debug!("Trigger detected. Waiting for interaction lock...");
                            run_after_unpaused = true;
                        } else {
                            tracing::debug!(
                                "Trigger ignored: AI is speaking and barge-in is disabled"
                            );
                        }
                    }
                    .. if let permit = ai_speaking_semaphore.clone().acquire_owned()
                        && run_after_unpaused =>
                    {
                        tracing::debug!("Interaction lock acquired. Proceeding.");
                        _cycle_permit =
                            Some(permit.unwrap_or_else(|e| panic!("Semaphore closed: {:?}", e)));
                        break;
                    }
                    .. if let res = tool_resolved_rx.recv() => {
                        if let Some((doc_text, tool_call)) = res {
                            tracing::debug!(
                                "Cognitive Direct Task triggered by tool resolution: {}",
                                tool_call.fn_name
                            );
                            resolved_tools = Some(vec![(tool_call.clone(), doc_text)]);
                            // Document results also need the lock to start a cycle
                            run_after_unpaused = true;

                            // Remove the in-flight tool from memory since it has resolved
                            if let Err(e) = resolve_in_flight_tool_tx
                                .send(synapto_interface::tool::ToolCallId(
                                    tool_call.call_id.clone(),
                                ))
                                .await
                            {
                                tracing::warn!(
                                    "Failed to request resolving in-flight tool marker: {}",
                                    e
                                );
                            }
                        } else {
                            break;
                        }
                    }
                })
            }
        }

        run_after_unpaused = false;

        tracing::info!("\n💡 AI woken up. Starting cognitive cycle...");

        let new_speech_messages: Vec<PeerInputSpeech> =
            std::iter::from_fn(|| peer_input_speech_rx.try_recv().ok()).collect();

        let mut current_messages = pending_user_messages.clone();
        current_messages.extend(
            new_speech_messages
                .clone()
                .into_iter()
                .map(PeerInput::Speech),
        );

        let _video_frame = video_rx
            .as_ref()
            .map(|rx| rx.borrow().clone().data)
            .unwrap_or_default();
        let interaction_memory = interaction_memory_rx.borrow().clone();

        let mut recent_interactions = Vec::new();
        for i in interaction_memory.iter() {
            let mut peer_input = None;
            if let Some(msg) = i.user_messages.first() {
                match msg {
                    PeerInput::Speech(s) => {
                        peer_input = Some(s.transcript.to_string());
                    }
                    PeerInput::Text(t) => {
                        peer_input = Some(t.text.to_string());
                    }
                }
            }

            let ai_output = if let Some(spoken) = &i.ai_spoken {
                Some(spoken.0.clone())
            } else {
                i.ai_written.as_ref().map(|written| written.text.clone())
            };

            recent_interactions.push(synapto_interface::context::ContextInteraction {
                peer_input,
                ai_reasoning: i.ai_reasoning.as_ref().map(|r| r.0.clone()),
                ai_output,
            });
        }
        let request = synapto_interface::context::ContextRequest {
            recent_interactions,
            initial_run: initial_cognitive_trigger,
        };

        let historical_contexts = registries.historical.gather_contexts(&request).await;
        let current_contexts = registries.current.gather_contexts(&request).await;
        let prospective_contexts = registries.prospective.gather_contexts(&request).await;

        let content = CognitiveLLMContent {
            historical_contexts,
            current_contexts,
            prospective_contexts,

            interaction_memory: interaction_memory.into(),

            user_messages: current_messages
                .clone()
                .into_iter()
                .map(Into::into)
                .collect(),
        };

        let current_is_initial_run = initial_cognitive_trigger;

        let override_reasoning_effort = if initial_cognitive_trigger {
            Some(config.initial_run.reasoning_effort)
        } else {
            None
        };

        let content_value = serde_json::to_value(&content)
            .unwrap_or_else(|e| panic!("Failed to serialize content: {}", e));
        let dynamic_tools =
            super::types::evaluate_dynamic_tools(&tools, &request, &content_value).await;

        let prompt_config: P::Config =
            serde_json::from_value(config.prompt.clone()).unwrap_or_default();
        let dynamic_instructions = P::get_dynamic_instructions(
            &prompt_config,
            &content,
            initial_cognitive_trigger,
            CognitiveTarget::Direct,
        );

        let generated_text_result = better_tokio_select::tokio_select!(match .. {
            .. if let res = llm_client.call(
                content,
                Some(dynamic_instructions).filter(|v| !v.is_empty()),
                override_reasoning_effort,
                resolved_tools.clone(),
                Some(dynamic_tools),
                request,
            ) =>
                res,
            .. if let _ = interrupt.interrupted() => {
                tracing::info!("Cognitive task interrupted");
                pending_user_messages
                    .extend(new_speech_messages.into_iter().map(PeerInput::Speech));
                continue;
            }
        });

        let mut processor = DirectOutputProcessor {
            cognitive_speech_tx: &cognitive_speech_tx,
            cognitive_output_text_tx: cognitive_output_text_tx.as_ref(),
            initial_cognitive_trigger: &mut initial_cognitive_trigger,
            commands_registry: &commands,
        };

        let in_flight_tools = match &generated_text_result {
            Ok(synapto_llm::LLMResult::Interrupted(_, tool_calls)) => tool_calls
                .iter()
                .map(|call| InFlightTool {
                    id: call.call_id.clone(),
                    name: call.fn_name.clone(),
                    arguments: call.fn_arguments.clone(),
                })
                .collect(),
            _ => vec![],
        };

        let has_resolved_tools = resolved_tools.is_some();

        process_llm_output(
            new_speech_messages
                .into_iter()
                .map(PeerInput::Speech)
                .collect(),
            &mut pending_user_messages,
            generated_text_result,
            &mut processor,
            &new_interaction_tx,
            current_is_initial_run && config.initial_run.discard_interaction,
            in_flight_tools,
            has_resolved_tools,
        )
        .await;
    }
}
