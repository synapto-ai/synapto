//! # Cognitive Module
//!
//! This module manages the main LLM cognitive loops (`direct` for voice/primary and `side` for text/background).
//!
//! ## Asynchronous Native Tool Resolution
//!
//! When a cognitive task invokes a tool, the tool is spawned into a background task, keeping the main AI loop
//! responsive. The routing that determines **which cognitive task is triggered** when a tool resolves operates
//! entirely through **channel ownership** (Inversion of Control), rather than global state or a central router.
//!
//! ### Triggering and Routing Architecture
//!
//! 1. **Channel Provisioning:** Each cognitive loop (`direct` or `side`) creates its own isolated `mpsc::channel`
//!    (`tool_resolved_tx` / `tool_resolved_rx`) upon starting.
//! 2. **Injected Ownership:** The loop passes its specific transmitter (`tool_resolved_tx`) into the `RegistryToolExecutor`.
//! 3. **Background Execution:** The executor `tokio::spawn`s an isolated background task for the tool. This task captures
//!    the transmitter, wiring it exclusively to the loop that originated the call.
//! 4. **Resumption:** The cognitive loop waits concurrently in a `better_tokio_select::tokio_select!` for either new user input or a
//!    message on its `tool_resolved_rx` channel. When the tool finishes, it sends the output back through the
//!    transmitter, immediately waking the correct loop and triggering the next LLM cycle.

mod direct;
pub mod processor;
pub mod prompt_provider;
mod side;
pub mod speaking_coordinator;
pub mod types;
use derive_more::Display;
use std::sync::Arc;
use synapto_interface::sync::{broadcast, mpsc, watch};
use synapto_interface::types::{CognitiveOutputSpeech, CognitiveStateUpdate};

use crate::interactions::types::CognitiveOutputText;

use synapto_llm::Instruction;

use crate::{
    config::Config,
    interactions::{
        Interaction, InteractionMemory,
        types::{PeerInputSpeech, PeerInputText},
    },
};

use direct::cognitive_direct_task;
use side::cognitive_side_task;

#[allow(clippy::too_many_arguments)]
pub async fn start<P: prompt_provider::CognitivePromptProvider>(
    config: Config,
    llm_executor: std::sync::Arc<dyn synapto_interface::llm::LlmExecutor>,
    trigger_cognitive_direct: CognitiveDirectTrigger,
    interrupt_cognitive_direct: CognitiveDirectInterrupt,
    ai_speaking_semaphore: Arc<tokio::sync::Semaphore>,
    text_rx: broadcast::Receiver<PeerInputText>,
    peer_input_speech_rx: mpsc::Receiver<PeerInputSpeech>,
    interaction_memory_rx: watch::Receiver<InteractionMemory>,
    cognitive_speech_tx: broadcast::Sender<CognitiveOutputSpeech>,
    new_interaction_tx: mpsc::Sender<Interaction>,
    video_rx: Option<watch::Receiver<synapto_interface::types::CameraInputFrame>>,

    registries: Arc<synapto_interface::types::ContextRegistries>,
    tools: Arc<synapto_interface::types::ToolRegistryBuilder>,
    commands: Arc<synapto_interface::types::CommandRegistryBuilder>,

    cognitive_output_text_tx: Option<mpsc::Sender<CognitiveOutputText>>,

    cognitive_state_tx: broadcast::Sender<CognitiveStateUpdate>,
    resolve_in_flight_tool_tx: mpsc::Sender<synapto_interface::types::ToolCallId>,
) {
    if cognitive_output_text_tx.is_some() && !config.disable_cognitive_side {
        tokio::spawn(cognitive_side_task::<P>(
            config.clone(),
            text_rx,
            interaction_memory_rx.clone(),
            new_interaction_tx.clone(),
            registries.clone(),
            tools.clone(),
            commands.clone(),
            cognitive_output_text_tx.clone(),
            cognitive_state_tx.clone(),
            llm_executor.clone(),
            resolve_in_flight_tool_tx.clone(),
        ));
    }
    if !config.disable_cognitive_direct {
        tokio::spawn(cognitive_direct_task::<P>(
            config,
            trigger_cognitive_direct,
            interrupt_cognitive_direct,
            ai_speaking_semaphore,
            peer_input_speech_rx,
            interaction_memory_rx,
            cognitive_speech_tx,
            new_interaction_tx,
            video_rx,
            registries,
            tools,
            commands,
            cognitive_output_text_tx,
            llm_executor,
            resolve_in_flight_tool_tx,
        ));
    }
}

#[derive(Display)]
pub struct Capability(pub &'static str);

pub use direct::{CognitiveDirectInterrupt, CognitiveDirectTrigger};

pub use types::CognitiveLLMInteraction;

fn get_cognitive_system_prompt<P: prompt_provider::CognitivePromptProvider>(
    config: &Config,
) -> Vec<Instruction> {
    let mut instructions = Vec::new();

    instructions.push(Instruction::Text(format!(
        "Speak in a way that a {} can understand.",
        config.audience
    )));

    let prompt_config: P::Config =
        serde_json::from_value(config.prompt.clone()).unwrap_or_default();
    let prompt_content = P::get_system_prompt(&config.data_dir, &prompt_config);

    instructions.extend(prompt_content);

    // FIXME
    // config.prompt
    // #[cfg(feature = "assistant")]
    // instructions.push(Instruction::Text(format!(
    //     "Your name is {}.",
    //     config.assistant.full_name
    // )));

    // FIXME
    // #[cfg(feature = "rpg")]
    // if !config.players.is_empty() {
    //     instructions.push(Instruction::Text(format!(
    //         "These human players are in the game: {}",
    //         config.players.join(", ")
    //     )));
    // }

    if let capabilities = crate::get_dynamic_capabilities()
        && !capabilities.is_empty()
    {
        instructions.push(Instruction::Section(
            Box::new(Instruction::Text("Capabilities".to_string())),
            vec![
                Instruction::Text("Besides the obvious, you are also capable of:".to_string()),
                Instruction::Item(capabilities.join("\n")),
                Instruction::Text(
                    "Do not mention these capabilities unless the user directly asks about them."
                        .to_string(),
                ),
            ],
        ));
    }

    // FIXME
    // instructions.push(Instruction::Text(format!(
    //     "Speak in a way that a {} can understand.",
    //     config.audience
    // )));
    instructions
}
