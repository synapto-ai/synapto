use std::sync::Arc;
use synapto_interface::context::ContextRequest;
use synapto_interface::plugin::PluginContext;
use synapto_interface::plugin::{Plugin, PluginRegistry};
use synapto_interface::tool::Tool;

use synapto_interface::llm::LLMSafe;

pub struct MockSlowReadPlugin;

#[async_trait::async_trait]
impl Plugin for MockSlowReadPlugin {
    async fn create(_context: PluginContext) -> Result<Self, String> {
        Ok(Self)
    }

    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R) {
        registry.register_tool(MockSlowReadTool);
    }
}

pub struct MockSlowReadTool;

#[derive(serde::Deserialize, schemars::JsonSchema, LLMSafe)]
pub struct MockSlowReadArgs {}

#[async_trait::async_trait]
impl Tool for MockSlowReadTool {
    type Arguments = MockSlowReadArgs;
    const NAME: &'static str = "mock_slow_read";
    const DESCRIPTION: &'static str =
        "A mock slow tool that reads a document and returns its content after a delay.";

    async fn execute(
        &self,
        _ctx_request: &ContextRequest,
        _args: Self::Arguments,
    ) -> Result<serde_json::Value, String> {
        tokio::time::sleep(std::time::Duration::from_millis(5000)).await;
        Ok(serde_json::json!({
            "content": "The architect of the platform is Alice. Mentoring is available on Tuesdays."
        }))
    }
}
