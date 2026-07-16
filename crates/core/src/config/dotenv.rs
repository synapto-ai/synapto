pub struct DotEnv {
    vars: Vec<(String, String)>,
}

impl crate::config::ConfigProvider for DotEnv {
    fn init() -> Self {
        let mut vars = Vec::new();
        if let Ok(iter) = dotenvy::dotenv_iter() {
            for item in iter {
                if let Ok(pair) = item {
                    vars.push(pair);
                }
            }
        }
        DotEnv { vars }
    }

    fn load_core_config(&self) -> serde_json::Value {
        crate::config::env::build_json_from_vars(self.vars.clone(), "SYNAPTO__")
    }

    fn load_plugin_config(&self, crate_name: &str, plugin_type_name: &str) -> serde_json::Value {
        let prefix = format!(
            "SYNAPTO__PLUGINS__{}__{}__",
            crate_name.to_uppercase().replace(['-', '.'], "_"),
            plugin_type_name.to_uppercase().replace(['-', '.'], "_")
        );
        crate::config::env::build_json_from_vars(self.vars.clone(), &prefix)
    }

    fn load_storage_config(&self, crate_name: &str, storage_type_name: &str) -> serde_json::Value {
        let prefix = format!(
            "SYNAPTO__STORAGE__{}__{}__",
            crate_name.to_uppercase().replace(['-', '.'], "_"),
            storage_type_name.to_uppercase().replace(['-', '.'], "_")
        );
        crate::config::env::build_json_from_vars(self.vars.clone(), &prefix)
    }
}
