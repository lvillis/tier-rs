use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display, Formatter};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::{
    self, DeserializeOwned, IntoDeserializer, MapAccess, SeqAccess, Visitor,
    value::{Error as ValueDeError, MapAccessDeserializer},
};
use serde_json::{Map, Value};

#[cfg(any(feature = "json", feature = "toml", feature = "yaml"))]
use crate::error::LineColumn;
use crate::error::{ConfigError, UnknownField, ValidationError, ValidationErrors};
use crate::report::{
    ConfigReport, ConfigWarning, DeprecatedField, ResolutionStep, collect_diff_paths,
    collect_paths, get_value_at_path, join_path, normalize_path,
};
use crate::{ConfigMetadata, MergeStrategy, TierMetadata, ValidationCheck, ValidationRule};

type Normalizer<T> = Box<dyn Fn(&mut T) -> Result<(), String> + Send + Sync>;
type Validator<T> = Box<dyn Fn(&T) -> Result<(), ValidationErrors> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Kind of source that contributed configuration values.
pub enum SourceKind {
    /// Values originating from in-code defaults.
    Default,
    /// Values originating from configuration files.
    File,
    /// Values originating from environment variables.
    Environment,
    /// Values originating from CLI overrides.
    Arguments,
    /// Values originating from normalization hooks.
    Normalization,
    /// Values originating from a custom layer.
    Custom,
}

impl Display for SourceKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::File => write!(f, "file"),
            Self::Environment => write!(f, "env"),
            Self::Arguments => write!(f, "cli"),
            Self::Normalization => write!(f, "normalize"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Policy applied when unknown configuration paths are discovered.
pub enum UnknownFieldPolicy {
    /// Accept unknown fields silently.
    Allow,
    /// Accept unknown fields but emit warnings.
    Warn,
    #[default]
    /// Reject unknown fields with an error.
    Deny,
}

impl Display for UnknownFieldPolicy {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allow => write!(f, "allow"),
            Self::Warn => write!(f, "warn"),
            Self::Deny => write!(f, "deny"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Human-readable description of where a configuration value came from.
pub struct SourceTrace {
    /// High-level source category.
    pub kind: SourceKind,
    /// Source name, such as a file path or environment variable name.
    pub name: String,
    /// Optional location inside the source, when available.
    pub location: Option<String>,
}

impl SourceTrace {
    fn new(kind: SourceKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            location: None,
        }
    }
}

impl Display for SourceTrace {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.location {
            Some(location) if self.name.is_empty() => write!(f, "{}({location})", self.kind),
            Some(location) => write!(f, "{}({}:{location})", self.kind, self.name),
            None if self.name.is_empty() => write!(f, "{}", self.kind),
            None => write!(f, "{}({})", self.kind, self.name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Supported on-disk configuration file formats.
pub enum FileFormat {
    /// JSON source file.
    Json,
    /// TOML source file.
    Toml,
    /// YAML source file.
    Yaml,
}

impl Display for FileFormat {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json => write!(f, "json"),
            Self::Toml => write!(f, "toml"),
            Self::Yaml => write!(f, "yaml"),
        }
    }
}

#[derive(Debug, Clone)]
/// File-backed configuration source definition.
pub struct FileSource {
    candidates: Vec<PathBuf>,
    required: bool,
    format: Option<FileFormat>,
}

impl FileSource {
    /// Creates a required file source for a single path.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            candidates: vec![path.into()],
            required: true,
            format: None,
        }
    }

    /// Creates an optional file source for a single path.
    #[must_use]
    pub fn optional(path: impl Into<PathBuf>) -> Self {
        Self {
            candidates: vec![path.into()],
            required: false,
            format: None,
        }
    }

    /// Creates a required file source that searches candidate paths in order.
    #[must_use]
    pub fn search<I, P>(paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        Self {
            candidates: paths.into_iter().map(Into::into).collect(),
            required: true,
            format: None,
        }
    }

    /// Creates an optional file source that searches candidate paths in order.
    #[must_use]
    pub fn optional_search<I, P>(paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        Self {
            candidates: paths.into_iter().map(Into::into).collect(),
            required: false,
            format: None,
        }
    }

    /// Returns configured candidate paths in priority order.
    #[must_use]
    pub fn candidates(&self) -> &[PathBuf] {
        &self.candidates
    }

    /// Overrides format inference with an explicit file format.
    #[must_use]
    pub fn format(mut self, format: FileFormat) -> Self {
        self.format = Some(format);
        self
    }
}

#[derive(Debug, Clone)]
/// Environment variable source definition.
pub struct EnvSource {
    vars: BTreeMap<String, String>,
    prefix: Option<String>,
    separator: String,
    lowercase_segments: bool,
}

impl EnvSource {
    /// Captures the current process environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_pairs(std::env::vars())
    }

    /// Captures the current process environment using a prefix filter.
    #[must_use]
    pub fn prefixed(prefix: impl Into<String>) -> Self {
        Self::from_env().prefix(prefix)
    }

    /// Creates an environment source from explicit key/value pairs.
    #[must_use]
    pub fn from_pairs<I, K, V>(iter: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let vars = iter
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        Self {
            vars,
            prefix: None,
            separator: "__".to_owned(),
            lowercase_segments: true,
        }
    }

    /// Sets an environment variable prefix filter.
    #[must_use]
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Sets the segment separator used to map variables to paths.
    #[must_use]
    pub fn separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Preserves segment case instead of lowercasing them.
    #[must_use]
    pub fn preserve_case(mut self) -> Self {
        self.lowercase_segments = false;
        self
    }
}

#[derive(Debug, Clone)]
/// CLI override source definition.
pub struct ArgsSource {
    args: Vec<String>,
}

impl ArgsSource {
    /// Captures the current process arguments.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_args(std::env::args())
    }

    /// Creates an argument source from explicit argv values.
    #[must_use]
    pub fn from_args<I, S>(iter: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            args: iter.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone)]
/// Custom serializable configuration layer.
pub struct Layer {
    trace: SourceTrace,
    value: Value,
    entries: BTreeMap<String, SourceTrace>,
}

impl Layer {
    /// Creates a custom layer from a serializable value.
    pub fn custom<T>(name: impl Into<String>, value: T) -> Result<Self, ConfigError>
    where
        T: Serialize,
    {
        Self::from_serializable(SourceTrace::new(SourceKind::Custom, name), value)
    }

    fn from_serializable<T>(trace: SourceTrace, value: T) -> Result<Self, ConfigError>
    where
        T: Serialize,
    {
        let value = serde_json::to_value(value)?;
        Self::from_value(trace, value)
    }

    fn from_value(trace: SourceTrace, value: Value) -> Result<Self, ConfigError> {
        ensure_root_object(&value)?;

        let mut paths = Vec::new();
        collect_paths(&value, "", &mut paths);
        let entries = paths
            .into_iter()
            .map(|path| (path, trace.clone()))
            .collect::<BTreeMap<_, _>>();

        Ok(Self {
            trace,
            value,
            entries,
        })
    }
}

struct NamedNormalizer<T> {
    name: String,
    run: Normalizer<T>,
}

struct NamedValidator<T> {
    name: String,
    run: Validator<T>,
}

#[derive(Debug, Clone)]
struct ParsedArgs {
    profile: Option<String>,
    files: Vec<FileSource>,
    layer: Option<Layer>,
}

#[derive(Debug)]
/// Loaded configuration plus its diagnostic report.
pub struct LoadedConfig<T> {
    config: T,
    report: ConfigReport,
}

impl<T> LoadedConfig<T> {
    /// Returns the loaded configuration value.
    #[must_use]
    pub fn config(&self) -> &T {
        &self.config
    }

    /// Returns the diagnostic report associated with the load.
    #[must_use]
    pub fn report(&self) -> &ConfigReport {
        &self.report
    }

    /// Splits the loaded configuration into its value and report.
    pub fn into_parts(self) -> (T, ConfigReport) {
        (self.config, self.report)
    }

    /// Returns the loaded configuration value, discarding the report.
    pub fn into_inner(self) -> T {
        self.config
    }
}

impl<T> Clone for LoadedConfig<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            report: self.report.clone(),
        }
    }
}

impl<T> std::ops::Deref for LoadedConfig<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

/// Builder for layered configuration loading.
///
/// `ConfigLoader<T>` is the main entry point for `tier`. It starts from
/// in-code defaults and then applies additional layers in a deterministic
/// order. The loader can also attach metadata, secret paths, normalizers, and
/// validators before producing a typed [`LoadedConfig`].
///
/// # Examples
///
/// ```no_run
/// use serde::{Deserialize, Serialize};
/// use tier::{ConfigLoader, EnvSource};
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct AppConfig {
///     host: String,
///     port: u16,
/// }
///
/// impl Default for AppConfig {
///     fn default() -> Self {
///         Self {
///             host: "127.0.0.1".to_owned(),
///             port: 3000,
///         }
///     }
/// }
///
/// let loaded = ConfigLoader::new(AppConfig::default())
///     .file("config/app.toml")
///     .env(EnvSource::prefixed("APP"))
///     .load()?;
///
/// assert!(loaded.port >= 1);
/// # Ok::<(), tier::ConfigError>(())
/// ```
pub struct ConfigLoader<T> {
    defaults: T,
    files: Vec<FileSource>,
    env_sources: Vec<EnvSource>,
    args_source: Option<ArgsSource>,
    custom_layers: Vec<Layer>,
    metadata: ConfigMetadata,
    secret_paths: BTreeSet<String>,
    normalizers: Vec<NamedNormalizer<T>>,
    validators: Vec<NamedValidator<T>>,
    profile: Option<String>,
    unknown_field_policy: UnknownFieldPolicy,
}

impl<T> ConfigLoader<T>
where
    T: Serialize + DeserializeOwned,
{
    /// Creates a loader with the provided in-code defaults.
    #[must_use]
    pub fn new(defaults: T) -> Self {
        Self {
            defaults,
            files: Vec::new(),
            env_sources: Vec::new(),
            args_source: None,
            custom_layers: Vec::new(),
            metadata: ConfigMetadata::default(),
            secret_paths: BTreeSet::new(),
            normalizers: Vec::new(),
            validators: Vec::new(),
            profile: None,
            unknown_field_policy: UnknownFieldPolicy::Deny,
        }
    }

    /// Adds a required configuration file.
    #[must_use]
    pub fn file(mut self, path: impl Into<PathBuf>) -> Self {
        self.files.push(FileSource::new(path));
        self
    }

    /// Adds an optional configuration file.
    #[must_use]
    pub fn optional_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.files.push(FileSource::optional(path));
        self
    }

    /// Adds a custom file source definition.
    #[must_use]
    pub fn with_file(mut self, file: FileSource) -> Self {
        self.files.push(file);
        self
    }

    /// Adds an environment variable source.
    #[must_use]
    pub fn env(mut self, source: EnvSource) -> Self {
        self.env_sources.push(source);
        self
    }

    /// Adds CLI overrides from an [`ArgsSource`].
    #[must_use]
    pub fn args(mut self, source: ArgsSource) -> Self {
        self.args_source = Some(source);
        self
    }

    /// Adds a custom serializable layer.
    pub fn layer(mut self, layer: Layer) -> Self {
        self.custom_layers.push(layer);
        self
    }

    /// Marks a dot-delimited path as sensitive for report redaction.
    #[must_use]
    pub fn secret_path(mut self, path: impl Into<String>) -> Self {
        self.secret_paths.insert(normalize_path(&path.into()));
        self
    }

    /// Applies explicit field metadata to the loader.
    ///
    /// This is the manual alternative to [`ConfigLoader::derive_metadata`].
    /// Use it when you want env overrides, secrets, merge policies, or
    /// declared validations without enabling the `derive` feature.
    #[must_use]
    pub fn metadata(mut self, metadata: ConfigMetadata) -> Self {
        self.secret_paths.extend(metadata.secret_paths());
        self.metadata.extend(metadata);
        self
    }

    /// Sets the active profile used by `{profile}` path templates.
    #[must_use]
    pub fn profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    /// Applies metadata-derived secret paths for the target configuration type.
    ///
    /// This is the most ergonomic way to connect `#[derive(TierConfig)]`
    /// metadata to the loader when the `derive` feature is enabled.
    #[must_use]
    pub fn derive_metadata(self) -> Self
    where
        T: TierMetadata,
    {
        self.metadata(T::metadata())
    }

    /// Sets the unknown field policy.
    #[must_use]
    pub fn unknown_field_policy(mut self, policy: UnknownFieldPolicy) -> Self {
        self.unknown_field_policy = policy;
        self
    }

    /// Allows unknown fields without warnings.
    #[must_use]
    pub fn allow_unknown_fields(self) -> Self {
        self.unknown_field_policy(UnknownFieldPolicy::Allow)
    }

    /// Allows unknown fields and records warnings.
    #[must_use]
    pub fn warn_unknown_fields(self) -> Self {
        self.unknown_field_policy(UnknownFieldPolicy::Warn)
    }

    /// Rejects unknown fields with an error.
    #[must_use]
    pub fn deny_unknown_fields(self) -> Self {
        self.unknown_field_policy(UnknownFieldPolicy::Deny)
    }

    /// Registers a normalization hook applied after merge and before validation.
    #[must_use]
    pub fn normalizer<F, E>(mut self, name: impl Into<String>, normalizer: F) -> Self
    where
        F: Fn(&mut T) -> Result<(), E> + Send + Sync + 'static,
        E: Display,
    {
        self.normalizers.push(NamedNormalizer {
            name: name.into(),
            run: Box::new(move |config| normalizer(config).map_err(|error| error.to_string())),
        });
        self
    }

    /// Registers a validation hook applied after normalization.
    #[must_use]
    pub fn validator<F>(mut self, name: impl Into<String>, validator: F) -> Self
    where
        F: Fn(&T) -> Result<(), ValidationErrors> + Send + Sync + 'static,
    {
        self.validators.push(NamedValidator {
            name: name.into(),
            run: Box::new(validator),
        });
        self
    }

    /// Loads configuration from all configured layers.
    pub fn load(self) -> Result<LoadedConfig<T>, ConfigError> {
        let unknown_field_policy = self.unknown_field_policy;
        let metadata = self.metadata.clone();
        let parsed_args = match self.args_source {
            Some(source) => Some(parse_args(source)?),
            None => None,
        };

        let profile = parsed_args
            .as_ref()
            .and_then(|args| args.profile.clone())
            .or(self.profile);

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

        for layer in self.custom_layers {
            layers.push(canonicalize_layer_paths(layer, &metadata)?);
        }

        for env_source in self.env_sources {
            let layer = env_source.into_layer(&metadata)?;
            if let Some(layer) = layer {
                layers.push(canonicalize_layer_paths(layer, &metadata)?);
            }
        }

        if let Some(parsed) = parsed_args
            && let Some(layer) = parsed.layer
        {
            layers.push(canonicalize_layer_paths(layer, &metadata)?);
        }

        let defaults_value =
            canonicalize_value_paths(&serde_json::to_value(&self.defaults)?, &metadata)?;
        let merge_strategies = metadata.merge_strategies();

        let mut report = ConfigReport::new(defaults_value.clone(), self.secret_paths.clone());
        let mut string_coercion_paths = BTreeSet::new();

        let mut merged = defaults_value;
        ensure_root_object(&merged)?;

        for layer in layers {
            if matches!(
                layer.trace.kind,
                SourceKind::Environment | SourceKind::Arguments
            ) {
                collect_coercion_paths(&layer.value, "", &mut string_coercion_paths);
            }
            report.record_source(layer.trace.clone());
            record_layer_steps(&mut report, &layer, &self.secret_paths);
            record_deprecation_warnings(&mut report, &layer, &metadata);
            if !matches!(layer.trace.kind, SourceKind::Default) {
                merge_values(&mut merged, layer.value, "", &merge_strategies);
            }
        }

        let mut config = deserialize_with_path(&merged, &report, &string_coercion_paths)?;
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
            let trace = SourceTrace::new(SourceKind::Normalization, normalizer.name.clone());
            report.record_source(trace.clone());
            record_diff_steps(&mut report, &before, &after, &trace, &self.secret_paths);
        }

        let normalized_value =
            canonicalize_value_paths(&serde_json::to_value(&config)?, &metadata)?;
        let mut declared_errors =
            validate_declared_rules(&normalized_value, &metadata, &self.secret_paths);
        declared_errors.extend(validate_declared_checks(
            &normalized_value,
            &metadata,
            &self.secret_paths,
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

#[cfg(feature = "schema")]
impl<T> ConfigLoader<T>
where
    T: Serialize + DeserializeOwned + schemars::JsonSchema,
{
    /// Discovers secret paths from the target type's JSON Schema.
    #[must_use]
    pub fn discover_secret_paths_from_schema(mut self) -> Self {
        for path in schema_secret_paths::<T>() {
            self.secret_paths.insert(path);
        }
        self
    }
}

impl EnvSource {
    fn into_layer(self, metadata: &ConfigMetadata) -> Result<Option<Layer>, ConfigError> {
        let EnvSource {
            vars,
            prefix,
            separator,
            lowercase_segments,
        } = self;
        let env_overrides = metadata.env_overrides();
        let mut root = Value::Object(Map::new());
        let mut entries = BTreeMap::new();

        for (name, raw_value) in vars {
            let path = match env_overrides.get(&name) {
                Some(path) => path.clone(),
                None => {
                    let Some(path) =
                        path_for_env_var(&name, prefix.as_deref(), &separator, lowercase_segments)
                    else {
                        continue;
                    };
                    path
                }
            };
            if path.is_empty() {
                continue;
            }

            let value = parse_override_value(&raw_value);
            let segments = path.split('.').collect::<Vec<_>>();
            insert_path(&mut root, &segments, value).map_err(|message| {
                ConfigError::InvalidEnv {
                    name: name.clone(),
                    path: path.clone(),
                    message,
                }
            })?;

            entries.insert(
                path.clone(),
                SourceTrace::new(SourceKind::Environment, name.clone()),
            );

            // Record parents so explain() can work on non-leaf nodes too.
            let mut prefix = String::new();
            for segment in segments {
                if !prefix.is_empty() {
                    prefix.push('.');
                }
                prefix.push_str(segment);
                entries
                    .entry(prefix.clone())
                    .or_insert_with(|| SourceTrace::new(SourceKind::Environment, name.clone()));
            }
        }

        if entries.is_empty() {
            return Ok(None);
        }

        Ok(Some(Layer {
            trace: SourceTrace::new(SourceKind::Environment, "environment"),
            value: root,
            entries,
        }))
    }
}

fn path_for_env_var(
    key: &str,
    prefix: Option<&str>,
    separator: &str,
    lowercase_segments: bool,
) -> Option<String> {
    let mut remainder = key;
    if let Some(prefix) = prefix {
        remainder = remainder.strip_prefix(prefix)?;
        remainder = remainder.trim_start_matches('_');
    }

    if remainder.is_empty() {
        return None;
    }

    let mut segments = Vec::new();
    for segment in remainder.split(separator) {
        if segment.is_empty() {
            return None;
        }
        let segment = if lowercase_segments {
            segment.to_ascii_lowercase()
        } else {
            segment.to_owned()
        };
        segments.push(segment);
    }

    Some(segments.join("."))
}

fn record_layer_steps(report: &mut ConfigReport, layer: &Layer, secret_paths: &BTreeSet<String>) {
    for (path, trace) in &layer.entries {
        if let Some(value) = get_value_at_path(&layer.value, path) {
            let redacted = is_secret_path(secret_paths, path);
            let rendered = if redacted {
                Value::String("***redacted***".to_owned())
            } else {
                value.clone()
            };
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
    let mut paths = Vec::new();
    collect_diff_paths(before, after, "", &mut paths);
    paths.sort();
    paths.dedup();

    for path in paths {
        if let Some(value) = get_value_at_path(after, &path) {
            let redacted = is_secret_path(secret_paths, &path);
            let rendered = if redacted {
                Value::String("***redacted***".to_owned())
            } else {
                value.clone()
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
        let prefix = format!("{}.", field.path);
        let used = used_paths
            .iter()
            .any(|path| path == &field.path || path.starts_with(&prefix));
        if used && warned.insert(field.path.clone()) {
            report.record_warning(ConfigWarning::DeprecatedField(
                DeprecatedField::new(field.path.clone())
                    .with_source(Some(layer.trace.clone()))
                    .with_note(field.deprecated.clone()),
            ));
        }
    }
}

fn canonicalize_layer_paths(layer: Layer, metadata: &ConfigMetadata) -> Result<Layer, ConfigError> {
    let aliases = metadata.alias_overrides();
    if aliases.is_empty() {
        return Ok(layer);
    }

    let value = canonicalize_value_paths(&layer.value, metadata)?;
    if value == layer.value {
        return Ok(layer);
    }

    let entries = layer
        .entries
        .into_iter()
        .map(|(path, trace)| (aliases.get(&path).cloned().unwrap_or(path), trace))
        .collect();

    Ok(Layer {
        trace: layer.trace,
        value,
        entries,
    })
}

fn canonicalize_value_paths(
    value: &Value,
    metadata: &ConfigMetadata,
) -> Result<Value, ConfigError> {
    let aliases = metadata.alias_overrides();
    if aliases.is_empty() {
        return Ok(value.clone());
    }

    ensure_root_object(value)?;
    let mut canonical = Value::Object(Map::new());
    canonicalize_object(value, "", "", &aliases, &mut canonical)?;
    Ok(canonical)
}

fn canonicalize_object(
    value: &Value,
    input_current: &str,
    canonical_current: &str,
    aliases: &BTreeMap<String, String>,
    root: &mut Value,
) -> Result<(), ConfigError> {
    match value {
        Value::Object(map) if map.is_empty() && !canonical_current.is_empty() => {
            let segments = canonical_current.split('.').collect::<Vec<_>>();
            insert_path(root, &segments, Value::Object(Map::new())).map_err(|message| {
                ConfigError::InvalidArg {
                    arg: canonical_current.to_owned(),
                    message,
                }
            })?;
        }
        Value::Object(map) => {
            for (key, child) in map {
                let input_path = join_path(input_current, key);
                let default_canonical_path = join_path(canonical_current, key);
                let canonical_path = aliases
                    .get(&input_path)
                    .cloned()
                    .unwrap_or(default_canonical_path);
                match child {
                    Value::Object(_) => {
                        canonicalize_object(child, &input_path, &canonical_path, aliases, root)?;
                    }
                    _ => {
                        let segments = canonical_path.split('.').collect::<Vec<_>>();
                        insert_path(root, &segments, child.clone()).map_err(|message| {
                            ConfigError::InvalidArg {
                                arg: canonical_path.clone(),
                                message,
                            }
                        })?;
                    }
                }
            }
        }
        _ => {
            return Err(ConfigError::RootMustBeObject {
                actual: value_kind(value),
            });
        }
    }

    Ok(())
}

fn is_secret_path(secret_paths: &BTreeSet<String>, path: &str) -> bool {
    secret_paths
        .iter()
        .any(|secret| path == secret || path.starts_with(&format!("{secret}.")))
}

fn validate_declared_rules(
    value: &Value,
    metadata: &ConfigMetadata,
    secret_paths: &BTreeSet<String>,
) -> ValidationErrors {
    let mut errors = ValidationErrors::new();

    for field in metadata.fields() {
        if field.validations.is_empty() || field.path.is_empty() {
            continue;
        }
        let Some(actual) = get_value_at_path(value, &field.path) else {
            continue;
        };
        for rule in &field.validations {
            if let Some(error) = validate_declared_rule(&field.path, actual, rule, secret_paths) {
                errors.push(error);
            }
        }
    }

    errors
}

fn validate_declared_checks(
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
                if path_is_present(value, path) {
                    let missing = missing_paths(value, requires);
                    if !missing.is_empty() {
                        errors.push(group_validation_error(
                            check,
                            std::iter::once(path.as_str())
                                .chain(missing.iter().map(String::as_str)),
                            secret_paths,
                            &format!("{} requires {}", path, missing.join(", ")),
                            Some(serde_json::json!({ "trigger": path, "requires": requires })),
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
                let matches =
                    get_value_at_path(value, path).is_some_and(|actual| actual == &equals.0);
                if matches {
                    let missing = missing_paths(value, requires);
                    if !missing.is_empty() {
                        errors.push(group_validation_error(
                            check,
                            std::iter::once(path.as_str())
                                .chain(missing.iter().map(String::as_str)),
                            secret_paths,
                            &format!("{} == {} requires {}", path, equals, missing.join(", ")),
                            Some(serde_json::json!({
                                "trigger": path,
                                "equals": equals,
                                "requires": requires
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
            let valid = match actual {
                Value::String(value) => !value.is_empty(),
                Value::Array(values) => !values.is_empty(),
                Value::Object(values) => !values.is_empty(),
                Value::Null => false,
                _ => true,
            };

            (!valid).then(|| {
                validation_error(path, actual, rule, secret_paths, "must not be empty", None)
            })
        }
        ValidationRule::Min(min) if !min.is_finite() => Some(validation_error(
            path,
            actual,
            rule,
            secret_paths,
            &format!("declared minimum bound must be finite, got {min}"),
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
            &format!("declared maximum bound must be finite, got {max}"),
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

fn path_is_present(value: &Value, path: &str) -> bool {
    get_value_at_path(value, path).is_some_and(is_present_value)
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
            for child in values {
                redact_group_value_recursive(child, current, related_paths, secret_paths);
            }
        }
        _ => {}
    }
}

fn is_valid_hostname(value: &str) -> bool {
    if value.is_empty() || value.len() > 253 {
        return false;
    }

    value.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    })
}

fn parse_args(source: ArgsSource) -> Result<ParsedArgs, ConfigError> {
    let mut args = source.args.into_iter();
    let mut files = Vec::new();
    let mut profile = None;
    let mut root = Value::Object(Map::new());
    let mut entries = BTreeMap::new();

    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--config=") {
            files.push(FileSource::new(value));
            continue;
        }

        if arg == "--config" {
            let value = args.next().ok_or_else(|| ConfigError::MissingArgValue {
                flag: "--config".to_owned(),
            })?;
            files.push(FileSource::new(value));
            continue;
        }

        if let Some(value) = arg.strip_prefix("--profile=") {
            profile = Some(value.to_owned());
            continue;
        }

        if arg == "--profile" {
            profile = Some(args.next().ok_or_else(|| ConfigError::MissingArgValue {
                flag: "--profile".to_owned(),
            })?);
            continue;
        }

        let set_value = if let Some(value) = arg.strip_prefix("--set=") {
            Some(value.to_owned())
        } else if arg == "--set" {
            Some(args.next().ok_or_else(|| ConfigError::MissingArgValue {
                flag: "--set".to_owned(),
            })?)
        } else {
            None
        };

        let Some(set_value) = set_value else {
            continue;
        };

        let (path, raw_value) =
            set_value
                .split_once('=')
                .ok_or_else(|| ConfigError::InvalidArg {
                    arg: set_value.clone(),
                    message: "expected key=value".to_owned(),
                })?;
        let path = normalize_path(path);
        if path.is_empty() {
            return Err(ConfigError::InvalidArg {
                arg: set_value,
                message: "configuration path cannot be empty".to_owned(),
            });
        }

        let segments = path.split('.').collect::<Vec<_>>();
        insert_path(&mut root, &segments, parse_override_value(raw_value)).map_err(|message| {
            ConfigError::InvalidArg {
                arg: format!("--set {path}={raw_value}"),
                message,
            }
        })?;

        entries.insert(
            path.clone(),
            SourceTrace::new(SourceKind::Arguments, format!("--set {path}={raw_value}")),
        );

        let mut prefix = String::new();
        for segment in segments {
            if !prefix.is_empty() {
                prefix.push('.');
            }
            prefix.push_str(segment);
            entries.entry(prefix.clone()).or_insert_with(|| {
                SourceTrace::new(SourceKind::Arguments, format!("--set {path}={raw_value}"))
            });
        }
    }

    let layer = if entries.is_empty() {
        None
    } else {
        Some(Layer {
            trace: SourceTrace::new(SourceKind::Arguments, "arguments"),
            value: root,
            entries,
        })
    };

    Ok(ParsedArgs {
        profile,
        files,
        layer,
    })
}

fn load_file_layer(file: FileSource, profile: Option<&str>) -> Result<Option<Layer>, ConfigError> {
    let resolved_paths = file
        .candidates
        .iter()
        .map(|path| resolve_profile_path(path, profile))
        .collect::<Result<Vec<_>, _>>()?;
    let path = resolved_paths.iter().find(|path| path.exists()).cloned();
    let Some(path) = path else {
        return if file.required {
            match resolved_paths.as_slice() {
                [] => Err(ConfigError::InvalidArg {
                    arg: "file source".to_owned(),
                    message: "at least one candidate path must be provided".to_owned(),
                }),
                [single] => Err(ConfigError::MissingFile {
                    path: single.clone(),
                }),
                _ => Err(ConfigError::MissingFiles {
                    paths: resolved_paths,
                }),
            }
        } else {
            Ok(None)
        };
    };

    let content = std::fs::read_to_string(&path).map_err(|source| ConfigError::ReadFile {
        path: path.clone(),
        source,
    })?;
    let format = match file.format {
        Some(format) => format,
        None => infer_format(&path)?,
    };
    let value = parse_file_value(&path, &content, format)?;

    let layer = Layer::from_value(
        SourceTrace::new(SourceKind::File, path.display().to_string()),
        value,
    )?;

    Ok(Some(layer))
}

fn resolve_profile_path(path: &Path, profile: Option<&str>) -> Result<PathBuf, ConfigError> {
    let raw = path.to_string_lossy();
    if raw.contains("{profile}") {
        let profile = profile.ok_or_else(|| ConfigError::MissingProfile {
            path: path.to_path_buf(),
        })?;
        Ok(PathBuf::from(raw.replace("{profile}", profile)))
    } else {
        Ok(path.to_path_buf())
    }
}

fn collect_known_paths<T>(config: &T) -> Result<BTreeSet<String>, ConfigError>
where
    T: Serialize,
{
    let value = serde_json::to_value(config)?;
    let mut paths = Vec::new();
    collect_paths(&value, "", &mut paths);
    Ok(paths.into_iter().collect())
}

fn collect_suggestion_paths(
    metadata: &ConfigMetadata,
    known_paths: &BTreeSet<String>,
) -> BTreeMap<String, String> {
    let mut candidates = BTreeMap::new();

    for field in metadata.fields() {
        candidates.insert(field.path.clone(), field.path.clone());
        for alias in &field.aliases {
            candidates.insert(alias.clone(), field.path.clone());
        }
    }

    if candidates.is_empty() {
        for path in known_paths {
            candidates.insert(path.clone(), path.clone());
        }
    } else {
        for path in known_paths {
            candidates
                .entry(path.clone())
                .or_insert_with(|| path.clone());
        }
    }

    candidates
}

fn collect_unknown_fields<T>(
    value: &Value,
    suggestion_paths: &BTreeMap<String, String>,
    report: &ConfigReport,
    string_coercion_paths: &BTreeSet<String>,
) -> Result<Vec<UnknownField>, ConfigError>
where
    T: DeserializeOwned,
{
    let mut ignored = Vec::new();
    let deserializer = CoercingDeserializer::new(value, "", string_coercion_paths);
    let result: Result<T, ValueDeError> = serde_ignored::deserialize(deserializer, |path| {
        ignored.push(normalize_external_path(&path.to_string()))
    });
    result.map_err(|error| ConfigError::Deserialize {
        path: "<unknown>".to_owned(),
        provenance: None,
        message: error.to_string(),
    })?;

    ignored.sort();
    ignored.dedup();

    Ok(ignored
        .into_iter()
        .map(|path| {
            let source = find_source_for_unknown_path(report, &path);
            let suggestion = best_path_suggestion(&path, suggestion_paths);
            UnknownField::new(path)
                .with_source(source)
                .with_suggestion(suggestion)
        })
        .collect())
}

fn find_source_for_unknown_path(report: &ConfigReport, path: &str) -> Option<SourceTrace> {
    let mut current = Some(normalize_external_path(path));
    while let Some(candidate) = current {
        if let Some(source) = report.latest_source_for(&candidate) {
            return Some(source);
        }
        current = candidate
            .rsplit_once('.')
            .map(|(parent, _)| parent.to_owned())
            .filter(|parent| !parent.is_empty());
    }
    None
}

fn best_path_suggestion(path: &str, suggestion_paths: &BTreeMap<String, String>) -> Option<String> {
    if suggestion_paths.is_empty() {
        return None;
    }

    let normalized = normalize_external_path(path);
    let (parent, leaf) = normalized
        .rsplit_once('.')
        .map_or(("", normalized.as_str()), |(parent, leaf)| (parent, leaf));

    let mut sibling_best: Option<(usize, String)> = None;
    for (candidate, canonical) in suggestion_paths {
        let (candidate_parent, candidate_leaf) = candidate
            .rsplit_once('.')
            .map_or(("", candidate.as_str()), |(parent, leaf)| (parent, leaf));
        if candidate_parent != parent {
            continue;
        }

        let distance = levenshtein(leaf, candidate_leaf);
        match &mut sibling_best {
            Some((best_distance, best_candidate)) if distance < *best_distance => {
                *best_distance = distance;
                *best_candidate = canonical.clone();
            }
            None => sibling_best = Some((distance, canonical.clone())),
            _ => {}
        }
    }

    if let Some((distance, suggestion)) = sibling_best
        && distance <= 3
    {
        return Some(suggestion);
    }

    let mut best: Option<(usize, String)> = None;
    for (candidate, canonical) in suggestion_paths {
        let distance = levenshtein(&normalized, candidate);
        match &mut best {
            Some((best_distance, best_candidate)) if distance < *best_distance => {
                *best_distance = distance;
                *best_candidate = canonical.clone();
            }
            None => best = Some((distance, canonical.clone())),
            _ => {}
        }
    }

    best.and_then(|(distance, suggestion)| {
        let max_len = normalized.len().max(suggestion.len());
        (distance <= (max_len / 3).max(2)).then_some(suggestion)
    })
}

fn levenshtein(left: &str, right: &str) -> usize {
    if left == right {
        return 0;
    }
    if left.is_empty() {
        return right.chars().count();
    }
    if right.is_empty() {
        return left.chars().count();
    }

    let right_chars = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right_chars.len()).collect::<Vec<_>>();
    let mut current = vec![0; right_chars.len() + 1];

    for (left_index, left_char) in left.chars().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let cost = usize::from(left_char != *right_char);
            current[right_index + 1] = (current[right_index] + 1)
                .min(previous[right_index + 1] + 1)
                .min(previous[right_index] + cost);
        }
        previous.clone_from_slice(&current);
    }

    previous[right_chars.len()]
}

#[cfg(feature = "schema")]
fn schema_secret_paths<T>() -> BTreeSet<String>
where
    T: schemars::JsonSchema,
{
    let schema = crate::schema::json_schema_for::<T>();
    let mut paths = BTreeSet::new();
    collect_secret_paths_from_schema(&schema, &schema, "", &mut paths, &mut BTreeSet::new());
    paths
}

#[cfg(feature = "schema")]
fn collect_secret_paths_from_schema(
    schema: &Value,
    root: &Value,
    current: &str,
    paths: &mut BTreeSet<String>,
    visited_refs: &mut BTreeSet<String>,
) {
    let Some(object) = schema.as_object() else {
        return;
    };

    let is_secret = object
        .get("x-tier-secret")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || object
            .get("writeOnly")
            .and_then(Value::as_bool)
            .unwrap_or(false);

    if is_secret && !current.is_empty() {
        paths.insert(current.to_owned());
    }

    if let Some(reference) = object.get("$ref").and_then(Value::as_str)
        && visited_refs.insert(reference.to_owned())
        && let Some(target) = resolve_schema_ref(root, reference)
    {
        collect_secret_paths_from_schema(target, root, current, paths, visited_refs);
    }

    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        for (key, child) in properties {
            let next = crate::report::join_path(current, key);
            collect_secret_paths_from_schema(child, root, &next, paths, visited_refs);
        }
    }

    if let Some(items) = object.get("items") {
        collect_secret_paths_from_schema(items, root, current, paths, visited_refs);
    }

    for keyword in ["allOf", "anyOf", "oneOf"] {
        if let Some(array) = object.get(keyword).and_then(Value::as_array) {
            for child in array {
                collect_secret_paths_from_schema(child, root, current, paths, visited_refs);
            }
        }
    }
}

#[cfg(feature = "schema")]
fn resolve_schema_ref<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    root.pointer(pointer)
}

fn infer_format(path: &Path) -> Result<FileFormat, ConfigError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| ConfigError::InvalidArg {
            arg: path.display().to_string(),
            message: "cannot infer file format without an extension".to_owned(),
        })?;

    match extension.as_str() {
        "json" => Ok(FileFormat::Json),
        "toml" => Ok(FileFormat::Toml),
        "yaml" | "yml" => Ok(FileFormat::Yaml),
        other => Err(ConfigError::InvalidArg {
            arg: path.display().to_string(),
            message: format!("unsupported file format extension: {other}"),
        }),
    }
}

fn parse_file_value(path: &Path, content: &str, format: FileFormat) -> Result<Value, ConfigError> {
    match format {
        FileFormat::Json => {
            #[cfg(feature = "json")]
            {
                let value =
                    serde_json::from_str(content).map_err(|error| ConfigError::ParseFile {
                        path: path.to_path_buf(),
                        format,
                        location: Some(LineColumn {
                            line: error.line(),
                            column: error.column(),
                        }),
                        message: error.to_string(),
                    })?;
                Ok(value)
            }

            #[cfg(not(feature = "json"))]
            {
                let _ = (path, content);
                Err(ConfigError::InvalidArg {
                    arg: "json".to_owned(),
                    message: "json support is disabled for this build".to_owned(),
                })
            }
        }
        FileFormat::Toml => {
            #[cfg(feature = "toml")]
            {
                let value = toml::from_str::<toml::Value>(content).map_err(|error| {
                    ConfigError::ParseFile {
                        path: path.to_path_buf(),
                        format,
                        location: error
                            .span()
                            .map(|span| offset_to_line_column(content, span.start)),
                        message: error.to_string(),
                    }
                })?;
                serde_json::to_value(value).map_err(ConfigError::from)
            }

            #[cfg(not(feature = "toml"))]
            {
                let _ = (path, content);
                Err(ConfigError::InvalidArg {
                    arg: "toml".to_owned(),
                    message: "toml support is disabled for this build".to_owned(),
                })
            }
        }
        FileFormat::Yaml => {
            #[cfg(feature = "yaml")]
            {
                let value = serde_yaml::from_str::<Value>(content).map_err(|error| {
                    ConfigError::ParseFile {
                        path: path.to_path_buf(),
                        format,
                        location: error.location().map(|location| LineColumn {
                            line: location.line(),
                            column: location.column(),
                        }),
                        message: error.to_string(),
                    }
                })?;
                Ok(value)
            }

            #[cfg(not(feature = "yaml"))]
            {
                let _ = (path, content);
                Err(ConfigError::InvalidArg {
                    arg: "yaml".to_owned(),
                    message: "yaml support is disabled for this build".to_owned(),
                })
            }
        }
    }
}

#[cfg(feature = "toml")]
fn offset_to_line_column(input: &str, offset: usize) -> LineColumn {
    let mut line = 1;
    let mut column = 1;
    for (index, byte) in input.bytes().enumerate() {
        if index == offset {
            break;
        }
        if byte == b'\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    LineColumn { line, column }
}

fn deserialize_with_path<T>(
    value: &Value,
    report: &ConfigReport,
    string_coercion_paths: &BTreeSet<String>,
) -> Result<T, ConfigError>
where
    T: DeserializeOwned,
{
    let deserializer = CoercingDeserializer::new(value, "", string_coercion_paths);
    let result: Result<T, serde_path_to_error::Error<ValueDeError>> =
        serde_path_to_error::deserialize(deserializer);
    result.map_err(|error| {
        let path = error.path().to_string();
        let lookup_path = normalize_external_path(&path);
        let source = find_source_for_unknown_path(report, &lookup_path);
        ConfigError::Deserialize {
            path,
            provenance: source,
            message: error.inner().to_string(),
        }
    })
}

fn ensure_root_object(value: &Value) -> Result<(), ConfigError> {
    if matches!(value, Value::Object(_)) {
        Ok(())
    } else {
        Err(ConfigError::RootMustBeObject {
            actual: value_kind(value),
        })
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn merge_values(
    target: &mut Value,
    overlay: Value,
    current_path: &str,
    strategies: &BTreeMap<String, MergeStrategy>,
) {
    let strategy = strategies
        .get(current_path)
        .copied()
        .unwrap_or(MergeStrategy::Merge);

    match strategy {
        MergeStrategy::Replace if !current_path.is_empty() => *target = overlay,
        MergeStrategy::Append => match (target, overlay) {
            (Value::Array(target), Value::Array(mut overlay)) => target.append(&mut overlay),
            (Value::Object(target), Value::Object(overlay)) => {
                for (key, value) in overlay {
                    let path = join_path(current_path, &key);
                    match target.get_mut(&key) {
                        Some(existing) => merge_values(existing, value, &path, strategies),
                        None => {
                            target.insert(key, value);
                        }
                    }
                }
            }
            (target, overlay) => *target = overlay,
        },
        MergeStrategy::Merge | MergeStrategy::Replace => match (target, overlay) {
            (Value::Object(target), Value::Object(overlay)) => {
                for (key, value) in overlay {
                    let path = join_path(current_path, &key);
                    match target.get_mut(&key) {
                        Some(existing) => merge_values(existing, value, &path, strategies),
                        None => {
                            target.insert(key, value);
                        }
                    }
                }
            }
            (target, overlay) => *target = overlay,
        },
    }
}

fn collect_coercion_paths(value: &Value, current: &str, paths: &mut BTreeSet<String>) {
    if !current.is_empty() {
        paths.insert(current.to_owned());
    }

    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let next = join_path(current, key);
                collect_coercion_paths(child, &next, paths);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                let next = join_path(current, &index.to_string());
                collect_coercion_paths(child, &next, paths);
            }
        }
        _ => {}
    }
}

fn normalize_external_path(path: &str) -> String {
    if path == "." {
        return String::new();
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = path.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '.' => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
            }
            '[' => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
                let mut index = String::new();
                for next in chars.by_ref() {
                    if next == ']' {
                        break;
                    }
                    index.push(next);
                }
                if !index.is_empty() {
                    segments.push(index);
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        segments.push(current);
    }

    normalize_path(&segments.join("."))
}

fn parse_override_value(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Value::String(String::new());
    }

    let uses_explicit_json_syntax =
        matches!(trimmed.chars().next(), Some('{') | Some('[') | Some('"'));

    if uses_explicit_json_syntax && let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return value;
    }

    Value::String(raw.to_owned())
}

fn unexpected_value(value: &Value) -> de::Unexpected<'_> {
    match value {
        Value::Null => de::Unexpected::Unit,
        Value::Bool(value) => de::Unexpected::Bool(*value),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                de::Unexpected::Signed(value)
            } else if let Some(value) = number.as_u64() {
                de::Unexpected::Unsigned(value)
            } else if let Some(value) = number.as_f64() {
                de::Unexpected::Float(value)
            } else {
                de::Unexpected::Other("number")
            }
        }
        Value::String(value) => de::Unexpected::Str(value),
        Value::Array(_) => de::Unexpected::Other("array"),
        Value::Object(_) => de::Unexpected::Other("object"),
    }
}

struct CoercingDeserializer<'a> {
    value: &'a Value,
    path: String,
    string_coercion_paths: &'a BTreeSet<String>,
}

impl<'a> CoercingDeserializer<'a> {
    fn new(
        value: &'a Value,
        path: impl Into<String>,
        string_coercion_paths: &'a BTreeSet<String>,
    ) -> Self {
        Self {
            value,
            path: path.into(),
            string_coercion_paths,
        }
    }

    fn coercible_string(&self) -> Option<&'a str> {
        match self.value {
            Value::String(value) if self.string_coercion_paths.contains(&self.path) => Some(value),
            _ => None,
        }
    }

    fn invalid_type<'de, V>(&self, visitor: &V) -> ValueDeError
    where
        V: Visitor<'de>,
    {
        de::Error::invalid_type(unexpected_value(self.value), visitor)
    }

    fn invalid_string_type<'de, V>(&self, raw: &str, visitor: &V) -> ValueDeError
    where
        V: Visitor<'de>,
    {
        de::Error::invalid_type(de::Unexpected::Str(raw), visitor)
    }
}

macro_rules! deserialize_integer_from_value {
    ($method:ident, $visit:ident, $ty:ty) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            if let Some(raw) = self.coercible_string() {
                return raw
                    .trim()
                    .parse::<$ty>()
                    .map_err(|_| self.invalid_string_type(raw, &visitor))
                    .and_then(|value| visitor.$visit(value));
            }

            match self.value {
                Value::Number(number) => number
                    .to_string()
                    .parse::<$ty>()
                    .map_err(|_| self.invalid_type(&visitor))
                    .and_then(|value| visitor.$visit(value)),
                _ => Err(self.invalid_type(&visitor)),
            }
        }
    };
}

macro_rules! deserialize_float_from_value {
    ($method:ident, $visit:ident, $ty:ty) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            if let Some(raw) = self.coercible_string() {
                return raw
                    .trim()
                    .parse::<$ty>()
                    .map_err(|_| self.invalid_string_type(raw, &visitor))
                    .and_then(|value| visitor.$visit(value));
            }

            match self.value {
                Value::Number(number) => number
                    .to_string()
                    .parse::<$ty>()
                    .map_err(|_| self.invalid_type(&visitor))
                    .and_then(|value| visitor.$visit(value)),
                _ => Err(self.invalid_type(&visitor)),
            }
        }
    };
}

impl<'de, 'a> de::Deserializer<'de> for CoercingDeserializer<'a>
where
    'a: 'de,
{
    type Error = ValueDeError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Null => visitor.visit_unit(),
            Value::Bool(value) => visitor.visit_bool(*value),
            Value::Number(number) => {
                if let Some(value) = number.as_i64() {
                    visitor.visit_i64(value)
                } else if let Some(value) = number.as_u64() {
                    visitor.visit_u64(value)
                } else if let Some(value) = number.as_f64() {
                    visitor.visit_f64(value)
                } else {
                    Err(self.invalid_type(&visitor))
                }
            }
            Value::String(value) => visitor.visit_borrowed_str(value),
            Value::Array(values) => visitor.visit_seq(CoercingSeqAccess::new(
                values.iter().enumerate(),
                self.path,
                self.string_coercion_paths,
            )),
            Value::Object(map) => visitor.visit_map(CoercingMapAccess::new(
                map.iter(),
                self.path,
                self.string_coercion_paths,
            )),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if let Some(raw) = self.coercible_string() {
            return match raw.trim() {
                "true" => visitor.visit_bool(true),
                "false" => visitor.visit_bool(false),
                _ => Err(self.invalid_string_type(raw, &visitor)),
            };
        }

        match self.value {
            Value::Bool(value) => visitor.visit_bool(*value),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    deserialize_integer_from_value!(deserialize_i8, visit_i8, i8);
    deserialize_integer_from_value!(deserialize_i16, visit_i16, i16);
    deserialize_integer_from_value!(deserialize_i32, visit_i32, i32);
    deserialize_integer_from_value!(deserialize_i64, visit_i64, i64);
    deserialize_integer_from_value!(deserialize_i128, visit_i128, i128);
    deserialize_integer_from_value!(deserialize_u8, visit_u8, u8);
    deserialize_integer_from_value!(deserialize_u16, visit_u16, u16);
    deserialize_integer_from_value!(deserialize_u32, visit_u32, u32);
    deserialize_integer_from_value!(deserialize_u64, visit_u64, u64);
    deserialize_integer_from_value!(deserialize_u128, visit_u128, u128);
    deserialize_float_from_value!(deserialize_f32, visit_f32, f32);
    deserialize_float_from_value!(deserialize_f64, visit_f64, f64);

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let Value::String(value) = self.value else {
            return Err(self.invalid_type(&visitor));
        };
        let mut chars = value.chars();
        match (chars.next(), chars.next()) {
            (Some(ch), None) => visitor.visit_char(ch),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) => visitor.visit_borrowed_str(value),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) => visitor.visit_string(value.clone()),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) => visitor.visit_borrowed_bytes(value.as_bytes()),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) => visitor.visit_byte_buf(value.as_bytes().to_vec()),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if matches!(self.value, Value::Null) {
            return visitor.visit_none();
        }

        if let Some(raw) = self.coercible_string()
            && raw.trim() == "null"
        {
            return visitor.visit_none();
        }

        visitor.visit_some(self)
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if matches!(self.value, Value::Null) {
            return visitor.visit_unit();
        }

        if let Some(raw) = self.coercible_string()
            && raw.trim() == "null"
        {
            return visitor.visit_unit();
        }

        Err(self.invalid_type(&visitor))
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Array(values) => visitor.visit_seq(CoercingSeqAccess::new(
                values.iter().enumerate(),
                self.path,
                self.string_coercion_paths,
            )),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Object(map) => visitor.visit_map(CoercingMapAccess::new(
                map.iter(),
                self.path,
                self.string_coercion_paths,
            )),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) => visitor.visit_enum(value.as_str().into_deserializer()),
            Value::Object(map) => visitor.visit_enum(MapAccessDeserializer::new(
                CoercingMapAccess::new(map.iter(), self.path, self.string_coercion_paths),
            )),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_string(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }
}

struct CoercingSeqAccess<'a, I> {
    iter: I,
    parent_path: String,
    string_coercion_paths: &'a BTreeSet<String>,
}

impl<'a, I> CoercingSeqAccess<'a, I> {
    fn new(iter: I, parent_path: String, string_coercion_paths: &'a BTreeSet<String>) -> Self {
        Self {
            iter,
            parent_path,
            string_coercion_paths,
        }
    }
}

impl<'de, 'a, I> SeqAccess<'de> for CoercingSeqAccess<'a, I>
where
    'a: 'de,
    I: Iterator<Item = (usize, &'a Value)>,
{
    type Error = ValueDeError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        let Some((index, value)) = self.iter.next() else {
            return Ok(None);
        };
        let path = join_path(&self.parent_path, &index.to_string());
        seed.deserialize(CoercingDeserializer::new(
            value,
            path,
            self.string_coercion_paths,
        ))
        .map(Some)
    }
}

struct CoercingMapAccess<'a, I> {
    iter: I,
    current: Option<(&'a str, &'a Value)>,
    parent_path: String,
    string_coercion_paths: &'a BTreeSet<String>,
}

impl<'a, I> CoercingMapAccess<'a, I> {
    fn new(iter: I, parent_path: String, string_coercion_paths: &'a BTreeSet<String>) -> Self {
        Self {
            iter,
            current: None,
            parent_path,
            string_coercion_paths,
        }
    }
}

impl<'de, 'a, I> MapAccess<'de> for CoercingMapAccess<'a, I>
where
    'a: 'de,
    I: Iterator<Item = (&'a String, &'a Value)>,
{
    type Error = ValueDeError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        let Some((key, value)) = self.iter.next() else {
            return Ok(None);
        };
        self.current = Some((key.as_str(), value));
        seed.deserialize(key.as_str().into_deserializer()).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        let (key, value) = self
            .current
            .take()
            .expect("map value requested before key was deserialized");
        let path = join_path(&self.parent_path, key);
        seed.deserialize(CoercingDeserializer::new(
            value,
            path,
            self.string_coercion_paths,
        ))
    }
}

fn insert_path(root: &mut Value, segments: &[&str], value: Value) -> Result<(), String> {
    if segments.is_empty() {
        return Err("configuration path cannot be empty".to_owned());
    }

    let mut current = root;
    for (index, segment) in segments.iter().enumerate() {
        if segment.is_empty() {
            return Err("configuration path contains an empty segment".to_owned());
        }

        let is_last = index == segments.len() - 1;
        match current {
            Value::Object(map) if is_last => {
                map.insert((*segment).to_owned(), value);
                return Ok(());
            }
            Value::Object(map) => {
                current = map
                    .entry((*segment).to_owned())
                    .or_insert_with(|| Value::Object(Map::new()));
                if !matches!(current, Value::Object(_)) {
                    return Err(format!(
                        "path segment {segment} conflicts with an existing non-object value"
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "path segment {segment} conflicts with an existing non-object value"
                ));
            }
        }
    }

    Ok(())
}
