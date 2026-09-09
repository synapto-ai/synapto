pub mod continuum;
pub mod progression;
pub mod session;

pub use continuum::{
    CognitiveLLMContinuum, CognitiveLLMContinuumMemory, Continuum, ContinuumMemory,
    continuum_memory_task,
};
pub use progression::{
    CognitiveLLMProgression, CognitiveLLMProgressionMemory, Progression, ProgressionMemory,
    progression_memory_task,
};
pub use session::{
    CognitiveLLMSession, CognitiveLLMSessionMemory, Session, SessionMemory, session_memory_task,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use synapto_interface::context::{ContextProvider, ContextRequest, TemporalScope};
use synapto_interface::interaction::InteractionObserver;
use synapto_interface::interaction::{ObservedInteraction, Timestamp};
use synapto_interface::llm::ModelConfig;
use synapto_interface::plugin::{Plugin, PluginRegistry};
use synapto_interface::storage::{RecordStore, StorageConnection};
use synapto_interface::sync::{mpsc, watch};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EpisodicMemoryConfig {
    pub session: ModelConfig,
    pub progression: ModelConfig,
    pub continuum: ModelConfig,
}

pub struct SessionMemoryProvider {
    pub rx: watch::Receiver<SessionMemory>,
}

#[async_trait]
impl ContextProvider for SessionMemoryProvider {
    type Context = CognitiveLLMSessionMemory;
    const NAME: &'static str = "session_memory";
    const SCOPE: TemporalScope = TemporalScope::Current;

    async fn context(&self, _request: &ContextRequest) -> Result<Self::Context, String> {
        let memory = self.rx.borrow().clone();
        Ok(CognitiveLLMSessionMemory::from(memory))
    }

    fn subscribe(&self) -> Option<watch::Receiver<()>> {
        let mut rx = self.rx.clone();
        let (tx, out_rx) = watch::channel(());
        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                tx.send(()).inspect_err(|e| tracing::error!("{}", e)).ok();
            }
        });
        Some(out_rx)
    }
}

pub struct ProgressionMemoryProvider {
    pub rx: watch::Receiver<ProgressionMemory>,
}

#[async_trait]
impl ContextProvider for ProgressionMemoryProvider {
    type Context = CognitiveLLMProgressionMemory;
    const NAME: &'static str = "progression_memory";
    const SCOPE: TemporalScope = TemporalScope::Historical;

    async fn context(&self, _request: &ContextRequest) -> Result<Self::Context, String> {
        let memory = self.rx.borrow().clone();
        Ok(CognitiveLLMProgressionMemory::from(memory))
    }

    fn subscribe(&self) -> Option<watch::Receiver<()>> {
        let mut rx = self.rx.clone();
        let (tx, out_rx) = watch::channel(());
        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                tx.send(()).inspect_err(|e| tracing::error!("{}", e)).ok();
            }
        });
        Some(out_rx)
    }
}

pub struct ContinuumMemoryProvider {
    pub rx: watch::Receiver<ContinuumMemory>,
}

#[async_trait]
impl ContextProvider for ContinuumMemoryProvider {
    type Context = CognitiveLLMContinuumMemory;
    const NAME: &'static str = "continuum_memory";
    const SCOPE: TemporalScope = TemporalScope::Historical;

    async fn context(&self, _request: &ContextRequest) -> Result<Self::Context, String> {
        let memory = self.rx.borrow().clone();
        Ok(CognitiveLLMContinuumMemory::from(memory))
    }

    fn subscribe(&self) -> Option<watch::Receiver<()>> {
        let mut rx = self.rx.clone();
        let (tx, out_rx) = watch::channel(());
        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                tx.send(()).inspect_err(|e| tracing::error!("{}", e)).ok();
            }
        });
        Some(out_rx)
    }
}

pub struct EpisodicMemoryPlugin<S: RecordStore + StorageConnection> {
    config: EpisodicMemoryConfig,
    llm_executor: synapto_interface::llm::LlmExecutor,
    store: Arc<S>,

    // Context Providers
    pub session_provider: Arc<SessionMemoryProvider>,
    pub progression_provider: Arc<ProgressionMemoryProvider>,
    pub continuum_provider: Arc<ContinuumMemoryProvider>,
    pub session_interaction_rollout_rx: watch::Receiver<Timestamp>,

    // Senders to pass to start
    session_memory_tx: Mutex<Option<watch::Sender<SessionMemory>>>,
    session_interaction_rollout_tx: Mutex<Option<watch::Sender<Timestamp>>>,
    progression_memory_tx: Mutex<Option<watch::Sender<ProgressionMemory>>>,
    continuum_memory_tx: Mutex<Option<watch::Sender<ContinuumMemory>>>,
}

#[async_trait::async_trait]
impl<S: RecordStore + StorageConnection> Plugin for EpisodicMemoryPlugin<S> {
    async fn create(
        context: &synapto_interface::plugin::PluginInitContext<'_>,
    ) -> Result<Self, String> {
        let config: EpisodicMemoryConfig = context.config()?;
        let store = context.store::<S>().await?;
        let (session_memory_tx, session_memory_rx) = watch::channel(SessionMemory::default());
        let (session_interaction_rollout_tx, session_interaction_rollout_rx) =
            watch::channel(Timestamp(i64::MAX));
        let (progression_memory_tx, progression_memory_rx) =
            watch::channel(ProgressionMemory::default());
        let (continuum_memory_tx, continuum_memory_rx) = watch::channel(ContinuumMemory::default());

        let session_provider = Arc::new(SessionMemoryProvider {
            rx: session_memory_rx,
        });
        let progression_provider = Arc::new(ProgressionMemoryProvider {
            rx: progression_memory_rx,
        });
        let continuum_provider = Arc::new(ContinuumMemoryProvider {
            rx: continuum_memory_rx,
        });

        Ok(Self {
            config,
            llm_executor: context.llm_executor().clone(),
            store,
            session_provider,
            progression_provider,
            continuum_provider,
            session_interaction_rollout_rx,
            session_memory_tx: Mutex::new(Some(session_memory_tx)),
            session_interaction_rollout_tx: Mutex::new(Some(session_interaction_rollout_tx)),
            progression_memory_tx: Mutex::new(Some(progression_memory_tx)),
            continuum_memory_tx: Mutex::new(Some(continuum_memory_tx)),
        })
    }

    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R)
    where
        Self: Sized,
    {
        registry.register_interaction_observer(self.clone());
        registry.register_context_provider(self.session_provider.clone());
        registry.register_context_provider(self.progression_provider.clone());
        registry.register_context_provider(self.continuum_provider.clone());
    }
}

#[async_trait]
impl<S: RecordStore + StorageConnection> InteractionObserver for EpisodicMemoryPlugin<S> {
    async fn start(
        &self,
        interaction_rx: mpsc::Receiver<ObservedInteraction>,
    ) -> Result<(), String> {
        let session_memory_tx = self
            .session_memory_tx
            .lock()
            .unwrap_or_else(|e| panic!("Error: {:?}", e))
            .take()
            .ok_or("Already started")?;
        #[allow(clippy::expect_used)]
        let session_interaction_rollout_tx = self
            .session_interaction_rollout_tx
            .lock()
            .unwrap_or_else(|e| panic!("Error: {:?}", e))
            .take()
            .expect("Missing value");
        #[allow(clippy::expect_used)]
        let progression_memory_tx = self
            .progression_memory_tx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e))
            .take()
            .expect("Missing value");
        let continuum_memory_tx = self
            .continuum_memory_tx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e))
            .take()
            .expect("Missing value");

        // 1. Setup internal communication channels between sub-tasks
        let (new_session_tx, new_session_rx) = mpsc::channel::<Session>(10);
        let (new_progression_tx, new_progression_rx) = mpsc::channel::<Progression>(10);

        let config = self.config.clone();
        let store = self.store.clone();
        let llm_executor = self.llm_executor.clone();

        // 2. Spawn background tasks
        tokio::spawn(session_memory_task(
            config.clone(),
            store.clone(),
            interaction_rx,
            session_memory_tx,
            session_interaction_rollout_tx,
            new_session_tx,
            llm_executor.clone(),
        ));

        let config_clone = config.clone();
        let store_clone = store.clone();
        let llm_executor_clone = llm_executor.clone();
        tokio::spawn(async move {
            progression_memory_task(
                config_clone,
                store_clone,
                new_session_rx,
                progression_memory_tx,
                new_progression_tx,
                llm_executor_clone,
            )
            .await;
        });

        tokio::spawn(continuum_memory_task(
            config,
            store,
            new_progression_rx,
            continuum_memory_tx,
            llm_executor,
        ));

        Ok(())
    }
}
