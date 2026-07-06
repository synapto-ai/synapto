use crate::plugin::Plugin;
use crate::sync;
use async_trait::async_trait;

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
