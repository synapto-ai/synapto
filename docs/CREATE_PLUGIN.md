# Creating a Custom Plugin

Plugins are the sensory organs (inputs) and actuators (outputs) of the AI system. By design, the core system knows nothing about how to connect to Slack, a VoIP client, or a local microphone.

Instead, the core defines strongly-typed channels and specialized plugin traits. Plugins implement these traits and translate platform-specific protocols into the system's core types.

---

## The Specialized Plugin Architecture

Rather than a single monolithic plugin trait with optional/nullable fields, the system uses a **Specialized Plugin Architecture**.

1. **`Plugin` (The Base Lifecycle Trait)**:
   All plugins implement the core `Plugin` trait. It handles initialization (`create` with `PluginContext`), metadata, and registration via the `register` hook.
2. **Specialized Role Traits**:
   Depending on what capabilities your plugin provides, it implements one or more specialized execution traits defined in `ai-interface`:
   - `ChatPlugin`: For text-based dialogue interfaces (e.g., Slack, Google Chat).
   - `AudioInputPlugin`: For raw audio sources (e.g., local microphone captures).
   - `AudioOutputPlugin`: For raw audio sinks (e.g., local speakers).
   - `STTPlugin`: Speech-to-Text translation engine interfaces.
   - `TTSPlugin`: Text-to-Speech synthesis engine interfaces.
   - `DiarizationPlugin`: Speaker identification and segmenting.
   - `DocumentsPlugin`: Managing document ingestion, vector retrieval, and reading URLs.
   - `CallPlugin`: Handling VOIP call loops, active voice indicators, and speaking detection.
   - `AudioRecorderPlugin`: Managing audio record flows on active calls.
   - `InteractionObserver`: Observing finalized conversation history asynchronously in the background (lossless queue).

---

## Step-by-Step Guide: Creating a Chat Plugin

Here is how to create a custom plugin crate that handles text-based dialogue.

### 1. Create a New Library Crate

Generate a new library crate inside the `plugins/` directory:

```bash
cargo new plugins/my-chat-plugin --lib
```

Update its `Cargo.toml` to depend on `ai-interface`, `async-trait`, and other required workspace dependencies:

```toml
[package]
name = "my-chat-plugin"
version = "0.1.0"
edition = "2024"

[dependencies]
synapto-interface = "0.1.0"
async-trait.workspace = true
schemars.workspace = true
serde.workspace = true
tokio.workspace = true
tracing.workspace = true
```

### 2. Define Configuration & Implement the `Plugin` Trait

In `src/lib.rs`, define your plugin structure, its configuration schema, and the base `Plugin` implementation. The `register` method is where your plugin registers itself to the appropriate slot in the `PluginRegistry`.

```rust
use std::path::PathBuf;
use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use ai_interface::{Plugin, PluginRegistry, ChatPlugin};
use ai_interface::sync::{mpsc, broadcast};
use ai_interface::peer_input_text::types::PeerInputText;
use ai_interface::cognitive_output_text::types::CognitiveOutputText;
use ai_interface::types::CognitiveStateUpdate;

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct MyChatConfig {
    pub api_token: String,
    #[serde(default = "default_room")]
    pub default_room: String,
    #[serde(default)]
    pub auto_join: bool, // Default is false because bool::default() is false
}

fn default_room() -> String {
    "general".to_string()
}

pub struct MyChatPlugin {
    config: MyChatConfig,
    data_dir: std::path::PathBuf,
}

#[async_trait]
impl Plugin for MyChatPlugin {
    // Optional: Compile-time semantic description for LLM tools/capabilities
    const CAPABILITY: Option<&'static str> = Some("my-chat-service");

    async fn create(context: ai_interface::types::PluginContext) -> Result<Self, String> {
        let config: MyChatConfig = context.config()?;
        if config.api_token.is_empty() {
            return Err("api_token must not be empty".to_string());
        }

        // PluginContext provides secure namespaces, storage connectors, etc.
        let data_dir = context.data_dir().to_path_buf();

        Ok(Self { config, data_dir })
    }

    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R)
    where
        Self: Sized,
    {
        // Instructs the registry that this plugin implements the Chat capability
        registry.register_chat(self);
    }
}
```

> **Note on Serde Configuration Defaults:**
> The `PluginContext::config()?` method performs strict JSON structural deserialization of the configuration at the boundary. If your configuration struct (which must derive `Deserialize`) expects a field that is omitted in the config file, `serde` will return a `missing field` error — even if your struct implements `Default`.
>
> To make a configuration parameter optional, use the `#[serde(default)]` attribute on the field. This instructs `serde` to fall back to the type's `Default::default()` (or a custom function) if the user omits the key from their configuration file.

### 3. Implement the Specialized `ChatPlugin` Trait

Implement the specialized `ChatPlugin` trait to receive direct channels from the cognitive core. This method is called asynchronously within a spawned `tokio` task.

```rust
#[async_trait]
impl ChatPlugin for MyChatPlugin {
    // Defines a JSON Schema that explains to the LLM what routing metadata
    // it needs to include when directing text back to this plugin (e.g. channel_id)
    fn channel_context_schema() -> schemars::Schema
    where
        Self: Sized,
    {
        // For simple plugins without contextual metadata, use schemars::schema_for!(())
        // Otherwise, define a DTO representing room or channel information
        schemars::schema_for!(())
    }

    async fn start(
        &self,
        peer_input_text_tx: mpsc::Sender<PeerInputText>,
        mut cognitive_output_text_rx: mpsc::Receiver<CognitiveOutputText>,
        _cognitive_state_rx: broadcast::Receiver<CognitiveStateUpdate>,
        _add_document_tx: Option<mpsc::Sender<ai_interface::types::AddDocumentRequest>>,
    ) -> Result<(), String> {
        let token = self.config.api_token.clone();
        let room = self.config.default_room.clone();

        // 1. Spawn a background task to handle outgoing AI text (AI -> external chat platform)
        tokio::spawn(async move {
            while let Some(ai_msg) = cognitive_output_text_rx.recv().await {
                tracing::info!("Sending AI response to chat room: {}", ai_msg.text);
                // In practice, invoke your external chat platform API here:
                // client.send_message(&room, &ai_msg.text, &token).await;
            }
        });

        // 2. Spawn a background task or listen to events to forward incoming user chat (external chat platform -> AI)
        tokio::spawn(async move {
            loop {
                // Periodically fetch, receive webhooks, or pull from socket
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

                let user_message = PeerInputText {
                    text: "Hello, system!".to_string(),
                    ..Default::default()
                };

                // Send the message into the core
                if let Err(e) = peer_input_text_tx.send(user_message).await {
                    tracing::error!("Failed to forward peer text: {:?}", e);
                    break;
                }
            }
        });

        Ok(())
    }
}
```

---

## Multi-Trait Plugins

A major advantage of this architecture is that a single plugin struct can implement **multiple specialized traits** to share underlying sockets, API clients, state, or device handles.

For example, a VoIP or Hardware audio-input-output integration might implement both `AudioInputPlugin` and `AudioOutputPlugin`:

```rust
pub struct MyVoipPlugin {
    client: Arc<VoipClient>,
}

impl Plugin for MyVoipPlugin {
    async fn create(context: ai_interface::types::PluginContext) -> Result<Self, String> {
        let config: VoipConfig = context.config()?;
        let client = Arc::new(VoipClient::new(config));
        Ok(Self { client })
    }
    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R)
    where
        Self: Sized,
    {
        registry.register_audio_input(self.clone());
        registry.register_audio_output(self);
    }
}

#[async_trait]
impl AudioInputPlugin for MyVoipPlugin {
    async fn start(&self, tx: mpsc::Sender<PeerInputAudio>) -> Result<(), String> {
        // Feed VoIP network audio stream into the AI core...
        Ok(())
    }
}

#[async_trait]
impl AudioOutputPlugin for MyVoipPlugin {
    async fn start(&self, mut rx: mpsc::Receiver<CognitiveOutputAudio>) -> Result<(), String> {
        // Output synthesized voice audio from AI core back to VoIP network...
        Ok(())
    }
}
```

---

## Federated Contexts with `ContextProvider`

### When to Use It

Use `ContextProvider` when your plugin or subsystem needs to feed custom state (e.g., live sensor readings, active room, list of connected users) or search results (e.g., vector database lookups) directly into the LLM prompt's context dynamically, without modifying the central cognitive orchestrator.

### The `TemporalScope` Dimension Pattern

Every context provider maps its data to one of three semantic dimensions via the `SCOPE` constant:

1. **`TemporalScope::Historical`**: Long-term archives, past interactions, or documents (e.g., vector-store RAG queries).
2. **`TemporalScope::Current`**: The objective state of the present moment (e.g., current active room, live sensor readings, system statuses).
3. **`TemporalScope::Prospective`**: Upcoming steps, objective lists, or plans (e.g., active task queues, upcoming narrative beats).

### Associative RAG via `ContextRequest`

During prompt compilation, the core invokes your provider with a `ContextRequest` containing a sliding window of recent conversation dialogue (`recent_interactions`). This allows historical or database-backed providers to perform **Associative RAG** dynamically, fetching only the records relevant to the current conversation topic.

### How to Use It (Example)

Define your serializable DTO, implement `LLMSafe`, and implement `ContextProvider`:

```rust
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Serialize;
use ai_interface::llm::LLMSafe;
use ai_interface::types::{ContextProvider, TemporalScope, ContextRequest};
use ai_interface::sync::watch;

#[derive(Serialize, JsonSchema, Clone, Debug, LLMSafe)]
pub struct TemperatureContext {
    pub current_temp_celsius: f32,
    pub humidity_percent: f32,
}

pub struct TemperatureSensorProvider {
    reading_rx: watch::Receiver<TemperatureContext>,
    mutation_rx: watch::Receiver<()>,
}

#[async_trait]
impl ContextProvider for TemperatureSensorProvider {
    type Context = TemperatureContext;

    // The key name under which this context JSON will appear in the prompt
    const NAME: &'static str = "room_environment";

    // Assigns this context to the live, present-moment dimension
    const SCOPE: TemporalScope = TemporalScope::Current;

    async fn context(&self, _request: &ContextRequest) -> Result<Self::Context, String> {
        Ok(self.reading_rx.borrow().clone())
    }

    // Decentralized Wakeup: Returning a watch receiver participates in the "No-Tick" protocol.
    // When the sensor value mutates and fires this channel, the cognitive loop wakes up instantly.
    fn subscribe(&self) -> Option<watch::Receiver<()>> {
        Some(self.mutation_rx.clone())
    }
}
```

To register a provider, call `.register(provider)` on your registries instance at startup:

```rust
registries.current.register(TemperatureSensorProvider::new(reading_rx, mutation_rx));
```

---

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

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use ai_interface::{Plugin, PluginRegistry, sync::watch, sync::mpsc};
use ai_interface::types::{Interaction, Timestamp, InteractionMemory};

pub struct MyObserverPlugin {
    // A private sender to populate your internal state
    memory_tx: std::sync::Mutex<Option<watch::Sender<MyCustomState>>>,
}

impl Plugin for MyObserverPlugin {
    async fn create(context: ai_interface::types::PluginContext) -> Result<Self, String> {
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
impl ai_interface::InteractionObserver for MyObserverPlugin {
    async fn start(
        &self,
        mut interaction_rx: mpsc::Receiver<ai_interface::types::ObservedInteraction>,
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

---

## Persistent Storage with `CollectionStore`

### When to Use It

Use `CollectionStore` when your plugin needs to persist lists of data across reboots (e.g., historical conversation summaries, behavioral insights, or cached configurations).

### Storage Providers

The AI architecture defines storage capabilities as generic traits in `ai_interface::storage`. The bundle initializing your plugin will inject a concrete storage provider at compile time.

There are currently two available providers:

1. **`storage-local` (`LocalStorageProvider`)**: A zero-dependency, human-readable file backend that writes JSON arrays directly to the plugin's namespace directory. Ideal for small, localized deployments or testing.
2. **`storage-surrealdb` (`SurrealStorage`)**: A full database backend for heavy, high-frequency logging or complex querying.

### How to Use It (Example)

Define your plugin with a generic type `S` bound to `CollectionStore + StorageConnection`, and inject it via the bundle's `main.rs`.

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use ai_interface::storage::{CollectionStore, StorageConnection};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Insight {
    pub content: String,
}

pub struct MyMemoryPlugin<S: CollectionStore + StorageConnection> {
    store: Arc<S>,
}

impl<S: CollectionStore + StorageConnection> MyMemoryPlugin<S> {
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }

    pub async fn add_insight(&self, insight: String) {
        let doc = Insight { content: insight };
        if let Err(e) = self.store.push("insights", doc).await {
            tracing::error!("Failed to save insight: {}", e);
        }
    }
}
```

In the bundle `main.rs`:

```rust
// Using the lightweight JSON file provider
use storage_local::LocalStorageProvider;

let storage = Arc::new(
    LocalStorageProvider::connect(registries.clone(), &data_dir, "my_plugin_namespace")
        .await
        .unwrap()
);

let my_plugin = MyMemoryPlugin::new(storage);
```

---

## Dynamic Actions with `Command`

### When to Use It

Use `Command` when your custom plugin needs to expose executable tools or actions (e.g., controlling a device, triggering a notification, updating state) that the LLM can dynamically choose to invoke inside its structured outputs.

### How to Use It (Example)

Define your deserializable argument DTO, implement `LLMSafe`, and implement `Command`:

```rust
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use ai_interface::types::{Command};
use ai_interface::llm::LLMSafe;

#[derive(Deserialize, JsonSchema, Clone, Debug, LLMSafe)]
pub struct AdjustThermostatArgs {
    pub target_temp_celsius: f32,
}

pub struct AdjustThermostatCommand {
    hardware_client: Arc<MyThermostatClient>,
}

#[async_trait]
impl Command for AdjustThermostatCommand {
    type Arguments = AdjustThermostatArgs;

    // The unique action identifier exposed to the LLM
    const NAME: &'static str = "adjust_thermostat";

    async fn execute(&self, args: Self::Arguments) -> Result<(), String> {
        tracing::info!("Adjusting room temperature to: {}°C", args.target_temp_celsius);
        self.hardware_client
            .set_temperature(args.target_temp_celsius)
            .await
            .map_err(|e| e.to_string())
    }
}
```

To expose this tool to the LLM, register it inside your command registry builder at startup:

```rust
command_registry.register(AdjustThermostatCommand::new(hardware_client));
```

---

## Dynamic Tools with `Tool`

The `Tool` interface is functionally similar to `Command`, but serves an entirely different purpose: it natively leverages the LLM's Function Calling mechanics to resolve external data _during_ the reasoning phase, rather than mutating the environment _after_ the reasoning phase.

### When to Use It

- You need the LLM to query an external database.
- You need to pull data from a URL, a specific document, or an external API into the LLM's context window.
- The LLM needs the result of the tool's execution to form its final response.

### How to Use It (Example)

Tools are defined via a schema struct using `schemars` and explicitly evaluated per-turn for availability.

#### Best Practice: State-Locked Availability

Tool availability (`is_available`) is dynamically evaluated on every turn using the fully compiled prompt context JSON. This eliminates race conditions.

**1. Same-Plugin State (Direct Schema Coupling)**
If your tool relies on context produced by a `ContextProvider` within the _same_ plugin, it is perfectly safe and encouraged to check that JSON structure directly.

```rust
    async fn is_available(
        &self,
        _ctx: &ContextRequest,
        compiled_context: &serde_json::Value
    ) -> Result<bool, String> {
        // Safe: Checking our own plugin's context
        let has_docs = compiled_context
            .get("available_documents")
            .and_then(|arr| arr.as_array())
            .is_some_and(|arr| !arr.is_empty());
        Ok(has_docs)
    }
```

**2. Cross-Plugin State (Fulltext Scan Anti-Coupling)**
Never tightly couple a tool's `is_available` check to the internal JSON schema of _another_ plugin. If your tool (e.g., `ReadUrlTool`) needs to activate when a URL is present—regardless of whether it was injected by the Chat plugin or the Memory plugin—serialize the global context to a string and perform a fast pattern scan.

```rust
    async fn is_available(
        &self,
        _ctx: &ContextRequest,
        compiled_context: &serde_json::Value
    ) -> Result<bool, String> {
        // Safe: Universal activation without cross-plugin schema coupling
        let full_context_str = serde_json::to_string(compiled_context).unwrap_or_default();
        let has_url = full_context_str.contains("http://") || full_context_str.contains("https://");
        Ok(has_url)
    }
```

To expose this tool, register it within your `Plugin` trait implementation:

```rust
impl Plugin for MyPlugin {
    fn register<R: ai_interface::PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R) {
        registry.register_tool(ReadDocumentPluginTool { ... });
    }
}
```

---

## Adding Your Plugin to a Composition Bundle

To use your custom plugin, add it as a dependency to your bundle's `Cargo.toml`:

```toml
# bundles/my-custom-assistant/Cargo.toml
[dependencies]
ai-core = { path = "../../core" }
my-chat-plugin = { path = "../../plugins/my-chat-plugin" }
```

And register it in your bundle's `main.rs` using `.register_plugin::<T>()`:

```rust
// bundles/my-custom-assistant/src/main.rs
use ai_core::AI;
use my_chat_plugin::MyChatPlugin;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    // Initialize the core with the appropriate configuration provider
    Synapto::<
        datadir_local::DataLocalDir<"my-assistant">,
        config_providers::file::FileConfigProvider,
        prompt_file::FilePromptProvider
    >::run::<(MyChatPlugin,)>()
    .await
}
```

---

## Architectural Guidelines

1. **Own Your I/O**: The core engine must remain completely agnostic of plugin-specific control protocols, network connection details, or authentication secrets.
2. **Specialized, Type-Safe Start Signatures**: The parameters of `start` methods are bare, non-optional `mpsc` and `broadcast` channels. This enforces type-safe direct coupling at the compiler level and guarantees you are provided with exactly the channels required.
3. **Never Block Ingestion Loops**: All heavy network calls, external API fetches, and synchronous procedures must run inside spawned `tokio::spawn` tasks detached from the main loops.
4. **Panic Isolation & Lifecycle**: The core is designed to terminate the process if any critical task panics, ensuring failures are immediate and clearly visible in logs.
5. **No Intermediate Translators**: Plugins connect directly with the core switchboard using the same channels (opposite sender/receiver ends) with no intermediate forwarding or translation overhead.

---

## Telemetry & Instrumentation

To simplify debugging, use the `ai_interface::sync` module (`mpsc`, `broadcast`, `watch`) instead of raw `tokio::sync` imports.

In **Debug** mode, any message sent over these instrumented channels automatically records telemetry that can be visualized in the Rerun window. In **Release** mode, these resolve directly to standard `tokio::sync` types with **zero performance overhead**.

---

## Testing Your Plugin

We mandate that all custom plugins include automated integration tests to verify correctness and prevent payload format regressions.

For step-by-step guidance, standard crate layouts, and templates for writing local plugin integration tests using `test_config.json`, see the [Creating an Integration Test](TESTING.md#42-creating-an-integration-test) section of the Testing Guidelines.
