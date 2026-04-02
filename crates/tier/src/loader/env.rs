use super::*;

impl EnvSource {
    pub(super) fn into_layer(
        self,
        metadata: &ConfigMetadata,
    ) -> Result<Option<Layer>, ConfigError> {
        let EnvSource {
            vars,
            prefix,
            separator,
            lowercase_segments,
        } = self;
        let env_overrides = metadata.env_overrides()?;
        let mut root = Value::Object(Map::new());
        let mut entries = BTreeMap::new();
        let mut coercible_string_paths = BTreeSet::new();
        let mut indexed_array_paths = BTreeSet::new();
        let mut indexed_array_base_lengths = BTreeMap::new();
        let mut current_array_lengths = BTreeMap::new();
        let mut direct_array_paths = BTreeSet::new();

        for (name, raw_value) in vars {
            let path = match env_overrides.get(&name) {
                Some(path) => path.clone(),
                None => {
                    let Some(path) =
                        path_for_env_var(&name, prefix.as_deref(), &separator, lowercase_segments)
                            .map_err(|error| ConfigError::InvalidEnv {
                                name: name.clone(),
                                path: error.path,
                                message: error.message,
                            })?
                    else {
                        continue;
                    };
                    path
                }
            };
            if path.is_empty() {
                continue;
            }

            let parsed =
                parse_override_value(&raw_value).map_err(|message| ConfigError::InvalidEnv {
                    name: name.clone(),
                    path: path.clone(),
                    message,
                })?;
            let is_direct_array = parsed.value.is_array();
            let segments = path.split('.').collect::<Vec<_>>();
            record_indexed_array_state(
                &mut current_array_lengths,
                &mut indexed_array_base_lengths,
                &path,
                &segments,
            );
            if is_direct_array {
                record_direct_array_state(
                    &mut current_array_lengths,
                    &mut indexed_array_base_lengths,
                    &path,
                    &parsed.value,
                );
            }
            insert_path(&mut root, &segments, parsed.value).map_err(|message| {
                ConfigError::InvalidEnv {
                    name: name.clone(),
                    path: path.clone(),
                    message,
                }
            })?;
            if parsed.allow_string_coercion {
                coercible_string_paths.insert(path.clone());
            }
            indexed_array_paths.extend(indexed_array_container_paths(&segments));
            if is_direct_array {
                direct_array_paths.insert(path.clone());
            }

            entries.insert(
                path.clone(),
                SourceTrace::new(SourceKind::Environment, name.clone()),
            );

            let mut prefix = String::new();
            for segment in segments {
                if !prefix.is_empty() {
                    prefix.push('.');
                }
                prefix.push_str(segment);
                let entry = entries
                    .entry(prefix.clone())
                    .or_insert_with(|| SourceTrace::new(SourceKind::Environment, name.clone()));
                if prefix != path && entry.name != name {
                    *entry = SourceTrace::new(SourceKind::Environment, "environment");
                }
            }
        }

        if entries.is_empty() {
            return Ok(None);
        }

        Ok(Some(Layer {
            trace: SourceTrace::new(SourceKind::Environment, "environment"),
            value: root,
            entries,
            coercible_string_paths,
            indexed_array_paths,
            indexed_array_base_lengths,
            direct_array_paths,
        }))
    }
}

pub(super) struct DerivedEnvPathError {
    pub(super) path: String,
    pub(super) message: String,
}

pub(super) fn path_for_env_var(
    key: &str,
    prefix: Option<&str>,
    separator: &str,
    lowercase_segments: bool,
) -> Result<Option<String>, DerivedEnvPathError> {
    let remainder = if let Some(prefix) = prefix {
        let normalized = normalize_env_prefix(prefix, separator);
        if normalized.is_empty() {
            key
        } else {
            if key == normalized {
                return Ok(None);
            }
            let Some(remainder) = key.strip_prefix(&normalized) else {
                return Ok(None);
            };
            let boundary = if prefix.ends_with(separator) && !separator.is_empty() {
                PrefixBoundary::SeparatorOnly
            } else {
                PrefixBoundary::Flexible
            };
            let Some(remainder) = parse_prefixed_env_remainder(remainder, separator, boundary)
            else {
                return Ok(None);
            };
            remainder
        }
    } else {
        key
    };

    if remainder.is_empty() {
        return Ok(None);
    }

    let mut segments = Vec::new();
    for segment in remainder.split(separator) {
        if segment.is_empty() {
            return Ok(None);
        }
        let segment = if lowercase_segments {
            segment.to_ascii_lowercase()
        } else {
            segment.to_owned()
        };
        if let Some(message) = invalid_path_key_message(&segment) {
            let mut path = segments.join(".");
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(&segment);
            return Err(DerivedEnvPathError {
                path,
                message: format!(
                    "environment variable segments must not contain reserved path syntax: {message}"
                ),
            });
        }
        segments.push(segment);
    }

    Ok(Some(segments.join(".")))
}

#[derive(Clone, Copy)]
pub(super) enum PrefixBoundary {
    SeparatorOnly,
    Flexible,
}

pub(super) fn parse_prefixed_env_remainder<'a>(
    remainder: &'a str,
    separator: &str,
    boundary: PrefixBoundary,
) -> Option<&'a str> {
    let remainder = match boundary {
        PrefixBoundary::SeparatorOnly => remainder.strip_prefix(separator)?,
        PrefixBoundary::Flexible => {
            if let Some(stripped) = remainder.strip_prefix(separator) {
                stripped
            } else if separator == "__" {
                remainder.strip_prefix('_')?
            } else {
                return None;
            }
        }
    };

    (!remainder.is_empty()).then_some(remainder)
}

pub(super) fn normalize_env_prefix(prefix: &str, separator: &str) -> String {
    if prefix.is_empty() {
        return String::new();
    }

    let mut normalized = prefix.to_owned();
    if !separator.is_empty() {
        while normalized.ends_with(separator) {
            normalized.truncate(normalized.len() - separator.len());
        }
    }
    if separator != "_" {
        normalized = normalized.trim_end_matches('_').to_owned();
    }
    normalized
}
