use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;

use serde_json::Value;
use thiserror::Error;

use crate::loader::{FileFormat, SourceTrace};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// A single validation failure returned by a validator hook.
pub struct ValidationError {
    /// Dot-delimited configuration path associated with the failure.
    pub path: String,
    /// Additional paths related to the failure, used for cross-field validations.
    pub related_paths: Vec<String>,
    /// Human-readable failure message.
    pub message: String,
    /// Optional rule identifier for machine-readable consumers.
    pub rule: Option<String>,
    /// Optional expected value associated with the failed rule.
    pub expected: Option<Value>,
    /// Optional actual value observed during validation.
    pub actual: Option<Value>,
}

impl ValidationError {
    /// Creates a new validation error.
    #[must_use]
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            related_paths: Vec::new(),
            message: message.into(),
            rule: None,
            expected: None,
            actual: None,
        }
    }

    /// Attaches a machine-readable rule identifier.
    #[must_use]
    pub fn with_rule(mut self, rule: impl Into<String>) -> Self {
        self.rule = Some(rule.into());
        self
    }

    /// Attaches related paths for cross-field validation failures.
    #[must_use]
    pub fn with_related_paths<I, S>(mut self, related_paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.related_paths = related_paths.into_iter().map(Into::into).collect();
        self
    }

    /// Attaches the expected value for the failed rule.
    #[must_use]
    pub fn with_expected(mut self, expected: Value) -> Self {
        self.expected = Some(expected);
        self
    }

    /// Attaches the actual value observed during validation.
    #[must_use]
    pub fn with_actual(mut self, actual: Value) -> Self {
        self.actual = Some(actual);
        self
    }
}

impl Display for ValidationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// A collection of validation failures returned by a validator hook.
pub struct ValidationErrors {
    errors: Vec<ValidationError>,
}

impl ValidationErrors {
    /// Creates an empty validation error collection.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a collection containing a single validation error.
    #[must_use]
    pub fn from_error(error: ValidationError) -> Self {
        Self {
            errors: vec![error],
        }
    }

    /// Creates a collection containing a single message-based validation error.
    #[must_use]
    pub fn from_message(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::from_error(ValidationError::new(path, message))
    }

    /// Appends a validation error.
    pub fn push(&mut self, error: ValidationError) {
        self.errors.push(error);
    }

    /// Appends multiple validation errors.
    pub fn extend<I>(&mut self, errors: I)
    where
        I: IntoIterator<Item = ValidationError>,
    {
        self.errors.extend(errors);
    }

    /// Returns `true` when the collection is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns the number of validation errors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Consumes the collection into a vector.
    pub fn into_vec(self) -> Vec<ValidationError> {
        self.errors
    }

    /// Returns an iterator over validation errors.
    pub fn iter(&self) -> impl Iterator<Item = &ValidationError> {
        self.errors.iter()
    }
}

impl IntoIterator for ValidationErrors {
    type Item = ValidationError;
    type IntoIter = std::vec::IntoIter<ValidationError>;

    fn into_iter(self) -> Self::IntoIter {
        self.errors.into_iter()
    }
}

impl Display for ValidationErrors {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for (index, error) in self.errors.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "- {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationErrors {}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Information about an unknown configuration path discovered during loading.
pub struct UnknownField {
    /// Dot-delimited path that was not recognized.
    pub path: String,
    /// Most recent source that contributed the unknown path, when known.
    pub source: Option<SourceTrace>,
    /// Best-effort suggestion for the intended path.
    pub suggestion: Option<String>,
}

impl UnknownField {
    /// Creates an unknown field description for a path.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            source: None,
            suggestion: None,
        }
    }

    /// Attaches source information.
    #[must_use]
    pub fn with_source(mut self, source: Option<SourceTrace>) -> Self {
        self.source = source;
        self
    }

    /// Attaches a best-effort suggestion.
    #[must_use]
    pub fn with_suggestion(mut self, suggestion: Option<String>) -> Self {
        self.suggestion = suggestion;
        self
    }
}

impl Display for UnknownField {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "unknown field `{}`", self.path)?;
        if let Some(source) = &self.source {
            write!(f, " from {source}")?;
        }
        if let Some(suggestion) = &self.suggestion {
            write!(f, "; did you mean `{suggestion}`?")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Line and column information for parse diagnostics.
pub struct LineColumn {
    /// One-based line number.
    pub line: usize,
    /// One-based column number.
    pub column: usize,
}

impl Display for LineColumn {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, column {}", self.line, self.column)
    }
}

#[derive(Debug, Error)]
/// Errors returned while building, loading, validating, or inspecting configuration.
pub enum ConfigError {
    /// The root serialized value was not a JSON-like object.
    #[error("configuration root must serialize to a map-like object, got {actual}")]
    RootMustBeObject {
        /// Human-readable kind of the unexpected root value.
        actual: &'static str,
    },

    /// Serializing configuration state into an intermediate value failed.
    #[error("failed to serialize configuration state: {source}")]
    Serialize {
        /// Serialization error from the intermediate serde representation.
        #[from]
        source: serde_json::Error,
    },

    /// Starting or running a filesystem watcher failed.
    #[error("failed to watch configuration files: {message}")]
    Watch {
        /// Human-readable watcher failure details.
        message: String,
    },

    /// A required configuration file was not found.
    #[error("required configuration file not found: {}", path.display())]
    MissingFile {
        /// Missing file path.
        path: PathBuf,
    },

    /// None of the required candidate files were found.
    #[error("none of the required configuration files were found:\n{paths}", paths = format_missing_paths(paths))]
    MissingFiles {
        /// Candidate paths that were checked.
        paths: Vec<PathBuf>,
    },

    /// Reading a configuration file failed.
    #[error("failed to read configuration file {}: {source}", path.display())]
    ReadFile {
        /// File path being read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Parsing a configuration file failed.
    #[error("failed to parse {format} configuration file {}{location}: {message}", path.display(), location = format_location(*location))]
    ParseFile {
        /// File path being parsed.
        path: PathBuf,
        /// Detected or configured file format.
        format: FileFormat,
        /// Optional line and column information for the parse error.
        location: Option<LineColumn>,
        /// Human-readable parse failure.
        message: String,
    },

    /// An environment variable could not be converted into configuration data.
    #[error("invalid environment variable {name} for path {path}: {message}")]
    InvalidEnv {
        /// Environment variable name.
        name: String,
        /// Derived configuration path.
        path: String,
        /// Human-readable conversion failure.
        message: String,
    },

    /// A CLI override argument was invalid.
    #[error("invalid CLI argument {arg}: {message}")]
    InvalidArg {
        /// Raw CLI fragment.
        arg: String,
        /// Human-readable validation failure.
        message: String,
    },

    /// A typed sparse patch could not be converted into a configuration layer.
    #[error("invalid patch {name} for path {path}: {message}")]
    InvalidPatch {
        /// Human-readable patch source name.
        name: String,
        /// Target configuration path.
        path: String,
        /// Human-readable validation failure.
        message: String,
    },

    /// Multiple input paths resolved to the same canonical path.
    #[error(
        "configuration paths `{first_path}` and `{second_path}` both resolve to `{canonical_path}`"
    )]
    PathConflict {
        /// First input path that mapped to the canonical path.
        first_path: String,
        /// Second conflicting input path.
        second_path: String,
        /// Canonical path both inputs resolved to.
        canonical_path: String,
    },

    /// A source attempted to write to a field that restricts allowed source kinds.
    #[error(
        "source {trace} is not allowed to set `{path}`; allowed sources: {allowed}",
        allowed = format_source_kind_list(allowed_sources)
    )]
    SourcePolicyViolation {
        /// Concrete path rejected by the source policy.
        path: String,
        /// Actual source attempting to set the path.
        trace: SourceTrace,
        /// Allowed source kinds for the path.
        allowed_sources: Vec<crate::loader::SourceKind>,
    },

    /// A serialized object key could not be represented in tier's dot-delimited path model.
    #[error(
        "configuration object key `{key}` under {location} cannot be represented in tier paths: {message}",
        location = format_path_location(path)
    )]
    InvalidPathKey {
        /// Parent path containing the unsupported key.
        path: String,
        /// Unsupported object key segment.
        key: String,
        /// Human-readable validation failure.
        message: String,
    },

    /// Metadata declared the same alias or environment variable more than once.
    #[error("metadata {kind} `{name}` is assigned to both `{first_path}` and `{second_path}`")]
    MetadataConflict {
        /// Human-readable conflict category such as `alias` or `environment variable`.
        kind: &'static str,
        /// Conflicting alias or environment variable name.
        name: String,
        /// First path using the name.
        first_path: String,
        /// Second path using the name.
        second_path: String,
    },

    /// Metadata declared an unsupported or invalid field configuration.
    #[error("invalid metadata for `{path}`: {message}")]
    MetadataInvalid {
        /// Metadata path that triggered the validation failure.
        path: String,
        /// Human-readable validation failure.
        message: String,
    },

    /// A CLI flag requiring a value was missing one.
    #[error("missing value for CLI flag {flag}")]
    MissingArgValue {
        /// Flag name missing a required value.
        flag: String,
    },

    /// A file path template referenced `{profile}` without a profile being set.
    #[error("path template {} contains {{profile}} but no profile was set", path.display())]
    MissingProfile {
        /// Path template containing `{profile}`.
        path: PathBuf,
    },

    /// Deserializing the merged intermediate value into the target type failed.
    #[error(
        "failed to deserialize merged configuration at {path}: {message}{source_suffix}",
        source_suffix = deserialize_source_suffix(provenance)
    )]
    Deserialize {
        /// Configuration path reported by serde.
        path: String,
        /// Most recent source that contributed the failing value, when known.
        provenance: Option<SourceTrace>,
        /// Human-readable deserialization failure.
        message: String,
    },

    /// The requested explain path did not exist in the final report.
    #[error(
        "cannot explain configuration path `{path}` because it does not exist in the final report"
    )]
    ExplainPathNotFound {
        /// Requested explain path.
        path: String,
    },

    /// Unknown configuration paths were found and the active policy rejected them.
    #[error("unknown configuration fields:\n{fields}", fields = format_unknown_fields(fields))]
    UnknownFields {
        /// Unknown paths discovered during loading.
        fields: Vec<UnknownField>,
    },

    /// A normalizer hook failed.
    #[error("normalizer {name} failed: {message}")]
    Normalize {
        /// Normalizer name.
        name: String,
        /// Human-readable failure.
        message: String,
    },

    /// A validator hook failed.
    #[error("validator {name} failed:\n{errors}")]
    Validation {
        /// Validator name.
        name: String,
        /// Validation failures returned by the hook.
        errors: ValidationErrors,
    },

    /// Built-in field validation rules failed.
    #[error("declared validation failed:\n{errors}")]
    DeclaredValidation {
        /// Validation failures returned by metadata-driven rules.
        errors: ValidationErrors,
    },
}

fn format_location(location: Option<LineColumn>) -> String {
    match location {
        Some(location) => format!(" ({location})"),
        None => String::new(),
    }
}

fn format_missing_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| format!("- {}", path.display()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_unknown_fields(fields: &[UnknownField]) -> String {
    fields
        .iter()
        .map(|field| format!("- {field}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_path_location(path: &str) -> String {
    if path.is_empty() {
        "the configuration root".to_owned()
    } else {
        format!("`{path}`")
    }
}

fn format_source_kind_list(kinds: &[crate::loader::SourceKind]) -> String {
    kinds
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn deserialize_source_suffix(provenance: &Option<SourceTrace>) -> String {
    provenance
        .as_ref()
        .map_or_else(String::new, |source| format!(" from {source}"))
}
