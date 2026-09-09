pub mod goals;
pub mod missions;
pub mod tasks;
pub mod types;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use synapto_interface::context::{ContextProvider, ContextRequest, TemporalScope};
use synapto_interface::interaction::InteractionObserver;
use synapto_interface::interaction::ObservedInteraction;
use synapto_interface::plugin::{Plugin, PluginRegistry};
use synapto_interface::sync::{mpsc, watch};

use crate::types::{
    GoalId, GoalMemory, GoalStatus, LLMVisibleGoal, LLMVisibleMission, LLMVisibleTask, MissionId,
    MissionMemory, MissionStatus, TaskCommand, TaskMemory, TaskStatus,
};

// -------------------------------------------------------------
// 1. Private Configuration Slice
// -------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GoogleServiceAccountCredentials(pub serde_json::Value);

impl From<GoogleServiceAccountCredentials> for String {
    fn from(value: GoogleServiceAccountCredentials) -> Self {
        serde_json::to_string(&value.0).unwrap_or_else(|e| panic!("Failed to serialize: {:?}", e))
    }
}

use synapto_interface::llm::ModelConfig;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TaskMemoryConfig {
    #[serde(default)]
    pub google_project_id: String,
    #[serde(default)]
    pub google_vertex_ai_location: String,
    #[serde(default)]
    pub google_service_account_credentials: GoogleServiceAccountCredentials,
    #[serde(default)]
    pub gemini_api_key: String,
    pub task: ModelConfig,
    pub goal: ModelConfig,
    pub mission: ModelConfig,
}

// -------------------------------------------------------------
// 2. Prospective Context Providers
// -------------------------------------------------------------

pub struct ActiveTasksProvider {
    rx: watch::Receiver<TaskMemory>,
}

#[async_trait]
impl ContextProvider for ActiveTasksProvider {
    type Context = Vec<LLMVisibleTask>;
    const NAME: &'static str = "active_tasks";
    const SCOPE: TemporalScope = TemporalScope::Prospective;

    async fn context(&self, _request: &ContextRequest) -> Result<Self::Context, String> {
        let tasks = self.rx.borrow().clone();
        let visible_tasks = tasks
            .values()
            .filter(|t| t.status == TaskStatus::Active)
            .map(|t| LLMVisibleTask {
                id: t.id.clone(),
                goal_id: t.goal_id.clone(),
                title: t.title.clone(),
                steps: t.steps.clone(),
                priority: t.priority,
            })
            .collect();
        Ok(visible_tasks)
    }

    fn subscribe(&self) -> Option<watch::Receiver<()>> {
        let mut rx = self.rx.clone();
        let (tx, out_rx) = watch::channel(());
        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                tx.send(())
                    .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                    .ok();
            }
        });
        Some(out_rx)
    }
}

pub struct ActiveGoalsProvider {
    rx: watch::Receiver<GoalMemory>,
}

#[async_trait]
impl ContextProvider for ActiveGoalsProvider {
    type Context = Vec<LLMVisibleGoal>;
    const NAME: &'static str = "active_goals";
    const SCOPE: TemporalScope = TemporalScope::Prospective;

    async fn context(&self, _request: &ContextRequest) -> Result<Self::Context, String> {
        let goals = self.rx.borrow().clone();
        let visible_goals = goals
            .values()
            .filter(|g| g.status == GoalStatus::Active)
            .map(|g| LLMVisibleGoal {
                id: g.id.clone(),
                mission_id: g.mission_id.clone(),
                title: g.title.clone(),
                description: g.description.clone(),
                status: g.status.clone(),
            })
            .collect();
        Ok(visible_goals)
    }

    fn subscribe(&self) -> Option<watch::Receiver<()>> {
        let mut rx = self.rx.clone();
        let (tx, out_rx) = watch::channel(());
        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                tx.send(())
                    .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                    .ok();
            }
        });
        Some(out_rx)
    }
}

pub struct ActiveMissionsProvider {
    rx: watch::Receiver<MissionMemory>,
}

#[async_trait]
impl ContextProvider for ActiveMissionsProvider {
    type Context = Vec<LLMVisibleMission>;
    const NAME: &'static str = "active_missions";
    const SCOPE: TemporalScope = TemporalScope::Prospective;

    async fn context(&self, _request: &ContextRequest) -> Result<Self::Context, String> {
        let missions = self.rx.borrow().clone();
        let visible_missions = missions
            .values()
            .filter(|m| m.status == MissionStatus::Active)
            .map(|m| LLMVisibleMission {
                id: m.id.clone(),
                title: m.title.clone(),
                description: m.description.clone(),
                status: m.status.clone(),
            })
            .collect();
        Ok(visible_missions)
    }

    fn subscribe(&self) -> Option<watch::Receiver<()>> {
        let mut rx = self.rx.clone();
        let (tx, out_rx) = watch::channel(());
        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                tx.send(())
                    .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                    .ok();
            }
        });
        Some(out_rx)
    }
}

// -------------------------------------------------------------
// 3. Generic Command Executor
// -------------------------------------------------------------

#[derive(Clone)]
pub struct TaskCommandExecutor {
    tx: mpsc::Sender<TaskCommand>,
}

#[async_trait]
impl synapto_interface::command::Command for TaskCommandExecutor {
    type Arguments = TaskCommand;
    const NAME: &'static str = "task_command";

    async fn execute(&self, args: Self::Arguments) -> Result<(), String> {
        self.tx.send(args).await.map_err(|e| e.to_string())
    }
}

// -------------------------------------------------------------
// 4. Main Plugin Struct
// -------------------------------------------------------------

pub struct TaskMemoryPlugin<
    S: synapto_interface::storage::RecordStore + synapto_interface::storage::StorageConnection,
> {
    config: TaskMemoryConfig,
    llm_executor: synapto_interface::llm::LlmExecutor,
    store: Arc<S>,

    // Providers & Executors
    tasks_provider: Arc<ActiveTasksProvider>,
    goals_provider: Arc<ActiveGoalsProvider>,
    missions_provider: Arc<ActiveMissionsProvider>,
    command_executor: TaskCommandExecutor,

    // Channels to pass at start time
    task_command_rx: Mutex<Option<mpsc::Receiver<TaskCommand>>>,
    task_memory_tx: Mutex<Option<watch::Sender<TaskMemory>>>,
    goal_memory_tx: Mutex<Option<watch::Sender<GoalMemory>>>,
    mission_memory_tx: Mutex<Option<watch::Sender<MissionMemory>>>,
}

#[async_trait::async_trait]
impl<S> Plugin for TaskMemoryPlugin<S>
where
    S: synapto_interface::storage::RecordStore + synapto_interface::storage::StorageConnection,
{
    async fn create(
        context: &synapto_interface::plugin::PluginInitContext<'_>,
    ) -> Result<Self, String>
    where
        Self: Sized,
    {
        let config: TaskMemoryConfig = context.config()?;
        let store = context.store::<S>().await?;

        let (task_command_tx, task_command_rx) = mpsc::channel::<TaskCommand>(100);
        let (task_memory_tx, task_memory_rx) = watch::channel::<TaskMemory>(TaskMemory::default());
        let (goal_memory_tx, goal_memory_rx) = watch::channel::<GoalMemory>(GoalMemory::default());
        let (mission_memory_tx, mission_memory_rx) =
            watch::channel::<MissionMemory>(MissionMemory::default());

        let tasks_provider = Arc::new(ActiveTasksProvider { rx: task_memory_rx });
        let goals_provider = Arc::new(ActiveGoalsProvider { rx: goal_memory_rx });
        let missions_provider = Arc::new(ActiveMissionsProvider {
            rx: mission_memory_rx,
        });
        let command_executor = TaskCommandExecutor {
            tx: task_command_tx,
        };

        Ok(Self {
            config,
            llm_executor: context.llm_executor(),
            store,
            tasks_provider,
            goals_provider,
            missions_provider,
            command_executor,
            task_command_rx: Mutex::new(Some(task_command_rx)),
            task_memory_tx: Mutex::new(Some(task_memory_tx)),
            goal_memory_tx: Mutex::new(Some(goal_memory_tx)),
            mission_memory_tx: Mutex::new(Some(mission_memory_tx)),
        })
    }

    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R)
    where
        Self: Sized,
    {
        registry.register_interaction_observer(self.clone());
        registry.register_context_provider(self.tasks_provider.clone());
        registry.register_context_provider(self.goals_provider.clone());
        registry.register_context_provider(self.missions_provider.clone());
        registry.register_command(self.command_executor.clone());
    }
}

#[async_trait]
impl<S> InteractionObserver for TaskMemoryPlugin<S>
where
    S: synapto_interface::storage::RecordStore + synapto_interface::storage::StorageConnection,
{
    async fn start(
        &self,
        interaction_rx: mpsc::Receiver<ObservedInteraction>,
    ) -> Result<(), String> {
        let task_command_rx = self
            .task_command_rx
            .lock()
            .unwrap_or_else(|e| panic!("Error: {:?}", e))
            .take()
            .ok_or("Already started")?;
        let task_memory_tx = self
            .task_memory_tx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e))
            .take()
            .expect("Missing value");
        let goal_memory_tx = self
            .goal_memory_tx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e))
            .take()
            .expect("Missing value");
        let mission_memory_tx = self
            .mission_memory_tx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e))
            .take()
            .expect("Missing value");

        let (goal_dirty_tx, goal_dirty_rx) = mpsc::channel::<GoalId>(100);
        let (mission_dirty_tx, mission_dirty_rx) = mpsc::channel::<MissionId>(100);

        // Spawn evaluator tasks
        let llm_executor = self.llm_executor.clone();
        let task_model_config = self.config.task.clone();
        let store = self.store.clone();
        let task_memory_rx = self.tasks_provider.rx.clone();
        let llm_executor_clone = llm_executor.clone();
        tokio::spawn(async move {
            crate::tasks::task_memory_task(
                llm_executor_clone,
                task_model_config,
                store,
                interaction_rx,
                task_command_rx,
                task_memory_tx,
                goal_dirty_tx,
            )
            .await;
        });

        let goal_model_config = self.config.goal.clone();
        let store = self.store.clone();
        let goal_memory_rx = self.goals_provider.rx.clone();
        let llm_executor_clone = llm_executor.clone();
        tokio::spawn(async move {
            crate::goals::goal_memory_task(
                llm_executor_clone,
                goal_model_config,
                store,
                goal_dirty_rx,
                task_memory_rx,
                goal_memory_tx,
                mission_dirty_tx,
            )
            .await;
        });

        let mission_model_config = self.config.mission.clone();
        let store = self.store.clone();
        let llm_executor_clone = llm_executor.clone();
        tokio::spawn(async move {
            crate::missions::mission_memory_task(
                llm_executor_clone,
                mission_model_config,
                store,
                mission_dirty_rx,
                goal_memory_rx,
                mission_memory_tx,
            )
            .await;
        });

        Ok(())
    }
}
