use serde_json::Value;

pub struct Env;

impl crate::config::ConfigProvider for Env {
    fn init() -> Self {
        Env
    }

    fn load_core_config(&self) -> serde_json::Value {
        build_env_json("SYNAPTO__")
    }

    fn load_plugin_config(&self, crate_name: &str, plugin_type_name: &str) -> serde_json::Value {
        let prefix = format!(
            "SYNAPTO__PLUGINS__{}__{}__",
            crate_name.to_uppercase().replace(['-', '.'], "_"),
            plugin_type_name.to_uppercase().replace(['-', '.'], "_")
        );
        build_env_json(&prefix)
    }

    fn load_storage_config(&self, crate_name: &str, storage_type_name: &str) -> serde_json::Value {
        let prefix = format!(
            "SYNAPTO__STORAGE__{}__{}__",
            crate_name.to_uppercase().replace(['-', '.'], "_"),
            storage_type_name.to_uppercase().replace(['-', '.'], "_")
        );
        build_env_json(&prefix)
    }
}

pub(crate) fn build_env_json(prefix: &str) -> Value {
    let mut map = serde_json::Map::new();
    for (k, v) in std::env::vars() {
        if k.starts_with(prefix) {
            let key_path = k.trim_start_matches(prefix).to_lowercase();
            // Try to parse as JSON (boolean, number), otherwise string
            let val = serde_json::from_str(&v).unwrap_or(Value::String(v));

            let parts: Vec<&str> = key_path.split("__").collect();
            let mut current = &mut map;

            for (i, part) in parts.iter().enumerate() {
                if i == parts.len() - 1 {
                    current.insert(part.to_string(), val.clone());
                } else {
                    if !current.contains_key(*part) || !current[*part].is_object() {
                        current.insert(part.to_string(), Value::Object(serde_json::Map::new()));
                    }
                    if let Value::Object(next) = current
                        .get_mut(*part)
                        .expect("Key was just inserted or already existed")
                    {
                        current = next;
                    } else {
                        unreachable!();
                    }
                }
            }
        }
    }
    Value::Object(map)
}

pub(crate) fn merge_json(a: &mut Value, b: Value) {
    match (a, b) {
        (&mut Value::Object(ref mut a_map), Value::Object(b_map)) => {
            for (k, v) in b_map {
                merge_json(a_map.entry(k).or_insert(Value::Null), v);
            }
        }
        (a_val, b_val) => {
            *a_val = b_val;
        }
    }
}
