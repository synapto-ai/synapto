use schemars::Schema;
use serde_json::Value;

pub(crate) fn flatten_enum(schema: &mut Schema) {
    if let Some(Value::Array(one_of)) = schema.remove("oneOf") {
        let mut descriptions = Vec::new();
        let mut enum_values = Vec::new();

        for variant in one_of {
            let Value::Object(mut variant) = variant else {
                panic!("Variant schema not an object: {variant:?}")
            };

            let Some(Value::String(name)) = variant.remove("const") else {
                panic!("Missing `const` schema property in variant: {variant:?}")
            };

            descriptions.push(format!(
                "{} = {}",
                name,
                variant
                    .get("description")
                    .expect("Variant must have a description")
                    .as_str()
                    .expect("Description must be a string")
            ));

            enum_values.push(Value::String(name));
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
    }
}
