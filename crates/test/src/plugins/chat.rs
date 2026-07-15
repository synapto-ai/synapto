use crate::ACTIVE_COORDINATOR;
use async_trait::async_trait;
use synapto_interface::chat::ChatPlugin;
use synapto_interface::cognitive::CognitiveStateUpdate;
use synapto_interface::document::{AddDocumentRequest, DocumentProviderPlugin};
use synapto_interface::plugin::Plugin;
use synapto_interface::sync::{broadcast, mpsc};

pub struct MockChatPlugin;

#[async_trait::async_trait]
impl Plugin for MockChatPlugin {
    async fn create(
        _context: &synapto_interface::plugin::PluginInitContext<'_>,
    ) -> Result<Self, String> {
        Ok(Self)
    }

    fn register<R: synapto_interface::plugin::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_chat(self.clone());
        registry.register_document_provider(self);
    }
}

#[async_trait]
impl ChatPlugin for MockChatPlugin {
    async fn start(
        &self,
        peer_input_text_tx: mpsc::Sender<synapto_interface::peer_input_text::PeerInputText>,
        mut cognitive_output_text_rx: mpsc::Receiver<
            synapto_interface::cognitive_output_text::CognitiveOutputText,
        >,
        _cognitive_state_rx: broadcast::Receiver<CognitiveStateUpdate>,
    ) -> Result<(), String> {
        let coordinator = ACTIVE_COORDINATOR.lock().unwrap().clone().ok_or_else(|| {
            "ScenarioCoordinator is not initialized in ACTIVE_COORDINATOR Mutex".to_string()
        })?;

        coordinator.peer_input_text_tx.set(peer_input_text_tx).ok();

        while let Some(msg) = cognitive_output_text_rx.recv().await {
            coordinator.check_text_response(&msg.text).await;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl DocumentProviderPlugin for MockChatPlugin {
    async fn start_document_provider(
        &self,
        add_document_tx: mpsc::Sender<AddDocumentRequest>,
    ) -> Result<(), String> {
        let coordinator = ACTIVE_COORDINATOR.lock().unwrap().clone().ok_or_else(|| {
            "ScenarioCoordinator is not initialized in ACTIVE_COORDINATOR Mutex".to_string()
        })?;

        coordinator.add_document_tx.set(add_document_tx).ok();
        Ok(())
    }
}
