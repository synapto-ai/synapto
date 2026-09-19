use crate::credentials::CredentialsHandle;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

/// Pure capability trait for decision providers.
/// Decision providers do NOT implement Plugin and have no actor or channel capabilities.
pub trait DecisionProvider: Send + Sync + 'static {
    type Config: serde::de::DeserializeOwned;

    fn init(config: Self::Config, credentials: CredentialsHandle) -> Result<Self, String>
    where
        Self: Sized;

    fn raw_decision_executor(&self) -> Arc<dyn RawDecisionExecutor>;
}

/// Binary condition evaluation question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NoulQuestion {
    pub instructions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub criteria: Option<NoulCriteria>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NoulCriteria {
    pub r#true: String,
    pub r#false: String,
}

/// Categorical selection question from a defined rubric.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChoiceQuestion {
    pub instructions: String,
    pub criteria: BTreeMap<String, String>,
}

/// Graded continuous or multi-level scoring question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreQuestion {
    pub instructions: String,
    pub criteria: Vec<String>,
}

/// Typed question variants supported by the Decision Subsystem.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DecisionQuestion {
    Noul(NoulQuestion),
    Choice(ChoiceQuestion),
    Score(ScoreQuestion),
}

/// Answers returned by the decision backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DecisionAnswer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: f64,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
}

#[async_trait]
pub trait RawDecisionExecutor: Send + Sync + 'static {
    async fn evaluate_raw(
        &self,
        model: Option<&str>,
        state: serde_json::Value,
        questions: BTreeMap<String, DecisionQuestion>,
    ) -> Result<BTreeMap<String, DecisionAnswer>, String>;
}

/// Opaque, thread-safe handle to the decision backend.
/// Uses internal shared cell resolution to eliminate bundle ordering dependencies.
#[derive(Clone, Default)]
pub struct DecisionHandle {
    backend: Arc<RwLock<Option<Arc<dyn RawDecisionExecutor>>>>,
}

impl DecisionHandle {
    pub fn empty() -> Self {
        Self {
            backend: Arc::new(RwLock::new(None)),
        }
    }

    pub fn set_backend<B: RawDecisionExecutor + 'static>(&self, backend: B) {
        let mut lock = self
            .backend
            .write()
            .unwrap_or_else(|e| panic!("Poisoned RwLock: {:?}", e));
        let arc: Arc<dyn RawDecisionExecutor> = Arc::new(backend);
        *lock = Some(arc);
    }

    pub fn set_arc_backend(&self, backend: Arc<dyn RawDecisionExecutor>) {
        let mut lock = self
            .backend
            .write()
            .unwrap_or_else(|e| panic!("Poisoned RwLock: {:?}", e));
        *lock = Some(backend);
    }

    pub fn is_available(&self) -> bool {
        self.backend
            .read()
            .unwrap_or_else(|e| panic!("Poisoned RwLock: {:?}", e))
            .is_some()
    }

    #[tracing::instrument(
        level = "info",
        skip_all,
        fields(
            track_stats = true,
            model = model.unwrap_or("default"),
            questions_count = questions.len()
        )
    )]
    pub async fn evaluate(
        &self,
        model: Option<&str>,
        state: serde_json::Value,
        questions: BTreeMap<String, DecisionQuestion>,
    ) -> Result<BTreeMap<String, DecisionAnswer>, String> {
        let backend = {
            let lock = self
                .backend
                .read()
                .unwrap_or_else(|e| panic!("Poisoned RwLock: {:?}", e));
            lock.clone()
        };
        match backend {
            Some(executor) => executor.evaluate_raw(model, state, questions).await,
            None => Err("No decision provider registered".to_string()),
        }
    }
}
