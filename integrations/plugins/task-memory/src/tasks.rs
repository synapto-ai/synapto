use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use synapto_interface::{
    llm::LLMSafe,
    sync::{mpsc, watch},
};

use tracing::instrument;

use synapto_llm::LLM;

use crate::types::{
    GoalId, Task, TaskAction, TaskCommand, TaskId, TaskMemory, TaskStatus, TriggerCondition,
};

#[derive(JsonSchema, Serialize, Clone, Debug)]
pub struct LLMUserMessage {
    pub speaker: String,
    pub text: String,
}

#[derive(JsonSchema, Serialize, Clone, Debug)]
pub struct CognitiveLLMInteraction {
    pub user_messages: Vec<LLMUserMessage>,
    pub cognitive_spoken: Option<String>,
    pub cognitive_reasoning: Option<String>,
}

#[derive(JsonSchema, Serialize, Clone, Debug)]
pub struct SummaryLLMInteraction {
    pub timestamp: i64,
    pub interaction: CognitiveLLMInteraction,
}

use synapto_interface::interaction::ObservedInteraction;
use synapto_interface::peer_input::{PeerInput, Speaker};

impl From<&ObservedInteraction> for SummaryLLMInteraction {
    fn from(interaction: &ObservedInteraction) -> Self {
        let user_messages = interaction
            .user_messages
            .iter()
            .map(|msg| match msg {
                PeerInput::Speech(s) => LLMUserMessage {
                    speaker: match &s.speaker {
                        Speaker::Recognized(id) => id.0.to_string(),
                        Speaker::Unknown(None) => "Unknown".to_string(),
                        Speaker::Unknown(Some(id)) => id.0.to_string(),
                    },
                    text: s.transcript.to_string(),
                },
                PeerInput::Text(t) => LLMUserMessage {
                    speaker: t.sender_id.to_string(),
                    text: t.text.to_string(),
                },
            })
            .collect();

        Self {
            timestamp: interaction.timestamp.0,
            interaction: CognitiveLLMInteraction {
                user_messages,
                cognitive_spoken: interaction.cognitive_spoken.as_ref().map(|s| s.0.clone()),
                cognitive_reasoning: interaction
                    .cognitive_reasoning
                    .as_ref()
                    .map(|r| r.0.clone()),
            },
        }
    }
}

#[derive(JsonSchema, Serialize, Clone, Debug)]
struct LLMVisiblePendingTask {
    pub id: TaskId,
    pub title: String,
    pub trigger: Option<TriggerCondition>,
}

#[derive(JsonSchema, Serialize, Clone, Debug, LLMSafe)]
struct TaskLLMContent {
    pub pending_tasks: Vec<LLMVisiblePendingTask>,
    pub recent_interactions: Vec<SummaryLLMInteraction>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, LLMSafe)]
struct TaskLLMOutput {
    pub activated_task_ids: Vec<TaskId>,
    pub cancelled_task_ids: Vec<TaskId>,
}

struct TaskLLMPrompt;

impl LLM for TaskLLMPrompt {
    type Content = TaskLLMContent;
    type Output = TaskLLMOutput;
}

#[instrument(skip_all, fields(subsystem))]
pub async fn task_memory_task<S: synapto_interface::storage::RecordStore>(
    llm_executor: synapto_interface::llm::LlmExecutor,
    task_model_config: synapto_interface::llm::ModelConfig,
    store: std::sync::Arc<S>,
    mut interaction_rx: mpsc::Receiver<ObservedInteraction>,
    mut task_command_rx: mpsc::Receiver<TaskCommand>,
    task_memory_tx: watch::Sender<TaskMemory>,
    goal_dirty_tx: mpsc::Sender<GoalId>,
) {
    let items = store
        .get_ordered_records::<Task>(
            "tasks",
            None,
            synapto_interface::storage::SortOrder::Ascending,
        )
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(_, item)| (item.id.clone(), item))
        .collect::<Vec<_>>();

    let mut tasks: TaskMemory = items.into_iter().collect();

    task_memory_tx
        .send(tasks.clone())
        .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
        .ok();

    let llm_client = TaskLLMPrompt::create_client(
        llm_executor,
        task_model_config,
        vec![synapto_llm::Instruction::ImportantSection(
            Box::new(synapto_llm::Instruction::Text("Executive Function Evaluator".to_string())),
            vec![
                synapto_llm::Instruction::Text("You are an Executive Function Evaluator.".to_string()),
                synapto_llm::Instruction::Text("Your task is to check the list of `Pending` tasks (Pending Tasks) and decide whether events (new interaction) meet their activation conditions (Triggers).".to_string()),
                synapto_llm::Instruction::ImportantItem("If the conditions are met, include the task ID in `activated_task_ids`.".to_string()),
                synapto_llm::Instruction::ImportantItem("If a task is clearly unfeasible or obsolete, include it in `cancelled_task_ids`.".to_string()),
                synapto_llm::Instruction::Text("If nothing has changed, return empty arrays.".to_string()),
            ]
        )],
    );

    loop {
        let batch = better_tokio_select::tokio_select!(match .. {
            .. if let cmd_res = task_command_rx.recv() => {
                match cmd_res {
                    Some(cmd) => {
                        handle_task_command(&mut tasks, cmd, &goal_dirty_tx).await;
                        task_memory_tx
                            .send(tasks.clone())
                            .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                            .ok();
                        save_tasks(&*store, &tasks).await;
                        continue;
                    }
                    None => {
                        tracing::error!("task_command_rx closed");
                        return;
                    }
                }
            }
            .. if let res = interaction_rx.recv() => {
                match res {
                    Some(first_interaction) => {
                        let mut batch = vec![first_interaction];
                        while let Ok(next_interaction) = interaction_rx.try_recv() {
                            batch.push(next_interaction);
                        }
                        batch
                    }
                    None => {
                        tracing::error!("interaction_rx closed");
                        return;
                    }
                }
            }
        });

        let pending_tasks: Vec<LLMVisiblePendingTask> = tasks
            .values()
            .filter(|t| t.status == TaskStatus::Pending)
            .map(|t| LLMVisiblePendingTask {
                id: t.id.clone(),
                title: t.title.clone(),
                trigger: t.trigger.clone(),
            })
            .collect();

        if pending_tasks.is_empty() {
            continue;
        }

        let recent_interactions: Vec<SummaryLLMInteraction> =
            batch.iter().map(SummaryLLMInteraction::from).collect();

        if recent_interactions.is_empty() {
            continue;
        }

        let llm_output = match llm_client
            .call(
                TaskLLMContent {
                    pending_tasks,
                    recent_interactions,
                },
                None,
                None,
            )
            .await
        {
            Ok(output) => output,
            Err(e) => {
                tracing::error!("Failed to evaluate tasks: {:?}", e);
                continue;
            }
        };

        let mut dirty = false;
        for id in llm_output.activated_task_ids {
            if let Some(task) = tasks.get_mut(&id)
                && task.status == TaskStatus::Pending
            {
                task.status = TaskStatus::Active;
                dirty = true;
            }
        }
        for id in llm_output.cancelled_task_ids {
            if let Some(task) = tasks.get_mut(&id)
                && task.status == TaskStatus::Pending
            {
                task.status = TaskStatus::Cancelled;
                if let Some(ref goal_id) = task.goal_id {
                    goal_dirty_tx
                        .send(goal_id.clone())
                        .await
                        .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                        .ok();
                }
                dirty = true;
            }
        }

        if dirty {
            task_memory_tx
                .send(tasks.clone())
                .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                .ok();
            save_tasks(&*store, &tasks).await;
        }
    }
}

async fn handle_task_command(
    tasks: &mut TaskMemory,
    cmd: TaskCommand,
    goal_dirty_tx: &mpsc::Sender<GoalId>,
) {
    match cmd.action {
        TaskAction::Create => {
            if let Some(task) = cmd.task {
                let id = TaskId(format!("task-{}", chrono::Utc::now().timestamp_millis()));
                let status = if task.trigger.is_some() {
                    TaskStatus::Pending
                } else {
                    TaskStatus::Active
                };
                let new_task = Task {
                    id: id.clone(),
                    goal_id: task.goal_id,
                    title: task.title,
                    steps: task.steps,
                    trigger: task.trigger,
                    status,
                    priority: task.priority,
                };
                tasks.insert(id, new_task);
            }
        }
        TaskAction::Complete | TaskAction::Fail | TaskAction::Defer | TaskAction::Cancel => {
            if let Some(task_id) = cmd.task_id
                && let Some(task) = tasks.get_mut(&task_id)
            {
                task.status = match cmd.action {
                    TaskAction::Complete => TaskStatus::Completed,
                    TaskAction::Fail => TaskStatus::Failed,
                    TaskAction::Defer => TaskStatus::Deferred,
                    TaskAction::Cancel => TaskStatus::Cancelled,
                    _ => unreachable!(),
                };
                if let Some(ref goal_id) = task.goal_id
                    && matches!(
                        task.status,
                        TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
                    )
                {
                    goal_dirty_tx
                        .send(goal_id.clone())
                        .await
                        .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                        .ok();
                }
            }
        }
    }
}

async fn save_tasks<S: synapto_interface::storage::RecordStore>(store: &S, tasks: &TaskMemory) {
    for task in tasks.values() {
        if let Err(e) = store.upsert_record("tasks", &task.id.0, task.clone()).await {
            tracing::error!("Failed to write task {}: {:?}", task.id.0, e);
        }
    }
}
