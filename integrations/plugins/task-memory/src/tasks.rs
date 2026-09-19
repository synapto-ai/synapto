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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TaskEvaluationKind {
    Trigger,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TaskQuestionKey {
    pub task_id: TaskId,
    pub kind: TaskEvaluationKind,
}

impl TaskQuestionKey {
    pub fn to_string_key(&self) -> String {
        format!("{}:{:?}", self.task_id.0, self.kind)
    }

    pub fn parse_string_key(key: &str) -> Option<(TaskId, TaskEvaluationKind)> {
        let (id, kind_str) = key.rsplit_once(':')?;
        let kind = match kind_str {
            "Trigger" => TaskEvaluationKind::Trigger,
            "Cancel" => TaskEvaluationKind::Cancel,
            _ => return None,
        };
        Some((TaskId(id.to_string()), kind))
    }
}

pub fn generate_task_questions(
    task: &Task,
) -> (
    Option<synapto_interface::decision::NoulQuestion>,
    synapto_interface::decision::NoulQuestion,
) {
    let trigger_question =
        task.trigger
            .as_ref()
            .map(|t| synapto_interface::decision::NoulQuestion {
                instructions: format!(
                    "Did the user trigger condition occur in the interaction for task '{}'?",
                    task.title
                ),
                criteria: Some(synapto_interface::decision::NoulCriteria {
                    r#true: format!(
                        "The condition '{}' was satisfied in the conversation.",
                        t.description
                    ),
                    r#false: format!("The condition '{}' did not occur.", t.description),
                }),
            });

    let cancel_question = synapto_interface::decision::NoulQuestion {
        instructions: format!(
            "Did the user explicitly cancel, reject, or discard the task '{}'?",
            task.title
        ),
        criteria: Some(synapto_interface::decision::NoulCriteria {
            r#true: "The user commanded to stop, delete, or cancel this specific task.".to_string(),
            r#false: "The user did not state an intention to cancel this task.".to_string(),
        }),
    };

    (trigger_question, cancel_question)
}

enum TaskEvaluationBackend {
    Decision(synapto_interface::decision::DecisionHandle),
    Generative(
        std::sync::Arc<
            synapto_llm::LLMClient<TaskLLMContent, TaskLLMOutput, synapto_llm::WithoutTools>,
        >,
    ),
}

#[instrument(skip_all, fields(subsystem))]
pub async fn task_memory_task<S: synapto_interface::storage::RecordStore>(
    llm_executor: synapto_interface::llm::LlmExecutor,
    decision_handle: synapto_interface::decision::DecisionHandle,
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

    let backend = if decision_handle.is_available() {
        tracing::info!("TaskMemory using Decision backend");
        TaskEvaluationBackend::Decision(decision_handle)
    } else {
        tracing::info!("TaskMemory using Generative LLM backend");
        let client = TaskLLMPrompt::create_client(
            llm_executor,
            task_model_config,
            vec![synapto_llm::Instruction::ImportantSection(
                Box::new(synapto_llm::Instruction::Text(
                    "Executive Function Evaluator".to_string(),
                )),
                vec![
                    synapto_llm::Instruction::Text(
                        "You are an Executive Function Evaluator.".to_string(),
                    ),
                    synapto_llm::Instruction::Text(
                        "Your task is to check the list of `Pending` tasks (Pending Tasks) and decide whether events (new interaction) meet their activation conditions (Triggers).".to_string(),
                    ),
                    synapto_llm::Instruction::ImportantItem(
                        "If the conditions are met, include the task ID in `activated_task_ids`."
                            .to_string(),
                    ),
                    synapto_llm::Instruction::ImportantItem(
                        "If a task is clearly unfeasible or obsolete, include it in `cancelled_task_ids`."
                            .to_string(),
                    ),
                    synapto_llm::Instruction::Text(
                        "If nothing has changed, return empty arrays.".to_string(),
                    ),
                ],
            )],
        );
        TaskEvaluationBackend::Generative(std::sync::Arc::new(client))
    };

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

        let (activated_task_ids, cancelled_task_ids) = match &backend {
            TaskEvaluationBackend::Decision(decision) => {
                let mut questions = std::collections::BTreeMap::new();
                for pt in &pending_tasks {
                    if let Some(task) = tasks.get(&pt.id) {
                        let (trigger_q, cancel_q) = generate_task_questions(task);
                        if let Some(t_q) = trigger_q {
                            let key = TaskQuestionKey {
                                task_id: pt.id.clone(),
                                kind: TaskEvaluationKind::Trigger,
                            }
                            .to_string_key();
                            questions.insert(
                                key,
                                synapto_interface::decision::DecisionQuestion::Noul(t_q),
                            );
                        }
                        let key = TaskQuestionKey {
                            task_id: pt.id.clone(),
                            kind: TaskEvaluationKind::Cancel,
                        }
                        .to_string_key();
                        questions.insert(
                            key,
                            synapto_interface::decision::DecisionQuestion::Noul(cancel_q),
                        );
                    }
                }

                let state = serde_json::json!({
                    "pending_tasks": pending_tasks,
                    "recent_interactions": recent_interactions,
                });

                let answers = match decision.evaluate(None, state, questions).await {
                    Ok(ans) => ans,
                    Err(e) => {
                        tracing::error!("Failed to evaluate tasks via decision backend: {:?}", e);
                        continue;
                    }
                };

                let mut activated = Vec::new();
                let mut cancelled = Vec::new();

                for pt in &pending_tasks {
                    let cancel_key = TaskQuestionKey {
                        task_id: pt.id.clone(),
                        kind: TaskEvaluationKind::Cancel,
                    }
                    .to_string_key();

                    let trigger_key = TaskQuestionKey {
                        task_id: pt.id.clone(),
                        kind: TaskEvaluationKind::Trigger,
                    }
                    .to_string_key();

                    let cancel_prob = match answers.get(&cancel_key) {
                        Some(synapto_interface::decision::DecisionAnswer::Noul { noul }) => *noul,
                        Some(other) => {
                            tracing::warn!(
                                "Unexpected answer variant for {}: {:?}",
                                cancel_key,
                                other
                            );
                            0.0
                        }
                        None => {
                            tracing::warn!("Missing decision answer for {}", cancel_key);
                            0.0
                        }
                    };

                    let trigger_prob = match answers.get(&trigger_key) {
                        Some(synapto_interface::decision::DecisionAnswer::Noul { noul }) => *noul,
                        Some(other) => {
                            tracing::warn!(
                                "Unexpected answer variant for {}: {:?}",
                                trigger_key,
                                other
                            );
                            0.0
                        }
                        None => 0.0,
                    };

                    // Precedence rule: cancellation overrides activation
                    if cancel_prob > 0.75 {
                        cancelled.push(pt.id.clone());
                    } else if trigger_prob > 0.75 {
                        activated.push(pt.id.clone());
                    }
                }

                (activated, cancelled)
            }
            TaskEvaluationBackend::Generative(llm_client) => {
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
                (llm_output.activated_task_ids, llm_output.cancelled_task_ids)
            }
        };

        let mut dirty = false;
        for id in activated_task_ids {
            if let Some(task) = tasks.get_mut(&id)
                && task.status == TaskStatus::Pending
            {
                task.status = TaskStatus::Active;
                dirty = true;
            }
        }
        for id in cancelled_task_ids {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{TaskId, TriggerCondition, TriggerType};

    #[test]
    fn test_task_question_key_roundtrip() {
        let key = TaskQuestionKey {
            task_id: TaskId("task-123".to_string()),
            kind: TaskEvaluationKind::Trigger,
        };
        let s = key.to_string_key();
        assert_eq!(s, "task-123:Trigger");
        let parsed = TaskQuestionKey::parse_string_key(&s).expect("parse failed");
        assert_eq!(parsed.0.0, "task-123");
        assert_eq!(parsed.1, TaskEvaluationKind::Trigger);
    }

    #[test]
    fn test_task_question_key_with_colons() {
        let key = TaskQuestionKey {
            task_id: TaskId("urn:task:01J8Y".to_string()),
            kind: TaskEvaluationKind::Cancel,
        };
        let s = key.to_string_key();
        assert_eq!(s, "urn:task:01J8Y:Cancel");
        let parsed = TaskQuestionKey::parse_string_key(&s).expect("parse failed");
        assert_eq!(parsed.0.0, "urn:task:01J8Y");
        assert_eq!(parsed.1, TaskEvaluationKind::Cancel);
    }

    #[test]
    fn test_generate_task_questions_with_trigger() {
        let task = Task {
            id: TaskId("t1".to_string()),
            goal_id: None,
            title: "Turn off lamp".to_string(),
            steps: vec![],
            trigger: Some(TriggerCondition {
                condition_type: TriggerType::Event,
                description: "leaving home".to_string(),
            }),
            status: TaskStatus::Pending,
            priority: 1,
        };

        let (trigger_q, cancel_q) = generate_task_questions(&task);
        assert!(trigger_q.is_some());
        assert!(trigger_q.unwrap().instructions.contains("Turn off lamp"));
        assert!(cancel_q.instructions.contains("Turn off lamp"));
    }

    #[test]
    fn test_generate_task_questions_without_trigger() {
        let task = Task {
            id: TaskId("t1".to_string()),
            goal_id: None,
            title: "Turn off lamp".to_string(),
            steps: vec![],
            trigger: None,
            status: TaskStatus::Pending,
            priority: 1,
        };

        let (trigger_q, cancel_q) = generate_task_questions(&task);
        assert!(trigger_q.is_none());
        assert!(cancel_q.instructions.contains("Turn off lamp"));
    }

    #[test]
    fn test_conflict_precedence_cancellation_overrides_activation() {
        let cancel_prob = 0.95;
        let trigger_prob = 0.88;

        let mut activated = Vec::new();
        let mut cancelled = Vec::new();
        let task_id = TaskId("t1".to_string());

        if cancel_prob > 0.75 {
            cancelled.push(task_id.clone());
        } else if trigger_prob > 0.75 {
            activated.push(task_id.clone());
        }

        assert_eq!(cancelled.len(), 1);
        assert_eq!(activated.len(), 0);
    }
}
