pub struct DotEnv;

impl crate::config::ConfigProvider for DotEnv {
    fn init(_data_dir: std::path::PathBuf) -> Self {
        dotenvy::dotenv().ok();
        DotEnv
    }

    fn load_core_config(&self) -> serde_json::Value {
        // DotEnv only populates standard environment variables during `init`.
        // The actual reading is deferred to `Env` if it's in the chain.
        // If someone uses DotEnv without Env, they just get environment variables in process,
        // but no overrides in JSON. It's best used as `(..., DotEnv, Env)`.
        serde_json::Value::Object(serde_json::Map::new())
    }

    fn load_plugin_config(&self, _crate_name: &str, _plugin_type_name: &str) -> serde_json::Value {
        serde_json::Value::Object(serde_json::Map::new())
    }

    fn load_storage_config(
        &self,
        _crate_name: &str,
        _storage_type_name: &str,
    ) -> serde_json::Value {
        serde_json::Value::Object(serde_json::Map::new())
    }
}
