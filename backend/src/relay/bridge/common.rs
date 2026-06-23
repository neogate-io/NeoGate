use serde_json::{Map, Value};

use super::content_value_to_text;

pub(super) fn rename_field(object: &mut Map<String, Value>, from: &str, to: &str) {
    if let Some(value) = object.remove(from) {
        object.insert(to.to_string(), value);
    }
}

pub(super) fn remove_fields(object: &mut Map<String, Value>, fields: &[&str]) {
    for field in fields {
        object.remove(*field);
    }
}

pub(super) fn openai_response_content_to_text(value: &Value) -> String {
    content_value_to_text(value, &["input_text", "output_text", "text"], true)
}

pub(super) fn text_field_content_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}
