use super::*;

pub(super) struct ParsedOverride {
    pub(super) value: Value,
    pub(super) string_coercion_suffixes: BTreeSet<String>,
}

pub(super) fn parse_override_value(raw: &str) -> Result<ParsedOverride, String> {
    if raw.is_empty() {
        return Ok(ParsedOverride {
            value: Value::String(String::new()),
            string_coercion_suffixes: BTreeSet::from([String::new()]),
        });
    }

    let trimmed = raw.trim();

    let uses_explicit_json_syntax =
        matches!(trimmed.chars().next(), Some('{') | Some('[') | Some('"'));

    if uses_explicit_json_syntax {
        let value = serde_json::from_str::<Value>(trimmed)
            .map_err(|error| format!("invalid explicit JSON override: {error}"))?;
        return Ok(ParsedOverride {
            value,
            string_coercion_suffixes: BTreeSet::new(),
        });
    }

    Ok(ParsedOverride {
        value: Value::String(raw.to_owned()),
        string_coercion_suffixes: BTreeSet::from([String::new()]),
    })
}

pub(super) fn parse_env_override_value(
    raw: &str,
    decoder: Option<EnvDecoder>,
    custom_decoder: Option<&CustomEnvDecoder>,
) -> Result<ParsedOverride, String> {
    match (custom_decoder, decoder) {
        (Some(custom_decoder), _) => {
            let value = custom_decoder(raw)?;
            Ok(ParsedOverride {
                string_coercion_suffixes: collect_string_leaf_suffixes(&value, ""),
                value,
            })
        }
        (None, Some(decoder)) => {
            let value = decode_env_override_value(raw, decoder)?;
            Ok(ParsedOverride {
                string_coercion_suffixes: collect_string_leaf_suffixes(&value, ""),
                value,
            })
        }
        (None, None) => parse_override_value(raw),
    }
}

fn decode_env_override_value(raw: &str, decoder: EnvDecoder) -> Result<Value, String> {
    match decoder {
        EnvDecoder::Csv => Ok(Value::Array(
            raw.split(',')
                .map(str::trim)
                .filter(|segment| !segment.is_empty())
                .map(|segment| Value::String(segment.to_owned()))
                .collect(),
        )),
        EnvDecoder::Whitespace => Ok(Value::Array(
            raw.split_whitespace()
                .map(|segment| Value::String(segment.to_owned()))
                .collect(),
        )),
        EnvDecoder::PathList => {
            let values = std::env::split_paths(OsStr::new(raw))
                .map(|path| Value::String(path.to_string_lossy().into_owned()))
                .collect();
            Ok(Value::Array(values))
        }
        EnvDecoder::KeyValueMap => {
            let mut map = Map::new();
            for entry in raw
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
            {
                let (key, value) = entry.split_once('=').ok_or_else(|| {
                    format!("invalid key_value_map entry `{entry}`, expected key=value")
                })?;
                let key = key.trim();
                let value = value.trim();
                if key.is_empty() {
                    return Err("key_value_map entries must not use an empty key".to_owned());
                }
                map.insert(key.to_owned(), Value::String(value.to_owned()));
            }
            Ok(Value::Object(map))
        }
    }
}

pub(super) fn collect_string_leaf_suffixes(value: &Value, prefix: &str) -> BTreeSet<String> {
    let mut suffixes = BTreeSet::new();
    collect_string_leaf_suffixes_inner(value, prefix, &mut suffixes);
    suffixes
}

fn collect_string_leaf_suffixes_inner(
    value: &Value,
    prefix: &str,
    suffixes: &mut BTreeSet<String>,
) {
    match value {
        Value::String(_) => {
            suffixes.insert(prefix.to_owned());
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                let next = join_path(prefix, &index.to_string());
                collect_string_leaf_suffixes_inner(value, &next, suffixes);
            }
        }
        Value::Object(map) => {
            for (key, value) in map {
                let next = join_path(prefix, key);
                collect_string_leaf_suffixes_inner(value, &next, suffixes);
            }
        }
        _ => {}
    }
}

pub(super) fn coerce_retry_scalars(
    value: &Value,
    current_path: &str,
    string_coercion_paths: &BTreeSet<String>,
) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, child)| {
                    let next = join_path(current_path, key);
                    (
                        key.clone(),
                        coerce_retry_scalars(child, &next, string_coercion_paths),
                    )
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .enumerate()
                .map(|(index, child)| {
                    let next = join_path(current_path, &index.to_string());
                    coerce_retry_scalars(child, &next, string_coercion_paths)
                })
                .collect(),
        ),
        Value::String(raw) if string_coercion_paths.contains(current_path) => {
            retry_scalar_value(raw).unwrap_or_else(|| Value::String(raw.clone()))
        }
        other => other.clone(),
    }
}

fn retry_scalar_value(raw: &str) -> Option<Value> {
    let value = serde_json::from_str::<Value>(raw.trim()).ok()?;
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => Some(value),
        _ => None,
    }
}
