use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use synapto_interface::llm::LLMSafe;

// -------------------------------------------------------------
// 1. Opaque Resource Identifiers (ORI Pattern - ARCHITECTURE.md #17 & #15)
// -------------------------------------------------------------

#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    derive_more::Display,
    derive_more::From,
    derive_more::Deref,
)]
pub struct MissionId(pub String);

#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    derive_more::Display,
    derive_more::From,
    derive_more::Deref,
)]
pub struct GoalId(pub String);

#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    derive_more::Display,
    derive_more::From,
    derive_more::Deref,
)]
pub struct TaskId(pub String);

// -------------------------------------------------------------
// 2. Domain Statuses & Enums
// -------------------------------------------------------------

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub enum MissionStatus {
    Active,
    Achieved,
    Abandoned,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub enum GoalStatus {
    Active,
    Achieved,
    Abandoned,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,   // Waiting for activation condition
    Active,    // Condition met, injected into Cognitive Loop for execution
    Deferred,  // Blocked or manually pushed back
    Completed, // Execution finished, awaiting Goal re-evaluation
    Cancelled, // No longer relevant
    Failed,    // Execution blocked/impossible
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub enum TriggerType {
    Temporal,
    StateMatch,
    Event,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct TriggerCondition {
    pub condition_type: TriggerType,
    pub description: String,
}

// -------------------------------------------------------------
// 3. Internal State Models - Marked Non-LLM-Safe
// -------------------------------------------------------------

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub id: TaskId,
    pub goal_id: Option<GoalId>,
    pub title: String,
    pub steps: Vec<String>,
    pub trigger: Option<TriggerCondition>,
    pub status: TaskStatus,
    pub priority: u8,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct Goal {
    pub id: GoalId,
    pub mission_id: Option<MissionId>,
    pub title: String,
    pub description: String,
    pub status: GoalStatus,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct Mission {
    pub id: MissionId,
    pub title: String,
    pub description: String,
    pub status: MissionStatus,
}

// Memory Aliases for persistence and channels
pub type TaskMemory = std::collections::HashMap<TaskId, Task>;
pub type GoalMemory = std::collections::HashMap<GoalId, Goal>;
pub type MissionMemory = std::collections::HashMap<MissionId, Mission>;

// -------------------------------------------------------------
// 4. Public LLM DTOs (Safe Prompt Representations)
// -------------------------------------------------------------

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct LLMVisibleTask {
    pub id: TaskId,
    pub goal_id: Option<GoalId>,
    pub title: String,
    pub steps: Vec<String>,
    pub priority: u8,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct LLMVisibleGoal {
    pub id: GoalId,
    pub mission_id: Option<MissionId>,
    pub title: String,
    pub description: String,
    pub status: GoalStatus,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct LLMVisibleMission {
    pub id: MissionId,
    pub title: String,
    pub description: String,
    pub status: MissionStatus,
}

// -------------------------------------------------------------
// 5. Command Structures
// -------------------------------------------------------------

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct LLMCreateTask {
    pub goal_id: Option<GoalId>,
    pub title: String,
    pub steps: Vec<String>,
    pub trigger: Option<TriggerCondition>,
    pub priority: u8,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub enum TaskAction {
    Create,
    Complete,
    Fail,
    Defer,
    Cancel,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct TaskCommand {
    pub action: TaskAction,
    pub task_id: Option<TaskId>,
    pub task: Option<LLMCreateTask>,
}
