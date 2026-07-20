use crate::plugin::Plugin;
use crate::sync::{broadcast, mpsc};
use async_trait::async_trait;

#[async_trait]
pub trait ChatPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        peer_input_text_tx: mpsc::Sender<crate::peer_input_text::PeerInputText>,
        cognitive_output_text_rx: mpsc::Receiver<crate::cognitive_output_text::CognitiveOutputText>,
        cognitive_state_rx: broadcast::Receiver<crate::cognitive::CognitiveStateUpdate>,
    ) -> Result<(), String>;
}
