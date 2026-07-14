use serde_json::Value;
use std::path::PathBuf;

pub trait ConfigPathStrategy: Send + Sync + 'static {
    fn resolve_path(data_dir: PathBuf) -> PathBuf;
}

pub struct DataDirStrategy;
impl ConfigPathStrategy for DataDirStrategy {
    fn resolve_path(data_dir: PathBuf) -> PathBuf {
        data_dir
    }
}

pub struct CurrentDirStrategy;
impl ConfigPathStrategy for CurrentDirStrategy {
    fn resolve_path(_data_dir: PathBuf) -> PathBuf {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }
}

pub struct ConfigJson<S: ConfigPathStrategy = DataDirStrategy> {
    config: Value,
    _marker: std::marker::PhantomData<S>,
}

impl<S: ConfigPathStrategy> crate::config::ConfigProvider for ConfigJson<S> {
    fn init(data_dir: PathBuf) -> Self {
        let base_dir = S::resolve_path(data_dir);
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
        self.config.clone()
    }

    fn load_plugin_config(&self, crate_name: &str, plugin_type_name: &str) -> Value {
        self.config
            .get("plugins")
            .and_then(|p| p.get(crate_name))
            .and_then(|c| c.get(plugin_type_name))
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
    }

    fn load_storage_config(&self, crate_name: &str, storage_type_name: &str) -> Value {
        self.config
            .get("storage")
            .and_then(|s| s.get(crate_name))
            .and_then(|c| c.get(storage_type_name))
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
    }
}
