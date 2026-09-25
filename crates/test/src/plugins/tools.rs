use std::sync::Arc;
use synapto_interface::context::ContextRequest;
use synapto_interface::plugin::PluginInitContext;
use synapto_interface::plugin::{Plugin, PluginRegistry};
use synapto_interface::tool::Tool;

use synapto_interface::llm::LLMSafe;

pub struct MockSlowReadPlugin;

#[async_trait::async_trait]
impl Plugin for MockSlowReadPlugin {
    async fn create(_context: &PluginInitContext<'_>) -> Result<Self, String> {
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

pub struct MockChainedToolsPlugin;

#[async_trait::async_trait]
impl Plugin for MockChainedToolsPlugin {
    async fn create(_context: &PluginInitContext<'_>) -> Result<Self, String> {
        Ok(Self)
    }

    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R) {
        registry.register_tool(MockLookupKeyTool);
        registry.register_tool(MockStoreKeyTool);
    }
}

pub struct MockLookupKeyTool;

#[derive(serde::Deserialize, schemars::JsonSchema, LLMSafe)]
pub struct MockLookupKeyArgs {}

#[async_trait::async_trait]
impl Tool for MockLookupKeyTool {
    type Arguments = MockLookupKeyArgs;
    const NAME: &'static str = "mock_lookup_key";
    const DESCRIPTION: &'static str =
        "Retrieves a security access key required to perform secure operations.";

    async fn execute(
        &self,
        _ctx_request: &ContextRequest,
        _args: Self::Arguments,
    ) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "security_key": "ALPHA-7789-SECURE"
        }))
    }
}

pub struct MockStoreKeyTool;

#[derive(serde::Deserialize, schemars::JsonSchema, LLMSafe)]
pub struct MockStoreKeyArgs {
    #[schemars(description = "The security key obtained from mock_lookup_key")]
    pub security_key: String,
}

#[async_trait::async_trait]
impl Tool for MockStoreKeyTool {
    type Arguments = MockStoreKeyArgs;
    const NAME: &'static str = "mock_store_key";
    const DESCRIPTION: &'static str =
        "Stores the retrieved security access key into the secure registry.";

    async fn execute(
        &self,
        _ctx_request: &ContextRequest,
        args: Self::Arguments,
    ) -> Result<serde_json::Value, String> {
        if args.security_key == "ALPHA-7789-SECURE" {
            Ok(serde_json::json!({
                "status": "success",
                "confirmation_code": "KEY-STORED-SUCCESSFULLY"
            }))
        } else {
            Err(format!("Invalid security key: {}", args.security_key))
        }
    }
}
