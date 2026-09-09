use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use synapto_interface::{
    llm::LLMSafe,
    sync::{mpsc, watch},
};

use tracing::instrument;

use synapto_llm::LLM;

use crate::types::{
    GoalId, GoalMemory, GoalStatus, Mission, MissionId, MissionMemory, MissionStatus,
};

#[derive(JsonSchema, Serialize, Clone, Debug)]
struct LLMVisibleMission {
    pub id: MissionId,
    pub title: String,
    pub description: String,
    pub status: MissionStatus,
}

#[derive(JsonSchema, Serialize, Clone, Debug)]
struct LLMVisibleChildGoal {
    pub id: GoalId,
    pub title: String,
    pub description: String,
    pub status: GoalStatus,
}

#[derive(JsonSchema, Serialize, Clone, Debug, LLMSafe)]
struct MissionLLMContent {
    pub current_mission: LLMVisibleMission,
    pub child_goals: Vec<LLMVisibleChildGoal>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, LLMSafe)]
struct MissionLLMOutput {
    #[schemars(
        description = "Synthesized big picture of the mission's current state based on goals"
    )]
    pub description: String,
    pub status: MissionStatus,
}

struct MissionLLMPrompt;

impl LLM for MissionLLMPrompt {
    type Content = MissionLLMContent;
    type Output = MissionLLMOutput;
}

#[instrument(skip_all, fields(subsystem))]
pub async fn mission_memory_task<S: synapto_interface::storage::RecordStore>(
    llm_executor: synapto_interface::llm::LlmExecutor,
    mission_model_config: synapto_interface::llm::ModelConfig,
    store: std::sync::Arc<S>,
    mut mission_dirty_rx: mpsc::Receiver<MissionId>,
    goal_memory_rx: watch::Receiver<GoalMemory>,
    mission_memory_tx: watch::Sender<MissionMemory>,
) {
    let items = store
        .get_ordered_records::<Mission>(
            "missions",
            None,
            synapto_interface::storage::SortOrder::Ascending,
        )
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(_, item)| (item.id.clone(), item))
        .collect::<Vec<_>>();
    let mut missions: MissionMemory = items.into_iter().collect();

    mission_memory_tx
        .send(missions.clone())
        .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
        .ok();

    let llm_client = MissionLLMPrompt::create_client(
        llm_executor,
        mission_model_config,
        vec![synapto_llm::Instruction::ImportantSection(
            Box::new(synapto_llm::Instruction::Text("Mission Strategic Director".to_string())),
            vec![
                synapto_llm::Instruction::Text("You are a Strategic Director (Mission Synthesizer).".to_string()),
                synapto_llm::Instruction::Text("Your task is to evaluate the current state of a long-term Mission based on the status of its subordinate Goals.".to_string()),
                synapto_llm::Instruction::Section(
                    Box::new(synapto_llm::Instruction::Text("Steps".to_string())),
                    vec![
                        synapto_llm::Instruction::NumberedItem("Study the Mission and the list of all its Goals.".to_string()),
                        synapto_llm::Instruction::NumberedItem("Write \"The Big Picture\" (Mission description) - summarize strategic progress and overall impact.".to_string()),
                        synapto_llm::Instruction::Section(
                            Box::new(synapto_llm::Instruction::NumberedItem("Determine the Mission status:".to_string())),
                            vec![
                                synapto_llm::Instruction::Item("`Active` (mission continues, requires further goals, or current goals are not finished)".to_string()),
                                synapto_llm::Instruction::Item("`Achieved` (the final purpose of the mission has been fulfilled, no further steps are needed)".to_string()),
                                synapto_llm::Instruction::Item("`Abandoned` (the mission has lost meaning or is permanently unfeasible)".to_string()),
                            ]
                        ),
                    ]
                ),
                synapto_llm::Instruction::ImportantText("Remember that Missions are long-term and rarely end. Completing one Goal usually only means progress in an active Mission.".to_string()),
            ]
        )],
    );

    while let Some(dirty_mission_id) = mission_dirty_rx.recv().await {
        let mut mission = missions
            .entry(dirty_mission_id.clone())
            .or_insert_with(|| Mission {
                id: dirty_mission_id.clone(),
                title: format!("Auto-generated Mission {}", dirty_mission_id),
                description: "".to_string(),
                status: MissionStatus::Active,
            })
            .clone();

        let goal_memory = goal_memory_rx.borrow().clone();
        let child_goals: Vec<LLMVisibleChildGoal> = goal_memory
            .values()
            .filter(|g| g.mission_id.as_ref() == Some(&dirty_mission_id))
            .map(|g| LLMVisibleChildGoal {
                id: g.id.clone(),
                title: g.title.clone(),
                description: g.description.clone(),
                status: g.status.clone(),
            })
            .collect();

        let llm_output = match llm_client
            .call(
                MissionLLMContent {
                    current_mission: LLMVisibleMission {
                        id: mission.id.clone(),
                        title: mission.title.clone(),
                        description: mission.description.clone(),
                        status: mission.status.clone(),
                    },
                    child_goals,
                },
                None,
                None,
            )
            .await
        {
            Ok(output) => output,
            Err(e) => {
                tracing::error!("Failed to generate mission summary: {:?}", e);
                continue;
            }
        };

        mission.description = llm_output.description;
        mission.status = llm_output.status;

        missions.insert(dirty_mission_id.clone(), mission.clone());

        if let Err(e) = store
            .upsert_record("missions", &mission.id.0, mission.clone())
            .await
        {
            tracing::error!("Failed to write mission memory: {:?}", e);
        }

        mission_memory_tx
            .send(missions.clone())
            .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
            .ok();
    }
    tracing::error!("mission_dirty_rx closed");
}
