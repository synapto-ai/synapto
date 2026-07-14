use crate::audio_recorder::AudioRecorderPlugin;
use crate::call::CallPlugin;
use crate::camera::CameraPlugin;
use crate::chat::ChatPlugin;
use crate::cognitive_output_audio::AudioOutputPlugin;
use crate::document::DocumentsPlugin;
use crate::gui::GuiPlugin;
use crate::interaction::{InteractionObserver, RetrospectiveConsolidationPlugin};
use crate::peer_input_audio::AudioInputPlugin;
use crate::rollout::RolloutController;
use crate::speech_to_text::{DiarizationPlugin, STTPlugin, TTSPlugin};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct PluginContext {
    llm_executor: std::sync::Arc<dyn crate::llm::LlmExecutor>,
    plugin_config: serde_json::Value,
    storage: std::sync::Arc<crate::storage::StorageRegistry>,
    plugin_namespace: String,
    storage_config_resolver: std::sync::Arc<dyn crate::storage::StorageConfigResolver>,
    current_context_rx: tokio::sync::watch::Receiver<serde_json::Value>,
}

impl PluginContext {
    #[doc = " Internal constructor used by the Core AI engine."]
    pub fn new(
        llm_executor: std::sync::Arc<dyn crate::llm::LlmExecutor>,
        plugin_config: serde_json::Value,
        storage: std::sync::Arc<crate::storage::StorageRegistry>,
        plugin_namespace: String,
        storage_config_resolver: std::sync::Arc<dyn crate::storage::StorageConfigResolver>,
        current_context_rx: tokio::sync::watch::Receiver<serde_json::Value>,
    ) -> Self {
        Self {
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
    #[doc = " Deserializes the raw JSON configuration into the plugin's requested config struct."]
    #[doc = ""]
    #[doc = " **Note on Serde Configuration Defaults:**"]
    #[doc = " This performs strict structural deserialization. If the JSON object provided by the"]
    #[doc = " `ConfigProvider` is missing a field that your Rust struct (which must derive `Deserialize`) expects, `serde` will"]
    #[doc = " return a `missing field` error — even if your struct implements `Default`."]
    #[doc = ""]
    #[doc = " To make a configuration field optional, use the `#[serde(default)]`"]
    #[doc = " attribute on the struct field. This instructs `serde` to fall back to `Default::default()`"]
    #[doc = " when the key is omitted."]
    pub fn config<C: serde::de::DeserializeOwned>(&self) -> Result<C, String> {
        serde_json::from_value(self.plugin_config.clone())
            .map_err(|e| format!("Failed to parse plugin config: {}", e))
    }
    #[doc = " Initializes and returns a database connection scoped strictly to this plugin's namespace."]
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
        S::connect(config, self.storage.clone(), &self.plugin_namespace).await
    }
    #[doc = " Read-only subscription channel to receive the global state updates"]
    pub fn subscribe_context_updates(&self) -> tokio::sync::watch::Receiver<serde_json::Value> {
        self.current_context_rx.clone()
    }
}

pub trait PluginRegistry {
    fn register_gui<P: GuiPlugin>(&mut self, plugin: std::sync::Arc<P>);
    fn register_audio_input<P: AudioInputPlugin>(&mut self, plugin: std::sync::Arc<P>);
    fn register_audio_output<P: AudioOutputPlugin>(&mut self, plugin: std::sync::Arc<P>);
    fn register_stt<P: STTPlugin>(&mut self, plugin: std::sync::Arc<P>);
    fn register_tts<P: TTSPlugin>(&mut self, plugin: std::sync::Arc<P>);
    fn register_diarization<P: DiarizationPlugin>(&mut self, plugin: std::sync::Arc<P>);
    fn register_chat<P: ChatPlugin>(&mut self, plugin: std::sync::Arc<P>);
    fn register_documents<P: DocumentsPlugin>(&mut self, plugin: std::sync::Arc<P>);
    fn register_interaction_observer<P: InteractionObserver>(&mut self, plugin: std::sync::Arc<P>);
    fn register_rollout_controller<P: RolloutController>(&mut self, plugin: std::sync::Arc<P>);
    fn register_retrospective_consolidation<P: RetrospectiveConsolidationPlugin>(
        &mut self,
        plugin: std::sync::Arc<P>,
    );
    fn register_camera<P: CameraPlugin>(&mut self, plugin: std::sync::Arc<P>);
    fn register_context_provider<P: crate::context::ContextProvider>(
        &mut self,
        provider: std::sync::Arc<P>,
    );
    fn register_command<C: crate::command::Command>(&mut self, command: C);
    fn register_tool<T: crate::tool::Tool>(&mut self, tool: T);
    fn register_call<P: CallPlugin>(
        &mut self,
        plugin: std::sync::Arc<P>,
        capability: Option<&'static str>,
    );
    fn register_recorder<P: AudioRecorderPlugin>(&mut self, plugin: std::sync::Arc<P>);
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmptyPluginConfig {}

#[async_trait]
pub trait Plugin: Send + Sync + 'static {
    #[doc = " Compile-time semantic description of this plugin's capability for the LLM."]
    const CAPABILITY: Option<&'static str> = None;
    #[doc = " This is the method for instantiating plugins, allowing them to await"]
    #[doc = " their database connections (via `context.store::<S>().await`) before returning."]
    #[doc = ""]
    #[doc = " **Note on Configuration:** When calling `context.config()?` to extract your configuration struct,"]
    #[doc = " ensure any optional fields in your struct are marked with `#[serde(default)]`. Otherwise,"]
    #[doc = " omitted fields in the config file will cause strict deserialization errors."]
    async fn create(context: crate::plugin::PluginContext) -> Result<Self, String>
    where
        Self: Sized;
    fn register<R: PluginRegistry + ?Sized>(self: std::sync::Arc<Self>, registry: &mut R)
    where
        Self: Sized;
}

#[doc = " An opaque channel identifier used to route messages within the system."]
#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct MessageChannel {
    #[doc = " Opaque JSON context provided by plugins or core modules."]
    pub context: serde_json::Value,
}
