use synapto_interface::sync::{mpsc, watch};
use tracing::instrument;

use super::Timestamp;

use crate::{
    cognitive::CognitiveLLMInteraction,
    config::Config,
    interactions::types::{NotClearInteraction, NotClearInteractionMemory},
};

impl From<&NotClearInteraction> for super::SummaryLLMInteraction {
    fn from(interaction: &NotClearInteraction) -> Self {
        Self {
            timestamp: interaction.timestamp,
            interaction: CognitiveLLMInteraction {
                user_messages: interaction
                    .user_messages
                    .clone()
                    .into_iter()
                    .map(Into::into)
                    .collect(),
                ai_spoken: interaction.ai_spoken.clone(),
                ai_reasoning: None,
                in_flight_tools: vec![],
            },
        }
    }
}

#[instrument(skip_all, fields(subsystem))]
pub async fn not_clear_interactions_task(
    config: Config,
    mut not_clear_rx: mpsc::Receiver<NotClearInteraction>,
    mut resolve_not_clear_rx: mpsc::Receiver<Timestamp>,
    not_clear_memory_tx: watch::Sender<NotClearInteractionMemory>,
) {
    let memory_dir = config.data_dir.join("memory");
    tokio::fs::create_dir_all(&memory_dir).await.ok();
    let memory_file = memory_dir.join("not_clear_interactions.json");

    let mut not_clear_memory: NotClearInteractionMemory =
        if let Ok(content) = tokio::fs::read_to_string(&memory_file).await {
            serde_json::from_str::<NotClearInteractionMemory>(&content)
                .unwrap_or_else(|e| panic!("Failed to deserialize not_clear_memory: {}", e))
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
                    changed = true;
                }
            }
            _ => {
                tracing::error!("not_clear channels closed");
                return;
            }
        });

        if changed {
            if let Err(e) = not_clear_memory_tx.send(not_clear_memory.clone()) {
                tracing::error!("Failed to send not_clear_memory: {}", e);
            }
            if let Err(e) = tokio::fs::write(
                &memory_file,
                serde_json::to_string_pretty(&not_clear_memory)
                    .unwrap_or_else(|e| panic!("Failed to serialize not_clear_memory: {}", e)),
            )
            .await
            {
                tracing::error!("Failed to write not_clear_interactions memory: {:?}", e);
            }
        }
    }
}
