# Configuration Providers

The AI framework uses a `ConfigProvider` trait to decouple the *source* of configuration data (e.g., file system, memory, database, cloud secret manager) from the *business logic* of the application.

Importantly, Configuration Providers do not *only* dictate where the configuration comes from, but they also determine the base `data_dir` for the application. This `data_dir` is exposed on the core configuration object and can be (optionally) used by Storage Providers (like SQLite or Local Storage) as the foundational path for writing physical data to disk.

## How it Works

The configuration system operates on a "Base + Overlay" architecture via Tuple Composition.

1. **Base Configuration (The Provider Chain):** The core accepts a tuple of up to 6 `ConfigProvider` implementations. During the `init` phase, the `init()` method of **every** provider in the tuple is called sequentially. Providers that require access to the base `data_dir` must retrieve it statically through the `DataDirProvider` generic parameter.
2. **JSON Overlay:** The providers are evaluated left-to-right. Each provider returns raw data as a weakly-typed `serde_json::Value` which recursively overlays/merges into the output of the previous provider.
3. **Deserialization (The Core):** The resulting composite JSON object is strictly deserialized into the strongly-typed `Config` or plugin configuration structs. The `Config::data_dir` field relies entirely on being populated by the merged JSON output (e.g., set explicitly in `config.json` or via environment variables).

> [!NOTE]
> **Data Dir Injection via Generics:** Because providers that need file system access receive the base path statically via the `DataDirProvider` generic trait, setting a different `"data_dir"` in your configuration JSON overrides the `Config::data_dir` path for runtime usage (like storage providers), but it does **not** change where `ConfigJson` itself looks for `config.json` at boot time. This guarantees deterministic initialization.

**Strict Validation:**
All core and plugin configuration structs use `#[serde(deny_unknown_fields)]`. If a configuration source (base JSON or environment variable) provides an unknown field, the system will intentionally panic at boot.

### Environment Variable Overrides

By including the built-in `Env` provider at the end of your tuple (or `DotEnv` for `.env` files), environment variables are automatically merged into the base JSON using double underscores (`__`) to represent object nesting.

> [!WARNING]
> **Strict Case-Sensitivity:**
> Environment variable resolution is strictly case-sensitive. The subsystem prefixes are uppercase (`SYNAPTO__`, `PLUGINS__`, `STORAGE__`, `CREDENTIALS__`), but the interpolated crate names, type names, and field names MUST match the exact casing used in Rust code. Because structs derive `#[serde(deny_unknown_fields)]`, any case mismatch will fail deserialization.

*   **Core Config:** Prefix `SYNAPTO__<field_name>` (fields are exact `snake_case`).
    *   Example: `SYNAPTO__cognitive__model="gemini-1.5-pro"` overrides `Config::cognitive.model`.
*   **Plugin Config:** Prefix `SYNAPTO__PLUGINS__<crate_name>__<PluginTypeName>__<field_name>`:
    *   `<crate_name>`: Crate identifier with `-` replaced by `_` (`snake_case`).
    *   `<PluginTypeName>`: Exact Rust struct name (`PascalCase`).
    *   `<field_name>`: Target configuration struct field (`snake_case`).
    *   Example: `SYNAPTO__PLUGINS__synapto_plugin_google_chat__GoogleChatPlugin__api_key="secret"` overrides `GoogleChatConfig::api_key`.
*   **Credentials Config:** Prefix `SYNAPTO__CREDENTIALS__<crate_name>__<ProviderTypeName>__<field_name>`:
    *   Example: `SYNAPTO__CREDENTIALS__synapto_credentials_typesafe__TypeSafeCredentials__api_key="secret"` overrides `TypeSafeCredentialsConfig::api_key`.
*   **Decision Config:** Prefix `SYNAPTO__DECISION__<crate_name>__<ProviderTypeName>__<field_name>`:
    *   Example: `SYNAPTO__DECISION__synapto_decision_typesafe__TypeSafeDecision__model="jev-latest"` overrides `TypeSafeDecisionConfig::model`.
*   **Storage Config:** Prefix `SYNAPTO__STORAGE__<crate_name>__<StorageTypeName>__<field_name>`:
    *   Example: `SYNAPTO__STORAGE__synapto_storage_firestore__FirestoreStorage__project_id="my-project"`.

## Existing Providers

- **`ConfigJson`:** Reads `config.json` from the `data_dir` initialized path. Plugin configs are nested inside the `plugins` field by crate name and plugin type name. Storage configs are nested in `storage`, credentials in `credentials`, and decision providers in `decision`.
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
    fn init() -> Self {
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
Synapto::builder()
    .configs::<VaultConfigProvider>() // Exclusively loads from Vault
    .storage::<Storage>()
    .prompt::<prompt_file::FilePromptProvider<DataDir>>()
    .plugins::<(MyPlugin,)>()
    .run()
    .await
```

If you do want layered overrides (File -> .env -> Real Env Variables):

```rust,ignore
Synapto::builder()
    .configs::<(ConfigJson<DataDir>, DotEnv, Env)>()
    .storage::<Storage>()
    .prompt::<prompt_file::FilePromptProvider<DataDir>>()
    .plugins::<(MyPlugin,)>()
    .run()
    .await
```