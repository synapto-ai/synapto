use crate::plugin::Plugin;
use crate::sync::{broadcast, mpsc};
use async_trait::async_trait;

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
        cognitive_output_text_rx: mpsc::Receiver<crate::cognitive_output_text::CognitiveOutputText>,
        cognitive_state_rx: broadcast::Receiver<crate::cognitive::CognitiveStateUpdate>,
        add_document_tx: Option<mpsc::Sender<crate::document::AddDocumentRequest>>,
    ) -> Result<(), String>;
}
