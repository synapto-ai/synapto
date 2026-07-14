#![doc = include_str!("provider.md")]

pub trait ConfigProvider: Send + Sync + Sized + 'static {
    /// Initializes the config provider.
    /// Provider-specific contexts (like use cases) must be handled internally
    /// by the implementing type (e.g., via trait markers or environment variables).
    fn init(data_dir: std::path::PathBuf) -> Self;

    /// Returns the core configuration BEFORE environment variables are applied (if not using Env).
    /// Providers should return the raw JSON Value representing their stored config.
    fn load_core_config(&self) -> serde_json::Value;

    /// Returns the plugin configuration.
    fn load_plugin_config(&self, crate_name: &str, plugin_type_name: &str) -> serde_json::Value;

    /// Returns the storage configuration.
    fn load_storage_config(&self, crate_name: &str, storage_type_name: &str) -> serde_json::Value;

    /// Returns the core configuration struct.
    fn get_core_config(&self, base_data_dir: std::path::PathBuf) -> crate::config::Config {
        let mut val = self.load_core_config();

        if let Some(obj) = val.as_object_mut() {
            // Ensure the 'plugins' key is removed before parsing
            obj.remove("plugins");

            // DEFAULT PASSTHROUGH:
            // If no provider overrode "data_dir" in the JSON, inject the base one
            if !obj.contains_key("data_dir") {
                obj.insert(
                    "data_dir".to_string(),
                    serde_json::Value::String(base_data_dir.to_string_lossy().into_owned()),
                );
            }
        } else {
            let mut new_obj = serde_json::Map::new();
            new_obj.insert(
                "data_dir".to_string(),
                serde_json::Value::String(base_data_dir.to_string_lossy().into_owned()),
            );
            val = serde_json::Value::Object(new_obj);
        }

        serde_json::from_value(val)
            .unwrap_or_else(|e| panic!("Failed to parse core configuration: {}", e))
    }

    /// Retrieves the raw configuration value for a specific plugin before deserialization.
    fn get_plugin_config_value(
        &self,
        crate_name: &str,
        plugin_type_name: &str,
    ) -> serde_json::Value {
        self.load_plugin_config(crate_name, plugin_type_name)
    }

    /// Retrieves and deserializes the configuration for a specific plugin.
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
    fn get_storage_config(&self, crate_name: &str, storage_type_name: &str) -> serde_json::Value {
        self.load_storage_config(crate_name, storage_type_name)
    }
}

impl ConfigProvider for () {
    fn init(_data_dir: std::path::PathBuf) -> Self {}

    fn load_core_config(&self) -> serde_json::Value {
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

macro_rules! impl_config_provider_tuple {
    ($($T:ident),+) => {
        impl<$($T: ConfigProvider),+> ConfigProvider for ($($T,)+) {
            fn init(data_dir: std::path::PathBuf) -> Self {
                (
                    $( $T::init(data_dir.clone()), )+
                )
            }

            fn load_core_config(&self) -> serde_json::Value {
                let mut val = serde_json::Value::Object(serde_json::Map::new());
                #[allow(non_snake_case)]
                let ($($T,)+) = self;
                $(
                    crate::config::env::merge_json(&mut val, $T.load_core_config());
                )+
                val
            }

            fn load_plugin_config(
                &self,
                crate_name: &str,
                plugin_type_name: &str,
            ) -> serde_json::Value {
                let mut val = serde_json::Value::Object(serde_json::Map::new());
                #[allow(non_snake_case)]
                let ($($T,)+) = self;
                $(
                    crate::config::env::merge_json(&mut val, $T.load_plugin_config(crate_name, plugin_type_name));
                )+
                val
            }

            fn load_storage_config(
                &self,
                crate_name: &str,
                storage_type_name: &str,
            ) -> serde_json::Value {
                let mut val = serde_json::Value::Object(serde_json::Map::new());
                #[allow(non_snake_case)]
                let ($($T,)+) = self;
                $(
                    crate::config::env::merge_json(&mut val, $T.load_storage_config(crate_name, storage_type_name));
                )+
                val
            }
        }
    };
}

impl_config_provider_tuple!(C1);
impl_config_provider_tuple!(C1, C2);
impl_config_provider_tuple!(C1, C2, C3);
impl_config_provider_tuple!(C1, C2, C3, C4);
impl_config_provider_tuple!(C1, C2, C3, C4, C5);
impl_config_provider_tuple!(C1, C2, C3, C4, C5, C6);
