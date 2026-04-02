use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;
use serde_json::Value;

#[cfg(feature = "toml")]
use crate::report::path_matches_pattern;
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
    build_example_value(&schema, &schema, &mut BTreeSet::new(), None)
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
        annotate_schema_path(schema, &snapshot, &field.path, field);
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
        if let Some(additional) = object
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

    if let Some(additional) = object
        .get("additionalItems")
        .filter(|value| !value.is_array())
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
        if let Value::Array(items) = &mut merged
            && array_requires_unique_items(object)
        {
            uniquify_array_example_items(items, object, root);
        }
        return Some(if is_secret {
            redact_example_value(&merged)
        } else {
            merged
        });
    }

    let fallback = match schema_type(object).as_str() {
        "string" => Some(Value::String(fallback_string_example(object))),
        "integer" => Some(Value::Number(fallback_integer_example(object).into())),
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

fn fallback_integer_example(object: &serde_json::Map<String, Value>) -> i64 {
    let lower = object
        .get("exclusiveMinimum")
        .and_then(Value::as_f64)
        .map_or_else(
            || {
                object
                    .get("minimum")
                    .and_then(Value::as_f64)
                    .map(|minimum| minimum.ceil() as i64)
            },
            |minimum| Some(minimum.floor() as i64 + 1),
        );
    let upper = object
        .get("exclusiveMaximum")
        .and_then(Value::as_f64)
        .map_or_else(
            || {
                object
                    .get("maximum")
                    .and_then(Value::as_f64)
                    .map(|maximum| maximum.floor() as i64)
            },
            |maximum| Some(maximum.ceil() as i64 - 1),
        );

    integer_candidate_in_range(lower.unwrap_or(0), upper, object)
        .or(upper)
        .unwrap_or(lower.unwrap_or(0))
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
    lower: i64,
    upper: Option<i64>,
    object: &serde_json::Map<String, Value>,
) -> Option<i64> {
    let mut candidate = lower;
    let max_iterations = upper
        .map(|upper| upper.saturating_sub(lower).saturating_add(1) as usize)
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

fn integer_value_matches_constraints(value: i64, object: &serde_json::Map<String, Value>) -> bool {
    let as_f64 = value as f64;

    if object
        .get("minimum")
        .and_then(Value::as_f64)
        .is_some_and(|minimum| as_f64 < minimum)
    {
        return false;
    }
    if object
        .get("maximum")
        .and_then(Value::as_f64)
        .is_some_and(|maximum| as_f64 > maximum)
    {
        return false;
    }
    if object
        .get("exclusiveMinimum")
        .and_then(Value::as_f64)
        .is_some_and(|minimum| as_f64 <= minimum)
    {
        return false;
    }
    if object
        .get("exclusiveMaximum")
        .and_then(Value::as_f64)
        .is_some_and(|maximum| as_f64 >= maximum)
    {
        return false;
    }

    if let Some(multiple_of) = object.get("multipleOf").and_then(Value::as_f64)
        && multiple_of.is_normal()
        && multiple_of > 0.0
    {
        let quotient = as_f64 / multiple_of;
        if (quotient - quotient.round()).abs() > f64::EPSILON * 8.0 {
            return false;
        }
    }

    true
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
    object
        .get("maxItems")
        .and_then(Value::as_u64)
        .is_none_or(|max_items| fixed_item_count < max_items as usize)
}

fn available_additional_array_slots(
    object: &serde_json::Map<String, Value>,
    existing_len: usize,
) -> usize {
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
            object
                .get("additionalItems")
                .filter(|value| !value.is_array() && !matches!(value, Value::Bool(false)))
        })
}

fn uniquify_array_example_items(
    items: &mut [Value],
    object: &serde_json::Map<String, Value>,
    root: &Value,
) {
    let mut seen = Vec::<Value>::new();
    for (index, item) in items.iter_mut().enumerate() {
        if seen.contains(item)
            && let Some(schema) = array_item_schema(object, index)
            && let Some(unique) = uniquify_example_value(item.clone(), schema, root, &seen)
        {
            *item = unique;
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

    let fixed_examples = merged_fixed_example_items(object, root);
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

    match value {
        Value::String(text) => uniquify_string_example(text, schema, root, existing),
        Value::Number(number) => uniquify_number_example(number, schema, root, existing),
        Value::Bool(flag) => {
            let candidate = Value::Bool(!flag);
            (!existing.contains(&candidate)
                && example_matches_schema(&candidate, schema, root, &mut BTreeSet::new()))
            .then_some(candidate)
        }
        _ => None,
    }
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

fn uniquify_number_example(
    number: serde_json::Number,
    schema: &Value,
    root: &Value,
    existing: &[Value],
) -> Option<Value> {
    if let Some(integer) = number.as_i64() {
        for attempt in 1..=1024i64 {
            let candidate = Value::Number((integer.saturating_add(attempt)).into());
            if !existing.contains(&candidate)
                && example_matches_schema(&candidate, schema, root, &mut BTreeSet::new())
            {
                return Some(candidate);
            }
        }
        return None;
    }

    if let Some(integer) = number.as_u64() {
        for attempt in 1..=1024u64 {
            let candidate = Value::Number((integer.saturating_add(attempt)).into());
            if !existing.contains(&candidate)
                && example_matches_schema(&candidate, schema, root, &mut BTreeSet::new())
            {
                return Some(candidate);
            }
        }
        return None;
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

    if let Some(multiple_of) = object.get("multipleOf").and_then(Value::as_f64)
        && let Some(number) = value.as_f64()
        && multiple_of.is_normal()
    {
        let quotient = number / multiple_of;
        if (quotient - quotient.round()).abs() > f64::EPSILON * 8.0 {
            return false;
        }
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
        Value::Number(number) => {
            if let Some(number) = number.as_f64() {
                if object
                    .get("minimum")
                    .and_then(Value::as_f64)
                    .is_some_and(|minimum| number < minimum)
                {
                    return false;
                }
                if object
                    .get("maximum")
                    .and_then(Value::as_f64)
                    .is_some_and(|maximum| number > maximum)
                {
                    return false;
                }
                if object
                    .get("exclusiveMinimum")
                    .and_then(Value::as_f64)
                    .is_some_and(|minimum| number <= minimum)
                {
                    return false;
                }
                if object
                    .get("exclusiveMaximum")
                    .and_then(Value::as_f64)
                    .is_some_and(|maximum| number >= maximum)
                {
                    return false;
                }
            }
        }
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

            if let Some(additional_schema) = object
                .get("additionalItems")
                .filter(|value| !value.is_array())
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

#[cfg(feature = "toml")]
fn render_example_toml(value: &Value, metadata: &ConfigMetadata) -> String {
    let mut output = String::new();
    render_toml_root_comments(metadata, &mut output);
    match value {
        Value::Object(root) => render_toml_table("", "", root, metadata, &mut output),
        other => render_toml_root_value(other, metadata, &mut output),
    }

    if !output.ends_with('\n') {
        output.push('\n');
    }

    output
}

#[cfg(feature = "toml")]
fn render_toml_root_value(value: &Value, metadata: &ConfigMetadata, output: &mut String) {
    if matches!(value, Value::Array(_)) {
        render_toml_inline_array_item_comments("", metadata, output);
    }

    output.push_str("# root values are rendered as comments because TOML documents require a table at the top level\n");
    output.push_str("# ");
    output.push_str(&toml_inline_value(value));
    output.push('\n');
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
    display_path: &str,
    metadata_path: &str,
    table: &serde_json::Map<String, Value>,
    metadata: &ConfigMetadata,
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

        let metadata_child_path =
            resolve_toml_object_child_metadata_path(metadata_path, key, metadata);
        render_toml_comments(&metadata_child_path, metadata, output);
        if matches!(value, Value::Array(_)) {
            render_toml_inline_array_item_comments(&metadata_child_path, metadata, output);
        }
        output.push_str(&toml_key(key));
        output.push_str(" = ");
        output.push_str(&toml_inline_value(value));
        output.push('\n');
    }

    let nested_count = nested.len();
    for (index, (key, value)) in nested.into_iter().enumerate() {
        let display_child_path = join_metadata_path(display_path, key);
        let metadata_child_path =
            resolve_toml_object_child_metadata_path(metadata_path, key, metadata);
        match value {
            Value::Object(child) => {
                start_toml_section(output);
                render_toml_comments(&metadata_child_path, metadata, output);
                output.push('[');
                output.push_str(&toml_table_name(&display_child_path));
                output.push_str("]\n");
                render_toml_table(
                    &display_child_path,
                    &metadata_child_path,
                    child,
                    metadata,
                    output,
                );
            }
            Value::Array(items) => {
                for (item_index, item) in items.iter().enumerate() {
                    let Some(child) = item.as_object() else {
                        continue;
                    };
                    let item_metadata_path = resolve_toml_array_item_metadata_path(
                        &metadata_child_path,
                        item_index,
                        metadata,
                    );
                    start_toml_section(output);
                    if item_index == 0 {
                        render_toml_comments(&metadata_child_path, metadata, output);
                    }
                    render_toml_array_table_item_comments(&item_metadata_path, metadata, output);
                    output.push_str("[[");
                    output.push_str(&toml_table_name(&display_child_path));
                    output.push_str("]]\n");
                    render_toml_table(
                        &display_child_path,
                        &item_metadata_path,
                        child,
                        metadata,
                        output,
                    );
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
fn render_toml_comments(path: &str, metadata: &ConfigMetadata, output: &mut String) {
    for field in toml_comment_fields(path, metadata) {
        for line in toml_comment_lines(field) {
            output.push_str("# ");
            output.push_str(&line);
            output.push('\n');
        }
    }
}

#[cfg(feature = "toml")]
fn render_toml_array_table_item_comments(
    path: &str,
    metadata: &ConfigMetadata,
    output: &mut String,
) {
    for field in toml_comment_fields(path, metadata) {
        for line in toml_comment_lines(field) {
            output.push_str("# ");
            output.push_str(&line);
            output.push('\n');
        }
    }
}

#[cfg(feature = "toml")]
fn render_toml_inline_array_item_comments(
    path: &str,
    metadata: &ConfigMetadata,
    output: &mut String,
) {
    let prefix = if path.is_empty() {
        String::new()
    } else {
        format!("{path}.")
    };
    let mut wildcard = None::<&FieldMetadata>;
    let mut indexed = BTreeMap::<Vec<TomlArrayCommentSegment>, &FieldMetadata>::new();
    for field in metadata.fields() {
        let Some(rest) = field.path.strip_prefix(&prefix) else {
            continue;
        };
        if rest == "*" {
            wildcard = Some(field);
            continue;
        }
        let segments = rest
            .split('.')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.is_empty() {
            continue;
        }
        if !segments
            .iter()
            .all(|segment| *segment == "*" || segment.parse::<usize>().is_ok())
        {
            continue;
        }
        indexed.insert(parse_toml_comment_segments(&segments), field);
    }

    if let Some(field) = wildcard {
        for line in toml_comment_lines(field) {
            output.push_str("# [*] ");
            output.push_str(&line);
            output.push('\n');
        }
    }

    for (segments, field) in indexed {
        let marker = segments
            .iter()
            .map(|segment| match segment {
                TomlArrayCommentSegment::Wildcard => "[*]".to_owned(),
                TomlArrayCommentSegment::Index(index) => format!("[{index}]"),
            })
            .collect::<String>();
        for line in toml_comment_lines(field) {
            output.push_str("# ");
            output.push_str(&marker);
            output.push(' ');
            output.push_str(&line);
            output.push('\n');
        }
    }
}

#[cfg(feature = "toml")]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum TomlArrayCommentSegment {
    Wildcard,
    Index(usize),
}

#[cfg(feature = "toml")]
fn parse_toml_comment_segments(segments: &[&str]) -> Vec<TomlArrayCommentSegment> {
    segments
        .iter()
        .map(|segment| {
            if *segment == "*" {
                TomlArrayCommentSegment::Wildcard
            } else {
                TomlArrayCommentSegment::Index(
                    segment
                        .parse::<usize>()
                        .expect("validated numeric array comment index"),
                )
            }
        })
        .collect()
}

#[cfg(feature = "toml")]
fn toml_comment_sort_key(path: &str) -> (usize, usize) {
    let segments = path.split('.').filter(|segment| !segment.is_empty());
    let total = segments.clone().count();
    let specificity = segments.filter(|segment| *segment != "*").count();
    (specificity, total)
}

#[cfg(feature = "toml")]
fn toml_comment_fields<'a>(path: &str, metadata: &'a ConfigMetadata) -> Vec<&'a FieldMetadata> {
    let mut fields = metadata
        .fields()
        .iter()
        .filter(|field| field.path == path || path_matches_pattern(path, &field.path))
        .collect::<Vec<_>>();
    fields.sort_by(|left, right| {
        toml_comment_sort_key(&left.path)
            .cmp(&toml_comment_sort_key(&right.path))
            .then_with(|| left.path.cmp(&right.path))
    });
    fields
}

#[cfg(feature = "toml")]
fn toml_comment_lines(field: &FieldMetadata) -> Vec<String> {
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
    lines
}

#[cfg(feature = "toml")]
fn resolve_toml_array_item_metadata_path(
    parent_path: &str,
    index: usize,
    metadata: &ConfigMetadata,
) -> String {
    let concrete = join_metadata_path(parent_path, &index.to_string());
    if metadata_has_path_or_descendant(metadata, &concrete) {
        concrete
    } else {
        join_metadata_path(parent_path, "*")
    }
}

#[cfg(feature = "toml")]
fn metadata_has_path_or_descendant(metadata: &ConfigMetadata, path: &str) -> bool {
    let prefix = if path.is_empty() {
        String::new()
    } else {
        format!("{path}.")
    };
    metadata
        .fields()
        .iter()
        .any(|field| field.path == path || field.path.starts_with(&prefix))
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
        Value::Array(items) => !items.is_empty() && items.iter().all(Value::is_object),
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

#[cfg(feature = "toml")]
fn resolve_toml_object_child_metadata_path(
    parent_path: &str,
    key: &str,
    metadata: &ConfigMetadata,
) -> String {
    let literal = join_metadata_path(parent_path, key);
    if key != "{item}" || metadata_has_path_or_descendant(metadata, &literal) {
        return literal;
    }

    join_metadata_path(parent_path, "*")
}
