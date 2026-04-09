use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::ConfigError;
use crate::report::{join_path, normalize_path};

pub(super) fn direct_child_array_index(container_path: &str, entry_path: &str) -> Option<usize> {
    let remainder = if container_path.is_empty() {
        entry_path
    } else {
        entry_path.strip_prefix(container_path)?.strip_prefix('.')?
    };
    remainder.split('.').next()?.parse::<usize>().ok()
}

pub(super) fn ensure_path_safe_keys(value: &Value, current_path: &str) -> Result<(), ConfigError> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                validate_path_key(current_path, key)?;
                let next = join_path(current_path, key);
                ensure_path_safe_keys(child, &next)?;
            }
            Ok(())
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                let next = join_path(current_path, &index.to_string());
                ensure_path_safe_keys(child, &next)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_path_key(current_path: &str, key: &str) -> Result<(), ConfigError> {
    let message = invalid_path_key_message(key);
    if let Some(message) = message {
        Err(ConfigError::InvalidPathKey {
            path: current_path.to_owned(),
            key: key.to_owned(),
            message,
        })
    } else {
        Ok(())
    }
}

pub(super) fn invalid_path_key_message(key: &str) -> Option<String> {
    if key.is_empty() {
        Some("empty object keys are not supported".to_owned())
    } else if key == "*" {
        Some("`*` is reserved for wildcard metadata paths".to_owned())
    } else if key.contains('.') {
        Some("`.` is reserved as the configuration path separator".to_owned())
    } else if key.contains('[') || key.contains(']') {
        Some("`[` and `]` are reserved for external array path syntax".to_owned())
    } else {
        None
    }
}

pub(crate) fn indexed_array_container_paths(segments: &[&str]) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for index in 0..segments.len() {
        if segments[index].parse::<usize>().is_ok() && index > 0 {
            paths.insert(segments[..index].join("."));
        }
    }
    paths
}

pub(crate) fn record_indexed_array_state(
    current_array_lengths: &mut BTreeMap<String, usize>,
    indexed_array_base_lengths: &mut BTreeMap<String, usize>,
    path: &str,
    segments: &[&str],
) {
    for container_path in indexed_array_container_paths(segments) {
        let Some(index) = direct_child_array_index(&container_path, path) else {
            continue;
        };
        let Some(current_length) = current_array_lengths.get_mut(&container_path) else {
            continue;
        };

        indexed_array_base_lengths
            .entry(container_path.clone())
            .or_insert(*current_length);
        if index >= *current_length {
            *current_length = index + 1;
        }
    }
}

pub(crate) fn record_direct_array_state(
    current_array_lengths: &mut BTreeMap<String, usize>,
    indexed_array_base_lengths: &mut BTreeMap<String, usize>,
    path: &str,
    value: &Value,
) {
    clear_array_state(current_array_lengths, path);
    clear_array_state(indexed_array_base_lengths, path);
    collect_array_lengths(value, path, current_array_lengths);
}

fn clear_array_state<T>(state: &mut BTreeMap<String, T>, path: &str) {
    let nested_prefix = format!("{path}.");
    state.retain(|candidate, _| candidate != path && !candidate.starts_with(&nested_prefix));
}

fn collect_array_lengths(value: &Value, path: &str, lengths: &mut BTreeMap<String, usize>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let next = join_path(path, key);
                collect_array_lengths(child, &next, lengths);
            }
        }
        Value::Array(values) => {
            lengths.insert(path.to_owned(), values.len());
            for (index, child) in values.iter().enumerate() {
                let next = join_path(path, &index.to_string());
                collect_array_lengths(child, &next, lengths);
            }
        }
        _ => {}
    }
}

pub(crate) fn normalize_external_path(path: &str) -> String {
    try_normalize_external_path(path).unwrap_or_else(|_| normalize_path(path))
}

pub(crate) fn try_normalize_external_path(path: &str) -> Result<String, String> {
    if path == "." {
        return Ok(String::new());
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = path.chars().peekable();
    let mut after_index = false;
    let mut expecting_segment = true;

    while let Some(ch) = chars.next() {
        if after_index {
            match ch {
                '.' => {
                    if chars.peek().is_none() {
                        return Err("configuration path cannot end with `.`".to_owned());
                    }
                    after_index = false;
                    expecting_segment = true;
                }
                '[' => {
                    let index = parse_external_array_index(&mut chars)?;
                    segments.push(index);
                    after_index = true;
                    expecting_segment = false;
                }
                _ => {
                    return Err(
                        "expected `.` or `[` after an array index in configuration path".to_owned(),
                    );
                }
            }
            continue;
        }

        match ch {
            '.' => {
                if current.is_empty() {
                    return Err("empty path segment in configuration path".to_owned());
                }
                segments.push(std::mem::take(&mut current));
                expecting_segment = true;
            }
            '[' => {
                if current.is_empty() {
                    return Err("array indices must follow a field name".to_owned());
                }
                segments.push(std::mem::take(&mut current));
                let index = parse_external_array_index(&mut chars)?;
                segments.push(index);
                after_index = true;
                expecting_segment = false;
            }
            ']' => return Err("unexpected `]` in configuration path".to_owned()),
            _ => {
                current.push(ch);
                expecting_segment = false;
            }
        }
    }

    if expecting_segment && !segments.is_empty() && current.is_empty() && !after_index {
        return Err("configuration path cannot end with `.`".to_owned());
    }

    if !current.is_empty() {
        segments.push(current);
    }

    Ok(normalize_path(&segments.join(".")))
}

fn parse_external_array_index<I>(chars: &mut std::iter::Peekable<I>) -> Result<String, String>
where
    I: Iterator<Item = char>,
{
    let mut index = String::new();
    let mut closed = false;
    for next in chars.by_ref() {
        if next == ']' {
            closed = true;
            break;
        }
        index.push(next);
    }
    if !closed {
        return Err("unclosed `[` in configuration path".to_owned());
    }
    if index.is_empty() {
        return Err("empty array index in configuration path".to_owned());
    }
    if !index.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("array indices in configuration paths must be numeric".to_owned());
    }
    index
        .parse::<usize>()
        .map(|value| value.to_string())
        .map_err(|_| "array indices in configuration paths must fit in usize".to_owned())
}
