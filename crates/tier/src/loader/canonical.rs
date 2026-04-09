use super::*;
use crate::ValidationCheck;

pub(super) fn canonicalize_layer_paths(
    layer: Layer,
    metadata: &ConfigMetadata,
) -> Result<Layer, ConfigError> {
    let raw_value = layer.value;
    let aliases = canonicalize_alias_overrides_against_value(metadata, &raw_value)?;
    let value = canonicalize_value_paths_with_aliases(&raw_value, &aliases)?;

    let entries = layer
        .entries
        .into_iter()
        .map(|(path, trace)| (canonicalize_layer_path(&raw_value, &path, &aliases), trace))
        .collect();
    let coercible_string_paths = layer
        .coercible_string_paths
        .into_iter()
        .map(|path| canonicalize_layer_path(&raw_value, &path, &aliases))
        .collect();
    let indexed_array_paths = layer
        .indexed_array_paths
        .into_iter()
        .map(|path| canonicalize_layer_path(&raw_value, &path, &aliases))
        .collect();
    let indexed_array_base_lengths = layer
        .indexed_array_base_lengths
        .into_iter()
        .map(|(path, length)| (canonicalize_layer_path(&raw_value, &path, &aliases), length))
        .collect();
    let direct_array_paths = layer
        .direct_array_paths
        .into_iter()
        .map(|path| canonicalize_layer_path(&raw_value, &path, &aliases))
        .collect();

    Ok(Layer {
        trace: layer.trace,
        value,
        entries,
        coercible_string_paths,
        indexed_array_paths,
        indexed_array_base_lengths,
        direct_array_paths,
    })
}

pub(super) fn canonicalize_alias_overrides_against_value(
    metadata: &ConfigMetadata,
    value: &Value,
) -> Result<BTreeMap<String, String>, ConfigError> {
    let aliases = metadata.alias_overrides()?;
    let mut canonicalized = BTreeMap::new();

    for (alias, canonical) in aliases {
        let alias = canonicalize_runtime_path(value, &alias);
        let canonical = canonicalize_runtime_path(value, &canonical);
        if alias == canonical {
            continue;
        }
        if let Some(first_path) = canonicalized.insert(alias.clone(), canonical.clone())
            && first_path != canonical
        {
            return Err(ConfigError::MetadataConflict {
                kind: "alias",
                name: alias,
                first_path,
                second_path: canonical,
            });
        }
    }

    Ok(canonicalized)
}

pub(super) fn canonicalize_layer_path(
    value: &Value,
    path: &str,
    aliases: &BTreeMap<String, String>,
) -> String {
    let runtime = canonicalize_runtime_path(value, path);
    canonicalize_path_with_aliases(&runtime, aliases)
}

pub(super) fn canonicalize_runtime_path(value: &Value, path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }

    let segments = path.split('.').collect::<Vec<_>>();
    let mut current = value;
    let mut canonical = Vec::new();
    let mut index = 0;
    while index < segments.len() {
        let segment = segments[index];
        match current {
            Value::Object(map) => {
                canonical.push(segment.to_owned());
                let Some(next) = map.get(segment) else {
                    canonical.extend(
                        segments[index + 1..]
                            .iter()
                            .map(|segment| (*segment).to_owned()),
                    );
                    break;
                };
                current = next;
            }
            Value::Array(values) => {
                let Ok(array_index) = segment.parse::<usize>() else {
                    canonical.push(segment.to_owned());
                    canonical.extend(
                        segments[index + 1..]
                            .iter()
                            .map(|segment| (*segment).to_owned()),
                    );
                    break;
                };
                canonical.push(array_index.to_string());
                let Some(next) = values.get(array_index) else {
                    canonical.extend(
                        segments[index + 1..]
                            .iter()
                            .map(|segment| (*segment).to_owned()),
                    );
                    break;
                };
                current = next;
            }
            _ => {
                canonical.push(segment.to_owned());
                canonical.extend(
                    segments[index + 1..]
                        .iter()
                        .map(|segment| (*segment).to_owned()),
                );
                break;
            }
        }
        index += 1;
    }

    canonical.join(".")
}

pub(super) fn canonicalize_secret_paths(
    secret_paths: &BTreeSet<String>,
    aliases: &BTreeMap<String, String>,
) -> BTreeSet<String> {
    secret_paths
        .iter()
        .map(|path| canonicalize_path_with_aliases(path, aliases))
        .collect()
}

pub(super) fn canonicalize_metadata_against_layers(
    metadata: &ConfigMetadata,
    layers: &[Layer],
) -> Result<ConfigMetadata, ConfigError> {
    let fields = metadata.fields().iter().cloned().map(|mut field| {
        field.path = canonicalize_runtime_path_across_layers(&field.path, layers);
        field.aliases = canonicalize_runtime_paths_across_layers(field.aliases, layers);
        field
    });

    let mut resolved = ConfigMetadata::new();
    resolved.extend_fields(fields);
    let aliases = resolved.alias_overrides()?;
    let checks = metadata.checks().iter().cloned().map(|check| {
        canonicalize_check_with_aliases(canonicalize_check_against_layers(check, layers), &aliases)
    });
    resolved.extend_checks(checks);
    Ok(resolved)
}

pub(super) fn canonicalize_secret_paths_against_layers(
    secret_paths: &BTreeSet<String>,
    layers: &[Layer],
    aliases: &BTreeMap<String, String>,
) -> BTreeSet<String> {
    secret_paths
        .iter()
        .map(|path| {
            layers.iter().fold(path.clone(), |current, layer| {
                canonicalize_layer_path(&layer.value, &current, aliases)
            })
        })
        .collect()
}

pub(super) fn canonicalize_metadata_against_value(
    metadata: &ConfigMetadata,
    value: &Value,
) -> Result<ConfigMetadata, ConfigError> {
    let fields = metadata.fields().iter().cloned().map(|mut field| {
        field.path = canonicalize_runtime_path(value, &field.path);
        field.aliases = canonicalize_runtime_paths_against_value(field.aliases, value);
        field
    });

    let mut resolved = ConfigMetadata::new();
    resolved.extend_fields(fields);
    let aliases = resolved.alias_overrides()?;
    let checks = metadata.checks().iter().cloned().map(|check| {
        canonicalize_check_with_aliases(canonicalize_check_against_value(check, value), &aliases)
    });
    resolved.extend_checks(checks);
    Ok(resolved)
}

pub(super) fn canonicalize_secret_paths_against_value(
    secret_paths: &BTreeSet<String>,
    value: &Value,
    aliases: &BTreeMap<String, String>,
) -> BTreeSet<String> {
    secret_paths
        .iter()
        .map(|path| canonicalize_layer_path(value, path, aliases))
        .collect()
}

pub(super) fn canonicalize_runtime_paths_against_value<I>(paths: I, value: &Value) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut canonicalized = Vec::new();
    for path in paths {
        let canonical = canonicalize_runtime_path(value, &path);
        if canonical.is_empty() || canonicalized.contains(&canonical) {
            continue;
        }
        canonicalized.push(canonical);
    }
    canonicalized
}

pub(super) fn canonicalize_runtime_paths_across_layers<I>(paths: I, layers: &[Layer]) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut canonicalized = Vec::new();
    for path in paths {
        let canonical = canonicalize_runtime_path_across_layers(&path, layers);
        if canonical.is_empty() || canonicalized.contains(&canonical) {
            continue;
        }
        canonicalized.push(canonical);
    }
    canonicalized
}

pub(super) fn canonicalize_runtime_path_across_layers(path: &str, layers: &[Layer]) -> String {
    layers.iter().fold(normalize_path(path), |current, layer| {
        canonicalize_runtime_path(&layer.value, &current)
    })
}

pub(super) fn canonicalize_paths_with_aliases<I>(
    paths: I,
    aliases: &BTreeMap<String, String>,
) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut canonicalized = Vec::new();
    for path in paths {
        let canonical = canonicalize_path_with_aliases(&path, aliases);
        if canonical.is_empty() || canonicalized.contains(&canonical) {
            continue;
        }
        canonicalized.push(canonical);
    }
    canonicalized
}

pub(super) fn canonicalize_check_with_aliases(
    check: ValidationCheck,
    aliases: &BTreeMap<String, String>,
) -> ValidationCheck {
    match check {
        ValidationCheck::AtLeastOneOf { paths } => ValidationCheck::AtLeastOneOf {
            paths: canonicalize_paths_with_aliases(paths, aliases),
        },
        ValidationCheck::ExactlyOneOf { paths } => ValidationCheck::ExactlyOneOf {
            paths: canonicalize_paths_with_aliases(paths, aliases),
        },
        ValidationCheck::MutuallyExclusive { paths } => ValidationCheck::MutuallyExclusive {
            paths: canonicalize_paths_with_aliases(paths, aliases),
        },
        ValidationCheck::RequiredWith { path, requires } => ValidationCheck::RequiredWith {
            path: canonicalize_path_with_aliases(&path, aliases),
            requires: canonicalize_paths_with_aliases(requires, aliases),
        },
        ValidationCheck::RequiredIf {
            path,
            equals,
            requires,
        } => ValidationCheck::RequiredIf {
            path: canonicalize_path_with_aliases(&path, aliases),
            equals,
            requires: canonicalize_paths_with_aliases(requires, aliases),
        },
    }
}

pub(super) fn canonicalize_check_against_layers(
    check: ValidationCheck,
    layers: &[Layer],
) -> ValidationCheck {
    match check {
        ValidationCheck::AtLeastOneOf { paths } => ValidationCheck::AtLeastOneOf {
            paths: canonicalize_runtime_paths_across_layers(paths, layers),
        },
        ValidationCheck::ExactlyOneOf { paths } => ValidationCheck::ExactlyOneOf {
            paths: canonicalize_runtime_paths_across_layers(paths, layers),
        },
        ValidationCheck::MutuallyExclusive { paths } => ValidationCheck::MutuallyExclusive {
            paths: canonicalize_runtime_paths_across_layers(paths, layers),
        },
        ValidationCheck::RequiredWith { path, requires } => ValidationCheck::RequiredWith {
            path: canonicalize_runtime_path_across_layers(&path, layers),
            requires: canonicalize_runtime_paths_across_layers(requires, layers),
        },
        ValidationCheck::RequiredIf {
            path,
            equals,
            requires,
        } => ValidationCheck::RequiredIf {
            path: canonicalize_runtime_path_across_layers(&path, layers),
            equals,
            requires: canonicalize_runtime_paths_across_layers(requires, layers),
        },
    }
}

pub(super) fn canonicalize_check_against_value(
    check: ValidationCheck,
    value: &Value,
) -> ValidationCheck {
    match check {
        ValidationCheck::AtLeastOneOf { paths } => ValidationCheck::AtLeastOneOf {
            paths: canonicalize_runtime_paths_against_value(paths, value),
        },
        ValidationCheck::ExactlyOneOf { paths } => ValidationCheck::ExactlyOneOf {
            paths: canonicalize_runtime_paths_against_value(paths, value),
        },
        ValidationCheck::MutuallyExclusive { paths } => ValidationCheck::MutuallyExclusive {
            paths: canonicalize_runtime_paths_against_value(paths, value),
        },
        ValidationCheck::RequiredWith { path, requires } => ValidationCheck::RequiredWith {
            path: canonicalize_runtime_path(value, &path),
            requires: canonicalize_runtime_paths_against_value(requires, value),
        },
        ValidationCheck::RequiredIf {
            path,
            equals,
            requires,
        } => ValidationCheck::RequiredIf {
            path: canonicalize_runtime_path(value, &path),
            equals,
            requires: canonicalize_runtime_paths_against_value(requires, value),
        },
    }
}

pub(super) fn canonicalize_value_paths(
    value: &Value,
    metadata: &ConfigMetadata,
) -> Result<Value, ConfigError> {
    ensure_root_object(value)?;
    ensure_path_safe_keys(value, "")?;

    let aliases = metadata.alias_overrides()?;
    canonicalize_value_paths_with_aliases(value, &aliases)
}

pub(super) fn canonicalize_value_paths_with_aliases(
    value: &Value,
    aliases: &BTreeMap<String, String>,
) -> Result<Value, ConfigError> {
    ensure_root_object(value)?;
    ensure_path_safe_keys(value, "")?;
    if aliases.is_empty() {
        return Ok(value.clone());
    }

    let mut canonical = Value::Object(Map::new());
    let mut nodes = Vec::new();
    collect_value_nodes(value, "", &mut nodes);
    let mut seen = BTreeMap::<String, String>::new();

    for (path, node) in nodes {
        let canonical_path = canonicalize_path_with_aliases(&path, aliases);
        if let Some(first_path) = seen.get(&canonical_path)
            && first_path != &path
        {
            return Err(ConfigError::PathConflict {
                first_path: first_path.clone(),
                second_path: path,
                canonical_path,
            });
        }
        seen.insert(canonical_path.clone(), path);
        let segments = canonical_path.split('.').collect::<Vec<_>>();
        insert_path(&mut canonical, &segments, node).map_err(|message| {
            ConfigError::InvalidArg {
                arg: canonical_path.clone(),
                message,
            }
        })?;
    }

    Ok(canonical)
}

pub(super) fn collect_value_nodes(value: &Value, current: &str, nodes: &mut Vec<(String, Value)>) {
    match value {
        Value::Object(map) if map.is_empty() && !current.is_empty() => {
            nodes.push((current.to_owned(), Value::Object(Map::new())));
        }
        Value::Object(map) => {
            for (key, child) in map {
                let next = join_path(current, key);
                collect_value_nodes(child, &next, nodes);
            }
        }
        Value::Array(values) if values.is_empty() && !current.is_empty() => {
            nodes.push((current.to_owned(), Value::Array(Vec::new())));
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                let next = join_path(current, &index.to_string());
                collect_value_nodes(child, &next, nodes);
            }
        }
        _ if !current.is_empty() => nodes.push((current.to_owned(), value.clone())),
        _ => {}
    }
}
