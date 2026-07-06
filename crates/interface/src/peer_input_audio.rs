use crate::plugin::Plugin;
use derive_more::{Deref, DerefMut, IntoIterator};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const PEER_INPUT_AUDIO_SAMPLE_RATE: usize = 16_000;

pub const PEER_INPUT_AUDIO_CHUNK_SIZE: usize = 1280_usize;

pub const PEER_INPUT_AUDIO_CHUNK_DURATION: Duration = Duration::from_millis(
    PEER_INPUT_AUDIO_CHUNK_SIZE as u64 * 1000 / PEER_INPUT_AUDIO_SAMPLE_RATE as u64,
);

/// Represents a chunk of raw audio input from a peer.
#[derive(Deref, DerefMut, IntoIterator, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PeerInputAudio(Vec<i16>);

impl PeerInputAudio {
    pub fn new(data: [i16; PEER_INPUT_AUDIO_CHUNK_SIZE]) -> Self {
        Self(data.to_vec())
    }
}

impl From<PeerInputAudio> for Vec<u16> {
    fn from(value: PeerInputAudio) -> Self {
        value.0.iter().map(|&s| (s as u16) ^ 0x8000).collect()
    }
}

impl From<PeerInputAudio> for Vec<u8> {
    fn from(value: PeerInputAudio) -> Self {
        value.0.iter().flat_map(|&s| s.to_ne_bytes()).collect()
    }
}

impl From<PeerInputAudio> for [i32; PEER_INPUT_AUDIO_CHUNK_SIZE] {
    fn from(peer_input_audio: PeerInputAudio) -> Self {
        let mut result = [0i32; PEER_INPUT_AUDIO_CHUNK_SIZE];
        for (i, &sample) in peer_input_audio.0.iter().enumerate() {
            if i < PEER_INPUT_AUDIO_CHUNK_SIZE {
                result[i] = sample as i32;
            }
        }
        result
    }
}

// impl From<PeerInputAudio> for &[i16] {
//     fn from(peer_input_audio: PeerInputAudio) -> Self {
//         peer_input_audio
//     }
// }

use crate::sync::mpsc;
use async_trait::async_trait;
#[async_trait]
pub trait AudioInputPlugin: Plugin + Send + Sync {
    async fn start(&self, tx: mpsc::Sender<PeerInputAudio>) -> Result<(), String>;
}
