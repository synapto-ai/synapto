use crate::llm::LLMSafe;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
#[schemars(
    description = "A resolved tool execution output from the active session. If the required information is present here, answer directly without re-invoking the tool."
)]
pub struct WorkingMemoryEntry {
    #[schemars(description = "Name of the tool that generated this output.")]
    pub tool_name: String,
    #[schemars(description = "Arguments that were passed to the tool invocation.")]
    pub arguments: serde_json::Value,
    #[schemars(description = "Execution result returned by the tool.")]
    pub output: serde_json::Value,
}

#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    LLMSafe,
    derive_more::Deref,
    derive_more::DerefMut,
    derive_more::IntoIterator,
)]
#[schemars(
    description = "Active working memory containing outputs of previously resolved tools. Consult this memory before calling any tool; if the required information is already available here, answer immediately using these facts instead of re-executing tools."
)]
pub struct ActiveWorkingMemory(pub Vec<WorkingMemoryEntry>);

impl ActiveWorkingMemory {
    pub fn new(entries: Vec<WorkingMemoryEntry>) -> Self {
        Self(entries)
    }
}
