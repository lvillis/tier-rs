use super::*;

impl ConfigMetadata {
    /// Creates an empty metadata set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates metadata from a list of field entries.
    #[must_use]
    pub fn from_fields<I>(fields: I) -> Self
    where
        I: IntoIterator<Item = FieldMetadata>,
    {
        let mut metadata = Self::default();
        metadata.extend_fields(fields);
        metadata
    }

    /// Returns all merged field metadata entries.
    #[must_use]
    pub fn fields(&self) -> &[FieldMetadata] {
        &self.fields
    }

    /// Returns all normalized cross-field validation checks.
    #[must_use]
    pub fn checks(&self) -> &[ValidationCheck] {
        &self.checks
    }

    /// Returns the metadata entry for a normalized configuration path or alias.
    #[must_use]
    pub fn field(&self, path: &str) -> Option<&FieldMetadata> {
        let normalized = try_normalize_metadata_path(path).ok()?;
        let mut best = None::<(MetadataMatchScore, &FieldMetadata)>;
        for field in &self.fields {
            for candidate in
                std::iter::once(field.path.as_str()).chain(field.aliases.iter().map(String::as_str))
            {
                let Some(score) = metadata_match_score(&normalized, candidate) else {
                    continue;
                };

                match &mut best {
                    Some((best_score, best_field)) if score > *best_score => {
                        *best_score = score;
                        *best_field = field;
                    }
                    None => best = Some((score, field)),
                    _ => {}
                }
            }
        }

        best.map(|(_, field)| field)
    }

    pub(crate) fn matching_fields_for_path(&self, path: &str) -> Vec<&FieldMetadata> {
        let normalized = match try_normalize_metadata_path(path) {
            Ok(normalized) => normalized,
            Err(_) => return Vec::new(),
        };

        let mut matches = self
            .fields
            .iter()
            .filter_map(|field| {
                let best = std::iter::once(field.path.as_str())
                    .chain(field.aliases.iter().map(String::as_str))
                    .filter_map(|candidate| metadata_match_score(&normalized, candidate))
                    .max();
                best.map(|score| (score, field))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.path.cmp(&right.1.path))
        });
        matches.into_iter().map(|(_, field)| field).collect()
    }

    pub(crate) fn effective_source_policy_for(&self, path: &str) -> Option<EffectiveSourcePolicy> {
        let mut policy = EffectiveSourcePolicy::default();
        let mut has_policy = false;

        for field in self.matching_fields_for_path(path) {
            if field.allowed_sources.is_some() || field.denied_sources.is_some() {
                has_policy = true;
                policy.apply_field(field);
            }
        }

        has_policy.then_some(policy)
    }

    pub(crate) fn effective_validations_for(&self, path: &str) -> Vec<EffectiveValidation> {
        let Some(field) = self.effective_field_for(path) else {
            return Vec::new();
        };

        field
            .validations
            .iter()
            .cloned()
            .map(|rule| EffectiveValidation {
                field: field.clone(),
                rule,
            })
            .collect()
    }

    pub(crate) fn effective_field_for(&self, path: &str) -> Option<FieldMetadata> {
        let mut matches = self.matching_fields_for_path(path).into_iter();
        let mut effective = matches.next()?.clone();
        for field in matches {
            effective.merge_from(field.clone());
        }
        Some(effective)
    }

    /// Returns metadata entries keyed by normalized path.
    #[must_use]
    pub fn fields_by_path(&self) -> BTreeMap<String, FieldMetadata> {
        self.fields
            .iter()
            .cloned()
            .map(|field| (field.path.clone(), field))
            .collect()
    }

    /// Adds a field metadata entry and merges duplicates by path.
    pub fn push(&mut self, field: FieldMetadata) {
        self.fields.push(field);
        self.normalize();
    }

    /// Extends the metadata with additional field entries.
    pub fn extend_fields<I>(&mut self, fields: I)
    where
        I: IntoIterator<Item = FieldMetadata>,
    {
        self.fields.extend(fields);
        self.normalize();
    }

    /// Extends the metadata with another metadata set.
    pub fn extend(&mut self, other: Self) {
        self.fields.extend(other.fields);
        self.checks.extend(other.checks);
        self.normalize();
    }

    /// Adds a cross-field validation check.
    pub fn push_check(&mut self, check: ValidationCheck) {
        self.checks.push(check);
        self.normalize();
    }

    /// Extends the metadata with additional cross-field validation checks.
    pub fn extend_checks<I>(&mut self, checks: I)
    where
        I: IntoIterator<Item = ValidationCheck>,
    {
        self.checks.extend(checks);
        self.normalize();
    }

    /// Adds a cross-field validation check in builder style.
    #[must_use]
    pub fn check(mut self, check: ValidationCheck) -> Self {
        self.push_check(check);
        self
    }

    /// Requires that at least one of the given paths is configured.
    #[must_use]
    pub fn at_least_one_of<I, S>(self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.check(ValidationCheck::AtLeastOneOf {
            paths: paths.into_iter().map(Into::into).collect(),
        })
    }

    /// Requires that exactly one of the given paths is configured.
    #[must_use]
    pub fn exactly_one_of<I, S>(self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.check(ValidationCheck::ExactlyOneOf {
            paths: paths.into_iter().map(Into::into).collect(),
        })
    }

    /// Requires that at most one of the given paths is configured.
    #[must_use]
    pub fn mutually_exclusive<I, S>(self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.check(ValidationCheck::MutuallyExclusive {
            paths: paths.into_iter().map(Into::into).collect(),
        })
    }

    /// Requires one or more paths whenever `path` is configured.
    #[must_use]
    pub fn required_with<I, S>(self, path: impl Into<String>, requires: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.check(ValidationCheck::RequiredWith {
            path: path.into(),
            requires: requires.into_iter().map(Into::into).collect(),
        })
    }

    /// Requires one or more paths whenever `path` equals `equals`.
    #[must_use]
    pub fn required_if<I, S, V>(self, path: impl Into<String>, equals: V, requires: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
        V: Into<ValidationValue>,
    {
        self.check(ValidationCheck::RequiredIf {
            path: path.into(),
            equals: equals.into(),
            requires: requires.into_iter().map(Into::into).collect(),
        })
    }

    /// Returns all normalized secret paths.
    #[must_use]
    pub fn secret_paths(&self) -> Vec<String> {
        self.fields
            .iter()
            .filter(|field| field.secret)
            .map(|field| field.path.clone())
            .collect()
    }

    pub(crate) fn canonicalize_env_decoder_paths(&mut self) -> Result<(), ConfigError> {
        let alias_source_fields = self
            .fields
            .iter()
            .filter(|field| !field.is_env_decoder_only())
            .cloned()
            .collect::<Vec<_>>();
        let aliases = ConfigMetadata {
            fields: alias_source_fields,
            checks: Vec::new(),
        }
        .alias_overrides()?;

        let mut seen = BTreeMap::<String, (String, EnvDecoder)>::new();
        for field in &mut self.fields {
            if !field.is_env_decoder_only() {
                continue;
            }

            let original_path = field.path.clone();
            let canonical = canonicalize_path_with_aliases(&original_path, &aliases);
            let decoder = field
                .env_decode
                .expect("decoder-only fields must have a decoder");
            if let Some((first_path, first_decoder)) = seen.get(&canonical)
                && (first_path != &original_path || *first_decoder != decoder)
            {
                return Err(ConfigError::MetadataConflict {
                    kind: "environment decoder",
                    name: canonical,
                    first_path: first_path.clone(),
                    second_path: original_path,
                });
            }

            seen.insert(canonical.clone(), (original_path, decoder));
            field.path = canonical;
        }

        self.normalize();
        Ok(())
    }

    /// Returns explicit environment variable name overrides keyed by env name.
    pub fn env_overrides(&self) -> Result<BTreeMap<String, String>, ConfigError> {
        let aliases = self.alias_overrides()?;
        let mut envs = BTreeMap::new();
        let mut canonical_targets = BTreeMap::<String, String>::new();
        for field in &self.fields {
            let Some(env) = &field.env else {
                continue;
            };
            if env.is_empty() {
                return Err(ConfigError::MetadataInvalid {
                    path: field.path.clone(),
                    message: "explicit environment variable names cannot be empty".to_owned(),
                });
            }
            validate_metadata_path(&field.path)?;
            if field.path.is_empty() {
                return Err(ConfigError::MetadataInvalid {
                    path: field.path.clone(),
                    message: "explicit environment variable names cannot target the root path"
                        .to_owned(),
                });
            }
            if field.path.split('.').any(|segment| segment == "*") {
                return Err(ConfigError::MetadataInvalid {
                    path: field.path.clone(),
                    message: "explicit environment variable names cannot target wildcard paths"
                        .to_owned(),
                });
            }
            let canonical = canonicalize_path_with_aliases(&field.path, &aliases);
            if let Some(first_env) = canonical_targets.insert(canonical.clone(), env.clone())
                && first_env != *env
            {
                return Err(ConfigError::MetadataConflict {
                    kind: "environment override target",
                    name: canonical,
                    first_path: first_env,
                    second_path: env.clone(),
                });
            }
            if let Some(first_path) = envs.insert(env.clone(), field.path.clone())
                && first_path != field.path
            {
                return Err(ConfigError::MetadataConflict {
                    kind: "environment variable",
                    name: env.clone(),
                    first_path,
                    second_path: field.path.clone(),
                });
            }
        }
        Ok(envs)
    }

    /// Returns explicit path aliases keyed by alias path.
    pub fn alias_overrides(&self) -> Result<BTreeMap<String, String>, ConfigError> {
        let mut aliases = BTreeMap::<String, String>::new();
        let canonical_paths = self
            .fields
            .iter()
            .map(|field| field.path.clone())
            .collect::<BTreeSet<_>>();

        for field in &self.fields {
            validate_metadata_path(&field.path)?;
            for alias in &field.aliases {
                validate_metadata_path(alias)?;
                if alias.is_empty() {
                    return Err(ConfigError::MetadataInvalid {
                        path: alias.clone(),
                        message: "aliases cannot target the root path".to_owned(),
                    });
                }
                if field.path.is_empty() {
                    return Err(ConfigError::MetadataInvalid {
                        path: alias.clone(),
                        message: "aliases cannot rewrite the root path".to_owned(),
                    });
                }
                if !alias_mapping_is_lossless(alias, &field.path) {
                    return Err(ConfigError::MetadataInvalid {
                        path: alias.clone(),
                        message: format!(
                            "alias `{alias}` must preserve wildcard positions and cannot be deeper than canonical path `{}`",
                            field.path
                        ),
                    });
                }
                if canonical_paths.contains(alias) && alias != &field.path {
                    return Err(ConfigError::MetadataConflict {
                        kind: "alias",
                        name: alias.clone(),
                        first_path: alias.clone(),
                        second_path: field.path.clone(),
                    });
                }
                if let Some(first_path) = aliases.get(alias)
                    && first_path != &field.path
                {
                    return Err(ConfigError::MetadataConflict {
                        kind: "alias",
                        name: alias.clone(),
                        first_path: first_path.clone(),
                        second_path: field.path.clone(),
                    });
                }
                if let Some((other_alias, sample_path)) =
                    aliases.iter().find_map(|(other_alias, other_canonical)| {
                        alias_patterns_are_ambiguous(
                            alias,
                            &field.path,
                            other_alias,
                            other_canonical,
                        )
                        .then(|| {
                            (
                                other_alias.clone(),
                                alias_overlap_sample_path(alias, other_alias),
                            )
                        })
                    })
                {
                    return Err(ConfigError::MetadataInvalid {
                        path: alias.clone(),
                        message: format!(
                            "alias `{alias}` overlaps ambiguously with `{other_alias}` for concrete path `{sample_path}`"
                        ),
                    });
                }
                aliases.insert(alias.clone(), field.path.clone());
            }
        }
        Ok(aliases)
    }

    /// Returns explicitly declared field merge strategies keyed by normalized path.
    #[must_use]
    pub fn merge_strategies(&self) -> BTreeMap<String, MergeStrategy> {
        self.fields
            .iter()
            .filter(|field| field.merge_explicit)
            .map(|field| (field.path.clone(), field.merge))
            .collect()
    }

    /// Resolves the effective merge strategy for a concrete configuration path.
    #[must_use]
    pub fn merge_strategy_for(&self, path: &str) -> Option<MergeStrategy> {
        self.effective_field_for(path).map(|field| field.merge)
    }

    pub(crate) fn validate_paths(&self) -> Result<(), ConfigError> {
        let _ = self.env_overrides()?;

        for field in &self.fields {
            validate_metadata_path(&field.path)?;
            if field.path.is_empty() && !field.aliases.is_empty() {
                let alias = field.aliases.first().cloned().unwrap_or_default();
                return Err(ConfigError::MetadataInvalid {
                    path: alias,
                    message: "aliases cannot rewrite the root path".to_owned(),
                });
            }
            if field.path.is_empty() && field.merge_explicit {
                return Err(ConfigError::MetadataInvalid {
                    path: field.path.clone(),
                    message: "merge strategies cannot target the root path".to_owned(),
                });
            }
            if field.path.is_empty() && field.allowed_sources.is_some() {
                return Err(ConfigError::MetadataInvalid {
                    path: field.path.clone(),
                    message: "source policies cannot target the root path".to_owned(),
                });
            }
            if field.path.is_empty() && field.denied_sources.is_some() {
                return Err(ConfigError::MetadataInvalid {
                    path: field.path.clone(),
                    message: "source policies cannot target the root path".to_owned(),
                });
            }
            if let Some(allowed_sources) = &field.allowed_sources
                && allowed_sources.is_empty()
            {
                return Err(ConfigError::MetadataInvalid {
                    path: field.path.clone(),
                    message: "source policies must allow at least one source kind".to_owned(),
                });
            }
            if let Some(denied_sources) = &field.denied_sources
                && let Some(allowed_sources) = &field.allowed_sources
            {
                let overlap = allowed_sources
                    .intersection(denied_sources)
                    .copied()
                    .collect::<Vec<_>>();
                if !overlap.is_empty() {
                    return Err(ConfigError::MetadataInvalid {
                        path: field.path.clone(),
                        message: format!(
                            "source policies cannot both allow and deny the same source kinds: {}",
                            overlap
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    });
                }
            }
            if field.path.is_empty() && !field.validations.is_empty() {
                return Err(ConfigError::MetadataInvalid {
                    path: field.path.clone(),
                    message: "validation rules cannot target the root path".to_owned(),
                });
            }
            if field.path.is_empty() && !field.validation_configs.is_empty() {
                return Err(ConfigError::MetadataInvalid {
                    path: field.path.clone(),
                    message: "validation rules cannot target the root path".to_owned(),
                });
            }
            if field.path.is_empty() && field.secret {
                return Err(ConfigError::MetadataInvalid {
                    path: field.path.clone(),
                    message: "secret metadata cannot target the root path".to_owned(),
                });
            }
            let effective_rule_codes = self
                .effective_field_for(&field.path)
                .map(|field| {
                    field
                        .validations
                        .iter()
                        .map(ValidationRule::code)
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default();
            for rule_code in field.validation_configs.keys() {
                if !effective_rule_codes.contains(rule_code.as_str()) {
                    return Err(ConfigError::MetadataInvalid {
                        path: field.path.clone(),
                        message: format!(
                            "validation config references unknown rule `{rule_code}` for this field"
                        ),
                    });
                }
            }
            if field.path.is_empty() && field.deprecated.is_some() {
                return Err(ConfigError::MetadataInvalid {
                    path: field.path.clone(),
                    message: "deprecation metadata cannot target the root path".to_owned(),
                });
            }
            if field.env_decode.is_some() && field.path.is_empty() {
                return Err(ConfigError::MetadataInvalid {
                    path: field.path.clone(),
                    message: "environment decoder paths cannot target the root path".to_owned(),
                });
            }
            for alias in &field.aliases {
                validate_metadata_path(alias)?;
                if alias.is_empty() {
                    return Err(ConfigError::MetadataInvalid {
                        path: alias.clone(),
                        message: "aliases cannot target the root path".to_owned(),
                    });
                }
            }
        }

        for check in &self.checks {
            match check {
                ValidationCheck::AtLeastOneOf { paths }
                | ValidationCheck::ExactlyOneOf { paths }
                | ValidationCheck::MutuallyExclusive { paths } => {
                    for path in paths {
                        validate_check_path(path)?;
                    }
                }
                ValidationCheck::RequiredWith { path, requires }
                | ValidationCheck::RequiredIf { path, requires, .. } => {
                    validate_check_path(path)?;
                    for required in requires {
                        validate_check_path(required)?;
                    }
                }
            }
        }

        Ok(())
    }

    pub(super) fn normalize(&mut self) {
        let mut merged = BTreeMap::<String, FieldMetadata>::new();
        for mut field in self.fields.drain(..) {
            field.path = normalize_metadata_path(&field.path);
            field.aliases = field
                .aliases
                .into_iter()
                .map(|alias| normalize_metadata_path(&alias))
                .filter(|alias| alias != &field.path)
                .collect();
            field.aliases.sort();
            field.aliases.dedup();
            match merged.get_mut(&field.path) {
                Some(existing) => existing.merge_from(field),
                None => {
                    merged.insert(field.path.clone(), field);
                }
            }
        }
        self.fields = merged.into_values().collect();
        self.checks = normalize_checks(self.checks.drain(..));
    }
}
