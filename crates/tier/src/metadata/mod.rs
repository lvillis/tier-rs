use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::{self, Display, Formatter};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::ConfigError;
use crate::error::ValidationError;
use crate::loader::SourceKind;
use crate::report::{canonicalize_path_with_aliases, normalize_path, path_matches_pattern};

mod config;
mod field;
mod paths;
mod prefix;
mod validation;

use self::paths::*;
pub use self::prefix::prefixed_metadata;

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
    /// Whether the merge strategy was explicitly declared for this field.
    pub merge_explicit: bool,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EffectiveSourcePolicy {
    pub(crate) allowed_sources: Option<BTreeSet<SourceKind>>,
    pub(crate) denied_sources: Option<BTreeSet<SourceKind>>,
}

pub(crate) struct EffectiveValidation {
    pub(crate) field: FieldMetadata,
    pub(crate) rule: ValidationRule,
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
/// Scalar value used by declarative validation rules and conditions.
pub struct ValidationValue(pub serde_json::Value);

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

impl IntoIterator for ConfigMetadata {
    type Item = FieldMetadata;
    type IntoIter = std::vec::IntoIter<FieldMetadata>;

    fn into_iter(self) -> Self::IntoIter {
        self.fields.into_iter()
    }
}
