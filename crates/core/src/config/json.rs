use serde_json::Value;
use synapto_interface::data_dir::DataDirProvider;

pub struct ConfigJson<P: DataDirProvider> {
    config: Value,
    _marker: std::marker::PhantomData<P>,
}

impl<P: DataDirProvider> crate::config::ConfigProvider for ConfigJson<P> {
    fn init() -> Self {
        let base_dir = P::get_data_dir();
        let config_path = base_dir.join("config.json");

        let config = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .unwrap_or_else(|e| panic!("Failed to read config file {:?}: {}", config_path, e));
            serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse config file {:?}: {}", config_path, e))
        } else {
            Value::Object(serde_json::Map::new())
        };

        Self {
            config,
            _marker: std::marker::PhantomData,
        }
    }

    fn load_core_config(&self) -> Value {
        let mut core_config = self.config.clone();
        if let Some(obj) = core_config.as_object_mut() {
            obj.remove("plugins");
            obj.remove("storage");
            obj.remove("credentials");
            obj.remove("decision");
            obj.remove("llm");
        };
        core_config
    }

    fn describe_core_location(&self) -> Option<String> {
        Some("JSON root configuration".to_string())
    }

    fn load_plugin_config(&self, crate_name: &str, plugin_type_name: &str) -> Value {
        self.config
            .get("plugins")
            .and_then(|p| p.get(crate_name))
            .and_then(|c| c.get(plugin_type_name))
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
    }

    fn describe_plugin_location(&self, crate_name: &str, plugin_type_name: &str) -> Option<String> {
        Some(format!(
            "JSON key 'plugins.{}.{}'",
            crate_name, plugin_type_name
        ))
    }

    fn load_storage_config(&self, crate_name: &str, storage_type_name: &str) -> Value {
        self.config
            .get("storage")
            .and_then(|s| s.get(crate_name))
            .and_then(|c| c.get(storage_type_name))
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
    }

    fn describe_storage_location(
        &self,
        crate_name: &str,
        storage_type_name: &str,
    ) -> Option<String> {
        Some(format!(
            "JSON key 'storage.{}.{}'",
            crate_name, storage_type_name
        ))
    }

    fn load_credentials_config(&self, crate_name: &str, provider_type_name: &str) -> Value {
        self.config
            .get("credentials")
            .and_then(|c| c.get(crate_name))
            .and_then(|p| p.get(provider_type_name))
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
    }

    fn describe_credentials_location(
        &self,
        crate_name: &str,
        provider_type_name: &str,
    ) -> Option<String> {
        Some(format!(
            "JSON key 'credentials.{}.{}'",
            crate_name, provider_type_name
        ))
    }

    fn load_decision_config(&self, crate_name: &str, provider_type_name: &str) -> Value {
        self.config
            .get("decision")
            .and_then(|d| d.get(crate_name))
            .and_then(|p| p.get(provider_type_name))
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
    }

    fn describe_decision_location(
        &self,
        crate_name: &str,
        provider_type_name: &str,
    ) -> Option<String> {
        Some(format!(
            "JSON key 'decision.{}.{}'",
            crate_name, provider_type_name
        ))
    }

    fn load_llm_config(&self, crate_name: &str, provider_type_name: &str) -> Value {
        self.config
            .get("llm")
            .and_then(|l| l.get(crate_name))
            .and_then(|p| p.get(provider_type_name))
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
    }

    fn describe_llm_location(&self, crate_name: &str, provider_type_name: &str) -> Option<String> {
        Some(format!(
            "JSON key 'llm.{}.{}'",
            crate_name, provider_type_name
        ))
    }
}
