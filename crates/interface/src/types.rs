pub mod documents;
use crate::peer_input_text::types::PeerInputText;
use crate::{cognitive_output_text::types::CognitiveOutputText, llm::LLMSafe};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
pub enum DocumentIngestionPolicy {
    /// Save the raw document only, do not attempt to parse or extract text.
    Store,
    /// Save the raw document and run active parsers (e.g. PDF parser) to extract UTF-8 text.
    StoreAndParse,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
pub struct DocumentRegistrationRequest {
    pub original_filename: String,
    pub mime_type: String,
    pub data: Vec<u8>,
    pub policy: DocumentIngestionPolicy,
}

pub struct AddDocumentRequest {
    pub request: DocumentRegistrationRequest,
    pub reply_tx: tokio::sync::oneshot::Sender<DocumentId>,
}
crate::register_channel_name!(AddDocumentRequest, "add_document_request");

#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    PartialEq,
    Eq,
    Debug,
    Clone,
    derive_more::Display,
    derive_more::From,
    derive_more::Deref,
)]
pub struct ToolCallId(pub String);
crate::register_channel_name!(ToolCallId, "tool_call_id");

/// A unique identifier for a message sender.
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    PartialEq,
    Eq,
    Debug,
    Clone,
    derive_more::Display,
    derive_more::From,
    derive_more::Deref,
)]
pub struct SenderId(pub String);

/// Represents the text content of a message.
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    PartialEq,
    Eq,
    Debug,
    Clone,
    derive_more::Display,
    derive_more::From,
    derive_more::Deref,
)]
pub struct MessageText(pub String);

/// A unique identifier for a document resource.
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    PartialEq,
    Eq,
    Debug,
    Clone,
    derive_more::Display,
    derive_more::From,
    derive_more::Deref,
)]
pub struct DocumentId(pub String);
crate::register_channel_name!(DocumentId, "document_id");

/// An opaque channel identifier used to route messages within the system.
#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct MessageChannel {
    /// Opaque JSON context provided by plugins or core modules.
    pub context: serde_json::Value,
}

pub use crate::speech_to_text::types::SpeakerId;

/// Represents the identity of a speaker.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
pub enum Speaker {
    /// An unknown speaker, optionally with a temporary ID.
    Unknown(Option<SpeakerId>),
    /// A recognized speaker with a stable ID.
    Recognized(SpeakerId),
}

/// Input message originating from speech transcription.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
pub struct PeerInputSpeech {
    /// The channel where the speech was captured.
    pub channel: MessageChannel,
    /// The speaker who produced the speech.
    pub speaker: Speaker,
    /// The transcribed text.
    pub transcript: MessageText,
}
crate::register_channel_name!(PeerInputSpeech, "peer_input_speech");

/// Unified input message type for the cognitive loop.
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, JsonSchema)]
pub enum PeerInput {
    /// Input from a speech source.
    Speech(PeerInputSpeech),
    /// Input from a text source (e.g. chat).
    Text(crate::peer_input_text::types::PeerInputText),
}

/// Output message intended to be spoken by the system.
#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct CognitiveOutputSpeech {
    /// The target channel for the speech output.
    pub target_channel: MessageChannel,
    /// The text to be spoken.
    pub text: String,
}
crate::register_channel_name!(CognitiveOutputSpeech, "ai_output_speech");

/// Current operational state of the cognitive loop.
#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub enum CognitiveState {
    /// The AI is actively thinking/processing.
    Thinking,
    /// The AI is performing document search or RAG.
    Searching,
    /// The AI is executing an external command.
    Acting,
    /// The AI is waiting for new input.
    Idle,
}

/// Update event for the system's cognitive state.
#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct CognitiveStateUpdate {
    /// The context of the state update (e.g. plugin-specific metadata).
    pub context: serde_json::Value,
    /// The new cognitive state.
    pub state: CognitiveState,
}
crate::register_channel_name!(CognitiveStateUpdate, "cognitive_state");

/// A generic envelope wrapping a payload with its source plugin identity.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Enveloped<T> {
    /// The name/ID of the plugin associated with this payload.
    pub plugin: String,
    /// The actual data being carried.
    pub payload: T,
}

impl<T> Enveloped<T> {
    /// Creates a new enveloped payload.
    pub fn new(plugin: impl Into<String>, payload: T) -> Self {
        Self {
            plugin: plugin.into(),
            payload,
        }
    }
}

crate::register_channel_name!(Enveloped<PeerInputText>, "peer_input_text_enveloped");
crate::register_channel_name!(
    Enveloped<CognitiveOutputText>,
    "cognitive_output_text_enveloped"
);
crate::register_channel_name!(Enveloped<CognitiveStateUpdate>, "cognitive_state_enveloped");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalScope {
    Historical,
    Current, // TODO document what is usesd for in the core
    Prospective,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ContextInteraction {
    pub peer_input: Option<String>,
    pub ai_reasoning: Option<String>,
    pub ai_output: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, Default)]
pub struct ContextRequest {
    /// The sliding window of recent conversational flow.
    /// Used by plugins to perform Associative RAG.
    /// An empty list implies a request for the unfiltered, baseline state.
    pub recent_interactions: Vec<ContextInteraction>,
    pub initial_run: bool,
}

#[async_trait::async_trait]
pub trait ContextProvider: Send + Sync + 'static {
    type Context: schemars::JsonSchema + serde::Serialize + LLMSafe + Send + Sync + 'static;

    /// Declarative compile-time semantic key (e.g., "state", "active_tasks")
    const NAME: &'static str;

    /// The dimension this context belongs to
    const SCOPE: TemporalScope;

    /// Provide the JSON-serializable context view, filtered associatively via ContextRequest
    async fn context(&self, request: &ContextRequest) -> Result<Self::Context, String>;

    /// Decentralized Wakeup Signal:
    /// Returns a receiver that signals when this specific context mutates.
    fn subscribe(&self) -> Option<tokio::sync::watch::Receiver<()>> {
        None
    }
}

#[async_trait::async_trait]
pub trait ErasedContextProvider: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn scope(&self) -> TemporalScope;
    fn schema(&self) -> schemars::Schema;
    async fn erased_context(&self, request: &ContextRequest) -> Result<serde_json::Value, String>;
    fn subscribe(&self) -> Option<tokio::sync::watch::Receiver<()>>;
}

#[async_trait::async_trait]
impl<T> ErasedContextProvider for T
where
    T: ContextProvider,
{
    fn name(&self) -> &'static str {
        <T as ContextProvider>::NAME
    }
    fn scope(&self) -> TemporalScope {
        <T as ContextProvider>::SCOPE
    }
    fn schema(&self) -> schemars::Schema {
        schemars::schema_for!(<T as ContextProvider>::Context)
    }
    async fn erased_context(&self, request: &ContextRequest) -> Result<serde_json::Value, String> {
        let view = <T as ContextProvider>::context(self, request).await?;
        serde_json::to_value(view).map_err(|e| e.to_string())
    }
    fn subscribe(&self) -> Option<tokio::sync::watch::Receiver<()>> {
        <T as ContextProvider>::subscribe(self)
    }
}

pub struct ContextRegistryBuilder {
    pub providers: std::sync::RwLock<Vec<std::sync::Arc<dyn ErasedContextProvider>>>,
    change_tx: tokio::sync::watch::Sender<()>,
    change_rx: tokio::sync::watch::Receiver<()>,
}

impl Default for ContextRegistryBuilder {
    fn default() -> Self {
        let (change_tx, change_rx) = tokio::sync::watch::channel(());
        Self {
            providers: std::sync::RwLock::new(Vec::new()),
            change_tx,
            change_rx,
        }
    }
}

impl ContextRegistryBuilder {
    pub fn register<T>(&self, provider: T)
    where
        T: ErasedContextProvider + 'static,
    {
        let provider_arc: std::sync::Arc<dyn ErasedContextProvider> = std::sync::Arc::new(provider);
        self.register_erased(provider_arc);
    }

    pub fn register_erased(&self, provider: std::sync::Arc<dyn ErasedContextProvider>) {
        self.providers
            .write()
            .unwrap_or_else(|e| panic!("Failed to acquire write lock on providers: {:?}", e))
            .push(provider.clone());

        // Forward individual watch changes to the unified registry-wide watch channel
        if let Some(mut sub_rx) = provider.subscribe() {
            let change_tx = self.change_tx.clone();
            tokio::spawn(async move {
                while sub_rx.changed().await.is_ok() {
                    change_tx
                        .send(())
                        .inspect_err(|e| tracing::error!("{}", e))
                        .ok();
                }
            });
        }
    }

    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<()> {
        self.change_rx.clone()
    }
}

#[derive(Clone)]
pub struct PluginContext {
    llm_executor: std::sync::Arc<dyn crate::llm::LlmExecutor>,
    plugin_config: serde_json::Value,

    // Private fields: Plugins cannot access these directly.
    // They are exclusively used to securely bootstrap storage connections.
    storage: std::sync::Arc<crate::storage::StorageRegistry>,
    plugin_namespace: String,
    data_dir: std::path::PathBuf,
    storage_config_resolver: std::sync::Arc<dyn crate::storage::StorageConfigResolver>,
    current_context_rx: tokio::sync::watch::Receiver<serde_json::Value>,
}

impl PluginContext {
    /// Internal constructor used by the Core AI engine.
    pub fn new(
        data_dir: std::path::PathBuf,
        llm_executor: std::sync::Arc<dyn crate::llm::LlmExecutor>,
        plugin_config: serde_json::Value,
        storage: std::sync::Arc<crate::storage::StorageRegistry>,
        plugin_namespace: String,
        storage_config_resolver: std::sync::Arc<dyn crate::storage::StorageConfigResolver>,
        current_context_rx: tokio::sync::watch::Receiver<serde_json::Value>,
    ) -> Self {
        Self {
            data_dir,
            llm_executor,
            plugin_config,
            storage,
            plugin_namespace,
            storage_config_resolver,
            current_context_rx,
        }
    }

    pub fn llm_executor(&self) -> std::sync::Arc<dyn crate::llm::LlmExecutor> {
        self.llm_executor.clone()
    }

    /// Deserializes the raw JSON configuration into the plugin's requested config struct.
    ///
    /// **Note on Serde Configuration Defaults:**
    /// This performs strict structural deserialization. If the JSON object provided by the
    /// `ConfigProvider` is missing a field that your Rust struct (which must derive `Deserialize`) expects, `serde` will
    /// return a `missing field` error — even if your struct implements `Default`.
    ///
    /// To make a configuration field optional, use the `#[serde(default)]`
    /// attribute on the struct field. This instructs `serde` to fall back to `Default::default()`
    /// when the key is omitted.
    pub fn config<C: serde::de::DeserializeOwned>(&self) -> Result<C, String> {
        serde_json::from_value(self.plugin_config.clone())
            .map_err(|e| format!("Failed to parse plugin config: {}", e))
    }

    /// Initializes and returns a database connection scoped strictly to this plugin's namespace.
    pub async fn store<S: crate::storage::StorageConnection>(&self) -> Result<S, String> {
        let full_path = std::any::type_name::<S>();
        let crate_name = full_path
            .split("::")
            .next()
            .unwrap_or("")
            .to_string()
            .replace('-', "_");
        let base_path = full_path.split('<').next().unwrap_or(full_path);
        let storage_type_name = base_path.split("::").last().unwrap_or("").to_string();

        let config_val = self
            .storage_config_resolver
            .resolve_config(&crate_name, &storage_type_name)
            .unwrap_or_else(|| serde_json::json!({}));

        let config: S::Config = serde_json::from_value(config_val).map_err(|e| {
            format!(
                "Failed to parse config for storage '{}::{}': {}",
                crate_name, storage_type_name, e
            )
        })?;

        S::connect(
            config,
            self.storage.clone(),
            &self.data_dir,
            &self.plugin_namespace,
        )
        .await
    }

    /// Read-only subscription channel to receive the global state updates
    pub fn subscribe_context_updates(&self) -> tokio::sync::watch::Receiver<serde_json::Value> {
        self.current_context_rx.clone()
    }
}

#[derive(Default)]
pub struct ContextRegistries {
    pub historical: ContextRegistryBuilder,
    pub current: ContextRegistryBuilder,
    pub prospective: ContextRegistryBuilder,
}

impl ContextRegistries {
    pub fn subscribe(&self, scope: TemporalScope) -> tokio::sync::watch::Receiver<()> {
        match scope {
            TemporalScope::Historical => self.historical.subscribe(),
            TemporalScope::Current => self.current.subscribe(),
            TemporalScope::Prospective => self.prospective.subscribe(),
        }
    }
}

#[async_trait::async_trait]
pub trait Command: Send + Sync + 'static {
    type Arguments: schemars::JsonSchema
        + serde::de::DeserializeOwned
        + LLMSafe
        + Send
        + Sync
        + 'static;

    const NAME: &'static str;

    async fn execute(&self, args: Self::Arguments) -> Result<(), String>;
}

#[async_trait::async_trait]
pub trait ErasedCommand: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn schema(&self) -> schemars::Schema;
    async fn erased_execute(&self, args: serde_json::Value) -> Result<(), String>;
}

#[async_trait::async_trait]
impl<T> ErasedCommand for T
where
    T: Command,
{
    fn name(&self) -> &'static str {
        <T as Command>::NAME
    }
    fn schema(&self) -> schemars::Schema {
        schemars::schema_for!(<T as Command>::Arguments)
    }
    async fn erased_execute(&self, args: serde_json::Value) -> Result<(), String> {
        let parsed_args = serde_json::from_value(args).map_err(|e| e.to_string())?;
        <T as Command>::execute(self, parsed_args).await
    }
}

#[derive(Default)]
pub struct CommandRegistryBuilder {
    pub commands:
        std::sync::RwLock<std::collections::HashMap<String, std::sync::Arc<dyn ErasedCommand>>>,
}

impl CommandRegistryBuilder {
    pub fn register<T>(&self, command: T)
    where
        T: ErasedCommand + 'static,
    {
        let command_arc: std::sync::Arc<dyn ErasedCommand> = std::sync::Arc::new(command);
        self.register_erased(command_arc);
    }

    pub fn register_erased(&self, command: std::sync::Arc<dyn ErasedCommand>) {
        self.commands
            .write()
            .unwrap_or_else(|e| panic!("Failed to acquire write lock on commands: {:?}", e))
            .insert(command.name().to_string(), command);
    }
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync + 'static {
    type Arguments: schemars::JsonSchema
        + serde::de::DeserializeOwned
        + LLMSafe
        + Send
        + Sync
        + 'static;

    const NAME: &'static str;
    const DESCRIPTION: &'static str;

    /// Evaluated dynamically every turn AFTER the ContextProviders have compiled the World State.
    /// `compiled_context` is the JSON value generated by all ContextProviders that the LLM is about to see.
    async fn is_available(
        &self,
        _ctx_request: &ContextRequest,
        _compiled_context: &serde_json::Value,
    ) -> Result<bool, String> {
        Ok(true)
    }

    /// Executes the tool. The result is serialized and fed back to the LLM.
    async fn execute(
        &self,
        ctx_request: &ContextRequest,
        args: Self::Arguments,
    ) -> Result<serde_json::Value, String>;
}

#[async_trait::async_trait]
pub trait ErasedTool: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn schema(&self) -> schemars::Schema;
    async fn erased_is_available(
        &self,
        ctx_request: &ContextRequest,
        compiled_context: &serde_json::Value,
    ) -> Result<bool, String>;
    async fn erased_execute(
        &self,
        ctx_request: &ContextRequest,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String>;
}

#[async_trait::async_trait]
impl<T> ErasedTool for T
where
    T: Tool,
{
    fn name(&self) -> &'static str {
        <T as Tool>::NAME
    }
    fn description(&self) -> &'static str {
        <T as Tool>::DESCRIPTION
    }
    fn schema(&self) -> schemars::Schema {
        schemars::schema_for!(<T as Tool>::Arguments)
    }
    async fn erased_is_available(
        &self,
        ctx_request: &ContextRequest,
        compiled_context: &serde_json::Value,
    ) -> Result<bool, String> {
        <T as Tool>::is_available(self, ctx_request, compiled_context).await
    }
    async fn erased_execute(
        &self,
        ctx_request: &ContextRequest,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let parsed_args = serde_json::from_value(args).map_err(|e| e.to_string())?;
        <T as Tool>::execute(self, ctx_request, parsed_args).await
    }
}

#[derive(Default)]
pub struct ToolRegistryBuilder {
    pub tools: std::sync::RwLock<std::collections::HashMap<String, std::sync::Arc<dyn ErasedTool>>>,
}

impl ToolRegistryBuilder {
    pub fn register<T>(&self, tool: T)
    where
        T: ErasedTool + 'static,
    {
        let tool_arc: std::sync::Arc<dyn ErasedTool> = std::sync::Arc::new(tool);
        self.register_erased(tool_arc);
    }

    pub fn register_erased(&self, tool: std::sync::Arc<dyn ErasedTool>) {
        self.tools
            .write()
            .unwrap_or_else(|e| panic!("Failed to acquire write lock on tools: {:?}", e))
            .insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<std::sync::Arc<dyn ErasedTool>> {
        self.tools
            .read()
            .unwrap_or_else(|e| panic!("Failed to acquire read lock on tools: {:?}", e))
            .get(name)
            .cloned()
    }

    pub fn get_all(&self) -> Vec<std::sync::Arc<dyn ErasedTool>> {
        self.tools
            .read()
            .unwrap_or_else(|e| panic!("Failed to acquire read lock on tools: {:?}", e))
            .values()
            .cloned()
            .collect()
    }
}

#[derive(
    Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone, PartialOrd, Ord, Copy,
)]
pub struct Timestamp(pub i64);
crate::register_channel_name!(Timestamp, "timestamp");

#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    PartialEq,
    Eq,
    Debug,
    Clone,
    derive_more::Display,
    derive_more::From,
    derive_more::Deref,
)]
pub struct SpaceId(pub String);

#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    PartialEq,
    Eq,
    Debug,
    Clone,
    derive_more::Display,
    derive_more::From,
    derive_more::Deref,
)]
pub struct ThreadId(pub String);

#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    PartialEq,
    Eq,
    Debug,
    Clone,
    derive_more::Display,
    derive_more::From,
    derive_more::Deref,
    Default,
)]
pub struct MessageId(pub String);

#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct AiSpoken(pub String);

#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct AiWritten {
    pub target_channel: MessageChannel,
    pub text: String,
}

#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    PartialEq,
    Eq,
    Debug,
    Clone,
    derive_more::Display,
    derive_more::From,
    derive_more::Deref,
)]
pub struct CognitiveReasoning(pub String);

#[derive(
    Clone, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct ObservedInteraction {
    pub timestamp: Timestamp,
    pub user_messages: Vec<PeerInput>,
    pub ai_spoken: Option<AiSpoken>,
    pub ai_written: Option<AiWritten>,
    pub ai_reasoning: Option<CognitiveReasoning>,
}

crate::register_channel_name!(ObservedInteraction, "observed_interaction");

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, schemars::JsonSchema)]
pub struct NotClearInteraction {
    pub timestamp: Timestamp,
    pub user_messages: Vec<PeerInput>,
    pub ai_spoken: Option<AiSpoken>,
    pub ai_written: Option<AiWritten>,
}
crate::register_channel_name!(NotClearInteraction, "not_clear_interaction");

#[derive(
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Debug,
    Clone,
    Default,
    schemars::JsonSchema,
    derive_more::Deref,
    derive_more::DerefMut,
    derive_more::IntoIterator,
)]
pub struct NotClearInteractionMemory(pub std::collections::VecDeque<NotClearInteraction>);

impl From<Vec<NotClearInteraction>> for NotClearInteractionMemory {
    fn from(value: Vec<NotClearInteraction>) -> Self {
        Self(value.into())
    }
}

/// Represents a single visual frame captured from a camera peripheral.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CameraInputFrame {
    /// JPEG encoded binary frame data.
    pub data: Vec<u8>,
}

crate::register_channel_name!(CameraInputFrame, "camera_input_frame");
