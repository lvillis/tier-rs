use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::{self, Display, Formatter};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::ConfigError;
use crate::loader::SourceKind;
use crate::report::{canonicalize_path_with_aliases, normalize_path, path_matches_pattern};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// Structured metadata describing configuration fields.
///
/// `ConfigMetadata` is the manual metadata API behind `tier`'s higher-level
/// derive support. It can describe:
///
/// - field-level behavior such as env names, aliases, secret paths, examples,
///   merge policies, and declared validation rules
/// - cross-field validation checks such as mutually exclusive or required-if
///   relationships
///
/// # Examples
///
/// ```
/// use tier::{ConfigMetadata, FieldMetadata};
///
/// let metadata = ConfigMetadata::from_fields([
///     FieldMetadata::new("db.url").env("DATABASE_URL"),
///     FieldMetadata::new("db.password").secret(),
/// ])
/// .required_with("tls.enabled", ["tls.cert", "tls.key"]);
///
/// assert_eq!(
///     metadata
///         .env_overrides()
///         .expect("valid metadata")
///         .get("DATABASE_URL")
///         .map(String::as_str),
///     Some("db.url")
/// );
/// assert_eq!(metadata.secret_paths(), vec!["db.password".to_owned()]);
/// assert_eq!(metadata.checks().len(), 1);
/// ```
pub struct ConfigMetadata {
    fields: Vec<FieldMetadata>,
    checks: Vec<ValidationCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MetadataMatchScore {
    segment_count: usize,
    specificity: usize,
    positional_specificity: Vec<bool>,
}

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

    /// Returns explicit environment variable name overrides keyed by env name.
    pub fn env_overrides(&self) -> Result<BTreeMap<String, String>, ConfigError> {
        let mut envs = BTreeMap::new();
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

    /// Returns field merge strategies keyed by normalized path.
    #[must_use]
    pub fn merge_strategies(&self) -> BTreeMap<String, MergeStrategy> {
        self.fields
            .iter()
            .map(|field| (field.path.clone(), field.merge))
            .collect()
    }

    /// Resolves the effective merge strategy for a concrete configuration path.
    #[must_use]
    pub fn merge_strategy_for(&self, path: &str) -> Option<MergeStrategy> {
        let normalized = try_normalize_metadata_path(path).ok()?;
        if normalized.is_empty() {
            return None;
        }

        let mut best = None::<(MetadataMatchScore, MergeStrategy)>;
        for field in &self.fields {
            for candidate in
                std::iter::once(field.path.as_str()).chain(field.aliases.iter().map(String::as_str))
            {
                let Some(score) = metadata_match_score(&normalized, candidate) else {
                    continue;
                };

                match &mut best {
                    Some((best_score, best_merge)) if score > *best_score => {
                        *best_score = score;
                        *best_merge = field.merge;
                    }
                    None => best = Some((score, field.merge)),
                    _ => {}
                }
            }
        }

        best.map(|(_, merge)| merge)
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
            if field.path.is_empty() && field.merge != MergeStrategy::Merge {
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
            let declared_rule_codes = field
                .validations
                .iter()
                .map(ValidationRule::code)
                .collect::<BTreeSet<_>>();
            for rule_code in field.validation_configs.keys() {
                if !declared_rule_codes.contains(rule_code.as_str()) {
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

    fn normalize(&mut self) {
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// Metadata for a single configuration path.
pub struct FieldMetadata {
    /// Dot-delimited configuration path.
    pub path: String,
    /// Alternate dot-delimited paths accepted by serde during deserialization.
    pub aliases: Vec<String>,
    /// Whether values at this path should be treated as sensitive.
    pub secret: bool,
    /// Exact environment variable name to map to this path.
    pub env: Option<String>,
    /// Decoder applied to environment variable values before deserialization.
    pub env_decode: Option<EnvDecoder>,
    /// Human-readable field documentation.
    pub doc: Option<String>,
    /// Example value rendered in generated docs.
    pub example: Option<String>,
    /// Deprecation note shown in generated docs and runtime warnings.
    pub deprecated: Option<String>,
    /// Whether the field accepts omission via `serde(default)`.
    pub has_default: bool,
    /// Strategy used when merging layered values into this field.
    pub merge: MergeStrategy,
    /// Source kinds allowed to override this field.
    ///
    /// When unset, the field accepts values from any source kind.
    pub allowed_sources: Option<BTreeSet<SourceKind>>,
    /// Source kinds explicitly denied from overriding this field.
    ///
    /// When unset, the field does not deny any source kinds.
    pub denied_sources: Option<BTreeSet<SourceKind>>,
    /// Declarative validation rules applied after normalization.
    pub validations: Vec<ValidationRule>,
    /// Per-rule configuration such as custom messages, warning levels, and tags.
    pub validation_configs: BTreeMap<String, ValidationRuleConfig>,
}

impl FieldMetadata {
    /// Creates metadata for a single configuration path.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: normalize_metadata_path(&path.into()),
            aliases: Vec::new(),
            secret: false,
            env: None,
            env_decode: None,
            doc: None,
            example: None,
            deprecated: None,
            has_default: false,
            merge: MergeStrategy::Merge,
            allowed_sources: None,
            denied_sources: None,
            validations: Vec::new(),
            validation_configs: BTreeMap::new(),
        }
    }

    /// Adds an alternate serde path for this field.
    #[must_use]
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Marks this path as sensitive.
    #[must_use]
    pub fn secret(mut self) -> Self {
        self.secret = true;
        self
    }

    /// Overrides the environment variable name for this path.
    #[must_use]
    pub fn env(mut self, env: impl Into<String>) -> Self {
        self.env = Some(env.into());
        self
    }

    /// Decodes environment variables for this path with a built-in decoder.
    ///
    /// This can be used together with [`ConfigMetadata`] when metadata is
    /// built manually instead of derived.
    #[must_use]
    pub fn env_decoder(mut self, decoder: EnvDecoder) -> Self {
        self.env_decode = Some(decoder);
        self
    }

    /// Adds human-readable field documentation.
    #[must_use]
    pub fn doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = Some(doc.into());
        self
    }

    /// Adds an example value used by generated docs.
    #[must_use]
    pub fn example(mut self, example: impl Into<String>) -> Self {
        self.example = Some(example.into());
        self
    }

    /// Marks the field as deprecated with an optional note.
    #[must_use]
    pub fn deprecated(mut self, note: impl Into<String>) -> Self {
        self.deprecated = Some(note.into());
        self
    }

    /// Marks the field as accepting omission via `serde(default)`.
    #[must_use]
    pub fn defaulted(mut self) -> Self {
        self.has_default = true;
        self
    }

    /// Sets the field-level merge strategy.
    #[must_use]
    pub fn merge_strategy(mut self, merge: MergeStrategy) -> Self {
        self.merge = merge;
        self
    }

    /// Restricts the field to a specific set of source kinds.
    ///
    /// This is useful for fields that should only be loaded from selected
    /// layers, such as requiring secrets to come from environment variables or
    /// disallowing file-based overrides for a path.
    #[must_use]
    pub fn allow_sources<I>(mut self, sources: I) -> Self
    where
        I: IntoIterator<Item = SourceKind>,
    {
        self.allowed_sources = Some(sources.into_iter().collect());
        self
    }

    /// Explicitly denies a set of source kinds from overriding this field.
    #[must_use]
    pub fn deny_sources<I>(mut self, sources: I) -> Self
    where
        I: IntoIterator<Item = SourceKind>,
    {
        self.denied_sources = Some(sources.into_iter().collect());
        self
    }

    /// Appends a declarative validation rule.
    #[must_use]
    pub fn validate(mut self, rule: ValidationRule) -> Self {
        self.validations.push(rule);
        self
    }

    /// Overrides the human-readable message for a validation rule.
    #[must_use]
    pub fn validation_message(
        mut self,
        rule_code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.validation_configs
            .entry(rule_code.into())
            .or_default()
            .message = Some(message.into());
        self
    }

    /// Sets the runtime level for a validation rule.
    #[must_use]
    pub fn validation_level(
        mut self,
        rule_code: impl Into<String>,
        level: ValidationLevel,
    ) -> Self {
        self.validation_configs
            .entry(rule_code.into())
            .or_default()
            .level = level;
        self
    }

    /// Attaches machine-readable tags to a validation rule.
    #[must_use]
    pub fn validation_tags<I, S>(mut self, rule_code: impl Into<String>, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.validation_configs
            .entry(rule_code.into())
            .or_default()
            .tags = tags.into_iter().map(Into::into).collect();
        self
    }

    /// Requires the field to be non-empty.
    #[must_use]
    pub fn non_empty(self) -> Self {
        self.validate(ValidationRule::NonEmpty)
    }

    /// Requires the field to be greater than or equal to `min`.
    #[must_use]
    pub fn min(self, min: impl Into<ValidationNumber>) -> Self {
        self.validate(ValidationRule::Min(min.into()))
    }

    /// Requires the field to be less than or equal to `max`.
    #[must_use]
    pub fn max(self, max: impl Into<ValidationNumber>) -> Self {
        self.validate(ValidationRule::Max(max.into()))
    }

    /// Requires the field length to be greater than or equal to `min`.
    #[must_use]
    pub fn min_length(self, min: usize) -> Self {
        self.validate(ValidationRule::MinLength(min))
    }

    /// Requires the field length to be less than or equal to `max`.
    #[must_use]
    pub fn max_length(self, max: usize) -> Self {
        self.validate(ValidationRule::MaxLength(max))
    }

    /// Requires the field to be an array with at least `min` items.
    #[must_use]
    pub fn min_items(self, min: usize) -> Self {
        self.validate(ValidationRule::MinItems(min))
    }

    /// Requires the field to be an array with at most `max` items.
    #[must_use]
    pub fn max_items(self, max: usize) -> Self {
        self.validate(ValidationRule::MaxItems(max))
    }

    /// Requires the field to be an object with at least `min` properties.
    #[must_use]
    pub fn min_properties(self, min: usize) -> Self {
        self.validate(ValidationRule::MinProperties(min))
    }

    /// Requires the field to be an object with at most `max` properties.
    #[must_use]
    pub fn max_properties(self, max: usize) -> Self {
        self.validate(ValidationRule::MaxProperties(max))
    }

    /// Requires the field to be an exact multiple of `factor`.
    #[must_use]
    pub fn multiple_of(self, factor: impl Into<ValidationNumber>) -> Self {
        self.validate(ValidationRule::MultipleOf(factor.into()))
    }

    /// Requires the field to match a regular expression.
    #[must_use]
    pub fn pattern(self, pattern: impl Into<String>) -> Self {
        self.validate(ValidationRule::Pattern(pattern.into()))
    }

    /// Requires the field to be an array with unique items.
    #[must_use]
    pub fn unique_items(self) -> Self {
        self.validate(ValidationRule::UniqueItems)
    }

    /// Requires the field to match one of the provided scalar values.
    #[must_use]
    pub fn one_of<I, V>(self, values: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<ValidationValue>,
    {
        self.validate(ValidationRule::OneOf(
            values.into_iter().map(Into::into).collect(),
        ))
    }

    /// Requires the field to be a valid hostname.
    #[must_use]
    pub fn hostname(self) -> Self {
        self.validate(ValidationRule::Hostname)
    }

    /// Requires the field to be a valid absolute URL string.
    #[must_use]
    pub fn url(self) -> Self {
        self.validate(ValidationRule::Url)
    }

    /// Requires the field to be a valid email address.
    #[must_use]
    pub fn email(self) -> Self {
        self.validate(ValidationRule::Email)
    }

    /// Requires the field to be a valid IP address.
    #[must_use]
    pub fn ip_addr(self) -> Self {
        self.validate(ValidationRule::IpAddr)
    }

    /// Requires the field to be a valid socket address.
    #[must_use]
    pub fn socket_addr(self) -> Self {
        self.validate(ValidationRule::SocketAddr)
    }

    /// Requires the field to be an absolute filesystem path.
    #[must_use]
    pub fn absolute_path(self) -> Self {
        self.validate(ValidationRule::AbsolutePath)
    }

    fn merge_from(&mut self, other: Self) {
        self.aliases.extend(other.aliases);
        self.aliases.sort();
        self.aliases.dedup();
        self.secret |= other.secret;
        if let Some(env) = other.env {
            self.env = Some(env);
        }
        if let Some(env_decode) = other.env_decode {
            self.env_decode = Some(env_decode);
        }
        if let Some(doc) = other.doc {
            self.doc = Some(doc);
        }
        if let Some(example) = other.example {
            self.example = Some(example);
        }
        if let Some(deprecated) = other.deprecated {
            self.deprecated = Some(deprecated);
        }
        self.has_default |= other.has_default;
        if other.merge != MergeStrategy::Merge || self.merge == MergeStrategy::Merge {
            self.merge = other.merge;
        }
        if let Some(allowed_sources) = other.allowed_sources {
            self.allowed_sources = Some(allowed_sources);
        }
        if let Some(denied_sources) = other.denied_sources {
            self.denied_sources = Some(denied_sources);
        }
        for rule in other.validations {
            if !self.validations.contains(&rule) {
                self.validations.push(rule);
            }
        }
        for (rule_code, config) in other.validation_configs {
            self.validation_configs.insert(rule_code, config);
        }
    }

    fn is_env_decoder_only(&self) -> bool {
        self.env_decode.is_some()
            && self.aliases.is_empty()
            && !self.secret
            && self.env.is_none()
            && self.doc.is_none()
            && self.example.is_none()
            && self.deprecated.is_none()
            && !self.has_default
            && self.merge == MergeStrategy::Merge
            && self.allowed_sources.is_none()
            && self.denied_sources.is_none()
            && self.validations.is_empty()
            && self.validation_configs.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Built-in decoders for structured environment variable values.
///
/// These decoders are intended for operational formats that are common in
/// deployments but inconvenient to express as JSON.
///
/// # Examples
///
/// ```
/// use tier::{ConfigMetadata, EnvDecoder, FieldMetadata};
///
/// let mut metadata = ConfigMetadata::new();
/// metadata.push(FieldMetadata::new("no_proxy").env_decoder(EnvDecoder::Csv));
/// metadata.push(FieldMetadata::new("labels").env_decoder(EnvDecoder::KeyValueMap));
///
/// assert_eq!(metadata.fields().len(), 2);
/// ```
pub enum EnvDecoder {
    /// Comma-separated values such as `a,b,c`.
    Csv,
    /// Platform-native path list syntax such as `PATH`.
    PathList,
    /// Comma-separated `key=value` pairs such as `a=1,b=2`.
    KeyValueMap,
    /// Whitespace-separated values such as `a b c`.
    Whitespace,
}

impl Display for EnvDecoder {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Csv => write!(f, "csv"),
            Self::PathList => write!(f, "path_list"),
            Self::KeyValueMap => write!(f, "key_value_map"),
            Self::Whitespace => write!(f, "whitespace"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Strategy applied when multiple layers write to the same configuration path.
pub enum MergeStrategy {
    /// Recursively merge objects and replace non-object values.
    #[default]
    Merge,
    /// Replace the current value at this path with the overlay value.
    Replace,
    /// Append array overlays while still recursively merging nested objects.
    Append,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
/// Runtime severity applied to a declarative validation rule.
pub enum ValidationLevel {
    /// Reject the configuration when the rule fails.
    #[default]
    Error,
    /// Record a warning and continue loading when the rule fails.
    Warning,
}

impl Display for ValidationLevel {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error => write!(f, "error"),
            Self::Warning => write!(f, "warning"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Additional configuration attached to a declarative validation rule.
pub struct ValidationRuleConfig {
    /// Runtime severity for the rule.
    pub level: ValidationLevel,
    /// Optional custom message shown when the rule fails.
    pub message: Option<String>,
    /// Optional machine-readable tags for downstream consumers.
    pub tags: Vec<String>,
}

impl Display for MergeStrategy {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Merge => write!(f, "merge"),
            Self::Replace => write!(f, "replace"),
            Self::Append => write!(f, "append"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
/// Numeric bound used by declarative validation rules.
pub enum ValidationNumber {
    /// A finite JSON-compatible number.
    Finite(serde_json::Number),
    /// An invalid non-finite value such as `NaN` or `inf`.
    Invalid(String),
}

impl ValidationNumber {
    /// Returns the numeric value as `f64`, when representable.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Finite(number) => number.as_f64(),
            Self::Invalid(_) => None,
        }
    }

    /// Returns `true` when the bound is a finite JSON-compatible number.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        matches!(self, Self::Finite(_))
    }

    /// Returns the bound rendered as a JSON value.
    #[must_use]
    pub fn as_json_value(&self) -> serde_json::Value {
        match self {
            Self::Finite(number) => serde_json::Value::Number(number.clone()),
            Self::Invalid(value) => serde_json::Value::String(value.clone()),
        }
    }
}

impl Display for ValidationNumber {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Finite(number) => Display::fmt(number, f),
            Self::Invalid(value) => Display::fmt(value, f),
        }
    }
}

impl From<i8> for ValidationNumber {
    fn from(value: i8) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<i16> for ValidationNumber {
    fn from(value: i16) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<i32> for ValidationNumber {
    fn from(value: i32) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<i64> for ValidationNumber {
    fn from(value: i64) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<isize> for ValidationNumber {
    fn from(value: isize) -> Self {
        Self::Finite(serde_json::Number::from(value as i64))
    }
}

impl From<u8> for ValidationNumber {
    fn from(value: u8) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<u16> for ValidationNumber {
    fn from(value: u16) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<u32> for ValidationNumber {
    fn from(value: u32) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<u64> for ValidationNumber {
    fn from(value: u64) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<usize> for ValidationNumber {
    fn from(value: usize) -> Self {
        Self::Finite(serde_json::Number::from(value as u64))
    }
}

impl From<f32> for ValidationNumber {
    fn from(value: f32) -> Self {
        match serde_json::Number::from_f64(value as f64) {
            Some(number) => Self::Finite(number),
            None => Self::Invalid(value.to_string()),
        }
    }
}

impl From<f64> for ValidationNumber {
    fn from(value: f64) -> Self {
        match serde_json::Number::from_f64(value) {
            Some(number) => Self::Finite(number),
            None => Self::Invalid(value.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
/// Scalar value used by declarative validation rules and conditions.
pub struct ValidationValue(pub serde_json::Value);

impl Display for ValidationValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.0 {
            serde_json::Value::String(value) => write!(f, "{value:?}"),
            value => Display::fmt(value, f),
        }
    }
}

impl From<bool> for ValidationValue {
    fn from(value: bool) -> Self {
        Self(serde_json::Value::Bool(value))
    }
}

impl From<String> for ValidationValue {
    fn from(value: String) -> Self {
        Self(serde_json::Value::String(value))
    }
}

impl From<&str> for ValidationValue {
    fn from(value: &str) -> Self {
        Self(serde_json::Value::String(value.to_owned()))
    }
}

impl From<f32> for ValidationValue {
    fn from(value: f32) -> Self {
        match serde_json::Number::from_f64(value as f64) {
            Some(number) => Self(serde_json::Value::Number(number)),
            None => Self(serde_json::Value::String(value.to_string())),
        }
    }
}

impl From<f64> for ValidationValue {
    fn from(value: f64) -> Self {
        match serde_json::Number::from_f64(value) {
            Some(number) => Self(serde_json::Value::Number(number)),
            None => Self(serde_json::Value::String(value.to_string())),
        }
    }
}

macro_rules! impl_validation_value_from_number {
    ($($ty:ty),* $(,)?) => {
        $(
            impl From<$ty> for ValidationValue {
                fn from(value: $ty) -> Self {
                    Self(serde_json::to_value(value).expect("validation values must serialize"))
                }
            }
        )*
    };
}

impl_validation_value_from_number!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Declarative validation rule applied to a single configuration path.
pub enum ValidationRule {
    /// The field must not be empty.
    NonEmpty,
    /// The field must be greater than or equal to the given numeric bound.
    Min(ValidationNumber),
    /// The field must be less than or equal to the given numeric bound.
    Max(ValidationNumber),
    /// The field length must be at least the given number of units.
    MinLength(usize),
    /// The field length must be at most the given number of units.
    MaxLength(usize),
    /// The field must be an array with at least the given number of items.
    MinItems(usize),
    /// The field must be an array with at most the given number of items.
    MaxItems(usize),
    /// The field must be an object with at least the given number of properties.
    MinProperties(usize),
    /// The field must be an object with at most the given number of properties.
    MaxProperties(usize),
    /// The field must be a numeric multiple of the given factor.
    MultipleOf(ValidationNumber),
    /// The field must match the given regular expression.
    Pattern(String),
    /// The field must be an array whose items are unique.
    UniqueItems,
    /// The field must equal one of the provided scalar values.
    OneOf(Vec<ValidationValue>),
    /// The field must be a valid hostname.
    Hostname,
    /// The field must be a valid absolute URL string.
    Url,
    /// The field must be a valid email address.
    Email,
    /// The field must be a valid IP address.
    IpAddr,
    /// The field must be a valid socket address.
    SocketAddr,
    /// The field must be an absolute filesystem path.
    AbsolutePath,
}

impl ValidationRule {
    /// Returns a stable machine-readable rule identifier.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NonEmpty => "non_empty",
            Self::Min(_) => "min",
            Self::Max(_) => "max",
            Self::MinLength(_) => "min_length",
            Self::MaxLength(_) => "max_length",
            Self::MinItems(_) => "min_items",
            Self::MaxItems(_) => "max_items",
            Self::MinProperties(_) => "min_properties",
            Self::MaxProperties(_) => "max_properties",
            Self::MultipleOf(_) => "multiple_of",
            Self::Pattern(_) => "pattern",
            Self::UniqueItems => "unique_items",
            Self::OneOf(_) => "one_of",
            Self::Hostname => "hostname",
            Self::Url => "url",
            Self::Email => "email",
            Self::IpAddr => "ip_addr",
            Self::SocketAddr => "socket_addr",
            Self::AbsolutePath => "absolute_path",
        }
    }
}

impl Display for ValidationRule {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonEmpty => write!(f, "non_empty"),
            Self::Min(value) => write!(f, "min={value}"),
            Self::Max(value) => write!(f, "max={value}"),
            Self::MinLength(value) => write!(f, "min_length={value}"),
            Self::MaxLength(value) => write!(f, "max_length={value}"),
            Self::MinItems(value) => write!(f, "min_items={value}"),
            Self::MaxItems(value) => write!(f, "max_items={value}"),
            Self::MinProperties(value) => write!(f, "min_properties={value}"),
            Self::MaxProperties(value) => write!(f, "max_properties={value}"),
            Self::MultipleOf(value) => write!(f, "multiple_of={value}"),
            Self::Pattern(value) => write!(f, "pattern={value:?}"),
            Self::UniqueItems => write!(f, "unique_items"),
            Self::OneOf(values) => write!(
                f,
                "one_of=[{}]",
                values
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Hostname => write!(f, "hostname"),
            Self::Url => write!(f, "url"),
            Self::Email => write!(f, "email"),
            Self::IpAddr => write!(f, "ip_addr"),
            Self::SocketAddr => write!(f, "socket_addr"),
            Self::AbsolutePath => write!(f, "absolute_path"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Cross-field declarative validation applied to the final normalized configuration.
pub enum ValidationCheck {
    /// Requires that at least one of the given paths is configured.
    AtLeastOneOf { paths: Vec<String> },
    /// Requires that exactly one of the given paths is configured.
    ExactlyOneOf { paths: Vec<String> },
    /// Requires that no more than one of the given paths is configured.
    MutuallyExclusive { paths: Vec<String> },
    /// Requires one or more paths whenever `path` is configured.
    RequiredWith { path: String, requires: Vec<String> },
    /// Requires one or more paths whenever `path` equals `equals`.
    RequiredIf {
        /// Path whose value is inspected.
        path: String,
        /// Value that triggers the requirement.
        equals: ValidationValue,
        /// Paths that must be configured when the condition matches.
        requires: Vec<String>,
    },
}

impl ValidationCheck {
    /// Returns a stable machine-readable rule identifier.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::AtLeastOneOf { .. } => "at_least_one_of",
            Self::ExactlyOneOf { .. } => "exactly_one_of",
            Self::MutuallyExclusive { .. } => "mutually_exclusive",
            Self::RequiredWith { .. } => "required_with",
            Self::RequiredIf { .. } => "required_if",
        }
    }

    fn normalize(self) -> Option<Self> {
        match self {
            Self::AtLeastOneOf { paths } => {
                normalize_check_path_group(paths).map(|paths| Self::AtLeastOneOf { paths })
            }
            Self::ExactlyOneOf { paths } => {
                normalize_check_path_group(paths).map(|paths| Self::ExactlyOneOf { paths })
            }
            Self::MutuallyExclusive { paths } => {
                normalize_check_path_group(paths).map(|paths| Self::MutuallyExclusive { paths })
            }
            Self::RequiredWith { path, requires } => {
                let path = normalize_metadata_path(&path);
                let requires = normalize_check_path_group(requires)?;
                Some(Self::RequiredWith { path, requires })
            }
            Self::RequiredIf {
                path,
                equals,
                requires,
            } => {
                let path = normalize_metadata_path(&path);
                let requires = normalize_check_path_group(requires)?;
                Some(Self::RequiredIf {
                    path,
                    equals,
                    requires,
                })
            }
        }
    }

    fn prefixed(self, prefix: &str) -> Option<Self> {
        let prefix = if prefix.is_empty() {
            String::new()
        } else {
            try_normalize_metadata_path(prefix)
                .ok()
                .filter(|normalized| !normalized.is_empty())
                .unwrap_or_else(|| prefix.to_owned())
        };
        if prefix.is_empty() {
            return self.normalize();
        }

        let join = |path: String| {
            if path.is_empty() {
                prefix.clone()
            } else {
                format!("{prefix}.{path}")
            }
        };

        match self {
            Self::AtLeastOneOf { paths } => Some(Self::AtLeastOneOf {
                paths: paths.into_iter().map(join).collect(),
            })
            .and_then(Self::normalize),
            Self::ExactlyOneOf { paths } => Some(Self::ExactlyOneOf {
                paths: paths.into_iter().map(join).collect(),
            })
            .and_then(Self::normalize),
            Self::MutuallyExclusive { paths } => Some(Self::MutuallyExclusive {
                paths: paths.into_iter().map(join).collect(),
            })
            .and_then(Self::normalize),
            Self::RequiredWith { path, requires } => Some(Self::RequiredWith {
                path: join(path),
                requires: requires.into_iter().map(join).collect(),
            })
            .and_then(Self::normalize),
            Self::RequiredIf {
                path,
                equals,
                requires,
            } => Some(Self::RequiredIf {
                path: join(path),
                equals,
                requires: requires.into_iter().map(join).collect(),
            })
            .and_then(Self::normalize),
        }
    }
}

impl Display for ValidationCheck {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AtLeastOneOf { paths } => {
                write!(f, "at_least_one_of({})", paths.join(", "))
            }
            Self::ExactlyOneOf { paths } => {
                write!(f, "exactly_one_of({})", paths.join(", "))
            }
            Self::MutuallyExclusive { paths } => {
                write!(f, "mutually_exclusive({})", paths.join(", "))
            }
            Self::RequiredWith { path, requires } => {
                write!(f, "required_with({path} -> {})", requires.join(", "))
            }
            Self::RequiredIf {
                path,
                equals,
                requires,
            } => write!(
                f,
                "required_if({path} == {equals} -> {})",
                requires.join(", ")
            ),
        }
    }
}

/// Metadata produced for a configuration type.
pub trait TierMetadata {
    /// Returns metadata for the configuration type.
    #[must_use]
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::default()
    }

    /// Returns configuration paths that should be treated as secrets.
    #[must_use]
    fn secret_paths() -> Vec<String> {
        Self::metadata().secret_paths()
    }
}

impl<T> TierMetadata for super::Secret<T> {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("").secret()])
    }
}
impl TierMetadata for String {}
impl TierMetadata for bool {}
impl TierMetadata for char {}
impl TierMetadata for u8 {}
impl TierMetadata for u16 {}
impl TierMetadata for u32 {}
impl TierMetadata for u64 {}
impl TierMetadata for u128 {}
impl TierMetadata for usize {}
impl TierMetadata for i8 {}
impl TierMetadata for i16 {}
impl TierMetadata for i32 {}
impl TierMetadata for i64 {}
impl TierMetadata for i128 {}
impl TierMetadata for isize {}
impl TierMetadata for f32 {}
impl TierMetadata for f64 {}
impl TierMetadata for Duration {}
impl TierMetadata for SystemTime {}
impl TierMetadata for PathBuf {}
impl TierMetadata for IpAddr {}
impl TierMetadata for Ipv4Addr {}
impl TierMetadata for Ipv6Addr {}
impl TierMetadata for SocketAddr {}
impl TierMetadata for SocketAddrV4 {}
impl TierMetadata for SocketAddrV6 {}

impl<T> TierMetadata for Option<T>
where
    T: TierMetadata,
{
    fn metadata() -> ConfigMetadata {
        T::metadata()
    }
}

impl<T> TierMetadata for Vec<T>
where
    T: TierMetadata,
{
    fn metadata() -> ConfigMetadata {
        prefixed_metadata("*", Vec::new(), T::metadata())
    }
}

impl<T, const N: usize> TierMetadata for [T; N]
where
    T: TierMetadata,
{
    fn metadata() -> ConfigMetadata {
        prefixed_metadata("*", Vec::new(), T::metadata())
    }
}

impl<T> TierMetadata for BTreeSet<T>
where
    T: TierMetadata,
{
    fn metadata() -> ConfigMetadata {
        prefixed_metadata("*", Vec::new(), T::metadata())
    }
}

impl<T> TierMetadata for HashSet<T>
where
    T: TierMetadata,
{
    fn metadata() -> ConfigMetadata {
        prefixed_metadata("*", Vec::new(), T::metadata())
    }
}

impl<K, V> TierMetadata for BTreeMap<K, V>
where
    V: TierMetadata,
{
    fn metadata() -> ConfigMetadata {
        prefixed_metadata("*", Vec::new(), V::metadata())
    }
}

impl<K, V, S> TierMetadata for HashMap<K, V, S>
where
    V: TierMetadata,
{
    fn metadata() -> ConfigMetadata {
        prefixed_metadata("*", Vec::new(), V::metadata())
    }
}

impl<T> TierMetadata for Box<T>
where
    T: TierMetadata,
{
    fn metadata() -> ConfigMetadata {
        T::metadata()
    }
}

impl<T> TierMetadata for Arc<T>
where
    T: TierMetadata,
{
    fn metadata() -> ConfigMetadata {
        T::metadata()
    }
}

/// Prefixes child metadata paths with a parent field name.
#[must_use]
pub fn prefixed_metadata(
    prefix: &str,
    prefix_aliases: Vec<String>,
    metadata: ConfigMetadata,
) -> ConfigMetadata {
    let prefix = if prefix.is_empty() {
        String::new()
    } else {
        try_normalize_metadata_path(prefix)
            .ok()
            .filter(|normalized| !normalized.is_empty())
            .unwrap_or_else(|| prefix.to_owned())
    };
    if prefix.is_empty() {
        return metadata;
    }
    let prefix_aliases = prefix_aliases
        .into_iter()
        .map(|alias| {
            if alias.is_empty() {
                alias
            } else {
                try_normalize_metadata_path(&alias)
                    .ok()
                    .filter(|normalized| !normalized.is_empty())
                    .unwrap_or(alias)
            }
        })
        .collect::<Vec<_>>();

    let mut prefixed = ConfigMetadata::from_fields(metadata.fields.into_iter().map(|field| {
        let canonical_suffix = field.path.clone();
        let alias_suffixes = if field.aliases.is_empty() {
            vec![canonical_suffix.clone()]
        } else {
            let mut suffixes = vec![canonical_suffix.clone()];
            suffixes.extend(field.aliases.iter().cloned());
            suffixes
        };

        let path = if canonical_suffix.is_empty() {
            prefix.clone()
        } else {
            format!("{prefix}.{}", canonical_suffix)
        };

        let mut aliases = field
            .aliases
            .into_iter()
            .map(|alias| {
                if alias.is_empty() {
                    prefix.clone()
                } else {
                    format!("{prefix}.{}", alias)
                }
            })
            .collect::<Vec<_>>();

        for prefix_alias in &prefix_aliases {
            if canonical_suffix.is_empty() {
                aliases.push(prefix_alias.clone());
                continue;
            }
            for suffix in &alias_suffixes {
                if prefix_alias.is_empty() {
                    aliases.push(suffix.clone());
                } else {
                    aliases.push(format!("{prefix_alias}.{suffix}"));
                }
            }
        }

        FieldMetadata {
            path,
            aliases,
            ..field
        }
    }));
    prefixed.extend_checks(
        metadata
            .checks
            .into_iter()
            .filter_map(|check| check.prefixed(&prefix)),
    );
    prefixed
}

impl IntoIterator for ConfigMetadata {
    type Item = FieldMetadata;
    type IntoIter = std::vec::IntoIter<FieldMetadata>;

    fn into_iter(self) -> Self::IntoIter {
        self.fields.into_iter()
    }
}

fn normalize_checks<I>(checks: I) -> Vec<ValidationCheck>
where
    I: IntoIterator<Item = ValidationCheck>,
{
    let mut normalized = Vec::new();
    for check in checks {
        let Some(check) = check.normalize() else {
            continue;
        };
        if !normalized.contains(&check) {
            normalized.push(check);
        }
    }
    normalized
}

fn normalize_check_path_group<I>(paths: I) -> Option<Vec<String>>
where
    I: IntoIterator<Item = String>,
{
    let mut normalized = Vec::new();
    for path in paths {
        let path = normalize_metadata_path(&path);
        if normalized.contains(&path) {
            continue;
        }
        normalized.push(path);
    }
    (!normalized.is_empty()).then_some(normalized)
}

fn normalize_metadata_path(path: &str) -> String {
    try_normalize_metadata_path(path).unwrap_or_else(|_| path.to_owned())
}

fn validate_metadata_path(path: &str) -> Result<(), ConfigError> {
    try_normalize_metadata_path(path)
        .map(|_| ())
        .map_err(|message| ConfigError::MetadataInvalid {
            path: path.to_owned(),
            message: format!("invalid metadata path: {message}"),
        })
}

fn validate_check_path(path: &str) -> Result<(), ConfigError> {
    validate_metadata_path(path)?;
    if normalize_metadata_path(path).is_empty() {
        return Err(ConfigError::MetadataInvalid {
            path: path.to_owned(),
            message: "invalid metadata path: cross-field checks cannot use the root path"
                .to_owned(),
        });
    }
    Ok(())
}

fn try_normalize_metadata_path(path: &str) -> Result<String, String> {
    if path.is_empty() {
        return Ok(String::new());
    }
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
                    let index = parse_metadata_array_index(&mut chars)?;
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
                let index = parse_metadata_array_index(&mut chars)?;
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

    for segment in &segments {
        if segment.contains('*') && segment != "*" {
            return Err("wildcard path segments must be exactly `*`".to_owned());
        }
    }

    Ok(segments.join("."))
}

fn parse_metadata_array_index<I>(chars: &mut std::iter::Peekable<I>) -> Result<String, String>
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

    Ok(index
        .parse::<usize>()
        .expect("checked numeric array indices")
        .to_string())
}

fn metadata_match_score(path: &str, candidate: &str) -> Option<MetadataMatchScore> {
    if candidate != path && !path_matches_pattern(path, candidate) {
        return None;
    }

    let segments = candidate
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let positional_specificity = segments
        .iter()
        .map(|segment| *segment != "*")
        .collect::<Vec<_>>();
    let specificity = positional_specificity
        .iter()
        .filter(|segment| **segment)
        .count();
    Some(MetadataMatchScore {
        segment_count: segments.len(),
        specificity,
        positional_specificity,
    })
}

fn alias_mapping_is_lossless(alias: &str, canonical: &str) -> bool {
    let alias_segments = path_segments(alias);
    let canonical_segments = path_segments(canonical);
    if canonical_segments.len() < alias_segments.len() {
        return false;
    }

    for index in 0..alias_segments.len() {
        let alias_wildcard = alias_segments[index] == "*";
        let canonical_wildcard = canonical_segments[index] == "*";
        if alias_wildcard != canonical_wildcard {
            return false;
        }
    }

    !canonical_segments[alias_segments.len()..].contains(&"*")
}

fn alias_patterns_are_ambiguous(
    left_alias: &str,
    left_canonical: &str,
    right_alias: &str,
    right_canonical: &str,
) -> bool {
    if alias_rank(left_alias) != alias_rank(right_alias) {
        return false;
    }

    let left_segments = path_segments(left_alias);
    let right_segments = path_segments(right_alias);
    if left_segments.len() != right_segments.len() {
        return false;
    }

    if !left_segments
        .iter()
        .zip(right_segments.iter())
        .all(|(left, right)| *left == "*" || *right == "*" || left == right)
    {
        return false;
    }

    let sample_path = alias_overlap_sample_path(left_alias, right_alias);
    rewrite_alias_sample(&sample_path, left_alias, left_canonical)
        != rewrite_alias_sample(&sample_path, right_alias, right_canonical)
}

fn alias_rank(alias: &str) -> (usize, usize) {
    let segments = path_segments(alias);
    let specificity = segments.iter().filter(|segment| **segment != "*").count();
    (segments.len(), specificity)
}

fn alias_overlap_sample_path(left: &str, right: &str) -> String {
    path_segments(left)
        .into_iter()
        .zip(path_segments(right))
        .map(|(left, right)| {
            if left == "*" && right == "*" {
                "item".to_owned()
            } else if left == "*" {
                right.to_owned()
            } else {
                left.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn rewrite_alias_sample(path: &str, alias: &str, canonical: &str) -> String {
    let concrete_segments = path_segments(path);
    let alias_segments = path_segments(alias);
    let canonical_segments = path_segments(canonical);

    let mut rewritten = canonical_segments
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            if *segment == "*" && alias_segments.get(index) == Some(&"*") {
                concrete_segments[index].to_owned()
            } else {
                (*segment).to_owned()
            }
        })
        .collect::<Vec<_>>();
    rewritten.extend(
        concrete_segments[alias_segments.len()..]
            .iter()
            .map(|segment| (*segment).to_owned()),
    );
    normalize_path(&rewritten.join("."))
}

fn path_segments(path: &str) -> Vec<&str> {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .collect()
}
