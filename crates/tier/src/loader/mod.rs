use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt::{self, Display, Formatter};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use regex::Regex;
use serde::Serialize;
use serde::de::{
    DeserializeOwned, IntoDeserializer, MapAccess, SeqAccess, Visitor,
    value::{Error as ValueDeError, MapAccessDeserializer},
};
use serde_json::{Map, Value};

#[cfg(any(feature = "json", feature = "toml", feature = "yaml"))]
use crate::error::LineColumn;
use crate::error::{ConfigError, UnknownField, ValidationErrors};
#[cfg(feature = "schema")]
use crate::export::{json_pretty, json_value};
use crate::patch::DeferredPatchLayer;
use crate::report::{
    AppliedMigration, ConfigReport, ConfigWarning, DeprecatedField, ResolutionStep,
    canonicalize_path_with_aliases, collect_diff_paths, collect_paths, get_value_at_path,
    join_path, normalize_path, path_matches_pattern, path_overlaps_pattern,
    path_starts_with_pattern, redact_value,
};
use crate::{ConfigMetadata, EnvDecoder, MergeStrategy, TierMetadata, TierPatch};

mod canonical;
mod de;
mod env;
mod load;
mod merge;
mod overrides;
mod path;
mod policy;
mod unknown;
mod validation;

use self::canonical::*;
use self::de::deserialize_with_path;
use self::merge::*;
use self::overrides::*;
use self::path::*;
use self::policy::enforce_source_policies;
use self::unknown::*;
use self::validation::{validate_declared_checks, validate_declared_rules};

pub(crate) use self::de::insert_path;
pub(crate) use self::load::is_secret_path;
pub(crate) use self::path::record_direct_array_state;

type Normalizer<T> = Box<dyn Fn(&mut T) -> Result<(), String> + Send + Sync>;
type Validator<T> = Box<dyn Fn(&T) -> Result<(), ValidationErrors> + Send + Sync>;
type CustomEnvDecoder = Arc<dyn Fn(&str) -> Result<Value, String> + Send + Sync>;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Migration action applied when upgrading older configuration payloads.
pub enum ConfigMigrationKind {
    /// Renames one configuration path to another.
    Rename {
        /// Original path used by older configs.
        from: String,
        /// Replacement path used by newer configs.
        to: String,
    },
    /// Removes a configuration path that is no longer supported.
    Remove {
        /// Path removed from newer configs.
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Declarative migration rule applied to loaded configuration values.
pub struct ConfigMigration {
    /// Version introduced by this migration rule.
    pub since_version: u32,
    /// Concrete migration action.
    pub kind: ConfigMigrationKind,
    /// Optional operator-facing migration note.
    pub note: Option<String>,
}

impl ConfigMigration {
    /// Creates a rename migration from `from` to `to`.
    #[must_use]
    pub fn rename(from: impl Into<String>, to: impl Into<String>, since_version: u32) -> Self {
        Self {
            since_version,
            kind: ConfigMigrationKind::Rename {
                from: from.into(),
                to: to.into(),
            },
            note: None,
        }
    }

    /// Creates a removal migration for `path`.
    #[must_use]
    pub fn remove(path: impl Into<String>, since_version: u32) -> Self {
        Self {
            since_version,
            kind: ConfigMigrationKind::Remove { path: path.into() },
            note: None,
        }
    }

    /// Attaches an operator-facing migration note.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
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
    bindings: BTreeMap<String, EnvBinding>,
    binding_conflicts: Vec<EnvBindingConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EnvBinding {
    path: String,
    decoder: Option<EnvDecoder>,
    fallback: bool,
}

#[derive(Debug, Clone)]
struct EnvBindingConflict {
    name: String,
    first: EnvBinding,
    second: EnvBinding,
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
            bindings: BTreeMap::new(),
            binding_conflicts: Vec::new(),
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

    /// Maps an explicit environment variable name to a configuration path.
    ///
    /// This is useful for compatibility with standard operational variables
    /// such as `HTTP_PROXY` alongside application-scoped names.
    #[must_use]
    pub fn with_alias(mut self, name: impl Into<String>, path: impl Into<String>) -> Self {
        self.insert_binding(
            name.into(),
            EnvBinding {
                path: path.into(),
                decoder: None,
                fallback: false,
            },
        );
        self
    }

    /// Maps an explicit environment variable name to a configuration path and
    /// decodes it with a built-in env decoder.
    #[must_use]
    pub fn with_alias_decoder(
        mut self,
        name: impl Into<String>,
        path: impl Into<String>,
        decoder: EnvDecoder,
    ) -> Self {
        self.insert_binding(
            name.into(),
            EnvBinding {
                path: path.into(),
                decoder: Some(decoder),
                fallback: false,
            },
        );
        self
    }

    /// Registers a lower-priority compatibility env mapping for a path.
    ///
    /// Fallback env names only apply when the same configuration path was not
    /// already written by a more specific env binding from this source.
    #[must_use]
    pub fn with_fallback(mut self, name: impl Into<String>, path: impl Into<String>) -> Self {
        self.insert_binding(
            name.into(),
            EnvBinding {
                path: path.into(),
                decoder: None,
                fallback: true,
            },
        );
        self
    }

    /// Registers a lower-priority compatibility env mapping with a built-in
    /// decoder for structured values such as `NO_PROXY`.
    #[must_use]
    pub fn with_fallback_decoder(
        mut self,
        name: impl Into<String>,
        path: impl Into<String>,
        decoder: EnvDecoder,
    ) -> Self {
        self.insert_binding(
            name.into(),
            EnvBinding {
                path: path.into(),
                decoder: Some(decoder),
                fallback: true,
            },
        );
        self
    }

    fn insert_binding(&mut self, name: String, binding: EnvBinding) {
        if let Some(existing) = self.bindings.get(&name) {
            if existing != &binding {
                self.binding_conflicts.push(EnvBindingConflict {
                    name: name.clone(),
                    first: existing.clone(),
                    second: binding,
                });
            }
            return;
        }

        self.bindings.insert(name, binding);
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
}

struct NamedNormalizer<T> {
    name: String,
    run: Normalizer<T>,
}

struct NamedValidator<T> {
    name: String,
    run: Validator<T>,
}

enum PendingCustomLayer {
    Immediate(Layer),
    DeferredPatch(DeferredPatchLayer),
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

#[cfg(feature = "schema")]
impl<T> LoadedConfig<T>
where
    T: Serialize + DeserializeOwned + crate::JsonSchema + crate::TierMetadata,
{
    /// Builds a versioned machine-readable export bundle for downstream tools.
    #[must_use]
    pub fn export_bundle(&self, options: &crate::EnvDocOptions) -> crate::ExportBundleReport {
        crate::ExportBundleReport {
            format_version: crate::EXPORT_BUNDLE_FORMAT_VERSION,
            doctor: self.report.doctor_report(),
            audit: self.report.audit_report(),
            env_docs: crate::env_docs_report::<T>(options),
            json_schema: crate::json_schema_report::<T>(),
            annotated_json_schema: crate::annotated_json_schema_report::<T>(),
            example: crate::config_example_report::<T>(),
        }
    }

    /// Renders the versioned export bundle as JSON.
    #[must_use]
    pub fn export_bundle_json(&self, options: &crate::EnvDocOptions) -> Value {
        json_value(
            &self.export_bundle(options),
            Value::Object(Default::default()),
        )
    }

    /// Renders the versioned export bundle as pretty JSON.
    #[must_use]
    pub fn export_bundle_json_pretty(&self, options: &crate::EnvDocOptions) -> String {
        json_pretty(
            &self.export_bundle_json(options),
            "{\"error\":\"failed to render export bundle\"}",
        )
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
    custom_layers: Vec<PendingCustomLayer>,
    typed_arg_layers: Vec<DeferredPatchLayer>,
    metadata: ConfigMetadata,
    secret_paths: BTreeSet<String>,
    normalizers: Vec<NamedNormalizer<T>>,
    validators: Vec<NamedValidator<T>>,
    profile: Option<String>,
    unknown_field_policy: UnknownFieldPolicy,
    env_decoders: BTreeMap<String, EnvDecoder>,
    custom_env_decoders: BTreeMap<String, CustomEnvDecoder>,
    config_version: Option<(String, u32)>,
    migrations: Vec<ConfigMigration>,
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
            typed_arg_layers: Vec::new(),
            metadata: ConfigMetadata::default(),
            secret_paths: BTreeSet::new(),
            normalizers: Vec::new(),
            validators: Vec::new(),
            profile: None,
            unknown_field_policy: UnknownFieldPolicy::Deny,
            env_decoders: BTreeMap::new(),
            custom_env_decoders: BTreeMap::new(),
            config_version: None,
            migrations: Vec::new(),
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
        let path = path.into();
        self.env_decoders.insert(path, decoder);
        self
    }

    /// Registers a custom environment decoder for a configuration path.
    ///
    /// This keeps application-specific env parsing inside `tier` without
    /// requiring pre-normalization before building an [`EnvSource`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> Result<(), tier::ConfigError> {
    /// use serde::{Deserialize, Serialize};
    /// use serde_json::Value;
    /// use tier::{ConfigLoader, EnvSource};
    ///
    /// #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    /// struct AppConfig {
    ///     no_proxy: Vec<String>,
    /// }
    ///
    /// let loaded = ConfigLoader::new(AppConfig { no_proxy: Vec::new() })
    ///     .env_decoder_with("no_proxy", |raw| {
    ///         Ok(Value::Array(
    ///             raw.split(';')
    ///                 .map(str::trim)
    ///                 .filter(|segment| !segment.is_empty())
    ///                 .map(|segment| Value::String(segment.to_owned()))
    ///                 .collect(),
    ///         ))
    ///     })
    ///     .env(EnvSource::from_pairs([("APP__NO_PROXY", "localhost;.internal")]).prefix("APP"))
    ///     .load()?;
    ///
    /// assert_eq!(loaded.no_proxy, vec!["localhost", ".internal"]);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn env_decoder_with<F>(mut self, path: impl Into<String>, decoder: F) -> Self
    where
        F: Fn(&str) -> Result<Value, String> + Send + Sync + 'static,
    {
        let path = path.into();
        self.custom_env_decoders.insert(path, Arc::new(decoder));
        self
    }

    /// Declares the configuration version path and the newest version this
    /// loader understands.
    #[must_use]
    pub fn config_version(mut self, path: impl Into<String>, current_version: u32) -> Self {
        self.config_version = Some((path.into(), current_version));
        self
    }

    /// Registers a migration rule applied before deserialization.
    #[must_use]
    pub fn migration(mut self, migration: ConfigMigration) -> Self {
        self.migrations.push(migration);
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
        self.custom_layers
            .push(PendingCustomLayer::Immediate(layer));
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
        let mut builder = crate::patch::PatchLayerBuilder::from_trace_deferred(SourceTrace {
            kind: SourceKind::Custom,
            name: name.into(),
            location: None,
        });
        patch.write_layer(&mut builder, "")?;
        let layer = builder.finish_deferred();
        if !layer.is_empty() {
            self.custom_layers
                .push(PendingCustomLayer::DeferredPatch(layer));
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
    /// `clap` remains responsible for CLI grammar, subcommands, trailing args,
    /// and parse-time validation. `tier` only applies the already-parsed typed
    /// values as a last-layer configuration patch.
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
    ///     token: Option<String>,
    /// }
    ///
    /// #[derive(Debug, Parser, TierPatch)]
    /// struct AppCli {
    ///     #[arg(long)]
    ///     port: Option<u16>,
    ///     #[arg(long = "db-token")]
    ///     #[tier(path_expr = tier::path!(AppConfig.token))]
    ///     token: Option<String>,
    /// }
    ///
    /// let cli = AppCli::parse_from(["app", "--port", "8080", "--db-token", "from-cli"]);
    /// let loaded = ConfigLoader::new(AppConfig {
    ///         port: 3000,
    ///         token: None,
    ///     })
    ///     .clap_overrides(&cli)?
    ///     .load()?;
    ///
    /// assert_eq!(loaded.port, 8080);
    /// assert_eq!(loaded.token.as_deref(), Some("from-cli"));
    /// # Ok(())
    /// # }
    /// # }
    /// ```
    pub fn clap_overrides<P>(mut self, patch: &P) -> Result<Self, ConfigError>
    where
        P: TierPatch,
    {
        let mut builder = crate::patch::PatchLayerBuilder::from_trace_deferred(SourceTrace {
            kind: SourceKind::Arguments,
            name: "typed-clap".to_owned(),
            location: None,
        });
        patch.write_layer(&mut builder, "")?;
        let layer = builder.finish_deferred();
        if !layer.is_empty() {
            self.typed_arg_layers.push(layer);
        }
        Ok(self)
    }

    #[cfg(feature = "clap")]
    /// Projects a parsed CLI value onto the config-bearing patch portion and
    /// applies it as the last CLI layer.
    ///
    /// This is the CLI-first companion to [`ConfigLoader::clap_overrides`].
    /// It lets an application keep a full `clap` model with subcommands,
    /// positional arguments, or trailing args, while only the selected
    /// config-bearing sub-structure participates in `tier` overrides.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # #[cfg(all(feature = "derive", feature = "clap"))] {
    /// # fn main() -> Result<(), tier::ConfigError> {
    /// use clap::{Parser, Subcommand};
    /// use serde::{Deserialize, Serialize};
    /// use tier::{ConfigLoader, TierPatch};
    ///
    /// #[derive(Debug, Clone, Serialize, Deserialize)]
    /// struct AppConfig {
    ///     port: u16,
    /// }
    ///
    /// #[derive(Debug, Clone, clap::Args, TierPatch, Default)]
    /// struct ConfigArgs {
    ///     #[arg(long)]
    ///     port: Option<u16>,
    /// }
    ///
    /// #[derive(Debug, Clone, Subcommand)]
    /// enum Command {
    ///     Serve {
    ///         #[arg(last = true)]
    ///         trailing: Vec<String>,
    ///     },
    /// }
    ///
    /// #[derive(Debug, Clone, Parser)]
    /// struct AppCli {
    ///     #[command(flatten)]
    ///     config: ConfigArgs,
    ///     #[command(subcommand)]
    ///     command: Option<Command>,
    /// }
    ///
    /// let cli = AppCli::parse_from(["app", "--port", "8080", "serve", "--", "extra"]);
    /// let loaded = ConfigLoader::new(AppConfig { port: 3000 })
    ///     .clap_overrides_from(&cli, |cli| &cli.config)?
    ///     .load()?;
    ///
    /// assert_eq!(loaded.port, 8080);
    /// # Ok(())
    /// # }
    /// # }
    /// ```
    pub fn clap_overrides_from<C, P, F>(self, cli: &C, project: F) -> Result<Self, ConfigError>
    where
        P: TierPatch,
        F: FnOnce(&C) -> &P,
    {
        self.clap_overrides(project(cli))
    }

    /// Marks a dot-delimited path as sensitive for report redaction.
    #[must_use]
    pub fn secret_path(mut self, path: impl Into<String>) -> Self {
        let path = path.into();
        if !path.is_empty() && path != "." {
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

fn is_valid_url(value: &str) -> bool {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return false;
    }

    let Some((scheme, rest)) = value.split_once(':') else {
        return false;
    };
    if !is_valid_url_scheme(scheme) || rest.is_empty() {
        return false;
    }

    if scheme == "mailto" {
        return !rest.starts_with('/')
            && has_valid_percent_escapes(rest)
            && rest
                .chars()
                .all(|ch| !ch.is_whitespace() && !ch.is_control());
    }

    if let Some(authority_and_tail) = rest.strip_prefix("//") {
        return is_valid_hierarchical_url(scheme, authority_and_tail);
    }

    if rest.starts_with('/') {
        return matches!(scheme, "file" | "unix")
            && has_valid_percent_escapes(rest)
            && rest
                .chars()
                .all(|ch| !ch.is_whitespace() && !ch.is_control());
    }

    false
}

fn is_valid_url_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

fn is_valid_hierarchical_url(scheme: &str, authority_and_tail: &str) -> bool {
    let split_at = authority_and_tail
        .find(['/', '?', '#'])
        .unwrap_or(authority_and_tail.len());
    let (authority, tail) = authority_and_tail.split_at(split_at);

    if authority.is_empty() {
        return matches!(scheme, "file" | "unix")
            && !tail.is_empty()
            && has_valid_percent_escapes(tail)
            && tail
                .chars()
                .all(|ch| !ch.is_whitespace() && !ch.is_control());
    }

    if !has_valid_percent_escapes(authority) || !has_valid_percent_escapes(tail) {
        return false;
    }

    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(userinfo, host_port)| {
            if userinfo.is_empty() || userinfo.contains('@') || !is_valid_url_userinfo(userinfo) {
                ""
            } else {
                host_port
            }
        });
    if !is_valid_url_host_port(host_port) {
        return false;
    }

    tail.chars()
        .all(|ch| !ch.is_whitespace() && !ch.is_control())
}

fn is_valid_url_host_port(host_port: &str) -> bool {
    if host_port.is_empty() {
        return false;
    }

    if let Some(ipv6) = host_port.strip_prefix('[') {
        let Some((host, suffix)) = ipv6.split_once(']') else {
            return false;
        };
        if host.parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        return suffix.is_empty() || parse_url_port(suffix.strip_prefix(':')).is_some();
    }

    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => (host, Some(port)),
        Some(_) => return false,
        None => (host_port, None),
    };

    if host.is_empty() || !(host.parse::<IpAddr>().is_ok() || is_valid_hostname(host)) {
        return false;
    }

    port.is_none_or(|port| parse_url_port(Some(port)).is_some())
}

fn parse_url_port(port: Option<&str>) -> Option<u16> {
    let port = port?;
    if port.is_empty() || !port.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    port.parse::<u16>().ok()
}

fn is_valid_url_userinfo(value: &str) -> bool {
    value.chars().all(|ch| {
        ch.is_ascii_alphanumeric()
            || matches!(
                ch,
                '-' | '.'
                    | '_'
                    | '~'
                    | '!'
                    | '$'
                    | '&'
                    | '\''
                    | '('
                    | ')'
                    | '*'
                    | '+'
                    | ','
                    | ';'
                    | '='
                    | ':'
                    | '%'
            )
    })
}

fn has_valid_percent_escapes(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let Some(first) = bytes.get(index + 1) else {
                return false;
            };
            let Some(second) = bytes.get(index + 2) else {
                return false;
            };
            if !first.is_ascii_hexdigit() || !second.is_ascii_hexdigit() {
                return false;
            }
            index += 3;
            continue;
        }
        index += 1;
    }
    true
}

fn is_valid_email(value: &str) -> bool {
    if value.is_empty() || value.contains(char::is_whitespace) || value.matches('@').count() != 1 {
        return false;
    }

    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };

    if local.is_empty()
        || domain.is_empty()
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
    {
        return false;
    }

    static LOCAL_PART_RE: OnceLock<Regex> = OnceLock::new();
    let local_part_re = LOCAL_PART_RE.get_or_init(|| {
        Regex::new(r"^[A-Za-z0-9!#$%&'*+/=?^_`{|}~-]+(?:\.[A-Za-z0-9!#$%&'*+/=?^_`{|}~-]+)*$")
            .expect("email local-part regex must compile")
    });
    if !local_part_re.is_match(local) {
        return false;
    }

    if let Some(ip_literal) = domain
        .strip_prefix('[')
        .and_then(|domain| domain.strip_suffix(']'))
    {
        return ip_literal.parse::<IpAddr>().is_ok();
    }

    domain.parse::<std::net::Ipv4Addr>().is_err() && is_valid_hostname(domain)
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
    let mut claimed_paths = BTreeMap::<String, String>::new();

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
        let arg_trace_name = format!("--set {raw_path}={raw_value}");
        let is_direct_array = parsed.value.is_array();
        claim_arg_path(
            &arg_trace_name,
            &path,
            is_direct_array,
            &direct_array_paths,
            &mut claimed_paths,
        )?;
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

fn claim_arg_path(
    arg: &str,
    path: &str,
    is_direct_array: bool,
    direct_array_paths: &BTreeSet<String>,
    claimed_paths: &mut BTreeMap<String, String>,
) -> Result<(), ConfigError> {
    for (existing_path, existing_arg) in claimed_paths.iter() {
        if existing_path == path {
            return Err(ConfigError::InvalidArg {
                arg: arg.to_owned(),
                message: format!(
                    "conflicting CLI overrides `{existing_arg}` and `{arg}` both target `{path}`"
                ),
            });
        }

        if existing_path
            .strip_prefix(path)
            .is_some_and(|suffix| suffix.starts_with('.'))
            || path
                .strip_prefix(existing_path)
                .is_some_and(|suffix| suffix.starts_with('.'))
        {
            if direct_array_overlap_allowed(
                existing_path,
                path,
                is_direct_array,
                direct_array_paths,
            ) {
                continue;
            }
            return Err(ConfigError::InvalidArg {
                arg: arg.to_owned(),
                message: format!(
                    "conflicting CLI overrides `{existing_arg}` and `{arg}` target overlapping configuration paths `{existing_path}` and `{path}`"
                ),
            });
        }
    }

    claimed_paths.insert(path.to_owned(), arg.to_owned());
    Ok(())
}

fn direct_array_overlap_allowed(
    existing_path: &str,
    new_path: &str,
    new_is_direct_array: bool,
    direct_array_paths: &BTreeSet<String>,
) -> bool {
    direct_array_prefix_allows(
        existing_path,
        new_path,
        direct_array_paths.contains(existing_path),
    ) || direct_array_prefix_allows(new_path, existing_path, new_is_direct_array)
}

fn direct_array_prefix_allows(prefix: &str, other: &str, is_direct_array: bool) -> bool {
    if !is_direct_array {
        return false;
    }
    let remainder = if prefix.is_empty() {
        other
    } else {
        let Some(remainder) = other.strip_prefix(prefix) else {
            return false;
        };
        let Some(remainder) = remainder.strip_prefix('.') else {
            return false;
        };
        remainder
    };
    remainder
        .split('.')
        .next()
        .is_some_and(|segment| segment.parse::<usize>().is_ok())
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
