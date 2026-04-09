#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

#[cfg(feature = "clap")]
mod cli;
#[cfg(feature = "schema")]
mod docs;
mod error;
mod loader;
/// Internal metadata helpers used by the derive macro.
#[doc(hidden)]
pub mod metadata;
/// Internal patch helpers used by the derive macro.
#[doc(hidden)]
pub mod patch;
/// Internal path helpers used by exported path macros.
#[doc(hidden)]
pub mod path;
mod reload;
mod report;
#[cfg(feature = "schema")]
mod schema;
mod secret;

#[cfg(feature = "clap")]
#[cfg_attr(docsrs, doc(cfg(feature = "clap")))]
pub use crate::cli::{TierCli, TierCliCommand};
#[cfg(feature = "schema")]
#[cfg_attr(docsrs, doc(cfg(feature = "schema")))]
pub use crate::docs::{
    ENV_DOCS_FORMAT_VERSION, EnvDocEntry, EnvDocOptions, EnvDocsReport, env_docs_for,
    env_docs_json, env_docs_json_pretty, env_docs_markdown, env_docs_report, env_docs_report_json,
    env_docs_report_json_pretty,
};
pub use crate::error::{ConfigError, UnknownField, ValidationError, ValidationErrors};
pub use crate::loader::{
    ArgsSource, ConfigLoader, EnvSource, FileFormat, FileSource, Layer, LoadedConfig, SourceKind,
    SourceTrace, UnknownFieldPolicy,
};
pub use crate::metadata::{
    ConfigMetadata, EnvDecoder, FieldMetadata, MergeStrategy, TierMetadata, ValidationCheck,
    ValidationNumber, ValidationRule, ValidationValue,
};
pub use crate::patch::{Patch, TierPatch};
#[cfg(feature = "watch")]
#[cfg_attr(docsrs, doc(cfg(feature = "watch")))]
pub use crate::reload::NativeWatcher;
pub use crate::reload::{
    ConfigChange, PollingWatcher, ReloadEvent, ReloadFailure, ReloadFailurePolicy, ReloadHandle,
    ReloadOptions, ReloadSummary,
};
pub use crate::report::{
    AuditReport, ConfigReport, ConfigWarning, DeprecatedField, DoctorReport, Explanation,
    REPORT_FORMAT_VERSION, ReportSummary, ResolutionStep, TraceAudit,
};
#[cfg(all(feature = "schema", feature = "toml"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "schema", feature = "toml"))))]
pub use crate::schema::config_example_toml;
#[cfg(feature = "schema")]
#[cfg_attr(docsrs, doc(cfg(feature = "schema")))]
pub use crate::schema::{
    ConfigExampleReport, JsonSchema, JsonSchemaReport, SCHEMA_EXPORT_FORMAT_VERSION,
    annotated_json_schema_for, annotated_json_schema_pretty, annotated_json_schema_report,
    annotated_json_schema_report_json, annotated_json_schema_report_json_pretty,
    config_example_for, config_example_pretty, config_example_report, config_example_report_json,
    config_example_report_json_pretty, json_schema_for, json_schema_pretty, json_schema_report,
    json_schema_report_json, json_schema_report_json_pretty,
};
pub use crate::secret::Secret;
#[cfg(feature = "derive")]
#[cfg_attr(docsrs, doc(cfg(feature = "derive")))]
pub use tier_derive::{TierConfig, TierPatch};
