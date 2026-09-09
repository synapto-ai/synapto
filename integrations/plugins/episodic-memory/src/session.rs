use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::EpisodicMemoryConfig;
use synapto_interface::cognitive::CognitiveReasoning;
use synapto_interface::interaction::{CognitiveSpoken, Timestamp};
use synapto_interface::llm::LLMSafe;
use synapto_interface::storage::RecordStore;
use synapto_interface::sync::{mpsc, watch};
use synapto_llm::{Instruction, LLM};

// -------------------------------------------------------------
// Base Episodic Session Models
// -------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub timestamp: Timestamp,
    pub text: String,
}

impl Session {
    pub fn new(text: String) -> Self {
        Self {
            timestamp: Timestamp(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_else(|e| panic!("Error: {:?}", e))
                    .as_secs() as i64,
            ),
            text,
        }
    }
}

#[derive(
    derive_more::IntoIterator,
    derive_more::Deref,
    derive_more::DerefMut,
    Serialize,
    Deserialize,
    Default,
    Clone,
    Debug,
    PartialEq,
    Eq,
)]
pub struct SessionMemory(pub Vec<Session>);

impl SessionMemory {
    pub fn push(&mut self, session: Session) {
        self.0.push(session);
    }
}

// -------------------------------------------------------------
// LLM View Models
// -------------------------------------------------------------

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct CognitiveLLMSession(pub String);

impl From<&Session> for CognitiveLLMSession {
    fn from(session: &Session) -> Self {
        Self(session.text.clone())
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct CognitiveLLMSessionMemory {
    pub previous_session: Option<CognitiveLLMSession>,
    pub current_session: Option<CognitiveLLMSession>,
}

impl From<SessionMemory> for CognitiveLLMSessionMemory {
    fn from(session_memory: SessionMemory) -> Self {
        let mut rev = session_memory.iter().rev();
        let (current_session, previous_session) = (
            rev.next().map(CognitiveLLMSession::from),
            rev.next().map(CognitiveLLMSession::from),
        );
        Self {
            previous_session,
            current_session,
        }
    }
}

// -------------------------------------------------------------
// Summary Interaction Payload Mapping
// -------------------------------------------------------------

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub enum LLMUserMessage {
    Speech { speaker: String, transcript: String },
    Text { sender: String, text: String },
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct CognitiveLLMInteraction {
    pub user_messages: Vec<LLMUserMessage>,
    pub cognitive_spoken: Option<CognitiveSpoken>,
    pub cognitive_reasoning: Option<CognitiveReasoning>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct SummaryLLMInteraction {
    pub timestamp: Timestamp,
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
                PeerInput::Speech(s) => LLMUserMessage::Speech {
                    speaker: match &s.speaker {
                        Speaker::Recognized(id) => id.0.to_string(),
                        Speaker::Unknown(None) => "Unknown".to_string(),
                        Speaker::Unknown(Some(id)) => id.0.to_string(),
                    },
                    transcript: s.transcript.to_string(),
                },
                PeerInput::Text(t) => LLMUserMessage::Text {
                    sender: t.sender_id.to_string(),
                    text: t.text.to_string(),
                },
            })
            .collect();

        Self {
            timestamp: interaction.timestamp,
            interaction: CognitiveLLMInteraction {
                user_messages,
                cognitive_spoken: interaction.cognitive_spoken.clone(),
                cognitive_reasoning: interaction.cognitive_reasoning.clone(),
            },
        }
    }
}

// -------------------------------------------------------------
// 4. Background Task: Session Memory
// -------------------------------------------------------------

#[derive(JsonSchema, Serialize, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct SessionLLMContent {
    active_session: Option<CognitiveLLMSession>,
    new_interactions: Vec<SummaryLLMInteraction>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct SessionLLMOutput {
    #[schemars(description = "None if active_session is None or does not need updates")]
    active_session_update: Option<CognitiveLLMSession>,
    #[schemars(
        description = "New sessions created from interactions. When active_session is None, create new session(s) here."
    )]
    new_sessions: Vec<CognitiveLLMSession>,

    #[schemars(description = "None if no interaction was included")]
    last_included_interaction: Option<Timestamp>,
}

pub struct SessionLLMPrompt {}
impl LLM for SessionLLMPrompt {
    type Content = SessionLLMContent;
    type Output = SessionLLMOutput;
}

#[instrument(skip_all, fields(subsystem))]
pub async fn session_memory_task<S: RecordStore>(
    config: EpisodicMemoryConfig,
    store: std::sync::Arc<S>,
    mut interaction_rx: mpsc::Receiver<ObservedInteraction>,
    session_memory_tx: watch::Sender<SessionMemory>,
    session_interaction_rollout_tx: watch::Sender<Timestamp>,
    new_session_tx: mpsc::Sender<Session>,
    llm_executor: synapto_interface::llm::LlmExecutor,
) {
    let mut session_memory: SessionMemory = match store
        .get_ordered_records(
            "sessions",
            None,
            synapto_interface::storage::SortOrder::Ascending,
        )
        .await
    {
        Ok(items) => SessionMemory(items.into_iter().map(|(_, item)| item).collect()),
        Err(e) => {
            tracing::error!("Failed to load sessions: {}", e);
            SessionMemory::default()
        }
    };

    session_memory_tx.send_replace(session_memory.clone());

    let llm_client = SessionLLMPrompt::create_client(
        llm_executor,
        config.session,
        vec![Instruction::Section(
            Box::new(Instruction::Text(
                "Senior Developer / Story Chronicler".to_string(),
            )),
            vec![
                Instruction::Text(
                    "You are an advanced cognitive agent recording the ongoing episodic events."
                        .to_string(),
                ),
                Instruction::Text(
                    "Focus strictly on factual, empirical steps of what has occurred, addressing modifications, specific decisions made, and key consequences. Do not speculate or infer future trends."
                        .to_string(),
                ),
                Instruction::Text(
                    "If you detect that the current block or scene has reached its logical conclusion, produce a new one."
                        .to_string(),
                ),
            ],
        )],
    );

    while let Some(first_interaction) = interaction_rx.recv().await {
        let mut batch = vec![first_interaction];
        while let Ok(next_interaction) = interaction_rx.try_recv() {
            batch.push(next_interaction);
        }

        let new_interactions: Vec<_> = batch.iter().map(SummaryLLMInteraction::from).collect();

        if new_interactions.is_empty() {
            if let Some(last_interaction) = batch.last() {
                session_interaction_rollout_tx
                    .send(last_interaction.timestamp)
                    .inspect_err(|e| tracing::error!("{}", e))
                    .ok();
            }
            continue;
        }

        let response_res = llm_client
            .call(
                SessionLLMContent {
                    active_session: session_memory
                        .0
                        .clone()
                        .iter()
                        .last()
                        .map(CognitiveLLMSession::from),
                    new_interactions,
                },
                None,
                None,
            )
            .await;

        let SessionLLMOutput {
            active_session_update,
            new_sessions: new_sessions_creation,
            last_included_interaction,
        } = match response_res {
            Ok(output) => output,
            Err(e) => {
                tracing::error!("LLM Call Failed in session_memory_task: {}", e);
                continue;
            }
        };

        if let Some(update) = active_session_update {
            if let Some(active_session) = session_memory.0.last_mut() {
                active_session.text = update.0;
            } else if new_sessions_creation.is_empty() {
                session_memory.push(Session::new(update.0));
            } else {
                tracing::error!("Received both session update and creation while memory is empty");
            }
        }

        for creation in new_sessions_creation {
            let new_session = Session::new(creation.0);
            session_memory.push(new_session.clone());
            new_session_tx
                .send(new_session)
                .await
                .inspect_err(|e| tracing::error!("{}", e))
                .ok();
        }

        if let Some(last_included_interaction) = last_included_interaction {
            session_interaction_rollout_tx
                .send(last_included_interaction)
                .inspect_err(|e| tracing::error!("{}", e))
                .ok();
            session_memory_tx
                .send(session_memory.clone())
                .inspect_err(|e| tracing::error!("{}", e))
                .ok();
        }

        if let Err(e) = store
            .trim_records_before("sessions", "REPLACE_ME_TODO")
            .await
        {
            tracing::error!("Failed to write session memory: {:?}", e);
        }
    }
}
