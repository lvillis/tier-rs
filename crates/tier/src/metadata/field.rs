use super::*;
#[cfg(feature = "schema")]
use serde_json::Value;

#[cfg(feature = "schema")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FieldValidationExport {
    pub(crate) levels: std::collections::BTreeMap<String, ValidationLevel>,
    pub(crate) messages: std::collections::BTreeMap<String, String>,
    pub(crate) tags: std::collections::BTreeMap<String, Vec<String>>,
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
            merge_explicit: false,
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
        self.merge_explicit = true;
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
        self.upsert_validation(rule);
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

    #[cfg(feature = "schema")]
    pub(crate) fn allowed_source_names(&self) -> Vec<String> {
        self.allowed_sources
            .as_ref()
            .map(|allowed_sources| allowed_sources.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .map(|source| source.to_string())
            .collect()
    }

    #[cfg(feature = "schema")]
    pub(crate) fn denied_source_names(&self) -> Vec<String> {
        self.denied_sources
            .as_ref()
            .map(|denied_sources| denied_sources.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .map(|source| source.to_string())
            .collect()
    }

    pub(crate) fn validation_config_for(
        &self,
        rule: &ValidationRule,
    ) -> Option<&ValidationRuleConfig> {
        self.validation_configs.get(rule.code())
    }

    pub(crate) fn validation_level_for(&self, rule: &ValidationRule) -> ValidationLevel {
        self.validation_config_for(rule)
            .map(|config| config.level)
            .unwrap_or(ValidationLevel::Error)
    }

    #[cfg(feature = "schema")]
    pub(crate) fn validation_export(&self) -> FieldValidationExport {
        let mut export = FieldValidationExport::default();
        for (rule_code, config) in &self.validation_configs {
            export.levels.insert(rule_code.clone(), config.level);
            if let Some(message) = &config.message {
                export.messages.insert(rule_code.clone(), message.clone());
            }
            if !config.tags.is_empty() {
                export.tags.insert(rule_code.clone(), config.tags.clone());
            }
        }
        export
    }

    #[cfg(feature = "schema")]
    pub(crate) fn validation_config_json(&self) -> Option<Value> {
        if self.validation_configs.is_empty() {
            None
        } else {
            Some(
                serde_json::to_value(&self.validation_configs)
                    .unwrap_or_else(|_| Value::Object(Default::default())),
            )
        }
    }

    pub(crate) fn decorate_validation_error(
        &self,
        rule: &ValidationRule,
        mut error: ValidationError,
    ) -> ValidationError {
        if let Some(config) = self.validation_config_for(rule) {
            if let Some(message) = &config.message {
                error.message = message.clone();
            }
            if !config.tags.is_empty() {
                error = error.with_tags(config.tags.clone());
            }
        }
        error
    }

    pub(super) fn merge_from(&mut self, other: Self) {
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
        if other.merge_explicit {
            self.merge = other.merge;
            self.merge_explicit = true;
        }
        if let Some(allowed_sources) = other.allowed_sources {
            self.allowed_sources = Some(allowed_sources);
        }
        if let Some(denied_sources) = other.denied_sources {
            self.denied_sources = Some(denied_sources);
        }
        for rule in other.validations {
            self.upsert_validation(rule);
        }
        for (rule_code, config) in other.validation_configs {
            self.validation_configs.insert(rule_code, config);
        }
    }

    fn upsert_validation(&mut self, rule: ValidationRule) {
        if let Some(existing) = self
            .validations
            .iter_mut()
            .find(|existing| existing.code() == rule.code())
        {
            *existing = rule;
        } else {
            self.validations.push(rule);
        }
    }

    pub(super) fn is_env_decoder_only(&self) -> bool {
        self.env_decode.is_some()
            && self.aliases.is_empty()
            && !self.secret
            && self.env.is_none()
            && self.doc.is_none()
            && self.example.is_none()
            && self.deprecated.is_none()
            && !self.has_default
            && !self.merge_explicit
            && self.allowed_sources.is_none()
            && self.denied_sources.is_none()
            && self.validations.is_empty()
            && self.validation_configs.is_empty()
    }
}

impl EffectiveSourcePolicy {
    pub(crate) fn apply_field(&mut self, field: &FieldMetadata) {
        if let Some(allowed_sources) = &field.allowed_sources {
            self.allowed_sources = Some(allowed_sources.clone());
        }
        if let Some(denied_sources) = &field.denied_sources {
            self.denied_sources = Some(denied_sources.clone());
        }
    }

    pub(crate) fn source_kind_allowed(&self, kind: SourceKind) -> bool {
        self.allowed_sources
            .as_ref()
            .is_none_or(|allowed_sources| allowed_sources.contains(&kind))
    }

    pub(crate) fn source_kind_denied(&self, kind: SourceKind) -> bool {
        self.denied_sources
            .as_ref()
            .is_some_and(|denied_sources| denied_sources.contains(&kind))
    }

    pub(crate) fn allowed_sources_vec(&self) -> Vec<SourceKind> {
        self.allowed_sources
            .as_ref()
            .map(|allowed_sources| allowed_sources.iter().copied().collect())
            .unwrap_or_default()
    }

    pub(crate) fn denied_sources_vec(&self) -> Vec<SourceKind> {
        self.denied_sources
            .as_ref()
            .map(|denied_sources| denied_sources.iter().copied().collect())
            .unwrap_or_default()
    }
}
