use async_trait::async_trait;
use synapto_interface::plugin::Plugin;
use synapto_interface::speech_to_text::DiarizationPlugin;
use synapto_interface::speech_to_text::{InputVoiceAudio, SpeakerSegment};
use synapto_interface::sync::{broadcast, mpsc};

pub struct MockDiarizationPlugin;

#[async_trait::async_trait]
impl Plugin for MockDiarizationPlugin {
    async fn create(_context: &synapto_interface::plugin::PluginInitContext<'_>) -> Result<Self, String> {
        Ok(Self)
    }

    fn register<R: synapto_interface::plugin::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_diarization(self);
    }
}

#[async_trait]
impl DiarizationPlugin for MockDiarizationPlugin {
    async fn start(
        &self,
        mut audio_rx: broadcast::Receiver<InputVoiceAudio>,
        _segment_tx: mpsc::Sender<SpeakerSegment>,
    ) -> Result<(), String> {
        // Keep the task alive and the channel open by waiting on the audio stream
        while audio_rx.recv().await.is_ok() {}
        Ok(())
    }
}
