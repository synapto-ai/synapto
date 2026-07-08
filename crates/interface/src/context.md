## Federated Contexts with `ContextProvider`

### When to Use It

Use `ContextProvider` when your plugin or subsystem needs to feed custom state (e.g., live sensor readings, active room, list of connected users) or search results (e.g., vector database lookups) directly into the LLM prompt's context dynamically, without modifying the central cognitive orchestrator.

### The `TemporalScope` Dimension Pattern

Every context provider maps its data to one of three semantic dimensions via the `SCOPE` constant:

1. **`TemporalScope::Historical`**: Long-term archives, past interactions, memories.
2. **`TemporalScope::Current`**: The objective state of the **present moment** (e.g., current active room, live sensor readings, system statuses, current meeting, currently available documents).
3. **`TemporalScope::Prospective`**: Upcoming steps, objective lists, or plans (e.g., active task queues, upcoming narrative beats).

### Two Paradigms of Context Resolution

The framework evaluates context providers **concurrently** via `.gather_contexts(&request).await`. Because the system is multi-threaded, one slow provider doesn't block the gathering of others, but the overall prompt compilation won't finish until all providers resolve.

When building a `ContextProvider`, you should choose between two main architectural paradigms depending on your latency and filtering requirements:

#### 1. The "Latency-Friendly" Paradigm (Pre-computed State)

This approach is used for live, objective state (like active sensors, memory snapshots, or configuration). The plugin maintains state in a background task and exposes a `tokio::sync::watch::Receiver`. The `context()` method simply clones the current value, returning instantly.

- **Trade-off**: Extremely fast (zero cognitive latency), but it ignores the conversation history in `ContextRequest` since it just returns a fixed structural snapshot.

```rust,ignore
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Serialize;
use synapto_interface::llm::LLMSafe;
use synapto_interface::context::{ContextProvider, TemporalScope, ContextRequest};
use synapto_interface::sync::watch;

#[derive(Serialize, JsonSchema, Clone, Debug, LLMSafe)]
pub struct TemperatureContext {
    pub current_temp_celsius: f32,
}

pub struct TemperatureSensorProvider {
    reading_rx: watch::Receiver<TemperatureContext>,
}

#[async_trait]
impl ContextProvider for TemperatureSensorProvider {
    type Context = TemperatureContext;
    const NAME: &'static str = "room_environment";
    const SCOPE: TemporalScope = TemporalScope::Current;

    async fn context(&self, _request: &ContextRequest) -> Result<Self::Context, String> {
        // Instantaneous read. Never blocks the gather_contexts cycle.
        Ok(self.reading_rx.borrow().clone())
    }
}
```

#### 2. The "On-Demand" Paradigm (Dynamic / Filtered State)

This approach is used for dense or large-scale data (like documents, historical logs, or complex external APIs). Instead of returning everything, the `context()` method evaluates the `ContextRequest` (which contains recent interactions) to perform an on-the-fly lookup, database query, or filtering operation.

- **Trade-off**: Context is highly relevant and token-efficient, but computing or fetching it adds latency to the cognitive cycle.

```rust,ignore
pub struct DocumentSearchProvider {
    db: Arc<MyDatabaseConnection>,
}

#[async_trait]
impl ContextProvider for DocumentSearchProvider {
    type Context = Vec<DocumentSnippet>;
    const NAME: &'static str = "relevant_documents";
    const SCOPE: TemporalScope = TemporalScope::Historical;

    async fn context(&self, request: &ContextRequest) -> Result<Self::Context, String> {
        // Compile the recent conversational context into a search query string
        let query = request
            .recent_interactions
            .iter()
            .filter_map(|i| i.peer_input.as_deref())
            .collect::<Vec<_>>()
            .join(" ");

        // Perform an on-demand database search.
        // This blocks this specific provider's future during `gather_contexts`,
        // but runs concurrently alongside other providers.
        let docs = self.db.search(&query).await?;
        Ok(docs)
    }
}
```

### Inter-Plugin Wakeups via `subscribe()`

Orthogonal to how data is gathered, providers can optionally implement the `subscribe()` method to participate in the framework's "No-Tick" propagation protocol.

By returning a `tokio::sync::watch::Receiver<()>`, the provider signals the core system whenever its internal state mutates. The core engine listens to this decentralized signal and instantly gathers and broadcasts the updated context payload (`current_context_tx`).

This mechanism is used exclusively for **inter-plugin wakeups**. It ensures that any other plugins observing the live world state—such as a GUI renderer drawing the scene or a Scenarist plugin evaluating rules—instantly wake up and react to the fresh data without wasting CPU cycles on polling.

#### Note: This Does Not Wake the Cognitive Engine

It is important to note that mutating a context provider and firing its `subscribe()` channel **does not** trigger the AI to run an inference cycle or start speaking.

The Cognitive engine (which drives the AI's thoughts and speech) is strictly woken up by specific conversational or interaction triggers:

1. **Speech / Audio Input**: When the STT pipeline detects a completed user utterance.
2. **Text Chat**: When a user sends a text message.
3. **Video / Vision Changes**: If a camera plugin emits a new frame that needs immediate attention.
4. **Tool Resolutions**: When a long-running asynchronous tool completes and returns its result to the engine.

```rust,ignore
    // Optional implementation inside a ContextProvider
    fn subscribe(&self) -> Option<watch::Receiver<()>> {
        Some(self.mutation_rx.clone()) // Where mutation_rx is a watch::Receiver<()>
    }
```

To register a provider, call `.register(provider)` on your registries instance at startup:

```rust,ignore
registries.current.register(TemperatureSensorProvider::new(reading_rx, mutation_rx));
```