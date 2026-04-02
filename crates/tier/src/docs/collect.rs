use super::*;

pub(super) fn collect_env_docs(
    schema: &Value,
    root: &Value,
    path: &str,
    required: bool,
    docs: &mut Vec<EnvDocEntry>,
    visited_refs: &mut BTreeSet<String>,
    scope_reserved_keys: Option<&BTreeSet<String>>,
) {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if visited_refs.insert(reference.to_owned()) {
            if let Some(inlined) = inlined_schema_ref(schema, root) {
                collect_env_docs(
                    &inlined,
                    root,
                    path,
                    required,
                    docs,
                    visited_refs,
                    scope_reserved_keys,
                );
            }
            visited_refs.remove(reference);
        }
        return;
    }

    let Some(object) = schema.as_object() else {
        match schema {
            Value::Bool(true) if !path.is_empty() => {
                docs.push(EnvDocEntry {
                    path: path.to_owned(),
                    env: String::new(),
                    ty: "any".to_owned(),
                    required,
                    secret: false,
                    description: None,
                    example: None,
                    deprecated: None,
                    aliases: Vec::new(),
                    has_default: false,
                    merge: MergeStrategy::Merge,
                    validations: Vec::new(),
                });
            }
            Value::Bool(false) => {}
            _ if !path.is_empty() => {
                docs.push(EnvDocEntry {
                    path: path.to_owned(),
                    env: String::new(),
                    ty: "unknown".to_owned(),
                    required,
                    secret: false,
                    description: None,
                    example: None,
                    deprecated: None,
                    aliases: Vec::new(),
                    has_default: false,
                    merge: MergeStrategy::Merge,
                    validations: Vec::new(),
                });
            }
            _ => {}
        }
        return;
    };
    let reserved_keys = merged_object_level_property_names(schema, root, scope_reserved_keys);
    let required_properties = object
        .get("required")
        .and_then(Value::as_array)
        .map(|required| {
            required
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let min_properties = object
        .get("minProperties")
        .and_then(Value::as_u64)
        .map_or(0, |min_properties| min_properties as usize);

    let mut traversed_combinator = false;
    for keyword in ["allOf", "anyOf", "oneOf"] {
        if let Some(children) = object.get(keyword).and_then(Value::as_array) {
            traversed_combinator = true;
            for child in children {
                let is_null = child
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|ty| ty == "null");
                if !is_null {
                    let branch_required = if keyword == "allOf" { required } else { false };
                    collect_env_docs(
                        child,
                        root,
                        path,
                        branch_required,
                        docs,
                        visited_refs,
                        Some(&reserved_keys),
                    );
                }
            }
        }
    }

    let mut traversed_children = traversed_combinator;

    let properties = object.get("properties").and_then(Value::as_object);
    if let Some(properties) = properties {
        traversed_children = true;
        let max_properties = object
            .get("maxProperties")
            .and_then(Value::as_u64)
            .map(|max_properties| max_properties as usize);
        let additional_properties_forbidden = object
            .get("additionalProperties")
            .and_then(Value::as_bool)
            .is_some_and(|allowed| !allowed);
        let all_known_properties_required = additional_properties_forbidden
            && !properties.is_empty()
            && min_properties >= properties.len();
        let allows_optional_properties =
            max_properties.is_none_or(|max_properties| max_properties > required_properties.len());

        for (key, child_schema) in properties {
            let property_required =
                required && (required_properties.contains(key) || all_known_properties_required);
            if !property_required && !allows_optional_properties {
                continue;
            }
            let next = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            collect_env_docs(
                child_schema,
                root,
                &next,
                property_required,
                docs,
                visited_refs,
                None,
            );
        }
    }

    let known_property_count = properties.map_or(0, serde_json::Map::len);
    let max_properties = object
        .get("maxProperties")
        .and_then(Value::as_u64)
        .map(|max_properties| max_properties as usize);
    let required_dynamic_properties = required && min_properties > known_property_count;
    let required_fixed_properties = required_properties.len();
    let allows_dynamic_properties =
        max_properties.is_none_or(|max_properties| max_properties > required_fixed_properties);

    if let Some(pattern_properties) = object.get("patternProperties").and_then(Value::as_object) {
        traversed_children = true;
        let property_names_allow_dynamic_keys = object.get("propertyNames").is_none_or(|_| {
            dynamic_object_placeholder_for_schema(object, root, &reserved_keys).is_some()
        });
        if allows_dynamic_properties && property_names_allow_dynamic_keys {
            let placeholder = dynamic_object_placeholder(&reserved_keys);
            let segment = if placeholder == "{item}" {
                "*".to_owned()
            } else {
                placeholder
            };
            let next = if path.is_empty() {
                segment
            } else {
                format!("{path}.{segment}")
            };
            for child_schema in pattern_properties.values() {
                collect_env_docs(
                    child_schema,
                    root,
                    &next,
                    required_dynamic_properties,
                    docs,
                    visited_refs,
                    None,
                );
            }
        }
    }

    if let Some(items) = object.get("prefixItems").and_then(Value::as_array) {
        traversed_children = true;
        for (index, child) in items.iter().enumerate() {
            let next = if path.is_empty() {
                index.to_string()
            } else {
                format!("{path}.{index}")
            };
            collect_env_docs(
                child,
                root,
                &next,
                required && array_item_is_required(object, index),
                docs,
                visited_refs,
                None,
            );
        }
    }

    if let Some(items) = object.get("items").and_then(Value::as_array) {
        traversed_children = true;
        for (index, child) in items.iter().enumerate() {
            let next = if path.is_empty() {
                index.to_string()
            } else {
                format!("{path}.{index}")
            };
            collect_env_docs(
                child,
                root,
                &next,
                required && array_item_is_required(object, index),
                docs,
                visited_refs,
                None,
            );
        }
    }

    if let Some(items) = object
        .get("items")
        .filter(|value| !value.is_array() && !matches!(value, Value::Bool(false)))
    {
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
        if allows_additional_array_items(object, fixed_item_count) {
            traversed_children = true;
            let next = if path.is_empty() {
                "*".to_owned()
            } else {
                format!("{path}.*")
            };
            collect_env_docs(
                items,
                root,
                &next,
                required && required_additional_array_items(object, fixed_item_count) > 0,
                docs,
                visited_refs,
                None,
            );
        }
    }

    let implicit_additional = Value::Bool(true);
    let has_explicit_additional_properties = object.contains_key("additionalProperties");
    let additional_properties = object
        .get("additionalProperties")
        .filter(|value| !matches!(value, Value::Bool(false)))
        .or({
            if !has_explicit_additional_properties && required_dynamic_properties {
                Some(&implicit_additional)
            } else {
                None
            }
        });
    if let Some(additional) = additional_properties {
        traversed_children = true;
        let property_names_allow_dynamic_keys = object.get("propertyNames").is_none_or(|_| {
            dynamic_object_placeholder_for_schema(object, root, &reserved_keys).is_some()
        });
        if allows_dynamic_properties && property_names_allow_dynamic_keys {
            let placeholder = dynamic_object_placeholder(&reserved_keys);
            let segment = if placeholder == "{item}" {
                "*".to_owned()
            } else {
                placeholder
            };
            let next = if path.is_empty() {
                segment
            } else {
                format!("{path}.{segment}")
            };
            collect_env_docs(
                additional,
                root,
                &next,
                required_dynamic_properties,
                docs,
                visited_refs,
                None,
            );
        }
    }

    if let Some(additional) = object
        .get("additionalItems")
        .filter(|value| !value.is_array() && !matches!(value, Value::Bool(false)))
    {
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
        if allows_additional_array_items(object, fixed_item_count) {
            traversed_children = true;
            let next = if path.is_empty() {
                "*".to_owned()
            } else {
                format!("{path}.*")
            };
            collect_env_docs(
                additional,
                root,
                &next,
                required && required_additional_array_items(object, fixed_item_count) > 0,
                docs,
                visited_refs,
                None,
            );
        }
    }

    if let Some(contains) = object
        .get("contains")
        .filter(|value| !matches!(value, Value::Bool(false)))
    {
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
        if fixed_item_count == 0 || allows_additional_array_items(object, fixed_item_count) {
            traversed_children = true;
            let next = if path.is_empty() {
                "*".to_owned()
            } else {
                format!("{path}.*")
            };
            collect_env_docs(
                contains,
                root,
                &next,
                required && required_contains_additional_items_for_docs(object, root) > 0,
                docs,
                visited_refs,
                None,
            );
        }
    }

    if traversed_children {
        if traversed_combinator {
            apply_local_schema_entry_overrides(path, required, object, docs);
        }
        return;
    }

    if !path.is_empty() {
        docs.push(EnvDocEntry {
            path: path.to_owned(),
            env: String::new(),
            ty: schema_type(object),
            required,
            secret: object
                .get("writeOnly")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || object
                    .get("x-tier-secret")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            description: object
                .get("description")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            example: None,
            deprecated: None,
            aliases: Vec::new(),
            has_default: false,
            merge: MergeStrategy::Merge,
            validations: Vec::new(),
        });
    }
}

pub(super) fn dynamic_object_placeholder(reserved: &BTreeSet<String>) -> String {
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

pub(super) fn array_item_is_required(
    object: &serde_json::Map<String, Value>,
    index: usize,
) -> bool {
    object
        .get("minItems")
        .and_then(Value::as_u64)
        .is_some_and(|min_items| index < min_items as usize)
}

pub(super) fn allows_additional_array_items(
    object: &serde_json::Map<String, Value>,
    fixed_item_count: usize,
) -> bool {
    object
        .get("maxItems")
        .and_then(Value::as_u64)
        .is_none_or(|max_items| fixed_item_count < max_items as usize)
}

pub(super) fn required_additional_array_items(
    object: &serde_json::Map<String, Value>,
    fixed_item_count: usize,
) -> usize {
    object
        .get("minItems")
        .and_then(Value::as_u64)
        .map_or(0, |min_items| {
            min_items.saturating_sub(fixed_item_count as u64) as usize
        })
}

pub(super) fn merged_object_level_property_names(
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

pub(super) fn collect_object_level_property_names(
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
