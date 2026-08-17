/// A configuration provider that loads variables from a `.env` file.
///
/// If the `.env` file is missing, this provider fails silently and returns an
/// empty configuration, allowing the application to seamlessly fall back to other
/// configured providers (such as `Env` or `ConfigJson`) or `#[serde(default)]` values.
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
        let mut core_config = crate::config::env::build_json_from_vars(self.vars.clone(), "SYNAPTO__");
        if let Some(obj) = core_config.as_object_mut() {
            obj.remove("PLUGINS");
            obj.remove("STORAGE");
        };
        core_config
    }

    fn load_plugin_config(&self, crate_name: &str, plugin_type_name: &str) -> serde_json::Value {
        let prefix = format!(
            "SYNAPTO__PLUGINS__{}__{}__",
            crate_name.replace(['-', '.'], "_"),
            plugin_type_name.replace(['-', '.'], "_")
        );
        crate::config::env::build_json_from_vars(self.vars.clone(), &prefix)
    }

    fn load_storage_config(&self, crate_name: &str, storage_type_name: &str) -> serde_json::Value {
        let prefix = format!(
            "SYNAPTO__STORAGE__{}__{}__",
            crate_name.replace(['-', '.'], "_"),
            storage_type_name.replace(['-', '.'], "_")
        );
        crate::config::env::build_json_from_vars(self.vars.clone(), &prefix)
    }
}
