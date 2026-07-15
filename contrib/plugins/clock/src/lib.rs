use async_trait::async_trait;
use serde::Serialize;
use std::sync::Arc;
use synapto_interface::llm::LLMSafe;
use synapto_interface::context::{ContextProvider, ContextRequest, TemporalScope};
use synapto_interface::plugin::{Plugin, PluginRegistry};

#[derive(Serialize, schemars::JsonSchema, Clone, Debug, LLMSafe)]
pub struct ClockContext {
    /// Number of non-leap seconds since January 1, 1970 0:00:00 UTC (aka “UNIX timestamp”).
    pub current_timestamp: i64,
}

pub struct ClockContextProvider;

#[async_trait]
impl ContextProvider for ClockContextProvider {
    type Context = ClockContext;
    const NAME: &'static str = "clock";
    const SCOPE: TemporalScope = TemporalScope::Current;

    async fn context(&self, _request: &ContextRequest) -> Result<<Self as ContextProvider>::Context, String> {
        Ok(ClockContext {
            current_timestamp: chrono::Utc::now().timestamp(),
        })
    }
}

#[derive(serde::Deserialize)]
pub struct ClockConfig {}

pub struct ClockPlugin {
    provider: Arc<ClockContextProvider>,
}

#[async_trait::async_trait]
impl Plugin for ClockPlugin {
    async fn create(_context: &synapto_interface::plugin::PluginInitContext<'_>) -> Result<Self, String> {
        let provider = Arc::new(ClockContextProvider {});
        Ok(Self { provider })
    }

    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R)
    where
        Self: Sized,
    {
        registry.register_context_provider(self.provider.clone());
    }
}
