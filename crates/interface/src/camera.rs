use crate::plugin::Plugin;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[doc = " Represents a single visual frame captured from a camera peripheral."]
#[derive(Debug, Clone, Serialize, Deserialize, schemars :: JsonSchema)]
pub struct CameraInputFrame {
    #[doc = " JPEG encoded binary frame data."]
    pub data: Vec<u8>,
}

#[async_trait]
pub trait CameraPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        video_tx: crate::sync::watch::Sender<crate::camera::CameraInputFrame>,
    ) -> Result<(), String>;
}
