# Configuration Providers

The AI framework uses a `ConfigProvider` trait to decouple the *source* of configuration data (e.g., file system, memory, database, cloud secret manager) from the *business logic* of the application.

Importantly, Configuration Providers do not *only* dictate where the configuration comes from, but they also determine the base `data_dir` for the application. This `data_dir` is exposed on the core configuration object and can be (optionally) used by Storage Providers (like SQLite or Local Storage) as the foundational path for writing physical data to disk.

## How it Works

The configuration system operates on a two-step "Base + Overlay" architecture.

1. **Base Configuration (The Provider):** The concrete `ConfigProvider` implementation is strictly responsible for fetching raw data from its medium and returning it as a weakly-typed `serde_json::Value`.
2. **Overlay & Deserialization (The Core):** The `ai_core` automatically overlays environment variables on top of the base JSON, and then strictly deserializes it into the strongly-typed `Config` or plugin configuration structs.

**Strict Validation:**
All core and plugin configuration structs use `#[serde(deny_unknown_fields)]`. If a configuration source (base JSON or environment variable) provides an unknown field, the system will intentionally panic at boot.

### Environment Variable Overrides

Environment variables are automatically managed by the core. They deeply merge into the base JSON using double underscores (`__`) to represent object nesting.

*   **Core Config:** Prefix `AI__`.
    *   Example: `AI__COGNITIVE__MODEL="gemini-1.5-pro"` overrides `Config::cognitive.model`.
*   **Plugin Config:** Prefix `AI__PLUGINS__<CRATE_NAME>__<PLUGIN_TYPE_NAME>__` (Crate and Plugin name are uppercase, hyphens/dots replaced with underscores).
    *   Example: For `google-chat`, `AI__PLUGINS__GOOGLE_CHAT__GOOGLECHATPLUGIN__API_KEY="secret"` overrides `GoogleChatConfig::api_key`.

## Existing Providers

- **`config-file` (`FileConfigProvider`):** Reads `config.json` from a profile directory within `AI_DATA_DIR` or the OS local data directory. Sets the `data_dir` to the exact profile directory it read from. Plugin configs are nested inside the `plugins` field by crate name and plugin type name.
- **`config-ephemeral` (`EphemeralConfigProvider`):** An ephemeral provider used primarily for tests. Returns an empty base JSON but automatically provisions and assigns a temporary directory to `data_dir` to ensure storage providers have a safe, ephemeral location to write files during tests. Relies entirely on environment variable overrides for other settings.

## How to Create a New Provider

To create a new provider (e.g., to load configurations from a PostgreSQL database or AWS Parameter Store), you only need to implement three methods returning raw JSON. The core handles the rest.

### 1. Create the Struct and Implement `ConfigProvider`

```rust
use ai_core::config::ConfigProvider;
use serde_json::{json, Value};

pub struct DatabaseConfigProvider {
    // Internal state (e.g., db connection pool)
}

impl ConfigProvider for DatabaseConfigProvider {
    /// 1. Initialize your connection or state
    fn init() -> Self {
        // e.g., connect to database using standard env vars like DATABASE_URL
        Self {}
    }

    /// 2. Fetch the Core Configuration
    fn load_base_core_config(&self) -> Value {
        // Fetch raw JSON from your medium. 
        // Example: SELECT config_json FROM core_configs WHERE id = 1;
        json!({
            "barge_in": true,
            "data_dir": "/var/lib/my-ai-app" // Dictate the base data directory
        })
    }

    /// 3. Fetch the Plugin Configuration
    fn load_base_plugin_config(&self, plugin_name: &str) -> Value {
        // Fetch raw JSON for the specific plugin.
        // Example: SELECT config_json FROM plugin_configs WHERE name = $1;
        match plugin_name {
            "my-plugin" => json!({ "api_key": "db-secret" }),
            _ => json!({}) // Return empty object if no config exists
        }
    }
}
```

### 2. Opting Out of Environment Variables

The environment variable merging logic is implemented as **default methods** on the `ConfigProvider` trait (`get_core_config` and `get_plugin_config`).

If you are building a highly secure provider (e.g., `HashicorpVaultProvider`) and you explicitly want to **disable** the ability for environment variables to override your fetched secrets, simply override the default methods:

```rust
impl ConfigProvider for VaultConfigProvider {
    fn init() -> Self { Self {} }
    fn load_base_core_config(&self) -> Value { /* ... */ }
    fn load_base_plugin_config(&self, _name: &str) -> Value { /* ... */ }

    // OVERRIDE default behavior to bypass env merging!
    fn get_core_config(&self) -> ai_core::config::Config {
        let val = self.load_base_core_config();
        serde_json::from_value(val).expect("Invalid core config from Vault")
    }

    fn get_plugin_config<T: serde::de::DeserializeOwned>(&self, name: &str) -> T {
        let val = self.load_base_plugin_config(name);
        serde_json::from_value(val).unwrap_or_else(|e| {
            panic!("Invalid config for plugin {}: {}", name, e)
        })
    }
}
```
