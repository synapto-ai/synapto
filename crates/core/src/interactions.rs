pub mod not_clear;
pub mod recent;
pub mod types;

use synapto_interface::sync::{mpsc, watch};

use crate::config::Config;

pub use recent::{InteractionMemory, SummaryLLMInteraction};
pub use types::{Interaction, Timestamp};

pub use types::{NotClearInteraction, NotClearInteractionMemory};

#[allow(clippy::too_many_arguments)]
pub async fn start(
    config: Config,
    new_interaction_rx: mpsc::Receiver<types::Interaction>,
    rollout_receivers: Vec<(String, watch::Receiver<types::Timestamp>)>,
    observers_tx: Vec<mpsc::Sender<synapto_interface::types::ObservedInteraction>>,
    interaction_memory_tx: watch::Sender<recent::InteractionMemory>,
    resolve_not_clear_rx: mpsc::Receiver<Timestamp>,
    not_clear_memory_tx: watch::Sender<NotClearInteractionMemory>,
    resolve_in_flight_tool_rx: mpsc::Receiver<synapto_interface::types::ToolCallId>,
) {
    let (not_clear_tx, not_clear_rx) = mpsc::channel::<types::NotClearInteraction>(100);

    tokio::spawn(not_clear::not_clear_interactions_task(
        config.clone(),
        not_clear_rx,
        resolve_not_clear_rx,
        not_clear_memory_tx,
    ));

    tokio::spawn(recent::interaction_memory_task(
        config.clone(),
        new_interaction_rx,
        rollout_receivers,
        observers_tx,
        interaction_memory_tx,
        not_clear_tx,
        resolve_in_flight_tool_rx,
    ));
}
