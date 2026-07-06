use crate::types::{DocumentId, MessageChannel, MessageText, SenderId};
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
