use crate::types::MessageChannel;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Output message intended to be written to a text source (e.g. chat).
#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Debug, Clone)]
pub struct CognitiveOutputText {
    /// The target channel for the text output.
    pub target_channel: MessageChannel,
    /// The text content to be written.
    pub text: String,
}
crate::register_channel_name!(CognitiveOutputText, "cognitive_output_text");
