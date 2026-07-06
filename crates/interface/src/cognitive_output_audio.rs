use crate::plugin::Plugin;
use derive_more::Deref;
use serde::{Deserialize, Serialize};

/// Represents a chunk of raw audio output produced by the system.
#[derive(Deref, Clone, Debug, Serialize, Deserialize)]
pub struct CognitiveOutputAudio(pub Vec<u8>);

use crate::sync::mpsc;
use async_trait::async_trait;
#[async_trait]
pub trait AudioOutputPlugin: Plugin + Send + Sync {
    async fn start(&self, rx: mpsc::Receiver<CognitiveOutputAudio>) -> Result<(), String>;
}
