pub mod cognitive_output_audio;
pub mod cognitive_output_text;
pub mod peer_input_audio;
pub mod peer_input_text;
pub mod speech_to_text;
pub mod storage;

/// Instrumented synchronization primitives and re-exports of `tokio::sync`.
pub mod sync;
/// Core data types used across the interface and core engine.
pub mod types;

pub mod llm;

use async_trait::async_trait;

use crate::cognitive_output_audio::CognitiveOutputAudio;
use crate::peer_input_audio::PeerInputAudio;
use crate::speech_to_text::{
    InputVoiceAudio, SpeakerSegment, SpeechDetected, SpeechTranscript,
};
use crate::sync::{broadcast, mpsc, watch};
use crate::types::CognitiveOutputSpeech;

#[async_trait]
pub trait AudioInputPlugin: Plugin + Send + Sync {
    async fn start(&self, tx: mpsc::Sender<PeerInputAudio>) -> Result<(), String>;
}

#[async_trait]
pub trait AudioOutputPlugin: Plugin + Send + Sync {
    async fn start(&self, rx: mpsc::Receiver<CognitiveOutputAudio>) -> Result<(), String>;
}

#[async_trait]
pub trait STTPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        audio_rx: mpsc::Receiver<InputVoiceAudio>,
        transcript_tx: mpsc::Sender<SpeechTranscript>,
        speech_detected: SpeechDetected,
    ) -> Result<(), String>;
}

#[async_trait]
pub trait TTSPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        speech_rx: broadcast::Receiver<CognitiveOutputSpeech>,
        audio_tx: mpsc::Sender<CognitiveOutputAudio>,
    ) -> Result<(), String>;
}

#[async_trait]
pub trait DiarizationPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        audio_rx: broadcast::Receiver<InputVoiceAudio>,
        segment_tx: mpsc::Sender<SpeakerSegment>,
    ) -> Result<(), String>;

    fn heuristic(&self) -> Option<crate::speech_to_text::SpeakerHeuristicCallback> {
        None
    }
}

#[async_trait]
pub trait ChatPlugin: Plugin + Send + Sync {
    fn channel_context_schema() -> schemars::Schema
    where
        Self: Sized,
    {
        schemars::schema_for!(())
    }

    async fn start(
        &self,
        peer_input_text_tx: mpsc::Sender<crate::peer_input_text::PeerInputText>,
        cognitive_output_text_rx: mpsc::Receiver<
            crate::cognitive_output_text::CognitiveOutputText,
        >,
        cognitive_state_rx: broadcast::Receiver<crate::types::CognitiveStateUpdate>,
        add_document_tx: Option<mpsc::Sender<crate::types::AddDocumentRequest>>,
    ) -> Result<(), String>;
}

#[async_trait]
pub trait GuiPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        registries: std::sync::Arc<crate::types::ContextRegistries>,
        error_rx: std::sync::mpsc::Receiver<String>,
    ) -> Result<(), String>;
}

#[async_trait]
pub trait DocumentsPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        add_document_rx: mpsc::Receiver<crate::types::AddDocumentRequest>,
    ) -> Result<(), String>;
}

#[async_trait]
pub trait RetrospectiveConsolidationPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        not_clear_memory_rx: watch::Receiver<crate::types::NotClearInteractionMemory>,
        resolve_not_clear_tx: mpsc::Sender<crate::types::Timestamp>,
    ) -> Result<(), String>;
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
    fn register_context_provider<P: crate::types::ContextProvider>(
        &mut self,
        provider: std::sync::Arc<P>,
    );
    fn register_command<C: crate::types::Command>(&mut self, command: C);
    fn register_tool<T: crate::types::Tool>(&mut self, tool: T);
    fn register_call<P: CallPlugin>(
        &mut self,
        plugin: std::sync::Arc<P>,
        capability: Option<&'static str>,
    );
    fn register_recorder<P: AudioRecorderPlugin>(&mut self, plugin: std::sync::Arc<P>);
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmptyPluginConfig {}

#[async_trait]
pub trait Plugin: Send + Sync + 'static {
    /// Compile-time semantic description of this plugin's capability for the LLM.
    const CAPABILITY: Option<&'static str> = None;

    /// This is the method for instantiating plugins, allowing them to await
    /// their database connections (via `context.store::<S>().await`) before returning.
    ///
    /// **Note on Configuration:** When calling `context.config()?` to extract your configuration struct,
    /// ensure any optional fields in your struct are marked with `#[serde(default)]`. Otherwise,
    /// omitted fields in the config file will cause strict deserialization errors.
    async fn create(context: crate::types::PluginContext) -> Result<Self, String>
    where
        Self: Sized;

    fn register<R: PluginRegistry + ?Sized>(self: std::sync::Arc<Self>, registry: &mut R)
    where
        Self: Sized;
}

#[async_trait]
pub trait CallPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        peer_input_text_rx: sync::broadcast::Receiver<crate::peer_input_text::PeerInputText>,
        cognitive_output_text_tx: sync::mpsc::Sender<
            crate::cognitive_output_text::CognitiveOutputText,
        >,
        last_voice_time_rx: sync::watch::Receiver<std::time::Instant>,
        ai_speaking_rx: sync::watch::Receiver<bool>,
        call_active_tx: sync::watch::Sender<bool>,
    ) -> Result<(), String>;
}

#[async_trait]
pub trait AudioRecorderPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        call_active_rx: watch::Receiver<bool>,
        input_voice_audio_rx: broadcast::Receiver<InputVoiceAudio>,
    ) -> Result<(), String>;
}

#[async_trait]
pub trait CameraPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        video_tx: crate::sync::watch::Sender<crate::types::CameraInputFrame>,
    ) -> Result<(), String>;
}

#[async_trait]
pub trait RolloutController: Plugin + Send + Sync {
    async fn start(&self, rollout_tx: watch::Sender<crate::types::Timestamp>)
    -> Result<(), String>;
}

#[async_trait]
pub trait InteractionObserver: Plugin + Send + Sync {
    async fn start(
        &self,
        interaction_rx: mpsc::Receiver<crate::types::ObservedInteraction>,
    ) -> Result<(), String>;
}
