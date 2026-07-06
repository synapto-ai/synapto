use crate::plugin::MessageChannel;
use crate::speech_to_text::SpeakerId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[doc = " Represents the identity of a speaker."]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
pub enum Speaker {
    #[doc = " An unknown speaker, optionally with a temporary ID."]
    Unknown(Option<SpeakerId>),
    #[doc = " A recognized speaker with a stable ID."]
    Recognized(SpeakerId),
}

#[doc = " Input message originating from speech transcription."]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
pub struct PeerInputSpeech {
    #[doc = " The channel where the speech was captured."]
    pub channel: MessageChannel,
    #[doc = " The speaker who produced the speech."]
    pub speaker: Speaker,
    #[doc = " The transcribed text."]
    pub transcript: MessageText,
}

#[doc = " Unified input message type for the cognitive loop."]
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, JsonSchema)]
pub enum PeerInput {
    #[doc = " Input from a speech source."]
    Speech(PeerInputSpeech),
    #[doc = " Input from a text source (e.g. chat)."]
    Text(crate::peer_input_text::PeerInputText),
}

#[doc = " Represents the text content of a message."]
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    PartialEq,
    Eq,
    Debug,
    Clone,
    derive_more :: Display,
    derive_more :: From,
    derive_more :: Deref,
)]
pub struct MessageText(pub String);
