use std::collections::BTreeSet;

use serde_json::Value;

use crate::{ConfigMetadata, FieldMetadata, TierMetadata};

/// Re-export of `schemars::JsonSchema` used by `tier` schema helpers.
pub use schemars::JsonSchema;

/// Exports the JSON Schema for a configuration type.
#[must_use]
pub fn json_schema_for<T>() -> Value
where
    T: JsonSchema,
{
    serde_json::to_value(schemars::schema_for!(T))
        .unwrap_or_else(|_| Value::Object(Default::default()))
}

/// Exports the JSON Schema for a configuration type as pretty JSON.
#[must_use]
pub fn json_schema_pretty<T>() -> String
where
    T: JsonSchema,
{
    serde_json::to_string_pretty(&json_schema_for::<T>())
        .unwrap_or_else(|_| "{\"error\":\"failed to render schema\"}".to_owned())
}

/// Exports the JSON Schema for a configuration type annotated with `tier` metadata.
#[must_use]
pub fn annotated_json_schema_for<T>() -> Value
where
    T: JsonSchema + TierMetadata,
{
    let mut schema = json_schema_for::<T>();
    let metadata = T::metadata();
    apply_metadata_annotations(&mut schema, &metadata);
    schema
}

/// Exports the annotated JSON Schema for a configuration type as pretty JSON.
#[must_use]
pub fn annotated_json_schema_pretty<T>() -> String
where
    T: JsonSchema + TierMetadata,
{
    serde_json::to_string_pretty(&annotated_json_schema_for::<T>())
        .unwrap_or_else(|_| "{\"error\":\"failed to render schema\"}".to_owned())
}

/// Generates a machine-readable example configuration value from schema and metadata.
#[must_use]
pub fn config_example_for<T>() -> Value
where
    T: JsonSchema + TierMetadata,
{
    let schema = annotated_json_schema_for::<T>();
    build_example_value(&schema, &schema, &mut BTreeSet::new())
        .unwrap_or(Value::Object(Default::default()))
}

/// Renders the generated example configuration as pretty JSON.
#[must_use]
pub fn config_example_pretty<T>() -> String
where
    T: JsonSchema + TierMetadata,
{
    serde_json::to_string_pretty(&config_example_for::<T>())
        .unwrap_or_else(|_| "{\"error\":\"failed to render example config\"}".to_owned())
}

#[cfg(feature = "toml")]
/// Renders the generated example configuration as commented TOML.
#[must_use]
pub fn config_example_toml<T>() -> String
where
    T: JsonSchema + TierMetadata,
{
    let example = config_example_for::<T>();
    let metadata = T::metadata();
    render_example_toml(&example, &metadata)
}

fn apply_metadata_annotations(schema: &mut Value, metadata: &ConfigMetadata) {
    if let Some(object) = schema.as_object_mut()
        && !metadata.checks().is_empty()
    {
        object.insert(
            "x-tier-checks".to_owned(),
            serde_json::to_value(metadata.checks()).unwrap_or_else(|_| Value::Array(Vec::new())),
        );
    }

    for field in metadata.fields() {
        let Some(pointer) = schema_pointer_for_path(schema, &field.path) else {
            continue;
        };
        let Some(node) = schema.pointer_mut(&pointer) else {
            continue;
        };
        annotate_schema_node(node, field);
    }
}

fn annotate_schema_node(node: &mut Value, field: &FieldMetadata) {
    let Some(object) = node.as_object_mut() else {
        return;
    };

    if let Some(doc) = &field.doc {
        object.insert("description".to_owned(), Value::String(doc.clone()));
    }
    if let Some(example) = &field.example {
        object.insert("example".to_owned(), parse_example_value(example));
    }
    if let Some(note) = &field.deprecated {
        object.insert("deprecated".to_owned(), Value::Bool(true));
        object.insert(
            "x-tier-deprecated-note".to_owned(),
            Value::String(note.clone()),
        );
    }
    if field.secret {
        object.insert("writeOnly".to_owned(), Value::Bool(true));
        object.insert("x-tier-secret".to_owned(), Value::Bool(true));
    }
    if let Some(env) = &field.env {
        object.insert("x-tier-env".to_owned(), Value::String(env.clone()));
    }
    if !field.aliases.is_empty() {
        object.insert(
            "x-tier-aliases".to_owned(),
            Value::Array(field.aliases.iter().cloned().map(Value::String).collect()),
        );
    }
    if field.has_default {
        object.insert("x-tier-has-default".to_owned(), Value::Bool(true));
    }
    object.insert(
        "x-tier-merge".to_owned(),
        Value::String(field.merge.to_string()),
    );
    if !field.validations.is_empty() {
        object.insert(
            "x-tier-validate".to_owned(),
            serde_json::to_value(&field.validations).unwrap_or_else(|_| Value::Array(Vec::new())),
        );
    }
}

fn parse_example_value(example: &str) -> Value {
    serde_json::from_str(example).unwrap_or_else(|_| Value::String(example.to_owned()))
}

fn schema_pointer_for_path(document: &Value, path: &str) -> Option<String> {
    let mut pointer = String::new();
    for segment in path.split('.').filter(|segment| !segment.is_empty()) {
        pointer = resolve_schema_pointer(document, &pointer)?;
        let properties = document.pointer(&pointer)?.get("properties")?.as_object()?;
        if !properties.contains_key(segment) {
            return None;
        }
        pointer.push_str("/properties/");
        pointer.push_str(&escape_json_pointer(segment));
    }

    resolve_schema_pointer(document, &pointer)
}

fn resolve_schema_pointer(document: &Value, pointer: &str) -> Option<String> {
    let mut current = pointer.to_owned();
    loop {
        let node = document.pointer(&current)?;
        let Some(reference) = node.get("$ref").and_then(Value::as_str) else {
            return Some(current);
        };
        current = reference.strip_prefix('#')?.to_owned();
    }
}

fn escape_json_pointer(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

fn build_example_value(
    schema: &Value,
    root: &Value,
    visited_refs: &mut BTreeSet<String>,
) -> Option<Value> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if visited_refs.insert(reference.to_owned()) {
            let target = resolve_schema_ref(root, reference)?;
            let example = build_example_value(target, root, visited_refs);
            visited_refs.remove(reference);
            return example;
        }
        return None;
    }

    let object = schema.as_object()?;

    if let Some(example) = object.get("example") {
        return Some(example.clone());
    }

    if let Some(values) = object.get("enum").and_then(Value::as_array)
        && let Some(first) = values.first()
    {
        return Some(first.clone());
    }

    if let Some(values) = object.get("oneOf").and_then(Value::as_array)
        && let Some(first) = values.first()
    {
        return build_example_value(first, root, visited_refs);
    }

    if let Some(values) = object.get("anyOf").and_then(Value::as_array) {
        for candidate in values {
            let is_null = candidate
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|ty| ty == "null");
            if !is_null && let Some(example) = build_example_value(candidate, root, visited_refs) {
                return Some(example);
            }
        }
    }

    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        let mut rendered = serde_json::Map::new();
        for (key, child) in properties {
            if let Some(example) = build_example_value(child, root, visited_refs) {
                rendered.insert(key.clone(), example);
            }
        }
        return Some(Value::Object(rendered));
    }

    if let Some(items) = object.get("items") {
        return Some(Value::Array(
            build_example_value(items, root, visited_refs)
                .into_iter()
                .collect(),
        ));
    }

    match schema_type(object).as_str() {
        "string" => Some(
            if object
                .get("x-tier-secret")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                Value::String("<secret>".to_owned())
            } else {
                Value::String("example".to_owned())
            },
        ),
        "integer" => Some(Value::Number(0.into())),
        "number" => Some(serde_json::Number::from_f64(0.0).map_or(Value::Null, Value::Number)),
        "boolean" => Some(Value::Bool(false)),
        "array" => Some(Value::Array(Vec::new())),
        "object" => Some(Value::Object(Default::default())),
        _ => Some(Value::Null),
    }
}

fn schema_type(object: &serde_json::Map<String, Value>) -> String {
    match object.get("type") {
        Some(Value::String(ty)) => ty.clone(),
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .find(|ty| *ty != "null")
            .unwrap_or("null")
            .to_owned(),
        _ if object.contains_key("enum") => "enum".to_owned(),
        _ if object.contains_key("items") => "array".to_owned(),
        _ => "object".to_owned(),
    }
}

fn resolve_schema_ref<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    root.pointer(pointer)
}

#[cfg(feature = "toml")]
fn render_example_toml(value: &Value, metadata: &ConfigMetadata) -> String {
    let mut output = String::new();
    render_toml_root_comments(metadata, &mut output);
    let field_metadata = metadata.fields_by_path();
    match value {
        Value::Object(root) => render_toml_table("", root, &field_metadata, &mut output),
        other => {
            output.push_str(&toml_inline_value(other));
            output.push('\n');
        }
    }

    if !output.ends_with('\n') {
        output.push('\n');
    }

    output
}

#[cfg(feature = "toml")]
fn render_toml_root_comments(metadata: &ConfigMetadata, output: &mut String) {
    if metadata.checks().is_empty() {
        return;
    }

    for check in metadata.checks() {
        output.push_str("# validate: ");
        output.push_str(&check.to_string());
        output.push('\n');
    }

    output.push('\n');
}

#[cfg(feature = "toml")]
fn render_toml_table(
    current_path: &str,
    table: &serde_json::Map<String, Value>,
    metadata: &std::collections::BTreeMap<String, FieldMetadata>,
    output: &mut String,
) {
    let mut nested = Vec::new();

    for (key, value) in table {
        if value.is_null() {
            continue;
        }

        if is_nested_toml_value(value) {
            nested.push((key.as_str(), value));
            continue;
        }

        let path = join_metadata_path(current_path, key);
        render_toml_comments(&path, metadata, output);
        output.push_str(&toml_key(key));
        output.push_str(" = ");
        output.push_str(&toml_inline_value(value));
        output.push('\n');
    }

    let nested_count = nested.len();
    for (index, (key, value)) in nested.into_iter().enumerate() {
        let path = join_metadata_path(current_path, key);
        match value {
            Value::Object(child) => {
                start_toml_section(output);
                render_toml_comments(&path, metadata, output);
                output.push('[');
                output.push_str(&toml_table_name(&path));
                output.push_str("]\n");
                render_toml_table(&path, child, metadata, output);
            }
            Value::Array(items) => {
                for (item_index, item) in items.iter().enumerate() {
                    let Some(child) = item.as_object() else {
                        continue;
                    };
                    start_toml_section(output);
                    if item_index == 0 {
                        render_toml_comments(&path, metadata, output);
                    }
                    output.push_str("[[");
                    output.push_str(&toml_table_name(&path));
                    output.push_str("]]\n");
                    render_toml_table(&path, child, metadata, output);
                }
            }
            _ => {}
        }

        if index + 1 < nested_count && !output.ends_with("\n\n") {
            output.push('\n');
        }
    }
}

#[cfg(feature = "toml")]
fn render_toml_comments(
    path: &str,
    metadata: &std::collections::BTreeMap<String, FieldMetadata>,
    output: &mut String,
) {
    let Some(field) = metadata.get(path) else {
        return;
    };

    let mut lines = Vec::new();
    if let Some(doc) = &field.doc {
        lines.extend(
            doc.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned),
        );
    }
    if let Some(env) = &field.env {
        lines.push(format!("env: {env}"));
    }
    if !field.aliases.is_empty() {
        lines.push(format!("aliases: {}", field.aliases.join(", ")));
    }
    if field.has_default {
        lines.push("default: provided by serde".to_owned());
    }
    if field.merge != crate::MergeStrategy::Merge {
        lines.push(format!("merge: {}", field.merge));
    }
    if !field.validations.is_empty() {
        lines.push(format!(
            "validate: {}",
            field
                .validations
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if field.secret {
        lines.push("secret: true".to_owned());
    }
    if let Some(note) = &field.deprecated {
        lines.push(format!("deprecated: {note}"));
    }

    for line in lines {
        output.push_str("# ");
        output.push_str(&line);
        output.push('\n');
    }
}

#[cfg(feature = "toml")]
fn start_toml_section(output: &mut String) {
    if !output.is_empty() && !output.ends_with("\n\n") {
        output.push('\n');
    }
}

#[cfg(feature = "toml")]
fn is_nested_toml_value(value: &Value) -> bool {
    match value {
        Value::Object(_) => true,
        Value::Array(items) => items.iter().any(Value::is_object),
        _ => false,
    }
}

#[cfg(feature = "toml")]
fn toml_inline_value(value: &Value) -> String {
    match value {
        Value::Null => toml_string("<unset>"),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(string) => toml_string(string),
        Value::Array(items) => {
            let rendered = items
                .iter()
                .map(toml_inline_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{rendered}]")
        }
        Value::Object(map) => {
            let rendered = map
                .iter()
                .filter(|(_, value)| !value.is_null())
                .map(|(key, value)| format!("{} = {}", toml_key(key), toml_inline_value(value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {rendered} }}")
        }
    }
}

#[cfg(feature = "toml")]
fn toml_table_name(path: &str) -> String {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .map(toml_key)
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(feature = "toml")]
fn toml_key(segment: &str) -> String {
    if segment
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        segment.to_owned()
    } else {
        toml_string(segment)
    }
}

#[cfg(feature = "toml")]
fn toml_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0C}' => output.push_str("\\f"),
            control if control.is_control() => {
                let code = control as u32;
                output.push_str(&format!("\\u{:04X}", code));
            }
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

#[cfg(feature = "toml")]
fn join_metadata_path(parent: &str, segment: &str) -> String {
    if parent.is_empty() {
        segment.to_owned()
    } else {
        format!("{parent}.{segment}")
    }
}
