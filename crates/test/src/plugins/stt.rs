use crate::ACTIVE_COORDINATOR;
use async_trait::async_trait;
use synapto_interface::plugin::Plugin;
use synapto_interface::speech_to_text::STTPlugin;
use synapto_interface::speech_to_text::{InputVoiceAudio, SpeechDetected, SpeechTranscript};
use synapto_interface::sync::mpsc;

pub struct MockSttPlugin;

#[async_trait::async_trait]
impl Plugin for MockSttPlugin {
    async fn create(_context: synapto_interface::plugin::PluginContext) -> Result<Self, String> {
        Ok(Self)
    }

    fn register<R: synapto_interface::plugin::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_stt(self);
    }
}

#[async_trait]
impl STTPlugin for MockSttPlugin {
    async fn start(
        &self,
        mut _audio_rx: mpsc::Receiver<InputVoiceAudio>,
        transcript_tx: mpsc::Sender<SpeechTranscript>,
        speech_detected: SpeechDetected,
    ) -> Result<(), String> {
        let coordinator = ACTIVE_COORDINATOR.lock().unwrap().clone().ok_or_else(|| {
            "ScenarioCoordinator is not initialized in ACTIVE_COORDINATOR Mutex".to_string()
        })?;

        coordinator.transcript_tx.set(transcript_tx).ok();
        coordinator.speech_detected.set(speech_detected).ok();
        Ok(())
    }
}
