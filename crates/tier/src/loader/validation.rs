use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;

use regex::Regex;
use serde_json::Value;

use crate::error::{ValidationError, ValidationErrors};
use crate::metadata::{ConfigMetadata, ValidationCheck, ValidationRule};
use crate::report::{ConfigReport, ConfigWarning, get_value_at_path, join_path, normalize_path};

use super::{is_secret_path, is_valid_email, is_valid_hostname, is_valid_url};

pub(super) fn validate_declared_rules(
    value: &Value,
    metadata: &ConfigMetadata,
    secret_paths: &BTreeSet<String>,
    report: &mut ConfigReport,
) -> ValidationErrors {
    let mut errors = ValidationErrors::new();
    let mut matched_paths = BTreeSet::<String>::new();

    for field in metadata.fields() {
        if field.validations.is_empty() || field.path.is_empty() {
            continue;
        }
        matched_paths.extend(
            collect_matching_values(value, &field.path)
                .into_iter()
                .map(|(matched_path, _)| matched_path),
        );
    }

    for matched_path in matched_paths {
        let Some(actual) = get_value_at_path(value, &matched_path) else {
            continue;
        };
        for effective in metadata.effective_validations_for(&matched_path) {
            let rule = &effective.rule;
            let field = effective.field;
            if let Some(error) = validate_declared_rule(&matched_path, actual, rule, secret_paths) {
                let error = field.decorate_validation_error(rule, error);
                match field.validation_level_for(rule) {
                    crate::ValidationLevel::Error => errors.push(error),
                    crate::ValidationLevel::Warning => {
                        report.record_warning(ConfigWarning::Validation(error))
                    }
                }
            }
        }
    }

    errors
}

pub(super) fn validate_declared_checks(
    value: &Value,
    metadata: &ConfigMetadata,
    secret_paths: &BTreeSet<String>,
) -> ValidationErrors {
    let mut errors = ValidationErrors::new();

    for check in metadata.checks() {
        match check {
            ValidationCheck::AtLeastOneOf { paths } => {
                let present = present_paths(value, paths);
                if present.is_empty() {
                    errors.push(group_validation_error(
                        check,
                        paths,
                        secret_paths,
                        &format!("at least one of {} must be configured", paths.join(", ")),
                        Some(serde_json::json!({ "min_present": 1, "paths": paths })),
                        Some(serde_json::json!({ "present": present })),
                    ));
                }
            }
            ValidationCheck::ExactlyOneOf { paths } => {
                let present = present_paths(value, paths);
                if present.len() != 1 {
                    errors.push(group_validation_error(
                        check,
                        paths,
                        secret_paths,
                        &format!("exactly one of {} must be configured", paths.join(", ")),
                        Some(serde_json::json!({ "exactly_one_of": paths })),
                        Some(serde_json::json!({ "present": present })),
                    ));
                }
            }
            ValidationCheck::MutuallyExclusive { paths } => {
                let present = present_paths(value, paths);
                if present.len() > 1 {
                    errors.push(group_validation_error(
                        check,
                        paths,
                        secret_paths,
                        &format!("{} are mutually exclusive", paths.join(", ")),
                        Some(serde_json::json!({ "max_present": 1, "paths": paths })),
                        Some(serde_json::json!({ "present": present })),
                    ));
                }
            }
            ValidationCheck::RequiredWith { path, requires } => {
                for (matched_path, _) in collect_matching_values(value, path)
                    .into_iter()
                    .filter(|(_, matched)| is_present_value(matched))
                {
                    let bound_requires = bind_required_paths(path, &matched_path, requires)
                        .unwrap_or_else(|| requires.clone());
                    let missing = missing_paths(value, &bound_requires);
                    if !missing.is_empty() {
                        errors.push(group_validation_error(
                            check,
                            std::iter::once(matched_path.clone()).chain(missing.iter().cloned()),
                            secret_paths,
                            &format!("{matched_path} requires {}", missing.join(", ")),
                            Some(serde_json::json!({
                                "trigger": matched_path,
                                "requires": bound_requires
                            })),
                            Some(serde_json::json!({ "missing": missing })),
                        ));
                    }
                }
            }
            ValidationCheck::RequiredIf {
                path,
                equals,
                requires,
            } => {
                for (matched_path, _actual) in collect_matching_values(value, path)
                    .into_iter()
                    .filter(|(_, matched)| *matched == &equals.0)
                {
                    let bound_requires = bind_required_paths(path, &matched_path, requires)
                        .unwrap_or_else(|| requires.clone());
                    let missing = missing_paths(value, &bound_requires);
                    if !missing.is_empty() {
                        errors.push(group_validation_error(
                            check,
                            std::iter::once(matched_path.clone()).chain(missing.iter().cloned()),
                            secret_paths,
                            &format!(
                                "{matched_path} == {} requires {}",
                                equals,
                                missing.join(", ")
                            ),
                            Some(serde_json::json!({
                                "trigger": matched_path,
                                "equals": equals,
                                "requires": bound_requires
                            })),
                            Some(serde_json::json!({ "missing": missing })),
                        ));
                    }
                }
            }
        }
    }

    errors
}

fn validate_declared_rule(
    path: &str,
    actual: &Value,
    rule: &ValidationRule,
    secret_paths: &BTreeSet<String>,
) -> Option<ValidationError> {
    match rule {
        ValidationRule::NonEmpty => {
            let is_empty = match actual {
                Value::String(value) => value.is_empty(),
                Value::Array(values) => values.is_empty(),
                Value::Object(values) => values.is_empty(),
                _ => false,
            };
            is_empty.then(|| {
                validation_error(path, actual, rule, secret_paths, "must not be empty", None)
            })
        }
        ValidationRule::Min(min) if !min.is_finite() => Some(validation_error(
            path,
            actual,
            rule,
            secret_paths,
            &format!("declared minimum must be finite, got {min}"),
            Some(min.as_json_value()),
        )),
        ValidationRule::Min(min) => match actual.as_f64() {
            Some(value) if value >= min.as_f64().unwrap_or(f64::INFINITY) => None,
            Some(_) => Some(validation_error(
                path,
                actual,
                rule,
                secret_paths,
                &format!("must be >= {min}"),
                Some(min.as_json_value()),
            )),
            None => Some(validation_error(
                path,
                actual,
                rule,
                secret_paths,
                "must be a numeric value",
                Some(min.as_json_value()),
            )),
        },
        ValidationRule::Max(max) if !max.is_finite() => Some(validation_error(
            path,
            actual,
            rule,
            secret_paths,
            &format!("declared maximum must be finite, got {max}"),
            Some(max.as_json_value()),
        )),
        ValidationRule::Max(max) => match actual.as_f64() {
            Some(value) if value <= max.as_f64().unwrap_or(f64::NEG_INFINITY) => None,
            Some(_) => Some(validation_error(
                path,
                actual,
                rule,
                secret_paths,
                &format!("must be <= {max}"),
                Some(max.as_json_value()),
            )),
            None => Some(validation_error(
                path,
                actual,
                rule,
                secret_paths,
                "must be a numeric value",
                Some(max.as_json_value()),
            )),
        },
        ValidationRule::MinLength(min) => {
            let length = validation_length(actual);
            match length {
                Some(length) if length >= *min => None,
                Some(_) => Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    &format!("length must be >= {min}"),
                    Some(Value::Number((*min as u64).into())),
                )),
                None => Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be a string, array, or object to apply length validation",
                    Some(Value::Number((*min as u64).into())),
                )),
            }
        }
        ValidationRule::MaxLength(max) => {
            let length = validation_length(actual);
            match length {
                Some(length) if length <= *max => None,
                Some(_) => Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    &format!("length must be <= {max}"),
                    Some(Value::Number((*max as u64).into())),
                )),
                None => Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be a string, array, or object to apply length validation",
                    Some(Value::Number((*max as u64).into())),
                )),
            }
        }
        ValidationRule::MinItems(min) => match actual {
            Value::Array(values) if values.len() >= *min => None,
            Value::Array(_) => Some(validation_error(
                path,
                actual,
                rule,
                secret_paths,
                &format!("item count must be >= {min}"),
                Some(Value::Number((*min as u64).into())),
            )),
            _ => Some(validation_error(
                path,
                actual,
                rule,
                secret_paths,
                "must be an array to apply item-count validation",
                Some(Value::Number((*min as u64).into())),
            )),
        },
        ValidationRule::MaxItems(max) => match actual {
            Value::Array(values) if values.len() <= *max => None,
            Value::Array(_) => Some(validation_error(
                path,
                actual,
                rule,
                secret_paths,
                &format!("item count must be <= {max}"),
                Some(Value::Number((*max as u64).into())),
            )),
            _ => Some(validation_error(
                path,
                actual,
                rule,
                secret_paths,
                "must be an array to apply item-count validation",
                Some(Value::Number((*max as u64).into())),
            )),
        },
        ValidationRule::MinProperties(min) => match actual {
            Value::Object(values) if values.len() >= *min => None,
            Value::Object(_) => Some(validation_error(
                path,
                actual,
                rule,
                secret_paths,
                &format!("property count must be >= {min}"),
                Some(Value::Number((*min as u64).into())),
            )),
            _ => Some(validation_error(
                path,
                actual,
                rule,
                secret_paths,
                "must be an object to apply property-count validation",
                Some(Value::Number((*min as u64).into())),
            )),
        },
        ValidationRule::MaxProperties(max) => match actual {
            Value::Object(values) if values.len() <= *max => None,
            Value::Object(_) => Some(validation_error(
                path,
                actual,
                rule,
                secret_paths,
                &format!("property count must be <= {max}"),
                Some(Value::Number((*max as u64).into())),
            )),
            _ => Some(validation_error(
                path,
                actual,
                rule,
                secret_paths,
                "must be an object to apply property-count validation",
                Some(Value::Number((*max as u64).into())),
            )),
        },
        ValidationRule::MultipleOf(factor) if !factor.is_finite() => Some(validation_error(
            path,
            actual,
            rule,
            secret_paths,
            &format!("declared multiple_of factor must be finite, got {factor}"),
            Some(factor.as_json_value()),
        )),
        ValidationRule::MultipleOf(factor) => {
            let Some(divisor) = factor.as_f64() else {
                return Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    &format!("declared multiple_of factor must be finite, got {factor}"),
                    Some(factor.as_json_value()),
                ));
            };
            if divisor <= 0.0 {
                return Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "declared multiple_of factor must be > 0",
                    Some(factor.as_json_value()),
                ));
            }

            match actual.as_f64() {
                Some(value) if is_multiple_of(value, divisor) => None,
                Some(_) => Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    &format!("must be a multiple of {factor}"),
                    Some(factor.as_json_value()),
                )),
                None => Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be a numeric value",
                    Some(factor.as_json_value()),
                )),
            }
        }
        ValidationRule::Pattern(pattern) => {
            let Some(value) = actual.as_str() else {
                return Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be a string to apply pattern validation",
                    Some(Value::String(pattern.clone())),
                ));
            };
            let regex = match Regex::new(pattern) {
                Ok(regex) => regex,
                Err(error) => {
                    return Some(validation_error(
                        path,
                        actual,
                        rule,
                        secret_paths,
                        &format!("declared pattern must be a valid regex: {error}"),
                        Some(Value::String(pattern.clone())),
                    ));
                }
            };
            (!regex.is_match(value)).then(|| {
                validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    &format!("must match pattern {pattern:?}"),
                    Some(Value::String(pattern.clone())),
                )
            })
        }
        ValidationRule::UniqueItems => match actual {
            Value::Array(values) => {
                let mut seen = Vec::<&Value>::new();
                let duplicate = values.iter().any(|value| {
                    let duplicate = seen.contains(&value);
                    if !duplicate {
                        seen.push(value);
                    }
                    duplicate
                });
                duplicate.then(|| {
                    validation_error(
                        path,
                        actual,
                        rule,
                        secret_paths,
                        "items must be unique",
                        Some(Value::Bool(true)),
                    )
                })
            }
            _ => Some(validation_error(
                path,
                actual,
                rule,
                secret_paths,
                "must be an array to apply unique-items validation",
                Some(Value::Bool(true)),
            )),
        },
        ValidationRule::OneOf(values) => {
            let expected = Value::Array(values.iter().map(|value| value.0.clone()).collect());
            values
                .iter()
                .any(|value| value.0 == *actual)
                .then_some(())
                .map_or_else(
                    || {
                        Some(validation_error(
                            path,
                            actual,
                            rule,
                            secret_paths,
                            &format!(
                                "must be one of {}",
                                values
                                    .iter()
                                    .map(ToString::to_string)
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            Some(expected),
                        ))
                    },
                    |_| None,
                )
        }
        ValidationRule::Hostname => {
            let Some(value) = actual.as_str() else {
                return Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be a hostname string",
                    None,
                ));
            };
            (!is_valid_hostname(value)).then(|| {
                validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be a valid hostname",
                    None,
                )
            })
        }
        ValidationRule::Url => {
            let Some(value) = actual.as_str() else {
                return Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be a URL string",
                    None,
                ));
            };
            (!is_valid_url(value)).then(|| {
                validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be a valid URL",
                    None,
                )
            })
        }
        ValidationRule::Email => {
            let Some(value) = actual.as_str() else {
                return Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be an email address string",
                    None,
                ));
            };
            (!is_valid_email(value)).then(|| {
                validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be a valid email address",
                    None,
                )
            })
        }
        ValidationRule::IpAddr => {
            let Some(value) = actual.as_str() else {
                return Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be an IP address string",
                    None,
                ));
            };
            value.parse::<IpAddr>().err().map(|_| {
                validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be a valid IP address",
                    None,
                )
            })
        }
        ValidationRule::SocketAddr => {
            let Some(value) = actual.as_str() else {
                return Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be a socket address string",
                    None,
                ));
            };
            value.parse::<SocketAddr>().err().map(|_| {
                validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be a valid socket address",
                    None,
                )
            })
        }
        ValidationRule::AbsolutePath => {
            let Some(value) = actual.as_str() else {
                return Some(validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be a filesystem path string",
                    None,
                ));
            };
            (!Path::new(value).is_absolute()).then(|| {
                validation_error(
                    path,
                    actual,
                    rule,
                    secret_paths,
                    "must be an absolute filesystem path",
                    None,
                )
            })
        }
    }
}

fn validation_length(value: &Value) -> Option<usize> {
    match value {
        Value::String(inner) => Some(inner.chars().count()),
        Value::Array(values) => Some(values.len()),
        Value::Object(values) => Some(values.len()),
        _ => None,
    }
}

fn is_multiple_of(value: f64, factor: f64) -> bool {
    if !value.is_finite() || !factor.is_finite() || factor <= 0.0 {
        return false;
    }

    let quotient = value / factor;
    let nearest = quotient.round();
    let tolerance = f64::EPSILON * 16.0 * quotient.abs().max(1.0);
    (quotient - nearest).abs() <= tolerance
}

fn validation_error(
    path: &str,
    actual: &Value,
    rule: &ValidationRule,
    secret_paths: &BTreeSet<String>,
    message: &str,
    expected: Option<Value>,
) -> ValidationError {
    let actual = if is_secret_path(secret_paths, path) {
        Value::String("***redacted***".to_owned())
    } else {
        actual.clone()
    };

    let mut error = ValidationError::new(path, message).with_rule(rule.code());
    if let Some(expected) = expected {
        error = error.with_expected(expected);
    }
    error.with_actual(actual)
}

fn group_validation_error<I, S>(
    check: &ValidationCheck,
    related_paths: I,
    secret_paths: &BTreeSet<String>,
    message: &str,
    expected: Option<Value>,
    actual: Option<Value>,
) -> ValidationError
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let related_paths = related_paths
        .into_iter()
        .map(|path| normalize_path(path.as_ref()))
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();

    let actual = actual.map(|value| redact_group_value(value, &related_paths, secret_paths));

    let mut error = ValidationError::new("", message)
        .with_rule(check.code())
        .with_related_paths(related_paths);
    if let Some(expected) = expected {
        error = error.with_expected(expected);
    }
    if let Some(actual) = actual {
        error = error.with_actual(actual);
    }
    error
}

fn collect_matching_values<'a>(value: &'a Value, path: &str) -> Vec<(String, &'a Value)> {
    let normalized = normalize_path(path);
    if normalized.is_empty() {
        return Vec::new();
    }

    let segments = normalized.split('.').collect::<Vec<_>>();
    let mut matches = Vec::new();
    collect_matching_values_recursive(value, "", &segments, 0, &mut matches);
    matches
}

fn bind_required_paths(
    trigger_pattern: &str,
    matched_path: &str,
    requires: &[String],
) -> Option<Vec<String>> {
    let bindings = wildcard_bindings(trigger_pattern, matched_path)?;
    Some(
        requires
            .iter()
            .map(|path| apply_wildcard_bindings(path, &bindings))
            .collect(),
    )
}

fn wildcard_bindings(pattern: &str, matched_path: &str) -> Option<Vec<String>> {
    let pattern_segments = pattern
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let path_segments = matched_path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    if pattern_segments.len() != path_segments.len() {
        return None;
    }

    let mut bindings = Vec::new();
    for (expected, actual) in pattern_segments.iter().zip(path_segments.iter()) {
        if *expected == "*" {
            bindings.push((*actual).to_owned());
        } else if expected != actual {
            return None;
        }
    }

    Some(bindings)
}

fn apply_wildcard_bindings(pattern: &str, bindings: &[String]) -> String {
    let mut binding_index = 0;
    pattern
        .split('.')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            if segment == "*" {
                let resolved = bindings
                    .get(binding_index)
                    .cloned()
                    .unwrap_or_else(|| "*".to_owned());
                binding_index += 1;
                resolved
            } else {
                segment.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn collect_matching_values_recursive<'a>(
    value: &'a Value,
    current: &str,
    segments: &[&str],
    index: usize,
    matches: &mut Vec<(String, &'a Value)>,
) {
    if index == segments.len() {
        matches.push((current.to_owned(), value));
        return;
    }

    let segment = segments[index];
    match (segment, value) {
        ("*", Value::Object(map)) => {
            for (key, child) in map {
                let next = join_path(current, key);
                collect_matching_values_recursive(child, &next, segments, index + 1, matches);
            }
        }
        ("*", Value::Array(values)) => {
            for (child_index, child) in values.iter().enumerate() {
                let next = join_path(current, &child_index.to_string());
                collect_matching_values_recursive(child, &next, segments, index + 1, matches);
            }
        }
        (_, Value::Object(map)) => {
            if let Some(child) = map.get(segment) {
                let next = join_path(current, segment);
                collect_matching_values_recursive(child, &next, segments, index + 1, matches);
            }
        }
        (_, Value::Array(values)) => {
            if let Ok(child_index) = segment.parse::<usize>()
                && let Some(child) = values.get(child_index)
            {
                let next = join_path(current, segment);
                collect_matching_values_recursive(child, &next, segments, index + 1, matches);
            }
        }
        _ => {}
    }
}

fn path_is_present(value: &Value, path: &str) -> bool {
    collect_matching_values(value, path)
        .iter()
        .any(|(_, value)| is_present_value(value))
}

fn present_paths(value: &Value, paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| path_is_present(value, path))
        .cloned()
        .collect()
}

fn missing_paths(value: &Value, paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| !path_is_present(value, path))
        .cloned()
        .collect()
}

fn is_present_value(value: &Value) -> bool {
    !matches!(value, Value::Null)
}

fn redact_group_value(
    value: Value,
    related_paths: &[String],
    secret_paths: &BTreeSet<String>,
) -> Value {
    let mut value = value;
    redact_group_value_recursive(&mut value, "", related_paths, secret_paths);
    value
}

fn redact_group_value_recursive(
    value: &mut Value,
    current: &str,
    related_paths: &[String],
    secret_paths: &BTreeSet<String>,
) {
    if related_paths.iter().any(|path| path == current) && is_secret_path(secret_paths, current) {
        *value = Value::String("***redacted***".to_owned());
        return;
    }

    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let next = join_path(current, key);
                redact_group_value_recursive(child, &next, related_paths, secret_paths);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter_mut().enumerate() {
                let next = join_path(current, &index.to_string());
                redact_group_value_recursive(child, &next, related_paths, secret_paths);
            }
        }
        _ => {}
    }
}
