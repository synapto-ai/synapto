## Dynamic Actions with `Command`

### When to Use It

Use `Command` when your custom plugin needs to expose executable tools or actions (e.g., controlling a device, triggering a notification, updating state) that the LLM can dynamically choose to invoke inside its structured outputs.

### How to Use It (Example)

Define your deserializable argument DTO, implement `LLMSafe`, and implement `Command`:

```rust,ignore
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use synapto_interface::core::{Command};
use synapto_interface::llm::LLMSafe;

#[derive(Deserialize, JsonSchema, Clone, Debug, LLMSafe)]
pub struct AdjustThermostatArgs {
    pub target_temp_celsius: f32,
}

pub struct AdjustThermostatCommand {
    hardware_client: Arc<MyThermostatClient>,
}

#[async_trait]
impl Command for AdjustThermostatCommand {
    type Arguments = AdjustThermostatArgs;

    // The unique action identifier exposed to the LLM
    const NAME: &'static str = "adjust_thermostat";

    async fn execute(&self, args: Self::Arguments) -> Result<(), String> {
        tracing::info!("Adjusting room temperature to: {}°C", args.target_temp_celsius);
        self.hardware_client
            .set_temperature(args.target_temp_celsius)
            .await
            .map_err(|e| e.to_string())
    }
}
```

To expose this tool to the LLM, register it inside your command registry builder at startup:

```rust,ignore
command_registry.register(AdjustThermostatCommand::new(hardware_client));
```