use crate::document::DocumentId;
use crate::peer_input::MessageText;
use crate::plugin::MessageChannel;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Input message originating from a text-based source (e.g. chat, console).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
pub struct PeerInputText {
    /// The channel where the message was received.
    pub channel: MessageChannel,
    /// The ID of the user who sent the message.
    pub sender_id: SenderId,
    /// The text content of the message.
    pub text: MessageText,
    /// Any documents attached to the message.
    pub attached_documents: Vec<DocumentId>,
    /// Whether the assistant was explicitly mentioned or addressed.
    pub explicitly_addressed: bool,
}

#[doc = " A unique identifier for a message sender."]
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
pub struct SenderId(pub String);
