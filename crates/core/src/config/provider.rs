#![doc = include_str!("provider.md")]

pub trait ConfigProvider: Send + Sync + Sized + 'static {
    /// Initializes the config provider.
    /// Provider-specific contexts (like use cases) must be handled internally
    /// by the implementing type (e.g., via trait markers or environment variables).
    fn init() -> Self;

    /// Returns the core configuration BEFORE environment variables are applied (if not using Env).
    /// Providers should return the raw JSON Value representing their stored config.
    fn load_core_config(&self) -> serde_json::Value;

    /// Returns the plugin configuration.
    fn load_plugin_config(&self, crate_name: &str, plugin_type_name: &str) -> serde_json::Value;

    /// Returns the storage configuration.
    fn load_storage_config(&self, crate_name: &str, storage_type_name: &str) -> serde_json::Value;

    /// Returns the credentials configuration.
    fn load_credentials_config(
        &self,
        crate_name: &str,
        provider_type_name: &str,
    ) -> serde_json::Value;

    /// Returns the decision provider configuration.
    fn load_decision_config(&self, crate_name: &str, provider_type_name: &str)
    -> serde_json::Value;

    /// Returns the LLM provider configuration.
    fn load_llm_config(&self, crate_name: &str, provider_type_name: &str) -> serde_json::Value;

    /// Returns the core configuration struct.
    fn get_core_config(&self) -> crate::config::Config {
        let val = self.load_core_config();
        serde_json::from_value(val).unwrap_or_else(|e| match self.describe_core_location() {
            Some(loc) => panic!(
                "Failed to parse core configuration (expected in {}): {}",
                loc, e
            ),
            None => panic!("Failed to parse core configuration: {}", e),
        })
    }

    /// Returns a human-readable description of the expected core configuration location.
    fn describe_core_location(&self) -> Option<String> {
        None
    }

    /// Retrieves the raw configuration value for a specific plugin before deserialization.
    fn get_plugin_config_value(
        &self,
        crate_name: &str,
        plugin_type_name: &str,
    ) -> serde_json::Value {
        self.load_plugin_config(crate_name, plugin_type_name)
    }

    /// Returns a human-readable description of the expected plugin configuration location.
    fn describe_plugin_location(
        &self,
        _crate_name: &str,
        _plugin_type_name: &str,
    ) -> Option<String> {
        None
    }

    /// Retrieves and deserializes the configuration for a specific plugin.
    fn get_plugin_config<T: serde::de::DeserializeOwned>(
        &self,
        crate_name: &str,
        plugin_type_name: &str,
    ) -> T {
        let val = self.get_plugin_config_value(crate_name, plugin_type_name);
        serde_json::from_value(val).unwrap_or_else(|e| {
            match self.describe_plugin_location(crate_name, plugin_type_name) {
                Some(loc) => panic!(
                    "Failed to parse config for plugin '{}::{}' (expected in {}): {}",
                    crate_name, plugin_type_name, loc, e
                ),
                None => panic!(
                    "Failed to parse config for plugin '{}::{}': {}",
                    crate_name, plugin_type_name, e
                ),
            }
        })
    }

    /// Retrieves the configuration value for a specific storage provider.
    fn get_storage_config(&self, crate_name: &str, storage_type_name: &str) -> serde_json::Value {
        self.load_storage_config(crate_name, storage_type_name)
    }

    /// Returns a human-readable description of the expected storage configuration location.
    fn describe_storage_location(
        &self,
        _crate_name: &str,
        _storage_type_name: &str,
    ) -> Option<String> {
        None
    }

    /// Returns a human-readable description of the expected credentials configuration location.
    fn describe_credentials_location(
        &self,
        _crate_name: &str,
        _provider_type_name: &str,
    ) -> Option<String> {
        None
    }

    /// Retrieves and deserializes the configuration for a specific credentials provider.
    fn get_credentials_config<T: serde::de::DeserializeOwned>(
        &self,
        crate_name: &str,
        provider_type_name: &str,
    ) -> T {
        let val = self.load_credentials_config(crate_name, provider_type_name);
        serde_json::from_value(val).unwrap_or_else(|e| {
            match self.describe_credentials_location(crate_name, provider_type_name) {
                Some(loc) => panic!(
                    "Failed to parse config for credentials provider '{}::{}' (expected in {}): {}",
                    crate_name, provider_type_name, loc, e
                ),
                None => panic!(
                    "Failed to parse config for credentials provider '{}::{}': {}",
                    crate_name, provider_type_name, e
                ),
            }
        })
    }

    /// Retrieves the raw configuration value for a specific decision provider before deserialization.
    fn get_decision_config_value(
        &self,
        crate_name: &str,
        provider_type_name: &str,
    ) -> serde_json::Value {
        self.load_decision_config(crate_name, provider_type_name)
    }

    /// Returns a human-readable description of the expected decision configuration location.
    fn describe_decision_location(
        &self,
        _crate_name: &str,
        _provider_type_name: &str,
    ) -> Option<String> {
        None
    }

    /// Retrieves and deserializes the configuration for a specific decision provider.
    fn get_decision_config<T: serde::de::DeserializeOwned>(
        &self,
        crate_name: &str,
        provider_type_name: &str,
    ) -> T {
        let val = self.get_decision_config_value(crate_name, provider_type_name);
        serde_json::from_value(val).unwrap_or_else(|e| {
            match self.describe_decision_location(crate_name, provider_type_name) {
                Some(loc) => panic!(
                    "Failed to parse config for decision provider '{}::{}' (expected in {}): {}",
                    crate_name, provider_type_name, loc, e
                ),
                None => panic!(
                    "Failed to parse config for decision provider '{}::{}': {}",
                    crate_name, provider_type_name, e
                ),
            }
        })
    }

    /// Retrieves the raw configuration value for a specific LLM provider before deserialization.
    fn get_llm_config_value(
        &self,
        crate_name: &str,
        provider_type_name: &str,
    ) -> serde_json::Value {
        self.load_llm_config(crate_name, provider_type_name)
    }

    /// Returns a human-readable description of the expected LLM configuration location.
    fn describe_llm_location(
        &self,
        _crate_name: &str,
        _provider_type_name: &str,
    ) -> Option<String> {
        None
    }

    /// Retrieves and deserializes the configuration for a specific LLM provider.
    fn get_llm_config<T: serde::de::DeserializeOwned>(
        &self,
        crate_name: &str,
        provider_type_name: &str,
    ) -> T {
        let val = self.get_llm_config_value(crate_name, provider_type_name);
        serde_json::from_value(val).unwrap_or_else(|e| {
            match self.describe_llm_location(crate_name, provider_type_name) {
                Some(loc) => panic!(
                    "Failed to parse config for LLM provider '{}::{}' (expected in {}): {}",
                    crate_name, provider_type_name, loc, e
                ),
                None => panic!(
                    "Failed to parse config for LLM provider '{}::{}': {}",
                    crate_name, provider_type_name, e
                ),
            }
        })
    }
}

impl ConfigProvider for () {
    fn init() -> Self {}

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

    fn load_credentials_config(
        &self,
        _crate_name: &str,
        _provider_type_name: &str,
    ) -> serde_json::Value {
        serde_json::Value::Object(serde_json::Map::new())
    }

    fn load_decision_config(
        &self,
        _crate_name: &str,
        _provider_type_name: &str,
    ) -> serde_json::Value {
        serde_json::Value::Object(serde_json::Map::new())
    }

    fn load_llm_config(&self, _crate_name: &str, _provider_type_name: &str) -> serde_json::Value {
        serde_json::Value::Object(serde_json::Map::new())
    }
}

macro_rules! impl_config_provider_tuple {
    ($($T:ident),+) => {
        impl<$($T: ConfigProvider),+> ConfigProvider for ($($T,)+) {
            fn init() -> Self {
                (
                    $( $T::init(), )+
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

            fn describe_core_location(&self) -> Option<String> {
                #[allow(non_snake_case)]
                let ($($T,)+) = self;
                let mut locs = Vec::new();
                $(
                    if let Some(loc) = $T.describe_core_location() {
                        locs.push(loc);
                    }
                )+
                if locs.is_empty() {
                    None
                } else {
                    Some(locs.join(" or "))
                }
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

            fn describe_plugin_location(
                &self,
                crate_name: &str,
                plugin_type_name: &str,
            ) -> Option<String> {
                #[allow(non_snake_case)]
                let ($($T,)+) = self;
                let mut locs = Vec::new();
                $(
                    if let Some(loc) = $T.describe_plugin_location(crate_name, plugin_type_name) {
                        locs.push(loc);
                    }
                )+
                if locs.is_empty() {
                    None
                } else {
                    Some(locs.join(" or "))
                }
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

            fn describe_storage_location(
                &self,
                crate_name: &str,
                storage_type_name: &str,
            ) -> Option<String> {
                #[allow(non_snake_case)]
                let ($($T,)+) = self;
                let mut locs = Vec::new();
                $(
                    if let Some(loc) = $T.describe_storage_location(crate_name, storage_type_name) {
                        locs.push(loc);
                    }
                )+
                if locs.is_empty() {
                    None
                } else {
                    Some(locs.join(" or "))
                }
            }

            fn load_credentials_config(
                &self,
                crate_name: &str,
                provider_type_name: &str,
            ) -> serde_json::Value {
                let mut val = serde_json::Value::Object(serde_json::Map::new());
                #[allow(non_snake_case)]
                let ($($T,)+) = self;
                $(
                    crate::config::env::merge_json(&mut val, $T.load_credentials_config(crate_name, provider_type_name));
                )+
                val
            }

            fn describe_credentials_location(
                &self,
                crate_name: &str,
                provider_type_name: &str,
            ) -> Option<String> {
                #[allow(non_snake_case)]
                let ($($T,)+) = self;
                let mut locs = Vec::new();
                $(
                    if let Some(loc) = $T.describe_credentials_location(crate_name, provider_type_name) {
                        locs.push(loc);
                    }
                )+
                if locs.is_empty() {
                    None
                } else {
                    Some(locs.join(" or "))
                }
            }

            fn load_decision_config(
                &self,
                crate_name: &str,
                provider_type_name: &str,
            ) -> serde_json::Value {
                let mut val = serde_json::Value::Object(serde_json::Map::new());
                #[allow(non_snake_case)]
                let ($($T,)+) = self;
                $(
                    crate::config::env::merge_json(&mut val, $T.load_decision_config(crate_name, provider_type_name));
                )+
                val
            }

            fn describe_decision_location(
                &self,
                crate_name: &str,
                provider_type_name: &str,
            ) -> Option<String> {
                #[allow(non_snake_case)]
                let ($($T,)+) = self;
                let mut locs = Vec::new();
                $(
                    if let Some(loc) = $T.describe_decision_location(crate_name, provider_type_name) {
                        locs.push(loc);
                    }
                )+
                if locs.is_empty() {
                    None
                } else {
                    Some(locs.join(" or "))
                }
            }

            fn load_llm_config(
                &self,
                crate_name: &str,
                provider_type_name: &str,
            ) -> serde_json::Value {
                let mut val = serde_json::Value::Object(serde_json::Map::new());
                #[allow(non_snake_case)]
                let ($($T,)+) = self;
                $(
                    crate::config::env::merge_json(&mut val, $T.load_llm_config(crate_name, provider_type_name));
                )+
                val
            }

            fn describe_llm_location(
                &self,
                crate_name: &str,
                provider_type_name: &str,
            ) -> Option<String> {
                #[allow(non_snake_case)]
                let ($($T,)+) = self;
                let mut locs = Vec::new();
                $(
                    if let Some(loc) = $T.describe_llm_location(crate_name, provider_type_name) {
                        locs.push(loc);
                    }
                )+
                if locs.is_empty() {
                    None
                } else {
                    Some(locs.join(" or "))
                }
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
