use synapto_interface::{
    interaction::{NotClearInteraction, NotClearInteractionMemory},
    sync::{mpsc, watch},
};
use tracing::instrument;

use super::{Interaction, Timestamp};

impl From<&Interaction> for NotClearInteraction {
    fn from(interaction: &Interaction) -> Self {
        Self {
            timestamp: interaction.timestamp,
            user_messages: interaction.user_messages.clone(),
            ai_spoken: interaction.ai_spoken.clone(),
            ai_written: interaction.ai_written.clone(),
        }
    }
}

// impl From<&NotClearInteraction> for super::SummaryLLMInteraction {
//     fn from(interaction: &NotClearInteraction) -> Self {
//         Self {
//             timestamp: interaction.timestamp,
//             interaction: CognitiveLLMInteraction {
//                 user_messages: interaction
//                     .user_messages
//                     .clone()
//                     .into_iter()
//                     .map(Into::into)
//                     .collect(),
//                 ai_spoken: interaction.ai_spoken.clone(),
//                 ai_reasoning: None,
//                 in_flight_tools: vec![],
//             },
//         }
//     }
// }

#[instrument(skip_all, fields(subsystem))]
pub(super) async fn not_clear_interactions_task<
    S: synapto_interface::storage::KeyValueStore + synapto_interface::storage::RecordStore,
>(
    mut not_clear_rx: mpsc::Receiver<NotClearInteraction>,
    mut resolve_not_clear_rx: mpsc::Receiver<Timestamp>,
    not_clear_memory_tx: watch::Sender<NotClearInteractionMemory>,
    storage: std::sync::Arc<S>,
) {
    let mut not_clear_memory: NotClearInteractionMemory = if let Ok(records) = storage
        .get_ordered_records::<NotClearInteraction>("not_clear_interactions", None, false)
        .await
    {
        NotClearInteractionMemory(records.into_iter().map(|(_, v)| v).collect())
    } else {
        NotClearInteractionMemory::default()
    };

    not_clear_memory_tx.send_replace(not_clear_memory.clone());

    loop {
        let mut changed = false;

        better_tokio_select::tokio_select!(match .. {
            .. if let Some(interaction) = not_clear_rx.recv() => {
                if !not_clear_memory
                    .iter()
                    .any(|i| i.timestamp == interaction.timestamp)
                {
                    let key = format!("{:020}", interaction.timestamp.0);
                    if let Err(e) = storage
                        .upsert_record("not_clear_interactions", &key, interaction.clone())
                        .await
                    {
                        tracing::error!("Failed to save not_clear_interaction: {}", e);
                    }
                    not_clear_memory.push_back(interaction);
                    changed = true;
                }
            }
            .. if let Some(resolved_timestamp) = resolve_not_clear_rx.recv() => {
                if let Some(pos) = not_clear_memory
                    .iter()
                    .position(|i| i.timestamp == resolved_timestamp)
                {
                    not_clear_memory.remove(pos);
                    let key = format!("{:020}", resolved_timestamp.0);
                    if let Err(e) = storage.delete_record("not_clear_interactions", &key).await {
                        tracing::error!("Failed to delete not_clear_interaction: {}", e);
                    }
                    changed = true;
                }
            }
            _ => {
                tracing::error!("not_clear channels closed");
                return;
            }
        });

        if changed && let Err(e) = not_clear_memory_tx.send(not_clear_memory.clone()) {
            tracing::error!("Failed to send not_clear_memory: {}", e);
        }
    }
}
