use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use synapto_interface::{
    llm::LLMSafe,
    sync::{mpsc, watch},
};

use tracing::instrument;

use synapto_llm::LLM;

use crate::types::{
    Goal, GoalId, GoalMemory, GoalStatus, MissionId, TaskId, TaskMemory, TaskStatus,
};

#[derive(JsonSchema, Serialize, Clone, Debug)]
struct LLMVisibleGoal {
    pub id: GoalId,
    pub mission_id: Option<MissionId>,
    pub title: String,
    pub description: String,
    pub status: GoalStatus,
}

#[derive(JsonSchema, Serialize, Clone, Debug)]
struct LLMVisibleChildTask {
    pub id: TaskId,
    pub title: String,
    pub status: TaskStatus,
}

#[derive(JsonSchema, Serialize, Clone, Debug, LLMSafe)]
struct GoalLLMContent {
    pub current_goal: LLMVisibleGoal,
    pub child_tasks: Vec<LLMVisibleChildTask>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, LLMSafe)]
struct GoalLLMOutput {
    #[schemars(description = "Synthesized big picture of the goal's current state based on tasks")]
    pub description: String,
    pub status: GoalStatus,
}

struct GoalLLMPrompt;

impl LLM for GoalLLMPrompt {
    type Content = GoalLLMContent;
    type Output = GoalLLMOutput;
}

#[instrument(skip_all, fields(subsystem))]
pub async fn goal_memory_task<S: synapto_interface::storage::RecordStore>(
    llm_executor: synapto_interface::llm::LlmExecutor,
    goal_model_config: synapto_interface::llm::ModelConfig,
    store: std::sync::Arc<S>,
    mut goal_dirty_rx: mpsc::Receiver<GoalId>,
    task_memory_rx: watch::Receiver<TaskMemory>,
    goal_memory_tx: watch::Sender<GoalMemory>,
    mission_dirty_tx: mpsc::Sender<MissionId>,
) {
    let items = store
        .get_ordered_records::<Goal>(
            "goals",
            None,
            synapto_interface::storage::SortOrder::Ascending,
        )
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(_, item)| (item.id.clone(), item))
        .collect::<Vec<_>>();
    let mut goals: GoalMemory = items.into_iter().collect();

    goal_memory_tx
        .send(goals.clone())
        .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
        .ok();

    let llm_client = GoalLLMPrompt::create_client(
        llm_executor,
        goal_model_config,
        vec![synapto_llm::Instruction::ImportantSection(
            Box::new(synapto_llm::Instruction::Text("Goal Evaluator".to_string())),
            vec![
                synapto_llm::Instruction::Text("You are a Goal Synthesizer and Project Manager.".to_string()),
                synapto_llm::Instruction::Text("Your task is to evaluate the current state of a specific Goal based on the status of its subordinate Tasks.".to_string()),
                synapto_llm::Instruction::Section(
                    Box::new(synapto_llm::Instruction::Text("Steps".to_string())),
                    vec![
                        synapto_llm::Instruction::NumberedItem("Study the Goal and the list of all its tasks.".to_string()),
                        synapto_llm::Instruction::NumberedItem("Write \"The Big Picture\" (Goal description) - summarize current progress, what has been achieved, and what is potentially blocked.".to_string()),
                        synapto_llm::Instruction::Section(
                            Box::new(synapto_llm::Instruction::NumberedItem("Determine the Goal status:".to_string())),
                            vec![
                                synapto_llm::Instruction::Item("`Active` (still ongoing, tasks are not finished, or the goal is not fulfilled)".to_string()),
                                synapto_llm::Instruction::Item("`Achieved` (the goal has been successfully fulfilled)".to_string()),
                                synapto_llm::Instruction::Item("`Abandoned` (tasks have failed, the goal is unrealistic or no longer relevant)".to_string()),
                            ]
                        ),
                    ]
                )
            ]
        )],
    );

    while let Some(dirty_goal_id) = goal_dirty_rx.recv().await {
        let mut goal = goals
            .entry(dirty_goal_id.clone())
            .or_insert_with(|| Goal {
                id: dirty_goal_id.clone(),
                mission_id: None,
                title: format!("Auto-generated Goal {}", dirty_goal_id),
                description: "".to_string(),
                status: GoalStatus::Active,
            })
            .clone();

        let task_memory = task_memory_rx.borrow().clone();
        let child_tasks: Vec<LLMVisibleChildTask> = task_memory
            .values()
            .filter(|t| t.goal_id.as_ref() == Some(&dirty_goal_id))
            .map(|t| LLMVisibleChildTask {
                id: t.id.clone(),
                title: t.title.clone(),
                status: t.status.clone(),
            })
            .collect();

        let llm_output = match llm_client
            .call(
                GoalLLMContent {
                    current_goal: LLMVisibleGoal {
                        id: goal.id.clone(),
                        mission_id: goal.mission_id.clone(),
                        title: goal.title.clone(),
                        description: goal.description.clone(),
                        status: goal.status.clone(),
                    },
                    child_tasks,
                },
                None,
                None,
            )
            .await
        {
            Ok(output) => output,
            Err(e) => {
                tracing::error!("Failed to generate goal summary: {:?}", e);
                continue;
            }
        };

        let status_changed = goal.status != llm_output.status;

        goal.description = llm_output.description;
        goal.status = llm_output.status;

        goals.insert(dirty_goal_id.clone(), goal.clone());

        if let Err(e) = store.upsert_record("goals", &goal.id.0, goal.clone()).await {
            tracing::error!("Failed to write goal memory: {:?}", e);
        }

        goal_memory_tx
            .send(goals.clone())
            .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
            .ok();

        if status_changed && let Some(ref mission_id) = goal.mission_id {
            mission_dirty_tx
                .send(mission_id.clone())
                .await
                .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                .ok();
        }
    }
    tracing::error!("goal_dirty_rx closed");
}
