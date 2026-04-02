use super::*;

impl EnvSource {
    pub(super) fn into_layer(
        self,
        metadata: &ConfigMetadata,
        custom_env_decoders: &BTreeMap<String, CustomEnvDecoder>,
    ) -> Result<Option<Layer>, ConfigError> {
        let EnvSource {
            vars,
            prefix,
            separator,
            lowercase_segments,
            bindings,
        } = self;
        let env_overrides = metadata.env_overrides()?;
        let mut root = Value::Object(Map::new());
        let mut entries = BTreeMap::new();
        let mut coercible_string_paths = BTreeSet::new();
        let mut indexed_array_paths = BTreeSet::new();
        let mut indexed_array_base_lengths = BTreeMap::new();
        let mut current_array_lengths = BTreeMap::new();
        let mut direct_array_paths = BTreeSet::new();
        let mut fallback_vars = Vec::new();

        for (name, raw_value) in vars {
            if let Some(binding) = bindings.get(&name) {
                if binding.fallback {
                    fallback_vars.push((name, raw_value, binding.clone()));
                } else {
                    insert_env_value(
                        &name,
                        &raw_value,
                        &binding.path,
                        binding.decoder,
                        metadata,
                        custom_env_decoders,
                        &mut root,
                        &mut entries,
                        &mut coercible_string_paths,
                        &mut indexed_array_paths,
                        &mut indexed_array_base_lengths,
                        &mut current_array_lengths,
                        &mut direct_array_paths,
                    )?;
                }
                continue;
            }

            if let Some(path) = env_overrides.get(&name) {
                insert_env_value(
                    &name,
                    &raw_value,
                    path,
                    None,
                    metadata,
                    custom_env_decoders,
                    &mut root,
                    &mut entries,
                    &mut coercible_string_paths,
                    &mut indexed_array_paths,
                    &mut indexed_array_base_lengths,
                    &mut current_array_lengths,
                    &mut direct_array_paths,
                )?;
                continue;
            }

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
            insert_env_value(
                &name,
                &raw_value,
                &path,
                None,
                metadata,
                custom_env_decoders,
                &mut root,
                &mut entries,
                &mut coercible_string_paths,
                &mut indexed_array_paths,
                &mut indexed_array_base_lengths,
                &mut current_array_lengths,
                &mut direct_array_paths,
            )?;
        }

        for (name, raw_value, binding) in fallback_vars {
            let normalized = try_normalize_external_path(&binding.path).map_err(|message| {
                ConfigError::InvalidEnv {
                    name: name.clone(),
                    path: binding.path.clone(),
                    message,
                }
            })?;
            if normalized.is_empty() || get_value_at_path(&root, &normalized).is_some() {
                continue;
            }
            insert_env_value(
                &name,
                &raw_value,
                &binding.path,
                binding.decoder,
                metadata,
                custom_env_decoders,
                &mut root,
                &mut entries,
                &mut coercible_string_paths,
                &mut indexed_array_paths,
                &mut indexed_array_base_lengths,
                &mut current_array_lengths,
                &mut direct_array_paths,
            )?;
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

#[allow(clippy::too_many_arguments)]
fn insert_env_value(
    name: &str,
    raw_value: &str,
    path: &str,
    decoder: Option<EnvDecoder>,
    metadata: &ConfigMetadata,
    custom_env_decoders: &BTreeMap<String, CustomEnvDecoder>,
    root: &mut Value,
    entries: &mut BTreeMap<String, SourceTrace>,
    coercible_string_paths: &mut BTreeSet<String>,
    indexed_array_paths: &mut BTreeSet<String>,
    indexed_array_base_lengths: &mut BTreeMap<String, usize>,
    current_array_lengths: &mut BTreeMap<String, usize>,
    direct_array_paths: &mut BTreeSet<String>,
) -> Result<(), ConfigError> {
    let path = try_normalize_external_path(path).map_err(|message| ConfigError::InvalidEnv {
        name: name.to_owned(),
        path: path.to_owned(),
        message,
    })?;
    if path.is_empty() {
        return Ok(());
    }

    let custom_decoder = custom_env_decoder_for_path(path.as_str(), metadata, custom_env_decoders);
    let decoder = decoder.or_else(|| metadata.field(&path).and_then(|field| field.env_decode));
    let parsed =
        parse_env_override_value(raw_value, decoder, custom_decoder).map_err(|message| {
            ConfigError::InvalidEnv {
                name: name.to_owned(),
                path: path.clone(),
                message,
            }
        })?;
    let is_direct_array = parsed.value.is_array();
    let segments = path.split('.').collect::<Vec<_>>();
    record_indexed_array_state(
        current_array_lengths,
        indexed_array_base_lengths,
        &path,
        &segments,
    );
    if is_direct_array {
        record_direct_array_state(
            current_array_lengths,
            indexed_array_base_lengths,
            &path,
            &parsed.value,
        );
    }
    insert_path(root, &segments, parsed.value).map_err(|message| ConfigError::InvalidEnv {
        name: name.to_owned(),
        path: path.clone(),
        message,
    })?;
    for suffix in parsed.string_coercion_suffixes {
        coercible_string_paths.insert(if suffix.is_empty() {
            path.clone()
        } else {
            join_path(&path, &suffix)
        });
    }
    indexed_array_paths.extend(indexed_array_container_paths(&segments));
    if is_direct_array {
        direct_array_paths.insert(path.clone());
    }

    entries.insert(
        path.clone(),
        SourceTrace::new(SourceKind::Environment, name.to_owned()),
    );

    let mut prefix = String::new();
    for segment in segments {
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(segment);
        let entry = entries
            .entry(prefix.clone())
            .or_insert_with(|| SourceTrace::new(SourceKind::Environment, name.to_owned()));
        if prefix != path && entry.name != name {
            *entry = SourceTrace::new(SourceKind::Environment, "environment");
        }
    }

    Ok(())
}

fn custom_env_decoder_for_path<'a>(
    path: &str,
    metadata: &ConfigMetadata,
    custom_env_decoders: &'a BTreeMap<String, CustomEnvDecoder>,
) -> Option<&'a CustomEnvDecoder> {
    let field_path = metadata.field(path).map(|field| field.path.as_str());
    field_path
        .and_then(|canonical| custom_env_decoders.get(canonical))
        .or_else(|| custom_env_decoders.get(path))
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
