use super::*;

impl EnvSource {
    pub(super) fn into_layer(
        self,
        metadata: &ConfigMetadata,
        env_decoders: &BTreeMap<String, EnvDecoder>,
        custom_env_decoders: &BTreeMap<String, CustomEnvDecoder>,
        runtime_layers: &[Layer],
    ) -> Result<Option<Layer>, ConfigError> {
        let EnvSource {
            vars,
            prefix,
            separator,
            lowercase_segments,
            bindings,
            binding_conflicts,
        } = self;
        if let Some(conflict) = binding_conflicts.into_iter().next() {
            let path = conflict.second.path.clone();
            return Err(ConfigError::InvalidEnv {
                name: conflict.name,
                path,
                message: format!(
                    "conflicting explicit env bindings target `{}` and `{}`",
                    conflict.first.path, conflict.second.path
                ),
            });
        }
        let alias_overrides = metadata.alias_overrides()?;
        validate_binding_paths(&bindings, &alias_overrides)?;
        let env_overrides = metadata.env_overrides()?;
        validate_binding_override_conflicts(
            &bindings,
            &env_overrides,
            &alias_overrides,
            runtime_layers,
        )?;
        let mut root = Value::Object(Map::new());
        let mut entries = BTreeMap::new();
        let mut coercible_string_paths = BTreeSet::new();
        let mut indexed_array_paths = BTreeSet::new();
        let mut indexed_array_base_lengths = BTreeMap::new();
        let mut current_array_lengths = BTreeMap::new();
        let mut direct_array_paths = BTreeSet::new();
        let mut claimed_paths = BTreeMap::<String, String>::new();
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
                        Some(binding.path.clone()),
                        binding.decoder,
                        metadata,
                        &alias_overrides,
                        env_decoders,
                        custom_env_decoders,
                        runtime_layers,
                        &mut root,
                        &mut entries,
                        &mut coercible_string_paths,
                        &mut indexed_array_paths,
                        &mut indexed_array_base_lengths,
                        &mut current_array_lengths,
                        &mut direct_array_paths,
                        &mut claimed_paths,
                    )?;
                }
                continue;
            }

            if let Some(path) = env_overrides.get(&name) {
                insert_env_value(
                    &name,
                    &raw_value,
                    path,
                    Some(path.clone()),
                    None,
                    metadata,
                    &alias_overrides,
                    env_decoders,
                    custom_env_decoders,
                    runtime_layers,
                    &mut root,
                    &mut entries,
                    &mut coercible_string_paths,
                    &mut indexed_array_paths,
                    &mut indexed_array_base_lengths,
                    &mut current_array_lengths,
                    &mut direct_array_paths,
                    &mut claimed_paths,
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
                Some(path.clone()),
                None,
                metadata,
                &alias_overrides,
                env_decoders,
                custom_env_decoders,
                runtime_layers,
                &mut root,
                &mut entries,
                &mut coercible_string_paths,
                &mut indexed_array_paths,
                &mut indexed_array_base_lengths,
                &mut current_array_lengths,
                &mut direct_array_paths,
                &mut claimed_paths,
            )?;
        }

        for (name, raw_value, binding) in fallback_vars {
            let normalized = canonicalize_runtime_env_target_path(
                &name,
                &binding.path,
                &alias_overrides,
                runtime_layers,
                &root,
            )?;
            if normalized.is_empty() || get_value_at_path(&root, &normalized).is_some() {
                continue;
            }
            insert_env_value(
                &name,
                &raw_value,
                &binding.path,
                Some(binding.path.clone()),
                binding.decoder,
                metadata,
                &alias_overrides,
                env_decoders,
                custom_env_decoders,
                runtime_layers,
                &mut root,
                &mut entries,
                &mut coercible_string_paths,
                &mut indexed_array_paths,
                &mut indexed_array_base_lengths,
                &mut current_array_lengths,
                &mut direct_array_paths,
                &mut claimed_paths,
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

fn validate_binding_paths(
    bindings: &BTreeMap<String, EnvBinding>,
    alias_overrides: &BTreeMap<String, String>,
) -> Result<(), ConfigError> {
    for (name, binding) in bindings {
        canonical_env_target_path(name, &binding.path, alias_overrides)?;
    }
    Ok(())
}

fn validate_binding_override_conflicts(
    bindings: &BTreeMap<String, EnvBinding>,
    env_overrides: &BTreeMap<String, String>,
    alias_overrides: &BTreeMap<String, String>,
    runtime_layers: &[Layer],
) -> Result<(), ConfigError> {
    for (name, binding) in bindings {
        let Some(metadata_path) = env_overrides.get(name) else {
            continue;
        };

        let binding_path = canonical_env_target_path(name, &binding.path, alias_overrides)?;
        let binding_path = canonicalize_runtime_path_across_layers(&binding_path, runtime_layers);
        let metadata_path = canonicalize_runtime_path_across_layers(metadata_path, runtime_layers);
        if binding_path != metadata_path {
            return Err(ConfigError::InvalidEnv {
                name: name.clone(),
                path: binding.path.clone(),
                message: format!(
                    "conflicting environment bindings target `{}` via EnvSource and `{}` via metadata",
                    binding_path, metadata_path
                ),
            });
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_env_value(
    name: &str,
    raw_value: &str,
    path: &str,
    external_path: Option<String>,
    decoder: Option<EnvDecoder>,
    metadata: &ConfigMetadata,
    alias_overrides: &BTreeMap<String, String>,
    env_decoders: &BTreeMap<String, EnvDecoder>,
    custom_env_decoders: &BTreeMap<String, CustomEnvDecoder>,
    runtime_layers: &[Layer],
    root: &mut Value,
    entries: &mut BTreeMap<String, SourceTrace>,
    coercible_string_paths: &mut BTreeSet<String>,
    indexed_array_paths: &mut BTreeSet<String>,
    indexed_array_base_lengths: &mut BTreeMap<String, usize>,
    current_array_lengths: &mut BTreeMap<String, usize>,
    direct_array_paths: &mut BTreeSet<String>,
    claimed_paths: &mut BTreeMap<String, String>,
) -> Result<(), ConfigError> {
    let original_path = external_path.unwrap_or_else(|| path.to_owned());
    let path = canonicalize_runtime_env_target_path(
        name,
        &original_path,
        alias_overrides,
        runtime_layers,
        root,
    )?;
    if path.is_empty() {
        return Ok(());
    }
    claim_env_path(name, &path, claimed_paths)?;

    // Variable-level decoder bindings must override path-level custom decoders.
    let custom_decoder = if decoder.is_some() {
        None
    } else {
        custom_env_decoder_for_path(path.as_str(), metadata, custom_env_decoders)
    };
    let decoder = decoder
        .or_else(|| env_decoder_for_path(path.as_str(), metadata, env_decoders))
        .or_else(|| metadata.field(&path).and_then(|field| field.env_decode));
    let parsed =
        parse_env_override_value(raw_value, decoder, custom_decoder).map_err(|message| {
            ConfigError::InvalidEnv {
                name: name.to_owned(),
                path: path.clone(),
                message,
            }
        })?;
    ensure_path_safe_keys(&parsed.value, &path).map_err(|error| match error {
        ConfigError::InvalidPathKey { path, key, message } => ConfigError::InvalidEnv {
            name: name.to_owned(),
            path,
            message: format!(
                "decoded environment value contains unsupported object key `{key}`: {message}"
            ),
        },
        _ => error,
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

fn canonical_env_target_path(
    name: &str,
    path: &str,
    alias_overrides: &BTreeMap<String, String>,
) -> Result<String, ConfigError> {
    let normalized =
        try_normalize_external_path(path).map_err(|message| ConfigError::InvalidEnv {
            name: name.to_owned(),
            path: path.to_owned(),
            message,
        })?;
    if normalized.is_empty() {
        return Err(ConfigError::InvalidEnv {
            name: name.to_owned(),
            path: path.to_owned(),
            message: "environment binding path cannot be empty".to_owned(),
        });
    }
    Ok(canonicalize_path_with_aliases(&normalized, alias_overrides))
}

fn canonicalize_runtime_env_target_path(
    name: &str,
    path: &str,
    alias_overrides: &BTreeMap<String, String>,
    runtime_layers: &[Layer],
    current_root: &Value,
) -> Result<String, ConfigError> {
    let path = canonical_env_target_path(name, path, alias_overrides)?;
    let path = canonicalize_runtime_path_across_layers(&path, runtime_layers);
    Ok(canonicalize_runtime_path(current_root, &path))
}

fn claim_env_path(
    name: &str,
    path: &str,
    claimed_paths: &mut BTreeMap<String, String>,
) -> Result<(), ConfigError> {
    for (existing_path, existing_name) in claimed_paths.iter() {
        if existing_name == name {
            continue;
        }

        if existing_path == path {
            return Err(ConfigError::InvalidEnv {
                name: name.to_owned(),
                path: path.to_owned(),
                message: format!(
                    "conflicting environment variables `{existing_name}` and `{name}` both target `{path}`"
                ),
            });
        }

        if paths_overlap(existing_path, path) {
            return Err(ConfigError::InvalidEnv {
                name: name.to_owned(),
                path: path.to_owned(),
                message: format!(
                    "conflicting environment variables `{existing_name}` and `{name}` target overlapping configuration paths `{existing_path}` and `{path}`"
                ),
            });
        }
    }

    claimed_paths.insert(path.to_owned(), name.to_owned());
    Ok(())
}

fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('.'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

fn custom_env_decoder_for_path<'a>(
    path: &str,
    metadata: &ConfigMetadata,
    custom_env_decoders: &'a BTreeMap<String, CustomEnvDecoder>,
) -> Option<&'a CustomEnvDecoder> {
    let canonical = metadata
        .field(path)
        .map_or(path, |field| field.path.as_str());
    let mut best = None::<((usize, usize, Vec<bool>), &CustomEnvDecoder)>;

    for (pattern, decoder) in custom_env_decoders {
        if !path_matches_pattern(canonical, pattern) {
            continue;
        }

        let segments = pattern.split('.').collect::<Vec<_>>();
        let score = (
            segments.len(),
            segments.iter().filter(|segment| **segment != "*").count(),
            segments
                .iter()
                .map(|segment| *segment != "*")
                .collect::<Vec<_>>(),
        );

        match &mut best {
            Some((best_score, best_decoder)) if score > *best_score => {
                *best_score = score;
                *best_decoder = decoder;
            }
            None => best = Some((score, decoder)),
            _ => {}
        }
    }

    best.map(|(_, decoder)| decoder)
}

fn env_decoder_for_path(
    path: &str,
    metadata: &ConfigMetadata,
    env_decoders: &BTreeMap<String, EnvDecoder>,
) -> Option<EnvDecoder> {
    let canonical = metadata
        .field(path)
        .map_or(path, |field| field.path.as_str());
    let mut best = None::<((usize, usize, Vec<bool>), EnvDecoder)>;

    for (pattern, decoder) in env_decoders {
        if !path_matches_pattern(canonical, pattern) {
            continue;
        }

        let segments = pattern.split('.').collect::<Vec<_>>();
        let score = (
            segments.len(),
            segments.iter().filter(|segment| **segment != "*").count(),
            segments
                .iter()
                .map(|segment| *segment != "*")
                .collect::<Vec<_>>(),
        );

        match &mut best {
            Some((best_score, best_decoder)) if score > *best_score => {
                *best_score = score;
                *best_decoder = *decoder;
            }
            None => best = Some((score, *decoder)),
            _ => {}
        }
    }

    best.map(|(_, decoder)| decoder)
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
