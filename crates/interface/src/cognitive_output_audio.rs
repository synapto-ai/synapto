use derive_more::Deref;
use serde::{Deserialize, Serialize};

/// Represents a chunk of raw audio output produced by the system.
#[derive(Deref, Clone, Debug, Serialize, Deserialize)]
pub struct CognitiveOutputAudio(pub Vec<u8>);
