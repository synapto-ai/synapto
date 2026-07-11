mod not_clear;
mod recent;

use synapto_interface::{
    interaction::Timestamp,
    interaction::{NotClearInteraction, NotClearInteractionMemory},
    sync::{mpsc, watch},
};

use crate::config::Config;

pub(crate) use recent::{InFlightTool, Interaction, InteractionMemory, SpeakerName};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn start<
    S: synapto_interface::storage::StorageConnection + synapto_interface::storage::KeyValueStore,
>(
    config: Config,
    new_interaction_rx: mpsc::Receiver<recent::Interaction>,
    rollout_receivers: Vec<(String, watch::Receiver<Timestamp>)>,
    observers_tx: Vec<mpsc::Sender<synapto_interface::interaction::ObservedInteraction>>,
    interaction_memory_tx: watch::Sender<recent::InteractionMemory>,
    resolve_not_clear_rx: mpsc::Receiver<Timestamp>,
    not_clear_memory_tx: watch::Sender<NotClearInteractionMemory>,
    resolve_in_flight_tool_rx: mpsc::Receiver<synapto_interface::tool::ToolCallId>,
    storage: std::sync::Arc<S>,
) {
    let (not_clear_tx, not_clear_rx) = mpsc::channel::<NotClearInteraction>(100);

    tokio::spawn(not_clear::not_clear_interactions_task(
        config.clone(),
        not_clear_rx,
        resolve_not_clear_rx,
        not_clear_memory_tx,
        storage.clone(),
    ));

    tokio::spawn(recent::interaction_memory_task(
        config,
        new_interaction_rx,
        rollout_receivers,
        observers_tx,
        interaction_memory_tx,
        not_clear_tx,
        resolve_in_flight_tool_rx,
        storage,
    ));
}
