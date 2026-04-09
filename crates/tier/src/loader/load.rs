use super::*;

impl<T> ConfigLoader<T>
where
    T: Serialize + DeserializeOwned,
{
    /// Loads configuration from all configured layers.
    pub fn load(self) -> Result<LoadedConfig<T>, ConfigError> {
        let unknown_field_policy = self.unknown_field_policy;
        let mut metadata = self.metadata.clone();
        metadata.canonicalize_env_decoder_paths()?;
        metadata.validate_paths()?;
        if let Some((path, _)) = &self.config_version {
            let _ = normalize_version_registration_path(path)?;
        }
        validate_config_migrations(&self.migrations)?;
        let parsed_args = match self.args_source {
            Some(source) => Some(parse_args(source)?),
            None => None,
        };

        let profile = parsed_args
            .as_ref()
            .and_then(|args| args.profile.clone())
            .or(self.profile);
        let defaults_shape = serde_json::to_value(&self.defaults)?;
        ensure_root_object(&defaults_shape)?;
        if !self.migrations.is_empty() && self.config_version.is_none() {
            return Err(ConfigError::MetadataInvalid {
                path: String::new(),
                message:
                    "configuration migrations require ConfigLoader::config_version(...) to be set"
                        .to_owned(),
            });
        }

        let mut layers = Vec::new();
        layers.push(canonicalize_layer_paths(
            Layer::from_serializable(
                SourceTrace::new(SourceKind::Default, "defaults"),
                &self.defaults,
            )?,
            &metadata,
        )?);

        let mut files = self.files;
        if let Some(parsed) = &parsed_args {
            files.extend(parsed.files.clone());
        }

        for file in files {
            if let Some(layer) = load_file_layer(file, profile.as_deref())? {
                layers.push(canonicalize_layer_paths(layer, &metadata)?);
            }
        }

        for pending in self.custom_layers {
            metadata = canonicalize_metadata_against_layers(&metadata, &layers)?;
            let layer = match pending {
                PendingCustomLayer::Immediate(layer) => layer,
                PendingCustomLayer::DeferredPatch(patch) => patch.into_layer_with_shape(
                    merged_shape_from_layers(&defaults_shape, &layers, &metadata)?,
                )?,
            };
            layers.push(canonicalize_layer_paths(layer, &metadata)?);
        }

        for env_source in self.env_sources {
            metadata = canonicalize_metadata_against_layers(&metadata, &layers)?;
            let env_decoders = canonicalize_env_decoders(&self.env_decoders, &metadata, &layers)?;
            let custom_env_decoders =
                canonicalize_custom_env_decoders(&self.custom_env_decoders, &metadata, &layers)?;
            let layer =
                env_source.into_layer(&metadata, &env_decoders, &custom_env_decoders, &layers)?;
            if let Some(layer) = layer {
                layers.push(canonicalize_layer_paths(layer, &metadata)?);
            }
        }

        if let Some(parsed) = parsed_args
            && let Some(layer) = parsed.layer
        {
            layers.push(canonicalize_layer_paths(layer, &metadata)?);
        }

        for patch in self.typed_arg_layers {
            metadata = canonicalize_metadata_against_layers(&metadata, &layers)?;
            let layer = patch.into_layer_with_shape(merged_shape_from_layers(
                &defaults_shape,
                &layers,
                &metadata,
            )?)?;
            layers.push(canonicalize_layer_paths(layer, &metadata)?);
        }

        let mut metadata = canonicalize_metadata_against_layers(&metadata, &layers)?;
        let mut alias_overrides = metadata.alias_overrides()?;
        let pending_secret_paths = canonicalize_secret_paths(
            &normalize_secret_registration_paths(&self.secret_paths)?,
            &alias_overrides,
        );
        let mut secret_paths = canonicalize_secret_paths_against_layers(
            &pending_secret_paths,
            &layers,
            &alias_overrides,
        );

        let defaults_value =
            canonicalize_value_paths(&serde_json::to_value(&self.defaults)?, &metadata)?;
        let default_known_paths = collect_known_paths_from_value(&defaults_value);
        let pre_deserialize_suggestion_paths =
            collect_suggestion_paths(&metadata, &default_known_paths);

        let mut report = ConfigReport::new(
            defaults_value.clone(),
            secret_paths.clone(),
            alias_overrides.clone(),
        );
        let mut string_coercion_paths = BTreeSet::new();

        let mut merged = defaults_value;
        ensure_root_object(&merged)?;

        for layer in layers {
            string_coercion_paths.extend(layer.coercible_string_paths.iter().cloned());
            validate_indexed_array_paths(&merged, &layer)?;
            enforce_source_policies(&layer, &metadata)?;
            report.record_source(layer.trace.clone());
            record_layer_steps(&mut report, &layer, &secret_paths);
            record_deprecation_warnings(&mut report, &layer, &metadata);
            if !matches!(layer.trace.kind, SourceKind::Default) {
                merge_values(
                    &mut merged,
                    layer.value,
                    "",
                    &metadata,
                    &layer.indexed_array_paths,
                    &layer.direct_array_paths,
                );
            }
        }

        if let Some((version_path, current_version)) = &self.config_version {
            let version_path = normalize_version_registration_path(version_path)?;
            apply_config_migrations(
                &mut merged,
                &version_path,
                *current_version,
                &self.migrations,
                &mut report,
            )?;
        }

        let mut config = match deserialize_with_path(&merged, &report, &string_coercion_paths) {
            Ok(config) => config,
            Err(error) => {
                if !matches!(unknown_field_policy, UnknownFieldPolicy::Allow) {
                    let mut unknown_fields = collect_unknown_fields_best_effort::<T>(
                        &merged,
                        &pre_deserialize_suggestion_paths,
                        &report,
                        &string_coercion_paths,
                    );
                    if unknown_fields.is_empty() && !metadata.fields().is_empty() {
                        unknown_fields = collect_unknown_fields_from_metadata_scope(
                            &merged,
                            &metadata,
                            &pre_deserialize_suggestion_paths,
                            &report,
                            deserialize_error_scope(error_path_for_scope(&error)),
                        );
                    }
                    if !unknown_fields.is_empty() {
                        return Err(ConfigError::UnknownFields {
                            fields: unknown_fields,
                        });
                    }
                }
                return Err(error);
            }
        };
        let known_paths = collect_known_paths(&config)?;
        let suggestion_paths = collect_suggestion_paths(&metadata, &known_paths);
        if !matches!(unknown_field_policy, UnknownFieldPolicy::Allow) {
            let unknown_fields = collect_unknown_fields::<T>(
                &merged,
                &suggestion_paths,
                &report,
                &string_coercion_paths,
            )?;
            if !unknown_fields.is_empty() {
                match unknown_field_policy {
                    UnknownFieldPolicy::Allow => {}
                    UnknownFieldPolicy::Warn => {
                        for field in unknown_fields {
                            report.record_warning(ConfigWarning::UnknownField(field));
                        }
                    }
                    UnknownFieldPolicy::Deny => {
                        return Err(ConfigError::UnknownFields {
                            fields: unknown_fields,
                        });
                    }
                }
            }
        }

        for normalizer in self.normalizers {
            let before = serde_json::to_value(&config)?;
            (normalizer.run)(&mut config).map_err(|message| ConfigError::Normalize {
                name: normalizer.name.clone(),
                message,
            })?;
            let after = serde_json::to_value(&config)?;
            ensure_root_object(&after)?;
            ensure_path_safe_keys(&after, "")?;
            metadata = canonicalize_metadata_against_value(&metadata, &after)?;
            alias_overrides = metadata.alias_overrides()?;
            secret_paths = canonicalize_secret_paths_against_value(
                &pending_secret_paths,
                &after,
                &alias_overrides,
            );
            let trace = SourceTrace::new(SourceKind::Normalization, normalizer.name.clone());
            report.record_source(trace.clone());
            record_diff_steps(&mut report, &before, &after, &trace, &secret_paths);
        }

        report.replace_runtime_metadata(secret_paths.clone(), alias_overrides.clone());
        let normalized_value =
            canonicalize_value_paths(&serde_json::to_value(&config)?, &metadata)?;
        let mut declared_errors =
            validate_declared_rules(&normalized_value, &metadata, &secret_paths, &mut report);
        declared_errors.extend(validate_declared_checks(
            &normalized_value,
            &metadata,
            &secret_paths,
        ));
        if !declared_errors.is_empty() {
            return Err(ConfigError::DeclaredValidation {
                errors: declared_errors,
            });
        }
        if metadata
            .fields()
            .iter()
            .any(|field| !field.validations.is_empty())
        {
            report.record_validation("tier::declared.fields".to_owned());
        }
        if !metadata.checks().is_empty() {
            report.record_validation("tier::declared.checks".to_owned());
        }

        for validator in self.validators {
            (validator.run)(&config).map_err(|errors| ConfigError::Validation {
                name: validator.name.clone(),
                errors,
            })?;
            report.record_validation(validator.name);
        }

        let final_value = canonicalize_value_paths(&serde_json::to_value(&config)?, &metadata)?;
        report.replace_final_value(final_value);

        Ok(LoadedConfig { config, report })
    }
}

pub(super) fn canonicalize_custom_env_decoders(
    decoders: &BTreeMap<String, CustomEnvDecoder>,
    metadata: &ConfigMetadata,
    layers: &[Layer],
) -> Result<BTreeMap<String, CustomEnvDecoder>, ConfigError> {
    let aliases = metadata.alias_overrides()?;
    let mut canonicalized = BTreeMap::new();
    let mut origins = BTreeMap::<String, String>::new();

    for (path, decoder) in decoders {
        let normalized = canonicalize_runtime_path_across_layers(
            &normalize_decoder_registration_path(path)?,
            layers,
        );
        let canonical = canonicalize_path_with_aliases(&normalized, &aliases);
        if let Some(first_path) = origins.get(&canonical)
            && first_path != &normalized
        {
            return Err(ConfigError::MetadataConflict {
                kind: "environment decoder",
                name: canonical,
                first_path: first_path.clone(),
                second_path: normalized,
            });
        }

        origins.insert(canonical.clone(), normalized);
        canonicalized.insert(canonical, Arc::clone(decoder));
    }

    Ok(canonicalized)
}

pub(super) fn canonicalize_env_decoders(
    decoders: &BTreeMap<String, EnvDecoder>,
    metadata: &ConfigMetadata,
    layers: &[Layer],
) -> Result<BTreeMap<String, EnvDecoder>, ConfigError> {
    let aliases = metadata.alias_overrides()?;
    let mut canonicalized = BTreeMap::new();
    let mut origins = BTreeMap::<String, (String, EnvDecoder)>::new();

    for (path, decoder) in decoders {
        let normalized = canonicalize_runtime_path_across_layers(
            &normalize_decoder_registration_path(path)?,
            layers,
        );
        let canonical = canonicalize_path_with_aliases(&normalized, &aliases);
        if let Some((first_path, first_decoder)) = origins.get(&canonical)
            && (first_path != &normalized || *first_decoder != *decoder)
        {
            return Err(ConfigError::MetadataConflict {
                kind: "environment decoder",
                name: canonical,
                first_path: first_path.clone(),
                second_path: normalized,
            });
        }

        origins.insert(canonical.clone(), (normalized, *decoder));
        canonicalized.insert(canonical, *decoder);
    }

    Ok(canonicalized)
}

pub(super) fn normalize_decoder_registration_path(path: &str) -> Result<String, ConfigError> {
    let normalized =
        try_normalize_external_path(path).map_err(|message| ConfigError::MetadataInvalid {
            path: path.to_owned(),
            message: format!("invalid environment decoder path: {message}"),
        })?;
    if normalized.is_empty() {
        return Err(ConfigError::MetadataInvalid {
            path: path.to_owned(),
            message: "invalid environment decoder path: configuration path cannot be empty"
                .to_owned(),
        });
    }
    Ok(normalized)
}

fn normalize_secret_registration_paths(
    secret_paths: &BTreeSet<String>,
) -> Result<BTreeSet<String>, ConfigError> {
    secret_paths
        .iter()
        .map(|path| {
            let normalized = try_normalize_external_path(path).map_err(|message| {
                ConfigError::MetadataInvalid {
                    path: path.clone(),
                    message: format!("invalid secret path: {message}"),
                }
            })?;
            if normalized.is_empty() {
                return Err(ConfigError::MetadataInvalid {
                    path: path.clone(),
                    message: "invalid secret path: configuration path cannot be empty".to_owned(),
                });
            }
            Ok(normalized)
        })
        .collect()
}

fn normalize_version_registration_path(path: &str) -> Result<String, ConfigError> {
    let normalized =
        try_normalize_external_path(path).map_err(|message| ConfigError::MetadataInvalid {
            path: path.to_owned(),
            message: format!("invalid configuration version path: {message}"),
        })?;
    if normalized.is_empty() {
        return Err(ConfigError::MetadataInvalid {
            path: path.to_owned(),
            message: "configuration version path cannot be empty".to_owned(),
        });
    }
    Ok(normalized)
}

fn normalize_migration_registration_path(path: &str) -> Result<String, ConfigError> {
    let normalized =
        try_normalize_external_path(path).map_err(|message| ConfigError::MetadataInvalid {
            path: path.to_owned(),
            message: format!("invalid migration path: {message}"),
        })?;
    if normalized.is_empty() {
        return Err(ConfigError::MetadataInvalid {
            path: path.to_owned(),
            message: "migration paths cannot target the configuration root".to_owned(),
        });
    }
    Ok(normalized)
}

fn validate_config_migrations(migrations: &[ConfigMigration]) -> Result<(), ConfigError> {
    for migration in migrations {
        match &migration.kind {
            ConfigMigrationKind::Rename { from, to } => {
                let _ = normalize_migration_registration_path(from)?;
                let _ = normalize_migration_registration_path(to)?;
            }
            ConfigMigrationKind::Remove { path } => {
                let _ = normalize_migration_registration_path(path)?;
            }
        }
    }

    Ok(())
}

fn apply_config_migrations(
    merged: &mut Value,
    version_path: &str,
    current_version: u32,
    migrations: &[ConfigMigration],
    report: &mut ConfigReport,
) -> Result<(), ConfigError> {
    let version_path = canonicalize_runtime_path(merged, version_path);
    let mut working_version = read_config_version(merged, &version_path)?;
    if working_version > current_version {
        return Err(ConfigError::UnsupportedConfigVersion {
            path: version_path,
            found: working_version,
            supported: current_version,
        });
    }

    let mut sorted = migrations.to_vec();
    sorted.sort_by_key(|migration| migration.since_version);

    for migration in sorted {
        if migration.since_version <= working_version || migration.since_version > current_version {
            continue;
        }

        match &migration.kind {
            ConfigMigrationKind::Rename { from, to } => {
                let from = canonicalize_runtime_path(
                    merged,
                    &normalize_migration_registration_path(from)?,
                );
                let to =
                    canonicalize_runtime_path(merged, &normalize_migration_registration_path(to)?);
                if let Some(value) = take_value_at_path(merged, &from) {
                    insert_normalized_path(merged, &to, value).map_err(|message| {
                        ConfigError::MetadataInvalid {
                            path: to.clone(),
                            message: format!("failed to apply migration: {message}"),
                        }
                    })?;
                    report.record_migration(AppliedMigration {
                        kind: "rename".to_owned(),
                        from_version: working_version,
                        to_version: migration.since_version,
                        from_path: from,
                        to_path: Some(to),
                        note: migration.note.clone(),
                    });
                }
            }
            ConfigMigrationKind::Remove { path } => {
                let path = canonicalize_runtime_path(
                    merged,
                    &normalize_migration_registration_path(path)?,
                );
                if take_value_at_path(merged, &path).is_some() {
                    report.record_migration(AppliedMigration {
                        kind: "remove".to_owned(),
                        from_version: working_version,
                        to_version: migration.since_version,
                        from_path: path,
                        to_path: None,
                        note: migration.note.clone(),
                    });
                }
            }
        }

        working_version = migration.since_version;
    }

    insert_normalized_path(
        merged,
        &version_path,
        Value::Number(serde_json::Number::from(current_version)),
    )
    .map_err(|message| ConfigError::InvalidConfigVersion {
        path: version_path,
        message,
    })?;

    Ok(())
}

fn read_config_version(value: &Value, path: &str) -> Result<u32, ConfigError> {
    let Some(found) = get_value_at_path(value, path) else {
        return Ok(0);
    };

    let Some(version) = found.as_u64() else {
        return Err(ConfigError::InvalidConfigVersion {
            path: path.to_owned(),
            message: "expected an unsigned integer".to_owned(),
        });
    };

    u32::try_from(version).map_err(|_| ConfigError::InvalidConfigVersion {
        path: path.to_owned(),
        message: "version must fit in u32".to_owned(),
    })
}

fn insert_normalized_path(root: &mut Value, path: &str, value: Value) -> Result<(), String> {
    let segments = path.split('.').collect::<Vec<_>>();
    insert_path(root, &segments, value)
}

fn take_value_at_path(root: &mut Value, path: &str) -> Option<Value> {
    let segments = path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    take_value_at_segments(root, &segments)
}

fn take_value_at_segments(current: &mut Value, segments: &[&str]) -> Option<Value> {
    let segment = segments.first()?;
    if segments.len() == 1 {
        return match current {
            Value::Object(map) => map.remove(*segment),
            Value::Array(values) => {
                let index = segment.parse::<usize>().ok()?;
                (index < values.len()).then(|| values.remove(index))
            }
            _ => None,
        };
    }

    match current {
        Value::Object(map) => {
            let child = map.get_mut(*segment)?;
            take_value_at_segments(child, &segments[1..])
        }
        Value::Array(values) => {
            let index = segment.parse::<usize>().ok()?;
            let child = values.get_mut(index)?;
            take_value_at_segments(child, &segments[1..])
        }
        _ => None,
    }
}

fn validate_indexed_array_paths(base: &Value, layer: &Layer) -> Result<(), ConfigError> {
    for path in &layer.indexed_array_paths {
        let base_len = if let Some(base_len) = layer.indexed_array_base_lengths.get(path) {
            *base_len
        } else if layer.direct_array_paths.contains(path) {
            continue;
        } else {
            match get_value_at_path(base, path) {
                Some(Value::Array(values)) => values.len(),
                _ => 0,
            }
        };

        let mut explicit_indices = layer
            .entries
            .iter()
            .filter_map(|(entry_path, _)| direct_child_array_index(path, entry_path))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if explicit_indices.is_empty() {
            continue;
        }
        explicit_indices.retain(|index| *index >= base_len);
        if explicit_indices.is_empty() {
            continue;
        }

        for (offset, index) in explicit_indices.iter().enumerate() {
            let expected = base_len + offset;
            if *index != expected {
                return Err(sparse_indexed_array_error(layer, path, *index, expected));
            }
        }
    }

    Ok(())
}

fn sparse_indexed_array_error(
    layer: &Layer,
    container_path: &str,
    offending_index: usize,
    expected_index: usize,
) -> ConfigError {
    let offending_path = join_path(container_path, &offending_index.to_string());
    let source = layer
        .entries
        .iter()
        .filter(|(entry_path, _)| {
            direct_child_array_index(container_path, entry_path) == Some(offending_index)
        })
        .max_by_key(|(entry_path, trace)| {
            (
                !is_generic_layer_trace(trace),
                entry_path.split('.').count(),
                entry_path.len(),
            )
        })
        .or_else(|| {
            layer
                .entries
                .iter()
                .find(|(entry_path, _)| *entry_path == &offending_path)
        });
    let message = format!(
        "sparse array override at `{container_path}`: index {offending_index} requires index {expected_index} to be set first"
    );

    match source {
        Some((_, trace)) => match trace.kind {
            SourceKind::Environment => ConfigError::InvalidEnv {
                name: trace.name.clone(),
                path: offending_path,
                message,
            },
            SourceKind::Arguments => ConfigError::InvalidArg {
                arg: trace.name.clone(),
                message,
            },
            _ => ConfigError::MetadataInvalid {
                path: offending_path,
                message,
            },
        },
        None => ConfigError::MetadataInvalid {
            path: offending_path,
            message,
        },
    }
}

fn is_generic_layer_trace(trace: &SourceTrace) -> bool {
    matches!(
        (trace.kind, trace.name.as_str()),
        (SourceKind::Arguments, "arguments") | (SourceKind::Environment, "environment")
    )
}

fn record_layer_steps(report: &mut ConfigReport, layer: &Layer, secret_paths: &BTreeSet<String>) {
    report.record_step(
        String::new(),
        ResolutionStep {
            source: layer.trace.clone(),
            value: redact_value(&layer.value, "", secret_paths),
            redacted: path_contains_secret(secret_paths, ""),
        },
    );

    for (path, trace) in &layer.entries {
        if let Some(value) = get_value_at_path(&layer.value, path) {
            let redacted = path_contains_secret(secret_paths, path);
            let rendered = redact_value(value, path, secret_paths);
            report.record_step(
                path.clone(),
                ResolutionStep {
                    source: trace.clone(),
                    value: rendered,
                    redacted,
                },
            );
        }
    }
}

fn record_diff_steps(
    report: &mut ConfigReport,
    before: &Value,
    after: &Value,
    trace: &SourceTrace,
    secret_paths: &BTreeSet<String>,
) {
    if before != after {
        report.record_step(
            String::new(),
            ResolutionStep {
                source: trace.clone(),
                value: redact_value(after, "", secret_paths),
                redacted: path_contains_secret(secret_paths, ""),
            },
        );
    }

    let mut paths = Vec::new();
    collect_diff_paths(before, after, "", &mut paths);
    paths.sort();
    paths.dedup();

    for path in paths {
        let after_value = get_value_at_path(after, &path).cloned();
        let removed = after_value.is_none() && get_value_at_path(before, &path).is_some();
        if !removed && after_value.is_none() {
            continue;
        }

        let redacted = path_contains_secret(secret_paths, &path);
        let rendered = match after_value {
            Some(value) => redact_value(&value, &path, secret_paths),
            None => Value::Null,
        };
        report.record_step(
            path,
            ResolutionStep {
                source: trace.clone(),
                value: rendered,
                redacted,
            },
        );
    }
}

fn record_deprecation_warnings(
    report: &mut ConfigReport,
    layer: &Layer,
    metadata: &ConfigMetadata,
) {
    if matches!(layer.trace.kind, SourceKind::Default) {
        return;
    }

    let deprecated = metadata
        .fields()
        .iter()
        .filter(|field| field.deprecated.is_some())
        .collect::<Vec<_>>();
    if deprecated.is_empty() {
        return;
    }

    let mut used_paths = Vec::new();
    collect_paths(&layer.value, "", &mut used_paths);
    used_paths.sort();
    used_paths.dedup();

    let mut warned = BTreeSet::new();
    for field in deprecated {
        let used = used_paths
            .iter()
            .any(|path| path_starts_with_pattern(path, &field.path));
        if used && warned.insert(field.path.clone()) {
            report.record_warning(ConfigWarning::DeprecatedField(
                DeprecatedField::new(field.path.clone())
                    .with_source(Some(layer.trace.clone()))
                    .with_note(field.deprecated.clone()),
            ));
        }
    }
}

pub(crate) fn is_secret_path(secret_paths: &BTreeSet<String>, path: &str) -> bool {
    secret_paths
        .iter()
        .any(|secret| path_starts_with_pattern(path, secret))
}

fn path_contains_secret(secret_paths: &BTreeSet<String>, path: &str) -> bool {
    secret_paths
        .iter()
        .any(|secret| path_overlaps_pattern(path, secret))
}
