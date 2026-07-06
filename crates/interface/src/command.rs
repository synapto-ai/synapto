use crate::llm::LLMSafe;

#[async_trait::async_trait]
pub trait Command: Send + Sync + 'static {
    type Arguments: schemars::JsonSchema
        + serde::de::DeserializeOwned
        + LLMSafe
        + Send
        + Sync
        + 'static;
    const NAME: &'static str;
    async fn execute(&self, args: Self::Arguments) -> Result<(), String>;
}

#[async_trait::async_trait]
pub trait ErasedCommand: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn schema(&self) -> schemars::Schema;
    async fn erased_execute(&self, args: serde_json::Value) -> Result<(), String>;
}

#[async_trait::async_trait]
impl<T> ErasedCommand for T
where
    T: Command,
{
    fn name(&self) -> &'static str {
        <T as Command>::NAME
    }
    fn schema(&self) -> schemars::Schema {
        schemars::schema_for!(<T as Command>::Arguments)
    }
    async fn erased_execute(&self, args: serde_json::Value) -> Result<(), String> {
        let parsed_args = serde_json::from_value(args).map_err(|e| e.to_string())?;
        <T as Command>::execute(self, parsed_args).await
    }
}

#[derive(Default)]
pub struct CommandRegistryBuilder {
    pub commands:
        std::sync::RwLock<std::collections::HashMap<String, std::sync::Arc<dyn ErasedCommand>>>,
}

impl CommandRegistryBuilder {
    pub fn register<T>(&self, command: T)
    where
        T: ErasedCommand + 'static,
    {
        let command_arc: std::sync::Arc<dyn ErasedCommand> = std::sync::Arc::new(command);
        self.register_erased(command_arc);
    }
    pub fn register_erased(&self, command: std::sync::Arc<dyn ErasedCommand>) {
        self.commands
            .write()
            .unwrap_or_else(|e| panic!("Failed to acquire write lock on commands: {:?}", e))
            .insert(command.name().to_string(), command);
    }
}
