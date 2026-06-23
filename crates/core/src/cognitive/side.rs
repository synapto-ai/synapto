use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use synapto_interface::llm::LLMSafe;
use std::sync::Arc;
use synapto_interface::sync::{broadcast, mpsc, watch};
use synapto_interface::types::{CognitiveState, CognitiveStateUpdate};
use synapto_llm_client::LLM;
use synapto_llm_client::LLMClient;
use tracing::instrument;

use super::{
    processor::{CognitiveOutputProcessor, SideEffectMetadata, process_llm_output},
    types::{CognitiveLLM, CognitiveLLMContent, CognitiveLLMOutput},
};

use crate::interactions::types::CognitiveOutputText;

use crate::{
    config::Config,
    interactions::{
        Interaction, InteractionMemory,
        recent::LLMUserMessage,
        types::{InFlightTool, PeerInput, PeerInputText},
    },
};

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Clone, Default, LLMSafe)]
#[serde(default)]
/// Side effects produced by the AI. Omit not used fields.
struct CognitiveSideCommands {
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Write something to the chat. Snippets, code, links. Everything that is not natural to say. Mention it in the speech when you have written something in the chat. Null if you have nothing to write now.
    pub write: Option<CognitiveOutputText>,

    #[serde(flatten)]
    pub commands_map: std::collections::BTreeMap<String, serde_json::Value>,
}

struct SideOutputProcessor<'a> {
    cognitive_output_text_tx: Option<&'a mpsc::Sender<CognitiveOutputText>>,
    commands_registry: &'a Arc<synapto_interface::types::CommandRegistryBuilder>,
}

impl<'a> CognitiveOutputProcessor<CognitiveSideCommands> for SideOutputProcessor<'a> {
    fn sanitize_commands(
        &self,
        commands: &mut CognitiveSideCommands,
        evaluation: &super::types::UsersMessagesEvaluation,
    ) {
        if evaluation != &super::types::UsersMessagesEvaluation::Actionable {
            commands.write = None;
        }

        if self.cognitive_output_text_tx.is_none() && commands.write.is_some() {
            tracing::warn!("Chat plugin not wired; dropping write command.");
            commands.write = None;
        }
    }

    async fn execute_side_effects(
        &mut self,
        model_response: &CognitiveLLMOutput<CognitiveSideCommands>,
    ) -> Option<SideEffectMetadata> {
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

        let ai_written = model_response.commands.write.as_ref().map(|cmd| {
            crate::interactions::types::AiWritten {
                target_channel: cmd.target_channel.clone(),
                text: cmd.text.clone(),
            }
        });

        Some(SideEffectMetadata {
            ai_spoken: None,
            ai_written,
        })
    }

    fn on_unintelligible_input(&mut self) {}

    fn on_cycle_finished(&mut self) {}
}

#[instrument(skip_all, fields(subsystem))]
#[allow(clippy::too_many_arguments)]
pub async fn cognitive_side_task<P: super::prompt_provider::CognitivePromptProvider>(
    config: Config,
    mut text_rx: broadcast::Receiver<PeerInputText>,
    mut interaction_memory_rx: watch::Receiver<InteractionMemory>,
    new_interaction_tx: mpsc::Sender<Interaction>,
    registries: Arc<synapto_interface::types::ContextRegistries>,
    tools: Arc<synapto_interface::types::ToolRegistryBuilder>,
    commands: Arc<synapto_interface::types::CommandRegistryBuilder>,

    cognitive_output_text_tx: Option<mpsc::Sender<CognitiveOutputText>>,

    cognitive_state_tx: broadcast::Sender<CognitiveStateUpdate>,
    llm_executor: std::sync::Arc<dyn synapto_interface::llm::LlmExecutor>,
    resolve_in_flight_tool_tx: mpsc::Sender<synapto_interface::types::ToolCallId>,
) {
    let (tool_resolved_tx, mut tool_resolved_rx) = tokio::sync::mpsc::channel(10);

    let executor = crate::cognitive::types::RegistryToolExecutor {
        tool_resolved_tx,
        tools: tools.clone(),
    };

    let llm_client: LLMClient<
        CognitiveLLMContent,
        CognitiveLLMOutput<CognitiveSideCommands>,
        synapto_llm_client::WithTools<crate::cognitive::types::RegistryToolExecutor>,
    > = CognitiveLLM::create_client_with_tools(
        llm_executor,
        config.cognitive.clone(),
        super::get_cognitive_system_prompt::<P>(&config),
        executor,
        vec![], // Tools are dynamically passed in each turn now
    );

    let mut historical_rx =
        registries.subscribe(synapto_interface::types::TemporalScope::Historical);

    tokio::try_join!(interaction_memory_rx.changed(), async {
        if !registries
            .historical
            .providers
            .read()
            .unwrap_or_else(|e| panic!("Historical providers lock poisoned: {:?}", e))
            .is_empty()
        {
            historical_rx.changed().await
        } else {
            Ok(())
        }
    },)
    .inspect_err(|e| tracing::error!("{}", e))
    .unwrap_or_else(|e| panic!("Historical provider background task failed: {:?}", e));

    let mut pending_user_messages: Vec<PeerInput> = Vec::new();

    loop {
        let (do_process, msg, resolved_tools) = tokio::select! {
            res = text_rx.recv() => {
                let Ok(msg) = res else {
                    break;
                };
                tracing::debug!("Cognitive Side Task triggered by text message");
                (true, Some(msg), None)
            }
            res = tool_resolved_rx.recv() => {
                let Some((doc_text, tool_call)) = res else {
                    break;
                };

                tracing::debug!("Cognitive Side Task triggered by tool resolution: {}", tool_call.fn_name);

                // Remove the in-flight tool from memory since it has resolved
                if let Err(e) = resolve_in_flight_tool_tx.send(synapto_interface::types::ToolCallId(tool_call.call_id.clone())).await {
                    tracing::warn!("Failed to request resolving in-flight tool marker: {}", e);
                }

                (true, None, Some(vec![(tool_call, doc_text)]))
            }
        };

        if !do_process {
            continue;
        }

        let new_messages = if let Some(ref m) = msg {
            vec![PeerInput::Text(m.clone())]
        } else {
            vec![]
        };

        let mut current_messages = pending_user_messages.clone();
        current_messages.extend(new_messages.clone());

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

            let ai_output = i
                .ai_spoken
                .as_ref()
                .map(|spoken| spoken.0.clone())
                .or_else(|| i.ai_written.as_ref().map(|written| written.text.clone()));

            recent_interactions.push(synapto_interface::types::ContextInteraction {
                peer_input,
                ai_reasoning: i.ai_reasoning.as_ref().map(|r| r.0.clone()),
                ai_output,
            });
        }
        let request = synapto_interface::types::ContextRequest {
            recent_interactions,
            ..Default::default()
        };

        let mut historical_contexts = std::collections::BTreeMap::new();
        let providers: Vec<_> = registries
            .historical
            .providers
            .read()
            .unwrap_or_else(|e| panic!("Historical providers lock poisoned: {:?}", e))
            .clone();
        for provider in providers {
            if let Ok(val) = provider.erased_context(&request).await {
                historical_contexts.insert(provider.name().to_string(), val);
            }
        }

        let mut current_contexts = std::collections::BTreeMap::new();
        let providers: Vec<_> = registries
            .current
            .providers
            .read()
            .unwrap_or_else(|e| panic!("Current providers lock poisoned: {:?}", e))
            .clone();
        for provider in providers {
            if let Ok(val) = provider.erased_context(&request).await {
                current_contexts.insert(provider.name().to_string(), val);
            }
        }

        let mut prospective_contexts = std::collections::BTreeMap::new();
        let providers: Vec<_> = registries
            .prospective
            .providers
            .read()
            .unwrap_or_else(|e| panic!("Prospective providers lock poisoned: {:?}", e))
            .clone();
        for provider in providers {
            if let Ok(val) = provider.erased_context(&request).await {
                prospective_contexts.insert(provider.name().to_string(), val);
            }
        }

        let content = CognitiveLLMContent {
            historical_contexts,
            current_contexts,
            prospective_contexts,

            interaction_memory: interaction_memory.into(),

            user_messages: current_messages
                .clone()
                .into_iter()
                .map(LLMUserMessage::from)
                .collect(),
        };

        if let Some(msg) = &msg {
            cognitive_state_tx
                .send(CognitiveStateUpdate {
                    context: msg.channel.context.clone(),
                    state: CognitiveState::Thinking,
                })
                .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                .ok();
        }

        let content_value = serde_json::to_value(&content)
            .unwrap_or_else(|e| panic!("Failed to serialize content: {}", e));
        let dynamic_tools =
            crate::cognitive::types::evaluate_dynamic_tools(&tools, &request, &content_value).await;

        let prompt_config: P::Config =
            serde_json::from_value(config.prompt.clone()).unwrap_or_default();
        let dynamic_instructions = P::get_dynamic_instructions(
            &prompt_config,
            &content,
            false,
            crate::cognitive::prompt_provider::CognitiveTarget::Side,
        );

        let generated_text_result = llm_client
            .call(
                content,
                Some(dynamic_instructions).filter(|v| !v.is_empty()),
                None,
                resolved_tools.clone(),
                Some(dynamic_tools),
                request,
            )
            .await;

        let mut processor = SideOutputProcessor {
            cognitive_output_text_tx: cognitive_output_text_tx.as_ref(),
            commands_registry: &commands,
        };

        let in_flight_tools = match &generated_text_result {
            Ok(synapto_llm_client::LLMResult::Interrupted(_, tool_calls)) => tool_calls
                .iter()
                .map(|call| InFlightTool {
                    id: call.call_id.clone(),
                    name: call.fn_name.clone(),
                    arguments: call.fn_arguments.clone(),
                })
                .collect(),
            _ => vec![],
        };

        process_llm_output(
            new_messages.clone(),
            &mut pending_user_messages,
            generated_text_result,
            &mut processor,
            &new_interaction_tx,
            false,
            in_flight_tools,
        )
        .await;

        if let Some(msg) = msg {
            cognitive_state_tx
                .send(CognitiveStateUpdate {
                    context: msg.channel.context,
                    state: CognitiveState::Idle,
                })
                .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                .ok();
        }

        tracing::info!("\n✅ Cognitive task finished.\n");
    }
}
