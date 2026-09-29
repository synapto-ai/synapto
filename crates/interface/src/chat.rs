use crate::plugin::Plugin;
use crate::sync::mpsc;
use async_trait::async_trait;

#[async_trait]
pub trait ChatPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        peer_input_text_tx: mpsc::Sender<crate::peer_input_text::PeerInputText>,
        cognitive_output_text_rx: mpsc::Receiver<crate::cognitive_output_text::CognitiveOutputText>,
    ) -> Result<(), String>;
}
