## Asynchronous History Tracking with `InteractionObserver`

### When to Use It

Use `InteractionObserver` when your plugin needs to monitor conversation history in the background to build long-term memory, distill insights, log dialogue sentiment, or update external tracking systems.

### The Finalized, Lossless Queue Pattern

Unlike the main cognitive loop which uses a highly filtered state, background observers receive a **lossless, backpressured, private `mpsc` queue** containing finalized interactions.
An interaction is considered "finalized" by the core when it falls out of the active sliding window (becoming older than the last N active interactions). This ensures observers only process stable, permanent records, and avoids processing intermediate/unfinished states.

> [!IMPORTANT]
> **Implementation Rule:** The actual size of the active sliding window (e.g. 8 interactions) is completely arbitrary, controlled by the core, and must remain completely opaque to plugins. Observers must never assume a specific window size or wait for a threshold; they must simply process all incoming finalized interactions from `interaction_rx` as they are received.

### The Batched Draining Queue Pattern

Because LLM distillation or external API requests are slow and expensive, observers should never process every interaction sequentially. Instead, use the **Batched Draining Pattern**: wake up on `recv()`, drain any other immediately available interactions using non-blocking `try_recv()`, package them into a batch, and execute a single combined process or LLM inference.

### How to Use It (Example)

Implement the specialized `InteractionObserver` trait, and register it inside your base `Plugin::register` hook:

```rust,ignore
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use synapto_interface::plugin::{Plugin, PluginRegistry, sync::watch, sync::mpsc};
use synapto_interface::core::{Interaction, Timestamp, InteractionMemory};

pub struct MyObserverPlugin {
    // A private sender to populate your internal state
    memory_tx: std::sync::Mutex<Option<watch::Sender<MyCustomState>>>,
}

impl Plugin for MyObserverPlugin {
    async fn create(context: synapto_interface::plugin::PluginContext) -> Result<Self, String> {
        // let config: MyObserverConfig = context.config()?; // Load config if needed
        let (memory_tx, memory_rx) = watch::channel(MyCustomState::default());
        Ok(Self {
            memory_tx: std::sync::Mutex::new(Some(memory_tx)),
        })
    }

    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R)
    where
        Self: Sized,
    {
        // 1. Registers as an InteractionObserver to receive the background queue
        registry.register_interaction_observer(self.clone());
    }
}

#[async_trait]
impl synapto_interface::interaction::InteractionObserver for MyObserverPlugin {
    async fn start(
        &self,
        mut interaction_rx: mpsc::Receiver<synapto_interface::interaction::ObservedInteraction>,
    ) -> Result<(), String> {
        let memory_tx = self.memory_tx.lock().unwrap().take().unwrap();

        tokio::spawn(async move {
            // Wake up when the first new interaction arrives
            while let Some(first) = interaction_rx.recv().await {

                // IMPORTANT BEST PRACTICE: Dynamic Batching
                // Once awake, immediately try to drain any additional interactions
                // that arrived concurrently into a single batch.
                // This is highly token-efficient and produces better summaries
                // during rapid multi-turn conversations or when catching up
                // from a long-running background task.
                let mut batch = vec![first];
                while let Ok(next) = interaction_rx.try_recv() {
                    batch.push(next);
                }

                // Process the batch (e.g. call an LLM to distill insights or log to database)
                // Doing this once per batch rather than per interaction saves API calls.
                tracing::info!("Processing a batch of {} interactions asynchronously...", batch.len());

                // Report progress back to the core to advance the sliding window
                let last_processed_timestamp = batch.last().unwrap().timestamp;
                let _ = rollout_tx.send(last_processed_timestamp);
            }
        });

        Ok(())
    }
}
```