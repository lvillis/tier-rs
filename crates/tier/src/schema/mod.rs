use std::cmp::Ordering;
use std::collections::BTreeSet;

use crate::{
    ConfigMetadata, FieldMetadata, TierMetadata,
    export::{json_pretty, json_value},
};
use regex::Regex;
use serde_json::{Number, Value};
#[cfg(feature = "toml")]
mod toml;

#[cfg(feature = "toml")]
use self::toml::render_example_toml;

/// Re-export of `schemars::JsonSchema` used by `tier` schema helpers.
pub use schemars::JsonSchema;

/// Stable version tag for machine-readable schema and example export payloads.
pub const SCHEMA_EXPORT_FORMAT_VERSION: u32 = 3;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
/// Versioned machine-readable JSON Schema payload.
pub struct JsonSchemaReport {
    /// Stable schema version for external consumers.
    pub format_version: u32,
    /// Exported JSON Schema document.
    pub schema: Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
/// Versioned machine-readable example configuration payload.
pub struct ConfigExampleReport {
    /// Stable schema version for external consumers.
    pub format_version: u32,
    /// Generated example configuration value.
    pub example: Value,
}

/// Exports the JSON Schema for a configuration type.
///
/// # Examples
///
/// ```
/// use schemars::JsonSchema;
/// use serde::{Deserialize, Serialize};
/// use tier::json_schema_for;
///
/// #[derive(Debug, Serialize, Deserialize, JsonSchema)]
/// struct AppConfig {
///     port: u16,
/// }
///
/// let schema = json_schema_for::<AppConfig>();
/// assert_eq!(schema["type"], "object");
/// ```
#[must_use]
pub fn json_schema_for<T>() -> Value
where
    T: JsonSchema,
{
    json_value(&schemars::schema_for!(T), Value::Object(Default::default()))
}

/// Exports the JSON Schema for a configuration type as pretty JSON.
#[must_use]
pub fn json_schema_pretty<T>() -> String
where
    T: JsonSchema,
{
    json_pretty(
        &json_schema_for::<T>(),
        "{\"error\":\"failed to render schema\"}",
    )
}

/// Exports the JSON Schema in a versioned machine-readable wrapper.
#[must_use]
pub fn json_schema_report<T>() -> JsonSchemaReport
where
    T: JsonSchema,
{
    JsonSchemaReport {
        format_version: SCHEMA_EXPORT_FORMAT_VERSION,
        schema: json_schema_for::<T>(),
    }
}

/// Renders the versioned JSON Schema export as JSON.
#[must_use]
pub fn json_schema_report_json<T>() -> Value
where
    T: JsonSchema,
{
    json_value(
        &json_schema_report::<T>(),
        Value::Object(Default::default()),
    )
}

/// Renders the versioned JSON Schema export as pretty JSON.
#[must_use]
pub fn json_schema_report_json_pretty<T>() -> String
where
    T: JsonSchema,
{
    json_pretty(
        &json_schema_report_json::<T>(),
        "{\"error\":\"failed to render schema report\"}",
    )
}

/// Exports the JSON Schema for a configuration type annotated with `tier` metadata.
///
/// # Examples
///
/// ```
/// use schemars::JsonSchema;
/// use serde::{Deserialize, Serialize};
/// use tier::{ConfigMetadata, FieldMetadata, TierMetadata, annotated_json_schema_for};
///
/// #[derive(Debug, Serialize, Deserialize, JsonSchema)]
/// struct AppConfig {
///     port: u16,
/// }
///
/// impl TierMetadata for AppConfig {
///     fn metadata() -> ConfigMetadata {
///         ConfigMetadata::from_fields([
///             FieldMetadata::new("port")
///                 .env("APP_PORT")
///                 .doc("Port used for incoming traffic"),
///         ])
///     }
/// }
///
/// let schema = annotated_json_schema_for::<AppConfig>();
/// assert_eq!(schema["properties"]["port"]["x-tier-env"], "APP_PORT");
/// ```
#[must_use]
pub fn annotated_json_schema_for<T>() -> Value
where
    T: JsonSchema + TierMetadata,
{
    let mut schema = json_schema_for::<T>();
    let metadata = T::metadata();
    apply_metadata_annotations(&mut schema, &metadata);
    let snapshot = schema.clone();
    redact_secret_schema_examples(&mut schema, &snapshot);
    schema
}

/// Exports the annotated JSON Schema for a configuration type as pretty JSON.
#[must_use]
pub fn annotated_json_schema_pretty<T>() -> String
where
    T: JsonSchema + TierMetadata,
{
    json_pretty(
        &annotated_json_schema_for::<T>(),
        "{\"error\":\"failed to render schema\"}",
    )
}

/// Exports the annotated JSON Schema in a versioned machine-readable wrapper.
#[must_use]
pub fn annotated_json_schema_report<T>() -> JsonSchemaReport
where
    T: JsonSchema + TierMetadata,
{
    JsonSchemaReport {
        format_version: SCHEMA_EXPORT_FORMAT_VERSION,
        schema: annotated_json_schema_for::<T>(),
    }
}

/// Renders the versioned annotated JSON Schema export as JSON.
#[must_use]
pub fn annotated_json_schema_report_json<T>() -> Value
where
    T: JsonSchema + TierMetadata,
{
    json_value(
        &annotated_json_schema_report::<T>(),
        Value::Object(Default::default()),
    )
}

/// Renders the versioned annotated JSON Schema export as pretty JSON.
#[must_use]
pub fn annotated_json_schema_report_json_pretty<T>() -> String
where
    T: JsonSchema + TierMetadata,
{
    json_pretty(
        &annotated_json_schema_report_json::<T>(),
        "{\"error\":\"failed to render schema report\"}",
    )
}

/// Generates a machine-readable example configuration value from schema and metadata.
///
/// # Examples
///
/// ```
/// use schemars::JsonSchema;
/// use serde::{Deserialize, Serialize};
/// use tier::{ConfigMetadata, FieldMetadata, TierMetadata, config_example_for};
///
/// #[derive(Debug, Serialize, Deserialize, JsonSchema)]
/// struct AppConfig {
///     port: u16,
/// }
///
/// impl TierMetadata for AppConfig {
///     fn metadata() -> ConfigMetadata {
///         ConfigMetadata::from_fields([FieldMetadata::new("port").example("8080")])
///     }
/// }
///
/// let example = config_example_for::<AppConfig>();
/// assert_eq!(example["port"], 8080);
/// ```
#[must_use]
pub fn config_example_for<T>() -> Value
where
    T: JsonSchema + TierMetadata,
{
    let schema = annotated_json_schema_for::<T>();
    build_example_value(&schema, &schema, &mut BTreeSet::new(), None)
        .unwrap_or(Value::Object(Default::default()))
}

/// Renders the generated example configuration as pretty JSON.
#[must_use]
pub fn config_example_pretty<T>() -> String
where
    T: JsonSchema + TierMetadata,
{
    json_pretty(
        &config_example_for::<T>(),
        "{\"error\":\"failed to render example config\"}",
    )
}

/// Exports the generated example configuration in a versioned wrapper.
#[must_use]
pub fn config_example_report<T>() -> ConfigExampleReport
where
    T: JsonSchema + TierMetadata,
{
    ConfigExampleReport {
        format_version: SCHEMA_EXPORT_FORMAT_VERSION,
        example: config_example_for::<T>(),
    }
}

/// Renders the versioned example export as JSON.
#[must_use]
pub fn config_example_report_json<T>() -> Value
where
    T: JsonSchema + TierMetadata,
{
    json_value(
        &config_example_report::<T>(),
        Value::Object(Default::default()),
    )
}

/// Renders the versioned example export as pretty JSON.
#[must_use]
pub fn config_example_report_json_pretty<T>() -> String
where
    T: JsonSchema + TierMetadata,
{
    json_pretty(
        &config_example_report_json::<T>(),
        "{\"error\":\"failed to render example report\"}",
    )
}

#[cfg(feature = "toml")]
/// Renders the generated example configuration as commented TOML.
///
/// # Examples
///
/// ```
/// use schemars::JsonSchema;
/// use serde::{Deserialize, Serialize};
/// use tier::{ConfigMetadata, FieldMetadata, TierMetadata, config_example_toml};
///
/// #[derive(Debug, Serialize, Deserialize, JsonSchema)]
/// struct AppConfig {
///     port: u16,
/// }
///
/// impl TierMetadata for AppConfig {
///     fn metadata() -> ConfigMetadata {
///         ConfigMetadata::from_fields([
///             FieldMetadata::new("port")
///                 .doc("Port used for incoming traffic")
///                 .example("8080"),
///         ])
///     }
/// }
///
/// let example = config_example_toml::<AppConfig>();
/// assert!(example.contains("8080"));
/// assert!(example.contains("incoming traffic"));
/// ```
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
    let snapshot = schema.clone();
    if let Some(object) = schema.as_object_mut()
        && !metadata.checks().is_empty()
    {
        object.insert(
            "x-tier-checks".to_owned(),
            serde_json::to_value(metadata.checks()).unwrap_or_else(|_| Value::Array(Vec::new())),
        );
    }

    for field in metadata.fields() {
        let effective = metadata
            .effective_field_for(&field.path)
            .unwrap_or_else(|| field.clone());
        annotate_schema_path(schema, &snapshot, &field.path, &effective);
    }
}

fn annotate_schema_node(node: &mut Value, field: &FieldMetadata) {
    let Some(object) = node.as_object_mut() else {
        return;
    };
    let is_secret = field.secret
        || object
            .get("writeOnly")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || object
            .get("x-tier-secret")
            .and_then(Value::as_bool)
            .unwrap_or(false);

    if let Some(doc) = &field.doc {
        object.insert("description".to_owned(), Value::String(doc.clone()));
    }
    if let Some(example) = &field.example {
        let value = parse_example_value(example);
        object.insert(
            "example".to_owned(),
            if is_secret {
                redact_example_value(&value)
            } else {
                value
            },
        );
    }
    if let Some(note) = &field.deprecated {
        object.insert("deprecated".to_owned(), Value::Bool(true));
        object.insert(
            "x-tier-deprecated-note".to_owned(),
            Value::String(note.clone()),
        );
    }
    if is_secret {
        object.insert("writeOnly".to_owned(), Value::Bool(true));
        object.insert("x-tier-secret".to_owned(), Value::Bool(true));
    }
    if let Some(env) = &field.env {
        object.insert("x-tier-env".to_owned(), Value::String(env.clone()));
    }
    if let Some(env_decode) = &field.env_decode {
        object.insert(
            "x-tier-env-decode".to_owned(),
            Value::String(env_decode.to_string()),
        );
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
    let allowed_sources = field.allowed_source_names();
    if !allowed_sources.is_empty() {
        let sources = allowed_sources
            .into_iter()
            .map(Value::String)
            .collect::<Vec<_>>();
        object.insert("x-tier-sources".to_owned(), Value::Array(sources));
    }
    let denied_sources = field.denied_source_names();
    if !denied_sources.is_empty() {
        let sources = denied_sources
            .into_iter()
            .map(Value::String)
            .collect::<Vec<_>>();
        object.insert("x-tier-denied-sources".to_owned(), Value::Array(sources));
    }
    if !field.validations.is_empty() {
        object.insert(
            "x-tier-validate".to_owned(),
            serde_json::to_value(&field.validations).unwrap_or_else(|_| Value::Array(Vec::new())),
        );
    }
    if let Some(validation_config) = field.validation_config_json() {
        object.insert("x-tier-validation-config".to_owned(), validation_config);
    }
}

fn parse_example_value(example: &str) -> Value {
    serde_json::from_str(example).unwrap_or_else(|_| Value::String(example.to_owned()))
}

fn is_secret_schema_node(object: &serde_json::Map<String, Value>) -> bool {
    object
        .get("writeOnly")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || object
            .get("x-tier-secret")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn redact_example_value(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(redact_example_value).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), redact_example_value(value)))
                .collect(),
        ),
        _ => Value::String("<secret>".to_owned()),
    }
}

fn redact_secret_schema_examples(node: &mut Value, root: &Value) {
    match node {
        Value::Object(object) => {
            project_secret_ref_annotations(object, root);
            let is_secret = is_secret_schema_node(object);
            if is_secret && let Some(example) = object.get_mut("example") {
                let redacted = redact_example_value(example);
                *example = redacted;
            }
            for value in object.values_mut() {
                redact_secret_schema_examples(value, root);
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_secret_schema_examples(value, root);
            }
        }
        _ => {}
    }
}

fn project_secret_ref_annotations(object: &mut serde_json::Map<String, Value>, root: &Value) {
    let Some(reference) = object.get("$ref").and_then(Value::as_str) else {
        return;
    };
    let Some(target) = resolve_schema_ref(root, reference).and_then(Value::as_object) else {
        return;
    };
    if !is_secret_schema_node(target) {
        return;
    }

    object.insert("writeOnly".to_owned(), Value::Bool(true));
    object.insert("x-tier-secret".to_owned(), Value::Bool(true));
    if !object.contains_key("example")
        && let Some(example) = target.get("example")
    {
        object.insert("example".to_owned(), redact_example_value(example));
    }
}

fn annotate_schema_path(
    schema: &mut Value,
    root: &Value,
    path: &str,
    field: &FieldMetadata,
) -> bool {
    let segments = path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    annotate_schema_segments(schema, root, &segments, field)
}

fn annotate_schema_segments(
    node: &mut Value,
    root: &Value,
    segments: &[&str],
    field: &FieldMetadata,
) -> bool {
    if segments.is_empty() {
        annotate_schema_node(node, field);
        return true;
    }

    inline_schema_ref(node, root);
    let Some(object) = node.as_object_mut() else {
        return false;
    };

    let segment = segments[0];
    let remaining = &segments[1..];

    let mut matched = false;
    if segment == "*" {
        if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
            for child in properties.values_mut() {
                matched |= annotate_schema_segments(child, root, remaining, field);
            }
        }
        if let Some(pattern_properties) = object
            .get_mut("patternProperties")
            .and_then(Value::as_object_mut)
        {
            for child in pattern_properties.values_mut() {
                matched |= annotate_schema_segments(child, root, remaining, field);
            }
        }
        if let Some(children) = object.get_mut("prefixItems").and_then(Value::as_array_mut) {
            for child in children {
                matched |= annotate_schema_segments(child, root, remaining, field);
            }
        }
        if let Some(children) = object.get_mut("items").and_then(Value::as_array_mut) {
            for child in children {
                matched |= annotate_schema_segments(child, root, remaining, field);
            }
        }
        if let Some(items) = object.get_mut("items") {
            matched |= annotate_schema_segments(items, root, remaining, field);
        }
        if let Some(additional) = object
            .get_mut("additionalProperties")
            .filter(|value| value.is_object())
        {
            matched |= annotate_schema_segments(additional, root, remaining, field);
        }
        let has_legacy_tuple_items = object.get("items").is_some_and(Value::is_array);
        if has_legacy_tuple_items
            && let Some(additional) = object
                .get_mut("additionalItems")
                .filter(|value| value.is_object())
        {
            matched |= annotate_schema_segments(additional, root, remaining, field);
        }
        if let Some(contains) = object.get_mut("contains").filter(|value| value.is_object()) {
            matched |= annotate_schema_segments(contains, root, remaining, field);
        }
    } else {
        if let Some(child) = object
            .get_mut("properties")
            .and_then(Value::as_object_mut)
            .and_then(|properties| properties.get_mut(segment))
        {
            matched |= annotate_schema_segments(child, root, remaining, field);
        }
        if let Ok(index) = segment.parse::<usize>() {
            if let Some(child) = object
                .get_mut("prefixItems")
                .and_then(Value::as_array_mut)
                .and_then(|items| items.get_mut(index))
            {
                matched |= annotate_schema_segments(child, root, remaining, field);
            }
            if let Some(child) = object
                .get_mut("items")
                .and_then(Value::as_array_mut)
                .and_then(|items| items.get_mut(index))
            {
                matched |= annotate_schema_segments(child, root, remaining, field);
            }
        }
    }

    for keyword in ["allOf", "anyOf", "oneOf"] {
        if let Some(children) = object.get_mut(keyword).and_then(Value::as_array_mut) {
            for child in children {
                matched |= annotate_schema_segments(child, root, segments, field);
            }
        }
    }

    matched
}

fn inline_schema_ref(node: &mut Value, root: &Value) {
    if let Some(inlined) = inlined_schema_ref(node, root) {
        *node = inlined;
    }
}

fn build_example_value(
    schema: &Value,
    root: &Value,
    visited_refs: &mut BTreeSet<String>,
    scope_reserved_keys: Option<&BTreeSet<String>>,
) -> Option<Value> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if visited_refs.insert(reference.to_owned()) {
            let inlined = inlined_schema_ref(schema, root)?;
            let example = build_example_value(&inlined, root, visited_refs, scope_reserved_keys);
            visited_refs.remove(reference);
            return example;
        }
        return None;
    }

    match schema {
        Value::Bool(true) => return Some(Value::Null),
        Value::Bool(false) => return None,
        _ => {}
    }

    let object = schema.as_object()?;
    let is_secret = is_secret_schema_node(object);
    let reserved_keys = merged_object_level_property_names(schema, root, scope_reserved_keys);

    if let Some(constant) = object.get("const") {
        return Some(if is_secret {
            redact_example_value(constant)
        } else {
            constant.clone()
        });
    }

    if let Some(example) = object.get("example") {
        return Some(if is_secret {
            redact_example_value(example)
        } else {
            example.clone()
        });
    }

    if let Some(default) = object.get("default") {
        return Some(if is_secret {
            redact_example_value(default)
        } else {
            default.clone()
        });
    }

    let mut merged = None;
    if let Some(values) = object.get("allOf").and_then(Value::as_array) {
        for child in values {
            let Some(example) =
                build_example_value(child, root, visited_refs, Some(&reserved_keys))
            else {
                continue;
            };
            merge_example_value(&mut merged, example);
        }
    }

    if let Some(values) = object.get("enum").and_then(Value::as_array)
        && let Some(first) = values.first()
    {
        return Some(if is_secret {
            redact_example_value(first)
        } else {
            first.clone()
        });
    }

    if let Some(values) = object.get("oneOf").and_then(Value::as_array) {
        for candidate in values {
            let is_null = candidate
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|ty| ty == "null");
            if !is_null
                && let Some(example) =
                    build_example_value(candidate, root, visited_refs, Some(&reserved_keys))
            {
                merge_example_value(&mut merged, example);
                break;
            }
        }
    }

    if let Some(values) = object.get("anyOf").and_then(Value::as_array) {
        for candidate in values {
            let is_null = candidate
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|ty| ty == "null");
            if !is_null
                && let Some(example) =
                    build_example_value(candidate, root, visited_refs, Some(&reserved_keys))
            {
                merge_example_value(&mut merged, example);
                break;
            }
        }
    }

    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        let mut rendered = serde_json::Map::new();
        for (key, child) in properties {
            if let Some(example) = build_example_value(child, root, visited_refs, None) {
                rendered.insert(key.clone(), example);
            }
        }
        trim_object_example_properties(&mut rendered, object);
        merge_example_value(&mut merged, Value::Object(rendered));
    }

    if let Some(pattern_properties) = object.get("patternProperties").and_then(Value::as_object) {
        let existing_len = merged
            .as_ref()
            .and_then(Value::as_object)
            .map_or(0, serde_json::Map::len);
        let required_dynamic = object
            .get("minProperties")
            .and_then(Value::as_u64)
            .map_or(0, |min_properties| {
                min_properties.saturating_sub(existing_len as u64) as usize
            });
        let available_slots = object
            .get("maxProperties")
            .and_then(Value::as_u64)
            .map_or(usize::MAX, |max_properties| max_properties as usize)
            .saturating_sub(existing_len);
        if available_slots > 0 {
            let mut taken = reserved_keys.clone();
            if let Some(existing) = merged.as_ref().and_then(Value::as_object) {
                taken.extend(existing.keys().cloned());
            }

            let mut rendered = serde_json::Map::new();
            let target_entries =
                available_slots.min(required_dynamic.max(pattern_properties.len()));
            while rendered.len() < target_entries {
                let mut made_progress = false;
                for (pattern, child) in pattern_properties {
                    if rendered.len() >= target_entries {
                        break;
                    }
                    if let Some(key) =
                        pattern_property_placeholder_for_schema(pattern, object, root, &taken)
                        && let Some(example) = build_example_value(child, root, visited_refs, None)
                    {
                        taken.insert(key.clone());
                        rendered.insert(key, example);
                        made_progress = true;
                    }
                }

                if !made_progress {
                    break;
                }
            }
            if !rendered.is_empty() {
                merge_example_value(&mut merged, Value::Object(rendered));
            }
        }
    }

    if let Some(items) = object.get("prefixItems").and_then(Value::as_array) {
        let rendered = items
            .iter()
            .map(|child| {
                build_example_value(child, root, visited_refs, None).unwrap_or(Value::Null)
            })
            .collect::<Vec<_>>();
        merge_example_value(&mut merged, Value::Array(rendered));
    }

    if let Some(items) = object.get("items").and_then(Value::as_array) {
        let rendered = items
            .iter()
            .map(|child| {
                build_example_value(child, root, visited_refs, None).unwrap_or(Value::Null)
            })
            .collect::<Vec<_>>();
        merge_example_value(&mut merged, Value::Array(rendered));
    }

    if let Some(items) = object.get("items").filter(|value| !value.is_array()) {
        let existing_len = merged
            .as_ref()
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let additional_items = additional_example_item_count(object, existing_len);
        if additional_items > 0 {
            match build_example_value(items, root, visited_refs, None) {
                Some(example) => match &mut merged {
                    Some(Value::Array(existing)) => {
                        existing.extend(std::iter::repeat_n(example, additional_items))
                    }
                    _ => {
                        merge_example_value(
                            &mut merged,
                            Value::Array(std::iter::repeat_n(example, additional_items).collect()),
                        );
                    }
                },
                None => {
                    if merged.is_none() {
                        merge_example_value(&mut merged, Value::Array(Vec::new()));
                    }
                }
            }
        }
    }

    if let Some(additional) =
        legacy_additional_items_for_schema(object).filter(|value| !value.is_array())
    {
        let existing_len = merged
            .as_ref()
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let additional_items = additional_example_item_count(object, existing_len);
        if additional_items > 0 {
            match build_example_value(additional, root, visited_refs, None) {
                Some(example) => match &mut merged {
                    Some(Value::Array(existing)) => {
                        existing.extend(std::iter::repeat_n(example, additional_items))
                    }
                    _ => {
                        merge_example_value(
                            &mut merged,
                            Value::Array(std::iter::repeat_n(example, additional_items).collect()),
                        );
                    }
                },
                None => {
                    if merged.is_none() {
                        merge_example_value(&mut merged, Value::Array(Vec::new()));
                    }
                }
            }
        }
    }

    uniquify_merged_array_example(&mut merged, object, root);

    if let Some(contains) = object.get("contains") {
        let required_matches = required_contains_item_count(object);
        if required_matches > 0 {
            let unique_items = array_requires_unique_items(object);
            let existing_matches = merged
                .as_ref()
                .and_then(Value::as_array)
                .map_or(0, |values| {
                    count_matching_example_items(values, contains, root, unique_items)
                });
            let missing = required_matches.saturating_sub(existing_matches);
            let available_slots = available_additional_array_slots(
                object,
                merged
                    .as_ref()
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len),
            );
            let additional_items = missing.min(available_slots);
            if additional_items > 0 {
                match build_example_value(contains, root, visited_refs, None) {
                    Some(example) => match &mut merged {
                        Some(Value::Array(existing)) => {
                            let additions = build_repeated_example_values(
                                example,
                                contains,
                                root,
                                additional_items,
                                unique_items,
                                existing,
                            );
                            existing.extend(additions);
                        }
                        _ => {
                            let additions = build_repeated_example_values(
                                example,
                                contains,
                                root,
                                additional_items,
                                unique_items,
                                &[],
                            );
                            merge_example_value(&mut merged, Value::Array(additions));
                        }
                    },
                    None => {
                        if merged.is_none() {
                            merge_example_value(&mut merged, Value::Array(Vec::new()));
                        }
                    }
                }
            }
        }
    }

    let existing_object_len = merged
        .as_ref()
        .and_then(Value::as_object)
        .map_or(0, serde_json::Map::len);
    let required_dynamic = object
        .get("minProperties")
        .and_then(Value::as_u64)
        .map_or(0, |min_properties| {
            min_properties.saturating_sub(existing_object_len as u64) as usize
        });
    let implicit_additional = Value::Bool(true);
    let additional_properties = object.get("additionalProperties").or({
        if required_dynamic > 0 {
            Some(&implicit_additional)
        } else {
            None
        }
    });
    if let Some(additional) = additional_properties
        && let Some(example) = build_example_value(additional, root, visited_refs, None)
    {
        let available_slots = object
            .get("maxProperties")
            .and_then(Value::as_u64)
            .map_or(usize::MAX, |max_properties| max_properties as usize)
            .saturating_sub(existing_object_len);
        let additional_entries = if object.contains_key("additionalProperties") {
            required_dynamic.max(1).min(available_slots)
        } else {
            required_dynamic.min(available_slots)
        };
        if additional_entries > 0 {
            let placeholders = dynamic_object_placeholders_for_schema(
                object,
                root,
                &reserved_keys,
                additional_entries,
            );
            let rendered = placeholders
                .into_iter()
                .map(|placeholder| (placeholder, example.clone()))
                .collect::<serde_json::Map<_, _>>();
            merge_example_value(&mut merged, Value::Object(rendered));
        }
    }

    if let Some(mut merged) = merged {
        uniquify_example_value_in_place(&mut merged, object, root);
        return Some(if is_secret {
            redact_example_value(&merged)
        } else {
            merged
        });
    }

    let fallback = match schema_type(object).as_str() {
        "string" => Some(Value::String(fallback_string_example(object))),
        "integer" => fallback_integer_example(object).map(Value::Number),
        "number" => Some(
            serde_json::Number::from_f64(fallback_number_example(object))
                .map_or(Value::Null, Value::Number),
        ),
        "boolean" => Some(Value::Bool(false)),
        "array" => Some(Value::Array(Vec::new())),
        "object" => Some(Value::Object(Default::default())),
        _ => Some(Value::Null),
    }?;

    Some(if is_secret {
        redact_example_value(&fallback)
    } else {
        fallback
    })
}

fn fallback_string_example(object: &serde_json::Map<String, Value>) -> String {
    let secret = object
        .get("x-tier-secret")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut value = if secret {
        "<secret>".to_owned()
    } else {
        "example".to_owned()
    };

    let min_length = object
        .get("minLength")
        .and_then(Value::as_u64)
        .map_or(0, |min| min as usize);
    let max_length = object
        .get("maxLength")
        .and_then(Value::as_u64)
        .map(|max| max as usize);

    if value.chars().count() < min_length {
        value = "x".repeat(min_length);
    }

    if let Some(max_length) = max_length
        && value.chars().count() > max_length
    {
        value = value.chars().take(max_length).collect();
    }

    value
}

fn fallback_integer_example(object: &serde_json::Map<String, Value>) -> Option<Number> {
    let lower = integer_lower_bound(object).unwrap_or(0);
    let upper = integer_upper_bound(object);
    let candidate = integer_candidate_in_range(lower, upper, object)
        .or_else(|| upper.filter(|upper| integer_value_matches_constraints(*upper, object)))
        .or_else(|| integer_value_matches_constraints(lower, object).then_some(lower))
        .or_else(|| integer_value_matches_constraints(0, object).then_some(0))?;

    number_from_i128(candidate)
}

fn fallback_number_example(object: &serde_json::Map<String, Value>) -> f64 {
    let lower = object
        .get("minimum")
        .and_then(Value::as_f64)
        .or_else(|| object.get("exclusiveMinimum").and_then(Value::as_f64));
    let upper = object
        .get("maximum")
        .and_then(Value::as_f64)
        .or_else(|| object.get("exclusiveMaximum").and_then(Value::as_f64));

    let mut candidate = lower.unwrap_or(0.0);
    if object.get("minimum").is_none()
        && let Some(exclusive_minimum) = object.get("exclusiveMinimum").and_then(Value::as_f64)
    {
        candidate = exclusive_minimum + 1.0;
    }
    if let Some(upper) = upper
        && candidate > upper
    {
        candidate = upper;
    }
    if object.get("maximum").is_none()
        && let Some(exclusive_maximum) = object.get("exclusiveMaximum").and_then(Value::as_f64)
        && candidate >= exclusive_maximum
    {
        candidate = exclusive_maximum - 1.0;
    }

    if let Some(multiple_of) = object.get("multipleOf").and_then(Value::as_f64)
        && multiple_of.is_normal()
        && multiple_of > 0.0
    {
        candidate = (candidate / multiple_of).ceil() * multiple_of;
        if let Some(upper) = upper
            && candidate > upper
        {
            let adjusted = candidate - multiple_of;
            if adjusted >= lower.unwrap_or(f64::NEG_INFINITY) {
                candidate = adjusted;
            }
        }
    }

    candidate
}

fn integer_candidate_in_range(
    lower: i128,
    upper: Option<i128>,
    object: &serde_json::Map<String, Value>,
) -> Option<i128> {
    if let Some(step) = integer_multiple_step_constraint(object)
        && let Some(candidate) = first_integer_multiple_at_or_above(lower, step)
        && upper.is_none_or(|upper| candidate <= upper)
        && integer_value_matches_constraints(candidate, object)
    {
        return Some(candidate);
    }

    let mut candidate = lower;
    let max_iterations = upper
        .and_then(|upper| upper.checked_sub(lower))
        .and_then(|span| span.checked_add(1))
        .and_then(|span| usize::try_from(span).ok())
        .unwrap_or(10_000);

    for _ in 0..max_iterations {
        if upper.is_some_and(|upper| candidate > upper) {
            break;
        }
        if integer_value_matches_constraints(candidate, object) {
            return Some(candidate);
        }
        candidate = candidate.saturating_add(1);
    }

    None
}

fn integer_multiple_step_constraint(object: &serde_json::Map<String, Value>) -> Option<i128> {
    object
        .get("multipleOf")
        .and_then(Value::as_number)
        .and_then(integer_multiple_step_from_number)
}

fn integer_multiple_step_from_number(number: &Number) -> Option<i128> {
    if let Some(integer) = number_as_integral_i128(number) {
        return (integer > 0).then_some(integer);
    }

    positive_decimal_rational_from_number(number).and_then(|(numerator, denominator)| {
        let divisor = gcd_i128(numerator, denominator);
        numerator.checked_div(divisor).filter(|step| *step > 0)
    })
}

fn first_integer_multiple_at_or_above(lower: i128, factor: i128) -> Option<i128> {
    if factor <= 0 {
        return None;
    }

    let quotient = lower.div_euclid(factor);
    let candidate = quotient.checked_mul(factor)?;
    if candidate >= lower {
        Some(candidate)
    } else {
        candidate.checked_add(factor)
    }
}

fn last_integer_multiple_at_or_below(upper: i128, factor: i128) -> Option<i128> {
    if factor <= 0 {
        return None;
    }

    upper.div_euclid(factor).checked_mul(factor)
}

fn integer_lower_bound(object: &serde_json::Map<String, Value>) -> Option<i128> {
    object
        .get("exclusiveMinimum")
        .and_then(schema_number_floor)
        .and_then(|minimum| minimum.checked_add(1))
        .or_else(|| object.get("minimum").and_then(schema_number_ceil))
}

fn integer_upper_bound(object: &serde_json::Map<String, Value>) -> Option<i128> {
    object
        .get("exclusiveMaximum")
        .and_then(schema_number_ceil)
        .and_then(|maximum| maximum.checked_sub(1))
        .or_else(|| object.get("maximum").and_then(schema_number_floor))
}

fn schema_number_floor(value: &Value) -> Option<i128> {
    let number = value.as_number()?;
    number_as_integral_i128(number).or_else(|| finite_f64_to_i128(number.as_f64()?.floor()))
}

fn schema_number_ceil(value: &Value) -> Option<i128> {
    let number = value.as_number()?;
    number_as_integral_i128(number).or_else(|| finite_f64_to_i128(number.as_f64()?.ceil()))
}

fn integer_value_matches_constraints(value: i128, object: &serde_json::Map<String, Value>) -> bool {
    number_from_i128(value).is_some_and(|number| number_matches_numeric_schema(&number, object))
}

fn number_matches_numeric_schema(number: &Number, object: &serde_json::Map<String, Value>) -> bool {
    if let Some(minimum) = object.get("minimum").and_then(Value::as_number)
        && compare_numbers(number, minimum).is_some_and(|ordering| ordering == Ordering::Less)
    {
        return false;
    }
    if let Some(maximum) = object.get("maximum").and_then(Value::as_number)
        && compare_numbers(number, maximum).is_some_and(|ordering| ordering == Ordering::Greater)
    {
        return false;
    }
    if let Some(minimum) = object.get("exclusiveMinimum").and_then(Value::as_number)
        && compare_numbers(number, minimum).is_some_and(|ordering| ordering != Ordering::Greater)
    {
        return false;
    }
    if let Some(maximum) = object.get("exclusiveMaximum").and_then(Value::as_number)
        && compare_numbers(number, maximum).is_some_and(|ordering| ordering != Ordering::Less)
    {
        return false;
    }

    number_matches_multiple_of(number, object)
}

fn number_matches_multiple_of(number: &Number, object: &serde_json::Map<String, Value>) -> bool {
    let Some(multiple_of) = object.get("multipleOf").and_then(Value::as_number) else {
        return true;
    };

    if let (Some(number), Some(multiple_of)) = (
        number_as_integral_i128(number),
        number_as_integral_i128(multiple_of),
    ) {
        return multiple_of <= 0 || number % multiple_of == 0;
    }

    let Some(number) = number.as_f64() else {
        return true;
    };
    let Some(multiple_of) = multiple_of.as_f64() else {
        return true;
    };
    if !multiple_of.is_normal() || multiple_of <= 0.0 {
        return true;
    }

    let quotient = number / multiple_of;
    (quotient - quotient.round()).abs() <= f64::EPSILON * 8.0
}

fn compare_numbers(left: &Number, right: &Number) -> Option<Ordering> {
    if let (Some(left), Some(right)) = (
        number_as_integral_i128(left),
        number_as_integral_i128(right),
    ) {
        return Some(left.cmp(&right));
    }

    let left = left.as_f64()?;
    let right = right.as_f64()?;
    if !left.is_finite() || !right.is_finite() {
        return None;
    }
    left.partial_cmp(&right)
}

fn number_as_integral_i128(value: &Number) -> Option<i128> {
    if let Some(value) = value.as_i64() {
        Some(i128::from(value))
    } else {
        value.as_u64().map(i128::from)
    }
}

fn number_from_i128(value: i128) -> Option<Number> {
    i64::try_from(value)
        .map(Number::from)
        .or_else(|_| u64::try_from(value).map(Number::from))
        .ok()
}

fn finite_f64_to_i128(value: f64) -> Option<i128> {
    value.is_finite().then_some(value as i128)
}

fn positive_decimal_rational_from_number(number: &Number) -> Option<(i128, i128)> {
    let text = number.to_string();
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
        None => (text.as_str(), 0),
    };
    if mantissa.starts_with('-') {
        return None;
    }

    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    let numerator = digits.parse::<i128>().ok()?;
    if numerator <= 0 {
        return None;
    }

    let fraction_digits = i32::try_from(fraction.len()).ok()?;
    let scale = fraction_digits.checked_sub(exponent).unwrap_or(i32::MAX);
    if scale <= 0 {
        let multiplier = pow10_i128(scale.unsigned_abs())?;
        numerator.checked_mul(multiplier).map(|value| (value, 1))
    } else {
        pow10_i128(scale as u32).map(|denominator| (numerator, denominator))
    }
}

fn pow10_i128(exponent: u32) -> Option<i128> {
    let mut value = 1i128;
    for _ in 0..exponent {
        value = value.checked_mul(10)?;
    }
    Some(value)
}

fn gcd_i128(mut left: i128, mut right: i128) -> i128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.abs().max(1)
}

fn merge_example_value(target: &mut Option<Value>, overlay: Value) {
    match target {
        Some(current) => merge_example_branch(current, overlay),
        None => *target = Some(overlay),
    }
}

fn merge_example_branch(target: &mut Value, overlay: Value) {
    match (target, overlay) {
        (Value::Object(existing), Value::Object(overlay_map)) => {
            merge_example_object(existing, overlay_map);
        }
        (Value::Array(existing), Value::Array(overlay_items)) => {
            merge_example_array(existing, overlay_items);
        }
        (existing, other) if existing.is_null() && !other.is_null() => *existing = other,
        (existing, other) if !other.is_null() => *existing = other,
        _ => {}
    }
}

fn merge_example_object(
    target: &mut serde_json::Map<String, Value>,
    overlay: serde_json::Map<String, Value>,
) {
    for (key, value) in overlay {
        if let Some(existing) = target.get_mut(&key) {
            merge_example_branch(existing, value);
        } else {
            target.insert(key, value);
        }
    }
}

fn merge_example_array(target: &mut Vec<Value>, overlay: Vec<Value>) {
    for (index, value) in overlay.into_iter().enumerate() {
        if let Some(existing) = target.get_mut(index) {
            merge_example_branch(existing, value);
        } else {
            target.push(value);
        }
    }
}

fn dynamic_object_placeholder(reserved: &BTreeSet<String>) -> String {
    for candidate in ["{item}", "{key}", "{entry}", "{value}"] {
        if !reserved.contains(candidate) {
            return candidate.to_owned();
        }
    }

    let mut index = 0usize;
    loop {
        let candidate = format!("{{item_{index}}}");
        if !reserved.contains(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

pub(crate) fn dynamic_object_placeholder_for_schema(
    object: &serde_json::Map<String, Value>,
    root: &Value,
    reserved: &BTreeSet<String>,
) -> Option<String> {
    let property_names = object.get("propertyNames")?;
    let mut candidates = property_name_candidates(property_names);
    if candidates.is_empty() {
        candidates.extend([
            "item".to_owned(),
            "key".to_owned(),
            "entry".to_owned(),
            "value".to_owned(),
            "name".to_owned(),
            "field".to_owned(),
            "property".to_owned(),
            "primary".to_owned(),
            "secondary".to_owned(),
            "example".to_owned(),
            "default".to_owned(),
            "x".to_owned(),
            "0".to_owned(),
            "1".to_owned(),
        ]);
    }

    let mut taken = reserved.clone();
    for candidate in candidates {
        if taken.contains(&candidate) {
            continue;
        }
        if property_name_matches_schema(&candidate, property_names, root) {
            return Some(candidate);
        }
        taken.insert(candidate);
    }

    let mut index = 0usize;
    loop {
        for stem in ["item", "key", "entry", "value", "field", "name"] {
            let candidate = format!("{stem}_{index}");
            if reserved.contains(&candidate) {
                continue;
            }
            if property_name_matches_schema(&candidate, property_names, root) {
                return Some(candidate);
            }
        }
        if index > 1024 {
            break;
        }
        index += 1;
    }

    None
}

fn dynamic_object_placeholders_for_schema(
    object: &serde_json::Map<String, Value>,
    root: &Value,
    reserved: &BTreeSet<String>,
    count: usize,
) -> Vec<String> {
    let mut taken = reserved.clone();
    let mut placeholders = Vec::with_capacity(count);
    let constrained_by_property_names = object.contains_key("propertyNames");
    for _ in 0..count {
        let placeholder = if constrained_by_property_names {
            match dynamic_object_placeholder_for_schema(object, root, &taken) {
                Some(placeholder) => placeholder,
                None => break,
            }
        } else {
            dynamic_object_placeholder(&taken)
        };
        taken.insert(placeholder.clone());
        placeholders.push(placeholder);
    }
    placeholders
}

fn pattern_property_placeholder_for_schema(
    pattern: &str,
    object: &serde_json::Map<String, Value>,
    root: &Value,
    reserved: &BTreeSet<String>,
) -> Option<String> {
    let regex = Regex::new(pattern).ok()?;
    let prefix = literal_regex_prefix(pattern);
    let property_names = object.get("propertyNames");

    let mut candidates = Vec::new();
    if let Some(property_names) = property_names {
        candidates.extend(property_name_candidates(property_names));
    }
    if let Some(prefix) = &prefix
        && !prefix.is_empty()
    {
        candidates.extend([
            prefix.clone(),
            format!("{prefix}item"),
            format!("{prefix}token"),
            format!("{prefix}value"),
            format!("{prefix}example"),
            format!("{prefix}1"),
        ]);
    }
    candidates.extend(
        ["item", "key", "entry", "value", "example", "name", "field"]
            .into_iter()
            .map(ToOwned::to_owned),
    );

    for candidate in candidates {
        if !reserved.contains(&candidate)
            && regex.is_match(&candidate)
            && property_names.is_none_or(|property_names| {
                property_name_matches_schema(&candidate, property_names, root)
            })
        {
            return Some(candidate);
        }
    }

    for index in 0..1024 {
        for stem in ["item", "key", "entry", "value", "example", "name", "field"] {
            let candidate = match prefix.as_deref() {
                Some(prefix) if !prefix.is_empty() => format!("{prefix}{stem}{index}"),
                _ => format!("{stem}_{index}"),
            };
            if !reserved.contains(&candidate)
                && regex.is_match(&candidate)
                && property_names.is_none_or(|property_names| {
                    property_name_matches_schema(&candidate, property_names, root)
                })
            {
                return Some(candidate);
            }
        }
    }

    None
}

fn literal_regex_prefix(pattern: &str) -> Option<String> {
    let pattern = pattern.strip_prefix('^').unwrap_or(pattern);
    let mut prefix = String::new();
    let mut escaped = false;

    for ch in pattern.chars() {
        if escaped {
            prefix.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '$' | '.' | '*' | '+' | '?' | '|' | '(' | '[' | '{' => break,
            _ => prefix.push(ch),
        }
    }

    Some(prefix)
}

fn property_name_candidates(schema: &Value) -> Vec<String> {
    let mut candidates = Vec::new();

    if let Some(constant) = schema.get("const").and_then(Value::as_str) {
        candidates.push(constant.to_owned());
    }

    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        candidates.extend(
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned),
        );
    }

    candidates.sort();
    candidates.dedup();
    candidates
}

fn property_name_matches_schema(candidate: &str, schema: &Value, root: &Value) -> bool {
    example_matches_schema(
        &Value::String(candidate.to_owned()),
        schema,
        root,
        &mut BTreeSet::new(),
    )
}

fn trim_object_example_properties(
    rendered: &mut serde_json::Map<String, Value>,
    object: &serde_json::Map<String, Value>,
) {
    let Some(max_properties) = object
        .get("maxProperties")
        .and_then(Value::as_u64)
        .map(|max_properties| max_properties as usize)
    else {
        return;
    };

    if rendered.len() <= max_properties {
        return;
    }

    let required = object
        .get("required")
        .and_then(Value::as_array)
        .map(|required| {
            required
                .iter()
                .filter_map(Value::as_str)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    let required_present = rendered
        .keys()
        .filter(|key| required.contains(key.as_str()))
        .count();
    if required_present >= max_properties {
        rendered.retain(|key, _| required.contains(key.as_str()));
        return;
    }

    let mut optional_slots = max_properties - required_present;
    rendered.retain(|key, _| {
        if required.contains(key.as_str()) {
            true
        } else if optional_slots > 0 {
            optional_slots -= 1;
            true
        } else {
            false
        }
    });
}

fn allows_additional_array_items(
    object: &serde_json::Map<String, Value>,
    fixed_item_count: usize,
) -> bool {
    allows_additional_array_items_for_schema(object, fixed_item_count)
}

pub(crate) fn allows_additional_array_items_for_schema(
    object: &serde_json::Map<String, Value>,
    fixed_item_count: usize,
) -> bool {
    if additional_array_items_forbidden(object) {
        return false;
    }

    object
        .get("maxItems")
        .and_then(Value::as_u64)
        .is_none_or(|max_items| fixed_item_count < max_items as usize)
}

fn additional_array_items_forbidden(object: &serde_json::Map<String, Value>) -> bool {
    object
        .get("items")
        .is_some_and(|value| matches!(value, Value::Bool(false)))
        || legacy_additional_items_for_schema(object)
            .is_some_and(|value| matches!(value, Value::Bool(false)))
}

pub(crate) fn legacy_additional_items_for_schema(
    object: &serde_json::Map<String, Value>,
) -> Option<&Value> {
    object.get("items").filter(|value| value.is_array())?;
    object.get("additionalItems")
}

fn available_additional_array_slots(
    object: &serde_json::Map<String, Value>,
    existing_len: usize,
) -> usize {
    if !allows_additional_array_items(object, existing_len) {
        return 0;
    }

    object
        .get("maxItems")
        .and_then(Value::as_u64)
        .map_or(usize::MAX, |max_items| max_items as usize)
        .saturating_sub(existing_len)
}

fn array_requires_unique_items(object: &serde_json::Map<String, Value>) -> bool {
    object
        .get("uniqueItems")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn uniquify_merged_array_example(
    merged: &mut Option<Value>,
    object: &serde_json::Map<String, Value>,
    root: &Value,
) {
    if let Some(value) = merged {
        uniquify_example_value_in_place(value, object, root);
    }
}

fn uniquify_example_value_in_place(
    value: &mut Value,
    object: &serde_json::Map<String, Value>,
    root: &Value,
) {
    if let Value::Array(items) = value
        && array_requires_unique_items(object)
    {
        uniquify_array_example_items(items, object, root);
    }
}

fn array_item_schema(object: &serde_json::Map<String, Value>, index: usize) -> Option<&Value> {
    if let Some(prefix_items) = object.get("prefixItems").and_then(Value::as_array)
        && let Some(schema) = prefix_items.get(index)
    {
        return Some(schema);
    }

    if let Some(item_schemas) = object.get("items").and_then(Value::as_array)
        && let Some(schema) = item_schemas.get(index)
    {
        return Some(schema);
    }

    object
        .get("items")
        .filter(|value| !value.is_array() && !matches!(value, Value::Bool(false)))
        .or_else(|| {
            legacy_additional_items_for_schema(object)
                .filter(|value| !value.is_array() && !matches!(value, Value::Bool(false)))
        })
}

fn uniquify_array_example_items(
    items: &mut [Value],
    object: &serde_json::Map<String, Value>,
    root: &Value,
) {
    let mut seen = Vec::<Value>::new();
    for index in 0..items.len() {
        let Some((item, future)) = items[index..].split_first_mut() else {
            break;
        };
        if seen.contains(item)
            && let Some(schema) = array_item_schema(object, index)
        {
            let mut reserved = seen.clone();
            reserved.extend(future.iter().cloned());
            let unique = uniquify_example_value(item.clone(), schema, root, &reserved)
                .or_else(|| uniquify_example_value(item.clone(), schema, root, &seen));
            if let Some(unique) = unique {
                *item = unique;
            }
        }
        seen.push(item.clone());
    }
}

fn additional_example_item_count(
    object: &serde_json::Map<String, Value>,
    fixed_item_count: usize,
) -> usize {
    if !allows_additional_array_items(object, fixed_item_count) {
        return 0;
    }

    let required_additional = object
        .get("minItems")
        .and_then(Value::as_u64)
        .map_or(0, |min_items| {
            min_items.saturating_sub(fixed_item_count as u64) as usize
        });
    required_additional.max(1)
}

fn required_contains_item_count(object: &serde_json::Map<String, Value>) -> usize {
    if !object.contains_key("contains") {
        return 0;
    }

    object
        .get("minContains")
        .and_then(Value::as_u64)
        .map_or(1, |min_contains| min_contains as usize)
}

pub(crate) fn required_contains_additional_items_for_docs(
    object: &serde_json::Map<String, Value>,
    root: &Value,
) -> usize {
    let Some(contains) = object.get("contains") else {
        return 0;
    };

    let required_matches = required_contains_item_count(object);
    if required_matches == 0 {
        return 0;
    }

    let mut fixed_examples = merged_fixed_example_items(object, root);
    if array_requires_unique_items(object) {
        uniquify_array_example_items(&mut fixed_examples, object, root);
    }
    let existing_matches = count_matching_example_items(
        &fixed_examples,
        contains,
        root,
        array_requires_unique_items(object),
    );
    required_matches.saturating_sub(existing_matches)
}

fn merged_fixed_example_items(object: &serde_json::Map<String, Value>, root: &Value) -> Vec<Value> {
    let mut merged = None;

    if let Some(items) = object.get("prefixItems").and_then(Value::as_array) {
        let rendered = items
            .iter()
            .map(|child| {
                build_example_value(child, root, &mut BTreeSet::new(), None).unwrap_or(Value::Null)
            })
            .collect::<Vec<_>>();
        merge_example_value(&mut merged, Value::Array(rendered));
    }

    if let Some(items) = object.get("items").and_then(Value::as_array) {
        let rendered = items
            .iter()
            .map(|child| {
                build_example_value(child, root, &mut BTreeSet::new(), None).unwrap_or(Value::Null)
            })
            .collect::<Vec<_>>();
        merge_example_value(&mut merged, Value::Array(rendered));
    }

    merged
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
}

fn count_matching_example_items(
    values: &[Value],
    schema: &Value,
    root: &Value,
    unique_items: bool,
) -> usize {
    let mut matching = Vec::<Value>::new();
    for value in values {
        if !example_matches_schema(value, schema, root, &mut BTreeSet::new()) {
            continue;
        }
        if unique_items && matching.contains(value) {
            continue;
        }
        matching.push(value.clone());
    }
    matching.len()
}

fn build_repeated_example_values(
    example: Value,
    schema: &Value,
    root: &Value,
    count: usize,
    unique_items: bool,
    existing: &[Value],
) -> Vec<Value> {
    let mut taken = if unique_items {
        existing.to_vec()
    } else {
        Vec::new()
    };
    let mut rendered = Vec::with_capacity(count);
    for _ in 0..count {
        let value = if unique_items {
            uniquify_example_value(example.clone(), schema, root, &taken)
                .unwrap_or_else(|| example.clone())
        } else {
            example.clone()
        };
        if unique_items {
            taken.push(value.clone());
        }
        rendered.push(value);
    }
    rendered
}

fn uniquify_example_value(
    value: Value,
    schema: &Value,
    root: &Value,
    existing: &[Value],
) -> Option<Value> {
    if !existing.contains(&value) {
        return Some(value);
    }

    let specialized = match value {
        Value::String(text) => uniquify_string_example(text, schema, root, existing),
        Value::Number(number) => uniquify_number_example(number, schema, root, existing),
        Value::Bool(flag) => {
            let candidate = Value::Bool(!flag);
            (!existing.contains(&candidate)
                && example_matches_schema(&candidate, schema, root, &mut BTreeSet::new()))
            .then_some(candidate)
        }
        _ => None,
    };

    specialized.or_else(|| uniquify_generic_example_value(schema, root, existing))
}

fn uniquify_generic_example_value(
    schema: &Value,
    root: &Value,
    existing: &[Value],
) -> Option<Value> {
    let mut object = serde_json::Map::new();
    object.insert("example".to_owned(), Value::Bool(true));

    let candidates = [
        Value::Null,
        Value::Bool(false),
        Value::Bool(true),
        Value::Number(0.into()),
        Value::Number(1.into()),
        Value::String("example".to_owned()),
        Value::String("value".to_owned()),
        Value::Array(Vec::new()),
        Value::Array(vec![Value::Null]),
        Value::Object(serde_json::Map::new()),
        Value::Object(object),
    ];

    candidates.into_iter().find(|candidate| {
        !existing.contains(candidate)
            && example_matches_schema(candidate, schema, root, &mut BTreeSet::new())
    })
}

fn uniquify_string_example(
    text: String,
    schema: &Value,
    root: &Value,
    existing: &[Value],
) -> Option<Value> {
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        for value in values {
            let candidate = value.clone();
            if !existing.contains(&candidate)
                && example_matches_schema(&candidate, schema, root, &mut BTreeSet::new())
            {
                return Some(candidate);
            }
        }
    }

    for candidate in constrained_string_unique_candidates(&text, schema) {
        if !existing.contains(&candidate)
            && example_matches_schema(&candidate, schema, root, &mut BTreeSet::new())
        {
            return Some(candidate);
        }
    }

    for attempt in 1..=1024 {
        for candidate in [
            Value::String(format!("{text}-{attempt}")),
            Value::String(format!("{text}_{attempt}")),
            Value::String(format!("item{attempt}")),
            Value::String(format!("value{attempt}")),
        ] {
            if !existing.contains(&candidate)
                && example_matches_schema(&candidate, schema, root, &mut BTreeSet::new())
            {
                return Some(candidate);
            }
        }
    }

    None
}

fn constrained_string_unique_candidates(text: &str, schema: &Value) -> Vec<Value> {
    let min_length = schema
        .get("minLength")
        .and_then(Value::as_u64)
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or(0);
    let max_length = schema
        .get("maxLength")
        .and_then(Value::as_u64)
        .and_then(|length| usize::try_from(length).ok());

    let mut lengths = BTreeSet::new();
    let text_length = text.chars().count();
    if text_length > 0 {
        lengths.insert(text_length);
    }
    if min_length > 0 {
        lengths.insert(min_length);
    }
    if let Some(max_length) = max_length
        && max_length > 0
    {
        lengths.insert(max_length);
    }

    let mut candidates = Vec::new();
    for length in lengths.into_iter().take(8) {
        if max_length.is_some_and(|max_length| length > max_length) || length < min_length {
            continue;
        }
        for seed in ['a', 'b', 'c', 'x', 'y', 'z', '0', '1', '2', 'A', 'B', 'C'] {
            let candidate = std::iter::repeat_n(seed, length).collect::<String>();
            if candidate != text {
                candidates.push(Value::String(candidate));
            }
        }
    }

    candidates
}

fn uniquify_number_example(
    number: serde_json::Number,
    schema: &Value,
    root: &Value,
    existing: &[Value],
) -> Option<Value> {
    if let Some(integer) = number.as_i64() {
        return uniquify_integer_number_example(i128::from(integer), schema, root, existing);
    }

    if let Some(integer) = number.as_u64() {
        return uniquify_integer_number_example(i128::from(integer), schema, root, existing);
    }

    let base = number.as_f64()?;
    let step = schema
        .get("multipleOf")
        .and_then(Value::as_f64)
        .filter(|step| step.is_normal() && *step > 0.0)
        .unwrap_or(1.0);
    for attempt in 1..=1024u64 {
        let candidate =
            serde_json::Number::from_f64(base + step * attempt as f64).map(Value::Number)?;
        if !existing.contains(&candidate)
            && example_matches_schema(&candidate, schema, root, &mut BTreeSet::new())
        {
            return Some(candidate);
        }
    }

    None
}

fn uniquify_integer_number_example(
    integer: i128,
    schema: &Value,
    root: &Value,
    existing: &[Value],
) -> Option<Value> {
    if let Some(object) = schema.as_object()
        && let Some(step) = integer_multiple_step_constraint(object)
    {
        let mut next = integer
            .checked_add(1)
            .and_then(|lower| first_integer_multiple_at_or_above(lower, step));
        let mut previous = integer
            .checked_sub(1)
            .and_then(|upper| last_integer_multiple_at_or_below(upper, step));

        for _ in 0..1024 {
            if let Some(value) =
                next.and_then(number_from_i128)
                    .map(Value::Number)
                    .filter(|value| {
                        !existing.contains(value)
                            && example_matches_schema(value, schema, root, &mut BTreeSet::new())
                    })
            {
                return Some(value);
            }
            if let Some(value) = previous
                .and_then(number_from_i128)
                .map(Value::Number)
                .filter(|value| {
                    !existing.contains(value)
                        && example_matches_schema(value, schema, root, &mut BTreeSet::new())
                })
            {
                return Some(value);
            }

            next = next.and_then(|candidate| candidate.checked_add(step));
            previous = previous.and_then(|candidate| candidate.checked_sub(step));
        }
        return None;
    }

    let mut candidate = integer;
    for _ in 0..1024 {
        let Some(next) = candidate.checked_add(1) else {
            break;
        };
        candidate = next;
        if let Some(value) = number_from_i128(candidate).map(Value::Number)
            && !existing.contains(&value)
            && example_matches_schema(&value, schema, root, &mut BTreeSet::new())
        {
            return Some(value);
        }
    }

    let mut candidate = integer;
    for _ in 0..1024 {
        let Some(previous) = candidate.checked_sub(1) else {
            break;
        };
        candidate = previous;
        if let Some(value) = number_from_i128(candidate).map(Value::Number)
            && !existing.contains(&value)
            && example_matches_schema(&value, schema, root, &mut BTreeSet::new())
        {
            return Some(value);
        }
    }

    None
}

fn example_matches_schema(
    value: &Value,
    schema: &Value,
    root: &Value,
    visited_refs: &mut BTreeSet<String>,
) -> bool {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if !visited_refs.insert(reference.to_owned()) {
            return true;
        }

        let result = inlined_schema_ref(schema, root)
            .is_some_and(|inlined| example_matches_schema(value, &inlined, root, visited_refs));
        visited_refs.remove(reference);
        return result;
    }

    let Some(object) = schema.as_object() else {
        return schema.as_bool().unwrap_or(true);
    };

    if let Some(constant) = object.get("const") {
        return value == constant;
    }

    if let Some(values) = object.get("enum").and_then(Value::as_array) {
        return values.contains(value);
    }

    if let Some(children) = object.get("allOf").and_then(Value::as_array)
        && !children.iter().all(|child| {
            let mut branch_refs = visited_refs.clone();
            example_matches_schema(value, child, root, &mut branch_refs)
        })
    {
        return false;
    }

    if let Some(children) = object.get("anyOf").and_then(Value::as_array)
        && !children.iter().any(|child| {
            let mut branch_refs = visited_refs.clone();
            example_matches_schema(value, child, root, &mut branch_refs)
        })
    {
        return false;
    }

    if let Some(children) = object.get("oneOf").and_then(Value::as_array) {
        let matches = children
            .iter()
            .filter(|child| {
                let mut branch_refs = visited_refs.clone();
                example_matches_schema(value, child, root, &mut branch_refs)
            })
            .take(2)
            .count();
        if matches != 1 {
            return false;
        }
    }

    if let Some(types) = object.get("type")
        && !value_matches_schema_type(value, types)
    {
        return false;
    }

    if let Value::Number(number) = value
        && !number_matches_numeric_schema(number, object)
    {
        return false;
    }

    match value {
        Value::String(text) => {
            if object
                .get("minLength")
                .and_then(Value::as_u64)
                .is_some_and(|min_length| text.chars().count() < min_length as usize)
            {
                return false;
            }
            if object
                .get("maxLength")
                .and_then(Value::as_u64)
                .is_some_and(|max_length| text.chars().count() > max_length as usize)
            {
                return false;
            }
            if let Some(pattern) = object.get("pattern").and_then(Value::as_str)
                && !Regex::new(pattern).is_ok_and(|regex| regex.is_match(text))
            {
                return false;
            }
        }
        Value::Number(_) => {}
        Value::Object(map) => {
            if object
                .get("minProperties")
                .and_then(Value::as_u64)
                .is_some_and(|min_properties| map.len() < min_properties as usize)
            {
                return false;
            }
            if object
                .get("maxProperties")
                .and_then(Value::as_u64)
                .is_some_and(|max_properties| map.len() > max_properties as usize)
            {
                return false;
            }

            if let Some(property_names) = object.get("propertyNames")
                && map.keys().any(|key| {
                    !example_matches_schema(
                        &Value::String(key.clone()),
                        property_names,
                        root,
                        &mut visited_refs.clone(),
                    )
                })
            {
                return false;
            }

            if let Some(required) = object.get("required").and_then(Value::as_array)
                && required
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|key| !map.contains_key(key))
            {
                return false;
            }

            if let Some(properties) = object.get("properties").and_then(Value::as_object) {
                for (key, child_schema) in properties {
                    if let Some(child_value) = map.get(key)
                        && !example_matches_schema(child_value, child_schema, root, visited_refs)
                    {
                        return false;
                    }
                }
            }

            let pattern_properties = object.get("patternProperties").and_then(Value::as_object);
            let fixed_properties = object
                .get("properties")
                .and_then(Value::as_object)
                .map_or_else(BTreeSet::new, |properties| {
                    properties.keys().cloned().collect::<BTreeSet<_>>()
                });
            let mut pattern_matched_keys = BTreeSet::new();
            if let Some(pattern_properties) = pattern_properties {
                for (key, child_value) in map {
                    for (pattern, child_schema) in pattern_properties {
                        if Regex::new(pattern).is_ok_and(|regex| regex.is_match(key)) {
                            pattern_matched_keys.insert(key.clone());
                            if !example_matches_schema(
                                child_value,
                                child_schema,
                                root,
                                visited_refs,
                            ) {
                                return false;
                            }
                        }
                    }
                }
            }

            if let Some(additional) = object.get("additionalProperties") {
                for (key, child_value) in map {
                    if !fixed_properties.contains(key)
                        && !pattern_matched_keys.contains(key)
                        && !example_matches_schema(child_value, additional, root, visited_refs)
                    {
                        return false;
                    }
                }
            }
        }
        Value::Array(items) => {
            if object
                .get("minItems")
                .and_then(Value::as_u64)
                .is_some_and(|min_items| items.len() < min_items as usize)
            {
                return false;
            }
            if object
                .get("maxItems")
                .and_then(Value::as_u64)
                .is_some_and(|max_items| items.len() > max_items as usize)
            {
                return false;
            }

            if array_requires_unique_items(object) {
                let mut seen = Vec::<Value>::new();
                for item in items {
                    if seen.contains(item) {
                        return false;
                    }
                    seen.push(item.clone());
                }
            }

            if let Some(prefix_items) = object.get("prefixItems").and_then(Value::as_array) {
                for (index, child_schema) in prefix_items.iter().enumerate() {
                    if let Some(child_value) = items.get(index)
                        && !example_matches_schema(child_value, child_schema, root, visited_refs)
                    {
                        return false;
                    }
                }
            }

            if let Some(item_schemas) = object.get("items").and_then(Value::as_array) {
                for (index, child_schema) in item_schemas.iter().enumerate() {
                    if let Some(child_value) = items.get(index)
                        && !example_matches_schema(child_value, child_schema, root, visited_refs)
                    {
                        return false;
                    }
                }
            }

            let fixed_item_count = object
                .get("prefixItems")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
                .max(
                    object
                        .get("items")
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len),
                );

            if let Some(items_schema) = object.get("items").filter(|value| !value.is_array()) {
                for child_value in items.iter().skip(fixed_item_count) {
                    if !example_matches_schema(child_value, items_schema, root, visited_refs) {
                        return false;
                    }
                }
            }

            if let Some(additional_schema) =
                legacy_additional_items_for_schema(object).filter(|value| !value.is_array())
            {
                for child_value in items.iter().skip(fixed_item_count) {
                    if !example_matches_schema(child_value, additional_schema, root, visited_refs) {
                        return false;
                    }
                }
            }

            if let Some(contains_schema) = object.get("contains") {
                let matching_items = items
                    .iter()
                    .filter(|child_value| {
                        example_matches_schema(child_value, contains_schema, root, visited_refs)
                    })
                    .count();
                let min_contains = required_contains_item_count(object);
                let max_contains = object
                    .get("maxContains")
                    .and_then(Value::as_u64)
                    .map_or(usize::MAX, |max_contains| max_contains as usize);
                if matching_items < min_contains || matching_items > max_contains {
                    return false;
                }
            }
        }
        _ => {}
    }

    true
}

fn value_matches_schema_type(value: &Value, types: &Value) -> bool {
    match types {
        Value::String(ty) => value_matches_single_schema_type(value, ty),
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .any(|ty| value_matches_single_schema_type(value, ty)),
        _ => true,
    }
}

fn value_matches_single_schema_type(value: &Value, ty: &str) -> bool {
    match ty {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "string" => value.is_string(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => true,
    }
}

fn merged_object_level_property_names(
    schema: &Value,
    root: &Value,
    inherited: Option<&BTreeSet<String>>,
) -> BTreeSet<String> {
    let mut reserved = BTreeSet::new();
    collect_object_level_property_names(schema, root, &mut reserved, &mut BTreeSet::new());
    if let Some(inherited) = inherited {
        reserved.extend(inherited.iter().cloned());
    }
    reserved
}

fn collect_object_level_property_names(
    schema: &Value,
    root: &Value,
    reserved: &mut BTreeSet<String>,
    visited_refs: &mut BTreeSet<String>,
) {
    let Some(object) = schema.as_object() else {
        return;
    };

    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        if visited_refs.insert(reference.to_owned()) {
            if let Some(inlined) = inlined_schema_ref(schema, root) {
                collect_object_level_property_names(&inlined, root, reserved, visited_refs);
            }
            visited_refs.remove(reference);
        }
        return;
    }

    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        reserved.extend(properties.keys().cloned());
    }

    for keyword in ["allOf", "anyOf", "oneOf"] {
        if let Some(children) = object.get(keyword).and_then(Value::as_array) {
            for child in children {
                collect_object_level_property_names(child, root, reserved, visited_refs);
            }
        }
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
        _ if object.contains_key("const") => object
            .get("const")
            .map_or_else(|| "object".to_owned(), infer_type_from_value),
        _ if object.contains_key("enum") => "enum".to_owned(),
        _ if object.contains_key("items") || object.contains_key("prefixItems") => {
            "array".to_owned()
        }
        _ => "object".to_owned(),
    }
}

fn infer_type_from_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(_) => "boolean".to_owned(),
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                "integer".to_owned()
            } else {
                "number".to_owned()
            }
        }
        Value::String(_) => "string".to_owned(),
        Value::Array(_) => "array".to_owned(),
        Value::Object(_) => "object".to_owned(),
    }
}

fn resolve_schema_ref<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    root.pointer(pointer)
}

fn inlined_schema_ref(schema: &Value, root: &Value) -> Option<Value> {
    let reference = schema.get("$ref").and_then(Value::as_str)?;
    let target = resolve_schema_ref(root, reference)?;
    let mut inlined = target.clone();
    if let (Some(inlined_object), Some(reference_object)) =
        (inlined.as_object_mut(), schema.as_object())
    {
        for (key, value) in reference_object {
            if key != "$ref" {
                merge_schema_keyword(inlined_object, key, value);
            }
        }
    }
    Some(inlined)
}

fn merge_schema_keyword(target: &mut serde_json::Map<String, Value>, key: &str, overlay: &Value) {
    match key {
        "required" => {
            let mut merged = target
                .get(key)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(values) = overlay.as_array() {
                for value in values {
                    if !merged.contains(value) {
                        merged.push(value.clone());
                    }
                }
            } else {
                target.insert(key.to_owned(), overlay.clone());
                return;
            }
            target.insert(key.to_owned(), Value::Array(merged));
        }
        "prefixItems" | "items" if overlay.is_array() => {
            let mut merged = target
                .get(key)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(values) = overlay.as_array() {
                merge_schema_arrays(&mut merged, values);
                target.insert(key.to_owned(), Value::Array(merged));
            } else {
                target.insert(key.to_owned(), overlay.clone());
            }
        }
        "allOf" | "anyOf" | "oneOf" => {
            let mut merged = target
                .get(key)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(values) = overlay.as_array() {
                merged.extend(values.iter().cloned());
                target.insert(key.to_owned(), Value::Array(merged));
            } else {
                target.insert(key.to_owned(), overlay.clone());
            }
        }
        _ => match (target.get_mut(key), overlay) {
            (Some(Value::Object(existing)), Value::Object(overlay_map)) => {
                merge_schema_objects(existing, overlay_map);
            }
            _ => {
                target.insert(key.to_owned(), overlay.clone());
            }
        },
    }
}

fn merge_schema_objects(
    target: &mut serde_json::Map<String, Value>,
    overlay: &serde_json::Map<String, Value>,
) {
    for (key, value) in overlay {
        merge_schema_keyword(target, key, value);
    }
}

fn merge_schema_arrays(target: &mut Vec<Value>, overlay: &[Value]) {
    for (index, value) in overlay.iter().enumerate() {
        if value.is_null() {
            continue;
        }
        if let Some(existing) = target.get_mut(index) {
            merge_schema_value(existing, value);
        } else {
            target.push(value.clone());
        }
    }
}

fn merge_schema_value(target: &mut Value, overlay: &Value) {
    match overlay {
        Value::Object(overlay_map) if target.is_object() => {
            let Value::Object(existing) = target else {
                unreachable!("checked object target")
            };
            merge_schema_objects(existing, overlay_map);
        }
        Value::Array(overlay_items) if target.is_array() => {
            let Value::Array(existing) = target else {
                unreachable!("checked array target")
            };
            merge_schema_arrays(existing, overlay_items);
        }
        _ => *target = overlay.clone(),
    }
}
