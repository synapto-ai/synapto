# Configuration Providers

The AI framework uses a `ConfigProvider` trait to decouple the *source* of configuration data (e.g., file system, memory, database, cloud secret manager) from the *business logic* of the application.

Importantly, Configuration Providers do not *only* dictate where the configuration comes from, but they also determine the base `data_dir` for the application. This `data_dir` is exposed on the core configuration object and can be (optionally) used by Storage Providers (like SQLite or Local Storage) as the foundational path for writing physical data to disk.

## How it Works

The configuration system operates on a "Base + Overlay" architecture via Tuple Composition.

1. **Base Configuration (The Provider Chain):** The core accepts a tuple of up to 6 `ConfigProvider` implementations. During the `init` phase, the base `data_dir` (resolved by the `DataDirProvider` parameter) is cloned and passed identically to the `init()` method of **every** provider in the tuple in parallel.
2. **JSON Overlay:** The providers are evaluated left-to-right. Each provider returns raw data as a weakly-typed `serde_json::Value` which recursively overlays/merges into the output of the previous provider.
3. **Data Directory Passthrough:** As a final step before parsing, the core checks if any provider explicitly overrode the `"data_dir"` key in the JSON. If not, it automatically injects the original base `data_dir` into the JSON payload (Passthrough).
4. **Deserialization (The Core):** The resulting composite JSON object is strictly deserialized into the strongly-typed `Config` or plugin configuration structs.

> [!NOTE]
> **Data Dir Initialization is Parallel, not Chained:** Because all providers receive the *exact same* initial `data_dir` during the `init` phase, overriding the `"data_dir"` in a JSON config (e.g., via `ConfigJson`) will successfully change the application's final output directory, but it will *not* change the initialization path fed to downstream providers in the tuple. This guarantees deterministic behavior during boot.

**Strict Validation:**
All core and plugin configuration structs use `#[serde(deny_unknown_fields)]`. If a configuration source (base JSON or environment variable) provides an unknown field, the system will intentionally panic at boot.

### Environment Variable Overrides

By including the built-in `Env` provider at the end of your tuple, environment variables are automatically merged into the base JSON using double underscores (`__`) to represent object nesting.

*   **Core Config:** Prefix `SYNAPTO__`.
    *   Example: `SYNAPTO__COGNITIVE__MODEL="gemini-1.5-pro"` overrides `Config::cognitive.model`.
*   **Plugin Config:** Prefix `SYNAPTO__PLUGINS__<CRATE_NAME>__<PLUGIN_TYPE_NAME>__` (Crate and Plugin name are uppercase, hyphens/dots replaced with underscores).
    *   Example: For `google-chat`, `SYNAPTO__PLUGINS__GOOGLE_CHAT__GOOGLECHATPLUGIN__API_KEY="secret"` overrides `GoogleChatConfig::api_key`.

## Existing Providers

- **`ConfigJson`:** Reads `config.json` from the `data_dir` initialized path. Plugin configs are nested inside the `plugins` field by crate name and plugin type name. Storage configs are nested in `storage`.
- **`DotEnv`:** Reads variables from a `.env` file and translates `SYNAPTO__` prefixed variables into nested JSON configuration overrides exactly like `Env`, but without polluting or reading the global process environment variables.
- **`Env`:** Translates `SYNAPTO__` prefixed environment variables directly into deeply-nested JSON configuration overrides.
- **`EphemeralConfigProvider`:** An ephemeral provider used primarily for tests. Returns an empty base JSON but automatically provisions and assigns a temporary directory to `data_dir` to ensure storage providers have a safe, ephemeral location to write files during tests. Relies entirely on environment variable overrides for other settings.

## How to Create a New Provider

To create a new provider (e.g., to load configurations from a PostgreSQL database or AWS Parameter Store), you only need to implement four methods returning raw JSON. The core handles the rest.

### 1. Create the Struct and Implement `ConfigProvider`

```rust,ignore
use synapto::config::ConfigProvider;
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct DatabaseConfigProvider {
    // Internal state (e.g., db connection pool)
}

impl ConfigProvider for DatabaseConfigProvider {
    /// 1. Initialize your connection or state
    fn init(_data_dir: PathBuf) -> Self {
        // e.g., connect to database using standard env vars like DATABASE_URL
        Self {}
    }

    /// 2. Fetch the Core Configuration
    fn load_core_config(&self) -> Value {
        // Fetch raw JSON from your medium.
        // Example: SELECT config_json FROM core_configs WHERE id = 1;
        json!({
            "barge_in": true,
            "data_dir": "/var/lib/my-ai-app" // Dictate the base data directory
        })
    }

    /// 3. Fetch the Plugin Configuration
    fn load_plugin_config(&self, _crate_name: &str, plugin_name: &str) -> Value {
        // Fetch raw JSON for the specific plugin.
        // Example: SELECT config_json FROM plugin_configs WHERE name = $1;
        match plugin_name {
            "my-plugin" => json!({ "api_key": "db-secret" }),
            _ => json!({}) // Return empty object if no config exists
        }
    }

    /// 4. Fetch the Storage Configuration
    fn load_storage_config(&self, _crate_name: &str, _storage_name: &str) -> Value {
        json!({})
    }
}
```

### 2. Composing Providers

Instead of opting out of environment variables directly, you can strictly control them by modifying the `ConfigProvider` tuple in your composition root:

```rust,ignore
// Secure deployment without environment variable overrides:
Synapto::<
    datadir_cwd::CurrentWorkDir,
    VaultConfigProvider, // Exclusively loads from Vault
    prompt_file::FilePromptProvider
>::run::<(MyPlugin,)>().await
```

If you do want layered overrides (File -> .env -> Real Env Variables):

```rust,ignore
Synapto::<
    datadir_cwd::CurrentWorkDir,
    (ConfigJson, DotEnv, Env),
    prompt_file::FilePromptProvider
>::run::<(MyPlugin,)>().await
```