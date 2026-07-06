use crate::plugin::Plugin;
use crate::speech_to_text::InputVoiceAudio;
use crate::sync::{broadcast, watch};
use async_trait::async_trait;

#[async_trait]
pub trait AudioRecorderPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        call_active_rx: watch::Receiver<bool>,
        input_voice_audio_rx: broadcast::Receiver<InputVoiceAudio>,
    ) -> Result<(), String>;
}
