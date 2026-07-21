#![doc = include_str!("context.md")]

use crate::llm::LLMSafe;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalScope {
    Historical,
    Current,
    Prospective,
}

#[derive(Clone, Debug, serde :: Serialize, serde :: Deserialize)]
pub struct ContextInteraction {
    pub peer_input: Option<String>,
    pub cognitive_reasoning: Option<String>,
    pub cognitive_output: Option<String>,
}

#[derive(Clone, Debug, serde :: Serialize, serde :: Deserialize, Default)]
pub struct ContextRequest {
    #[doc = " The sliding window of recent conversational flow."]
    #[doc = " Used by plugins to perform Associative RAG."]
    #[doc = " An empty list implies a request for the unfiltered, baseline state."]
    pub recent_interactions: Vec<ContextInteraction>,
    pub initial_run: bool,
}

#[async_trait::async_trait]
pub trait ContextProvider: Send + Sync + 'static {
    type Context: schemars::JsonSchema + serde::Serialize + LLMSafe + Send + Sync + 'static;
    #[doc = " Declarative compile-time semantic key (e.g., \"state\", \"active_tasks\")"]
    const NAME: &'static str;
    #[doc = " The dimension this context belongs to"]
    const SCOPE: TemporalScope;
    #[doc = " Provide the JSON-serializable context view, filtered associatively via ContextRequest"]
    async fn context(&self, request: &ContextRequest) -> Result<Self::Context, String>;
    #[doc = " Decentralized Wakeup Signal:"]
    #[doc = " Returns a receiver that signals when this specific context mutates."]
    fn subscribe(&self) -> Option<tokio::sync::watch::Receiver<()>> {
        None
    }
}

#[async_trait::async_trait]
pub trait ErasedContextProvider: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn scope(&self) -> TemporalScope;
    fn schema(&self) -> schemars::Schema;
    async fn erased_context(&self, request: &ContextRequest) -> Result<serde_json::Value, String>;
    fn subscribe(&self) -> Option<tokio::sync::watch::Receiver<()>>;
}

#[async_trait::async_trait]
impl<T> ErasedContextProvider for T
where
    T: ContextProvider,
{
    fn name(&self) -> &'static str {
        <T as ContextProvider>::NAME
    }
    fn scope(&self) -> TemporalScope {
        <T as ContextProvider>::SCOPE
    }
    fn schema(&self) -> schemars::Schema {
        schemars::schema_for!(<T as ContextProvider>::Context)
    }
    async fn erased_context(&self, request: &ContextRequest) -> Result<serde_json::Value, String> {
        let view = <T as ContextProvider>::context(self, request).await?;
        serde_json::to_value(view).map_err(|e| e.to_string())
    }
    fn subscribe(&self) -> Option<tokio::sync::watch::Receiver<()>> {
        <T as ContextProvider>::subscribe(self)
    }
}

pub struct ContextRegistryBuilder {
    providers: std::sync::RwLock<Vec<std::sync::Arc<dyn ErasedContextProvider>>>,
    change_tx: tokio::sync::watch::Sender<()>,
    change_rx: tokio::sync::watch::Receiver<()>,
}

impl Default for ContextRegistryBuilder {
    fn default() -> Self {
        let (change_tx, change_rx) = tokio::sync::watch::channel(());
        Self {
            providers: std::sync::RwLock::new(Vec::new()),
            change_tx,
            change_rx,
        }
    }
}

impl ContextRegistryBuilder {
    pub async fn gather_contexts(
        &self,
        request: &ContextRequest,
    ) -> std::collections::BTreeMap<String, serde_json::Value> {
        let providers: Vec<_> = self
            .providers
            .read()
            .unwrap_or_else(|e| panic!("Providers lock poisoned: {:?}", e))
            .clone();

        let futures = providers.into_iter().map(|provider| {
            let request = request.clone();
            async move {
                let name = provider.name().to_string();
                let res = provider.erased_context(&request).await;
                (name, res)
            }
        });

        let results = futures::future::join_all(futures).await;

        let mut contexts = std::collections::BTreeMap::new();
        for (name, res) in results {
            if let Ok(val) = res {
                contexts.insert(name, val);
            }
        }
        contexts
    }

    pub fn register<T>(&self, provider: T)
    where
        T: ErasedContextProvider + 'static,
    {
        let provider_arc: std::sync::Arc<dyn ErasedContextProvider> = std::sync::Arc::new(provider);
        self.register_erased(provider_arc);
    }

    pub fn is_empty(&self) -> bool {
        self.providers
            .read()
            .unwrap_or_else(|e| panic!("Providers lock poisoned: {:?}", e))
            .is_empty()
    }

    pub fn register_erased(&self, provider: std::sync::Arc<dyn ErasedContextProvider>) {
        self.providers
            .write()
            .unwrap_or_else(|e| panic!("Failed to acquire write lock on providers: {:?}", e))
            .push(provider.clone());
        if let Some(mut sub_rx) = provider.subscribe() {
            let change_tx = self.change_tx.clone();
            tokio::spawn(async move {
                while sub_rx.changed().await.is_ok() {
                    change_tx
                        .send(())
                        .inspect_err(|e| tracing::error!("{}", e))
                        .ok();
                }
            });
        }
    }
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<()> {
        self.change_rx.clone()
    }
}

#[derive(Default)]
pub struct ContextRegistries {
    pub historical: ContextRegistryBuilder,
    pub current: ContextRegistryBuilder,
    pub prospective: ContextRegistryBuilder,
}

impl ContextRegistries {
    pub fn subscribe(&self, scope: TemporalScope) -> tokio::sync::watch::Receiver<()> {
        match scope {
            TemporalScope::Historical => self.historical.subscribe(),
            TemporalScope::Current => self.current.subscribe(),
            TemporalScope::Prospective => self.prospective.subscribe(),
        }
    }
}
