pub trait ConfigProvider: Send + Sync + Sized + 'static {
    /// Initializes the config provider.
    /// Provider-specific contexts (like use cases) must be handled internally
    /// by the implementing type (e.g., via trait markers or environment variables).
    fn init() -> Self;

    /// Returns the BASE core configuration BEFORE environment variables are applied.
    /// Providers should return the raw JSON Value representing their stored config.
    fn load_base_core_config(&self) -> serde_json::Value;

    /// Returns the BASE plugin configuration BEFORE environment variables are applied.
    fn load_base_plugin_config(
        &self,
        crate_name: &str,
        plugin_type_name: &str,
    ) -> serde_json::Value;

    /// Returns the BASE storage configuration BEFORE environment variables are applied.
    fn load_base_storage_config(
        &self,
        crate_name: &str,
        storage_type_name: &str,
    ) -> serde_json::Value;

    /// Returns the core configuration struct.
    /// Default implementation applies "SYNAPTO__" prefixed environment variables automatically.
    fn get_core_config(&self) -> crate::config::Config {
        let mut val = self.load_base_core_config();
        crate::config::env::merge_env_overrides(&mut val, "SYNAPTO__");

        // Ensure the 'plugins' key is removed before parsing,
        // since AI__PLUGINS__ env vars might have re-injected it.
        if let Some(obj) = val.as_object_mut() {
            obj.remove("plugins");
        }

        serde_json::from_value(val)
            .unwrap_or_else(|e| panic!("Failed to parse core configuration: {}", e))
    }

    /// Retrieves the raw configuration value for a specific plugin before deserialization.
    /// Default implementation applies "SYNAPTO__PLUGINS__<CRATE_NAME>__<PLUGIN_TYPE_NAME>__" prefixed environment variables automatically.
    fn get_plugin_config_value(
        &self,
        crate_name: &str,
        plugin_type_name: &str,
    ) -> serde_json::Value {
        let mut val = self.load_base_plugin_config(crate_name, plugin_type_name);
        let prefix = format!(
            "SYNAPTO__PLUGINS__{}__{}__",
            crate_name.to_uppercase().replace(['-', '.'], "_"),
            plugin_type_name.to_uppercase().replace(['-', '.'], "_")
        );
        crate::config::env::merge_env_overrides(&mut val, &prefix);
        val
    }

    /// Retrieves and deserializes the configuration for a specific plugin.
    /// Default implementation applies "SYNAPTO__PLUGINS__<CRATE_NAME>__<PLUGIN_TYPE_NAME>__" prefixed environment variables automatically.
    fn get_plugin_config<T: serde::de::DeserializeOwned>(
        &self,
        crate_name: &str,
        plugin_type_name: &str,
    ) -> T {
        let val = self.get_plugin_config_value(crate_name, plugin_type_name);
        serde_json::from_value(val).unwrap_or_else(|e| {
            panic!(
                "Failed to parse config for plugin '{}::{}': {}",
                crate_name, plugin_type_name, e
            )
        })
    }

    /// Retrieves the configuration value for a specific storage provider.
    /// Default implementation applies "SYNAPTO__STORAGE__<CRATE_NAME>__<STORAGE_TYPE_NAME>__" prefixed environment variables automatically.
    fn get_storage_config(&self, crate_name: &str, storage_type_name: &str) -> serde_json::Value {
        let mut val = self.load_base_storage_config(crate_name, storage_type_name);
        let prefix = format!(
            "SYNAPTO__STORAGE__{}__{}__",
            crate_name.to_uppercase().replace(['-', '.'], "_"),
            storage_type_name.to_uppercase().replace(['-', '.'], "_")
        );
        crate::config::env::merge_env_overrides(&mut val, &prefix);
        val
    }
}
