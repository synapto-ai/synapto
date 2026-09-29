use schemars::Schema;
use serde_json::Value;

pub(crate) fn flatten_enum(schema: &mut Schema) {
    if let Some(Value::Array(one_of)) = schema.remove("oneOf") {
        let mut descriptions = Vec::new();
        let mut enum_values = Vec::new();

        for variant in &one_of {
            let Value::Object(variant_obj) = variant else {
                panic!("Variant schema not an object: {variant:?}")
            };

            let Some(Value::String(name)) = variant_obj.get("const") else {
                panic!("Missing `const` schema property in variant: {variant:?}")
            };

            descriptions.push(format!(
                "{} = {}",
                name,
                variant_obj
                    .get("description")
                    .expect("Variant must have a description")
                    .as_str()
                    .expect("Description must be a string")
            ));

            enum_values.push(Value::String(name.clone()));
        }

        schema.insert("enum".to_owned(), Value::Array(enum_values));
        schema.insert("type".to_owned(), Value::String("string".to_owned()));

        // Append variant descriptions to the parent schema description
        match schema.ensure_object().get_mut("description") {
            Some(Value::String(d)) => d.push_str(&format!(
                "\n\nOutput exactly ONE of these exact strings:\n{}",
                descriptions.join("\n")
            )),
            _ => {
                schema.insert(
                    "description".to_owned(),
                    Value::String(format!(
                        "Output exactly ONE of these exact strings:\n{}",
                        descriptions.join("\n")
                    )),
                );
            }
        };

        // Retain oneOf for programmatic introspection (e.g. choice questions)
        schema.insert("oneOf".to_owned(), Value::Array(one_of));
    }
}

pub(crate) fn customize_cognitive_commands_schema(
    mut root_schema: Schema,
    commands_type_name: &str,
    has_chat: bool,
    has_speech: bool,
    commands_registry: &synapto_interface::command::CommandRegistryBuilder,
) -> Schema {
    let commands = commands_registry.list();

    // 1. Resolve target key first while root_schema is not borrowed mutably
    let target_key = root_schema
        .get("properties")
        .and_then(|p| p.get("commands"))
        .and_then(|c| c.get("$ref"))
        .and_then(|r| r.as_str())
        .and_then(|r| r.strip_prefix("#/$defs/"))
        .unwrap_or(commands_type_name)
        .to_string();

    // 2. Prepare dynamic commands: collect schemas, descriptions, and defs
    let mut collected_defs = Vec::new();
    let mut processed_commands = Vec::new();

    for cmd in commands {
        let cmd_name = cmd.name().to_string();
        let cmd_desc = cmd.description();
        let mut cmd_schema = cmd.schema();

        if let Some(cmd_defs) = cmd_schema.remove("$defs") {
            if let Some(cmd_defs_obj) = cmd_defs.as_object() {
                for (k, v) in cmd_defs_obj {
                    collected_defs.push((k.clone(), v.clone()));
                }
            }
        }

        cmd_schema.remove("$schema");
        if !cmd_desc.is_empty() {
            cmd_schema.insert(
                "description".to_string(),
                Value::String(cmd_desc.to_string()),
            );
        }

        processed_commands.push((
            cmd_name,
            serde_json::to_value(cmd_schema).expect("Valid JSON schema value"),
        ));
    }

    // 3. Mutate root_schema
    let root_obj = root_schema.ensure_object();
    let root_defs = root_obj
        .entry("$defs".to_string())
        .or_insert_with(|| serde_json::json!({}));
    if let Some(root_defs_obj) = root_defs.as_object_mut() {
        for (k, v) in collected_defs {
            root_defs_obj.insert(k, v);
        }
    }

    // 4. Locate and update commands_obj
    let in_defs = root_obj
        .get("$defs")
        .and_then(|d| d.as_object())
        .map(|d| d.contains_key(&target_key))
        .unwrap_or(false);

    let commands_obj = if in_defs {
        root_obj
            .get_mut("$defs")
            .and_then(|d| d.as_object_mut())
            .and_then(|d| d.get_mut(&target_key))
            .and_then(|v| v.as_object_mut())
    } else {
        root_obj
            .get_mut("properties")
            .and_then(|p| p.get_mut("commands"))
            .and_then(|c| c.as_object_mut())
    };

    let Some(commands_obj) = commands_obj else {
        return root_schema;
    };

    let props = commands_obj
        .entry("properties".to_string())
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .expect("properties in commands schema must be an object");

    if !has_chat {
        props.remove("write");
    }
    if !has_speech {
        props.remove("say");
    }

    for (cmd_name, cmd_val) in processed_commands {
        props.insert(cmd_name, cmd_val);
    }

    commands_obj.insert("additionalProperties".to_string(), Value::Bool(false));

    root_schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use synapto_interface::command::{Command, CommandRegistryBuilder};
    use synapto_interface::llm::LLMSafe;

    #[derive(Debug, Serialize, Deserialize, JsonSchema, LLMSafe)]
    struct SubType {
        detail: String,
    }

    #[derive(Debug, Serialize, Deserialize, JsonSchema, LLMSafe)]
    struct MockArgs {
        sub: SubType,
    }

    struct MockCommand;

    #[async_trait::async_trait]
    impl Command for MockCommand {
        type Arguments = MockArgs;
        const NAME: &'static str = "mock_command";
        const DESCRIPTION: &'static str = "Mock command for testing schema injection";

        async fn execute(&self, _args: Self::Arguments) -> Result<(), String> {
            Ok(())
        }
    }

    #[derive(Debug, Serialize, Deserialize, JsonSchema)]
    struct DummyCommands {
        say: Option<String>,
        write: Option<String>,
        #[serde(flatten)]
        commands_map: std::collections::BTreeMap<String, serde_json::Value>,
    }

    #[derive(Debug, Serialize, Deserialize, JsonSchema)]
    struct DummyOutput {
        commands: DummyCommands,
        reasoning: String,
    }

    #[test]
    fn test_customize_cognitive_commands_schema() {
        let registry = CommandRegistryBuilder::default();
        registry.register(MockCommand);

        let base_schema = schemars::schema_for!(DummyOutput);
        let customized = customize_cognitive_commands_schema(
            base_schema,
            "DummyCommands",
            false, // no chat
            true,  // has speech
            &registry,
        );

        let defs = customized
            .get("$defs")
            .and_then(|v| v.as_object())
            .expect("Must have $defs");

        // SubType should have been merged into root $defs
        assert!(defs.contains_key("SubType"));

        let commands_def = defs
            .get("DummyCommands")
            .and_then(|v| v.as_object())
            .expect("Must have DummyCommands");

        assert_eq!(
            commands_def.get("additionalProperties"),
            Some(&Value::Bool(false))
        );

        let props = commands_def
            .get("properties")
            .and_then(|v| v.as_object())
            .expect("Must have properties");

        // 'write' must be removed
        assert!(!props.contains_key("write"));
        // 'say' must be present
        assert!(props.contains_key("say"));
        // 'mock_command' must be present with description
        let mock_cmd = props
            .get("mock_command")
            .and_then(|v| v.as_object())
            .expect("Must have mock_command");
        assert_eq!(
            mock_cmd.get("description"),
            Some(&Value::String(
                "Mock command for testing schema injection".to_string()
            ))
        );
    }
}
