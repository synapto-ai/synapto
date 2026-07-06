use crate::plugin::Plugin;
use async_trait::async_trait;

#[async_trait]
pub trait GuiPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        registries: std::sync::Arc<crate::context::ContextRegistries>,
        error_rx: std::sync::mpsc::Receiver<String>,
    ) -> Result<(), String>;
}
