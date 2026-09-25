use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use synapto_interface::context::{ContextProvider, ContextRequest, TemporalScope};
use synapto_interface::llm::LLMSafe;
use synapto_interface::sync::watch;
use tokio::sync::RwLock;

#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, LLMSafe)]
pub(crate) enum WorkingMemoryState {
    /// The raw, uncompressed output directly from the tool execution.
    Original,
    /// A distilled summary of the tool output produced by background consolidation.
    Condensed,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
#[schemars(
    description = "A resolved tool execution output from the active session. If the required information is present here, answer directly without re-invoking the tool."
)]
pub(crate) struct WorkingMemoryEntry {
    #[schemars(description = "Name of the tool that generated this output.")]
    pub tool_name: String,
    #[schemars(description = "Arguments that were passed to the tool invocation.")]
    pub arguments: serde_json::Value,
    #[schemars(description = "Execution result returned by the tool.")]
    pub output: serde_json::Value,
    #[schemars(
        description = "Whether this is the verbatim raw tool output (Original) or a condensed summary (Condensed)."
    )]
    pub state: WorkingMemoryState,
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
pub(crate) struct ActiveWorkingMemory(pub Vec<WorkingMemoryEntry>);

impl ActiveWorkingMemory {
    pub(crate) fn new(entries: Vec<WorkingMemoryEntry>) -> Self {
        Self(entries)
    }
}

#[derive(Clone)]
pub(crate) struct WorkingMemoryStore {
    memory: Arc<RwLock<Vec<WorkingMemoryEntry>>>,
    change_tx: watch::Sender<()>,
    change_rx: watch::Receiver<()>,
}

impl Default for WorkingMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "rerun")]
fn log_working_memory_to_rerun(mem: &[WorkingMemoryEntry]) {
    if let Ok(json) = serde_json::to_string_pretty(mem) {
        synapto_telemetry::log_to_rerun(
            "memory/working_memory",
            &synapto_telemetry::archetypes::TextDocument::new(json),
        );
    }
}

#[cfg(not(feature = "rerun"))]
fn log_working_memory_to_rerun(_mem: &[WorkingMemoryEntry]) {}

impl WorkingMemoryStore {
    pub(crate) fn new() -> Self {
        let (change_tx, change_rx) = watch::channel(());
        log_working_memory_to_rerun(&[]);
        Self {
            memory: Arc::new(RwLock::new(Vec::new())),
            change_tx,
            change_rx,
        }
    }

    pub(crate) async fn append(&self, entry: WorkingMemoryEntry) {
        let mut mem = self.memory.write().await;
        mem.push(entry);
        log_working_memory_to_rerun(&mem);
        self.change_tx.send_replace(());
    }

    pub(crate) async fn replace(&self, new_memory: Vec<WorkingMemoryEntry>) {
        let mut mem = self.memory.write().await;
        *mem = new_memory;
        log_working_memory_to_rerun(&mem);
        self.change_tx.send_replace(());
    }

    pub(crate) async fn get(&self) -> ActiveWorkingMemory {
        let mem = self.memory.read().await;
        ActiveWorkingMemory::new(mem.clone())
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<()> {
        self.change_rx.clone()
    }
}

#[derive(Clone)]
pub(crate) struct WorkingMemoryProvider {
    store: WorkingMemoryStore,
}

impl WorkingMemoryProvider {
    pub(crate) fn new(store: WorkingMemoryStore) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl ContextProvider for WorkingMemoryProvider {
    type Context = ActiveWorkingMemory;
    const NAME: &'static str = "working_memory";
    const SCOPE: TemporalScope = TemporalScope::Current;

    async fn context(&self, request: &ContextRequest) -> Result<Self::Context, String> {
        let mem = self.store.memory.read().await;
        let visible_len = mem.len().saturating_sub(request.resolved_tools_count);
        Ok(ActiveWorkingMemory::new(mem[..visible_len].to_vec()))
    }

    fn subscribe(&self) -> Option<watch::Receiver<()>> {
        Some(self.store.subscribe())
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_working_memory_tail_slicing() {
        let store = WorkingMemoryStore::new();
        let provider = WorkingMemoryProvider::new(store.clone());

        // 1. Initial state: empty
        let req_empty = ContextRequest::default();
        let ctx = provider.context(&req_empty).await.unwrap();
        assert_eq!(ctx.len(), 0);

        // 2. Append tool A (simulating arrival in Turn 1)
        store
            .append(WorkingMemoryEntry {
                tool_name: "tool_a".to_string(),
                arguments: json!({"arg": 1}),
                output: json!({"res": "A"}),
                state: WorkingMemoryState::Original,
            })
            .await;

        // Turn 1 compilation: 1 tool just resolved (k = 1) -> should be excluded from prompt
        let req_turn1 = ContextRequest {
            resolved_tools_count: 1,
            ..Default::default()
        };
        let ctx_turn1 = provider.context(&req_turn1).await.unwrap();
        assert_eq!(ctx_turn1.len(), 0);

        // Turn 2 follow-up: k = 0 -> tool A is now visible in working_memory
        let req_turn2 = ContextRequest {
            resolved_tools_count: 0,
            ..Default::default()
        };
        let ctx_turn2 = provider.context(&req_turn2).await.unwrap();
        assert_eq!(ctx_turn2.len(), 1);
        assert_eq!(ctx_turn2[0].tool_name, "tool_a");
        assert_eq!(ctx_turn2[0].state, WorkingMemoryState::Original);

        // 3. Append tool B (simulating staggered resolution in Turn 2)
        store
            .append(WorkingMemoryEntry {
                tool_name: "tool_b".to_string(),
                arguments: json!({"arg": 2}),
                output: json!({"res": "B"}),
                state: WorkingMemoryState::Original,
            })
            .await;

        // Turn 2 resolution: k = 1 (Tool B on wire) -> prompt should see Tool A only
        let req_turn2_res = ContextRequest {
            resolved_tools_count: 1,
            ..Default::default()
        };
        let ctx_turn2_res = provider.context(&req_turn2_res).await.unwrap();
        assert_eq!(ctx_turn2_res.len(), 1);
        assert_eq!(ctx_turn2_res[0].tool_name, "tool_a");

        // Turn 3: k = 0 -> prompt sees BOTH Tool A and Tool B
        let req_turn3 = ContextRequest {
            resolved_tools_count: 0,
            ..Default::default()
        };
        let ctx_turn3 = provider.context(&req_turn3).await.unwrap();
        assert_eq!(ctx_turn3.len(), 2);
        assert_eq!(ctx_turn3[0].tool_name, "tool_a");
        assert_eq!(ctx_turn3[1].tool_name, "tool_b");

        // 4. Test distillation replacement
        store
            .replace(vec![WorkingMemoryEntry {
                tool_name: "tool_distilled".to_string(),
                arguments: json!({}),
                output: json!({"summary": "A+B done"}),
                state: WorkingMemoryState::Condensed,
            }])
            .await;

        let ctx_distilled = provider.context(&req_turn3).await.unwrap();
        assert_eq!(ctx_distilled.len(), 1);
        assert_eq!(ctx_distilled[0].tool_name, "tool_distilled");
        assert_eq!(ctx_distilled[0].state, WorkingMemoryState::Condensed);
    }
}
