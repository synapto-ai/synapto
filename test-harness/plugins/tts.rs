use crate::ACTIVE_COORDINATOR;
use async_trait::async_trait;
use synapto_interface::cognitive::CognitiveOutputSpeech;
use synapto_interface::plugin::Plugin;
use synapto_interface::speech_to_text::TTSPlugin;
use synapto_interface::sync::{broadcast, mpsc};

pub struct MockTtsPlugin;

#[async_trait::async_trait]
impl Plugin for MockTtsPlugin {
    async fn create(_context: synapto_interface::plugin::PluginContext) -> Result<Self, String> {
        Ok(Self)
    }

    fn register<R: synapto_interface::plugin::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_tts(self);
    }
}

#[async_trait]
impl TTSPlugin for MockTtsPlugin {
    async fn start(
        &self,
        mut speech_rx: broadcast::Receiver<CognitiveOutputSpeech>,
        mut _audio_tx: mpsc::Sender<
            synapto_interface::cognitive_output_audio::CognitiveOutputAudio,
        >,
    ) -> Result<(), String> {
        let coordinator = ACTIVE_COORDINATOR.get().ok_or_else(|| {
            "ScenarioCoordinator is not initialized in ACTIVE_COORDINATOR OnceLock".to_string()
        })?;

        while let Ok(msg) = speech_rx.recv().await {
            coordinator.check_text_response(&msg.text).await;
        }
        Ok(())
    }
}
