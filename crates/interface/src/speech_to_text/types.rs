use crate::peer_input_audio::types::{PEER_INPUT_AUDIO_CHUNK_DURATION, PeerInputAudio};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Calculates the discrete audio chunk indices for a given continuous time range.
///
/// This method converts real-world time (in seconds) into discrete audio chunk sequence numbers.
/// To prevent dropping short words and correctly handle boundary overlaps, it deliberately applies:
/// - `floor` to the start time: ensuring the entire chunk where the word begins is included.
/// - `ceil` to the end time: ensuring the chunk where the word ends is captured, forming a
///   strict mathematically exclusive boundary `[start_index, end_index)`.
pub fn calculate_chunk_indices(base_index: u64, start_secs: f64, end_secs: f64) -> (u64, u64) {
    let chunk_duration_secs = PEER_INPUT_AUDIO_CHUNK_DURATION.as_secs_f64();
    let start_index = base_index + (start_secs / chunk_duration_secs).floor() as u64;
    let end_index = base_index + (end_secs / chunk_duration_secs).ceil() as u64;
    (start_index, end_index)
}

/// Signal indicating that speech activity has been detected.
#[derive(Clone)]
pub struct SpeechDetected(Arc<tokio::sync::Notify>);

impl SpeechDetected {
    pub fn new(notify: Arc<tokio::sync::Notify>) -> Self {
        Self(notify)
    }
    pub fn notify(&self) {
        self.0.notify_waiters();
    }
}

/// A unique identifier for a speaker.
#[derive(
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Hash,
    Debug,
    Clone,
    JsonSchema,
    derive_more::Display,
    derive_more::From,
    derive_more::Deref,
)]
pub struct SpeakerId(pub String);

impl SpeakerId {
    pub fn new(speaker_id: String) -> Self {
        Self(speaker_id)
    }
}

/// Represents a single word within a transcription.
#[derive(Deserialize, Serialize, Default, Clone, Debug, JsonSchema)]
pub struct Word {
    /// The start index of the audio chunk where this word begins.
    pub start_index: Option<u64>,
    /// The end index of the audio chunk where this word ends.
    pub end_index: Option<u64>,
    /// The text of the word.
    pub word: String,
    /// Optional hint about the speaker identity for this specific word.
    pub speaker_hint: Option<String>,
}

/// A transcribed segment of speech.
#[derive(Deserialize, Serialize, Default, Clone, Debug, JsonSchema)]
pub struct SpeechTranscript {
    /// The sequence number of the starting audio chunk.
    pub start_index: u64,
    /// The sequence number of the ending audio chunk.
    pub end_index: u64,
    /// The complete transcribed text for this segment.
    pub transcript: String,
    /// Individual words with their respective timing and metadata.
    pub words: Vec<Word>,
}
crate::register_channel_name!(SpeechTranscript, "speech_transcript");

/// Represents an audio chunk with its associated voice activity status.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum InputVoiceAudio {
    /// The chunk contains active voice.
    Voice(PeerInputAudioIndexed),
    /// The chunk contains silence or non-voice background noise.
    NoVoice(PeerInputAudioIndexed),
}
crate::register_channel_name!(InputVoiceAudio, "input_voice_audio");

/// A raw audio chunk bundled with its sequence sequence index.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PeerInputAudioIndexed {
    /// The raw audio data.
    pub audio: PeerInputAudio,
    /// The monotonically increasing sequence number of this chunk.
    pub index: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CosineSimilarity(pub f32);

#[derive(Clone, Debug)]
pub struct WordOverlap {
    pub start_index: u64,
    pub end_index: u64,
    pub overlaps: std::collections::HashMap<InternalSpeaker, u64>,
    pub word: String,
}

pub type SpeakerHeuristicCallback = std::sync::Arc<
    dyn Fn(&[WordOverlap], &[SpeakerSegment]) -> Vec<Option<SpeakerId>> + Send + Sync,
>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum InternalSpeaker {
    Unknown(Option<(SpeakerId, CosineSimilarity)>),
    Recognized(SpeakerId),
}

impl PartialEq for InternalSpeaker {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (InternalSpeaker::Unknown(a), InternalSpeaker::Unknown(b)) => match (a, b) {
                (Some((id_a, score_a)), Some((id_b, score_b))) => {
                    id_a == id_b && score_a.0 == score_b.0
                }
                (None, None) => true,
                _ => false,
            },
            (InternalSpeaker::Recognized(a), InternalSpeaker::Recognized(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for InternalSpeaker {}

impl std::hash::Hash for InternalSpeaker {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            InternalSpeaker::Unknown(opt) => {
                0_u8.hash(state);
                if let Some((id, score)) = opt {
                    1_u8.hash(state);
                    id.hash(state);
                    score.0.to_bits().hash(state);
                } else {
                    0_u8.hash(state);
                }
            }
            InternalSpeaker::Recognized(id) => {
                1_u8.hash(state);
                id.hash(state);
            }
        }
    }
}

impl From<InternalSpeaker> for crate::types::Speaker {
    fn from(val: InternalSpeaker) -> Self {
        match val {
            InternalSpeaker::Unknown(None) => crate::types::Speaker::Unknown(None),
            InternalSpeaker::Unknown(Some((id, _))) => {
                crate::types::Speaker::Unknown(Some(SpeakerId(format!("Maybe {}", id).to_string())))
            }
            InternalSpeaker::Recognized(id) => {
                crate::types::Speaker::Recognized(SpeakerId(id.to_string()))
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpeakerSegment {
    pub speaker: InternalSpeaker,
    pub start_index: u64,
    pub end_index: u64,
}
crate::register_channel_name!(SpeakerSegment, "speaker_segment");

impl From<InputVoiceAudio> for PeerInputAudio {
    fn from(input_voice_audio: InputVoiceAudio) -> Self {
        match input_voice_audio {
            InputVoiceAudio::Voice(peer_input_audio) => peer_input_audio.audio,
            InputVoiceAudio::NoVoice(peer_input_audio) => peer_input_audio.audio,
        }
    }
}

impl From<InputVoiceAudio> for [i32; crate::peer_input_audio::types::PEER_INPUT_AUDIO_CHUNK_SIZE] {
    fn from(input_voice_audio: InputVoiceAudio) -> Self {
        let peer_input_audio: PeerInputAudio = input_voice_audio.into();
        peer_input_audio.into()
    }
}
