use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
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
    ConfigReport, ConfigWarning, DeprecatedField, ResolutionStep, canonicalize_path_with_aliases,
    collect_diff_paths, collect_paths, get_value_at_path, join_path, normalize_path,
    path_matches_pattern, path_overlaps_pattern, path_starts_with_pattern, redact_value,
};
use crate::{
    ConfigMetadata, EnvDecoder, MergeStrategy, TierMetadata, TierPatch, ValidationCheck,
    ValidationRule,
};

mod canonical;
mod env;
mod unknown;

use self::canonical::*;
use self::unknown::*;

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
///
/// `FileSource` is useful when you need more control than
/// [`ConfigLoader::file`] or [`ConfigLoader::optional_file`] provide, such as
/// candidate-path search or explicit format selection.
///
/// # Examples
///
/// ```no_run
/// use serde::{Deserialize, Serialize};
/// use tier::{ConfigLoader, FileFormat, FileSource};
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct AppConfig {
///     port: u16,
/// }
///
/// impl Default for AppConfig {
///     fn default() -> Self {
///         Self { port: 3000 }
///     }
/// }
///
/// let loaded = ConfigLoader::new(AppConfig::default())
///     .with_file(
///         FileSource::search(["config/local", "config/default.toml"]).format(FileFormat::Toml),
///     )
///     .load()?;
///
/// assert!(loaded.port >= 1);
/// # Ok::<(), tier::ConfigError>(())
/// ```
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
///
/// Use `EnvSource` when environment variables should participate in the same
/// layered pipeline as defaults and files.
///
/// # Examples
///
/// ```
/// use serde::{Deserialize, Serialize};
/// use tier::{ConfigLoader, EnvSource};
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct AppConfig {
///     server: ServerConfig,
/// }
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct ServerConfig {
///     port: u16,
/// }
///
/// impl Default for AppConfig {
///     fn default() -> Self {
///         Self {
///             server: ServerConfig { port: 3000 },
///         }
///     }
/// }
///
/// let loaded = ConfigLoader::new(AppConfig::default())
///     .env(EnvSource::from_pairs([("APP__SERVER__PORT", "7000")]).prefix("APP"))
///     .load()?;
///
/// assert_eq!(loaded.server.port, 7000);
/// # Ok::<(), tier::ConfigError>(())
/// ```
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
        let separator = separator.into();
        if !separator.is_empty() {
            self.separator = separator;
        }
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
///
/// `ArgsSource` parses the same `--config`, `--profile`, and `--set key=value`
/// flags that `tier` accepts through its reusable `clap` integration.
///
/// # Examples
///
/// ```
/// use serde::{Deserialize, Serialize};
/// use tier::{ArgsSource, ConfigLoader};
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct AppConfig {
///     port: u16,
/// }
///
/// impl Default for AppConfig {
///     fn default() -> Self {
///         Self { port: 3000 }
///     }
/// }
///
/// let loaded = ConfigLoader::new(AppConfig::default())
///     .args(ArgsSource::from_args(["app", "--set", "port=7000"]))
///     .load()?;
///
/// assert_eq!(loaded.port, 7000);
/// # Ok::<(), tier::ConfigError>(())
/// ```
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
    coercible_string_paths: BTreeSet<String>,
    indexed_array_paths: BTreeSet<String>,
    indexed_array_base_lengths: BTreeMap<String, usize>,
    direct_array_paths: BTreeSet<String>,
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
        ensure_path_safe_keys(&value, "")?;

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
            coercible_string_paths: BTreeSet::new(),
            indexed_array_paths: BTreeSet::new(),
            indexed_array_base_lengths: BTreeMap::new(),
            direct_array_paths: BTreeSet::new(),
        })
    }

    pub(crate) fn from_parts(
        trace: SourceTrace,
        value: Value,
        entries: BTreeMap<String, SourceTrace>,
        coercible_string_paths: BTreeSet<String>,
        indexed_array_paths: BTreeSet<String>,
        indexed_array_base_lengths: BTreeMap<String, usize>,
        direct_array_paths: BTreeSet<String>,
    ) -> Self {
        Self {
            trace,
            value,
            entries,
            coercible_string_paths,
            indexed_array_paths,
            indexed_array_base_lengths,
            direct_array_paths,
        }
    }

    /// Creates a custom configuration layer from a typed sparse patch.
    ///
    /// This is the typed alternative to manually building a [`Layer`] from a
    /// serializable shadow struct.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # #[cfg(feature = "derive")] {
    /// # fn main() -> Result<(), tier::ConfigError> {
    /// use tier::{Layer, TierPatch};
    ///
    /// #[derive(Debug, TierPatch, Default)]
    /// struct CliPatch {
    ///     port: Option<u16>,
    /// }
    ///
    /// let _layer = Layer::from_patch("typed-cli", &CliPatch { port: Some(7000) })?;
    /// # Ok(())
    /// # }
    /// # }
    /// ```
    pub fn from_patch<P>(name: impl Into<String>, patch: &P) -> Result<Self, ConfigError>
    where
        P: TierPatch,
    {
        Self::from_patch_with_trace(
            SourceTrace {
                kind: SourceKind::Custom,
                name: name.into(),
                location: None,
            },
            patch,
        )
    }

    pub(crate) fn from_patch_with_trace<P>(
        trace: SourceTrace,
        patch: &P,
    ) -> Result<Self, ConfigError>
    where
        P: TierPatch,
    {
        let mut builder = crate::patch::PatchLayerBuilder::from_trace(trace);
        patch.write_layer(&mut builder, "")?;
        Ok(builder.finish())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
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

    /// Registers a built-in environment decoder for a configuration path.
    ///
    /// This is useful for operational formats such as comma-separated lists or
    /// `key=value` maps that are common in environment variables but awkward to
    /// express as JSON.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> Result<(), tier::ConfigError> {
    /// use serde::{Deserialize, Serialize};
    /// use tier::{ConfigLoader, EnvDecoder, EnvSource};
    ///
    /// #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    /// struct AppConfig {
    ///     no_proxy: Vec<String>,
    /// }
    ///
    /// let loaded = ConfigLoader::new(AppConfig { no_proxy: Vec::new() })
    ///     .env_decoder("no_proxy", EnvDecoder::Csv)
    ///     .env(EnvSource::from_pairs([(
    ///         "APP__NO_PROXY",
    ///         "localhost,127.0.0.1,.internal.example.com",
    ///     )]).prefix("APP"))
    ///     .load()?;
    ///
    /// assert_eq!(
    ///     loaded.no_proxy,
    ///     vec![
    ///         "localhost".to_owned(),
    ///         "127.0.0.1".to_owned(),
    ///         ".internal.example.com".to_owned(),
    ///     ]
    /// );
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn env_decoder(mut self, path: impl Into<String>, decoder: EnvDecoder) -> Self {
        self.metadata
            .push(crate::FieldMetadata::new(path).env_decoder(decoder));
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

    /// Adds a typed sparse patch as a custom layer.
    ///
    /// This keeps sparse overrides typed and avoids maintaining a parallel
    /// serializable shadow hierarchy just to build a [`Layer`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # #[cfg(feature = "derive")] {
    /// # fn main() -> Result<(), tier::ConfigError> {
    /// use serde::{Deserialize, Serialize};
    /// use tier::{ConfigLoader, TierPatch};
    ///
    /// #[derive(Debug, Clone, Serialize, Deserialize)]
    /// struct AppConfig {
    ///     port: u16,
    /// }
    ///
    /// #[derive(Debug, TierPatch, Default)]
    /// struct CliPatch {
    ///     port: Option<u16>,
    /// }
    ///
    /// let loaded = ConfigLoader::new(AppConfig { port: 3000 })
    ///     .patch("typed-cli", &CliPatch { port: Some(7000) })?
    ///     .load()?;
    ///
    /// assert_eq!(loaded.port, 7000);
    /// # Ok(())
    /// # }
    /// # }
    /// ```
    pub fn patch<P>(mut self, name: impl Into<String>, patch: &P) -> Result<Self, ConfigError>
    where
        P: TierPatch,
    {
        let layer = Layer::from_patch(name, patch)?;
        if !layer.is_empty() {
            self.custom_layers.push(layer);
        }
        Ok(self)
    }

    #[cfg(feature = "clap")]
    /// Adds a typed `clap`-style sparse override struct as the last CLI layer.
    ///
    /// This is the ergonomic bridge for applications that already parse a
    /// typed `clap` CLI and want to feed the parsed values into `tier` without
    /// building a manual shadow patch struct.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # #[cfg(all(feature = "derive", feature = "clap"))] {
    /// # fn main() -> Result<(), tier::ConfigError> {
    /// use clap::Parser;
    /// use serde::{Deserialize, Serialize};
    /// use tier::{ConfigLoader, TierPatch};
    ///
    /// #[derive(Debug, Clone, Serialize, Deserialize)]
    /// struct AppConfig {
    ///     port: u16,
    /// }
    ///
    /// #[derive(Debug, Parser, TierPatch)]
    /// struct AppCli {
    ///     #[arg(long)]
    ///     port: Option<u16>,
    /// }
    ///
    /// let cli = AppCli::parse_from(["app", "--port", "8080"]);
    /// let loaded = ConfigLoader::new(AppConfig { port: 3000 })
    ///     .clap_overrides(&cli)?
    ///     .load()?;
    ///
    /// assert_eq!(loaded.port, 8080);
    /// # Ok(())
    /// # }
    /// # }
    /// ```
    pub fn clap_overrides<P>(mut self, patch: &P) -> Result<Self, ConfigError>
    where
        P: TierPatch,
    {
        let layer = Layer::from_patch_with_trace(
            SourceTrace {
                kind: SourceKind::Arguments,
                name: "typed-clap".to_owned(),
                location: None,
            },
            patch,
        )?;
        if !layer.is_empty() {
            self.custom_layers.push(layer);
        }
        Ok(self)
    }

    /// Marks a dot-delimited path as sensitive for report redaction.
    #[must_use]
    pub fn secret_path(mut self, path: impl Into<String>) -> Self {
        let path = normalize_path(&path.into());
        if !path.is_empty() {
            self.secret_paths.insert(path);
        }
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

        let mut metadata = canonicalize_metadata_against_layers(&metadata, &layers)?;
        let mut alias_overrides = metadata.alias_overrides()?;
        let pending_secret_paths = canonicalize_secret_paths(&self.secret_paths, &alias_overrides);
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
            validate_declared_rules(&normalized_value, &metadata, &secret_paths);
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

fn direct_child_array_index(container_path: &str, entry_path: &str) -> Option<usize> {
    let remainder = if container_path.is_empty() {
        entry_path
    } else {
        entry_path.strip_prefix(container_path)?.strip_prefix('.')?
    };
    remainder.split('.').next()?.parse::<usize>().ok()
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
        "sparse array override at `{container_path}`: index {offending_index} requires index {expected_index} to be provided first"
    );

    match source.map(|(_, trace)| trace) {
        Some(trace) => match trace.kind {
            SourceKind::Environment => ConfigError::InvalidEnv {
                name: trace.name.clone(),
                path: offending_path,
                message,
            },
            SourceKind::Arguments => ConfigError::InvalidArg {
                arg: trace.name.clone(),
                message,
            },
            _ => ConfigError::InvalidArg {
                arg: offending_path,
                message,
            },
        },
        None => ConfigError::InvalidArg {
            arg: offending_path,
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

fn is_secret_path(secret_paths: &BTreeSet<String>, path: &str) -> bool {
    secret_paths
        .iter()
        .any(|secret| path_starts_with_pattern(path, secret))
}

fn path_contains_secret(secret_paths: &BTreeSet<String>, path: &str) -> bool {
    secret_paths
        .iter()
        .any(|secret| path_overlaps_pattern(path, secret))
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
        let matches = collect_matching_values(value, &field.path);
        if matches.is_empty() {
            continue;
        }
        for rule in &field.validations {
            for (matched_path, actual) in &matches {
                if let Some(error) =
                    validate_declared_rule(matched_path, actual, rule, secret_paths)
                {
                    errors.push(error);
                }
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
                for (matched_path, _) in collect_matching_values(value, path)
                    .into_iter()
                    .filter(|(_, actual)| is_present_value(actual))
                {
                    let bound_requires = bind_required_paths(path, &matched_path, requires)
                        .unwrap_or_else(|| requires.to_vec());
                    let missing = missing_paths(value, &bound_requires);
                    if !missing.is_empty() {
                        errors.push(group_validation_error(
                            check,
                            std::iter::once(matched_path.as_str())
                                .chain(missing.iter().map(String::as_str)),
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
                for (matched_path, _) in collect_matching_values(value, path)
                    .into_iter()
                    .filter(|(_, actual)| *actual == &equals.0)
                {
                    let bound_requires = bind_required_paths(path, &matched_path, requires)
                        .unwrap_or_else(|| requires.to_vec());
                    let missing = missing_paths(value, &bound_requires);
                    if !missing.is_empty() {
                        errors.push(group_validation_error(
                            check,
                            std::iter::once(matched_path.as_str())
                                .chain(missing.iter().map(String::as_str)),
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
    let mut coercible_string_paths = BTreeSet::new();
    let mut indexed_array_paths = BTreeSet::new();
    let mut indexed_array_base_lengths = BTreeMap::new();
    let mut current_array_lengths = BTreeMap::new();
    let mut direct_array_paths = BTreeSet::new();

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

        let (raw_path, raw_value) =
            set_value
                .split_once('=')
                .ok_or_else(|| ConfigError::InvalidArg {
                    arg: set_value.clone(),
                    message: "expected key=value".to_owned(),
                })?;
        let path =
            try_normalize_external_path(raw_path).map_err(|message| ConfigError::InvalidArg {
                arg: format!("--set {raw_path}={raw_value}"),
                message,
            })?;
        if path.is_empty() {
            return Err(ConfigError::InvalidArg {
                arg: set_value,
                message: "configuration path cannot be empty".to_owned(),
            });
        }

        let segments = path.split('.').collect::<Vec<_>>();
        let parsed =
            parse_override_value(raw_value).map_err(|message| ConfigError::InvalidArg {
                arg: format!("--set {path}={raw_value}"),
                message,
            })?;
        let is_direct_array = parsed.value.is_array();
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
            ConfigError::InvalidArg {
                arg: format!("--set {path}={raw_value}"),
                message,
            }
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

        let arg_trace_name = format!("--set {raw_path}={raw_value}");
        entries.insert(
            path.clone(),
            SourceTrace::new(SourceKind::Arguments, arg_trace_name.clone()),
        );

        let mut prefix = String::new();
        for segment in segments {
            if !prefix.is_empty() {
                prefix.push('.');
            }
            prefix.push_str(segment);
            let entry = entries
                .entry(prefix.clone())
                .or_insert_with(|| SourceTrace::new(SourceKind::Arguments, arg_trace_name.clone()));
            if prefix != path && entry.name != arg_trace_name {
                *entry = SourceTrace::new(SourceKind::Arguments, "arguments");
            }
        }
    }

    let layer = if entries.is_empty() {
        None
    } else {
        Some(Layer {
            trace: SourceTrace::new(SourceKind::Arguments, "arguments"),
            value: root,
            entries,
            coercible_string_paths,
            indexed_array_paths,
            indexed_array_base_lengths,
            direct_array_paths,
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
        visited_refs.remove(reference);
    }

    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        for (key, child) in properties {
            let next = crate::report::join_path(current, key);
            collect_secret_paths_from_schema(child, root, &next, paths, visited_refs);
        }
    }

    if let Some(items) = object.get("prefixItems").and_then(Value::as_array) {
        for (index, child) in items.iter().enumerate() {
            let next = crate::report::join_path(current, &index.to_string());
            collect_secret_paths_from_schema(child, root, &next, paths, visited_refs);
        }
    }

    if let Some(items) = object.get("items").and_then(Value::as_array) {
        for (index, child) in items.iter().enumerate() {
            let next = crate::report::join_path(current, &index.to_string());
            collect_secret_paths_from_schema(child, root, &next, paths, visited_refs);
        }
    }

    if let Some(items) = object.get("items") {
        let next = crate::report::join_path(current, "*");
        collect_secret_paths_from_schema(items, root, &next, paths, visited_refs);
    }

    if let Some(additional) = object
        .get("additionalProperties")
        .filter(|value| value.is_object())
    {
        let next = crate::report::join_path(current, "*");
        collect_secret_paths_from_schema(additional, root, &next, paths, visited_refs);
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
    let deserialize_attempt = |value: &Value| {
        let deserializer = CoercingDeserializer::new(value, "", string_coercion_paths, None, None);
        let result: Result<T, serde_path_to_error::Error<ValueDeError>> =
            serde_path_to_error::deserialize(deserializer);
        result
    };

    match deserialize_attempt(value) {
        Ok(config) => Ok(config),
        Err(error) => {
            let retry_value = coerce_retry_scalars(value, "", string_coercion_paths);
            if retry_value != *value
                && let Ok(config) = deserialize_attempt(&retry_value)
            {
                return Ok(config);
            }
            Err(deserialization_error(report, error))
        }
    }
}

fn deserialization_error(
    report: &ConfigReport,
    error: serde_path_to_error::Error<ValueDeError>,
) -> ConfigError {
    let path = error.path().to_string();
    let lookup_path = normalize_external_path(&path);
    let source = find_source_for_unknown_path(report, &lookup_path);
    ConfigError::Deserialize {
        path,
        provenance: source,
        message: error.inner().to_string(),
    }
}

fn coerce_retry_scalars(
    value: &Value,
    current_path: &str,
    string_coercion_paths: &BTreeSet<String>,
) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, child)| {
                    let next = join_path(current_path, key);
                    (
                        key.clone(),
                        coerce_retry_scalars(child, &next, string_coercion_paths),
                    )
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .enumerate()
                .map(|(index, child)| {
                    let next = join_path(current_path, &index.to_string());
                    coerce_retry_scalars(child, &next, string_coercion_paths)
                })
                .collect(),
        ),
        Value::String(raw) if string_coercion_paths.contains(current_path) => {
            retry_scalar_value(raw).unwrap_or_else(|| Value::String(raw.clone()))
        }
        other => other.clone(),
    }
}

fn retry_scalar_value(raw: &str) -> Option<Value> {
    let value = serde_json::from_str::<Value>(raw.trim()).ok()?;
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => Some(value),
        _ => None,
    }
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

fn ensure_path_safe_keys(value: &Value, current_path: &str) -> Result<(), ConfigError> {
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

fn invalid_path_key_message(key: &str) -> Option<String> {
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
    metadata: &ConfigMetadata,
    indexed_array_paths: &BTreeSet<String>,
    direct_array_paths: &BTreeSet<String>,
) {
    let strategy = metadata
        .merge_strategy_for(current_path)
        .unwrap_or(MergeStrategy::Merge);
    let indexed_array_patch =
        indexed_array_paths.contains(current_path) && !direct_array_paths.contains(current_path);

    match (target, overlay, strategy) {
        (Value::Array(target), Value::Array(overlay), _)
            if indexed_array_patch && !current_path.is_empty() =>
        {
            merge_indexed_array_patch(
                target,
                overlay,
                current_path,
                metadata,
                indexed_array_paths,
                direct_array_paths,
            );
        }
        (target, overlay, MergeStrategy::Replace) if !current_path.is_empty() => *target = overlay,
        (target, overlay, MergeStrategy::Append) => match (target, overlay) {
            (Value::Array(target), Value::Array(mut overlay)) => target.append(&mut overlay),
            (Value::Object(target), Value::Object(overlay)) => {
                for (key, value) in overlay {
                    let path = join_path(current_path, &key);
                    match target.get_mut(&key) {
                        Some(existing) => merge_values(
                            existing,
                            value,
                            &path,
                            metadata,
                            indexed_array_paths,
                            direct_array_paths,
                        ),
                        None => {
                            target.insert(key, value);
                        }
                    }
                }
            }
            (target, overlay) => *target = overlay,
        },
        (target, overlay, MergeStrategy::Merge | MergeStrategy::Replace) => match (target, overlay)
        {
            (Value::Object(target), Value::Object(overlay)) => {
                for (key, value) in overlay {
                    let path = join_path(current_path, &key);
                    match target.get_mut(&key) {
                        Some(existing) => merge_values(
                            existing,
                            value,
                            &path,
                            metadata,
                            indexed_array_paths,
                            direct_array_paths,
                        ),
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

fn merge_indexed_array_patch(
    target: &mut Vec<Value>,
    overlay: Vec<Value>,
    current_path: &str,
    metadata: &ConfigMetadata,
    indexed_array_paths: &BTreeSet<String>,
    direct_array_paths: &BTreeSet<String>,
) {
    for (index, value) in overlay.into_iter().enumerate() {
        if value.is_null() {
            continue;
        }

        let path = join_path(current_path, &index.to_string());
        if target.len() <= index {
            target.resize(index + 1, Value::Null);
        }

        if target[index].is_null() {
            target[index] = value;
            continue;
        }

        merge_values(
            &mut target[index],
            value,
            &path,
            metadata,
            indexed_array_paths,
            direct_array_paths,
        );
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

struct ParsedOverride {
    value: Value,
    string_coercion_suffixes: BTreeSet<String>,
}

fn parse_override_value(raw: &str) -> Result<ParsedOverride, String> {
    if raw.is_empty() {
        return Ok(ParsedOverride {
            value: Value::String(String::new()),
            string_coercion_suffixes: BTreeSet::from([String::new()]),
        });
    }

    let trimmed = raw.trim();

    let uses_explicit_json_syntax =
        matches!(trimmed.chars().next(), Some('{') | Some('[') | Some('"'));

    if uses_explicit_json_syntax {
        let value = serde_json::from_str::<Value>(trimmed)
            .map_err(|error| format!("invalid explicit JSON override: {error}"))?;
        return Ok(ParsedOverride {
            value,
            string_coercion_suffixes: BTreeSet::new(),
        });
    }

    Ok(ParsedOverride {
        value: Value::String(raw.to_owned()),
        string_coercion_suffixes: BTreeSet::from([String::new()]),
    })
}

fn parse_env_override_value(
    raw: &str,
    decoder: Option<EnvDecoder>,
) -> Result<ParsedOverride, String> {
    match decoder {
        Some(decoder) => {
            let value = decode_env_override_value(raw, decoder)?;
            Ok(ParsedOverride {
                string_coercion_suffixes: collect_string_leaf_suffixes(&value, ""),
                value,
            })
        }
        None => parse_override_value(raw),
    }
}

fn decode_env_override_value(raw: &str, decoder: EnvDecoder) -> Result<Value, String> {
    match decoder {
        EnvDecoder::Csv => Ok(Value::Array(
            raw.split(',')
                .map(str::trim)
                .filter(|segment| !segment.is_empty())
                .map(|segment| Value::String(segment.to_owned()))
                .collect(),
        )),
        EnvDecoder::Whitespace => Ok(Value::Array(
            raw.split_whitespace()
                .map(|segment| Value::String(segment.to_owned()))
                .collect(),
        )),
        EnvDecoder::PathList => {
            let values = std::env::split_paths(OsStr::new(raw))
                .map(|path| Value::String(path.to_string_lossy().into_owned()))
                .collect();
            Ok(Value::Array(values))
        }
        EnvDecoder::KeyValueMap => {
            let mut map = Map::new();
            for entry in raw
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
            {
                let (key, value) = entry.split_once('=').ok_or_else(|| {
                    format!("invalid key_value_map entry `{entry}`, expected key=value")
                })?;
                let key = key.trim();
                let value = value.trim();
                if key.is_empty() {
                    return Err("key_value_map entries must not use an empty key".to_owned());
                }
                map.insert(key.to_owned(), Value::String(value.to_owned()));
            }
            Ok(Value::Object(map))
        }
    }
}

fn collect_string_leaf_suffixes(value: &Value, prefix: &str) -> BTreeSet<String> {
    let mut suffixes = BTreeSet::new();
    collect_string_leaf_suffixes_inner(value, prefix, &mut suffixes);
    suffixes
}

fn collect_string_leaf_suffixes_inner(
    value: &Value,
    prefix: &str,
    suffixes: &mut BTreeSet<String>,
) {
    match value {
        Value::String(_) => {
            suffixes.insert(prefix.to_owned());
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                let next = join_path(prefix, &index.to_string());
                collect_string_leaf_suffixes_inner(value, &next, suffixes);
            }
        }
        Value::Object(map) => {
            for (key, value) in map {
                let next = join_path(prefix, key);
                collect_string_leaf_suffixes_inner(value, &next, suffixes);
            }
        }
        _ => {}
    }
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
    known_paths: Option<&'a RefCell<BTreeSet<String>>>,
    ignored_paths: Option<&'a RefCell<Vec<String>>>,
}

impl<'a> CoercingDeserializer<'a> {
    fn new(
        value: &'a Value,
        path: impl Into<String>,
        string_coercion_paths: &'a BTreeSet<String>,
        known_paths: Option<&'a RefCell<BTreeSet<String>>>,
        ignored_paths: Option<&'a RefCell<Vec<String>>>,
    ) -> Self {
        Self {
            value,
            path: path.into(),
            string_coercion_paths,
            known_paths,
            ignored_paths,
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

    fn record_known_path(&self, path: &str) {
        if let Some(known_paths) = self.known_paths {
            let normalized = normalize_path(path);
            if !normalized.is_empty() {
                known_paths.borrow_mut().insert(normalized);
            }
        }
    }

    fn record_ignored_path(&self, path: &str) {
        if let Some(ignored_paths) = self.ignored_paths {
            let normalized = normalize_path(path);
            if !normalized.is_empty() {
                ignored_paths.borrow_mut().push(normalized);
            }
        }
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
                self.known_paths,
                self.ignored_paths,
            )),
            Value::Object(map) => visitor.visit_map(CoercingMapAccess::new(
                map.iter(),
                self.path,
                self.string_coercion_paths,
                self.known_paths,
                self.ignored_paths,
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
                self.known_paths,
                self.ignored_paths,
            )),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if let Value::Array(values) = self.value {
            for index in 0.._len {
                self.record_known_path(&join_path(&self.path, &index.to_string()));
            }
            for index in _len..values.len() {
                self.record_ignored_path(&join_path(&self.path, &index.to_string()));
            }
        }
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
        self.deserialize_tuple(_len, visitor)
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
                self.known_paths,
                self.ignored_paths,
            )),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        for field in fields {
            self.record_known_path(&join_path(&self.path, field));
        }
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
            Value::Object(map) => {
                visitor.visit_enum(MapAccessDeserializer::new(CoercingMapAccess::new(
                    map.iter(),
                    self.path,
                    self.string_coercion_paths,
                    self.known_paths,
                    self.ignored_paths,
                )))
            }
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
    known_paths: Option<&'a RefCell<BTreeSet<String>>>,
    ignored_paths: Option<&'a RefCell<Vec<String>>>,
}

impl<'a, I> CoercingSeqAccess<'a, I> {
    fn new(
        iter: I,
        parent_path: String,
        string_coercion_paths: &'a BTreeSet<String>,
        known_paths: Option<&'a RefCell<BTreeSet<String>>>,
        ignored_paths: Option<&'a RefCell<Vec<String>>>,
    ) -> Self {
        Self {
            iter,
            parent_path,
            string_coercion_paths,
            known_paths,
            ignored_paths,
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
            self.known_paths,
            self.ignored_paths,
        ))
        .map(Some)
    }
}

struct CoercingMapAccess<'a, I> {
    iter: I,
    current: Option<(&'a str, &'a Value)>,
    parent_path: String,
    string_coercion_paths: &'a BTreeSet<String>,
    known_paths: Option<&'a RefCell<BTreeSet<String>>>,
    ignored_paths: Option<&'a RefCell<Vec<String>>>,
}

impl<'a, I> CoercingMapAccess<'a, I> {
    fn new(
        iter: I,
        parent_path: String,
        string_coercion_paths: &'a BTreeSet<String>,
        known_paths: Option<&'a RefCell<BTreeSet<String>>>,
        ignored_paths: Option<&'a RefCell<Vec<String>>>,
    ) -> Self {
        Self {
            iter,
            current: None,
            parent_path,
            string_coercion_paths,
            known_paths,
            ignored_paths,
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
            self.known_paths,
            self.ignored_paths,
        ))
    }
}

pub(crate) fn insert_path(root: &mut Value, segments: &[&str], value: Value) -> Result<(), String> {
    if segments.is_empty() {
        return Err("configuration path cannot be empty".to_owned());
    }

    insert_path_recursive(root, segments, value)
}

fn insert_path_recursive(
    current: &mut Value,
    segments: &[&str],
    value: Value,
) -> Result<(), String> {
    let segment = segments[0];
    if segment.is_empty() {
        return Err("configuration path contains an empty segment".to_owned());
    }

    let is_last = segments.len() == 1;
    match current {
        Value::Object(map) => {
            if is_last {
                map.insert(segment.to_owned(), value);
                return Ok(());
            }

            let next_is_index = segments[1].parse::<usize>().is_ok();
            let child = map.entry(segment.to_owned()).or_insert_with(|| {
                if next_is_index {
                    Value::Array(Vec::new())
                } else {
                    Value::Object(Map::new())
                }
            });

            match child {
                Value::Object(_) if !next_is_index => {}
                Value::Array(_) if next_is_index => {}
                _ => {
                    return Err(format!(
                        "path segment {segment} conflicts with an existing non-container value"
                    ));
                }
            }

            insert_path_recursive(child, &segments[1..], value)
        }
        Value::Array(values) => {
            let index = segment.parse::<usize>().map_err(|_| {
                format!("path segment {segment} must be an array index at this position")
            })?;

            if is_last {
                if values.len() <= index {
                    values.resize(index + 1, Value::Null);
                }
                values[index] = value;
                return Ok(());
            }

            let next_is_index = segments[1].parse::<usize>().is_ok();
            if values.len() <= index {
                values.resize_with(index + 1, || {
                    if next_is_index {
                        Value::Array(Vec::new())
                    } else {
                        Value::Object(Map::new())
                    }
                });
            }

            let child = &mut values[index];
            if child.is_null() {
                *child = if next_is_index {
                    Value::Array(Vec::new())
                } else {
                    Value::Object(Map::new())
                };
            }

            match child {
                Value::Object(_) if !next_is_index => {}
                Value::Array(_) if next_is_index => {}
                _ => {
                    return Err(format!(
                        "path segment {segment} conflicts with an existing non-container value"
                    ));
                }
            }

            insert_path_recursive(child, &segments[1..], value)
        }
        _ => Err(format!(
            "path segment {segment} conflicts with an existing non-container value"
        )),
    }
}
